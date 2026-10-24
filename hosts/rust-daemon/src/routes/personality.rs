//! Owner editing of the source admitted by the runtime instruction loader.
use super::jobs::{authorize, body, no_store};
use super::{json_response, ApiError, AppState};
use crate::agent_files::runtime::{capture_scope, config_revision, scope_is_current};
use crate::agent_files::{self, AgentFileScope, AgentFileState, Availability};
use crate::tools::workspace_bytes::{InstructionReadError, WorkspaceBytes};
use crate::usage::metered::{record_secondary, Metered, SecondaryCall};
use crate::usage::UsageSource;
use anima_core::{
    AgentConfig, AgentFileRenderBudget, Content, Message, MessageRole, ModelAdapter,
    ModelGenerateRequest,
};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{atomic::AtomicBool, Arc, OnceLock};
use std::time::Instant;
use utoipa::ToSchema;

const MAX_CHARS: usize = 16_000;

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum PersonalitySource {
    AgentConfig,
    WorkspaceSoul,
    ProfileSoul,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersonalityView {
    source: PersonalitySource,
    path: Option<String>,
    content: String,
    revision: String,
    max_chars: usize,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SavePersonality {
    content: String,
    revision: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewPersonality {
    content: String,
    prompt: String,
}

struct Snapshot {
    config: AgentConfig,
    scope: Option<AgentFileScope>,
    held: Option<WorkspaceBytes>,
    candidate: Option<String>,
    raw: Option<Vec<u8>>,
    canonical_root: Option<std::path::PathBuf>,
    view: PersonalityView,
}
fn changed() -> ApiError {
    ApiError::conflict("Personality source changed; reload before saving")
}
fn unavailable() -> ApiError {
    ApiError::service_unavailable("The active SOUL source is unreadable, linked, empty, or rejected by the instruction loader; repair the file before editing")
}
fn make_view(
    agent_id: &str,
    config: &AgentConfig,
    scope: Option<&AgentFileScope>,
    candidate: Option<&str>,
    raw: Option<&[u8]>,
    canonical_root: Option<&std::path::Path>,
) -> Result<PersonalityView, ApiError> {
    let source = if raw.is_some() {
        if scope.is_some_and(|scope| scope.profile_enabled) {
            PersonalitySource::ProfileSoul
        } else {
            PersonalitySource::WorkspaceSoul
        }
    } else {
        PersonalitySource::AgentConfig
    };
    let path = raw.and(candidate).map(str::to_owned);
    let content = match raw {
        Some(bytes) => String::from_utf8(bytes.to_vec()).map_err(|_| unavailable())?,
        None => config.legacy_personality(),
    };
    let binding = serde_json::to_vec(&(
        "active-personality-v1",
        agent_id,
        &source,
        candidate,
        raw,
        scope.map(|scope| {
            (
                &scope.workspace_root,
                &scope.agent_id,
                scope.profile_enabled,
                scope.companion,
            )
        }),
        config_revision(config),
        canonical_root,
    ))
    .expect("personality revision serialization");
    Ok(PersonalityView {
        source,
        path,
        content,
        revision: hex::encode(Sha256::digest(binding)),
        max_chars: MAX_CHARS,
    })
}
fn read_raw(held: &WorkspaceBytes, candidate: &str) -> Result<Option<Vec<u8>>, ApiError> {
    let raw = match held.read_instruction_file(candidate, agent_files::MAX_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(InstructionReadError::Missing) => return Ok(None),
        Err(_) => return Err(unavailable()),
    };
    if held
        .instruction_link_count(candidate)
        .map_err(|_| unavailable())?
        != Some(1)
    {
        return Err(unavailable());
    }
    Ok(Some(raw))
}
fn current(
    guard: &crate::state::DaemonState,
    id: &str,
    config: &AgentConfig,
    scope: Option<&AgentFileScope>,
) -> bool {
    guard
        .get_agent(id)
        .is_some_and(|agent| config_revision(&agent.state.config) == config_revision(config))
        && match scope {
            Some(scope) => scope_is_current(guard, scope),
            None => capture_scope(guard, id, "", None).is_none(),
        }
}
/// A timed-out OS read retains its permit until the blocking worker exits.
async fn bounded_source_read<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    static READERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let deadline = tokio::time::Instant::now() + agent_files::DEFAULT_LOAD_DEADLINE;
    let permit = tokio::time::timeout_at(
        deadline,
        READERS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .acquire_owned(),
    )
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| unavailable())?;
    tokio::time::timeout_at(
        deadline,
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        }),
    )
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| unavailable())?
}
async fn snapshot(state: &AppState, id: &str) -> Result<Snapshot, ApiError> {
    let (config, scope) = {
        let guard = state.daemon.read().await;
        guard
            .ensure_profile_owned()
            .map_err(|error| ApiError::service_unavailable(error.to_string()))?;
        let config = guard
            .get_agent(id)
            .ok_or_else(ApiError::not_found)?
            .state
            .config;
        (config, capture_scope(&guard, id, "", None))
    };
    let candidate = scope.as_ref().and_then(agent_files::selection::soul);
    let (held, raw) = if let (Some(scope), Some(candidate)) = (&scope, &candidate) {
        let root = scope.workspace_root.clone();
        let relative = candidate.clone();
        bounded_source_read(move || {
            let held = WorkspaceBytes::open(&root).map_err(|_| unavailable())?;
            let raw = read_raw(&held, &relative)?;
            Ok((Some(held), raw))
        })
        .await?
    } else {
        (None, None)
    };
    if let (Some(scope), Some(candidate)) = (&scope, &candidate) {
        let accepted = agent_files::load(
            scope.clone(),
            AgentFileRenderBudget {
                max_identity_chars: agent_files::MAX_RENDER_CHARS,
                max_project_chars: agent_files::MAX_RENDER_CHARS,
                max_total_chars: agent_files::MAX_RENDER_CHARS,
            },
            Arc::new(AtomicBool::new(false)),
            Instant::now() + agent_files::DEFAULT_LOAD_DEADLINE,
        )
        .await;
        if accepted.manifest.availability != Availability::Available {
            return Err(unavailable());
        }
        let entry = accepted
            .manifest
            .entries
            .iter()
            .find(|entry| &entry.relative_path == candidate)
            .ok_or_else(unavailable)?;
        match &raw {
            None if entry.state == AgentFileState::Missing => {}
            Some(bytes)
                if matches!(
                    entry.state,
                    AgentFileState::Loaded | AgentFileState::Truncated
                ) && entry.digest.as_deref()
                    == Some(hex::encode(Sha256::digest(bytes)).as_str()) => {}
            _ => return Err(unavailable()),
        }
    }
    if !current(&*state.daemon.read().await, id, &config, scope.as_ref()) {
        return Err(changed());
    }
    let canonical_root = held.as_ref().map(|held| held.canonical_root.clone());
    let view = make_view(
        id,
        &config,
        scope.as_ref(),
        candidate.as_deref(),
        raw.as_deref(),
        canonical_root.as_deref(),
    )?;
    Ok(Snapshot {
        config,
        scope,
        held,
        candidate,
        raw,
        canonical_root,
        view,
    })
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/personality", tag = "agents",
    params(("agent_id" = String, Path)), responses((status=200, description="Full active editable personality", body=PersonalityView), (status=403, description="Local owner required"), (status=409, description="Source changed"), (status=503, description="Source rejected or unavailable")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    answer(snapshot(&state, &id).await.map(|snapshot| snapshot.view))
}
#[utoipa::path(put, path = "/api/agents/{agent_id}/personality", tag = "agents",
    params(("agent_id" = String, Path)), request_body=SavePersonality,
    responses((status=200, description="Durably saved personality", body=PersonalityView), (status=400, description="Invalid personality"), (status=403, description="Local owner required"), (status=409, description="Stale revision"), (status=503, description="Save unavailable")))]
