//! Learned local inference for derived project search, isolated from memory providers.

use anima_document_store_sqlite::{EmbeddingProfile, EMBEDDING_WINDOW_CONTRACT};
use anima_memory::MemoryVectorError;

use super::{FastEmbedMemoryEmbedder, MemoryEmbeddingProvider, MemoryEmbeddingRuntime};

/// A loaded learned-local capability that cannot select hashing or HTTP inference.
#[derive(Clone, Debug)]
pub(crate) struct LocalProjectEmbedder {
    embedder: FastEmbedMemoryEmbedder,
    generation: u64,
}

impl MemoryEmbeddingRuntime {
    pub(crate) fn local_project_embedder(&self) -> Option<LocalProjectEmbedder> {
        if !self.enabled {
            return None;
        }
        let MemoryEmbeddingProvider::FastEmbed(embedder) = &self.embedder else {
            return None;
        };
        Some(LocalProjectEmbedder {
            embedder: embedder.clone(),
            generation: self.generation,
        })
    }
}

impl LocalProjectEmbedder {
    pub(crate) fn model(&self) -> &str {
        &self.embedder.model_name
    }

    pub(crate) fn profile(
        &self,
    ) -> Result<EmbeddingProfile, anima_document_store_sqlite::StoreError> {
        EmbeddingProfile::new(
            format!(
                "project-v2:fastembed:{}:dim={}:prompt={:?}:{}:max:{}",
                self.embedder.model_name,
                self.embedder.dimension,
                self.embedder.prompt_style,
                EMBEDDING_WINDOW_CONTRACT,
                self.embedder.project_session,
            ),
            self.embedder.dimension,
        )
    }

    pub(crate) fn same_runtime(&self, other: &Self) -> bool {
        self.embedder.project_session == other.embedder.project_session
            && self.generation == other.generation
    }

    pub(crate) fn embed_query(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
        self.embedder.embed_query(text)
    }

