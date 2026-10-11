//! Immutable, bounded instruction snapshots. Async callers alone own acceptance.
mod disk;
pub(crate) mod manifest;
pub(crate) mod runtime;
mod scan;
mod selection;
pub(crate) mod write_policy;
use anima_core::agent_files::{AgentFileRenderBudget, RenderedAgentFiles};
pub(crate) use manifest::*;
#[cfg(test)]
pub(crate) use selection::profile_relative;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;

pub(crate) const MAX_FILE_BYTES: usize = 262_144;
pub(crate) const MAX_TOTAL_READ_BYTES: usize = 1_048_576;
pub(crate) const MAX_CANDIDATES: usize = 96;
pub(crate) const MAX_DIRECTORY_DEPTH: usize = 16;
pub(crate) const MAX_SCAN_CHARS: usize = 65_536;
pub(crate) const MAX_RENDER_CHARS: usize = 20_000;
pub(crate) const DEFAULT_LOAD_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub(crate) struct AgentFileScope {
    pub agent_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub workspace_root: PathBuf,
    pub project_relative: String,
    pub profile_enabled: bool,
    pub companion: bool,
    pub config_revision: String,
}
#[derive(Clone, Debug)]
pub(crate) struct AcceptedAgentFiles {
    pub rendered: RenderedAgentFiles,
    pub manifest: AgentFilesManifest,
}
impl AcceptedAgentFiles {
    pub(crate) fn empty(availability: Availability) -> Self {
        let mut value = Self {
            rendered: RenderedAgentFiles {
                identity: None,
                project_context: String::new(),
                estimated_tokens: 0,
            },
            manifest: AgentFilesManifest {
                availability,
                captured_at_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                snapshot_digest: String::new(),
                identity_source: IdentitySource::Legacy,
                project_source_kind: None,
                estimated_tokens: 0,
                entries: vec![],
            },
        };
        value.seal();
        value
    }
    pub(super) fn seal(&mut self) {
        // Stable serialization supplies unambiguous field/length framing. Capture time
        // is deliberately excluded so unchanged accepted content has the same digest.
        let data = serde_json::to_vec(&(
            &self.manifest.availability,
            &self.manifest.identity_source,
            &self.manifest.project_source_kind,
            &self.manifest.entries,
            self.rendered
                .identity
                .as_ref()
                .map(|identity| identity.text.as_str()),
            &self.rendered.project_context,
            "agent-files-render-v1",
        ))
        .expect("manifest serialization");
        self.manifest.snapshot_digest = format!("{:x}", Sha256::digest(data));
        self.manifest.estimated_tokens = self.rendered.estimated_tokens;
    }
}
pub(super) fn stopped(cancel: &AtomicBool, deadline: Instant) -> Option<AgentFileState> {
    if cancel.load(Ordering::Acquire) {
        Some(AgentFileState::Cancelled)
    } else if Instant::now() >= deadline {
        Some(AgentFileState::TimedOut)
    } else {
        None
    }
}
fn fallback(scope: &AgentFileScope, state: AgentFileState) -> AcceptedAgentFiles {
    let mut value = AcceptedAgentFiles::empty(Availability::Available);
    let source = selection::soul(scope);
    let kind = if source.is_some() {
        AgentFileKind::Soul
    } else {
        AgentFileKind::Agents
    };
    let mut entry = AgentFileManifestEntry::new(source.unwrap_or_else(|| "AGENTS.md".into()), kind);
    entry.state = state;
    entry.reason_code = Some(if state == AgentFileState::Cancelled {
        ReasonCode::Cancelled
    } else {
        ReasonCode::Deadline
    });
    value.manifest.entries.push(entry);
    value.seal();
    value
}
pub(crate) fn load_scope(
    scope: &AgentFileScope,
    budget: AgentFileRenderBudget,
    cancel: &AtomicBool,
    deadline: Instant,
) -> AcceptedAgentFiles {
    disk::load_scope(scope, budget, cancel, deadline)
}
/// Two process-wide permits are retained by the blocking closure until it really
/// exits. An OS-stuck read cannot be killed; later callers time out on admission.
/// Dropping this future signals cancellation and never publishes a late result.
pub(crate) async fn load(
    scope: AgentFileScope,
    budget: AgentFileRenderBudget,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
) -> AcceptedAgentFiles {
    static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    load_with_workers(
        scope,
        budget,
        cancel,
        deadline,
        WORKERS.get_or_init(|| Arc::new(Semaphore::new(2))).clone(),
    )
    .await
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
async fn wait_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
async fn load_with_workers(
    scope: AgentFileScope,
    budget: AgentFileRenderBudget,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    workers: Arc<Semaphore>,
) -> AcceptedAgentFiles {
    run_worker(scope, cancel, deadline, workers, move |scope, local| {
        load_scope(&scope, budget, &local, deadline)
    })
    .await
}
async fn run_worker<F>(
    scope: AgentFileScope,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    workers: Arc<Semaphore>,
    work: F,
) -> AcceptedAgentFiles
where
    F: FnOnce(AgentFileScope, Arc<AtomicBool>) -> AcceptedAgentFiles + Send + 'static,
{
    let local = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelOnDrop(local.clone());
    let until = tokio::time::Instant::from_std(deadline);
    let permit = tokio::select! { biased;
        _ = wait_cancel(&cancel) => return fallback(&scope, AgentFileState::Cancelled),
        _ = tokio::time::sleep_until(until) => return fallback(&scope, AgentFileState::TimedOut),
        permit = workers.acquire_owned() => match permit { Ok(permit) => permit, Err(_) => return fallback(&scope, AgentFileState::TimedOut) },
    };
    let worker_scope = scope.clone();
    let mut worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work(worker_scope, local)
    });
    tokio::select! { biased;
        _ = wait_cancel(&cancel) => fallback(&scope, AgentFileState::Cancelled),
        _ = tokio::time::sleep_until(until) => fallback(&scope, AgentFileState::TimedOut),
        result = &mut worker => {
            if let Some(state) = stopped(&cancel, deadline) { fallback(&scope,state) }
            else { result.unwrap_or_else(|_| fallback(&scope, AgentFileState::TimedOut)) }
        }
    }
}
#[cfg(test)]
mod tests;
