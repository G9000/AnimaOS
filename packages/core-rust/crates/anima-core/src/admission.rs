//! Host admission metadata is separate from resumable execution checkpoints.
use crate::ExecutionStoreError;
use async_trait::async_trait;
use uuid::Uuid;

pub const MAX_HOST_RUN_ADMISSION_BYTES: usize = 256 * 1024;

/// An authenticated host persists an immutable, versioned admission before
/// dispatch. Opaque payloads contain accepted configuration, never credentials.
/// Exact duplicates replay; changing any payload byte for an existing run fails.
/// Reads must enforce owner scoping. This port grants no execution authority.
#[async_trait]
pub trait HostRunAdmissionStore: Send + Sync {
    async fn admit_host_run(
        &self,
        owner: Uuid,
        run: Uuid,
        payload: Vec<u8>,
    ) -> Result<(), ExecutionStoreError>;
    async fn load_host_run(
        &self,
        owner: Uuid,
        run: Uuid,
    ) -> Result<Option<Vec<u8>>, ExecutionStoreError>;
    /// Allocates a persisted owner-bound compatibility policy ID in an explicit
    /// non-filesystem namespace, rejecting collisions with registered legacy IDs.
    async fn reserve_application_scope(
        &self,
        owner: Uuid,
        scope: Uuid,
    ) -> Result<String, ExecutionStoreError>;
}
