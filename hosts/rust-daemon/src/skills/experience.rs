//! Per-version native load observations and owner-attested outcomes.

use anima_core::skill_learning::{
    record_experience, settle_experience, SkillExperience, SkillUseStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{ProposedBy, SkillError, SkillRecord, SkillRegistry, SkillService};

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SkillFeedback {
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) run_id: String,
    pub(crate) version: String,
    pub(crate) verified_evidence: Option<String>,
    #[serde(default)]
    pub(crate) corrected: bool,
}

impl SkillService {
    async fn observe(
        &self,
        change: impl FnOnce(&mut SkillRegistry, u64) -> Result<(), SkillError> + Send + 'static,
    ) -> Result<(), SkillError> {
        self.locked(move |service, _root| async move {
            let mut guard = service.state.write().await;
            let previous = guard.skills.clone();
            change(&mut guard.skills, anima_core::primitives::now_millis())?;
            let persist = guard.control_plane_persist_request();
            drop(guard);
            if let Err(error) = persist.save().await {
                service.state.write().await.skills = previous;
                return Err(SkillError::Unavailable(error.to_string()));
            }
            Ok(())
        })
        .await
    }

    /// Called only after a native load verified the exact package bytes.
    pub(crate) async fn record_use(
        &self,
        slug: &str,
        version: &str,
        by: ProposedBy,
    ) -> Result<(), SkillError> {
        let (slug, version) = (slug.to_owned(), version.to_owned());
        self.observe(move |skills, now| {
            let mut record = skills.get(&slug).cloned().ok_or(SkillError::NotFound)?;
            record_experience(
                &mut record.experiences,
                SkillExperience {
                    agent_id: by.agent_id,
                    session_id: by.session_id,
                    run_id: by.run_id,
                    version,
                    loaded_at_ms: now.max(1),
                    finished_at_ms: None,
                    status: SkillUseStatus::Loaded,
                    verified_evidence: None,
                    corrected: false,
                },
            );
            skills.put(record).map_err(SkillError::conflict)?;
            Ok(())
        })
        .await
    }

    pub(crate) async fn settle_uses(
        &self,
        agent: &str,
        run: &str,
        status: SkillUseStatus,
    ) -> Result<(), SkillError> {
        let (agent, run) = (agent.to_owned(), run.to_owned());
        // Avoid a persistence operation for the common run with no skill load.
        if !self.state.read().await.skills.records().iter().any(|r| {
            r.experiences.iter().any(|e| {
                e.agent_id == agent && e.run_id == run && e.status == SkillUseStatus::Loaded
            })
        }) {
            return Ok(());
        }
        self.observe(move |skills, now| {
            let records: Vec<_> = skills.records().into_iter().cloned().collect();
            for mut record in records {
                settle_experience(&mut record.experiences, &agent, &run, status, now);
                skills.put(record).map_err(SkillError::conflict)?;
            }
            Ok(())
        })
        .await
    }

    /// Owner route only: models cannot certify their own task outcomes.
    pub(crate) async fn skill_feedback(
        &self,
        slug: &str,
        feedback: SkillFeedback,
    ) -> Result<SkillRecord, SkillError> {
        if !feedback.corrected && feedback.verified_evidence.is_none() {
            return Err(SkillError::invalid(
                "Provide correction or independent verification evidence",
            ));
        }
        if let Some(text) = &feedback.verified_evidence {
            if text.trim().is_empty() || text.len() > 1024 || super::validate_body(text).is_err() {
                return Err(SkillError::invalid(
                    "Verification evidence must be visible text of at most 1024 bytes",
                ));
            }
        }
        let target = slug.to_owned();
        self.observe(move |skills, _| {
            let mut record = skills.get(&target).cloned().ok_or(SkillError::NotFound)?;
            let entry = record
                .experiences
                .iter_mut()
                .find(|entry| {
                    entry.agent_id == feedback.agent_id
                        && entry.session_id == feedback.session_id
                        && entry.run_id == feedback.run_id
                        && entry.version == feedback.version
                })
                .ok_or_else(|| {
                    SkillError::conflict("No matching native skill load was observed")
                })?;
            if entry.status == SkillUseStatus::Loaded
                || (!feedback.corrected && entry.status != SkillUseStatus::Completed)
            {
                return Err(SkillError::conflict(
                    "Feedback needs a terminal use; independent verification requires completion",
                ));
            }
            entry.corrected = feedback.corrected;
            entry.verified_evidence = if feedback.corrected {
                None
            } else {
                feedback.verified_evidence
            };
            skills.put(record).map_err(SkillError::conflict)?;
            Ok(())
        })
        .await?;
        self.record(slug).await
    }

