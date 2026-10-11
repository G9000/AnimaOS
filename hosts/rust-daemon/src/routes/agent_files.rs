//! Current owner preview; never an assertion about a historical running snapshot.
use super::contracts::agent_files::{AgentFilesPreview, PreviewMode};
use super::http::json_response;
use super::jobs::{authorize, no_store};
use super::{ApiError, AppState};
use crate::agent_files::runtime::{capture_scope, scope_is_current};
use crate::agent_files::{self, AcceptedAgentFiles, Availability};
use anima_core::AgentFileRenderBudget;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/agent-files", tag = "agents",
    params(("agent_id" = String, Path)),
    responses((status=200, description="Content-free current preview, distinct from active run instructions", body=AgentFilesPreview),
        (status=403, description="Local owner required"), (status=404, description="Agent not found")))]
pub(super) async fn preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let flag = Arc::new(AtomicBool::new(false));
    let _cancel = CancelOnDrop(Arc::clone(&flag));
    let budget = AgentFileRenderBudget {
        max_identity_chars: 20_000,
        max_project_chars: 20_000,
        max_total_chars: 20_000,
    };
    for attempt in 0..2 {
        let scope = {
            let guard = state.daemon.read().await;
            if !guard.agents.contains_key(&id) {
                return no_store(ApiError::not_found().into_response());
            }
            capture_scope(&guard, &id, "", None)
        };
        let Some(scope) = scope else {
            return answer(AcceptedAgentFiles::empty(
                Availability::UnavailableWorkspace,
            ));
        };
        let files = agent_files::load(scope.clone(), budget, Arc::clone(&flag), deadline).await;
        let guard = state.daemon.read().await;
        if !guard.agents.contains_key(&id) {
            return no_store(ApiError::not_found().into_response());
        }
        if scope_is_current(&guard, &scope) {
            return answer(files);
        }
        if attempt == 1 {
            return answer(AcceptedAgentFiles::empty(Availability::StaleScope));
        }
    }
    unreachable!()
}

fn answer(files: AcceptedAgentFiles) -> Response {
    no_store(json_response(
        StatusCode::OK,
        &AgentFilesPreview {
            mode: PreviewMode::CurrentPreview,
            manifest: files.manifest,
        },
    ))
}
