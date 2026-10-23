use std::collections::{HashMap, HashSet};
use std::io;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anima_memory::{
    InMemoryVectorIndex, Memory, MemoryManager, MemoryTextEmbedder, MemoryVectorError,
    MemoryVectorIndex, RecentMemoryOptions, VectorMemoryHit,
};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use reqwest::blocking::Client as BlockingHttpClient;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock as AsyncRwLock;
use tracing::{info, warn};

pub(crate) type SharedMemoryEmbeddings = Arc<AsyncRwLock<MemoryEmbeddingRuntime>>;

/// Memories re-embedded per step of a model rebuild. The memory lock is never
/// held while a batch is embedded.
const REBUILD_BATCH_SIZE: usize = 32;
/// Longest fallback reason the readiness route reports.
const FALLBACK_REASON_CHARS: usize = 160;

const DEFAULT_EMBEDDING_DIMENSION: usize = 96;
const MIN_EMBEDDING_DIMENSION: usize = 24;
const DEFAULT_OPENAI_EMBEDDING_DIMENSION: usize = 1536;
const DEFAULT_OLLAMA_EMBEDDING_DIMENSION: usize = 768;
const DEFAULT_EMBEDDING_TIMEOUT_MS: u64 = 15_000;
const LOCAL_PROVIDER: &str = "local";
const LOCAL_MODEL: &str = "local-semantic-v1";
const FASTEMBED_PROVIDER: &str = "fastembed";
const DEFAULT_FASTEMBED_MODEL: &str = "intfloat/multilingual-e5-small";
const OPENAI_PROVIDER: &str = "openai";
const OPENAI_COMPATIBLE_PROVIDER: &str = "openai-compatible";
const OLLAMA_PROVIDER: &str = "ollama";
const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const DEFAULT_OLLAMA_EMBEDDING_MODEL: &str = "nomic-embed-text";
const DEFAULT_OPENAI_EMBEDDING_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OLLAMA_EMBEDDING_BASE_URL: &str = "http://127.0.0.1:11434/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemoryEmbeddingStatus {
    pub(crate) enabled: bool,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) dimension: usize,
    pub(crate) vector_count: usize,
    pub(crate) persisted: bool,
    pub(crate) storage_file: Option<PathBuf>,
    pub(crate) load: MemoryEmbeddingLoad,
    /// Memories with a vector from the active embedder, out of `total`.
    pub(crate) done: usize,
    pub(crate) total: usize,
}

/// Where the configured embedder stands (spec "Embeddings"). Until it is
/// `Ready`, vector search only sees vectors the active embedder made, and BM25
/// covers the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemoryEmbeddingLoad {
    /// The model is downloading or loading; recall runs on hashed vectors.
    Pending,
    /// The model is active and memories are being re-embedded with it.
    Rebuilding {
        done: usize,
        total: usize,
    },
    Ready,
    /// The model did not load; hashed vectors stay until restart.
    Fallback(String),
    /// Vectors are turned off.
    Off,
}

impl MemoryEmbeddingLoad {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Pending => "downloading",
            Self::Rebuilding { .. } => "rebuilding",
            Self::Ready => "ready",
            Self::Fallback(_) => "fallback",
            Self::Off => "off",
        }
    }
}

/// An embedder that finished loading, ready to replace the interim one.
pub(crate) struct LoadedMemoryEmbedder(MemoryEmbeddingProvider);

/// Loads the configured model off the async runtime. Production opens or
/// downloads fastembed's files; tests pass fakes, so they never download.
pub(crate) type MemoryModelLoader =
    Box<dyn FnOnce() -> Result<LoadedMemoryEmbedder, String> + Send + Sync>;

/// What a model swap left to embed, under the generation it started.
struct RebuildPlan {
    generation: u64,
    embedder: MemoryEmbeddingProvider,
    pending: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoryEmbeddingRebuildReport {
    pub(crate) loaded_vectors: usize,
    pub(crate) rebuilt_vectors: usize,
    pub(crate) removed_stale_vectors: usize,
}

pub(crate) struct MemoryEmbeddingRuntime {
    enabled: bool,
    embedder: MemoryEmbeddingProvider,
    index: InMemoryVectorIndex<MemoryEmbeddingProvider>,
    indexed_content: HashMap<String, [u8; 32]>,
    store: Option<SqliteMemoryEmbeddingStore>,
    owner: Option<crate::profile_owner::ProfileOwner>,
    load: MemoryEmbeddingLoad,
    /// Bumped on every embedder swap. Query vectors carry it, so one prepared
    /// before a swap is never scored against the new index.
    generation: u64,
    /// False while the hashed embedder stands in for a model that is loading
    /// or failed: the store keeps only the configured model's vectors.
    persist_vectors: bool,
    /// Provider and model of the embedder still loading, if any.
    awaiting: Option<(String, String)>,
    model_loader: Option<MemoryModelLoader>,
}

/// Cloneable query-only view. Model inference can run without holding the live
/// memory/index locks; search later revalidates its model identity against the
/// current index before using the prepared vector.
#[derive(Clone)]
pub(crate) struct MemoryQueryEmbedder {
    embedder: MemoryEmbeddingProvider,
    model_key: String,
}

impl MemoryQueryEmbedder {
    pub(crate) fn model_key(&self) -> &str {
        &self.model_key
    }

    pub(crate) fn embed_query(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        self.embedder.embed_query(text)
    }
}

impl MemoryEmbeddingRuntime {
    pub(crate) fn local_default() -> Self {
        Self::local(DEFAULT_EMBEDDING_DIMENSION)
    }

    pub(crate) fn local(dimension: usize) -> Self {
        Self::from_provider(MemoryEmbeddingProvider::Local(LocalMemoryEmbedder::new(
            dimension.max(MIN_EMBEDDING_DIMENSION),
        )))
    }

    pub(crate) fn disabled() -> Self {
        let mut runtime = Self::from_provider(MemoryEmbeddingProvider::Local(
            LocalMemoryEmbedder::new(DEFAULT_EMBEDDING_DIMENSION),
        ));
        runtime.enabled = false;
        runtime.load = MemoryEmbeddingLoad::Off;
        runtime
    }

    /// Starts on hashed vectors while `loader` brings up the configured model
    /// in the background (spec "Embeddings").
    fn awaiting_model(provider: &str, model: String, loader: MemoryModelLoader) -> Self {
        let mut runtime = Self::local_default();
        runtime.load = MemoryEmbeddingLoad::Pending;
        runtime.persist_vectors = false;
        runtime.awaiting = Some((provider.to_string(), model));
        runtime.model_loader = Some(loader);
        runtime
    }

    /// `ANIMAOS_RS_MEMORY_EMBEDDINGS` picks the mode; unset means `fastembed`,
    /// whose model loads later through [`Self::take_model_loader`] (see
    /// [`embeddings_mode`] for the one exception).
    pub(crate) fn from_env(
        default_sqlite_path: Option<PathBuf>,
        default_model_cache_dir: Option<PathBuf>,
    ) -> io::Result<Self> {
        Self::from_env_owned(default_sqlite_path, default_model_cache_dir, None)
    }

    pub(crate) fn from_env_owned(
        default_sqlite_path: Option<PathBuf>,
        default_model_cache_dir: Option<PathBuf>,
        owner: Option<crate::profile_owner::ProfileOwner>,
    ) -> io::Result<Self> {
        if let Some(owner) = &owner {
            owner.ensure_owned()?;
        }
        let explicit = std::env::var("ANIMAOS_RS_MEMORY_EMBEDDINGS").ok();
        if explicit.as_deref().is_some_and(|mode| {
            matches!(
                mode.to_ascii_lowercase().as_str(),
                "0" | "off" | "false" | "disabled"
            )
        }) {
            let mut runtime = Self::disabled();
            runtime.owner = owner;
            return Ok(runtime);
        }

        let dimension = embedding_dimension_from_env()?;
        let (mode, kept_local) = embeddings_mode(explicit.as_deref(), dimension);
        if kept_local {
            tracing::warn!(
                dimension = dimension.unwrap_or_default(),
                "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS is set without ANIMAOS_RS_MEMORY_EMBEDDINGS: \
                 keeping hashed `local` vectors. To switch to the fastembed model, unset \
                 ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS (or set ANIMAOS_RS_MEMORY_EMBEDDINGS=fastembed \
                 with the model's dimension, 384 for multilingual-e5-small)"
            );
        }
        let mut runtime = if matches!(
            mode.as_str(),
            FASTEMBED_PROVIDER | "local-model" | "local-neural" | "local-multilingual"
        ) {
            // The configuration is checked now; only the download waits.
            let config = FastEmbedEmbeddingConfig::from_env(dimension, default_model_cache_dir)?;
            let model = config.model_name.clone();
            Self::awaiting_model(
                FASTEMBED_PROVIDER,
                model,
                Box::new(move || {
                    FastEmbedMemoryEmbedder::load(config)
                        .map(|embedder| {
                            LoadedMemoryEmbedder(MemoryEmbeddingProvider::FastEmbed(embedder))
                        })
                        .map_err(|error| error.to_string())
                }),
            )
        } else {
            Self::from_provider(Self::provider_from_env(&mode, dimension)?)
        };
        let storage_path =
            env_path("ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE")?.or(default_sqlite_path);
        if let Some(path) = storage_path {
            let path = owner
                .as_ref()
                .map(|owner| owner.owned_sqlite_path(&path))
                .transpose()?
                .unwrap_or(path);
            runtime.store = Some(SqliteMemoryEmbeddingStore::new_with_owner(
                path,
                owner.clone(),
            )?);
        }
        runtime.owner = owner;
        Ok(runtime)
    }

