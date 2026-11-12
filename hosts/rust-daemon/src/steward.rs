//! The workspace steward: the one system agent every workspace has.
//!
//! It is a standalone agent like any other, with three differences: it is
//! created with the workspace and cannot be deleted, it sees every agent in
//! the roster, and it still answers when no language model is connected. Its
//! last fallback is the `steward` provider, which runs the rule engine in
//! `anima-steward` instead of a model; the actions that engine asks for go
//! through the ordinary tool and approval path.

use std::collections::BTreeMap;

use anima_core::{
    AgentConfig, AgentSettings, Content, DataValue, MessageRole, ModelAdapter, ModelChoice,
    ModelGenerateRequest, ModelGenerateResponse, ModelStopReason, TokenUsage, ToolCall,
};
use anima_steward::StewardTurn;
use async_trait::async_trait;

/// Settings key that marks an agent's kind.
pub(crate) const AGENT_KIND: &str = "agentKind";
pub(crate) const STEWARD_KIND: &str = "steward";
/// The rule-based provider and its only model.
pub(crate) const STEWARD_PROVIDER: &str = "steward";
pub(crate) const STEWARD_MODEL: &str = "rules";
pub(crate) const STEWARD_NAME: &str = "Steward";
const CALL_PREFIX: &str = "steward-";

const STEWARD_SYSTEM: &str = "You are the workspace steward, the system agent of this anima workspace. Every other agent works on its own; you look after the workspace for the owner. Keep answers short and plain. You can list the agents in the workspace and turn an agent's talking with other agents on or off when the owner asks. You do not do the agents' work for them. If something needs an agent you cannot reach, tell the owner which agent to open.";

pub(crate) fn is_steward_config(config: &AgentConfig) -> bool {
    config
        .settings
        .as_ref()
        .and_then(|settings| settings.additional.get(AGENT_KIND))
        == Some(&DataValue::String(STEWARD_KIND.into()))
}

/// A new steward. It uses the workspace's model when there is one, with the
/// rules as its last fallback; with no model at all it runs on the rules.
pub(crate) fn steward_config(provider: Option<&str>, model: Option<&str>) -> AgentConfig {
    let model_choice = provider
        .map(str::trim)
        .filter(|provider| {
            !provider.is_empty() && !crate::runtime_model::is_mock_provider(provider)
        })
        .zip(model.map(str::trim).filter(|model| !model.is_empty()));
    let (provider, model, fallbacks) = match model_choice {
        Some((provider, model)) => (
            provider.to_string(),
            model.to_string(),
            vec![ModelChoice {
                provider: STEWARD_PROVIDER.into(),
                model: STEWARD_MODEL.into(),
            }],
        ),
        None => (STEWARD_PROVIDER.into(), STEWARD_MODEL.into(), Vec::new()),
    };
    AgentConfig {
        name: STEWARD_NAME.into(),
        model,
        bio: Some("Looks after this workspace. Works even without a language model.".into()),
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: Some(provider),
        system: Some(STEWARD_SYSTEM.into()),
        tools: Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors([anima_steward::LIST_AGENTS_TOOL])
                .expect("the roster tool is registered"),
        ),
        plugins: None,
        settings: Some(AgentSettings {
            additional: BTreeMap::from([(
                AGENT_KIND.to_string(),
                DataValue::String(STEWARD_KIND.into()),
            )]),
            ..AgentSettings::default()
        }),
        fallbacks,
    }
}

/// Creates the workspace steward unless one exists. Returns the steward and
/// whether this call created it.
pub(crate) fn ensure_steward(
    state: &mut crate::state::DaemonState,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<(anima_core::AgentRuntimeSnapshot, bool), String> {
    if let Some(existing) = state
        .agent_states()
        .into_iter()
        .find(|agent| is_steward_config(&agent.config))
        .and_then(|agent| state.get_agent(&agent.id))
    {
        return Ok((existing, false));
    }
    let snapshot = state.create_agent(steward_config(provider, model))?;
    state.start_free(&snapshot.state.id);
    Ok((snapshot, true))
}

/// The pre-language-model steward: fixed phrasings, honest answers.
pub(crate) struct StewardRulesAdapter;

fn json_to_data(value: &serde_json::Value) -> DataValue {
    match value {
        serde_json::Value::Null => DataValue::Null,
        serde_json::Value::Bool(value) => DataValue::Bool(*value),
        serde_json::Value::Number(value) => DataValue::Number(value.as_f64().unwrap_or_default()),
        serde_json::Value::String(value) => DataValue::String(value.clone()),
        serde_json::Value::Array(items) => {
            DataValue::Array(items.iter().map(json_to_data).collect())
        }
        serde_json::Value::Object(map) => DataValue::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), json_to_data(value)))
                .collect(),
        ),
    }
}

fn text(text: String) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: Content {
            text,
            ..Content::default()
        },
        tool_calls: None,
        usage: TokenUsage::default(),
        stop_reason: ModelStopReason::End,
    }
}

