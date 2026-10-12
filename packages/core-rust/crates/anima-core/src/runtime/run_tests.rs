//! Isolated per-run copies, run deltas, and run-scoped step keys.

use super::{new_room_id, AgentRuntime, MAX_RETAINED_EVENTS};
use crate::agent::{AgentConfig, AgentStatus, TokenUsage};
use crate::events::EventType;
use crate::model::{ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, ModelStopReason};
use crate::primitives::{Content, DataValue, MessageRole, TaskStatus};
use async_trait::async_trait;
use futures::executor::block_on;
use std::sync::Arc;

struct EchoModel;

#[async_trait]
impl ModelAdapter for EchoModel {
    fn provider(&self) -> &str {
        "echo"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let input = request
            .messages
            .last()
            .map(|message| message.content.text.clone())
            .unwrap_or_default();
        Ok(ModelGenerateResponse {
            content: text(&format!("echo: {input}")),
            tool_calls: None,
            usage: usage(),
            stop_reason: ModelStopReason::End,
        })
    }
}

fn usage() -> TokenUsage {
    TokenUsage {
        prompt_tokens: 5,
        completion_tokens: 7,
        total_tokens: 12,
        cached_prompt_tokens: 2,
        reasoning_tokens: 3,
    }
}

fn config() -> AgentConfig {
    AgentConfig {
        name: "isolated".into(),
        model: "echo-model".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: None,
        system: None,
        tools: None,
        plugins: None,
        settings: None,
    }
}

fn text(value: &str) -> Content {
    Content {
        text: value.into(),
        ..Content::default()
    }
}

fn canonical() -> AgentRuntime {
    let mut runtime = AgentRuntime::new(config(), Arc::new(EchoModel));
    runtime.init();
    runtime
}

fn room_history(runtime: &AgentRuntime, room_id: &str) -> Vec<crate::primitives::Message> {
    runtime
        .messages()
        .iter()
        .filter(|message| message.room_id == room_id)
        .cloned()
        .collect()
}

fn isolated_copy(canonical: &AgentRuntime, room_id: &str) -> AgentRuntime {
    AgentRuntime::from_snapshot(
        canonical.run_snapshot(room_history(canonical, room_id)),
        Arc::new(EchoModel),
    )
}

fn run_in(
    runtime: &mut AgentRuntime,
    room_id: &str,
    input: &str,
) -> crate::primitives::TaskResult<Content> {
    let history = room_history(runtime, room_id);
    block_on(runtime.run_in_room_with_context(room_id.into(), history, text(input)))
}

#[test]
fn isolated_copy_carries_only_its_room_and_no_events() {
    let mut canonical = canonical();
    run_in(&mut canonical, "room-a", "in a");
    run_in(&mut canonical, "room-b", "in b");

    let copy = canonical.run_snapshot(room_history(&canonical, "room-b"));
    let full = canonical.snapshot();

    assert_eq!(copy.messages.len(), 2);
    assert_eq!(copy.message_count, 2);
    assert!(copy
        .messages
        .iter()
        .all(|message| message.room_id == "room-b"));
    assert!(
        copy.events.is_empty(),
        "a run copy never carries the event log"
    );
    assert_eq!(copy.event_count, full.event_count);
    assert_eq!(copy.step_count, full.step_count);
    assert_eq!(copy.state, full.state);
    assert_eq!(copy.last_task, full.last_task);
}

