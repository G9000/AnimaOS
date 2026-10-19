use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anima_core::{
    AgentConfig, AgentConfigUpdate, AgentSettings, Content, DataValue, ModelAdapter,
    ModelGenerateRequest, ModelGenerateResponse, ModelStopReason, TokenUsage,
};
use async_trait::async_trait;
use tokio::sync::{RwLock, Semaphore};

use super::{ConnectorManager, ConnectorRuntimeStatus, TelegramTransport};
use crate::agent_runs::{AgentRunCoordinator, AgentRunRequest, RunRoom};
use crate::app::SharedDaemonState;
use crate::connectors::credentials::{
    ConnectorCredentialStore, CredentialStoreError, InMemoryCredentialStore, TelegramBotToken,
};
use crate::connectors::telegram::{
    TelegramSentMessage, TelegramTextUpdate, TelegramTransportError, TelegramUpdateBatch,
};
use crate::connectors::{
    InboundProcessingState, TelegramBotIdentity, TelegramChatKind, TelegramChatMetadata,
    TelegramSenderMetadata,
};
use crate::runs::RunSource;
use crate::state::DaemonState;

#[derive(Default)]
struct FakeTransport {
    calls: Mutex<Vec<&'static str>>,
    sent: Mutex<Vec<(String, String)>>,
    poll_calls: AtomicUsize,
    remaining_poll_failures: AtomicUsize,
    remaining_send_failures: AtomicUsize,
    revoked_poll: AtomicBool,
    revoked_send: AtomicBool,
}

#[derive(Default)]
struct UncertainDeleteStore {
    inner: InMemoryCredentialStore,
}

#[derive(Default)]
struct UncertainLoadStore;

#[derive(Default)]
struct FailingCredentialStore {
    inner: InMemoryCredentialStore,
    fail_next_load: AtomicBool,
    fail_next_put: AtomicBool,
}

struct GateLoadCredentialStore {
    inner: InMemoryCredentialStore,
    gate_load: AtomicBool,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

struct GatePutCredentialStore {
    inner: InMemoryCredentialStore,
    gate_put: AtomicBool,
    fail_put: AtomicBool,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

struct GatePutUncertainDeleteStore {
    inner: InMemoryCredentialStore,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
    fail_next_delete: AtomicBool,
}

struct GateModelAdapter {
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

struct DropAwarePollingTransport {
    entered: Arc<Semaphore>,
    exited: Arc<Semaphore>,
}

struct ReplacementRaceCredentialStore {
    inner: InMemoryCredentialStore,
    fixed_worker_delivery: Arc<AtomicBool>,
    worker_load_entered: Arc<Semaphore>,
    replacement_put_done: Arc<Semaphore>,
    load_calls: AtomicUsize,
    put_calls: AtomicUsize,
}

struct ReplacementRaceTransport {
    fixed_worker_delivery: Arc<AtomicBool>,
    send_entered: Arc<Semaphore>,
    release_send: Arc<Semaphore>,
    gate_first_send: AtomicBool,
    sent_tokens: Mutex<Vec<String>>,
}

struct InboundBackpressureTransport {
    enabled: AtomicBool,
    fail_delivery: AtomicBool,
    delivery_attempts: Arc<Semaphore>,
    update: TelegramTextUpdate,
}

struct PollDropGuard(Arc<Semaphore>);

impl Drop for PollDropGuard {
    fn drop(&mut self) {
        self.0.add_permits(1);
    }
}

#[async_trait]
impl ModelAdapter for GateModelAdapter {
    fn provider(&self) -> &str {
        "gate"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.entered.add_permits(1);
        self.release
            .acquire()
            .await
            .expect("release semaphore should remain open")
            .forget();
        Ok(ModelGenerateResponse {
            content: Content {
                text: "gated response".into(),
                ..Content::default()
            },
            tool_calls: None,
            usage: TokenUsage::default(),
            stop_reason: ModelStopReason::End,
        })
    }
}

#[async_trait]
impl ConnectorCredentialStore for UncertainDeleteStore {
    async fn load(
        &self,
        connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        self.inner.load(connector_id).await
    }

    async fn put(
        &self,
        connector_id: &str,
        token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        self.inner.put(connector_id, token).await
    }

    async fn delete(&self, _connector_id: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::CredentialStateUncertain)
    }
}

#[async_trait]
impl ConnectorCredentialStore for UncertainLoadStore {
    async fn load(
        &self,
        _connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        Err(CredentialStoreError::CredentialStateUncertain)
    }

    async fn put(
        &self,
        _connector_id: &str,
        _token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        Ok(())
    }

