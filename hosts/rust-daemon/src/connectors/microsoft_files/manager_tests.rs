use super::super::{
    client::{MicrosoftFilePage, MicrosoftIdentity},
    store::{
        InMemoryMicrosoftFileCredentialStore, MicrosoftFileCredentialStore, MicrosoftFileGrant,
    },
};
use super::*;
use crate::connected_search::SourceReader;
use crate::connectors::oauth_apps::OAuthAppCredentials;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct FakeProvider {
    verifier: std::sync::Mutex<Option<String>>,
    deny_list: AtomicBool,
    fail_list: AtomicBool,
    deny_metadata: AtomicBool,
    fail_metadata: AtomicBool,
    revoke_refresh: AtomicBool,
    reduced_grant: AtomicBool,
    stall_content: AtomicBool,
    content_calls: AtomicUsize,
    metadata_calls: AtomicUsize,
    active_content: AtomicUsize,
    content_entered: tokio::sync::Notify,
    content_release: tokio::sync::Notify,
}
fn grant(mode: MicrosoftFileMode) -> MicrosoftFileGrant {
    MicrosoftFileGrant {
        mode,
        access_token: "access".to_string().into(),
        refresh_token: "refresh".to_string().into(),
        expires_at_ms: now() + 3_600_000,
        scopes: vec![match mode {
            MicrosoftFileMode::OneDrive => "Files.Read",
            MicrosoftFileMode::SharePoint => "Sites.Selected",
        }
        .into()],
    }
}
fn metadata() -> MicrosoftFileMetadata {
    MicrosoftFileMetadata {
        drive_id: "drive".into(),
        item_id: "item".into(),
        name: "report.txt".into(),
        mime: Some("text/plain".into()),
        size: Some(4),
        modified_at: None,
        revision: Some("v1".into()),
        source_url: None,
        is_folder: false,
    }
}
#[async_trait]
impl MicrosoftFileTransport for FakeProvider {
    async fn exchange(
        &self,
        _: &MicrosoftFileOAuthConfig,
        mode: MicrosoftFileMode,
        _: &str,
        verifier: &str,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        *self.verifier.lock().unwrap() = Some(verifier.into());
        let mut grant = grant(mode);
        if self.reduced_grant.load(Ordering::SeqCst) {
            grant.scopes.clear();
        }
        Ok(grant)
    }
    async fn refresh(
        &self,
        _: &MicrosoftFileOAuthConfig,
        old: &MicrosoftFileGrant,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        if self.revoke_refresh.load(Ordering::SeqCst) {
            Err(MicrosoftFileError::Reauthorization)
        } else {
            Ok(grant(old.mode))
        }
    }
    async fn identity(&self, _: &str) -> Result<MicrosoftIdentity, MicrosoftFileError> {
        Ok(MicrosoftIdentity {
            id: "account".into(),
            label: None,
        })
    }
    async fn site_id(&self, _: &str, _: &str) -> Result<String, MicrosoftFileError> {
        Ok("site".into())
    }
    async fn list(
        &self,
        _: &str,
        _: MicrosoftFileMode,
        _: Option<&str>,
        _: Option<(&str, &str)>,
        _: Option<&str>,
    ) -> Result<MicrosoftFilePage, MicrosoftFileError> {
        if self.deny_list.load(Ordering::SeqCst) {
            Err(MicrosoftFileError::PermissionDenied)
        } else if self.fail_list.load(Ordering::SeqCst) {
            Err(MicrosoftFileError::Unavailable)
        } else {
            Ok(MicrosoftFilePage {
                files: vec![metadata()],
                next_url: Some("next".into()),
            })
        }
    }
    async fn metadata(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<MicrosoftFileMetadata, MicrosoftFileError> {
        self.metadata_calls.fetch_add(1, Ordering::SeqCst);
        if self.deny_metadata.load(Ordering::SeqCst) {
            Err(MicrosoftFileError::PermissionDenied)
        } else if self.fail_metadata.load(Ordering::SeqCst) {
            Err(MicrosoftFileError::Unavailable)
        } else {
            Ok(metadata())
        }
    }
    async fn content(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<Vec<u8>, MicrosoftFileError> {
        struct ContentCustody<'a>(&'a AtomicUsize);
        impl Drop for ContentCustody<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        self.content_calls.fetch_add(1, Ordering::SeqCst);
        self.active_content.fetch_add(1, Ordering::SeqCst);
        let _custody = ContentCustody(&self.active_content);
        if self.stall_content.load(Ordering::SeqCst) {
            self.content_entered.notify_one();
            self.content_release.notified().await;
        }
        Ok(b"text".to_vec())
    }
}

async fn fixture() -> (
    SharedDaemonState,
    String,
    MicrosoftFileManager,
    Arc<FakeProvider>,
    Arc<InMemoryMicrosoftFileCredentialStore>,
) {
    let mut state = DaemonState::new();
    let agent = state
        .create_agent(
            serde_json::from_value(
                serde_json::json!({"name":"files-owner", "model":"deterministic"}),
            )
            .unwrap(),
        )
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(state));
    let oauth = OAuthAppService::in_memory();
    oauth
        .put(
            OAuthProvider::Microsoft,
            OAuthAppCredentials::new(OAuthProvider::Microsoft, "client", "secret", None).unwrap(),
        )
        .await
        .unwrap();
    let provider = Arc::new(FakeProvider::default());
    let credentials = Arc::new(InMemoryMicrosoftFileCredentialStore::default());
    let manager = MicrosoftFileManager::new(
        &state,
        Arc::new(Mutex::new(())),
        credentials.clone(),
        provider.clone(),
        oauth,
    );
    (state, agent, manager, provider, credentials)
}
fn consent_parts(url: &str) -> (String, String) {
    let parsed = reqwest::Url::parse(url).unwrap();
    let pairs = parsed
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    (
        pairs["state"].to_string(),
        pairs["code_challenge"].to_string(),
    )
}

