//! Cooperative stop and steering (spec §4.6–§4.7).

use super::{
    AgentRuntime, CancelSignal, RunControl, RunFrame, RunObserver, SteeringInbox,
    CANCELLED_TOOL_RESULT, RUN_STOPPED_ERROR,
};
use crate::agent::{AgentConfig, AgentStatus, TokenUsage, ToolDescriptor};
use crate::events::EventType;
use crate::model::{
    ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, ModelStopReason, ModelStreamFrame,
    ModelStreamSink, ToolCall,
};
use crate::primitives::{
    Attachment, AttachmentType, Content, DataValue, LockRecover, Message, MessageRole, TaskResult,
    TaskStatus,
};
use async_trait::async_trait;
use futures::executor::block_on;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Frames(Mutex<Vec<RunFrame>>);

impl RunObserver for Frames {
    fn on_frame(&self, frame: RunFrame) {
        self.0.lock_recover().push(frame);
    }
}

fn text(value: &str) -> Content {
    Content {
        text: value.into(),
        ..Content::default()
    }
}

fn config() -> AgentConfig {
    AgentConfig {
        name: "controlled".into(),
        model: "control-model".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: None,
        system: None,
        tools: Some(vec![ToolDescriptor {
            name: "search".into(),
            description: String::new(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        }]),
        plugins: None,
        settings: None,
        fallbacks: Vec::new(),
    }
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "search".into(),
        args: BTreeMap::new(),
    }
}

fn final_reply(content: &str) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: text(content),
        tool_calls: None,
        usage: TokenUsage::default(),
        stop_reason: ModelStopReason::End,
    }
}

fn tool_request(ids: &[&str]) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: text("searching"),
        tool_calls: Some(ids.iter().map(|id| call(id)).collect()),
        usage: TokenUsage::default(),
        stop_reason: ModelStopReason::ToolCall,
    }
}

/// Asks for tools (`ids`) until a tool result is in the conversation, then
/// replies. Records every request; optionally cancels `cancel` inside its
/// first call (after the final frame) and optionally streams `stall` text
/// then waits forever after cancelling.
struct ScriptedModel {
    ids: Vec<&'static str>,
    requests: Mutex<Vec<ModelGenerateRequest>>,
    cancel_on_first: Option<CancelSignal>,
    cancel_cause: super::CancelCause,
    stall_after: Option<&'static str>,
    usage_before_stall: Option<TokenUsage>,
}

struct BudgetWrapupStreamModel {
    requests: Mutex<Vec<(AgentConfig, ModelGenerateRequest)>>,
    cancel: CancelSignal,
    fail_stream: bool,
}

#[async_trait]
impl ModelAdapter for BudgetWrapupStreamModel {
    fn provider(&self) -> &str {
        "budget-wrapup-stream"
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        Err("this regression exercises streaming".into())
    }
    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let first = {
            let mut requests = self.requests.lock_recover();
            requests.push((config.clone(), request.clone()));
            requests.len() == 1
        };
        if first {
            return sink
                .emit(ModelStreamFrame::Final(tool_request(&["budget-search"])))
                .await;
        }
        assert_eq!(
            config.tools,
            Some(Vec::new()),
            "wrap-up streaming receives no tool capability"
        );
        sink.emit(ModelStreamFrame::Usage(TokenUsage {
            prompt_tokens: 2,
            completion_tokens: 3,
            total_tokens: 5,
            ..Default::default()
        }))
        .await?;
        sink.emit(ModelStreamFrame::TextDelta("Partial wrap-up".into()))
            .await?;
        if self.fail_stream {
            return Err("wrap-up stream disconnected".into());
        }
        self.cancel.cancel();
        std::future::pending().await
    }
}

