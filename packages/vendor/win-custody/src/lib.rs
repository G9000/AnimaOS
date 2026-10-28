//! Take custody of Windows child processes.
//!
//! A parent that launches helpers on Windows has to decide four things for
//! every child, and the Win32 defaults get each of them wrong for a service or
//! a launcher:
//!
//! * **What it inherits.** [`Launch`] passes an explicit
//!   `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, so the child receives exactly the
//!   handles you name and nothing else that happens to be inheritable in your
//!   process. The standard library cannot do this on stable Rust.
//! * **How long it lives.** [`Job`] puts the child in a kill-on-close job
//!   object before its first instruction runs, so it cannot outlive you, even
//!   when your process is killed.
//! * **Who it runs as.** [`PrimaryToken`] lets a service start the child in
//!   another session, as the logged-on user or as itself on that user's
//!   desktop.
//! * **Who can reach it.** [`SecurityDescriptor`] turns SDDL into the access
//!   rules for the pipes ([`PipeServer`]), mutexes ([`NamedMutex`]), jobs and
//!   directory trees you share with the child, and [`PipeServer`] tells you the
//!   process ID of whoever connected.
//!
//! # Not a sandbox
//!
//! Nothing here confines what a child can do with the rights its token
//! already has. A job object ties the child's lifetime and resource use to
//! yours; it does not stop the child from reading files, writing the registry
//! or opening network connections. Run untrusted code in an AppContainer or
//! a separate low-privilege account instead.
//!
//! # Example
//!
//! ```no_run
//! # #[cfg(windows)]
//! # fn main() -> std::io::Result<()> {
//! use std::time::Duration;
//! use win_custody::{Job, JobLimits, Launch, WaitOutcome};
//!
//! let job = Job::create(&JobLimits::new().kill_on_close(true))?;
//! let child = Launch::new(r"C:\Windows\System32\cmd.exe")
//!     .args(["/d", "/c", "exit 7"])
//!     .spawn_in_job(&job)?;
//! assert_eq!(child.wait(Some(Duration::from_secs(10)))?, WaitOutcome::Signaled);
//! assert_eq!(child.exit_code()?, Some(7));
//! # Ok(())
//! # }
//! # #[cfg(not(windows))]
//! # fn main() {}
//! ```
//!
//! # Where the `unsafe` lives
//!
//! `unsafe_code` is denied for the crate. Only the modules that call Win32
//! opt back in, each at its top, and every `unsafe` block there carries a
//! `SAFETY:` comment naming the invariant it relies on. Raw handles,
//! pointers and buffer lengths never cross the public API: handles come and
//! go as [`std::os::windows::io::OwnedHandle`] and
//! [`std::os::windows::io::BorrowedHandle`], and buffers Windows fills are
//! bounds-checked before they are read.
//!
//! On targets other than Windows the crate compiles to nothing.
#![cfg(windows)]

mod bounded_read;
mod event;
mod handle;
mod job;
mod launch;
mod mutex;
mod pipe;
mod process;
mod security;
mod session;
mod token;
mod wide;

pub use handle::{wait, WaitOutcome};
pub use job::{Job, JobLimits};
pub use launch::{anonymous_pipe, null_device, read_to_end_bounded, Launch, SuspendedChild};
pub use mutex::{MutexAcquisition, NamedMutex};
pub use pipe::{PipeAccess, PipeClient, PipeDirection, PipeError, PipeServer, PipeWait};
pub use process::{is_elevated, Process, ProcessAccess};
pub use security::{
    current_token_is_member_of, current_user_sid_string, AclValidationError, AllowedAce,
    ObjectSecurityDescriptor, OwnedSid, PrivilegeGuard, SecurityAclError, SecurityDescriptor,
    ValidatedDacl, ValidatedSid,
};
pub use session::{
    active_console_session_id, current_session_id, process_session_id, session_processes,
    SessionProcess,
};
pub use token::PrimaryToken;

// Builds the README examples as doctests, so the front page cannot drift from
// the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
