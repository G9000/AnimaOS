//! Bounded recovery evidence for ordinary host-owned turns. Never an authority grant.
use crate::{Message, MessageRole, ToolCall};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_TURN_PROGRESS_MESSAGES: usize = 24;
pub const MAX_TURN_PROGRESS_TEXT_BYTES: usize = 1024;
pub const MAX_TURN_CHECKPOINT_SEQUENCE: u64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnBinding {
    pub root_run_id: String,
    pub attempt_run_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub original_input_digest: String,
    pub attempt_input_digest: String,
    pub workspace_digest: String,
    pub config_digest: String,
    /// Captured by the trusted host at original admission. Never set from progress.
    pub automatic_eligible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    BeforeModel,
    ModelResponded,
    ToolsSettled,
}

impl TurnPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BeforeModel => "before_model",
            Self::ModelResponded => "model_responded",
            Self::ToolsSettled => "tools_settled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRecoveryState {
    SafelyRecoverable,
    OwnerReviewRequired,
    AlreadyCompleted,
    Stopped,
    InvalidOrStale,
}

impl TurnRecoveryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafelyRecoverable => "safely_recoverable",
            Self::OwnerReviewRequired => "owner_review_required",
            Self::AlreadyCompleted => "already_completed",
            Self::Stopped => "stopped",
            Self::InvalidOrStale => "invalid_or_stale",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnProgress {
    pub role: MessageRole,
    pub text: String,
}

