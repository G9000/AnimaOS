//! Session runs (spec §4.2): accept a message into a session, list a
//! session's runs, and read one run.

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures::future::BoxFuture;
use serde::Deserialize;
use utoipa::ToSchema;

use super::contracts::{ErrorBody, RunEnvelope, RunResponse, RunsEnvelope, SteerStatusResponse};
use super::http::{json_response, read_limited_body, request_query};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use crate::agent_runs::{
    AcceptRun, AcceptedRun, QueuedRunStart, QueuedStartError, SessionRunMode, SESSION_CANNOT_SEND,
};
use crate::connectors::runtime::ConnectorManagerError;
use crate::runs::{RunSource, MAX_RUN_ATTACHMENTS, MAX_RUN_INPUT_TEXT_BYTES};
use crate::sessions::{is_valid_session_id, SessionKind};
use crate::skills::{SKILL_CANNOT_STEER, SKILL_NOT_IN_TELEGRAM};

const DEFAULT_RUN_PAGE: usize = 20;
const MAX_RUN_PAGE: usize = 50;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;
/// Bodies this route reads, or the daemon-wide limit when that is larger: JSON
/// escapes a control character as six bytes (`\u0001`), so a 32 KiB text can
/// take 192 KiB, and the text check must answer, not the body limit (audit M29).
const MAX_RUN_REQUEST_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum StartRunMode {
    /// Wait behind the session's earlier messages.
    #[default]
    Queue,
    /// Join the session's active run before its next model call.
    Steer,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StartRunRequest {
    #[serde(default, deserialize_with = "crate::browser::optional_selection")]
    browser_selection: Option<crate::browser::BrowserSelection>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    attachment_ids: Vec<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    mode: StartRunMode,
    /// The sender's IANA time zone, as their browser reports it; it becomes
    /// the owner's time zone for every run, Telegram and automations too.
    #[serde(default)]
    time_zone: Option<String>,
}

/// Makes `zone` the owner's time zone and saves it in the background, so a
/// slow store never delays the run. A zone this build does not know is
/// ignored: the message still runs, on the zone held before.
async fn remember_owner_time_zone(state: &AppState, zone: Option<&str>) {
    let Some(zone) =
        zone.and_then(|name| crate::schedules::timing::parse_time_zone(name.trim()).ok())
    else {
        return;
    };
    let owner = state.daemon.read().await.owner_time_zone.clone();
    if !owner.set(Some(zone)) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let transaction = state.agent_runs.control_plane_transaction().await;
        let persist = state.daemon.write().await.control_plane_persist_request();
        if let Err(error) = persist.save().await {
            tracing::warn!(%error, "the owner's time zone was not saved");
        }
        drop(transaction);
    });
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let mut values = headers.get_all("idempotency-key").iter();
    let Some(value) = values.next() else {
        return Err(ApiError::bad_request_static(
            "Idempotency-Key header is required",
        ));
    };
    let single = values.next().is_none();
    match value.to_str() {
        Ok(key)
            if single
                && !key.is_empty()
                && key.len() <= MAX_IDEMPOTENCY_KEY_BYTES
                && key.bytes().all(|byte| matches!(byte, 0x21..=0x7e)) =>
        {
            Ok(key.to_string())
        }
        _ => Err(ApiError::bad_request_static(
            "Idempotency-Key header is invalid",
        )),
    }
}

async fn start_body(state: &AppState, request: Request) -> Result<StartRunRequest, Response> {
    let limit = state.config.max_request_bytes.max(MAX_RUN_REQUEST_BYTES);
    let bytes = read_limited_body(request, limit).await.map_err(no_store)?;
    parse_json_body(bytes).map_err(|error: ApiError| no_store(error.into_response()))
}

fn validate_input(input: &StartRunRequest) -> Result<(), ApiError> {
    if input.text.trim().is_empty() && input.attachment_ids.is_empty() {
        return Err(ApiError::bad_request_static(
            "text or attachments are required",
        ));
    }
    if input.text.len() > MAX_RUN_INPUT_TEXT_BYTES {
        return Err(ApiError::bad_request_static("text must be at most 32 KiB"));
    }
    if input.attachment_ids.len() > MAX_RUN_ATTACHMENTS {
        return Err(ApiError::bad_request_static(
            "at most 10 attachments are allowed per message",
        ));
    }
    crate::agent_runs::attachments::validate_attachment_ids(&input.attachment_ids)?;
    Ok(())
}

