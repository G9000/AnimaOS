//! Manual-reset events for overlapped I/O.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle};
use std::ptr::null;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Threading::CreateEventW;

use crate::handle::owned_from_creation;

/// An anonymous manual-reset event that starts unsignaled.
#[derive(Debug)]
pub(crate) struct ManualResetEvent(OwnedHandle);

impl ManualResetEvent {
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: no security attributes and no name are passed; the integer
        // flags request a manual-reset event that starts unsignaled.
        let handle = unsafe { CreateEventW(null(), 1, 0, null()) };
        Ok(Self(owned_from_creation(handle)?))
    }

    pub(crate) fn as_raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }
}

impl AsHandle for ManualResetEvent {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::ManualResetEvent;
    use crate::handle::{is_signaled, wait, WaitOutcome};

    #[test]
    fn a_new_event_is_unsignaled() {
        let event = ManualResetEvent::new().unwrap();
        assert!(!is_signaled(&event));
        assert_eq!(
            wait(&event, Some(Duration::ZERO)).unwrap(),
            WaitOutcome::TimedOut
        );
    }
}