struct ProjectKnowledgeFixture {
    state: SharedDaemonState,
    agent: String,
    manager: MicrosoftFileManager,
    provider: Arc<FakeProvider>,
    knowledge: crate::projects::knowledge::KnowledgeService,
    project: String,
    resource: String,
    _private: crate::document_edits::store::test_support::PrivateTestDirectory,
}

async fn project_knowledge_fixture() -> ProjectKnowledgeFixture {
    use crate::connected_search::{
        providers::document, source_authority, DocumentKind, SourceIndex,
    };
    use crate::projects::{
        connected::AddConnectedSourceRequest, knowledge::KnowledgeService, ProjectCreateRequest,
        ProjectMutation, ProjectService,
    };
    let (state, agent, manager, provider, _) = fixture().await;
    let started = manager
        .begin_connect(agent.clone(), MicrosoftFileMode::OneDrive, None)
        .await
        .unwrap();
    let (nonce, _) = consent_parts(&started.consent_url);
    manager
        .complete_connect(nonce, Some("code".into()))
        .await
        .unwrap();
    let files = manager
        .browse(agent.clone(), started.connector_id.clone(), None, None)
        .await
        .unwrap();
    let record = state.read().await.microsoft_file_records[&started.connector_id].clone();
    let key = files.files[0].key.clone();
    manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            record.selection_revision,
            vec![key.clone()],
        )
        .await
        .unwrap();
    let private = crate::document_edits::store::test_support::private_directory();
    let knowledge = KnowledgeService::open(private.path().to_owned(), None).unwrap();
    {
        let mut guard = state.write().await;
        guard.microsoft_files_manager = Some(manager.weak());
        guard.project_knowledge = knowledge.clone();
        let authority = source_authority(&guard, &agent, &record.id).unwrap();
        let mut index = SourceIndex::new(&authority);
        let mut selected = document(
            DocumentKind::File,
            key,
            "selected".into(),
            "Selected report".into(),
            String::new(),
        );
        selected.citation_id = "a".repeat(64);
        selected.revision = "b".repeat(64);
        index.documents.push(selected);
        guard
            .connected_search
            .sources
            .insert(record.id.clone(), index);
    }
    let projects = ProjectService::new(state.clone(), manager.0.transactions.clone());
    let created = projects
        .mutate(ProjectMutation::Create(ProjectCreateRequest {
            request_key: "microsoft-custody-project".into(),
            title: "Retained native source".into(),
            member_agent_ids: vec![agent.clone()],
            instructions: String::new(),
        }))
        .await
        .unwrap();
    let project = created["project"]["id"].as_str().unwrap().to_owned();
    let revision = state.read().await.projects.get(&project).unwrap().revision;
    projects
        .mutate(ProjectMutation::AddConnectedSource {
            id: project.clone(),
            input: AddConnectedSourceRequest {
                expected_revision: revision,
                request_key: "selected-microsoft-source".into(),
                as_agent_id: agent.clone(),
                owner_agent_id: agent.clone(),
                connector_id: record.id,
                citation_id: "a".repeat(64),
                share_with_project: true,
                label: None,
            },
        })
        .await
        .unwrap();
    let resource = state.read().await.projects.get(&project).unwrap().resources[0]
        .id
        .clone();
    provider.content_calls.store(0, Ordering::SeqCst);
    provider.metadata_calls.store(0, Ordering::SeqCst);
    ProjectKnowledgeFixture {
        state,
        agent,
        manager,
        provider,
        knowledge,
        project,
        resource,
        _private: private,
    }
}

