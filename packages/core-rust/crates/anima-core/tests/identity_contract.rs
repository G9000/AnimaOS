use anima_core::{IdentityBinding, WorldResidentBinding};

#[test]
fn identity_is_independent_of_session_definition_and_world() {
    let identity = IdentityBinding::new("owner-a", "individual-a").unwrap();
    let wire = serde_json::to_value(&identity).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"owner_id":"owner-a","individual_id":"individual-a"})
    );
    let restored: IdentityBinding = serde_json::from_value(wire).unwrap();
    assert_eq!(identity, restored);
    assert!(serde_json::from_str::<IdentityBinding>(
        r#"{"owner_id":"owner-a","individual_id":" "}"#
    )
    .is_err());
    let resident = WorldResidentBinding::new(identity.clone(), "world-a", "tavi").unwrap();
    assert_eq!(resident.identity(), &identity);
    assert_eq!(resident.world_id(), "world-a");
    assert!(WorldResidentBinding::new(identity, "", "tavi").is_err());
}
