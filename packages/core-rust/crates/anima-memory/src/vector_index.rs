use std::collections::HashMap;

use crate::memory_manager::{MemoryVectorIndex, VectorMemoryHit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryVectorError {
    EmbeddingUnavailable,
    InvalidMemoryId,
    EmptyEmbedding,
    NonFiniteEmbedding,
    ZeroMagnitudeEmbedding,
    DimensionMismatch,
}

impl MemoryVectorError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::EmbeddingUnavailable => "embedding is unavailable",
            Self::InvalidMemoryId => "memory ID must not be empty",
            Self::EmptyEmbedding => "embedding must not be empty",
            Self::NonFiniteEmbedding => "embedding values must be finite",
            Self::ZeroMagnitudeEmbedding => "embedding magnitude must be greater than zero",
            Self::DimensionMismatch => "embedding dimensions must match the vector index",
        }
    }
}

pub trait MemoryTextEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError>;
}

#[derive(Clone, Debug)]
pub struct InMemoryVectorIndex<E> {
    embedder: E,
    vectors: HashMap<String, Vec<f32>>,
    dimension: Option<usize>,
}

impl<E> InMemoryVectorIndex<E> {
    pub fn new(embedder: E) -> Self {
        Self {
            embedder,
            vectors: HashMap::new(),
            dimension: None,
        }
    }

    pub fn upsert_embedding(
        &mut self,
        memory_id: impl Into<String>,
        embedding: Vec<f32>,
    ) -> Result<(), MemoryVectorError> {
        let memory_id = memory_id.into().trim().to_string();
        if memory_id.is_empty() {
            return Err(MemoryVectorError::InvalidMemoryId);
        }

        let normalized = normalize_embedding(&embedding, self.dimension)?;
        self.dimension = Some(normalized.len());
        self.vectors.insert(memory_id, normalized);
        Ok(())
    }

    pub fn remove(&mut self, memory_id: &str) {
        self.vectors.remove(memory_id);
        if self.vectors.is_empty() {
            self.dimension = None;
        }
    }

