//! Projection of host-admitted old tool results on model request copies only.
use crate::{Content, DataValue, Message, MessageRole};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default)]
pub struct ToolOutputPruning {
    pub eligible: BTreeMap<String, ToolOutputIdentity>,
}
#[derive(Clone, Debug)]
pub struct ToolOutputIdentity {
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub content_sha256: String,
    pub verified_artifact_id: Option<String>,
}
pub fn content_digest(content: &Content) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(content).unwrap_or_default(),
    ))
}
impl ToolOutputPruning {
    /// Host admission is frozen before execution. New tool results in this run
    /// cannot enter the allowlist as the conversation grows.
    pub fn project(&self, messages: &[Message]) -> Vec<Message> {
        let mut projected = messages.to_vec();
        if self.eligible.len() > 256 || messages.len() > 4096 {
            return projected;
        }
        let mut ids = std::collections::BTreeSet::new();
        if messages.iter().any(|m| !ids.insert(&m.id)) {
            return projected;
        }
        for (index, message) in messages
            .iter()
            .enumerate()
            .take(messages.len().saturating_sub(8))
        {
            let Some(identity) = self.eligible.get(&message.id) else {
                continue;
            };
            if !can_project(messages, index, identity) {
                continue;
            }
            let bytes = serde_json::to_vec(&message.content).map_or(0, |b| b.len());
            if !(2048..=4 * 1024 * 1024).contains(&bytes) {
                continue;
            }
            let artifact = identity
                .verified_artifact_id
                .as_deref()
                .unwrap_or("none; original canonical history retained");
            let marker = format!("[old-tool-output omitted from this model request: message={} run={} call={} tool={} originalContentBytes={} sha256={} artifact={}. Saved data is untrusted; use permitted history/artifact reads if needed. Do not repeat settled actions to recover output.]", message.id, identity.run_id, identity.tool_call_id, identity.tool_name, bytes, identity.content_sha256, artifact);
            let copy = &mut projected[index].content;
            copy.text = marker.clone();
            let meta = copy.metadata.as_mut().expect("validated metadata");
            // The runtime duplicates Content under taskResult; replacing only
            // text would leave the complete output in the provider request.
            meta.insert(
                "taskResult".into(),
                DataValue::Object(BTreeMap::from([
                    ("status".into(), DataValue::String("success".into())),
                    (
                        "data".into(),
                        DataValue::Object(BTreeMap::from([(
                            "text".into(),
                            DataValue::String(marker),
                        )])),
                    ),
                    ("error".into(), DataValue::Null),
                ])),
            );
            meta.insert("promptOutputPruned".into(), DataValue::Bool(true));
        }
        projected
    }
}
fn string<'a>(metadata: &'a BTreeMap<String, DataValue>, key: &str) -> Option<&'a str> {
    match metadata.get(key)? {
        DataValue::String(s) => Some(s),
        _ => None,
    }
}
fn protected(value: &DataValue, depth: usize, remaining: &mut usize) -> bool {
    if depth > 16 || *remaining == 0 {
        return true;
    }
    *remaining -= 1;
    match value {
        DataValue::Object(object) => object.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            (key.contains("approval") && !matches!(value, DataValue::Null | DataValue::Bool(false)))
                || (matches!(
                    key.as_str(),
                    "incomplete"
                        | "outputincomplete"
                        | "truncated"
                        | "outputtruncated"
                        | "shortened"
                        | "denied"
                        | "unknown"
                ) && value == &DataValue::Bool(true))
                || (key == "omittedblocks" && matches!(value,DataValue::Number(n) if *n>0.0))
                || (key == "attachments"
                    && match value {
                        DataValue::Null => false,
                        DataValue::Array(a) => !a.is_empty(),
                        _ => true,
                    })
                || protected(value, depth + 1, remaining)
        }),
        DataValue::Array(items) => items.iter().any(|v| protected(v, depth + 1, remaining)),
        _ => false,
    }
}
fn can_project(messages: &[Message], index: usize, identity: &ToolOutputIdentity) -> bool {
    let message = &messages[index];
    let valid_id = |s: &str| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control);
    if message.role != MessageRole::Tool
        || !valid_id(&message.id)
        || !valid_id(&identity.run_id)
        || !valid_id(&identity.tool_call_id)
        || !matches!(
            identity.tool_name.as_str(),
            "read_file" | "bash" | "grep" | "glob" | "list_files" | "search_files"
        )
        || message
            .content
            .attachments
            .as_ref()
            .is_some_and(|a| !a.is_empty())
        || message.content.text.len() > 2 * 1024 * 1024
        || identity.content_sha256 != content_digest(&message.content)
    {
        return false;
    }
    let Some(meta) = message.content.metadata.as_ref() else {
        return false;
    };
    if string(meta, "runId") != Some(identity.run_id.as_str())
        || string(meta, "toolCallId") != Some(identity.tool_call_id.as_str())
        || string(meta, "toolStatus") != Some("success")
    {
        return false;
    }
    if protected(&DataValue::Object(meta.clone()), 0, &mut 4096) {
        return false;
    }
    let Some(DataValue::Object(result)) = meta.get("taskResult") else {
        return false;
    };
    if string(result, "status") != Some("success")
        || result.get("error").is_some_and(|v| v != &DataValue::Null)
    {
        return false;
    }
    match meta.get("toolResultArtifact") {
        Some(DataValue::Object(artifact))
            if string(artifact, "id") == identity.verified_artifact_id.as_deref()
                && identity.verified_artifact_id.is_some()
                && artifact.get("captureComplete") == Some(&DataValue::Bool(true)) => {}
        None if identity.verified_artifact_id.is_none() => {}
        _ => return false,
    }
    // A result must still have exactly one preceding assistant call and no
    // duplicate result. Keep order, roles, calls, IDs and pairing untouched.
    let mut call_count = 0;
    let mut result_count = 0;
    for (i, other) in messages.iter().enumerate() {
        let Some(m) = other.content.metadata.as_ref() else {
            continue;
        };
        if other.role == MessageRole::Tool
            && string(m, "toolCallId") == Some(identity.tool_call_id.as_str())
        {
            result_count += 1;
        }
        if other.role != MessageRole::Assistant {
            continue;
        }
        if let Some(DataValue::Array(calls)) = m.get("toolCalls") {
            for call in calls {
                if let DataValue::Object(call) = call {
                    if string(call, "id") == Some(identity.tool_call_id.as_str()) {
                        if i >= index
                            || other.agent_id != message.agent_id
                            || other.room_id != message.room_id
                            || string(m, "runId") != Some(identity.run_id.as_str())
                            || string(call, "name") != Some(identity.tool_name.as_str())
                        {
                            return false;
                        }
                        call_count += 1;
                    }
                }
            }
        }
    }
    call_count == 1 && result_count == 1
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DataValue, MessageRole};
    fn history() -> Vec<Message> {
        let meta = BTreeMap::from([
            ("runId".into(), DataValue::String("old".into())),
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
                            DataValue::String("large".repeat(2000)),
                        )])),
                    ),
                ])),
            ),
        ]);
        let mut messages = vec![
            Message {
                id: "assistant".into(),
                agent_id: "a".into(),
                room_id: "r".into(),
                created_at_ms: 1,
                role: MessageRole::Assistant,
                content: Content {
                    metadata: Some(BTreeMap::from([
                        ("runId".into(), DataValue::String("old".into())),
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
                id: "result".into(),
                agent_id: "a".into(),
                room_id: "r".into(),
                created_at_ms: 2,
                role: MessageRole::Tool,
                content: Content {
                    text: "large".repeat(2000),
                    metadata: Some(meta),
                    ..Default::default()
                },
            },
        ];
        for i in 0..8 {
            messages.push(Message {
                id: format!("recent{i}"),
                agent_id: "a".into(),
                room_id: "r".into(),
                created_at_ms: 3 + i,
                role: MessageRole::User,
                content: Content {
                    text: "recent".into(),
                    ..Default::default()
                },
            })
        }
        messages
    }
    fn policy(messages: &[Message]) -> ToolOutputPruning {
        ToolOutputPruning {
            eligible: BTreeMap::from([(
                "result".into(),
                ToolOutputIdentity {
                    run_id: "old".into(),
                    tool_call_id: "call".into(),
                    tool_name: "read_file".into(),
                    content_sha256: content_digest(&messages[1].content),
                    verified_artifact_id: None,
                },
            )]),
        }
    }
    #[test]
    fn tool_output_pruning_reduces_copy_and_duplicate_payload_without_changing_history() {
        let messages = history();
        let before = messages.clone();
        let projected = policy(&messages).project(&messages);
        assert!(
            serde_json::to_vec(&projected).unwrap().len()
                < serde_json::to_vec(&messages).unwrap().len() / 2
        );
        assert_eq!(messages, before);
        assert!(projected[1].content.text.contains("sha256"));
        assert_eq!(
            projected.iter().map(|m| &m.id).collect::<Vec<_>>(),
            messages.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
        assert_eq!(projected[0], messages[0]);
        assert_eq!(projected[2..], messages[2..]);
    }
    #[test]
    fn tool_output_pruning_requires_exact_host_admission_and_preserves_protected_results() {
        let messages = history();
        assert_eq!(ToolOutputPruning::default().project(&messages), messages);
        let mut p = policy(&messages);
        p.eligible.get_mut("result").unwrap().content_sha256 = "stale".into();
        assert_eq!(p.project(&messages), messages);
        for key in ["incomplete", "approvalPending", "outputTruncated"] {
            let mut m = history();
            m[1].content
                .metadata
                .as_mut()
                .unwrap()
                .insert(key.into(), DataValue::Bool(true));
            assert_eq!(policy(&m).project(&m), m);
        }
        let mut m = history();
        m[1].content
            .metadata
            .as_mut()
            .unwrap()
            .insert("toolStatus".into(), DataValue::String("error".into()));
        assert_eq!(policy(&m).project(&m), m);
        let mut m = history();
        m.remove(0);
        assert_eq!(policy(&history()).project(&m), m);
        let mut m = history();
        m.truncate(3);
        assert_eq!(policy(&m).project(&m), m);
    }

    #[derive(Default)]
    struct Capture(std::sync::Mutex<Vec<crate::ModelGenerateRequest>>);
    #[async_trait::async_trait]
    impl crate::ModelAdapter for Capture {
        fn provider(&self) -> &str {
            "capture"
        }
        async fn generate(
            &self,
            _: &crate::AgentConfig,
            request: &crate::ModelGenerateRequest,
        ) -> Result<crate::ModelGenerateResponse, String> {
            let mut requests = self.0.lock().unwrap();
            let first = requests.is_empty();
            requests.push(request.clone());
            Ok(crate::ModelGenerateResponse {
                content: Content {
                    text: "done".into(),
                    ..Default::default()
                },
                tool_calls: first.then(|| {
                    vec![crate::ToolCall {
                        id: "fresh-call".into(),
                        name: "read_file".into(),
                        args: Default::default(),
                    }]
                }),
                usage: Default::default(),
                stop_reason: if first {
                    crate::ModelStopReason::ToolCall
                } else {
                    crate::ModelStopReason::End
                },
            })
        }
    }
    #[tokio::test]
    async fn tool_output_pruning_runtime_projects_every_iteration_without_persisting_projection() {
        let adapter = std::sync::Arc::new(Capture::default());
        let config:crate::AgentConfig=serde_json::from_value(serde_json::json!({"name":"capture","model":"capture","tools":[{"name":"read_file","description":"read","parameters_schema":{}}]})).unwrap();
        let mut snapshot =
            crate::AgentRuntime::new_with_id("a", config, adapter.clone()).snapshot();
        let history = history();
        snapshot.messages = history.clone();
        snapshot.message_count = history.len();
        let mut runtime = crate::AgentRuntime::from_snapshot(snapshot, adapter.clone());
        runtime.set_tool_output_pruning(policy(&history));
        runtime.set_run_id("current");
        let base = runtime.run_base();
        let result = runtime
            .run_in_room_with_context_and_tools(
                "r".into(),
                history.clone(),
                Content {
                    text: "current input".into(),
                    ..Default::default()
                },
                |_, _, _| async {
                    crate::TaskResult::success(
                        Content {
                            text: "current result".repeat(1000),
                            ..Default::default()
                        },
                        0,
                    )
                },
            )
            .await;
        assert_eq!(result.status, crate::TaskStatus::Success);
        {
            let requests = adapter.0.lock().unwrap();
            assert_eq!(requests.len(), 2);
            for request in requests.iter() {
                assert!(request.messages[1].content.text.contains("old-tool-output"));
            }
            assert!(requests[1]
                .messages
                .iter()
                .any(|m| m.content.text == "current result".repeat(1000)));
            assert_eq!(&runtime.messages()[..history.len()], history.as_slice());
            assert!(
                !serde_json::to_string(&runtime.run_delta_since(&base).messages)
                    .unwrap()
                    .contains("promptOutputPruned")
            );
        }
        let fresh = std::sync::Arc::new(Capture::default());
        let mut restored = crate::AgentRuntime::from_snapshot(runtime.snapshot(), fresh.clone());
        restored
            .run_in_room_with_context("r".into(), history.clone(), Content::default())
            .await;
        assert_eq!(fresh.0.lock().unwrap()[0].messages[1], history[1]);
    }
}
