use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpToolDescriptorResponse {
    pub(crate) remote_name: String,
    pub(crate) description: String,
    #[schema(value_type = Object)]
    pub(crate) input_schema: Value,
    #[schema(value_type = Option<Object>, nullable)]
    pub(crate) output_schema: Option<Value>,
    pub(crate) descriptor_digest: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewedMcpToolResponse {
    pub(crate) alias: String,
    pub(crate) remote_name: String,
    pub(crate) description: String,
    #[schema(value_type = Object)]
    pub(crate) input_schema: Value,
    #[schema(value_type = Option<Object>, nullable)]
    pub(crate) output_schema: Option<Value>,
    pub(crate) descriptor_digest: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpConnectionResponse {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) label: String,
    pub(crate) endpoint: String,
    pub(crate) transport: String,
    #[schema(value_type = Option<McpStdioResponse>, nullable)]
    pub(crate) stdio: Option<McpStdioResponse>,
    pub(crate) enabled: bool,
    /// A saved reference exists; this does not probe the vault or remote service.
    pub(crate) has_bearer_credential: bool,
    #[serde(rename = "hasOAuthCredential")]
    pub(crate) has_oauth_credential: bool,
    pub(crate) version: u64,
    pub(crate) reviewed_tools: Vec<ReviewedMcpToolResponse>,
    /// Derived from the current agent permissions: automatic, review, or denied.
    pub(crate) tool_access: String,
    pub(crate) state: String,
    pub(crate) wire_generation: u64,
    pub(crate) discovery_id: Option<String>,
    pub(crate) candidates: Vec<McpToolDescriptorResponse>,
    pub(crate) physical_calls_active: bool,
    /// Static, redacted state only; never includes endpoint, request, or server details.
    pub(crate) error: Option<McpStaticErrorResponse>,
}

/// Redacted connection state or operation error; never contains request or endpoint details.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpStaticErrorResponse {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct McpConnectionEnvelope {
    pub(crate) connection: McpConnectionResponse,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct McpConnectionsEnvelope {
    pub(crate) connections: Vec<McpConnectionResponse>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateMcpConnectionRequest {
    pub(crate) label: String,
    #[serde(default, deserialize_with = "non_null")]
    #[schema(value_type = Option<String>, nullable)]
    pub(crate) endpoint: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    pub(crate) stdio: Option<McpStdioRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpStdioRequest {
    pub(crate) executable: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpStdioResponse {
    pub(crate) executable: String,
    pub(crate) args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateMcpConnectionRequest {
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

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpVersionRequest {
    pub(crate) expected_version: u64,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpOAuthStartRequest {
    pub(crate) expected_version: u64,
    pub(crate) client_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpOAuthCancelRequest {
    pub(crate) expected_version: u64,
    pub(crate) attempt_id: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpCredentialRequest {
    pub(crate) expected_version: u64,
    #[schema(value_type = String, min_length = 1, max_length = 4096, write_only)]
    pub(crate) bearer_token: crate::mcp::credentials::McpBearerToken,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewMcpToolRequest {
    pub(crate) expected_version: u64,
    pub(crate) discovery_id: String,
    pub(crate) descriptor_digest: String,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpGrantRequest {
    pub(crate) expected_version: u64,
    pub(crate) descriptor_digest: String,
    pub(crate) granted: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpGrantEnvelope {
    pub(crate) connection: McpConnectionResponse,
    pub(crate) granted: bool,
    pub(crate) tool_names: Vec<String>,
}
