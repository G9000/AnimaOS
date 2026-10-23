use super::*;
use crate::{
    Content, ModelAdapter, ModelGenerateResponse, ModelStopReason, RunFrame, RunObserver,
    TaskResult,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn config() -> AgentConfig {
    serde_json::from_value(serde_json::json!({"name":"regression","model":"test"})).unwrap()
}
fn input() -> Content {
    Content {
        text: "test".into(),
        ..Content::default()
    }
}
fn context() -> ModelCallContext {
    ModelCallContext::new(
        ExecutionContext::ephemeral(
            "run".into(),
            ExecutionLimits::default(),
            CancelSignal::new(),
        )
        .unwrap(),
        "logical".into(),
    )
}

#[tokio::test]
async fn execution_limits_concurrent_media_cannot_reserve_zero_input() {
    let context = context();
    let request = ModelGenerateRequest {
        system: "test".into(),
        messages: vec![crate::Message {
            id: "m".into(),
            agent_id: "a".into(),
            room_id: "r".into(),
            role: crate::MessageRole::User,
            created_at_ms: 1,
            content: Content {
                attachments: Some(vec![crate::Attachment {
                    attachment_type: crate::AttachmentType::Image,
                    name: "image".into(),
                    data: "opaque".into(),
                    mime_type: Some("image/png".into()),
                }]),
                ..input()
            },
        }],
        temperature: None,
        max_tokens: Some(1),
    };
    let (_, _first) = context.reserve_prepared(&config(), &request).await.unwrap();
    assert!(
        context.reserve_prepared(&config(), &request).await.is_err(),
        "second opaque request was admitted with zero input"
    );
}

#[tokio::test]
async fn execution_limits_overrun_revokes_other_granted_permits() {
    let context = context();
    let mut first = context.reserve_model_attempt(100, 10).await.unwrap();
    let second = context.reserve_model_attempt(100, 10).await.unwrap();
    first
        .finish(SettlementOutcome::Known(TokenUsage {
            prompt_tokens: 4_000_001,
            ..TokenUsage::default()
        }))
        .await
        .unwrap();
    assert!(
        second.revalidate().await.is_err(),
        "a granted permit survived a shared overrun"
    );
}

struct PendingPort;
#[async_trait]
impl ExecutionBudgetPort for PendingPort {
    async fn reserve(
        &self,
        _: &str,
        _: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        futures::future::pending().await
    }
    fn submit_settlement(
        &self,
        _: &str,
        _: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        futures::future::pending().boxed()
    }
    async fn allowance(&self, _: &str) -> Result<ExecutionAllowance, String> {
        futures::future::pending().await
    }
    fn abandon(&self, _: &str, _: ReservationSettlement) {}
}

#[tokio::test]
async fn execution_limits_cancel_interrupts_stalled_accounting_wait() {
    let mut context = context();
    context.execution.port = Arc::new(PendingPort);
    let cancel = context.execution.cancel.clone();
    let work = context.execution.check_authority().fuse();
    let stop = async {
        futures_timer::Delay::new(std::time::Duration::from_millis(5)).await;
        cancel.cancel();
    }
    .fuse();
    let watchdog = futures_timer::Delay::new(std::time::Duration::from_millis(100)).fuse();
    futures::pin_mut!(work, stop, watchdog);
    futures::select_biased! { _ = stop => {}, _ = work => panic!("unexpected completion") }
    futures::select_biased! { result = work => assert!(result.is_err()), _ = watchdog => panic!("cancel left an accounting wait hanging") }
}

struct ToolModel(AtomicUsize);
#[async_trait]
impl ModelAdapter for ToolModel {
    fn provider(&self) -> &str {
        "test"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
        Ok(ModelGenerateResponse {
            content: input(),
            usage: TokenUsage::default(),
            tool_calls: first.then(|| {
                vec![crate::ToolCall {
                    id: "tool".into(),
                    name: "echo".into(),
                    args: BTreeMap::new(),
                }]
            }),
            stop_reason: if first {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            },
        })
    }
}
fn runtime(model: Arc<ToolModel>) -> crate::AgentRuntime {
    let mut config = config();
    config.tools = Some(vec![crate::ToolDescriptor {
        name: "echo".into(),
        description: "test".into(),
        parameters_schema: BTreeMap::new(),
        examples: None,
    }]);
    let mut runtime = crate::AgentRuntime::new(config, model);
    runtime.set_execution_context(context().execution).unwrap();
    runtime
}
struct StopOnTool(CancelSignal);
impl RunObserver for StopOnTool {
    fn on_frame(&self, frame: RunFrame) {
        if matches!(frame, RunFrame::ToolStarted { .. }) {
            self.0.cancel();
        }
    }
}

#[tokio::test]
async fn execution_limits_observer_stop_prevents_synchronous_executor_prefix() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let mut runtime = runtime(model.clone());
    runtime.set_run_observer(Arc::new(StopOnTool(
        runtime.execution_context().unwrap().cancel.clone(),
    )));
    let calls = AtomicUsize::new(0);
    runtime
        .run_with_tools(input(), |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { TaskResult::success(input(), 0) }
        })
        .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(model.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn execution_limits_scoped_snapshot_requires_explicit_reattachment() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let runtime = runtime(model.clone());
    let mut restored = crate::AgentRuntime::from_snapshot(runtime.snapshot(), model.clone());
    let result = restored.run(input()).await;
    assert_eq!(
        model.0.load(Ordering::SeqCst),
        0,
        "restore silently removed execution authority"
    );
    assert_eq!(result.status, crate::TaskStatus::Error);
}

