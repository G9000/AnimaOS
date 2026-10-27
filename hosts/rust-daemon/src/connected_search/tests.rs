use super::service::{SourceReader, SourceSnapshot};
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{Mutex, Notify, RwLock};

fn document(id: &str, text: &str) -> IndexedDocument {
    IndexedDocument {
        citation_id: String::new(),
        external_id: id.into(),
        collection_id: "inbox".into(),
        kind: DocumentKind::Mail,
        title: "Invoice".into(),
        text: text.into(),
        sender: Some("accounts@example.test".into()),
        source_url: None,
        source_modified_at: None,
        event_start: None,
        event_end: None,
        revision: String::new(),
        truncated: false,
        indexed_at_ms: 0,
    }
}

#[test]
fn snapshots_suppress_baseline_and_retained_window_reentry() {
    let authority = SourceAuthority {
        agent_id: "a".into(),
        connector_id: "mail-a".into(),
        provider: "gmail".into(),
        generation: "g1".into(),
        enabled: true,
        read_granted: true,
        sync_started_at_ms: 10,
    };
    let mut source = SourceIndex::new(&authority);
    assert!(apply_snapshot(
        &mut source,
        &authority,
        vec![document("one", "invoice eighty")],
        20
    )
    .unwrap()
    .is_empty());
    let changes = apply_snapshot(
        &mut source,
        &authority,
        vec![document("two", "invoice ninety")],
        30,
    )
    .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, SourceChangeKind::Observed);
    assert!(
        apply_snapshot(
            &mut source,
            &authority,
            vec![document("one", "invoice eighty")],
            40
        )
        .unwrap()
        .is_empty(),
        "old items reentering retention do not start workflows"
    );
    let changes = apply_snapshot(
        &mut source,
        &authority,
        vec![document("one", "invoice changed")],
        50,
    )
    .unwrap();
    assert_eq!(changes[0].kind, SourceChangeKind::Changed);
}

#[test]
fn normalization_bounds_utf8_and_uses_opaque_ids_as_values() {
    let authority = SourceAuthority {
        agent_id: "a".into(),
        connector_id: "mail-a".into(),
        provider: "email".into(),
        generation: "g1".into(),
        enabled: true,
        read_granted: true,
        sync_started_at_ms: 10,
    };
    let mut source = SourceIndex::new(&authority);
    apply_snapshot(
        &mut source,
        &authority,
        vec![document("../opaque:message", &"猫".repeat(4000))],
        20,
    )
    .unwrap();
    let item = &source.documents[0];
    assert!(item.text.len() <= MAX_TEXT_BYTES);
    assert!(item.truncated);
    assert_eq!(item.citation_id.len(), 64);
    assert!(!item.citation_id.contains('/'));
}

#[test]
fn failed_capacity_does_not_mutate_the_previous_snapshot() {
    let authority = SourceAuthority {
        agent_id: "a".into(),
        connector_id: "mail-a".into(),
        provider: "gmail".into(),
        generation: "g1".into(),
        enabled: true,
        read_granted: true,
        sync_started_at_ms: 10,
    };
    let mut source = SourceIndex::new(&authority);
    apply_snapshot(
        &mut source,
        &authority,
        vec![document("one", "original")],
        20,
    )
    .unwrap();
    source.seen = (0..MAX_SEEN_ITEMS)
        .map(|i| (format!("seen-{i}"), "revision".into()))
        .collect();
    let before = source.clone();
    assert!(matches!(
        apply_snapshot(
            &mut source,
            &authority,
            vec![document("new", "changed")],
            30
        ),
        Err(SearchError::Capacity)
    ));
    assert_eq!(source, before);
}

