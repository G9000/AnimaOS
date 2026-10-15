use std::collections::BTreeMap;

mod adapter;
mod anthropic;
mod attachments;
mod catalog;
mod chatgpt;
mod common;
mod google;
mod models;
mod ollama;
mod openai_compatible;
mod stream;
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
pub use stream::DeterministicModelAdapter;

#[derive(Clone)]
pub struct ProviderCredential {
    pub api_key: Option<String>,
    pub base_url: String,
}

#[derive(Clone, Default)]
pub struct ProviderAdapterConfig {
    pub providers: BTreeMap<String, ProviderCredential>,
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
            .finish()
    }
}

#[cfg(test)]
mod attachments_tests;
#[cfg(test)]
mod tests;
