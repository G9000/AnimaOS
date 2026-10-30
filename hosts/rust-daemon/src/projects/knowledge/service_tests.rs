use super::*;
use crate::document_edits::store::test_support::{private_directory, PrivateTestDirectory};
use crate::projects::records::{text_digest, ProjectMutationReplay};
use crate::projects::{ProjectCreateRequest, ProjectResourceRecord, ProjectResourceRole};
use crate::sessions::test_support::agent_config;
use crate::state::DaemonState;

struct Fixture {
    service: KnowledgeService,
    state: SharedDaemonState,
    project: String,
    agent: String,
    outsider: String,
    resource: String,
    source: tempfile::TempDir,
    _private: PrivateTestDirectory,
}

struct FailsOnceKnowledgeExtractor {
    calls: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl DocumentExtractor for FailsOnceKnowledgeExtractor {
    async fn extract(
        &self,
        _format: anima_service_adapters::document_content::DocumentFormat,
        _bytes: Vec<u8>,
        _cancel: CancelSignal,
        _deadline: Option<Instant>,
    ) -> Result<anima_service_adapters::document_content::ExtractedDocumentText, WorkerFailure>
    {
        Err(WorkerFailure::Unavailable)
    }

    async fn extract_knowledge(
        &self,
        format: anima_service_adapters::document_content::DocumentFormat,
        _bytes: Vec<u8>,
        _cancel: CancelSignal,
        _deadline: Option<Instant>,
    ) -> Result<anima_service_adapters::document_content::ExtractedDocumentText, WorkerFailure>
    {
        use anima_service_adapters::document_content::{
            DocumentBlock, DocumentExtractionStatus, DocumentLocator, ExtractedDocumentText,
        };
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(WorkerFailure::Unavailable);
        }
        let mut extracted =
            ExtractedDocumentText::empty_for(format, DocumentExtractionStatus::Ready);
        extracted.blocks.push(DocumentBlock {
            locator: DocumentLocator::DocxParagraph { paragraph: 1 },
            text: "retrymarker recovered knowledge".into(),
        });
        Ok(extracted)
    }
}

