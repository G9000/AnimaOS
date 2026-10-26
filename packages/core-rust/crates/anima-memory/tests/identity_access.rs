use anima_core::{IdentityBinding, MemoryScope};
use anima_memory::MemoryAuthority;

#[test]
fn host_bound_authority_rejects_foreign_owner_and_individual_scopes() {
    let identity = IdentityBinding::new("owner-a", "tavi").unwrap();
    let own = MemoryScope::agent("tavi").unwrap();
    let shared = MemoryScope::workspace("world:a").unwrap();
    let grant = MemoryAuthority::trusted(
        identity.clone(),
        vec![own.clone(), shared.clone()],
        vec![own.clone()],
    )
    .unwrap();
    assert!(grant.allows_read(&shared));
    assert!(!grant.allows_write(&shared));
    assert!(!grant.allows_read(&MemoryScope::workspace("world:ab").unwrap()));
    assert!(MemoryAuthority::trusted(
        identity.clone(),
        vec![MemoryScope::owner("owner-b").unwrap()],
        vec![]
    )
    .is_err());
    assert!(
        MemoryAuthority::trusted(identity, vec![MemoryScope::agent("mira").unwrap()], vec![])
            .is_err()
    );
    assert_eq!(grant.identity().individual_id().as_str(), "tavi");
}