#[tokio::test]
async fn execution_limits_started_effect_keeps_custody_until_real_retirement() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let mut runtime = runtime(model.clone());
    let cancel = runtime.execution_context().unwrap().cancel.clone();
    let retired = Arc::new(AtomicUsize::new(0));
    let observed = retired.clone();
    let result = runtime
        .run_with_tools(input(), move |_, _, _| {
            let cancel = cancel.clone();
            let observed = observed.clone();
            async move {
                cancel.cancel();
                futures_timer::Delay::new(std::time::Duration::from_millis(5)).await;
                observed.store(1, Ordering::SeqCst);
                TaskResult::success(
                    Content {
                        text: "effect completed".into(),
                        ..Content::default()
                    },
                    0,
                )
            }
        })
        .await;
    assert_eq!(
        retired.load(Ordering::SeqCst),
        1,
        "started effect future was abandoned on Stop"
    );
    assert!(
        runtime
            .messages()
            .iter()
            .any(|message| message.role == crate::MessageRole::Tool
                && message.content.text.contains("effect completed")),
        "real settled effect was replaced with a synthetic failure"
    );
    assert_eq!(
        runtime.state().status,
        crate::AgentStatus::Idle,
        "OwnerStop must retain established Idle semantics"
    );
    assert_eq!(model.0.load(Ordering::SeqCst), 1);
    assert_eq!(result.status, crate::TaskStatus::Error);
}

struct FailedAccounting {
    inner: Arc<dyn ExecutionBudgetPort>,
    submitted: AtomicUsize,
}
#[async_trait]
impl ExecutionBudgetPort for FailedAccounting {
    async fn reserve(
        &self,
        scope: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        self.inner.reserve(scope, request).await
    }
    async fn allowance(&self, scope: &str) -> Result<ExecutionAllowance, String> {
        self.inner.allowance(scope).await
    }
    fn submit_settlement(
        &self,
        _: &str,
        _: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        self.submitted.fetch_add(1, Ordering::SeqCst);
        futures::future::ready(Err("storage acknowledgement lost".into())).boxed()
    }
}

