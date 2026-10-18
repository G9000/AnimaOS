//! Host-local physical capacity. Neither script input nor a token chooses scope.
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use super::config::ToolRpcConfig;
use crate::agent_runs::action_receipts::RunActionJournal;
use anima_core::{
    tool_rpc::{decode_program_request, ProgramOutcome, ProgramRequest, ProgramSummary},
    AgentState, CancelSignal, Content, DataValue, Message, TaskResult, ToolCall,
};
use async_trait::async_trait;
use tokio::{sync::oneshot, task::JoinHandle, time::Instant};

use super::{
    transport::{Dispatch, Transport},
    worker::{WorkerBootstrap, WorkerConfig, WorkerFailure, WorkerRuntime},
    Broker, BrokerDisposition, BrokerReply,
};

#[derive(Clone)]
pub(crate) struct ToolRpcService {
    config: ToolRpcConfig,
    runtime: Arc<WorkerRuntime>,
    cells: Arc<CellLeases>,
}
impl ToolRpcService {
    pub(crate) fn new(config: ToolRpcConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            runtime: Arc::new(WorkerRuntime::shared()),
            cells: CellLeases::new(),
        })
    }
    pub(crate) fn disabled() -> Arc<Self> {
        Self::new(ToolRpcConfig::default())
    }
    pub(crate) async fn execute(
        &self,
        context: crate::tools::ToolExecutionContext,
        agent: AgentState,
        input: Message,
        outer: ToolCall,
    ) -> TaskResult<Content> {
        if self.config.interpreter.is_none() || !cfg!(windows) {
            return TaskResult::error("Tool RPC physical interpreter is unavailable; explicit supported configuration is required", 0);
        }
        let plain = crate::routes::data_value_to_json(&DataValue::Object(outer.args.clone()));
        let Ok(bytes) = serde_json::to_vec(&plain) else {
            return TaskResult::error("Invalid tool RPC arguments", 0);
        };
        if !reviewable_arguments(&bytes) {
            return TaskResult::error("Tool RPC source exceeds the complete approval review budget or has invalid arguments", 0);
        }
        let Ok(request) = decode_program_request(&bytes) else {
            return TaskResult::error("Invalid tool RPC program", 0);
        };
        let (Some(run), Some(cancel), Some(journal), Some(workspace)) = (
            context.run_link.clone(),
            context.cancel.clone(),
            context.rpc_journal(),
            context.workspace_root.clone(),
        ) else {
            return TaskResult::error(
                "Tool RPC requires an authentic ordinary run and workspace",
                0,
            );
        };
        let deadline = journal
            .rpc_deadline()
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(300))
            .min(Instant::now() + Duration::from_secs(300));
        let journal = journal.clone();
        let broker = match Broker::capture(context, agent, input, outer).await {
            Ok(broker) => broker,
            Err(_) => return TaskResult::error("Tool RPC native admission refused", 0),
        };
        let Some(lease) = self.cells.acquire(&run.agent_id, &run.session_id) else {
            return TaskResult::error(
                "Tool RPC physical capacity is occupied for this scope or host",
                0,
            );
        };
        let service = self.clone();
        let mut caller = CancelCaller {
            cancel: cancel.clone(),
            armed: true,
        };
        let (sender, receiver) = oneshot::channel();
        let response = OwnerResponse {
            sender: Some(sender),
            journal: journal.clone(),
        };
        // No resource-owning await occurs between cell admission and registering
        // this independent owner. Caller drop signals cancellation, never aborts it.
        self.cells.keep(tokio::spawn(async move {
            execute_owned(
                service, lease, broker, request, workspace, cancel, deadline, response,
            )
            .await;
        }));
        let result = receiver
            .await
            .unwrap_or_else(|_| project_worker_failure(&journal, WorkerFailure::CleanupUnknown));
        caller.armed = false;
        result
    }
    #[cfg(test)]
    pub(super) fn active_cells(&self) -> usize {
        self.cells
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
    }
}

/// Retained before spawning the owner. Losing its response without a typed
/// outcome latches uncertainty during owner teardown, before channel closure.
struct OwnerResponse {
    sender: Option<oneshot::Sender<TaskResult<Content>>>,
    journal: RunActionJournal,
}
impl OwnerResponse {
    fn send(&mut self, result: TaskResult<Content>) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(result);
        }
    }
}
impl Drop for OwnerResponse {
    fn drop(&mut self) {
        if self.sender.is_some() {
            self.journal.stop_for_unproven_effects("Tool RPC owner ended without an acknowledged physical outcome; inspect effects before retrying");
        }
    }
}

