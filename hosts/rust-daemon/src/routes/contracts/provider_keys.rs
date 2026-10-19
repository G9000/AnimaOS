//! The provider key pool over HTTP. Keys only ever appear masked.
use serde::Serialize;
use utoipa::ToSchema;

use crate::provider_keys::{KeySource, KeyView, ProviderKeysView, Strategy};

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderKeyResponse {
    /// `env` for the key from the environment, else `key_` and 12 characters.
    pub(crate) id: String,
    pub(crate) label: Option<String>,
    /// The first six and last four characters, for example `sk-pro…abcd`.
    pub(crate) masked: String,
    pub(crate) source: KeySource,
    /// `ready` or `resting`.
    pub(crate) status: String,
    pub(crate) resting_until_ms: Option<u64>,
    pub(crate) last_error: Option<String>,
    pub(crate) uses: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderKeysResponse {
    pub(crate) id: String,
    pub(crate) strategy: Strategy,
    /// False when the daemon has no key file, so added keys last only until
    /// restart.
    pub(crate) persisted: bool,
    pub(crate) keys: Vec<ProviderKeyResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ProviderKeysEnvelope {
    pub(crate) providers: Vec<ProviderKeysResponse>,
}

impl From<KeyView> for ProviderKeyResponse {
    fn from(view: KeyView) -> Self {
        Self {
            id: view.id,
            label: view.label,
            masked: view.masked,
            source: view.source,
            status: view.status.to_owned(),
            resting_until_ms: view.resting_until_ms,
            last_error: view.last_error,
            uses: view.uses,
        }
    }
}

impl From<ProviderKeysView> for ProviderKeysResponse {
    fn from(view: ProviderKeysView) -> Self {
        Self {
            id: view.id,
            strategy: view.strategy,
            persisted: view.persisted,
            keys: view.keys.into_iter().map(Into::into).collect(),
        }
    }
}