#[tokio::test]
async fn microsoft_project_timeouts_retain_physical_permits_and_bound_nested_queued_reads() {
    use crate::projects::ProjectError;
    let fixture = project_knowledge_fixture().await;
    let baseline = Arc::strong_count(&fixture.manager.0);
    fixture.provider.stall_content.store(true, Ordering::SeqCst);
    let first = tokio::spawn({
        let state = fixture.state.clone();
        let knowledge = fixture.knowledge.clone();
        let project = fixture.project.clone();
        let agent = fixture.agent.clone();
        async move {
            knowledge
                .search(
                    &state,
                    &project,
                    &agent,
                    "text",
                    8,
                    anima_core::CancelSignal::new(),
                    Some(tokio::time::Instant::now() + std::time::Duration::from_millis(3300)),
                )
                .await
        }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        fixture.provider.content_entered.notified(),
    )
    .await
    .expect("initial selected native content read should start");
    tokio::time::timeout(std::time::Duration::from_secs(2), first)
        .await
        .expect("snapshot timeout must bound the first caller wait")
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture.provider.active_content.load(Ordering::SeqCst),
        1,
        "the first owned Microsoft read remains physically live after timeout"
    );
    let _second = fixture
        .knowledge
        .search(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            "text",
            8,
            anima_core::CancelSignal::new(),
            Some(tokio::time::Instant::now() + std::time::Duration::from_millis(3300)),
        )
        .await;
    for _ in 0..24 {
        fixture
            .knowledge
            .refresh(&fixture.state, &fixture.project, &fixture.agent, None)
            .await
            .unwrap();
        let _ = fixture
            .knowledge
            .search(
                &fixture.state,
                &fixture.project,
                &fixture.agent,
                "text",
                8,
                anima_core::CancelSignal::new(),
                Some(tokio::time::Instant::now() + std::time::Duration::from_millis(3300)),
            )
            .await;
        tokio::task::yield_now().await;
    }
    assert_eq!(
        fixture.provider.content_calls.load(Ordering::SeqCst),
        1,
        "only the first native operation may hold the per-agent gate"
    );
    assert_eq!(fixture.provider.metadata_calls.load(Ordering::SeqCst), 1);
    assert!(
        Arc::strong_count(&fixture.manager.0) <= baseline + 4,
        "two retained read slots bound both physical and gate-queued manager jobs"
    );
    let cancel = anima_core::CancelSignal::new();
    cancel.cancel();
    assert!(matches!(
        fixture
            .knowledge
            .search(
                &fixture.state,
                &fixture.project,
                &fixture.agent,
                "text",
                8,
                cancel,
                None
            )
            .await,
        Err(ProjectError::Cancelled)
    ));
    assert!(matches!(
        fixture
            .knowledge
            .search(
                &fixture.state,
                &fixture.project,
                &fixture.agent,
                "text",
                8,
                anima_core::CancelSignal::new(),
                Some(tokio::time::Instant::now() - std::time::Duration::from_millis(1))
            )
            .await,
        Err(ProjectError::Deadline)
    ));
    assert_eq!(
        fixture.provider.content_calls.load(Ordering::SeqCst),
        1,
        "pre-cancelled and expired requests start no additional I/O"
    );
    fixture
        .provider
        .stall_content
        .store(false, Ordering::SeqCst);
    fixture.provider.content_release.notify_waiters();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while Arc::strong_count(&fixture.manager.0) != baseline {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both nested native jobs must settle before retained capacity returns");
    assert_eq!(fixture.provider.active_content.load(Ordering::SeqCst), 0);
    let recovered = fixture
        .knowledge
        .search(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            "text",
            8,
            anima_core::CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        recovered.hits.len(),
        1,
        "all physical read permits return after settlement"
    );
}

#[tokio::test]
async fn microsoft_project_scope_is_rechecked_after_scheduling_before_native_dispatch() {
    use crate::projects::knowledge::test_support;
    for revoke in 0..4 {
        let fixture = project_knowledge_fixture().await;
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let cancel = anima_core::CancelSignal::new();
        let mut reading = Box::pin(test_support::connected_read(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &fixture.resource,
            &cancel,
            tokio::time::Instant::now()
                + if revoke == 3 {
                    std::time::Duration::from_millis(100)
                } else {
                    std::time::Duration::from_secs(2)
                },
            slots.clone(),
        ));
        // On this current-thread runtime, spawn never polls its child synchronously.
        assert!(futures::poll!(reading.as_mut()).is_pending());
        assert_eq!(
            slots.available_permits(),
            0,
            "the child has been scheduled but cannot run before this barrier"
        );
        if revoke == 2 {
            cancel.cancel();
        } else if revoke == 3 {
            // Keep the current-thread executor behind the pre-start barrier until expiry.
            std::thread::sleep(std::time::Duration::from_millis(120));
        } else {
            let mut state = fixture
                .state
                .try_write()
                .expect("the scheduling boundary retains no state lock");
            let revision = state.projects.get(&fixture.project).unwrap().revision;
            if revoke == 1 {
                state
                    .projects
                    .replace_members(&fixture.project, revision, Vec::new(), 3)
                    .unwrap();
            } else {
                state
                    .projects
                    .remove_resource(&fixture.project, revision, &fixture.resource, 3)
                    .unwrap();
            }
            state
                .project_access
                .publish_records(&state.projects)
                .unwrap();
        }
        assert!(reading.await.is_err());
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while slots.available_permits() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("revoked pre-start jobs must retire their permit");
        assert_eq!(fixture.provider.metadata_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture.provider.content_calls.load(Ordering::SeqCst),
            0,
            "source/member revocation, cancellation or expiry before the owned job starts must prevent native I/O"
        );
    }
}

#[tokio::test]
async fn site_consent_is_bound_to_site_pkce_and_browse_denial_retires_authority() {
    let (state, agent, manager, provider, _) = fixture().await;
    let started = manager
        .begin_connect(
            agent.clone(),
            MicrosoftFileMode::SharePoint,
            Some("https://org.sharepoint.com/sites/a/".into()),
        )
        .await
        .unwrap();
    let reused = manager
        .begin_connect(
            agent.clone(),
            MicrosoftFileMode::SharePoint,
            Some("https://org.sharepoint.com/sites/a".into()),
        )
        .await
        .unwrap();
    assert_eq!(started.consent_url, reused.consent_url);
    assert!(matches!(
        manager
            .begin_connect(
                agent.clone(),
                MicrosoftFileMode::SharePoint,
                Some("https://org.sharepoint.com/sites/b".into())
            )
            .await,
        Err(MicrosoftFileError::Conflict)
    ));
    let (nonce, challenge) = consent_parts(&started.consent_url);
    manager
        .complete_connect(nonce.clone(), Some("code".into()))
        .await
        .unwrap();
    let verifier = provider.verifier.lock().unwrap().clone().unwrap();
    assert_eq!(
        challenge,
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    );
    assert!(
        manager
            .complete_connect(nonce, Some("code".into()))
            .await
            .is_err(),
        "state is single-use"
    );
    manager
        .browse(agent.clone(), started.connector_id.clone(), None, None)
        .await
        .unwrap();
    assert!(!manager.0.items.lock().await.is_empty());
    assert!(!manager.0.cursors.lock().await.is_empty());
    provider.fail_list.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager
            .browse(agent.clone(), started.connector_id.clone(), None, None)
            .await,
        Err(MicrosoftFileError::Unavailable)
    ));
    assert_eq!(
        state.read().await.microsoft_file_records[&started.connector_id].status,
        MicrosoftFileStatus::Active
    );
    provider.fail_list.store(false, Ordering::SeqCst);
    {
        let mut guard = state.write().await;
        let authority =
            crate::connected_search::source_authority(&guard, &agent, &started.connector_id)
                .unwrap();
        guard.connected_search.sources.insert(
            started.connector_id.clone(),
            crate::connected_search::SourceIndex::new(&authority),
        );
    }
    provider.deny_list.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager
            .browse(agent.clone(), started.connector_id.clone(), None, None)
            .await,
        Err(MicrosoftFileError::PermissionDenied)
    ));
    assert_eq!(
        state.read().await.microsoft_file_records[&started.connector_id].status,
        MicrosoftFileStatus::ReauthRequired
    );
    assert!(!state
        .read()
        .await
        .connected_search
        .sources
        .contains_key(&started.connector_id));
    assert!(manager.0.items.lock().await.is_empty());
    assert!(manager.0.cursors.lock().await.is_empty());
}

