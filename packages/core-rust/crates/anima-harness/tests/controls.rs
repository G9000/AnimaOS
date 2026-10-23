use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anima_harness::anima_core::execution_limits::{ExecutionContext, ExecutionLimits};
use anima_harness::anima_core::turn_recovery::{TurnBinding, TurnCheckpoint, TurnCheckpointSink};
use anima_harness::anima_core::{
    AgentConfig, Content, DataValue, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, RunControl, RunFrame, RunObserver, TaskStatus, TokenUsage, ToolCall,
    ToolDescriptor,
};
use anima_harness::{Harness, HarnessTool, ToolApproval, ToolInvocation, ToolPolicy, ToolSet};
use async_trait::async_trait;

#[derive(Clone)]
struct Model {
    responses: Arc<Mutex<VecDeque<ModelGenerateResponse>>>,
    calls: Arc<AtomicUsize>,
}

impl Model {
    fn new(responses: Vec<ModelGenerateResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            calls: Arc::default(),
        }
    }
}

#[async_trait]
impl ModelAdapter for Model {
    fn provider(&self) -> &str {
        "controlled-fixture"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _response: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        _config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or("No fixture response".into())
    }
}

fn response(tool: bool) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: Content {
            text: "known answer".into(),
            ..Content::default()
        },
        tool_calls: tool.then(|| {
            vec![ToolCall {
                id: "exact-call".into(),
                name: "effect".into(),
                args: BTreeMap::from([("value".into(), DataValue::String("exact args".into()))]),
            }]
        }),
        usage: TokenUsage {
            prompt_tokens: 11,
            completion_tokens: 7,
            total_tokens: 18,
            ..TokenUsage::default()
        },
        stop_reason: if tool {
            ModelStopReason::ToolCall
        } else {
            ModelStopReason::End
        },
    }
}

struct Effect(Arc<AtomicUsize>);
#[async_trait]
impl HarnessTool for Effect {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "effect".into(),
            description: "registered effect".into(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        }
    }
    async fn execute(&self, args: BTreeMap<String, DataValue>) -> Result<Content, String> {
        assert_eq!(
            args.get("value"),
            Some(&DataValue::String("exact args".into()))
        );
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Content {
            text: "effect retired".into(),
            ..Content::default()
        })
    }
}

struct RetiringEffect {
    control: RunControl,
    retired: Arc<AtomicUsize>,
}
#[async_trait]
impl HarnessTool for RetiringEffect {
    fn descriptor(&self) -> ToolDescriptor {
        Effect(self.retired.clone()).descriptor()
    }
    async fn execute(&self, _args: BTreeMap<String, DataValue>) -> Result<Content, String> {
        self.control.cancel.cancel();
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.retired.fetch_add(1, Ordering::SeqCst);
        Ok(Content {
            text: "known retired effect".into(),
            ..Content::default()
        })
    }
}

#[derive(Default)]
struct Frames(Mutex<Vec<RunFrame>>);
impl RunObserver for Frames {
    fn on_frame(&self, frame: RunFrame) {
        self.0.lock().unwrap().push(frame);
    }
}

struct RejectCheckpoint;
#[async_trait]
impl TurnCheckpointSink for RejectCheckpoint {
    async fn save(&self, _checkpoint: TurnCheckpoint) -> Result<(), String> {
        Err("checkpoint unavailable".into())
    }
}

struct PendingCheckpoint(Option<RunControl>);
#[async_trait]
impl TurnCheckpointSink for PendingCheckpoint {
    async fn save(&self, _checkpoint: TurnCheckpoint) -> Result<(), String> {
        if let Some(control) = &self.0 {
            control.cancel.cancel();
        }
        std::future::pending().await
    }
}

fn binding() -> TurnBinding {
    TurnBinding {
        root_run_id: "root".into(),
        attempt_run_id: "attempt".into(),
        agent_id: "agent".into(),
        session_id: "session".into(),
        original_input_digest: "a".repeat(64),
        attempt_input_digest: "b".repeat(64),
        workspace_digest: "c".repeat(64),
        config_digest: "d".repeat(64),
        automatic_eligible: false,
    }
}

