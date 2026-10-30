//! Bounded learned-local retrieval with authority checks around native inference.

use anima_document_store_sqlite::{HybridSearch, PassageEmbedding, SearchHit};

use super::{check, read_retained_with_slots, KnowledgeService, Scope};
use crate::app::SharedDaemonState;
use crate::attachments::documents::worker::WorkerFailure;
use crate::memory_embeddings::{LocalProjectEmbedder, SharedMemoryEmbeddings};
use crate::projects::knowledge::types::{
    KnowledgeSearchMode, KnowledgeSearchRetrieval, SemanticSearchState,
};
use crate::projects::ProjectError;
use anima_core::CancelSignal;
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::time::Instant;

/// One search advances at most this many short semantic windows.
const EMBEDDING_BATCH: usize = 32;
const SEMANTIC_BUDGET: Duration = Duration::from_secs(4);
const KEYWORD_RESERVE: Duration = Duration::from_secs(2);

pub(super) struct SemanticAttempt {
    pub(super) outcome: Option<HybridSearch>,
    pub(super) retrieval: KnowledgeSearchRetrieval,
}

impl SemanticAttempt {
    fn keyword(reason: &'static str) -> Self {
        Self {
            outcome: None,
            retrieval: KnowledgeSearchRetrieval::keyword(reason),
        }
    }
}

struct Prepared {
    query: Vec<f32>,
    vectors: Vec<PassageEmbedding>,
    documents: Vec<SearchHit>,
    incomplete: bool,
}