#[tokio::test]
async fn selected_read_is_fenced_against_reselection_and_revoked_refresh() {
    let (state, agent, manager, provider, credentials) = fixture().await;
    let started = manager
        .begin_connect(agent.clone(), MicrosoftFileMode::OneDrive, None)
        .await
        .unwrap();
    let (nonce, _) = consent_parts(&started.consent_url);
    manager
        .complete_connect(nonce, Some("code".into()))
        .await
        .unwrap();
    let page = manager
        .browse(agent.clone(), started.connector_id.clone(), None, None)
        .await
        .unwrap();
    let record = state.read().await.microsoft_file_records[&started.connector_id].clone();
    let key = page.files[0].key.clone();
    let selected = manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            record.selection_revision,
            vec![key.clone()],
        )
        .await
        .unwrap();
    let content = manager
        .read_for_agent(agent.clone(), record.id.clone(), key.clone())
        .await
        .unwrap();
    let empty = manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            selected.selection_revision,
            vec![],
        )
        .await
        .unwrap();
    manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            empty.selection_revision,
            vec![key.clone()],
        )
        .await
        .unwrap();
    assert!(matches!(
        manager
            .verify_selected(
                &agent,
                &record.id,
                &key,
                &content.generation,
                content.selection_revision
            )
            .await,
        Err(MicrosoftFileError::Conflict)
    ));
    assert!(matches!(
        manager.retire_source(&record).await,
        Err(MicrosoftFileError::Conflict)
    ));
    assert_eq!(
        state.read().await.microsoft_file_records[&record.id].status,
        MicrosoftFileStatus::Active,
        "a stale failure must not retire newer authority"
    );
    let mut expired = grant(MicrosoftFileMode::OneDrive);
    expired.expires_at_ms = 1;
    credentials
        .put(&record.id, &record.generation, expired)
        .await
        .unwrap();
    provider.revoke_refresh.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager
            .browse(agent.clone(), record.id.clone(), None, None)
            .await,
        Err(MicrosoftFileError::Reauthorization)
    ));
    assert_eq!(
        state.read().await.microsoft_file_records[&record.id].status,
        MicrosoftFileStatus::ReauthRequired
    );
}