#[test]
fn applying_a_delta_merges_exactly_one_run_and_reverting_restores_the_record() {
    let mut canonical = canonical();
    run_in(&mut canonical, "room-a", "first");
    let before = canonical.snapshot();

    let mut isolated = isolated_copy(&canonical, "room-b");
    let base = isolated.run_base();
    let result = run_in(&mut isolated, "room-b", "second");
    let delta = isolated.run_delta_since(&base);

    assert_eq!(result.status, TaskStatus::Success);
    assert_eq!(
        delta
            .messages
            .iter()
            .map(|message| (message.role, message.room_id.as_str()))
            .collect::<Vec<_>>(),
        [
            (MessageRole::User, "room-b"),
            (MessageRole::Assistant, "room-b")
        ]
    );
    assert_eq!(delta.token_usage, usage());
    assert_eq!(delta.status, AgentStatus::Completed);
    assert_eq!(delta.last_task, Some(result));
    assert!(delta.event_total > 0);
    assert_eq!(delta.event_total, delta.events.len());
    assert_eq!(
        canonical.snapshot(),
        before,
        "building and running a copy never touches the canonical record"
    );

    let undo = canonical.apply_run_delta(&delta);
    let applied = canonical.snapshot();
    assert_eq!(
        &applied.messages[..before.messages.len()],
        &before.messages[..]
    );
    assert_eq!(
        &applied.messages[before.messages.len()..],
        &delta.messages[..]
    );
    assert_eq!(
        applied.state.token_usage.total_tokens,
        before.state.token_usage.total_tokens + 12
    );
    assert_eq!(
        applied.state.token_usage.cached_prompt_tokens,
        before.state.token_usage.cached_prompt_tokens + 2
    );
    assert_eq!(
        applied.state.token_usage.reasoning_tokens,
        before.state.token_usage.reasoning_tokens + 3
    );
    assert_eq!(applied.event_count, before.event_count + delta.event_total);
    assert_eq!(applied.step_count, before.step_count + delta.step_count);
    assert_eq!(applied.last_task, delta.last_task);
    assert_eq!(applied.state.status, AgentStatus::Completed);

    canonical.revert_run_delta(&delta, undo);
    assert_eq!(canonical.snapshot(), before);
}

#[test]
fn copies_from_one_base_merge_independently_and_revert_by_id() {
    let mut canonical = canonical();
    let before = canonical.snapshot();
    let mut first = isolated_copy(&canonical, "room-a");
    let mut second = isolated_copy(&canonical, "room-b");
    let (first_base, second_base) = (first.run_base(), second.run_base());
    run_in(&mut first, "room-a", "from a");
    run_in(&mut second, "room-b", "from b");
    let first_delta = first.run_delta_since(&first_base);
    let second_delta = second.run_delta_since(&second_base);

    let first_undo = canonical.apply_run_delta(&first_delta);
    canonical.apply_run_delta(&second_delta);
    canonical.revert_run_delta(&first_delta, first_undo);

    let merged = canonical.snapshot();
    assert_eq!(merged.messages, second_delta.messages);
    assert_eq!(merged.state.token_usage, usage());
    assert_eq!(
        merged.event_count,
        before.event_count + second_delta.event_total
    );
    assert_eq!(
        merged.last_task, second_delta.last_task,
        "reverting an earlier run keeps a later run's result"
    );
    assert_eq!(merged.state.status, AgentStatus::Completed);
}

#[test]
fn applying_a_delta_keeps_the_retained_event_cap() {
    let mut canonical = canonical();
    for _ in 0..MAX_RETAINED_EVENTS {
        canonical.record_event(EventType::AgentTokens, DataValue::Null);
    }
    let before_total = canonical.snapshot().event_count;
    let mut isolated = isolated_copy(&canonical, "room-a");
    let base = isolated.run_base();
    for _ in 0..100 {
        isolated.record_event(EventType::AgentTokens, DataValue::Null);
    }
    let delta = isolated.run_delta_since(&base);

    assert_eq!(delta.event_total, 100);
    canonical.apply_run_delta(&delta);
    let applied = canonical.snapshot();
    assert_eq!(applied.events.len(), MAX_RETAINED_EVENTS);
    assert_eq!(applied.event_count, before_total + 100);
    assert_eq!(
        applied.events.last().map(|event| &event.id),
        delta.events.last().map(|event| &event.id)
    );
}

#[test]
fn new_room_ids_are_unique_generated_rooms() {
    let first = new_room_id();
    let second = new_room_id();

    assert_ne!(first, second);
    assert!(first.starts_with("room-") && second.starts_with("room-"));
}

