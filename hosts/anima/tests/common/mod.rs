//! Capture a launcher, not EOF from its detached descendants on Windows.

use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

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
