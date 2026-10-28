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
fn restored_history_requires_matching_keys_and_import_receipts() {
    let authority = SourceAuthority {
        agent_id: "agent".into(),
        connector_id: "mail".into(),
        provider: "gmail".into(),
        generation: "generation".into(),
        enabled: true,
        read_granted: true,
        sync_started_at_ms: 1,
    };
    let mut source = SourceIndex::new(&authority);
    let item = normalize(document("message", "private text"), &authority, 1).unwrap();
    source
        .imported
        .insert(item.citation_id.clone(), item.revision.clone());
    source
        .historical
        .insert(item.citation_id.clone(), item.clone());
    let mut registry = SearchRegistry::default();
    registry
        .sources
        .insert(authority.connector_id.clone(), source);
    assert!(validate_snapshot(&registry).is_ok());
    let source = registry.sources.get_mut(&authority.connector_id).unwrap();
    source.historical.remove(&item.citation_id);
    source.historical.insert("mismatched-key".into(), item);
    assert!(validate_snapshot(&registry).is_err());
}

#[test]
fn history_pagination_cycle_stops_after_restart_without_workflow_events() {
    let authority = SourceAuthority {
        agent_id: "agent".into(),
        connector_id: "mail".into(),
        provider: "gmail".into(),
        generation: "generation".into(),
        enabled: true,
        read_granted: true,
        sync_started_at_ms: 1,
    };
    let mut source = SourceIndex::new(&authority);
    source.history = Some(HistoryProgress {
        coverage: HistoryCoverage {
            requested_from_ms: Some(0),
            requested_before_ms: Some(10_000),
            ..Default::default()
        },
        cursor: None,
        pass_id: "pass".into(),
        verification_cursor: None,
        cursor_fingerprints: vec![],
    });
    let page = |token: &str| HistoryPage {
        documents: vec![],
        next_cursor: Some(ImportCursor::Gmail {
            page_token: token.into(),
        }),
        exhausted: false,
    };
    apply_history_page(&mut source, &authority, page("A"), 1).unwrap();
    apply_history_page(&mut source, &authority, page("B"), 2).unwrap();
    let mut restored: SourceIndex =
        serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
    apply_history_page(&mut restored, &authority, page("A"), 3).unwrap();
    let progress = restored.history.unwrap();
    assert_eq!(progress.coverage.state, HistoryStatus::Partial);
    assert_eq!(progress.coverage.reason.as_deref(), Some("paginationCycle"));
    assert!(progress.cursor.is_none());
    assert!(restored.seen.is_empty());
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
fn history_import_is_silent_while_new_live_mail_is_admitted_once() {
    let authority = SourceAuthority {
        agent_id: "a".into(),
        connector_id: "mail-a".into(),
        provider: "gmail".into(),
        generation: "g1".into(),
        enabled: true,
        read_granted: true,
        sync_started_at_ms: 1_000,
    };
    let mut source = SourceIndex::new(&authority);
    apply_snapshot(&mut source, &authority, vec![], 1_000).unwrap();
    source.live_event_fence_at_ms = Some(2_000);
    source.settings.history_depth = HistoryDepth::Days30;
    source.history = Some(HistoryProgress {
        coverage: HistoryCoverage {
            state: HistoryStatus::Pending,
            requested_from_ms: Some(0),
            requested_before_ms: Some(2_000),
            ..Default::default()
        },
        cursor: None,
        pass_id: "pass".into(),
        verification_cursor: None,
        cursor_fingerprints: vec![],
    });
    let mut old = document("old", "historical invoice");
    old.source_modified_at = Some("1970-01-01T00:00:01Z".into());
    apply_history_page(
        &mut source,
        &authority,
        HistoryPage {
            documents: vec![old.clone()],
            next_cursor: None,
            exhausted: true,
        },
        3_000,
    )
    .unwrap();
    assert!(source.seen.is_empty(), "backfill never enters live dedupe");
    let mut new = document("new", "new invoice");
    new.source_modified_at = Some("1970-01-01T00:00:03Z".into());
    let changes = apply_snapshot(
        &mut source,
        &authority,
        vec![old.clone(), new.clone()],
        4_000,
    )
    .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].document.external_id, "new");
    assert_eq!(changes[0].observed_at_ms, 4_000);
    assert!(
        apply_snapshot(&mut source, &authority, vec![old.clone(), new], 5_000)
            .unwrap()
            .is_empty()
    );
    let old_citation = normalize(old.clone(), &authority, 1).unwrap().citation_id;
    source.seen.remove(&old_citation);
    source.historical.clear();
    assert!(
        apply_snapshot(&mut source, &authority, vec![old], 6_000)
            .unwrap()
            .is_empty(),
        "import receipt survives retained-text eviction"
    );
}