#[test]
fn budget_wrapup_stops_before_request_after_the_last_tool_round_is_cancelled() {
    for cause in [
        super::CancelCause::OwnerStop,
        super::CancelCause::HelperDeadline,
        super::CancelCause::DaemonShutdown,
    ] {
        let control = RunControl::new();
        let model = Arc::new(ScriptedModel::new(&["last-search"]));
        let (mut runtime, _) = controlled(model.clone(), &control);
        let mut config = runtime.config().clone();
        let mut settings = config.settings.clone().unwrap_or_default();
        settings.max_tool_iterations = Some(1);
        config.settings = Some(settings);
        runtime.replace_config(config);
        let dispatched = AtomicUsize::new(0);
        let cancel = control.cancel.clone();
        let result = block_on(runtime.run_with_context_and_tools(
            Vec::new(),
            text("Search once"),
            |_, _, _| {
                dispatched.fetch_add(1, Ordering::SeqCst);
                cancel.cancel_with(cause);
                async { TaskResult::error("terminal host cancellation", 1) }
            },
        ));
        assert_eq!(result.error.as_deref(), Some(control.cancel.run_error()));
        assert_eq!(
            model.requests.lock_recover().len(),
            1,
            "cancellation must win before the budget wrap-up request"
        );
        assert_eq!(dispatched.load(Ordering::SeqCst), 1);
        assert!(runtime
            .messages()
            .iter()
            .all(|message| metadata(message, "budgetWrapup").is_none()));
    }
}

#[test]
fn budget_wrapup_stream_stop_and_error_keep_partial_text_usage_and_never_retry() {
    for fail_stream in [false, true] {
        let control = RunControl::new();
        let model = Arc::new(BudgetWrapupStreamModel {
            requests: Mutex::new(Vec::new()),
            cancel: control.cancel.clone(),
            fail_stream,
        });
        let mut config = config();
        config.settings = Some(crate::agent::AgentSettings {
            max_tool_iterations: Some(1),
            ..Default::default()
        });
        let mut runtime = AgentRuntime::new(config, model.clone());
        let frames = Arc::new(Frames::default());
        runtime.set_run_id("run_budget_stop");
        runtime.set_run_control(control.clone());
        runtime.set_run_observer(frames.clone());
        let dispatched = AtomicUsize::new(0);
        let result =
            block_on(
                runtime.run_with_context_and_tools(Vec::new(), text("Search"), |_, _, _| {
                    dispatched.fetch_add(1, Ordering::SeqCst);
                    async { TaskResult::success(text("search complete"), 1) }
                }),
            );
        assert_eq!(
            result.error.as_deref(),
            Some(if fail_stream {
                "wrap-up stream disconnected"
            } else {
                RUN_STOPPED_ERROR
            })
        );
        assert_eq!(model.requests.lock_recover().len(), 2);
        assert_eq!(dispatched.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.state().token_usage.total_tokens, 5);
        let partial = runtime.messages().last().unwrap();
        assert_eq!(partial.content.text, "Partial wrap-up");
        assert_eq!(
            metadata(partial, if fail_stream { "incomplete" } else { "stopped" }),
            Some(&DataValue::Bool(true))
        );
        assert!(metadata(partial, "budgetWrapup").is_none());
        let frames = frames.0.lock_recover();
        assert_eq!(
            frames
                .iter()
                .filter(|frame| matches!(frame, RunFrame::ToolStarted { .. }))
                .count(),
            1
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| matches!(frame, RunFrame::ToolFinished { .. }))
                .count(),
            1
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| matches!(frame, RunFrame::StepStarted { .. }))
                .count(),
            2
        );
    }
}

impl ScriptedModel {
    fn new(ids: &[&'static str]) -> Self {
        Self {
            ids: ids.to_vec(),
            requests: Mutex::new(Vec::new()),
            cancel_on_first: None,
            cancel_cause: super::CancelCause::OwnerStop,
            stall_after: None,
            usage_before_stall: None,
        }
    }

    fn calls(&self) -> usize {
        self.requests.lock_recover().len()
    }
}

#[async_trait]
impl ModelAdapter for ScriptedModel {
    fn provider(&self) -> &str {
        "scripted"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        Err("the runtime streams every model call".into())
    }

