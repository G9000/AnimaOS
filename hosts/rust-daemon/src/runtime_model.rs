#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "runtime_model/timeout_tests.rs"]
mod timeout_tests;

use anima_core::{AgentConfig, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse};
use anima_model_adapters::{
    provider_definitions, ProviderAdapterConfig, ProviderCredential, ProviderModelAdapter,
};
use async_trait::async_trait;
use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

use crate::model::DeterministicModelAdapter;

fn first_env_value(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .and_then(|value| (!value.trim().is_empty()).then_some(value))
    })
}

/// Provider id to the key the environment supplies for it: the seed of the
/// key pool's `env` key.
pub(crate) fn provider_env_keys() -> BTreeMap<String, String> {
    provider_definitions()
        .iter()
        .filter_map(|definition| {
            first_env_value(definition.api_key_envs).map(|key| (definition.id.to_owned(), key))
        })
        .collect()
}

/// Every `(provider id, environment variable)` pair that can supply a provider's
/// API key, in the order the daemon reads them (the first one set wins).
pub fn provider_env_key_names() -> Vec<(&'static str, &'static str)> {
    provider_definitions()
        .iter()
        .flat_map(|definition| {
            definition
                .api_key_envs
                .iter()
                .map(|name| (definition.id, *name))
        })
        .collect()
}

fn provider_config_from_env() -> ProviderAdapterConfig {
    ProviderAdapterConfig {
        key_source: None,
        providers: provider_definitions()
            .iter()
            .map(|definition| {
                let api_key = first_env_value(definition.api_key_envs);
                let base_url = first_env_value(definition.base_url_envs)
                    .unwrap_or_else(|| definition.default_base_url.to_owned());
                (
                    definition.id.to_owned(),
                    ProviderCredential { api_key, base_url },
                )
            })
            .collect(),
    }
}

/// Whether `RuntimeModelAdapter` can route a call to `provider` (an id or
/// alias, any case).
pub(crate) fn is_routable_provider(provider: &str) -> bool {
    let provider = provider.trim().to_ascii_lowercase();
    matches!(provider.as_str(), "deterministic" | "test" | "chatgpt")
        || provider_definitions().iter().any(|definition| {
            definition.id == provider || definition.aliases.contains(&provider.as_str())
        })
}

/// `provider` as the id it routes to: trimmed, lower case, and an alias such
/// as `gemini` read as its provider's id (`google`).
pub(crate) fn canonical_provider(provider: &str) -> String {
    let provider = provider.trim().to_ascii_lowercase();
    provider_definitions()
        .iter()
        .find(|definition| definition.aliases.contains(&provider.as_str()))
        .map_or(provider, |definition| definition.id.to_owned())
}

/// Whether `provider` is one of the built-in mock providers (`deterministic`,
/// `test`): valid as an agent's own model, never as a fallback or side model.
pub(crate) fn is_mock_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "deterministic" | "test"
    )
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderSummary {
    pub id: &'static str,
    pub label: &'static str,
    pub requires_key: bool,
    pub configured: bool,
    pub api_key_envs: &'static [&'static str],
}

/// Opt-in API-key use only. No OAuth/subscription token lookup or network
/// request occurs while resolving capability or constructing this adapter.
pub(crate) fn transcription_service_from_env() -> crate::voice::VoiceService {
    if !std::env::var("ANIMAOS_STT_ENABLED").is_ok_and(|value| value == "true" || value == "1") {
        return crate::voice::VoiceService::disabled();
    }
    let Some(definition) = provider_definitions().iter().find(|d| d.id == "openai") else {
        return crate::voice::VoiceService::configured(
            anima_service_adapters::audio_content::TranscriptionAvailability::Unavailable,
            None,
        );
    };
    let base = first_env_value(definition.base_url_envs)
        .unwrap_or_else(|| definition.default_base_url.to_owned());
    if !canonical_transcription_base(&base) {
        return crate::voice::VoiceService::configured(
            anima_service_adapters::audio_content::TranscriptionAvailability::UnsupportedConfiguration,
            None,
        );
    }
    transcription_service_for_config(&base, first_env_value(definition.api_key_envs))
}

fn canonical_transcription_base(base: &str) -> bool {
    matches!(
        base,
        "https://api.openai.com/v1"
            | "https://api.openai.com/v1/"
            | "https://api.openai.com:443/v1"
            | "https://api.openai.com:443/v1/"
    )
}
fn transcription_service_for_config(base: &str, key: Option<String>) -> crate::voice::VoiceService {
    use anima_service_adapters::audio_content::TranscriptionAvailability as Availability;
    if !canonical_transcription_base(base) {
        return crate::voice::VoiceService::configured(
            Availability::UnsupportedConfiguration,
            None,
        );
    }
    let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
        return crate::voice::VoiceService::configured(Availability::MissingCredentials, None);
    };
    match anima_model_adapters::OpenAiTranscriptionAdapter::new(key) {
        Ok(port) => crate::voice::VoiceService::configured(
            Availability::Available,
            Some(std::sync::Arc::new(port)),
        ),
        Err(_) => crate::voice::VoiceService::configured(Availability::Unavailable, None),
    }
}

