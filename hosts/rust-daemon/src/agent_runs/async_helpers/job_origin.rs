//! Exact existing job attempts settle from published source events. No timer,
//! replacement attempt, scheduler occurrence or additional model worker.
use super::super::AgentRunCoordinator;
use crate::jobs::{helper_completion, AgentJobStatus};
use crate::runs::{
    DelegationFinalOutput, DelegationOutput, HelperCompletionState, PendingDelegations, RunStatus,
};
use serde_json::json;

enum Readiness {
    Waiting,
    Review(&'static str),
    Ready(Vec<crate::runs::RunRecord>),
}

fn readiness(
    state: &crate::state::DaemonState,
    job: &crate::jobs::AgentJobRecord,
    refs: &PendingDelegations,
) -> Readiness {
    let Some(owner) = state.agents.get(&job.agent_id) else {
        return Readiness::Review("Origin job owner is missing");
    };
    if !owner.config().allows_tool("spawn_helper_async") || job.stop_requested_at_ms.is_some() {
        return Readiness::Review("Origin delegation was stopped or its current grant was revoked");
    }
    let Some(parent) = state.runs.get(&refs.launch_run_id) else {
        return Readiness::Review("The exact source job run is missing");
    };
    if parent.stop.is_some()
        || parent.status != RunStatus::Completed
        || helper_completion::has_uncertain_effects(state, &parent.id)
    {
        return Readiness::Review(
            "Origin source did not prove a successful terminal effect outcome",
        );
    }
    let mut waiting = !parent.terminal_published;
    let mut callbacks = Vec::new();
    for (child_id, callback_id) in refs.child_run_ids.iter().zip(&refs.completion_run_ids) {
        let Some(child) = state.runs.get(child_id) else {
            return Readiness::Review("Required helper source evidence is missing");
        };
        let Some(helper) = &child.async_helper else {
            return Readiness::Review("Required child has no typed assignment");
        };
        if matches!(
            helper.completion,
            HelperCompletionState::ReviewRequired { .. }
                | HelperCompletionState::Suppressed { .. }
                | HelperCompletionState::Manual
        ) || child.stop.is_some()
            || (child.status.is_terminal() && child.status != RunStatus::Completed)
        {
            return Readiness::Review("A required helper was interrupted, failed or suppressed; no automatic retry was scheduled");
        }
        if !child.status.is_terminal() || !child.terminal_published {
            waiting = true;
            continue;
        }
        if helper_completion::has_uncertain_effects(state, child_id) {
            return Readiness::Review("A helper has an unresolved native effect receipt");
        }
        if !state
            .sessions
            .get(&helper.owner_agent_id, &helper.owner_session_id)
            .is_some_and(|session| !session.archived)
            || !helper_completion::origin_is_current(state, child)
        {
            return Readiness::Review("The exact source job/session is no longer current");
        }
        let Some(callback) = state.runs.get(callback_id) else {
            waiting = true;
            continue;
        };
        if callback.stop.is_some()
            || (callback.status.is_terminal() && callback.status != RunStatus::Completed)
        {
            return Readiness::Review(
                "A required callback failed or was stopped; no automatic retry was scheduled",
            );
        }
        if !callback.status.is_terminal() || !callback.terminal_published {
            waiting = true;
            continue;
        }
        if helper_completion::has_uncertain_effects(state, callback_id) {
            return Readiness::Review("A callback has an unresolved native effect receipt");
        }
        callbacks.push(callback.clone());
    }
    if waiting {
        Readiness::Waiting
    } else {
        Readiness::Ready(callbacks)
    }
}

impl AgentRunCoordinator {
    pub(in crate::agent_runs) async fn settle_job_origins(&self, event: &str) {
        let candidates: Vec<_> = self
            .state
            .read()
            .await
            .jobs
            .values()
            .filter(|job| job.status == AgentJobStatus::PendingDelegations)
            .filter(|job| {
                job.pending_delegations.as_ref().is_some_and(|refs| {
                    refs.launch_run_id == event
                        || refs
                            .child_run_ids
                            .iter()
                            .chain(&refs.completion_run_ids)
                            .any(|id| id == event)
                })
            })
            .map(|job| job.id.clone())
            .collect();
        for id in candidates {
            self.settle_job_origin(&id).await;
        }
    }

