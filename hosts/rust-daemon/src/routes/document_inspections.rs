//! Explicit owner-requested inspection and current-authority delivery of native facts.
use super::{
    http::json_response,
    jobs::{authorize, no_store},
    ApiError, AppState,
};
use crate::document_inspections::{DocumentInspectionService, InspectionRequest};
use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnerQuery {
    agent_id: String,
    session_id: String,
}

fn service(state: &AppState) -> DocumentInspectionService {
    DocumentInspectionService::new(state.daemon.clone())
}
fn response<T: serde::Serialize>(result: Result<T, String>) -> Response {
    match result {
        Ok(value) => no_store(json_response(StatusCode::OK, &value)),
        Err(reason) => no_store(json_response(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"kind":"inconclusive","reason":reason,"error":reason}),
        )),
    }
}

#[utoipa::path(post,path="/api/document-inspections",tag="documents",
    params(("agent_id"=String,Query),("session_id"=String,Query)),request_body=serde_json::Value,
    responses((status=200,description="Immutable facts from original captured Office bytes; never a quality pass",body=serde_json::Value),(status=400,description="Unavailable, unsupported, changed or inconclusive"),(status=403,description="Owner authentication required")))]
pub(super) async fn inspect(
    State(state): State<AppState>,
    Query(owner): Query<OwnerQuery>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let bytes = match axum::body::to_bytes(
        request.into_body(),
        state.config.max_request_bytes.min(4096),
    )
    .await
    {
        Ok(bytes) => bytes,
        Err(_) => {
            return no_store(
                ApiError::bad_request_static("Document inspection request exceeds its bounds")
                    .into_response(),
            )
        }
    };
    let input: InspectionRequest = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => {
            return no_store(
                ApiError::bad_request_static("Document inspection request is invalid")
                    .into_response(),
            )
        }
    };
    response(
        service(&state)
            .inspect_owner(
                &owner.agent_id,
                &owner.session_id,
                input,
                anima_core::CancelSignal::new(),
            )
            .await,
    )
}

#[utoipa::path(get,path="/api/document-inspections/{report_sha256}",tag="documents",
    params(("report_sha256"=String,Path),("agent_id"=String,Query),("session_id"=String,Query)),
    responses((status=200,description="Captured facts after current source and owner revalidation",body=serde_json::Value),(status=400,description="Report or current source authority unavailable"),(status=403,description="Owner authentication required")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(hash): Path<String>,
    Query(owner): Query<OwnerQuery>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    response(
        service(&state)
            .get(&owner.agent_id, &owner.session_id, &hash)
            .await,
    )
}

#[utoipa::path(get,path="/api/document-inspections/{report_sha256}/pages/{page}",tag="documents",
    params(("report_sha256"=String,Path),("page"=u32,Path),("agent_id"=String,Query),("session_id"=String,Query)),
    responses((status=200,description="Hash-checked actual rendered PNG under current source authority",body=Vec<u8>,content_type="image/png"),(status=400,description="Page or current source authority unavailable"),(status=403,description="Owner authentication required")))]
pub(super) async fn page(
    State(state): State<AppState>,
    Path((hash, page)): Path<(String, u32)>,
    Query(owner): Query<OwnerQuery>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    match service(&state)
        .page(&owner.agent_id, &owner.session_id, &hash, page)
        .await
    {
        Ok(bytes) => {
            let mut response = (StatusCode::OK, bytes).into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
            response.headers_mut().insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );
            no_store(response)
        }
        Err(reason) => response::<serde_json::Value>(Err(reason)),
    }
}

#[cfg(test)]
#[path = "tests/document_inspections.rs"]
mod tests;
