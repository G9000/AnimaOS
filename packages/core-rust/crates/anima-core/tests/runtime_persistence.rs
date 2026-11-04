mod support;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use anima_core::{
    AgentConfig, AgentRuntime, CancelSignal, Content, DataValue, DatabaseAdapter, MessageRole,
    ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, PersistenceError, PersistenceResult,
    RunControl, Step, StepStatus, TaskResult, TaskStatus, RUN_STOPPED_ERROR,
};
use async_trait::async_trait;

#[derive(Clone, Debug)]
enum Fault {
    Lookup,
    Terminal(StepStatus, Option<serde_json::Value>),
    Pending,
    PendingWrite,
}

struct FaultDatabase {
    fault: Fault,
    lookups: AtomicUsize,
    writes: AtomicUsize,
    stop_on_fault: Option<CancelSignal>,
}

#[async_trait]
impl DatabaseAdapter for FaultDatabase {
    async fn write_step(&self, _: &Step) -> PersistenceResult<()> {
        let index = self.writes.fetch_add(1, Ordering::SeqCst);
        if matches!(self.fault, Fault::PendingWrite) && index == 1 {
            return Err(PersistenceError::Write(
                "injected pending write failure".into(),
            ));
        }
        Ok(())
    }

    async fn get_step_by_idempotency_key(
        &self,
        agent_id: &str,
        key: &str,
    ) -> PersistenceResult<Option<Step>> {
        // The first call is safe: a failure in the second must block the entire batch.
        if self.lookups.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(None);
        }
        if let Some(stop) = &self.stop_on_fault {
            stop.cancel();
        }
        let (status, output) = match &self.fault {
            Fault::Lookup => return Err(PersistenceError::Query("injected lookup failure".into())),
            Fault::PendingWrite => return Ok(None),
            Fault::Pending => (StepStatus::Pending, None),
            Fault::Terminal(status, output) => (status.clone(), output.clone()),
        };
        Ok(Some(Step {
            id: "earlier-attempt".into(),
            agent_id: agent_id.into(),
            step_index: 1,
            idempotency_key: key.into(),
            step_type: "tool".into(),
            status,
            input: None,
            output,
        }))
    }

    async fn list_agent_steps(&self, _: &str) -> PersistenceResult<Vec<Step>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct BatchModel {
    calls: AtomicUsize,
}

#[async_trait]
impl ModelAdapter for BatchModel {
    fn provider(&self) -> &str {
        "persistence-regression"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut response = support::ScriptedModelAdapter
            .generate(config, request)
            .await?;
        if let Some(calls) = &mut response.tool_calls {
            let mut second = calls[0].clone();
            second.id = "tool-2".into();
            calls.push(second);
        }
        Ok(response)
    }
}

async fn assert_batch_blocked(fault: Fault, expected_error: &str) {
    let database = Arc::new(FaultDatabase {
        fault: fault.clone(),
        lookups: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        stop_on_fault: None,
    });
    let model = Arc::new(BatchModel::default());
    let executions = Arc::new(AtomicUsize::new(0));
    let mut runtime = AgentRuntime::new(support::config(), model.clone());
    runtime.set_database(database.clone());
    runtime.register_provider(Arc::new(support::ClockProvider));
    runtime.init();
    let calls = executions.clone();
    let result = runtime
        .run_with_tools(
            Content {
                text: "Run the tool batch".into(),
                ..Content::default()
            },
            move |_, _, _| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    TaskResult::success(Content::default(), 0)
                }
            },
        )
        .await;

    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "{fault:?}: no tool in the batch may execute"
    );
    assert_eq!(result.status, TaskStatus::Error, "{fault:?}");
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|error| error.contains(expected_error)),
        "{fault:?}: {:?}",
        result.error
    );
    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        1,
        "must not ask the model to retry uncertain actions"
    );
    assert_eq!(database.lookups.load(Ordering::SeqCst), 2);
    let expected_writes = if matches!(fault, Fault::PendingWrite) {
        2
    } else {
        0
    };
    assert_eq!(
        database.writes.load(Ordering::SeqCst),
        expected_writes,
        "must preserve prior evidence and finish lookup before writing"
    );

    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.state.status.as_str(), "failed");
    assert_eq!(
        snapshot.last_task.as_ref().map(|task| task.status),
        Some(TaskStatus::Error)
    );
    let results: Vec<_> = snapshot
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .collect();
    assert_eq!(
        results.len(),
        2,
        "every requested tool must have a protocol result"
    );
    for (message, call_id) in results.into_iter().zip(["tool-1", "tool-2"]) {
        assert!(message.content.text.contains(expected_error));
        assert_eq!(
            message
                .content
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("toolCallId")),
            Some(&DataValue::String(call_id.into())),
            "protocol result must match its original call"
        );
    }
    assert!(snapshot
        .events
        .iter()
        .any(|event| event.event_type.as_str() == "task:failed"));
    assert!(!snapshot
        .events
        .iter()
        .any(|event| event.event_type.as_str() == "task:completed"));
    assert!(
        !snapshot
            .events
            .iter()
            .any(|event| event.event_type.as_str() == "tool:before"),
        "failed admission must not announce a running tool"
    );
}

#[tokio::test]
async fn lookup_failure_blocks_entire_tool_batch() {
    assert_batch_blocked(Fault::Lookup, "injected lookup failure").await;
}

#[tokio::test]
async fn unreadable_terminal_result_blocks_entire_tool_batch() {
    for status in [StepStatus::Done, StepStatus::Failed] {
        assert_batch_blocked(
            Fault::Terminal(status.clone(), None),
            "terminal status without output",
        )
        .await;
        assert_batch_blocked(
            Fault::Terminal(status, Some(serde_json::json!({"invalid": true}))),
            "unreadable output",
        )
        .await;
    }
}

#[tokio::test]
async fn pending_prior_attempt_blocks_entire_tool_batch() {
    assert_batch_blocked(Fault::Pending, "pending").await;
}

#[tokio::test]
async fn pending_write_failure_blocks_entire_tool_batch() {
    assert_batch_blocked(Fault::PendingWrite, "injected pending write failure").await;
}

#[tokio::test]
async fn owner_stop_during_failed_lookup_wins_over_persistence_failure() {
    let control = RunControl::new();
    let database = Arc::new(FaultDatabase {
        fault: Fault::Lookup,
        lookups: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        stop_on_fault: Some(control.cancel.clone()),
    });
    let model = Arc::new(BatchModel::default());
    let mut runtime = AgentRuntime::new(support::config(), model.clone());
    runtime.set_database(database.clone());
    runtime.set_run_control(control);
    runtime.register_provider(Arc::new(support::ClockProvider));
    runtime.init();
    let result = runtime
        .run_with_tools(
            Content {
                text: "Run then stop during lookup".into(),
                ..Content::default()
            },
            |_, _, _| async { panic!("a stopped batch must not execute") },
        )
        .await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(result.error.as_deref(), Some(RUN_STOPPED_ERROR));
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(database.writes.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.snapshot().state.status.as_str(), "idle");
    assert_eq!(
        runtime
            .messages()
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count(),
        2
    );
    assert!(!runtime
        .events()
        .iter()
        .any(|event| event.event_type.as_str() == "agent:failed"));
}
