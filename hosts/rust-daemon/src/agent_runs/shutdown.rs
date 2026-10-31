//! Graceful shutdown of accepted runs (M3 final fix wave S2-A; spec §4.8).
//!
//! Once shutdown begins, a session's queue starts nothing more: its queued
//! runs stay queued, saved as nothing else, so the next start interrupts them
//! as never started (`restart_before_start`); an accepted run still waiting
//! for its room, slot, or permit gives up the wait and stays queued too; and a
//! new message is refused with 503. The accepted runs already going get a
//! bounded wait to commit, as `JobService::shutdown` finishes its admitted
//! runs. A run still going when the wait ends is logged and left to the
//! restart, which interrupts it (`restart_during_run`).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::warn;

use super::AgentRunCoordinator;
use crate::routes::{ApiError, ApiErrorKind};

/// How long shutdown waits for the accepted runs already going to commit.
pub(crate) const ACCEPTED_RUN_SHUTDOWN_WAIT_MS: u64 = 30_000;

/// `accept_run`'s answer (503) once shutdown has begun.
pub(crate) const DAEMON_SHUTTING_DOWN: &str =
    "The daemon is shutting down; send the message again once it restarts";

/// `accept_run`'s answer (503) while a drain before a restart is under way.
pub(crate) const DAEMON_DRAINING: &str = "Anima is restarting; send the message again in a moment";

#[derive(Debug, Default)]
struct Lifecycle {
    closing: bool,
    /// A drain before a restart (`crate::system`): new messages are refused,
    /// while queued and going runs carry on. It never sets `closing`.
    draining: bool,
    /// Accepted runs a session queue started whose start has not ended.
    going: HashSet<String>,
}

/// Whether shutdown has begun, and the accepted runs going. The lifecycle is
/// shared by a coordinator's clones; its lock is a leaf, never held across
/// an `.await`.
#[derive(Clone)]
pub(super) struct AcceptedRuns {
    lifecycle: Arc<watch::Sender<Lifecycle>>,
    wait: Duration,
}

impl Default for AcceptedRuns {
    fn default() -> Self {
        Self {
            lifecycle: Arc::new(watch::channel(Lifecycle::default()).0),
            wait: Duration::from_millis(ACCEPTED_RUN_SHUTDOWN_WAIT_MS),
        }
    }
}

/// An accepted run its session queue started, going until dropped.
pub(super) struct GoingRun {
    run_id: String,
    lifecycle: Arc<watch::Sender<Lifecycle>>,
}

impl Drop for GoingRun {
    fn drop(&mut self) {
        self.lifecycle.send_modify(|lifecycle| {
            lifecycle.going.remove(&self.run_id);
        });
    }
}

impl AcceptedRuns {
    pub(super) fn is_closing(&self) -> bool {
        self.lifecycle.borrow().closing
    }

    pub(super) fn is_draining(&self) -> bool {
        self.lifecycle.borrow().draining
    }

    fn set_draining(&self, draining: bool) {
        self.lifecycle.send_if_modified(|lifecycle| {
            let changed = lifecycle.draining != draining;
            lifecycle.draining = draining;
            changed
        });
    }

    /// Counts `run_id` as going, or `None` once shutdown has begun. The check
    /// and the count are one step under the lifecycle's lock, so shutdown
    /// either waits for the run or the run never starts.
    pub(super) fn begin(&self, run_id: &str) -> Option<GoingRun> {
        let mut began = false;
        self.lifecycle.send_if_modified(|lifecycle| {
            if lifecycle.closing {
                return false;
            }
            lifecycle.going.insert(run_id.to_string());
            began = true;
            true
        });
        began.then(|| GoingRun {
            run_id: run_id.to_string(),
            lifecycle: Arc::clone(&self.lifecycle),
        })
    }

    /// Resolves once shutdown has begun.
    pub(super) async fn closed(&self) {
        let mut lifecycle = self.lifecycle.subscribe();
        // `self` keeps the sender alive, so this ends only once closing.
        let _ = lifecycle.wait_for(|lifecycle| lifecycle.closing).await;
    }