fn run_limit(uri: &Uri) -> Result<usize, ApiError> {
    let params =
        request_query(uri).map_err(|()| ApiError::bad_request_static("malformed query"))?;
    match params.get("limit").map(String::as_str) {
        None | Some("") => Ok(DEFAULT_RUN_PAGE),
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|limit| (1..=MAX_RUN_PAGE).contains(limit))
            .ok_or_else(|| {
                ApiError::bad_request(format!("limit must be between 1 and {MAX_RUN_PAGE}"))
            }),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/runs", tag = "runs",
    params(
        ("agent_id" = String, Path),
        ("session_id" = String, Path, description = "Percent-encoded session id"),
        ("Idempotency-Key" = String, Header, description = "1–128 visible ASCII characters. The same key within 24 hours returns the original run and creates nothing, but only while its run is still in the ledger: at most 24 hours and, once the history store holds it, among the agent's newest 50 finished runs")
    ),
    request_body = StartRunRequest,
    responses(
        (status = 200, description = "The key was used for this message within 24 hours: the original run, nothing created; a retried steer still waiting in the run it joined is that run with steer.status pending", body = RunEnvelope),
        (status = 202, description = "Accepted: the queued run, or the active run a steer joined (with steer.status pending)", body = RunEnvelope),
        (status = 400, description = "Missing or invalid key, empty text with no attachments, text over 32 KiB, over 10 or unknown attachments, an unknown skill, one that is off or waiting for review, a skill with steer, a skill in a Telegram session, steer on a kind that cannot steer, or a body over 256 KiB", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody),
        (status = 409, description = "The kind cannot receive messages (a job or helper session, or a Telegram session without its connector), the agent is a helper or is being deleted, or the key was used for a different message", body = ErrorBody),
        (status = 429, description = "Eight messages are already waiting for this companion", body = ErrorBody),
        (status = 503, description = "The control plane could not be saved, or the daemon is shutting down", body = ErrorBody)
    ))]
