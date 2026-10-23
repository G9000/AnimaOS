//! Positive, host-owned admission of settled historical output for request projection.
use crate::{runs::RunRecord, state::DaemonState};
use anima_core::tool_output_pruning::ToolOutputPruning;
use anima_core::tool_output_pruning::{content_digest, ToolOutputIdentity};
use anima_core::{AgentConfig, DataValue, Message, MessageRole};
use std::collections::BTreeMap;
fn run_eligible(run: &RunRecord, pending: bool) -> bool {
    run.status == crate::runs::RunStatus::Completed
        && run.finished_at_ms.is_some()
        && run.error.is_none()
        && run.stop.is_none()
        && !pending
        && run.pending_steers.is_empty()
        && run
            .action_receipts
            .iter()
            .all(|r| r.status == crate::runs::ActionStatus::Succeeded && r.finished_at_ms.is_some())
}
fn text<'a>(map: &'a BTreeMap<String, DataValue>, key: &str) -> Option<&'a str> {
    match map.get(key)? {
        DataValue::String(s) => Some(s),
        _ => None,
    }
}
fn enabled(config: &AgentConfig) -> bool {
    config
        .settings
        .as_ref()
        .and_then(|s| s.additional.get("toolOutputPruning"))
        == Some(&DataValue::Bool(true))
}
pub(super) fn policy(
    state: &DaemonState,
    agent: &str,
    room: &str,
    config: &AgentConfig,
    history: &[Message],
) -> ToolOutputPruning {
    let mut result = ToolOutputPruning::default();
    if !enabled(config) || history.len() > 4096 {
        return result;
    }
    let Some(workspace) = state.workspace.as_ref().map(|w| &w.root_path) else {
        return result;
    };
    let Ok(workspace_key) = crate::tool_results::workspace_key(workspace) else {
        return result;
    };
    let store = state.tool_result_store();
    let mut artifact_reads = 0;
    let mut admitted_bytes = 0;
    for message in history.iter().take(history.len().saturating_sub(8)) {
        if result.eligible.len() >= 256 {
            break;
        }
        if message.role != MessageRole::Tool || message.agent_id != agent || message.room_id != room
        {
            continue;
        }
        let Some(meta) = message.content.metadata.as_ref() else {
            continue;
        };
        let (Some(run_id), Some(call_id)) = (text(meta, "runId"), text(meta, "toolCallId")) else {
            continue;
        };
        let Some(run) = state.runs.get(run_id).filter(|r| {
            r.agent_id == agent
                && r.session_id == room
                && run_eligible(r, !state.approvals.pending_ids_for_run(run_id).is_empty())
        }) else {
            continue;
        };
        let name = history
            .iter()
            .filter(|m| {
                m.role == MessageRole::Assistant && m.agent_id == agent && m.room_id == room
            })
            .filter_map(|m| m.content.metadata.as_ref())
            .filter(|m| text(m, "runId") == Some(run.id.as_str()))
            .filter_map(|m| match m.get("toolCalls") {
                Some(DataValue::Array(calls)) => Some(calls),
                _ => None,
            })
            .flatten()
            .filter_map(|c| match c {
                DataValue::Object(call) if text(call, "id") == Some(call_id) => text(call, "name"),
                _ => None,
            })
            .next();
        let Some(name) = name else { continue };
        let scope_valid = match meta.get("toolOutputScope") {
            Some(DataValue::Object(scope)) => {
                scope.get("version") == Some(&DataValue::Number(1.0))
                    && scope.get("captureComplete") == Some(&DataValue::Bool(true))
                    && text(scope, "workspace") == Some(workspace_key.as_str())
                    && text(scope, "agentId") == Some(agent)
                    && text(scope, "runId") == Some(run_id)
                    && text(scope, "toolCallId") == Some(call_id)
            }
            _ => false,
        };
        let artifact = if let Some(DataValue::Object(reference)) = meta.get("toolResultArtifact") {
            if artifact_reads >= 8 {
                continue;
            }
            artifact_reads += 1;
            let (Some(store), Some(id)) = (&store, text(reference, "id")) else {
                continue;
            };
            let Ok(a) = store.describe_verified(agent, workspace, id) else {
                continue;
            };
            if !a.capture_complete
                || a.run_id != run_id
                || a.tool_call_id != call_id
                || a.tool_name != name
                || text(reference, "sha256") != Some(a.sha256.as_str())
                || reference.get("bytes") != Some(&DataValue::Number(a.bytes as f64))
                || reference.get("captureComplete") != Some(&DataValue::Bool(true))
            {
                continue;
            }
            Some(a.id)
        } else {
            if !scope_valid {
                continue;
            }
            None
        };
        let Ok(bytes) = serde_json::to_vec(&message.content) else {
            continue;
        };
        if bytes.len() < 2048 || bytes.len() > 4 * 1024 * 1024 {
            continue;
        }
        admitted_bytes += bytes.len();
        if admitted_bytes > 4 * 1024 * 1024 {
            break;
        }
        result.eligible.insert(
            message.id.clone(),
            ToolOutputIdentity {
                run_id: run_id.into(),
                tool_call_id: call_id.into(),
                tool_name: name.into(),
                content_sha256: content_digest(&message.content),
                verified_artifact_id: artifact,
            },
        );
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::{
        ActionDurability, ActionReceipt, ActionStatus, RunSource, RunStart, RunStatus,
    };
    fn run() -> RunRecord {
        let mut r = RunRecord::running(
            RunStart {
                agent_id: "a".into(),
                session_id: "r".into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "old".into(),
                model: "m".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        r.finish(RunStatus::Completed, None, 2);
        r
    }
    #[test]
    fn quality_tool_output_pruning_admits_only_completed_runs_without_unresolved_authority() {
        let r = run();
        assert!(run_eligible(&r, false));
        assert!(!run_eligible(&r, true));
        for status in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::AwaitingApproval,
            RunStatus::Failed,
            RunStatus::Cancelled,
            RunStatus::Interrupted,
        ] {
            let mut r = run();
            r.status = status;
            assert!(!run_eligible(&r, false));
        }
        let mut r = run();
        r.action_receipts.push(ActionReceipt {
            tool_call_id: "call".into(),
            name: "bash".into(),
            status: ActionStatus::Started,
            started_at_ms: 1,
            finished_at_ms: None,
            duration_ms: None,
            arguments_preview: "".into(),
            arguments_truncated: false,
            result_preview: None,
            result_truncated: false,
            durability: ActionDurability::Memory,
        });
        assert!(!run_eligible(&r, false));
    }

    #[test]
    fn quality_tool_output_pruning_requires_native_current_workspace_scope_and_owner_opt_in() {
        let temp = tempfile::tempdir().unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let mut state = DaemonState::new();
        state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: temp.path().into(),
            company_name: "test".into(),
            mission: "test".into(),
            values: vec![],
        });
        let record = run();
        let run_id = record.id.clone();
        state.runs.insert(record);
        let call = anima_core::ToolCall {
            id: "call".into(),
            name: "read_file".into(),
            args: Default::default(),
        };
        let link = crate::runs::RunLink {
            run_id: run_id.clone(),
            session_id: "r".into(),
            agent_id: "a".into(),
        };
        let returned = crate::tool_results::project_result(
            None,
            "a",
            Some(temp.path()),
            Some(&link),
            &call,
            anima_core::TaskResult::success(
                anima_core::Content {
                    text: "bytes".repeat(1000),
                    metadata: Some(BTreeMap::from([(
                        "toolOutputScope".into(),
                        DataValue::String("forged".into()),
                    )])),
                    ..Default::default()
                },
                0,
            ),
        );
        let mut output = returned.data.unwrap();
        let text_copy = output.text.clone();
        let meta = output.metadata.as_mut().unwrap();
        assert!(matches!(
            meta.get("toolOutputScope"),
            Some(DataValue::Object(_))
        ));
        meta.extend(BTreeMap::from([
            ("runId".into(), DataValue::String(run_id.clone())),
            ("toolCallId".into(), DataValue::String("call".into())),
            ("toolStatus".into(), DataValue::String("success".into())),
            (
                "taskResult".into(),
                DataValue::Object(BTreeMap::from([
                    ("status".into(), DataValue::String("success".into())),
                    (
                        "data".into(),
                        DataValue::Object(BTreeMap::from([(
                            "text".into(),
                            DataValue::String(text_copy),
                        )])),
                    ),
                ])),
            ),
        ]));
        let mut history = vec![
            Message {
                id: "assistant".into(),
                agent_id: "a".into(),
                room_id: "r".into(),
                created_at_ms: 1,
                role: MessageRole::Assistant,
                content: anima_core::Content {
                    metadata: Some(BTreeMap::from([
                        ("runId".into(), DataValue::String(run_id)),
                        (
                            "toolCalls".into(),
                            DataValue::Array(vec![DataValue::Object(BTreeMap::from([
                                ("id".into(), DataValue::String("call".into())),
                                ("name".into(), DataValue::String("read_file".into())),
                            ]))]),
                        ),
                    ])),
                    ..Default::default()
                },
            },
            Message {
                id: "output".into(),
                agent_id: "a".into(),
                room_id: "r".into(),
                created_at_ms: 2,
                role: MessageRole::Tool,
                content: output,
            },
        ];
        for i in 0..8 {
            history.push(Message {
                id: format!("recent{i}"),
                agent_id: "a".into(),
                room_id: "r".into(),
                created_at_ms: 3 + i,
                role: MessageRole::User,
                content: Default::default(),
            });
        }
        let mut config: AgentConfig =
            serde_json::from_value(serde_json::json!({"name":"test","model":"local"})).unwrap();
        assert!(policy(&state, "a", "r", &config, &history)
            .eligible
            .is_empty());
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("toolOutputPruning".into(), DataValue::Bool(true));
        let pruning = policy(&state, "a", "r", &config, &history);
        assert_eq!(pruning.eligible.len(), 1);
        assert!(pruning.project(&history)[1]
            .content
            .text
            .contains("old-tool-output"));
        let mut incomplete = history.clone();
        incomplete[1].content.text.push_str("\n[output truncated]");
        incomplete[1].content = crate::tool_results::project_result(
            None,
            "a",
            Some(temp.path()),
            Some(&link),
            &call,
            anima_core::TaskResult::success(incomplete[1].content.clone(), 0),
        )
        .data
        .unwrap();
        assert!(
            policy(&state, "a", "r", &config, &incomplete)
                .eligible
                .is_empty(),
            "native incomplete inline output must remain visible"
        );
        state.workspace.as_mut().unwrap().root_path = foreign.path().into();
        assert!(policy(&state, "a", "r", &config, &history)
            .eligible
            .is_empty());
        state.workspace.as_mut().unwrap().root_path = temp.path().into();
        history[1]
            .content
            .metadata
            .as_mut()
            .unwrap()
            .remove("toolOutputScope");
        assert!(policy(&state, "a", "r", &config, &history)
            .eligible
            .is_empty());
    }
}
