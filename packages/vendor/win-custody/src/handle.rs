//! Owned kernel handles and bounded waits.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    HANDLE, INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, WaitForSingleObject};

/// How a wait on a kernel object ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitOutcome {
    /// The object became signaled: a process or thread exited, an event was
    /// set, a mutex was acquired.
    Signaled,
    /// The timeout passed first.
    TimedOut,
    /// A mutex whose previous owner exited without releasing it. The wait
    /// still acquired ownership.
    Abandoned,
}

/// Waits until the object behind `handle` is signaled or `timeout` passes.
/// `None` waits forever.
///
/// Works on anything that exposes a waitable handle: a [`crate::Process`],
/// a [`std::process::Child`], an event or a mutex.
pub fn wait(handle: &impl AsHandle, timeout: Option<Duration>) -> io::Result<WaitOutcome> {
    let milliseconds = timeout.map_or(u32::MAX, clamp_timeout);
    let raw = handle.as_handle().as_raw_handle();
    // SAFETY: `raw` comes from a live borrowed handle that outlives the call;
    // the wait takes no pointers.
    match unsafe { WaitForSingleObject(raw, milliseconds) } {
        WAIT_OBJECT_0 => Ok(WaitOutcome::Signaled),
        WAIT_TIMEOUT => Ok(WaitOutcome::TimedOut),
        WAIT_ABANDONED => Ok(WaitOutcome::Abandoned),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        other => Err(io::Error::other(format!(
            "unexpected wait result {other:#x}"
        ))),
    }
}

/// Whether the object is signaled right now.
#[cfg(test)]
pub(crate) fn is_signaled(handle: &impl AsHandle) -> bool {
    matches!(
        wait(handle, Some(Duration::ZERO)),
        Ok(WaitOutcome::Signaled | WaitOutcome::Abandoned)
    )
}

/// The current-process pseudo-handle. It is always valid and never needs to
/// be closed.
pub(crate) fn current_process() -> HANDLE {
    // SAFETY: GetCurrentProcess takes no arguments and returns a constant
    // pseudo-handle.
    unsafe { GetCurrentProcess() }
}

/// Takes ownership of a handle that a Win32 creation or open call just
/// returned. Creation calls report failure as either null or
/// `INVALID_HANDLE_VALUE`, so both become the call's `GetLastError` value,
/// read before anything else can overwrite it.
pub(crate) fn owned_from_creation(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the handle was just returned by a successful creation or open
    // call, nothing else owns it, and `OwnedHandle` closes it exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Converts a wait duration into the millisecond count Win32 expects, keeping
/// `u32::MAX` (`INFINITE`) reserved for the explicit "wait forever" case.
pub(crate) fn clamp_timeout(timeout: Duration) -> u32 {
    timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::clamp_timeout;

    #[test]
    fn timeouts_never_collapse_into_infinite() {
        assert_eq!(clamp_timeout(Duration::ZERO), 0);
        assert_eq!(clamp_timeout(Duration::from_millis(250)), 250);
        assert_eq!(clamp_timeout(Duration::from_secs(u64::MAX)), u32::MAX - 1);
    }
}
