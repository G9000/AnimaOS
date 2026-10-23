use std::collections::{BTreeMap, HashMap};

use anima_core::{
    AgentConfig, AgentConfigUpdate, AgentRuntimeSnapshot, AgentSettings, AgentState, DataValue,
    Message, MessageRole, ModelChoice, PluginDescriptor, ToolDescriptor, ToolExample,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use super::shared::{
    data_value_to_json, json_to_data_value, number_value, parse_usize, required_string, u32_value,
    u64_value, usize_value, ContentResponse, TaskResultResponse, TokenUsageResponse,
};

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolExampleResponse {
    pub(crate) input: String,
    pub(crate) args: BTreeMap<String, Value>,
    pub(crate) output: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolDescriptorResponse {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: BTreeMap<String, Value>,
    pub(crate) examples: Option<Vec<ToolExampleResponse>>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginDescriptorResponse {
    pub(crate) name: String,
    pub(crate) description: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSettingsResponse {
    pub(crate) temperature: Option<f64>,
    #[serde(rename = "maxTokens")]
    pub(crate) max_tokens: Option<u32>,
    pub(crate) timeout_ms: Option<u64>,
    #[serde(rename = "maxRetries")]
    pub(crate) max_retries: Option<u32>,
    #[serde(rename = "maxToolIterations")]
    pub(crate) max_tool_iterations: Option<usize>,
    pub(crate) execution_limits: Option<ExecutionLimitsSettings>,
    pub(crate) additional: BTreeMap<String, Value>,
}

/// A provider + model the agent falls back to, in order.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelChoiceResponse {
    pub(crate) provider: String,
    pub(crate) model: String,
}

impl From<&ModelChoice> for ModelChoiceResponse {
    fn from(value: &ModelChoice) -> Self {
        Self {
            provider: value.provider.clone(),
            model: value.model.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentConfigResponse {
    pub(crate) name: String,
    pub(crate) model: String,
    pub(crate) bio: Option<String>,
    pub(crate) lore: Option<String>,
    pub(crate) knowledge: Option<Vec<String>>,
    pub(crate) topics: Option<Vec<String>>,
    pub(crate) adjectives: Option<Vec<String>>,
    pub(crate) style: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) system: Option<String>,
    pub(crate) tools: Option<Vec<ToolDescriptorResponse>>,
    pub(crate) plugins: Option<Vec<PluginDescriptorResponse>>,
    pub(crate) settings: Option<AgentSettingsResponse>,
    /// Tried in order when the agent's own model fails before any text.
    pub(crate) fallbacks: Vec<ModelChoiceResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentStateResponse {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) config: AgentConfigResponse,
    pub(crate) created_at_ms: u64,
    pub(crate) token_usage: TokenUsageResponse,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRuntimeSnapshotResponse {
    pub(crate) state: AgentStateResponse,
    pub(crate) message_count: usize,
    pub(crate) messages: Vec<AgentMessageResponse>,
    pub(crate) event_count: usize,
    pub(crate) last_task: Option<TaskResultResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentMessageResponse {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) room_id: String,
    pub(crate) content: ContentResponse,
    pub(crate) role: String,
    pub(crate) created_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct AgentEnvelope {
    pub(crate) agent: AgentRuntimeSnapshotResponse,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct AgentsEnvelope {
    pub(crate) agents: Vec<AgentRuntimeSnapshotResponse>,
}

/// An agent without its transcript (`GET /api/agents?view=summary`).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSummaryResponse {
    pub(crate) state: AgentStateResponse,
    pub(crate) message_count: usize,
    pub(crate) event_count: usize,
    pub(crate) last_task: Option<TaskResultResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct AgentSummariesEnvelope {
    pub(crate) agents: Vec<AgentSummaryResponse>,
}

impl From<&AgentRuntimeSnapshot> for AgentSummaryResponse {
    fn from(value: &AgentRuntimeSnapshot) -> Self {
        Self {
            state: AgentStateResponse::from(&value.state),
            message_count: value.message_count,
            event_count: value.event_count,
            last_task: value.last_task.as_ref().map(TaskResultResponse::from),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct AgentRunEnvelope {
    pub(crate) agent: AgentRuntimeSnapshotResponse,
    pub(crate) result: TaskResultResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolExampleRequestObject {
    pub(crate) input: Option<String>,
    #[serde(default)]
    pub(crate) args: BTreeMap<String, Value>,
    pub(crate) output: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolDescriptorRequestObject {
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) parameters: BTreeMap<String, Value>,
    pub(crate) examples: Option<Vec<ToolExampleRequestObject>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum ToolDescriptorRequest {
    Name(String),
    Detailed(ToolDescriptorRequestObject),
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginDescriptorRequestObject {
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum PluginDescriptorRequest {
    Name(String),
    Detailed(PluginDescriptorRequestObject),
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSettingsRequest {
    pub(crate) temperature: Option<Value>,
    #[serde(rename = "maxTokens")]
    pub(crate) max_tokens: Option<Value>,
    #[serde(alias = "timeout")]
    pub(crate) timeout_ms: Option<Value>,
    #[serde(rename = "maxRetries")]
    pub(crate) max_retries: Option<Value>,
    #[serde(rename = "maxToolIterations")]
    pub(crate) max_tool_iterations: Option<Value>,
    pub(crate) execution_limits: Option<ExecutionLimitsSettings>,
    #[serde(flatten)]
    pub(crate) additional: HashMap<String, Value>,
}

/// Owner limits only tighten the finite runtime ceiling. Omitted fields use
/// the documented default ceiling; changing settings never replenishes usage.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecutionLimitsSettings {
    pub(crate) max_wall_time_ms: Option<u64>,
    pub(crate) max_input_tokens: Option<u64>,
    pub(crate) max_output_tokens: Option<u64>,
    pub(crate) max_total_tokens: Option<u64>,
    pub(crate) max_model_attempts: Option<u64>,
    pub(crate) max_tool_calls: Option<u64>,
    pub(crate) max_retries: Option<u64>,
    pub(crate) max_inflight_operations: Option<u64>,
}

impl ExecutionLimitsSettings {
    fn into_domain(self) -> Result<anima_core::execution_limits::ExecutionLimits, &'static str> {
        let mut limits = anima_core::execution_limits::ExecutionLimits::default();
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = self.$field {
                    limits.$field = value;
                }
            };
        }
        apply!(max_model_attempts);
        apply!(max_tool_calls);
        apply!(max_inflight_operations);
        limits.max_retries = self
            .max_retries
            .unwrap_or(limits.max_retries.min(limits.max_model_attempts));
        macro_rules! apply_budget {
            ($field:ident) => {
                if let Some(value) = self.$field {
                    limits.budget.$field = Some(value);
                }
            };
        }
        apply_budget!(max_wall_time_ms);
        apply_budget!(max_input_tokens);
        apply_budget!(max_output_tokens);
        apply_budget!(max_total_tokens);
        limits.validate().map_err(|_| {
            "executionLimits must contain finite positive bounds and coherent attempt counts"
        })?;
        Ok(limits)
    }
}

impl From<&anima_core::execution_limits::ExecutionLimits> for ExecutionLimitsSettings {
    fn from(limits: &anima_core::execution_limits::ExecutionLimits) -> Self {
        Self {
            max_wall_time_ms: limits.budget.max_wall_time_ms,
            max_input_tokens: limits.budget.max_input_tokens,
            max_output_tokens: limits.budget.max_output_tokens,
            max_total_tokens: limits.budget.max_total_tokens,
            max_model_attempts: Some(limits.max_model_attempts),
            max_tool_calls: Some(limits.max_tool_calls),
            max_retries: Some(limits.max_retries),
            max_inflight_operations: Some(limits.max_inflight_operations),
        }
    }
}

/// A fallback provider + model. The route checks the chain as a whole.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelChoiceRequest {
    pub(crate) provider: String,
    pub(crate) model: String,
}

impl ModelChoiceRequest {
    fn into_domain(self) -> ModelChoice {
        ModelChoice {
            provider: self.provider.trim().to_ascii_lowercase(),
            model: self.model.trim().to_string(),
        }
    }
}

fn parse_fallbacks(requests: Option<Vec<ModelChoiceRequest>>) -> Option<Vec<ModelChoice>> {
    requests.map(|requests| {
        requests
            .into_iter()
            .map(ModelChoiceRequest::into_domain)
            .collect()
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentConfigRequest {
    pub(crate) name: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) bio: Option<String>,
    pub(crate) lore: Option<String>,
    pub(crate) knowledge: Option<Vec<String>>,
    pub(crate) topics: Option<Vec<String>>,
    pub(crate) adjectives: Option<Vec<String>>,
    pub(crate) style: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) system: Option<String>,
    pub(crate) tools: Option<Vec<ToolDescriptorRequest>>,
    pub(crate) plugins: Option<Vec<PluginDescriptorRequest>>,
    pub(crate) settings: Option<AgentSettingsRequest>,
    /// At most 3, none equal to the agent's own provider + model.
    pub(crate) fallbacks: Option<Vec<ModelChoiceRequest>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentUpdateRequest {
    pub(crate) name: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) system: Option<String>,
    pub(crate) tools: Option<Vec<ToolDescriptorRequest>>,
    /// Only execution limits are supported as an in-place settings update.
    pub(crate) settings: Option<AgentSettingsRequest>,
    /// Replaces the fallback chain; `[]` clears it.
    pub(crate) fallbacks: Option<Vec<ModelChoiceRequest>>,
}

impl AgentUpdateRequest {
    pub(crate) fn into_domain(self) -> Result<AgentConfigUpdate, &'static str> {
        let name = self
            .name
            .map(|value| required_string(Some(value), "name must not be empty"))
            .transpose()?;
        let model = self
            .model
            .map(|value| required_string(Some(value), "model must not be empty"))
            .transpose()?;
        // Empty string clears provider/system back to the daemon default.
        let provider = self.provider.map(|value| value.trim().to_string());
        let system = self.system.map(|value| value.trim().to_string());
        let tools = parse_tool_requests(self.tools)?;
        let fallbacks = parse_fallbacks(self.fallbacks);
        let execution_limits = self
            .settings
            .map(|settings| {
                if settings.temperature.is_some()
                    || settings.max_tokens.is_some()
                    || settings.timeout_ms.is_some()
                    || settings.max_retries.is_some()
                    || settings.max_tool_iterations.is_some()
                    || !settings.additional.is_empty()
                {
                    return Err("PATCH settings supports executionLimits only");
                }
                settings
                    .execution_limits
                    .map(ExecutionLimitsSettings::into_domain)
                    .transpose()
            })
            .transpose()?
            .flatten();
        if name.is_none()
            && model.is_none()
            && provider.is_none()
            && system.is_none()
            && tools.is_none()
            && fallbacks.is_none()
            && execution_limits.is_none()
        {
            return Err("at least one field must be provided");
        }
        Ok(AgentConfigUpdate {
            name,
            model,
            provider,
            system,
            tools,
            fallbacks,
            execution_limits,
        })
    }
}

#[derive(Clone, Debug, Deserialize, IntoParams, ToSchema, Default)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRecentMemoriesQuery {
    pub(crate) limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceIdentityRequest {
    pub(crate) company_name: String,
    pub(crate) mission: String,
    #[serde(default)]
    pub(crate) values: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateProfileRequest {
    pub(crate) preset_id: String,
    pub(crate) intent: String,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) workspace: WorkspaceIdentityRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentProfileResponse {
    pub(crate) bio: String,
    pub(crate) adjectives: Vec<String>,
    pub(crate) style: String,
    pub(crate) system: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentProfileEnvelope {
    pub(crate) profile: AgentProfileResponse,
}

impl AgentConfigRequest {
    pub(crate) fn into_domain(self) -> Result<AgentConfig, &'static str> {
        Ok(AgentConfig {
            name: required_string(self.name, "name is required")?,
            model: required_string(self.model, "model is required")?,
            bio: self.bio,
            lore: self.lore,
            knowledge: self.knowledge,
            topics: self.topics,
            adjectives: self.adjectives,
            style: self.style,
            provider: self.provider,
            system: self.system,
            tools: parse_tool_requests(self.tools)?,
            plugins: self
                .plugins
                .map(|plugins| {
                    plugins
                        .into_iter()
                        .map(PluginDescriptorRequest::into_domain)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            settings: self
                .settings
                .map(AgentSettingsRequest::into_domain)
                .transpose()?,
            fallbacks: parse_fallbacks(self.fallbacks).unwrap_or_default(),
        })
    }
}

impl ToolDescriptorRequest {
    fn into_domain(self) -> Result<ToolDescriptor, &'static str> {
        let name = match self {
            Self::Name(name) => name,
            Self::Detailed(value) => value.name.unwrap_or_default(),
        };
        let name = required_string(Some(name.trim().to_string()), "tool name is required")?;
        Ok(ToolDescriptor {
            name,
            description: String::new(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        })
    }
}

fn parse_tool_requests(
    tools: Option<Vec<ToolDescriptorRequest>>,
) -> Result<Option<Vec<ToolDescriptor>>, &'static str> {
    tools
        .map(|tools| {
            tools
                .into_iter()
                .map(ToolDescriptorRequest::into_domain)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
}

impl PluginDescriptorRequest {
    fn into_domain(self) -> Result<PluginDescriptor, &'static str> {
        match self {
            Self::Name(name) if !name.is_empty() => Ok(PluginDescriptor {
                name,
                description: String::new(),
            }),
            Self::Name(_) => Err("plugins must contain strings or objects"),
            Self::Detailed(value) => Ok(PluginDescriptor {
                name: required_string(value.name, "plugin name is required")?,
                description: value.description,
            }),
        }
    }
}

impl AgentSettingsRequest {
    fn into_domain(mut self) -> Result<AgentSettings, &'static str> {
        let mut settings = AgentSettings::default();
        settings.execution_limits = self
            .execution_limits
            .take()
            .map(ExecutionLimitsSettings::into_domain)
            .transpose()?;
        if self
            .additional
            .get("toolDiscovery")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err("toolDiscovery must be a boolean");
        }
        if self
            .additional
            .get("agentFilesProfileEnabled")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err("agentFilesProfileEnabled must be a boolean");
        }

        settings.temperature = self
            .temperature
            .take()
            .map(|value| number_value(value, "temperature"))
            .transpose()?;
        settings.max_tokens = self
            .max_tokens
            .take()
            .map(|value| u32_value(value, "maxTokens"))
            .transpose()?;
        settings.timeout_ms = self
            .timeout_ms
            .take()
            .map(|value| u64_value(value, "timeoutMs"))
            .transpose()?;
        settings.max_retries = self
            .max_retries
            .take()
            .map(|value| u32_value(value, "maxRetries"))
            .transpose()?;
        settings.max_tool_iterations = self
            .max_tool_iterations
            .take()
            .map(|value| usize_value(value, "maxToolIterations"))
            .transpose()?;

        settings.additional = self
            .additional
            .into_iter()
            .map(|(key, value)| {
                Ok::<(String, DataValue), &'static str>((key, json_to_data_value(value)?))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        Ok(settings)
    }
}

impl AgentRecentMemoriesQuery {
    pub(crate) fn from_query_map(query: &HashMap<String, String>) -> Result<Self, &'static str> {
        Ok(Self {
            limit: query
                .get("limit")
                .map(String::as_str)
                .map(parse_usize)
                .transpose()?,
        })
    }
}

impl From<&ToolExample> for ToolExampleResponse {
    fn from(value: &ToolExample) -> Self {
        Self {
            input: value.input.clone(),
            args: value
                .args
                .iter()
                .map(|(key, value)| (key.clone(), data_value_to_json(value)))
                .collect(),
            output: value.output.clone(),
        }
    }
}

impl From<&ToolDescriptor> for ToolDescriptorResponse {
    fn from(value: &ToolDescriptor) -> Self {
        Self {
            name: value.name.clone(),
            description: value.description.clone(),
            parameters: value
                .parameters_schema
                .iter()
                .map(|(key, value)| (key.clone(), data_value_to_json(value)))
                .collect(),
            examples: value
                .examples
                .as_ref()
                .map(|examples| examples.iter().map(ToolExampleResponse::from).collect()),
        }
    }
}

impl From<&PluginDescriptor> for PluginDescriptorResponse {
    fn from(value: &PluginDescriptor) -> Self {
        Self {
            name: value.name.clone(),
            description: value.description.clone(),
        }
    }
}

impl From<&AgentSettings> for AgentSettingsResponse {
    fn from(value: &AgentSettings) -> Self {
        Self {
            temperature: value.temperature,
            max_tokens: value.max_tokens,
            timeout_ms: value.timeout_ms,
            max_retries: value.max_retries,
            max_tool_iterations: value.max_tool_iterations,
            execution_limits: value
                .execution_limits
                .as_ref()
                .map(ExecutionLimitsSettings::from),
            additional: value
                .additional
                .iter()
                .map(|(key, value)| (key.clone(), data_value_to_json(value)))
                .collect(),
        }
    }
}

impl From<&AgentConfig> for AgentConfigResponse {
    fn from(value: &AgentConfig) -> Self {
        Self {
            name: value.name.clone(),
            model: value.model.clone(),
            bio: value.bio.clone(),
            lore: value.lore.clone(),
            knowledge: value.knowledge.clone(),
            topics: value.topics.clone(),
            adjectives: value.adjectives.clone(),
            style: value.style.clone(),
            provider: value.provider.clone(),
            system: value.system.clone(),
            tools: value
                .tools
                .as_ref()
                .map(|tools| tools.iter().map(ToolDescriptorResponse::from).collect()),
            plugins: value
                .plugins
                .as_ref()
                .map(|plugins| plugins.iter().map(PluginDescriptorResponse::from).collect()),
            settings: value.settings.as_ref().map(AgentSettingsResponse::from),
            fallbacks: value
                .fallbacks
                .iter()
                .map(ModelChoiceResponse::from)
                .collect(),
        }
    }
}

impl From<&AgentState> for AgentStateResponse {
    fn from(value: &AgentState) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            status: value.status.as_str().to_string(),
            config: AgentConfigResponse::from(&value.config),
            created_at_ms: value.created_at_ms,
            token_usage: TokenUsageResponse::from(&value.token_usage),
        }
    }
}

impl From<&AgentRuntimeSnapshot> for AgentRuntimeSnapshotResponse {
    fn from(value: &AgentRuntimeSnapshot) -> Self {
        Self {
            state: AgentStateResponse::from(&value.state),
            message_count: value.message_count,
            messages: value
                .messages
                .iter()
                .map(AgentMessageResponse::from)
                .collect(),
            event_count: value.event_count,
            last_task: value.last_task.as_ref().map(TaskResultResponse::from),
        }
    }
}

impl From<&Message> for AgentMessageResponse {
    fn from(value: &Message) -> Self {
        Self {
            id: value.id.clone(),
            agent_id: value.agent_id.clone(),
            room_id: value.room_id.clone(),
            content: ContentResponse::from(&value.content),
            role: message_role_to_str(value.role).to_string(),
            created_at_ms: value.created_at_ms,
        }
    }
}

fn message_role_to_str(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
    }
}

#[cfg(test)]
mod execution_limit_tests {
    use super::*;

    #[test]
    fn owner_execution_limits_are_typed_finite_and_returned_by_the_owner_contract() {
        let request: AgentSettingsRequest = serde_json::from_value(serde_json::json!({
            "executionLimits": { "maxModelAttempts": 1, "maxRetries": 0 }
        }))
        .unwrap();
        let settings = request.into_domain().unwrap();
        let domain = serde_json::to_value(&settings).unwrap();
        assert_eq!(domain["execution_limits"]["maxModelAttempts"], 1);
        assert!(domain["additional"].get("executionLimits").is_none());
        let response = serde_json::to_value(AgentSettingsResponse::from(&settings)).unwrap();
        assert_eq!(response["executionLimits"]["maxModelAttempts"], 1);
        assert!(
            response["executionLimits"]["maxWallTimeMs"]
                .as_u64()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn owner_execution_limits_reject_unbounded_or_incoherent_authority() {
        for limits in [
            serde_json::json!({ "maxModelAttempts": 0 }),
            serde_json::json!({ "maxModelAttempts": 1, "maxRetries": 2 }),
            serde_json::json!({ "maxInflightOperations": 257 }),
            serde_json::json!({ "maxTotalTokens": 0 }),
        ] {
            let request: AgentSettingsRequest = serde_json::from_value(serde_json::json!({
                "executionLimits": limits
            }))
            .unwrap();
            assert!(request.into_domain().is_err());
        }
    }

    #[test]
    fn owner_execution_limits_can_be_tightened_through_native_patch() {
        let request: AgentUpdateRequest = serde_json::from_value(serde_json::json!({
            "settings": { "executionLimits": { "maxModelAttempts": 1, "maxRetries": 0 } }
        }))
        .unwrap();
        let update = request.into_domain();
        assert!(
            update.is_ok(),
            "owner PATCH must expose the typed execution limits"
        );
        let update = serde_json::to_value(update.unwrap()).unwrap();
        assert_eq!(update["execution_limits"]["maxModelAttempts"], 1);
    }
}