    async fn stream(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let first = {
            let mut requests = self.requests.lock_recover();
            requests.push(request.clone());
            requests.len() == 1
        };
        if let Some(usage) = &self.usage_before_stall {
            sink.emit(ModelStreamFrame::Usage(usage.clone())).await?;
        }
        if let Some(stall) = self.stall_after {
            sink.emit(ModelStreamFrame::TextDelta(stall.into())).await?;
            if let Some(cancel) = &self.cancel_on_first {
                cancel.cancel_with(self.cancel_cause);
            }
            futures::future::pending::<()>().await;
            unreachable!("a stalled stream never resumes");
        }
        let wants_tools = !self.ids.is_empty()
            && !request
                .messages
                .iter()
                .any(|message| message.role == MessageRole::Tool);
        let response = if wants_tools {
            tool_request(&self.ids)
        } else {
            final_reply("all done")
        };
        sink.emit(ModelStreamFrame::Final(response)).await?;
        if first {
            if let Some(cancel) = &self.cancel_on_first {
                cancel.cancel_with(self.cancel_cause);
            }
        }
        Ok(())
    }
}

fn controlled(model: Arc<ScriptedModel>, control: &RunControl) -> (AgentRuntime, Arc<Frames>) {
    let frames = Arc::new(Frames::default());
    let mut runtime = AgentRuntime::new(config(), model);
    runtime.init();
    runtime.set_run_id("run_c");
    runtime.set_run_observer(frames.clone());
    runtime.set_run_control(control.clone());
    (runtime, frames)
}

fn metadata<'a>(message: &'a Message, key: &str) -> Option<&'a DataValue> {
    message.content.metadata.as_ref()?.get(key)
}

#[test]
fn a_cancel_signal_wakes_every_waiter_and_is_shared_by_clones() {
    let signal = CancelSignal::new();
    let clone = signal.clone();
    assert!(!clone.is_cancelled());
    let canceller = signal.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        canceller.cancel();
        canceller.cancel();
    });
    block_on(futures::future::join(signal.cancelled(), clone.cancelled()));
    thread.join().unwrap();
    assert!(signal.is_cancelled() && clone.is_cancelled());
    block_on(signal.cancelled());
}

#[test]
fn the_steering_inbox_drains_in_order_and_refuses_items_once_closed() {
    let inbox = SteeringInbox::new();
    inbox.push(text("a")).unwrap();
    inbox.push(text("b")).unwrap();
    assert_eq!(inbox.pending(), vec![text("a"), text("b")]);
    assert_eq!(inbox.drain(), vec![text("a"), text("b")]);
    assert!(inbox.drain().is_empty());
    inbox.push(text("c")).unwrap();
    let shared = inbox.clone();
    assert_eq!(
        shared.close(),
        vec![text("c")],
        "leftovers come back in order"
    );
    assert!(inbox.is_closed());
    assert_eq!(inbox.push(text("d")), Err(text("d")));
    assert!(inbox.close().is_empty());
}

#[test]
fn a_stop_before_the_first_model_call_never_calls_the_model() {
    let control = RunControl::new();
    control.cancel.cancel();
    let model = Arc::new(ScriptedModel::new(&[]));
    let (mut runtime, _frames) = controlled(model.clone(), &control);

    let result = block_on(runtime.run(text("hi")));

    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(result.error.as_deref(), Some(RUN_STOPPED_ERROR));
    assert_eq!(model.calls(), 0);
    assert_eq!(
        runtime.state().status,
        AgentStatus::Idle,
        "a stop is not a failure"
    );
    assert_eq!(runtime.messages().len(), 1, "only the owner's message");
    let kinds: Vec<EventType> = runtime
        .events()
        .iter()
        .map(|event| event.event_type)
        .collect();
    assert!(kinds.contains(&EventType::TaskFailed));
    assert!(!kinds.contains(&EventType::AgentFailed));
    assert_eq!(runtime.last_task(), Some(&result));
}

#[test]
fn a_stop_while_streaming_keeps_the_partial_text_marked_stopped() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel {
        cancel_on_first: Some(control.cancel.clone()),
        stall_after: Some("Partial ans"),
        ..ScriptedModel::new(&[])
    });
    let (mut runtime, frames) = controlled(model, &control);

    let result = block_on(runtime.run(text("hi")));

    assert_eq!(result.error.as_deref(), Some(RUN_STOPPED_ERROR));
    let messages = runtime.messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].role, MessageRole::Assistant);
    assert_eq!(messages[1].content.text, "Partial ans");
    assert_eq!(
        metadata(&messages[1], "stopped"),
        Some(&DataValue::Bool(true))
    );
    assert_eq!(
        metadata(&messages[1], "stepId"),
        Some(&DataValue::String("run_c:1".into()))
    );
    assert_eq!(
        frames.0.lock_recover().last(),
        Some(&RunFrame::StepFinished {
            step_id: "run_c:1".into(),
            message_id: Some(messages[1].id.clone()),
        })
    );
    assert_eq!(runtime.state().status, AgentStatus::Idle);
}

