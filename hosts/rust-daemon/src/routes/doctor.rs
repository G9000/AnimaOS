//! Authenticated local-owner Doctor controls. Read observations do not execute repairs.
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, AppState};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RepairRequest {
    id: String,
    fingerprint: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RebuildRequest {
    fingerprint: String,
}
#[derive(Serialize, ToSchema)]
pub(super) struct DoctorRepairs {
    candidates: Vec<crate::doctor::RepairCandidate>,
    history: Vec<crate::doctor::RepairView>,
}
fn service(s: &AppState) -> crate::doctor::DoctorService {
    s.agent_runs.doctor()
}
fn result<T: Serialize>(r: Result<T, super::ApiError>) -> Response {
    match r {
        Ok(v) => no_store(json_response(StatusCode::OK, &v)),
        Err(e) => rejected(e),
    }
}
#[utoipa::path(get,path="/api/doctor",tag="system",responses((status=200,description="Bounded local observations; no external probes",body=crate::doctor::DoctorReport)))]
pub(super) async fn report(State(s): State<AppState>, r: Request) -> Response {
    if let Err(r) = authorize(&s, &r, true) {
        return r;
    }
    result(Ok(service(&s).report().await))
}
#[utoipa::path(get,path="/api/agents/{agent_id}/doctor/repairs",tag="system",params(("agent_id"=String,Path)),responses((status=200,description="Exact candidates and durable undo history",body=DoctorRepairs)))]
pub(super) async fn repairs(
    State(s): State<AppState>,
    Path(a): Path<String>,
    r: Request,
) -> Response {
    if let Err(r) = authorize(&s, &r, true) {
        return r;
    }
    let svc = service(&s);
    let out = async {
        Ok(DoctorRepairs {
            candidates: svc.candidates(&a).await?,
            history: svc.history(&a).await?,
        })
    }
    .await;
    result(out)
}
async fn mutate(s: AppState, a: String, r: Request, undo: bool) -> Response {
    if let Err(r) = authorize(&s, &r, false) {
        return r;
    }
    if s.agent_runs.is_draining() {
        return rejected(super::ApiError::conflict(
            "Doctor repair admission is draining",
        ));
    }
    let bytes = match read_limited_body(r, 2048).await {
        Ok(b) => b,
        Err(r) => return no_store(r),
    };
    let q: RepairRequest = match parse_json_body(bytes) {
        Ok(q) => q,
        Err(e) => return rejected(e),
    };
    result(if undo {
        service(&s).undo(a, q.id, q.fingerprint, None).await
    } else {
        service(&s).repair(a, q.id, q.fingerprint, None).await
    })
}
#[utoipa::path(post,path="/api/agents/{agent_id}/doctor/repairs",tag="system",params(("agent_id"=String,Path)),request_body=RepairRequest,responses((status=200,description="Exact pause repair saved",body=crate::doctor::RepairView),(status=409,description="Candidate changed")))]
pub(super) async fn repair(
    State(s): State<AppState>,
    Path(a): Path<String>,
    r: Request,
) -> Response {
    mutate(s, a, r, false).await
}
#[utoipa::path(post,path="/api/agents/{agent_id}/doctor/undo",tag="system",params(("agent_id"=String,Path)),request_body=RepairRequest,responses((status=200,description="Exact pause undone and future schedule resumed",body=crate::doctor::RepairView),(status=409,description="Intervening change")))]
pub(super) async fn undo(State(s): State<AppState>, Path(a): Path<String>, r: Request) -> Response {
    mutate(s, a, r, true).await
}
#[utoipa::path(get,path="/api/doctor/memory-index",tag="system",responses((status=200,description="Current canonical memory and embedding model fingerprint")))]
pub(super) async fn index(State(s): State<AppState>, r: Request) -> Response {
    if let Err(r) = authorize(&s, &r, true) {
        return r;
    }
    result(service(&s).index_candidate().await.map(|f|serde_json::json!({"fingerprint":f,"repair":"rebuild-local-derived-index","undoSupported":false})))
}
#[utoipa::path(post,path="/api/doctor/memory-index",tag="system",request_body=RebuildRequest,responses((status=200,description="Bounded local derived index rebuilt; canonical memories unchanged"),(status=409,description="Changed, busy, nonlocal or over budget")))]
pub(super) async fn rebuild(State(s): State<AppState>, r: Request) -> Response {
    if let Err(r) = authorize(&s, &r, false) {
        return r;
    }
    if s.agent_runs.is_draining() {
        return rejected(super::ApiError::conflict(
            "Doctor repair admission is draining",
        ));
    }
    let b = match read_limited_body(r, 1024).await {
        Ok(v) => v,
        Err(r) => return no_store(r),
    };
    let q: RebuildRequest = match parse_json_body(b) {
        Ok(v) => v,
        Err(e) => return rejected(e),
    };
    result(service(&s).rebuild_index(q.fingerprint).await)
}
