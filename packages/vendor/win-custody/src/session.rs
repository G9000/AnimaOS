//! Terminal Services sessions.
#![allow(unsafe_code)]

use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTSEnumerateProcessesW, WTSFreeMemory, WTSGetActiveConsoleSessionId,
    WTS_PROCESS_INFOW,
};

use crate::wide::bounded_string;

const MAX_PROCESS_NAME_UNITS: usize = 32_768;
const NO_CONSOLE_SESSION: u32 = 0xFFFF_FFFF;

/// The session the calling process runs in.
pub fn current_session_id() -> io::Result<u32> {
    process_session_id(std::process::id())
}

/// The session the process with `pid` runs in.
pub fn process_session_id(pid: u32) -> io::Result<u32> {
    let mut session_id = 0;
    // SAFETY: plain integer in, local `u32` out.
    if unsafe { ProcessIdToSessionId(pid, &mut session_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(session_id)
}

/// The session attached to the physical console, or `None` while the console
/// is between sessions (during a user switch, for example).
pub fn active_console_session_id() -> Option<u32> {
    // SAFETY: the call takes no arguments.
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    (session != NO_CONSOLE_SESSION).then_some(session)
}

/// One process as Terminal Services reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProcess {
    /// The process ID.
    pub pid: u32,
    /// The session the process runs in.
    pub session_id: u32,
    /// The image name, or `None` when it was unreadable or exceeded a fixed
    /// bound.
    pub name: Option<String>,
}

/// Every process on the local machine with its session, copied out of the
/// Terminal Services buffer before it is freed.
pub fn session_processes() -> io::Result<Vec<SessionProcess>> {
    let mut processes = null_mut();
    let mut count = 0_u32;
    // SAFETY: a null server handle means the local machine; both out pointers
    // are locals that receive a WTS-allocated array and its length.
    if unsafe { WTSEnumerateProcessesW(null_mut(), 0, 1, &mut processes, &mut count) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let list = WtsProcessList(processes);
    if count == 0 {
        return Ok(Vec::new());
    }
    if list.0.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Terminal Services reported processes without a buffer",
        ));
    }
    // SAFETY: WTS returned `count` contiguous entries in `list.0`, which stays
    // allocated until `list` is dropped after this copy.
    let entries = unsafe { std::slice::from_raw_parts(list.0, count as usize) };
    Ok(entries
        .iter()
        .map(|entry| SessionProcess {
            pid: entry.ProcessId,
            session_id: entry.SessionId,
            // SAFETY: the name pointer belongs to the same WTS allocation and
            // is read within a fixed bound.
            name: unsafe { bounded_string(entry.pProcessName, MAX_PROCESS_NAME_UNITS) },
        })
        .collect())
}

struct WtsProcessList(*mut WTS_PROCESS_INFOW);

impl Drop for WtsProcessList {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the buffer was allocated by WTSEnumerateProcessesW and is
            // freed exactly once.
            unsafe { WTSFreeMemory(self.0.cast()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{current_session_id, session_processes};

    #[test]
    fn the_current_process_appears_with_its_session_and_name() {
        let processes = session_processes().unwrap();
        let own = processes
            .iter()
            .find(|process| process.pid == std::process::id())
            .expect("the enumerating process is listed");
        assert!(own.name.as_deref().is_some_and(|name| !name.is_empty()));
        assert_eq!(own.session_id, current_session_id().unwrap());
    }
}
