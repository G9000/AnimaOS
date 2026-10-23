//! Capturing current authority and the shared final promotion gate.
use super::evaluation::*;
use super::package::{read_package, SkillPackage};
use super::service::blocking;
use super::{compose_skill_file, parse_skill_file, DraftApproval, SkillError, SkillService};
use crate::state::DaemonState;
use anima_core::skill_evaluation::{digest, EvaluationSuite, GRADER_VERSION};
use anima_core::AgentConfig;

pub(super) struct CapturedEvaluation {
    pub(super) identity: EvaluationIdentity,
    pub(super) suite: SuiteRecord,
    pub(super) config: AgentConfig,
    pub(super) baseline: Option<SkillPackage>,
    pub(super) candidate: SkillPackage,
}
pub(super) struct EvaluationGate {
    pub(super) draft_id: String,
    pub(super) slug: String,
    pub(super) candidate_hash: String,
    pub(super) source_hash: Option<String>,
    pub(super) evidence: Option<EvaluationIdentity>,
}
fn conflict() -> SkillError {
    SkillError::conflict("An enforced skill suite needs a passing evaluation of this exact current package, source, suite, and model configuration")
}
pub(super) fn workspace_key(root: &std::path::Path) -> String {
    root.to_string_lossy().into_owned()
}
pub(super) fn current_suite<'a>(
    state: &'a DaemonState,
    root: &std::path::Path,
    slug: &str,
) -> Option<&'a SuiteRecord> {
    state
        .skill_evaluations
        .suites
        .get(slug)
        .filter(|s| s.workspace == workspace_key(root))
}
pub(super) fn file_source_digest(id: &str, hash: Option<&str>) -> String {
    digest(&(id, "file", hash))
}

/// Must execute under the final state write guard, before changing the registry.
/// A fresh suite enabled after preflight cannot be bypassed by missing evidence.
pub(super) fn check_promotion(
    state: &DaemonState,
    root: &std::path::Path,
    gate: &EvaluationGate,
) -> Result<(), SkillError> {
    let Some(suite) = current_suite(state, root, &gate.slug).filter(|s| s.suite.enforced) else {
        return Ok(());
    };
    let identity = gate.evidence.as_ref().ok_or_else(conflict)?;
    if identity.candidate_hash != gate.candidate_hash
        || identity.draft_id != gate.draft_id
        || identity.slug != gate.slug
        || !identity_current(state, identity)
        || identity.suite_revision != suite.revision
        || (identity.baseline_hash.is_some() && identity.baseline_hash != gate.source_hash)
        || (gate.draft_id.starts_with(super::FILE_DRAFT_ID_PREFIX)
            && identity.draft_digest
                != file_source_digest(&gate.draft_id, gate.source_hash.as_deref()))
        || !state
            .skill_evaluations
            .reports
            .iter()
            .rev()
            .find(|r| r.identity == *identity)
            .is_some_and(|r| {
                r.status == EvaluationStatus::Passed
                    && anima_core::skill_evaluation::grade(&suite.suite, &r.trials)
                        == anima_core::skill_evaluation::EvaluationVerdict::Passed
            })
    {
        return Err(conflict());
    }
    Ok(())
}
pub(super) fn identity_current(state: &DaemonState, identity: &EvaluationIdentity) -> bool {
    let Some(root) = state.workspace.as_ref().map(|w| &w.root_path) else {
        return false;
    };
    let Some(suite) = current_suite(state, root, &identity.slug) else {
        return false;
    };
    let Some(agent) = state.agents.get(&suite.suite.agent_id) else {
        return false;
    };
    suite.suite.enforced
        && suite.revision == identity.suite_revision
        && suite.suite.digest() == identity.suite_digest
        && workspace_key(root) == identity.workspace
        && identity.grader == GRADER_VERSION
        && digest(agent.config()) == identity.model_config_digest
        && state
            .skills
            .get(&identity.slug)
            .map(|r| r.approved_hash.clone())
            == identity.baseline_hash
        && (identity.draft_id.starts_with(super::FILE_DRAFT_ID_PREFIX)
            || state
                .skills
                .draft(&identity.draft_id)
                .is_some_and(|d| d.is_pending() && digest(d) == identity.draft_digest))
}