#[tokio::test]
async fn execution_limits_accounting_failure_preserves_known_model_and_pairs() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let mut runtime = runtime(model.clone());
    let mut execution = runtime.execution_context().unwrap().clone();
    let port = Arc::new(FailedAccounting {
        inner: execution.port.clone(),
        submitted: AtomicUsize::new(0),
    });
    execution.port = port.clone();
    runtime.set_execution_context(execution).unwrap();
    let calls = AtomicUsize::new(0);
    let result = runtime
        .run_with_tools(input(), |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { TaskResult::success(input(), 0) }
        })
        .await;
    assert_eq!(port.submitted.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(model.0.load(Ordering::SeqCst), 1);
    assert_eq!(result.status, crate::TaskStatus::Error);
    assert!(runtime
        .messages()
        .iter()
        .any(|message| message.role == crate::MessageRole::Assistant
            && message.content.text == "test"));
    assert_eq!(
        runtime
            .messages()
            .iter()
            .filter(|message| message.role == crate::MessageRole::Tool)
            .count(),
        1
    );
}

#[tokio::test]
async fn execution_limits_finished_accounting_is_submitted_even_after_stop() {
    let mut context = context();
    let port = Arc::new(FailedAccounting {
        inner: context.execution.port.clone(),
        submitted: AtomicUsize::new(0),
    });
    context.execution.port = port.clone();
    let mut permit = context.reserve_model_attempt(100, 10).await.unwrap();
    context.execution.cancel.cancel();
    assert!(permit
        .finish(SettlementOutcome::Known(TokenUsage::default()))
        .await
        .is_err());
    assert_eq!(
        port.submitted.load(Ordering::SeqCst),
        1,
        "already-cancelled ack waiter skipped submitting known accounting"
    );
}

#[tokio::test]
async fn execution_limits_matching_reattachment_does_not_reset_exhausted_scope() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let context = ExecutionContext::ephemeral(
        "new-run".into(),
        ExecutionLimits {
            max_model_attempts: 1,
            max_retries: 0,
            ..ExecutionLimits::default()
        },
        CancelSignal::new(),
    )
    .unwrap();
    // Use a fresh runtime: replacing an already-bound scope is forbidden.
    let mut runtime = crate::AgentRuntime::new(config(), model.clone());
    runtime.set_execution_context(context.clone()).unwrap();
    let mut permit = ModelCallContext::new(context.clone(), "spent".into())
        .reserve_model_attempt(100, 10)
        .await
        .unwrap();
    permit
        .finish(SettlementOutcome::Known(TokenUsage::default()))
        .await
        .unwrap();
    let mut restored = crate::AgentRuntime::from_snapshot(runtime.snapshot(), model.clone());
    assert!(restored
        .set_execution_context(
            ExecutionContext::ephemeral(
                "new-run".into(),
                ExecutionLimits::default(),
                CancelSignal::new()
            )
            .unwrap()
        )
        .is_err());
    restored.set_execution_context(context).unwrap();
    let result = restored.run(input()).await;
    assert_eq!(result.status, crate::TaskStatus::Error);
    assert_eq!(model.0.load(Ordering::SeqCst), 0);
}

