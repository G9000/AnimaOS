//! Ephemeral current-principal catalog. Metadata never selects a model or runs tools.
use super::{ChatGptAuth, Credential, Manifest, VAULT_ERROR};
use async_trait::async_trait;
use serde::Serialize;
use std::{sync::atomic::Ordering, time::Duration};
use tokio::time::Instant;

const UNAVAILABLE: &str = "Model metadata is unavailable.";
const TIMEOUT: &str = "Model metadata request timed out.";
const INVALID: &str = "Model metadata response is invalid.";
const TTL: Duration = Duration::from_secs(300);
const DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatGptModelEntry {
    pub(crate) id: String,
}
#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatGptModelsSnapshot {
    pub(crate) state: &'static str,
    pub(crate) connection_revision: Option<String>,
    pub(crate) catalog_revision: Option<String>,
    pub(crate) fetched_at_ms: Option<u64>,
    pub(crate) expires_at_ms: Option<u64>,
    pub(crate) models: Vec<ChatGptModelEntry>,
    pub(crate) error: Option<&'static str>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelsError {
    Busy,
    Vault,
}

// Private stamp is deliberately neither Serialize nor Debug. Revisions exposed
// to owners are independent random IDs, never credential/account fingerprints.
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    generation: u64,
    manifest: String,
    account: String,
}
pub(super) struct AuthorizedModels {
    stamp: Stamp,
    pub(super) credential: Credential,
}
impl AuthorizedModels {
    pub(super) fn new(generation: u64, manifest: Manifest, credential: Credential) -> Self {
        Self {
            stamp: Stamp {
                generation,
                manifest: manifest.generation,
                account: credential.account_id.clone(),
            },
            credential,
        }
    }
}

pub(super) struct ModelsCache {
    epoch: u64,
    stamp: Option<Stamp>,
    snapshot: ChatGptModelsSnapshot,
    expires: Option<Instant>,
    operation: Option<String>,
}
impl Default for ModelsCache {
    fn default() -> Self {
        Self {
            epoch: 0,
            stamp: None,
            snapshot: ChatGptModelsSnapshot {
                state: "disconnected",
                connection_revision: None,
                catalog_revision: None,
                fetched_at_ms: None,
                expires_at_ms: None,
                models: vec![],
                error: None,
            },
            expires: None,
            operation: None,
        }
    }
}
impl ModelsCache {
    fn reconcile(&mut self, epoch: u64, stamp: Option<Stamp>) {
        if self.epoch == epoch && self.stamp == stamp {
            return;
        }
        *self = Self::default();
        self.epoch = epoch;
        if stamp.is_some() {
            self.snapshot.state = "unrequested";
            self.snapshot.connection_revision = Some(uuid::Uuid::new_v4().to_string());
        }
        self.stamp = stamp;
    }
    fn view(&mut self) -> ChatGptModelsSnapshot {
        if matches!(self.snapshot.state, "ready" | "empty")
            && self.expires.is_some_and(|expiry| Instant::now() >= expiry)
        {
            self.snapshot.state = "stale";
        }
        self.snapshot.clone()
    }
    fn fail(&mut self, error: &'static str) {
        self.snapshot.state = if self.snapshot.catalog_revision.is_some() {
            "stale"
        } else {
            "unavailable"
        };
        self.snapshot.error = Some(error);
        self.operation = None;
    }
}

#[async_trait]
pub(super) trait ModelsTransport: Send + Sync {
    async fn fetch(&self, token: String, account: String) -> Result<Vec<String>, String>;
}
pub(super) struct OfficialModelsTransport;
#[async_trait]
impl ModelsTransport for OfficialModelsTransport {
    async fn fetch(&self, token: String, account: String) -> Result<Vec<String>, String> {
        anima_model_adapters::ChatGptModelsAdapter::new(token, account)?
            .fetch()
            .await
    }
}
pub(super) struct UnavailableModelsTransport;
#[async_trait]
impl ModelsTransport for UnavailableModelsTransport {
    async fn fetch(&self, _: String, _: String) -> Result<Vec<String>, String> {
        Err(UNAVAILABLE.into())
    }
}