pub(super) struct FixtureReader {
    fail: AtomicBool,
    block_first: AtomicBool,
    entered: Notify,
    release: Notify,
    epoch: std::sync::Mutex<Option<String>>,
}
impl Default for FixtureReader {
    fn default() -> Self {
        Self {
            fail: AtomicBool::new(false),
            block_first: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
            epoch: std::sync::Mutex::new(None),
        }
    }
}
#[async_trait::async_trait]
impl SourceReader for FixtureReader {
    async fn read(&self, a: &SourceAuthority) -> Result<SourceSnapshot, SearchError> {
        if self.block_first.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(SearchError::Unavailable);
        }
        let mut d = document(
            &format!("{}-opaque-id", a.provider),
            &format!("Full invoice body {} amount 778", a.provider),
        );
        d.kind = match a.provider.as_str() {
            "gdrive" => DocumentKind::File,
            "gcalendar" => DocumentKind::Event,
            _ => DocumentKind::Mail,
        };
        Ok(SourceSnapshot {
            documents: vec![d],
            stream_epoch: self.epoch.lock().unwrap().clone(),
        })
    }
}
pub(super) fn fixture() -> (
    crate::app::SharedDaemonState,
    String,
    Arc<FixtureReader>,
    SearchService,
) {
    let mut state = DaemonState::new();
    let agent = state
        .create_agent(
            serde_json::from_value(
                serde_json::json!({"name":"search-owner","model":"deterministic"}),
            )
            .unwrap(),
        )
        .unwrap()
        .state
        .id;
    let tools = state
        .tool_registry
        .resolve_descriptors(["mail_list_messages", "drive_read", "calendar_list_events"])
        .unwrap();
    state
        .update_agent(
            &agent,
            anima_core::AgentConfigUpdate {
                tools: Some(tools),
                ..Default::default()
            },
        )
        .unwrap();
    for provider in [
        crate::connectors::mail::Provider::Gmail,
        crate::connectors::mail::Provider::Outlook,
        crate::connectors::mail::Provider::Email,
    ] {
        let id = format!("mail-{}", provider.name());
        state.mail_records.insert(
            id.clone(),
            crate::connectors::mail::MailRecord {
                connector: crate::connectors::mail::MailConnector {
                    id,
                    agent_id: agent.clone(),
                    provider,
                    account_label: Some(format!("{}@example.test", provider.name())),
                    status: "active".into(),
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    last_synced_at_ms: None,
                    error: None,
                    email: None,
                },
                messages: vec![],
                deleted: false,
                auth_generation: "mail-grant".into(),
                email: None,
            },
        );
    }
    state.drive_records.insert(
        "drive-source".into(),
        crate::connectors::drive::model::DriveRecord {
            id: "drive-source".into(),
            agent_id: agent.clone(),
            status: crate::connectors::drive::model::DriveStatus::Active,
            generation: "drive-grant".into(),
            selected_file_ids: vec![crate::connectors::drive::model::DriveFileId::parse(
                "selected",
            )
            .unwrap()],
            account_permission_id: Some("account".into()),
            account_label: Some("owner@example.test".into()),
            oauth_config_revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
            cleanup_generations: vec![],
        },
    );
    state.calendar_connectors.insert(
        "calendar-source".into(),
        crate::connectors::gcalendar::GoogleCalendarConnectorRecord {
            id: "calendar-source".into(),
            agent_id: agent.clone(),
            authorization_generation: "calendar-grant".into(),
            account_label: Some("owner@example.test".into()),
            calendar_ids: vec!["primary".into()],
            pending_auth: None,
            reauth_required: false,
            enabled: true,
            deleted_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        },
    );
    let state = Arc::new(RwLock::new(state));
    let reader = Arc::new(FixtureReader::default());
    let service = SearchService::with_reader(&state, Arc::new(Mutex::new(())), reader.clone());
    (state, agent, reader, service)
}
pub(super) async fn enable(service: &SearchService, agent: &str, id: &str) {
    let view = service
        .sources(agent)
        .await
        .unwrap()
        .sources
        .into_iter()
        .find(|s| s.connector_id == id)
        .unwrap();
    service
        .configure(
            agent,
            id,
            SourceInput {
                enabled: true,
                expected_generation: view.generation,
            },
        )
        .await
        .unwrap();
}
fn invoice_query() -> SearchQuery {
    SearchQuery {
        query: "invoice 778".into(),
        connector_ids: vec![],
        kinds: vec![],
        limit: Some(30),
    }
}

#[tokio::test]
async fn all_native_sources_search_full_text_with_private_scoped_citations_and_restore() {
    let (state, agent, _, service) = fixture();
    for id in [
        "mail-gmail",
        "mail-outlook",
        "mail-email",
        "drive-source",
        "calendar-source",
    ] {
        enable(&service, &agent, id).await;
    }
    service.refresh(&agent, None).await.unwrap();
    let response = service.query(&agent, &invoice_query()).await.unwrap();
    assert_eq!(response.hits.len(), 5);
    let citation = response.hits[0].citation_id.clone();
    assert!(service
        .item(&agent, &citation)
        .await
        .unwrap()
        .text
        .contains("Full invoice body"));
    assert!(service.item("other-agent", &citation).await.is_err());
    let snapshot = state.read().await.control_plane_snapshot();
    let serialized = serde_json::to_vec(&snapshot).unwrap();
    let snapshot = serde_json::from_slice(&serialized).unwrap();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        query(&restored, &agent, &invoice_query())
            .unwrap()
            .hits
            .len(),
        5
    );
}