    fn provider_from_env(
        mode: &str,
        dimension: Option<usize>,
    ) -> io::Result<MemoryEmbeddingProvider> {
        Ok(match mode {
            LOCAL_PROVIDER | "local-hash" | "local-semantic" => {
                let dimension = dimension.unwrap_or(DEFAULT_EMBEDDING_DIMENSION);
                if dimension < MIN_EMBEDDING_DIMENSION {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS must be at least {MIN_EMBEDDING_DIMENSION} for local embeddings"
                        ),
                    ));
                }
                MemoryEmbeddingProvider::Local(LocalMemoryEmbedder::new(dimension))
            }
            OLLAMA_PROVIDER => MemoryEmbeddingProvider::OpenAiCompatible(
                OpenAiCompatibleMemoryEmbedder::from_env(OpenAiCompatibleEmbeddingConfig {
                    provider: OLLAMA_PROVIDER,
                    default_base_url: DEFAULT_OLLAMA_EMBEDDING_BASE_URL,
                    default_model: DEFAULT_OLLAMA_EMBEDDING_MODEL,
                    default_dimension: DEFAULT_OLLAMA_EMBEDDING_DIMENSION,
                    api_key_envs: &["ANIMAOS_RS_MEMORY_EMBEDDINGS_API_KEY", "OLLAMA_API_KEY"],
                    base_url_envs: &["ANIMAOS_RS_MEMORY_EMBEDDINGS_BASE_URL", "OLLAMA_BASE_URL"],
                    requires_key: false,
                    dimension,
                })?,
            ),
            OPENAI_PROVIDER => MemoryEmbeddingProvider::OpenAiCompatible(
                OpenAiCompatibleMemoryEmbedder::from_env(OpenAiCompatibleEmbeddingConfig {
                    provider: OPENAI_PROVIDER,
                    default_base_url: DEFAULT_OPENAI_EMBEDDING_BASE_URL,
                    default_model: DEFAULT_OPENAI_EMBEDDING_MODEL,
                    default_dimension: DEFAULT_OPENAI_EMBEDDING_DIMENSION,
                    api_key_envs: &[
                        "ANIMAOS_RS_MEMORY_EMBEDDINGS_API_KEY",
                        "OPENAI_API_KEY",
                        "OPENAI_KEY",
                        "OPENAI_TOKEN",
                    ],
                    base_url_envs: &["ANIMAOS_RS_MEMORY_EMBEDDINGS_BASE_URL", "OPENAI_BASE_URL"],
                    requires_key: true,
                    dimension,
                })?,
            ),
            OPENAI_COMPATIBLE_PROVIDER | "openai_compatible" | "compatible" => {
                MemoryEmbeddingProvider::OpenAiCompatible(
                    OpenAiCompatibleMemoryEmbedder::from_env(OpenAiCompatibleEmbeddingConfig {
                        provider: OPENAI_COMPATIBLE_PROVIDER,
                        default_base_url: DEFAULT_OPENAI_EMBEDDING_BASE_URL,
                        default_model: DEFAULT_OPENAI_EMBEDDING_MODEL,
                        default_dimension: DEFAULT_OPENAI_EMBEDDING_DIMENSION,
                        api_key_envs: &[
                            "ANIMAOS_RS_MEMORY_EMBEDDINGS_API_KEY",
                            "OPENAI_API_KEY",
                            "OPENAI_KEY",
                            "OPENAI_TOKEN",
                        ],
                        base_url_envs: &[
                            "ANIMAOS_RS_MEMORY_EMBEDDINGS_BASE_URL",
                            "OPENAI_BASE_URL",
                        ],
                        requires_key: false,
                        dimension,
                    })?,
                )
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ANIMAOS_RS_MEMORY_EMBEDDINGS must be local, fastembed, ollama, openai, openai-compatible, or disabled",
                ))
            }
        })
    }

    fn from_provider(embedder: MemoryEmbeddingProvider) -> Self {
        Self {
            enabled: true,
            embedder: embedder.clone(),
            index: InMemoryVectorIndex::new(embedder),
            indexed_content: HashMap::new(),
            store: None,
            owner: None,
            load: MemoryEmbeddingLoad::Ready,
            generation: 0,
            persist_vectors: true,
            awaiting: None,
            model_loader: None,
        }
    }

    pub(crate) fn status(&self) -> MemoryEmbeddingStatus {
        let (provider, model) = match (&self.awaiting, self.enabled) {
            (_, false) => ("disabled".to_string(), "none".to_string()),
            (Some((provider, model)), true) => (provider.clone(), model.clone()),
            (None, true) => (
                self.embedder.provider().to_string(),
                self.embedder.model().to_string(),
            ),
        };
        let (done, total) = match &self.load {
            MemoryEmbeddingLoad::Rebuilding { done, total } => (*done, *total),
            MemoryEmbeddingLoad::Ready => (self.index.len(), self.index.len()),
            _ => (0, 0),
        };
        MemoryEmbeddingStatus {
            enabled: self.enabled,
            provider,
            model,
            dimension: self
                .index
                .dimension()
                .or_else(|| self.embedder.dimension_hint())
                .unwrap_or_default(),
            vector_count: self.index.len(),
            persisted: self.store.is_some(),
            storage_file: self.store.as_ref().map(|store| store.path.clone()),
            load: self.load.clone(),
            done,
            total,
        }
    }

    /// Identifies the vectors in the index: the embedder's model and the swap
    /// that installed it.
    fn vector_key(&self) -> String {
        format!("{}#{}", self.embedder.storage_model(), self.generation)
    }

    pub(crate) fn query_embedder(&self) -> Option<MemoryQueryEmbedder> {
        self.enabled.then(|| MemoryQueryEmbedder {
            embedder: self.embedder.clone(),
            model_key: self.vector_key(),
        })
    }

    pub(crate) fn matches_current(&self, memory: &Memory) -> bool {
        self.enabled
            && self.indexed_content.get(&memory.id) == Some(&content_digest(&memory.content))
    }

    pub(crate) fn for_memories<'a>(
        &'a self,
        memories: &'a MemoryManager,
    ) -> CurrentMemoryVectorIndex<'a> {
        CurrentMemoryVectorIndex {
            runtime: self,
            memories,
        }
    }

    pub(crate) fn search_precomputed_query(
        &self,
        model_key: &str,
        query: &[f32],
        limit: usize,
        eligible: &dyn Fn(&str) -> bool,
    ) -> Vec<VectorMemoryHit> {
        if !self.enabled || self.vector_key() != model_key {
            return Vec::new();
        }
        self.index
            .search_embedding_filtered(query, limit, eligible)
            .unwrap_or_default()
    }

    pub(crate) fn rebuild_from_memories(
        &mut self,
        memories: &[Memory],
    ) -> Result<MemoryEmbeddingRebuildReport, String> {
        let mut report = MemoryEmbeddingRebuildReport::default();
        self.index.clear();
        self.indexed_content.clear();
        if !self.enabled {
            return Ok(report);
        }

        let valid_ids: HashSet<_> = memories.iter().map(|memory| memory.id.clone()).collect();
        let mut stored_by_id = HashMap::new();
        let storage_model = self.embedder.storage_model();
        if let Some(store) = &self.store {
            if self.persist_vectors {
                for stored in store.load_all().map_err(|error| error.to_string())? {
                    if valid_ids.contains(&stored.memory_id)
                        && stored.model == storage_model
                        && self.embedder.accepts_dimension(stored.vector.len())
                    {
                        stored_by_id.insert(stored.memory_id, (stored.vector, stored.content_hash));
                    }
                }
            }
            report.removed_stale_vectors = store
                .delete_stale(&valid_ids)
                .map_err(|error| error.to_string())?;
        }

        for memory in memories {
            let digest = content_digest(&memory.content);
            if let Some((vector, Some(stored_digest))) = stored_by_id.remove(&memory.id) {
                if stored_digest == digest
                    && self
                        .index
                        .upsert_embedding(memory.id.clone(), vector)
                        .is_ok()
                {
                    self.indexed_content.insert(memory.id.clone(), digest);
                    report.loaded_vectors += 1;
                    continue;
                }
            }

            let vector = self
                .embedder
                .embed(&memory.content)
                .map_err(|error| error.message().to_string())?;
            self.index
                .upsert_embedding(memory.id.clone(), vector.clone())
                .map_err(|error| error.message().to_string())?;
            self.indexed_content.insert(memory.id.clone(), digest);
            if let Some(store) = self.store.as_ref().filter(|_| self.persist_vectors) {
                store
                    .upsert(&memory.id, &storage_model, &vector, &digest)
                    .map_err(|error| error.to_string())?;
            }
            report.rebuilt_vectors += 1;
        }

        Ok(report)
    }

    /// Rebuild a small local RAM index without reading, deleting or rewriting the on-disk cache.
    pub(crate) fn rebuild_bounded_local_runtime_index(
        &mut self,
        memories: &[Memory],
    ) -> Result<(), String> {
        if !self.enabled
            || self.embedder.provider() != LOCAL_PROVIDER
            || self.load != MemoryEmbeddingLoad::Ready
            || memories.len() > 256
            || self.index.len() > 512
            || memories.iter().map(|m| m.content.len()).sum::<usize>() > 1024 * 1024
        {
            return Err("Local runtime index repair is unavailable or exceeds its budget".into());
        }
        let mut index = InMemoryVectorIndex::new(self.embedder.clone());
        let mut content = HashMap::new();
        for memory in memories {
            let vector = self
                .embedder
                .embed(&memory.content)
                .map_err(|e| e.message().to_string())?;
            index
                .upsert_embedding(memory.id.clone(), vector)
                .map_err(|e| e.message().to_string())?;
            content.insert(memory.id.clone(), content_digest(&memory.content));
        }
        self.index = index;
        self.indexed_content = content;
        Ok(())
    }

    pub(crate) fn upsert_memory(&mut self, memory: &Memory) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let vector = self
            .embedder
            .embed(&memory.content)
            .map_err(|error| error.message().to_string())?;
        self.index
            .upsert_embedding(memory.id.clone(), vector.clone())
            .map_err(|error| error.message().to_string())?;
        let digest = content_digest(&memory.content);
        self.indexed_content.insert(memory.id.clone(), digest);
        if let Some(store) = self.store.as_ref().filter(|_| self.persist_vectors) {
            store
                .upsert(&memory.id, &self.embedder.storage_model(), &vector, &digest)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(crate) fn remove_memories(&mut self, memory_ids: &[String]) -> Result<(), String> {
        for memory_id in memory_ids {
            self.index.remove(memory_id);
            self.indexed_content.remove(memory_id);
        }
        if let Some(store) = &self.store {
            store
                .delete_many(memory_ids)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Hands out the background model loader once; `None` when nothing loads.
    pub(crate) fn take_model_loader(&mut self) -> Option<MemoryModelLoader> {
        let loader = self.model_loader.take()?;
        let Some(owner) = self.owner.clone() else {
            return Some(loader);
        };
        Some(Box::new(move || {
            owner.ensure_owned().map_err(|error| error.to_string())?;
            let loaded = loader()?;
            owner.ensure_owned().map_err(|error| error.to_string())?;
            Ok(loaded)
        }))
    }

    /// The model did not load: keep the hashed vectors until restart.
    pub(crate) fn fall_back(&mut self, error: &str) {
        self.model_loader = None;
        self.load = MemoryEmbeddingLoad::Fallback(fallback_reason(error));
    }

    /// Installs the loaded model. Every interim vector is dropped at once, so
    /// a search never mixes embedders: until a memory is re-embedded it is
    /// reachable only through BM25. Stored vectors this model made for the
    /// current text are reused; the rest are returned to embed.
    fn begin_rebuild(
        &mut self,
        loaded: LoadedMemoryEmbedder,
        memories: &[Memory],
        stored: Vec<StoredEmbedding>,
    ) -> RebuildPlan {
        let embedder = loaded.0;
        self.generation += 1;
        self.embedder = embedder.clone();
        self.index = InMemoryVectorIndex::new(embedder.clone());
        self.indexed_content.clear();
        self.awaiting = None;
        self.model_loader = None;
        self.persist_vectors = true;

        let storage_model = embedder.storage_model();
        let mut stored_by_id: HashMap<_, _> = stored
            .into_iter()
            .filter(|stored| {
                stored.model == storage_model && embedder.accepts_dimension(stored.vector.len())
            })
            .filter_map(|stored| {
                let digest = stored.content_hash?;
                Some((stored.memory_id, (stored.vector, digest)))
            })
            .collect();
        let mut pending = Vec::new();
        for memory in memories {
            let digest = content_digest(&memory.content);
            if let Some((vector, stored_digest)) = stored_by_id.remove(&memory.id) {
                if stored_digest == digest
                    && self
                        .index
                        .upsert_embedding(memory.id.clone(), vector)
                        .is_ok()
                {
                    self.indexed_content.insert(memory.id.clone(), digest);
                    continue;
                }
            }
            pending.push((memory.id.clone(), memory.content.clone()));
        }
        self.load = MemoryEmbeddingLoad::Rebuilding {
            done: memories.len() - pending.len(),
            total: memories.len(),
        };
        RebuildPlan {
            generation: self.generation,
            embedder,
            pending,
        }
    }

    /// Indexes one re-embedded batch. A memory changed, deleted or already
    /// indexed by a write since the snapshot is skipped. Returns `false` when
    /// another swap replaced this rebuild.
    fn apply_rebuild_batch(
        &mut self,
        generation: u64,
        memories: &MemoryManager,
        batch: &[(String, String)],
        vectors: Vec<Option<Vec<f32>>>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        let mut rows = Vec::new();
        for ((memory_id, content), vector) in batch.iter().zip(vectors) {
            let Some(vector) = vector else {
                continue;
            };
            let unchanged = memories
                .get(memory_id)
                .is_some_and(|memory| memory.content == *content);
            if !unchanged || self.indexed_content.contains_key(memory_id) {
                continue;
            }
            if self
                .index
                .upsert_embedding(memory_id.clone(), vector.clone())
                .is_err()
            {
                continue;
            }
            let digest = content_digest(content);
            self.indexed_content.insert(memory_id.clone(), digest);
            rows.push((memory_id.as_str(), vector, digest));
        }
        if let (Some(store), false) = (&self.store, rows.is_empty()) {
            if let Err(error) = store.upsert_many(&self.embedder.storage_model(), &rows) {
                warn!(%error, "could not save rebuilt memory embeddings; they stay in process");
            }
        }
        if let MemoryEmbeddingLoad::Rebuilding { done, total } = &mut self.load {
            *done = (*done + batch.len()).min(*total);
        }
        true
    }

    fn finish_rebuild(&mut self, generation: u64) {
        if self.generation == generation
            && matches!(self.load, MemoryEmbeddingLoad::Rebuilding { .. })
        {
            self.load = MemoryEmbeddingLoad::Ready;
        }
    }
}

/// Loads the configured model and rebuilds every vector with it, off the
/// async runtime: boot never waits for the download (spec "Embeddings").
/// A detached thread rather than `spawn_blocking`, because dropping the tokio
/// runtime waits for blocking tasks and shutdown must not wait for a download.
pub(crate) fn spawn_model_load(
    memory: Arc<AsyncRwLock<MemoryManager>>,
    embeddings: SharedMemoryEmbeddings,
    loader: MemoryModelLoader,
) -> io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("memory-model-load".into())
        .spawn(move || run_model_load(&memory, &embeddings, loader))
}

fn run_model_load(
    memory: &AsyncRwLock<MemoryManager>,
    embeddings: &AsyncRwLock<MemoryEmbeddingRuntime>,
    loader: MemoryModelLoader,
) {
    let loaded = match std::panic::catch_unwind(AssertUnwindSafe(loader)) {
        Ok(Ok(loaded)) => loaded,
        Ok(Err(error)) => {
            warn!(%error, "the memory search model did not load; using basic search until restart");
            embeddings.blocking_write().fall_back(&error);
            return;
        }
        Err(_) => {
            warn!("the memory search model loader panicked; using basic search until restart");
            embeddings
                .blocking_write()
                .fall_back("the search model loader crashed");
            return;
        }
    };
    let store = embeddings.blocking_read().store.clone();
    let stored = match store.as_ref().map(SqliteMemoryEmbeddingStore::load_all) {
        Some(Ok(stored)) => stored,
        Some(Err(error)) => {
            warn!(%error, "could not read saved memory embeddings; re-embedding every memory");
            Vec::new()
        }
        None => Vec::new(),
    };
    let plan = {
        // Snapshot and swap under one memory read, so no write slips between them.
        let memory = memory.blocking_read();
        let memories = memory.get_recent(RecentMemoryOptions {
            limit: Some(usize::MAX),
            ..RecentMemoryOptions::default()
        });
        embeddings
            .blocking_write()
            .begin_rebuild(loaded, &memories, stored)
    };
    for batch in plan.pending.chunks(REBUILD_BATCH_SIZE) {
        let texts: Vec<&str> = batch.iter().map(|(_, content)| content.as_str()).collect();
        let vectors = std::panic::catch_unwind(AssertUnwindSafe(|| {
            plan.embedder.embed_memory_batch(&texts)
        }))
        .unwrap_or_else(|_| vec![None; texts.len()]);
        let memory = memory.blocking_read();
        if !embeddings.blocking_write().apply_rebuild_batch(
            plan.generation,
            &memory,
            batch,
            vectors,
        ) {
            return;
        }
    }
    let mut runtime = embeddings.blocking_write();
    runtime.finish_rebuild(plan.generation);
    let status = runtime.status();
    info!(
        provider = %status.provider,
        model = %status.model,
        vector_count = status.vector_count,
        "memory search model ready"
    );
}

/// The owner-facing reason: the first line, without paths or URLs and cut
/// short. The log keeps the full error.
fn fallback_reason(error: &str) -> String {
    let line = error
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let reason = line
        .split_whitespace()
        .map(|word| {
            if word.contains('/') || word.contains('\\') {
                "…"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|character| !character.is_control())
        .take(FALLBACK_REASON_CHARS)
        .collect::<String>();
    if reason.trim().is_empty() {
        "the search model did not load".to_string()
    } else {
        reason
    }
}

pub(crate) struct CurrentMemoryVectorIndex<'a> {
    runtime: &'a MemoryEmbeddingRuntime,
    memories: &'a MemoryManager,
}

impl MemoryVectorIndex for CurrentMemoryVectorIndex<'_> {
    fn search(&self, query: &str, limit: usize) -> Vec<VectorMemoryHit> {
        self.search_filtered(query, limit, &|_| true)
    }

    fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        eligible: &dyn Fn(&str) -> bool,
    ) -> Vec<VectorMemoryHit> {
        self.runtime.search_filtered(query, limit, &|id| {
            eligible(id)
                && self
                    .memories
                    .get(id)
                    .is_some_and(|memory| self.runtime.matches_current(&memory))
        })
    }
}