impl KnowledgeService {
    pub(super) async fn semantic_search(
        &self,
        state: &SharedDaemonState,
        scope: &Scope,
        ready_sources: &[String],
        query: &str,
        limit: usize,
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<SemanticAttempt, ProjectError> {
        scope.check(state).await?;
        check(cancel, deadline)?;
        if ready_sources.is_empty() {
            return Ok(SemanticAttempt::keyword(
                "No current source text is available",
            ));
        }
        let memory = { state.read().await.memory_embeddings_handle() };
        let embedder = memory
            .try_read()
            .ok()
            .and_then(|runtime| runtime.local_project_embedder());
        let Some(embedder) = embedder else {
            return Ok(SemanticAttempt::keyword(
                "The local meaning model is not ready. Keyword search remains available",
            ));
        };
        if deadline.saturating_duration_since(Instant::now()) <= KEYWORD_RESERVE {
            return Ok(SemanticAttempt::keyword(
                "Meaning search reached its time budget",
            ));
        }
        let semantic_deadline = (deadline - KEYWORD_RESERVE).min(Instant::now() + SEMANTIC_BUDGET);
        let Ok(profile) = embedder.profile() else {
            return Ok(SemanticAttempt::keyword(
                "The local meaning model is unsupported",
            ));
        };
        let cache = self.cache()?;
        let shared = state.clone();
        let captured = scope.clone();
        let allowed = ready_sources.to_vec();
        let text = query.to_owned();
        let local = embedder.clone();
        let signal = cancel.clone();
        let prepared = read_retained_with_slots(
            move || {
                check(&signal, semantic_deadline)?;
                {
                    let state = shared.blocking_read();
                    captured.check_state(&state)?;
                    ensure_model(&state.memory_embeddings_handle(), &local)?;
                }
                let pending = cache
                    .run(|store| {
                        store.pending_embeddings(
                            &captured.project.id,
                            &allowed,
                            &profile,
                            EMBEDDING_BATCH,
                        )
                    })
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                let mut seen = BTreeSet::new();
                let mut documents = Vec::new();
                for window in &pending {
                    if seen.insert(window.citation_id.clone()) {
                        let hit = cache
                            .run(|store| store.citation(&captured.project.id, &window.citation_id))
                            .map_err(|_| ProjectError::AccessUnavailable)?
                            .ok_or(ProjectError::ScopeChanged)?;
                        documents.push(hit);
                    }
                }
                check(&signal, semantic_deadline)?;
                let query = local
                    .embed_query(&text)
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                check(&signal, semantic_deadline)?;
                let texts: Vec<&str> = pending.iter().map(|window| window.text.as_str()).collect();
                let (vectors, incomplete) = match local.embed_passages(&texts) {
                    Ok(vectors) if vectors.len() == pending.len() => (
                        pending
                            .into_iter()
                            .zip(vectors)
                            .map(|(window, vector)| PassageEmbedding {
                                citation_id: window.citation_id,
                                window: window.window,
                                vector,
                            })
                            .collect(),
                        false,
                    ),
                    _ => (Vec::new(), true),
                };
                check(&signal, semantic_deadline)?;
                {
                    let state = shared.blocking_read();
                    captured.check_state(&state)?;
                    ensure_model(&state.memory_embeddings_handle(), &local)?;
                }
                Ok::<_, ProjectError>(Prepared {
                    query,
                    vectors,
                    documents,
                    incomplete,
                })
            },
            cancel,
            semantic_deadline,
            self.inner.inference.clone(),
        )
        .await;
        scope.check(state).await?;
        check(cancel, deadline)?;
        let prepared =
            match prepared {
                Ok(Ok(prepared)) => prepared,
                Ok(Err(
                    error @ (ProjectError::ScopeChanged
                    | ProjectError::AccessRevoked
                    | ProjectError::Cancelled),
                )) => return Err(error),
                Err(WorkerFailure::Cancelled) => return Err(ProjectError::Cancelled),
                _ => return Ok(SemanticAttempt::keyword(
                    "Local meaning search is busy or unavailable. Keyword search remains available",
                )),
            };
        // Every input document is rechecked after inference, including connected-source bytes.
        let witness = self.witness(scope, &prepared.documents, cancel, deadline)?;
        witness.check(state).await?;
        scope.check(state).await?;
        let current_memory = { state.read().await.memory_embeddings_handle() };
        let current = current_memory
            .try_read()
            .ok()
            .and_then(|runtime| runtime.local_project_embedder());
        if !current
            .as_ref()
            .is_some_and(|current| current.same_runtime(&embedder))
        {
            return Ok(SemanticAttempt::keyword(
                "The local meaning model changed or is busy",
            ));
        }
        let Ok(profile) = embedder.profile() else {
            return Ok(SemanticAttempt::keyword(
                "The local meaning model is unsupported",
            ));
        };
        let cache = self.cache()?;
        let shared = state.clone();
        let captured = scope.clone();
        let allowed = ready_sources.to_vec();
        let text = query.to_owned();
        let local = embedder.clone();
        let signal = cancel.clone();
        let incomplete = prepared.incomplete;
        let outcome = read_retained_with_slots(
            move || {
                check(&signal, deadline)?;
                let state = shared.blocking_read();
                captured.check_state(&state)?;
                ensure_model(&state.memory_embeddings_handle(), &local)?;
                for source in captured
                    .sources
                    .iter()
                    .filter(|source| allowed.contains(&source.id))
                {
                    captured.check_source_state(&state, source)?;
                    captured.check_source(source)?;
                    if let super::SourceGrant::Connected(grant) = &source.grant {
                        crate::projects::connected::check(&state, grant)?;
                    }
                }
                // Vector cache failure cannot discard successfully indexed keyword evidence.
                let _ = cache.run(|store| {
                    store.publish_embeddings(&captured.project.id, &profile, &prepared.vectors)
                });
                let result = cache
                    .run(|store| {
                        store.hybrid_search(
                            &captured.project.id,
                            &allowed,
                            &text,
                            limit,
                            &profile,
                            &prepared.query,
                        )
                    })
                    .map_err(|_| ProjectError::AccessUnavailable)?;
                check(&signal, deadline)?;
                ensure_model(&state.memory_embeddings_handle(), &local)?;
                captured.check_state(&state)?;
                Ok::<_, ProjectError>(result)
            },
            cancel,
            deadline,
            self.inner.reads.clone(),
        )
        .await;
        scope.check(state).await?;
        check(cancel, deadline)?;
        let outcome = match outcome {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(
                error @ (ProjectError::ScopeChanged
                | ProjectError::AccessRevoked
                | ProjectError::Cancelled),
            )) => return Err(error),
            Err(WorkerFailure::Cancelled) => return Err(ProjectError::Cancelled),
            _ => {
                return Ok(SemanticAttempt::keyword(
                    "The local meaning index is unavailable. Keyword search remains available",
                ))
            }
        };
        let current_memory = { state.read().await.memory_embeddings_handle() };
        let current = current_memory
            .try_read()
            .ok()
            .and_then(|runtime| runtime.local_project_embedder());
        if !current
            .as_ref()
            .is_some_and(|current| current.same_runtime(&embedder))
        {
            return Ok(SemanticAttempt::keyword(
                "The local meaning model changed or is busy",
            ));
        }
        let coverage = &outcome.coverage;
        let has_vectors = coverage.indexed_windows > 0;
        let partial = incomplete
            || coverage.indexed_windows < coverage.total_windows
            || coverage.omitted_windows > 0;
        let retrieval = KnowledgeSearchRetrieval {
            mode: if has_vectors { KnowledgeSearchMode::Hybrid } else { KnowledgeSearchMode::Keyword },
            semantic_state: if partial { SemanticSearchState::Partial } else { SemanticSearchState::Ready },
            provider: Some("fastembed"),
            model: Some(embedder.model().to_owned()),
            indexed_windows: coverage.indexed_windows,
            eligible_windows: coverage.total_windows,
            omitted_windows: coverage.omitted_windows,
            reason: partial.then_some("Meaning search covers part of the current index. Keyword search covers all indexed text"),
        };
        Ok(SemanticAttempt {
            outcome: Some(outcome),
            retrieval,
        })
    }
}

fn ensure_model(
    memory: &SharedMemoryEmbeddings,
    captured: &LocalProjectEmbedder,
) -> Result<(), ProjectError> {
    let current = memory
        .try_read()
        .ok()
        .and_then(|runtime| runtime.local_project_embedder());
    if current
        .as_ref()
        .is_some_and(|current| current.same_runtime(captured))
    {
        Ok(())
    } else {
        Err(ProjectError::AccessUnavailable)
    }
}
