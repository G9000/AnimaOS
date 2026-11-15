//! Owner-only configuration; reads metadata and never calls a remote service.
use super::{
    jobs::{authorize, body, no_store},
    json_response, ApiError, AppState,
};
use crate::{
    agent_files::runtime::config_revision,
    memory_provider::binding::{self as provider, MemoryProviderBinding},
    workspace_context::{binding, WorkspaceContextBinding, SETTING},
};
use anima_core::DataValue;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub(super) struct ContextConnection {
    id: String,
    label: String,
    state: String,
    tool: String,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceContextView {
    revision: String,
    binding: Option<WorkspaceContextBinding>,
    provider_binding: Option<MemoryProviderBinding>,
    status: String,
    connections: Vec<ContextConnection>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SaveWorkspaceContext {
    expected_revision: String,
    #[serde(deserialize_with = "required_binding")]
    binding: Option<WorkspaceContextBinding>,
    /// Omitted preserves the current provider; explicit null disables it.
    #[serde(default, deserialize_with = "present_provider_binding")]
    #[schema(value_type = Option<MemoryProviderBinding>, nullable)]
    provider_binding: Option<Option<MemoryProviderBinding>>,
}

fn present_provider_binding<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<MemoryProviderBinding>>, D::Error> {
    Option::deserialize(deserializer).map(Some)
}

fn required_binding<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<WorkspaceContextBinding>, D::Error> {
    Option::deserialize(deserializer)
}

async fn view(state: &AppState, id: &str) -> Result<WorkspaceContextView, ApiError> {
    let (config, rows, manager) = {
        let guard = state.daemon.read().await;
        guard
            .ensure_profile_owned()
            .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
        let config = guard
            .get_agent(id)
            .ok_or_else(ApiError::not_found)?
            .state
            .config;
        let rows = guard
            .mcp_registry
            .connections
            .values()
            .filter(|row| row.agent_id == id)
            .cloned()
            .collect::<Vec<_>>();
        (config, rows, guard.mcp_manager.clone())
    };
    let mut connections = Vec::new();
    for row in rows {
        let state = match &manager {
            Some(manager) => manager
                .get(id, &row.id)
                .await
                .ok()
                .map(|v| v.state.as_str().to_string())
                .unwrap_or_else(|| "disconnected".into()),
            None => "disconnected".into(),
        };
        for tool in &row.reviewed_tools {
            connections.push(ContextConnection {
                id: row.id.clone(),
                label: row.label.clone(),
                state: state.clone(),
                tool: tool.descriptor.remote_name.clone(),
            });
        }
    }
    Ok(WorkspaceContextView {
        revision: config_revision(&config),
        binding: binding(&config),
        provider_binding: provider::binding(&config).ok().flatten(),
        status: if crate::workspace_context::configured(&config) || provider::configured(&config) {
            "configured"
        } else {
            "independent"
        }
        .into(),
        connections,
    })
}

#[utoipa::path(get,path="/api/agents/{agent_id}/workspace-context",tag="agents",params(("agent_id"=String,Path)),responses((status=200,description="Owner workspace context binding",body=WorkspaceContextView),(status=403,description="Owner required")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    answer(view(&state, &id).await)
}
#[utoipa::path(put,path="/api/agents/{agent_id}/workspace-context",tag="agents",params(("agent_id"=String,Path)),request_body=SaveWorkspaceContext,responses((status=200,description="Durably saved binding",body=WorkspaceContextView),(status=400,description="Invalid binding"),(status=409,description="Stale revision")))]
pub(super) async fn put(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: SaveWorkspaceContext = match body(&state, request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    answer(
        tokio::spawn(async move { save(&state, &id, input).await })
            .await
            .unwrap_or_else(|_| {
                Err(ApiError::service_unavailable(
                    "Workspace context save failed",
                ))
            }),
    )
}
fn portable(binding: &WorkspaceContextBinding) -> DataValue {
    let text = serde_json::to_value(binding).expect("binding serialization");
    DataValue::Object(
        text.as_object()
            .expect("binding object")
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    DataValue::String(value.as_str().expect("binding string").into()),
                )
            })
            .collect(),
    )
}
async fn save(
    state: &AppState,
    id: &str,
    input: SaveWorkspaceContext,
) -> Result<WorkspaceContextView, ApiError> {
    let _transaction = state.agent_runs.control_plane_transaction().await;
    let mut guard = state.daemon.write().await;
    guard
        .ensure_profile_owned()
        .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
    if let Some(owner) = guard.profile_owner() {
        owner
            .verify_owned()
            .await
            .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
    }
    let before = guard
        .get_agent(id)
        .ok_or_else(ApiError::not_found)?
        .state
        .config;
    if input.expected_revision != config_revision(&before) {
        return Err(ApiError::conflict(
            "Agent configuration changed; reload before saving",
        ));
    }
    let provider_present = match &input.provider_binding {
        Some(value) => value.is_some(),
        None => provider::configured(&before),
    };
    if input.binding.is_some() && provider_present {
        return Err(ApiError::bad_request(
            "Legacy workspace context and additive memory cannot be enabled together",
        ));
    }
    if let Some(Some(provider)) = &input.provider_binding {
        provider.validate(id).map_err(ApiError::bad_request)?;
        if crate::agent_runs::is_helper_config(&before) {
            return Err(ApiError::bad_request(
                "Helpers cannot bind a memory provider",
            ));
        }
        let connection = guard
            .mcp_registry
            .connections
            .get(&provider.connection_id)
            .filter(|row| row.agent_id == id && row.enabled)
            .ok_or_else(|| {
                ApiError::bad_request("Select this agent's enabled reviewed MCP connection")
            })?;
        let tool = connection
            .reviewed_tools
            .iter()
            .find(|tool| tool.descriptor.remote_name == provider::CONTEXT_READER)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "M.O.T.H.E.R context reader must already be reviewed on this connection",
                )
            })?;
        let call = anima_core::ToolCall {
            id: "memory-provider-binding".into(),
            name: tool.alias.clone(),
            args: Default::default(),
        };
        if crate::approvals::evaluate(&guard.approvals.policy(id), &[], &[], &call)
            == crate::approvals::Verdict::Deny
        {
            return Err(ApiError::bad_request(
                "The selected context reader is denied by agent policy",
            ));
        }
    }
    if let Some(binding) = &input.binding {
        binding.validate(id).map_err(ApiError::bad_request)?;
        if crate::agent_runs::is_helper_config(&before) {
            return Err(ApiError::bad_request(
                "Helpers cannot bind workspace identity or context",
            ));
        }
        let connection = guard
            .mcp_registry
            .connections
            .get(&binding.connection_id)
            .filter(|row| row.agent_id == id)
            .ok_or_else(|| ApiError::bad_request("Select this agent's reviewed MCP connection"))?;
        if !connection
            .reviewed_tools
            .iter()
            .any(|tool| tool.descriptor.remote_name == binding.tool)
        {
            return Err(ApiError::bad_request(
                "Workspace context tool must already be reviewed on this connection",
            ));
        }
    }
    let mut config = before.clone();
    let settings = config.settings.get_or_insert_with(Default::default);
    match input.binding {
        Some(binding) => {
            settings
                .additional
                .insert(SETTING.into(), portable(&binding));
        }
        None => {
            settings.additional.remove(SETTING);
        }
    }
    match input.provider_binding {
        Some(Some(binding)) => {
            settings
                .additional
                .insert(provider::SETTING.into(), binding.portable());
        }
        Some(None) => {
            settings.additional.remove(provider::SETTING);
        }
        None => {}
    }
    guard.restore_agent_config(id, config);
    let persist = guard.control_plane_persist_request();
    let Some(store) = persist.store_config() else {
        guard.restore_agent_config(id, before);
        return Err(ApiError::service_unavailable(
            "Workspace context requires durable control-plane storage",
        ));
    };
    if let Err(error) = persist.save().await {
        if crate::control_plane_store::is_durability_uncertain(&error) {
            let observed = crate::control_plane_store::load_control_plane_snapshot(&store)
                .await
                .ok()
                .flatten()
                .and_then(|snapshot| {
                    snapshot
                        .agents
                        .into_iter()
                        .find(|agent| agent.state.id == id)
                        .map(|agent| agent.state.config)
                });
            if let Some(observed) = observed {
                guard.restore_agent_config(id, observed);
            } else if let Some(owner) = guard.profile_owner() {
                owner.mark_lost();
            }
            return Err(ApiError::service_unavailable(
                "Save acknowledgement is uncertain; reload the stored binding before retrying",
            ));
        }
        guard.restore_agent_config(id, before);
        return Err(ApiError::service_unavailable(error.to_string()));
    }
    drop(guard);
    view(state, id).await
}
fn answer(result: Result<WorkspaceContextView, ApiError>) -> Response {
    no_store(match result {
        Ok(value) => json_response(StatusCode::OK, &value),
        Err(error) => error.into_response(),
    })
}
