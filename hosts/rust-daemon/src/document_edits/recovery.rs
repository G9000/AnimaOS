//! Read-only publication evidence. These records never authorize a new file effect.
use super::{
    approval::{self, DocumentEditOwnerApproval},
    DocumentEditRecord, DocumentEditScope, DocumentPublicationPhase,
};
use crate::runs::{ActionStatus, RunRecord};
use anima_core::{DataValue, ToolCall};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_PROOF_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublicationReadbackProof {
    version: u8,
    proposal_id: String,
    review_sha256: String,
    candidate_sha256: String,
    candidate_bytes: u64,
    source_sha256: String,
    file_path: String,
    scope: DocumentEditScope,
    owner_approval: DocumentEditOwnerApproval,
    tool_call_id: String,
    pub(crate) checkpoint_id: Option<String>,
    observed_at_ms: u64,
}

pub(crate) fn apply_call(record: &DocumentEditRecord, call_id: &str) -> ToolCall {
    let value = crate::tools::generated_documents::schema_data(
        serde_json::json!({"proposal_id":record.draft.id,"review_sha256":record.review_sha256,"file_path":record.draft.file_path,"expected_destination":record.draft.expected_destination}),
    );
    let DataValue::Object(args) = value else {
        unreachable!("apply projection is an object")
    };
    ToolCall {
        id: call_id.into(),
        name: approval::APPLY_TOOL.into(),
        args,
    }
}

pub(crate) fn native_intent(
    record: &DocumentEditRecord,
    run: &RunRecord,
    call_id: &str,
) -> Option<ActionStatus> {
    if run.id != record.draft.scope.run_id
        || run.agent_id != record.draft.scope.agent_id
        || run.session_id != record.draft.scope.session_id
        || run.source != crate::runs::RunSource::Web
        || run.parent_run_id.is_some()
    {
        return None;
    }
    let call = apply_call(record, call_id);
    let expected = crate::routes::data_value_to_json(&DataValue::Object(call.args));
    run.action_receipts
        .iter()
        .find(|receipt| {
            receipt.tool_call_id == call_id
                && receipt.name == approval::APPLY_TOOL
                && receipt.durability == crate::runs::ActionDurability::Persistent
                && !receipt.arguments_truncated
                && serde_json::from_str::<serde_json::Value>(&receipt.arguments_preview)
                    .ok()
                    .as_ref()
                    == Some(&expected)
        })
        .map(|receipt| receipt.status)
}

