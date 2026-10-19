//! Committing a core-memory change: persist the store, re-index the touched
//! entry and rewrite the agent's file copy, all under the memory write lock so
//! two changes cannot land their vectors or copies out of order. The agent's
//! `core_memory` tool and conversation reviews (apply and Undo) share it.

use std::path::PathBuf;

use anima_memory::MemoryManager;
use tracing::warn;

use crate::memory_embeddings::SharedMemoryEmbeddings;
use crate::memory_store::{MemoryMutation, MemoryStoreConfig};
use crate::state::SharedMemoryStore;

/// The stores a committed core change touches.
pub(crate) struct CoreStores {
    pub(crate) memory: SharedMemoryStore,
    pub(crate) embeddings: SharedMemoryEmbeddings,
    pub(crate) store: Option<MemoryStoreConfig>,
    /// The workspace holding the file copy; `None` skips the copy.
    pub(crate) files_root: Option<PathBuf>,
}

/// The vector to update after a change.
pub(crate) enum Reindex {
    /// Nothing changed: nothing is persisted, indexed or copied.
    Unchanged,
    /// The entry was added or its text changed.
    Upsert(String),
    /// The entry left the store.
    Remove(String),
}

#[derive(Debug)]
pub(crate) enum CommitError {
    /// A core-memory rule refused the change; the store is unchanged.
    Refused(String),
    /// The store could not be saved; the change was rolled back.
    Persist(std::io::Error),
}

/// Runs `change` on the store and commits what it reports: persist, re-index,
/// then rewrite `agent_id`'s file copy. A refusal or a failed save leaves the
/// store as it was.
pub(crate) async fn commit<T>(
    stores: &CoreStores,
    agent_id: &str,
    change: impl FnOnce(&mut MemoryManager) -> Result<(T, Reindex), String>,
) -> Result<T, CommitError> {
    let mut guard = stores.memory.write().await;
    let (value, reindex) = {
        let mut mutation = MemoryMutation::new(&mut guard);
        let (value, reindex) = change(&mut mutation).map_err(CommitError::Refused)?;
        // An unchanged outcome is dropped un-persisted; the mutation's
        // restore puts back the identical state.
        if !matches!(reindex, Reindex::Unchanged) {
            mutation
                .persist(stores.store.as_ref())
                .await
                .map_err(CommitError::Persist)?;
        }
        (value, reindex)
    };
    // Vectors remain best effort, in the memory -> embeddings lock order
    // recall uses.
    match &reindex {
        Reindex::Unchanged => return Ok(value),
        Reindex::Upsert(id) => {
            if let Some(current) = guard.get(id) {
                if let Err(error) = stores.embeddings.write().await.upsert_memory(&current) {
                    warn!(memory_id = %id, %error, "failed to index a core memory entry");
                }
            }
        }
        Reindex::Remove(id) => {
            if guard.get(id).is_none() {
                if let Err(error) = stores
                    .embeddings
                    .write()
                    .await
                    .remove_memories(std::slice::from_ref(id))
                {
                    warn!(memory_id = %id, %error, "failed to remove a core memory embedding");
                }
            }
        }
    }
    super::files::write_copies(stores.files_root.as_deref(), &guard, agent_id);
    Ok(value)
}

/// What an applied edit changed, for `commit`.
pub(crate) fn reindex_for(change: &super::CoreChange) -> Reindex {
    match change {
        super::CoreChange::Added(memory)
        | super::CoreChange::Replaced {
            current: memory, ..
        } => Reindex::Upsert(memory.id.clone()),
        super::CoreChange::Removed(memory) => Reindex::Remove(memory.id.clone()),
        super::CoreChange::AlreadyPresent(_) => Reindex::Unchanged,
    }
}