#[test]
fn a_stop_before_the_tool_batch_answers_every_call_as_cancelled() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel {
        cancel_on_first: Some(control.cancel.clone()),
        ..ScriptedModel::new(&["call-a", "call-b"])
    });
    let (mut runtime, _frames) = controlled(model.clone(), &control);
    let executed = Arc::new(AtomicBool::new(false));

    let result = block_on(runtime.run_with_tools(text("look it up"), {
        let executed = Arc::clone(&executed);
        move |_, _, _| {
            executed.store(true, Ordering::SeqCst);
            async move { TaskResult::success(text("never"), 0) }
        }
    }));

    assert_eq!(result.error.as_deref(), Some(RUN_STOPPED_ERROR));
    assert!(
        !executed.load(Ordering::SeqCst),
        "no tool runs after a stop"
    );
    assert_eq!(model.calls(), 1);
    let tools: Vec<&Message> = runtime
        .messages()
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .collect();
    assert_eq!(tools.len(), 2, "every requested call gets a result");
    for (message, id) in tools.iter().zip(["call-a", "call-b"]) {
        assert_eq!(
            metadata(message, "toolCallId"),
            Some(&DataValue::String(id.into()))
        );
        assert_eq!(
            metadata(message, "toolStatus"),
            Some(&DataValue::String("error".into()))
        );
        assert!(message.content.text.contains(CANCELLED_TOOL_RESULT));
    }
}

#[test]
fn tools_already_running_finish_and_the_run_stops_before_the_next_call() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel::new(&["call-a"]));
    let (mut runtime, _frames) = controlled(model.clone(), &control);
    let cancel = control.cancel.clone();

    let result = block_on(runtime.run_with_tools(text("look it up"), move |_, _, _| {
        cancel.cancel();
        async move { TaskResult::success(text("found"), 0) }
    }));

    assert_eq!(result.error.as_deref(), Some(RUN_STOPPED_ERROR));
    assert_eq!(
        model.calls(),
        1,
        "the stop lands before the second model call"
    );
    let tool = runtime
        .messages()
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .expect("the running tool finished");
    assert_eq!(tool.content.text, "found");
}

#[test]
fn steers_join_the_conversation_before_the_next_model_call() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel::new(&["call-a"]));
    let (mut runtime, frames) = controlled(model.clone(), &control);
    let steering = control.steering.clone();
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let steered_content = Content {
        text: "Also book the gym".into(),
        attachments: Some(vec![Attachment {
            attachment_type: AttachmentType::File,
            name: "schedule.txt".into(),
            data: "uploads/schedule.txt".into(),
            mime_type: Some("text/plain".into()),
        }]),
        metadata: Some(BTreeMap::from([(
            "references".into(),
            DataValue::Array(vec![DataValue::String("ref-schedule".into())]),
        )])),
    };

    let result = block_on(runtime.run_with_tools(text("plan my week"), {
        let tool_calls = Arc::clone(&tool_calls);
        let steered_content = steered_content.clone();
        move |_, _, _| {
            tool_calls.fetch_add(1, Ordering::SeqCst);
            steering
                .push(steered_content.clone())
                .expect("the inbox is open while the run works");
            async move { TaskResult::success(text("calendar read"), 0) }
        }
    }));

    assert_eq!(result.status, TaskStatus::Success);
    let requests = model.requests.lock_recover().clone();
    assert_eq!(requests.len(), 2);
    let second = &requests[1].messages;
    let steer = second.last().expect("the steer is the newest message");
    assert_eq!(steer.role, MessageRole::User);
    assert_eq!(steer.content.text, "Also book the gym");
    assert_eq!(steer.content.attachments, steered_content.attachments);
    assert_eq!(
        metadata(steer, "references"),
        steered_content.metadata.as_ref().unwrap().get("references")
    );
    assert_eq!(metadata(steer, "steer"), Some(&DataValue::Bool(true)));
    assert_eq!(
        metadata(steer, "runId"),
        Some(&DataValue::String("run_c".into()))
    );
    let transcript: Vec<(&str, MessageRole)> = runtime
        .messages()
        .iter()
        .map(|message| (message.content.text.as_str(), message.role))
        .collect();
    assert_eq!(
        transcript,
        [
            ("plan my week", MessageRole::User),
            ("searching", MessageRole::Assistant),
            ("calendar read", MessageRole::Tool),
            ("Also book the gym", MessageRole::User),
            ("all done", MessageRole::Assistant),
        ]
    );
    assert!(frames.0.lock_recover().contains(&RunFrame::Steered {
        message_id: runtime.messages()[3].id.clone(),
        text: "Also book the gym".into(),
        content: runtime.messages()[3].content.clone(),
    }));
    assert!(control.steering.drain().is_empty());
}