impl PublicationReadbackProof {
    pub(crate) fn from_native_readback(
        record: &DocumentEditRecord,
        run: &RunRecord,
        call_id: &str,
        observed_sha256: &str,
        observed_bytes: u64,
        checkpoint_id: Option<String>,
        now_ms: u64,
    ) -> Result<Self, String> {
        if record.publication_phase != DocumentPublicationPhase::Publishing
            || !matches!(&record.status,super::DocumentEditStatus::Applying{tool_call_id,..}|super::DocumentEditStatus::RecoveryRequired{tool_call_id,..} if tool_call_id==call_id)
            || observed_sha256 != record.draft.candidate_sha256
            || observed_bytes != record.draft.candidate_bytes
            || native_intent(record, run, call_id) != Some(ActionStatus::Started)
        {
            return Err("Document native readback evidence is invalid".into());
        }
        let owner_approval = record
            .owner_approval
            .clone()
            .filter(|proof| proof.matches(&record.draft, &record.review_sha256, call_id))
            .ok_or("Document native owner evidence is unavailable")?;
        Ok(Self {
            version: 1,
            proposal_id: record.draft.id.clone(),
            review_sha256: record.review_sha256.clone(),
            candidate_sha256: record.draft.candidate_sha256.clone(),
            candidate_bytes: observed_bytes,
            source_sha256: record.draft.source.sha256().into(),
            file_path: record.draft.file_path.clone(),
            scope: record.draft.scope.clone(),
            owner_approval,
            tool_call_id: call_id.into(),
            checkpoint_id,
            observed_at_ms: now_ms,
        })
    }
    pub(crate) fn proposal_id(&self) -> &str {
        &self.proposal_id
    }
    pub(crate) fn matches(
        &self,
        record: &DocumentEditRecord,
        run: &RunRecord,
        call_id: &str,
    ) -> bool {
        self.version == 1
            && self.proposal_id == record.draft.id
            && self.review_sha256 == record.review_sha256
            && self.candidate_sha256 == record.draft.candidate_sha256
            && self.candidate_bytes == record.draft.candidate_bytes
            && self.source_sha256 == record.draft.source.sha256()
            && self.file_path == record.draft.file_path
            && self.scope == record.draft.scope
            && self.tool_call_id == call_id
            && record.publication_phase == DocumentPublicationPhase::Publishing
            && record.owner_approval.as_ref() == Some(&self.owner_approval)
            && self
                .owner_approval
                .matches(&record.draft, &record.review_sha256, call_id)
            && native_intent(record, run, call_id).is_some()
    }
    pub(crate) fn same_publication(&self, other: &Self) -> bool {
        let mut left = self.clone();
        left.observed_at_ms = 0;
        let mut right = other.clone();
        right.observed_at_ms = 0;
        left == right
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReconciliationDisposition {
    PublicationProven,
    NoEffectProven,
    ObservedCandidateEffectUnknown,
    EffectUnknown,
    ScopeUnavailable,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReconciliationReport {
    pub(crate) proposal_id: String,
    pub(crate) disposition: ReconciliationDisposition,
    pub(crate) native_action_status: Option<ActionStatus>,
}

impl super::DocumentEditService {
    /// Reconciles historical evidence only. It never dispatches, retries or approves a write.
    pub(crate) async fn reconcile_pending(&self) -> Result<Vec<ReconciliationReport>, String> {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction = service.transactions.clone().lock_owned().await;
            let (old, store, root, checkpoint_root, owner, runs) = {
                let state = service.state.read().await;
                state
                    .ensure_profile_owned()
                    .map_err(|_| "Document profile ownership was lost")?;
                let Some(store) = state.document_edit_store.clone() else {
                    return Ok(Vec::new());
                };
                let Some(workspace) = state.workspace.as_ref() else {
                    return Ok(Vec::new());
                };
                let runs = state
                    .document_edits
                    .records()
                    .filter_map(|record| {
                        state
                            .runs
                            .get(&record.draft.scope.run_id)
                            .cloned()
                            .map(|run| (record.draft.id.clone(), run))
                    })
                    .collect::<std::collections::BTreeMap<_, _>>();
                (
                    state.document_edits.clone(),
                    store,
                    workspace.root_path.clone(),
                    state.checkpoint_root.clone(),
                    state.profile_owner(),
                    runs,
                )
            };
            if let Some(owner) = &owner {
                owner
                    .verify_owned()
                    .await
                    .map_err(|_| "Document profile ownership was lost")?;
            }
            old.validate()?;
            let workspace = crate::tools::workspace_bytes::WorkspaceBytes::open(&root)
                .map_err(|_| "Document recovery workspace is unavailable")?;
            let workspace_key =
                crate::workspace_file_preview::workspace_key(&workspace.canonical_root);
            let profile_key = store.profile_key();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
            let cancel = anima_core::CancelSignal::new();
            let mut next = old.clone();
            let mut reports = Vec::new();
            for record in old
                .records()
                .filter(|record| record.status != super::DocumentEditStatus::Prepared)
            {
                let call_id = match &record.status {
                    super::DocumentEditStatus::Applying { tool_call_id, .. }
                    | super::DocumentEditStatus::RecoveryRequired { tool_call_id, .. } => {
                        tool_call_id.as_str()
                    }
                    _ => continue,
                };
                let run = runs.get(&record.draft.id);
                let status = run.and_then(|run| native_intent(record, run, call_id));
                let report = |disposition| ReconciliationReport {
                    proposal_id: record.draft.id.clone(),
                    disposition,
                    native_action_status: status,
                };
                // An ordinary live handler may still be between its two durable fences.
                if matches!(record.status, super::DocumentEditStatus::Applying { .. })
                    && run.is_some_and(|run| run.status.is_in_flight())
                    && status == Some(ActionStatus::Started)
                {
                    continue;
                }
                if record.draft.scope.profile_key != profile_key
                    || record.draft.scope.workspace_key != workspace_key
                    || !record.owner_approval.as_ref().is_some_and(|proof| {
                        proof.matches(&record.draft, &record.review_sha256, call_id)
                    })
                    || status.is_none()
                {
                    reports.push(report(ReconciliationDisposition::ScopeUnavailable));
                    continue;
                }
                let path = record.draft.file_path.clone();
                let read_root = root.clone();
                let actual = match crate::attachments::documents::source_read::read_retained(
                    move || super::service::workspace_snapshot(&read_root, &path),
                    &cancel,
                    deadline,
                )
                .await
                {
                    Ok(Ok(value)) => value,
                    _ => {
                        reports.push(report(ReconciliationDisposition::EffectUnknown));
                        continue;
                    }
                };
                if record.publication_phase == DocumentPublicationPhase::Reserved
                    && super::service::destination_matches(
                        actual.as_deref(),
                        &record.draft.expected_destination,
                    )
                    .is_ok()
                {
                    next.cancel(&record.draft.id, true)?;
                    reports.push(report(ReconciliationDisposition::NoEffectProven));
                    continue;
                }
                let candidate = actual.as_ref().is_some_and(|bytes| {
                    bytes.len() as u64 == record.draft.candidate_bytes
                        && super::service::digest(bytes) == record.draft.candidate_sha256
                });
                if !candidate || record.publication_phase != DocumentPublicationPhase::Publishing {
                    reports.push(report(ReconciliationDisposition::EffectUnknown));
                    continue;
                }
                let Some(run) = run else {
                    reports.push(report(ReconciliationDisposition::ScopeUnavailable));
                    continue;
                };
                let mut checkpoint_id = None;
                let proven = match store.readback_proof(&record.draft.id) {
                    Ok(Some(proof)) if proof.matches(record, run, call_id) => {
                        checkpoint_id = proof.checkpoint_id;
                        true
                    }
                    Ok(Some(_)) | Err(_) => false,
                    Ok(None) => {
                        if status == Some(ActionStatus::Succeeded) {
                            true
                        } else if let Some(checkpoint_root) = &checkpoint_root {
                            let expected = match &record.draft.expected_destination {
                                super::ExpectedDestination::Absent => None,
                                super::ExpectedDestination::Present { sha256 } => {
                                    Some(sha256.as_str())
                                }
                            };
                            let link = crate::runs::RunLink {
                                agent_id: record.draft.scope.agent_id.clone(),
                                session_id: record.draft.scope.session_id.clone(),
                                run_id: record.draft.scope.run_id.clone(),
                            };
                            match crate::filesystem_checkpoints::NativeCheckpointService::new(
                                checkpoint_root.clone(),
                            )
                            .with_profile_owner(owner.clone())
                            .publication_evidence(
                                &root,
                                &link,
                                &apply_call(record, call_id),
                                &record.draft.file_path,
                                expected,
                                &record.draft.candidate_sha256,
                                record.draft.candidate_bytes,
                            ) {
                                Ok(Some(evidence)) => {
                                    checkpoint_id = Some(evidence.checkpoint_id);
                                    true
                                }
                                _ => false,
                            }
                        } else {
                            false
                        }
                    }
                };
                if proven {
                    next.confirm_publication(
                        &record.draft.id,
                        call_id,
                        &record.draft.candidate_sha256,
                        checkpoint_id,
                        anima_core::primitives::now_millis(),
                    )?;
                    reports.push(report(ReconciliationDisposition::PublicationProven));
                } else {
                    reports.push(report(
                        ReconciliationDisposition::ObservedCandidateEffectUnknown,
                    ));
                }
            }
            let state = service.state.read().await;
            state
                .ensure_profile_owned()
                .map_err(|_| "Document profile ownership was lost")?;
            if state
                .workspace
                .as_ref()
                .map(|workspace| &workspace.root_path)
                != Some(&root)
                || state
                    .document_edit_store
                    .as_ref()
                    .map(super::DocumentEditStore::profile_key)
                    .as_deref()
                    != Some(profile_key.as_str())
            {
                return Err("Document recovery scope changed".into());
            }
            drop(state);
            if next != old {
                service.persist(&old, next).await?;
            }
            Ok(reports)
        })
        .await
        .map_err(|_| "Document read-only reconciliation did not settle")?
    }
}
