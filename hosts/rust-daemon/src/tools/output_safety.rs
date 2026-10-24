use anima_core::{Content, DataValue, TaskResult};

const REDACTED: &str = "[redacted]";
const OVERSIZED: &str =
    "[Output incomplete: oversized tool output omitted during secret sanitization]";
const MAX_STRING_BYTES: usize = crate::tool_results::MAX_ARTIFACT_BYTES;
const MAX_VALUE_DEPTH: usize = 64;

/// Removes recognizable credentials from tool output before it leaves the daemon.
/// Oversized strings and excessively nested values are replaced wholesale so
/// neither redaction cost nor recursive traversal is controlled by tool output.
pub(super) fn sanitize_result(result: &mut TaskResult<Content>) {
    if let Some(data) = result.data.as_mut() {
        data.text = sanitize_text(&data.text);
        if let Some(metadata) = data.metadata.as_mut() {
            for (key, value) in metadata {
                *value = sanitize_value_by_key(key, value, 0);
            }
        }
        if data.text.contains("[Output incomplete:")
            || data
                .metadata
                .as_ref()
                .is_some_and(|values| values.values().any(has_omitted_value))
        {
            data.metadata
                .get_or_insert_with(Default::default)
                .insert("outputIncomplete".into(), DataValue::Bool(true));
        }
    }
    if let Some(error) = result.error.as_mut() {
        *error = sanitize_text(error);
    }
}

pub(super) fn sanitize_text(text: &str) -> String {
    if text.len() > MAX_STRING_BYTES {
        return OVERSIZED.into();
    }
    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(text) {
        sanitize_json_value(&mut json, 0);
        serde_json::to_string(&json).unwrap_or_else(|_| OVERSIZED.into())
    } else {
        crate::foreign_sessions::redact_secrets(text)
    }
}

fn sanitize_value_by_key(key: &str, value: &DataValue, depth: usize) -> DataValue {
    if is_secret_key(key, matches!(value, DataValue::Number(_))) {
        return DataValue::String(REDACTED.into());
    }
    sanitize_data_value(value, depth)
}

fn is_secret_key(key: &str, numeric: bool) -> bool {
    let normalized = key.to_ascii_lowercase().replace('_', "").replace('-', "");
    if numeric
        && matches!(
            normalized.as_str(),
            "tokens"
                | "tokencount"
                | "tokenlimit"
                | "maxtokens"
                | "mintokens"
                | "maxinputtokens"
                | "maxoutputtokens"
                | "maxtotaltokens"
                | "maxoutputtokensperresponse"
                | "maxoutputtokensperstep"
                | "budgettokens"
                | "contextbudgettokens"
                | "replyreservetokens"
                | "estimatedtokens"
                | "reportedtokens"
                | "prompttokens"
                | "completiontokens"
                | "inputtokens"
                | "outputtokens"
                | "totaltokens"
                | "cachedtokens"
                | "cachedinputtokens"
                | "reasoningtokens"
        )
    {
        return false;
    }
    normalized.contains("token") || crate::logs::redact::is_secret_field(key)
}

fn has_omitted_value(value: &DataValue) -> bool {
    match value {
        DataValue::String(text) => text.contains("[Output incomplete:"),
        DataValue::Array(values) => values.iter().any(has_omitted_value),
        DataValue::Object(values) => values.values().any(has_omitted_value),
        _ => false,
    }
}

fn sanitize_data_value(value: &DataValue, depth: usize) -> DataValue {
    if depth >= MAX_VALUE_DEPTH {
        return DataValue::String(
            "[Output incomplete: excessively nested tool metadata omitted]".into(),
        );
    }
    match value {
        DataValue::String(text) => DataValue::String(sanitize_text(text)),
        DataValue::Array(values) => DataValue::Array(
            values
                .iter()
                .map(|value| sanitize_data_value(value, depth + 1))
                .collect(),
        ),
        DataValue::Object(values) => DataValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), sanitize_value_by_key(key, value, depth + 1)))
                .collect(),
        ),
        DataValue::Null => DataValue::Null,
        DataValue::Bool(value) => DataValue::Bool(*value),
        DataValue::Number(value) => DataValue::Number(*value),
    }
}

