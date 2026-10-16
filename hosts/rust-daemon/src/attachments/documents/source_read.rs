//! One shared, non-queued pool for physically retained document source reads.
use std::sync::{Arc, OnceLock};

use anima_core::CancelSignal;
use tokio::{sync::Semaphore, time::Instant};

use super::worker::WorkerFailure;

static SOURCE_READ_SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(crate) fn shared_read_slots() -> Arc<Semaphore> {
    SOURCE_READ_SLOTS
        .get_or_init(|| Arc::new(Semaphore::new(4)))
        .clone()
}

pub(crate) async fn read_retained<T: Send + 'static>(
    read: impl FnOnce() -> T + Send + 'static,
    cancel: &CancelSignal,
    deadline: Instant,
) -> Result<T, WorkerFailure> {
    read_retained_with_slots(read, cancel, deadline, shared_read_slots()).await
}

/// Explicit pool injection lets deterministic tests exercise capacity without
/// taking leases from unrelated tests. Production callers use the shared pool.
pub(crate) async fn read_retained_with_slots<T: Send + 'static>(
    read: impl FnOnce() -> T + Send + 'static,
    cancel: &CancelSignal,
    deadline: Instant,
    slots: Arc<Semaphore>,
) -> Result<T, WorkerFailure> {
    check(cancel, deadline)?;
    let lease = slots.try_acquire_owned().map_err(|_| WorkerFailure::Busy)?;
    check(cancel, deadline)?;
    let reading = tokio::task::spawn_blocking(move || {
        // Neither cancelling the await nor dropping its future releases physical
        // capacity. The lease dies only after this blocking operation retires.
        let _lease = lease;
        read()
    });
    let result = tokio::select! { biased;
        () = cancel.cancelled() => Err(WorkerFailure::Cancelled),
        () = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
        result = reading => result.map_err(|_| WorkerFailure::Unavailable),
    };
    check(cancel, deadline)?;
    result
}

fn check(cancel: &CancelSignal, deadline: Instant) -> Result<(), WorkerFailure> {
    if cancel.is_cancelled() {
        return Err(WorkerFailure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(WorkerFailure::TimedOut);
    }
    Ok(())
}