    /// Begins shutdown, then waits up to the bound for the going runs to
    /// end. Returns the ones still going.
    fn request_shutdown(&self) {
        self.lifecycle
            .send_modify(|lifecycle| lifecycle.closing = true);
    }

    #[cfg(test)]
    async fn close_and_wait(&self) -> Vec<String> {
        self.close_and_wait_until(tokio::time::Instant::now() + self.wait)
            .await
    }

    async fn close_and_wait_until(&self, deadline: tokio::time::Instant) -> Vec<String> {
        let mut lifecycle = self.lifecycle.subscribe();
        self.request_shutdown();
        let ended = tokio::time::timeout_at(deadline, async {
            let _ = lifecycle
                .wait_for(|lifecycle| lifecycle.going.is_empty())
                .await;
        })
        .await;
        if ended.is_ok() {
            return Vec::new();
        }
        let mut left = self
            .lifecycle
            .borrow()
            .going
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        left.sort();
        left
    }
}

/// `accept_run`'s refusal once shutdown has begun, and how an accepted run's
/// admission wait ends then.
pub(super) fn shutting_down() -> ApiError {
    ApiError::daemon_shutting_down(DAEMON_SHUTTING_DOWN)
}

/// `accept_run`'s refusal while draining: a 503 of its own kind, never
/// `shutting_down()`, so no caller takes the message for one accepted and
/// left queued; `is_draining_refusal` tells it apart.
pub(super) fn draining() -> ApiError {
    ApiError::daemon_draining(DAEMON_DRAINING)
}

/// Whether a refusal came from a drain: nothing was accepted, so the caller
/// can leave its message for later.
pub(crate) fn is_draining_refusal(error: &ApiError) -> bool {
    error.kind() == ApiErrorKind::DaemonDraining
}

/// Whether a start ended with `shutting_down()`: its run never started, so
/// its session queue leaves it queued.
pub(crate) fn is_shutting_down(error: &ApiError) -> bool {
    error.kind() == ApiErrorKind::DaemonShuttingDown
}

impl AgentRunCoordinator {
    pub(crate) fn request_shutdown(&self) {
        self.accepted_runs.request_shutdown();
        if let Some(manager) = self.mcp_manager() {
            manager.request_shutdown();
        }
    }

    /// Graceful shutdown's first step (`app::serve_with_state`): see the
    /// module documentation.
    pub(crate) async fn shutdown(&self) {
        self.request_shutdown();
        self.skills().close_evaluations().await;
        crate::task_regressions::TaskRegressionService::new(
            self.state.clone(),
            self.control_plane_transactions(),
            self.work_admission_probe(),
        )
        .close()
        .await;
        self.learning.shutdown().await;
        // Agent runs and retiring evaluation transports share one existing
        // shutdown wait budget; neither adds an unbounded second wait.
        let deadline = tokio::time::Instant::now() + self.accepted_runs.wait;
        let left = self.accepted_runs.close_and_wait_until(deadline).await;
        if !left.is_empty() {
            warn!(
                run_ids = ?left,
                wait_ms = self.accepted_runs.wait.as_millis() as u64,
                "accepted runs were still going when shutdown stopped waiting; the restart interrupts them"
            );
        }
        let evaluation_slots = self.state.read().await.skill_evaluation_slots.clone();
        while evaluation_slots.available_permits() == 0 {
            if tokio::time::timeout_at(deadline, tokio::time::sleep(Duration::from_millis(20)))
                .await
                .is_err()
            {
                warn!("skill evaluation transport did not retire within the shared shutdown deadline; late usage is unconfirmed and work will not replay");
                break;
            }
        }
        let task_slots = self.state.read().await.task_regression_slots.clone();
        while task_slots.available_permits() == 0 {
            if tokio::time::timeout_at(deadline, tokio::time::sleep(Duration::from_millis(20)))
                .await
                .is_err()
            {
                warn!("task comparison transport did not retire within the shared shutdown deadline; uncertain work will not replay");
                break;
            }
        }
        if let Some(manager) = self.mcp_manager() {
            manager.shutdown().await;
        }
    }

