//! Durable conversation preferences. Selection is applied to isolated turn copies.
use anima_core::{AgentConfig, DataValue};
use serde::{Deserialize, Serialize};

use crate::app::SharedDaemonState;
use crate::{routes::ApiError, runtime_model::canonical_provider, state::DaemonState};

const EFFORT_KEY: &str = "reasoningEffort";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ModelPreference {
    #[default]
    Inherit,
    Override {
        provider: String,
        model: String,
        #[serde(
            default,
            rename = "reasoningEffort",
            skip_serializing_if = "Option::is_none"
        )]
        reasoning_effort: Option<String>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelPreferences {
    #[serde(default)]
    pub(crate) primary: ModelPreference,
    #[serde(default)]
    pub(crate) reviewer: ModelPreference,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedModel {
    pub(crate) provider: String,
    pub(crate) model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<String>,
}

impl ResolvedModel {
    pub(crate) fn same_model(&self, other: &Self) -> bool {
        let family = |provider: &str| {
            let provider = canonical_provider(provider);
            if provider == "chatgpt" {
                "openai".to_string()
            } else {
                provider
            }
        };
        if family(&self.provider) != family(&other.provider) {
            return false;
        }
        let canonical_model = |choice: &Self| {
            anima_model_adapters::model_info(&choice.provider, &choice.model)
                .map(|info| info.model_prefix.to_string())
                .unwrap_or_else(|| choice.model.trim().to_ascii_lowercase())
        };
        canonical_model(self) == canonical_model(other)
    }
    pub(crate) fn of(config: &AgentConfig) -> Self {
        Self {
            provider: canonical_provider(config.provider.as_deref().unwrap_or("openai")),
            model: config.model.clone(),
            reasoning_effort: effort(config).map(str::to_owned),
        }
    }
    pub(crate) fn apply(&self, config: &mut AgentConfig) {
        config.provider = Some(self.provider.clone());
        config.model = self.model.clone();
        let additional = &mut config
            .settings
            .get_or_insert_with(Default::default)
            .additional;
        additional.remove(EFFORT_KEY);
        if let Some(effort) = &self.reasoning_effort {
            additional.insert(EFFORT_KEY.into(), DataValue::String(effort.clone()));
        }
    }
}

pub(crate) fn effort(config: &AgentConfig) -> Option<&str> {
    match config.settings.as_ref()?.additional.get(EFFORT_KEY)? {
        DataValue::String(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn primary_config(
    state: &DaemonState,
    agent_id: &str,
    session_id: &str,
    source: &AgentConfig,
) -> AgentConfig {
    let mut config = source.clone();
    if let Some(ModelPreference::Override {
        provider,
        model,
        reasoning_effort,
    }) = state
        .sessions
        .get(agent_id, session_id)
        .map(|session| &session.model_preferences.primary)
    {
        ResolvedModel {
            provider: provider.clone(),
            model: model.clone(),
            reasoning_effort: reasoning_effort.clone(),
        }
        .apply(&mut config);
        // A session override names the model chosen for this chat. The inherited
        // chain remains available but effort is validated per physical attempt.
    }
    config
}

/// Helpers follow the launching turn's admitted choice, even when preferences
/// change before their launch is processed. Permissions still come from the
/// helper's independently bounded configuration.
pub(crate) fn inherit_helper_model(
    state: &DaemonState,
    parent: Option<&crate::runs::RunLink>,
    config: &mut AgentConfig,
) {
    let Some(parent) = parent else {
        return;
    };
    let Some(run) = state.runs.get(&parent.run_id).filter(|run| {
        run.agent_id == parent.agent_id
            && run.session_id == parent.session_id
            && !run.model.trim().is_empty()
    }) else {
        return;
    };
    config.model = run.model.clone();
    config.provider = run.provider.clone();
    let additional = &mut config
        .settings
        .get_or_insert_with(Default::default)
        .additional;
    additional.remove(EFFORT_KEY);
    if let Some(effort) = &run.reasoning_effort {
        additional.insert(EFFORT_KEY.into(), DataValue::String(effort.clone()));
    }
}

/// Conservative, documented controls only. An empty list means provider default.
pub(crate) fn reasoning_efforts(provider: &str, model: &str) -> &'static [&'static str] {
    let provider = canonical_provider(provider);
    if provider == "openai"
        && std::env::var("OPENAI_BASE_URL").is_ok_and(|base| {
            !base.trim().is_empty()
                && !base
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case("https://api.openai.com/v1")
        })
    {
        return &[];
    }
    anima_model_adapters::supported_reasoning_efforts(&provider, model)
}

pub(crate) fn validate_preference(
    preference: &ModelPreference,
) -> Result<ModelPreference, ApiError> {
    let ModelPreference::Override {
        provider,
        model,
        reasoning_effort,
    } = preference
    else {
        return Ok(ModelPreference::Inherit);
    };
    let provider = canonical_provider(provider);
    let model = model.trim().to_string();
    if !crate::runtime_model::is_routable_provider(&provider)
        || crate::runtime_model::is_mock_provider(&provider)
    {
        return Err(ApiError::bad_request_static(
            "Choose a supported model provider",
        ));
    }
    if model.is_empty() || model.chars().count() > 200 || model.chars().any(char::is_control) {
        return Err(ApiError::bad_request_static(
            "model must contain 1-200 plain characters",
        ));
    }
    if reasoning_effort
        .as_ref()
        .is_some_and(|effort| !reasoning_efforts(&provider, &model).contains(&effort.as_str()))
    {
        return Err(ApiError::bad_request_static(
            "This reasoning effort is unsupported for the selected model; use provider default",
        ));
    }
    Ok(ModelPreference::Override {
        provider,
        model,
        reasoning_effort: reasoning_effort.clone(),
    })
}

pub(crate) fn resolve_reviewer(
    state: &DaemonState,
    prefs: &ModelPreferences,
    primary: &ResolvedModel,
) -> Result<ResolvedModel, &'static str> {
    let choice = match &prefs.reviewer {
        ModelPreference::Inherit => state.side_model.as_ref().map(|choice| ResolvedModel {
            provider: canonical_provider(&choice.provider),
            model: choice.model.clone(),
            reasoning_effort: None,
        }),
        ModelPreference::Override {
            provider,
            model,
            reasoning_effort,
        } => Some(ResolvedModel {
            provider: provider.clone(),
            model: model.clone(),
            reasoning_effort: reasoning_effort.clone(),
        }),
    }
    .ok_or("Choose a reviewer model or configure a different side-task model")?;
    if choice.same_model(primary) {
        return Err("Choose a reviewer different from the chat's primary model");
    }
    Ok(choice)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelOption {
    #[serde(flatten)]
    choice: ResolvedModel,
    reasoning_efforts: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreferencesView {
    pub(crate) preferences: ModelPreferences,
    pub(crate) primary: ResolvedModel,
    pub(crate) reviewer: Option<ResolvedModel>,
    pub(crate) reviewer_unavailable_reason: Option<String>,
    available_models: Vec<ModelOption>,
    applies_to: &'static str,
    clear_generation: u64,
}

pub(crate) async fn provider_available(state: &SharedDaemonState, provider: &str) -> bool {
    let provider = canonical_provider(provider);
    let guard = state.read().await;
    if provider == "chatgpt" {
        let auth = guard.chatgpt_auth.clone();
        drop(guard);
        return auth.status().await.is_ok_and(|status| status.connected);
    }
    crate::runtime_model::provider_summaries_with_pool(&guard.provider_keys)
        .iter()
        .any(|entry| {
            entry.id == provider
                && entry.configured
                && !crate::runtime_model::is_mock_provider(&provider)
        })
}

pub(crate) async fn view(
    state: &SharedDaemonState,
    agent: &str,
    session: &str,
) -> Result<PreferencesView, ApiError> {
    let (preferences, primary, candidate, summaries, auth, clear_generation) = {
        let guard = state.read().await;
        let record = guard
            .sessions
            .get(agent, session)
            .ok_or_else(ApiError::not_found)?;
        let runtime = guard.agents.get(agent).ok_or_else(ApiError::not_found)?;
        let primary = ResolvedModel::of(&primary_config(&guard, agent, session, runtime.config()));
        (
            record.model_preferences.clone(),
            primary.clone(),
            resolve_reviewer(&guard, &record.model_preferences, &primary),
            crate::runtime_model::provider_summaries_with_pool(&guard.provider_keys),
            guard.chatgpt_auth.clone(),
            record.clear_generation,
        )
    };
    let mut available_models = Vec::new();
    for entry in anima_model_adapters::model_table() {
        if summaries.iter().any(|provider| {
            provider.id == entry.provider
                && provider.configured
                && !crate::runtime_model::is_mock_provider(provider.id)
        }) {
            available_models.push(ModelOption {
                choice: ResolvedModel {
                    provider: entry.provider.into(),
                    model: entry.model_prefix.into(),
                    reasoning_effort: None,
                },
                reasoning_efforts: reasoning_efforts(entry.provider, entry.model_prefix)
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
            });
        }
    }
    if auth.status().await.is_ok_and(|status| status.connected) {
        if let Ok(catalog) = auth.models().await {
            for model in catalog.models {
                available_models.push(ModelOption {
                    reasoning_efforts: reasoning_efforts("chatgpt", &model.id)
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    choice: ResolvedModel {
                        provider: "chatgpt".into(),
                        model: model.id,
                        reasoning_effort: None,
                    },
                });
            }
        }
    }
    let (reviewer, reviewer_unavailable_reason) = match candidate {
        Ok(choice) if provider_available(state, &choice.provider).await => (Some(choice), None),
        Ok(_) => (
            None,
            Some("The reviewer provider is unavailable; connect it in Models".into()),
        ),
        Err(reason) => (None, Some(reason.into())),
    };
    Ok(PreferencesView {
        preferences,
        primary,
        reviewer,
        reviewer_unavailable_reason,
        available_models,
        applies_to: "next_turn",
        clear_generation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_inherits_frozen_parent_selection_and_none_clears_stale_effort() {
        use crate::runs::{RunLink, RunRecord, RunSource, RunStart};
        let mut state = DaemonState::new();
        let mut run = RunRecord::running(
            RunStart {
                agent_id: "owner".into(),
                session_id: "chat:one".into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "task".into(),
                model: "gpt-6-astra".into(),
                provider: Some("chatgpt".into()),
                parent_run_id: None,
            },
            1,
        );
        run.reasoning_effort = Some("high".into());
        let parent = RunLink {
            agent_id: run.agent_id.clone(),
            session_id: run.session_id.clone(),
            run_id: run.id.clone(),
        };
        state.runs.insert(run);
        let mut session = crate::sessions::SessionRecord::new(
            "owner",
            "chat:one",
            crate::sessions::SessionKind::Chat,
            crate::sessions::SessionOrigin::Web,
            "Chat".into(),
            crate::sessions::TitleSource::Owner,
            1,
        );
        session.model_preferences.primary = ModelPreference::Override {
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
            reasoning_effort: None,
        };
        state.sessions.insert(session);
        let mut helper = crate::agent_runs::test_support::companion_config("Helper");
        let original_tools = helper.tools.clone();
        inherit_helper_model(&state, Some(&parent), &mut helper);
        assert_eq!(helper.model, "gpt-6-astra");
        assert_eq!(helper.provider.as_deref(), Some("chatgpt"));
        assert_eq!(effort(&helper), Some("high"));
        assert_eq!(helper.tools, original_tools);
        state.runs.get_mut(&parent.run_id).unwrap().reasoning_effort = None;
        inherit_helper_model(&state, Some(&parent), &mut helper);
        assert_eq!(effort(&helper), None);
        let mut other = parent.clone();
        other.agent_id = "different-owner".into();
        helper.model = "unchanged".into();
        inherit_helper_model(&state, Some(&other), &mut helper);
        assert_eq!(helper.model, "unchanged");
    }

    #[tokio::test]
    async fn model_change_during_reply_applies_only_to_the_following_turn() {
        use crate::agent_runs::test_support::{
            add_chat, chat_request, coordinator_state, coordinator_with, Gate, ScriptedModel, Step,
        };
        let gate = Gate::new();
        let model = ScriptedModel::gated(
            vec![Step::Text(vec!["first"]), Step::Text(vec!["second"])],
            gate.clone(),
        );
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, "chat:model").await;
        let first = {
            let coordinator = coordinator.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                coordinator
                    .run(chat_request(&agent, "chat:model", "first"))
                    .await
            })
        };
        gate.entered().await;
        let original = model.configs()[0].model.clone();
        let state = coordinator_state(&coordinator);
        state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:model")
            .unwrap()
            .model_preferences
            .primary = ModelPreference::Override {
            provider: "openai".into(),
            model: "gpt-5.4-mini".into(),
            reasoning_effort: Some("high".into()),
        };
        gate.release();
        first.await.unwrap().unwrap();
        let second = {
            let coordinator = coordinator.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                coordinator
                    .run(chat_request(&agent, "chat:model", "second"))
                    .await
            })
        };
        gate.entered().await;
        gate.release();
        second.await.unwrap().unwrap();
        assert_eq!(model.configs()[0].model, original);
        assert_eq!(model.configs()[1].model, "gpt-5.4-mini");
        assert_eq!(effort(&model.configs()[1]), Some("high"));
        assert_eq!(
            state
                .read()
                .await
                .get_agent(&agent)
                .unwrap()
                .state
                .config
                .model,
            original
        );
    }

    #[test]
    fn inheritance_and_provider_default_are_explicit_and_legacy_safe() {
        assert_eq!(
            serde_json::from_str::<ModelPreferences>("{}").unwrap(),
            ModelPreferences::default()
        );
        assert_eq!(
            serde_json::to_value(ModelPreference::Inherit).unwrap(),
            serde_json::json!({"mode":"inherit"})
        );
        assert!(validate_preference(&ModelPreference::Override {
            provider: "openai".into(),
            model: "gpt-6-astra".into(),
            reasoning_effort: Some("none".into())
        })
        .is_err());
        assert!(validate_preference(&ModelPreference::Override {
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
            reasoning_effort: Some("high".into())
        })
        .is_err());
    }

    #[test]
    fn subscription_transport_and_model_aliases_do_not_create_false_independence() {
        let first = ResolvedModel {
            provider: "openai".into(),
            model: "gpt-5.4".into(),
            reasoning_effort: None,
        };
        let same = ResolvedModel {
            provider: "chatgpt".into(),
            model: "gpt-5.4-2026-03-05".into(),
            reasoning_effort: Some("high".into()),
        };
        assert!(first.same_model(&same));
        assert!(!first.same_model(&ResolvedModel {
            model: "gpt-5.4-mini".into(),
            ..same
        }));
    }

    #[test]
    fn selection_copy_does_not_mutate_agent_and_clear_survives_record_restore() {
        let mut state = DaemonState::new();
        let config = crate::agent_runs::test_support::companion_config("Companion");
        let mut record = crate::sessions::SessionRecord::new(
            "a",
            "chat:one",
            crate::sessions::SessionKind::Chat,
            crate::sessions::SessionOrigin::Web,
            "Chat".into(),
            crate::sessions::TitleSource::System,
            1,
        );
        record.model_preferences.primary = ModelPreference::Override {
            provider: "openai".into(),
            model: "gpt-5.4-mini".into(),
            reasoning_effort: Some("high".into()),
        };
        state.sessions.insert(record.clone());
        let chosen = primary_config(&state, "a", "chat:one", &config);
        assert_eq!(chosen.model, "gpt-5.4-mini");
        assert_eq!(effort(&chosen), Some("high"));
        assert_ne!(config.model, chosen.model);
        assert_eq!(
            serde_json::from_value::<crate::sessions::SessionRecord>(
                serde_json::to_value(record.clone()).unwrap()
            )
            .unwrap()
            .model_preferences,
            record.model_preferences
        );
    }
}