#[tokio::test]
async fn reduced_grant_never_activates_a_connection() {
    let (state, agent, manager, provider, _) = fixture().await;
    provider.reduced_grant.store(true, Ordering::SeqCst);
    let started = manager
        .begin_connect(agent, MicrosoftFileMode::OneDrive, None)
        .await
        .unwrap();
    let (nonce, _) = consent_parts(&started.consent_url);
    assert!(manager
        .complete_connect(nonce, Some("code".into()))
        .await
        .is_err());
    assert_ne!(
        state.read().await.microsoft_file_records[&started.connector_id].status,
        MicrosoftFileStatus::Active
    );
}

#[tokio::test]
async fn failed_retirement_save_still_fences_the_live_source() {
    let (state, agent, manager, provider, _) = fixture().await;
    let started = manager
        .begin_connect(agent.clone(), MicrosoftFileMode::OneDrive, None)
        .await
        .unwrap();
    let (nonce, _) = consent_parts(&started.consent_url);
    manager
        .complete_connect(nonce, Some("code".into()))
        .await
        .unwrap();
    manager
        .browse(agent.clone(), started.connector_id.clone(), None, None)
        .await
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let blocking = temporary.path().join("not-a-directory");
    std::fs::write(&blocking, b"x").unwrap();
    {
        let mut guard = state.write().await;
        let authority =
            crate::connected_search::source_authority(&guard, &agent, &started.connector_id)
                .unwrap();
        guard.connected_search.sources.insert(
            started.connector_id.clone(),
            crate::connected_search::SourceIndex::new(&authority),
        );
        guard.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(blocking.join("state.json")),
        ));
    }
    provider.deny_list.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager
            .browse(agent, started.connector_id.clone(), None, None)
            .await,
        Err(MicrosoftFileError::Persistence)
    ));
    let guard = state.read().await;
    assert_eq!(
        guard.microsoft_file_records[&started.connector_id].status,
        MicrosoftFileStatus::ReauthRequired
    );
    assert!(!guard
        .connected_search
        .sources
        .contains_key(&started.connector_id));
    assert!(manager.0.items.lock().await.is_empty());
}

