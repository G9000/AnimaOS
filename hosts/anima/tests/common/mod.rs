//! Private temporary homes and detached-launcher output capture.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Owns a private home without precreating an Anima profile.
#[derive(Debug)]
pub struct PrivateHome {
    _container: tempfile::TempDir,
    path: PathBuf,
}

impl PrivateHome {
    /// Creates an empty home with private permissions from its initial creation.
    pub fn new() -> io::Result<Self> {
        let container = tempfile::tempdir()?;
        let path = container.path().join("home");
        #[cfg(windows)]
        {
            use win_custody::{current_user_sid_string, SecurityDescriptor};
            let user = current_user_sid_string()?;
            SecurityDescriptor::from_sddl(&format!(
                "O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"
            ))?
            .create_directory(&path)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        }
        #[cfg(not(any(windows, unix)))]
        std::fs::create_dir(&path)?;
        Ok(Self {
            _container: container,
            path,
        })
    }

    /// Returns the private home directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn output(command: &mut Command, profile: &Path) -> io::Result<Output> {
    let stdout = tempfile::NamedTempFile::new_in(profile)?;
    let stderr = tempfile::NamedTempFile::new_in(profile)?;
    command
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Anima command did not exit within 40 seconds",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    Ok(Output {
        status,
        stdout: std::fs::read(stdout.path())?,
        stderr: std::fs::read(stderr.path())?,
    })
}