use crate::agent::{AgentState, ToolDescriptor};
use crate::model::ToolCall;
use crate::persistence::{in_memory::InMemoryAdapter, StepStatus};
use crate::primitives::{Message, TaskResult};
use std::collections::BTreeMap;

/// Asks for `memory_search` once, then answers.
struct SearchOnceModel;

#[async_trait]
impl ModelAdapter for SearchOnceModel {
    fn provider(&self) -> &str {
        "search-once"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        if request
            .messages
            .iter()
            .any(|message| message.role == MessageRole::Tool)
        {
            return Ok(ModelGenerateResponse {
                content: text("done"),
                tool_calls: None,
                usage: TokenUsage::default(),
                stop_reason: ModelStopReason::End,
            });
        }
        Ok(ModelGenerateResponse {
            content: Content::default(),
            tool_calls: Some(vec![ToolCall {
                id: "search-1".into(),
                name: "memory_search".into(),
                args: BTreeMap::new(),
            }]),
            usage: TokenUsage::default(),
            stop_reason: ModelStopReason::ToolCall,
        })
    }
}

fn search_config() -> AgentConfig {
    AgentConfig {
        tools: Some(vec![ToolDescriptor {
            name: "memory_search".into(),
            description: "Search memories".into(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        }]),
        ..config()
    }
}

#[test]
fn step_keys_include_the_run_id_unless_a_durable_retry_key_is_present() {
    let message = Message {
        id: "msg-1".into(),
        agent_id: "agent-1".into(),
        room_id: "room-a".into(),
        content: text("search"),
        role: MessageRole::User,
        created_at_ms: 1,
    };
    let call = ToolCall {
        id: "search-1".into(),
        name: "memory_search".into(),
        args: BTreeMap::new(),
    };
    let key = |run_id: Option<&str>, message: &Message| {
        super::tool_step_idempotency_key("agent-1", run_id, message, 1, 0, &call)
    };

    assert_ne!(key(Some("run_a"), &message), key(Some("run_b"), &message));
    assert_ne!(key(Some("run_a"), &message), key(None, &message));

    let mut keyed = message.clone();
    keyed.content.metadata = Some(BTreeMap::from([(
        "idempotencyKey".to_string(),
        DataValue::String("telegram-a:update:42".into()),
    )]));
    assert_eq!(
        key(Some("run_a"), &keyed),
        key(Some("run_b"), &keyed),
        "a durable retry key keeps tool steps replay-safe when a restart re-runs the work under a new run"
    );
    assert_eq!(key(Some("run_a"), &keyed), key(None, &keyed));
}

/// Proves that two isolated copies built from one canonical snapshot, run
/// concurrently, each persist their own completed tool step and neither
/// overwrites the other's row in the step log, even though both copies start
/// from the same step index. It does not pin the run-id key component: here
/// `message.id`/`message.room_id` already differ per iteration (each mints
/// fresh ids off the process-wide counters), so the keys would stay distinct
/// even without `run_id` in the seed. That component is covered by
/// `step_keys_include_the_run_id_unless_a_durable_retry_key_is_present`.
#[test]
fn copies_running_concurrently_persist_separate_tool_steps() {
    let db = Arc::new(InMemoryAdapter::new());
    let mut canonical = AgentRuntime::new(search_config(), Arc::new(SearchOnceModel));
    canonical.init();
    let tool =
        |_: AgentState, _: Message, _: ToolCall| async move { TaskResult::success(text("hit"), 1) };
    let mut steps_per_run = Vec::new();
    for run_id in ["run_a", "run_b"] {
        let mut copy = AgentRuntime::from_snapshot(
            canonical.run_snapshot(Vec::new()),
            Arc::new(SearchOnceModel),
        );
        copy.set_database(db.clone());
        copy.set_run_id(run_id);
        assert_eq!(copy.run_id(), Some(run_id));
        let base = copy.run_base();
        let result = block_on(copy.run_in_room_with_context_and_tools(
            new_room_id(),
            Vec::new(),
            text("search"),
            tool,
        ));
        assert_eq!(result.status, TaskStatus::Success);
        steps_per_run.push(copy.run_delta_since(&base).step_count);
    }

    let steps = db.recorded_steps();
    assert_eq!(steps_per_run, [1, 1]);
    assert_eq!(
        steps.len(),
        2,
        "copies share a starting step index but keep separate step rows"
    );
    assert_eq!(steps[0].step_index, steps[1].step_index);
    assert_ne!(steps[0].idempotency_key, steps[1].idempotency_key);
    assert!(steps.iter().all(|step| step.status == StepStatus::Done));
}

#[test]
fn content_retry_key_reads_the_supported_metadata_names() {
    for name in ["retryKey", "retry_key", "idempotencyKey", "idempotency_key"] {
        let content = Content {
            metadata: Some(BTreeMap::from([(
                name.to_string(),
                DataValue::String("key-1".into()),
            )])),
            ..Content::default()
        };
        assert_eq!(super::content_retry_key(&content), Some("key-1"), "{name}");
    }
    let blank = Content {
        metadata: Some(BTreeMap::from([(
            "idempotencyKey".to_string(),
            DataValue::String(String::new()),
        )])),
        ..Content::default()
    };
    assert_eq!(super::content_retry_key(&blank), None);
    assert_eq!(super::content_retry_key(&Content::default()), None);
}

#[test]
fn retain_messages_removes_only_rejected_messages_and_keeps_counters() {
    let mut canonical = canonical();
    run_in(&mut canonical, "room-a", "first");
    run_in(&mut canonical, "room-b", "second");
    let before = canonical.snapshot();

    let removed = canonical.retain_messages(|message| message.room_id != "room-a");

    assert_eq!(removed.len(), 2);
    assert!(removed.iter().all(|message| message.room_id == "room-a"));
    assert_eq!(removed[0].role, MessageRole::User);
    assert_eq!(removed[1].role, MessageRole::Assistant);
    let after = canonical.snapshot();
    assert_eq!(after.messages.len(), 2);
    assert!(after
        .messages
        .iter()
        .all(|message| message.room_id == "room-b"));
    assert_eq!(after.message_count, 2);
    assert_eq!(after.event_count, before.event_count);
    assert_eq!(after.events, before.events);
    assert_eq!(after.step_count, before.step_count);
    assert_eq!(after.state.token_usage, before.state.token_usage);
    assert_eq!(after.state.status, before.state.status);
    assert_eq!(after.last_task, before.last_task);
    assert!(
        canonical.retain_messages(|_| true).is_empty(),
        "keeping everything removes nothing"
    );
}

#[test]
fn retain_messages_leaves_no_room_for_the_removed_messages() {
    let mut canonical = canonical();
    let status = canonical.state().status;
    canonical.apply_run_delta(&super::RuntimeRunDelta {
        messages: (0..1_000)
            .map(|index| Message {
                id: format!("msg-{index}"),
                agent_id: "agent-1".into(),
                room_id: if index < 990 { "room-old" } else { "room-kept" }.into(),
                content: text("hello"),
                role: MessageRole::User,
                created_at_ms: index,
            })
            .collect(),
        events: Vec::new(),
        event_total: 0,
        token_usage: TokenUsage::default(),
        step_count: 0,
        last_task: None,
        status,
    });

    let removed = canonical.retain_messages(|message| message.room_id == "room-kept");

    assert_eq!(removed.len(), 990);
    assert_eq!(canonical.messages().len(), 10);
    assert!(
        canonical.messages.capacity() <= 2 * canonical.messages.len(),
        "the transcript keeps room for {} messages after keeping 10",
        canonical.messages.capacity()
    );
}

/// Advertises another action until the supplied call configuration
/// explicitly removes capability; absent tools intentionally mean defaults.
#[derive(Default)]
struct BudgetWrapupModel {
    requests: std::sync::Mutex<Vec<(AgentConfig, ModelGenerateRequest)>>,
}

#[async_trait]
impl ModelAdapter for BudgetWrapupModel {
    fn provider(&self) -> &str {
        "budget-wrapup-test"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.requests
            .lock()
            .unwrap()
            .push((config.clone(), request.clone()));
        let no_tools = config.tools.as_ref().is_some_and(Vec::is_empty);
        Ok(ModelGenerateResponse {
            content: text(if no_tools {
                "Confirmed: the read finished. The requested write remains undone."
            } else {
                "Reading"
            }),
            tool_calls: if no_tools {
                None
            } else {
                Some(vec![ToolCall {
                    id: "budget-read".into(),
                    name: "read".into(),
                    args: BTreeMap::new(),
                }])
            },
            usage: usage(),
            stop_reason: if no_tools {
                ModelStopReason::End
            } else {
                ModelStopReason::ToolCall
            },
        })
    }
}

fn budget_config(limit: usize) -> AgentConfig {
    let mut config = config();
    config.system = Some("Preserve the owner's identity guidance.".into());
    config.tools = Some(vec![ToolDescriptor {
        name: "read".into(),
        description: "Read the local input".into(),
        parameters_schema: BTreeMap::new(),
        examples: None,
    }]);
    config.settings = Some(crate::agent::AgentSettings {
        max_tool_iterations: Some(limit),
        temperature: Some(0.2),
        max_tokens: Some(128),
        ..Default::default()
    });
    config
}

#[test]
fn budget_wrapup_restricts_the_already_next_call_and_preserves_grants_on_reset() {
    let model = Arc::new(BudgetWrapupModel::default());
    let original = budget_config(1);
    let mut runtime = AgentRuntime::new(original.clone(), model.clone());
    runtime.init();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let first = block_on(runtime.run_with_context_and_tools(
        Vec::new(),
        text("Read then write"),
        |_, _, _| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { TaskResult::success(text("Read completed; no write performed."), 1) }
        },
    ));
    let requests = model.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "one ordinary call and its existing next call"
    );
    assert_eq!(requests[1].0.tools, Some(Vec::new()), "M23: budget boundary must explicitly remove advertised tools before asking for the final answer");
    assert_eq!(requests[0].0, original);
    assert_eq!(requests[1].0.model, original.model);
    assert_eq!(requests[1].1.temperature, Some(0.2));
    assert_eq!(requests[1].1.max_tokens, Some(128));
    assert!(requests[1]
        .1
        .system
        .contains("Preserve the owner's identity guidance."));
    assert!(requests[1].1.system.contains("Tool-round budget exhausted"));
    assert!(requests[1].1.system.contains("remaining"));
    assert_eq!(
        requests[1]
            .1
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        1,
        "wrap-up must not invent an owner instruction"
    );
    assert!(requests[1]
        .1
        .messages
        .iter()
        .any(|message| message.role == MessageRole::Tool
            && message.content.text.contains("Read completed")));
    drop(requests);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(first.status, TaskStatus::Success);
    assert_eq!(
        first
            .data
            .as_ref()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .get("budgetWrapup"),
        Some(&DataValue::Bool(true))
    );
    assert_eq!(runtime.config(), &original);
    assert_eq!(runtime.snapshot().state.config, original);
    assert_eq!(runtime.state().token_usage.total_tokens, 24);
    let mut restored = AgentRuntime::from_snapshot(runtime.snapshot(), model.clone());
    let second = block_on(restored.run_with_context_and_tools(
        Vec::new(),
        text("Next independent task"),
        |_, _, _| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { TaskResult::success(text("Read completed again"), 1) }
        },
    ));
    assert_eq!(second.status, TaskStatus::Success);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        model.requests.lock().unwrap()[2].0,
        original,
        "the next run resets its one-shot state and capability"
    );
    assert_eq!(restored.config(), &original);
}