    async fn delete(&self, _connector_id: &str) -> Result<(), CredentialStoreError> {
        Ok(())
    }
}

#[async_trait]
impl ConnectorCredentialStore for FailingCredentialStore {
    async fn load(
        &self,
        connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        if self.fail_next_load.swap(false, Ordering::SeqCst) {
            return Err(CredentialStoreError::BackendUnavailable);
        }
        self.inner.load(connector_id).await
    }

    async fn put(
        &self,
        connector_id: &str,
        token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        if self.fail_next_put.swap(false, Ordering::SeqCst) {
            return Err(CredentialStoreError::BackendUnavailable);
        }
        self.inner.put(connector_id, token).await
    }

    async fn delete(&self, connector_id: &str) -> Result<(), CredentialStoreError> {
        self.inner.delete(connector_id).await
    }
}

#[async_trait]
impl ConnectorCredentialStore for GateLoadCredentialStore {
    async fn load(
        &self,
        connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        if self.gate_load.load(Ordering::SeqCst) {
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| CredentialStoreError::OperationCancelled)?
                .forget();
        }
        self.inner.load(connector_id).await
    }

    async fn put(
        &self,
        connector_id: &str,
        token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        self.inner.put(connector_id, token).await
    }

    async fn delete(&self, connector_id: &str) -> Result<(), CredentialStoreError> {
        self.inner.delete(connector_id).await
    }
}

#[async_trait]
impl ConnectorCredentialStore for GatePutCredentialStore {
    async fn load(
        &self,
        connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        self.inner.load(connector_id).await
    }

    async fn put(
        &self,
        connector_id: &str,
        token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        if self.gate_put.swap(false, Ordering::SeqCst) {
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| CredentialStoreError::OperationCancelled)?
                .forget();
        }
        if self.fail_put.swap(false, Ordering::SeqCst) {
            return Err(CredentialStoreError::BackendUnavailable);
        }
        self.inner.put(connector_id, token).await
    }

    async fn delete(&self, connector_id: &str) -> Result<(), CredentialStoreError> {
        self.inner.delete(connector_id).await
    }
}

#[async_trait]
impl ConnectorCredentialStore for GatePutUncertainDeleteStore {
    async fn load(
        &self,
        connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        self.inner.load(connector_id).await
    }

    async fn put(
        &self,
        connector_id: &str,
        token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        self.entered.add_permits(1);
        self.release
            .acquire()
            .await
            .map_err(|_| CredentialStoreError::OperationCancelled)?
            .forget();
        self.inner.put(connector_id, token).await
    }

    async fn delete(&self, connector_id: &str) -> Result<(), CredentialStoreError> {
        if self.fail_next_delete.swap(false, Ordering::SeqCst) {
            return Err(CredentialStoreError::CredentialStateUncertain);
        }
        self.inner.delete(connector_id).await
    }
}

#[async_trait]
impl TelegramTransport for FakeTransport {
    async fn get_me(
        &self,
        token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        self.calls.lock().unwrap().push("get_me");
        Ok(TelegramBotIdentity {
            id: token.expose().split(':').next().unwrap_or("bot").into(),
            username: Some("anima_bot".into()),
            display_name: Some("Anima".into()),
        })
    }

    async fn get_updates(
        &self,
        _token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        self.poll_calls.fetch_add(1, Ordering::SeqCst);
        if self.revoked_poll.swap(false, Ordering::SeqCst) {
            return Err(TelegramTransportError::UpstreamApi { code: Some(401) });
        }
        if self
            .remaining_poll_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(TelegramTransportError::Transport);
        }
        Ok(TelegramUpdateBatch {
            updates: Vec::new(),
            next_update_id: offset,
        })
    }

    async fn send_message(
        &self,
        _token: &TelegramBotToken,
        chat_id: &str,
        text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        if self.revoked_send.swap(false, Ordering::SeqCst) {
            return Err(TelegramTransportError::HttpStatus { status: 401 });
        }
        if self
            .remaining_send_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(TelegramTransportError::Transport);
        }
        self.sent
            .lock()
            .unwrap()
            .push((chat_id.to_string(), text.to_string()));
        Ok(Vec::new())
    }
}

#[async_trait]
impl TelegramTransport for DropAwarePollingTransport {
    async fn get_me(
        &self,
        _token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        Ok(TelegramBotIdentity {
            id: "42".into(),
            username: Some("drop_bot".into()),
            display_name: Some("Drop Bot".into()),
        })
    }

    async fn get_updates(
        &self,
        _token: &TelegramBotToken,
        _offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        let _guard = PollDropGuard(Arc::clone(&self.exited));
        self.entered.add_permits(1);
        std::future::pending().await
    }

