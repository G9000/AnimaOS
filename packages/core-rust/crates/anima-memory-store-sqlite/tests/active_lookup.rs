use anima_core::*;
use anima_memory::{IdentityRegistry, MemoryAuthority};
use anima_memory_store_sqlite::SqliteMemoryStore;
use std::sync::Arc;

#[tokio::test]
async fn exact_active_lookup_enforces_both_scope_layers_lifecycle_and_durable_clock() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("active.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(10));
    let store = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let identity = IdentityBinding::new("owner", "agent").unwrap();
    let private = MemoryScope::agent("agent").unwrap();
    let shared = MemoryScope::workspace("shared").unwrap();
    let authority = MemoryAuthority::trusted(
        identity.clone(),
        vec![private.clone(), shared.clone()],
        vec![private.clone(), shared.clone()],
    )
    .unwrap();
    store.register_identity(identity).await.unwrap();
    let bound = store.bind(authority.clone()).await.unwrap();
    let access = authority.access_context().unwrap();
    for id in [
        "shared",
        "expires",
        "superseded",
        "replacement",
        "forgotten",
    ] {
        let mut write = MemoryWrite::new(
            id,
            MemoryKind::Fact,
            shared.clone(),
            "exact canonical record",
            1.0,
            MemoryProvenance::new("test", id, 10).unwrap(),
        )
        .unwrap();
        if id == "expires" {
            write = write.with_retention_deadline(20).unwrap();
        }
        bound.write(write).await.unwrap();
    }
    let exact = MemoryPort::get_active(&bound, &access, "shared")
        .await
        .unwrap();
    assert_eq!(exact.id(), "shared");
    let private_access = KnowledgeAccessContext::trusted(vec![private.clone()]).unwrap();
    assert_eq!(
        MemoryPort::get_active(&bound, &private_access, "shared")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    let narrow = MemoryAuthority::trusted(
        authority.identity().clone(),
        vec![private.clone()],
        vec![private],
    )
    .unwrap();
    let narrow = store.bind(narrow).await.unwrap();
    assert_eq!(
        MemoryPort::get_active(&narrow, &access, "shared")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    bound
        .revise(MemoryRevision::forget("forgotten", 1, "withdrawn").unwrap())
        .await
        .unwrap();
    bound
        .revise(MemoryRevision::correction("replacement", 1, "superseded", "corrected").unwrap())
        .await
        .unwrap();
    for id in ["forgotten", "superseded"] {
        assert_eq!(
            MemoryPort::get_active(&bound, &access, id)
                .await
                .unwrap_err()
                .code,
            MemoryPortErrorCode::Conflict
        );
    }
    clock.advance_ms(11).unwrap();
    assert_eq!(
        MemoryPort::get_active(&bound, &access, "expires")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Conflict
    );
    store.shutdown().await.unwrap();
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::new(1)))
        .await
        .unwrap();
    let bound = store.bind(authority).await.unwrap();
    assert_eq!(
        MemoryPort::get_active(&bound, &access, "expires")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Conflict,
        "exact lookup expiry must survive clock rollback and reopen"
    );
    store.shutdown().await.unwrap();
}

struct LegacyPort;
#[async_trait::async_trait]
impl MemoryPort for LegacyPort {
    async fn write(&self, _: MemoryWrite) -> Result<MemoryRecord, MemoryPortError> {
        Err(MemoryPortError::unsupported("legacy write"))
    }
    async fn query(
        &self,
        _: &KnowledgeAccessContext,
        _: MemoryQuery,
    ) -> Result<MemoryQueryResult, MemoryPortError> {
        Err(MemoryPortError::unsupported("legacy query"))
    }
    async fn revise(&self, _: MemoryRevision) -> Result<MemoryRecord, MemoryPortError> {
        Err(MemoryPortError::unsupported("legacy revise"))
    }
}
#[tokio::test]
async fn existing_memory_ports_compile_and_exact_lookup_defaults_to_deny() {
    let access =
        KnowledgeAccessContext::trusted(vec![MemoryScope::agent("agent").unwrap()]).unwrap();
    assert_eq!(
        LegacyPort
            .get_active(&access, "known-id")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Unsupported
    );
}
