//! Primary tokens for starting a child as another user or in another session.
#![allow(unsafe_code)]

use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Security::{
    DuplicateTokenEx, GetTokenInformation, SecurityImpersonation, SetTokenInformation,
    TokenPrimary, TokenSessionId, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows_sys::Win32::System::RemoteDesktop::WTSQueryUserToken;
use windows_sys::Win32::System::Threading::OpenProcessToken;

use crate::handle::{current_process, owned_from_creation};
use crate::process::token_is_elevated;

/// The rights every primary token here is opened or duplicated with: enough
/// to query it, start a process with it and move it to another session.
const PRIMARY_RIGHTS: u32 = TOKEN_QUERY
    | TOKEN_DUPLICATE
    | TOKEN_ASSIGN_PRIMARY
    | TOKEN_ADJUST_SESSIONID
    | TOKEN_ADJUST_DEFAULT;

/// A primary access token that [`crate::Launch::as_user`] can start a child
/// with.
#[derive(Debug)]
pub struct PrimaryToken(OwnedHandle);

impl PrimaryToken {
    /// A primary copy of this process's own token.
    ///
    /// A service running as LocalSystem uses this, followed by
    /// [`set_session_id`](Self::set_session_id), to start a helper as
    /// LocalSystem on an interactive user's desktop.
    pub fn duplicate_current_process() -> io::Result<Self> {
        let mut own = null_mut();
        // SAFETY: the process pseudo-handle is always valid and `own` is a
        // local out pointer whose handle is owned immediately below.
        if unsafe { OpenProcessToken(current_process(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut own) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let own = owned_from_creation(own)?;
        let mut duplicate = null_mut();
        // SAFETY: `own` is a live token opened with TOKEN_DUPLICATE; no
        // security attributes are passed and `duplicate` is a local out value.
        let duplicated = unsafe {
            DuplicateTokenEx(
                own.as_raw_handle(),
                PRIMARY_RIGHTS,
                null(),
                SecurityImpersonation,
                TokenPrimary,
                &mut duplicate,
            )
        };
        if duplicated == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(owned_from_creation(duplicate)?))
    }

    /// The primary token of the user logged on to `session_id`, through
    /// `WTSQueryUserToken`.
    ///
    /// Only a LocalSystem caller holding `SeTcbPrivilege` can do this; anyone
    /// else gets `ERROR_PRIVILEGE_NOT_HELD` (1314). A session with no logged-on
    /// user reports `ERROR_NO_TOKEN` (1008).
    pub fn for_session_user(session_id: u32) -> io::Result<Self> {
        let mut token = null_mut();
        // SAFETY: plain integer in, local out pointer whose handle is owned
        // immediately below.
        if unsafe { WTSQueryUserToken(session_id, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(owned_from_creation(token)?))
    }

    /// The Terminal Services session a process started with this token runs
    /// in.
    pub fn session_id(&self) -> io::Result<u32> {
        let mut session = 0_u32;
        let mut returned = 0_u32;
        // SAFETY: the token is live; the buffer pointer and length describe
        // exactly the local `u32`.
        let queried = unsafe {
            GetTokenInformation(
                self.raw(),
                TokenSessionId,
                (&mut session as *mut u32).cast(),
                size_of::<u32>() as u32,
                &mut returned,
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(session)
    }

    /// Moves the token to `session_id`, so a child started with it appears in
    /// that session.
    ///
    /// Windows requires `SeTcbPrivilege` enabled in the caller's token, which
    /// in practice means a LocalSystem service. Anyone else gets
    /// `ERROR_PRIVILEGE_NOT_HELD` (1314).
    pub fn set_session_id(&mut self, session_id: u32) -> io::Result<()> {
        // SAFETY: the token is live and was opened with
        // TOKEN_ADJUST_SESSIONID | TOKEN_ADJUST_DEFAULT; the buffer pointer and
        // length describe exactly the local `u32`.
        let set = unsafe {
            SetTokenInformation(
                self.raw(),
                TokenSessionId,
                (&session_id as *const u32).cast(),
                size_of::<u32>() as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Whether the token is elevated (a full administrator token).
    pub fn is_elevated(&self) -> io::Result<bool> {
        token_is_elevated(self.raw())
    }

    fn raw(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

impl AsHandle for PrimaryToken {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

impl AsRawHandle for PrimaryToken {
    fn as_raw_handle(&self) -> RawHandle {
        self.raw()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{ERROR_NO_TOKEN, ERROR_PRIVILEGE_NOT_HELD};

    use super::PrimaryToken;
    use crate::handle::WaitOutcome;
    use crate::job::{Job, JobLimits};
    use crate::launch::Launch;
    use crate::session::current_session_id;

    fn cmd() -> std::path::PathBuf {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set");
        std::path::Path::new(&root).join("System32").join("cmd.exe")
    }

    fn running_as_system() -> bool {
        crate::security::current_user_sid_string().unwrap() == "S-1-5-18"
    }

    #[test]
    fn a_duplicate_of_the_own_token_stays_in_the_current_session() {
        let token = PrimaryToken::duplicate_current_process().unwrap();
        assert_eq!(token.session_id().unwrap(), current_session_id().unwrap());
        assert_eq!(
            token.is_elevated().unwrap(),
            crate::process::is_elevated().unwrap()
        );
    }

    #[test]
    fn moving_a_token_needs_tcb_unless_running_as_system() {
        let mut token = PrimaryToken::duplicate_current_process().unwrap();
        let session = token.session_id().unwrap();
        match token.set_session_id(session) {
            Ok(()) => assert!(running_as_system()),
            Err(error) => {
                assert!(!running_as_system());
                assert_eq!(error.raw_os_error(), Some(ERROR_PRIVILEGE_NOT_HELD as i32));
            }
        }
    }

    #[test]
    fn querying_a_session_user_needs_tcb_unless_running_as_system() {
        let session = current_session_id().unwrap();
        match PrimaryToken::for_session_user(session) {
            Ok(token) => {
                assert!(running_as_system());
                assert_eq!(token.session_id().unwrap(), session);
            }
            Err(error) if running_as_system() => {
                assert_eq!(error.raw_os_error(), Some(ERROR_NO_TOKEN as i32));
            }
            Err(error) => {
                assert_eq!(error.raw_os_error(), Some(ERROR_PRIVILEGE_NOT_HELD as i32));
            }
        }
    }

    #[test]
    fn a_child_started_with_the_own_token_runs_and_exits() {
        let token = PrimaryToken::duplicate_current_process().unwrap();
        let job = Job::create(&JobLimits::new().kill_on_close(true)).unwrap();
        let child = Launch::new(cmd())
            .args(["/d", "/c", "exit 9"])
            .as_user(&token)
            .spawn_in_job(&job)
            .unwrap();
        assert_eq!(
            child.wait(Some(Duration::from_secs(30))).unwrap(),
            WaitOutcome::Signaled
        );
        assert_eq!(child.exit_code().unwrap(), Some(9));
    }
}
