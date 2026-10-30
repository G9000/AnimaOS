use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[utoipa::path(get,path="/api/agents/{agent_id}/sessions/{session_id}/images",tag="sessions",params(("agent_id"=String,Path),("session_id"=String,Path)),responses((status=200,description="Owner-scoped native image operations and immutable saved versions",body=serde_json::Value),(status=403,description="Owner required"),(status=404,description="Conversation not found")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let _transaction = state.agent_runs.control_plane_transaction().await;
    let guard = state.daemon.read().await;
    if let Err(error) = guard.ensure_profile_owned() {
        return no_store(ApiError::service_unavailable(error.to_string()).into_response());
    }
    let Some(session) = guard.sessions.get(&agent, &id).filter(|s| {
        s.kind == crate::sessions::SessionKind::Chat
            && matches!(
                s.origin,
                crate::sessions::SessionOrigin::Web | crate::sessions::SessionOrigin::Api
            )
    }) else {
        return no_store(ApiError::not_found().into_response());
    };
    if let Err(error) = crate::images::validate_session(session) {
        return no_store(ApiError::service_unavailable(error).into_response());
    }
    let operations = session
        .image_operations
        .iter()
        .map(|operation| {
            let mut value = operation.public_value();
            if operation.status == crate::images::ImageStatus::Dispatching
                && !guard
                    .runs
                    .get(&operation.run_id)
                    .is_some_and(|r| r.status.is_in_flight() && r.stop.is_none())
            {
                value["status"] = serde_json::json!("outcome_unknown");
            }
            value
        })
        .collect::<Vec<_>>();
    no_store(json_response(
        StatusCode::OK,
        &serde_json::json!({"clearGeneration":session.clear_generation,"provider":"openai","model":anima_service_adapters::IMAGE_MODEL,"operations":operations}),
    ))
}