#[test]
fn budget_wrapup_zero_shadows_absent_tools_and_dispatches_nothing() {
    let model = Arc::new(BudgetWrapupModel::default());
    let mut original = budget_config(0);
    original.tools = None;
    let mut runtime = AgentRuntime::new(original.clone(), model.clone());
    let result = block_on(runtime.run_with_context_and_tools(
        Vec::new(),
        text("Explain remaining work"),
        |_, _, _| async { panic!("zero tool-round budget must never dispatch a tool") },
    ));
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].0.tools,
        Some(Vec::new()),
        "M23: tools=None must not allow adapter defaults during wrap-up"
    );
    assert_eq!(result.status, TaskStatus::Success);
    assert_eq!(
        result
            .data
            .as_ref()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .get("budgetWrapup"),
        Some(&DataValue::Bool(true))
    );
    assert_eq!(runtime.config(), &original);
}

struct BudgetBoundaryModel {
    responses: std::sync::Mutex<std::collections::VecDeque<Result<ModelGenerateResponse, String>>>,
    requests: std::sync::Mutex<Vec<(AgentConfig, ModelGenerateRequest)>>,
}

impl BudgetBoundaryModel {
    fn new(responses: Vec<Result<ModelGenerateResponse, String>>) -> Arc<Self> {
        Arc::new(Self {
            responses: std::sync::Mutex::new(responses.into()),
            requests: std::sync::Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl ModelAdapter for BudgetBoundaryModel {
    fn provider(&self) -> &str {
        "budget-boundary"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.requests
            .lock()
            .unwrap()
            .push((config.clone(), request.clone()));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("no unplanned model request")
    }
}

fn budget_reply(stop_reason: ModelStopReason, value: &str, calls: bool) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: Content {
            text: value.into(),
            metadata: Some(BTreeMap::from([(
                "budgetWrapup".into(),
                DataValue::Bool(true),
            )])),
            ..Content::default()
        },
        tool_calls: calls.then(|| {
            vec![ToolCall {
                id: "boundary-read".into(),
                name: "read".into(),
                args: BTreeMap::new(),
            }]
        }),
        usage: usage(),
        stop_reason,
    }
}

#[test]
fn budget_wrapup_rejects_tools_empty_and_truncated_replies_without_retry_or_unpaired_calls() {
    for (reason, value, tool_calls, expected) in [
        (
            ModelStopReason::ToolCall,
            "need another action",
            true,
            "tool iteration limit exceeded",
        ),
        (
            ModelStopReason::End,
            "pretend this is final",
            true,
            "tool iteration limit exceeded",
        ),
        (
            ModelStopReason::End,
            "  \n\t",
            false,
            "budget wrap-up returned empty text",
        ),
        (
            ModelStopReason::MaxTokens,
            "partial wrap-up",
            false,
            "budget wrap-up was truncated",
        ),
    ] {
        let model = BudgetBoundaryModel::new(vec![
            Ok(budget_reply(ModelStopReason::ToolCall, "read first", true)),
            Ok(budget_reply(reason, value, tool_calls)),
            Ok(budget_reply(ModelStopReason::End, "forbidden retry", false)),
        ]);
        let mut runtime = AgentRuntime::new(budget_config(1), model.clone());
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result = block_on(runtime.run_with_context_and_tools(
            Vec::new(),
            text("Read then write"),
            |_, _, _| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { TaskResult::success(text("read complete"), 1) }
            },
        ));
        assert_eq!(result.status, TaskStatus::Error, "{reason:?}");
        assert_eq!(result.error.as_deref(), Some(expected));
        assert_eq!(model.requests.lock().unwrap().len(), 2);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(runtime.state().token_usage.total_tokens, 24);
        assert_eq!(
            runtime
                .messages()
                .iter()
                .filter(|message| message.role == MessageRole::Tool)
                .count(),
            1
        );
        assert_eq!(
            runtime
                .messages()
                .iter()
                .filter(|message| message
                    .content
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.contains_key("toolCalls")))
                .count(),
            1,
            "rejected wrap-up must not record unpaired assistant calls"
        );
        assert!(
            runtime.messages().iter().all(|message| !message
                .content
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.contains_key("budgetWrapup"))),
            "provider metadata cannot fabricate accepted wrap-up provenance"
        );
        assert!(runtime
            .messages()
            .last()
            .unwrap()
            .content
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.get("incomplete") == Some(&DataValue::Bool(true))));
    }
}