#[tokio::test]
async fn project_knowledge_explicit_refresh_retries_unchanged_failed_extraction() {
    let mut fixture = fixture("");
    std::fs::remove_file(fixture.source.path().join("notes.txt")).unwrap();
    std::fs::write(
        fixture.source.path().join("selected.docx"),
        b"unchanged source bytes",
    )
    .unwrap();
    let extractor = Arc::new(FailsOnceKnowledgeExtractor {
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    fixture.service = KnowledgeService::with_cache_and_extractor(
        fixture.service.inner.cache.clone(),
        extractor.clone(),
    );
    fixture.state.write().await.project_knowledge = fixture.service.clone();

    let failed = search(&fixture, "retrymarker").await;
    assert!(failed.hits.is_empty());
    assert_eq!(
        failed.binding.sources[0].status,
        KnowledgeSourceState::Partial
    );
    assert_eq!(failed.binding.sources[0].omitted_count, 1);
    assert!(
        search(&fixture, "retrymarker").await.hits.is_empty(),
        "ordinary searches retain the bounded unchanged-source cache"
    );

    let occupied = fixture
        .service
        .inner
        .jobs
        .clone()
        .try_acquire_owned()
        .unwrap();
    let scope = Scope::capture(
        &fixture.state,
        &fixture.project,
        &fixture.agent,
        Purpose::Manual,
    )
    .await
    .unwrap();
    fixture
        .service
        .schedule_job(&fixture.state, scope, IndexRefresh::Reuse)
        .unwrap();
    let queued = fixture
        .service
        .refresh(&fixture.state, &fixture.project, &fixture.agent, None)
        .await
        .unwrap();
    assert_eq!(queued.sources[0].status, KnowledgeSourceState::Indexing);
    let pending = fixture.service.inner.pending.lock().unwrap().len();
    drop(occupied);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if fixture.service.inner.pending.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let recovered = search(&fixture, "retrymarker").await;
    assert_eq!(
        recovered.hits.len(),
        1,
        "explicit refresh must re-extract unchanged bytes after a worker failure"
    );
    assert!(recovered.hits[0]
        .excerpt
        .contains("retrymarker recovered knowledge"));
    assert_eq!(
        recovered.binding.sources[0].status,
        KnowledgeSourceState::Ready
    );
    assert_eq!(
        pending, 2,
        "explicit retry must not coalesce into a queued ordinary cache-reuse job"
    );
    assert_eq!(extractor.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn project_knowledge_busy_slot_queues_two_projects_and_deduplicates_refresh() {
    let fixture = fixture("cobalt first project");
    let other = tempfile::Builder::new()
        .prefix("anima-knowledge-second-")
        .tempdir()
        .unwrap();
    std::fs::write(other.path().join("second.txt"), "silver second project").unwrap();
    let second = {
        let mut state = fixture.state.write().await;
        let (project, _) = state
            .projects
            .create(
                ProjectCreateRequest {
                    request_key: uuid::Uuid::new_v4().to_string(),
                    title: "Second knowledge project".into(),
                    member_agent_ids: vec![fixture.agent.clone()],
                    instructions: String::new(),
                },
                1,
            )
            .unwrap();
        add_folder(&mut state, &project.id, other.path());
        project.id
    };
    let occupied = fixture
        .service
        .inner
        .jobs
        .clone()
        .try_acquire_owned()
        .unwrap();
    for id in [&fixture.project, &second, &second] {
        let queued = fixture
            .service
            .refresh(&fixture.state, id, &fixture.agent, None)
            .await
            .unwrap();
        assert_eq!(queued.sources[0].status, KnowledgeSourceState::Indexing);
    }
    assert_eq!(fixture.service.inner.pending.lock().unwrap().len(), 2);
    drop(occupied);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let first = fixture
                .service
                .status(&fixture.state, &fixture.project, &fixture.agent)
                .await
                .unwrap();
            let second = fixture
                .service
                .status(&fixture.state, &second, &fixture.agent)
                .await
                .unwrap();
            if first.sources[0].status == KnowledgeSourceState::Ready
                && second.sources[0].status == KnowledgeSourceState::Ready
            {
                assert_eq!(first.sources[0].document_count, 1);
                assert_eq!(second.sources[0].document_count, 1);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn project_knowledge_witness_honors_cancellation_during_final_admission() {
    let fixture = fixture("cobalt cancellation witness");
    let project = fixture
        .state
        .read()
        .await
        .projects
        .get(&fixture.project)
        .unwrap()
        .clone();
    let cancel = CancelSignal::new();
    let retrieval = fixture
        .service
        .retrieve(
            &fixture.state,
            &project,
            &fixture.agent,
            "cobalt",
            cancel.clone(),
            None,
        )
        .await
        .unwrap();
    cancel.cancel();
    assert!(matches!(
        retrieval.witness.check(&fixture.state).await,
        Err(ProjectError::Cancelled)
    ));
}

#[tokio::test]
async fn project_knowledge_one_replaced_folder_does_not_block_healthy_sources() {
    let fixture = fixture("cobalt old folder evidence");
    let healthy = tempfile::Builder::new()
        .prefix("anima-knowledge-healthy-")
        .tempdir()
        .unwrap();
    std::fs::write(
        healthy.path().join("healthy.txt"),
        "cobalt healthy evidence",
    )
    .unwrap();
    let healthy_resource = {
        let mut state = fixture.state.write().await;
        add_folder(&mut state, &fixture.project, healthy.path())
    };
    let first = search(&fixture, "cobalt").await;
    assert_eq!(first.hits.len(), 2);
    let stale = first
        .hits
        .iter()
        .find(|hit| hit.resource_id == fixture.resource)
        .unwrap()
        .citation_id
        .clone();
    let retired = tempfile::Builder::new()
        .prefix("anima-knowledge-retired-")
        .tempdir()
        .unwrap();
    std::fs::rename(fixture.source.path(), retired.path().join("old-root")).unwrap();
    std::fs::create_dir(fixture.source.path()).unwrap();
    std::fs::write(
        fixture.source.path().join("notes.txt"),
        "cobalt replacement must stay unread",
    )
    .unwrap();
    let found = search(&fixture, "cobalt").await;
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].resource_id, healthy_resource);
    assert!(fixture
        .service
        .citation(&fixture.state, &fixture.project, &fixture.agent, &stale)
        .await
        .is_err());
    let status = fixture
        .service
        .status(&fixture.state, &fixture.project, &fixture.agent)
        .await
        .unwrap();
    assert_eq!(
        status
            .sources
            .iter()
            .find(|source| source.resource_id == fixture.resource)
            .unwrap()
            .status,
        KnowledgeSourceState::Unavailable
    );
    assert_eq!(
        status
            .sources
            .iter()
            .find(|source| source.resource_id == healthy_resource)
            .unwrap()
            .status,
        KnowledgeSourceState::Ready
    );
}

#[tokio::test]
async fn project_knowledge_revoked_connected_source_does_not_hide_healthy_folder_passages() {
    use anima_document_store_sqlite::connected::{ConnectedSourceGrant, ConnectedSourceKind};
    let fixture = fixture("cobalt healthy folder evidence");
    let remote = {
        let mut state = fixture.state.write().await;
        let project = state.projects.get(&fixture.project).unwrap().clone();
        let resource = ProjectResourceRecord {
            id: format!("resource_{}", uuid::Uuid::new_v4()),
            target: ProjectResourceTarget::ConnectedSource {
                grant: ConnectedSourceGrant {
                    owner_agent_id: fixture.agent.clone(),
                    connector_id: "removed-connection".into(),
                    provider: "gdrive".into(),
                    remote_id: "selected".into(),
                    citation_id: "a".repeat(64),
                    connection_generation: "native".into(),
                    search_generation: "a".repeat(64),
                    grant_epoch: "enrollment".into(),
                    selection_revision: 0,
                    stream_epoch: None,
                    source_kind: ConnectedSourceKind::File,
                },
            },
            role: ProjectResourceRole::Reference,
            label: Some("Unavailable remote source".into()),
            use_in_context: true,
            created_at_ms: 2,
            add_replay: ProjectMutationReplay {
                request_key: "connected".into(),
                request_sha256: "b".repeat(64),
            },
            donor: Some(crate::projects::ProjectDonorBinding {
                agent_id: fixture.agent.clone(),
                member_generation: project.member(&fixture.agent).unwrap().generation,
            }),
            source_project_binding_revision: None,
            workspace_key: None,
        };
        let id = resource.id.clone();
        state
            .projects
            .add_resource(&fixture.project, project.revision, resource, 3)
            .unwrap();
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
        id
    };
    let found = search(&fixture, "cobalt").await;
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].resource_id, fixture.resource);
    assert_eq!(
        found
            .binding
            .sources
            .iter()
            .find(|source| source.resource_id == remote)
            .unwrap()
            .status,
        KnowledgeSourceState::Unavailable
    );
}

#[tokio::test]
async fn project_knowledge_status_restarts_only_unknown_source_jobs() {
    let fixture = fixture("cobalt restart coverage");
    let first = fixture
        .service
        .status(&fixture.state, &fixture.project, &fixture.agent)
        .await
        .unwrap();
    assert!(matches!(
        first.sources[0].status,
        KnowledgeSourceState::Indexing | KnowledgeSourceState::Ready
    ));
    let ready = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = fixture
                .service
                .status(&fixture.state, &fixture.project, &fixture.agent)
                .await
                .unwrap();
            if status.sources[0].status != KnowledgeSourceState::Indexing {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(ready.sources[0].status, KnowledgeSourceState::Ready);
    assert_eq!(ready.sources[0].document_count, 1);
    let status = fixture
        .service
        .status(&fixture.state, &fixture.project, &fixture.agent)
        .await
        .unwrap();
    assert_eq!(status.sources[0].generation, ready.sources[0].generation);
    std::fs::rename(
        fixture.source.path().join("notes.txt"),
        fixture.source.path().join("other.txt"),
    )
    .unwrap();
    assert_eq!(
        fixture
            .service
            .status(&fixture.state, &fixture.project, &fixture.agent)
            .await
            .unwrap()
            .sources[0]
            .generation,
        ready.sources[0].generation,
        "status polling does not repeatedly reindex known sources"
    );
}

fn add_folder(state: &mut DaemonState, project: &str, source: &std::path::Path) -> String {
    let grant = folder::capture_folder(source, &[]).unwrap();
    let resource = ProjectResourceRecord {
        id: format!("resource_{}", uuid::Uuid::new_v4()),
        target: ProjectResourceTarget::KnowledgeFolder {
            root_path: grant.root_path,
            root_identity: grant.root_identity,
        },
        role: ProjectResourceRole::Reference,
        label: None,
        use_in_context: true,
        created_at_ms: 2,
        add_replay: ProjectMutationReplay {
            request_key: uuid::Uuid::new_v4().to_string(),
            request_sha256: text_digest("selected folder"),
        },
        donor: None,
        source_project_binding_revision: None,
        workspace_key: None,
    };
    let id = resource.id.clone();
    let revision = state.projects.get(project).unwrap().revision;
    state
        .projects
        .add_resource(project, revision, resource, 3)
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    id
}

fn fixture(text: &str) -> Fixture {
    let private = private_directory();
    let service = KnowledgeService::open(private.path().to_owned(), None).unwrap();
    let source = tempfile::Builder::new()
        .prefix("anima-knowledge-source-")
        .tempdir()
        .unwrap();
    std::fs::write(source.path().join("notes.txt"), text).unwrap();
    let mut state = DaemonState::new();
    let agent = state.create_agent(agent_config("Reader")).unwrap().state.id;
    let outsider = state
        .create_agent(agent_config("Outside"))
        .unwrap()
        .state
        .id;
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: uuid::Uuid::new_v4().to_string(),
                title: "Document knowledge".into(),
                member_agent_ids: vec![agent.clone()],
                instructions: String::new(),
            },
            1,
        )
        .unwrap();
    let resource = add_folder(&mut state, &project.id, source.path());
    state.project_knowledge = service.clone();
    Fixture {
        service,
        state: Arc::new(tokio::sync::RwLock::new(state)),
        project: project.id,
        agent,
        outsider,
        resource,
        source,
        _private: private,
    }
}

async fn search(fixture: &Fixture, query: &str) -> KnowledgeSearch {
    fixture
        .service
        .search(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            query,
            8,
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn keyword_fallback_does_not_claim_learned_project_search() {
    let fixture = fixture("NIGHTJAR clause 9.4 retains exact keyword search");
    let found = search(&fixture, "NIGHTJAR").await;
    assert_eq!(found.hits.len(), 1);
    assert_eq!(
        found.retrieval.mode,
        super::super::types::KnowledgeSearchMode::Keyword
    );
    assert_eq!(
        found.retrieval.semantic_state,
        super::super::types::SemanticSearchState::Unavailable
    );
    assert!(found.retrieval.provider.is_none());
    assert!(found.retrieval.model.is_none());
    assert_eq!(found.retrieval.indexed_windows, 0);
}

#[tokio::test]
async fn busy_memory_runtime_cannot_block_project_keyword_search() {
    let fixture = fixture("NIGHTJAR keyword evidence remains available");
    let memory = fixture.state.read().await.memory_embeddings_handle();
    let _busy = memory.write().await;
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        fixture.service.search(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            "NIGHTJAR",
            8,
            CancelSignal::new(),
            Some(Instant::now() + Duration::from_secs(1)),
        ),
    )
    .await
    .expect("a busy memory write must not hold project search past its deadline")
    .unwrap();
    assert_eq!(response.hits.len(), 1);
    assert_eq!(
        response.retrieval.mode,
        super::super::types::KnowledgeSearchMode::Keyword
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires explicit cached local E5 files; no download or remote provider"]
async fn offline_learned_search_uses_current_project_witnesses() {
    let snapshot = std::env::var("ANIMAOS_PROJECT_SEARCH_OFFLINE_MODEL_DIR")
        .expect("provide a complete local E5 snapshot");
    let runtime = tokio::task::spawn_blocking(move || {
        let runtime =
            crate::memory_embeddings::offline_project_runtime(std::path::Path::new(&snapshot))
                .unwrap();
        runtime
            .local_project_embedder()
            .unwrap()
            .embed_query("warmup")
            .unwrap();
        runtime
    })
    .await
    .unwrap();
    let fixture = fixture("Either party may terminate this agreement upon thirty days written notice. Outstanding invoices remain payable following cancellation.");
    fixture
        .state
        .write()
        .await
        .replace_memory_embeddings(runtime);
    let found = search(&fixture, "How can we back out?").await;
    assert_eq!(
        found.retrieval.mode,
        super::super::types::KnowledgeSearchMode::Hybrid
    );
    assert_eq!(found.hits.len(), 1);
    let citation = found.hits[0].citation_id.clone();
    assert!(fixture
        .service
        .citation(&fixture.state, &fixture.project, &fixture.agent, &citation)
        .await
        .unwrap()
        .text
        .contains("terminate"));
    let project = fixture
        .state
        .read()
        .await
        .projects
        .get(&fixture.project)
        .unwrap()
        .clone();
    let automatic = fixture
        .service
        .retrieve(
            &fixture.state,
            &project,
            &fixture.agent,
            "How can we back out?",
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(automatic.passages[0].citation_id, citation);
    std::fs::write(
        fixture.source.path().join("notes.txt"),
        "The replacement document describes conference catering.",
    )
    .unwrap();
    assert!(fixture
        .service
        .citation(&fixture.state, &fixture.project, &fixture.agent, &citation)
        .await
        .is_err());
    let changed = search(&fixture, "How can we back out?").await;
    assert!(changed.hits.iter().all(|hit| hit.citation_id != citation));
    assert!(fixture
        .service
        .search(
            &fixture.state,
            &fixture.project,
            &fixture.outsider,
            "How can we back out?",
            8,
            CancelSignal::new(),
            None
        )
        .await
        .is_err());
}

#[tokio::test]
async fn project_knowledge_finds_late_passages_and_rejects_outsiders() {
    let fixture = fixture(&format!(
        "{} cobalt harbour latepassage",
        "ordinary background ".repeat(4000)
    ));
    let found = search(&fixture, "latepassage").await;
    assert_eq!(found.hits.len(), 1);
    assert!(found.hits[0].excerpt.contains("latepassage"));
    let cited = fixture
        .service
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &found.hits[0].citation_id,
        )
        .await
        .unwrap();
    assert!(cited.text.contains("latepassage"));
    assert_eq!(cited.hit.resource_id, fixture.resource);
    assert!(matches!(
        fixture
            .service
            .search(
                &fixture.state,
                &fixture.project,
                &fixture.outsider,
                "latepassage",
                8,
                CancelSignal::new(),
                None
            )
            .await,
        Err(ProjectError::NotMember)
    ));
}

#[tokio::test]
async fn project_knowledge_reindexes_same_length_changes_and_deletions() {
    let fixture = fixture("cobalt harbour 7319");
    let old = search(&fixture, "cobalt").await.hits.remove(0);
    std::fs::write(
        fixture.source.path().join("notes.txt"),
        "silver harbour 8420",
    )
    .unwrap();
    assert!(fixture
        .service
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &old.citation_id
        )
        .await
        .is_err());
    assert!(search(&fixture, "cobalt").await.hits.is_empty());
    assert_eq!(search(&fixture, "silver").await.hits.len(), 1);
    std::fs::remove_file(fixture.source.path().join("notes.txt")).unwrap();
    assert!(search(&fixture, "silver").await.hits.is_empty());
}

#[tokio::test]
async fn project_knowledge_removed_and_readded_source_never_revives_old_citations() {
    let fixture = fixture("cobalt harbour source evidence");
    let old = search(&fixture, "cobalt").await.hits.remove(0);
    let project = fixture
        .state
        .read()
        .await
        .projects
        .get(&fixture.project)
        .unwrap()
        .clone();
    let retrieval = fixture
        .service
        .retrieve(
            &fixture.state,
            &project,
            &fixture.agent,
            "cobalt",
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
    {
        let mut state = fixture.state.write().await;
        let revision = state.projects.get(&fixture.project).unwrap().revision;
        state
            .projects
            .remove_resource(&fixture.project, revision, &fixture.resource, 4)
            .unwrap();
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
    }
    assert!(retrieval.witness.check(&fixture.state).await.is_err());
    assert!(fixture
        .service
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &old.citation_id
        )
        .await
        .is_err());
    {
        let mut state = fixture.state.write().await;
        add_folder(&mut state, &fixture.project, fixture.source.path());
    }
    let new = search(&fixture, "cobalt").await.hits.remove(0);
    assert_ne!(old.resource_id, new.resource_id);
    assert_ne!(old.citation_id, new.citation_id);
    assert!(fixture
        .service
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &old.citation_id
        )
        .await
        .is_err());
}

#[tokio::test]
async fn project_knowledge_sensitive_text_is_omitted_and_witness_detects_later_edits() {
    let fixture = fixture("ordinary cobalt evidence");
    std::fs::write(
        fixture.source.path().join("unsafe.txt"),
        "password=hunter2 cobalt",
    )
    .unwrap();
    let found = search(&fixture, "cobalt").await;
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.binding.sources[0].omitted_count, 1);
    assert_eq!(
        found.binding.sources[0].status,
        KnowledgeSourceState::Partial
    );
    let project = fixture
        .state
        .read()
        .await
        .projects
        .get(&fixture.project)
        .unwrap()
        .clone();
    let retrieval = fixture
        .service
        .retrieve(
            &fixture.state,
            &project,
            &fixture.agent,
            "cobalt",
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
    std::fs::write(
        fixture.source.path().join("notes.txt"),
        "replaced cobalt evidence",
    )
    .unwrap();
    assert!(matches!(
        retrieval.witness.check(&fixture.state).await,
        Err(ProjectError::ScopeChanged)
    ));
}

#[tokio::test]
async fn project_knowledge_cache_reopen_still_requires_current_files() {
    let fixture = fixture("cobalt persisted evidence");
    let found = search(&fixture, "cobalt").await;
    let reopened = KnowledgeService::open(fixture._private.path().to_owned(), None).unwrap();
    std::fs::remove_file(fixture.source.path().join("notes.txt")).unwrap();
    assert!(reopened
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &found.hits[0].citation_id
        )
        .await
        .is_err());
    assert!(reopened
        .search(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            "cobalt",
            8,
            CancelSignal::new(),
            None
        )
        .await
        .unwrap()
        .hits
        .is_empty());
}

#[tokio::test]
async fn project_knowledge_context_exclusion_fences_old_witness_but_manual_search_remains_readable()
{
    let fixture = fixture("cobalt selected folder");
    let project = fixture
        .state
        .read()
        .await
        .projects
        .get(&fixture.project)
        .unwrap()
        .clone();
    let admitted = fixture
        .service
        .retrieve(
            &fixture.state,
            &project,
            &fixture.agent,
            "cobalt",
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(admitted.passages.len(), 1);
    let project = {
        let mut state = fixture.state.write().await;
        let revision = state.projects.get(&fixture.project).unwrap().revision;
        let project = state
            .projects
            .patch_resource(
                &fixture.project,
                revision,
                &fixture.resource,
                None,
                None,
                Some(false),
                4,
            )
            .unwrap();
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
        project
    };
    assert!(admitted.witness.check(&fixture.state).await.is_err());
    let excluded = fixture
        .service
        .retrieve(
            &fixture.state,
            &project,
            &fixture.agent,
            "cobalt",
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
    assert!(excluded.passages.is_empty());
    assert_eq!(search(&fixture, "cobalt").await.hits.len(), 1);
}

#[test]
fn pdf_citation_page_locator_keeps_original_page_and_refuses_other_formats() {
    assert_eq!(pdf_page_locator("pdf#page=7;byte=4096"), Some(7));
    assert_eq!(pdf_page_locator("pdf#page=100"), Some(100));
    for locator in [
        "pdf#page=0",
        "pdf#page=101",
        "pdf#page=-1",
        "pdf#page=1.0",
        "text#lines=1-2",
        "pdf#page=1/path",
    ] {
        assert_eq!(pdf_page_locator(locator), None);
    }
}

struct ScanKnowledgeExtractor;
#[async_trait::async_trait]
impl DocumentExtractor for ScanKnowledgeExtractor {
    async fn extract(
        &self,
        _: anima_service_adapters::document_content::DocumentFormat,
        _: Vec<u8>,
        _: CancelSignal,
        _: Option<Instant>,
    ) -> Result<anima_service_adapters::document_content::ExtractedDocumentText, WorkerFailure>
    {
        Err(WorkerFailure::Unavailable)
    }

    async fn extract_knowledge(
        &self,
        format: anima_service_adapters::document_content::DocumentFormat,
        _: Vec<u8>,
        _: CancelSignal,
        _: Option<Instant>,
    ) -> Result<anima_service_adapters::document_content::ExtractedDocumentText, WorkerFailure>
    {
        use anima_service_adapters::document_content::*;
        let mut value = ExtractedDocumentText::empty_for(format, DocumentExtractionStatus::NoText);
        value.omissions.push(DocumentOmission::TextLayerOnly);
        value.pdf_coverage = Some(PdfCoverage {
            page_count: 2,
            text_page_count: 0,
            included_page_count: 0,
        });
        Ok(value)
    }

    async fn inspect_pdf(
        &self,
        bytes: Vec<u8>,
        pages: Vec<u32>,
        _: CancelSignal,
        _: Option<Instant>,
    ) -> Result<anima_document_visual::PdfEvidence, WorkerFailure> {
        use base64::Engine;
        Ok(anima_document_visual::PdfEvidence {
            version: 1, source_sha256: anima_document_store_sqlite::source_sha256(&bytes),
            page_count: 2, pages: pages.into_iter().map(|page| anima_document_visual::PdfPageEvidence {
                page, width: 1, height: 1,
                png: base64::engine::general_purpose::STANDARD.decode(
                    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGP4DwABAQEAsTj2FAAAAABJRU5ErkJggg=="
                ).unwrap(),
                recognized: if page == 1 {
                    anima_document_visual::RecognizedText::Text { text: "scanrecognitionmarker cobalt harbour".into(), language: "en-US".into() }
                } else { anima_document_visual::RecognizedText::Unavailable },
            }).collect(),
        })
    }
}

#[tokio::test]
async fn scanned_project_knowledge_preserves_recognition_and_page_only_citation_provenance() {
    let mut fixture = fixture("ordinary notes");
    let bytes = include_bytes!("../../../tests/fixtures/documents/m55-selected.pdf");
    std::fs::write(fixture.source.path().join("scan.pdf"), bytes).unwrap();
    fixture.service = KnowledgeService::with_cache_and_extractor(
        fixture.service.inner.cache.clone(),
        Arc::new(ScanKnowledgeExtractor),
    );
    fixture.state.write().await.project_knowledge = fixture.service.clone();
    let found = search(&fixture, "scanrecognitionmarker").await;
    assert_eq!(found.hits.len(), 1);
    assert!(matches!(
        found.hits[0].recognition,
        Some(anima_document_store_sqlite::PassageOrigin::Ocr { .. })
    ));
    let citation = fixture
        .service
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &found.hits[0].citation_id,
        )
        .await
        .unwrap();
    assert!(citation.text.contains("scanrecognitionmarker"));
    let by_title = search(&fixture, "scan").await;
    let page_only = by_title
        .hits
        .iter()
        .find(|hit| {
            matches!(
                hit.recognition,
                Some(anima_document_store_sqlite::PassageOrigin::PageImage { .. })
            )
        })
        .unwrap();
    let page_citation = fixture
        .service
        .citation(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &page_only.citation_id,
        )
        .await
        .unwrap();
    assert!(page_citation.text.is_empty());
    let png = fixture
        .service
        .citation_page(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &page_only.citation_id,
        )
        .await
        .unwrap();
    assert!(png.starts_with(b"\x89PNG"));
    std::fs::write(fixture.source.path().join("scan.pdf"), b"changed source").unwrap();
    assert!(fixture
        .service
        .citation_page(
            &fixture.state,
            &fixture.project,
            &fixture.agent,
            &page_only.citation_id
        )
        .await
        .is_err());
}
