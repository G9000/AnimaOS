use crate::{
    approvals::{ApprovalDecisionKind, ApprovalRequest, ApprovalStatus, ResolvedBy},
    attachments::AttachmentRecord,
    runs::RunLink,
};
use anima_core::{DataValue, ToolCall};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct ImageWitness {
    pub(crate) run: RunLink,
    pub(crate) call: ToolCall,
    pub(crate) prompt: String,
    pub(crate) source: Option<AttachmentRecord>,
    pub(crate) generation: u64,
    pub(crate) project_id: Option<String>,
    pub(crate) project_binding_revision: u64,
    pub(crate) workspace: PathBuf,
}

#[derive(Clone)]
pub(crate) struct ImageOwnerApproval {
    pub(crate) witness: ImageWitness,
    record: ApprovalRequest,
}

impl ImageWitness {
    pub(crate) fn approval_call(&self) -> ToolCall {
        let value = serde_json::json!({"_anima_exact_image":true,"prompt":self.prompt,
            "sourceAttachmentId":self.source.as_ref().map(|s|&s.id),"sourceName":self.source.as_ref().map(|s|&s.name),"sourceSha256":self.source.as_ref().map(|s|&s.sha256),
            "provider":"openai","model":anima_service_adapters::IMAGE_MODEL,"size":"1024x1024","quality":"medium","count":1,
            "disclosure":"Send this prompt and the selected image, if any, to OpenAI using the saved API key. API charges apply. Save one new local version and preserve the original. Remote cancellation and refunds cannot be guaranteed."});
        let DataValue::Object(args) = crate::tools::generated_documents::schema_data(value) else {
            unreachable!()
        };
        ToolCall {
            id: self.call.id.clone(),
            name: self.call.name.clone(),
            args,
        }
    }

    pub(crate) fn resolve(&self, record: &ApprovalRequest) -> Result<ImageOwnerApproval, String> {
        let expected =
            crate::routes::data_value_to_json(&DataValue::Object(self.approval_call().args));
        let actual: serde_json::Value =
            serde_json::from_str(&record.arguments).map_err(|_| "Image review is invalid")?;
        if !super::is_tool(&self.call.name)
            || record.tool != self.call.name
            || record.agent_id != self.run.agent_id
            || record.session_id != self.run.session_id
            || record.run_id != self.run.run_id
            || record.tool_call_id != self.call.id
            || record.status != ApprovalStatus::Allowed
            || record.arguments_truncated
            || actual != expected
            || !record.resolution.as_ref().is_some_and(|r| {
                r.resolved_by == ResolvedBy::Owner
                    && r.decision == Some(ApprovalDecisionKind::AllowOnce)
                    && r.matcher.is_none()
                    && r.rule_id.is_none()
            })
        {
            return Err("Images require the exact owner AllowOnce review".into());
        }
        Ok(ImageOwnerApproval {
            witness: self.clone(),
            record: record.clone(),
        })
    }
}

impl ImageOwnerApproval {
    pub(crate) fn record(&self) -> &ApprovalRequest {
        &self.record
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approvals::{
        ApprovalDecisionKind, ApprovalResolution, ApprovalStatus, PendingApprovalStart, ResolvedBy,
    };

    fn witness() -> ImageWitness {
        ImageWitness {
            run: RunLink {
                agent_id: "agent".into(),
                session_id: "chat:image".into(),
                run_id: "run".into(),
            },
            call: ToolCall {
                id: "call".into(),
                name: "generate_image".into(),
                args: std::collections::BTreeMap::from([(
                    "prompt".into(),
                    DataValue::String("A red kite".into()),
                )]),
            },
            prompt: "A red kite".into(),
            source: None,
            generation: 0,
            project_id: None,
            project_binding_revision: 0,
            workspace: PathBuf::from("workspace"),
        }
    }
    fn allowed(witness: &ImageWitness, decision: ApprovalDecisionKind) -> ApprovalRequest {
        let call = witness.approval_call();
        let mut request = ApprovalRequest::pending(
            PendingApprovalStart {
                agent_id: "agent",
                session_id: "chat:image",
                run_id: "run",
                call: &call,
                timeout_ms: 1000,
                reason: None,
            },
            1,
        );
        request.resolve(
            ApprovalStatus::Allowed,
            ApprovalResolution {
                decision: Some(decision),
                note: None,
                matcher: None,
                rule_id: None,
                resolved_by: ResolvedBy::Owner,
                resolved_at_ms: 2,
            },
        );
        request
    }
    #[test]
    fn image_review_contains_the_complete_remote_transfer_and_charge_disclosure() {
        let call = witness().approval_call();
        assert_eq!(
            call.args.get("_anima_exact_image"),
            Some(&DataValue::Bool(true))
        );
        let Some(DataValue::String(disclosure)) = call.args.get("disclosure") else {
            panic!("missing disclosure")
        };
        assert!(disclosure.contains("OpenAI") && disclosure.contains("API charges"));
    }
    #[test]
    fn only_the_exact_owner_allow_once_creates_image_authority() {
        let witness = witness();
        let request = allowed(&witness, ApprovalDecisionKind::AllowOnce);
        assert!(witness.resolve(&request).is_ok());
        for decision in [
            ApprovalDecisionKind::AllowAlways,
            ApprovalDecisionKind::AllowSession,
        ] {
            assert!(witness.resolve(&allowed(&witness, decision)).is_err());
        }
        let mut forged = request.clone();
        forged.arguments = "{}".into();
        assert!(witness.resolve(&forged).is_err());
        let mut changed = witness.clone();
        changed.prompt = "Another prompt".into();
        assert!(changed.resolve(&request).is_err());
    }
}