impl SkillService {
    pub(super) async fn capture_evaluation(
        &self,
        id: &str,
        approval: &DraftApproval,
    ) -> Result<CapturedEvaluation, SkillError> {
        let root = self.workspace().await?;
        let (draft, slug, record, suite, config) = {
            let state = self.state.read().await;
            let draft = if id.starts_with(super::FILE_DRAFT_ID_PREFIX) {
                None
            } else {
                Some(
                    state
                        .skills
                        .draft(id)
                        .filter(|d| d.is_pending())
                        .cloned()
                        .ok_or_else(conflict)?,
                )
            };
            let slug = draft
                .as_ref()
                .map(|d| d.slug.as_str())
                .or_else(|| id.strip_prefix(super::FILE_DRAFT_ID_PREFIX))
                .ok_or(SkillError::NotFound)?
                .to_owned();
            if !super::is_valid_slug(&slug) {
                return Err(SkillError::NotFound);
            }
            let suite = current_suite(&state, &root, &slug)
                .filter(|s| s.suite.enforced)
                .cloned()
                .ok_or_else(conflict)?;
            suite.suite.validate().map_err(SkillError::invalid)?;
            if draft
                .as_ref()
                .and_then(|d| d.proposed_by.as_ref())
                .is_some_and(|by| by.agent_id != suite.suite.agent_id)
            {
                return Err(SkillError::conflict(
                    "The suite must use the originating agent's pinned current model",
                ));
            }
            let config = state
                .agents
                .get(&suite.suite.agent_id)
                .map(|a| a.config().clone())
                .ok_or_else(conflict)?;
            if config
                .provider
                .as_deref()
                .is_some_and(|provider| provider.trim().eq_ignore_ascii_case("chatgpt"))
            {
                return Err(SkillError::conflict("The ChatGPT subscription transport cannot enforce the evaluation output-token bound; select a supported provider before running this suite"));
            }
            if crate::agent_runs::is_helper_config(&config) {
                return Err(conflict());
            }
            let record = state.skills.get(&slug).cloned();
            (draft, slug, record, suite, config)
        };
        let (read_root, target) = (root.clone(), slug.clone());
        let disk = blocking(move || read_package(&read_root, &target))
            .await?
            .map_err(SkillError::Unavailable)?;
        let baseline_hash = record.as_ref().map(|r| r.approved_hash.clone());
        if baseline_hash.is_some() && disk.as_ref().map(SkillPackage::hash) != baseline_hash {
            return Err(SkillError::conflict("The complete approved baseline is unavailable; evaluation cannot compare an unapproved or missing package"));
        }
        if draft.as_ref().is_some_and(|d| d.base_hash != baseline_hash) {
            return Err(SkillError::conflict(
                "Skill base changed; review a fresh draft before evaluation",
            ));
        }
        let (candidate, draft_digest) = if let Some(draft) = draft {
            let body = approval.body.as_deref().unwrap_or(&draft.body);
            super::validate_body(body).map_err(SkillError::invalid)?;
            (
                SkillPackage {
                    bytes: compose_skill_file(&draft.name, &draft.description, body).into_bytes(),
                    files: approval
                        .files
                        .clone()
                        .unwrap_or_else(|| draft.files.clone()),
                },
                digest(&draft),
            )
        } else {
            if record.is_some() {
                return Err(conflict());
            }
            let package = disk.as_ref().ok_or(SkillError::NotFound)?;
            let reviewed = approval
                .hash
                .as_deref()
                .ok_or_else(|| SkillError::invalid(super::SKILL_HASH_REQUIRED))?;
            if !package.hash().eq_ignore_ascii_case(reviewed) {
                return Err(SkillError::conflict(super::SKILL_HASH_MISMATCH));
            }
            let parsed = parse_skill_file(&package.bytes).map_err(SkillError::invalid)?;
            let bytes = if approval.body.is_some() || approval.files.is_some() {
                let body = approval.body.as_deref().unwrap_or(&parsed.body);
                super::validate_body(body).map_err(SkillError::invalid)?;
                compose_skill_file(&parsed.name, &parsed.description, body).into_bytes()
            } else {
                package.bytes.clone()
            };
            (
                SkillPackage {
                    bytes,
                    files: approval
                        .files
                        .clone()
                        .unwrap_or_else(|| package.files.clone()),
                },
                file_source_digest(id, Some(&package.hash())),
            )
        };
        anima_core::skill_packages::validate_skill_package(&candidate.bytes, &candidate.files)
            .map_err(SkillError::invalid)?;
        let model_config_digest = digest(&config);
        if model_config_digest == "invalid-canonical-json" {
            return Err(conflict());
        }
        Ok(CapturedEvaluation {
            identity: EvaluationIdentity {
                workspace: workspace_key(&root),
                slug,
                suite_revision: suite.revision,
                suite_digest: suite.suite.digest(),
                baseline_hash: baseline_hash.clone(),
                candidate_hash: candidate.hash(),
                draft_id: id.into(),
                draft_digest,
                model_config_digest,
                grader: GRADER_VERSION.into(),
            },
            suite,
            config,
            baseline: if baseline_hash.is_some() { disk } else { None },
            candidate,
        })
    }