struct SearchFixtureReader {
    state: SharedDaemonState,
    manager: MicrosoftFileManager,
}
#[async_trait]
impl crate::connected_search::SourceReader for SearchFixtureReader {
    async fn read(
        &self,
        source: &crate::connected_search::SourceAuthority,
    ) -> Result<crate::connected_search::SourceSnapshot, crate::connected_search::SearchError> {
        use crate::connected_search::{DocumentKind, IndexedDocument, SearchError, SourceSnapshot};
        let record = self
            .state
            .read()
            .await
            .microsoft_file_records
            .get(&source.connector_id)
            .cloned()
            .ok_or(SearchError::Conflict)?;
        let mut documents = Vec::new();
        for file in &record.selected_files {
            let metadata = self
                .manager
                .metadata_selected(
                    source.agent_id.clone(),
                    source.connector_id.clone(),
                    record.generation.clone(),
                    file.key.clone(),
                )
                .await
                .map_err(|_| SearchError::Unavailable)?;
            documents.push(IndexedDocument {
                citation_id: String::new(),
                external_id: file.key.clone(),
                collection_id: "selected".into(),
                kind: DocumentKind::File,
                title: "private parent report".into(),
                text: String::new(),
                sender: None,
                source_url: None,
                source_modified_at: metadata.modified_at,
                event_start: None,
                event_end: None,
                revision: metadata.revision.unwrap_or_default(),
                truncated: false,
                indexed_at_ms: 0,
            });
        }
        Ok(SourceSnapshot {
            documents,
            stream_epoch: None,
            selection_revision: Some(record.selection_revision),
            incomplete_count: 0,
        })
    }
    async fn attachments(
        &self,
        _: &crate::connected_search::SourceAuthority,
        _: &crate::connected_search::IndexedDocument,
    ) -> Result<crate::connected_search::AttachmentBatch, crate::connected_search::SearchError>
    {
        use crate::connected_search::{AttachmentBatch, RawAttachment};
        Ok(AttachmentBatch {
            items: vec![RawAttachment {
                id: "file-content".into(),
                name: "private child.txt".into(),
                mime: "text/plain".into(),
                bytes: Some(b"private child content".to_vec()),
                status: "ready".into(),
            }],
            omitted: 0,
        })
    }
}

struct GatedSearchFixtureReader {
    inner: SearchFixtureReader,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait]
impl crate::connected_search::SourceReader for GatedSearchFixtureReader {
    async fn read(
        &self,
        source: &crate::connected_search::SourceAuthority,
    ) -> Result<crate::connected_search::SourceSnapshot, crate::connected_search::SearchError> {
        let snapshot = self.inner.read(source).await?;
        self.entered.notify_one();
        self.release.notified().await;
        Ok(snapshot)
    }
    async fn attachments(
        &self,
        source: &crate::connected_search::SourceAuthority,
        parent: &crate::connected_search::IndexedDocument,
    ) -> Result<crate::connected_search::AttachmentBatch, crate::connected_search::SearchError>
    {
        self.inner.attachments(source, parent).await
    }
}

