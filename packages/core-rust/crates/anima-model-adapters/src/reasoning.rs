//! Conservative provider capabilities for explicit reasoning controls.
use anima_core::{AgentConfig, DataValue};

pub fn supported_reasoning_efforts(provider: &str, model: &str) -> &'static [&'static str] {
    if !matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "openai" | "chatgpt"
    ) {
        return &[];
    }
    let model = model.to_ascii_lowercase();
    // Pro variants have model-specific restrictions; leave their controls at
    // provider default instead of advertising an unsupported common choice.
    if model.contains("-pro") {
        return &[];
    }
    if model.starts_with("gpt-6.1-sol") || model.starts_with("gpt-6-astra") {
        return &["low", "medium", "high", "xhigh", "max"];
    }
    if model.starts_with("gpt-6-sol") || model.starts_with("gpt-6-luna") {
        return &["none", "low", "medium", "high", "xhigh", "max"];
    }
    if (model.starts_with("gpt-5") && !model.contains("-chat"))
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        return &["low", "medium", "high"];
    }
    &[]
}

pub(crate) fn effort(config: &AgentConfig) -> Option<&str> {
    let DataValue::String(value) = config
        .settings
        .as_ref()?
        .additional
        .get("reasoningEffort")?
    else {
        return None;
    };
    supported_reasoning_efforts(
        config.provider.as_deref().unwrap_or("openai"),
        &config.model,
    )
    .contains(&value.as_str())
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn configured(model: &str, provider: &str) -> AgentConfig {
        let mut config: AgentConfig = serde_json::from_value(
            serde_json::json!({"name":"test","model":model,"provider":provider}),
        )
        .unwrap();
        config.settings = Some(anima_core::AgentSettings {
            additional: std::collections::BTreeMap::from([(
                "reasoningEffort".into(),
                DataValue::String("high".into()),
            )]),
            ..Default::default()
        });
        config
    }
    #[test]
    fn unsupported_provider_and_effort_are_not_sent_to_fallbacks() {
        let mut config = configured("gpt-6-astra", "chatgpt");
        assert_eq!(effort(&config), Some("high"));
        config.provider = Some("anthropic".into());
        assert_eq!(effort(&config), None);
        assert!(!supported_reasoning_efforts("chatgpt", "gpt-6-astra").contains(&"none"));
        assert!(supported_reasoning_efforts("openai", "custom-unknown").is_empty());
        assert!(supported_reasoning_efforts("openai", "gpt-5-pro").is_empty());
    }

    #[test]
    fn openai_wire_effort_is_explicit_and_non_reasoning_parameters_are_removed() {
        let config = configured("gpt-5.4", "openai");
        let request = anima_core::ModelGenerateRequest {
            system: "test".into(),
            messages: vec![],
            temperature: Some(0.2),
            max_tokens: Some(128),
        };
        let body =
            crate::openai_compatible::build_openai_compatible_body(&config, &request).unwrap();
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("temperature").is_none());
    }
}