struct BudgetRetryEvaluator;

#[async_trait]
impl crate::components::Evaluator for BudgetRetryEvaluator {
    fn name(&self) -> &str {
        "budget-retry"
    }
    fn description(&self) -> &str {
        "Reject the one-shot wrap-up draft"
    }
    async fn validate(&self, _: &AgentRuntime, _: &Message) -> Result<bool, String> {
        Ok(true)
    }
    async fn evaluate(
        &self,
        _: &AgentRuntime,
        _: &Message,
        _: &Content,
    ) -> Result<crate::components::EvaluatorResult, String> {
        Ok(crate::components::EvaluatorResult::retry(
            "correct the draft",
        ))
    }
}

#[test]
fn budget_wrapup_evaluator_retry_is_terminal_and_never_tagged_accepted() {
    let model = BudgetBoundaryModel::new(vec![
        Ok(budget_reply(ModelStopReason::ToolCall, "read first", true)),
        Ok(budget_reply(
            ModelStopReason::End,
            "a rejected final draft",
            false,
        )),
        Ok(budget_reply(
            ModelStopReason::End,
            "forbidden evaluator retry",
            false,
        )),
    ]);
    let mut runtime = AgentRuntime::new(budget_config(1), model.clone());
    runtime.register_evaluator(Arc::new(BudgetRetryEvaluator));
    let result = block_on(runtime.run_with_context_and_tools(
        Vec::new(),
        text("Read"),
        |_, _, _| async { TaskResult::success(text("read complete"), 1) },
    ));
    assert_eq!(
        result.error.as_deref(),
        Some("evaluator retry limit exceeded")
    );
    assert_eq!(model.requests.lock().unwrap().len(), 2);
    assert!(runtime.messages().iter().all(|message| !message
        .content
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.contains_key("budgetWrapup"))));
    assert_eq!(
        runtime
            .messages()
            .iter()
            .filter(|message| message.role == MessageRole::System)
            .count(),
        0,
        "there is no evaluator retry turn"
    );
}