pub(super) async fn start_session_run(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    let idempotency_key = match idempotency_key(request.headers()) {
        Ok(key) => key,
        Err(error) => return rejected(error),
    };
    let input = match start_body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if let Err(error) = validate_input(&input) {
        return rejected(error);
    }
    remember_owner_time_zone(&state, input.time_zone.as_deref()).await;
    if let Some(replayed) = state
        .agent_runs
        .replayed_run_with_attachments(
            &agent_id,
            &session_id,
            &input.text,
            &idempotency_key,
            &input.attachment_ids,
            input.skill.as_deref(),
        )
        .await
    {
        return accepted_response(replayed);
    }
    if let Some(skill) = input.skill.as_deref() {
        if matches!(input.mode, StartRunMode::Steer) {
            return rejected(ApiError::bad_request_static(SKILL_CANNOT_STEER));
        }
        // Advisory: the run itself rereads and hash-checks the skill. A
        // retried key still gets its run (audit m11).
        if let Err(message) = state.agent_runs.skills().check_runnable(skill).await {
            return match state
                .agent_runs
                .replayed_run_with_attachments(
                    &agent_id,
                    &session_id,
                    &input.text,
                    &idempotency_key,
                    &input.attachment_ids,
                    input.skill.as_deref(),
                )
                .await
            {
                Some(answer) => accepted_response(answer),
                None => rejected(ApiError::bad_request_static(message)),
            };
        }
    }
    let browser = if let Some(selection) = input.browser_selection.clone() {
        if matches!(input.mode, StartRunMode::Steer) {
            return rejected(ApiError::bad_request_static(
                "Browser selection cannot steer a run",
            ));
        }
        match state
            .browser
            .prepare_selection(&agent_id, &session_id, selection)
            .await
        {
            Ok(prepared) => Some(prepared),
            Err(_) => {
                return rejected(ApiError::conflict(
                    "Browser selection is unavailable or changed",
                ))
            }
        }
    } else {
        None
    };
    // Which flow runs the message; `accept_run` re-checks the session under
    // the control-plane transaction.
    let target = {
        let guard = state.daemon.read().await;
        guard.sessions.get(&agent_id, &session_id).map(|record| {
            let connector = matches!(record.kind, SessionKind::Telegram | SessionKind::Discord)
                .then(|| {
                    if record.kind == SessionKind::Discord {
                        return guard
                            .discord_connectors
                            .values()
                            .find(|c| {
                                c.is_active()
                                    && c.agent_id == agent_id
                                    && c.room_id == record.room_id()
                            })
                            .map(|c| c.id.clone());
                    }
                    guard
                        .connectors
                        .values()
                        .find(|connector| {
                            connector.is_active()
                                && connector.agent_id == agent_id
                                && connector.room_id == record.room_id()
                        })
                        .map(|connector| connector.id.clone())
                });
            (
                record.room_id().to_string(),
                connector,
                record.kind == SessionKind::Discord,
            )
        })
    };
    let Some((room_id, connector, is_discord)) = target else {
        return rejected(ApiError::not_found());
    };
    if is_discord && (input.skill.is_some() || !input.attachment_ids.is_empty()) {
        return rejected(ApiError::bad_request_static(
            "Discord sessions support text only; skills and attachments are unavailable",
        ));
    }
    if input.skill.is_some() && connector.is_some() {
        return rejected(ApiError::bad_request_static(SKILL_NOT_IN_TELEGRAM));
    }
    let (source, source_ref, start): (RunSource, Option<String>, QueuedRunStart) = match connector {
        // A Telegram session's message is the connector's owner turn (spec §4.2).
        Some(Some(connector_id)) if is_discord => {
            let manager = state.discord.clone();
            let runs = state.agent_runs.clone();
            let (agent, text, key, connector) = (
                agent_id.clone(),
                input.text.clone(),
                idempotency_key.clone(),
                connector_id.clone(),
            );
            let start = QueuedRunStart::new(
                move |run_id| -> BoxFuture<'static, Result<(), QueuedStartError>> {
                    Box::pin(async move {
                        match manager
                            .send_from_owner_accepted(agent, connector, text, key, run_id)
                            .await
                        {
                            Ok(()) => Ok(()),
                            Err(_) if manager.is_closing() || runs.is_shutting_down() => {
                                Err(QueuedStartError::ShuttingDown)
                            }
                            Err(e) => Err(QueuedStartError::Failed(e.to_string())),
                        }
                    })
                },
            );
            (RunSource::Discord, Some(connector_id), start)
        }
        Some(Some(connector_id)) => {
            let manager = state.connector_manager.clone();
            let runs = state.agent_runs.clone();
            let (agent, text, key, connector) = (
                agent_id.clone(),
                input.text.clone(),
                idempotency_key.clone(),
                connector_id.clone(),
            );
            let start = QueuedRunStart::new(
                move |run_id| -> BoxFuture<'static, Result<(), QueuedStartError>> {
                    Box::pin(async move {
                        match manager
                            .send_from_owner_accepted(agent, connector, text, key, run_id)
                            .await
                        {
                            Ok(()) => Ok(()),
                            // The connectors closed for shutdown before its
                            // turn, or shutdown ended its wait for a slot or
                            // permit: it stays queued for the restart.
                            Err(ConnectorManagerError::WorkerStopped)
                                if manager.is_closing() || runs.is_shutting_down() =>
                            {
                                Err(QueuedStartError::ShuttingDown)
                            }
                            Err(error) => Err(QueuedStartError::Failed(error.to_string())),
                        }
                    })
                },
            );
            (RunSource::Telegram, Some(connector_id), start)
        }
        // No active connector runs it: a new message is refused, but a
        // retried key still gets its run (review Minor 6).
        Some(None) => {
            return match state
                .agent_runs
                .replayed_run_with_attachments(
                    &agent_id,
                    &session_id,
                    &input.text,
                    &idempotency_key,
                    &input.attachment_ids,
                    input.skill.as_deref(),
                )
                .await
            {
                Some(answer) => accepted_response(answer),
                None => rejected(ApiError::conflict(SESSION_CANNOT_SEND)),
            };
        }
        None => (
            RunSource::Web,
            None,
            if let Some(prepared) = &browser {
                state.agent_runs.web_start_with_browser(
                    agent_id.clone(),
                    room_id,
                    idempotency_key.clone(),
                    prepared.clone(),
                )
            } else {
                state.agent_runs.web_start_with_attachments(
                    agent_id.clone(),
                    room_id,
                    input.text.clone(),
                    idempotency_key.clone(),
                    input.skill.clone(),
                    input.attachment_ids.clone(),
                )
            },
        ),
    };
    let mode = match input.mode {
        StartRunMode::Queue => SessionRunMode::Queue,
        StartRunMode::Steer => SessionRunMode::Steer,
    };
    let request = AcceptRun {
        agent_id,
        session_id,
        text: input.text,
        attachment_ids: input.attachment_ids,
        idempotency_key,
        mode,
        source,
        source_ref,
        skill: input.skill,
    };
    let accepted = match browser {
        Some(prepared) => {
            state
                .agent_runs
                .accept_browser_run(request, start, prepared)
                .await
        }
        None => state.agent_runs.accept_run(request, start).await,
    };
    accepted_response(accepted)
}

