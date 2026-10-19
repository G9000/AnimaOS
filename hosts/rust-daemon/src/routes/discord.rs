//! Owner-only native Discord routes; authorize before body/state/network access.
use super::{contracts::*, http::json_response, AppState};
use crate::connectors::discord::client::DiscordTransportError;
use crate::connectors::discord::credentials::DiscordBotToken;
use crate::connectors::discord::{
    DiscordConnectorRecord, DiscordDeliveryState, DiscordManagerError, DiscordRuntimeStatus,
    DiscordSnowflake,
};
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use serde::de::DeserializeOwned;
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    no_store(json_response(
        status,
        &ConnectorErrorBody {
            code: code.into(),
            error: message.into(),
        },
    ))
}
pub(super) fn manager_error(e: DiscordManagerError) -> Response {
    let (status, code) = match e {
        DiscordManagerError::NotFound => (StatusCode::NOT_FOUND, "connector_not_found"),
        DiscordManagerError::Busy => (StatusCode::CONFLICT, "agent_busy"),
        DiscordManagerError::InvalidInput | DiscordManagerError::UnsupportedInput => {
            (StatusCode::BAD_REQUEST, "invalid_request")
        }
        DiscordManagerError::Conflict => (StatusCode::CONFLICT, "connector_conflict"),
        DiscordManagerError::DuplicateChannel => {
            (StatusCode::CONFLICT, "discord_channel_already_connected")
        }
        DiscordManagerError::UnknownDelivery => {
            (StatusCode::CONFLICT, "discord_delivery_uncertain")
        }
        DiscordManagerError::InvalidToken => {
            (StatusCode::UNPROCESSABLE_ENTITY, "connector_token_invalid")
        }
        DiscordManagerError::Provider(error) => match error {
            DiscordTransportError::PermissionDenied => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "discord_permission_denied",
            ),
            DiscordTransportError::HttpStatus { status: 404 } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "discord_channel_not_found",
            ),
            DiscordTransportError::RateLimited { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "discord_rate_limited")
            }
            DiscordTransportError::InvalidResponse | DiscordTransportError::ResponseTooLarge => {
                (StatusCode::BAD_GATEWAY, "discord_invalid_response")
            }
            DiscordTransportError::Transport => {
                (StatusCode::SERVICE_UNAVAILABLE, "discord_network_error")
            }
            _ => (StatusCode::BAD_GATEWAY, "discord_provider_unavailable"),
        },
        _ => (StatusCode::SERVICE_UNAVAILABLE, "connector_unavailable"),
    };
    error(status, code, &e.to_string())
}
fn authorize(state: &AppState, request: &Request, read: bool) -> Result<(), Response> {
    let result = if read {
        state.local_owner.authorize_read(request.headers())
    } else {
        state.local_owner.authorize(request.headers())
    };
    result.map_err(|_| {
        error(
            StatusCode::FORBIDDEN,
            "connector_local_admin_required",
            "local owner authorization required",
        )
    })
}
async fn body<T: DeserializeOwned>(state: &AppState, request: Request) -> Result<T, Response> {
    let bytes = super::http::read_limited_body(request, state.config.max_request_bytes.min(8192))
        .await
        .map_err(no_store)?;
    serde_json::from_slice(&bytes).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid Discord request body",
        )
    })
}
async fn response(state: &AppState, c: &DiscordConnectorRecord) -> DiscordConnectorResponse {
    let status = state.discord.status(&c.id).await;
    let warning = state.discord.warning(&c.id).await;
    let daemon = state.daemon.read().await;
    let pending_inbound = daemon
        .discord_inbound
        .values()
        .filter(|r| r.connector_id == c.id && !r.processing_state.is_terminal())
        .count();
    let mut outbound = daemon
        .discord_outbound
        .values()
        .filter(|r| r.connector_id == c.id)
        .collect::<Vec<_>>();
    outbound.sort_by_key(|r| (r.created_at_ms, &r.id));
    let pending_outbound = outbound
        .iter()
        .filter(|r| !r.delivery_state.is_settled())
        .count();
    let unknown_outbound = outbound
        .iter()
        .filter(|r| r.delivery_state == DiscordDeliveryState::Unknown)
        .count();
    let mut delivered_message_ids = outbound
        .iter()
        .flat_map(|r| r.chunks.iter())
        .filter_map(|c| c.provider_message_id.clone())
        .collect::<Vec<_>>();
    if delivered_message_ids.len() > 20 {
        delivered_message_ids.drain(..delivered_message_ids.len() - 20);
    }
    DiscordConnectorResponse {
        id: c.id.clone(),
        agent_id: c.agent_id.clone(),
        kind: "discord".into(),
        room_id: c.room_id.clone(),
        bot: DiscordBotResponse {
            id: c.bot.id.clone(),
            username: c.bot.username.clone(),
            display_name: c.bot.display_name.clone(),
        },
        channel: DiscordChannelResponse {
            id: c.channel.id.clone(),
            guild_id: c.channel.guild_id.clone(),
            name: c.channel.name.clone(),
            kind: "guildText".into(),
        },
        approved_user_ids: c.approved_user_ids.clone(),
        enabled: c.enabled,
        status: match status {
            DiscordRuntimeStatus::Disabled => "disabled",
            DiscordRuntimeStatus::Ready => "ready",
            DiscordRuntimeStatus::Connecting => "connecting",
            DiscordRuntimeStatus::Reconnecting => "reconnecting",
            DiscordRuntimeStatus::CredentialRequired => "credentialRequired",
            DiscordRuntimeStatus::Error => "error",
            DiscordRuntimeStatus::Degraded => "degraded",
            DiscordRuntimeStatus::Reconciling => "reconciling",
        }
        .into(),
        created_at_ms: c.created_at_ms,
        updated_at_ms: c.updated_at_ms,
        pending_inbound,
        pending_outbound,
        unknown_outbound,
        delivered_message_ids,
        warning: warning.map(str::to_string),
    }
}
async fn envelope(
    state: &AppState,
    result: Result<DiscordConnectorRecord, DiscordManagerError>,
    status: StatusCode,
) -> Response {
    match result {
        Ok(c) => no_store(json_response(
            status,
            &DiscordConnectorEnvelope {
                connector: response(state, &c).await,
            },
        )),
        Err(e) => manager_error(e),
    }
}
#[utoipa::path(get,path="/api/agents/{agent_id}/connectors/discord",tag="connectors",params(("agent_id"=String,Path)),responses((status=200,body=DiscordConnectorsEnvelope),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, true) {
        return e;
    }
    let records = {
        let daemon = state.daemon.read().await;
        if daemon.get_agent(&agent).is_none() {
            return manager_error(DiscordManagerError::NotFound);
        }
        let mut records = daemon
            .discord_connectors
            .values()
            .filter(|c| c.agent_id == agent && c.deleted_at_ms.is_none())
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by_key(|c| (c.created_at_ms, c.id.clone()));
        records
    };
    let mut connectors = Vec::new();
    for record in records {
        connectors.push(response(&state, &record).await);
    }
    no_store(json_response(
        StatusCode::OK,
        &DiscordConnectorsEnvelope { connectors },
    ))
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/discord",tag="connectors",params(("agent_id"=String,Path)),request_body=DiscordCreateRequest,responses((status=201,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn create(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    let input: DiscordCreateRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !input.private_channel_confirmed
        || DiscordSnowflake::parse(&input.guild_id).is_none()
        || DiscordSnowflake::parse(&input.channel_id).is_none()
        || input.approved_user_ids.is_empty()
        || input.approved_user_ids.len() > 16
        || input
            .approved_user_ids
            .iter()
            .any(|u| DiscordSnowflake::parse(u).is_none())
    {
        return manager_error(DiscordManagerError::InvalidInput);
    }
    let token = match DiscordBotToken::parse(input.bot_token) {
        Ok(t) => t,
        Err(_) => return manager_error(DiscordManagerError::InvalidInput),
    };
    envelope(
        &state,
        state
            .discord
            .create(
                agent,
                token,
                input.guild_id,
                input.channel_id,
                input.approved_user_ids,
            )
            .await,
        StatusCode::CREATED,
    )
    .await
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/discord/{connector_id}/activate",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),responses((status=200,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn activate(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    if let Err(e) = state.discord.get(&agent, &id).await {
        return manager_error(e);
    }
    envelope(
        &state,
        state.discord.activate(agent, id).await,
        StatusCode::OK,
    )
    .await
}
#[utoipa::path(put,path="/api/agents/{agent_id}/connectors/discord/{connector_id}/credential",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),request_body=DiscordCredentialRequest,responses((status=200,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn replace(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    if let Err(e) = state.discord.get(&agent, &id).await {
        return manager_error(e);
    }
    let input: DiscordCredentialRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let token = match DiscordBotToken::parse(input.bot_token) {
        Ok(t) => t,
        Err(_) => return manager_error(DiscordManagerError::InvalidInput),
    };
    envelope(
        &state,
        state.discord.replace_token(agent, id, token).await,
        StatusCode::OK,
    )
    .await
}
#[utoipa::path(patch,path="/api/agents/{agent_id}/connectors/discord/{connector_id}",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),request_body=DiscordEnabledRequest,responses((status=200,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn set_enabled(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    if let Err(e) = state.discord.get(&agent, &id).await {
        return manager_error(e);
    }
    let input: DiscordEnabledRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    envelope(
        &state,
        state.discord.set_enabled(agent, id, input.enabled).await,
        StatusCode::OK,
    )
    .await
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/discord/{connector_id}/restart",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),responses((status=200,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn restart(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    if let Err(e) = state.discord.get(&agent, &id).await {
        return manager_error(e);
    }
    envelope(
        &state,
        state.discord.restart(agent, id).await,
        StatusCode::OK,
    )
    .await
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/discord/{connector_id}/retry",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),responses((status=200,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn retry(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    if let Err(e) = state.discord.get(&agent, &id).await {
        return manager_error(e);
    }
    envelope(&state, state.discord.retry(agent, id).await, StatusCode::OK).await
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/discord/{connector_id}/outbound/suppress",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),request_body=DiscordSuppressRequest,responses((status=200,body=DiscordConnectorEnvelope),(status=400,body=ConnectorErrorBody),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn suppress(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    if let Err(e) = state.discord.get(&agent, &id).await {
        return manager_error(e);
    }
    let input: DiscordSuppressRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !input.acknowledge_uncertainty {
        return manager_error(DiscordManagerError::InvalidInput);
    }
    envelope(
        &state,
        state.discord.suppress_unknown(agent, id).await,
        StatusCode::OK,
    )
    .await
}
#[utoipa::path(delete,path="/api/agents/{agent_id}/connectors/discord/{connector_id}",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),responses((status=200,body=DeleteResponse),(status=403,body=ConnectorErrorBody),(status=404,body=ConnectorErrorBody),(status=409,body=ConnectorErrorBody),(status=422,body=ConnectorErrorBody),(status=502,body=ConnectorErrorBody),(status=503,body=ConnectorErrorBody)))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(e) = authorize(&state, &request, false) {
        return e;
    }
    match state.discord.delete(agent, id).await {
        Ok(()) => no_store(json_response(
            StatusCode::OK,
            &DeleteResponse { deleted: true },
        )),
        Err(e) => manager_error(e),
    }
}

