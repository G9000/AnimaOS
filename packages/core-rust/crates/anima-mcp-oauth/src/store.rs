use async_trait::async_trait;
use rmcp::transport::auth::{
    AuthError, CredentialStore, StoredCredentials, VendorExtraTokenFields,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub(super) struct MemoryCredentials(Arc<Mutex<Option<StoredCredentials>>>);
impl MemoryCredentials {
    pub(super) fn with_credentials(credentials: StoredCredentials) -> Self {
        Self(Arc::new(Mutex::new(Some(credentials))))
    }
    pub(super) async fn snapshot(&self) -> Option<StoredCredentials> {
        self.0.lock().await.clone()
    }
}
#[async_trait]
impl CredentialStore for MemoryCredentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        Ok(self.snapshot().await)
    }
    async fn save(&self, mut credentials: StoredCredentials) -> Result<(), AuthError> {
        // ID tokens/vendor fields are not needed for MCP and must not enlarge
        // the vault payload or retain unrelated account data.
        if let Some(token) = &mut credentials.token_response {
            token.set_extra_fields(VendorExtraTokenFields::default());
        }
        *self.0.lock().await = Some(credentials);
        Ok(())
    }
    async fn clear(&self) -> Result<(), AuthError> {
        *self.0.lock().await = None;
        Ok(())
    }
}