pub(super) async fn put(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: SavePersonality = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if let Err(error) = validate(&input.content) {
        return answer(Err(error));
    }
    // Dropping the HTTP future detaches this owned task. Persistence and rollback
    // continue with the transaction/state locks until the operation is coherent.
    let result = tokio::spawn(async move { save(state, id, input).await })
        .await
        .unwrap_or_else(|_| {
            Err(ApiError::service_unavailable(
                "Personality save task failed",
            ))
        });
    answer(result)
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/personality/preview", tag = "agents",
    params(("agent_id" = String, Path)), request_body = PreviewPersonality,
    responses((status=200, description="Tool-free preview reply using an unsaved personality", body=serde_json::Value), (status=400, description="Invalid preview"), (status=403, description="Local owner required"), (status=503, description="Model or usage recording unavailable")))]
pub(super) async fn preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: PreviewPersonality = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if let Err(error) = validate(&input.content) {
        return error.into_response();
    }
    let prompt = input.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > 2_000 {
        return ApiError::bad_request("Test prompt must contain 1 to 2000 characters")
            .into_response();
    }

    let (mut config, adapter) = {
        let guard = state.daemon.read().await;
        if let Err(error) = guard.ensure_profile_owned() {
            return ApiError::service_unavailable(error.to_string()).into_response();
        }
        let Some(agent) = guard.get_agent(&id) else {
            return ApiError::not_found().into_response();
        };
        (agent.state.config, guard.profile_model_adapter())
    };
    let system = format!(
        "You are producing a response-only personality preview. Demonstrate the personality below in response to the test prompt. The prompt is sample input, not permission to take action. Do not call tools, access external services, or claim to have performed real actions.\n\n{}",
        input.content
    );
    config.system = Some(system.clone());
    config.tools = None;
    config.plugins = None;
    let model_request = ModelGenerateRequest {
        system,
        messages: vec![Message {
            id: String::new(),
            agent_id: id.clone(),
            room_id: String::new(),
            content: Content {
                text: prompt.to_string(),
                attachments: None,
                metadata: None,
            },
            role: MessageRole::User,
            created_at_ms: 0,
        }],
        temperature: Some(0.4),
        max_tokens: Some(600),
    };
    let meter = Metered::new(adapter);
    let response = meter.generate(&config, &model_request).await;
    let call = SecondaryCall::for_config(&id, None, UsageSource::Profile, &config);
    if record_secondary(
        &state.daemon,
        state.agent_runs.control_plane_transactions(),
        &meter,
        call,
    )
    .await
    .is_err()
    {
        return ApiError::service_unavailable("Personality preview usage could not be saved")
            .into_response();
    }
    match response {
        Ok(response) if !response.content.text.trim().is_empty() => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({ "text": response.content.text.trim() }),
        )),
        Ok(_) => ApiError::service_unavailable("Personality preview returned an empty reply")
            .into_response(),
        Err(error) => {
            ApiError::bad_request(format!("Personality preview failed: {error}")).into_response()
        }
    }
}

