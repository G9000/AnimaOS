//! Mirrors only Anima's existing credential *read* layout. No OAuth refresh or writes.
use anima_harness::{
    anima_core::{
        execution_limits::ModelCallContext, AgentConfig, ModelAdapter, ModelGenerateRequest,
        ModelGenerateResponse, ModelStreamSink,
    },
    anima_model_adapters::ChatGptResponsesAdapter,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "anima.chatgpt.subscription.v1";
const VAULT_ERROR: &str = "Existing Anima ChatGPT credential vault is unavailable";
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
#[derive(Deserialize)]
struct Credential {
    access_token: String,
    account_id: String,
    expires_at_ms: u64,
}
impl Drop for Credential {
    fn drop(&mut self) {
        self.access_token.zeroize();
    }
}
#[derive(Deserialize)]
struct Manifest {
    version: u32,
    generation: String,
    chunks: usize,
}
#[derive(Deserialize)]
struct Stored {
    version: u32,
    generation: String,
    credential: Credential,
}

struct ReadOnlySubscription {
    service: String,
}
pub(super) fn adapter() -> Result<Arc<dyn ModelAdapter>, String> {
    let namespace = std::env::var("VILLAGE_KEYCHAIN_NAMESPACE")
        .ok()
        .or_else(|| std::env::var("ANIMAOS_RS_KEYCHAIN_NAMESPACE").ok());
    if namespace.as_ref().is_some_and(|n| {
        n.is_empty()
            || n.len() > 32
            || !n
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    }) {
        return Err("Invalid Anima keychain namespace".into());
    }
    let service = namespace
        .map(|n| format!("{n}.{SERVICE}"))
        .unwrap_or_else(|| SERVICE.into());
    let adapter = ReadOnlySubscription { service };
    adapter.transport()?;
    Ok(Arc::new(adapter))
}
fn vault(service: &str, account: &str) -> Result<Option<Zeroizing<String>>, String> {
    let entry = keyring::Entry::new(service, account).map_err(|_| VAULT_ERROR)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(Zeroizing::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(VAULT_ERROR.into()),
    }
}
fn read_credential(
    mut read: impl FnMut(&str) -> Result<Option<Zeroizing<String>>, String>,
) -> Result<Credential, String> {
    if !cfg!(windows) {
        if let Some(payload) = read("credential")? {
            if payload.len() > 65_536 {
                return Err(VAULT_ERROR.into());
            }
            let stored: Stored = serde_json::from_str(&payload).map_err(|_| VAULT_ERROR)?;
            if stored.version != 2 || uuid::Uuid::parse_str(&stored.generation).is_err() {
                return Err(VAULT_ERROR.into());
            }
            return validate(stored.credential);
        }
    }
    let manifest = read("account")?
        .ok_or("No existing Anima ChatGPT connection is available in this namespace")?;
    let manifest: Manifest = serde_json::from_str(&manifest).map_err(|_| VAULT_ERROR)?;
    if manifest.version != 1
        || manifest.chunks == 0
        || manifest.chunks > 132
        || uuid::Uuid::parse_str(&manifest.generation).is_err()
    {
        return Err(VAULT_ERROR.into());
    }
    let mut payload = Zeroizing::new(String::new());
    for index in 0..manifest.chunks {
        let chunk = read(&format!("{}.{index}", manifest.generation))?.ok_or(VAULT_ERROR)?;
        if chunk.len() > 2000 || payload.len() + chunk.len() > 65_536 {
            return Err(VAULT_ERROR.into());
        }
        payload.push_str(&chunk);
    }
    validate(serde_json::from_str(&payload).map_err(|_| VAULT_ERROR)?)
}
fn validate(credential: Credential) -> Result<Credential, String> {
    if credential.access_token.is_empty() || credential.account_id.is_empty() {
        return Err(VAULT_ERROR.into());
    }
    if credential.expires_at_ms <= now_ms() + 30_000 {
        return Err("Existing Anima ChatGPT access has expired. Renew the connection in Anima, then restart the village. The game does not refresh or change assistant credentials.".into());
    }
    Ok(credential)
}
impl ReadOnlySubscription {
    fn transport(&self) -> Result<ChatGptResponsesAdapter, String> {
        let credential = read_credential(|account| vault(&self.service, account))?;
        ChatGptResponsesAdapter::new(
            credential.access_token.clone(),
            credential.account_id.clone(),
        )
        .map_err(|_| "Cannot initialize existing ChatGPT transport".into())
    }
}
#[async_trait]
impl ModelAdapter for ReadOnlySubscription {
    fn provider(&self) -> &str {
        "chatgpt"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.transport()?.generate(config, request).await
    }
    async fn generate_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        self.transport()?
            .generate_controlled(config, request, context)
            .await
    }
    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &ModelCallContext,
    ) -> Result<(), String> {
        self.transport()?
            .stream_controlled(config, request, sink, context)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_harness::{anima_core::Content, Harness};
    #[tokio::test]
    async fn harness_controlled_stream_reaches_read_only_credential_resolution() {
        let adapter = ReadOnlySubscription {
            service: format!("anima-village-test-{}", uuid::Uuid::new_v4()),
        };
        let mut harness = Harness::builder()
            .model("fixture")
            .adapter(Arc::new(adapter))
            .max_tool_iterations(0)
            .build()
            .unwrap();
        let result = harness.run("hello").await;
        assert!(result
            .data
            .as_ref()
            .is_none_or(|c: &Content| c.text.is_empty()));
        let error = result.error.unwrap();
        assert!(
            !error.contains("must explicitly support controlled"),
            "{error}"
        );
        assert!(
            error.contains("credential vault") || error.contains("No existing Anima"),
            "{error}"
        );
    }
}
