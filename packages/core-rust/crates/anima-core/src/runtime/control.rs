//! Cooperative control of one run (spec §4.6–§4.7): a stop signal the
//! runtime checks at its checkpoints, and a steering inbox it drains before
//! each model call. Both are shared with the host by cloning.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::primitives::{Content, LockRecover};

/// The error of a stopped run's result (spec §4.6).
pub const RUN_STOPPED_ERROR: &str = "stopped";
/// The result of a requested tool call that never ran because of a stop.
pub const CANCELLED_TOOL_RESULT: &str = "Cancelled before running (stopped by owner)";
/// Marks the partial text of a model call a stop interrupted.
pub const STOPPED_METADATA_KEY: &str = "stopped";
/// Marks an owner message steered into a running run.
pub const STEER_METADATA_KEY: &str = "steer";

/// Why cooperative cancellation was requested. The first cause wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CancelCause {
    OwnerStop = 1,
    HelperDeadline = 2,
    DaemonShutdown = 3,
    ProfileOwnershipLost = 4,
    ExecutionDeadline = 5,
}

pub const HELPER_DEADLINE_TOOL_RESULT: &str = "Cancelled before running (helper deadline reached)";
pub const DAEMON_SHUTDOWN_TOOL_RESULT: &str = "Cancelled before running (daemon shutting down)";
pub const HELPER_DEADLINE_ERROR: &str = "Helper task timed out";
pub const DAEMON_SHUTDOWN_ERROR: &str = "Daemon shutting down";
pub const PROFILE_OWNERSHIP_LOST_ERROR: &str = "Profile ownership lost; effects require review";

#[derive(Debug, Default)]
struct CancelState {
    cause: AtomicU8,
    waiters: Mutex<Vec<Waker>>,
}

/// A one-way stop flag. Cancelling wakes every task waiting on it.
#[derive(Clone, Debug, Default)]
pub struct CancelSignal {
    state: Arc<CancelState>,
}

impl CancelSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancel_with(CancelCause::OwnerStop);
    }

    pub fn cancel_with(&self, cause: CancelCause) {
        if self
            .state
            .cause
            .compare_exchange(0, cause as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let waiters = std::mem::take(&mut *self.state.waiters.lock_recover());
            for waiter in waiters {
                waiter.wake();
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cause().is_some()
    }

    pub fn cause(&self) -> Option<CancelCause> {
        match self.state.cause.load(Ordering::SeqCst) {
            1 => Some(CancelCause::OwnerStop),
            2 => Some(CancelCause::HelperDeadline),
            3 => Some(CancelCause::DaemonShutdown),
            4 => Some(CancelCause::ProfileOwnershipLost),
            5 => Some(CancelCause::ExecutionDeadline),
            _ => None,
        }
    }

    pub fn tool_result(&self) -> &'static str {
        match self.cause() {
            Some(CancelCause::HelperDeadline) => HELPER_DEADLINE_TOOL_RESULT,
            Some(CancelCause::DaemonShutdown) => DAEMON_SHUTDOWN_TOOL_RESULT,
            Some(CancelCause::ProfileOwnershipLost) => {
                "Cancelled before running (profile ownership lost)"
            }
            Some(CancelCause::ExecutionDeadline) => {
                "Cancelled before running (shared execution deadline reached)"
            }
            _ => CANCELLED_TOOL_RESULT,
        }
    }

    pub fn run_error(&self) -> &'static str {
        match self.cause() {
            Some(CancelCause::HelperDeadline) => HELPER_DEADLINE_ERROR,
            Some(CancelCause::DaemonShutdown) => DAEMON_SHUTDOWN_ERROR,
            Some(CancelCause::ProfileOwnershipLost) => PROFILE_OWNERSHIP_LOST_ERROR,
            Some(CancelCause::ExecutionDeadline) => "Shared execution deadline reached",
            _ => RUN_STOPPED_ERROR,
        }
    }

    /// Resolves once the signal is cancelled (at once if it already is).
    pub fn cancelled(&self) -> CancelWait {
        CancelWait {
            signal: self.clone(),
        }
    }
}

