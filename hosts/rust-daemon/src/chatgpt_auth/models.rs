//! Ephemeral current-principal catalog. Metadata never selects a model or runs tools.
use super::{ChatGptAuth, Credential, Manifest, VAULT_ERROR};
use anima_model_adapters::{
    ChatGptCatalogDispatch as Dispatch, ChatGptModelsFailure, ChatGptModelsReason as Reason,
    ChatGptModelsStage as Stage,
};
use async_trait::async_trait;
use serde::Serialize;
use std::{sync::atomic::Ordering, time::Duration};
use tokio::time::Instant;

const UNAVAILABLE: &str = "Model metadata is unavailable.";
const TIMEOUT: &str = "Model metadata request timed out.";
const INVALID: &str = "Model metadata response is invalid.";
const TTL: Duration = Duration::from_secs(300);
const DEADLINE: Duration = Duration::from_secs(10);

// Returned only to this invocation, never retained in a cache or public DTO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ModelsDiagnostic {
    stage: Stage,
    catalog_dispatch: Dispatch,
    reason: Reason,
    http_status: Option<u16>,
}
impl ModelsDiagnostic {
    fn new(
        stage: Stage,
        catalog_dispatch: Dispatch,
        reason: Reason,
        http_status: Option<u16>,
    ) -> Self {
        Self {
            stage,
            catalog_dispatch,
            reason,
            http_status,
        }
    }
    fn before(stage: Stage, reason: Reason) -> Self {
        Self::new(stage, Dispatch::NotStarted, reason, None)
    }
    fn complete() -> Self {
        Self::new(
            Stage::Complete,
            Dispatch::ResponseReceived,
            Reason::None,
            None,
        )
    }
    fn deadline() -> Self {
        Self::new(Stage::Deadline, Dispatch::Unknown, Reason::Timeout, None)
    }
    fn scope(mut self, reason: Reason) -> Self {
        self.stage = Stage::Scope;
        self.reason = reason;
        self
    }
    fn public_message(self) -> &'static str {
        match self.reason {
            Reason::Timeout => TIMEOUT,
            Reason::InvalidCatalog => INVALID,
            _ => UNAVAILABLE,
        }
    }
}
impl From<ChatGptModelsFailure> for ModelsDiagnostic {
    fn from(failure: ChatGptModelsFailure) -> Self {
        Self::new(
            failure.stage(),
            failure.catalog_dispatch(),
            failure.reason(),
            failure.http_status(),
        )
    }
}
struct ModelsOutcome {
    result: Result<ChatGptModelsSnapshot, ModelsError>,
    diagnostic: ModelsDiagnostic,
}
impl ModelsOutcome {
    fn new(
        result: Result<ChatGptModelsSnapshot, ModelsError>,
        diagnostic: ModelsDiagnostic,
    ) -> Self {
        Self { result, diagnostic }
    }
    fn timeout() -> Self {
        Self::new(
            Ok(ChatGptAuth::models_timeout_snapshot()),
            ModelsDiagnostic::deadline(),
        )
    }
}
pub(super) enum ModelsFetchFailure {
    Core(ChatGptModelsFailure),
    Unavailable,
    #[cfg(test)]
    Synthetic(ModelsDiagnostic),
}
impl ModelsFetchFailure {
    fn diagnostic(self) -> ModelsDiagnostic {
        match self {
            Self::Core(failure) => failure.into(),
            Self::Unavailable => ModelsDiagnostic::before(Stage::Control, Reason::None),
            #[cfg(test)]
            Self::Synthetic(diagnostic) => diagnostic,
        }
    }
}

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
    async fn fetch(
        &self,
        token: String,
        account: String,
    ) -> Result<Vec<String>, ModelsFetchFailure>;
}
pub(super) struct OfficialModelsTransport;
#[async_trait]
impl ModelsTransport for OfficialModelsTransport {
    async fn fetch(
        &self,
        token: String,
        account: String,
    ) -> Result<Vec<String>, ModelsFetchFailure> {
        anima_model_adapters::ChatGptModelsAdapter::new_detailed(token, account)
            .map_err(ModelsFetchFailure::Core)?
            .fetch_detailed()
            .await
            .map_err(ModelsFetchFailure::Core)
    }
}
pub(super) struct UnavailableModelsTransport;
#[async_trait]
impl ModelsTransport for UnavailableModelsTransport {
    async fn fetch(&self, _: String, _: String) -> Result<Vec<String>, ModelsFetchFailure> {
        Err(ModelsFetchFailure::Unavailable)
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
        self.finish_models_checked(
            expected,
            operation,
            result,
            None,
            ModelsDiagnostic::complete(),
        )
        .await
        .result
    }
    async fn finish_models_checked(
        &self,
        expected: &Stamp,
        operation: Option<&str>,
        result: Result<Vec<String>, &'static str>,
        deadline: Option<Instant>,
        diagnostic: ModelsDiagnostic,
    ) -> ModelsOutcome {
        let lifecycle = self.lifecycle.lock().await;
        let current = match self.models_stamp(lifecycle.generation) {
            Ok(current) => current,
            Err(error) => {
                *self.models_cache.lock().await = ModelsCache::default();
                return ModelsOutcome::new(Err(error), diagnostic.scope(Reason::Vault));
            }
        };
        let mut cache = self.models_cache.lock().await;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return ModelsOutcome::timeout();
        }
        cache.reconcile(self.models_epoch.load(Ordering::Acquire), current.clone());
        if current.as_ref() != Some(expected)
            || operation.is_some_and(|op| cache.operation.as_deref() != Some(op))
        {
            return ModelsOutcome::new(Ok(cache.view()), diagnostic.scope(Reason::ScopeChanged));
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
        ModelsOutcome::new(Ok(cache.view()), diagnostic)
    }
    pub(crate) async fn refresh_models(&self) -> Result<ChatGptModelsSnapshot, ModelsError> {
        self.refresh_models_until(Instant::now() + DEADLINE).await
    }
    async fn refresh_models_until(
        &self,
        deadline: Instant,
    ) -> Result<ChatGptModelsSnapshot, ModelsError> {
        let outcome = self.refresh_models_diagnostic_until(deadline).await;
        let _ = outcome.diagnostic; // Ordinary refresh deliberately discards request-local evidence.
        outcome.result
    }
    #[cfg(test)]
    async fn refresh_models_diagnostic(&self) -> ModelsOutcome {
        self.refresh_models_diagnostic_until(Instant::now() + DEADLINE)
            .await
    }
    async fn refresh_models_diagnostic_until(&self, deadline: Instant) -> ModelsOutcome {
        let _single = match self.models_refresh.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                return ModelsOutcome::new(
                    Err(ModelsError::Busy),
                    ModelsDiagnostic::before(Stage::Control, Reason::Busy),
                )
            }
        };
        let result = tokio::time::timeout_at(deadline, self.refresh_models_work(deadline)).await;
        match result {
            Ok(result) if Instant::now() < deadline => result,
            _ => {
                // Invalidate synchronously before releasing single-flight. The
                // next cache access discards uncertain rows under its own lock;
                // no cleanup future can later erase a newer explicit refresh.
                self.models_epoch.fetch_add(1, Ordering::AcqRel);
                ModelsOutcome::timeout()
            }
        }
    }
    fn models_timeout_snapshot() -> ChatGptModelsSnapshot {
        let mut snapshot = ModelsCache::default().snapshot;
        snapshot.state = "unavailable";
        snapshot.error = Some(TIMEOUT);
        snapshot
    }
    async fn refresh_models_work(&self, deadline: Instant) -> ModelsOutcome {
        if Instant::now() >= deadline {
            return ModelsOutcome::timeout();
        }
        let initial = {
            let lifecycle = self.lifecycle.lock().await;
            match self.models_stamp(lifecycle.generation) {
                Ok(stamp) => stamp,
                Err(error) => {
                    *self.models_cache.lock().await = ModelsCache::default();
                    return ModelsOutcome::new(
                        Err(error),
                        ModelsDiagnostic::before(Stage::Authorization, Reason::Vault),
                    );
                }
            }
        };
        let Some(initial) = initial else {
            return self
                .reconcile_models_outcome(ModelsDiagnostic::before(
                    Stage::Authorization,
                    Reason::Disconnected,
                ))
                .await;
        };
        if Instant::now() >= deadline {
            return ModelsOutcome::timeout();
        }
        let context = match self.usable_context(true).await {
            Ok(Some(context)) => context,
            Ok(None) => {
                return self
                    .reconcile_models_outcome(ModelsDiagnostic::before(
                        Stage::Authorization,
                        Reason::Disconnected,
                    ))
                    .await
            }
            Err(error) if error == VAULT_ERROR => {
                *self.models_cache.lock().await = ModelsCache::default();
                return ModelsOutcome::new(
                    Err(ModelsError::Vault),
                    ModelsDiagnostic::before(Stage::Authorization, Reason::Vault),
                );
            }
            Err(_) => {
                return self
                    .finish_models_checked(
                        &initial,
                        None,
                        Err(UNAVAILABLE),
                        Some(deadline),
                        ModelsDiagnostic::before(Stage::Authorization, Reason::Credential),
                    )
                    .await
            }
        };
        let operation = match self.begin_models(&context.stamp).await {
            Ok(Some(operation)) => operation,
            Ok(None) => {
                return self
                    .reconcile_models_outcome(ModelsDiagnostic::before(
                        Stage::Scope,
                        Reason::ScopeChanged,
                    ))
                    .await
            }
            Err(error) => {
                return ModelsOutcome::new(
                    Err(error),
                    ModelsDiagnostic::before(Stage::Scope, Reason::Vault),
                )
            }
        };
        if Instant::now() >= deadline {
            return ModelsOutcome::timeout();
        }
        let fetched = self
            .models_transport
            .fetch(
                context.credential.access_token.clone(),
                context.credential.account_id.clone(),
            )
            .await;
        let (result, diagnostic) = match fetched {
            Ok(ids) => (Ok(ids), ModelsDiagnostic::complete()),
            Err(failure) => {
                let diagnostic = failure.diagnostic();
                (Err(diagnostic.public_message()), diagnostic)
            }
        };
        self.finish_models_checked(
            &context.stamp,
            Some(&operation),
            result,
            Some(deadline),
            diagnostic,
        )
        .await
    }
    async fn reconcile_models_outcome(&self, mut diagnostic: ModelsDiagnostic) -> ModelsOutcome {
        let result = self.models().await;
        match &result {
            Err(_) => diagnostic = diagnostic.scope(Reason::Vault),
            Ok(snapshot)
                if diagnostic.reason == Reason::Disconnected
                    && snapshot.state != "disconnected" =>
            {
                diagnostic = diagnostic.scope(Reason::ScopeChanged)
            }
            _ => {}
        }
        ModelsOutcome::new(result, diagnostic)
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
