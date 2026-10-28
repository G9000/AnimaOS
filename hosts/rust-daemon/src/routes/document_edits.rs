//! Authenticated complete structural review and explicit cancellation, never an apply endpoint.
use super::contracts::document_edits::DocumentEditOwnerQuery;
use super::{
    http::json_response,
    jobs::{authorize, no_store},
    sessions::rejected,
    ApiError, AppState,
};
use crate::document_edits::DocumentEditService;
use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
};

fn service(state: &AppState) -> DocumentEditService {
    state.document_edits.clone()
}
fn response<T: serde::Serialize>(value: Result<T, String>) -> Response {
    match value {
        Ok(value) => no_store(json_response(StatusCode::OK, &value)),
        Err(error) if error == crate::document_edits::service::UNCERTAIN => rejected(
            ApiError::bad_request_static(crate::document_edits::service::UNCERTAIN),
        ),
        Err(_) => rejected(ApiError::bad_request_static(
            "Document review is unavailable, changed or expired",
        )),
    }
}

#[utoipa::path(get,path="/api/document-edits/{proposal_id}",tag="documents",
    params(("proposal_id"=String,Path),("agent_id"=String,Query),("session_id"=String,Query)),
    responses((status=200,description="Complete exact structural content review with source/destination binding; no source or candidate binary"),(status=400,description="Review changed, unavailable or expired"),(status=403,description="Owner authorization required")))]
pub(super) async fn review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DocumentEditOwnerQuery>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    response(
        service(&state)
            .review(&id, &query.agent_id, &query.session_id)
            .await,
    )
}

#[utoipa::path(delete,path="/api/document-edits/{proposal_id}",tag="documents",
    params(("proposal_id"=String,Path),("agent_id"=String,Query),("session_id"=String,Query)),
    responses((status=200,description="Pending private proposal cancelled; published files are unaffected"),(status=400,description="Proposal unavailable or already applying"),(status=403,description="Owner authorization required")))]
pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DocumentEditOwnerQuery>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let result = service(&state)
        .cancel(id.clone(), query.agent_id.clone(), query.session_id.clone())
        .await;
    if result.is_ok() {
        let pending = {
            let guard = state.daemon.read().await;
            guard
                .approvals
                .pending()
                .into_iter()
                .filter(|record| {
                    record.agent_id == query.agent_id
                        && record.session_id == query.session_id
                        && record.tool == crate::document_edits::approval::APPLY_TOOL
                })
                .filter(|record| {
                    serde_json::from_str::<serde_json::Value>(&record.arguments)
                        .ok()
                        .is_some_and(|value| {
                            value.get("proposal_id").and_then(serde_json::Value::as_str)
                                == Some(id.as_str())
                        })
                })
                .map(|record| record.id.clone())
                .collect::<Vec<_>>()
        };
        for approval in pending {
            state
                .agent_runs
                .settle_approval(&approval, crate::state::Settlement::Stopped)
                .await;
        }
    }
    response(result)
}

#[cfg(test)]
#[path = "tests/document_edits.rs"]
mod tests;
