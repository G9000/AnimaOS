use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

struct ImageLedgerSnapshot {
    generation: u64,
    project_id: Option<String>,
    binding_revision: u64,
    workspace: Option<std::path::PathBuf>,
    operations: Vec<crate::images::ImageOperation>,
}

impl ImageLedgerSnapshot {
    fn capture(state: &crate::state::DaemonState, agent: &str, id: &str) -> Result<Self, ApiError> {
        let session = state
            .sessions
            .get(agent, id)
            .filter(|session| {
                session.kind == crate::sessions::SessionKind::Chat
                    && matches!(
                        session.origin,
                        crate::sessions::SessionOrigin::Web | crate::sessions::SessionOrigin::Api
                    )
            })
            .ok_or_else(ApiError::not_found)?;
        if state.pending_history_deletions.iter().any(|deletion| {
            deletion.agent_id == agent
                && deletion
                    .session_id
                    .as_ref()
                    .is_none_or(|session| session == id)
        }) {
            return Err(ApiError::not_found());
        }
        crate::images::validate_session(session).map_err(ApiError::service_unavailable)?;
        Ok(Self {
            generation: session.clear_generation,
            project_id: session.project_id.clone(),
            binding_revision: session.project_binding_revision,
            workspace: state
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone()),
            operations: session.image_operations.clone(),
        })
    }

    fn is_current(&self, state: &crate::state::DaemonState, agent: &str, id: &str) -> bool {
        Self::capture(state, agent, id).is_ok_and(|current| {
            current.generation == self.generation
                && current.project_id == self.project_id
                && current.binding_revision == self.binding_revision
                && current.workspace == self.workspace
                && current.operations == self.operations
        })
    }
}

#[utoipa::path(get,path="/api/agents/{agent_id}/sessions/{session_id}/images",tag="sessions",params(("agent_id"=String,Path),("session_id"=String,Path)),responses((status=200,description="Owner-scoped native image operations, immutable saved versions and bounded local byte observations",body=serde_json::Value),(status=403,description="Owner required"),(status=404,description="Conversation not found"),(status=409,description="Conversation changed during readback")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let snapshot = {
        let _transaction = state.agent_runs.control_plane_transaction().await;
        let guard = state.daemon.read().await;
        if let Err(error) = guard.ensure_profile_owned() {
            return no_store(ApiError::service_unavailable(error.to_string()).into_response());
        }
        match ImageLedgerSnapshot::capture(&guard, &agent, &id) {
            Ok(snapshot) => snapshot,
            Err(error) => return no_store(error.into_response()),
        }
    };
    let mut values = crate::images::observation::observed_values(
        snapshot.workspace.clone(),
        &snapshot.operations,
    )
    .await;
    let guard = state.daemon.read().await;
    if let Err(error) = guard.ensure_profile_owned() {
        return no_store(ApiError::service_unavailable(error.to_string()).into_response());
    }
    if !snapshot.is_current(&guard, &agent, &id) {
        return no_store(
            ApiError::conflict("Image conversation changed during readback; refresh images")
                .into_response(),
        );
    }
    for (value, operation) in values.iter_mut().zip(&snapshot.operations) {
        if operation.status == crate::images::ImageStatus::Dispatching
            && !guard
                .runs
                .get(&operation.run_id)
                .is_some_and(|run| run.status.is_in_flight() && run.stop.is_none())
        {
            value["status"] = serde_json::json!("outcome_unknown");
        }
    }
    no_store(json_response(
        StatusCode::OK,
        &serde_json::json!({"clearGeneration":snapshot.generation,"provider":"openai","model":anima_service_adapters::IMAGE_MODEL,"operations":values}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};

    #[test]
    fn image_readback_scope_rejects_clear_rebind_and_workspace_changes() {
        let mut state = crate::state::DaemonState::new();
        let session = SessionRecord::new(
            "agent",
            "chat:image",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Images".into(),
            TitleSource::Owner,
            1,
        );
        state.sessions.insert(session);
        let snapshot =
            ImageLedgerSnapshot::capture(&state, "agent", "chat:image").expect("image snapshot");
        assert!(snapshot.is_current(&state, "agent", "chat:image"));
        let session = state
            .sessions
            .get_mut("agent", "chat:image")
            .expect("session");
        session.clear_generation += 1;
        assert!(!snapshot.is_current(&state, "agent", "chat:image"));
        let session = state
            .sessions
            .get_mut("agent", "chat:image")
            .expect("session");
        session.clear_generation -= 1;
        session.project_binding_revision += 1;
        assert!(!snapshot.is_current(&state, "agent", "chat:image"));
        state
            .sessions
            .get_mut("agent", "chat:image")
            .expect("session")
            .project_binding_revision -= 1;
        state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: std::path::PathBuf::from("different-workspace"),
            company_name: "Test".into(),
            mission: "Test".into(),
            values: vec![],
        });
        assert!(!snapshot.is_current(&state, "agent", "chat:image"));
        assert!(!snapshot.is_current(&state, "agent", "chat:other"));
    }
}