/// Also covers upstream API-key rejection, timeout and extractor errors.
pub(super) async fn protect_cache(request: Request, next: axum::middleware::Next) -> Response {
    let native = request.uri().path().contains("/connectors/discord");
    let response = next.run(request).await;
    if native {
        no_store(response)
    } else {
        response
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use tower::ServiceExt;
    #[tokio::test]
    async fn duplicate_channel_has_a_specific_safe_conflict_code() {
        let response = manager_error(DiscordManagerError::DuplicateChannel);
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "discord_channel_already_connected");
    }
    #[tokio::test]
    async fn provider_errors_have_distinct_safe_codes_and_no_store() {
        for (provider, status, code) in [
            (
                DiscordTransportError::PermissionDenied,
                StatusCode::UNPROCESSABLE_ENTITY,
                "discord_permission_denied",
            ),
            (
                DiscordTransportError::HttpStatus { status: 404 },
                StatusCode::UNPROCESSABLE_ENTITY,
                "discord_channel_not_found",
            ),
            (
                DiscordTransportError::RateLimited {
                    retry_after_ms: 1000,
                    global: false,
                },
                StatusCode::SERVICE_UNAVAILABLE,
                "discord_rate_limited",
            ),
            (
                DiscordTransportError::Transport,
                StatusCode::SERVICE_UNAVAILABLE,
                "discord_network_error",
            ),
            (
                DiscordTransportError::InvalidResponse,
                StatusCode::BAD_GATEWAY,
                "discord_invalid_response",
            ),
            (
                DiscordTransportError::ResponseTooLarge,
                StatusCode::BAD_GATEWAY,
                "discord_invalid_response",
            ),
            (
                DiscordTransportError::HttpStatus { status: 500 },
                StatusCode::BAD_GATEWAY,
                "discord_provider_unavailable",
            ),
        ] {
            let response = manager_error(DiscordManagerError::Provider(provider));
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["code"], code);
            assert!(
                body["error"].as_str().unwrap().starts_with("Discord")
                    || body["error"].as_str().unwrap().starts_with("The daemon")
            );
        }
    }
    #[tokio::test]
    async fn authorization_precedes_body_and_unknown_owner_scope_and_every_native_response_is_no_store(
    ) {
        let state = std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let app = crate::app::app_with_state(state, crate::app::DaemonConfig::default());
        for (method, path, origin, expected) in [
            (
                "GET",
                "/api/agents/missing/connectors/discord",
                "https://evil.example",
                StatusCode::FORBIDDEN,
            ),
            (
                "POST",
                "/api/agents/missing/connectors/discord",
                "https://evil.example",
                StatusCode::FORBIDDEN,
            ),
            (
                "GET",
                "/api/agents/missing/connectors/discord",
                "http://localhost:4200",
                StatusCode::NOT_FOUND,
            ),
            (
                "POST",
                "/api/agents/missing/connectors/discord/c/outbound/suppress",
                "http://localhost:4200",
                StatusCode::NOT_FOUND,
            ),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8080")
                .header("origin", origin)
                .header("content-type", "application/json")
                .body(Body::from("invalid body"))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("invalid body"));
        }
    }
}