fn content_digest(content: &str) -> [u8; 32] {
    Sha256::digest(content.as_bytes()).into()
}

fn encode_digest(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(digest)
}

impl MemoryVectorIndex for MemoryEmbeddingRuntime {
    fn search(&self, query: &str, limit: usize) -> Vec<VectorMemoryHit> {
        self.search_filtered(query, limit, &|_| true)
    }

    fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        eligible: &dyn Fn(&str) -> bool,
    ) -> Vec<VectorMemoryHit> {
        if limit == 0 || !self.enabled {
            return Vec::new();
        }
        let Ok(embedding) = self.embedder.embed_query(query) else {
            return Vec::new();
        };
        self.index
            .search_embedding_filtered(&embedding, limit, eligible)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
enum MemoryEmbeddingProvider {
    Local(LocalMemoryEmbedder),
    FastEmbed(FastEmbedMemoryEmbedder),
    OpenAiCompatible(OpenAiCompatibleMemoryEmbedder),
    /// Stands in for a downloaded model in tests.
    #[cfg(test)]
    Fake(test_support::FakeModelEmbedder),
}

impl MemoryEmbeddingProvider {
    fn provider(&self) -> &str {
        match self {
            Self::Local(_) => LOCAL_PROVIDER,
            Self::FastEmbed(_) => FASTEMBED_PROVIDER,
            Self::OpenAiCompatible(embedder) => &embedder.provider,
            #[cfg(test)]
            Self::Fake(_) => test_support::FAKE_PROVIDER,
        }
    }

    fn model(&self) -> &str {
        match self {
            Self::Local(_) => LOCAL_MODEL,
            Self::FastEmbed(embedder) => &embedder.model_name,
            Self::OpenAiCompatible(embedder) => &embedder.model,
            #[cfg(test)]
            Self::Fake(_) => test_support::FAKE_MODEL,
        }
    }

    fn storage_model(&self) -> String {
        match self {
            Self::Local(_) => LOCAL_MODEL.to_string(),
            Self::FastEmbed(embedder) => format!("{FASTEMBED_PROVIDER}:{}", embedder.model_name),
            Self::OpenAiCompatible(embedder) => format!("{}:{}", embedder.provider, embedder.model),
            #[cfg(test)]
            Self::Fake(_) => format!(
                "{}:{}",
                test_support::FAKE_PROVIDER,
                test_support::FAKE_MODEL
            ),
        }
    }

    fn dimension_hint(&self) -> Option<usize> {
        match self {
            Self::Local(embedder) => Some(embedder.dimension),
            Self::FastEmbed(embedder) => Some(embedder.dimension),
            Self::OpenAiCompatible(embedder) => Some(embedder.dimension),
            #[cfg(test)]
            Self::Fake(embedder) => Some(embedder.dimension()),
        }
    }

    fn accepts_dimension(&self, dimension: usize) -> bool {
        match self.dimension_hint() {
            Some(expected) => expected == dimension,
            None => true,
        }
    }

    fn embed_memory(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        match self {
            Self::Local(embedder) => embedder.embed(text),
            Self::FastEmbed(embedder) => embedder.embed_memory(text),
            Self::OpenAiCompatible(embedder) => embedder.embed(text),
            #[cfg(test)]
            Self::Fake(embedder) => embedder.embed(text),
        }
    }

    /// Embeds a rebuild batch; a text that cannot embed gets `None`.
    fn embed_memory_batch(&self, texts: &[&str]) -> Vec<Option<Vec<f32>>> {
        if let Self::FastEmbed(embedder) = self {
            if let Some(vectors) = embedder.embed_passages(texts) {
                return vectors;
            }
        }
        texts
            .iter()
            .map(|text| self.embed_memory(text).ok())
            .collect()
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        match self {
            Self::Local(embedder) => embedder.embed(text),
            Self::FastEmbed(embedder) => embedder.embed_query(text),
            Self::OpenAiCompatible(embedder) => embedder.embed(text),
            #[cfg(test)]
            Self::Fake(embedder) => embedder.embed(text),
        }
    }
}

impl MemoryTextEmbedder for MemoryEmbeddingProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        self.embed_memory(text)
    }
}

#[derive(Clone, Debug)]
struct LocalMemoryEmbedder {
    dimension: usize,
}

impl LocalMemoryEmbedder {
    fn new(dimension: usize) -> Self {
        Self { dimension }
    }
}

impl MemoryTextEmbedder for LocalMemoryEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        let tokens = tokenize(text);
        if tokens.is_empty() {
            return Err(MemoryVectorError::EmbeddingUnavailable);
        }

        let mut vector = vec![0.0_f32; self.dimension];
        for token in &tokens {
            for (group_index, group) in SEMANTIC_GROUPS.iter().enumerate() {
                if group.iter().any(|term| *term == token.as_str()) {
                    vector[group_index % self.dimension] += 2.5;
                }
            }

            let hashed_index = SEMANTIC_GROUPS.len()
                + (stable_hash(token) as usize
                    % self.dimension.saturating_sub(SEMANTIC_GROUPS.len()).max(1));
            vector[hashed_index % self.dimension] += 1.0;
        }

        Ok(vector)
    }
}

#[derive(Clone)]
struct FastEmbedMemoryEmbedder {
    model_name: String,
    dimension: usize,
    prompt_style: FastEmbedPromptStyle,
    model: Arc<Mutex<TextEmbedding>>,
}

impl std::fmt::Debug for FastEmbedMemoryEmbedder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FastEmbedMemoryEmbedder")
            .field("model_name", &self.model_name)
            .field("dimension", &self.dimension)
            .field("prompt_style", &self.prompt_style)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FastEmbedPromptStyle {
    None,
    E5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FastEmbedEmbeddingConfig {
    model: EmbeddingModel,
    model_name: String,
    dimension: usize,
    prompt_style: FastEmbedPromptStyle,
    cache_dir: Option<PathBuf>,
    show_download_progress: bool,
}

impl FastEmbedMemoryEmbedder {
    /// Opens the model, downloading it into the cache on first use. Blocking:
    /// the daemon runs it on a background thread.
    fn load(config: FastEmbedEmbeddingConfig) -> io::Result<Self> {
        let mut options = TextInitOptions::new(config.model.clone())
            .with_show_download_progress(config.show_download_progress);
        if let Some(cache_dir) = &config.cache_dir {
            options = options.with_cache_dir(cache_dir.clone());
        }
        let model = TextEmbedding::try_new(options).map_err(fastembed_error)?;
        Ok(Self {
            model_name: config.model_name,
            dimension: config.dimension,
            prompt_style: config.prompt_style,
            model: Arc::new(Mutex::new(model)),
        })
    }

    fn embed_memory(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        self.embed_prefixed(text, FastEmbedTextRole::Passage)
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        self.embed_prefixed(text, FastEmbedTextRole::Query)
    }

    /// One model call for a rebuild batch; `None` when that call fails.
    fn embed_passages(&self, texts: &[&str]) -> Option<Vec<Option<Vec<f32>>>> {
        let inputs: Vec<(usize, String)> = texts
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                let text = text.trim();
                (!text.is_empty())
                    .then(|| (index, self.prefixed_text(text, FastEmbedTextRole::Passage)))
            })
            .collect();
        let mut vectors = vec![None; texts.len()];
        if inputs.is_empty() {
            return Some(vectors);
        }
        let embeddings = self
            .model
            .lock()
            .ok()?
            .embed(
                inputs
                    .iter()
                    .map(|(_, input)| input.as_str())
                    .collect::<Vec<_>>(),
                Some(REBUILD_BATCH_SIZE),
            )
            .ok()?;
        if embeddings.len() != inputs.len() {
            return None;
        }
        for ((index, _), embedding) in inputs.iter().zip(embeddings) {
            if embedding.len() == self.dimension {
                vectors[*index] = Some(embedding);
            }
        }
        Some(vectors)
    }

    fn embed_prefixed(
        &self,
        text: &str,
        role: FastEmbedTextRole,
    ) -> Result<Vec<f32>, MemoryVectorError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(MemoryVectorError::EmbeddingUnavailable);
        }
        let input = self.prefixed_text(text, role);
        let mut model = self
            .model
            .lock()
            .map_err(|_| MemoryVectorError::EmbeddingUnavailable)?;
        let embedding = model
            .embed([input], None)
            .map_err(|_| MemoryVectorError::EmbeddingUnavailable)?
            .into_iter()
            .next()
            .filter(|embedding| !embedding.is_empty())
            .ok_or(MemoryVectorError::EmbeddingUnavailable)?;
        if embedding.len() != self.dimension {
            return Err(MemoryVectorError::DimensionMismatch);
        }
        Ok(embedding)
    }

    fn prefixed_text(&self, text: &str, role: FastEmbedTextRole) -> String {
        match (self.prompt_style, role) {
            (FastEmbedPromptStyle::E5, FastEmbedTextRole::Query) => format!("query: {text}"),
            (FastEmbedPromptStyle::E5, FastEmbedTextRole::Passage) => format!("passage: {text}"),
            (FastEmbedPromptStyle::None, _) => text.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FastEmbedTextRole {
    Query,
    Passage,
}

impl FastEmbedEmbeddingConfig {
    /// The cache dir comes from [`model_cache_dir`]; `default_cache_dir` is
    /// `models/` beside the memory store, when there is one.
    fn from_env(dimension: Option<usize>, default_cache_dir: Option<PathBuf>) -> io::Result<Self> {
        let model_name = std::env::var("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_FASTEMBED_MODEL.to_string());
        let model = parse_fastembed_model(&model_name)?;
        let model_info = TextEmbedding::get_model_info(&model).map_err(fastembed_error)?;
        let expected_dimension = model_info.dim;
        if let Some(dimension) = dimension {
            if dimension != expected_dimension {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS must be {expected_dimension} for fastembed model {model_name}"
                    ),
                ));
            }
        }
        let resolved_model_name = model_info.model_code.clone();
        let prompt_style = prompt_style_for_model(&model_info.model);
        Ok(Self {
            model,
            model_name: resolved_model_name,
            dimension: expected_dimension,
            prompt_style,
            cache_dir: model_cache_dir(
                env_path("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR")?,
                default_cache_dir,
                &UserCacheEnv::current(),
            ),
            // Progress bars would fill the daemon's logs; opt in with the env.
            show_download_progress: bool_from_env(
                "ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS",
                false,
            )?,
        })
    }
}

#[derive(Clone, Debug)]
struct OpenAiCompatibleMemoryEmbedder {
    provider: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
    dimension: usize,
    client: BlockingHttpClient,
}

struct OpenAiCompatibleEmbeddingConfig {
    provider: &'static str,
    default_base_url: &'static str,
    default_model: &'static str,
    default_dimension: usize,
    api_key_envs: &'static [&'static str],
    base_url_envs: &'static [&'static str],
    requires_key: bool,
    dimension: Option<usize>,
}

impl OpenAiCompatibleMemoryEmbedder {
    fn from_env(config: OpenAiCompatibleEmbeddingConfig) -> io::Result<Self> {
        let timeout = Duration::from_millis(timeout_millis_from_env()?);
        let api_key = first_non_empty_env_value(config.api_key_envs);
        if config.requires_key && api_key.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} must be configured for {} memory embeddings",
                    config.api_key_envs.join(" or "),
                    config.provider
                ),
            ));
        }
        let model = std::env::var("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| config.default_model.to_string());
        let base_url = first_non_empty_env_value(config.base_url_envs)
            .unwrap_or_else(|| config.default_base_url.to_string());
        let dimension = config.dimension.unwrap_or(config.default_dimension);
        if dimension == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS must be greater than 0",
            ));
        }
        Self::new(
            config.provider,
            base_url,
            model,
            api_key,
            dimension,
            timeout,
        )
    }

    fn new(
        provider: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
        dimension: usize,
        timeout: Duration,
    ) -> io::Result<Self> {
        let client = BlockingHttpClient::builder()
            .timeout(timeout)
            .build()
            .map_err(http_client_error)?;
        Ok(Self {
            provider: provider.into(),
            model: model.into(),
            base_url: trim_base_url(base_url.into()),
            api_key,
            dimension,
            client,
        })
    }

    fn embeddings_url(&self) -> String {
        format!("{}/embeddings", self.base_url)
    }
}