struct CancelCaller {
    cancel: CancelSignal,
    armed: bool,
}
impl Drop for CancelCaller {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.cancel();
        }
    }
}

struct NativeDispatch {
    broker: Broker,
    summary: Arc<Mutex<ProgramSummary>>,
}
#[async_trait]
impl Dispatch for NativeDispatch {
    async fn dispatch(&mut self, bytes: &[u8]) -> BrokerReply {
        let reply = self.broker.request_reply(bytes).await;
        let mut summary = self.broker.summary(Some(0));
        if reply.disposition == BrokerDisposition::ReviewRequired {
            summary.outcome = ProgramOutcome::ReviewRequired;
        }
        *self.summary.lock().unwrap_or_else(|p| p.into_inner()) = summary;
        reply
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_owned(
    service: ToolRpcService,
    lease: CellLease,
    broker: Broker,
    request: ProgramRequest,
    workspace: std::path::PathBuf,
    cancel: CancelSignal,
    deadline: Instant,
    mut response: OwnerResponse,
) {
    let credentials = broker.credentials();
    let descriptors = broker.captured_descriptors().into_iter().map(|descriptor| {
        serde_json::json!({"name":descriptor.name, "description":descriptor.description,
            "parametersSchema":crate::routes::data_value_to_json(&DataValue::Object(descriptor.parameters_schema))})
    }).collect();
    let summary = Arc::new(Mutex::new(broker.summary(Some(0))));
    let transport = match Transport::bind(
        Box::new(NativeDispatch {
            broker,
            summary: summary.clone(),
        }),
        deadline,
    )
    .await
    {
        Ok(transport) => transport,
        Err(_) => {
            response.send(TaskResult::error(
                "Tool RPC private transport is unavailable",
                0,
            ));
            return;
        }
    };
    let endpoint = transport.endpoint;
    let launcher = Arc::new(super::windows::CellLauncher::new(CellResources {
        lease,
        transport,
    }));
    let result = service
        .runtime
        .run_with_descriptors(
            WorkerConfig {
                interpreter: service.config.interpreter.clone(),
                workspace,
            },
            request,
            WorkerBootstrap {
                endpoint,
                token: credentials.token.clone(),
                cell_epoch: credentials.cell_epoch.clone(),
            },
            descriptors,
            cancel,
            deadline,
            launcher.clone(),
        )
        .await;
    if matches!(&result, Err(WorkerFailure::CleanupUnknown)) {
        // Latch before any cleanup handoff/response work. Exact unknown worker
        // resources and their capacity remain retained by the physical owner.
        response.journal.stop_for_unproven_effects(
            "Tool RPC physical lifetime is unproven; inspect effects before retrying",
        );
    }
    // A prelaunch refusal may leave the never-started transport in the launcher.
    // Transfer it to an independent exact retirement owner, not a dropped future.
    let retirement = launcher.retire_pending();
    if let Some(mut pending) = retirement {
        pending.transport.close();
        if pending.transport.retire().await.is_err() {
            let result = project_worker_failure(&response.journal, WorkerFailure::CleanupUnknown);
            pending.quarantine();
            response.send(result);
            return;
        }
        drop(pending);
    }
    let mut native = *summary.lock().unwrap_or_else(|p| p.into_inner());
    let output = match result {
        Ok(worker) => {
            if worker.exit_code != 0 && native.outcome != ProgramOutcome::ReviewRequired {
                native.outcome = ProgramOutcome::Failed;
            }
            let redact = |bytes: Vec<u8>| {
                String::from_utf8_lossy(&bytes)
                    .replace(&credentials.token, "[redacted cell credential]")
                    .replace(&credentials.cell_epoch, "[redacted cell epoch]")
            };
            let text = serde_json::json!({"mode":worker.mode, "exitCode":worker.exit_code,
                "stdout":redact(worker.stdout),"stderr":redact(worker.stderr),"cleanupVerified":worker.cleanup_verified,
                "outcome":native.outcome,"configuredCallLimit":native.configured_call_limit,
                "effectiveCallLimit":native.effective_call_limit,"attempts":native.attempts,
                "dispatchedCalls":native.dispatched_calls,"failedCalls":native.failed_calls,"resultBytes":native.result_bytes}).to_string();
            if native.outcome == ProgramOutcome::Completed {
                TaskResult::success(
                    Content {
                        text,
                        ..Default::default()
                    },
                    0,
                )
            } else {
                TaskResult::error(text, 0)
            }
        }
        Err(failure) => project_worker_failure(&response.journal, failure),
    };
    response.send(output);
}

fn worker_error(failure: WorkerFailure) -> TaskResult<Content> {
    TaskResult::error(
        serde_json::json!({"mode":"one_shot","error":format!("{failure:?}"),
        "outcome":if failure == WorkerFailure::CleanupUnknown { ProgramOutcome::ReviewRequired } else { ProgramOutcome::Failed },
        "cleanupVerified":failure != WorkerFailure::CleanupUnknown,
        "automaticRetry":false})
        .to_string(),
        0,
    )
}

/// Typed physical-outcome projection; never classify uncertainty from text.
pub(super) fn project_worker_failure(
    journal: &RunActionJournal,
    failure: WorkerFailure,
) -> TaskResult<Content> {
    if failure == WorkerFailure::CleanupUnknown {
        journal.stop_for_unproven_effects(
            "Tool RPC physical lifetime is unproven; inspect effects before retrying",
        );
    }
    worker_error(failure)
}

pub(super) struct CellLeases {
    active: Mutex<BTreeSet<(String, String)>>,
    owners: Mutex<Vec<JoinHandle<()>>>,
    quarantine: Mutex<Vec<CellResources>>,
}

/// Moves with the real owned worker before process creation. A pending logical
/// response cannot release either exact scope or global capacity.
pub(super) struct CellResources {
    pub(super) lease: CellLease,
    pub(super) transport: Transport,
}
impl CellResources {
    pub(super) fn quarantine(self) {
        let state = self.lease.owner.clone();
        state
            .quarantine
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(self);
    }
    pub(super) fn retire_later(mut self) {
        self.transport.close();
        let state = self.lease.owner.clone();
        state.keep(tokio::spawn(async move {
            if self.transport.retire().await.is_err() {
                self.quarantine();
            }
        }));
    }
}
pub(super) struct CellLease {
    owner: Arc<CellLeases>,
    scope: (String, String),
}

impl Drop for CellLease {
    fn drop(&mut self) {
        self.owner
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.scope);
    }
}

impl CellLeases {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            active: Mutex::new(BTreeSet::new()),
            owners: Mutex::new(Vec::new()),
            quarantine: Mutex::new(Vec::new()),
        })
    }

    fn keep(&self, owner: JoinHandle<()>) {
        let mut owners = self.owners.lock().unwrap_or_else(|p| p.into_inner());
        owners.retain(|task| !task.is_finished());
        owners.push(owner);
    }

    pub(super) fn acquire(self: &Arc<Self>, agent: &str, session: &str) -> Option<CellLease> {
        let scope = (agent.to_owned(), session.to_owned());
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if active.len() >= 4 || active.contains(&scope) {
            return None;
        }
        active.insert(scope.clone());
        Some(CellLease {
            owner: self.clone(),
            scope,
        })
    }
}