/// 202 with a new run or the run a steer joined, 200 with a replayed one, or
/// the refusal.
fn accepted_response(accepted: Result<AcceptedRun, ApiError>) -> Response {
    match accepted {
        Ok(AcceptedRun::Created(record)) => no_store(json_response(
            StatusCode::ACCEPTED,
            &RunEnvelope::of(&record),
        )),
        Ok(AcceptedRun::Replayed(record)) => {
            no_store(json_response(StatusCode::OK, &RunEnvelope::of(&record)))
        }
        Ok(AcceptedRun::Steered(record)) => {
            no_store(json_response(StatusCode::ACCEPTED, &pending_steer(&record)))
        }
        Ok(AcceptedRun::ReplayedSteer(record)) => {
            no_store(json_response(StatusCode::OK, &pending_steer(&record)))
        }
        Err(error) => rejected(error),
    }
}

/// The run a steer joined, with the steer still waiting in it.
fn pending_steer(record: &crate::runs::RunRecord) -> RunEnvelope {
    RunEnvelope {
        run: RunResponse::from(record),
        steer: Some(SteerStatusResponse {
            status: "pending".into(),
        }),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/runs", tag = "runs",
    params(
        ("agent_id" = String, Path),
        ("session_id" = String, Path, description = "Percent-encoded session id"),
        ("limit" = Option<usize>, Query, description = "1–50, default 20")
    ),
    responses(
        (status = 200, description = "The session's runs the ledger holds, newest first", body = RunsEnvelope),
        (status = 400, description = "Invalid limit", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody)
    ))]
pub(super) async fn list_session_runs(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let limit = match run_limit(request.uri()) {
        Ok(limit) => limit,
        Err(error) => return rejected(error),
    };
    let guard = state.daemon.read().await;
    if !guard.agents.contains_key(&agent_id) || guard.sessions.get(&agent_id, &session_id).is_none()
    {
        return rejected(ApiError::not_found());
    }
    let runs = guard
        .runs
        .for_session(&agent_id, &session_id)
        .into_iter()
        .take(limit)
        .map(|record| RunResponse::from(&guard.with_live_tools(record.clone())))
        .collect();
    no_store(json_response(StatusCode::OK, &RunsEnvelope { runs }))
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/runs/{run_id}", tag = "runs",
    params(("agent_id" = String, Path), ("run_id" = String, Path)),
    responses(
        (status = 200, description = "The run, from the ledger or, once pruned, the history store", body = RunEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or run not found", body = ErrorBody),
        (status = 503, description = "The history store cannot be read", body = ErrorBody)
    ))]
pub(super) async fn get_run(
    State(state): State<AppState>,
    Path((agent_id, run_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let (known, history) = {
        let guard = state.daemon.read().await;
        if !guard.agents.contains_key(&agent_id) {
            return rejected(ApiError::not_found());
        }
        (
            guard
                .runs
                .get(&run_id)
                .filter(|record| record.agent_id == agent_id)
                .map(|record| guard.with_live_tools(record.clone())),
            guard.history.clone(),
        )
    };
    let record = match known {
        Some(record) => record,
        // Read outside the state lock (M2 lock rule).
        None => match history.store().get_run(&run_id).await {
            Ok(Some(record)) if record.agent_id == agent_id => record,
            Ok(_) => return rejected(ApiError::not_found()),
            Err(error) => return rejected(ApiError::service_unavailable(error.message())),
        },
    };
    no_store(json_response(StatusCode::OK, &RunEnvelope::of(&record)))
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/runs/{run_id}/stop", tag = "runs",
    params(("agent_id" = String, Path), ("run_id" = String, Path)),
    responses(
        (status = 202, description = "Stop accepted: a queued run is cancelled; a running run stops at its next checkpoint; a finished run is returned as it is", body = RunEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or run not found", body = ErrorBody),
        (status = 503, description = "The stop could not be saved, or the history store could not be read for a run the ledger no longer holds", body = ErrorBody)
    ))]
pub(super) async fn stop_run(
    State(state): State<AppState>,
    Path((agent_id, run_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    match state.agent_runs.stop_run(&agent_id, &run_id).await {
        Ok(record) => no_store(json_response(
            StatusCode::ACCEPTED,
            &RunEnvelope::of(&record),
        )),
        Err(error) => rejected(error),
    }
}