impl MemoryTextEmbedder for OpenAiCompatibleMemoryEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(MemoryVectorError::EmbeddingUnavailable);
        }

        let request = OpenAiEmbeddingRequest {
            model: &self.model,
            input: text,
        };
        let mut builder = self.client.post(self.embeddings_url()).json(&request);
        if let Some(api_key) = &self.api_key {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder
            .send()
            .map_err(|_| MemoryVectorError::EmbeddingUnavailable)?;
        if !response.status().is_success() {
            return Err(MemoryVectorError::EmbeddingUnavailable);
        }
        let response = response
            .json::<OpenAiEmbeddingResponse>()
            .map_err(|_| MemoryVectorError::EmbeddingUnavailable)?;
        let embedding = response
            .data
            .into_iter()
            .next()
            .map(|data| data.embedding)
            .filter(|embedding| !embedding.is_empty())
            .ok_or(MemoryVectorError::EmbeddingUnavailable)?;
        if embedding.len() != self.dimension {
            return Err(MemoryVectorError::DimensionMismatch);
        }
        Ok(embedding)
    }
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
}

const UPSERT_EMBEDDING_SQL: &str = r#"
    INSERT INTO memory_embeddings(memory_id, model, dimension, vector_json, updated_at, content_hash)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
    ON CONFLICT(memory_id) DO UPDATE SET
        model = excluded.model,
        dimension = excluded.dimension,
        vector_json = excluded.vector_json,
        updated_at = excluded.updated_at,
        content_hash = excluded.content_hash
"#;

#[derive(Clone, Debug)]
struct SqliteMemoryEmbeddingStore {
    path: PathBuf,
    owner: Option<crate::profile_owner::ProfileOwner>,
}

#[derive(Clone, Debug, PartialEq)]
struct StoredEmbedding {
    memory_id: String,
    model: String,
    vector: Vec<f32>,
    content_hash: Option<[u8; 32]>,
}

impl SqliteMemoryEmbeddingStore {
    fn new(path: PathBuf) -> io::Result<Self> {
        Self::new_with_owner(path, None)
    }

    fn new_with_owner(
        path: PathBuf,
        owner: Option<crate::profile_owner::ProfileOwner>,
    ) -> io::Result<Self> {
        let path = owner
            .as_ref()
            .map(|owner| owner.owned_sqlite_path(&path))
            .transpose()?
            .unwrap_or(path);
        if let Some(owner) = &owner {
            owner.ensure_owned()?;
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { path, owner };
        store.ensure_schema()?;
        Ok(store)
    }

    fn connection(&self) -> io::Result<Connection> {
        if let Some(owner) = &self.owner {
            owner.ensure_owned()?;
        }
        let connection = Connection::open(&self.path).map_err(sqlite_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(sqlite_error)?;
        Ok(connection)
    }

    fn ensure_schema(&self) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        transaction
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS memory_embedding_schema (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS memory_embeddings (
                    memory_id TEXT PRIMARY KEY NOT NULL,
                    model TEXT NOT NULL,
                    dimension INTEGER NOT NULL,
                    vector_json TEXT NOT NULL,
                    content_hash TEXT,
                    updated_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_memory_embeddings_model
                    ON memory_embeddings(model);

                "#,
            )
            .map_err(sqlite_error)?;
        let has_content_hash = {
            let mut statement = transaction
                .prepare("PRAGMA table_info(memory_embeddings)")
                .map_err(sqlite_error)?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(sqlite_error)?;
            let mut found = false;
            for column in columns {
                found |= column.map_err(sqlite_error)? == "content_hash";
            }
            found
        };
        if !has_content_hash {
            transaction
                .execute_batch("ALTER TABLE memory_embeddings ADD COLUMN content_hash TEXT")
                .map_err(sqlite_error)?;
        }
        transaction
            .execute(
                "INSERT OR REPLACE INTO memory_embedding_schema(key,value) VALUES ('version','2')",
                [],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)
    }

    fn load_all(&self) -> io::Result<Vec<StoredEmbedding>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT memory_id, model, dimension, vector_json, content_hash FROM memory_embeddings ORDER BY memory_id",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement.query([]).map_err(sqlite_error)?;
        let mut embeddings = Vec::new();

        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let memory_id: String = row.get(0).map_err(sqlite_error)?;
            let model: String = row.get(1).map_err(sqlite_error)?;
            let dimension: usize = row
                .get::<_, i64>(2)
                .map_err(sqlite_error)?
                .try_into()
                .unwrap_or(0);
            let raw: String = row.get(3).map_err(sqlite_error)?;
            let content_hash = row
                .get::<_, Option<String>>(4)
                .map_err(sqlite_error)?
                .as_deref()
                .and_then(decode_digest);
            let Ok(vector) = serde_json::from_str::<Vec<f32>>(&raw) else {
                warn!(memory_id = %memory_id, "skipping malformed persisted memory embedding");
                continue;
            };
            if vector.len() != dimension || vector.iter().any(|value| !value.is_finite()) {
                warn!(memory_id = %memory_id, "skipping invalid persisted memory embedding");
                continue;
            }
            embeddings.push(StoredEmbedding {
                memory_id,
                model,
                vector,
                content_hash,
            });
        }

        Ok(embeddings)
    }

    fn upsert(
        &self,
        memory_id: &str,
        model: &str,
        vector: &[f32],
        digest: &[u8; 32],
    ) -> io::Result<()> {
        let vector_json = serde_json::to_string(vector).map_err(json_error)?;
        self.connection()?
            .execute(
                UPSERT_EMBEDDING_SQL,
                params![
                    memory_id,
                    model,
                    vector.len() as i64,
                    vector_json,
                    anima_core::primitives::now_millis().to_string(),
                    encode_digest(digest),
                ],
            )
            .map(|_| ())
            .map_err(sqlite_error)
    }

    /// Saves a rebuild batch in one transaction.
    fn upsert_many(&self, model: &str, rows: &[(&str, Vec<f32>, [u8; 32])]) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        {
            let mut statement = transaction
                .prepare(UPSERT_EMBEDDING_SQL)
                .map_err(sqlite_error)?;
            let updated_at = anima_core::primitives::now_millis().to_string();
            for (memory_id, vector, digest) in rows {
                let vector_json = serde_json::to_string(vector).map_err(json_error)?;
                statement
                    .execute(params![
                        memory_id,
                        model,
                        vector.len() as i64,
                        vector_json,
                        updated_at,
                        encode_digest(digest),
                    ])
                    .map_err(sqlite_error)?;
            }
        }
        transaction.commit().map_err(sqlite_error)
    }

    fn delete_many(&self, memory_ids: &[String]) -> io::Result<usize> {
        let connection = self.connection()?;
        let mut removed = 0;
        for memory_id in memory_ids {
            removed += connection
                .execute(
                    "DELETE FROM memory_embeddings WHERE memory_id = ?1",
                    params![memory_id],
                )
                .map_err(sqlite_error)?;
        }
        Ok(removed)
    }

    fn delete_stale(&self, valid_ids: &HashSet<String>) -> io::Result<usize> {
        let stale_ids = self
            .load_all()?
            .into_iter()
            .filter(|embedding| !valid_ids.contains(&embedding.memory_id))
            .map(|embedding| embedding.memory_id)
            .collect::<Vec<_>>();
        self.delete_many(&stale_ids)
    }
}

/// The mode `ANIMAOS_RS_MEMORY_EMBEDDINGS` names, lowercased; unset means
/// `fastembed`, except when `ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS` is set:
/// that knob predates the fastembed default and meant hashed vectors, so an
/// upgraded daemon keeps `local` rather than refusing to boot. The flag says
/// that exception applied, for the boot warning.
fn embeddings_mode(explicit: Option<&str>, dimension: Option<usize>) -> (String, bool) {
    match explicit {
        Some(mode) => (mode.to_ascii_lowercase(), false),
        None if dimension.is_some() => (LOCAL_PROVIDER.to_string(), true),
        None => (FASTEMBED_PROVIDER.to_string(), false),
    }
}

/// What the per-user cache location depends on, read once from the
/// environment.
#[derive(Clone, Debug, Default)]
struct UserCacheEnv {
    macos: bool,
    home: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
}

impl UserCacheEnv {
    fn current() -> Self {
        let non_empty = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Self {
            macos: cfg!(target_os = "macos"),
            home: non_empty("HOME").or_else(|| non_empty("USERPROFILE")),
            xdg_cache_home: non_empty("XDG_CACHE_HOME"),
        }
    }
}

/// Where the embedding model is cached: `explicit`
/// (`ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR`), then `beside_store` (`models/`
/// next to the memory store), then the per-user cache: macOS
/// `~/Library/Caches/animaos/models`, elsewhere `$XDG_CACHE_HOME/animaos/models`
/// or `~/.cache/animaos/models`. Never the working directory; `None` (fastembed's
/// own default) only when there is no home directory either.
fn model_cache_dir(
    explicit: Option<PathBuf>,
    beside_store: Option<PathBuf>,
    user: &UserCacheEnv,
) -> Option<PathBuf> {
    explicit.or(beside_store).or_else(|| {
        let base = if user.macos {
            user.home.as_ref()?.join("Library").join("Caches")
        } else {
            match user
                .xdg_cache_home
                .as_ref()
                .filter(|path| path.is_absolute())
            {
                Some(xdg) => xdg.clone(),
                None => user.home.as_ref()?.join(".cache"),
            }
        };
        Some(base.join("animaos").join("models"))
    })
}

fn env_path(name: &'static str) -> io::Result<Option<PathBuf>> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must not be empty"),
        ));
    }
    Ok(Some(PathBuf::from(value)))
}

fn embedding_dimension_from_env() -> io::Result<Option<usize>> {
    let Some(value) = std::env::var_os("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS") else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS must not be empty",
        ));
    }
    let value = value.to_string_lossy();
    let dimension = value.parse::<usize>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS must be a positive integer",
        )
    })?;
    if dimension == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS must be greater than 0",
        ));
    }
    Ok(Some(dimension))
}

fn timeout_millis_from_env() -> io::Result<u64> {
    let Some(value) = std::env::var_os("ANIMAOS_RS_MEMORY_EMBEDDINGS_TIMEOUT_MS") else {
        return Ok(DEFAULT_EMBEDDING_TIMEOUT_MS);
    };
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MEMORY_EMBEDDINGS_TIMEOUT_MS must not be empty",
        ));
    }
    let timeout = value.to_string_lossy().parse::<u64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MEMORY_EMBEDDINGS_TIMEOUT_MS must be a positive integer",
        )
    })?;
    if timeout == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MEMORY_EMBEDDINGS_TIMEOUT_MS must be greater than 0",
        ));
    }
    Ok(timeout)
}

fn bool_from_env(name: &'static str, default: bool) -> io::Result<bool> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(default);
    };
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must not be empty"),
        ));
    }
    match value.to_string_lossy().trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be true or false"),
        )),
    }
}

fn parse_fastembed_model(model: &str) -> io::Result<EmbeddingModel> {
    let normalized = model.trim().to_ascii_lowercase().replace('_', "-");
    let parsed = match normalized.as_str() {
        "intfloat/multilingual-e5-small" | "multilingual-e5-small" | "e5-small" => {
            EmbeddingModel::MultilingualE5Small
        }
        "intfloat/multilingual-e5-base" | "multilingual-e5-base" | "e5-base" => {
            EmbeddingModel::MultilingualE5Base
        }
        "qdrant/multilingual-e5-large-onnx"
        | "intfloat/multilingual-e5-large"
        | "multilingual-e5-large"
        | "e5-large" => EmbeddingModel::MultilingualE5Large,
        "baai/bge-m3" | "bge-m3" => EmbeddingModel::BGEM3,
        "xenova/paraphrase-multilingual-minilm-l12-v2"
        | "paraphrase-multilingual-minilm-l12-v2" => EmbeddingModel::ParaphraseMLMiniLML12V2,
        "qdrant/paraphrase-multilingual-minilm-l12-v2-onnx-q"
        | "paraphrase-multilingual-minilm-l12-v2-q" => EmbeddingModel::ParaphraseMLMiniLML12V2Q,
        "xenova/paraphrase-multilingual-mpnet-base-v2"
        | "paraphrase-multilingual-mpnet-base-v2" => EmbeddingModel::ParaphraseMLMpnetBaseV2,
        _ => model.parse::<EmbeddingModel>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported fastembed memory embedding model: {model}"),
            )
        })?,
    };
    Ok(parsed)
}

fn prompt_style_for_model(model: &EmbeddingModel) -> FastEmbedPromptStyle {
    match model {
        EmbeddingModel::MultilingualE5Small
        | EmbeddingModel::MultilingualE5Base
        | EmbeddingModel::MultilingualE5Large => FastEmbedPromptStyle::E5,
        _ => FastEmbedPromptStyle::None,
    }
}

