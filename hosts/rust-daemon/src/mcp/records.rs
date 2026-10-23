use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};

use anima_core::{prepare_external_tool, DataValue, ExternalToolDescriptor, ToolDescriptor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const MAX_CONNECTIONS: usize = 4;
pub(crate) const MAX_REVIEWED_TOOLS: usize = 32;
pub(crate) const MAX_STDIO_EXECUTABLE_BYTES: usize = 2_048;
pub(crate) const MAX_STDIO_ARGS: usize = 32;
pub(crate) const MAX_STDIO_ARG_BYTES: usize = 2_048;
pub(crate) const MAX_STDIO_CONFIG_BYTES: usize = 16 * 1_024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpStdioSpec {
    pub(crate) executable: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
}

impl McpStdioSpec {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.executable.is_empty()
            || self.executable.len() > MAX_STDIO_EXECUTABLE_BYTES
            || self.args.len() > MAX_STDIO_ARGS
            || self.executable.chars().any(char::is_control)
            || self
                .args
                .iter()
                .any(|arg| arg.len() > MAX_STDIO_ARG_BYTES || arg.chars().any(char::is_control))
            || self.args.iter().map(String::len).sum::<usize>() + self.executable.len()
                > MAX_STDIO_CONFIG_BYTES
        {
            return Err("Invalid MCP stdio launch".into());
        }
        let executable = std::path::Path::new(&self.executable);
        if !executable.is_absolute() {
            return Err("MCP stdio executable must be an absolute path".into());
        }
        let name = executable
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Invalid MCP stdio executable".to_string())?
            .to_ascii_lowercase();
        let stem = name.strip_suffix(".exe").unwrap_or(&name);
        if matches!(
            stem,
            "cmd"
                | "powershell"
                | "pwsh"
                | "bash"
                | "sh"
                | "zsh"
                | "fish"
                | "wsl"
                | "npm"
                | "npx"
                | "pnpm"
                | "yarn"
                | "pip"
                | "pipx"
                | "uv"
                | "uvx"
                | "bun"
        ) {
            return Err("Shell and package-manager launchers are not accepted".into());
        }
        #[cfg(windows)]
        if !name.ends_with(".exe") {
            return Err("MCP stdio executable must be a Windows .exe".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewedMcpTool {
    pub(crate) alias: String,
    pub(crate) descriptor: ExternalToolDescriptor,
}

impl ReviewedMcpTool {
    pub(crate) fn tool_descriptor(&self) -> ToolDescriptor {
        let DataValue::Object(parameters_schema) = portable_value(&self.descriptor.input_schema)
        else {
            unreachable!("reviewed input schema is an object")
        };
        ToolDescriptor {
            name: self.alias.clone(),
            description: self.descriptor.description.clone(),
            parameters_schema,
            examples: None,
        }
    }
}

fn portable_value(value: &Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(value) => DataValue::Bool(*value),
        Value::Number(value) => DataValue::Number(value.as_f64().expect("validated finite number")),
        Value::String(value) => DataValue::String(value.clone()),
        Value::Array(values) => DataValue::Array(values.iter().map(portable_value).collect()),
        Value::Object(values) => DataValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), portable_value(value)))
                .collect(),
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpConnectionRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) label: String,
    pub(crate) endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stdio: Option<McpStdioSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) bearer_credential_ref: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) version: u64,
    pub(crate) reviewed_tools: Vec<ReviewedMcpTool>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct McpRegistry {
    pub(crate) connections: BTreeMap<String, McpConnectionRecord>,
}
impl McpRegistry {
    pub(crate) fn snapshot(&self) -> Vec<McpConnectionRecord> {
        self.connections.values().cloned().collect()
    }
    pub(crate) fn validate(rows: &[McpConnectionRecord]) -> Result<(), String> {
        if rows.len() > MAX_CONNECTIONS {
            return Err("MCP connection limit exceeded".into());
        }
        let mut ids = HashSet::new();
        let mut aliases = HashSet::new();
        for row in rows {
            if row.bearer_credential_ref.as_ref().is_some_and(|reference| {
                !super::credentials::valid_reference(reference)
                    || row.stdio.is_some()
                    || !credential_endpoint_valid(&row.endpoint)
            }) {
                return Err("Invalid MCP credential reference".into());
            }
            if uuid::Uuid::parse_str(&row.id).is_err()
                || !ids.insert(&row.id)
                || !valid_id(&row.agent_id)
                || normalized_label(&row.label).as_deref() != Ok(row.label.as_str())
                || if let Some(stdio) = &row.stdio {
                    !row.endpoint.is_empty() || stdio.validate().is_err()
                } else {
                    normalized_endpoint(&row.endpoint).as_deref() != Ok(row.endpoint.as_str())
                }
                || row.version == 0
                || row.version > 9_007_199_254_740_991
                || row.reviewed_tools.len() > MAX_REVIEWED_TOOLS
            {
                return Err("Invalid MCP connection record".into());
            }
            for tool in &row.reviewed_tools {
                let checked = prepare_external_tool(
                    &tool.descriptor.remote_name,
                    &tool.descriptor.description,
                    tool.descriptor.input_schema.clone(),
                    tool.descriptor.output_schema.clone(),
                )
                .map_err(|_| "Invalid reviewed MCP descriptor")?;
                if checked != tool.descriptor
                    || alias_for(&row.id, &checked.descriptor_digest).as_deref()
                        != Ok(tool.alias.as_str())
                    || !aliases.insert(&tool.alias)
                {
                    return Err("Invalid reviewed MCP binding".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn restored(
        rows: Vec<McpConnectionRecord>,
        live: &HashSet<String>,
    ) -> Result<Self, String> {
        Self::validate(&rows)?;
        if rows.iter().any(|row| !live.contains(&row.agent_id)) {
            return Err("MCP owner is missing or ineligible".into());
        }
        Ok(Self {
            connections: rows.into_iter().map(|row| (row.id.clone(), row)).collect(),
        })
    }
    pub(crate) fn known_descriptor(&self, agent: &str, alias: &str) -> Option<ToolDescriptor> {
        self.connections
            .values()
            .filter(|row| row.agent_id == agent)
            .flat_map(|row| &row.reviewed_tools)
            .find(|tool| tool.alias == alias)
            .map(ReviewedMcpTool::tool_descriptor)
    }
    pub(crate) fn published(&self) -> SharedMcpPublication {
        Arc::new(RwLock::new(McpPublishedCatalog::from_registry(self)))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct McpKnownBinding {
    pub(crate) connection_id: String,
    pub(crate) tool: ReviewedMcpTool,
}
#[derive(Clone, Debug)]
pub(crate) struct McpReadyBinding {
    pub(crate) connection_id: String,
    pub(crate) version: u64,
    pub(crate) generation: u64,
    pub(crate) tool: ReviewedMcpTool,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct McpPublishedCatalog {
    pub(crate) known: BTreeMap<(String, String), McpKnownBinding>,
    pub(crate) live: BTreeMap<(String, String), McpReadyBinding>,
    pub(crate) access: BTreeMap<String, McpToolAccess>,
}
impl McpPublishedCatalog {
    pub(crate) fn from_registry(registry: &McpRegistry) -> Self {
        let known = registry
            .connections
            .values()
            .flat_map(|row| {
                row.reviewed_tools.iter().map(|tool| {
                    (
                        (row.agent_id.clone(), tool.alias.clone()),
                        McpKnownBinding {
                            connection_id: row.id.clone(),
                            tool: tool.clone(),
                        },
                    )
                })
            })
            .collect();
        Self {
            known,
            live: Default::default(),
            access: Default::default(),
        }
    }
}
pub(crate) type SharedMcpPublication = Arc<RwLock<McpPublishedCatalog>>;

/// Derived live authority, never a durable tool review or grant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpToolAccess {
    Automatic,
    #[default]
    Review,
    Denied,
}

impl McpToolAccess {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Review => "review",
            Self::Denied => "denied",
        }
    }

    pub(crate) fn for_agent(state: &crate::state::DaemonState, agent: &str) -> Self {
        use crate::approvals::{autonomy::AutonomyMode, PolicyAction};
        if state
            .agents
            .get(agent)
            .is_none_or(|runtime| crate::agent_runs::is_helper_config(runtime.config()))
        {
            return Self::Denied;
        }
        match state.autonomy_of(agent).mode {
            AutonomyMode::Free => Self::Automatic,
            AutonomyMode::Strict => Self::Review,
            AutonomyMode::Custom => match state.approvals.policy(agent).exec {
                PolicyAction::Allow => Self::Automatic,
                PolicyAction::Ask => Self::Review,
                PolicyAction::Deny => Self::Denied,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpConnectionState {
    Disconnected,
    Connecting,
    NeedsReview,
    Ready,
    Degraded,
    Stopping,
}
impl McpConnectionState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::NeedsReview => "needs_review",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Stopping => "stopping",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StaticMcpError {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}
#[derive(Clone, Debug)]
pub(crate) struct McpConnectionView {
    pub(crate) record: McpConnectionRecord,
    pub(crate) tool_access: McpToolAccess,
    pub(crate) state: McpConnectionState,
    pub(crate) wire_generation: u64,
    pub(crate) discovery_id: Option<String>,
    pub(crate) candidates: Vec<ExternalToolDescriptor>,
    pub(crate) physical_calls_active: bool,
    pub(crate) error: Option<StaticMcpError>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateMcpConnection {
    pub(crate) label: String,
    pub(crate) endpoint: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateMcpConnection {
    pub(crate) expected_version: u64,
    #[serde(default, deserialize_with = "non_null")]
    pub(crate) label: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    pub(crate) endpoint: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    pub(crate) enabled: Option<bool>,
}
fn non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewMcpTool {
    pub(crate) expected_version: u64,
    pub(crate) discovery_id: String,
    pub(crate) descriptor_digest: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpGrantInput {
    pub(crate) expected_version: u64,
    pub(crate) descriptor_digest: String,
    pub(crate) granted: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct McpGrantOutcome {
    pub(crate) connection: McpConnectionView,
    pub(crate) granted: bool,
    pub(crate) tool_names: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum McpServiceError {
    InvalidRequest,
    NotFound,
    Conflict,
    Busy,
    Unavailable,
    Revoked,
    Cancelled,
    TimedOut,
    OutcomeUnknown,
}
impl McpServiceError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Busy => "busy",
            Self::Unavailable => "unavailable",
            Self::Revoked => "revoked",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }
    pub(crate) fn diagnostic(self) -> StaticMcpError {
        StaticMcpError {
            code: self.code(),
            message: self.message(),
        }
    }
    fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "MCP request is invalid or exceeds its bounds",
            Self::NotFound => "MCP connection not found",
            Self::Conflict => "MCP configuration or discovery changed; refresh before retrying",
            Self::Busy => "MCP work limit reached or connection is retiring",
            Self::Unavailable => "MCP service or durable storage unavailable",
            Self::Revoked => "MCP permission or connection was revoked",
            Self::Cancelled => "MCP call cancelled before dispatch",
            Self::TimedOut => "MCP admission deadline reached",
            Self::OutcomeUnknown => "MCP call outcome is unknown; remote effects may have occurred",
        }
    }
}
impl std::fmt::Display for McpServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for McpServiceError {}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && !value.chars().any(char::is_control)
        && !crate::memory_text::has_hidden_text(value)
}
pub(crate) fn normalized_label(label: &str) -> Result<String, McpServiceError> {
    let label = label.trim();
    if !valid_id(label) || label.len() > 128 {
        return Err(McpServiceError::InvalidRequest);
    }
    Ok(label.into())
}
pub(crate) fn normalized_endpoint(endpoint: &str) -> Result<String, McpServiceError> {
    if endpoint.len() > 2048 || !valid_url_text(endpoint) {
        return Err(McpServiceError::InvalidRequest);
    }
    let url = reqwest::Url::parse(endpoint).map_err(|_| McpServiceError::InvalidRequest)?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(McpServiceError::InvalidRequest);
    }
    if url.scheme() == "http" {
        let authority = endpoint
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .ok_or(McpServiceError::InvalidRequest)?;
        let literal = authority == "127.0.0.1"
            || authority == "[::1]"
            || authority == "localhost"
            || authority
                .strip_prefix("127.0.0.1:")
                .or_else(|| authority.strip_prefix("[::1]:"))
                .or_else(|| authority.strip_prefix("localhost:"))
                .is_some_and(|port| port.parse::<u16>().is_ok());
        if !literal {
            return Err(McpServiceError::InvalidRequest);
        }
    } else if url.scheme() != "https" {
        return Err(McpServiceError::InvalidRequest);
    }
    Ok(url.to_string())
}
/// Bearer credentials are accepted only for canonical HTTPS or local loopback HTTP.
pub(crate) fn credential_endpoint_valid(endpoint: &str) -> bool {
    normalized_endpoint(endpoint).is_ok_and(|value| value == endpoint)
}
fn valid_url_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && !value.chars().any(char::is_control)
        && !crate::memory_text::has_hidden_text(value)
}
pub(crate) fn alias_for(connection: &str, digest: &str) -> Result<String, McpServiceError> {
    let uuid = uuid::Uuid::parse_str(connection).map_err(|_| McpServiceError::InvalidRequest)?;
    let hex = digest
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or(McpServiceError::InvalidRequest)?;
    Ok(format!("mcp_{}_{}", uuid.simple(), &hex[..16]))
}
