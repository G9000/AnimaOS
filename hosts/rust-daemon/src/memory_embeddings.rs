use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anima_memory::{
    InMemoryVectorIndex, Memory, MemoryManager, MemoryTextEmbedder, MemoryVectorError,
    MemoryVectorIndex, VectorMemoryHit,
};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use reqwest::blocking::Client as BlockingHttpClient;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock as AsyncRwLock;
use tracing::warn;

pub(crate) type SharedMemoryEmbeddings = Arc<AsyncRwLock<MemoryEmbeddingRuntime>>;

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
        let embedder = MemoryEmbeddingProvider::Local(LocalMemoryEmbedder::new(
            dimension.max(MIN_EMBEDDING_DIMENSION),
        ));
        Self {
            enabled: true,
            embedder: embedder.clone(),
            index: InMemoryVectorIndex::new(embedder),
            indexed_content: HashMap::new(),
            store: None,
        }
    }

    pub(crate) fn disabled() -> Self {
        let embedder =
            MemoryEmbeddingProvider::Local(LocalMemoryEmbedder::new(DEFAULT_EMBEDDING_DIMENSION));
        Self {
            enabled: false,
            embedder: embedder.clone(),
            index: InMemoryVectorIndex::new(embedder),
            indexed_content: HashMap::new(),
            store: None,
        }
    }

    pub(crate) fn from_env(default_sqlite_path: Option<PathBuf>) -> io::Result<Self> {
        let mode = std::env::var("ANIMAOS_RS_MEMORY_EMBEDDINGS")
            .unwrap_or_else(|_| LOCAL_PROVIDER.to_string())
            .to_ascii_lowercase();
        if matches!(mode.as_str(), "0" | "off" | "false" | "disabled") {
            return Ok(Self::disabled());
        }

        let dimension = embedding_dimension_from_env()?;
        let embedder = match mode.as_str() {
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
            FASTEMBED_PROVIDER | "local-model" | "local-neural" | "local-multilingual" => {
                MemoryEmbeddingProvider::FastEmbed(FastEmbedMemoryEmbedder::from_env(dimension)?)
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
        };

        let mut runtime = Self::from_provider(embedder);
        let storage_path =
            env_path("ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE")?.or(default_sqlite_path);
        if let Some(path) = storage_path {
            runtime.store = Some(SqliteMemoryEmbeddingStore::new(path)?);
        }
        Ok(runtime)
    }

    fn from_provider(embedder: MemoryEmbeddingProvider) -> Self {
        Self {
            enabled: true,
            embedder: embedder.clone(),
            index: InMemoryVectorIndex::new(embedder),
            indexed_content: HashMap::new(),
            store: None,
        }
    }

    pub(crate) fn status(&self) -> MemoryEmbeddingStatus {
        MemoryEmbeddingStatus {
            enabled: self.enabled,
            provider: if self.enabled {
                self.embedder.provider().to_string()
            } else {
                "disabled".to_string()
            },
            model: if self.enabled {
                self.embedder.model().to_string()
            } else {
                "none".to_string()
            },
            dimension: self
                .index
                .dimension()
                .or_else(|| self.embedder.dimension_hint())
                .unwrap_or_default(),
            vector_count: self.index.len(),
            persisted: self.store.is_some(),
            storage_file: self.store.as_ref().map(|store| store.path.clone()),
        }
    }

    pub(crate) fn query_embedder(&self) -> Option<MemoryQueryEmbedder> {
        self.enabled.then(|| MemoryQueryEmbedder {
            embedder: self.embedder.clone(),
            model_key: self.embedder.storage_model(),
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
        if !self.enabled || self.embedder.storage_model() != model_key {
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
            for stored in store.load_all().map_err(|error| error.to_string())? {
                if valid_ids.contains(&stored.memory_id)
                    && stored.model == storage_model
                    && self.embedder.accepts_dimension(stored.vector.len())
                {
                    stored_by_id.insert(stored.memory_id, (stored.vector, stored.content_hash));
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
            if let Some(store) = &self.store {
                store
                    .upsert(&memory.id, &storage_model, &vector, &digest)
                    .map_err(|error| error.to_string())?;
            }
            report.rebuilt_vectors += 1;
        }

        Ok(report)
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
        if let Some(store) = &self.store {
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
}

impl MemoryEmbeddingProvider {
    fn provider(&self) -> &str {
        match self {
            Self::Local(_) => LOCAL_PROVIDER,
            Self::FastEmbed(_) => FASTEMBED_PROVIDER,
            Self::OpenAiCompatible(embedder) => &embedder.provider,
        }
    }

    fn model(&self) -> &str {
        match self {
            Self::Local(_) => LOCAL_MODEL,
            Self::FastEmbed(embedder) => &embedder.model_name,
            Self::OpenAiCompatible(embedder) => &embedder.model,
        }
    }

    fn storage_model(&self) -> String {
        match self {
            Self::Local(_) => LOCAL_MODEL.to_string(),
            Self::FastEmbed(embedder) => format!("{FASTEMBED_PROVIDER}:{}", embedder.model_name),
            Self::OpenAiCompatible(embedder) => format!("{}:{}", embedder.provider, embedder.model),
        }
    }

    fn dimension_hint(&self) -> Option<usize> {
        match self {
            Self::Local(embedder) => Some(embedder.dimension),
            Self::FastEmbed(embedder) => Some(embedder.dimension),
            Self::OpenAiCompatible(embedder) => Some(embedder.dimension),
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
        }
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        match self {
            Self::Local(embedder) => embedder.embed(text),
            Self::FastEmbed(embedder) => embedder.embed_query(text),
            Self::OpenAiCompatible(embedder) => embedder.embed(text),
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
    fn from_env(dimension: Option<usize>) -> io::Result<Self> {
        let config = FastEmbedEmbeddingConfig::from_env(dimension)?;
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
    fn from_env(dimension: Option<usize>) -> io::Result<Self> {
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
            cache_dir: env_path("ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR")?,
            show_download_progress: bool_from_env(
                "ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS",
                true,
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

#[derive(Clone, Debug)]
struct SqliteMemoryEmbeddingStore {
    path: PathBuf,
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
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { path };
        store.ensure_schema()?;
        Ok(store)
    }

    fn connection(&self) -> io::Result<Connection> {
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
                r#"
                INSERT INTO memory_embeddings(memory_id, model, dimension, vector_json, updated_at, content_hash)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(memory_id) DO UPDATE SET
                    model = excluded.model,
                    dimension = excluded.dimension,
                    vector_json = excluded.vector_json,
                    updated_at = excluded.updated_at,
                    content_hash = excluded.content_hash
                "#,
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

        let mut runtime = MemoryEmbeddingRuntime::from_env(None)
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

        let error = match MemoryEmbeddingRuntime::from_env(None) {
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

        let error = match MemoryEmbeddingRuntime::from_env(None) {
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

        let config = FastEmbedEmbeddingConfig::from_env(None)
            .expect("default fastembed config should parse without downloading");

        assert_eq!(config.model, EmbeddingModel::MultilingualE5Small);
        assert_eq!(config.model_name, DEFAULT_FASTEMBED_MODEL);
        assert_eq!(config.dimension, 384);
        assert_eq!(config.prompt_style, FastEmbedPromptStyle::E5);
        assert!(config.show_download_progress);
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

        let config = FastEmbedEmbeddingConfig::from_env(Some(1024))
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

        let error = match FastEmbedEmbeddingConfig::from_env(Some(768)) {
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

        let embedder = FastEmbedMemoryEmbedder::from_env(None)
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
            let runtime = MemoryEmbeddingRuntime::from_env(None)
                .expect("genuine cached E5 backend should load");
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
