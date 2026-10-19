mod evaluators;
mod providers;

use std::sync::Arc;

use anima_core::{Evaluator, Provider};

use self::{evaluators::ReflectionMemoryEvaluator, providers::RecentMemoriesProvider};
use crate::memory_embeddings::SharedMemoryEmbeddings;
use crate::memory_store::MemoryStoreConfig;
use crate::state::SharedMemoryStore;

pub(crate) fn default_providers(
    memory: SharedMemoryStore,
    memory_embeddings: SharedMemoryEmbeddings,
) -> Vec<Arc<dyn Provider>> {
    default_providers_with_memory_views(
        memory,
        memory_embeddings,
        Default::default(),
        Default::default(),
    )
}

pub(crate) fn default_providers_with_memory_views(
    memory: SharedMemoryStore,
    memory_embeddings: SharedMemoryEmbeddings,
    selections: crate::memory_views::SharedMemoryViewSelections,
    owner_time_zone: crate::clock::OwnerTimeZone,
) -> Vec<Arc<dyn Provider>> {
    vec![
        Arc::new(RecentMemoriesProvider {
            memory,
            memory_embeddings,
            selections,
        }),
        crate::clock::local_time_provider(owner_time_zone),
    ]
}

pub(crate) fn default_evaluators(
    memory: SharedMemoryStore,
    memory_embeddings: SharedMemoryEmbeddings,
    memory_store: Option<MemoryStoreConfig>,
) -> Vec<Arc<dyn Evaluator>> {
    vec![Arc::new(ReflectionMemoryEvaluator {
        memory,
        memory_embeddings,
        memory_store,
    })]
}