    pub(crate) async fn learning_report(
        &self,
        slug: &str,
    ) -> Result<serde_json::Value, SkillError> {
        let guard = self.state.read().await;
        let record = guard.skills.get(slug).ok_or(SkillError::NotFound)?;
        let normalize = |text: &str| {
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let own_body = guard
            .skills
            .scanned(slug)
            .and_then(|f| f.parsed.as_ref().ok())
            .map(|f| normalize(&f.body));
        let duplicates: Vec<_> = guard
            .skills
            .records()
            .into_iter()
            .filter(|other| {
                other.slug != slug
                    && other.status == super::SkillStatus::Active
                    && own_body.is_some()
                    && guard
                        .skills
                        .scanned(&other.slug)
                        .and_then(|f| f.parsed.as_ref().ok())
                        .is_some_and(|f| Some(normalize(&f.body)) == own_body)
            })
            .map(|r| r.slug.clone())
            .collect();
        let last = record
            .experiences
            .iter()
            .map(|e| e.loaded_at_ms)
            .max()
            .unwrap_or(record.approved_at_ms)
            .max(record.approved_at_ms);
        let now = anima_core::primitives::now_millis();
        let mut versions = std::collections::BTreeMap::<String, Vec<&SkillExperience>>::new();
        for entry in &record.experiences {
            versions
                .entry(entry.version.clone())
                .or_default()
                .push(entry);
        }
        let versions: Vec<_> = versions.into_iter().map(|(hash, entries)| json!({"hash":hash,"loaded":entries.len(),"completed":entries.iter().filter(|e| e.status==SkillUseStatus::Completed).count(),"failed":entries.iter().filter(|e| e.status==SkillUseStatus::Failed).count(),"corrected":entries.iter().filter(|e| e.corrected).count(),"verified":entries.iter().filter(|e| e.verified_evidence.is_some()).count()})).collect();
        Ok(
            json!({"slug":slug,"approvedHash":record.approved_hash,"experiences":record.experiences,"versions":versions,"maintenance":{"stale":now.saturating_sub(last)>30*86_400_000u64,"duplicateSlugs":duplicates,"automaticChanges":false},"meaning":"Loads and completed replies are observations, not proof of task success. Verified outcomes are owner-attested against independent evidence. History retains at most 64 uses per skill."}),
        )
    }
}

/// Applied inside the same transaction/snapshot as the terminal run outcome.
pub(crate) fn settle_registry_run(skills: &mut SkillRegistry, run: &crate::runs::RunRecord) {
    let status = match run.status {
        crate::runs::RunStatus::Completed => SkillUseStatus::Completed,
        crate::runs::RunStatus::Failed => SkillUseStatus::Failed,
        _ => SkillUseStatus::Cancelled,
    };
    let records: Vec<_> = skills
        .records()
        .into_iter()
        .filter(|r| {
            r.experiences.iter().any(|e| {
                e.agent_id == run.agent_id
                    && e.run_id == run.id
                    && e.status == SkillUseStatus::Loaded
            })
        })
        .cloned()
        .collect();
    for mut record in records {
        settle_experience(
            &mut record.experiences,
            &run.agent_id,
            &run.id,
            status,
            run.finished_at_ms
                .unwrap_or_else(anima_core::primitives::now_millis),
        );
        let _ = skills.put(record);
    }
}

/// A process interruption cannot leave an old loaded observation in flight forever.
pub(crate) fn reconcile_restored(skills: &mut SkillRegistry, runs: &crate::runs::RunLedger) {
    let records: Vec<_> = skills.records().into_iter().cloned().collect();
    for mut record in records {
        for entry in &mut record.experiences {
            if entry.status != SkillUseStatus::Loaded {
                continue;
            }
            let run = runs
                .get(&entry.run_id)
                .filter(|r| r.agent_id == entry.agent_id && r.session_id == entry.session_id);
            entry.status = match run.map(|r| r.status) {
                Some(crate::runs::RunStatus::Completed) => SkillUseStatus::Completed,
                Some(crate::runs::RunStatus::Failed) => SkillUseStatus::Failed,
                _ => SkillUseStatus::Cancelled,
            };
            entry.finished_at_ms = Some(
                run.and_then(|r| r.finished_at_ms)
                    .unwrap_or_else(anima_core::primitives::now_millis)
                    .max(entry.loaded_at_ms),
            );
        }
        let _ = skills.put(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::{RunLedger, RunRecord, RunSource, RunStart, RunStatus};
    use crate::skills::test_support::{content, service, temp_workspace, with_workspace};
    use crate::state::DaemonState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn observation(run_id: &str) -> SkillExperience {
        SkillExperience {
            agent_id: "agent".into(),
            session_id: "session".into(),
            run_id: run_id.into(),
            version: "a".repeat(64),
            loaded_at_ms: 100,
            finished_at_ms: None,
            status: SkillUseStatus::Loaded,
            verified_evidence: None,
            corrected: false,
        }
    }

    fn restored_skills(experiences: Vec<SkillExperience>) -> SkillRegistry {
        let file = crate::skills::SkillFile {
            name: "Notes".into(),
            description: "Take notes".into(),
            body: "Check the output.".into(),
        };
        let mut record = SkillRecord::approved("notes", &file, "a".repeat(64), 1);
        record.experiences = experiences;
        let mut skills = SkillRegistry::default();
        skills.put(record).unwrap();
        let mut state = DaemonState::new();
        state.skills = skills;
        let snapshot: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&state.control_plane_snapshot()).unwrap())
                .unwrap();
        SkillRegistry::restored(
            crate::skills::SkillSnapshot {
                skills: snapshot.skills,
                drafts: snapshot.skill_drafts,
            },
            1000,
        )
    }

    fn run_for_observation(id: &str, status: RunStatus) -> RunRecord {
        let mut run = RunRecord::running(
            RunStart {
                agent_id: "agent".into(),
                session_id: "session".into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "Use the reviewed workflow.".into(),
                model: "test-model".into(),
                provider: None,
                parent_run_id: None,
            },
            10,
        );
        run.id = id.into();
        if status.is_terminal() {
            run.finish(status, None, 200);
        } else {
            run.status = status;
        }
        run
    }

    #[test]
    fn restored_loaded_observations_settle_from_terminal_and_interrupted_runs() {
        let cases = [
            (
                "completed",
                Some(RunStatus::Completed),
                SkillUseStatus::Completed,
                Some(200),
            ),
            (
                "failed",
                Some(RunStatus::Failed),
                SkillUseStatus::Failed,
                Some(200),
            ),
            (
                "cancelled",
                Some(RunStatus::Cancelled),
                SkillUseStatus::Cancelled,
                Some(200),
            ),
            (
                "interrupted",
                Some(RunStatus::Interrupted),
                SkillUseStatus::Cancelled,
                Some(200),
            ),
            (
                "running",
                Some(RunStatus::Running),
                SkillUseStatus::Cancelled,
                Some(1000),
            ),
            (
                "queued",
                Some(RunStatus::Queued),
                SkillUseStatus::Cancelled,
                Some(1000),
            ),
            (
                "awaiting-approval",
                Some(RunStatus::AwaitingApproval),
                SkillUseStatus::Cancelled,
                Some(1000),
            ),
            ("missing", None, SkillUseStatus::Cancelled, None),
        ];
        let records = cases
            .iter()
            .filter_map(|(id, status, _, _)| status.map(|status| run_for_observation(id, status)))
            .collect();
        let runs = RunLedger::restored(
            records,
            &std::collections::HashSet::from(["agent".into()]),
            1000,
        );
        let mut skills =
            restored_skills(cases.iter().map(|(id, _, _, _)| observation(id)).collect());
        let before = anima_core::primitives::now_millis();
        reconcile_restored(&mut skills, &runs);
        let after = anima_core::primitives::now_millis();
        let experiences = &skills.get("notes").unwrap().experiences;
        for (id, _, expected_status, expected_finished) in cases {
            let actual = experiences.iter().find(|entry| entry.run_id == id).unwrap();
            assert_eq!(actual.status, expected_status, "{id}");
            if let Some(finished) = expected_finished {
                assert_eq!(actual.finished_at_ms, Some(finished), "{id}");
            } else {
                assert!(
                    (before..=after).contains(&actual.finished_at_ms.unwrap()),
                    "{id}"
                );
            }
            assert!(
                actual.verified_evidence.is_none(),
                "{id} must not become verified on restore"
            );
            assert!(!actual.corrected, "{id} must not invent owner feedback");
        }
        let settled = experiences.clone();
        reconcile_restored(&mut skills, &RunLedger::default());
        assert_eq!(skills.get("notes").unwrap().experiences, settled);
    }

    #[test]
    fn restored_observations_require_matching_agent_and_session_before_inheriting_a_run_outcome() {
        let mut wrong_agent = run_for_observation("wrong-agent", RunStatus::Completed);
        wrong_agent.agent_id = "foreign-agent".into();
        let mut wrong_session = run_for_observation("wrong-session", RunStatus::Completed);
        wrong_session.session_id = "foreign-session".into();
        let runs = RunLedger::restored(
            vec![wrong_agent, wrong_session],
            &std::collections::HashSet::from(["agent".into(), "foreign-agent".into()]),
            1000,
        );
        assert!(runs.get("wrong-agent").is_some());
        assert!(runs.get("wrong-session").is_some());
        let mut skills = restored_skills(vec![
            observation("wrong-agent"),
            observation("wrong-session"),
        ]);
        reconcile_restored(&mut skills, &runs);
        for entry in &skills.get("notes").unwrap().experiences {
            assert_eq!(entry.status, SkillUseStatus::Cancelled);
            assert!(entry.finished_at_ms.unwrap() >= entry.loaded_at_ms);
            assert!(entry.verified_evidence.is_none());
        }
    }

    #[test]
    fn restored_terminal_observations_keep_existing_verification_and_correction() {
        let mut verified = observation("verified");
        verified.status = SkillUseStatus::Completed;
        verified.finished_at_ms = Some(150);
        verified.verified_evidence = Some("Independent output bytes matched the fixture.".into());
        let mut corrected = observation("corrected");
        corrected.status = SkillUseStatus::Failed;
        corrected.finished_at_ms = Some(175);
        corrected.corrected = true;
        let expected = vec![verified, corrected];
        let mut skills = restored_skills(expected.clone());
        let runs = RunLedger::restored(
            vec![run_for_observation("verified", RunStatus::Failed)],
            &std::collections::HashSet::from(["agent".into()]),
            1000,
        );
        reconcile_restored(&mut skills, &runs);
        assert_eq!(skills.get("notes").unwrap().experiences, expected);
    }

    #[tokio::test]
    async fn owner_corrections_are_allowed_for_failed_and_cancelled_uses_but_verification_is_refused(
    ) {
        let root = temp_workspace("feedback-unsuccessful");
        let state = Arc::new(RwLock::new(with_workspace(DaemonState::new(), &root)));
        let service = service(&state);
        let (record, _) = service.save("notes", content("Notes")).await.unwrap();
        for (run, status) in [
            ("failed", SkillUseStatus::Failed),
            ("cancelled", SkillUseStatus::Cancelled),
        ] {
            service
                .record_use(
                    "notes",
                    &record.approved_hash,
                    ProposedBy {
                        agent_id: "agent".into(),
                        session_id: "session".into(),
                        run_id: run.into(),
                    },
                )
                .await
                .unwrap();
            service.settle_uses("agent", run, status).await.unwrap();
            let mut feedback = SkillFeedback {
                agent_id: "agent".into(),
                session_id: "session".into(),
                run_id: run.into(),
                version: record.approved_hash.clone(),
                corrected: false,
                verified_evidence: Some("Independent output bytes matched the fixture.".into()),
            };
            let before = state
                .read()
                .await
                .skills
                .get("notes")
                .unwrap()
                .experiences
                .clone();
            let refused = service.skill_feedback("notes", feedback.clone()).await;
            assert!(
                matches!(refused, Err(SkillError::Conflict(_))),
                "{run}: {refused:?}"
            );
            assert_eq!(
                state.read().await.skills.get("notes").unwrap().experiences,
                before
            );
            feedback.corrected = true;
            feedback.verified_evidence = None;
            let corrected = service
                .skill_feedback("notes", feedback.clone())
                .await
                .unwrap();
            let entry = corrected
                .experiences
                .iter()
                .find(|entry| entry.run_id == run)
                .unwrap();
            assert_eq!(entry.status, status);
            assert!(entry.finished_at_ms.is_some());
            assert!(entry.corrected);
            assert!(entry.verified_evidence.is_none());
            feedback.corrected = false;
            feedback.verified_evidence = Some("Attempted verification after correction.".into());
            assert!(matches!(
                service.skill_feedback("notes", feedback).await,
                Err(SkillError::Conflict(_))
            ));
            assert_eq!(
                state.read().await.skills.get("notes").unwrap().experiences,
                corrected.experiences
            );
        }
    }

    #[tokio::test]
    async fn skill_experience_survives_version_updates_and_completion_is_not_verification() {
        let root = temp_workspace("experience");
        let state = Arc::new(RwLock::new(with_workspace(DaemonState::new(), &root)));
        let service = service(&state);
        let (original, _) = service.save("notes", content("Notes")).await.unwrap();
        let by = crate::skills::ProposedBy {
            agent_id: "agent".into(),
            session_id: "session".into(),
            run_id: "run".into(),
        };
        service
            .record_use("notes", &original.approved_hash, by.clone())
            .await
            .unwrap();
        service
            .record_use("notes", &original.approved_hash, by)
            .await
            .unwrap();
        service
            .settle_uses(
                "agent",
                "run",
                anima_core::skill_learning::SkillUseStatus::Completed,
            )
            .await
            .unwrap();
        let mut next = content("Notes");
        next.body = "Read the result before reporting it.".into();
        let (updated, _) = service.save("notes", next).await.unwrap();
        assert_eq!(updated.experiences.len(), 1);
        assert_eq!(updated.experiences[0].version, original.approved_hash);
        assert!(updated.experiences[0].verified_evidence.is_none());
        let snapshot = state.read().await.skills.snapshot();
        let restored = crate::skills::SkillRegistry::restored(snapshot, 1);
        assert_eq!(
            restored.get("notes").unwrap().experiences,
            updated.experiences
        );
    }

    #[tokio::test]
    async fn skill_feedback_requires_the_exact_observed_agent_session_run_and_version() {
        let root = temp_workspace("feedback");
        let state = Arc::new(RwLock::new(with_workspace(DaemonState::new(), &root)));
        let service = service(&state);
        let (record, _) = service.save("notes", content("Notes")).await.unwrap();
        service
            .record_use(
                "notes",
                &record.approved_hash,
                crate::skills::ProposedBy {
                    agent_id: "agent".into(),
                    session_id: "session".into(),
                    run_id: "run".into(),
                },
            )
            .await
            .unwrap();
        let mut feedback = SkillFeedback {
            agent_id: "agent".into(),
            session_id: "foreign".into(),
            run_id: "run".into(),
            version: record.approved_hash.clone(),
            verified_evidence: Some(
                "Independent output bytes matched the expected fixture.".into(),
            ),
            corrected: false,
        };
        assert!(service
            .skill_feedback("notes", feedback.clone())
            .await
            .is_err());
        feedback.session_id = "session".into();
        assert!(
            service
                .skill_feedback("notes", feedback.clone())
                .await
                .is_err(),
            "a load alone cannot be verified"
        );
        service
            .settle_uses(
                "agent",
                "run",
                anima_core::skill_learning::SkillUseStatus::Completed,
            )
            .await
            .unwrap();
        let record = service.skill_feedback("notes", feedback).await.unwrap();
        assert!(record.experiences[0].verified_evidence.is_some());
        assert_eq!(record.approved_hash, record.experiences[0].version);
    }
}