impl ChatGptAuth {
    fn models_stamp(&self, generation: u64) -> Result<Option<Stamp>, ModelsError> {
        let loaded = self.load_with_manifest().map_err(|_| ModelsError::Vault)?;
        let Some((manifest, credential)) = loaded else {
            return Ok(None);
        };
        // The immutable chunks belong to the captured manifest. A replacement
        // during those reads must not authorize dispatch or cache publication.
        if self
            .manifest()
            .map_err(|_| ModelsError::Vault)?
            .as_ref()
            .map(|current| &current.generation)
            != Some(&manifest.generation)
        {
            return Err(ModelsError::Vault);
        }
        Ok(Some(Stamp {
            generation,
            manifest: manifest.generation,
            account: credential.account_id.clone(),
        }))
    }
    pub(crate) async fn models(&self) -> Result<ChatGptModelsSnapshot, ModelsError> {
        let lifecycle = self.lifecycle.lock().await;
        let stamp = match self.models_stamp(lifecycle.generation) {
            Ok(stamp) => stamp,
            Err(error) => {
                *self.models_cache.lock().await = ModelsCache::default();
                return Err(error);
            }
        };
        let mut cache = self.models_cache.lock().await;
        cache.reconcile(self.models_epoch.load(Ordering::Acquire), stamp);
        Ok(cache.view())
    }
    // Lock order is lifecycle -> catalog cache; no caller holds either over HTTP.
    async fn begin_models(&self, expected: &Stamp) -> Result<Option<String>, ModelsError> {
        let lifecycle = self.lifecycle.lock().await;
        let current = match self.models_stamp(lifecycle.generation) {
            Ok(current) => current,
            Err(error) => {
                *self.models_cache.lock().await = ModelsCache::default();
                return Err(error);
            }
        };
        let mut cache = self.models_cache.lock().await;
        cache.reconcile(self.models_epoch.load(Ordering::Acquire), current.clone());
        if current.as_ref() != Some(expected) {
            return Ok(None);
        }
        let operation = uuid::Uuid::new_v4().to_string();
        cache.operation = Some(operation.clone());
        Ok(Some(operation))
    }
    #[cfg(test)]
    async fn finish_models(
        &self,
        expected: &Stamp,
        operation: Option<&str>,
        result: Result<Vec<String>, &'static str>,
    ) -> Result<ChatGptModelsSnapshot, ModelsError> {
        self.finish_models_checked(expected, operation, result, None)
            .await
    }
    async fn finish_models_checked(
        &self,
        expected: &Stamp,
        operation: Option<&str>,
        result: Result<Vec<String>, &'static str>,
        deadline: Option<Instant>,
    ) -> Result<ChatGptModelsSnapshot, ModelsError> {
        let lifecycle = self.lifecycle.lock().await;
        let current = match self.models_stamp(lifecycle.generation) {
            Ok(current) => current,
            Err(error) => {
                *self.models_cache.lock().await = ModelsCache::default();
                return Err(error);
            }
        };
        let mut cache = self.models_cache.lock().await;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(Self::models_timeout_snapshot());
        }
        cache.reconcile(self.models_epoch.load(Ordering::Acquire), current.clone());
        if current.as_ref() != Some(expected)
            || operation.is_some_and(|op| cache.operation.as_deref() != Some(op))
        {
            return Ok(cache.view());
        }
        match result {
            Ok(ids) => {
                let now = super::now_ms();
                cache.snapshot.state = if ids.is_empty() { "empty" } else { "ready" };
                cache.snapshot.catalog_revision = Some(uuid::Uuid::new_v4().to_string());
                cache.snapshot.fetched_at_ms = Some(now);
                cache.snapshot.expires_at_ms = Some(now.saturating_add(TTL.as_millis() as u64));
                cache.snapshot.models =
                    ids.into_iter().map(|id| ChatGptModelEntry { id }).collect();
                cache.snapshot.error = None;
                cache.expires = Some(Instant::now() + TTL);
                cache.operation = None;
            }
            Err(error) => cache.fail(error),
        }
        Ok(cache.view())
    }
    pub(crate) async fn refresh_models(&self) -> Result<ChatGptModelsSnapshot, ModelsError> {
        self.refresh_models_until(Instant::now() + DEADLINE).await
    }
    async fn refresh_models_until(
        &self,
        deadline: Instant,
    ) -> Result<ChatGptModelsSnapshot, ModelsError> {
        let _single = self
            .models_refresh
            .try_lock()
            .map_err(|_| ModelsError::Busy)?;
        let result = tokio::time::timeout_at(deadline, self.refresh_models_work(deadline)).await;
        match result {
            Ok(result) if Instant::now() < deadline => result,
            _ => {
                // Invalidate synchronously before releasing single-flight. The
                // next cache access discards uncertain rows under its own lock;
                // no cleanup future can later erase a newer explicit refresh.
                self.models_epoch.fetch_add(1, Ordering::AcqRel);
                Ok(Self::models_timeout_snapshot())
            }
        }
    }
    fn models_timeout_snapshot() -> ChatGptModelsSnapshot {
        let mut snapshot = ModelsCache::default().snapshot;
        snapshot.state = "unavailable";
        snapshot.error = Some(TIMEOUT);
        snapshot
    }
    async fn refresh_models_work(
        &self,
        deadline: Instant,
    ) -> Result<ChatGptModelsSnapshot, ModelsError> {
        if Instant::now() >= deadline {
            return Ok(Self::models_timeout_snapshot());
        }
        let initial = {
            let lifecycle = self.lifecycle.lock().await;
            match self.models_stamp(lifecycle.generation) {
                Ok(stamp) => stamp,
                Err(error) => {
                    *self.models_cache.lock().await = ModelsCache::default();
                    return Err(error);
                }
            }
        };
        let Some(initial) = initial else {
            return self.models().await;
        };
        if Instant::now() >= deadline {
            return Ok(Self::models_timeout_snapshot());
        }
        let context = match self.usable_context(true).await {
            Ok(Some(context)) => context,
            Ok(None) => return self.models().await,
            Err(error) if error == VAULT_ERROR => {
                *self.models_cache.lock().await = ModelsCache::default();
                return Err(ModelsError::Vault);
            }
            Err(_) => {
                return self
                    .finish_models_checked(&initial, None, Err(UNAVAILABLE), Some(deadline))
                    .await
            }
        };
        let Some(operation) = self.begin_models(&context.stamp).await? else {
            return self.models().await;
        };
        if Instant::now() >= deadline {
            return Ok(Self::models_timeout_snapshot());
        }
        let fetched = self
            .models_transport
            .fetch(
                context.credential.access_token.clone(),
                context.credential.account_id.clone(),
            )
            .await;
        let result = match fetched {
            Ok(ids) => Ok(ids),
            Err(error) if error == INVALID => Err(INVALID),
            Err(error) if error == TIMEOUT => Err(TIMEOUT),
            Err(_) => Err(UNAVAILABLE),
        };
        self.finish_models_checked(&context.stamp, Some(&operation), result, Some(deadline))
            .await
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
