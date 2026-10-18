//! Process-tree custody for programs a host launches on the owner's behalf.
//! A caller may reuse capacity only after [`Custody::retire`] proves the whole
//! tree is gone:
//! - Windows: a Job Object; no descendant can leave it.
//! - Linux with a delegated cgroup v2: a fresh process group inside a private
//!   leaf cgroup; `cgroup.kill` also reaches descendants that left the group.
//! - Other Unix (macOS, Linux without delegation): a fresh process group. A
//!   descendant that calls `setsid` leaves it and is not contained.
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde::Serialize;
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;

/// Bound on proving an already-killed tree is empty before reporting failure.
#[cfg(unix)]
const RETIRE_LIMIT: Duration = Duration::from_secs(60);

/// How a launched program's tree is contained. Only a Job Object or cgroup
/// also covers descendants that leave their process group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Containment {
    JobObject,
    Cgroup,
    ProcessGroup,
    /// Windows app sessions: the tree is killed as it stands at Stop; a
    /// descendant whose parent already exited is missed.
    ProcessTree,
}

pub struct Custody {
    #[cfg(unix)]
    pgid: Option<nix::unistd::Pid>,
    #[cfg(target_os = "linux")]
    cgroup: Option<cgroup::Leaf>,
}

impl Custody {
    /// Builds the containment and the command that runs `executable` inside it.
    /// Callers add cwd, environment and stdio, then [`Custody::wrap`] it.
    pub fn prepare(executable: &Path, args: Vec<String>) -> (Self, tokio::process::Command) {
        #[cfg(target_os = "linux")]
        if let Some(leaf) = cgroup::Leaf::create() {
            // The shell joins the leaf, then execs the worker under the same
            // PID, so no worker instruction runs outside the cgroup.
            let mut command = tokio::process::Command::new("/bin/sh");
            command
                .arg("-c")
                .arg(r#"echo "$$" > "$0" || exit 125; exec "$@""#)
                .arg(leaf.procs())
                .arg(executable)
                .args(args);
            let custody = Self {
                pgid: None,
                cgroup: Some(leaf),
            };
            return (custody, command);
        }
        let mut command = tokio::process::Command::new(executable);
        command.args(args);
        let custody = Self {
            #[cfg(unix)]
            pgid: None,
            #[cfg(target_os = "linux")]
            cgroup: None,
        };
        (custody, command)
    }

    pub fn containment(&self) -> Containment {
        #[cfg(target_os = "linux")]
        if self.cgroup.is_some() {
            return Containment::Cgroup;
        }
        if cfg!(windows) {
            Containment::JobObject
        } else {
            Containment::ProcessGroup
        }
    }

    pub fn wrap(&self, command: &mut CommandWrap) {
        command.wrap(KillOnDrop);
        #[cfg(windows)]
        {
            use process_wrap::tokio::{CreationFlags, JobObject};
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let mut flags = CreationFlags(Default::default());
            flags.0 .0 = CREATE_NO_WINDOW;
            command.wrap(flags).wrap(JobObject);
        }
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
    }

    /// Records the group leader. The PID is unavailable once the leader is reaped.
    pub fn attach(&mut self, child: &dyn ChildWrapper) {
        #[cfg(unix)]
        {
            self.pgid = child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .map(nix::unistd::Pid::from_raw);
        }
        #[cfg(not(unix))]
        let _ = child;
    }

    /// Records a leader spawned outside process-wrap, such as a PTY child. It
    /// must already lead its own process group (a PTY child calls `setsid`).
    pub fn attach_leader(&mut self, pid: u32) {
        #[cfg(unix)]
        {
            self.pgid = i32::try_from(pid).ok().map(nix::unistd::Pid::from_raw);
        }
        #[cfg(not(unix))]
        let _ = pid;
    }

    /// Signals the whole tree without waiting, for drop paths that cannot
    /// await [`Custody::retire_group`].
    pub fn kill_now(&self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
        }
        #[cfg(target_os = "linux")]
        if let Some(leaf) = &self.cgroup {
            leaf.kill();
        }
    }

