//! Owner-pinned additive memory settings, independent of legacy primary-memory mode.
use anima_core::{AgentConfig, DataValue};
use serde::{Deserialize, Serialize};

use crate::workspace_context::{ContextMode, WorkspaceContextBinding};

pub(crate) const SETTING: &str = "memoryProvider";
pub(crate) const CONTEXT_READER: &str = "mother_get_context";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Provider {
    Mother,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PublicationPolicy {
    Selected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PersonaSource {
    Local,
    Workspace,
}

/// Selects one reviewed connection without storing credentials or changing native memory.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryProviderBinding {
    #[serde(deserialize_with = "version_one")]
    pub(crate) version: u8,
    pub(crate) provider: Provider,
    pub(crate) connection_id: String,
    pub(crate) workspace_id: String,
    pub(crate) generation: String,
    pub(crate) subject: String,
    pub(crate) publication: PublicationPolicy,
    pub(crate) persona_source: PersonaSource,
    pub(crate) access_mode: ContextMode,
}

fn version_one<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    // Portable agent settings represent all numbers as f64, including version 1.
    if f64::deserialize(deserializer)? == 1.0 {
        Ok(1)
    } else {
        Err(serde::de::Error::custom(
            "Unsupported memory provider version",
        ))
    }
}

impl MemoryProviderBinding {
    pub(crate) fn validate(&self, agent_id: &str) -> Result<(), &'static str> {
        if self.version != 1 {
            return Err("Unsupported memory provider version");
        }
        self.as_workspace_binding().validate(agent_id)?;
        if self.persona_source == PersonaSource::Workspace
            && self.access_mode != ContextMode::Companion
        {
            return Err(
                "Workspace personality requires companion access on the selected workspace",
            );
        }
        Ok(())
    }

    /// Pins the existing reviewed reader and authority checks without adopting its memory policy.
    pub(crate) fn as_workspace_binding(&self) -> WorkspaceContextBinding {
        WorkspaceContextBinding {
            connection_id: self.connection_id.clone(),
            tool: CONTEXT_READER.into(),
            mode: self.access_mode,
            workspace_id: self.workspace_id.clone(),
            generation: self.generation.clone(),
            subject: self.subject.clone(),
        }
    }

    pub(crate) fn portable(&self) -> DataValue {
        let fields = [
            ("provider", "mother"),
            ("connectionId", self.connection_id.as_str()),
            ("workspaceId", self.workspace_id.as_str()),
            ("generation", self.generation.as_str()),
            ("subject", self.subject.as_str()),
            ("publication", "selected"),
            (
                "personaSource",
                match self.persona_source {
                    PersonaSource::Local => "local",
                    PersonaSource::Workspace => "workspace",
                },
            ),
            (
                "accessMode",
                match self.access_mode {
                    ContextMode::Companion => "companion",
                    ContextMode::Shared => "shared",
                },
            ),
        ];
        DataValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), DataValue::String(value.into())))
                .chain(std::iter::once((
                    "version".into(),
                    DataValue::Number(f64::from(self.version)),
                )))
                .collect(),
        )
    }
}

/// Includes malformed settings so callers cannot accidentally enable another provider as fallback.
pub(crate) fn configured(config: &AgentConfig) -> bool {
    config
        .settings
        .as_ref()
        .is_some_and(|settings| settings.additional.contains_key(SETTING))
}

/// Strict decoding rejects ambiguous legacy/additive settings; absence remains native-only.
pub(crate) fn binding(config: &AgentConfig) -> Result<Option<MemoryProviderBinding>, &'static str> {
    let Some(value) = config
        .settings
        .as_ref()
        .and_then(|settings| settings.additional.get(SETTING))
    else {
        return Ok(None);
    };
    if crate::workspace_context::configured(config) {
        return Err("Legacy workspace context and additive memory cannot be enabled together");
    }
    let binding: MemoryProviderBinding =
        serde_json::from_value(crate::routes::data_value_to_json(value))
            .map_err(|_| "Invalid memory provider binding")?;
    binding.validate(&binding.subject)?;
    Ok(Some(binding))
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;