impl TurnProgress {
    pub fn bounded(role: MessageRole, text: &str) -> Self {
        let mut end = text.len().min(MAX_TURN_PROGRESS_TEXT_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            role,
            text: text[..end].into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnToolIntent {
    pub call_id: String,
    pub name: String,
    pub arguments_digest: String,
}

impl TurnToolIntent {
    pub fn from_call(call: &ToolCall) -> Self {
        Self {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments_digest: hex::encode(Sha256::digest(
                serde_json::to_vec(&call.args).expect("tool arguments serialize"),
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCheckpoint {
    pub binding: TurnBinding,
    pub sequence: u64,
    pub phase: TurnPhase,
    pub model_calls: u64,
    /// Untrusted excerpts only; partial streamed output is never a committed reply.
    pub progress: Vec<TurnProgress>,
    /// Intended requests only; no claim that any tool started or finished.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_tools: Vec<TurnToolIntent>,
}

#[async_trait]
pub trait TurnCheckpointSink: Send + Sync {
    /// A started durable write must retain its own physical custody if this
    /// acknowledgement future is dropped on Stop/deadline. Progress is evidence,
    /// never permission to execute or replay an effect.
    async fn save(&self, checkpoint: TurnCheckpoint) -> Result<(), String>;
}

impl TurnBinding {
    pub fn validate(&self) -> Result<(), String> {
        if [
            &self.root_run_id,
            &self.attempt_run_id,
            &self.agent_id,
            &self.session_id,
        ]
        .iter()
        .any(|id| id.trim().is_empty() || id.len() > 256)
            || [
                &self.original_input_digest,
                &self.attempt_input_digest,
                &self.workspace_digest,
                &self.config_digest,
            ]
            .iter()
            .any(|digest| {
                digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err("Invalid turn recovery binding".into());
        }
        Ok(())
    }
}

impl TurnCheckpoint {
    pub fn bounded(
        binding: TurnBinding,
        sequence: u64,
        phase: TurnPhase,
        model_calls: u64,
        messages: &[Message],
    ) -> Self {
        let progress = messages
            .iter()
            .rev()
            .take(MAX_TURN_PROGRESS_MESSAGES)
            .rev()
            .map(|message| TurnProgress::bounded(message.role, &message.content.text))
            .collect();
        Self {
            binding,
            sequence,
            phase,
            model_calls,
            progress,
            pending_tools: Vec::new(),
        }
    }

    pub fn validate(&self, binding: &TurnBinding) -> Result<(), String> {
        self.binding.validate()?;
        if &self.binding != binding
            || self.sequence == 0
            || self.sequence > MAX_TURN_CHECKPOINT_SEQUENCE
            || self.model_calls == 0
            || self.model_calls > MAX_TURN_CHECKPOINT_SEQUENCE
            || self.progress.len() > MAX_TURN_PROGRESS_MESSAGES
            || self
                .progress
                .iter()
                .any(|m| m.text.len() > MAX_TURN_PROGRESS_TEXT_BYTES)
            || self.pending_tools.len() > 64
            || (self.phase != TurnPhase::ModelResponded && !self.pending_tools.is_empty())
            || self.pending_tools.iter().any(|tool| {
                tool.call_id.is_empty()
                    || tool.call_id.len() > 256
                    || tool.name.is_empty()
                    || tool.name.len() > 256
                    || tool.arguments_digest.len() != 64
                    || !tool
                        .arguments_digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err("Invalid or oversized turn checkpoint".into());
        }
        let offset = match self.phase {
            TurnPhase::BeforeModel => 0,
            TurnPhase::ModelResponded => 1,
            TurnPhase::ToolsSettled => 2,
        };
        if self.sequence < self.model_calls * 2 - 1 + offset
            || self.sequence > self.model_calls * 3 - 2 + offset
        {
            return Err("Turn checkpoint does not match its model boundary".into());
        }
        Ok(())
    }

    pub fn validate_after(
        &self,
        binding: &TurnBinding,
        previous: Option<&Self>,
    ) -> Result<(), String> {
        self.validate(binding)?;
        if let Some(old) = previous {
            old.validate(binding)?;
        }
        let valid = match previous {
            None => {
                self.sequence == 1 && self.phase == TurnPhase::BeforeModel && self.model_calls == 1
            }
            Some(old) => {
                old.sequence.checked_add(1) == Some(self.sequence)
                    && match (old.phase, self.phase) {
                        (TurnPhase::BeforeModel, TurnPhase::ModelResponded) => {
                            self.model_calls == old.model_calls
                        }
                        (TurnPhase::ModelResponded, TurnPhase::ToolsSettled) => {
                            self.model_calls == old.model_calls
                        }
                        (
                            TurnPhase::ModelResponded | TurnPhase::ToolsSettled,
                            TurnPhase::BeforeModel,
                        ) => self.model_calls == old.model_calls + 1,
                        _ => false,
                    }
            }
        };
        if !valid {
            return Err("Turn checkpoint sequence or boundary regressed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> TurnBinding {
        TurnBinding {
            root_run_id: "root".into(),
            attempt_run_id: "attempt".into(),
            agent_id: "agent".into(),
            session_id: "chat".into(),
            original_input_digest: "a".repeat(64),
            attempt_input_digest: "b".repeat(64),
            workspace_digest: "c".repeat(64),
            config_digest: "d".repeat(64),
            automatic_eligible: true,
        }
    }
    #[test]
    fn checkpoints_reject_rebinding_skipped_boundaries_and_oversize_data() {
        let b = binding();
        let before = TurnCheckpoint::bounded(b.clone(), 1, TurnPhase::BeforeModel, 1, &[]);
        assert!(before.validate_after(&b, None).is_ok());
        let response = TurnCheckpoint::bounded(b.clone(), 2, TurnPhase::ModelResponded, 1, &[]);
        assert!(response.validate_after(&b, Some(&before)).is_ok());
        assert!(response.validate_after(&b, None).is_err());
        let mut stale = response.clone();
        stale.binding.config_digest = "f".repeat(64);
        assert!(stale.validate_after(&b, Some(&before)).is_err());
        stale = response.clone();
        stale.sequence = 1;
        assert!(stale.validate_after(&b, Some(&before)).is_err());
        stale = response;
        stale.progress = vec![TurnProgress {
            role: MessageRole::Assistant,
            text: "x".repeat(MAX_TURN_PROGRESS_TEXT_BYTES + 1),
        }];
        assert!(stale.validate_after(&b, Some(&before)).is_err());
    }
}