#[async_trait]
impl ModelAdapter for StewardRulesAdapter {
    fn provider(&self) -> &str {
        STEWARD_PROVIDER
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let last_user = request
            .messages
            .iter()
            .rposition(|message| message.role == MessageRole::User);
        let input = last_user
            .map(|index| request.messages[index].content.text.as_str())
            .unwrap_or("");
        // Results of the calls this adapter made since the owner's message.
        let results: Vec<(String, String)> = request.messages[last_user.map_or(0, |i| i + 1)..]
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .filter_map(|message| {
                let id = match message.content.metadata.as_ref()?.get("toolCallId")? {
                    DataValue::String(id) => id,
                    _ => return None,
                };
                let tool = id.strip_prefix(CALL_PREFIX)?.rsplit_once('-')?.0;
                Some((tool.to_string(), message.content.text.clone()))
            })
            .collect();
        match anima_steward::next_turn(input, &results) {
            StewardTurn::Reply(reply) => Ok(text(reply)),
            StewardTurn::Tool { name, args } => {
                // The run passes the tools this turn may call in `config`.
                let offered = config.allows_tool(name);
                if !offered {
                    return Ok(text(
                        "I can't do that here. Ask me in your own chat with me, the steward."
                            .into(),
                    ));
                }
                Ok(ModelGenerateResponse {
                    content: Content::default(),
                    tool_calls: Some(vec![ToolCall {
                        id: format!("{CALL_PREFIX}{name}-{}", results.len() + 1),
                        name: name.to_string(),
                        args: args
                            .iter()
                            .map(|(key, value)| (key.clone(), json_to_data(value)))
                            .collect(),
                    }]),
                    usage: TokenUsage::default(),
                    stop_reason: ModelStopReason::ToolCall,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::Message;

    fn message(role: MessageRole, text: &str, call: Option<&str>) -> Message {
        Message {
            id: "m".into(),
            agent_id: "steward".into(),
            room_id: "r".into(),
            content: Content {
                text: text.into(),
                metadata: call.map(|id| {
                    BTreeMap::from([("toolCallId".to_string(), DataValue::String(id.into()))])
                }),
                ..Content::default()
            },
            role,
            created_at_ms: 1,
        }
    }

    fn request(messages: Vec<Message>) -> ModelGenerateRequest {
        ModelGenerateRequest {
            system: String::new(),
            messages,
            temperature: None,
            max_tokens: None,
        }
    }

    fn offering(tools: &[&str]) -> AgentConfig {
        let mut config = steward_config(None, None);
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(tools.iter().copied())
                .unwrap(),
        );
        config
    }

    #[test]
    fn a_new_steward_runs_on_rules_without_a_model_and_falls_back_to_them_with_one() {
        let alone = steward_config(None, None);
        assert_eq!(alone.provider.as_deref(), Some(STEWARD_PROVIDER));
        assert!(alone.fallbacks.is_empty());
        assert!(is_steward_config(&alone));
        let with_model = steward_config(Some("openai"), Some("gpt-5.4"));
        assert_eq!(with_model.provider.as_deref(), Some("openai"));
        assert_eq!(with_model.fallbacks[0].provider, STEWARD_PROVIDER);
        assert!(!crate::agent_runs::is_helper_config(&with_model));
    }

    #[tokio::test]
    async fn rules_call_the_offered_tool_then_summarize_its_result() {
        let config = offering(&["set_agent_talk"]);
        let first = StewardRulesAdapter
            .generate(
                &config,
                &request(vec![message(
                    MessageRole::User,
                    "let Ada talk with other agents",
                    None,
                )]),
            )
            .await
            .unwrap();
        let call = &first.tool_calls.unwrap()[0];
        assert_eq!(call.name, "set_agent_talk");
        assert_eq!(call.args["agent"], DataValue::String("Ada".into()));
        assert_eq!(call.args["enabled"], DataValue::Bool(true));

        let second = StewardRulesAdapter
            .generate(
                &config,
                &request(vec![
                    message(MessageRole::User, "let Ada talk with other agents", None),
                    message(MessageRole::Assistant, "", None),
                    message(
                        MessageRole::Tool,
                        r#"{"name":"Ada","talksWithAgents":true,"changed":true}"#,
                        Some(&call.id),
                    ),
                ]),
            )
            .await
            .unwrap();
        assert!(second.tool_calls.is_none());
        assert!(second.content.text.contains("Ada can now talk"));
    }

    #[tokio::test]
    async fn rules_never_call_a_tool_they_were_not_offered() {
        let reply = StewardRulesAdapter
            .generate(
                &offering(&[]),
                &request(vec![message(
                    MessageRole::User,
                    "make Ada work alone",
                    None,
                )]),
            )
            .await
            .unwrap();
        assert!(reply.tool_calls.is_none());
        assert!(reply.content.text.contains("can't do that here"));
    }
}
