use std::collections::BTreeMap;
use std::sync::Arc;

use anima_core::{ModelFailure, ModelFailureKind};

mod adapter;
mod anthropic;
mod attachments;
mod catalog;
mod chatgpt;
mod common;
mod controlled;
mod google;
mod models;
mod ollama;
mod openai_compatible;
mod speech;
mod stream;
mod transcription;
mod transport;

pub use adapter::ProviderModelAdapter;
pub use attachments::{image_reference_text, model_supports_images};
pub use catalog::provider_definitions;
pub use chatgpt::{
    ChatGptCatalogDispatch, ChatGptModelsAdapter, ChatGptModelsFailure, ChatGptModelsReason,
    ChatGptModelsStage, ChatGptResponsesAdapter,
};
pub use models::{
    canonical_provider_id, estimate_cost_micros, model_info, model_table, price_usage,
    CostEstimate, ModelInfo, ModelPricing, PRICING_TABLE_DATE,
};
pub use speech::OpenAiSpeechAdapter;
pub use stream::DeterministicModelAdapter;
pub use transcription::OpenAiTranscriptionAdapter;

#[derive(Clone)]
pub struct ProviderCredential {
    pub api_key: Option<String>,
    pub base_url: String,
}

/// Where a provider call gets its API key from, one pick per attempt.
///
/// `pick` returns `(key_id, key)` for a key that is ready to use. After the
/// attempt the adapter calls `report` with that id: `failure` is `None` when
/// the call succeeded, else the classified error. A failure that blames the
/// key makes the adapter pick again within the same call.
pub trait KeySource: Send + Sync {
    fn pick(&self, provider: &str) -> Option<(String, String)>;
    fn report(&self, provider: &str, key_id: &str, failure: Option<&ModelFailure>);
    /// Whether the source holds any key for `provider`, ready or resting. A
    /// `pick` of `None` for such a provider means every key is resting, not
    /// that the provider has no key.
    fn holds_keys(&self, _provider: &str) -> bool {
        false
    }

    /// A ready key other than those in `tried`. The default asks `pick` again a
    /// bounded number of times; a source that can exclude keys should override
    /// it so an untried ready key is found even when a tried one woke up.
    fn pick_untried(&self, provider: &str, tried: &[String]) -> Option<(String, String)> {
        for _ in 0..=tried.len() {
            match self.pick(provider) {
                Some((key_id, _)) if tried.contains(&key_id) => continue,
                picked => return picked,
            }
        }
        None
    }

    /// Why every key held for `provider` is resting, and until when; `None`
    /// when some key is ready or the source does not say.
    fn resting(&self, _provider: &str) -> Option<KeysResting> {
        None
    }
}

/// Why a provider's keys are all resting, for the error a call reports
/// without sending anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeysResting {
    /// What put the keys to rest: `RateLimited`, `Billing` or `AuthFailed`.
    pub kind: ModelFailureKind,
    /// Seconds until the first key is ready again.
    pub ready_in_s: u64,
    /// That moment as the host shows clock times, for example `"14:05"`.
    pub ready_at: Option<String>,
}

#[derive(Clone, Default)]
pub struct ProviderAdapterConfig {
    pub providers: BTreeMap<String, ProviderCredential>,
    /// When set, calls take their key from here instead of
    /// `providers[..].api_key` (which stays the fallback when the source has no
    /// key for the provider).
    pub key_source: Option<Arc<dyn KeySource>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub aliases: &'static [&'static str],
    pub requires_key: bool,
    pub api_key_envs: &'static [&'static str],
    pub base_url_envs: &'static [&'static str],
    pub default_base_url: &'static str,
}

impl std::fmt::Debug for ProviderCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderCredential")
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl std::fmt::Debug for ProviderAdapterConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderAdapterConfig")
            .field("providers", &self.providers)
            .field("key_source", &self.key_source.as_ref().map(|_| "[set]"))
            .finish()
    }
}

#[cfg(test)]
mod attachments_tests;
#[cfg(test)]
mod tests;