    async fn send_message(
        &self,
        _token: &TelegramBotToken,
        _chat_id: &str,
        _text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ConnectorCredentialStore for ReplacementRaceCredentialStore {
    async fn load(
        &self,
        connector_id: &str,
    ) -> Result<Option<TelegramBotToken>, CredentialStoreError> {
        if self.fixed_worker_delivery.load(Ordering::SeqCst) {
            return self.inner.load(connector_id).await;
        }
        if self.load_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.worker_load_entered.add_permits(1);
            self.replacement_put_done
                .acquire()
                .await
                .map_err(|_| CredentialStoreError::OperationCancelled)?
                .forget();
        }
        self.inner.load(connector_id).await
    }

    async fn put(
        &self,
        connector_id: &str,
        token: TelegramBotToken,
    ) -> Result<(), CredentialStoreError> {
        self.inner.put(connector_id, token).await?;
        if self.put_calls.fetch_add(1, Ordering::SeqCst) > 0 {
            self.replacement_put_done.add_permits(2);
        }
        Ok(())
    }

    async fn delete(&self, connector_id: &str) -> Result<(), CredentialStoreError> {
        self.inner.delete(connector_id).await
    }
}

#[async_trait]
impl TelegramTransport for ReplacementRaceTransport {
    async fn get_me(
        &self,
        token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        Ok(TelegramBotIdentity {
            id: token.expose().split(':').next().unwrap_or("bot").into(),
            username: Some("race_bot".into()),
            display_name: Some("Race Bot".into()),
        })
    }

    async fn get_updates(
        &self,
        _token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        Ok(TelegramUpdateBatch {
            updates: Vec::new(),
            next_update_id: offset,
        })
    }

    async fn send_message(
        &self,
        token: &TelegramBotToken,
        _chat_id: &str,
        _text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        self.fixed_worker_delivery.store(true, Ordering::SeqCst);
        self.send_entered.add_permits(2);
        if self.gate_first_send.swap(false, Ordering::SeqCst) {
            self.release_send
                .acquire()
                .await
                .map_err(|_| TelegramTransportError::Transport)?
                .forget();
        }
        self.sent_tokens
            .lock()
            .unwrap()
            .push(token.expose().to_string());
        Ok(Vec::new())
    }
}

#[async_trait]
impl TelegramTransport for InboundBackpressureTransport {
    async fn get_me(
        &self,
        _token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        Ok(TelegramBotIdentity {
            id: "42".into(),
            username: Some("bounded_bot".into()),
            display_name: Some("Bounded Bot".into()),
        })
    }

    async fn get_updates(
        &self,
        _token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        if self.enabled.load(Ordering::SeqCst) && offset <= self.update.update_id {
            Ok(TelegramUpdateBatch {
                updates: vec![self.update.clone()],
                next_update_id: self.update.update_id + 1,
            })
        } else {
            Ok(TelegramUpdateBatch {
                updates: Vec::new(),
                next_update_id: offset,
            })
        }
    }

    async fn send_message(
        &self,
        _token: &TelegramBotToken,
        _chat_id: &str,
        _text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        self.delivery_attempts.add_permits(1);
        if self.fail_delivery.load(Ordering::SeqCst) {
            Err(TelegramTransportError::Transport)
        } else {
            Ok(Vec::new())
        }
    }
}

mod lifecycle;
mod processing;
mod startup;
mod transactions;
mod workers;

fn invalid_snapshot_directory(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "anima-connector-{label}-invalid-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn text_update(update_id: i64, chat_id: &str, text: &str) -> TelegramTextUpdate {
    TelegramTextUpdate {
        update_id,
        text: text.into(),
        sender: TelegramSenderMetadata {
            id: "sender-1".into(),
            username: Some("operator".into()),
            display_name: Some("Operator".into()),
        },
        chat: TelegramChatMetadata {
            id: chat_id.into(),
            kind: TelegramChatKind::Private,
            title: None,
            username: Some("operator".into()),
        },
    }
}

fn state_with_agent() -> SharedDaemonState {
    let mut state = DaemonState::new();
    state.create_agent(test_config()).unwrap();
    Arc::new(RwLock::new(state))
}

fn manager(
    state: SharedDaemonState,
    credentials: Arc<dyn ConnectorCredentialStore>,
    transport: Arc<dyn TelegramTransport>,
) -> ConnectorManager {
    let runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(4)));
    ConnectorManager::new(state, runs, credentials, transport)
}

async fn wait_for_status(
    manager: &ConnectorManager,
    connector_id: &str,
    expected: ConnectorRuntimeStatus,
    timeout: Duration,
) -> bool {
    tokio::time::timeout(timeout, async {
        loop {
            if manager.status(connector_id).await == Some(expected) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

fn test_config() -> AgentConfig {
    AgentConfig {
        name: "operator".into(),
        model: "deterministic".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: None,
        system: None,
        tools: None,
        plugins: None,
        settings: Some(AgentSettings::default()),
        fallbacks: Vec::new(),
    }
}
include!("../attachment_tests.rs");

mod delivery;
mod main_chat;