#[test]
fn budget_wrapup_can_report_an_ordinary_tool_error_but_never_recovers_provider_failure() {
    let model = BudgetBoundaryModel::new(vec![
        Ok(budget_reply(ModelStopReason::ToolCall, "read first", true)),
        Ok(budget_reply(
            ModelStopReason::End,
            "The read failed; the requested work remains unfinished.",
            false,
        )),
    ]);
    let mut runtime = AgentRuntime::new(budget_config(1), model.clone());
    let result = block_on(runtime.run_with_context_and_tools(
        Vec::new(),
        text("Read"),
        |_, _, _| async { TaskResult::error("file unavailable", 1) },
    ));
    assert_eq!(result.status, TaskStatus::Success);
    assert!(model.requests.lock().unwrap()[1]
        .1
        .messages
        .iter()
        .any(|message| message.role == MessageRole::Tool
            && message.content.text.contains("file unavailable")));
    assert_eq!(
        result
            .data
            .as_ref()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .get("budgetWrapup"),
        Some(&DataValue::Bool(true))
    );

    for failure_on_wrapup in [false, true] {
        let mut responses = Vec::new();
        if failure_on_wrapup {
            responses.push(Ok(budget_reply(
                ModelStopReason::ToolCall,
                "read first",
                true,
            )));
        }
        responses.push(Err("provider disconnected".into()));
        responses.push(Ok(budget_reply(
            ModelStopReason::End,
            "forbidden recovery",
            false,
        )));
        let model = BudgetBoundaryModel::new(responses);
        let mut runtime = AgentRuntime::new(budget_config(1), model.clone());
        let result = block_on(runtime.run_with_context_and_tools(
            Vec::new(),
            text("Read"),
            |_, _, _| async { TaskResult::success(text("read complete"), 1) },
        ));
        assert_eq!(result.error.as_deref(), Some("provider disconnected"));
        assert_eq!(
            model.requests.lock().unwrap().len(),
            if failure_on_wrapup { 2 } else { 1 }
        );
    }
}

