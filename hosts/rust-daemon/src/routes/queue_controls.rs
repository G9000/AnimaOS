//! Scoped owner controls for inputs that have not been claimed by the drainer.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Deserializer};
use utoipa::ToSchema;

use super::contracts::{ErrorBody, RunEnvelope};
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use crate::agent_runs::QueuedRunPatch;

fn present_position<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

fn present_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct QueuedRunPatchRequest {
    #[serde(default, deserialize_with = "present_text")]
    text: Option<String>,
    /// Omit to preserve position, null to move to the end, or a waiting run ID.
    #[serde(default, deserialize_with = "present_position")]
    #[schema(value_type = Option<String>)]
    before_run_id: Option<Option<String>>,
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/sessions/{session_id}/queue/{run_id}", tag = "runs",
    params(("agent_id" = String, Path), ("session_id" = String, Path), ("run_id" = String, Path)),
    request_body = QueuedRunPatchRequest,
    responses(
        (status = 200, description = "Waiting input and order saved", body = RunEnvelope),
        (status = 400, description = "Invalid patch, empty text without attachments, or text over 32 KiB", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent, session or scoped run not found", body = ErrorBody),
        (status = 409, description = "Run already claimed or source protected, or target no longer waiting; no partial edit", body = ErrorBody),
        (status = 503, description = "Save failed and mutation rolled back", body = ErrorBody)
    ))]
pub(super) async fn update_queued_run(
    State(state): State<AppState>,
    Path((agent_id, session_id, run_id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !crate::sessions::is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    let bytes =
        match read_limited_body(request, state.config.max_request_bytes.max(256 * 1024)).await {
            Ok(bytes) => bytes,
            Err(response) => return no_store(response),
        };
    let patch: QueuedRunPatchRequest = match parse_json_body(bytes) {
        Ok(patch) => patch,
        Err(error) => return rejected(error),
    };
    match state
        .agent_runs
        .update_queued_run(
            &agent_id,
            &session_id,
            &run_id,
            QueuedRunPatch {
                text: patch.text,
                before_run_id: patch.before_run_id,
            },
        )
        .await
    {
        Ok(run) => no_store(json_response(StatusCode::OK, &RunEnvelope::of(&run))),
        Err(error) => no_store(error.into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queued_patch_preserves_omitted_null_and_target_positions() {
        let text: QueuedRunPatchRequest = serde_json::from_value(json!({"text":"edited"})).unwrap();
        assert!(text.before_run_id.is_none());
        let end: QueuedRunPatchRequest =
            serde_json::from_value(json!({"beforeRunId":null})).unwrap();
        assert_eq!(end.before_run_id, Some(None));
        let before: QueuedRunPatchRequest =
            serde_json::from_value(json!({"beforeRunId":"other"})).unwrap();
        assert_eq!(before.before_run_id, Some(Some("other".into())));
        for invalid in [
            json!({"text":null}),
            json!({"attachmentIds":[]}),
            json!({"beforeRunId":4}),
        ] {
            assert!(serde_json::from_value::<QueuedRunPatchRequest>(invalid).is_err());
        }
    }
}
