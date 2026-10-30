//! Bounded local observations and two named repairs. No diagnostic executes a model or shell.
use crate::app::SharedDaemonState;
use crate::routes::ApiError;
use crate::schedules::{ScheduleTarget, ScheduleTrigger, ScheduledPromptRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use utoipa::ToSchema;
#[cfg(test)]
mod acceptance_tests;
#[cfg(test)]
mod tests;
const MAX_REPAIRS: usize = 200;
fn images(memories: &[anima_memory::Memory]) -> Vec<crate::memory_maintenance::MemoryImage> {
    memories
        .iter()
        .map(crate::memory_maintenance::MemoryImage::from)
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CheckStatus {
    Observed,
    Degraded,
    Unprobed,
    Unknown,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorCheck {
    pub(crate) id: String,
    pub(crate) status: CheckStatus,
    pub(crate) detail: String,
    pub(crate) elapsed_ms: u64,
    pub(crate) configured: Option<bool>,
    pub(crate) local_runtime: Option<String>,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorReport {
    pub(crate) runtime: Option<crate::routes::capabilities::RuntimeCapabilityObservation>,
    pub(crate) checks: Vec<DoctorCheck>,
    pub(crate) external_probes: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorRegistry {
    pub(crate) repairs: BTreeMap<String, RepairRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepairRecord {
    pub(crate) id: String,
    pub(crate) before: ScheduledPromptRecord,
    pub(crate) after_fingerprint: String,
    pub(crate) applied_at_ms: u64,
    pub(crate) undone: bool,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepairView {
    pub(crate) id: String,
    pub(crate) automation_id: String,
    pub(crate) fingerprint: String,
    pub(crate) undone: bool,
    pub(crate) undo_supported: bool,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepairCandidate {
    pub(crate) automation_id: String,
    pub(crate) fingerprint: String,
    pub(crate) action: String,
}
pub(crate) fn fingerprint<T: Serialize>(v: &T) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(v).expect("typed doctor value"),
    ))
}
fn conflict() -> ApiError {
    ApiError::conflict("Doctor repair state or authority changed; refresh the exact candidate")
}
fn unavailable() -> ApiError {
    ApiError::service_unavailable(
        "Doctor repair persistence is unavailable; inspect current state before retrying",
    )
}
fn eligible(r: &ScheduledPromptRecord) -> bool {
    anima_core::guarded_repair::RecurringPauseCandidate {
        enabled: r.enabled,
        recurring: matches!(
            r.trigger,
            ScheduleTrigger::Interval { .. }
                | ScheduleTrigger::Daily { .. }
                | ScheduleTrigger::Cron { .. }
        ),
        workspace_target: r.target == ScheduleTarget::Workspace,
        has_verification: r.verification.is_some() || r.verification_for.is_some(),
        completed: r.done_at_ms.is_some(),
        consecutive_failures: r.counters.consecutive_failures,
    }
    .eligible()
}
fn paused(before: &ScheduledPromptRecord, now: u64) -> ScheduledPromptRecord {
    let mut r = before.clone();
    r.enabled = false;
    r.updated_at_ms = now.max(r.updated_at_ms).max(r.created_at_ms);
    r
}
fn prepare_pause(
    r: &ScheduledPromptRecord,
    expected: &str,
    now: u64,
) -> Result<(ScheduledPromptRecord, RepairRecord), ApiError> {
    if !eligible(r) || !anima_core::guarded_repair::exact_image(&fingerprint(r), expected) {
        return Err(conflict());
    }
    let after = paused(r, now);
    let journal = RepairRecord {
        id: uuid::Uuid::new_v4().to_string(),
        before: r.clone(),
        after_fingerprint: fingerprint(&after),
        applied_at_ms: now,
        undone: false,
    };
    Ok((after, journal))
}
fn undo_pause(
    current: &ScheduledPromptRecord,
    journal: &RepairRecord,
    now: u64,
) -> Result<ScheduledPromptRecord, ApiError> {
    if !anima_core::guarded_repair::may_undo(
        &fingerprint(current),
        &journal.after_fingerprint,
        journal.undone,
    ) {
        return Err(conflict());
    }
    let mut restored = current.clone();
    restored.enabled = true;
    restored.next_due_at_ms =
        crate::schedules::next_due(&restored.trigger, restored.active_hours.as_ref(), now)
            .map_err(|_| conflict())?;
    restored.updated_at_ms = now.max(current.updated_at_ms).max(current.created_at_ms);
    Ok(restored)
}
impl DoctorRegistry {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.repairs.len() > MAX_REPAIRS {
            return Err("Doctor repair history exceeds its bound".into());
        }
        for (id, r) in &self.repairs {
            crate::schedules::automations::validate_stored_automation(&r.before)?;
            if id != &r.id
                || id.len() > 64
                || !eligible(&r.before)
                || r.after_fingerprint != fingerprint(&paused(&r.before, r.applied_at_ms))
            {
                return Err("Invalid Doctor repair image".into());
            }
        }
        Ok(())
    }
}
impl From<&RepairRecord> for RepairView {
    fn from(r: &RepairRecord) -> Self {
        Self {
            id: r.id.clone(),
            automation_id: r.before.id.clone(),
            fingerprint: r.after_fingerprint.clone(),
            undone: r.undone,
            undo_supported: !r.undone,
        }
    }
}
pub(crate) async fn report(state: &SharedDaemonState) -> DoctorReport {
    let start = Instant::now();
    let Ok(g) = tokio::time::timeout(Duration::from_millis(200), state.read()).await else {
        return DoctorReport {
            runtime: None,
            checks: vec![DoctorCheck {
                id: "local-state".into(),
                configured: None,
                local_runtime: None,
                status: CheckStatus::Unknown,
                detail: "Local state lock exceeded the 200 ms observation budget".into(),
                elapsed_ms: start.elapsed().as_millis() as u64,
            }],
            external_probes: false,
        };
    };
    let runtime = Some(crate::routes::capabilities::runtime_observation(&g));
    let provider_pools = g.provider_keys.configured_provider_count();
    let configured_models = g
        .agents
        .values()
        .filter(|a| !a.config().model.is_empty())
        .count();
    let connector_count = g.connectors.len()
        + g.discord_connectors.len()
        + g.calendar_connectors.len()
        + g.mail_records.len()
        + g.drive_records.len();
    let mcp_count = g.mcp_registry.connections.len();
    let mcp_live = g.mcp_publication.try_read().ok().map(|p| p.live.len());
    let execution_count = g.execution.targets().len();
    let paired_count = g
        .paired_devices
        .records
        .iter()
        .filter(|r| !r.revoked)
        .count();
    let browser_configured = g.browser_setup.is_some();
    let embeddings = g.memory_embeddings_handle();
    let stats = g.history.stats();
    let processes = g.background_process_count();
    let failing = g
        .schedules
        .values()
        .filter(|s| s.enabled && s.counters.consecutive_failures >= 3)
        .count();
    let mut checks = vec![
        (
            "storage",
            CheckStatus::Observed,
            format!(
                "Control-plane mode: {}; this observation does not probe a write",
                g.control_plane_durability()
            ),
        ),
        (
            "history",
            if stats.failing_since_ms.is_some() {
                CheckStatus::Degraded
            } else {
                CheckStatus::Observed
            },
            format!(
                "{} pending writes; {} recorded flush failures",
                stats.pending, stats.flush_errors
            ),
        ),
        (
            "automations",
            if failing > 0 {
                CheckStatus::Degraded
            } else {
                CheckStatus::Observed
            },
            format!(
                "{} enabled; {failing} repeatedly failing",
                g.schedules.values().filter(|s| s.enabled).count()
            ),
        ),
        (
            "processes",
            if processes.is_ok() {
                CheckStatus::Observed
            } else {
                CheckStatus::Unknown
            },
            processes
                .map(|n| format!("{n} tracked background processes"))
                .unwrap_or_else(|_| "Tracked process count is unavailable".into()),
        ),
        (
            "providers",
            CheckStatus::Unprobed,
            format!("{configured_models} configured agent model selections; {} credential pools with keys. No model or credential-vault request was made",provider_pools.map(|n|n.to_string()).unwrap_or_else(||"unknown".into())),
        ),
        (
            "connectors",
            CheckStatus::Unprobed,
            format!("{connector_count} stored connector records; reachability and delivery were not probed"),
        ),
        (
            "mcp",
            CheckStatus::Unprobed,
            format!("{mcp_count} configured MCP connections; {} locally published ready tool bindings; remote reachability was not probed",mcp_live.map(|n|n.to_string()).unwrap_or_else(||"unknown".into())),
        ),
        (
            "browser",
            CheckStatus::Unprobed,
            format!("Operator browser setup configured: {browser_configured}; browser availability and authenticated origin access were not probed"),
        ),
        ("paired-devices",CheckStatus::Unprobed,format!("{paired_count} active paired records; node reachability and capabilities were not probed")),
        (
            "execution",
            CheckStatus::Unprobed,
            format!("{execution_count} immutable execution profiles; Docker and SSH availability were not probed"),
        ),
    ]
    .into_iter()
    .map(|(id, status, detail)| DoctorCheck {
        id: id.into(),
        configured: match id {
            "providers" if configured_models > 0 => Some(true),
            "providers" => provider_pools.map(|n| n > 0),
            "connectors" => Some(connector_count > 0),
            "mcp" => Some(mcp_count > 0),
            "browser" => Some(browser_configured),
            "execution" => Some(execution_count > 0),
            "storage" => Some(g.control_plane_store.is_some()),
            _ => None,
        },
        local_runtime:match id {"mcp"=>mcp_live.map(|n|format!("{n} current locally published bindings")),"providers"=>provider_pools.map(|n|format!("{n} configured credential pools")),_=>None},
        status,
        detail,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
    .collect::<Vec<_>>();
    drop(g);
    let then = Instant::now();
    let (status, detail) =
        match tokio::time::timeout(Duration::from_millis(200), embeddings.read()).await {
            Ok(e) => {
                let s = e.status();
                (
                    if s.load.label() == "ready" {
                        CheckStatus::Observed
                    } else {
                        CheckStatus::Degraded
                    },
                    format!(
                        "{}; {} indexed vectors; embedding requests were not made",
                        s.load.label(),
                        s.vector_count
                    ),
                )
            }
            Err(_) => (
                CheckStatus::Unknown,
                "Embedding lock exceeded the 200 ms observation budget".into(),
            ),
        };
    checks.push(DoctorCheck {
        id: "memory-index".into(),
        configured: None,
        local_runtime: if status == CheckStatus::Unknown {
            None
        } else {
            Some("Observed active local embedding state".into())
        },
        status,
        detail,
        elapsed_ms: then.elapsed().as_millis() as u64,
    });
    DoctorReport {
        runtime,
        checks,
        external_probes: false,
    }
}
#[derive(Clone)]
pub(crate) struct DoctorService {
    state: SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    admission: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}
#[derive(Clone)]
pub(crate) struct DoctorAuthority {
    pub(crate) link: crate::runs::RunLink,
    pub(crate) call: anima_core::ToolCall,
    pub(crate) approved: bool,
    pub(crate) cancel: Option<anima_core::CancelSignal>,
}
impl DoctorAuthority {
    fn validate(&self, g: &crate::state::DaemonState, agent: &str) -> Result<(), ApiError> {
        let live = g.runs.get(&self.link.run_id).is_some_and(|r| {
            r.agent_id == agent
                && self.link.agent_id == agent
                && r.session_id == self.link.session_id
                && r.status == crate::runs::RunStatus::Running
                && r.stop.is_none()
        });
        let allowed = g.get_agent(agent).is_some_and(|a| {
            !crate::agent_runs::is_helper_config(&a.state.config)
                && a.state
                    .config
                    .tools
                    .iter()
                    .flatten()
                    .any(|t| t.name == self.call.name)
                && matches!(
                    (
                        g.approval_verdict(&a.state, &self.link.session_id, &self.call),
                        self.approved
                    ),
                    (crate::approvals::Verdict::Allow, _) | (crate::approvals::Verdict::Ask, true)
                )
        });
        if live
            && allowed
            && matches!(self.call.name.as_str(), "doctor_repair" | "doctor_undo")
            && !self.cancel.as_ref().is_some_and(|c| c.is_cancelled())
        {
            Ok(())
        } else {
            Err(conflict())
        }
    }
}
impl DoctorService {
    pub(crate) fn new(state: SharedDaemonState, transactions: Arc<Mutex<()>>) -> Self {
        Self {
            state,
            transactions,
            admission: None,
        }
    }
    pub(crate) fn with_admission(mut self, admission: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.admission = Some(admission);
        self
    }
    fn admitted(&self) -> Result<(), ApiError> {
        if self.admission.as_ref().is_some_and(|a| !a()) {
            Err(ApiError::conflict("Doctor repair admission is draining"))
        } else {
            Ok(())
        }
    }
    pub(crate) async fn report(&self) -> DoctorReport {
        report(&self.state).await
    }
    pub(crate) async fn request_shutdown(&self) {
        self.state.read().await.doctor_slots.close();
    }
    pub(crate) async fn shutdown(&self) {
        let slots = self.state.read().await.doctor_slots.clone();
        slots.close();
        while slots.available_permits() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub(crate) async fn candidates(&self, agent: &str) -> Result<Vec<RepairCandidate>, ApiError> {
        let g = self.state.read().await;
        if g.get_agent(agent).is_none() {
            return Err(ApiError::not_found());
        }
        Ok(g.schedules
            .values()
            .filter(|r| r.agent_id == agent && eligible(r))
            .take(32)
            .map(|r| RepairCandidate {
                automation_id: r.id.clone(),
                fingerprint: fingerprint(r),
                action: "pause-failing-automation".into(),
            })
            .collect())
    }
    pub(crate) async fn history(&self, agent: &str) -> Result<Vec<RepairView>, ApiError> {
        let g = self.state.read().await;
        if g.get_agent(agent).is_none() {
            return Err(ApiError::not_found());
        }
        Ok(g.doctor
            .repairs
            .values()
            .filter(|r| r.before.agent_id == agent)
            .map(RepairView::from)
            .collect())
    }
    pub(crate) async fn repair(
        &self,
        agent: String,
        target: String,
        expected: String,
        authority: Option<DoctorAuthority>,
    ) -> Result<RepairView, ApiError> {
        self.mutate(agent, target, expected, false, authority).await
    }
    pub(crate) async fn undo(
        &self,
        agent: String,
        id: String,
        expected: String,
        authority: Option<DoctorAuthority>,
    ) -> Result<RepairView, ApiError> {
        self.mutate(agent, id, expected, true, authority).await
    }
    async fn mutate(
        &self,
        agent: String,
        target: String,
        expected: String,
        undo: bool,
        authority: Option<DoctorAuthority>,
    ) -> Result<RepairView, ApiError> {
        if target.len() > 256 || expected.len() != 64 {
            return Err(conflict());
        }
        self.admitted()?;
        let permit = self
            .state
            .read()
            .await
            .doctor_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::conflict("Doctor repair admission is busy or closed"))?;
        let this = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _tx = this.transactions.lock().await;
            this.admitted()?;
            let mut g = this.state.write().await;
            if g.get_agent(&agent).is_none() {
                return Err(ApiError::not_found());
            }
            if g.control_plane_store.is_none() {
                return Err(unavailable());
            }
            if let Some(a) = authority {
                a.validate(&g, &agent)?;
            }
            let now = anima_core::primitives::now_millis();
            let old_registry = g.doctor.clone();
            let (before, after, journal) = if undo {
                let mut r = g
                    .doctor
                    .repairs
                    .get(&target)
                    .filter(|r| r.before.agent_id == agent)
                    .cloned()
                    .ok_or_else(ApiError::not_found)?;
                if r.after_fingerprint != expected {
                    return Err(conflict());
                }
                let current = g
                    .schedules
                    .get(&r.before.id)
                    .cloned()
                    .ok_or_else(conflict)?;
                let after = undo_pause(&current, &r, now)?;
                r.undone = true;
                (current, after, r)
            } else {
                if g.doctor.repairs.len() >= MAX_REPAIRS {
                    return Err(ApiError::conflict("Doctor repair history is full"));
                }
                let before = g
                    .schedules
                    .get(&target)
                    .filter(|r| r.agent_id == agent)
                    .cloned()
                    .ok_or_else(ApiError::not_found)?;
                let (after, r) = prepare_pause(&before, &expected, now)?;
                (before, after, r)
            };
            g.schedules.insert(after.id.clone(), after.clone());
            g.doctor.repairs.insert(journal.id.clone(), journal.clone());
            let persist = g.control_plane_persist_request();
            drop(g);
            if let Err(error) = persist.save().await {
                if !crate::control_plane_store::is_durability_uncertain(&error) {
                    let mut g = this.state.write().await;
                    g.schedules.insert(before.id.clone(), before);
                    g.doctor = old_registry;
                }
                return Err(unavailable());
            }
            this.state
                .read()
                .await
                .publish_automation_updated(&agent, &after.id, false);
            Ok(RepairView::from(&journal))
        })
        .await
        .unwrap_or_else(|_| Err(unavailable()))
    }
    /// Rebuild only the bounded, local derived index. Canonical memories are held read-only.
    pub(crate) async fn rebuild_index(
        &self,
        expected: String,
    ) -> Result<serde_json::Value, ApiError> {
        self.admitted()?;
        let permit = self
            .state
            .read()
            .await
            .doctor_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::conflict("A derived index repair is already running"))?;
        let this = self.clone();
        tokio::task::spawn_blocking(move||{
            let _permit=permit;
            let (memory,embeddings)={let g=this.state.blocking_read();(g.memory_handle(),g.memory_embeddings_handle())};
            let memories=memory.blocking_read();let snapshot=memories.bounded_memories(256,1024*1024).ok_or_else(conflict)?;let mut index=embeddings.blocking_write();let status=index.status();
            if status.provider!="local" || status.load.label()!="ready" || fingerprint(&(images(&snapshot),&status.provider,&status.model,status.dimension))!=expected{return Err(conflict())}
            index.rebuild_bounded_local_runtime_index(&snapshot).map_err(|_|ApiError::service_unavailable("Derived index rebuild failed; canonical memories are unchanged"))?;
            Ok(serde_json::json!({"rebuilt":true,"canonicalMemoryChanged":false,"persistedCacheChanged":false,"undoSupported":false,"vectors":index.status().vector_count}))
        }).await.unwrap_or_else(|_|Err(ApiError::service_unavailable("Derived index worker failed")))
    }
    pub(crate) async fn index_candidate(&self) -> Result<String, ApiError> {
        let (m, e) = {
            let g = self.state.read().await;
            (g.memory_handle(), g.memory_embeddings_handle())
        };
        let g = m.read().await;
        let e = e.read().await;
        let s = e.status();
        Ok(fingerprint(&(
            images(&g.bounded_memories(256, 1024 * 1024).ok_or_else(conflict)?),
            &s.provider,
            &s.model,
            s.dimension,
        )))
    }
}