struct RecordingPort {
    inner: Arc<dyn ExecutionBudgetPort>,
    last: Mutex<Option<ReservationSettlement>>,
}
#[async_trait]
impl ExecutionBudgetPort for RecordingPort {
    async fn reserve(
        &self,
        scope: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        self.inner.reserve(scope, request).await
    }
    async fn allowance(&self, scope: &str) -> Result<ExecutionAllowance, String> {
        self.inner.allowance(scope).await
    }
    fn submit_settlement(
        &self,
        scope: &str,
        settlement: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        *self.last.lock().unwrap() = Some(settlement.clone());
        self.inner.submit_settlement(scope, settlement)
    }
}
struct NullSink;
#[async_trait]
impl ModelStreamSink for NullSink {
    async fn emit(&self, _: ModelStreamFrame) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn execution_limits_complete_final_preserves_higher_stream_observation() {
    let mut context = context();
    let port = Arc::new(RecordingPort {
        inner: context.execution.port.clone(),
        last: Mutex::new(None),
    });
    context.execution.port = port.clone();
    let mut permit = context.reserve_model_attempt(100, 10).await.unwrap();
    let sink = permit.stream_sink(&NullSink);
    sink.emit(ModelStreamFrame::Usage(TokenUsage {
        prompt_tokens: 500,
        ..TokenUsage::default()
    }))
    .await
    .unwrap();
    sink.complete_usage(TokenUsage {
        prompt_tokens: 100,
        ..TokenUsage::default()
    })
    .await
    .unwrap();
    drop(sink);
    permit.finish(permit.observed_outcome()).await.unwrap();
    let settlement = port.last.lock().unwrap().clone().unwrap();
    assert!(matches!(
        settlement.outcome,
        SettlementOutcome::Known(TokenUsage {
            prompt_tokens: 100,
            ..
        })
    ));
    assert_eq!(settlement.observed_usage.unwrap().prompt_tokens, 500);
}

#[tokio::test]
async fn execution_limits_unattested_success_never_refunds_missing_statistics() {
    let mut context = context();
    let port = Arc::new(RecordingPort {
        inner: context.execution.port.clone(),
        last: Mutex::new(None),
    });
    context.execution.port = port.clone();
    let adapter = ToolModel(AtomicUsize::new(0));
    adapter
        .generate_controlled(
            &config(),
            &ModelGenerateRequest {
                system: "test".into(),
                messages: vec![],
                temperature: None,
                max_tokens: Some(5),
            },
            &context,
        )
        .await
        .unwrap();
    assert!(matches!(
        port.last.lock().unwrap().as_ref().unwrap().outcome,
        SettlementOutcome::Unknown { .. }
    ));
}

#[tokio::test]
async fn execution_limits_request_cancel_interrupts_stalled_allowance() {
    let mut context = context();
    context.execution.port = Arc::new(PendingPort);
    let cancel = context.request_cancel.clone();
    let request = ModelGenerateRequest {
        system: "test".into(),
        messages: vec![],
        temperature: None,
        max_tokens: Some(1),
    };
    let config = config();
    let work = context.prepare_request(&config, &request).fuse();
    let stop = async {
        futures_timer::Delay::new(std::time::Duration::from_millis(5)).await;
        cancel.cancel();
    }
    .fuse();
    let watchdog = futures_timer::Delay::new(std::time::Duration::from_millis(100)).fuse();
    futures::pin_mut!(work, stop, watchdog);
    futures::select_biased! { _ = stop => {}, _ = work => panic!("unexpected completion") }
    futures::select_biased! { result = work => assert!(result.is_err()), _ = watchdog => panic!("local cancellation left an allowance wait hanging") }
}

#[test]
fn execution_limits_new_independent_input_requires_deliberate_fresh_admission() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let mut runtime = runtime(model);
    let next = ExecutionContext::ephemeral(
        "next-independent-input".into(),
        ExecutionLimits::default(),
        CancelSignal::new(),
    )
    .unwrap();
    assert!(
        runtime.set_execution_context(next.clone()).is_err(),
        "matching reattachment must never mint a new scope"
    );
    runtime
        .begin_new_execution_context(next)
        .expect("trusted new independent input needs an explicit fresh-admission API");
    assert_eq!(
        runtime.execution_context().unwrap().run_id,
        "next-independent-input"
    );
}

struct AdvancingClock {
    now: std::sync::atomic::AtomicU64,
    advanced: CancelSignal,
}
#[async_trait]
impl ExecutionClock for AdvancingClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    async fn wait_until_ms(&self, deadline: u64) {
        if self.now_ms() < deadline {
            self.advanced.cancelled().await;
        }
    }
}
struct HangingCheckpoint(CancelSignal);
#[async_trait]
impl crate::turn_recovery::TurnCheckpointSink for HangingCheckpoint {
    async fn save(&self, _: crate::turn_recovery::TurnCheckpoint) -> Result<(), String> {
        self.0.cancel();
        futures::future::pending().await
    }
}