#[test]
fn budget_wrapup_is_not_claimed_for_natural_answers_and_default_budget_is_eight_rounds() {
    let model = BudgetBoundaryModel::new(vec![Ok(budget_reply(
        ModelStopReason::End,
        "natural completion",
        false,
    ))]);
    let mut runtime = AgentRuntime::new(budget_config(1), model.clone());
    let result = block_on(runtime.run_with_context_and_tools(
        Vec::new(),
        text("Explain"),
        |_, _, _| async { panic!("natural answer needs no tool") },
    ));
    assert_eq!(result.status, TaskStatus::Success);
    assert!(!result
        .data
        .as_ref()
        .unwrap()
        .metadata
        .as_ref()
        .unwrap()
        .contains_key("budgetWrapup"));
    assert_eq!(model.requests.lock().unwrap()[0].0, budget_config(1));

    let model = Arc::new(BudgetWrapupModel::default());
    let mut config = budget_config(1);
    config.settings.as_mut().unwrap().max_tool_iterations = None;
    let mut runtime = AgentRuntime::new(config.clone(), model.clone());
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let result =
        block_on(
            runtime.run_with_context_and_tools(Vec::new(), text("Read"), |_, _, _| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { TaskResult::success(text("read complete"), 1) }
            }),
        );
    assert_eq!(result.status, TaskStatus::Success);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 8);
    assert_eq!(model.requests.lock().unwrap().len(), 9);
    assert_eq!(runtime.config(), &config);
}

