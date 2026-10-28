//! Named mutexes with explicit security, for machine- or session-wide locks.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle};
use std::time::Duration;

use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};

use crate::handle::{owned_from_creation, wait, WaitOutcome};
use crate::security::SecurityDescriptor;
use crate::wide::wide_null;

/// How a mutex wait ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutexAcquisition {
    /// The mutex is now owned by this thread.
    Acquired,
    /// Owned by this thread after the previous owner exited without
    /// releasing it. Whatever the mutex protects may be inconsistent.
    Abandoned,
    /// The timeout passed first.
    TimedOut,
}

/// A named mutex created, or opened when it already exists, with a security
/// descriptor you choose.
///
/// Ownership belongs to the thread that acquired it, as with every Win32
/// mutex. Dropping the value releases it if this thread owns it.
#[derive(Debug)]
pub struct NamedMutex(OwnedHandle);

impl NamedMutex {
    /// Creates or opens `name` (for example `Global\my-service-setup`). The
    /// descriptor applies only when the mutex is created here. The error
    /// carries the Win32 code, so `ERROR_ACCESS_DENIED` identifies an
    /// existing mutex this process may not open.
    pub fn create(name: &str, descriptor: &SecurityDescriptor) -> io::Result<Self> {
        let name = wide_null(name)?;
        let attributes = descriptor.attributes(false);
        // SAFETY: the attribute block and the descriptor it points at outlive
        // the call, and `name` is NUL-terminated.
        let handle = unsafe { CreateMutexW(attributes.as_ptr(), 0, name.as_ptr()) };
        Ok(Self(owned_from_creation(handle)?))
    }

    /// Waits up to `timeout` for ownership.
    pub fn acquire(&self, timeout: Duration) -> io::Result<MutexAcquisition> {
        Ok(match wait(&self.0, Some(timeout))? {
            WaitOutcome::Signaled => MutexAcquisition::Acquired,
            WaitOutcome::Abandoned => MutexAcquisition::Abandoned,
            WaitOutcome::TimedOut => MutexAcquisition::TimedOut,
        })
    }
}

impl AsHandle for NamedMutex {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        // SAFETY: the handle is live. Releasing a mutex this thread does not
        // own fails with ERROR_NOT_OWNER and changes nothing, which is the
        // intended no-op for a lock that was never acquired.
        unsafe { ReleaseMutex(self.0.as_raw_handle()) };
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MutexAcquisition, NamedMutex};
    use crate::security::{ObjectSecurityDescriptor, OwnedSid, SecurityDescriptor};

    fn descriptor() -> SecurityDescriptor {
        SecurityDescriptor::from_sddl("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)").unwrap()
    }

    #[test]
    fn a_held_mutex_times_out_for_another_thread_and_frees_on_drop() {
        let name = format!(r"Local\win-custody-mutex-{}", std::process::id());
        let first = NamedMutex::create(&name, &descriptor()).unwrap();
        assert_eq!(
            first.acquire(Duration::from_millis(50)).unwrap(),
            MutexAcquisition::Acquired
        );

        let second = NamedMutex::create(&name, &descriptor()).unwrap();
        let waiter = std::thread::spawn(move || second.acquire(Duration::from_millis(50)).unwrap());
        assert_eq!(waiter.join().unwrap(), MutexAcquisition::TimedOut);

        drop(first);
        let third = NamedMutex::create(&name, &descriptor()).unwrap();
        let waiter = std::thread::spawn(move || third.acquire(Duration::from_millis(500)).unwrap());
        assert_eq!(waiter.join().unwrap(), MutexAcquisition::Acquired);
    }

    #[test]
    fn the_created_mutex_carries_the_requested_dacl() {
        let name = format!(r"Local\win-custody-dacl-{}", std::process::id());
        let mutex = NamedMutex::create(&name, &descriptor()).unwrap();
        let queried = ObjectSecurityDescriptor::query_kernel_object(&mutex).unwrap();
        let dacl = queried.dacl().unwrap();
        let system = OwnedSid::from_string("SY").unwrap();
        assert!(dacl.allowed_aces().any(|ace| ace.trustee_matches(&system)));
    }
}
