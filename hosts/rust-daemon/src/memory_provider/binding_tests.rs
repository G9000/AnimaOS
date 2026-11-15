use super::*;
use serde_json::json;

fn value() -> serde_json::Value {
    json!({"version":1,"provider":"mother","connectionId":"connection","workspaceId":"workspace",
        "generation":"generation","subject":"agent","publication":"selected","personaSource":"local","accessMode":"companion"})
}

fn config() -> AgentConfig {
    AgentConfig {
        name: "Synthetic".into(),
        model: "synthetic".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: None,
        system: None,
        tools: None,
        plugins: None,
        settings: None,
        fallbacks: vec![],
    }
}

#[test]
fn memory_provider_binding_round_trips_portable_numbers_without_enabling_primary_memory() {
    let provider: MemoryProviderBinding = serde_json::from_value(value()).unwrap();
    assert!(provider.validate("agent").is_ok());
    assert_eq!(provider.as_workspace_binding().tool, "mother_get_context");
    assert_eq!(provider.as_workspace_binding().mode, ContextMode::Companion);
    let mut config = config();
    assert_eq!(binding(&config), Ok(None));
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(SETTING.into(), provider.portable());
    assert!(configured(&config));
    assert_eq!(binding(&config), Ok(Some(provider)));
    assert!(!crate::workspace_context::configured(&config));
    assert!(!crate::workspace_context::primary_memory(&config));
}

#[test]
fn memory_provider_binding_rejects_invalid_versions_unknown_fields_and_shared_persona() {
    for version in [json!(0), json!(2), json!(1.5), json!("1")] {
        let mut malformed = value();
        malformed["version"] = version;
        assert!(serde_json::from_value::<MemoryProviderBinding>(malformed).is_err());
    }
    let mut malformed = value();
    malformed["token"] = json!("not-a-real-token");
    assert!(serde_json::from_value::<MemoryProviderBinding>(malformed).is_err());
    let mut shared = value();
    shared["accessMode"] = json!("shared");
    shared["personaSource"] = json!("workspace");
    let provider: MemoryProviderBinding = serde_json::from_value(shared).unwrap();
    assert!(provider.validate("agent").is_err());
    assert!(provider.validate("other").is_err());
}

#[test]
fn memory_provider_binding_invalid_or_conflicting_saved_settings_fail_closed() {
    let mut config = config();
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(SETTING.into(), DataValue::Null);
    assert!(configured(&config));
    assert!(binding(&config).is_err());
    let provider: MemoryProviderBinding = serde_json::from_value(value()).unwrap();
    let settings = config.settings.as_mut().unwrap();
    settings
        .additional
        .insert(SETTING.into(), provider.portable());
    settings.additional.insert(
        crate::workspace_context::SETTING.into(),
        DataValue::Object(std::collections::BTreeMap::from([(
            "mode".into(),
            DataValue::String("companion".into()),
        )])),
    );
    assert!(binding(&config).is_err());
    assert!(
        crate::workspace_context::primary_memory(&config),
        "legacy decoding stays unchanged"
    );
}