enum PolicyMode {
    Deny,
    Error,
    Stop(RunControl),
    Pending,
    Revoke(ExecutionContext),
    RevalidationError,
    Allow,
}
struct Policy {
    mode: PolicyMode,
    seen: Arc<Mutex<Vec<ToolInvocation>>>,
}
impl Policy {
    fn new(mode: PolicyMode) -> Self {
        Self {
            mode,
            seen: Arc::default(),
        }
    }
}
#[async_trait]
impl ToolPolicy for Policy {
    async fn approve(&self, invocation: &ToolInvocation) -> Result<ToolApproval, String> {
        self.seen.lock().unwrap().push(invocation.clone());
        match &self.mode {
            PolicyMode::Deny => Ok(ToolApproval::Denied("owner denied".into())),
            PolicyMode::Error => Err("approval service unavailable".into()),
            PolicyMode::Stop(control) => {
                control.cancel.cancel();
                std::future::pending().await
            }
            PolicyMode::Pending => std::future::pending().await,
            PolicyMode::Revoke(context) => {
                context.record_failure("host authority lost".into());
                Ok(ToolApproval::Approved)
            }
            PolicyMode::RevalidationError | PolicyMode::Allow => Ok(ToolApproval::Approved),
        }
    }
    fn revalidate(&self, _invocation: &ToolInvocation) -> Result<(), String> {
        if matches!(self.mode, PolicyMode::RevalidationError) {
            Err("approval revoked".into())
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn controls_builder_stop_prevents_model_dispatch() {
    let model = Model::new(vec![response(false)]);
    let control = RunControl::new();
    control.cancel.cancel();
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .run_control(control)
        .build()
        .unwrap();
    assert_eq!(harness.run("stop").await.status, TaskStatus::Error);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_setter_stop_prevents_model_dispatch() {
    let model = Model::new(vec![response(false)]);
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .build()
        .unwrap();
    let control = RunControl::new();
    control.cancel.cancel();
    harness.set_run_control(control);
    assert_eq!(harness.run("stop").await.status, TaskStatus::Error);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_failed_checkpoint_prevents_model_dispatch() {
    let model = Model::new(vec![response(false)]);
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .run_id("attempt")
        .turn_checkpoint(binding(), Arc::new(RejectCheckpoint))
        .build()
        .unwrap();
    let result = harness.run("checkpoint first").await;
    assert_eq!(result.status, TaskStatus::Error);
    assert!(result.error.unwrap().contains("checkpoint unavailable"));
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_stop_bounds_pending_checkpoint_acknowledgement() {
    let model = Model::new(vec![response(false)]);
    let control = RunControl::new();
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .run_id("attempt")
        .run_control(control.clone())
        .turn_checkpoint(binding(), Arc::new(PendingCheckpoint(Some(control))))
        .build()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), harness.run("checkpoint first"))
        .await
        .expect("Stop must bound checkpoint acknowledgement");
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_deadline_bounds_pending_checkpoint_acknowledgement() {
    let model = Model::new(vec![response(false)]);
    let mut limits = ExecutionLimits::default();
    limits.budget.max_wall_time_ms = Some(50);
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .run_id("attempt")
        .execution_limits(limits)
        .turn_checkpoint(binding(), Arc::new(PendingCheckpoint(None)))
        .build()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), harness.run("checkpoint first"))
        .await
        .expect("deadline must bound checkpoint acknowledgement");
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

async fn refused_policy(mode: PolicyMode) {
    let model = Model::new(vec![response(true), response(false)]);
    let effects = Arc::new(AtomicUsize::new(0));
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model))
        .tool(Effect(effects.clone()))
        .tool_policy(Arc::new(Policy::new(mode)))
        .build()
        .unwrap();
    harness.run("request effect").await;
    assert_eq!(
        effects.load(Ordering::SeqCst),
        0,
        "approval failure must prevent the actual effect"
    );
}

#[tokio::test]
async fn controls_denied_policy_prevents_effect() {
    refused_policy(PolicyMode::Deny).await;
}
#[tokio::test]
async fn controls_failed_policy_prevents_effect() {
    refused_policy(PolicyMode::Error).await;
}
#[tokio::test]
async fn controls_failed_live_revalidation_prevents_effect() {
    refused_policy(PolicyMode::RevalidationError).await;
}

#[tokio::test]
async fn controls_stop_during_pending_approval_prevents_effect_and_retires_wait() {
    let model = Model::new(vec![response(true), response(false)]);
    let control = RunControl::new();
    let effects = Arc::new(AtomicUsize::new(0));
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model))
        .run_control(control.clone())
        .tool(Effect(effects.clone()))
        .tool_policy(Arc::new(Policy::new(PolicyMode::Stop(control))))
        .build()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), harness.run("request effect"))
        .await
        .expect("pending approval must stop");
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(result.status, TaskStatus::Error);
}