#[tokio::test]
async fn execution_limits_checkpoint_acknowledgement_cannot_outlive_dispatch_deadline() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let mut runtime = crate::AgentRuntime::new(config(), model.clone());
    let clock = Arc::new(AdvancingClock {
        now: std::sync::atomic::AtomicU64::new(100),
        advanced: CancelSignal::new(),
    });
    let scope = ExecutionScope::new(
        "clock-scope".into(),
        "run".into(),
        ExecutionLimits::default(),
        100,
    )
    .unwrap();
    let execution = ExecutionContext {
        scope_id: scope.id.clone(),
        run_id: "run".into(),
        deadline_at_ms: scope.deadline_at_ms,
        port: Arc::new(InMemoryExecutionBudget::new(scope, clock.clone()).unwrap()),
        clock: clock.clone(),
        cancel: CancelSignal::new(),
        secondary_cancel: None,
        failures: Arc::new(Mutex::new(None)),
    };
    let deadline = execution.deadline_at_ms;
    runtime.set_execution_context(execution).unwrap();
    let entered = CancelSignal::new();
    runtime.set_turn_checkpoint_sink(
        crate::turn_recovery::TurnBinding {
            root_run_id: "run".into(),
            attempt_run_id: "run".into(),
            agent_id: runtime.id().into(),
            session_id: "session".into(),
            original_input_digest: "a".repeat(64),
            attempt_input_digest: "a".repeat(64),
            workspace_digest: "b".repeat(64),
            config_digest: "c".repeat(64),
            automatic_eligible: true,
        },
        Arc::new(HangingCheckpoint(entered.clone())),
    );
    let run = runtime.run(input());
    let advance = async {
        entered.cancelled().await;
        clock.now.store(deadline, Ordering::SeqCst);
        clock.advanced.cancel();
    };
    let work = async { futures::join!(run, advance).0 }.fuse();
    let watchdog = futures_timer::Delay::new(std::time::Duration::from_millis(200)).fuse();
    futures::pin_mut!(work, watchdog);
    futures::select_biased! { result = work => assert_eq!(result.status, crate::TaskStatus::Error), _ = watchdog => panic!("checkpoint acknowledgement ignored the immutable deadline") }
    assert_eq!(model.0.load(Ordering::SeqCst), 0);
}

struct RejectRespondedCheckpoint;
#[async_trait]
impl crate::turn_recovery::TurnCheckpointSink for RejectRespondedCheckpoint {
    async fn save(&self, checkpoint: crate::turn_recovery::TurnCheckpoint) -> Result<(), String> {
        if checkpoint.phase == crate::turn_recovery::TurnPhase::ModelResponded {
            Err("responded checkpoint failed".into())
        } else {
            Ok(())
        }
    }
}
struct KnownCheckpointModel(ToolModel);
#[async_trait]
impl ModelAdapter for KnownCheckpointModel {
    fn provider(&self) -> &str {
        "test"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let mut result = self.0.generate(config, request).await?;
        result.usage = TokenUsage {
            prompt_tokens: 13,
            completion_tokens: 4,
            total_tokens: 17,
            ..Default::default()
        };
        Ok(result)
    }
}
#[tokio::test]
async fn execution_limits_failed_responded_checkpoint_retains_final_and_unexecuted_pairs() {
    let model = Arc::new(KnownCheckpointModel(ToolModel(AtomicUsize::new(0))));
    let mut config = config();
    config.tools = Some(vec![crate::ToolDescriptor {
        name: "echo".into(),
        description: "test".into(),
        parameters_schema: BTreeMap::new(),
        examples: None,
    }]);
    let mut runtime = crate::AgentRuntime::new(config, model.clone());
    runtime.set_execution_context(context().execution).unwrap();
    runtime.set_turn_checkpoint_sink(
        crate::turn_recovery::TurnBinding {
            root_run_id: "run".into(),
            attempt_run_id: "run".into(),
            agent_id: runtime.id().into(),
            session_id: "session".into(),
            original_input_digest: "a".repeat(64),
            attempt_input_digest: "a".repeat(64),
            workspace_digest: "b".repeat(64),
            config_digest: "c".repeat(64),
            automatic_eligible: true,
        },
        Arc::new(RejectRespondedCheckpoint),
    );
    let physical = AtomicUsize::new(0);
    let result = runtime
        .run_with_tools(input(), |_, _, _| {
            physical.fetch_add(1, Ordering::SeqCst);
            async { TaskResult::success(input(), 0) }
        })
        .await;
    assert_eq!(result.status, crate::TaskStatus::Error);
    assert_eq!(physical.load(Ordering::SeqCst), 0);
    assert_eq!(model.0 .0.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.state().token_usage.total_tokens, 17);
    assert!(
        runtime
            .messages()
            .iter()
            .any(|message| message.role == crate::MessageRole::Assistant
                && message.content.text == "test"),
        "known Final disappeared after checkpoint acknowledgement failed"
    );
    assert_eq!(
        runtime
            .messages()
            .iter()
            .filter(|message| message.role == crate::MessageRole::Tool)
            .count(),
        1,
        "unexecuted tool intent must retain its protocol pair"
    );
}

