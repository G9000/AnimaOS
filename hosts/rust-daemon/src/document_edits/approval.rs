//! Server-created exact review witnesses; model arguments cannot create an approval marker.
use super::registry::{
    self, DocumentEditRegistry, DocumentEditScope, DocumentEditStatus, ExpectedDestination,
};
use crate::approvals::{ApprovalDecisionKind, ApprovalRequest, ApprovalStatus, ResolvedBy};
use anima_core::{DataValue, ToolCall};
use serde::{Deserialize, Serialize};

pub(crate) const APPLY_TOOL: &str = "apply_document_edit";
pub(crate) const EXACT_MARKER: &str = "_anima_exact_document_edit";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArguments {
    proposal_id: String,
    review_sha256: String,
    file_path: String,
    expected_destination: ExpectedDestination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentEditWitness {
    arguments: ApplyArguments,
    scope: DocumentEditScope,
    candidate_sha256: String,
    source_sha256: String,
}

/// A resolved record returned by the native gate, never supplied by tool arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentEditOwnerApproval {
    record: ApprovalRequest,
}

impl DocumentEditOwnerApproval {
    pub(crate) fn id(&self) -> &str {
        &self.record.id
    }
    pub(crate) fn matches(
        &self,
        draft: &super::DocumentEditDraft,
        review_sha256: &str,
        call_id: &str,
    ) -> bool {
        let call = ToolCall {
            id: call_id.into(),
            name: APPLY_TOOL.into(),
            args: match crate::tools::generated_documents::schema_data(serde_json::json!({
                "proposal_id":draft.id,"review_sha256":review_sha256,"file_path":draft.file_path,"expected_destination":draft.expected_destination
            })) {
                DataValue::Object(args) => args,
                _ => return false,
            },
        };
        let witness = DocumentEditWitness {
            arguments: match arguments(&call) {
                Ok(value) => value,
                Err(_) => return false,
            },
            scope: draft.scope.clone(),
            candidate_sha256: draft.candidate_sha256.clone(),
            source_sha256: draft.source.sha256().into(),
        };
        witness.resolved_approval(&call, &self.record).is_ok()
    }
}

fn arguments(call: &ToolCall) -> Result<ApplyArguments, String> {
    let value = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
    let args: ApplyArguments =
        serde_json::from_value(value).map_err(|_| "Document apply arguments are invalid")?;
    if !registry::valid_id(&args.proposal_id) || !registry::valid_hash(&args.review_sha256) {
        return Err("Document apply identity is invalid".into());
    }
    Ok(args)
}

pub(crate) fn matches_record(call: &ToolCall, record: &super::DocumentEditRecord) -> bool {
    arguments(call).is_ok_and(|args| {
        call.name == APPLY_TOOL
            && args.proposal_id == record.draft.id
            && args.review_sha256 == record.review_sha256
            && args.file_path == record.draft.file_path
            && args.expected_destination == record.draft.expected_destination
    })
}

/// Call only after proving a direct ordinary owner run and current source/session custody.
pub(crate) fn capture(
    registry: &DocumentEditRegistry,
    scope: &DocumentEditScope,
    call: &ToolCall,
    now_ms: u64,
) -> Result<Option<DocumentEditWitness>, String> {
    if call.name != APPLY_TOOL {
        return Ok(None);
    }
    let arguments = arguments(call)?;
    let record = registry
        .get(&arguments.proposal_id)
        .ok_or("Document proposal is unavailable")?;
    if &record.draft.scope == scope
        && matches!(record.status, DocumentEditStatus::RecoveryRequired { .. })
    {
        return Err(super::service::UNCERTAIN.into());
    }
    if record.status != DocumentEditStatus::Prepared
        || &record.draft.scope != scope
        || record.review_sha256 != arguments.review_sha256
        || record.draft.file_path != arguments.file_path
        || record.draft.expected_destination != arguments.expected_destination
        || now_ms >= record.draft.expires_at_ms
    {
        return Err("Document review changed or expired".into());
    }
    Ok(Some(DocumentEditWitness {
        arguments,
        scope: scope.clone(),
        candidate_sha256: record.draft.candidate_sha256.clone(),
        source_sha256: record.draft.source.sha256().into(),
    }))
}

impl DocumentEditWitness {
    pub(crate) fn resolved_approval(
        &self,
        call: &ToolCall,
        record: &ApprovalRequest,
    ) -> Result<DocumentEditOwnerApproval, String> {
        let expected =
            crate::routes::data_value_to_json(&DataValue::Object(self.approval_call(call).args));
        let actual: serde_json::Value = serde_json::from_str(&record.arguments)
            .map_err(|_| "Document owner decision is invalid")?;
        if call.name != APPLY_TOOL
            || arguments(call).ok().as_ref() != Some(&self.arguments)
            || record.tool != APPLY_TOOL
            || record.tool_call_id != call.id
            || record.agent_id != self.scope.agent_id
            || record.session_id != self.scope.session_id
            || record.run_id != self.scope.run_id
            || record.status != ApprovalStatus::Allowed
            || record.arguments_truncated
            || actual != expected
            || !record.resolution.as_ref().is_some_and(|resolution| {
                resolution.resolved_by == ResolvedBy::Owner
                    && resolution.decision == Some(ApprovalDecisionKind::AllowOnce)
                    && resolution.matcher.is_none()
                    && resolution.rule_id.is_none()
            })
        {
            return Err("Document apply requires the exact owner AllowOnce decision".into());
        }
        Ok(DocumentEditOwnerApproval {
            record: record.clone(),
        })
    }
    /// The marker is constructed here only; handlers reject it in original model arguments.
    pub(crate) fn approval_call(&self, call: &ToolCall) -> ToolCall {
        let value = serde_json::json!({"_anima_exact_document_edit":true,"proposal_id":self.arguments.proposal_id,"review_sha256":self.arguments.review_sha256,"file_path":self.arguments.file_path,"expected_destination":self.arguments.expected_destination,"candidate_sha256":self.candidate_sha256,"source_sha256":self.source_sha256});
        let DataValue::Object(args) = crate::tools::generated_documents::schema_data(value) else {
            unreachable!("projection is an object");
        };
        ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args,
        }
    }
    pub(crate) fn revalidate(
        &self,
        registry: &DocumentEditRegistry,
        scope: &DocumentEditScope,
        call: &ToolCall,
        now_ms: u64,
    ) -> bool {
        capture(registry, scope, call, now_ms)
            .ok()
            .flatten()
            .as_ref()
            == Some(self)
    }
}

