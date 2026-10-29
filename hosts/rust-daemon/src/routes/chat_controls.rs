use super::contracts::SessionResponse;
use super::http::json_response;
use super::jobs::{authorize, body, no_store};
use super::{ApiError, AppState};
use crate::sessions::views;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RevisionRequest {
    pub(crate) mode: String,
    pub(crate) through_message_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevisionResponse {
    session: SessionResponse,
    retry_prompt: Option<String>,
    retry_document_edit_context: Option<crate::document_edits::preview_context::PreviewEditContext>,
    attachments_omitted: bool,
    notice: String,
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/revise", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path)), request_body = RevisionRequest,
    responses((status=201, description="A preserved conversation revision; retry restores a draft and never sends automatically",body=RevisionResponse),
      (status=403,description="Owner required"),(status=409,description="Source busy or invalid revision"),(status=503,description="History or persistence unavailable")))]
pub(super) async fn revise_session(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: RevisionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state.agent_runs.revise_chat(&agent,&session,&input.mode,input.through_message_id.as_deref()).await {
        Err(error)=>super::sessions::rejected(error),
        Ok(revision)=>match views::session_view(&state.daemon,&agent,&revision.session_id).await {
            None=>super::sessions::rejected(ApiError::not_found()),
            Some(view)=>no_store(json_response(StatusCode::CREATED,&RevisionResponse {
                session: SessionResponse::from(&view), retry_prompt:revision.retry_prompt,
                retry_document_edit_context: revision.retry_document_edit_context,
                attachments_omitted: revision.attachments_omitted,
                notice: if revision.attachments_omitted {
                    "Original chat preserved. Completed actions are unchanged. Attachment files stay in the original chat; reattach any needed files before sending."
                } else {
                    "Original chat preserved. Completed actions are unchanged. Retrying is a new request and may repeat actions."
                }.into(),
            })),
        }
    }
}
