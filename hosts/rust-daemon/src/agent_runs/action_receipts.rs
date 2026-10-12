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

pub(crate) const RECEIPT_NOT_SAVED: &str = "Native action evidence could not be saved; effects may have occurred. Further actions were stopped.";
const ACTION_ADMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Clone)]
pub(crate) struct RunActionJournal {
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    run: RunLink,
    cancel: CancelSignal,
    deadline: Option<tokio::time::Instant>,
    failure: Arc<StdMutex<Option<String>>>,
    pending: Arc<PendingSaves>,
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
        }
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(crate) fn owns_caller(&self, agent_id: &str, input_agent_id: &str) -> bool {
        self.run.agent_id == agent_id && self.run.agent_id == input_agent_id
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
        .map_err(|_| self.fail())?
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

    pub(crate) async fn revalidate(&self) -> Result<(), String> {
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
        let state = self
            .state
            .upgrade()
            .ok_or("Native action owner is unavailable")?;
        let guard = state.read().await;
        self.check_owner(&guard, true)
    }

    fn check_owner(&self, state: &DaemonState, starting: bool) -> Result<(), String> {
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
        if let Some(error) = self.failure() {
            return Err(error);
        }
        let starting = outcome.is_none();
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
            if let Some((succeeded, text, duration)) = outcome {
                let receipt = candidate
                    .action_receipts
                    .iter_mut()
                    .find(|receipt| receipt.tool_call_id == call.id && receipt.name == call.name)
                    .filter(|receipt| receipt.status == ActionStatus::Started)
                    .ok_or("Native action intent is missing or already settled")?;
                let (text, truncated) = preview(&text);
                receipt.status = if succeeded {
                    ActionStatus::Succeeded
                } else {
                    ActionStatus::Failed
                };
                receipt.finished_at_ms = Some(now.max(receipt.started_at_ms));
                receipt.duration_ms = Some(duration);
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
                    return Err("Native action witness limit reached; the tool will not run".into());
                }
                let arguments =
                    crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()))
                        .to_string();
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
                .ok_or_else(|| self.fail())?;
            (receipts, persist)
        };
        // Once storage starts, retain ordering until its actual acknowledgement.
        // Dropping an async PostgreSQL query on a local timeout could let a
        // late server write overwrite a later deletion/terminal snapshot.
        // JSON sync I/O and database settlement have no claimed hard deadline.
        if persist.save_action_candidate().await.is_err() {
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
        Ok(())
    }
}