    pub(crate) fn embed_passages(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, MemoryVectorError> {
        self.embedder
            .embed_passages(texts)
            .ok_or(MemoryVectorError::EmbeddingUnavailable)?
            .into_iter()
            .map(|vector| vector.ok_or(MemoryVectorError::EmbeddingUnavailable))
            .collect()
    }
}

/// Test-only physical acceptance never enters the download-capable model loader.
#[cfg(test)]
pub(crate) fn offline_project_runtime(
    snapshot: &std::path::Path,
) -> std::io::Result<MemoryEmbeddingRuntime> {
    use fastembed::{
        InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
    };
    let files = TokenizerFiles {
        tokenizer_file: std::fs::read(snapshot.join("tokenizer.json"))?,
        config_file: std::fs::read(snapshot.join("config.json"))?,
        special_tokens_map_file: std::fs::read(snapshot.join("special_tokens_map.json"))?,
        tokenizer_config_file: std::fs::read(snapshot.join("tokenizer_config.json"))?,
    };
    let model =
        UserDefinedEmbeddingModel::new(std::fs::read(snapshot.join("onnx/model.onnx"))?, files)
            .with_pooling(Pooling::Mean);
    let model = TextEmbedding::try_new_from_user_defined(
        model,
        InitOptionsUserDefined::new().with_max_length(512),
    )
    .map_err(super::fastembed_error)?;
    Ok(MemoryEmbeddingRuntime::from_provider(
        MemoryEmbeddingProvider::FastEmbed(FastEmbedMemoryEmbedder {
            project_session: uuid::Uuid::new_v4().to_string(),
            model_name: super::DEFAULT_FASTEMBED_MODEL.to_owned(),
            dimension: 384,
            prompt_style: super::FastEmbedPromptStyle::E5,
            model: std::sync::Arc::new(std::sync::Mutex::new(model)),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct QualityFixture {
        documents: Vec<QualityDocument>,
        meaning_cases: Vec<QualityCase>,
        keyword_cases: Vec<QualityCase>,
    }

    #[derive(serde::Deserialize)]
    struct QualityDocument {
        id: String,
        text: String,
    }

    #[derive(serde::Deserialize)]
    struct QualityCase {
        query: String,
        expected: String,
    }

    #[test]
    #[ignore = "requires an explicit cached E5 snapshot; loads only local files and never downloads"]
    fn offline_learned_project_semantic_quality() {
        use anima_document_store_sqlite::{
            source_sha256, ChunkInput, DocumentInput, DocumentStore, PassageEmbedding,
        };
        let snapshot = std::env::var("ANIMAOS_PROJECT_SEARCH_OFFLINE_MODEL_DIR")
            .expect("set ANIMAOS_PROJECT_SEARCH_OFFLINE_MODEL_DIR to a complete local E5 snapshot");
        let runtime = super::offline_project_runtime(std::path::Path::new(&snapshot)).unwrap();
        let local = runtime.local_project_embedder().unwrap();
        let profile = local.profile().unwrap();
        let fixture: QualityFixture =
            serde_json::from_str(include_str!("../projects/knowledge/semantic_quality.json"))
                .unwrap();
        let store = DocumentStore::open(":memory:").unwrap();
        let source = vec!["selected".to_owned()];
        let documents: Vec<DocumentInput> = fixture
            .documents
            .iter()
            .map(|document| DocumentInput {
                document_id: document.id.clone(),
                relative_path: format!("{}.txt", document.id),
                title: document.id.clone(),
                source_sha256: source_sha256(document.text.as_bytes()),
                chunks: vec![ChunkInput {
                    locator: "text#line=1".into(),
                    text: document.text.clone(),
                }],
            })
            .collect();
        store
            .replace_source("project", "selected", &documents)
            .unwrap();
        loop {
            let pending = store
                .pending_embeddings("project", &source, &profile, 32)
                .unwrap();
            if pending.is_empty() {
                break;
            }
            let texts: Vec<&str> = pending.iter().map(|window| window.text.as_str()).collect();
            let vectors = local.embed_passages(&texts).unwrap();
            let vectors: Vec<PassageEmbedding> = pending
                .into_iter()
                .zip(vectors)
                .map(|(window, vector)| PassageEmbedding {
                    citation_id: window.citation_id,
                    window: window.window,
                    vector,
                })
                .collect();
            assert_eq!(
                store
                    .publish_embeddings("project", &profile, &vectors)
                    .unwrap(),
                vectors.len()
            );
        }
        for case in fixture.meaning_cases.iter().chain(&fixture.keyword_cases) {
            let baseline = store.search("project", &source, &case.query, 3).unwrap();
            let vector = local.embed_query(&case.query).unwrap();
            let result = store
                .hybrid_search("project", &source, &case.query, 3, &profile, &vector)
                .unwrap();
            let ranked: Vec<&str> = result
                .hits
                .iter()
                .map(|hit| hit.document_id.as_str())
                .collect();
            let keyword: Vec<&str> = baseline
                .iter()
                .map(|hit| hit.document_id.as_str())
                .collect();
            eprintln!("offline semantic fixture {:?}: keyword={keyword:?}, hybrid={ranked:?}, expected={:?}", case.query, case.expected);
            assert_eq!(
                ranked.first().copied(),
                Some(case.expected.as_str()),
                "expected {} ranked first for {:?}",
                case.expected,
                case.query
            );
            assert_eq!(result.coverage.omitted_windows, 0);
            for hit in result.hits {
                assert!(store
                    .citation("project", &hit.citation_id)
                    .unwrap()
                    .is_some());
            }
        }
        assert!(
            store
                .search("project", &source, "How can we back out?", 20)
                .unwrap()
                .is_empty(),
            "the explicit paraphrase case must separate keyword and learned retrieval"
        );

        let mut long = documents.clone();
        long[0].chunks[0].text = format!(
            "{} {}",
            "Routine archive pagination records contain administrative filler. ".repeat(45),
            fixture.documents[0].text
        );
        long[0].source_sha256 = source_sha256(long[0].chunks[0].text.as_bytes());
        store.replace_source("project", "selected", &long).unwrap();
        loop {
            let pending = store
                .pending_embeddings("project", &source, &profile, 32)
                .unwrap();
            if pending.is_empty() {
                break;
            }
            let texts: Vec<&str> = pending.iter().map(|window| window.text.as_str()).collect();
            let vectors = local.embed_passages(&texts).unwrap();
            store
                .publish_embeddings(
                    "project",
                    &profile,
                    &pending
                        .into_iter()
                        .zip(vectors)
                        .map(|(window, vector)| PassageEmbedding {
                            citation_id: window.citation_id,
                            window: window.window,
                            vector,
                        })
                        .collect::<Vec<_>>(),
                )
                .unwrap();
        }
        let query = local.embed_query("How can we back out?").unwrap();
        let late = store
            .hybrid_search(
                "project",
                &source,
                "How can we back out?",
                3,
                &profile,
                &query,
            )
            .unwrap();
        let first = late
            .hits
            .first()
            .expect("the long passage has a meaning match");
        eprintln!(
            "offline long-prefix ranking: {:?}",
            late.hits
                .iter()
                .map(|hit| (&hit.document_id, &hit.excerpt))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first.document_id, "termination",
            "the late clause must rank first after the unrelated prefix"
        );
        assert!(first.excerpt.contains("terminate this agreement"),
            "the selected semantic window must show the late clause, not merely its unrelated prefix: {:?}", first.excerpt);
        eprintln!(
            "offline late-clause fixture: first={}, excerpt={:?}",
            first.document_id, first.excerpt
        );
        store.retain_sources("project", &[]).unwrap();
        assert!(store
            .hybrid_search(
                "project",
                &source,
                "How can we back out?",
                3,
                &profile,
                &query
            )
            .unwrap()
            .hits
            .is_empty());
    }

    #[test]
    fn hashed_and_disabled_memory_do_not_authorize_project_semantics() {
        assert!(MemoryEmbeddingRuntime::local_default()
            .local_project_embedder()
            .is_none());
        assert!(MemoryEmbeddingRuntime::disabled()
            .local_project_embedder()
            .is_none());
    }

    #[test]
    fn http_memory_configuration_does_not_authorize_project_text_transfer() {
        let remote = OpenAiCompatibleMemoryEmbedder::new(
            "openai-compatible",
            "http://127.0.0.1:9/v1",
            "unused-test-model",
            None,
            384,
            Duration::from_millis(10),
        )
        .unwrap();
        let runtime = MemoryEmbeddingRuntime::from_provider(
            MemoryEmbeddingProvider::OpenAiCompatible(remote),
        );
        assert!(runtime.local_project_embedder().is_none());
    }
}