    pub(super) async fn evaluation_preflight(
        &self,
        id: &str,
        approval: &DraftApproval,
    ) -> Result<Option<EvaluationIdentity>, SkillError> {
        let enforced = {
            let state = self.state.read().await;
            let Some(root) = state.workspace.as_ref().map(|w| &w.root_path) else {
                return Ok(None);
            };
            let slug = state
                .skills
                .draft(id)
                .map(|d| d.slug.as_str())
                .or_else(|| id.strip_prefix(super::FILE_DRAFT_ID_PREFIX));
            slug.and_then(|slug| current_suite(&state, root, slug))
                .is_some_and(|s| s.suite.enforced)
        };
        if !enforced {
            return Ok(None);
        }
        let row = self
            .start_or_join_evaluation(id, approval.clone(), false)
            .await?;
        let settled = self.wait_evaluation(&row.id).await?;
        if settled.status != EvaluationStatus::Passed {
            return Err(conflict());
        }
        Ok(Some(settled.identity))
    }

    pub(crate) async fn evaluation_suite(
        &self,
        slug: &str,
    ) -> Result<Option<SuiteRecord>, SkillError> {
        let root = self.workspace().await?;
        Ok(current_suite(&*self.state.read().await, &root, slug).cloned())
    }
    pub(crate) async fn set_evaluation_suite(
        &self,
        slug: &str,
        suite: EvaluationSuite,
    ) -> Result<SuiteRecord, SkillError> {
        if !super::is_valid_slug(slug) {
            return Err(SkillError::invalid(super::SKILL_SLUG_INVALID));
        }
        suite.validate().map_err(SkillError::invalid)?;
        let slug = slug.to_owned();
        self.locked(move |service, root| async move {
            let mut state = service.state.write().await;
            if state.control_plane_store.is_none() {
                return Err(SkillError::Unavailable(
                    "Durable storage is required for evaluation suites".into(),
                ));
            }
            if !state.agents.contains_key(&suite.agent_id) {
                return Err(SkillError::invalid("Choose an existing evaluation agent"));
            }
            if !state.skill_evaluations.suites.contains_key(&slug)
                && state.skill_evaluations.suites.len() >= super::MAX_SKILLS
            {
                return Err(SkillError::conflict("Too many evaluation suites"));
            }
            let previous = state.skill_evaluations.clone();
            let revision = state
                .skill_evaluations
                .suites
                .get(&slug)
                .map_or(Some(1), |s| s.revision.checked_add(1))
                .ok_or_else(conflict)?;
            let record = SuiteRecord {
                workspace: workspace_key(&root),
                revision,
                suite,
            };
            state.skill_evaluations.invalidate(&slug);
            state.skill_evaluations.suites.insert(slug, record.clone());
            let persist = state.control_plane_persist_request();
            drop(state);
            if let Err(error) = persist.save().await {
                service.state.write().await.skill_evaluations = previous;
                return Err(SkillError::Unavailable(error.to_string()));
            }
            Ok(record)
        })
        .await
    }

    pub(crate) async fn draft_evaluation_report(
        &self,
        id: &str,
    ) -> Result<Option<EvaluationReport>, SkillError> {
        let root = self.workspace().await?;
        let Some(mut report) = self
            .state
            .read()
            .await
            .skill_evaluations
            .reports
            .iter()
            .rev()
            .find(|r| r.identity.workspace == workspace_key(&root) && r.identity.draft_id == id)
            .cloned()
        else {
            return Ok(None);
        };
        if matches!(
            report.status,
            EvaluationStatus::Passed | EvaluationStatus::Running
        ) {
            let (workspace, slug) = (root, report.identity.slug.clone());
            let disk = blocking(move || read_package(&workspace, &slug)).await;
            let (readable, disk_hash) = match disk {
                Ok(Ok(package)) => (true, package.map(|p| p.hash())),
                _ => (false, None),
            };
            // Compare the stored identity directly. Reconstructing a candidate
            // with a default approval would incorrectly invalidate legitimate
            // owner body/supporting-file overrides that this report evaluated.
            let current = identity_current(&*self.state.read().await, &report.identity)
                && readable
                && (report.identity.baseline_hash.is_none()
                    || report.identity.baseline_hash == disk_hash)
                && (!id.starts_with(super::FILE_DRAFT_ID_PREFIX)
                    || report.identity.draft_digest
                        == file_source_digest(id, disk_hash.as_deref()));
            if !current {
                report.status = EvaluationStatus::Stale;
                report
                    .finished_at_ms
                    .get_or_insert_with(anima_core::primitives::now_millis);
                report.error = Some("The current model, suite, registry, or disk source differs from this report's evaluated identity".into());
            }
        }
        Ok(Some(report))
    }
    pub(crate) async fn close_evaluations(&self) {
        self.state.read().await.skill_evaluation_slots.close();
        let service = self.clone();
        let _ = tokio::spawn(async move {
            let _transaction = service.transactions.lock().await;
            let mut state = service.state.write().await;
            state
                .skill_evaluations
                .interrupt_active(anima_core::primitives::now_millis());
            let persist = state.control_plane_persist_request();
            drop(state);
            persist
                .save()
                .await
                .map_err(|e| SkillError::Unavailable(e.to_string()))
        })
        .await;
    }
}
