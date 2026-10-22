//! Fake native authority/callback fixtures; no live services.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;
use tokio::sync::Semaphore;
#[test]
fn selected_ids_reject_duplicates_paths_and_empty_selection() {
    assert!(selected_ids(vec![]).is_err());
    assert!(selected_ids(vec!["a".into(), "a".into()]).is_err());
    assert!(selected_ids(vec!["../secret".into()]).is_err());
}
struct FakeTransport {
    calls: StdMutex<Vec<String>>,
    bytes: StdMutex<Vec<u8>>,
    read_gate: AtomicBool,
    metadata_gate: AtomicBool,
    changed: AtomicBool,
    metadata_count: AtomicUsize,
    metadata_name: StdMutex<Option<String>>,
    entered: Semaphore,
    release: Semaphore,
    refresh_scopes: StdMutex<Option<Vec<String>>>,
}
impl Default for FakeTransport {
    fn default() -> Self {
        Self {
            calls: StdMutex::new(vec![]),
            bytes: StdMutex::new(b"selected text".to_vec()),
            read_gate: AtomicBool::new(false),
            metadata_gate: AtomicBool::new(false),
            changed: AtomicBool::new(false),
            metadata_count: AtomicUsize::new(0),
            metadata_name: StdMutex::new(None),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            refresh_scopes: StdMutex::new(None),
        }
    }
}
fn grant(expiry: u64) -> DriveTokenGrant {
    DriveTokenGrant {
        tokens: crate::connectors::gcalendar::store::GoogleOAuthTokens::new(
            "access-fixture".to_string().into(),
            "refresh-fixture".to_string().into(),
            expiry,
        ),
        granted_scopes: vec![DRIVE_SCOPE.into()],
    }
}
#[async_trait::async_trait]
impl DriveTransport for FakeTransport {
    async fn exchange_code(
        &self,
        _: &DriveOAuthConfig,
        _: &str,
        verifier: &str,
    ) -> Result<DriveTokenGrant, DriveError> {
        assert!((43..=128).contains(&verifier.len()));
        self.calls.lock().unwrap().push("exchange".into());
        Ok(grant(now_ms() + 3_600_000))
    }
    async fn refresh_tokens(
        &self,
        _: &DriveOAuthConfig,
        _: &str,
    ) -> Result<DriveTokenGrant, DriveError> {
        self.calls.lock().unwrap().push("refresh".into());
        let mut refreshed = grant(now_ms() + 3_600_000);
        if let Some(scopes) = self.refresh_scopes.lock().unwrap().clone() {
            refreshed.granted_scopes = scopes;
        }
        Ok(refreshed)
    }
    async fn identify(&self, _: &str) -> Result<DriveAccount, DriveError> {
        self.calls.lock().unwrap().push("identify".into());
        Ok(DriveAccount {
            permission_id: "account-fixture".into(),
            label: Some("Selected owner".into()),
        })
    }
    async fn metadata(&self, _: &str, id: &DriveFileId) -> Result<DriveFileMetadata, DriveError> {
        self.calls.lock().unwrap().push(format!("metadata:{id}"));
        let count = self.metadata_count.fetch_add(1, Ordering::SeqCst);
        if self.metadata_gate.load(Ordering::SeqCst) {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
        Ok(DriveFileMetadata {
            id: id.clone(),
            name: self
                .metadata_name
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| format!("literal {id} name")),
            mime_type: "text/plain".into(),
            version: if self.changed.load(Ordering::SeqCst) && count > 0 {
                "changed"
            } else {
                "first"
            }
            .into(),
            modified_time: Some("2026-10-12T00:00:00Z".into()),
            size_bytes: None,
            trashed: false,
            can_download: true,
            source_url: format!("https://drive.google.com/file/d/{id}/view"),
        })
    }
    async fn read_text(
        &self,
        _: &str,
        _: &DriveFileMetadata,
    ) -> Result<DriveTextBytes, DriveError> {
        self.calls.lock().unwrap().push("read".into());
        if self.read_gate.load(Ordering::SeqCst) {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
        Ok(DriveTextBytes {
            bytes: self.bytes.lock().unwrap().clone(),
            representation: "stored_plain_text".into(),
        })
    }
}
async fn setup() -> (DriveManager, SharedDaemonState, Arc<FakeTransport>, String) {
    let mut s = DaemonState::new();
    let agent = s
        .create_agent(anima_core::AgentConfig {
            name: "Anima".into(),
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
            settings: Some(anima_core::AgentSettings::default()),
            fallbacks: Vec::new(),
        })
        .unwrap()
        .state
        .id;
    let shared = Arc::new(RwLock::new(s));
    let apps = OAuthAppService::in_memory();
    apps.put(
        OAuthProvider::Google,
        crate::connectors::oauth_apps::OAuthAppCredentials::new(
            OAuthProvider::Google,
            "test-id",
            "test-secret",
            None,
        )
        .unwrap(),
    )
    .await
    .unwrap();
    let transport = Arc::new(FakeTransport::default());
    let manager = DriveManager::new(
        &shared,
        Arc::new(Mutex::new(())),
        Arc::new(store::InMemoryDriveCredentialStore::default()),
        transport.clone(),
        apps,
    );
    (manager, shared, transport, agent)
}
fn nonce(url: &str) -> String {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned()
}
async fn active(manager: &DriveManager, agent: &str) -> DriveRecord {
    let (_, url) = manager
        .begin_connect(agent.into(), vec!["selected-a".into()])
        .await
        .unwrap();
    manager
        .complete_connect(nonce(&url), Some("private-code".into()))
        .await
        .unwrap()
}
async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(3), future)
        .await
        .expect("bounded fixture")
}
#[tokio::test]
async fn selected_reads_and_searches_are_current_agent_grant_bound_and_never_enumerate() {
    let (manager, shared, transport, agent) = setup().await;
    active(&manager, &agent).await;
    transport.calls.lock().unwrap().clear();
    assert_eq!(
        manager
            .read_for_agent(&agent, "unselected-b")
            .await
            .unwrap_err(),
        DriveError::NotSelected
    );
    assert!(transport.calls.lock().unwrap().is_empty());
    let search = manager
        .search_for_agent(&agent, "' or trashed=false")
        .await
        .unwrap();
    assert!(search.files.is_empty());
    assert_eq!(
        *transport.calls.lock().unwrap(),
        vec!["metadata:selected-a"]
    );
    assert_eq!(
        manager
            .read_for_agent("another-agent", "selected-a")
            .await
            .unwrap_err(),
        DriveError::NotConnected
    );
    let mut s = shared.write().await;
    set_drive_tools(&mut s, &agent, false).unwrap();
    drop(s);
    assert_eq!(
        manager
            .read_for_agent(&agent, "selected-a")
            .await
            .unwrap_err(),
        DriveError::NotConnected
    );
}
#[tokio::test]
async fn callback_is_single_use_and_disconnected_pending_cannot_exchange_or_reactivate() {
    let (manager, _shared, transport, agent) = setup().await;
    let (record, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    let state = nonce(&url);
    manager
        .disconnect(agent.clone(), record.id.clone())
        .await
        .unwrap();
    assert!(manager
        .complete_connect(state.clone(), Some("private-code".into()))
        .await
        .is_err());
    assert!(manager
        .complete_connect(state, Some("private-code".into()))
        .await
        .is_err());
    assert!(transport.calls.lock().unwrap().is_empty());
    assert_eq!(
        manager.status(&agent).await.unwrap().1.unwrap().status,
        DriveStatus::Disconnected
    );
}
#[tokio::test]
async fn pairing_save_failure_returns_no_consent_and_rolls_back_only_its_tools_and_record() {
    let (manager, shared, transport, agent) = setup().await;
    let gate = shared
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let connecting = tokio::spawn({
        let manager = manager.clone();
        let agent = agent.clone();
        async move {
            manager
                .begin_connect(agent, vec!["selected-a".into()])
                .await
        }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert_eq!(
        within(connecting).await.unwrap().unwrap_err(),
        DriveError::Persistence
    );
    assert!(shared.read().await.drive_records.is_empty());
    assert!(manager.inner.pending.lock().await.is_empty());
    assert!(transport.calls.lock().unwrap().is_empty());
    assert!(shared
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config
        .tools
        .is_none());
}
#[tokio::test]
async fn activation_save_failure_never_leaves_live_tool_grants_or_written_generation() {
    let (manager, shared, transport, agent) = setup().await;
    let store = Arc::new(IntentPutStore {
        inner: store::InMemoryDriveCredentialStore::default(),
        shared: shared.clone(),
        put_error: false,
        save_error: true,
        delete_error: false,
    });
    let manager = DriveManager::new(
        &shared,
        manager.inner.transactions.clone(),
        store.clone(),
        transport,
        manager.inner.oauth_apps.clone(),
    );
    let (record, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    assert_eq!(
        manager
            .complete_connect(nonce(&url), Some("private-code".into()))
            .await
            .unwrap_err(),
        DriveError::Persistence
    );
    assert_eq!(
        shared.read().await.drive_records[&record.id].status,
        DriveStatus::ReauthRequired
    );
    assert!(store
        .load(&record.id, &record.generation)
        .await
        .unwrap()
        .is_none());
    assert!(!shared
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config
        .tools
        .unwrap_or_default()
        .iter()
        .any(|tool| DRIVE_TOOL_NAMES.contains(&tool.name.as_str())));
}
#[tokio::test]
async fn fetched_content_is_discarded_after_local_disconnect_and_no_remote_revoke_exists() {
    let (manager, _shared, transport, agent) = setup().await;
    let record = active(&manager, &agent).await;
    transport.read_gate.store(true, Ordering::SeqCst);
    let reading = tokio::spawn({
        let manager = manager.clone();
        let agent = agent.clone();
        async move { manager.read_for_agent(&agent, "selected-a").await }
    });
    within(transport.entered.acquire()).await.unwrap().forget();
    manager
        .disconnect(agent.clone(), record.id.clone())
        .await
        .unwrap();
    transport.release.add_permits(1);
    assert_eq!(
        within(reading).await.unwrap().unwrap_err(),
        DriveError::Conflict
    );
    assert_eq!(
        manager.status(&agent).await.unwrap().1.unwrap().status,
        DriveStatus::Disconnected
    );
    assert!(transport
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|call| !call.contains("revoke")));
}
#[tokio::test]
async fn faithful_scalar_truncation_keeps_exact_source_and_returned_digests_and_version_conflict_discards(
) {
    let (manager, _shared, transport, agent) = setup().await;
    active(&manager, &agent).await;
    let text = "\u{1f642}".repeat(32770);
    *transport.bytes.lock().unwrap() = text.as_bytes().to_vec();
    let DriveReadResult::Text(result) = manager.read_for_agent(&agent, "selected-a").await.unwrap()
    else {
        panic!("text expected")
    };
    assert_eq!(result.returned_scalars, 32768);
    assert!(result.truncated);
    assert_eq!(
        result.source_bytes_sha256,
        hex::encode(Sha256::digest(text.as_bytes()))
    );
    assert_eq!(
        result.returned_text_sha256,
        hex::encode(Sha256::digest(result.text.as_bytes()))
    );
    transport.metadata_count.store(0, Ordering::SeqCst);
    transport.changed.store(true, Ordering::SeqCst);
    assert_eq!(
        manager
            .read_for_agent(&agent, "selected-a")
            .await
            .unwrap_err(),
        DriveError::Conflict
    );
}
#[tokio::test]
async fn omitted_refresh_scope_inherits_verified_grant_but_reduced_scope_fails_closed() {
    let (manager, _shared, transport, agent) = setup().await;
    let record = active(&manager, &agent).await;
    manager
        .inner
        .credentials
        .put(&record.id, &record.generation, grant(1))
        .await
        .unwrap();
    *transport.refresh_scopes.lock().unwrap() = Some(vec![]);
    manager.read_for_agent(&agent, "selected-a").await.unwrap();
    let saved = manager
        .inner
        .credentials
        .load(&record.id, &record.generation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.granted_scopes, vec![DRIVE_SCOPE]);
    manager
        .inner
        .credentials
        .put(&record.id, &record.generation, grant(1))
        .await
        .unwrap();
    *transport.refresh_scopes.lock().unwrap() = Some(vec![
        "https://www.googleapis.com/auth/drive.metadata.readonly".into(),
    ]);
    assert_eq!(
        manager
            .read_for_agent(&agent, "selected-a")
            .await
            .unwrap_err(),
        DriveError::ReauthRequired
    );
}
#[tokio::test]
async fn restart_invalidates_private_pending_and_snapshot_contains_no_nonce_verifier_or_tokens() {
    let (manager, shared, _, agent) = setup().await;
    let (_, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    let snapshot = shared.read().await.control_plane_snapshot();
    let payload = serde_json::to_string(&snapshot).unwrap();
    assert!(!payload.contains(&nonce(&url)));
    assert!(!payload.contains("private-code"));
    assert!(!payload.contains("access-fixture"));
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.drive_records.values().next().unwrap().status,
        DriveStatus::ReauthRequired
    );
    let weak = manager.weak();
    drop(manager);
    assert!(weak.upgrade().is_none());
}

struct DenyLoadStore {
    inner: store::InMemoryDriveCredentialStore,
    loads: AtomicUsize,
}
#[async_trait::async_trait]
impl DriveCredentialStore for DenyLoadStore {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<DriveTokenGrant>, DriveError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.inner.load(id, generation).await
    }
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: DriveTokenGrant,
    ) -> Result<(), DriveError> {
        self.inner.put(id, generation, grant).await
    }
    async fn delete(&self, _: &str, _: &str) -> Result<(), DriveError> {
        Err(DriveError::Credential)
    }
}
#[tokio::test]
async fn unselected_read_does_not_load_vault_and_failed_cleanup_leaves_durable_local_revoke() {
    let (manager, shared, transport, agent) = setup().await;
    let record = active(&manager, &agent).await;
    let store = Arc::new(DenyLoadStore {
        inner: store::InMemoryDriveCredentialStore::default(),
        loads: AtomicUsize::new(0),
    });
    store
        .put(&record.id, &record.generation, grant(now_ms() + 3_600_000))
        .await
        .unwrap();
    let isolated = DriveManager::new(
        &shared,
        manager.inner.transactions.clone(),
        store.clone(),
        transport,
        manager.inner.oauth_apps.clone(),
    );
    assert_eq!(
        isolated
            .read_for_agent(&agent, "not-selected")
            .await
            .unwrap_err(),
        DriveError::NotSelected
    );
    assert_eq!(store.loads.load(Ordering::SeqCst), 0);
    isolated
        .disconnect(agent.clone(), record.id.clone())
        .await
        .unwrap();
    let revoked = shared.read().await.drive_records[&record.id].clone();
    assert_eq!(revoked.status, DriveStatus::Disconnected);
    assert_eq!(revoked.cleanup_generations, vec![record.generation]);
    assert!(!shared
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config
        .tools
        .unwrap_or_default()
        .iter()
        .any(|tool| DRIVE_TOOL_NAMES.contains(&tool.name.as_str())));
}
#[tokio::test]
async fn wrong_pinned_account_and_replaced_application_revision_refuse_callback_without_grants() {
    let (manager, shared, transport, agent) = setup().await;
    let record = active(&manager, &agent).await;
    manager.disconnect(agent.clone(), record.id).await.unwrap();
    let (mut pairing, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    pairing.account_permission_id = Some("different-account".into());
    shared
        .write()
        .await
        .drive_records
        .insert(pairing.id.clone(), pairing.clone());
    assert_eq!(
        manager
            .complete_connect(nonce(&url), Some("private-code".into()))
            .await
            .unwrap_err(),
        DriveError::Conflict
    );
    assert!(shared
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config
        .tools
        .unwrap_or_default()
        .iter()
        .all(|tool| !DRIVE_TOOL_NAMES.contains(&tool.name.as_str())));
    let (_, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    manager
        .inner
        .oauth_apps
        .put(
            OAuthProvider::Google,
            crate::connectors::oauth_apps::OAuthAppCredentials::new(
                OAuthProvider::Google,
                "new-id",
                "new-secret",
                None,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let before = transport.calls.lock().unwrap().len();
    assert_eq!(
        manager
            .complete_connect(nonce(&url), Some("private-code".into()))
            .await
            .unwrap_err(),
        DriveError::Conflict
    );
    assert_eq!(transport.calls.lock().unwrap().len(), before);
}

struct IntentPutStore {
    inner: store::InMemoryDriveCredentialStore,
    shared: SharedDaemonState,
    put_error: bool,
    save_error: bool,
    delete_error: bool,
}
#[async_trait::async_trait]
impl DriveCredentialStore for IntentPutStore {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<DriveTokenGrant>, DriveError> {
        self.inner.load(id, generation).await
    }
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: DriveTokenGrant,
    ) -> Result<(), DriveError> {
        assert!(
            self.shared.read().await.drive_records[id]
                .cleanup_generations
                .iter()
                .any(|g| g == generation),
            "captured cleanup intent precedes vault effect"
        );
        self.inner.put(id, generation, grant).await?;
        if self.save_error {
            let gate = self
                .shared
                .write()
                .await
                .install_test_control_plane_save_gate(true);
            gate.release.add_permits(1);
        }
        if self.put_error {
            Err(DriveError::Credential)
        } else {
            Ok(())
        }
    }
    async fn delete(&self, id: &str, generation: &str) -> Result<(), DriveError> {
        if self.delete_error {
            Err(DriveError::Credential)
        } else {
            self.inner.delete(id, generation).await
        }
    }
}
#[tokio::test]
async fn uncertain_put_and_failed_compensation_save_retain_durable_exact_generation_cleanup() {
    let (manager, shared, transport, agent) = setup().await;
    let store = Arc::new(IntentPutStore {
        inner: store::InMemoryDriveCredentialStore::default(),
        shared: shared.clone(),
        put_error: true,
        save_error: true,
        delete_error: true,
    });
    let manager = DriveManager::new(
        &shared,
        manager.inner.transactions.clone(),
        store.clone(),
        transport,
        manager.inner.oauth_apps.clone(),
    );
    let (record, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    assert_eq!(
        manager
            .complete_connect(nonce(&url), Some("private-code".into()))
            .await
            .unwrap_err(),
        DriveError::Credential
    );
    let snapshot = shared.read().await.control_plane_snapshot();
    assert_eq!(
        snapshot.drive_records[0].cleanup_generations,
        vec![record.generation.clone()]
    );
    assert_ne!(snapshot.drive_records[0].status, DriveStatus::Active);
    assert!(store
        .load(&record.id, &record.generation)
        .await
        .unwrap()
        .is_some());
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.drive_records[&record.id].cleanup_generations,
        vec![record.generation]
    );
}
#[tokio::test]
async fn full_pending_admission_never_revokes_active_record_or_grants() {
    let (manager, shared, _, agent) = setup().await;
    let record = active(&manager, &agent).await;
    let before = shared
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config
        .tools;
    let mut pending = manager.inner.pending.lock().await;
    for n in 0..64 {
        pending.insert(
            format!("nonce-{n}"),
            Pending {
                connector_id: format!("other-{n}"),
                agent_id: format!("other-agent-{n}"),
                generation: "generation".into(),
                selected_digest: "digest".into(),
                revision: record.oauth_config_revision,
                expires_at_ms: now_ms() + AUTH_TTL_MS,
                verifier: "private-verifier".to_string().into(),
            },
        );
    }
    drop(pending);
    assert_eq!(
        manager
            .begin_connect(agent.clone(), vec!["selected-a".into()])
            .await
            .unwrap_err(),
        DriveError::Conflict
    );
    assert_eq!(shared.read().await.drive_records[&record.id], record);
    assert_eq!(
        shared
            .read()
            .await
            .get_agent(&agent)
            .unwrap()
            .state
            .config
            .tools,
        before
    );
}
#[tokio::test]
async fn callback_expiry_is_rechecked_after_gate_wait_and_whole_budget_includes_the_wait() {
    let (manager, _shared, transport, agent) = setup().await;
    let (_, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    let state = nonce(&url);
    manager
        .inner
        .callback_clock_ms
        .store(1000, Ordering::SeqCst);
    manager
        .inner
        .pending
        .lock()
        .await
        .get_mut(&state)
        .unwrap()
        .expires_at_ms = 1001;
    let gate = manager.gate(&agent).await;
    let held = gate.lock().await;
    let completing = tokio::spawn({
        let manager = manager.clone();
        async move {
            manager
                .complete_connect(state, Some("private-code".into()))
                .await
        }
    });
    within(async {
        loop {
            if manager.inner.pending.lock().await.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    manager
        .inner
        .callback_clock_ms
        .store(1002, Ordering::SeqCst);
    drop(held);
    assert_eq!(
        within(completing).await.unwrap().unwrap_err(),
        DriveError::InvalidInput
    );
    assert!(transport.calls.lock().unwrap().is_empty());
    let (_, url) = manager
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    let gate = manager.gate(&agent).await;
    let _held = gate.lock().await;
    assert_eq!(
        manager
            .complete_owned(
                nonce(&url),
                Some("private-code".into()),
                tokio::time::Instant::now() + std::time::Duration::from_millis(30)
            )
            .await
            .unwrap_err(),
        DriveError::Deadline
    );
    assert!(transport.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn selected_search_enforces_32_kib_serialized_result_bound() {
    let (manager, _shared, transport, agent) = setup().await;
    let ids = (0..16).map(|n| format!("selected-{n}")).collect();
    let (_, url) = manager.begin_connect(agent.clone(), ids).await.unwrap();
    manager
        .complete_connect(nonce(&url), Some("private-code".into()))
        .await
        .unwrap();
    *transport.metadata_name.lock().unwrap() = Some("\u{1f642}".repeat(1024));
    assert_eq!(
        manager.search_for_agent(&agent, "").await.unwrap_err(),
        DriveError::TooLarge
    );
}
async fn deletion_services(
    manager: &DriveManager,
    shared: &SharedDaemonState,
    transport: Arc<FakeTransport>,
) -> (DriveManager, crate::connectors::runtime::ConnectorManager) {
    let runs =
        crate::agent_runs::AgentRunCoordinator::new(shared.clone(), Arc::new(Semaphore::new(4)));
    let drive = DriveManager::new(
        shared,
        runs.control_plane_transactions(),
        manager.inner.credentials.clone(),
        transport,
        manager.inner.oauth_apps.clone(),
    );
    shared.write().await.drive_manager = Some(drive.weak());
    let connectors = crate::connectors::runtime::ConnectorManager::new(
        shared.clone(),
        runs,
        Arc::new(crate::connectors::credentials::InMemoryCredentialStore::default()),
        Arc::new(crate::app::DeterministicTelegramTransport),
    );
    (drive, connectors)
}
#[tokio::test]
async fn final_agent_delete_atomically_tombstones_late_pairing_and_cleanup_with_scoped_rollback() {
    let (manager, shared, transport, agent) = setup().await;
    let (drive, connectors) = deletion_services(&manager, &shared, transport).await;
    let (pairing, _) = drive
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    let gate = shared
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let deleting = tokio::spawn({
        let connectors = connectors.clone();
        let agent = agent.clone();
        async move { connectors.delete_agent(agent).await }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    {
        let s = shared.read().await;
        assert!(s.get_agent(&agent).is_none());
        assert_eq!(
            s.drive_records[&pairing.id].status,
            DriveStatus::Disconnected
        );
        assert!(s.drive_records[&pairing.id]
            .cleanup_generations
            .contains(&pairing.generation));
    }
    gate.release.add_permits(1);
    assert!(within(deleting).await.unwrap().is_err());
    assert_eq!(shared.read().await.drive_records[&pairing.id], pairing);
    assert!(shared.read().await.get_agent(&agent).is_some());
    connectors.delete_agent(agent.clone()).await.unwrap();
    let snapshot = shared.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(restored.agent_count(), 0);
    assert_eq!(
        restored.drive_records[&pairing.id].status,
        DriveStatus::Disconnected
    );
}
#[tokio::test]
async fn full_generation_cleanup_queue_refuses_agent_delete_before_mutating_agent_or_record() {
    let (manager, shared, transport, agent) = setup().await;
    let (drive, connectors) = deletion_services(&manager, &shared, transport).await;
    let (mut record, _) = drive
        .begin_connect(agent.clone(), vec!["selected-a".into()])
        .await
        .unwrap();
    record.cleanup_generations = (0..16).map(|_| uuid::Uuid::new_v4().to_string()).collect();
    shared
        .write()
        .await
        .drive_records
        .insert(record.id.clone(), record.clone());
    assert!(connectors.delete_agent(agent.clone()).await.is_err());
    assert!(shared.read().await.get_agent(&agent).is_some());
    assert_eq!(shared.read().await.drive_records[&record.id], record);
}
