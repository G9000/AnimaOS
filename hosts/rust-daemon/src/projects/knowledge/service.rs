//! Bounded indexing jobs, current-source search and reauthorized citation reads.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anima_core::CancelSignal;
use anima_document_store_sqlite::{DocumentInput, SearchHit};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use super::cache::Cache;
use super::extraction;
use super::folder::{self, FolderSnapshot};
use super::scope::{DocumentWitness, KnowledgeWitness, Purpose, Scope, Source, SourceGrant};
use super::types::{
    KnowledgeCitation, KnowledgeHit, KnowledgeSearch, KnowledgeSourceState, KnowledgeSourceStatus,
    KnowledgeStatus,
};
use crate::app::SharedDaemonState;
use crate::attachments::documents::source_read::read_retained_with_slots;
use crate::attachments::documents::worker::{DocumentExtractor, DocumentWorker, WorkerFailure};
use crate::profile_owner::ProfileOwner;
use crate::projects::{ProjectError, ProjectRecord, ProjectResourceTarget};

const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);
const BUSY_BACKOFF: Duration = Duration::from_millis(50);
const REMOTE_SOURCE_DEADLINE: Duration = Duration::from_secs(5);
const REMOTE_PASS_RESERVE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum IndexRefresh {
    Reuse,
    Force,
}

#[derive(Clone)]
pub(crate) struct KnowledgeService {
    inner: Arc<Inner>,
}

struct Inner {
    cache: Option<Arc<Cache>>,
    jobs: Arc<Semaphore>,
    reads: Arc<Semaphore>,
    queue_capacity: Arc<Semaphore>,
    pending: Mutex<BTreeSet<(String, u64, String, IndexRefresh)>>,
    extractor: Arc<dyn DocumentExtractor>,
    sources: Mutex<BTreeMap<(String, String), IndexedSource>>,
}

#[derive(Clone)]
struct IndexedSource {
    scope_revision: u64,
    fingerprint: Option<String>,
    status: KnowledgeSourceStatus,
}

struct QueueTicket {
    inner: Arc<Inner>,
    key: (String, u64, String, IndexRefresh),
    _capacity: OwnedSemaphorePermit,
}

impl Drop for QueueTicket {
    fn drop(&mut self) {
        self.inner
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&self.key);
    }
}

pub(crate) struct KnowledgeRetrieval {
    pub(crate) passages: Vec<KnowledgeHit>,
    pub(crate) sources: Vec<KnowledgeSourceStatus>,
    pub(crate) witness: KnowledgeWitness,
}

impl std::fmt::Debug for KnowledgeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeService")
            .field("enabled", &self.inner.cache.is_some())
            .finish()
    }
}

impl Default for KnowledgeService {
    fn default() -> Self {
        Self::disabled()
    }
}

impl KnowledgeService {
    pub(crate) fn disabled() -> Self {
        Self::with_cache(None)
    }

    pub(crate) fn open(root: PathBuf, owner: Option<ProfileOwner>) -> Result<Self, String> {
        Ok(Self::with_cache(Some(Cache::open(root, owner)?)))
    }

    pub(crate) fn cache_root(&self) -> Result<Option<PathBuf>, String> {
        self.inner
            .cache
            .as_ref()
            .map(|cache| cache.root_path())
            .transpose()
    }

    fn with_cache(cache: Option<Arc<Cache>>) -> Self {
        Self::with_cache_and_extractor(cache, Arc::new(DocumentWorker))
    }

