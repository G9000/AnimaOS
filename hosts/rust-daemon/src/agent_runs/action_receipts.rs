//! Awaited native witnesses. Storage jobs own data only, never tool effects.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex as StdMutex, Weak,
};

use anima_core::{CancelSignal, Content, DataValue, TaskResult, TaskStatus, ToolCall};
use tokio::sync::{Mutex, Notify, RwLock};

use crate::app::SharedDaemonState;
use crate::live::events::preview;
use crate::runs::{ActionDurability, ActionReceipt, ActionStatus, RunLink, RunStatus};
use crate::state::DaemonState;

#[cfg(test)]
use std::sync::atomic::AtomicBool;

pub(crate) const RECEIPT_NOT_SAVED: &str = "Native action evidence could not be saved; effects may have occurred. Further actions were stopped.";
const ACTION_ADMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const MAX_ACTION_SNAPSHOT_ATTEMPTS: usize = 8;

#[derive(Clone)]
pub(crate) struct RunActionJournal {
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    run: RunLink,
    cancel: CancelSignal,
    deadline: Option<tokio::time::Instant>,
    failure: Arc<StdMutex<Option<String>>>,
    pending: Arc<PendingSaves>,
    #[cfg(test)]
    rpc_authority_read_gate: Arc<StdMutex<Option<Arc<TestRpcAuthorityReadGate>>>>,
    #[cfg(test)]
    before_save_gate: Arc<StdMutex<Option<Arc<TestActionSaveGate>>>>,
}

#[cfg(test)]
pub(crate) struct TestActionSaveGate {
    pub(crate) entered: tokio::sync::Semaphore,
    pub(crate) release: tokio::sync::Semaphore,
}

/// Test scheduling only: no call arguments, outputs or transport material.
#[cfg(test)]
pub(crate) struct TestRpcAuthorityReadGate {
    pub(crate) entered: tokio::sync::Semaphore,
    pub(crate) release: tokio::sync::Semaphore,
    pub(crate) blocked: tokio::sync::Semaphore,
    armed: AtomicBool,
}

#[derive(Default)]
struct PendingSaves {
    count: AtomicUsize,
    settled: Notify,
}

struct SaveWork {
    journal: RunActionJournal,
    settled: bool,
}
impl Drop for SaveWork {
    fn drop(&mut self) {
        if !self.settled {
            self.journal.fail();
        }
        self.journal.pending.count.fetch_sub(1, Ordering::SeqCst);
        self.journal.pending.settled.notify_waiters();
    }
}