    pub fn clear(&mut self) {
        self.vectors.clear();
        self.dimension = None;
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    pub fn dimension(&self) -> Option<usize> {
        self.dimension
    }

    pub fn search_embedding(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorMemoryHit>, MemoryVectorError> {
        self.search_embedding_filtered(embedding, limit, &|_| true)
    }

    pub fn search_embedding_filtered(
        &self,
        embedding: &[f32],
        limit: usize,
        eligible: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<VectorMemoryHit>, MemoryVectorError> {
        if limit == 0 || self.vectors.is_empty() {
            return Ok(Vec::new());
        }

        let query = normalize_embedding(embedding, self.dimension)?;
        let mut hits: Vec<_> = self
            .vectors
            .iter()
            .filter(|(memory_id, _)| eligible(memory_id))
            .filter_map(|(memory_id, vector)| {
                let score = dot(&query, vector);
                (score > 0.0).then(|| VectorMemoryHit {
                    memory_id: memory_id.clone(),
                    score,
                })
            })
            .collect();
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

impl<E: MemoryTextEmbedder> InMemoryVectorIndex<E> {
    pub fn upsert_text(
        &mut self,
        memory_id: impl Into<String>,
        text: &str,
    ) -> Result<(), MemoryVectorError> {
        let embedding = self.embedder.embed(text)?;
        self.upsert_embedding(memory_id, embedding)
    }
}

impl<E: MemoryTextEmbedder> MemoryVectorIndex for InMemoryVectorIndex<E> {
    fn search(&self, query: &str, limit: usize) -> Vec<VectorMemoryHit> {
        let Ok(embedding) = self.embedder.embed(query) else {
            return Vec::new();
        };
        self.search_embedding(&embedding, limit).unwrap_or_default()
    }

    fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        eligible: &dyn Fn(&str) -> bool,
    ) -> Vec<VectorMemoryHit> {
        if limit == 0 {
            return Vec::new();
        }
        let Ok(embedding) = self.embedder.embed(query) else {
            return Vec::new();
        };
        self.search_embedding_filtered(&embedding, limit, eligible)
            .unwrap_or_default()
    }
}

fn normalize_embedding(
    embedding: &[f32],
    expected_dimension: Option<usize>,
) -> Result<Vec<f32>, MemoryVectorError> {
    if embedding.is_empty() {
        return Err(MemoryVectorError::EmptyEmbedding);
    }
    if expected_dimension.is_some_and(|dimension| dimension != embedding.len()) {
        return Err(MemoryVectorError::DimensionMismatch);
    }

    let mut magnitude_squared = 0.0_f64;
    for value in embedding {
        if !value.is_finite() {
            return Err(MemoryVectorError::NonFiniteEmbedding);
        }
        magnitude_squared += f64::from(*value) * f64::from(*value);
    }
    if magnitude_squared < f64::EPSILON {
        return Err(MemoryVectorError::ZeroMagnitudeEmbedding);
    }

    let magnitude = magnitude_squared.sqrt();
    Ok(embedding
        .iter()
        .map(|value| (f64::from(*value) / magnitude) as f32)
        .collect())
}

fn dot(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct KeywordEmbedder;

    impl MemoryTextEmbedder for KeywordEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
            let lower = text.to_ascii_lowercase();
            Ok(vec![
                vector_bit(contains_any(
                    &lower,
                    &["brief", "summary", "release", "ship"],
                )),
                vector_bit(contains_any(&lower, &["invoice", "billing", "ledger"])),
                vector_bit(contains_any(&lower, &["latency", "performance"])),
            ])
        }
    }

    #[test]
    fn filtered_vector_search_precedes_top_k_and_preserves_scores_and_ties() {
        let mut index = InMemoryVectorIndex::new(KeywordEmbedder);
        for (id, vector) in [
            ("hidden", vec![1.0, 0.0, 0.0]),
            ("eligible-b", vec![0.5, 0.5, 0.0]),
            ("eligible-a", vec![0.5, 0.5, 0.0]),
            ("negative", vec![-1.0, 0.0, 0.0]),
        ] {
            index.upsert_embedding(id, vector).unwrap();
        }
        let query = [1.0, 0.0, 0.0];
        let ordinary = index.search_embedding(&query, 4).unwrap();
        assert_eq!(ordinary[0].memory_id, "hidden");
        let filtered = index
            .search_embedding_filtered(&query, 1, &|id| id.starts_with("eligible"))
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].memory_id, "eligible-a");
        assert_eq!(filtered[0].score, ordinary[1].score);
        assert_eq!(ordinary[1].memory_id, "eligible-a");
        assert_eq!(ordinary[2].memory_id, "eligible-b");
        assert_eq!(ordinary.len(), 3, "nonpositive cosine hits remain excluded");
        assert_eq!(
            index
                .search_embedding_filtered(&query, 4, &|_| true)
                .unwrap(),
            ordinary
        );
        assert!(index
            .search_embedding_filtered(&[], 0, &|_| panic!("zero limit must not inspect IDs"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn filtered_text_search_embeds_once_and_zero_or_failure_is_empty() {
        use std::cell::Cell;

        struct CountingEmbedder(Cell<usize>);
        impl MemoryTextEmbedder for CountingEmbedder {
            fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryVectorError> {
                self.0.set(self.0.get() + 1);
                match text {
                    "unavailable" => Err(MemoryVectorError::EmbeddingUnavailable),
                    "wrong-dimension" => Ok(vec![1.0]),
                    _ => Ok(vec![1.0, 0.0]),
                }
            }
        }
        let mut index = InMemoryVectorIndex::new(CountingEmbedder(Cell::new(0)));
        index.upsert_embedding("hidden", vec![1.0, 0.0]).unwrap();
        index.upsert_embedding("eligible", vec![0.5, 0.5]).unwrap();
        let index_port: &dyn MemoryVectorIndex = &index;
        assert!(index_port.search_filtered("query", 0, &|_| true).is_empty());
        assert_eq!(index.embedder.0.get(), 0);
        let hits = index_port.search_filtered("query", 1, &|id| id == "eligible");
        assert_eq!(hits[0].memory_id, "eligible");
        assert_eq!(index.embedder.0.get(), 1);
        assert!(index_port
            .search_filtered("unavailable", 1, &|_| true)
            .is_empty());
        assert!(index_port
            .search_filtered("wrong-dimension", 1, &|_| true)
            .is_empty());
        assert_eq!(index.embedder.0.get(), 3);
        assert_eq!(index_port.search("query", 1)[0].memory_id, "hidden");
        assert_eq!(index.embedder.0.get(), 4);
    }

    #[test]
    fn in_memory_vector_index_ranks_cosine_hits() {
        let mut index = InMemoryVectorIndex::new(KeywordEmbedder);
        index
            .upsert_embedding("release-memory", vec![1.0, 0.0, 0.0])
            .expect("release vector should insert");
        index
            .upsert_embedding("billing-memory", vec![0.0, 1.0, 0.0])
            .expect("billing vector should insert");

        let hits = index
            .search_embedding(&[0.9, 0.1, 0.0], 2)
            .expect("query should search");

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].memory_id, "release-memory");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn in_memory_vector_index_embeds_text_queries() {
        let mut index = InMemoryVectorIndex::new(KeywordEmbedder);
        index
            .upsert_text("release-memory", "concise ship summary")
            .expect("text should embed");
        index
            .upsert_text("billing-memory", "billing ledger")
            .expect("text should embed");

        let hits = index.search("release brief", 1);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_id, "release-memory");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn in_memory_vector_index_rejects_invalid_vectors() {
        let mut index = InMemoryVectorIndex::new(KeywordEmbedder);

        assert_eq!(
            index.upsert_embedding("", vec![1.0]).unwrap_err(),
            MemoryVectorError::InvalidMemoryId
        );
        assert_eq!(
            index.upsert_embedding("empty", Vec::new()).unwrap_err(),
            MemoryVectorError::EmptyEmbedding
        );
        assert_eq!(
            index.upsert_embedding("nan", vec![f32::NAN]).unwrap_err(),
            MemoryVectorError::NonFiniteEmbedding
        );
        assert_eq!(
            index.upsert_embedding("zero", vec![0.0]).unwrap_err(),
            MemoryVectorError::ZeroMagnitudeEmbedding
        );
        index
            .upsert_embedding("one", vec![1.0, 0.0])
            .expect("first vector sets dimension");
        assert_eq!(
            index.upsert_embedding("two", vec![1.0]).unwrap_err(),
            MemoryVectorError::DimensionMismatch
        );
    }

    #[test]
    fn in_memory_vector_index_remove_and_clear_update_state() {
        let mut index = InMemoryVectorIndex::new(KeywordEmbedder);
        index
            .upsert_embedding("one", vec![1.0, 0.0])
            .expect("first vector should insert");

        assert_eq!(index.len(), 1);
        assert_eq!(index.dimension(), Some(2));

        index.remove("one");
        assert!(index.is_empty());
        assert_eq!(index.dimension(), None);

        index
            .upsert_embedding("two", vec![0.0, 1.0, 0.0])
            .expect("dimension resets after empty remove");
        assert_eq!(index.dimension(), Some(3));
        index.clear();
        assert!(index.is_empty());
        assert_eq!(index.dimension(), None);
    }

    fn contains_any(value: &str, needles: &[&str]) -> bool {
        needles.iter().any(|needle| value.contains(needle))
    }

    fn vector_bit(value: bool) -> f32 {
        if value {
            1.0
        } else {
            0.0
        }
    }
}