#[tokio::test]
async fn controls_stop_retains_actual_effect_until_retirement() {
    let control = RunControl::new();
    let retired = Arc::new(AtomicUsize::new(0));
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(true), response(false)])))
        .run_control(control.clone())
        .tool(RetiringEffect {
            control,
            retired: retired.clone(),
        })
        .build()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), harness.run("request effect"))
        .await
        .unwrap();
    assert_eq!(
        retired.load(Ordering::SeqCst),
        1,
        "Stop must not drop the actual effect future"
    );
    assert_eq!(result.status, TaskStatus::Error);
    assert!(harness.messages().iter().any(|message| message.role
        == anima_harness::anima_core::MessageRole::Tool
        && message.content.text.contains("known retired effect")));
}

#[tokio::test]
async fn controls_deadline_bounds_pending_approval_without_effect() {
    let effects = Arc::new(AtomicUsize::new(0));
    let mut limits = ExecutionLimits::default();
    limits.budget.max_wall_time_ms = Some(50);
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(true)])))
        .execution_limits(limits)
        .tool(Effect(effects.clone()))
        .tool_policy(Arc::new(Policy::new(PolicyMode::Pending)))
        .build()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), harness.run("request effect"))
        .await
        .expect("approval wait must be finite");
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(result.status, TaskStatus::Error);
}

#[tokio::test]
async fn controls_authority_loss_during_approval_prevents_effect() {
    let effects = Arc::new(AtomicUsize::new(0));
    let context = ExecutionContext::ephemeral(
        "attempt".into(),
        ExecutionLimits::default(),
        Default::default(),
    )
    .unwrap();
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(true), response(false)])))
        .execution_context(context.clone())
        .tool(Effect(effects.clone()))
        .tool_policy(Arc::new(Policy::new(PolicyMode::Revoke(context))))
        .build()
        .unwrap();
    let result = harness.run("request effect").await;
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(result.status, TaskStatus::Error);
}

#[tokio::test]
async fn controls_observer_receives_usage_and_explicit_run_identity() {
    let frames = Arc::new(Frames::default());
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(false)])))
        .run_id("host-attempt")
        .run_observer(frames.clone())
        .build()
        .unwrap();
    assert_eq!(harness.run("answer").await.status, TaskStatus::Success);
    assert_eq!(harness.run_id(), Some("host-attempt"));
    assert!(frames.0.lock().unwrap().iter().any(|frame| matches!(frame,
        RunFrame::StepUsage { step_id, usage } if step_id == "host-attempt:1" && usage.total_tokens == 18)));
}

#[tokio::test]
async fn controls_setters_bind_observer_identity_and_policy_before_dispatch() {
    let effects = Arc::new(AtomicUsize::new(0));
    let frames = Arc::new(Frames::default());
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(true), response(false)])))
        .tool(Effect(effects.clone()))
        .build()
        .unwrap();
    harness.set_run_id("setter-run").unwrap();
    harness.set_run_observer(frames.clone());
    harness.set_tool_policy(Arc::new(Policy::new(PolicyMode::Deny)));
    assert_eq!(
        harness.run("request effect").await.status,
        TaskStatus::Success
    );
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(harness.run_id(), Some("setter-run"));
    assert!(frames.0.lock().unwrap().iter().any(|frame| matches!(
        frame,
        RunFrame::ToolFinished {
            status: TaskStatus::Error,
            ..
        }
    )));
}

#[tokio::test]
async fn controls_setter_checkpoint_failure_prevents_dispatch() {
    let model = Model::new(vec![response(false)]);
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .build()
        .unwrap();
    harness
        .set_turn_checkpoint_sink(binding(), Arc::new(RejectCheckpoint))
        .unwrap();
    assert_eq!(
        harness.run("checkpoint first").await.status,
        TaskStatus::Error
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_model_attempt_limit_blocks_followup_dispatch() {
    let model = Model::new(vec![response(true), response(false)]);
    let effects = Arc::new(AtomicUsize::new(0));
    let limits = ExecutionLimits {
        max_model_attempts: 1,
        max_retries: 0,
        ..ExecutionLimits::default()
    };
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(model.clone()))
        .execution_limits(limits)
        .tool(Effect(effects.clone()))
        .build()
        .unwrap();
    let result = harness.run("request effect").await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        effects.load(Ordering::SeqCst),
        1,
        "the admitted effect remains known"
    );
    assert_eq!(harness.state().token_usage.total_tokens, 18);
}