#[tokio::test]
async fn attachment_extraction_keeps_the_parent_live_revision_unchanged() {
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
    let raw_parent = document("parent", "original mail");
    apply_snapshot(&mut source, &authority, vec![raw_parent.clone()], 20).unwrap();
    let parent = source.documents[0].clone();
    let attachment = attachments::extract_attachment(
        &authority,
        &parent,
        RawAttachment {
            id: "part-1".into(),
            name: "invoice.txt".into(),
            mime: "text/plain".into(),
            bytes: Some(b"invoice attachment 983".to_vec()),
            status: "ready".into(),
        },
        30,
    )
    .await
    .unwrap();
    assert_eq!(attachment.parent_citation_id, parent.citation_id);
    assert!(attachment.document.text.contains("983"));
    source
        .attachment_scanned
        .insert(parent.citation_id.clone(), parent.revision.clone());
    source
        .attachments
        .insert(attachment.document.citation_id.clone(), attachment);
    assert!(
        apply_snapshot(&mut source, &authority, vec![raw_parent], 40)
            .unwrap()
            .is_empty()
    );
    assert_eq!(source.documents[0].revision, parent.revision);
    assert_eq!(source.attachments.len(), 1);
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
    history_page: std::sync::Mutex<Option<HistoryPage>>,
    inbox_ids: std::sync::Mutex<HashSet<String>>,
    attachment_unavailable: AtomicBool,
}
impl Default for FixtureReader {
    fn default() -> Self {
        Self {
            fail: AtomicBool::new(false),
            block_first: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
            epoch: std::sync::Mutex::new(None),
            history_page: std::sync::Mutex::new(None),
            inbox_ids: std::sync::Mutex::new(HashSet::new()),
            attachment_unavailable: AtomicBool::new(false),
        }
    }
}
#[async_trait::async_trait]
impl SourceReader for FixtureReader {
    async fn attachments(
        &self,
        _: &SourceAuthority,
        _: &IndexedDocument,
    ) -> Result<AttachmentBatch, SearchError> {
        let unavailable = self.attachment_unavailable.load(Ordering::SeqCst);
        Ok(AttachmentBatch {
            items: vec![RawAttachment {
                id: "attachment".into(),
                name: "note.txt".into(),
                mime: "text/plain".into(),
                bytes: (!unavailable).then(|| b"retry succeeded".to_vec()),
                status: if unavailable { "unavailable" } else { "ready" }.into(),
            }],
            omitted: 0,
        })
    }
    async fn read_history(
        &self,
        _: &SourceAuthority,
        _: &HistoryPageRequest,
    ) -> Result<HistoryPage, SearchError> {
        self.history_page
            .lock()
            .unwrap()
            .clone()
            .ok_or(SearchError::Unavailable)
    }
    async fn inbox_membership(
        &self,
        _: &SourceAuthority,
        ids: &[String],
    ) -> Result<Vec<bool>, SearchError> {
        let present = self.inbox_ids.lock().unwrap();
        Ok(ids.iter().map(|id| present.contains(id)).collect())
    }
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
            selection_revision: None,
            incomplete_count: 0,
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
                expected_settings_revision: None,
                settings: None,
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
        query(&restored, &agent, &invoice_query(), None)
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
                expected_settings_revision: None,
                settings: None,
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
                    expected_generation: generation,
                    expected_settings_revision: None,
                    settings: None,
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
async fn deeper_search_settings_preserve_source_identity_and_live_history() {
    let (state, agent, _, service) = fixture();
    enable(&service, &agent, "mail-gmail").await;
    service.refresh(&agent, Some("mail-gmail")).await.unwrap();
    let before = state.read().await.connected_search.sources["mail-gmail"].clone();

    let input: SourceInput = serde_json::from_value(serde_json::json!({
        "enabled": true,
        "expectedGeneration": before.generation,
        "expectedSettingsRevision": 1,
        "settings": {
            "historyDepth": "days90",
            "retainedItemLimit": 500,
            "attachmentText": "supportedText",
            "historyPaused": false
        }
    }))
    .expect("deeper-search settings should be accepted by the source contract");
    let response = service
        .configure(&agent, "mail-gmail", input)
        .await
        .unwrap();
    let response = serde_json::to_value(response).unwrap();
    assert_eq!(response["settingsRevision"], 2);
    assert_eq!(response["settings"]["historyDepth"], "days90");

    let after = state.read().await.connected_search.sources["mail-gmail"].clone();
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.grant_epoch, before.grant_epoch);
    assert_eq!(after.stream_epoch, before.stream_epoch);
    assert_eq!(after.baseline_at_ms, before.baseline_at_ms);
    assert_eq!(after.seen, before.seen);
    assert_eq!(after.documents, before.documents);
}