    /// [`Custody::retire`] for a leader recorded with [`Custody::attach_leader`].
    /// `reaped` resolves once the caller has waited on the leader.
    #[cfg(unix)]
    pub async fn retire_group(&self, reaped: impl std::future::Future<Output = bool>) -> bool {
        use nix::{
            errno::Errno,
            sys::signal::{killpg, Signal},
        };
        let Some(pgid) = self.pgid else {
            return false;
        };
        // ESRCH: already gone; EPERM: unreaped zombies on macOS (see retire).
        let killed = match killpg(pgid, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH | Errno::EPERM) => true,
            Err(_) => false,
        };
        self.await_group_gone(pgid, killed, reaped.await).await
    }

    /// Kills members that left the group (cgroup only), then waits until no
    /// member of the tree is left.
    #[cfg(unix)]
    async fn await_group_gone(&self, pgid: nix::unistd::Pid, killed: bool, reaped: bool) -> bool {
        use nix::{errno::Errno, sys::signal::killpg};
        #[cfg(target_os = "linux")]
        let killed = killed && self.cgroup.as_ref().is_none_or(cgroup::Leaf::kill);
        let deadline = tokio::time::Instant::now() + RETIRE_LIMIT;
        loop {
            let group_empty = killpg(pgid, None) == Err(Errno::ESRCH);
            #[cfg(target_os = "linux")]
            let group_empty = group_empty
                && self
                    .cgroup
                    .as_ref()
                    .is_none_or(|leaf| leaf.populated() == Some(false));
            if group_empty {
                #[cfg(target_os = "linux")]
                if let Some(leaf) = &self.cgroup {
                    leaf.remove();
                }
                return killed && reaped;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The cgroup shell wrapper exits 125, before any output, if it could not
    /// join; the program itself never ran.
    pub fn join_failed(&self, exit_code: i32, produced_output: bool) -> bool {
        self.containment() == Containment::Cgroup && exit_code == 125 && !produced_output
    }

    /// Kills the tree and resolves true only once every member is proven gone.
    pub async fn retire(&self, child: &mut Box<dyn ChildWrapper>) -> bool {
        #[cfg(windows)]
        {
            let killed = child.start_kill().is_ok();
            child.wait().await.is_ok() && killed
        }
        #[cfg(unix)]
        {
            use nix::errno::Errno;
            let Some(pgid) = self.pgid else {
                return false;
            };
            // A group whose members all exited already reports ESRCH; macOS
            // reports EPERM while they are unreaped zombies. Either way
            // await_group_gone proves the group empty before success.
            let killed = match child.start_kill() {
                Ok(()) => true,
                Err(error) => matches!(
                    error.raw_os_error(),
                    Some(code) if code == Errno::ESRCH as i32 || code == Errno::EPERM as i32
                ),
            };
            let reaped = child.wait().await.is_ok();
            self.await_group_gone(pgid, killed, reaped).await
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = child;
            false
        }
    }
}

#[cfg(target_os = "linux")]
mod cgroup {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    /// A private, empty leaf under the daemon's own cgroup v2 directory.
    pub(super) struct Leaf {
        dir: PathBuf,
    }

    impl Leaf {
        /// None unless the unified hierarchy is mounted, the daemon's cgroup is
        /// delegated to it and the kernel supports `cgroup.kill` (5.14+).
        pub(super) fn create() -> Option<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = Path::new("/sys/fs/cgroup");
            if !root.join("cgroup.controllers").is_file() {
                return None;
            }
            let own = std::fs::read_to_string("/proc/self/cgroup").ok()?;
            let relative = own.lines().find_map(|line| line.strip_prefix("0::"))?;
            let parent = root.join(relative.trim().trim_start_matches('/'));
            // Moving a process needs write access to the common ancestor's
            // procs file; opening it writes nothing.
            std::fs::OpenOptions::new()
                .write(true)
                .open(parent.join("cgroup.procs"))
                .ok()?;
            let dir = parent.join(format!(
                "anima-coding-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).ok()?;
            let leaf = Self { dir };
            if !leaf.dir.join("cgroup.kill").is_file() {
                leaf.remove();
                return None;
            }
            Some(leaf)
        }
        pub(super) fn procs(&self) -> PathBuf {
            self.dir.join("cgroup.procs")
        }
        pub(super) fn kill(&self) -> bool {
            std::fs::write(self.dir.join("cgroup.kill"), "1").is_ok()
        }
        pub(super) fn populated(&self) -> Option<bool> {
            let events = std::fs::read_to_string(self.dir.join("cgroup.events")).ok()?;
            events
                .lines()
                .find_map(|line| line.strip_prefix("populated "))
                .map(|value| value.trim() != "0")
        }
        pub(super) fn remove(&self) {
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    /// Removing a still-populated cgroup fails harmlessly, leaving it for the
    /// kernel to report rather than hiding a live tree.
    impl Drop for Leaf {
        fn drop(&mut self) {
            self.remove();
        }
    }
}
