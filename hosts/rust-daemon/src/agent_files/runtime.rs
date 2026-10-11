//! Host-owned scope capture. Disk work starts only after the state guard is dropped.
use anima_core::{AgentConfig, DataValue};
use sha2::{Digest, Sha256};

use super::AgentFileScope;
use crate::state::DaemonState;

pub(crate) const PROFILE_SETTING: &str = "agentFilesProfileEnabled";

pub(crate) fn config_revision(config: &AgentConfig) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(config).unwrap_or_default())
    )
}

pub(crate) fn capture_scope(
    state: &DaemonState,
    agent_id: &str,
    session_id: &str,
    run_id: Option<&str>,
) -> Option<AgentFileScope> {
    let agent = state.agents.get(agent_id)?;
    let workspace = state.workspace.as_ref()?;
    let settings = agent.config().settings.as_ref();
    let profile_enabled = settings.and_then(|settings| settings.additional.get(PROFILE_SETTING))
        == Some(&DataValue::Bool(true));
    let companion = settings.and_then(|settings| settings.additional.get("workspaceRole"))
        == Some(&DataValue::String("lead".into()));
    Some(AgentFileScope {
        agent_id: agent_id.into(),
        session_id: session_id.into(),
        run_id: run_id.map(str::to_owned),
        workspace_root: workspace.root_path.clone(),
        project_relative: String::new(),
        profile_enabled,
        companion,
        config_revision: config_revision(agent.config()),
    })
}