#[tokio::test]
async fn failed_refresh_preserves_snapshot_but_reports_stale_and_removed_grants_purge() {
    let (state, agent, reader, service) = fixture();
    enable(&service, &agent, "mail-gmail").await;
    service.refresh(&agent, None).await.unwrap();
    reader.fail.store(true, Ordering::SeqCst);
    let response = service.refresh(&agent, None).await.unwrap();
    let source = response
        .sources
        .iter()
        .find(|s| s.connector_id == "mail-gmail")
        .unwrap();
    assert_eq!(source.status, "stale");
    assert!(source.error.is_some());
    assert_eq!(
        service
            .query(&agent, &invoice_query())
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    {
        let mut state = state.write().await;
        state
            .update_agent(
                &agent,
                anima_core::AgentConfigUpdate {
                    tools: Some(vec![]),
                    ..Default::default()
                },
            )
            .unwrap();
        let _ = state.control_plane_persist_request();
        assert!(state.connected_search.sources.is_empty());
    }
    assert!(service
        .query(&agent, &invoice_query())
        .await
        .unwrap()
        .hits
        .is_empty());
}

#[tokio::test]
async fn disabling_and_reenabling_during_a_read_rejects_the_previous_enrollment() {
    let (state, agent, reader, service) = fixture();
    enable(&service, &agent, "mail-gmail").await;
    reader.block_first.store(true, Ordering::SeqCst);
    let worker = {
        let service = service.clone();
        let agent = agent.clone();
        tokio::spawn(async move { service.refresh(&agent, Some("mail-gmail")).await })
    };
    reader.entered.notified().await;
    let view = service
        .sources(&agent)
        .await
        .unwrap()
        .sources
        .into_iter()
        .find(|s| s.connector_id == "mail-gmail")
        .unwrap();
    service
        .configure(
            &agent,
            "mail-gmail",
            SourceInput {
                enabled: false,
                expected_generation: view.generation,
            },
        )
        .await
        .unwrap();
    enable(&service, &agent, "mail-gmail").await;
    reader.release.notify_one();
    worker.await.unwrap().unwrap();
    let guard = state.read().await;
    let source = &guard.connected_search.sources["mail-gmail"];
    assert!(source.documents.is_empty());
    assert!(source.baseline_at_ms.is_none());
    drop(guard);
    service.refresh(&agent, None).await.unwrap();
    assert_eq!(
        service
            .query(&agent, &invoice_query())
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[tokio::test]
async fn persistence_failure_rolls_back_index_and_search_tool_grant() {
    let (state, agent, _, service) = fixture();
    let temporary = tempfile::tempdir().unwrap();
    let blocking = temporary.path().join("file");
    std::fs::write(&blocking, b"not a directory").unwrap();
    let before = {
        let mut state = state.write().await;
        state.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(blocking.join("state.json")),
        ));
        state.get_agent(&agent).unwrap().state.config.tools
    };
    let generation = service
        .sources(&agent)
        .await
        .unwrap()
        .sources
        .into_iter()
        .find(|s| s.connector_id == "mail-gmail")
        .unwrap()
        .generation;
    assert!(matches!(
        service
            .configure(
                &agent,
                "mail-gmail",
                SourceInput {
                    enabled: true,
                    expected_generation: generation
                }
            )
            .await,
        Err(SearchError::Persistence)
    ));
    let guard = state.read().await;
    assert!(guard.connected_search.sources.is_empty());
    assert_eq!(guard.get_agent(&agent).unwrap().state.config.tools, before);
}

#[tokio::test]
async fn imap_mailbox_identity_change_rebaselines_without_retaining_old_seen_ids() {
    let (state, agent, reader, service) = fixture();
    *reader.epoch.lock().unwrap() = Some("uidvalidity-1".into());
    enable(&service, &agent, "mail-email").await;
    service.refresh(&agent, None).await.unwrap();
    {
        let mut guard = state.write().await;
        guard
            .connected_search
            .sources
            .get_mut("mail-email")
            .unwrap()
            .seen
            .insert("f".repeat(64), "a".repeat(64));
    }
    *reader.epoch.lock().unwrap() = Some("uidvalidity-2".into());
    service.refresh(&agent, None).await.unwrap();
    let guard = state.read().await;
    let source = &guard.connected_search.sources["mail-email"];
    assert_eq!(source.stream_epoch.as_deref(), Some("uidvalidity-2"));
    assert_eq!(source.seen.len(), 1);
}