#[tokio::test]
async fn controls_approval_is_bound_to_exact_invocation() {
    let effects = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::new(PolicyMode::Allow));
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(true), response(false)])))
        .run_id("immutable-run")
        .tool(Effect(effects.clone()))
        .tool_policy(policy.clone())
        .build()
        .unwrap();
    assert_eq!(
        harness.run("request effect").await.status,
        TaskStatus::Success
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    let seen = policy.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].agent_id(), harness.id());
    assert_eq!(seen[0].execution().run_id, "immutable-run");
    assert_eq!(seen[0].tool_call().id, "exact-call");
    assert_eq!(
        seen[0].tool_call().args.get("value"),
        Some(&DataValue::String("exact args".into()))
    );
    assert_eq!(seen[0].descriptor().name, "effect");
}

#[tokio::test]
async fn controls_fresh_chat_inputs_get_finite_independent_ephemeral_scopes() {
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![response(false), response(false)])))
        .build()
        .unwrap();
    assert_eq!(harness.chat("one").await.status, TaskStatus::Success);
    let first = harness
        .snapshot()
        .execution_binding
        .expect("finite ephemeral scope");
    assert_eq!(harness.chat("two").await.status, TaskStatus::Success);
    let second = harness.snapshot().execution_binding.unwrap();
    assert_ne!(first.scope_id, second.scope_id);
    assert_ne!(first.run_id, second.run_id);
    assert!(first.deadline_at_ms > 0 && second.deadline_at_ms > 0);
}

#[tokio::test]
async fn controls_restore_requires_matching_explicit_authority_without_new_allowance() {
    let model = Arc::new(Model::new(vec![response(false), response(false)]));
    let context = ExecutionContext::ephemeral(
        "restore-run".into(),
        ExecutionLimits::default(),
        Default::default(),
    )
    .unwrap();
    let mut harness = Harness::builder()
        .model("fixture")
        .adapter(model.clone())
        .execution_context(context.clone())
        .build()
        .unwrap();
    assert_eq!(harness.run("one").await.status, TaskStatus::Success);
    let mut restored = Harness::restore(harness.snapshot(), model.clone(), ToolSet::new());
    assert!(
        restored.execution_context().is_none(),
        "snapshot is not authority"
    );
    let result = restored.chat("resume").await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let other = ExecutionContext::ephemeral(
        "restore-run".into(),
        ExecutionLimits::default(),
        Default::default(),
    )
    .unwrap();
    assert!(
        restored.set_execution_context(other).is_err(),
        "same run with fresh allowance cannot replace saved scope"
    );
    restored.set_execution_context(context).unwrap();
    assert_eq!(
        restored.chat("explicitly reattached").await.status,
        TaskStatus::Success
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn controls_builder_rejects_unbounded_limits() {
    let mut limits = ExecutionLimits::default();
    limits.budget.max_wall_time_ms = None;
    assert!(Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![])))
        .execution_limits(limits)
        .build()
        .is_err());
}

#[test]
fn controls_builder_rejects_mismatched_context_run_identity() {
    let context = ExecutionContext::ephemeral(
        "attached-run".into(),
        ExecutionLimits::default(),
        Default::default(),
    )
    .unwrap();
    assert!(Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![])))
        .run_id("different-run")
        .execution_context(context)
        .build()
        .is_err());
}

#[test]
fn controls_builder_rejects_mismatched_checkpoint_identity() {
    let context = ExecutionContext::ephemeral(
        "different-run".into(),
        ExecutionLimits::default(),
        Default::default(),
    )
    .unwrap();
    assert!(Harness::builder()
        .model("fixture")
        .adapter(Arc::new(Model::new(vec![])))
        .execution_context(context)
        .turn_checkpoint(binding(), Arc::new(RejectCheckpoint))
        .build()
        .is_err());
}
