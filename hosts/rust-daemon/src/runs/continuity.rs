//! Read-only, bounded continuity evidence; this never grants execution rights.
use anima_core::MessageRole;
use serde::Serialize;
use utoipa::ToSchema;

use super::{ActionReceipt, RunRecord};

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunContinuityEvidence {
    action_receipts: Vec<ActionReceipt>,
    unreceipted_tools: Vec<String>,
    omitted_unreceipted_tool_count: usize,
    checkpoint: Option<CheckpointEvidence>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CheckpointEvidence {
    sequence: u64,
    phase: String,
    model_calls: u64,
    excerpts: Vec<ProgressEvidence>,
    intended_tools: Vec<ToolIntentEvidence>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ProgressEvidence {
    role: String,
    text: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ToolIntentEvidence {
    call_id: String,
    name: String,
}

pub(crate) fn project(run: &RunRecord) -> RunContinuityEvidence {
    let missing: Vec<_> = run
        .tools_started
        .iter()
        .filter(|name| {
            !run.action_receipts
                .iter()
                .any(|receipt| &receipt.name == *name)
        })
        .cloned()
        .collect();
    let omitted_unreceipted_tool_count = missing.len().saturating_sub(50);
    let checkpoint = run
        .turn
        .as_ref()
        .and_then(|turn| turn.checkpoint.as_ref())
        .map(|saved| CheckpointEvidence {
            sequence: saved.sequence,
            phase: saved.phase.as_str().into(),
            model_calls: saved.model_calls,
            excerpts: saved
                .progress
                .iter()
                .filter_map(|progress| {
                    let role = match progress.role {
                        MessageRole::Assistant => "assistant",
                        MessageRole::Tool => "tool",
                        _ => return None,
                    };
                    Some(ProgressEvidence {
                        role: role.into(),
                        text: progress.text.clone(),
                    })
                })
                .collect(),
            intended_tools: saved
                .pending_tools
                .iter()
                .map(|intent| ToolIntentEvidence {
                    call_id: intent.call_id.clone(),
                    name: intent.name.clone(),
                })
                .collect(),
        });
    RunContinuityEvidence {
        action_receipts: run.action_receipts.clone(),
        unreceipted_tools: missing.into_iter().take(50).collect(),
        omitted_unreceipted_tool_count,
        checkpoint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::{ActionDurability, ActionStatus, RunSource, RunStart};
    use anima_core::turn_recovery::{
        TurnBinding, TurnCheckpoint, TurnPhase, TurnProgress, TurnToolIntent,
    };

    #[test]
    fn continuity_projection_preserves_unknown_outcomes_and_legacy_names() {
        let mut run = RunRecord::running(
            RunStart {
                agent_id: "agent".into(),
                session_id: "chat".into(),
                source: RunSource::Web,
                source_ref: None,
                idempotency_key: None,
                text: "Work".into(),
                model: "test".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        run.tools_started = vec!["send".into(), "legacy".into()];
        run.action_receipts.push(ActionReceipt {
            tool_call_id: "call".into(),
            name: "send".into(),
            status: ActionStatus::Started,
            started_at_ms: 1,
            finished_at_ms: None,
            duration_ms: None,
            arguments_preview: "preview".into(),
            arguments_truncated: false,
            result_preview: None,
            result_truncated: false,
            durability: ActionDurability::Memory,
        });
        let evidence = serde_json::to_value(project(&run)).unwrap();
        assert_eq!(evidence["actionReceipts"][0]["status"], "started");
        assert_eq!(evidence["actionReceipts"][0]["durability"], "memory");
        assert_eq!(evidence["unreceiptedTools"], serde_json::json!(["legacy"]));
        assert!(evidence["checkpoint"].is_null());
        assert_eq!(run.action_receipts[0].status, ActionStatus::Started);
    }

    #[test]
    fn continuity_projection_filters_private_binding_and_untrusted_roles() {
        let mut run = RunRecord::running(
            RunStart {
                agent_id: "agent".into(),
                session_id: "chat".into(),
                source: RunSource::Web,
                source_ref: None,
                idempotency_key: None,
                text: "Owner input".into(),
                model: "test".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        let input_digest = crate::runs::turn::digest(&run.input);
        let binding = TurnBinding {
            root_run_id: run.id.clone(),
            attempt_run_id: run.id.clone(),
            agent_id: "agent".into(),
            session_id: "chat".into(),
            original_input_digest: input_digest.clone(),
            attempt_input_digest: input_digest,
            workspace_digest: "c".repeat(64),
            config_digest: "d".repeat(64),
            automatic_eligible: false,
        };
        let checkpoint = TurnCheckpoint {
            binding: binding.clone(),
            sequence: 2,
            phase: TurnPhase::ModelResponded,
            model_calls: 1,
            progress: vec![
                TurnProgress::bounded(MessageRole::System, "private system text"),
                TurnProgress::bounded(MessageRole::User, "owner input"),
                TurnProgress::bounded(MessageRole::Assistant, "saved excerpt"),
                TurnProgress::bounded(MessageRole::Tool, "tool excerpt"),
            ],
            pending_tools: vec![TurnToolIntent {
                call_id: "call".into(),
                name: "send".into(),
                arguments_digest: "e".repeat(64),
            }],
        };
        run.turn = Some(crate::runs::turn::RunTurn {
            binding,
            original_input: run.input.clone(),
            checkpoint: Some(checkpoint),
            terminal: None,
        });
        crate::runs::turn::validate(&run).expect("fixture checkpoint is structurally valid");
        let evidence = serde_json::to_value(project(&run)).unwrap();
        assert_eq!(
            evidence["checkpoint"]["excerpts"].as_array().unwrap().len(),
            2
        );
        assert_eq!(evidence["checkpoint"]["excerpts"][0]["role"], "assistant");
        assert_eq!(evidence["checkpoint"]["intendedTools"][0]["name"], "send");
        let json = evidence.to_string();
        assert!(!json.contains("private system text"));
        assert!(!json.contains("owner input"));
        assert!(!json.contains("argumentsDigest"));
        assert!(!json.contains("workspaceDigest"));
    }
}
