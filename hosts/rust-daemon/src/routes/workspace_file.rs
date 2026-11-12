//! `anima.yaml`: the workspace file. It names the workspace and lists its
//! agents as equals; there is no orchestrator or rank. The steward is the
//! workspace's own system agent and is not listed here.
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ApiError;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceFile {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mission: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) values: Option<Vec<String>>,
    /// Default model for agents that name none.
    #[serde(default)]
    pub(crate) model: String,
    /// Default provider for agents that name none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) provider: String,
    #[serde(default)]
    pub(crate) agents: Vec<WorkspaceFileAgent>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceFileAgent {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) bio: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lore: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) knowledge: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) topics: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) adjectives: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) style: Option<String>,
    pub(crate) system: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<String>>,
}

impl WorkspaceFileAgent {
    pub(crate) fn effective_provider(&self, workspace_provider: &str) -> String {
        self.provider
            .as_deref()
            .map(str::trim)
            .filter(|provider| !provider.is_empty())
            .or_else(|| {
                let provider = workspace_provider.trim();
                (!provider.is_empty()).then_some(provider)
            })
            .unwrap_or("openai")
            .to_string()
    }
}

/// Parses and checks an `anima.yaml`: a name and at least one agent, each
/// with a name and instructions.
pub(crate) fn load_workspace_file(path: &Path) -> Result<WorkspaceFile, ApiError> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        ApiError::bad_request(format!("could not read {}: {error}", path.display()))
    })?;
    let file: WorkspaceFile = serde_yaml::from_str(&raw)
        .map_err(|error| ApiError::bad_request(format!("anima.yaml is not valid: {error}")))?;
    if file.name.trim().is_empty() {
        return Err(ApiError::bad_request_static("anima.yaml: name is required"));
    }
    if file.agents.is_empty() {
        return Err(ApiError::bad_request_static(
            "anima.yaml: agents must list at least one agent",
        ));
    }
    for agent in &file.agents {
        if agent.name.trim().is_empty() {
            return Err(ApiError::bad_request_static(
                "anima.yaml: agent name is required",
            ));
        }
        if agent.system.trim().is_empty() {
            return Err(ApiError::bad_request(format!(
                "anima.yaml: agent {} system is required",
                agent.name.trim()
            )));
        }
    }
    Ok(file)
}
