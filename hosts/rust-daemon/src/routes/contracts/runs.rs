//! Run bodies (spec §4.1–§4.2, §4.6, §6).

use serde::Serialize;
use utoipa::ToSchema;

use super::shared::TokenUsageResponse;
use crate::runs::{HelperCompletionState, RunRecord};

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunInputResponse {
    pub(crate) text: String,
    pub(crate) attachment_ids: Vec<String>,
    pub(crate) skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) document_edit_context:
        Option<crate::document_edits::preview_context::PreviewEditContext>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunErrorResponse {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunStopResponse {
    pub(crate) requested_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunStepResponse {
    pub(crate) step_id: String,
    pub(crate) usage: TokenUsageResponse,
    /// The provider and model that answered, when a fallback did instead of
    /// the run's own; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    /// Why the run's own model did not answer, e.g. "was rate limited".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fallback_reason: Option<String>,
}

/// Response-only disposition; native launch keys and steer contents stay private.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum HelperCompletionDispositionResponse {
    Manual,
    AwaitingResult {
        #[serde(rename = "reservedRunId")]
        reserved_run_id: String,
    },
    AwaitingParent {
        #[serde(rename = "reservedRunId")]
        reserved_run_id: String,
    },
    Queued {
        #[serde(rename = "runId")]
        run_id: String,
    },
    Settled {
        #[serde(rename = "runId")]
        run_id: String,
    },
    ReviewRequired {
        reason: String,
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
    Suppressed {
        reason: String,
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
}

impl From<&HelperCompletionState> for HelperCompletionDispositionResponse {
    fn from(state: &HelperCompletionState) -> Self {
        match state {
            HelperCompletionState::Manual => Self::Manual,
            HelperCompletionState::AwaitingResult { reserved_run_id } => Self::AwaitingResult {
                reserved_run_id: reserved_run_id.clone(),
            },
            HelperCompletionState::AwaitingParent { reserved_run_id } => Self::AwaitingParent {
                reserved_run_id: reserved_run_id.clone(),
            },
            HelperCompletionState::Queued { run_id } => Self::Queued {
                run_id: run_id.clone(),
            },
            HelperCompletionState::Settled { run_id } => Self::Settled {
                run_id: run_id.clone(),
            },
            HelperCompletionState::ReviewRequired { reason, run_id } => Self::ReviewRequired {
                reason: reason.clone(),
                run_id: run_id.clone(),
            },
            HelperCompletionState::Suppressed { reason, run_id } => Self::Suppressed {
                reason: reason.clone(),
                run_id: run_id.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AsyncHelperSummaryResponse {
    pub(crate) owner_agent_id: String,
    pub(crate) owner_session_id: String,
    pub(crate) parent_run_id: String,
    pub(crate) completion: HelperCompletionDispositionResponse,
    pub(crate) pending_steer_count: usize,
    pub(crate) archive_unconsumed_count: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HelperCompletionOriginResponse {
    pub(crate) helper_run_id: String,
    pub(crate) launch_parent_run_id: String,
}

/// A ledger run (spec §4.1) as clients see it.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnSummaryResponse {
    pub(crate) root_run_id: String,
    pub(crate) original_input_digest: String,
    pub(crate) attempt_input_digest: String,
    pub(crate) checkpoint_sequence: Option<u64>,
    pub(crate) checkpoint_phase: Option<String>,
    pub(crate) terminal_reply_message_id: Option<String>,
    pub(crate) terminal_reply_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<serde_json::Value>)]
    pub(crate) outcome: Option<anima_core::outcomes::RunOutcome>,
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    /// `web`, `api`, `telegram`, `schedule`, `job`, `delegation`, or `peer`.
    pub(crate) source: String,
    pub(crate) source_ref: Option<String>,
    /// The `Idempotency-Key` the run was accepted or started with, so a
    /// client matches its own sends (owner-only, as every runs route and the
    /// event stream are); `null` for a run without one.
    pub(crate) idempotency_key: Option<String>,
    /// `queued`, `running`, `awaiting_approval`, `completed`, `failed`,
    /// `cancelled`, or `interrupted`.
    pub(crate) status: String,
    pub(crate) input: RunInputResponse,
    pub(crate) created_at_ms: u64,
    /// Durable waiting order, with acceptance time as the legacy fallback.
    pub(crate) queue_order: u64,
    pub(crate) started_at_ms: Option<u64>,
    pub(crate) finished_at_ms: Option<u64>,
    pub(crate) error: Option<RunErrorResponse>,
    pub(crate) stop: Option<RunStopResponse>,
    pub(crate) tools_started: Vec<String>,
    pub(crate) action_receipts: Vec<crate::runs::ActionReceipt>,
    pub(crate) steps: Vec<RunStepResponse>,
    pub(crate) usage: TokenUsageResponse,
    pub(crate) model: String,
    pub(crate) provider: Option<String>,
    /// Requested effort at admission; fallback steps may use another model.
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recovery: Option<crate::runs::recovery::RecoveryRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recovery_of: Option<String>,
    pub(crate) recovery_attempt: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) turn: Option<TurnSummaryResponse>,
    /// The committed final reply, once the run completed.
    pub(crate) reply_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) async_helper: Option<AsyncHelperSummaryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) helper_completion: Option<HelperCompletionOriginResponse>,
}

impl From<&RunRecord> for RunResponse {
    fn from(record: &RunRecord) -> Self {
        Self {
            outcome: record.outcome.clone(),
            id: record.id.clone(),
            agent_id: record.agent_id.clone(),
            session_id: record.session_id.clone(),
            source: record.source.as_str().into(),
            source_ref: record.source_ref.clone(),
            idempotency_key: record.idempotency_key.clone(),
            status: record.status.as_str().into(),
            input: RunInputResponse {
                text: record.effective_input().text.clone(),
                attachment_ids: record.effective_input().attachment_ids.clone(),
                skill: record.effective_input().skill.clone(),
                document_edit_context: record.effective_input().document_edit_context.clone(),
            },
            created_at_ms: record.created_at_ms,
            queue_order: record.queue_order(),
            started_at_ms: record.started_at_ms,
            finished_at_ms: record.finished_at_ms,
            error: record.error.as_ref().map(|error| RunErrorResponse {
                code: error.code.clone(),
                message: error.message.clone(),
            }),
            stop: record.stop.as_ref().map(|stop| RunStopResponse {
                requested_at_ms: stop.requested_at_ms,
            }),
            tools_started: record.tools_started.clone(),
            action_receipts: record.action_receipts.clone(),
            steps: record
                .steps
                .iter()
                .map(|step| RunStepResponse {
                    step_id: step.step_id.clone(),
                    usage: TokenUsageResponse::from(&step.usage),
                    provider: step.provider.clone(),
                    model: step.model.clone(),
                    fallback_reason: step.fallback_reason.clone(),
                })
                .collect(),
            usage: TokenUsageResponse::from(&record.usage),
            model: record.model.clone(),
            provider: record.provider.clone(),
            reasoning_effort: record.reasoning_effort.clone(),
            parent_run_id: record.parent_run_id.clone(),
            recovery: record.recovery.clone(),
            recovery_of: record.recovery_of.clone(),
            recovery_attempt: record.recovery_attempt,
            turn: record.turn.as_ref().map(|turn| TurnSummaryResponse {
                root_run_id: turn.binding.root_run_id.clone(),
                original_input_digest: turn.binding.original_input_digest.clone(),
                attempt_input_digest: turn.binding.attempt_input_digest.clone(),
                checkpoint_sequence: turn.checkpoint.as_ref().map(|c| c.sequence),
                checkpoint_phase: turn
                    .checkpoint
                    .as_ref()
                    .map(|c| c.phase.as_str().to_string()),
                terminal_reply_message_id: turn
                    .terminal
                    .as_ref()
                    .and_then(|t| t.reply_message_id.clone()),
                terminal_reply_sha256: turn.terminal.as_ref().and_then(|t| t.reply_sha256.clone()),
            }),
            reply_message_id: record.reply_message_id.clone(),
            async_helper: record
                .async_helper
                .as_ref()
                .map(|helper| AsyncHelperSummaryResponse {
                    owner_agent_id: helper.owner_agent_id.clone(),
                    owner_session_id: helper.owner_session_id.clone(),
                    parent_run_id: helper.parent_run_id.clone(),
                    completion: HelperCompletionDispositionResponse::from(&helper.completion),
                    pending_steer_count: record.pending_steers.len(),
                    archive_unconsumed_count: helper.unconsumed_steers.len(),
                }),
            helper_completion: record.helper_completion.as_ref().map(|origin| {
                HelperCompletionOriginResponse {
                    helper_run_id: origin.helper_run_id.clone(),
                    launch_parent_run_id: origin.launch_parent_run_id.clone(),
                }
            }),
        }
    }
}

/// A steer that joined the session's active run (spec §4.2).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SteerStatusResponse {
    /// `pending` until the run's next model call drains it.
    pub(crate) status: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunEnvelope {
    pub(crate) run: RunResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) steer: Option<SteerStatusResponse>,
}

impl RunEnvelope {
    pub(crate) fn of(record: &RunRecord) -> Self {
        Self {
            run: RunResponse::from(record),
            steer: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunsEnvelope {
    pub(crate) runs: Vec<RunResponse>,
}

#[cfg(test)]
mod async_helper_response_tests {
    use super::*;
    use serde_json::json;

    fn record_with(fields: serde_json::Value) -> RunRecord {
        let record = crate::history::conformance::terminal_run("helper", "helper:1");
        let mut value = serde_json::to_value(record).unwrap();
        for (key, value_field) in fields.as_object().unwrap() {
            value[key] = value_field.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn fp05_public_run_omits_absent_async_origins() {
        let record = record_with(json!({}));
        let value = serde_json::to_value(RunResponse::from(&record)).unwrap();
        assert!(value.get("asyncHelper").is_none());
        assert!(value.get("helperCompletion").is_none());
    }

    #[test]
    fn fp05_public_run_exposes_bounded_summary_without_private_steers_or_launch_keys() {
        let states = vec![
            json!({"state":"manual"}),
            json!({"state":"awaiting_result","reservedRunId":"completion"}),
            json!({"state":"awaiting_parent","reservedRunId":"completion"}),
            json!({"state":"queued","runId":"completion"}),
            json!({"state":"settled","runId":"completion"}),
            json!({"state":"review_required","reason":"restart","runId":"completion"}),
            json!({"state":"suppressed","reason":"stopped"}),
        ];
        for completion in states {
            let steer = json!({"idempotencyKey":"secret-steer-key", "text":"private steer text", "attachmentIds":[], "skill":null, "acceptedAtMs":1});
            let record = record_with(json!({
                "asyncHelper": {
                    "ownerAgentId":"owner", "ownerSessionId":"chat:owner", "parentRunId":"parent",
                    "launchKey":"private-launch-key", "launchFingerprint":"a".repeat(64),
                    "completion":completion, "unconsumedSteers":[steer.clone()]
                },
                "pendingSteers":[steer]
            }));
            let value = serde_json::to_value(RunResponse::from(&record)).unwrap();
            assert_eq!(
                value["asyncHelper"],
                json!({
                    "ownerAgentId":"owner", "ownerSessionId":"chat:owner", "parentRunId":"parent",
                    "completion":completion, "pendingSteerCount":1, "archiveUnconsumedCount":1
                })
            );
            let wire = value.to_string();
            for private in [
                "launchKey",
                "launchFingerprint",
                "private steer text",
                "secret-steer-key",
                "unconsumedSteers",
            ] {
                assert!(!wire.contains(private), "leaked {private}");
            }
        }
    }

    #[test]
    fn fp05_public_run_exposes_typed_completion_lineage() {
        let record = record_with(
            json!({"helperCompletion":{"helperRunId":"child", "launchParentRunId":"parent"}}),
        );
        let value = serde_json::to_value(RunResponse::from(&record)).unwrap();
        assert_eq!(
            value["helperCompletion"],
            json!({"helperRunId":"child", "launchParentRunId":"parent"})
        );
        assert!(value.get("asyncHelper").is_none());
    }
}