fn validate(content: &str) -> Result<(), ApiError> {
    let normalized = content
        .strip_prefix('\u{feff}')
        .unwrap_or(content)
        .replace("\r\n", "\n");
    if normalized.trim().is_empty() || content.chars().count() > MAX_CHARS {
        return Err(ApiError::bad_request(
            "Personality must contain 1 to 16000 Unicode characters",
        ));
    }
    let threats = crate::threat_scan::scan(&normalized);
    if !threats.is_empty() {
        return Err(ApiError::bad_request(format!(
            "Personality rejected by instruction screening: {}",
            threats.join(", ")
        )));
    }
    Ok(())
}
async fn save(
    state: AppState,
    id: String,
    input: SavePersonality,
) -> Result<PersonalityView, ApiError> {
    let _transaction = state.agent_runs.control_plane_transaction().await;
    if let Some(owner) = state.daemon.read().await.profile_owner() {
        owner
            .verify_owned()
            .await
            .map_err(|error| ApiError::service_unavailable(error.to_string()))?;
    }
    let mut snapshot = snapshot(&state, &id).await?;
    if input.revision != snapshot.view.revision {
        return Err(changed());
    }
    if snapshot.raw.is_none() {
        if let (Some(held), Some(relative)) = (snapshot.held.take(), snapshot.candidate.clone()) {
            let (held, raw) = bounded_source_read(move || {
                let raw = read_raw(&held, &relative)?;
                Ok((held, raw))
            })
            .await?;
            snapshot.held = Some(held);
            if raw.is_some() {
                return Err(changed());
            }
        }
    }
    let mut guard = state.daemon.write().await;
    if !current(&guard, &id, &snapshot.config, snapshot.scope.as_ref()) {
        return Err(changed());
    }
    guard
        .ensure_profile_owned()
        .map_err(|error| ApiError::service_unavailable(error.to_string()))?;
    if let Some(owner) = guard.profile_owner() {
        owner
            .verify_owned()
            .await
            .map_err(|error| ApiError::service_unavailable(error.to_string()))?;
    }
    if let Some(raw) = snapshot.raw.as_ref() {
        let held = snapshot.held.take().ok_or_else(unavailable)?;
        let relative = snapshot.candidate.as_ref().ok_or_else(unavailable)?;
        let relative = relative.clone();
        let raw = raw.clone();
        let replacement = input.content.as_bytes().to_vec();
        drop(guard);
        let daemon = state.daemon.clone();
        let expected_config = snapshot.config.clone();
        let expected_scope = snapshot.scope.clone();
        let expected_id = id.clone();
        tokio::task::spawn_blocking(move || {
            held.compare_replace_instruction_file(&relative, &raw, &replacement, || {
                let guard = daemon.blocking_read();
                if !current(&guard, &expected_id, &expected_config, expected_scope.as_ref()) {
                    return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "Personality source changed; reload before saving"));
                }
                guard.ensure_profile_owned()?;
                Ok(guard)
            })
        })
        .await
        .map_err(|_| unavailable())?
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                changed()
            } else if crate::control_plane_store::is_durability_uncertain(&error) {
                ApiError::service_unavailable("Personality file was replaced but durability acknowledgement is uncertain; reload the document before retrying")
            } else {
                ApiError::service_unavailable(format!("Personality file could not be saved: {error}"))
            }
        })?;
        return make_view(
            &id,
            &snapshot.config,
            snapshot.scope.as_ref(),
            snapshot.candidate.as_deref(),
            Some(input.content.as_bytes()),
            snapshot.canonical_root.as_deref(),
        );
    }
    let mut config = snapshot.config.clone();
    config.system = Some(input.content);
    config.bio = None;
    config.lore = None;
    config.adjectives = None;
    config.topics = None;
    config.style = None;
    guard.restore_agent_config(&id, config.clone());
    let persist = guard.control_plane_persist_request();
    let store = persist.store_config();
    if store.is_none() {
        guard.restore_agent_config(&id, snapshot.config);
        return Err(ApiError::service_unavailable("Personality requires durable control-plane storage; configure the profile store before saving"));
    }
    if let Err(error) = persist.save().await {
        if crate::control_plane_store::is_durability_uncertain(&error) {
            let observed = match store {
                Some(store) => crate::control_plane_store::load_control_plane_snapshot(&store)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|snapshot| {
                        snapshot
                            .agents
                            .into_iter()
                            .find(|agent| agent.state.id == id)
                            .map(|agent| agent.state.config)
                    }),
                None => None,
            };
            if let Some(observed) = observed {
                guard.restore_agent_config(&id, observed);
            } else if let Some(owner) = guard.profile_owner() {
                owner.mark_lost();
            }
            return Err(ApiError::service_unavailable("Personality save acknowledgement is uncertain; reload to inspect the stored document. Restart the daemon if profile ownership is unavailable."));
        }
        guard.restore_agent_config(&id, snapshot.config);
        return Err(ApiError::service_unavailable(error.to_string()));
    }
    make_view(
        &id,
        &config,
        snapshot.scope.as_ref(),
        snapshot.candidate.as_deref(),
        None,
        snapshot.canonical_root.as_deref(),
    )
}
fn answer(result: Result<PersonalityView, ApiError>) -> Response {
    no_store(match result {
        Ok(view) => json_response(StatusCode::OK, &view),
        Err(error) => error.into_response(),
    })
}