    async fn settle_job_origin(&self, id: &str) {
        let transaction = self.control_plane_transaction().await;
        let (before, refs, status) = {
            let state = self.state.read().await;
            let Some(job) = state
                .jobs
                .get(id)
                .filter(|job| job.status == AgentJobStatus::PendingDelegations)
            else {
                return;
            };
            let Some(refs) = job.pending_delegations.clone() else {
                return;
            };
            (job.clone(), refs.clone(), readiness(&state, job, &refs))
        };
        let (mut output, mut reason) = match status {
            Readiness::Waiting => return,
            Readiness::Review(reason) => (None, reason),
            Readiness::Ready(callbacks) => {
                let mut entries = Vec::new();
                let mut missing = false;
                for callback in callbacks {
                    let text = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        self.committed_helper_text(&callback),
                    )
                    .await
                    .ok()
                    .flatten();
                    let Some(text) = text else {
                        missing = true;
                        break;
                    };
                    let digest = crate::runs::committed_output_sha256(&text);
                    if callback.reply_sha256.as_deref() != Some(digest.as_str()) {
                        missing = true;
                        break;
                    }
                    let Some(reply) = callback.reply_message_id else {
                        missing = true;
                        break;
                    };
                    let mut entry = DelegationOutput {
                        run_id: callback.id,
                        reply_message_id: reply,
                        output_sha256: digest,
                        preview: super::bounded_text(&text, 8192),
                        preview_truncated: text.len() > 8192,
                    };
                    while serde_json::to_vec(&entry).map_or(true, |bytes| bytes.len() > 8192) {
                        if entry.preview.is_empty() {
                            missing = true;
                            break;
                        }
                        entry.preview =
                            super::bounded_text(&entry.preview, entry.preview.len() / 2);
                        entry.preview_truncated = true;
                    }
                    if missing {
                        break;
                    }
                    entries.push(entry);
                }
                if missing {
                    (None,"Required committed callback bytes/digest are unavailable; owner review required")
                } else {
                    let text=json!({"kind":"job_delegation_output","untrustedEvidence":true,"outputs":entries}).to_string();
                    let final_output = DelegationFinalOutput {
                        generation: 1,
                        sha256: crate::runs::committed_output_sha256(&text),
                        outputs: entries,
                    };
                    if text.len() > 64 * 1024 || final_output.validate(Some(&text)).is_err() {
                        (
                            None,
                            "Final callback aggregate exceeds its saved evidence bounds",
                        )
                    } else {
                        (Some((text,final_output)),"All required callback outputs committed in original launch order; independent owner review remains required")
                    }
                }
            }
        };
        let successful = output.is_some();
        let (before_runs, persist) = {
            let mut state = self.state.write().await;
            // Recheck current authority after transcript lookup, under the same
            // source transaction that makes final result and generation durable.
            let current = state.jobs.get(id).cloned();
            if current.as_ref() != Some(&before) {
                return;
            }
            if output.is_some() {
                match readiness(&state, &before, &refs) {
                    Readiness::Ready(_) => {}
                    Readiness::Review(current_reason) => {
                        output = None;
                        reason = current_reason;
                    }
                    Readiness::Waiting => return,
                }
            }
            let before_runs = refs
                .child_run_ids
                .iter()
                .filter_map(|id| state.runs.get(id).cloned())
                .collect::<Vec<_>>();
            let changed = helper_completion::finish_origin(&mut state, id, &refs, output, reason);
            if changed != Ok(true)
                || crate::runs::validate_origin_snapshot(&state.control_plane_snapshot()).is_err()
            {
                state.jobs.insert(id.into(), before.clone());
                for run in &before_runs {
                    state.runs.insert(run.clone());
                }
                return;
            }
            (before_runs, state.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            let mut state = self.state.write().await;
            state.jobs.insert(id.into(), before);
            for run in before_runs {
                state.runs.insert(run);
            }
            // Failed final publication must not expose successful evidence or
            // retain live callback rights. Disk's pending claim recovers review.
            let _ = helper_completion::finish_origin(
                &mut state,
                id,
                &refs,
                None,
                "Final origin evidence could not be saved; inspect effects before retrying",
            );
            tracing::warn!(job_id=id,%error,"job origin settlement save failed; no replay scheduled");
        }
        let cancel = !successful
            || self
                .state
                .read()
                .await
                .jobs
                .get(id)
                .is_some_and(|job| job.status == AgentJobStatus::NeedsReview);
        drop(transaction);
        if cancel {
            let state = self.state.read().await;
            for run_id in refs.child_run_ids.iter().chain(&refs.completion_run_ids) {
                if let Some(control) = state.live.runs().control(run_id) {
                    control.cancel.cancel();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::test_support::{lead_config, ScriptedModel};
    use crate::runs::{
        AsyncHelper, AsyncHelperBounds, DelegationOrigin, RunRecord, RunSource, RunStart,
    };

    #[test]
    fn fp05_origin_authority_known_failed_child_overrides_earlier_queued_wait_without_model_work() {
        let model = ScriptedModel::new(vec![]);
        let mut state = crate::state::DaemonState::with_model_adapter(model.clone());
        let mut config = lead_config("Ordered source");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["spawn_helper_async"])
                .unwrap(),
        );
        let owner = state.create_agent(config.clone()).unwrap().state.id;
        let mut parent = RunRecord::running(
            RunStart {
                agent_id: owner.clone(),
                session_id: "job:ordered-origin".into(),
                source: RunSource::Job,
                source_ref: Some("ordered-origin:1".into()),
                idempotency_key: Some("job:ordered-origin:attempt:1".into()),
                text: "Bounded source".into(),
                model: "recording".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        parent.id = "launch".into();
        parent.reply_message_id = Some("reply_parent".into());
        parent.finish(RunStatus::Completed, None, 4);
        parent.terminal_published = true;
        let mut first = RunRecord::queued(
            RunStart {
                agent_id: "helper-a".into(),
                session_id: "helper:first".into(),
                source: RunSource::Delegation,
                source_ref: None,
                idempotency_key: None,
                text: "First task".into(),
                model: "recording".into(),
                provider: None,
                parent_run_id: Some(parent.id.clone()),
            },
            2,
        );
        first.id = "child-first".into();
        first.async_helper = Some(AsyncHelper {
            owner_agent_id: owner.clone(),
            owner_session_id: parent.session_id.clone(),
            parent_run_id: parent.id.clone(),
            launch_key: "launch-first".into(),
            launch_fingerprint: "a".repeat(64),
            effective_bounds: Some(AsyncHelperBounds::for_config(&config)),
            origin: Some(DelegationOrigin::JobAttempt {
                job_id: "ordered-origin".into(),
                attempt: 1,
                claim_revision: 2,
            }),
            completion: HelperCompletionState::AwaitingResult {
                reserved_run_id: "callback-first".into(),
            },
            unconsumed_steers: vec![],
        });
        let mut second = first.clone();
        second.id = "child-second".into();
        second.agent_id = "helper-b".into();
        second.session_id = "helper:second".into();
        second.async_helper.as_mut().unwrap().launch_key = "launch-second".into();
        second.async_helper.as_mut().unwrap().completion = HelperCompletionState::AwaitingResult {
            reserved_run_id: "callback-second".into(),
        };
        second.finish(RunStatus::Failed, None, 4);
        let refs = PendingDelegations {
            launch_run_id: parent.id.clone(),
            child_run_ids: vec![first.id.clone(), second.id.clone()],
            completion_run_ids: vec!["callback-first".into(), "callback-second".into()],
        };
        let job:crate::jobs::AgentJobRecord=serde_json::from_value(json!({"id":"ordered-origin","agentId":owner,"title":"Order",
            "prompt":"Bounded tasks","requestKey":"ordered-origin","status":"pending_delegations","revision":3,"attempt":1,
            "createdAtMs":1,"updatedAtMs":4,"startedAtMs":2,"finishedAtMs":null,"result":null,"error":null,"maxAttempts":3,
            "requiresApproval":false,"approvedAtMs":null,"attempts":[],"goalId":null,"pendingDelegations":refs})).unwrap();
        job.validate().unwrap();
        state.jobs.insert(job.id.clone(), job.clone());
        state.runs.insert(parent);
        state.runs.insert(first);
        state.runs.insert(second);
        let queued_first_reviews = matches!(readiness(&state, &job, &refs), Readiness::Review(_));
        let mut reversed = refs.clone();
        reversed.child_run_ids.reverse();
        reversed.completion_run_ids.reverse();
        assert!(
            matches!(readiness(&state, &job, &reversed), Readiness::Review(_)),
            "known failure control"
        );
        let second = state.runs.get_mut("child-second").unwrap();
        second.status = RunStatus::Queued;
        second.finished_at_ms = None;
        second.async_helper.as_mut().unwrap().completion = HelperCompletionState::AwaitingResult {
            reserved_run_id: "callback-second".into(),
        };
        assert!(
            matches!(readiness(&state, &job, &refs), Readiness::Waiting),
            "all queued work must keep waiting"
        );
        assert!(model.requests().is_empty());
        assert!(queued_first_reviews,"a known failed later child must close the source to review without waiting for a global permit or another event");
    }
}