#[tokio::test]
async fn execution_limits_stopped_admission_attaches_for_terminal_recording_without_dispatch() {
    let model = Arc::new(ToolModel(AtomicUsize::new(0)));
    let mut runtime = runtime(model.clone());
    let next = ExecutionContext::ephemeral(
        "stopped-before-attachment".into(),
        ExecutionLimits::default(),
        CancelSignal::new(),
    )
    .unwrap();
    next.cancel.cancel();
    runtime
        .begin_new_execution_context(next)
        .expect("already admitted custody must attach after Stop");
    let result = runtime
        .run_with_tools(input(), |_, _, _| async {
            panic!("stopped input dispatched a tool")
        })
        .await;
    assert_eq!(result.status, crate::TaskStatus::Error);
    assert_eq!(result.error.as_deref(), Some("stopped"));
    assert_eq!(model.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn execution_limits_adapter_stop_winning_stream_race_keeps_stopped_partial() {
    struct StopsInStream(CancelSignal);
    #[async_trait]
    impl ModelAdapter for StopsInStream {
        fn provider(&self) -> &str {
            "stop-race"
        }
        fn single_attempt_contract(&self) -> bool {
            true
        }
        async fn generate(
            &self,
            _: &AgentConfig,
            _: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            panic!("stream fixture only")
        }
        async fn stream(
            &self,
            _: &AgentConfig,
            _: &ModelGenerateRequest,
            sink: &dyn ModelStreamSink,
        ) -> Result<(), String> {
            sink.emit(ModelStreamFrame::TextDelta("partial before Stop".into()))
                .await?;
            self.0.cancel();
            Err("stopped".into())
        }
    }
    let cancel = CancelSignal::new();
    let model = Arc::new(StopsInStream(cancel.clone()));
    let mut runtime = crate::AgentRuntime::new(config(), model);
    runtime
        .set_execution_context(
            ExecutionContext::ephemeral(
                "stop-race".into(),
                ExecutionLimits::default(),
                cancel.clone(),
            )
            .unwrap(),
        )
        .unwrap();
    runtime.set_run_control(crate::RunControl {
        cancel,
        ..Default::default()
    });
    let result = runtime
        .run_with_tools(input(), |_, _, _| async {
            panic!("Stop dispatched a tool")
        })
        .await;
    assert_eq!(result.error.as_deref(), Some("stopped"));
    let snapshot = runtime.snapshot();
    let partial = snapshot
        .messages
        .iter()
        .find(|message| message.role == crate::MessageRole::Assistant)
        .expect("partial remains");
    assert_eq!(partial.content.text, "partial before Stop");
    assert_eq!(
        partial.content.metadata.as_ref().unwrap().get("stopped"),
        Some(&crate::DataValue::Bool(true))
    );
    assert_ne!(snapshot.state.status, crate::AgentStatus::Failed);
}