impl RunActionJournal {
    pub(crate) fn rpc_deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    pub(crate) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        run: RunLink,
        cancel: CancelSignal,
        deadline: Option<tokio::time::Instant>,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            transactions,
            run,
            cancel,
            deadline,
            failure: Arc::new(StdMutex::new(None)),
            pending: Arc::new(PendingSaves::default()),
            #[cfg(test)]
            rpc_authority_read_gate: Arc::new(StdMutex::new(None)),
            #[cfg(test)]
            before_save_gate: Arc::new(StdMutex::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn install_test_before_save_gate(&self) -> Arc<TestActionSaveGate> {
        let gate = Arc::new(TestActionSaveGate {
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
        });
        *self.before_save_gate.lock().unwrap() = Some(gate.clone());
        gate
    }

    #[cfg(test)]
    pub(crate) fn install_test_rpc_authority_read_gate(&self) -> Arc<TestRpcAuthorityReadGate> {
        let gate = Arc::new(TestRpcAuthorityReadGate {
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            blocked: tokio::sync::Semaphore::new(0),
            armed: AtomicBool::new(false),
        });
        *self.rpc_authority_read_gate.lock().unwrap() = Some(gate.clone());
        gate
    }

    #[cfg(test)]
    pub(crate) fn arm_test_final_rpc_authority_read(&self) {
        if let Some(gate) = self.rpc_authority_read_gate.lock().unwrap().as_ref() {
            gate.armed.store(true, Ordering::SeqCst);
        }
    }

    #[cfg(test)]
    async fn test_rpc_authority_read<'a>(
        &self,
        state: &'a RwLock<DaemonState>,
    ) -> tokio::sync::RwLockReadGuard<'a, DaemonState> {
        use std::future::Future;
        let gate = self
            .rpc_authority_read_gate
            .lock()
            .unwrap()
            .clone()
            .filter(|gate| gate.armed.swap(false, Ordering::SeqCst));
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("test release gate")
                .forget();
            let read = state.read();
            tokio::pin!(read);
            let mut noted = false;
            std::future::poll_fn(|context| {
                let result = read.as_mut().poll(context);
                if result.is_pending() && !noted {
                    noted = true;
                    gate.blocked.add_permits(1);
                }
                result
            })
            .await
        } else {
            state.read().await
        }
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// A dispatched native effect lost physical cleanup proof. Reuse the same
    /// fail-closed latch as evidence persistence without inventing a save error
    /// or an owner Stop. The acknowledged Started receipt remains unsettled.
    pub(crate) fn stop_for_unproven_effects(&self, message: &'static str) {
        self.failure
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_or_insert_with(|| message.to_string());
        self.cancel.cancel();
    }

    pub(crate) fn owns_caller(&self, agent_id: &str, input_agent_id: &str) -> bool {
        self.run.agent_id == agent_id && self.run.agent_id == input_agent_id
    }

    pub(crate) fn bound_run(&self) -> &RunLink {
        &self.run
    }

    /// Final, non-waiting authority snapshot after checkpoint publication.
    /// The short guard is released before the caller performs any file effect.
    pub(crate) fn checkpoint_revalidate_now(
        &self,
        workspace: &std::path::Path,
        store: &std::path::Path,
        call: &ToolCall,
        approved: bool,
        recovery: bool,
    ) -> Result<(), String> {
        self.check_liveness()?;
        let state = self
            .state
            .upgrade()
            .ok_or("Native action owner is unavailable")?;
        let guard = state
            .try_read()
            .map_err(|_| "Checkpoint authority is busy; no file effect was admitted")?;
        self.check_owner(&guard, true)?;
        // A worktree helper's checkpoints follow its own checkout; every
        // other run must still match the global workspace exactly.
        let expected_workspace = guard
            .agents
            .get(&self.run.agent_id)
            .and_then(|agent| super::worktrees::run_root(agent.config()))
            .or_else(|| {
                guard
                    .workspace
                    .as_ref()
                    .map(|value| value.root_path.clone())
            });
        if expected_workspace.as_deref() != Some(workspace)
            || guard.checkpoint_root.as_deref() != Some(store)
        {
            return Err("Checkpoint workspace or host configuration changed".into());
        }
        let record = guard.runs.get(&self.run.run_id).unwrap();
        if !record.action_receipts.iter().any(|receipt| {
            receipt.tool_call_id == call.id
                && receipt.name == call.name
                && receipt.status == ActionStatus::Started
        }) {
            return Err("Checkpoint native action intent is unavailable".into());
        }
        let current = guard
            .get_agent(&self.run.agent_id)
            .ok_or("Checkpoint owner is unavailable")?
            .state;
        if !current.config.allows_tool(&call.name) {
            return Err("Checkpoint tool permission was revoked".into());
        }
        if recovery
            && (record.parent_run_id.is_some()
                || record.source == crate::runs::RunSource::Delegation
                || super::is_helper_config(&current.config))
        {
            return Err("Checkpoint recovery requires a direct ordinary run".into());
        }
        if let Some(parent) = super::config_helper_parent(&current.config) {
            if !guard
                .get_agent(parent)
                .is_some_and(|agent| agent.state.config.allows_tool(&call.name))
            {
                return Err("Checkpoint parent tool permission was revoked".into());
            }
        }
        if let Some(parent) = record
            .parent_run_id
            .as_deref()
            .and_then(|id| guard.runs.get(id))
            .filter(|_| {
                super::is_helper_config(&current.config)
                    || record.source == crate::runs::RunSource::Delegation
            })
        {
            if parent.stop.is_some() || !parent.status.is_in_flight() {
                return Err("Checkpoint parent run is no longer active".into());
            }
        }
        match guard.approval_verdict(&current, &self.run.session_id, call) {
            crate::approvals::Verdict::Allow => Ok(()),
            crate::approvals::Verdict::Ask if approved => Ok(()),
            _ => Err("Checkpoint action is no longer approved".into()),
        }
    }

    /// Recheck current execution authority at the actual selected transport boundary.
    pub(crate) async fn execution_authority(
        &self,
        call: &ToolCall,
        approved: bool,
    ) -> Result<(), String> {
        self.check_liveness()?;
        let shared = self
            .state
            .upgrade()
            .ok_or("Native action owner is unavailable")?;
        super::ownership::verify(&shared).await?;
        let guard = shared.read().await;
        self.check_owner(&guard, true)?;
        let record = guard
            .runs
            .get(&self.run.run_id)
            .ok_or("Execution source run is unavailable")?;
        let current = guard
            .get_agent(&self.run.agent_id)
            .ok_or("Execution owner is unavailable")?
            .state;
        if call.name != "execution_run"
            || record.status != RunStatus::Running
            || record.parent_run_id.is_some()
            || record.source == crate::runs::RunSource::Delegation
            || super::is_helper_config(&current.config)
            || !current.config.allows_tool("execution_run")
        {
            return Err("Selected execution requires a currently granted ordinary run".into());
        }
        if !record.action_receipts.iter().any(|r| {
            r.tool_call_id == call.id && r.name == call.name && r.status == ActionStatus::Started
        }) {
            return Err("Execution native action intent is unavailable".into());
        }
        match guard.approval_verdict(&current, &self.run.session_id, call) {
            crate::approvals::Verdict::Allow => Ok(()),
            crate::approvals::Verdict::Ask if approved => Ok(()),
            _ => Err("Selected execution is no longer approved".into()),
        }
    }

    /// One authoritative snapshot for the stricter RPC grant intersection.
    /// No model input supplies the run, wrapper call or captured grant set.
    pub(crate) async fn rpc_authority(
        &self,
        run: &RunLink,
        outer: &ToolCall,
        inner: Option<&str>,
    ) -> Result<(usize, std::collections::BTreeSet<String>), String> {
        if run != &self.run || outer.name != "execute_code" {
            return Err("Tool RPC scope does not match its native action journal".into());
        }
        self.revalidate().await?;
        let state = self
            .state
            .upgrade()
            .ok_or("Native action owner is unavailable")?;
        #[cfg(test)]
        let guard = self.test_rpc_authority_read(&state).await;
        #[cfg(not(test))]
        let guard = state.read().await;
        self.check_owner(&guard, true)?;
        let record = guard.runs.get(&self.run.run_id).unwrap();
        if record.parent_run_id.is_some() || record.source == crate::runs::RunSource::Delegation {
            return Err("Tool RPC requires a direct ordinary run".into());
        }
        if !record.action_receipts.iter().any(|receipt| {
            receipt.tool_call_id == outer.id
                && receipt.name == outer.name
                && receipt.status == ActionStatus::Started
        }) {
            return Err("Tool RPC outer native action intent is missing".into());
        }
        let runtime = guard.agents.get(&self.run.agent_id).unwrap();
        if super::is_helper_config(runtime.config()) {
            return Err("Tool RPC is unavailable to helpers".into());
        }
        if !runtime.config().allows_tool("execute_code")
            || !runtime.config().allows_tool("bash")
            || inner.is_some_and(|name| !runtime.config().allows_tool(name))
            || guard
                .approvals
                .policy(&self.run.agent_id)
                .action(crate::approvals::RiskClass::Exec)
                == Some(crate::approvals::PolicyAction::Deny)
        {
            return Err("Tool RPC current grants or execution policy refuse this action".into());
        }
        let granted = runtime
            .config()
            .tools
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|descriptor| descriptor.name.clone())
            .collect();
        Ok((
            crate::live::MAX_LIVE_TOOL_CARDS.saturating_sub(record.action_receipts.len()),
            granted,
        ))
    }

    pub(crate) async fn settle(&self) {
        loop {
            let notified = self.pending.settled.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.pending.count.load(Ordering::SeqCst) == 0 {
                return;
            }
            notified.await;
        }
    }

    async fn save(
        &self,
        call: ToolCall,
        outcome: Option<(bool, String, u64)>,
    ) -> Result<(), String> {
        self.pending.count.fetch_add(1, Ordering::SeqCst);
        let mut work = SaveWork {
            journal: self.clone(),
            settled: false,
        };
        tokio::spawn(async move {
            let result = work.journal.update(call, outcome).await;
            work.settled = true;
            drop(work);
            result
        })
        .await
        .map_err(|error| {
            tracing::warn!(run_id = %self.run.run_id, panicked = error.is_panic(), cancelled = error.is_cancelled(), "native action evidence task did not settle");
            self.fail()
        })?
    }

    fn fail(&self) -> String {
        let mut failure = self
            .failure
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        failure.get_or_insert_with(|| RECEIPT_NOT_SAVED.into());
        self.cancel.cancel();
        RECEIPT_NOT_SAVED.into()
    }

    fn check_liveness(&self) -> Result<(), String> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        if self.cancel.is_cancelled() {
            return Err(self.cancel.tool_result().into());
        }
        if self
            .deadline
            .is_some_and(|at| tokio::time::Instant::now() >= at)
        {
            return Err("Execution deadline reached before running the tool".into());
        }
        Ok(())
    }

    pub(crate) async fn revalidate(&self) -> Result<(), String> {
        self.check_liveness()?;
        let state = self
            .state
            .upgrade()
            .ok_or("Native action owner is unavailable")?;
        super::ownership::verify(&state).await?;
        let guard = state.read().await;
        self.check_owner(&guard, true)
    }

    fn check_owner(&self, state: &DaemonState, starting: bool) -> Result<(), String> {
        state.ensure_profile_owned().map_err(|e| e.to_string())?;
        if starting {
            // State locks may have waited since the previous check. Recheck
            // synchronous liveness under the acquired guard before admission;
            // outcome acknowledgement still settles after cancellation.
            self.check_liveness()?;
        }
        if !state.agents.contains_key(&self.run.agent_id)
            || !state
                .sessions
                .contains(&self.run.agent_id, &self.run.session_id)
        {
            return Err("Native action owner or session was deleted".into());
        }
        let record = state
            .runs
            .get(&self.run.run_id)
            .filter(|record| {
                record.agent_id == self.run.agent_id && record.session_id == self.run.session_id
            })
            .ok_or("Native action run is unavailable")?;
        if starting
            && (!matches!(
                record.status,
                RunStatus::Running | RunStatus::AwaitingApproval
            ) || record.stop.is_some())
        {
            return Err("Native action run is no longer running".into());
        }
        Ok(())
    }

    pub(crate) async fn begin(&self, call: &ToolCall) -> Result<(), String> {
        self.revalidate().await?;
        // Dropping a caller does not drop a partially acknowledged save.
        // This job may save an unused started intent, but never dispatches.
        self.save(call.clone(), None).await
    }

    pub(crate) async fn finish(
        &self,
        call: &ToolCall,
        result: &TaskResult<Content>,
        duration_ms: u64,
    ) -> Result<(), String> {
        let succeeded = result.status == TaskStatus::Success && result.error.is_none();
        let text = result.error.clone().unwrap_or_else(|| {
            result
                .data
                .as_ref()
                .map(|data| data.text.clone())
                .unwrap_or_default()
        });
        self.save(call.clone(), Some((succeeded, text, duration_ms)))
            .await
    }

    async fn update(
        &self,
        call: ToolCall,
        outcome: Option<(bool, String, u64)>,
    ) -> Result<(), String> {
        let _transaction = tokio::time::timeout(
            ACTION_ADMISSION_TIMEOUT,
            Arc::clone(&self.transactions).lock_owned(),
        )
        .await
        .map_err(|_| self.fail())?;
        let starting = outcome.is_none();
        // Ordinary snapshots can overtake an unpublished candidate before it
        // acquires persistence ordering. Only that typed, pre-I/O rejection
        // permits a fresh snapshot; never retry storage uncertainty or a tool.
        for _ in 0..MAX_ACTION_SNAPSHOT_ATTEMPTS {
            if let Some(error) = self.failure() {
                return Err(error);
            }
            if starting {
                self.revalidate().await?;
            }
            let state = self
                .state
                .upgrade()
                .ok_or("Native action owner is unavailable")?;
            let (receipts, persist) = {
                let mut guard = tokio::time::timeout(ACTION_ADMISSION_TIMEOUT, state.write())
                    .await
                    .map_err(|_| self.fail())?;
                self.check_owner(&guard, starting)?;
                let record = guard.runs.get(&self.run.run_id).unwrap();
                let mut candidate = record.clone();
                let now = anima_core::primitives::now_millis();
                if let Some((succeeded, text, duration)) = &outcome {
                    let receipt = candidate
                        .action_receipts
                        .iter_mut()
                        .find(|receipt| {
                            receipt.tool_call_id == call.id && receipt.name == call.name
                        })
                        .filter(|receipt| receipt.status == ActionStatus::Started)
                        .ok_or("Native action intent is missing or already settled")?;
                    let (text, truncated) = preview(text);
                    receipt.status = if *succeeded {
                        ActionStatus::Succeeded
                    } else {
                        ActionStatus::Failed
                    };
                    receipt.finished_at_ms = Some(now.max(receipt.started_at_ms));
                    receipt.duration_ms = Some(*duration);
                    receipt.result_preview = Some(text);
                    receipt.result_truncated = truncated;
                } else {
                    if call.id.trim().is_empty()
                        || call.name.trim().is_empty()
                        || call.id.len() > crate::live::MAX_PREVIEW_BYTES
                        || call.name.len() > crate::live::MAX_PREVIEW_BYTES
                    {
                        return Err("Invalid native action identity".into());
                    }
                    if candidate
                        .action_receipts
                        .iter()
                        .any(|receipt| receipt.tool_call_id == call.id)
                    {
                        return Err(
                        "Native action call identity has already been used; it will not run again"
                            .into(),
                    );
                    }
                    if candidate.action_receipts.len() >= crate::live::MAX_LIVE_TOOL_CARDS {
                        return Err(
                            "Native action witness limit reached; the tool will not run".into()
                        );
                    }
                    let arguments = if call.name == crate::tools::mcp_management::NAME {
                        crate::tools::mcp_management::arguments_preview(&call)
                    } else {
                        crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()))
                            .to_string()
                    };
                    let (arguments_preview, arguments_truncated) = preview(&arguments);
                    candidate.action_receipts.push(ActionReceipt {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        status: ActionStatus::Started,
                        started_at_ms: now,
                        finished_at_ms: None,
                        duration_ms: None,
                        arguments_preview,
                        arguments_truncated,
                        result_preview: None,
                        result_truncated: false,
                        durability: if guard.control_plane_store.is_some() {
                            ActionDurability::Persistent
                        } else {
                            ActionDurability::Memory
                        },
                    });
                }
                candidate.note_tool_started(&call.name);
                let receipts = candidate.action_receipts;
                let persist = guard
                .control_plane_persist_request()
                .with_run_actions(&self.run.run_id, receipts.clone(), candidate.tools_started)
                .ok_or_else(|| {
                    tracing::warn!(run_id = %self.run.run_id, "native action run is absent from the persistence snapshot");
                    self.fail()
                })?;
                (receipts, persist)
            };
            #[cfg(test)]
            {
                let gate = self.before_save_gate.lock().unwrap().take();
                if let Some(gate) = gate {
                    gate.entered.add_permits(1);
                    gate.release.acquire().await.unwrap().forget();
                }
            }
            // Once storage starts, retain ordering until its actual acknowledgement.
            // Dropping an async PostgreSQL query on a local timeout could let a
            // late server write overwrite a later deletion/terminal snapshot.
            // JSON sync I/O and database settlement have no claimed hard deadline.
            if let Err(error) = persist.save_action_candidate().await {
                if crate::state::is_superseded_control_plane_save(&error) {
                    continue;
                }
                tracing::warn!(run_id = %self.run.run_id, tool_call_id = %call.id, error = %error, "could not save native action evidence");
                return Err(self.fail());
            }
            // Public reads see only the previous receipt while I/O runs. Publish
            // the acknowledged action patch, never a whole stale run snapshot.
            let mut guard = state.write().await;
            self.check_owner(&guard, false)?;
            let record = guard.runs.get_mut(&self.run.run_id).unwrap();
            record.action_receipts = receipts;
            record.note_tool_started(&call.name);
            record.mirrored = false;
            return Ok(());
        }
        tracing::warn!(run_id = %self.run.run_id, "native action snapshot attempt limit reached");
        Err(self.fail())
    }
}
