//! Persistence contract for host-assigned identities and optional world residents.
use anima_core::{IdentityBinding, MemoryPortError, WorldResidentBinding};
use async_trait::async_trait;

#[async_trait]
pub trait IdentityRegistry: Send + Sync {
    async fn register_identity(&self, identity: IdentityBinding) -> Result<(), MemoryPortError>;
    async fn register_resident(
        &self,
        resident: WorldResidentBinding,
    ) -> Result<(), MemoryPortError>;
    async fn resident_identity(
        &self,
        owner_id: &str,
        world_id: &str,
        resident_id: &str,
    ) -> Result<Option<IdentityBinding>, MemoryPortError>;
}