#[tokio::test]
async fn failed_file_denial_and_fresh_restart_never_publish_unverified_cached_text() {
    use crate::connected_search::{SearchError, SearchQuery, SearchService, SourceInput};
    use crate::control_plane_store::ControlPlaneStoreConfig;
    let (state, agent, manager, provider, credentials) = fixture().await;
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.json");
    let config = ControlPlaneStoreConfig::Json(path.clone());
    state
        .write()
        .await
        .set_control_plane_store(Some(config.clone()));
    let started = manager
        .begin_connect(agent.clone(), MicrosoftFileMode::OneDrive, None)
        .await
        .unwrap();
    let (nonce, _) = consent_parts(&started.consent_url);
    manager
        .complete_connect(nonce, Some("code".into()))
        .await
        .unwrap();
    let page = manager
        .browse(agent.clone(), started.connector_id.clone(), None, None)
        .await
        .unwrap();
    let record = state.read().await.microsoft_file_records[&started.connector_id].clone();
    let key = page.files[0].key.clone();
    manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            record.selection_revision,
            vec![key.clone()],
        )
        .await
        .unwrap();
    let reader = Arc::new(SearchFixtureReader {
        state: state.clone(),
        manager: manager.clone(),
    });
    let service = SearchService::with_reader(&state, manager.0.transactions.clone(), reader);
    let view = service
        .sources(&agent)
        .await
        .unwrap()
        .sources
        .into_iter()
        .find(|view| view.connector_id == record.id)
        .unwrap();
    let input: SourceInput = serde_json::from_value(serde_json::json!({"enabled":true, "expectedGeneration":view.generation,
        "expectedSettingsRevision":1, "settings":{"historyDepth":"recent", "retainedItemLimit":100, "attachmentText":"supportedText", "historyPaused":false}})).unwrap();
    service.configure(&agent, &record.id, input).await.unwrap();
    service.refresh(&agent, Some(&record.id)).await.unwrap();
    let query = SearchQuery {
        query: "private".into(),
        connector_ids: vec![],
        kinds: vec![],
        limit: None,
    };
    let hits = service.query(&agent, &query).await.unwrap().hits;
    assert_eq!(
        hits.len(),
        2,
        "the original parent and extracted child must both be readable"
    );
    let ids = hits
        .iter()
        .map(|hit| hit.citation_id.clone())
        .collect::<Vec<_>>();
    for id in &ids {
        assert!(service.item(&agent, id).await.is_ok());
    }
    let current = state.read().await.microsoft_file_records[&record.id].clone();
    let empty = manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            current.selection_revision,
            vec![],
        )
        .await
        .unwrap();
    manager
        .select(
            agent.clone(),
            record.id.clone(),
            record.generation.clone(),
            empty.selection_revision,
            vec![key.clone()],
        )
        .await
        .unwrap();
    assert!(
        service.query(&agent, &query).await.unwrap().hits.is_empty(),
        "reselection invalidates prior runtime proof"
    );
    service.refresh(&agent, Some(&record.id)).await.unwrap();
    assert_eq!(service.query(&agent, &query).await.unwrap().hits.len(), 2);
    let saved_before_denial = std::fs::read(&path).unwrap();

    let blocking = temporary.path().join("not-a-directory");
    std::fs::write(&blocking, b"x").unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let gated = SearchService::with_reader(
        &state,
        manager.0.transactions.clone(),
        Arc::new(GatedSearchFixtureReader {
            inner: SearchFixtureReader {
                state: state.clone(),
                manager: manager.clone(),
            },
            entered: entered.clone(),
            release: release.clone(),
        }),
    );
    let refresh = tokio::spawn({
        let agent = agent.clone();
        let id = record.id.clone();
        async move { gated.refresh(&agent, Some(&id)).await }
    });
    entered.notified().await;
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            blocking.join("failed.json"),
        )));
    provider.deny_metadata.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager
            .metadata_selected(
                agent.clone(),
                record.id.clone(),
                record.generation.clone(),
                key.clone()
            )
            .await,
        Err(MicrosoftFileError::PermissionDenied)
    ));
    state.write().await.set_control_plane_store(Some(config));
    release.notify_one();
    refresh.await.unwrap().unwrap();
    assert!(service.query(&agent, &query).await.unwrap().hits.is_empty());
    for id in &ids {
        assert!(matches!(
            service.item(&agent, id).await,
            Err(SearchError::NotFound)
        ));
    }

    let snapshot = serde_json::from_slice(&saved_before_denial).unwrap();
    let mut fresh = DaemonState::new();
    fresh.restore_control_plane_snapshot(snapshot).unwrap();
    let fresh = Arc::new(RwLock::new(fresh));
    let transactions = Arc::new(Mutex::new(()));
    let fresh_manager = MicrosoftFileManager::new(
        &fresh,
        transactions.clone(),
        credentials,
        provider.clone(),
        manager.0.oauth_apps.clone(),
    );
    let fresh_service = SearchService::with_reader(
        &fresh,
        transactions,
        Arc::new(SearchFixtureReader {
            state: fresh.clone(),
            manager: fresh_manager.clone(),
        }),
    );
    let before = fresh_service.query(&agent, &query).await.unwrap();
    assert!(
        before.hits.is_empty(),
        "saved parent and child are hidden before runtime authority checks"
    );
    assert_eq!(
        before
            .sources
            .iter()
            .find(|source| source.connector_id == record.id)
            .unwrap()
            .status,
        "syncing"
    );
    for id in &ids {
        assert!(matches!(
            fresh_service.item(&agent, id).await,
            Err(SearchError::NotFound)
        ));
    }
    provider.deny_metadata.store(false, Ordering::SeqCst);
    provider.fail_metadata.store(true, Ordering::SeqCst);
    fresh_service
        .refresh(&agent, Some(&record.id))
        .await
        .unwrap();
    assert!(
        fresh_service
            .query(&agent, &query)
            .await
            .unwrap()
            .hits
            .is_empty(),
        "transient verification cannot release cached content"
    );
    provider.fail_metadata.store(false, Ordering::SeqCst);
    fresh_service
        .refresh(&agent, Some(&record.id))
        .await
        .unwrap();
    assert_eq!(
        fresh_service
            .query(&agent, &query)
            .await
            .unwrap()
            .hits
            .len(),
        2,
        "current provider proof may release the retained parent and child"
    );

    fresh
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            blocking.join("retirement.json"),
        )));
    provider.deny_list.store(true, Ordering::SeqCst);
    assert!(matches!(
        fresh_manager
            .browse(agent.clone(), record.id.clone(), None, None)
            .await,
        Err(MicrosoftFileError::Persistence)
    ));
    fresh.write().await.set_control_plane_store(None);
    assert!(
        fresh_service
            .query(&agent, &query)
            .await
            .unwrap()
            .hits
            .is_empty(),
        "retirement save failure fences the live parent and child"
    );
    let snapshot = serde_json::from_slice(&saved_before_denial).unwrap();
    let mut second_fresh = DaemonState::new();
    second_fresh
        .restore_control_plane_snapshot(snapshot)
        .unwrap();
    let second_fresh = Arc::new(RwLock::new(second_fresh));
    let second_transactions = Arc::new(Mutex::new(()));
    let second_manager = MicrosoftFileManager::new(
        &second_fresh,
        second_transactions.clone(),
        fresh_manager.0.credentials.clone(),
        provider.clone(),
        fresh_manager.0.oauth_apps.clone(),
    );
    let second_service = SearchService::with_reader(
        &second_fresh,
        second_transactions,
        Arc::new(SearchFixtureReader {
            state: second_fresh.clone(),
            manager: second_manager,
        }),
    );
    assert!(
        second_service
            .query(&agent, &query)
            .await
            .unwrap()
            .hits
            .is_empty(),
        "a restart after failed retirement does not expose saved content"
    );
    for id in &ids {
        assert!(matches!(
            second_service.item(&agent, id).await,
            Err(SearchError::NotFound)
        ));
    }
}