#[test]
fn budget_wrapup_counts_a_parallel_last_batch_as_one_round_and_keeps_paired_results() {
    let mut batch = budget_reply(ModelStopReason::ToolCall, "two reads", false);
    batch.tool_calls = Some(
        ["read-a", "read-b"]
            .into_iter()
            .map(|id| ToolCall {
                id: id.into(),
                name: "read".into(),
                args: BTreeMap::new(),
            })
            .collect(),
    );
    let model = BudgetBoundaryModel::new(vec![
        Ok(batch),
        Ok(budget_reply(
            ModelStopReason::End,
            "Both reads completed; the write remains undone.",
            false,
        )),
    ]);
    let mut runtime = AgentRuntime::new(budget_config(1), model.clone());
    let dispatched = std::sync::atomic::AtomicUsize::new(0);
    let result = block_on(runtime.run_with_context_and_tools(
        Vec::new(),
        text("Read two files then write"),
        |_, _, call| {
            dispatched.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move { TaskResult::success(text(&format!("{} completed", call.id)), 1) }
        },
    ));
    assert_eq!(result.status, TaskStatus::Success);
    assert_eq!(dispatched.load(std::sync::atomic::Ordering::SeqCst), 2);
    let requests = model.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "two parallel tools still consume only one tool round"
    );
    assert_eq!(requests[1].0.tools, Some(Vec::new()));
    let tool_results: Vec<_> = requests[1]
        .1
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .collect();
    assert_eq!(tool_results.len(), 2);
    for (message, id) in tool_results.iter().zip(["read-a", "read-b"]) {
        assert_eq!(
            message.content.metadata.as_ref().unwrap().get("toolCallId"),
            Some(&DataValue::String(id.into()))
        );
        assert!(message.content.text.contains(id));
    }
    assert_eq!(
        runtime
            .messages()
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count(),
        2
    );
    assert_eq!(
        result
            .data
            .as_ref()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .get("budgetWrapup"),
        Some(&DataValue::Bool(true))
    );
}