/// Used by native approval settlement to limit this review to allow-once or deny.
pub(crate) fn exact_record(arguments: &str) -> bool {
    let Ok(serde_json::Value::Object(value)) = serde_json::from_str(arguments) else {
        return false;
    };
    value.len() == 7
        && value.get(EXACT_MARKER).and_then(serde_json::Value::as_bool) == Some(true)
        && value
            .get("proposal_id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(registry::valid_id)
        && ["review_sha256", "candidate_sha256", "source_sha256"]
            .iter()
            .all(|key| {
                value
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(registry::valid_hash)
            })
        && value
            .get("file_path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|p| !p.is_empty() && p.len() <= 1024)
        && value
            .get("expected_destination")
            .is_some_and(|v| serde_json::from_value::<ExpectedDestination>(v.clone()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner_fixture() -> (DocumentEditWitness, ToolCall, ApprovalRequest) {
        let arguments = ApplyArguments {
            proposal_id: "proposal".into(),
            review_sha256: "a".repeat(64),
            file_path: "report.docx".into(),
            expected_destination: ExpectedDestination::Absent,
        };
        let value = crate::tools::generated_documents::schema_data(
            serde_json::to_value(&arguments).unwrap(),
        );
        let DataValue::Object(args) = value else {
            unreachable!()
        };
        let call = ToolCall {
            id: "call".into(),
            name: APPLY_TOOL.into(),
            args,
        };
        let witness = DocumentEditWitness {
            arguments,
            scope: DocumentEditScope {
                profile_key: "profile".into(),
                workspace_key: "workspace".into(),
                agent_id: "agent".into(),
                session_id: "session".into(),
                clear_generation: 1,
                run_id: "run".into(),
            },
            candidate_sha256: "b".repeat(64),
            source_sha256: "c".repeat(64),
        };
        let projected = witness.approval_call(&call);
        let mut record = ApprovalRequest::pending(
            crate::approvals::PendingApprovalStart {
                agent_id: "agent",
                session_id: "session",
                run_id: "run",
                call: &projected,
                timeout_ms: 1000,
                reason: None,
            },
            1,
        );
        record.resolve(
            ApprovalStatus::Allowed,
            crate::approvals::ApprovalResolution {
                decision: Some(ApprovalDecisionKind::AllowOnce),
                note: None,
                matcher: None,
                rule_id: None,
                resolved_by: ResolvedBy::Owner,
                resolved_at_ms: 2,
            },
        );
        (witness, call, record)
    }
    #[test]
    fn document_edits_owner_proof_rejects_reusable_forged_or_cross_run_decisions() {
        let (witness, call, record) = owner_fixture();
        assert!(witness.resolved_approval(&call, &record).is_ok());
        for kind in [
            ApprovalDecisionKind::AllowSession,
            ApprovalDecisionKind::AllowAlways,
            ApprovalDecisionKind::Deny,
        ] {
            let mut changed = record.clone();
            changed.resolution.as_mut().unwrap().decision = Some(kind);
            assert!(witness.resolved_approval(&call, &changed).is_err());
        }
        let mut changed = record.clone();
        changed.run_id = "other".into();
        assert!(witness.resolved_approval(&call, &changed).is_err());
        let mut changed = record.clone();
        changed.arguments = changed.arguments.replace(&"b".repeat(64), &"d".repeat(64));
        assert!(witness.resolved_approval(&call, &changed).is_err());
        let mut changed = record;
        changed.resolution.as_mut().unwrap().resolved_by = ResolvedBy::Restart;
        assert!(witness.resolved_approval(&call, &changed).is_err());
    }
    #[test]
    fn document_edits_exact_record_requires_the_complete_server_projection() {
        let value = serde_json::json!({"_anima_exact_document_edit":true,"proposal_id":"proposal","review_sha256":"a".repeat(64),"candidate_sha256":"b".repeat(64),"source_sha256":"c".repeat(64),"file_path":"report.docx","expected_destination":{"state":"absent"}});
        assert!(exact_record(&value.to_string()));
        assert!(!exact_record(r#"{"_anima_exact_document_edit":true}"#));
        let mut bad = value.clone();
        bad["candidate_sha256"] = serde_json::json!("unsealed");
        assert!(!exact_record(&bad.to_string()));
        let mut bad = value;
        bad["approved"] = serde_json::json!(true);
        assert!(!exact_record(&bad.to_string()));
    }
    #[test]
    fn document_edits_original_arguments_reject_an_owner_approval_claim() {
        let value = serde_json::json!({"proposal_id":"proposal","review_sha256":"a".repeat(64),"file_path":"report.docx","expected_destination":{"state":"absent"},"approved":true});
        assert!(serde_json::from_value::<ApplyArguments>(value).is_err());
    }
}