pub(crate) fn scope_is_current(state: &DaemonState, scope: &AgentFileScope) -> bool {
    capture_scope(
        state,
        &scope.agent_id,
        &scope.session_id,
        scope.run_id.as_deref(),
    )
    .is_some_and(|current| {
        current.workspace_root == scope.workspace_root
            && current.config_revision == scope.config_revision
            && scope.run_id.as_ref().is_none_or(|run_id| {
                state.runs.get(run_id).is_some_and(|run| {
                    run.agent_id == scope.agent_id && run.session_id == scope.session_id
                }) && state
                    .sessions
                    .get(&scope.agent_id, &scope.session_id)
                    .is_some()
            })
    })
}
use super::{AcceptedAgentFiles, Availability};
use crate::{app::SharedDaemonState, routes::ApiError, state::RunBuild};
use anima_core::{AgentFileRenderBudget, CancelSignal, Content, TokenEstimator};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct CancelLoad(Arc<AtomicBool>);
impl Drop for CancelLoad {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn freeze_scope(scope: &mut AgentFileScope, config: &AgentConfig) {
    let additional = config.settings.as_ref().map(|s| &s.additional);
    scope.profile_enabled =
        additional.and_then(|s| s.get(PROFILE_SETTING)) == Some(&DataValue::Bool(true));
    scope.companion =
        additional.and_then(|s| s.get("workspaceRole")) == Some(&DataValue::String("lead".into()));
    scope.config_revision = config_revision(config);
}

/// Admission has one five-second budget, including a single root-change retry.
/// Only this caller installs its snapshot; blocking workers own no daemon state.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn admit_run(
    state: &SharedDaemonState,
    transactions: Arc<tokio::sync::Mutex<()>>,
    agent_id: &str,
    session_id: &str,
    room_id: &str,
    run_id: &str,
    input: &Content,
    config: &AgentConfig,
    cancel: CancelSignal,
    helper_deadline: Option<tokio::time::Instant>,
) -> Result<(RunBuild, Arc<AcceptedAgentFiles>), ApiError> {
    admit_run_with_loader(
        state,
        transactions,
        agent_id,
        session_id,
        room_id,
        run_id,
        input,
        config,
        cancel,
        helper_deadline,
        |scope, budget, flag, deadline| Box::pin(super::load(scope, budget, flag, deadline)),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn admit_run_with_loader<F>(
    state: &SharedDaemonState,
    transactions: Arc<tokio::sync::Mutex<()>>,
    agent_id: &str,
    session_id: &str,
    room_id: &str,
    run_id: &str,
    input: &Content,
    config: &AgentConfig,
    cancel: CancelSignal,
    helper_deadline: Option<tokio::time::Instant>,
    mut loader: F,
) -> Result<(RunBuild, Arc<AcceptedAgentFiles>), ApiError>
where
    F: FnMut(
        AgentFileScope,
        AgentFileRenderBudget,
        Arc<AtomicBool>,
        std::time::Instant,
    ) -> futures::future::BoxFuture<'static, AcceptedAgentFiles>,
{
    let deadline = std::time::Instant::now() + super::DEFAULT_LOAD_DEADLINE;
    let deadline = helper_deadline.map_or(deadline, |helper| deadline.min(helper.into_std()));
    for attempt in 0..2 {
        let (scope, budget) = {
            let guard = state.read().await;
            let mut scope = capture_scope(&guard, agent_id, session_id, Some(run_id));
            if let Some(scope) = &mut scope {
                freeze_scope(scope, config);
            }
            let factor = guard
                .sessions
                .get(agent_id, session_id)
                .and_then(|s| s.context_calibration_permille)
                .map_or(1.0, |p| f64::from(p) / 1000.0);
            let estimator = TokenEstimator::new(factor.max(1.0));
            let available = crate::sessions::context::ContextBudget::for_config(config)
                .history_tokens(estimator.text_tokens(&input.text))
                .saturating_sub(
                    guard
                        .sessions
                        .get(agent_id, session_id)
                        .and_then(|session| session.summary.as_ref())
                        .map_or(0, |summary| estimator.text_tokens(&summary.text)),
                );
            let chars = ((available as f64 / estimator.calibration()).floor() as u64)
                .saturating_mul(anima_core::CHARS_PER_TOKEN)
                .min(super::MAX_RENDER_CHARS as u64) as usize;
            (
                scope,
                AgentFileRenderBudget {
                    max_identity_chars: chars,
                    max_project_chars: chars,
                    max_total_chars: chars,
                },
            )
        };
        let mut accepted = if let Some(scope) = &scope {
            let flag = Arc::new(AtomicBool::new(false));
            let _signal_on_drop = CancelLoad(flag.clone());
            tokio::select! { biased;
                () = cancel.cancelled() => AcceptedAgentFiles::empty(Availability::Available),
                accepted = loader(scope.clone(), budget, flag, deadline) => accepted,
            }
        } else {
            AcceptedAgentFiles::empty(Availability::UnavailableWorkspace)
        };
        if helper_deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            cancel.cancel_with(anima_core::CancelCause::HelperDeadline);
        }
        let transaction = transactions.clone().lock_owned().await;
        let (build, previous, persist) = {
            let mut guard = state.write().await;
            if !guard.agents.contains_key(agent_id)
                || guard.sessions.get(agent_id, session_id).is_none()
                || !guard.runs.get(run_id).is_some_and(|r| {
                    r.agent_id == agent_id && r.session_id == session_id && !r.status.is_terminal()
                })
            {
                return Err(ApiError::not_found());
            }
            let current_root = guard.workspace.as_ref().map(|w| &w.root_path);
            let root_matches = current_root == scope.as_ref().map(|s| &s.workspace_root);
            if !cancel.is_cancelled() && !root_matches {
                if attempt == 0 && std::time::Instant::now() < deadline {
                    continue;
                }
                accepted = AcceptedAgentFiles::empty(Availability::StaleScope);
            }
            if cancel.is_cancelled() {
                accepted = AcceptedAgentFiles::empty(Availability::Available);
            }
            let build = guard
                .build_run_runtime_with_files(
                    agent_id,
                    room_id,
                    input,
                    Some(config),
                    &accepted.rendered,
                )
                .ok_or_else(ApiError::not_found)?;
            let session = guard
                .sessions
                .get_mut(agent_id, session_id)
                .ok_or_else(ApiError::not_found)?;
            let previous = crate::sessions::context::mark_context_trimmed(
                session,
                build.context.trimmed_through.as_deref(),
                anima_core::primitives::now_millis(),
            );
            let changed = session.context_trimmed != previous;
            (
                build,
                previous,
                changed.then(|| guard.control_plane_persist_request()),
            )
        };
        if let Some(persist) = persist {
            if let Err(error) = persist.save().await {
                if let Some(session) = state.write().await.sessions.get_mut(agent_id, session_id) {
                    session.context_trimmed = previous;
                }
                return Err(ApiError::service_unavailable(error.to_string()));
            }
        }
        drop(transaction);
        return Ok((build, Arc::new(accepted)));
    }
    unreachable!("the second scope mismatch accepts an empty stale snapshot")
}