#[tokio::test]
async fn temporary_attachment_failure_is_retried_after_restart_without_live_changes() {
    let (state, agent, reader, service) = fixture();
    enable(&service, &agent, "mail-gmail").await;
    service.refresh(&agent, Some("mail-gmail")).await.unwrap();
    reader
        .inbox_ids
        .lock()
        .unwrap()
        .insert("gmail-opaque-id".into());
    reader.attachment_unavailable.store(true, Ordering::SeqCst);
    let generation = state.read().await.connected_search.sources["mail-gmail"]
        .generation
        .clone();
    let input: SourceInput = serde_json::from_value(serde_json::json!({
        "enabled": true, "expectedGeneration": generation, "expectedSettingsRevision": 1,
        "settings": {"historyDepth":"recent", "retainedItemLimit":100, "attachmentText":"supportedText", "historyPaused":false}
    })).unwrap();
    service
        .configure(&agent, "mail-gmail", input)
        .await
        .unwrap();
    service.refresh(&agent, Some("mail-gmail")).await.unwrap();
    let (mut restored, seen, revision) = {
        let state = state.read().await;
        let index = &state.connected_search.sources["mail-gmail"];
        assert_eq!(index.attachment_retries.len(), 1);
        assert!(index.attachment_scanned.is_empty());
        (
            serde_json::from_slice::<SourceIndex>(&serde_json::to_vec(index).unwrap()).unwrap(),
            index.seen.clone(),
            index.documents[0].revision.clone(),
        )
    };
    restored
        .attachment_retries
        .values_mut()
        .for_each(|retry| retry.retry_at_ms = 0);
    state
        .write()
        .await
        .connected_search
        .sources
        .insert("mail-gmail".into(), restored);
    reader.attachment_unavailable.store(false, Ordering::SeqCst);
    let restarted = SearchService::with_reader(&state, Arc::new(Mutex::new(())), reader);
    restarted.refresh(&agent, Some("mail-gmail")).await.unwrap();
    let state = state.read().await;
    let index = &state.connected_search.sources["mail-gmail"];
    assert!(index.attachment_retries.is_empty());
    assert_eq!(index.attachment_scanned.len(), 1);
    assert!(index
        .attachments
        .values()
        .any(|entry| entry.document.text.contains("retry succeeded")));
    assert_eq!(index.seen, seen);
    assert_eq!(index.documents[0].revision, revision);
}

#[tokio::test]
async fn moved_mail_is_excluded_during_import_and_purged_after_restart() {
    let (state, agent, reader, service) = fixture();
    enable(&service, &agent, "mail-gmail").await;
    service.refresh(&agent, Some("mail-gmail")).await.unwrap();
    let generation = state.read().await.connected_search.sources["mail-gmail"]
        .generation
        .clone();
    let mut old = document("old", "private archived invoice");
    let received = crate::connectors::gcalendar::now_ms().saturating_sub(60_000);
    old.source_modified_at = Some(
        chrono::DateTime::from_timestamp_millis(received as i64)
            .unwrap()
            .to_rfc3339(),
    );
    *reader.history_page.lock().unwrap() = Some(HistoryPage {
        documents: vec![old],
        next_cursor: None,
        exhausted: true,
    });
    let configure = |depth: &str, revision| {
        serde_json::from_value::<SourceInput>(serde_json::json!({
        "enabled": true, "expectedGeneration": generation,
        "expectedSettingsRevision": revision,
        "settings": {"historyDepth": depth, "retainedItemLimit": 500, "attachmentText": "metadataOnly", "historyPaused": false}
    })).unwrap()
    };
    service
        .configure(&agent, "mail-gmail", configure("days90", 1))
        .await
        .unwrap();
    service.refresh(&agent, Some("mail-gmail")).await.unwrap();
    assert!(
        state.read().await.connected_search.sources["mail-gmail"]
            .historical
            .is_empty(),
        "an item moved during the page must not be committed"
    );

    reader.inbox_ids.lock().unwrap().insert("old".into());
    service
        .configure(&agent, "mail-gmail", configure("days30", 2))
        .await
        .unwrap();
    service.refresh(&agent, Some("mail-gmail")).await.unwrap();
    let serialized = {
        let state = state.read().await;
        let index = &state.connected_search.sources["mail-gmail"];
        assert_eq!(index.historical.len(), 1);
        assert_eq!(index.imported.len(), 1);
        serde_json::to_vec(index).unwrap()
    };
    let restored: SourceIndex = serde_json::from_slice(&serialized).unwrap();
    state
        .write()
        .await
        .connected_search
        .sources
        .insert("mail-gmail".into(), restored);
    reader.inbox_ids.lock().unwrap().clear();
    let restarted = SearchService::with_reader(&state, Arc::new(Mutex::new(())), reader.clone());
    let query = SearchQuery {
        query: "archived invoice".into(),
        connector_ids: vec![],
        kinds: vec![],
        limit: None,
    };
    let result = restarted.query(&agent, &query).await.unwrap();
    assert!(
        result.hits.is_empty(),
        "restored history is hidden until inbox membership is rechecked"
    );
    assert_eq!(
        result
            .sources
            .iter()
            .find(|source| source.connector_id == "mail-gmail")
            .unwrap()
            .history_coverage
            .reason
            .as_deref(),
        Some("needsVerification")
    );
    restarted.refresh(&agent, Some("mail-gmail")).await.unwrap();
    let state = state.read().await;
    let index = &state.connected_search.sources["mail-gmail"];
    assert!(
        index.historical.is_empty(),
        "archived content is purged after restore"
    );
    assert_eq!(
        index.imported.len(),
        1,
        "suppression receipts survive a purge"
    );
    assert!(state.workflows.is_empty());
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