#[cfg(test)]
mod speech_configuration_tests {
    use super::*;
    use anima_service_adapters::SpeechAvailability as Availability;

    #[test]
    fn speech_requires_independent_opt_in_and_only_canonical_platform_configuration() {
        for enabled in [None, Some("false"), Some("TRUE"), Some("0"), Some(" true")] {
            assert_eq!(
                speech_service_for_config(
                    enabled,
                    "https://api.openai.com/v1",
                    Some("synthetic-key".into())
                )
                .availability(),
                Availability::Disabled
            );
        }
        for enabled in ["true", "1"] {
            for base in [
                "https://api.openai.com/v1",
                "https://api.openai.com/v1/",
                "https://api.openai.com:443/v1",
                "https://api.openai.com:443/v1/",
            ] {
                assert_eq!(
                    speech_service_for_config(Some(enabled), base, None).availability(),
                    Availability::MissingCredentials
                );
                assert_eq!(
                    speech_service_for_config(Some(enabled), base, Some("synthetic-key".into()))
                        .availability(),
                    Availability::Available
                );
            }
            for base in [
                "http://api.openai.com/v1",
                "https://other.test/v1",
                "https://api.openai.com:444/v1",
                "https://@api.openai.com/v1",
                "https://user@api.openai.com/v1",
                "https://api.openai.com/v1/../v1",
                "https://api.openai.com/./v1/",
                "https://api.openai.com/v1?key=x",
                "https://api.openai.com/v1#x",
            ] {
                assert_eq!(
                    speech_service_for_config(Some(enabled), base, Some("synthetic-key".into()))
                        .availability(),
                    Availability::UnsupportedConfiguration,
                    "{base}"
                );
            }
        }
    }
}

/// Speech has independent explicit API charge consent. Never read STT enablement,
/// OAuth credentials, model subscriptions, or connector profiles here.
pub(crate) fn speech_service_from_env() -> crate::speech::SpeechService {
    let enabled = std::env::var("ANIMAOS_TTS_ENABLED").ok();
    if !matches!(enabled.as_deref(), Some("true" | "1")) {
        return crate::speech::SpeechService::disabled();
    }
    let Some(definition) = provider_definitions().iter().find(|d| d.id == "openai") else {
        return crate::speech::SpeechService::configured(
            anima_service_adapters::SpeechAvailability::Unavailable,
            None,
        );
    };
    let base = first_env_value(definition.base_url_envs)
        .unwrap_or_else(|| definition.default_base_url.to_owned());
    speech_service_for_config(
        enabled.as_deref(),
        &base,
        first_env_value(definition.api_key_envs),
    )
}

fn speech_service_for_config(
    enabled: Option<&str>,
    base: &str,
    key: Option<String>,
) -> crate::speech::SpeechService {
    use crate::speech::SpeechService;
    use anima_service_adapters::SpeechAvailability as Availability;
    if !matches!(enabled, Some("true" | "1")) {
        return SpeechService::disabled();
    }
    if !canonical_transcription_base(base) {
        return SpeechService::configured(Availability::UnsupportedConfiguration, None);
    }
    let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
        return SpeechService::configured(Availability::MissingCredentials, None);
    };
    match anima_model_adapters::OpenAiSpeechAdapter::new(key) {
        Ok(port) => {
            SpeechService::configured(Availability::Available, Some(std::sync::Arc::new(port)))
        }
        Err(_) => SpeechService::configured(Availability::Unavailable, None),
    }
}

#[cfg(test)]
mod transcription_configuration_tests {
    use super::*;
    use anima_service_adapters::audio_content::TranscriptionAvailability as Availability;
    #[test]
    fn only_canonical_openai_api_configuration_is_eligible_without_discovery() {
        for base in [
            "https://api.openai.com/v1",
            "https://api.openai.com:443/v1/",
        ] {
            assert_eq!(
                transcription_service_for_config(base, None).availability(),
                Availability::MissingCredentials
            );
            assert_eq!(
                transcription_service_for_config(base, Some("synthetic-key".into())).availability(),
                Availability::Available
            );
        }
        for base in [
            "http://api.openai.com/v1",
            "https://elsewhere.test/v1",
            "https://api.openai.com:444/v1",
            "https://user@api.openai.com/v1",
            "https://api.openai.com/v1?key=x",
            "https://api.openai.com/v1#fragment",
            "https://api.openai.com/other",
        ] {
            assert_eq!(
                transcription_service_for_config(base, Some("synthetic-key".into())).availability(),
                Availability::UnsupportedConfiguration
            );
        }
    }

    #[test]
    fn m56_review_regression_noncanonical_openai_base_spellings_are_unavailable() {
        for base in [
            "https://@api.openai.com/v1",
            "https://api.openai.com/v1/../v1",
            "https://api.openai.com/./v1/",
        ] {
            assert_eq!(
                transcription_service_for_config(base, Some("synthetic-key".into())).availability(),
                Availability::UnsupportedConfiguration,
                "{base}"
            );
        }
    }
}

