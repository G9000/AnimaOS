//! Selected-chat viewer grants. A share ID is a locator, never a credential.
//! The owning session persists grants; clear generation changes end access.
use anima_core::MessageRole;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::sessions::{views::PageMessage, SessionKind, SessionRecord};

pub(crate) const VIEWER_ID_HEADER: &str = "x-anima-viewer-id";
pub(crate) const VIEWER_SECRET_HEADER: &str = "x-anima-viewer-secret";
pub(crate) const MAX_GRANTS_PER_CHAT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConversationShareGrant {
    pub(crate) id: String,
    pub(crate) viewer_id: String,
    pub(crate) viewer_name: String,
    credential_sha256: String,
    // Older records deserialize successfully, but empty bindings never authorize.
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    session_id: String,
    session_created_at_ms: u64,
    clear_generation: u64,
    pub(crate) created_at_ms: u64,
    pub(crate) revoked_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationShareView {
    pub(crate) id: String,
    pub(crate) viewer_id: String,
    pub(crate) viewer_name: String,
    pub(crate) created_at_ms: u64,
    pub(crate) revoked_at_ms: Option<u64>,
    pub(crate) active: bool,
}

impl ConversationShareGrant {
    fn matches_session(&self, session: &SessionRecord) -> bool {
        session.kind == SessionKind::Chat
            && self.agent_id == session.agent_id
            && self.session_id == session.id
            && self.session_created_at_ms == session.created_at_ms
            && self.clear_generation == session.clear_generation
            && session.pending_clear_run_id.is_none()
    }

    pub(crate) fn authorizes(
        &self,
        session: &SessionRecord,
        viewer: &str,
        secret: &str,
        _now_ms: u64,
    ) -> bool {
        let well_formed = secret.len() == 64 && secret.bytes().all(|b| b.is_ascii_hexdigit());
        let expected = digest(&self.id, viewer, secret);
        self.revoked_at_ms.is_none()
            && self.matches_session(session)
            && self.viewer_id == viewer
            && well_formed
            && constant_time_eq(self.credential_sha256.as_bytes(), expected.as_bytes())
    }

    pub(crate) fn view(&self, session: &SessionRecord) -> ConversationShareView {
        ConversationShareView {
            id: self.id.clone(),
            viewer_id: self.viewer_id.clone(),
            viewer_name: self.viewer_name.clone(),
            created_at_ms: self.created_at_ms,
            revoked_at_ms: self.revoked_at_ms,
            active: self.revoked_at_ms.is_none() && self.matches_session(session),
        }
    }
}

pub(crate) fn issue(
    session: &SessionRecord,
    name: &str,
    now_ms: u64,
) -> Result<(ConversationShareGrant, String), &'static str> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || crate::memory_text::has_hidden_text(name)
    {
        return Err("Viewer name must contain 1-80 visible characters");
    }
    if session.kind != SessionKind::Chat || session.pending_clear_run_id.is_some() {
        return Err("Only an available chat conversation can be shared");
    }
    if session.conversation_shares.len() >= MAX_GRANTS_PER_CHAT {
        return Err("This conversation has reached its viewer grant limit");
    }
    let id = format!("share-{}", uuid::Uuid::new_v4());
    let viewer_id = format!("viewer-{}", uuid::Uuid::new_v4());
    let secret = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let grant = ConversationShareGrant {
        credential_sha256: digest(&id, &viewer_id, &secret),
        agent_id: session.agent_id.clone(),
        session_id: session.id.clone(),
        id,
        viewer_id,
        viewer_name: name.into(),
        session_created_at_ms: session.created_at_ms,
        clear_generation: session.clear_generation,
        created_at_ms: now_ms,
        revoked_at_ms: None,
    };
    Ok((grant, secret))
}

fn digest(grant: &str, viewer: &str, secret: &str) -> String {
    Sha256::digest(format!("anima-chat-viewer-v1|{grant}|{viewer}|{secret}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SharedMessage {
    id: String,
    role: &'static str,
    text: String,
    created_at_ms: u64,
}

pub(crate) fn project_message(entry: &PageMessage) -> Option<SharedMessage> {
    let revised = entry.message.role == MessageRole::Assistant
        && matches!(
            entry
                .message
                .content
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get(anima_core::REVISED_METADATA_KEY)),
            Some(anima_core::DataValue::Bool(true))
        );
    if entry.hidden || revised {
        return None;
    }
    let role = match entry.message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System | MessageRole::Tool => return None,
    };
    Some(SharedMessage {
        id: entry.message.id.clone(),
        role,
        text: entry.message.content.text.clone(),
        created_at_ms: entry.message.created_at_ms,
    })
}

#[cfg(test)]
#[path = "conversation_shares/tests.rs"]
mod tests;