/// Task 2 review carry-forward: a stop leaves the steers the run never
/// drained in the inbox, so the host decides what becomes of them (spec
/// §4.7; the daemon turns them into interrupted records, audit M8).
#[test]
fn a_stop_leaves_the_steers_it_never_drained_in_the_inbox_for_the_host() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel::new(&["call-a"]));
    let (mut runtime, frames) = controlled(model.clone(), &control);
    let steering = control.steering.clone();
    let cancel = control.cancel.clone();

    let result = block_on(
        runtime.run_with_tools(text("plan my week"), move |_, _, _| {
            steering
                .push(text("Also book the gym"))
                .expect("the inbox is open while the run works");
            cancel.cancel();
            async move { TaskResult::success(text("calendar read"), 0) }
        }),
    );

    assert_eq!(result.error.as_deref(), Some(RUN_STOPPED_ERROR));
    assert_eq!(
        model.calls(),
        1,
        "the stop lands before the next model call"
    );
    assert!(
        runtime
            .messages()
            .iter()
            .all(|message| message.content.text != "Also book the gym"),
        "a steer the run never drained is not part of its transcript"
    );
    assert!(!frames
        .0
        .lock_recover()
        .iter()
        .any(|frame| matches!(frame, RunFrame::Steered { .. })));
    assert!(!control.steering.is_closed(), "the host closes the inbox");
    assert_eq!(
        control.steering.close(),
        vec![text("Also book the gym")],
        "the host gets the steer back"
    );
}

#[test]
fn helper_deadline_answers_tools_with_its_cause() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel {
        cancel_on_first: Some(control.cancel.clone()),
        cancel_cause: super::CancelCause::HelperDeadline,
        ..ScriptedModel::new(&["deadline-tool"])
    });
    let (mut runtime, _) = controlled(model, &control);
    let result = block_on(runtime.run_with_tools(text("look it up"), |_, _, _| async {
        panic!("a cancelled tool must not execute");
        #[allow(unreachable_code)]
        TaskResult::success(text("never"), 0)
    }));
    assert_eq!(result.error.as_deref(), Some("Helper task timed out"));
    assert_eq!(runtime.state().status, AgentStatus::Failed);
    let tool = runtime
        .messages()
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap();
    assert!(tool.content.text.contains("helper deadline reached"));
    assert!(!tool.content.text.contains("stopped by owner"));
    assert_eq!(
        metadata(tool, "toolCallId"),
        Some(&DataValue::String("deadline-tool".into()))
    );
}

#[test]
fn helper_deadline_while_streaming_keeps_partial_text() {
    let control = RunControl::new();
    let model = Arc::new(ScriptedModel {
        cancel_on_first: Some(control.cancel.clone()),
        cancel_cause: super::CancelCause::HelperDeadline,
        stall_after: Some("Partial helper answer"),
        usage_before_stall: Some(TokenUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
            ..TokenUsage::default()
        }),
        ..ScriptedModel::new(&[])
    });
    let (mut runtime, _) = controlled(model, &control);
    let result = block_on(runtime.run(text("hi")));
    assert_eq!(result.error.as_deref(), Some("Helper task timed out"));
    assert_eq!(runtime.messages()[1].content.text, "Partial helper answer");
    assert_eq!(runtime.state().token_usage.total_tokens, 15);
}