fn sanitize_json_value(value: &mut serde_json::Value, depth: usize) {
    if depth >= MAX_VALUE_DEPTH {
        *value = serde_json::Value::String(
            "[Output incomplete: excessively nested tool metadata omitted]".into(),
        );
        return;
    }
    match value {
        serde_json::Value::String(text) => {
            *text = if text.len() > MAX_STRING_BYTES {
                OVERSIZED.into()
            } else {
                crate::foreign_sessions::redact_secrets(text)
            };
        }
        serde_json::Value::Array(values) => {
            for value in values {
                sanitize_json_value(value, depth + 1);
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if is_secret_key(key, value.is_number()) {
                    *value = serde_json::Value::String(REDACTED.into());
                } else {
                    sanitize_json_value(value, depth + 1);
                }
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use anima_core::{AgentState, AgentStatus, Message, MessageRole, ToolCall, ToolDescriptor};
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::sync::RwLock;

    fn fixture(
        _: super::super::ToolExecutionContext,
        _: AgentState,
        _: Message,
        _: ToolCall,
    ) -> futures::future::BoxFuture<'static, TaskResult<Content>> {
        Box::pin(async {
            TaskResult::success(
                Content {
                    text: "api_key=synthetic-secret-value".into(),
                    metadata: Some(BTreeMap::from([(
                        "password".into(),
                        DataValue::String("opaque-secret-value".into()),
                    )])),
                    ..Default::default()
                },
                1,
            )
        })
    }

    #[tokio::test]
    async fn secret_output_is_sanitized_at_actual_native_dispatch_boundary() {
        let descriptor = ToolDescriptor {
            name: "redaction_fixture".into(),
            description: "isolated fixture".into(),
            parameters_schema: Default::default(),
            examples: None,
        };
        let mut registry = super::super::ToolRegistry::new();
        registry.register(descriptor.clone(), fixture);
        let context = super::super::ToolExecutionContext::new(
            Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
            Arc::new(RwLock::new(
                crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
            )),
            None,
            registry,
            super::super::new_shared_process_manager_with_limit(1),
            None,
            None,
        );
        let agent = AgentState {
            id: "redaction-owner".into(),
            name: "fixture".into(),
            status: AgentStatus::Running,
            config: serde_json::from_value(
                serde_json::json!({"name":"fixture","model":"fixture","tools":[descriptor]}),
            )
            .unwrap(),
            created_at_ms: 1,
            token_usage: Default::default(),
        };
        let message = Message {
            id: "redaction-input".into(),
            agent_id: agent.id.clone(),
            room_id: "redaction-room".into(),
            content: Content::default(),
            role: MessageRole::User,
            created_at_ms: 1,
        };
        let result = context
            .execute_tool(
                agent,
                message,
                ToolCall {
                    id: "redaction-call".into(),
                    name: "redaction_fixture".into(),
                    args: BTreeMap::new(),
                },
            )
            .await;
        assert_eq!(result.status, anima_core::TaskStatus::Success);
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("synthetic-secret-value"));
        assert!(!serialized.contains("opaque-secret-value"));
        assert!(serialized.contains("[redacted]"));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anima_core::{Content, DataValue, TaskResult};

    use super::sanitize_result;

    #[test]
    fn credential_token_arrays_and_objects_are_not_accounting_exemptions() {
        let mut result = TaskResult::success(Content {
            text: r#"{"refresh_tokens":["opaque-refresh-value"],"api_tokens":{"primary":"opaque-api-value"},"maxTokens":12}"#.into(),
            ..Default::default()
        }, 0);
        sanitize_result(&mut result);
        let text = &result.data.unwrap().text;
        assert!(!text.contains("opaque-refresh-value"));
        assert!(!text.contains("opaque-api-value"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(text).unwrap()["maxTokens"],
            12
        );
    }

    #[test]
    fn omitted_output_is_explicitly_incomplete() {
        let mut result = TaskResult::success(
            Content {
                text: "a".repeat(super::MAX_STRING_BYTES + 1),
                ..Default::default()
            },
            0,
        );
        sanitize_result(&mut result);
        let data = result.data.unwrap();
        assert!(data.text.contains("[Output incomplete:"));
        assert_eq!(
            data.metadata.unwrap()["outputIncomplete"],
            DataValue::Bool(true)
        );
    }

    #[test]
    fn numeric_execution_and_context_budgets_survive_without_exempting_credentials() {
        let mut result = TaskResult::success(
            Content {
                text: serde_json::json!({
                    "bounds": {"maxOutputTokensPerResponse": 6000, "maxTotalTokens": 24000},
                    "contextBudgetTokens": 32000,
                    "api_tokens": 12345,
                    "maxOutputTokens": "opaque-credential"
                })
                .to_string(),
                ..Default::default()
            },
            0,
        );
        sanitize_result(&mut result);
        let parsed: serde_json::Value = serde_json::from_str(&result.data.unwrap().text).unwrap();
        assert_eq!(parsed["bounds"]["maxOutputTokensPerResponse"], 6000);
        assert_eq!(parsed["bounds"]["maxTotalTokens"], 24000);
        assert_eq!(parsed["contextBudgetTokens"], 32000);
        assert_eq!(parsed["api_tokens"], "[redacted]");
        assert_eq!(parsed["maxOutputTokens"], "[redacted]");
    }

    fn object(entries: impl IntoIterator<Item = (&'static str, DataValue)>) -> DataValue {
        DataValue::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn sanitizes_secret_labeled_compound_metadata_and_preserves_accounting() {
        let mut result = TaskResult::success(
            Content {
                text: "finished".into(),
                attachments: None,
                metadata: Some(BTreeMap::from([(
                    "details".into(),
                    object([
                        (
                            "api_key" as &'static str,
                            DataValue::Array(vec![
                                DataValue::String("plain-secret-value".into()),
                                DataValue::Number(42.0),
                            ]),
                        ),
                        ("tokenCount", DataValue::Number(12.0)),
                        (
                            "nested",
                            object([("message", DataValue::String("password=hunter2".into()))]),
                        ),
                    ]),
                )])),
            },
            37,
        );

        sanitize_result(&mut result);

        let content = result.data.as_ref().unwrap();
        let metadata = content.metadata.as_ref().unwrap();
        let details = &metadata["details"];
        let DataValue::Object(details) = details else {
            panic!("details should remain an object");
        };
        assert_eq!(details["api_key"], DataValue::String("[redacted]".into()));
        assert_eq!(details["tokenCount"], DataValue::Number(12.0));
        let DataValue::Object(nested) = &details["nested"] else {
            panic!("nested should remain an object");
        };
        assert_eq!(
            nested["message"],
            DataValue::String("password=[redacted]".into())
        );
        assert_eq!(result.duration_ms, 37);
        assert_eq!(result.status, anima_core::TaskStatus::Success);
    }

    #[test]
    fn sanitizes_json_text_by_secret_key_and_plain_text_patterns() {
        let mut result = TaskResult::success(
            Content {
                text: r#"{"access_token":{"value":"opaque"},"count":3,"note":"api_key=rawsecret"}"#
                    .into(),
                attachments: None,
                metadata: None,
            },
            0,
        );

        sanitize_result(&mut result);

        let text = &result.data.as_ref().unwrap().text;
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["access_token"], "[redacted]");
        assert_eq!(parsed["count"], 3);
        assert_eq!(parsed["note"], "api_key=[redacted]");
        assert!(!text.contains("opaque"));
        assert!(!text.contains("rawsecret"));
    }

    #[test]
    fn sanitizes_error_text_including_multiline_private_keys() {
        let pem = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----";
        let mut result: TaskResult<Content> =
            TaskResult::error(format!("provider failed: api_key=secret123\n{pem}"), 8);

        sanitize_result(&mut result);

        let error = result.error.as_deref().unwrap();
        assert!(!error.contains("secret123"));
        assert!(!error.contains("MIIEvQIB"));
        assert!(error.contains("[redacted private key]"));
        assert!(error.contains("api_key=[redacted]"));
        assert_eq!(result.duration_ms, 8);
    }

    #[test]
    fn preserves_benign_text_and_numeric_metadata() {
        let mut result = TaskResult::success(
            Content {
                text: "processed 3 items in 12 ms".into(),
                attachments: None,
                metadata: Some(BTreeMap::from([
                    ("maxTokens".into(), DataValue::Number(2048.0)),
                    ("count".into(), DataValue::Number(3.0)),
                    ("label".into(), DataValue::String("complete".into())),
                ])),
            },
            12,
        );

        sanitize_result(&mut result);

        let content = result.data.as_ref().unwrap();
        assert_eq!(content.text, "processed 3 items in 12 ms");
        assert_eq!(
            content.metadata.as_ref().unwrap()["maxTokens"],
            DataValue::Number(2048.0)
        );
        assert_eq!(
            content.metadata.as_ref().unwrap()["count"],
            DataValue::Number(3.0)
        );
        assert_eq!(
            content.metadata.as_ref().unwrap()["label"],
            DataValue::String("complete".into())
        );
    }
}