pub(crate) fn reviewable_arguments(bytes: &[u8]) -> bool {
    bytes.len() <= crate::approvals::MAX_APPROVAL_ARGUMENTS_BYTES
        && decode_program_request(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fp06_service_global_four_and_exact_scope_remain_owned_until_retirement() {
        let leases = CellLeases::new();
        let one = leases.acquire("agent-a", "session-a").expect("first owner");
        assert!(leases.acquire("agent-a", "session-a").is_none());
        let two = leases
            .acquire("agent-a", "session-b")
            .expect("different session");
        let three = leases
            .acquire("agent-b", "session-a")
            .expect("different agent");
        let four = leases
            .acquire("agent-c", "session-c")
            .expect("fourth owner");
        assert!(leases.acquire("agent-d", "session-d").is_none());
        // A pending logical result cannot drop this physical lease. Only the
        // owner retires it after Job, listener, broker and pipes are proven done.
        let pending_cleanup_owner = one;
        assert!(leases.acquire("agent-a", "session-a").is_none());
        drop(pending_cleanup_owner);
        assert!(leases.acquire("agent-a", "session-a").is_some());
        drop((two, three, four));
    }

    #[test]
    fn fp06_service_refuses_serialized_source_that_full_preview_cannot_cover() {
        let small =
            serde_json::to_vec(&serde_json::json!({"code":"print(1)","reset":false})).unwrap();
        assert!(reviewable_arguments(&small));
        let escaped = serde_json::to_vec(&serde_json::json!({"code":"x\n".repeat(6000)})).unwrap();
        assert!(escaped.len() > 16_384);
        assert!(!reviewable_arguments(&escaped));
        assert!(!reviewable_arguments(
            br#"{"code":"print(1)","tools":["bash"]}"#
        ));
    }
}