    fn with_cache_and_extractor(
        cache: Option<Arc<Cache>>,
        extractor: Arc<dyn DocumentExtractor>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                cache,
                jobs: Arc::new(Semaphore::new(1)),
                reads: Arc::new(Semaphore::new(2)),
                queue_capacity: Arc::new(Semaphore::new(32)),
                pending: Mutex::new(BTreeSet::new()),
                extractor,
                sources: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub(crate) async fn status(
        &self,
        state: &SharedDaemonState,
        project_id: &str,
        agent: &str,
    ) -> Result<KnowledgeStatus, ProjectError> {
        let scope = Scope::capture(state, project_id, agent, Purpose::Manual).await?;
        self.reconcile(state, project_id).await?;
        scope.check(state).await?;
        for source in &scope.sources {
            if let SourceGrant::Connected(grant) = &source.grant {
                if scope
                    .check_source_state(&*state.read().await, source)
                    .is_err()
                    || crate::projects::connected::check(&*state.read().await, grant).is_err()
                {
                    self.mark_source_unavailable(&scope, source);
                }
                continue;
            }
            let physical_scope = scope.clone();
            let checked_source = source.clone();
            let health = read_retained_with_slots(
                move || physical_scope.check_source(&checked_source),
                &CancelSignal::new(),
                Instant::now() + Duration::from_secs(5),
                self.inner.reads.clone(),
            )
            .await;
            scope.check(state).await?;
            if matches!(
                health,
                Ok(Err(_))
                    | Err(WorkerFailure::Unavailable
                        | WorkerFailure::TimedOut
                        | WorkerFailure::InvalidOutput)
            ) {
                self.mark_source_unavailable(&scope, source);
            }
        }
        if self.inner.cache.is_some() {
            let mut pending = scope.clone();
            {
                let sources = self
                    .inner
                    .sources
                    .lock()
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                pending.sources.retain(|source| {
                    !sources.contains_key(&(scope.project.id.clone(), source.id.clone()))
                });
            }
            if !pending.sources.is_empty() {
                let _ = self.schedule_job(state, pending, IndexRefresh::Reuse);
            }
        }
        scope.check(state).await?;
        Ok(self.projection(&scope))
    }

    pub(crate) async fn refresh(
        &self,
        state: &SharedDaemonState,
        project_id: &str,
        agent: &str,
        resource_id: Option<&str>,
    ) -> Result<KnowledgeStatus, ProjectError> {
        let mut scope = Scope::capture(state, project_id, agent, Purpose::Manual).await?;
        if let Some(id) = resource_id {
            scope.sources.retain(|source| source.id == id);
            if scope.sources.is_empty() {
                return Err(ProjectError::NotFound);
            }
        }
        self.cache()?;
        scope.check(state).await?;
        self.schedule_job(state, scope.clone(), IndexRefresh::Force)?;
        scope.check(state).await?;
        Ok(self.projection(&scope))
    }

    fn schedule_job(
        &self,
        state: &SharedDaemonState,
        job_scope: Scope,
        refresh: IndexRefresh,
    ) -> Result<(), ProjectError> {
        let key = (
            job_scope.project.id.clone(),
            job_scope.project.revision,
            job_scope
                .sources
                .iter()
                .map(|source| source.id.as_str())
                .collect::<Vec<_>>()
                .join(","),
            refresh,
        );
        let ticket = {
            let mut pending = self
                .inner
                .pending
                .lock()
                .map_err(|_| ProjectError::AccessUnavailable)?;
            if pending.contains(&key) {
                return Ok(());
            }
            let capacity = self
                .inner
                .queue_capacity
                .clone()
                .try_acquire_owned()
                .map_err(|_| ProjectError::Busy)?;
            pending.insert(key.clone());
            QueueTicket {
                inner: self.inner.clone(),
                key,
                _capacity: capacity,
            }
        };
        self.mark_indexing(&job_scope);
        let service = self.clone();
        let shared = state.clone();
        tokio::spawn(async move {
            let _ticket = ticket;
            let cancel = CancelSignal::new();
            let deadline = Instant::now() + DEFAULT_DEADLINE;
            let indexed = async {
                let _permit = service.acquire_job(&job_scope, &cancel, deadline).await?;
                job_scope.check(&shared).await?;
                service
                    .index_sources(&shared, &job_scope, &cancel, deadline, refresh)
                    .await
            }
            .await;
            if indexed.is_err() {
                service.mark_unavailable(&job_scope);
            }
        });
        Ok(())
    }

    pub(crate) async fn search(
        &self,
        state: &SharedDaemonState,
        project_id: &str,
        agent: &str,
        query: &str,
        limit: usize,
        cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<KnowledgeSearch, ProjectError> {
        validate_query(query, limit)?;
        let scope = Scope::capture(state, project_id, agent, Purpose::Manual).await?;
        let (hits, witness) = self
            .search_scope(state, &scope, query, limit, cancel, deadline)
            .await?;
        witness.check(state).await?;
        Ok(KnowledgeSearch {
            binding: self.projection(&scope),
            hits: hits
                .into_iter()
                .map(|hit| public_hit(hit, &scope))
                .collect(),
        })
    }

    pub(crate) async fn retrieve(
        &self,
        state: &SharedDaemonState,
        project: &ProjectRecord,
        agent: &str,
        query: &str,
        cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<KnowledgeRetrieval, ProjectError> {
        validate_query(query, 8)?;
        let scope = Scope::capture(state, &project.id, agent, Purpose::Automatic).await?;
        if &scope.project != project {
            return Err(ProjectError::ScopeChanged);
        }
        let (hits, witness) = self
            .search_scope(state, &scope, query, 8, cancel, deadline)
            .await?;
        witness.check(state).await?;
        Ok(KnowledgeRetrieval {
            passages: hits
                .into_iter()
                .map(|hit| public_hit(hit, &scope))
                .collect(),
            sources: self.projection(&scope).sources,
            witness,
        })
    }

    pub(crate) async fn citation(
        &self,
        state: &SharedDaemonState,
        project_id: &str,
        agent: &str,
        citation_id: &str,
    ) -> Result<KnowledgeCitation, ProjectError> {
        if citation_id.len() > 128
            || !citation_id.starts_with("cite_")
            || citation_id.chars().any(char::is_control)
        {
            return Err(ProjectError::NotFound);
        }
        let scope = Scope::capture(state, project_id, agent, Purpose::Manual).await?;
        let cache = self.cache()?;
        let project_id = project_id.to_owned();
        let citation_id = citation_id.to_owned();
        let physical_scope = scope.clone();
        let shared = state.clone();
        let hit = read_retained_with_slots(
            move || {
                let state = shared.blocking_read();
                physical_scope.check_state(&state)?;
                cache
                    .run(|store| store.citation(&project_id, &citation_id))
                    .map_err(|_| ProjectError::AccessUnavailable)
            },
            &CancelSignal::new(),
            Instant::now() + Duration::from_secs(10),
            self.inner.reads.clone(),
        )
        .await
        .map_err(worker_error)??
        .ok_or(ProjectError::NotFound)?;
        scope.check(state).await?;
        let witness = self.witness(
            &scope,
            std::slice::from_ref(&hit),
            &CancelSignal::new(),
            Instant::now() + Duration::from_secs(10),
        )?;
        witness.check(state).await?;
        let text = hit.text.clone();
        Ok(KnowledgeCitation {
            project_revision: scope.project.revision,
            access_epoch: scope.project.access_epoch,
            member_generation: scope.member_generation,
            hit: public_hit(hit, &scope),
            text,
        })
    }

    pub(crate) async fn citation_page(
        &self,
        state: &SharedDaemonState,
        project_id: &str,
        agent: &str,
        citation_id: &str,
    ) -> Result<Vec<u8>, ProjectError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        if citation_id.len() > 128
            || !citation_id.starts_with("cite_")
            || citation_id.chars().any(char::is_control)
        {
            return Err(ProjectError::NotFound);
        }
        let scope = tokio::time::timeout_at(
            deadline,
            Scope::capture(state, project_id, agent, Purpose::Manual),
        )
        .await
        .map_err(|_| ProjectError::Deadline)??;
        let cancel = CancelSignal::new();
        let cache = self.cache()?;
        let project = project_id.to_owned();
        let id = citation_id.to_owned();
        let hit = read_retained_with_slots(
            move || cache.run(|store| store.citation(&project, &id)),
            &cancel,
            deadline,
            self.inner.reads.clone(),
        )
        .await
        .map_err(worker_error)?
        .map_err(|_| ProjectError::AccessUnavailable)?
        .ok_or(ProjectError::NotFound)?;
        let page = pdf_page_locator(&hit.locator).ok_or(ProjectError::AccessUnavailable)?;
        let witness = self.witness(&scope, std::slice::from_ref(&hit), &cancel, deadline)?;
        witness.check(state).await?;
        let source = scope
            .sources
            .iter()
            .find(|source| source.id == hit.source_id)
            .ok_or(ProjectError::AccessRevoked)?
            .clone();
        let bytes = match &source.grant {
            SourceGrant::Folder(grant) => {
                let grant = grant.clone();
                let relative = hit.relative_path.clone();
                let digest = hit.source_sha256.clone();
                let physical_scope = scope.clone();
                let physical_source = source.clone();
                read_retained_with_slots(
                    move || {
                        physical_scope.check_source(&physical_source)?;
                        folder::read_document(&grant, &relative, &digest)
                            .map_err(|_| ProjectError::ScopeChanged)
                    },
                    &cancel,
                    deadline,
                    self.inner.reads.clone(),
                )
                .await
                .map_err(worker_error)??
            }
            SourceGrant::Connected(_) => {
                let snapshot = scope
                    .read_connected_retained(
                        state,
                        &source,
                        &cancel,
                        deadline,
                        self.inner.reads.clone(),
                    )
                    .await?;
                if snapshot.name != hit.relative_path || snapshot.source_sha256 != hit.source_sha256
                {
                    return Err(ProjectError::ScopeChanged);
                }
                snapshot.bytes
            }
        };
        scope.check(state).await?;
        let evidence = tokio::select! { biased;
            () = scope.lease.cancelled() => return Err(ProjectError::AccessRevoked),
            () = tokio::time::sleep_until(deadline) => return Err(ProjectError::Deadline),
            value = self.inner.extractor.inspect_pdf(bytes, vec![page], cancel.clone(), Some(deadline)) => value.map_err(worker_error)?,
        };
        if evidence.source_sha256 != hit.source_sha256
            || evidence.pages.len() != 1
            || evidence.pages[0].page != page
            || anima_document_visual::validate_pdf_evidence(&evidence).is_err()
        {
            return Err(ProjectError::AccessUnavailable);
        }
        witness.check(state).await?;
        scope.check(state).await?;
        Ok(evidence
            .pages
            .into_iter()
            .next()
            .ok_or(ProjectError::AccessUnavailable)?
            .png)
    }

    /// Retires derived rows against the currently published grants, including after restart.
    pub(crate) async fn reconcile(
        &self,
        state: &SharedDaemonState,
        project_id: &str,
    ) -> Result<(), ProjectError> {
        let Some(cache) = self.inner.cache.clone() else {
            return Ok(());
        };
        let shared = state.clone();
        let id = project_id.to_owned();
        let current = read_retained_with_slots(
            move || {
                let state = shared.blocking_read();
                state
                    .ensure_profile_owned()
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                let ids = state
                    .projects
                    .get(&id)
                    .map(|project| {
                        project
                            .resources
                            .iter()
                            .filter_map(|resource| {
                                crate::projects::connected::is_knowledge(&resource.target)
                                    .then_some(resource.id.clone())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                cache
                    .run(|store| store.retain_sources(&id, &ids))
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                Ok::<_, ProjectError>(ids)
            },
            &CancelSignal::new(),
            Instant::now() + Duration::from_secs(10),
            self.inner.reads.clone(),
        )
        .await
        .map_err(worker_error)??;
        let allowed = current.into_iter().collect::<BTreeSet<_>>();
        self.inner
            .sources
            .lock()
            .map_err(|_| ProjectError::AccessUnavailable)?
            .retain(|(project, source), _| project != project_id || allowed.contains(source));
        Ok(())
    }

    async fn search_scope(
        &self,
        state: &SharedDaemonState,
        scope: &Scope,
        query: &str,
        limit: usize,
        cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<(Vec<SearchHit>, KnowledgeWitness), ProjectError> {
        let deadline = deadline
            .unwrap_or_else(|| Instant::now() + DEFAULT_DEADLINE)
            .min(Instant::now() + DEFAULT_DEADLINE);
        check(&cancel, deadline)?;
        if scope.sources.is_empty() {
            return Ok((Vec::new(), self.witness(scope, &[], &cancel, deadline)?));
        }
        self.reconcile(state, &scope.project.id).await?;
        scope.check(state).await?;
        let _permit = self.acquire_job(scope, &cancel, deadline).await?;
        scope.check(state).await?;
        self.mark_indexing(scope);
        let ready_sources = match self
            .index_sources(state, scope, &cancel, deadline, IndexRefresh::Reuse)
            .await
        {
            Ok(ids) => ids,
            Err(error) => {
                self.mark_unavailable(scope);
                return Err(error);
            }
        };
        let cache = self.cache()?;
        let physical_scope = scope.clone();
        let shared = state.clone();
        let query = query.to_owned();
        let hits = read_retained_with_slots(
            move || {
                let state = shared.blocking_read();
                physical_scope.check_state(&state)?;
                cache
                    .run(|store| {
                        store.search(&physical_scope.project.id, &ready_sources, &query, limit)
                    })
                    .map_err(|_| ProjectError::AccessUnavailable)
            },
            &cancel,
            deadline,
            self.inner.reads.clone(),
        )
        .await
        .map_err(worker_error)??;
        check(&cancel, deadline)?;
        scope.check(state).await?;
        let witness = self.witness(scope, &hits, &cancel, deadline)?;
        Ok((hits, witness))
    }

    async fn acquire_job(
        &self,
        scope: &Scope,
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<OwnedSemaphorePermit, ProjectError> {
        loop {
            scope.check_physical()?;
            check(cancel, deadline)?;
            if let Ok(permit) = self.inner.jobs.clone().try_acquire_owned() {
                return Ok(permit);
            }
            pause(scope, cancel, deadline).await?;
        }
    }

    async fn snapshot(
        &self,
        state: &SharedDaemonState,
        scope: &Scope,
        source: &Source,
        cancel: &CancelSignal,
        deadline: Instant,
        budget: usize,
    ) -> Result<FolderSnapshot, ProjectError> {
        if let SourceGrant::Connected(_) = &source.grant {
            scope.check_source_state(&*state.read().await, source)?;
            if deadline.saturating_duration_since(Instant::now()) <= REMOTE_PASS_RESERVE {
                return Err(ProjectError::AccessUnavailable);
            }
            // Leave time to return already verified healthy sources after a remote timeout.
            let source_deadline =
                (deadline - REMOTE_PASS_RESERVE).min(Instant::now() + REMOTE_SOURCE_DEADLINE);
            let snapshot = scope
                .read_connected_retained(
                    state,
                    source,
                    cancel,
                    source_deadline,
                    self.inner.reads.clone(),
                )
                .await
                .map_err(|error| match error {
                    ProjectError::Deadline | ProjectError::Busy => ProjectError::AccessUnavailable,
                    error => error,
                })?;
            if snapshot.bytes.len() > budget {
                return Ok(FolderSnapshot {
                    documents: Vec::new(),
                    omitted_count: 1,
                });
            }
            return Ok(FolderSnapshot {
                documents: vec![super::folder::FolderDocument {
                    relative_path: snapshot.name,
                    bytes: snapshot.bytes,
                    sha256: snapshot.source_sha256,
                }],
                omitted_count: usize::from(snapshot.partial),
            });
        }
        loop {
            scope.check_physical()?;
            check(cancel, deadline)?;
            let physical_scope = scope.clone();
            let SourceGrant::Folder(grant) = source.grant.clone() else {
                return Err(ProjectError::AccessUnavailable);
            };
            let checked_source = source.clone();
            let signal = cancel.clone();
            let result = read_retained_with_slots(
                move || {
                    physical_scope.check_source(&checked_source)?;
                    folder::scan_folder_with_budget(&grant, &signal, budget, || {
                        physical_scope
                            .check_physical()
                            .map_err(|error| error.to_string())?;
                        check(&signal, deadline).map_err(|error| error.to_string())
                    })
                    .map_err(|_| ProjectError::AccessUnavailable)
                },
                cancel,
                deadline,
                self.inner.reads.clone(),
            )
            .await;
            match result {
                Err(WorkerFailure::Busy) => pause(scope, cancel, deadline).await?,
                result => return result.map_err(worker_error)?,
            }
        }
    }

    async fn index_sources(
        &self,
        state: &SharedDaemonState,
        scope: &Scope,
        cancel: &CancelSignal,
        deadline: Instant,
        refresh: IndexRefresh,
    ) -> Result<Vec<String>, ProjectError> {
        let mut remaining = folder::MAX_PASS_BYTES;
        let mut ready_sources = Vec::new();
        let mut recognition_pages_left = 2usize;
        for source in &scope.sources {
            scope.check(state).await?;
            let snapshot = self
                .snapshot(state, scope, source, cancel, deadline, remaining)
                .await;
            scope.check(state).await?;
            check(cancel, deadline)?;
            let snapshot = match snapshot {
                Ok(value) => value,
                Err(ProjectError::AccessUnavailable | ProjectError::AccessRevoked) => {
                    self.mark_source_unavailable(scope, source);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let read_bytes = snapshot
                .documents
                .iter()
                .map(|document| document.bytes.len())
                .sum::<usize>();
            remaining = remaining.saturating_sub(read_bytes);
            let manifest = fingerprint(&snapshot);
            let key = (scope.project.id.clone(), source.id.clone());
            let unchanged = refresh == IndexRefresh::Reuse
                && self
                    .inner
                    .sources
                    .lock()
                    .map_err(|_| ProjectError::AccessUnavailable)?
                    .get(&key)
                    .is_some_and(|entry| entry.fingerprint.as_ref() == Some(&manifest));
            if unchanged {
                let mut statuses = self
                    .inner
                    .sources
                    .lock()
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                if let Some(entry) = statuses.get_mut(&key) {
                    entry.status.status = if entry.status.omitted_count == 0 {
                        KnowledgeSourceState::Ready
                    } else {
                        KnowledgeSourceState::Partial
                    };
                    entry.status.error = None;
                }
                ready_sources.push(source.id.clone());
                continue;
            }
            let mut omitted = snapshot.omitted_count;
            let mut documents = Vec::new();
            let mut extracted_bytes = 0usize;
            for document in snapshot.documents {
                scope.check(state).await?;
                check(cancel, deadline)?;
                let chunks = if let Some(format) =
                    extraction::document_format(&document.relative_path)
                {
                    let extracted = loop {
                        // Background indexing yields between attempts to the shared interactive Office worker.
                        tokio::task::yield_now().await;
                        scope.check(state).await?;
                        let result = self
                            .inner
                            .extractor
                            .extract_knowledge(
                                format,
                                document.bytes.clone(),
                                cancel.clone(),
                                Some(deadline),
                            )
                            .await;
                        scope.check(state).await?;
                        match result {
                            Err(WorkerFailure::Busy) => pause(scope, cancel, deadline).await?,
                            result => break result,
                        }
                    };
                    match extracted {
                        Ok(value) => {
                            omitted += value.omissions.len();
                            let mut chunks = extraction::extracted_chunks(&value);
                            if let (Ok(chunks), Some(coverage)) = (&mut chunks, value.pdf_coverage)
                            {
                                let pages = (1..=coverage.page_count).filter(|page| {
                                        !value.blocks.iter().any(|block| matches!(block.locator,
                                            anima_service_adapters::document_content::DocumentLocator::PdfPage { page: found } if found == *page))
                                    }).take(2).collect::<Vec<_>>();
                                if !pages.is_empty() {
                                    let count = pages.len().min(recognition_pages_left);
                                    recognition_pages_left -= count;
                                    let inspected = if count > 0 {
                                        let requested =
                                            pages.iter().copied().take(count).collect::<Vec<_>>();
                                        let result = tokio::select! { biased;
                                            () = scope.lease.cancelled() => return Err(ProjectError::AccessRevoked),
                                            () = cancel.cancelled() => return Err(ProjectError::Cancelled),
                                            () = tokio::time::sleep_until(deadline) => return Err(ProjectError::Deadline),
                                            value = self.inner.extractor.inspect_pdf(document.bytes.clone(), requested.clone(), cancel.clone(), Some(deadline)) => value,
                                        };
                                        scope.check(state).await?;
                                        result.ok().and_then(|evidence| {
                                            extraction::visual_chunks(
                                                &evidence,
                                                &document.sha256,
                                                &requested,
                                            )
                                            .ok()
                                        })
                                    } else {
                                        None
                                    };
                                    let mut derived = inspected.unwrap_or_default();
                                    let represented = derived
                                        .iter()
                                        .filter_map(|chunk| pdf_page_locator(&chunk.locator))
                                        .collect::<BTreeSet<_>>();
                                    let missing = pages
                                        .into_iter()
                                        .filter(|page| !represented.contains(page))
                                        .collect::<Vec<_>>();
                                    if !missing.is_empty() {
                                        omitted += missing.len();
                                    }
                                    if let Ok(references) = extraction::page_references(&missing) {
                                        derived.extend(references);
                                    }
                                    chunks.extend(derived);
                                }
                            }
                            chunks
                        }
                        Err(WorkerFailure::Cancelled) => return Err(ProjectError::Cancelled),
                        Err(WorkerFailure::TimedOut) => return Err(ProjectError::Deadline),
                        Err(_) => Err("Knowledge extraction is unavailable".into()),
                    }
                } else {
                    std::str::from_utf8(&document.bytes)
                        .map_err(|_| "Knowledge text encoding is unsupported".to_owned())
                        .and_then(|text| extraction::text_chunks(text, "text#line=1"))
                };
                let Ok(chunks) = chunks else {
                    omitted += 1;
                    continue;
                };
                let bytes = chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>();
                if chunks.is_empty()
                    || extracted_bytes.saturating_add(bytes)
                        > anima_document_store_sqlite::MAX_SOURCE_TEXT_BYTES
                {
                    omitted += 1;
                    continue;
                }
                extracted_bytes += bytes;
                let mut title = document
                    .relative_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&document.relative_path)
                    .to_owned();
                while title.len() > 512 {
                    title.pop();
                }
                documents.push(DocumentInput {
                    document_id: format!(
                        "doc_{}",
                        crate::projects::records::text_digest(&document.relative_path)
                    ),
                    relative_path: document.relative_path,
                    title,
                    source_sha256: document.sha256,
                    chunks,
                });
            }
            scope.check(state).await?;
            let count = documents.len();
            let cache = self.cache()?;
            let shared = state.clone();
            let physical_scope = scope.clone();
            let source_id = source.id.clone();
            let checked_source = source.clone();
            let signal = cancel.clone();
            let expected_manifest = manifest.clone();
            if matches!(&source.grant, SourceGrant::Connected(_)) {
                match self
                    .snapshot(state, scope, source, cancel, deadline, read_bytes)
                    .await
                {
                    Ok(verified) if fingerprint(&verified) == expected_manifest => {}
                    Ok(_) | Err(ProjectError::AccessUnavailable | ProjectError::AccessRevoked) => {
                        self.mark_source_unavailable(scope, source);
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            let publication = read_retained_with_slots(
                move || {
                    let state = shared.blocking_read();
                    physical_scope.check_state(&state)?;
                    physical_scope.check_source(&checked_source)?;
                    if let SourceGrant::Connected(grant) = &checked_source.grant {
                        physical_scope.check_source_state(&state, &checked_source)?;
                        crate::projects::connected::check(&state, grant)?;
                    }
                    if let SourceGrant::Folder(grant) = &checked_source.grant {
                        let verified =
                            folder::scan_folder_with_budget(grant, &signal, read_bytes, || {
                                physical_scope
                                    .check_physical()
                                    .map_err(|error| error.to_string())?;
                                check(&signal, deadline).map_err(|error| error.to_string())
                            })
                            .map_err(|_| ProjectError::AccessUnavailable)?;
                        if fingerprint(&verified) != expected_manifest {
                            return Err(ProjectError::ScopeChanged);
                        }
                    }
                    cache
                        .run(|store| {
                            store.replace_source(&physical_scope.project.id, &source_id, &documents)
                        })
                        .map_err(|_| ProjectError::AccessUnavailable)?;
                    physical_scope.check_state(&state)
                },
                cancel,
                deadline,
                self.inner.reads.clone(),
            )
            .await
            .map_err(worker_error)
            .and_then(|result| result);
            scope.check(state).await?;
            check(cancel, deadline)?;
            if let Err(error) = publication {
                match error {
                    ProjectError::ScopeChanged | ProjectError::AccessUnavailable => {
                        self.mark_source_unavailable(scope, source);
                        continue;
                    }
                    error => return Err(error),
                }
            }
            let mut statuses = self
                .inner
                .sources
                .lock()
                .map_err(|_| ProjectError::AccessUnavailable)?;
            let generation = statuses
                .get(&key)
                .map_or(1, |entry| entry.status.generation.saturating_add(1));
            statuses.insert(
                key,
                IndexedSource {
                    scope_revision: scope.project.revision,
                    fingerprint: Some(manifest),
                    status: KnowledgeSourceStatus {
                        resource_id: source.id.clone(),
                        status: if omitted == 0 {
                            KnowledgeSourceState::Ready
                        } else {
                            KnowledgeSourceState::Partial
                        },
                        document_count: count,
                        indexed_at_ms: Some(now_ms()),
                        generation,
                        omitted_count: omitted,
                        error: None,
                    },
                },
            );
            ready_sources.push(source.id.clone());
        }
        Ok(ready_sources)
    }

    fn cache(&self) -> Result<Arc<Cache>, ProjectError> {
        self.inner
            .cache
            .clone()
            .ok_or(ProjectError::AccessUnavailable)
    }

    fn witness(
        &self,
        scope: &Scope,
        hits: &[SearchHit],
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<KnowledgeWitness, ProjectError> {
        let mut seen = BTreeSet::new();
        let mut documents = Vec::new();
        for hit in hits {
            let source = scope
                .sources
                .iter()
                .find(|source| source.id == hit.source_id)
                .ok_or(ProjectError::AccessRevoked)?;
            if anima_document_store_sqlite::source_sha256(hit.text.as_bytes()) != hit.chunk_sha256
                || anima_document_store_sqlite::passage_origin(&hit.locator).is_err()
                || extraction::unsafe_text(&hit.text)
            {
                return Err(ProjectError::AccessUnavailable);
            }
            if seen.insert((source.id.clone(), hit.relative_path.clone())) {
                documents.push(DocumentWitness {
                    source: source.clone(),
                    relative_path: hit.relative_path.clone(),
                    sha256: hit.source_sha256.clone(),
                });
            }
        }
        Ok(KnowledgeWitness {
            scope: scope.clone(),
            documents,
            read_slots: self.inner.reads.clone(),
            cancel: cancel.clone(),
            deadline,
        })
    }

    fn projection(&self, scope: &Scope) -> KnowledgeStatus {
        let sources = self
            .inner
            .sources
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        KnowledgeStatus {
            project_revision: scope.project.revision,
            access_epoch: scope.project.access_epoch,
            member_generation: scope.member_generation,
            sources: scope
                .sources
                .iter()
                .map(|source| {
                    sources
                        .get(&(scope.project.id.clone(), source.id.clone()))
                        .map(|entry| entry.status.clone())
                        .unwrap_or_else(|| KnowledgeSourceStatus {
                            resource_id: source.id.clone(),
                            status: KnowledgeSourceState::Unavailable,
                            document_count: 0,
                            indexed_at_ms: None,
                            generation: 0,
                            omitted_count: 0,
                            error: Some(
                                if self.inner.cache.is_some() {
                                    "This source has not been indexed"
                                } else {
                                    "Project knowledge cache is unavailable"
                                }
                                .into(),
                            ),
                        })
                })
                .collect(),
        }
    }

    fn mark_indexing(&self, scope: &Scope) {
        let mut sources = self
            .inner
            .sources
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for source in &scope.sources {
            let entry = sources
                .entry((scope.project.id.clone(), source.id.clone()))
                .or_insert_with(|| IndexedSource {
                    scope_revision: scope.project.revision,
                    fingerprint: None,
                    status: KnowledgeSourceStatus {
                        resource_id: source.id.clone(),
                        status: KnowledgeSourceState::Indexing,
                        document_count: 0,
                        indexed_at_ms: None,
                        generation: 0,
                        omitted_count: 0,
                        error: None,
                    },
                });
            entry.status.status = KnowledgeSourceState::Indexing;
            entry.scope_revision = scope.project.revision;
            entry.status.error = None;
        }
    }

    fn mark_unavailable(&self, scope: &Scope) {
        for source in &scope.sources {
            self.mark_source_unavailable(scope, source);
        }
    }

    fn mark_source_unavailable(&self, scope: &Scope, source: &Source) {
        let mut sources = self
            .inner
            .sources
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let entry = sources
            .entry((scope.project.id.clone(), source.id.clone()))
            .or_insert_with(|| IndexedSource {
                scope_revision: scope.project.revision,
                fingerprint: None,
                status: KnowledgeSourceStatus {
                    resource_id: source.id.clone(),
                    status: KnowledgeSourceState::Unavailable,
                    document_count: 0,
                    indexed_at_ms: None,
                    generation: 0,
                    omitted_count: 0,
                    error: None,
                },
            });
        if entry.scope_revision > scope.project.revision {
            return;
        }
        entry.scope_revision = scope.project.revision;
        entry.fingerprint = None;
        entry.status.status = KnowledgeSourceState::Unavailable;
        entry.status.document_count = 0;
        entry.status.error = Some("Source access changed or indexing could not finish".into());
    }
}

fn public_hit(hit: SearchHit, scope: &Scope) -> KnowledgeHit {
    let origin = scope
        .sources
        .iter()
        .find(|source| source.id == hit.source_id)
        .and_then(|source| {
            let SourceGrant::Connected(grant) = &source.grant else {
                return None;
            };
            Some(super::types::ConnectedCitationOrigin {
                owner_agent_id: grant.owner_agent_id.clone(),
                connector_id: grant.connector_id.clone(),
                provider: grant.provider.clone(),
                owner_name: source.owner_name.clone(),
                account_label: source.account_label.clone(),
                remote_id: grant.remote_id.clone(),
                revision_sha256: hit.source_sha256.clone(),
                verified_at_ms: now_ms(),
                coverage: if grant.provider == "email" {
                    "Selected retained IMAP message only; full thread unavailable"
                } else if grant.source_kind
                    == anima_document_store_sqlite::connected::ConnectedSourceKind::EmailThread
                {
                    "Up to 25 messages and 1 MiB text; attachments omitted"
                } else {
                    "One selected file; supported bounded text"
                },
            })
        });
    let recognition = match anima_document_store_sqlite::passage_origin(&hit.locator).ok() {
        Some(anima_document_store_sqlite::PassageOrigin::Literal) | None => None,
        other => other,
    };
    KnowledgeHit {
        recognition,
        citation_id: hit.citation_id,
        resource_id: hit.source_id,
        document_id: hit.document_id,
        title: hit.title,
        locator: hit.locator,
        excerpt: hit.excerpt,
        origin,
        source_sha256: hit.source_sha256,
        chunk_sha256: hit.chunk_sha256,
    }
}

fn fingerprint(snapshot: &FolderSnapshot) -> String {
    crate::projects::records::text_digest(
        &snapshot
            .documents
            .iter()
            .map(|document| {
                format!(
                    "{}:{}:{}",
                    document.relative_path.len(),
                    document.relative_path,
                    document.sha256
                )
            })
            .chain(std::iter::once(format!(
                "pipeline:{}",
                extraction::ocr_pipeline()
            )))
            .chain(std::iter::once(format!(
                "omitted:{}",
                snapshot.omitted_count
            )))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn validate_query(query: &str, limit: usize) -> Result<(), ProjectError> {
    if query.trim().is_empty()
        || query.chars().count() > 512
        || query.chars().any(char::is_control)
        || !(1..=20).contains(&limit)
    {
        return Err(ProjectError::InvalidRecord(
            "Use a search query of 1 to 512 characters and a result limit of 1 to 20",
        ));
    }
    Ok(())
}

fn check(cancel: &CancelSignal, deadline: Instant) -> Result<(), ProjectError> {
    if cancel.is_cancelled() {
        return Err(ProjectError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ProjectError::Deadline);
    }
    Ok(())
}

async fn pause(
    scope: &Scope,
    cancel: &CancelSignal,
    deadline: Instant,
) -> Result<(), ProjectError> {
    scope.check_physical()?;
    check(cancel, deadline)?;
    tokio::select! {biased;
        ()=scope.lease.cancelled()=>Err(ProjectError::AccessRevoked),
        ()=cancel.cancelled()=>Err(ProjectError::Cancelled),
        ()=tokio::time::sleep_until((Instant::now()+BUSY_BACKOFF).min(deadline))=>{scope.check_physical()?;check(cancel,deadline)},
    }
}

pub(super) fn worker_error(error: WorkerFailure) -> ProjectError {
    match error {
        WorkerFailure::Busy => ProjectError::Busy,
        WorkerFailure::Cancelled => ProjectError::Cancelled,
        WorkerFailure::TimedOut => ProjectError::Deadline,
        _ => ProjectError::AccessUnavailable,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

fn pdf_page_locator(locator: &str) -> Option<u32> {
    let rest = locator.strip_prefix("pdf#page=")?;
    let number = rest.split(';').next()?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let page = number.parse::<u32>().ok()?;
    (1..=100).contains(&page).then_some(page)
}