fn first_non_empty_env_value(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn trim_base_url(mut base_url: String) -> String {
    while base_url.ends_with('/') {
        base_url.pop();
    }
    base_url
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            current.push(character.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(normalize_token(&current));
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(normalize_token(&current));
    }
    tokens
}

fn normalize_token(token: &str) -> String {
    for suffix in ["ing", "ed", "es", "s"] {
        if token.len() > suffix.len() + 3 && token.ends_with(suffix) {
            return token[..token.len() - suffix.len()].to_string();
        }
    }
    token.to_string()
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn sqlite_error(error: rusqlite::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn http_client_error(error: reqwest::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

fn fastembed_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

static SEMANTIC_GROUPS: &[&[&str]] = &[
    &[
        "release", "ship", "launch", "deploy", "delivery", "publish", "rollout",
    ],
    &[
        "brief",
        "briefing",
        "summary",
        "summar",
        "note",
        "notes",
        "changelog",
        "report",
    ],
    &["concise", "terse", "short", "compact", "succinct", "tight"],
    &["preference", "prefer", "like", "style", "want", "wants"],
    &["rollback", "risk", "fallback", "revert", "recovery"],
    &["billing", "invoice", "ledger", "finance", "payment"],
    &[
        "latency",
        "performance",
        "speed",
        "slow",
        "fast",
        "throughput",
    ],
    &["memory", "remember", "recall", "fact", "context"],
    &["agent", "assistant", "worker", "planner", "critic"],
    &["user", "operator", "human", "customer"],
];

/// Model loaders and a fake model for tests, so no test downloads a model.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::*;

    pub(super) const FAKE_PROVIDER: &str = "fake";
    pub(super) const FAKE_MODEL: &str = "fake-model";

    /// Hashed vectors under another model id, counting every embed call.
    #[derive(Clone, Debug)]
    pub(crate) struct FakeModelEmbedder {
        hashed: LocalMemoryEmbedder,
        calls: Arc<AtomicUsize>,
    }

    impl FakeModelEmbedder {
        pub(super) fn dimension(&self) -> usize {
            self.hashed.dimension
        }

        pub(super) fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.hashed.embed(text)
        }
    }

    pub(crate) fn fake_model(dimension: usize) -> (LoadedMemoryEmbedder, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let embedder = FakeModelEmbedder {
            hashed: LocalMemoryEmbedder::new(dimension),
            calls: Arc::clone(&calls),
        };
        (
            LoadedMemoryEmbedder(MemoryEmbeddingProvider::Fake(embedder)),
            calls,
        )
    }

    /// A loader that succeeds at once with the fake model.
    pub(crate) fn fake_model_loader(dimension: usize) -> (MemoryModelLoader, Arc<AtomicUsize>) {
        let (loaded, calls) = fake_model(dimension);
        (Box::new(move || Ok(loaded)), calls)
    }

    pub(crate) fn failing_model_loader(error: &str) -> MemoryModelLoader {
        let error = error.to_string();
        Box::new(move || Err(error))
    }

    /// A loader that waits until the returned sender fires (or is dropped).
    pub(crate) fn gated_model_loader(dimension: usize) -> (MemoryModelLoader, mpsc::Sender<()>) {
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let (loaded, _) = fake_model(dimension);
        let loader: MemoryModelLoader = Box::new(move || {
            let _ = gate.lock().expect("gate lock").recv();
            Ok(loaded)
        });
        (loader, release)
    }

    /// The runtime fastembed mode boots with, waiting on `loader` instead.
    pub(crate) fn awaiting(loader: MemoryModelLoader) -> MemoryEmbeddingRuntime {
        MemoryEmbeddingRuntime::awaiting_model(FAKE_PROVIDER, FAKE_MODEL.to_string(), loader)
    }

    /// Swaps in the fake model and re-embeds only `reembedded`, as a rebuild
    /// part way through.
    pub(crate) fn swap_part_way(
        runtime: &mut MemoryEmbeddingRuntime,
        memories: &MemoryManager,
        dimension: usize,
        reembedded: &[&str],
    ) {
        let (loaded, _) = fake_model(dimension);
        let all = memories.get_recent(RecentMemoryOptions {
            limit: Some(usize::MAX),
            ..RecentMemoryOptions::default()
        });
        let plan = runtime.begin_rebuild(loaded, &all, Vec::new());
        let batch: Vec<_> = plan
            .pending
            .into_iter()
            .filter(|(id, _)| reembedded.contains(&id.as_str()))
            .collect();
        let texts: Vec<&str> = batch.iter().map(|(_, content)| content.as_str()).collect();
        let vectors = plan.embedder.embed_memory_batch(&texts);
        assert!(runtime.apply_rebuild_batch(plan.generation, memories, &batch, vectors));
    }
}

#[cfg(test)]
mod tests {
    use std::fs::remove_file;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;
    use anima_memory::{MemoryScope, MemoryType};

    static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn local_embedder_connects_semantic_release_words() {
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        let memory = test_memory("memory-1", "Operator wants concise ship notes");

        runtime
            .upsert_memory(&memory)
            .expect("embedding should index");
        let hits = runtime.search("release briefing style", 1);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_id, "memory-1");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn native_filtered_vector_search_selects_eligible_before_limit_and_preserves_unfiltered_order()
    {
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        for memory in [
            test_memory("excluded-a", "release briefing style"),
            test_memory("excluded-b", "release briefing style"),
            test_memory("owned", "Operator wants concise ship notes"),
        ] {
            runtime.upsert_memory(&memory).unwrap();
        }
        let unfiltered = runtime.search("release briefing style", 3);
        assert_eq!(unfiltered.len(), 3);
        assert_eq!(unfiltered[0].memory_id, "excluded-a");
        assert_eq!(unfiltered[1].memory_id, "excluded-b");
        assert_eq!(
            runtime.search_filtered("release briefing style", 3, &|_| true),
            unfiltered
        );
        let filtered = runtime.search_filtered("release briefing style", 1, &|id| id == "owned");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].memory_id, "owned");
        assert_eq!(filtered[0].score, unfiltered[2].score);
        assert!(filtered[0].score > 0.0);
        assert!(runtime
            .search_filtered("release briefing style", 1, &|_| false)
            .is_empty());
    }

    #[test]
    fn native_filtered_vector_search_disabled_zero_empty_and_unavailable_fail_empty() {
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        let no_scan = |_: &str| -> bool { panic!("empty search must not scan candidates") };
        assert!(runtime
            .search_filtered("release briefing", 1, &no_scan)
            .is_empty());
        runtime
            .upsert_memory(&test_memory("owned", "ship notes"))
            .unwrap();
        assert!(runtime
            .search_filtered("release briefing", 0, &no_scan)
            .is_empty());
        assert!(runtime.search("release briefing", 0).is_empty());
        assert!(runtime.search_filtered("", 1, &no_scan).is_empty());
        assert!(runtime.search_filtered("!!!", 1, &no_scan).is_empty());
        let mut disabled = MemoryEmbeddingRuntime::disabled();
        disabled
            .index
            .upsert_embedding("owned", vec![1.0; DEFAULT_EMBEDDING_DIMENSION])
            .unwrap();
        assert!(disabled
            .search_filtered("release briefing", 1, &no_scan)
            .is_empty());
    }

    #[test]
    fn sqlite_store_reloads_vectors_and_rebuilds_missing() {
        let path = temp_sqlite_path("reload");
        let _ = remove_file(&path);
        let memories = vec![
            test_memory("memory-1", "Operator wants concise ship notes"),
            test_memory("memory-2", "Billing ledger exports include invoice IDs"),
        ];

        let mut first = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        first.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).expect("store opens"));
        let report = first
            .rebuild_from_memories(&memories)
            .expect("first rebuild should work");
        assert_eq!(report.rebuilt_vectors, 2);

        let mut second = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        second.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).expect("store opens"));
        let report = second
            .rebuild_from_memories(&memories)
            .expect("second rebuild should work");
        assert_eq!(report.loaded_vectors, 2);
        assert_eq!(report.rebuilt_vectors, 0);
        assert_eq!(second.search("invoice ledger", 1)[0].memory_id, "memory-2");

        let _ = remove_file(&path);
    }

    #[test]
    fn memory_embedding_content_identity_rebuilds_changed_text_cache() {
        let path = temp_sqlite_path("content-identity");
        let _ = remove_file(&path);
        let original = test_memory("changed", "Operator wants concise ship notes");
        let neighbor = test_memory("neighbor", "Operator wants concise ship notes");
        let mut first = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        first.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
        first.upsert_memory(&original).unwrap();
        first.upsert_memory(&neighbor).unwrap();
        assert_eq!(first.search(&original.content, 1)[0].memory_id, original.id);

        // Simulate canonical persistence succeeding before a vector rewrite: same
        // ID/model/dimension, but the full current text now describes another fact.
        let mut changed = original.clone();
        changed.content = "Billing ledger exports include invoice IDs".into();
        let canonical = vec![changed.clone(), neighbor.clone()];
        assert_eq!(canonical[1], neighbor);
        let mut fresh = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        for memory in &canonical {
            fresh.upsert_memory(memory).unwrap();
        }
        assert_eq!(fresh.search(&original.content, 1)[0].memory_id, neighbor.id);
        assert_eq!(fresh.search(&changed.content, 1)[0].memory_id, changed.id);
        assert_ne!(
            first.search(&original.content, 2),
            fresh.search(&original.content, 2)
        );

        let mut restored = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        restored.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
        let report = restored.rebuild_from_memories(&canonical).unwrap();
        assert_eq!(
            report.rebuilt_vectors, 1,
            "changed canonical text must invalidate its old cached vector"
        );
        assert_eq!(
            report.loaded_vectors, 1,
            "the unchanged neighbor retains its cached vector"
        );
        for query in [&original.content, &changed.content] {
            assert_eq!(restored.search(query, 2), fresh.search(query, 2));
        }
        assert_eq!(canonical, vec![changed, neighbor]);
        drop(first);
        drop(restored);
        let _ = remove_file(&path);
    }

    #[test]
    fn memory_embedding_content_identity_migrates_legacy_null_cache_once() {
        let path = temp_sqlite_path("legacy-content-hash");
        let _ = remove_file(&path);
        let memory = test_memory("legacy", "Operator wants concise ship notes");
        let vector = LocalMemoryEmbedder::new(DEFAULT_EMBEDDING_DIMENSION)
            .embed(&memory.content)
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE memory_embedding_schema(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            INSERT INTO memory_embedding_schema VALUES('version','1');
            CREATE TABLE memory_embeddings(memory_id TEXT PRIMARY KEY,model TEXT NOT NULL,dimension INTEGER NOT NULL,vector_json TEXT NOT NULL,updated_at TEXT NOT NULL);").unwrap();
        connection
            .execute(
                "INSERT INTO memory_embeddings VALUES(?1,?2,?3,?4,'1')",
                params![
                    memory.id,
                    LOCAL_MODEL,
                    DEFAULT_EMBEDDING_DIMENSION as i64,
                    serde_json::to_string(&vector).unwrap()
                ],
            )
            .unwrap();
        drop(connection);
        let store = SqliteMemoryEmbeddingStore::new(path.clone()).unwrap();
        assert_eq!(
            store.load_all().unwrap()[0].content_hash,
            None,
            "legacy identity is never a wildcard"
        );
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        runtime.store = Some(store);
        assert_eq!(
            runtime
                .rebuild_from_memories(std::slice::from_ref(&memory))
                .unwrap()
                .rebuilt_vectors,
            1
        );
        assert!(runtime.matches_current(&memory));
        let store = SqliteMemoryEmbeddingStore::new(path.clone()).unwrap();
        store.ensure_schema().unwrap();
        let version: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT value FROM memory_embedding_schema WHERE key='version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "2");
        assert_eq!(
            store.load_all().unwrap()[0].content_hash,
            Some(content_digest(&memory.content))
        );
        let mut reloaded = MemoryEmbeddingRuntime::local_default();
        reloaded.store = Some(store);
        let report = reloaded
            .rebuild_from_memories(std::slice::from_ref(&memory))
            .unwrap();
        assert_eq!((report.loaded_vectors, report.rebuilt_vectors), (1, 0));
        let _ = remove_file(path);
    }

    #[test]
    fn memory_embedding_content_identity_rejects_bad_hash_model_dimension_and_zero_vector() {
        for case in 0..5 {
            let path = temp_sqlite_path("invalid-content-cache");
            let _ = remove_file(&path);
            let memory = test_memory("current", "Billing invoice ledger");
            let mut runtime = MemoryEmbeddingRuntime::local_default();
            runtime.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
            runtime.upsert_memory(&memory).unwrap();
            let connection = runtime.store.as_ref().unwrap().connection().unwrap();
            match case {
                0 => {
                    connection
                        .execute(
                            "UPDATE memory_embeddings SET content_hash=?1",
                            ["G".repeat(64)],
                        )
                        .unwrap();
                }
                1 => {
                    connection
                        .execute(
                            "UPDATE memory_embeddings SET content_hash=?1",
                            [encode_digest(&content_digest("old text"))],
                        )
                        .unwrap();
                }
                2 => {
                    connection
                        .execute("UPDATE memory_embeddings SET model='other-model'", [])
                        .unwrap();
                }
                3 => {
                    connection
                        .execute(
                            "UPDATE memory_embeddings SET dimension=2,vector_json='[1,0]'",
                            [],
                        )
                        .unwrap();
                }
                _ => {
                    connection
                        .execute(
                            "UPDATE memory_embeddings SET vector_json=?1",
                            [
                                serde_json::to_string(&vec![0.0_f32; DEFAULT_EMBEDDING_DIMENSION])
                                    .unwrap(),
                            ],
                        )
                        .unwrap();
                }
            }
            drop(connection);
            let report = runtime
                .rebuild_from_memories(std::slice::from_ref(&memory))
                .unwrap();
            assert_eq!(
                (report.loaded_vectors, report.rebuilt_vectors),
                (0, 1),
                "case {case}"
            );
            assert!(runtime.matches_current(&memory));
            assert_eq!(
                runtime
                    .rebuild_from_memories(std::slice::from_ref(&memory))
                    .unwrap()
                    .loaded_vectors,
                1,
                "repaired case {case} loads next time"
            );
            let _ = remove_file(path);
        }
    }

    #[test]
    fn memory_embedding_content_identity_filters_failed_and_late_old_upserts_before_top_k() {
        let original = test_memory("a-stale", "Operator wants concise ship notes");
        let current_match = test_memory("b-current", &original.content);
        let neighbor = test_memory("neighbor", "Bicycle chain requires oil");
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        for memory in [&original, &current_match, &neighbor] {
            runtime.upsert_memory(memory).unwrap();
        }
        let before_hits = runtime.search(&original.content, 3);
        let mut changed = original.clone();
        changed.content = "!!!".into();
        assert!(runtime.upsert_memory(&changed).is_err());
        assert_eq!(runtime.search(&original.content, 3), before_hits);
        assert!(runtime.matches_current(&original));
        assert!(
            !runtime.matches_current(&changed),
            "failed embedding cannot relabel the old vector"
        );
        changed.content = "Billing invoice ledger".into();
        runtime.upsert_memory(&changed).unwrap();
        assert!(runtime.matches_current(&changed));
        runtime.upsert_memory(&original).unwrap(); // A delayed old snapshot wins the index write.
        assert!(!runtime.matches_current(&changed));
        let canonical = vec![changed.clone(), current_match.clone(), neighbor.clone()];
        let mut manager = MemoryManager::new();
        manager.replace_snapshot(anima_memory::MemoryManagerSnapshot {
            memories: canonical.clone(),
            ..Default::default()
        });
        assert_eq!(
            runtime.search(&original.content, 1)[0].memory_id,
            original.id
        );
        assert_eq!(
            runtime.for_memories(&manager).search(&original.content, 1)[0].memory_id,
            current_match.id
        );
        assert!(runtime
            .for_memories(&manager)
            .search_filtered(&original.content, 1, &|id| id == original.id)
            .is_empty());
        assert_eq!(manager.snapshot().memories, canonical);
        let mut exact = current_match.clone();
        for text in [
            format!(" {}", exact.content),
            format!("{} ", exact.content),
            format!("{}é", exact.content),
        ] {
            exact.content = text;
            assert!(
                !runtime.matches_current(&exact),
                "identity is full exact UTF8, not normalized"
            );
        }
        let long = test_memory("long", &format!("{}first", "ship ".repeat(200)));
        runtime.upsert_memory(&long).unwrap();
        let mut changed_tail = long.clone();
        changed_tail.content = format!("{}other", "ship ".repeat(200));
        assert!(
            !runtime.matches_current(&changed_tail),
            "identity includes text beyond context excerpts"
        );
        assert!(
            !runtime
                .for_memories(&manager)
                .search(&long.content, 10)
                .iter()
                .any(|hit| hit.memory_id == long.id),
            "missing canonical ID is excluded"
        );
    }

    #[test]
    fn memory_embedding_content_identity_pairs_cache_failure_and_remove_clear_lifecycle() {
        let path = temp_sqlite_path("content-pair-lifecycle");
        let _ = remove_file(&path);
        let original = test_memory("current", "Operator wants concise ship notes");
        let neighbor = test_memory("neighbor", "Bicycle chain oil");
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        runtime.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
        runtime.upsert_memory(&original).unwrap();
        runtime.upsert_memory(&neighbor).unwrap();
        runtime.store.as_ref().unwrap().connection().unwrap().execute_batch("CREATE TRIGGER deny_cache_write BEFORE INSERT ON memory_embeddings BEGIN SELECT RAISE(FAIL,'synthetic cache failure'); END;").unwrap();
        let mut changed = original.clone();
        changed.content = "Billing invoice ledger".into();
        assert!(runtime.upsert_memory(&changed).is_err());
        assert!(
            runtime.matches_current(&changed),
            "in-memory successful vector/hash remains truthful after cache failure"
        );
        assert!(!runtime.matches_current(&original));
        assert_eq!(
            runtime
                .store
                .as_ref()
                .unwrap()
                .load_all()
                .unwrap()
                .iter()
                .find(|row| row.memory_id == original.id)
                .unwrap()
                .content_hash,
            Some(content_digest(&original.content))
        );
        runtime
            .store
            .as_ref()
            .unwrap()
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER deny_cache_write")
            .unwrap();
        let report = runtime
            .rebuild_from_memories(&[changed.clone(), neighbor.clone()])
            .unwrap();
        assert_eq!((report.rebuilt_vectors, report.loaded_vectors), (1, 1));
        runtime
            .remove_memories(std::slice::from_ref(&changed.id))
            .unwrap();
        assert!(!runtime.matches_current(&changed));
        assert!(runtime.matches_current(&neighbor));
        assert!(!runtime.indexed_content.contains_key(&changed.id));
        assert_eq!(runtime.status().vector_count, 1);
        runtime.rebuild_from_memories(&[]).unwrap();
        assert_eq!(runtime.status().vector_count, 0);
        assert!(runtime.indexed_content.is_empty());
        assert!(runtime
            .store
            .as_ref()
            .unwrap()
            .load_all()
            .unwrap()
            .is_empty());
        let _ = remove_file(path);
    }

    #[test]
    fn sqlite_store_skips_corrupt_vectors_and_rebuilds() {
        let path = temp_sqlite_path("corrupt");
        let _ = remove_file(&path);
        let store = SqliteMemoryEmbeddingStore::new(path.clone()).expect("store opens");
        store
            .connection()
            .expect("connection opens")
            .execute(
                "INSERT INTO memory_embeddings(memory_id, model, dimension, vector_json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["memory-1", LOCAL_MODEL, DEFAULT_EMBEDDING_DIMENSION as i64, "not-json", "1"],
            )
            .expect("corrupt row inserts");

        let mut runtime = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        runtime.store = Some(store);
        let report = runtime
            .rebuild_from_memories(&[test_memory("memory-1", "Operator wants concise ship notes")])
            .expect("rebuild should repair corrupt row");

        assert_eq!(report.loaded_vectors, 0);
        assert_eq!(report.rebuilt_vectors, 1);
        assert_eq!(
            runtime.search("release briefing style", 1)[0].memory_id,
            "memory-1"
        );

        let _ = remove_file(&path);
    }

    #[test]
    fn sqlite_store_removes_stale_vectors() {
        let path = temp_sqlite_path("stale");
        let _ = remove_file(&path);
        let store = SqliteMemoryEmbeddingStore::new(path.clone()).expect("store opens");
        store
            .upsert(
                "stale",
                LOCAL_MODEL,
                &[1.0, 0.0, 0.0],
                &content_digest("stale"),
            )
            .expect("stale vector inserts");
        let mut runtime = MemoryEmbeddingRuntime::local(DEFAULT_EMBEDDING_DIMENSION);
        runtime.store = Some(store);

        let report = runtime
            .rebuild_from_memories(&[test_memory("memory-1", "Operator wants concise ship notes")])
            .expect("rebuild should remove stale row");

        assert_eq!(report.removed_stale_vectors, 1);
        let loaded = runtime
            .store
            .as_ref()
            .unwrap()
            .load_all()
            .expect("load works");
        assert!(loaded
            .iter()
            .all(|embedding| embedding.memory_id != "stale"));

        let _ = remove_file(&path);
    }

    #[test]
    fn openai_compatible_embedder_posts_embeddings_request() {
        let (base_url, requests) = spawn_embedding_server(vec![
            r#"{"data":[{"embedding":[1.0,0.0,0.0]}]}"#,
            r#"{"data":[{"embedding":[1.0,0.0,0.0]}]}"#,
        ]);
        let embedder = OpenAiCompatibleMemoryEmbedder::new(
            OPENAI_COMPATIBLE_PROVIDER,
            &base_url,
            "text-embedding-test",
            Some("test-key".into()),
            3,
            Duration::from_secs(5),
        )
        .expect("embedder should construct");
        let mut runtime = MemoryEmbeddingRuntime::from_provider(
            MemoryEmbeddingProvider::OpenAiCompatible(embedder),
        );

        runtime
            .upsert_memory(&test_memory(
                "memory-1",
                "Operator wants concise release notes",
            ))
            .expect("provider should index memory");
        let hits = runtime.search("release briefing style", 1);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_id, "memory-1");
        assert_eq!(runtime.status().provider, OPENAI_COMPATIBLE_PROVIDER);
        assert_eq!(runtime.status().model, "text-embedding-test");
        assert_eq!(runtime.status().dimension, 3);
        let requests = requests.lock().expect("requests should be available");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("POST /v1/embeddings HTTP/1.1"));
        assert!(requests[0].contains("authorization: Bearer test-key"));
        assert!(requests[0].contains("text-embedding-test"));
        assert!(requests[1].contains("release briefing style"));
    }

    #[test]
    fn env_configures_ollama_openai_compatible_embeddings() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let (base_url, _requests) =
            spawn_embedding_server(vec![r#"{"data":[{"embedding":[0.0,1.0,0.0,0.0]}]}"#]);
        let guards = [
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS", OLLAMA_PROVIDER),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS_BASE_URL", &base_url),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL", "nomic-test"),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS", "4"),
        ];

        let mut runtime = MemoryEmbeddingRuntime::from_env(None, None)
            .expect("ollama embedding runtime should configure");
        runtime
            .upsert_memory(&test_memory("memory-1", "semantic memory"))
            .expect("ollama-compatible provider should index");

        let status = runtime.status();
        assert_eq!(status.provider, OLLAMA_PROVIDER);
        assert_eq!(status.model, "nomic-test");
        assert_eq!(status.dimension, 4);
        drop(guards);
    }

    #[test]
    fn env_rejects_openai_embeddings_without_api_key() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let guards = [
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS", OPENAI_PROVIDER),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_API_KEY"),
            EnvGuard::unset("OPENAI_API_KEY"),
            EnvGuard::unset("OPENAI_KEY"),
            EnvGuard::unset("OPENAI_TOKEN"),
        ];

        let error = match MemoryEmbeddingRuntime::from_env(None, None) {
            Ok(_) => panic!("openai provider should require an API key"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("OPENAI_API_KEY"));
        drop(guards);
    }

    #[test]
    fn env_rejects_too_small_dimension() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let guards = [
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS", LOCAL_PROVIDER),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS", "4"),
        ];

        let error = match MemoryEmbeddingRuntime::from_env(None, None) {
            Ok(_) => panic!("small embedding dimension should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        drop(guards);
    }

    #[test]
    fn fastembed_config_defaults_to_multilingual_e5_small() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let guards = [
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS"),
        ];

        let config = FastEmbedEmbeddingConfig::from_env(None, None)
            .expect("default fastembed config should parse without downloading");

        assert_eq!(config.model, EmbeddingModel::MultilingualE5Small);
        assert_eq!(config.model_name, DEFAULT_FASTEMBED_MODEL);
        assert_eq!(config.dimension, 384);
        assert_eq!(config.prompt_style, FastEmbedPromptStyle::E5);
        assert!(
            !config.show_download_progress,
            "download progress bars stay out of daemon logs"
        );
        drop(guards);
    }

    #[test]
    fn fastembed_config_accepts_model_aliases_and_cache_dir() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let cache_dir = std::env::temp_dir().join("anima-fastembed-cache-test");
        let cache_dir_string = cache_dir.to_string_lossy().to_string();
        let guards = [
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL", "bge-m3"),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS", "1024"),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR", &cache_dir_string),
            EnvGuard::set(
                "ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS",
                "false",
            ),
        ];

        let config = FastEmbedEmbeddingConfig::from_env(Some(1024), None)
            .expect("alias config should parse without downloading");

        assert_eq!(config.model, EmbeddingModel::BGEM3);
        assert_eq!(config.model_name, "BAAI/bge-m3");
        assert_eq!(config.dimension, 1024);
        assert_eq!(config.prompt_style, FastEmbedPromptStyle::None);
        assert_eq!(config.cache_dir, Some(cache_dir));
        assert!(!config.show_download_progress);
        drop(guards);
    }

    #[test]
    fn fastembed_config_rejects_dimension_mismatch_before_download() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let guards = [
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL", "multilingual-e5-small"),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS", "768"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS"),
        ];

        let error = match FastEmbedEmbeddingConfig::from_env(Some(768), None) {
            Ok(_) => panic!("dimension mismatch should fail before model download"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("must be 384"));
        drop(guards);
    }

    #[test]
    #[ignore = "downloads a fastembed model from Hugging Face"]
    fn fastembed_real_model_generates_multilingual_vectors() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let cache_dir = std::env::temp_dir().join("anima-fastembed-real-model-test");
        let cache_dir_string = cache_dir.to_string_lossy().to_string();
        let guards = [
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL", DEFAULT_FASTEMBED_MODEL),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS"),
            EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR", &cache_dir_string),
            EnvGuard::set(
                "ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS",
                "false",
            ),
        ];

        let config = FastEmbedEmbeddingConfig::from_env(None, None).expect("config parses");
        let embedder = FastEmbedMemoryEmbedder::load(config)
            .expect("fastembed model should download and load");
        let memory = embedder
            .embed_memory("Le gusta el te de menta antes de las demos de lanzamiento")
            .expect("Spanish passage should embed");
        let query = embedder
            .embed_query("What drink does the user like before launch demos?")
            .expect("cross-language query should embed");

        assert_eq!(memory.len(), 384);
        assert_eq!(query.len(), 384);
        assert!(memory.iter().all(|value| value.is_finite()));
        assert!(query.iter().all(|value| value.is_finite()));
        assert!(cosine(&memory, &query) > 0.0);
        drop(guards);
    }

    #[tokio::test]
    #[ignore = "loads public multilingual E5 assets for isolated learned-context acceptance"]
    async fn learned_context_acceptance_ranks_paraphrases_and_injects_the_actual_fact() {
        use anima_core::{AgentConfig, AgentRuntime, Content, Message, MessageRole};
        use anima_memory::{MemoryContextOptions, MemoryManager};
        // Declare the corpus, expected record and queries before observing any
        // learned scores; no local alias table or paid generation participates.
        const FACT: &str = "Severe peanut allergy: peanuts trigger anaphylaxis.";
        const QUERIES: [&str; 2] = [
            "¿Qué alimento debemos excluir para prevenir una reacción alérgica grave?",
            "Which ingredient is dangerous for this person to eat?",
        ];
        const DISTRACTORS: [&str; 8] = [
            "Preferred calendar reminders arrive twenty minutes before meetings.",
            "Vacation luggage should include a compact umbrella.",
            "The bicycle has a red frame and needs chain lubrication.",
            "Project archives belong on an encrypted external drive.",
            "Weekend entertainment choices include detective novels.",
            "Workspace lighting should be warm rather than fluorescent.",
            "The next language lesson covers verb conjugation.",
            "Monthly electricity bills are paid by bank transfer.",
        ];
        let mut learned = {
            let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
            let cache = std::env::temp_dir().join("anima-fastembed-real-model-test");
            let guards = [
                EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS", FASTEMBED_PROVIDER),
                EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL", DEFAULT_FASTEMBED_MODEL),
                EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS"),
                EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE"),
                EnvGuard::set(
                    "ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR",
                    &cache.to_string_lossy(),
                ),
                EnvGuard::set(
                    "ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS",
                    "false",
                ),
            ];
            let mut runtime = MemoryEmbeddingRuntime::from_env(None, None)
                .expect("fastembed configuration should parse");
            let loader = runtime
                .take_model_loader()
                .expect("fastembed loads in the background");
            let loaded = loader().expect("genuine cached E5 backend should load");
            let plan = runtime.begin_rebuild(loaded, &[], Vec::new());
            runtime.finish_rebuild(plan.generation);
            drop(guards);
            runtime
        };
        assert_eq!(learned.status().provider, FASTEMBED_PROVIDER);
        let runtime = AgentRuntime::new_with_id(
            "learned-context-owner",
            AgentConfig {
                name: "Planner".into(),
                model: "unused-deterministic".into(),
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
                settings: None,
                fallbacks: Vec::new(),
            },
            Arc::new(crate::model::DeterministicModelAdapter),
        );
        let mut target = test_memory("learned-peanut-fact", FACT);
        target.agent_id = runtime.id().into();
        target.importance = 0.5;
        let mut corpus = vec![target.clone()];
        for (index, text) in DISTRACTORS.iter().enumerate() {
            let mut memory = test_memory(&format!("unrelated-{index}"), text);
            memory.agent_id = runtime.id().into();
            memory.importance = 1.0;
            memory.created_at = index as u64 + 10;
            corpus.push(memory);
        }
        let mut excluded_ids = Vec::new();
        for (index, query) in QUERIES.iter().enumerate() {
            let foreign = test_memory(&format!("foreign-{index}"), query);
            let mut hidden = test_memory(&format!("hidden-{index}"), query);
            hidden.agent_id = runtime.id().into();
            hidden.tags = Some(vec!["hidden\u{202E}tag".into()]);
            excluded_ids.extend([foreign.id.clone(), hidden.id.clone()]);
            corpus.extend([foreign, hidden]);
        }
        let mut manager = MemoryManager::new();
        manager.replace_snapshot(anima_memory::MemoryManagerSnapshot {
            memories: corpus.clone(),
            ..Default::default()
        });
        for memory in &corpus {
            learned
                .upsert_memory(memory)
                .expect("real E5 passage should index");
        }
        let memory = Arc::new(tokio::sync::RwLock::new(manager));
        let embeddings = Arc::new(tokio::sync::RwLock::new(learned));
        let before = memory.read().await.snapshot();
        // Prove both predeclared ranking cases before testing the missing
        // automatic-context wiring, so its RED cannot conceal quality failures.
        for query in QUERIES {
            let manager = memory.read().await;
            let eligible = |id: &str| {
                manager.get(id).is_some_and(|record| {
                    record.agent_id == runtime.id()
                        && record.memory_type == MemoryType::Fact
                        && !record.tags.as_ref().is_some_and(|tags| {
                            tags.iter()
                                .any(|tag| crate::memory_text::has_hidden_text(tag))
                        })
                })
            };
            let index = embeddings.read().await;
            let ranking = index.search_filtered(query, corpus.len(), &eligible);
            assert_eq!(ranking.len(), DISTRACTORS.len() + 1);
            assert_eq!(
                ranking[0].memory_id, target.id,
                "predeclared learned relevance must beat all unrelated facts for {query:?}"
            );
            assert!(ranking
                .iter()
                .all(|hit| hit.score.is_finite() && hit.score > 0.0));
            assert!(ranking[1..].iter().all(|hit| hit.score < ranking[0].score));
            let global = index.search(query, corpus.len());
            assert!(
                global.iter().any(
                    |hit| excluded_ids.contains(&hit.memory_id) && hit.score > ranking[0].score
                ),
                "stronger ineligible vectors must not establish context authority"
            );
            let lexical = manager.select_context(
                MemoryContextOptions {
                    agent_id: runtime.id().into(),
                    room_id: Some("learned-room".into()),
                    query: query.into(),
                    limit: 6,
                },
                |_| true,
            );
            assert!(
                lexical.iter().all(|record| record.id != target.id),
                "the expected fact must not be a literal lexical match"
            );
        }
        let providers = crate::components::default_providers(memory.clone(), embeddings.clone());
        for query in QUERIES {
            let message = Message {
                id: "learned-context-input".into(),
                agent_id: runtime.id().into(),
                room_id: "learned-room".into(),
                role: MessageRole::User,
                created_at_ms: 1,
                content: Content {
                    text: query.into(),
                    ..Default::default()
                },
            };
            let context = providers[0]
                .get(&runtime, &message)
                .await
                .expect("existing provider should return context");
            assert!(context.text.contains(&target.id), "M33: learned relevant Fact must reach automatic context for {query:?}; actual={:?}", context.text);
            assert!(context.text.contains(&serde_json::to_string(FACT).unwrap()));
            assert!(context.text.chars().count() <= 4_800);
            assert!(excluded_ids.iter().all(|id| !context.text.contains(id)));
        }
        assert_eq!(memory.read().await.snapshot(), before);
    }

    #[test]
    fn unset_env_defaults_to_fastembed_and_boots_on_hashed_vectors_without_loading() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let guards = [
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE"),
        ];

        let mut runtime = MemoryEmbeddingRuntime::from_env(None, None)
            .expect("the default mode configures without downloading");
        let status = runtime.status();
        assert_eq!(status.provider, FASTEMBED_PROVIDER);
        assert_eq!(status.model, DEFAULT_FASTEMBED_MODEL);
        assert_eq!(status.load, MemoryEmbeddingLoad::Pending);
        assert_eq!(status.load.label(), "downloading");
        assert_eq!(status.dimension, DEFAULT_EMBEDDING_DIMENSION);
        runtime
            .upsert_memory(&test_memory(
                "memory-1",
                "Operator wants concise ship notes",
            ))
            .unwrap();
        assert_eq!(runtime.search("release briefing style", 1).len(), 1);
        assert!(
            runtime.take_model_loader().is_some(),
            "the model loads later, in the background"
        );
        assert!(runtime.take_model_loader().is_none());

        let local = EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS", LOCAL_PROVIDER);
        let mut runtime = MemoryEmbeddingRuntime::from_env(None, None).unwrap();
        assert_eq!(runtime.status().provider, LOCAL_PROVIDER);
        assert_eq!(runtime.status().load, MemoryEmbeddingLoad::Ready);
        assert!(runtime.take_model_loader().is_none());
        drop(local);
        let off = EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS", "off");
        let status = MemoryEmbeddingRuntime::from_env(None, None)
            .unwrap()
            .status();
        assert!(!status.enabled);
        assert_eq!(status.load, MemoryEmbeddingLoad::Off);
        drop(off);
        // An upgraded daemon with only the dimensions knob keeps booting.
        let dimensions = EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS", "256");
        let status = MemoryEmbeddingRuntime::from_env(None, None)
            .expect("only the dimensions knob still boots")
            .status();
        assert_eq!(status.provider, LOCAL_PROVIDER);
        assert_eq!(status.dimension, 256);
        drop(dimensions);
        drop(guards);
    }

    #[test]
    fn model_cache_dir_without_a_store_is_the_per_user_cache() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("owner");
        let xdg = fixture.path().join("xdg");
        assert!(xdg.is_absolute());
        let user = |macos, xdg: Option<&PathBuf>| UserCacheEnv {
            macos,
            home: Some(home.clone()),
            xdg_cache_home: xdg.cloned(),
        };
        assert_eq!(
            model_cache_dir(None, None, &user(true, Some(&xdg))),
            Some(home.join("Library/Caches/animaos/models"))
        );
        assert_eq!(
            model_cache_dir(None, None, &user(false, Some(&xdg))),
            Some(xdg.join("animaos/models"))
        );
        assert_eq!(
            model_cache_dir(None, None, &user(false, None)),
            Some(home.join(".cache/animaos/models"))
        );
        // A relative XDG_CACHE_HOME is ignored, as the XDG spec says.
        assert_eq!(
            model_cache_dir(None, None, &user(false, Some(&PathBuf::from("relative")))),
            Some(home.join(".cache/animaos/models"))
        );
        // The store-adjacent dir, then the explicit setting, win.
        let beside = PathBuf::from("data/models");
        assert_eq!(
            model_cache_dir(None, Some(beside.clone()), &user(true, None)),
            Some(beside.clone())
        );
        assert_eq!(
            model_cache_dir(Some("explicit".into()), Some(beside), &user(true, None)),
            Some(PathBuf::from("explicit"))
        );
        // Without a home directory only fastembed's default is left.
        assert_eq!(model_cache_dir(None, None, &UserCacheEnv::default()), None);
    }

    #[test]
    fn only_the_dimensions_knob_set_keeps_hashed_local_vectors() {
        assert_eq!(
            embeddings_mode(None, None),
            (FASTEMBED_PROVIDER.into(), false)
        );
        assert_eq!(
            embeddings_mode(None, Some(256)),
            (LOCAL_PROVIDER.into(), true),
            "an upgraded operator's dimension meant hashed vectors"
        );
        assert_eq!(
            embeddings_mode(Some("FastEmbed"), Some(384)),
            (FASTEMBED_PROVIDER.into(), false)
        );
        assert_eq!(
            embeddings_mode(Some("local"), Some(256)),
            (LOCAL_PROVIDER.into(), false)
        );
    }

    #[test]
    fn model_cache_dir_prefers_env_then_beside_the_memory_store_then_fastembed_default() {
        let _env_lock = ENV_LOCK.lock().expect("env lock should not poison");
        let guards = [
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_MODEL"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS"),
            EnvGuard::unset("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR"),
        ];
        let beside_store = PathBuf::from("data").join("models");

        let config = FastEmbedEmbeddingConfig::from_env(None, Some(beside_store.clone())).unwrap();
        assert_eq!(config.cache_dir, Some(beside_store.clone()));
        let config = FastEmbedEmbeddingConfig::from_env(None, None).unwrap();
        assert_eq!(
            config.cache_dir,
            model_cache_dir(None, None, &UserCacheEnv::current()),
            "the per-user cache, never the working directory"
        );
        let env = EnvGuard::set("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR", "explicit-cache");
        let config = FastEmbedEmbeddingConfig::from_env(None, Some(beside_store)).unwrap();
        assert_eq!(config.cache_dir, Some(PathBuf::from("explicit-cache")));
        drop(env);
        drop(guards);
    }

    #[tokio::test]
    async fn a_failing_loader_falls_back_and_search_keeps_working() {
        let memory = test_memory("memory-1", "Operator wants concise ship notes");
        let manager = manager_with(vec![memory.clone()]);
        let mut runtime = test_support::awaiting(test_support::failing_model_loader(
            "could not fetch https://huggingface.co/intfloat/model.onnx into /Users/someone/cache: offline\nsecond line",
        ));
        runtime.rebuild_from_memories(&[memory]).unwrap();
        let loader = runtime.take_model_loader().unwrap();
        let embeddings = Arc::new(AsyncRwLock::new(runtime));

        join(spawn_model_load(
            Arc::clone(&manager),
            Arc::clone(&embeddings),
            loader,
        ))
        .await;

        let runtime = embeddings.read().await;
        let status = runtime.status();
        let MemoryEmbeddingLoad::Fallback(reason) = &status.load else {
            panic!("a failed load falls back, got {:?}", status.load);
        };
        assert_eq!(reason, "could not fetch … into … offline");
        assert_eq!(status.load.label(), "fallback");
        assert_eq!(status.provider, test_support::FAKE_PROVIDER);
        assert_eq!(status.dimension, DEFAULT_EMBEDDING_DIMENSION);
        let hits = runtime.search("release briefing style", 1);
        assert_eq!(hits.len(), 1, "hashed vectors are kept");
        assert_eq!(hits[0].memory_id, "memory-1");
    }

    #[tokio::test]
    async fn a_slow_loader_does_not_block_boot_and_finishes_in_the_background() {
        let memory = test_memory("memory-1", "Operator wants concise ship notes");
        let manager = manager_with(vec![memory.clone()]);
        let (loader, release) = test_support::gated_model_loader(40);
        let mut runtime = test_support::awaiting(loader);
        runtime.rebuild_from_memories(&[memory]).unwrap();
        let loader = runtime.take_model_loader().unwrap();
        let embeddings = Arc::new(AsyncRwLock::new(runtime));

        let load = spawn_model_load(Arc::clone(&manager), Arc::clone(&embeddings), loader).unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!load.is_finished(), "the loader is still waiting");
        {
            let runtime = embeddings.read().await;
            assert_eq!(runtime.status().load, MemoryEmbeddingLoad::Pending);
            assert_eq!(runtime.search("release briefing style", 1).len(), 1);
        }
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), join(Ok(load)))
            .await
            .expect("the load finishes once released");

        let status = embeddings.read().await.status();
        assert_eq!(status.load, MemoryEmbeddingLoad::Ready);
        assert_eq!((status.done, status.total), (1, 1));
        assert_eq!(status.dimension, 40);
        assert_eq!(status.model, test_support::FAKE_MODEL);
    }

    #[test]
    fn during_rebuilding_hashed_vectors_are_never_returned() {
        let reembedded = test_memory("memory-new", "Operator wants concise ship notes");
        let hashed_only = test_memory("memory-old", "Operator likes short release briefs");
        let manager = MemoryManager::new();
        let mut manager = manager;
        manager.replace_snapshot(anima_memory::MemoryManagerSnapshot {
            memories: vec![reembedded.clone(), hashed_only.clone()],
            ..Default::default()
        });
        // The fake model has the hashed dimension, so only the embedder
        // identity keeps the two kinds of vectors apart.
        let (loader, _) = test_support::fake_model_loader(DEFAULT_EMBEDDING_DIMENSION);
        let mut runtime = test_support::awaiting(loader);
        runtime
            .rebuild_from_memories(&[reembedded.clone(), hashed_only.clone()])
            .unwrap();
        assert_eq!(runtime.search("release briefing style", 2).len(), 2);
        let old_query = runtime.query_embedder().unwrap();
        let old_vector = old_query.embed_query("release briefing style").unwrap();

        test_support::swap_part_way(
            &mut runtime,
            &manager,
            DEFAULT_EMBEDDING_DIMENSION,
            &["memory-new"],
        );

        assert_eq!(
            runtime.status().load,
            MemoryEmbeddingLoad::Rebuilding { done: 1, total: 2 }
        );
        assert_eq!(runtime.status().load.label(), "rebuilding");
        let ids = |hits: Vec<VectorMemoryHit>| {
            hits.into_iter()
                .map(|hit| hit.memory_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(runtime.search("release briefing style", 5)),
            ["memory-new"]
        );
        assert_eq!(
            ids(runtime
                .for_memories(&manager)
                .search("release briefing style", 5)),
            ["memory-new"]
        );
        assert!(
            runtime
                .search_precomputed_query(old_query.model_key(), &old_vector, 5, &|_| true)
                .is_empty(),
            "a query vector from the old embedder is never scored"
        );
        let new_query = runtime.query_embedder().unwrap();
        assert_ne!(new_query.model_key(), old_query.model_key());
        let new_vector = new_query.embed_query("release briefing style").unwrap();
        assert_eq!(
            ids(runtime.search_precomputed_query(new_query.model_key(), &new_vector, 5, &|_| true)),
            ["memory-new"]
        );
    }

    #[test]
    fn a_memory_changed_deleted_or_rewritten_during_a_batch_is_skipped() {
        use anima_memory::MemoryPatch;
        let memories = vec![
            test_memory("changed", "Operator wants concise ship notes"),
            test_memory("deleted", "Billing ledger exports include invoice IDs"),
            test_memory("rewritten", "Latency budget is tight"),
            test_memory("kept", "Rollback plan needs review"),
        ];
        let mut manager = MemoryManager::new();
        manager.replace_snapshot(anima_memory::MemoryManagerSnapshot {
            memories: memories.clone(),
            ..Default::default()
        });
        let (loaded, _) = test_support::fake_model(40);
        let mut runtime = MemoryEmbeddingRuntime::local_default();
        let plan = runtime.begin_rebuild(loaded, &memories, Vec::new());
        assert_eq!(plan.pending.len(), 4);

        manager
            .update_memory(
                "changed",
                MemoryPatch {
                    content: Some("Operator wants long notes".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        manager.forget("deleted");
        // A write after the swap already indexed this one with the model.
        let rewritten = test_memory("rewritten", "Latency budget is tight");
        runtime.upsert_memory(&rewritten).unwrap();
        let texts: Vec<&str> = plan.pending.iter().map(|(_, text)| text.as_str()).collect();
        let vectors = plan.embedder.embed_memory_batch(&texts);
        assert!(runtime.apply_rebuild_batch(plan.generation, &manager, &plan.pending, vectors));
        runtime.finish_rebuild(plan.generation);

        let mut indexed: Vec<_> = runtime.indexed_content.keys().cloned().collect();
        indexed.sort();
        assert_eq!(indexed, ["kept", "rewritten"]);
        assert_eq!(runtime.status().load, MemoryEmbeddingLoad::Ready);
        assert!(
            !runtime.apply_rebuild_batch(plan.generation + 1, &manager, &[], Vec::new()),
            "a stale rebuild stops"
        );
    }

    #[tokio::test]
    async fn rebuild_runs_in_batches_persists_model_vectors_and_reuses_them_after_restart() {
        let path = temp_sqlite_path("model-rebuild");
        let _ = remove_file(&path);
        let memories: Vec<_> = (0..70)
            .map(|index| {
                test_memory(
                    &format!("memory-{index:02}"),
                    &format!("ship notes {index}"),
                )
            })
            .collect();
        let manager = manager_with(memories.clone());

        let boot = |loader: MemoryModelLoader| {
            let mut runtime = test_support::awaiting(loader);
            runtime.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
            let report = runtime.rebuild_from_memories(&memories).unwrap();
            assert_eq!(
                report.loaded_vectors, 0,
                "interim vectors never load from the store"
            );
            runtime
        };
        let (loader, calls) = test_support::fake_model_loader(40);
        let mut runtime = boot(loader);
        assert!(
            runtime
                .store
                .as_ref()
                .unwrap()
                .load_all()
                .unwrap()
                .is_empty(),
            "interim hashed vectors are never saved"
        );
        let loader = runtime.take_model_loader().unwrap();
        let embeddings = Arc::new(AsyncRwLock::new(runtime));
        join(spawn_model_load(
            Arc::clone(&manager),
            Arc::clone(&embeddings),
            loader,
        ))
        .await;
        let status = embeddings.read().await.status();
        assert_eq!(status.load, MemoryEmbeddingLoad::Ready);
        assert_eq!(
            (status.done, status.total, status.vector_count),
            (70, 70, 70)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 70);
        let stored = SqliteMemoryEmbeddingStore::new(path.clone())
            .unwrap()
            .load_all()
            .unwrap();
        assert_eq!(stored.len(), 70);
        assert!(stored
            .iter()
            .all(|row| row.model == "fake:fake-model" && row.vector.len() == 40));

        // Restart: boot on hashed vectors again, without overwriting the
        // saved model vectors, then reuse them once the model is back.
        let (loader, calls) = test_support::fake_model_loader(40);
        let mut runtime = boot(loader);
        let stored = runtime.store.as_ref().unwrap().load_all().unwrap();
        assert!(stored.iter().all(|row| row.model == "fake:fake-model"));
        let loader = runtime.take_model_loader().unwrap();
        let embeddings = Arc::new(AsyncRwLock::new(runtime));
        join(spawn_model_load(
            Arc::clone(&manager),
            Arc::clone(&embeddings),
            loader,
        ))
        .await;
        let status = embeddings.read().await.status();
        assert_eq!(status.load, MemoryEmbeddingLoad::Ready);
        assert_eq!(status.vector_count, 70);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let _ = remove_file(&path);
    }

    #[tokio::test]
    async fn hashed_vectors_saved_by_local_mode_are_reembedded_by_the_model() {
        let path = temp_sqlite_path("local-to-model");
        let _ = remove_file(&path);
        let memories = vec![
            test_memory("memory-1", "Operator wants concise ship notes"),
            test_memory("memory-2", "Billing ledger exports include invoice IDs"),
        ];
        let mut local = MemoryEmbeddingRuntime::local_default();
        local.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
        local.rebuild_from_memories(&memories).unwrap();
        assert!(
            local
                .store
                .as_ref()
                .unwrap()
                .load_all()
                .unwrap()
                .iter()
                .all(|row| row.model == LOCAL_MODEL
                    && row.vector.len() == DEFAULT_EMBEDDING_DIMENSION)
        );

        let (loader, calls) = test_support::fake_model_loader(DEFAULT_EMBEDDING_DIMENSION);
        let mut runtime = test_support::awaiting(loader);
        runtime.store = Some(SqliteMemoryEmbeddingStore::new(path.clone()).unwrap());
        runtime.rebuild_from_memories(&memories).unwrap();
        let loader = runtime.take_model_loader().unwrap();
        let embeddings = Arc::new(AsyncRwLock::new(runtime));
        join(spawn_model_load(
            manager_with(memories),
            Arc::clone(&embeddings),
            loader,
        ))
        .await;

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let stored = SqliteMemoryEmbeddingStore::new(path.clone())
            .unwrap()
            .load_all()
            .unwrap();
        assert!(stored.iter().all(|row| row.model == "fake:fake-model"));
        let _ = remove_file(&path);
    }

    #[test]
    fn fallback_reason_keeps_the_first_line_without_paths_or_urls() {
        assert_eq!(
            fallback_reason("Failed to load C:\\models\\e5.onnx: bad file\nstack"),
            "Failed to load … bad file"
        );
        assert_eq!(fallback_reason("\n\n"), "the search model did not load");
        assert_eq!(
            fallback_reason(&"word ".repeat(100)).chars().count(),
            FALLBACK_REASON_CHARS
        );
    }

    /// Waits for a model load thread without blocking the test runtime.
    async fn join(load: io::Result<std::thread::JoinHandle<()>>) {
        let load = load.expect("the load thread starts");
        tokio::task::spawn_blocking(move || load.join().expect("the load thread finishes"))
            .await
            .unwrap();
    }

    fn manager_with(memories: Vec<Memory>) -> Arc<AsyncRwLock<MemoryManager>> {
        let mut manager = MemoryManager::new();
        manager.replace_snapshot(anima_memory::MemoryManagerSnapshot {
            memories,
            ..Default::default()
        });
        Arc::new(AsyncRwLock::new(manager))
    }

    fn test_memory(id: &str, content: &str) -> Memory {
        Memory {
            id: id.to_string(),
            agent_id: "agent-1".into(),
            agent_name: "Planner".into(),
            memory_type: MemoryType::Fact,
            content: content.to_string(),
            importance: 0.8,
            created_at: 1,
            tags: None,
            scope: MemoryScope::Private,
            room_id: None,
            world_id: None,
            session_id: None,
        }
    }

    fn temp_sqlite_path(label: &str) -> PathBuf {
        let suffix = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("anima-memory-embeddings-{label}-{suffix}.sqlite"))
    }

    fn spawn_embedding_server(responses: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener.local_addr().expect("listener should have addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);

        thread::spawn(move || {
            for response_body in responses {
                let (mut stream, _) = listener.accept().expect("request should arrive");
                let mut buffer = [0_u8; 8192];
                let bytes_read = stream.read(&mut buffer).expect("request should read");
                let request = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                server_requests
                    .lock()
                    .expect("requests lock should not poison")
                    .push(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("response should write");
            }
        });

        (format!("http://{address}/v1"), requests)
    }

    fn cosine(left: &[f32], right: &[f32]) -> f64 {
        let dot: f64 = left
            .iter()
            .zip(right.iter())
            .map(|(left, right)| f64::from(*left) * f64::from(*right))
            .sum();
        let left_magnitude = left
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>()
            .sqrt();
        let right_magnitude = right
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>()
            .sqrt();
        dot / (left_magnitude * right_magnitude)
    }

    struct EnvGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }

        fn unset(name: &'static str) -> Self {
            let previous = std::env::var_os(name);
            std::env::remove_var(name);
            Self { name, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.name, previous);
            } else {
                std::env::remove_var(self.name);
            }
        }
    }
}

#[cfg(test)]
mod profile_ownership_tests {
    use super::*;
    use crate::profile_owner::ProfileOwner;

    #[test]
    fn profile_owner_extracted_model_loader_retains_lease_after_runtime_is_dropped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.sqlite");
        let owner =
            ProfileOwner::acquire_resources([crate::profile_owner::ProfileResource::Sqlite(
                path.clone(),
            )])
            .unwrap();
        let mut runtime = MemoryEmbeddingRuntime::awaiting_model(
            "test",
            "test".into(),
            Box::new(|| {
                Ok(LoadedMemoryEmbedder(MemoryEmbeddingProvider::Local(
                    LocalMemoryEmbedder::new(16),
                )))
            }),
        );
        runtime.owner = Some(owner);
        let loader = runtime.take_model_loader().unwrap();
        drop(runtime);
        assert!(
            ProfileOwner::acquire([path.clone()]).is_err(),
            "the detached model loader dropped its profile lease"
        );
        loader().unwrap();
        ProfileOwner::acquire([path]).unwrap();
    }

    #[test]
    fn profile_owner_revoked_embedding_store_cannot_write_vectors() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.sqlite");
        let owner =
            ProfileOwner::acquire_resources([crate::profile_owner::ProfileResource::Sqlite(
                path.clone(),
            )])
            .unwrap();
        let store =
            SqliteMemoryEmbeddingStore::new_with_owner(path.clone(), Some(owner.clone())).unwrap();
        store
            .upsert("memory", "test", &[1.0, 2.0], &[1; 32])
            .unwrap();
        owner.mark_lost();
        assert!(store
            .upsert("memory", "test", &[3.0, 4.0], &[2; 32])
            .is_err());
        let read_only =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let saved: String = read_only
            .query_row(
                "SELECT vector_json FROM memory_embeddings WHERE memory_id = 'memory'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<f32>>(&saved).unwrap(),
            vec![1.0, 2.0]
        );
    }
}
