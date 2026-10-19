mod providers;

use std::sync::Arc;

use anima_core::Provider;

use self::providers::RecentMemoriesProvider;
use crate::memory_embeddings::SharedMemoryEmbeddings;
use crate::state::SharedMemoryStore;

pub(crate) fn default_providers(
    memory: SharedMemoryStore,
    memory_embeddings: SharedMemoryEmbeddings,
) -> Vec<Arc<dyn Provider>> {
    default_providers_in_zone(memory, memory_embeddings, Default::default())
}

pub(crate) fn default_providers_in_zone(
    memory: SharedMemoryStore,
    memory_embeddings: SharedMemoryEmbeddings,
    owner_time_zone: crate::clock::OwnerTimeZone,
) -> Vec<Arc<dyn Provider>> {
    vec![
        Arc::new(RecentMemoriesProvider {
            memory,
            memory_embeddings,
        }),
        crate::clock::local_time_provider(owner_time_zone),
    ]
}