    /// A leaf-only probe for producers outside accepted agent runs.
    /// Capturing AcceptedRuns avoids a coordinator/state/service ownership cycle.
    pub(crate) fn work_admission_probe(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let accepted = self.accepted_runs.clone();
        Arc::new(move || {
            let lifecycle = accepted.lifecycle.borrow();
            !lifecycle.draining && !lifecycle.closing
        })
    }

    /// Whether shutdown has begun.
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.accepted_runs.is_closing()
    }

    /// Begins a drain: new messages are refused until `cancel_drain` or
    /// shutdown, while accepted runs, queued ones included, start and finish.
    pub(crate) fn begin_drain(&self) {
        self.accepted_runs.set_draining(true);
    }

    pub(crate) fn cancel_drain(&self) {
        self.accepted_runs.set_draining(false);
    }

    pub(crate) fn is_draining(&self) -> bool {
        self.accepted_runs.is_draining()
    }

    /// The drain refusal, for an entry point that admits new work.
    pub(crate) fn refuse_while_draining(&self) -> Result<(), ApiError> {
        if self.is_draining() {
            return Err(draining());
        }
        Ok(())
    }

    /// A shorter shutdown wait, so tests need not wait thirty seconds.
    #[cfg(test)]
    pub(crate) fn with_accepted_shutdown_wait(mut self, wait: Duration) -> Self {
        self.accepted_runs.wait = wait;
        self
    }
}

#[cfg(test)]
mod typed_shutdown_tests {
    use super::*;
    #[tokio::test]
    async fn quality_skill_evaluation_shutdown_waits_for_physical_retirement() {
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let coordinator =
            AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(1)))
                .with_accepted_shutdown_wait(Duration::from_millis(200));
        let slots = state.read().await.skill_evaluation_slots.clone();
        let physical = slots.clone().try_acquire_owned().unwrap();
        let shutdown = tokio::spawn(async move { coordinator.shutdown().await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(slots.is_closed());
        assert!(
            !shutdown.is_finished(),
            "shutdown must retain its worker retirement wait"
        );
        drop(physical);
        tokio::time::timeout(Duration::from_millis(400), shutdown)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn quality_skill_evaluation_shutdown_shares_the_existing_run_wait_deadline() {
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let coordinator =
            AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(1)))
                .with_accepted_shutdown_wait(Duration::from_millis(300));
        let going = coordinator.accepted_runs.begin("held-run").unwrap();
        let slots = state.read().await.skill_evaluation_slots.clone();
        let physical = slots.clone().try_acquire_owned().unwrap();
        let started = tokio::time::Instant::now();
        coordinator.shutdown().await;
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "retirement must not stack another shutdown budget"
        );
        assert_eq!(slots.available_permits(), 0);
        drop(going);
        drop(physical);
    }
    #[test]
    fn classification_uses_kind_instead_of_display_text() {
        assert!(is_shutting_down(&ApiError::daemon_shutting_down(
            "different display text"
        )));
        assert!(!is_shutting_down(&ApiError::service_unavailable(
            DAEMON_SHUTTING_DOWN
        )));
    }
    #[tokio::test]
    async fn request_shutdown_closes_admission_before_drain_and_is_idempotent() {
        let accepted = AcceptedRuns::default();
        let going = accepted.begin("running").unwrap();
        accepted.request_shutdown();
        accepted.request_shutdown();
        assert!(accepted.begin("new").is_none());
        assert!(accepted.is_closing());
        drop(going);
        assert!(accepted.close_and_wait().await.is_empty());
    }
    #[test]
    fn a_draining_refusal_is_its_own_kind_and_never_a_shutdown() {
        let refusal = draining();
        assert_eq!(
            refusal.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(is_draining_refusal(&refusal));
        assert!(!is_shutting_down(&refusal));
        assert!(!is_draining_refusal(&shutting_down()));
        assert!(!is_draining_refusal(&ApiError::service_unavailable(
            DAEMON_DRAINING
        )));
    }
    #[test]
    fn draining_keeps_queued_runs_starting_and_never_closes() {
        let accepted = AcceptedRuns::default();
        accepted.set_draining(true);
        assert!(accepted.is_draining());
        assert!(!accepted.is_closing());
        assert!(accepted.begin("queued").is_some());
        accepted.set_draining(false);
        assert!(!accepted.is_draining());
    }
}