/// The future `CancelSignal::cancelled` returns.
#[derive(Debug)]
pub struct CancelWait {
    signal: CancelSignal,
}

impl Future for CancelWait {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.signal.is_cancelled() {
            return Poll::Ready(());
        }
        {
            let mut waiters = self.signal.state.waiters.lock_recover();
            if !waiters
                .iter()
                .any(|waiter| waiter.will_wake(context.waker()))
            {
                waiters.push(context.waker().clone());
            }
        }
        // `cancel` sets the flag before it takes the waiters, so a cancel
        // racing the registration above is seen here.
        if self.signal.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[derive(Debug, Default)]
struct SteeringState {
    items: VecDeque<Content>,
    closed: bool,
}

/// Owner messages waiting to join a running run (spec §4.7).
#[derive(Clone, Debug, Default)]
pub struct SteeringInbox {
    state: Arc<Mutex<SteeringState>>,
}

impl SteeringInbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `content` for the next model call; gives it back once the
    /// inbox is closed (the run is finishing), so the host can queue a run.
    pub fn push(&self, content: Content) -> Result<(), Content> {
        let mut state = self.state.lock_recover();
        if state.closed {
            return Err(content);
        }
        state.items.push_back(content);
        Ok(())
    }

    /// Everything waiting, oldest first; the inbox is left empty.
    pub fn drain(&self) -> Vec<Content> {
        self.state.lock_recover().items.drain(..).collect()
    }

    /// A copy of everything waiting, oldest first.
    pub fn pending(&self) -> Vec<Content> {
        self.state.lock_recover().items.iter().cloned().collect()
    }

    /// Refuses further items and returns the ones never drained, oldest first.
    pub fn close(&self) -> Vec<Content> {
        let mut state = self.state.lock_recover();
        state.closed = true;
        state.items.drain(..).collect()
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock_recover().closed
    }
}

/// The controls a host holds for one run.
#[derive(Clone, Debug, Default)]
pub struct RunControl {
    pub cancel: CancelSignal,
    pub steering: SteeringInbox,
}

impl RunControl {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod cancellation_cause_tests {
    use super::*;
    #[test]
    fn first_cause_wins_and_owner_wrapper_keeps_its_text() {
        for cause in [
            CancelCause::OwnerStop,
            CancelCause::HelperDeadline,
            CancelCause::DaemonShutdown,
        ] {
            let signal = CancelSignal::new();
            signal.cancel_with(cause);
            signal.cancel();
            signal.cancel_with(CancelCause::DaemonShutdown);
            assert!(signal.is_cancelled());
            assert_eq!(signal.cause(), Some(cause));
        }
        let owner = CancelSignal::new();
        owner.cancel();
        assert_eq!(owner.tool_result(), CANCELLED_TOOL_RESULT);
        assert_eq!(owner.run_error(), RUN_STOPPED_ERROR);
        let helper = CancelSignal::new();
        helper.cancel_with(CancelCause::HelperDeadline);
        assert_eq!(helper.tool_result(), HELPER_DEADLINE_TOOL_RESULT);
        assert_eq!(helper.run_error(), HELPER_DEADLINE_ERROR);
    }
    #[test]
    fn competing_causes_publish_one_complete_value() {
        let signal = CancelSignal::new();
        std::thread::scope(|scope| {
            for cause in [
                CancelCause::OwnerStop,
                CancelCause::HelperDeadline,
                CancelCause::DaemonShutdown,
            ] {
                let signal = signal.clone();
                scope.spawn(move || signal.cancel_with(cause));
            }
        });
        assert!(signal.is_cancelled());
        assert!(signal.cause().is_some());
        let winner = signal.cause();
        signal.cancel();
        assert_eq!(signal.cause(), winner);
    }
}
