//! Reviewed continuation of interrupted owner work. Original action evidence is
//! immutable; a continuation is a new run with current permissions.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use super::{ActionStatus, RunRecord, RunSource, RunStatus};

pub(crate) const MAX_RECOVERY_ATTEMPTS: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryRecord {
    pub(crate) fingerprint: String,
    pub(crate) continuation_run_id: String,
    pub(crate) reviewed_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoverySummary {
    pub(crate) source_run_id: String,
    pub(crate) fingerprint: String,
    pub(crate) eligible: bool,
    pub(crate) review_required: bool,
    pub(crate) unknown_action_count: usize,
    pub(crate) remaining_attempts: u8,
    pub(crate) continuation_run_id: Option<String>,
    pub(crate) evidence: super::continuity::RunContinuityEvidence,
    pub(crate) root_run_id: Option<String>,
    pub(crate) checkpoint_sequence: Option<u64>,
    pub(crate) checkpoint_phase: Option<String>,
    pub(crate) recovery_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project_context_notice: Option<String>,
}

pub(crate) fn summary(run: &RunRecord) -> RecoverySummary {
    let unknown = run
        .action_receipts
        .iter()
        .filter(|r| r.status == ActionStatus::Started)
        .count()
        + run
            .tools_started
            .iter()
            .filter(|name| !run.action_receipts.iter().any(|r| &r.name == *name))
            .count();
    let eligible = matches!(run.status, RunStatus::Interrupted | RunStatus::Failed)
        && matches!(run.source, RunSource::Web | RunSource::Api)
        && (run.system_followup != Some(true) || run.recovery_of.is_some())
        && run.async_helper.is_none()
        && run.helper_completion.is_none()
        && run.stop.is_none()
        && run.recovery_attempt < MAX_RECOVERY_ATTEMPTS;
    let evidence = serde_json::to_vec(&(
        &run.id,
        &run.agent_id,
        &run.session_id,
        run.status,
        &run.input,
        &run.edited_input,
        &run.tools_started,
        &run.action_receipts,
        &run.error,
        run.recovery_attempt,
        &run.turn,
        &run.project_witness,
        &run.project_context,
    ))
    .expect("run evidence serializes");
    RecoverySummary {
        source_run_id: run.id.clone(),
        // A late native acknowledgement may settle an originally unknown
        // receipt. Keep the reviewed fingerprint as historical evidence once
        // its single continuation is saved; that acknowledgement cannot reopen it.
        fingerprint: run
            .recovery
            .as_ref()
            .map(|r| r.fingerprint.clone())
            .unwrap_or_else(|| hex::encode(Sha256::digest(evidence))),
        eligible,
        review_required: !run.tools_started.is_empty()
            || !run.action_receipts.is_empty()
            || run.started_at_ms.is_some(),
        unknown_action_count: unknown,
        remaining_attempts: MAX_RECOVERY_ATTEMPTS.saturating_sub(run.recovery_attempt),
        continuation_run_id: run.recovery.as_ref().map(|r| r.continuation_run_id.clone()),
        evidence: super::continuity::project(run),
        root_run_id: run.turn.as_ref().map(|t| t.binding.root_run_id.clone()),
        checkpoint_sequence: run
            .turn
            .as_ref()
            .and_then(|t| t.checkpoint.as_ref().map(|c| c.sequence)),
        checkpoint_phase: run
            .turn
            .as_ref()
            .and_then(|t| t.checkpoint.as_ref().map(|c| c.phase.as_str().to_string())),
        recovery_state: if run.status == RunStatus::Completed {
            "already_completed"
        } else if run.stop.is_some() || run.status == RunStatus::Cancelled {
            "stopped"
        } else if eligible {
            "owner_review_required"
        } else {
            "invalid_or_stale"
        }
        .into(),
        project_context_notice: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::RunStart;
    fn interrupted() -> RunRecord {
        let mut r = RunRecord::queued(
            RunStart {
                agent_id: "a".into(),
                session_id: "chat:one".into(),
                source: RunSource::Web,
                source_ref: None,
                idempotency_key: Some("original".into()),
                text: "Finish the report".into(),
                model: "test".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        r.finish(RunStatus::Interrupted, None, 2);
        r
    }
    #[test]
    fn recovery_only_unstarted_evidence_can_continue_without_review() {
        let mut r = interrupted();
        assert!(summary(&r).eligible);
        assert!(!summary(&r).review_required);
        r.tools_started.push("send_email".into());
        assert!(summary(&r).review_required);
        assert_eq!(summary(&r).unknown_action_count, 1);
    }
    #[test]
    fn recovery_fingerprint_tracks_evidence_and_attempt_budget() {
        let mut r = interrupted();
        let before = summary(&r).fingerprint;
        r.input.text.push('!');
        assert_ne!(summary(&r).fingerprint, before);
        r.recovery_attempt = MAX_RECOVERY_ATTEMPTS;
        assert!(!summary(&r).eligible);
        r.recovery_attempt = 0;
        r.source = RunSource::Schedule;
        assert!(!summary(&r).eligible);
        r.source = RunSource::Web;
        r.system_followup = Some(true);
        assert!(!summary(&r).eligible);
        r.recovery_of = Some("prior-reviewed-attempt".into());
        r.recovery_attempt = 1;
        assert!(
            summary(&r).eligible,
            "reviewed recovery prompts are continuable without becoming owner-authored"
        );
        r.recovery_attempt = MAX_RECOVERY_ATTEMPTS;
        assert!(!summary(&r).eligible);
    }

    #[test]
    fn recovery_fingerprint_binds_admitted_project_evidence() {
        let mut run = interrupted();
        let before = summary(&run).fingerprint;
        run.project_context = Some(crate::projects::ProjectContextManifest {
            version: 1,
            run_id: run.id.clone(),
            agent_id: run.agent_id.clone(),
            session_id: run.session_id.clone(),
            session_binding_revision: 1,
            project: crate::projects::RunProjectBinding {
                project_id: format!("project_{}", uuid::Uuid::new_v4()),
                project_revision: 1,
                access_epoch: 1,
                member_generation: 1,
            },
            captured_at_ms: 2,
            instructions_sha256: "a".repeat(64),
            rendered_sha256: "b".repeat(64),
            estimated_tokens: 10,
            sources: vec![],
        });
        assert_ne!(summary(&run).fingerprint, before);
        let admitted = summary(&run).fingerprint;
        run.project_context.as_mut().unwrap().rendered_sha256 = "c".repeat(64);
        assert_ne!(summary(&run).fingerprint, admitted);
    }

    #[test]
    fn recovery_snapshot_rejects_cross_scope_and_reset_attempt_links() {
        let mut source = interrupted();
        let mut child = interrupted();
        child.recovery_of = Some(source.id.clone());
        child.recovery_attempt = 1;
        source.recovery = Some(RecoveryRecord {
            fingerprint: summary(&source).fingerprint,
            continuation_run_id: child.id.clone(),
            reviewed_at_ms: 3,
        });
        assert!(super::super::RunLedger::validate(&[source.clone(), child.clone()]).is_ok());
        let reviewed = summary(&source).fingerprint;
        // A native owner may acknowledge a formerly unknown effect after the
        // source became terminal. That cannot invalidate historical review.
        source.tools_started.push("write_file".into());
        assert_eq!(summary(&source).fingerprint, reviewed);
        assert!(super::super::RunLedger::validate(&[source.clone(), child.clone()]).is_ok());
        child.agent_id = "foreign".into();
        assert!(super::super::RunLedger::validate(&[source.clone(), child.clone()]).is_err());
        child.agent_id = source.agent_id.clone();
        child.recovery_attempt = 0;
        assert!(super::super::RunLedger::validate(&[source, child]).is_err());
    }
}
