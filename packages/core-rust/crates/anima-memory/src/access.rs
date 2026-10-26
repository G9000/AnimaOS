//! Host-issued exact memory grants; a workspace ID is an opaque space, never a path grant.
use anima_core::{
    IdentityBinding, KnowledgeAccessContext, MemoryPortError, MemoryPortErrorCode, MemoryScope,
    MAX_KNOWLEDGE_SCOPES,
};

/// Construct only from authenticated host membership. Intentionally not deserializable.
#[derive(Clone, Debug)]
pub struct MemoryAuthority {
    identity: IdentityBinding,
    read_scopes: Vec<MemoryScope>,
    write_scopes: Vec<MemoryScope>,
}
impl MemoryAuthority {
    pub fn trusted(
        identity: IdentityBinding,
        read_scopes: Vec<MemoryScope>,
        write_scopes: Vec<MemoryScope>,
    ) -> Result<Self, MemoryPortError> {
        if read_scopes.len() > MAX_KNOWLEDGE_SCOPES
            || write_scopes.len() > MAX_KNOWLEDGE_SCOPES
            || read_scopes.is_empty()
        {
            return Err(MemoryPortError::invalid(
                "memory authority requires 1..64 read scopes and at most 64 write scopes",
            ));
        }
        for scope in read_scopes.iter().chain(&write_scopes) {
            match scope {
                MemoryScope::Owner(id) if id.as_str() != identity.owner_id().as_str() => {
                    return Err(unauthorized())
                }
                MemoryScope::Agent(id) if id.as_str() != identity.individual_id().as_str() => {
                    return Err(unauthorized())
                }
                _ => {}
            }
        }
        if write_scopes
            .iter()
            .any(|scope| !read_scopes.contains(scope))
        {
            return Err(unauthorized());
        }
        Ok(Self {
            identity,
            read_scopes,
            write_scopes,
        })
    }
    pub fn identity(&self) -> &IdentityBinding {
        &self.identity
    }
    pub fn allows_read(&self, scope: &MemoryScope) -> bool {
        self.read_scopes.contains(scope)
    }
    pub fn allows_write(&self, scope: &MemoryScope) -> bool {
        self.write_scopes.contains(scope)
    }
    pub fn read_scopes(&self) -> &[MemoryScope] {
        &self.read_scopes
    }
    pub fn access_context(&self) -> Result<KnowledgeAccessContext, MemoryPortError> {
        KnowledgeAccessContext::trusted(self.read_scopes.clone())
    }
}
pub fn unauthorized() -> MemoryPortError {
    MemoryPortError {
        code: MemoryPortErrorCode::UnauthorizedScope,
        message: "memory scope is not authorized for this individual".into(),
    }
}