pub(crate) fn provider_summaries() -> Vec<ProviderSummary> {
    provider_summaries_for_config(&provider_config_from_env())
}

/// The provider catalog with `configured` also true for a provider whose key
/// pool holds a key (env or file), resting or not: it is configured, and the
/// key list shows when it is resting.
pub(crate) fn provider_summaries_with_pool(
    pool: &crate::provider_keys::KeyPool,
) -> Vec<ProviderSummary> {
    let mut summaries = provider_summaries();
    for summary in &mut summaries {
        if summary.requires_key && !summary.configured && pool.holds_keys(summary.id) {
            summary.configured = true;
        }
    }
    summaries
}

fn provider_summaries_for_config(config: &ProviderAdapterConfig) -> Vec<ProviderSummary> {
    let mut summaries = Vec::with_capacity(provider_definitions().len() + 1);
    summaries.push(ProviderSummary {
        id: "deterministic",
        label: "Deterministic (mock)",
        requires_key: false,
        configured: true,
        api_key_envs: &[],
    });
    summaries.extend(provider_definitions().iter().map(|definition| {
        let configured = !definition.requires_key
            || config
                .providers
                .get(definition.id)
                .and_then(|credential| credential.api_key.as_deref())
                .is_some_and(|api_key| !api_key.trim().is_empty());
        ProviderSummary {
            id: definition.id,
            label: definition.label,
            requires_key: definition.requires_key,
            configured,
            api_key_envs: definition.api_key_envs,
        }
    }));
    summaries
}

#[derive(Clone)]
pub(crate) struct RuntimeModelAdapter {
    providers: ProviderModelAdapter,
    chatgpt_auth: crate::chatgpt_auth::ChatGptAuth,
}

impl RuntimeModelAdapter {
    #[cfg(test)]
    pub(crate) fn from_env(chatgpt_auth: crate::chatgpt_auth::ChatGptAuth) -> Self {
        Self {
            providers: ProviderModelAdapter::new(provider_config_from_env()),
            chatgpt_auth,
        }
    }

    /// `provider_keys` supplies the API key for each call and rotates on key
    /// failures; the environment keys remain the fallback for providers it
    /// holds no key for.
    pub(crate) fn try_from_env(
        chatgpt_auth: crate::chatgpt_auth::ChatGptAuth,
        provider_keys: Option<crate::provider_keys::KeyPool>,
    ) -> Result<Self, String> {
        let mut config = provider_config_from_env();
        config.key_source = provider_keys.map(|pool| Arc::new(pool) as Arc<_>);
        Ok(Self {
            providers: ProviderModelAdapter::try_new(config)?,
            chatgpt_auth,
        })
    }

    #[cfg(test)]
    fn with_config(config: ProviderAdapterConfig) -> Self {
        Self::from_config(config)
    }

    #[cfg(test)]
    fn from_config(config: ProviderAdapterConfig) -> Self {
        Self {
            providers: ProviderModelAdapter::new(config),
            chatgpt_auth: crate::chatgpt_auth::ChatGptAuth::in_memory(),
        }
    }
}

async fn configured_model_call<T>(
    config: &AgentConfig,
    call: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    match config
        .settings
        .as_ref()
        .and_then(|settings| settings.timeout_ms)
    {
        Some(timeout_ms) => tokio::time::timeout(Duration::from_millis(timeout_ms), call)
            .await
            .map_err(|_| "Model call timed out".to_owned())?,
        None => call.await,
    }
}

#[async_trait]
impl ModelAdapter for RuntimeModelAdapter {
    fn provider(&self) -> &str {
        "runtime"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        configured_model_call(config, async {
            let provider = config
                .provider
                .as_deref()
                .map(str::trim)
                .filter(|provider| !provider.is_empty())
                .unwrap_or("deterministic")
                .to_ascii_lowercase();

            match provider.as_str() {
                "deterministic" | "test" => {
                    DeterministicModelAdapter.generate(config, request).await
                }
                "chatgpt" => {
                    let (access_token, account_id) = self.chatgpt_auth.usable_credential().await?;
                    anima_model_adapters::ChatGptResponsesAdapter::new(access_token, account_id)?
                        .generate(config, request)
                        .await
                }
                _ => self.providers.generate(config, request).await,
            }
        })
        .await
    }
    /// Every provider streams (spec §4.5, §12.4).
    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn anima_core::ModelStreamSink,
    ) -> Result<(), String> {
        configured_model_call(config, async {
            let provider = config
                .provider
                .as_deref()
                .map(str::trim)
                .filter(|provider| !provider.is_empty())
                .unwrap_or("deterministic")
                .to_ascii_lowercase();
            match provider.as_str() {
                "deterministic" | "test" => {
                    DeterministicModelAdapter
                        .stream(config, request, sink)
                        .await
                }
                "chatgpt" => {
                    let (access_token, account_id) = self.chatgpt_auth.usable_credential().await?;
                    anima_model_adapters::ChatGptResponsesAdapter::new(access_token, account_id)?
                        .stream(config, request, sink)
                        .await
                }
                _ => self.providers.stream(config, request, sink).await,
            }
        })
        .await
    }
}
