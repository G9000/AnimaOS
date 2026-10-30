use crate::{attachments::AttachmentRecord, sessions::SessionRecord};
use serde::{Deserialize, Serialize};
pub(crate) mod approval;
pub(crate) mod observation;

pub(crate) const MAX_IMAGE_OPERATIONS: usize = 32;
pub(crate) const MAX_IMAGE_PROMPT_BYTES: usize = 2048;
pub(crate) const MAX_SAVED_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) fn is_tool(name: &str) -> bool {
    matches!(name, "generate_image" | "edit_image")
}

pub(crate) fn validate_generated_attachments(
    state: &crate::state::DaemonState,
    rows: &[AttachmentRecord],
) -> Result<(), String> {
    for row in rows {
        if row.id.starts_with("image_")
            && !state
                .sessions
                .get(&row.agent_id, &row.session_id)
                .is_some_and(|session| {
                    session.image_operations.iter().any(|operation| {
                        operation.status == ImageStatus::Succeeded
                            && operation.output.as_ref() == Some(row)
                            && operation
                                .version
                                .as_ref()
                                .is_some_and(|v| v.attachment_id == row.id)
                    })
                })
        {
            return Err("Generated image has no acknowledged version in this conversation".into());
        }
    }
    Ok(())
}
pub(crate) struct ImageProvider {
    pub(crate) port: std::sync::Arc<dyn anima_service_adapters::ImagePort>,
    pub(crate) key_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageStatus {
    Dispatching,
    Succeeded,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImageVersion {
    pub(crate) id: String,
    pub(crate) attachment_id: String,
    pub(crate) parent_attachment_id: Option<String>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) model: String,
    pub(crate) created_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImageOperation {
    pub(crate) id: String,
    pub(crate) run_id: String,
    pub(crate) tool_call_id: String,
    pub(crate) clear_generation: u64,
    pub(crate) prompt: String,
    pub(crate) source: Option<AttachmentRecord>,
    pub(crate) status: ImageStatus,
    pub(crate) created_at_ms: u64,
    pub(crate) output: Option<AttachmentRecord>,
    pub(crate) version: Option<ImageVersion>,
    #[serde(default)]
    pub(crate) usage: Option<anima_service_adapters::ImageUsage>,
    #[serde(default)]
    pub(crate) credential_id: String,
    #[serde(default)]
    pub(crate) approval_id: String,
}

impl ImageOperation {
    pub(crate) fn public_value(&self) -> serde_json::Value {
        let version = self.version.as_ref().zip(self.output.as_ref()).map(|(version,output)|serde_json::json!({"id":version.id,"attachment":output.metadata(),
            "parentAttachmentId":version.parent_attachment_id,"width":version.width,"height":version.height,"model":version.model,"createdAtMs":version.created_at_ms}));
        serde_json::json!({"id":self.id,"prompt":self.prompt,"sourceAttachmentId":self.source.as_ref().map(|s|&s.id),"createdAtMs":self.created_at_ms,"status":self.status,"version":version})
    }
}

pub(crate) fn valid_prompt(prompt: &str) -> bool {
    !prompt.trim().is_empty() && prompt.len() <= MAX_IMAGE_PROMPT_BYTES && !prompt.contains('\0')
}

pub(crate) fn validate_session(session: &SessionRecord) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut versions = std::collections::HashSet::new();
    if session.image_operations.len() > MAX_IMAGE_OPERATIONS {
        return Err("Saved image capacity is invalid".into());
    }
    for operation in &session.image_operations {
        if operation.id.is_empty()
            || operation.id.len() > 128
            || !ids.insert(&operation.id)
            || operation.run_id.is_empty()
            || operation.tool_call_id.is_empty()
            || operation.created_at_ms == 0
            || !valid_prompt(&operation.prompt)
            || operation.clear_generation != session.clear_generation
        {
            return Err("Saved image operation identity is invalid".into());
        }
        for record in [&operation.source, &operation.output].into_iter().flatten() {
            if record.agent_id != session.agent_id
                || record.session_id != session.id
                || record.kind != crate::attachments::AttachmentKind::Image
                || !matches!(
                    record.mime.as_str(),
                    "image/png" | "image/jpeg" | "image/webp"
                )
                || record.size == 0
                || record.size > crate::attachments::MAX_IMAGE_BYTES as u64
                || record.sha256.len() != 64
                || !record.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || record.workspace_root.is_empty()
            {
                return Err("Saved image attachment identity is invalid".into());
            }
        }
        match (&operation.status, &operation.output, &operation.version) {
            (ImageStatus::Succeeded, Some(output), Some(version))
                if version.attachment_id == output.id
                    && output.mime == "image/png"
                    && !version.id.is_empty()
                    && versions.insert(&version.id)
                    && version.width == 1024
                    && version.height == 1024
                    && version.model == "gpt-image-2.5-sunburst"
                    && version.parent_attachment_id.as_deref()
                        == operation.source.as_ref().map(|s| s.id.as_str()) => {}
            (
                ImageStatus::Dispatching | ImageStatus::Failed | ImageStatus::OutcomeUnknown,
                None,
                None,
            ) => {}
            _ => return Err("Saved image outcome is inconsistent".into()),
        }
    }
    Ok(())
}

pub(crate) fn recover(operations: &mut [ImageOperation]) {
    for operation in operations {
        if operation.status == ImageStatus::Dispatching {
            operation.status = ImageStatus::OutcomeUnknown;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation() -> ImageOperation {
        ImageOperation {
            id: "image_one".into(),
            run_id: "run".into(),
            tool_call_id: "call".into(),
            clear_generation: 0,
            prompt: "An illustrated red kite".into(),
            source: None,
            status: ImageStatus::Dispatching,
            created_at_ms: 1,
            output: None,
            version: None,
            usage: None,
            credential_id: String::new(),
            approval_id: String::new(),
        }
    }

    #[test]
    fn restart_never_replays_an_unsettled_paid_image_request() {
        let mut operations = vec![operation()];
        recover(&mut operations);
        assert_eq!(operations[0].status, ImageStatus::OutcomeUnknown);
    }

    #[test]
    fn image_snapshot_rejects_success_without_a_saved_version() {
        let mut session = SessionRecord::new(
            "agent",
            "chat:image",
            crate::sessions::SessionKind::Chat,
            crate::sessions::SessionOrigin::Web,
            "Images".into(),
            crate::sessions::TitleSource::Owner,
            1,
        );
        let mut image = operation();
        image.status = ImageStatus::Succeeded;
        session.image_operations.push(image);
        assert!(validate_session(&session).is_err());
    }
}
