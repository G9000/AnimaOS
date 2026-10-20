//! Update checks, and getting an update ready, beside the lifecycle task.
//!
//! One check runs at a time: the one at start and every 6 hours (unless
//! `[updates] check = false`), the one `check-update` asks for, and the one an
//! update begins with.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use semver::Version;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::state::UpdateStatus;
use super::{lock, SharedStatus};
use crate::config::Config;
use crate::update::{self, ReleaseInfo, UpdateError};

pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// A verified binary, ready to install.
pub struct Prepared {
    pub version: String,
    pub binary: Vec<u8>,
}

/// How getting an update ready goes.
pub enum Progress {
    /// The stage the status shows, e.g. `Downloading v0.2.0`.
    Stage(String),
    /// The binary, or why there is none.
    Done(Result<Prepared, String>),
}

/// A preparation's progress, tagged with its job number.
pub type JobProgress = (u64, Progress);

/// Checks for updates, keeping `update` in the status current.
#[derive(Clone)]
pub struct Updater {
    status: SharedStatus,
    /// Held for each check. Inside: the error the checks last put in `lastError`.
    turn: Arc<tokio::sync::Mutex<Option<String>>>,
}

impl Updater {
    pub fn new(status: SharedStatus) -> Self {
        Self {
            status,
            turn: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Asks for a newer release, after any check already under way.
    pub async fn check(&self) -> Result<Option<ReleaseInfo>, UpdateError> {
        let mut shown = self.turn.lock().await;
        let outcome = find_release().await;
        record(&mut lock(&self.status).update, &outcome, &mut shown);
        outcome
    }
}

async fn find_release() -> Result<Option<ReleaseInfo>, UpdateError> {
    // An unconfigured build says so on every platform.
    if !update::release_key_configured() {
        return Err(UpdateError::NotConfigured);
    }
    let target = update::current_target().ok_or(UpdateError::Unsupported)?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| UpdateError::Io(std::io::Error::other(error)))?;
    update::check(&update::http_client()?, update::REPO, &current, target).await
}

/// Puts a check's outcome in `update`. `shown` is the error the checks last put
/// there: a successful check clears only that one, so an error from elsewhere
/// (an update that failed or was rolled back) stays.
fn record(
    update: &mut UpdateStatus,
    outcome: &Result<Option<ReleaseInfo>, UpdateError>,
    shown: &mut Option<String>,
) {
    match outcome {
        Ok(release) => {
            update.available = release.as_ref().map(|release| release.version.to_string());
            if shown.is_some() && update.last_error == *shown {
                update.last_error = None;
            }
            *shown = None;
        }
        Err(error) => {
            let message = error.to_string();
            update.last_error = Some(message.clone());
            *shown = Some(message);
        }
    }
}

/// Checks at start and then every [`CHECK_INTERVAL`] while `[updates] check`
/// is on in `config_file`, and whenever `now` asks. A check that can never
/// succeed in this build (no release key, no releases for this platform) ends
/// the schedule; asking still answers.
pub async fn run_checks(updater: Updater, config_file: PathBuf, mut now: mpsc::Receiver<()>) {
    let mut due = Some(Instant::now());
    loop {
        let periodic = async move {
            match due {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        let asked = tokio::select! {
            () = periodic => false,
            asked = now.recv() => match asked {
                Some(()) => true,
                None => return,
            },
        };
        if !asked {
            due = Some(Instant::now() + CHECK_INTERVAL);
            if !Config::load(&config_file).map_or(true, |cfg| cfg.updates.check) {
                continue;
            }
        }
        if let Err(error) = updater.check().await {
            if error.is_permanent() {
                due = None;
            }
        }
    }
}

/// Gets the newest release ready to install, while the server keeps running:
/// checks, then downloads and verifies it, reporting each stage as job `job`.
pub fn prepare(
    updater: Updater,
    job: u64,
    events: mpsc::UnboundedSender<JobProgress>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let stage = |stage: String| {
            let _ = events.send((job, Progress::Stage(stage)));
        };
        let outcome = fetch(&updater, &stage).await;
        let _ = events.send((job, Progress::Done(outcome)));
    })
}

async fn fetch(updater: &Updater, stage: &impl Fn(String)) -> Result<Prepared, String> {
    stage("Checking for updates".to_string());
    let release = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "No update is available".to_string())?;
    let fetched = async {
        let client = update::http_client()?;
        update::download_binary(&client, &release, update::RELEASE_PUBLIC_KEY, stage).await
    };
    let binary = fetched
        .await
        .map_err(|error: UpdateError| error.to_string())?;
    Ok(Prepared {
        version: release.version.to_string(),
        binary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            version: Version::parse(version).unwrap(),
            target: "aarch64-apple-darwin".into(),
            archive_name: "anima-aarch64-apple-darwin.tar.gz".into(),
            archive_url: "https://example.invalid/a".into(),
            sha256_url: "https://example.invalid/a.sha256".into(),
            sig_url: "https://example.invalid/a.minisig".into(),
        }
    }

    #[test]
    fn a_found_release_is_offered() {
        let mut update = UpdateStatus::default();
        let mut shown = None;
        record(&mut update, &Ok(Some(release("0.2.0"))), &mut shown);
        assert_eq!(update.available.as_deref(), Some("0.2.0"));
        record(&mut update, &Ok(None), &mut shown);
        assert_eq!(update.available, None);
    }

    #[test]
    fn an_unconfigured_build_says_so() {
        let mut update = UpdateStatus::default();
        let mut shown = None;
        let outcome = Err(UpdateError::NotConfigured);
        record(&mut update, &outcome, &mut shown);
        assert_eq!(
            update.last_error.as_deref(),
            Some("Updates are not configured for this build")
        );
        assert!(UpdateError::NotConfigured.is_permanent());
        assert!(!UpdateError::Http("offline".into()).is_permanent());
    }

    #[test]
    fn a_good_check_clears_only_its_own_error() {
        let mut update = UpdateStatus::default();
        let mut shown = None;
        record(
            &mut update,
            &Err(UpdateError::Http("offline".into())),
            &mut shown,
        );
        record(&mut update, &Ok(None), &mut shown);
        assert_eq!(update.last_error, None);

        // A rolled-back update stays visible through a good check.
        let rolled_back = "Update to v0.2.0 failed to start; rolled back".to_string();
        update.last_error = Some(rolled_back.clone());
        record(&mut update, &Ok(Some(release("0.2.0"))), &mut shown);
        assert_eq!(update.last_error, Some(rolled_back));
    }
}
