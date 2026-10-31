use super::fixture_tools::FixtureWorkspace;
use anima_core::task_regressions::{ActorFixture, MockEffect};
use anima_core::{DataValue, TaskStatus, ToolCall};
use std::collections::{BTreeMap, BTreeSet};

use super::contracts::{
    CompareRequest, RegressionError, RegressionReport, RunStatus, ToolPolicy, VariantSelection,
};
use super::TaskRegressionService;
use anima_core::task_regressions::{FixtureOracle, RegressionFixture, RegressionSuite};
use anima_core::trace_evaluation::TraceVariant;
use anima_core::{
    AgentConfig, Content, MessageRole, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, ModelStreamFrame, ModelStreamSink, TokenUsage,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use crate::agent_runs::test_support::Gate;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::state::DaemonState;

fn actor() -> ActorFixture {
    ActorFixture {
        prompt: "Update the permitted file and record a synthetic effect.".into(),
        initial_files: BTreeMap::from([("input.txt".into(), "old".into())]),
        readable_files: vec!["input.txt".into()],
        writable_files: vec!["input.txt".into(), "answer.txt".into()],
        mock_effects: true,
    }
}

fn call(name: &str, fields: &[(&str, &str)]) -> ToolCall {
    ToolCall {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        args: fields
            .iter()
            .map(|(key, value)| ((*key).into(), DataValue::String((*value).into())))
            .collect(),
    }
}

#[test]
fn task_regressions_tools_change_real_files_and_record_only_mock_effects() {
    let workspace = FixtureWorkspace::new(&actor(), true).unwrap();
    assert_eq!(
        workspace
            .execute(call(
                "replace_file",
                &[("path", "input.txt"), ("old", "old"), ("new", "new")]
            ))
            .status,
        TaskStatus::Success
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("input.txt")).unwrap(),
        "new"
    );
    assert_eq!(
        workspace
            .execute(call(
                "record_effect",
                &[("key", "demo"), ("payload", "synthetic")]
            ))
            .status,
        TaskStatus::Success
    );
    assert_eq!(
        workspace.effects(),
        vec![MockEffect {
            key: "demo".into(),
            payload: "synthetic".into()
        }]
    );
    assert_eq!(
        workspace
            .execute(call(
                "record_effect",
                &[("key", "demo"), ("payload", "synthetic")]
            ))
            .status,
        TaskStatus::Success
    );
    assert_eq!(
        workspace.effects().len(),
        1,
        "same mock idempotency key must not repeat an effect"
    );
    assert_eq!(
        workspace
            .execute(call(
                "record_effect",
                &[("key", "demo"), ("payload", "different")]
            ))
            .status,
        TaskStatus::Error
    );
}

#[test]
fn task_regressions_actor_permissions_do_not_depend_on_private_expectations() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    for path in [
        "private-oracle.txt",
        "../outside",
        "C:secret",
        "CON",
        "input.txt:stream",
    ] {
        let result = workspace.execute(call("read_file", &[("path", path)]));
        assert_eq!(result.status, TaskStatus::Error, "denied {path}");
        assert!(!result.error.unwrap_or_default().contains("expected"));
    }
    assert_eq!(
        workspace
            .execute(call("record_effect", &[("key", "demo"), ("payload", "x")]))
            .status,
        TaskStatus::Error
    );
    assert!(!workspace
        .descriptors()
        .iter()
        .any(|tool| tool.name == "record_effect"));
}

#[test]
fn task_regressions_workspace_rejects_linked_files() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("private.txt");
    std::fs::write(&victim, "private-witness").unwrap();
    std::fs::remove_file(workspace.path().join("input.txt")).unwrap();
    std::fs::hard_link(&victim, workspace.path().join("input.txt")).unwrap();
    assert_eq!(
        workspace
            .execute(call(
                "replace_file",
                &[
                    ("path", "input.txt"),
                    ("old", "private"),
                    ("new", "changed")
                ]
            ))
            .status,
        TaskStatus::Error
    );
    assert_eq!(std::fs::read_to_string(victim).unwrap(), "private-witness");
}

/// This adapter has no suite or oracle reference. Its arithmetic input comes only
/// from the actual public read_file result in the runtime's next model request.
#[derive(Default)]
struct NumericModel {
    calls: StdMutex<Vec<(AgentConfig, ModelGenerateRequest)>>,
    first_call_gate: StdMutex<Option<Gate>>,
    indexed_call_gate: StdMutex<Option<(usize, Gate)>>,
    repetition_gate: Option<Gate>,
    probe_private_path: bool,
}

impl NumericModel {
    fn reply(
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let tool_result = |id: &str| {
            request.messages.iter().find(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String(id.into()))
            })
        };
        let Some(numbers) = tool_result("read-numbers") else {
            return Ok(Self::tools(ToolCall {
                id: "read-numbers".into(),
                ..call("read_file", &[("path", "numbers.txt")])
            }));
        };
        let values = numbers
            .content
            .text
            .split_whitespace()
            .map(|part| {
                part.parse::<i64>()
                    .map_err(|_| "public numeric input is malformed".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty() {
            return Err("public numeric input is empty".into());
        }
        let total = values.iter().sum::<i64>() + i64::from(config.model == "sum-off-by-one");
        let answer = total.to_string();
        if tool_result("write-answer").is_none() {
            return Ok(Self::tools(ToolCall {
                id: "write-answer".into(),
                ..call(
                    "write_file",
                    &[("path", "answer.txt"), ("content", &answer)],
                )
            }));
        }
        Ok(ModelGenerateResponse {
            content: Content {
                text: answer,
                ..Content::default()
            },
            tool_calls: None,
            usage: Self::usage(),
            stop_reason: ModelStopReason::End,
        })
    }

    fn usage() -> TokenUsage {
        TokenUsage {
            prompt_tokens: 13,
            completion_tokens: 7,
            total_tokens: 20,
            ..TokenUsage::default()
        }
    }

    fn tools(tool_call: ToolCall) -> ModelGenerateResponse {
        ModelGenerateResponse {
            content: Content::default(),
            tool_calls: Some(vec![tool_call]),
            usage: Self::usage(),
            stop_reason: ModelStopReason::ToolCall,
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().expect("numeric model capture lock").len()
    }
}

#[async_trait]
impl ModelAdapter for NumericModel {
    fn provider(&self) -> &str {
        "openai"
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
        let call_index = {
            let mut calls = self.calls.lock().expect("numeric model capture lock");
            calls.push((config.clone(), request.clone()));
            calls.len()
        };
        let indexed_gate = {
            let mut pending = self
                .indexed_call_gate
                .lock()
                .expect("indexed model gate lock");
            if pending
                .as_ref()
                .is_some_and(|(index, _)| *index == call_index)
            {
                pending.take().map(|(_, gate)| gate)
            } else {
                None
            }
        };
        let gate = self
            .first_call_gate
            .lock()
            .expect("numeric model gate lock")
            .take()
            .or(indexed_gate);
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("test gate remains open")
                .forget();
        }
        if self.probe_private_path
            && !request.messages.iter().any(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String("probe-private".into()))
            })
        {
            return Ok(Self::tools(ToolCall {
                id: "probe-private".into(),
                ..call("read_file", &[("path", "private-oracle.txt")])
            }));
        }
        Self::reply(config, request)
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let response = self.generate(config, request).await?;
        sink.emit(ModelStreamFrame::Served {
            provider: "openai".into(),
            model: config.model.clone(),
            reason: "in-process deterministic test adapter".into(),
        })
        .await?;
        if let Some(gate) = &self.repetition_gate {
            // An admitted producer deliberately ignores consumer failure. The
            // host must keep custody until this producer physically retires.
            let _ = sink
                .emit(ModelStreamFrame::TextDelta(
                    "repeating line with no progress\n".repeat(700),
                ))
                .await;
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("repetition gate remains open")
                .forget();
            let _ = sink.complete_usage(response.usage.clone()).await;
            let _ = sink.emit(ModelStreamFrame::Final(response)).await;
            return Ok(());
        }
        sink.complete_usage(response.usage.clone()).await?;
        sink.emit(ModelStreamFrame::Final(response)).await
    }
}

/// Pauses after the normal controlled model path has finished its own
/// settlement, making the host's subsequent child-dispatch settlement the
/// next accounting save without replacing any budget or transport behavior.
struct ModelRetirementGate {
    inner: Arc<NumericModel>,
    call_index: usize,
    gate: Gate,
}

#[async_trait]
impl ModelAdapter for ModelRetirementGate {
    fn provider(&self) -> &str {
        self.inner.provider()
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.inner.generate(config, request).await
    }

    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<(), String> {
        let result = self
            .inner
            .stream_controlled(config, request, sink, context)
            .await;
        if self.inner.call_count() == self.call_index {
            self.gate.entered.add_permits(1);
            self.gate
                .release
                .acquire()
                .await
                .expect("model retirement gate stays open")
                .forget();
        }
        result
    }
}

fn numeric_suite() -> RegressionSuite {
    // Expected answers are independently authored literals. The adapter never
    // sees these values and must read the public file to calculate its answer.
    RegressionSuite {
        name: "Two independent numeric file tasks".into(),
        fixtures: [("signed", "17\n-4\n32\n", "45"), ("subtraction", "100\n-25\n-18\n", "57")]
            .into_iter().map(|(id, input, expected)| RegressionFixture {
                id: id.into(),
                actor: ActorFixture {
                    prompt: "Read numbers.txt, add every signed integer, write the decimal sum to answer.txt, and reply with only that sum. Preserve keep.txt.".into(),
                    initial_files: BTreeMap::from([("numbers.txt".into(), input.into()), ("keep.txt".into(), "unrelated fixture content".into())]),
                    readable_files: vec!["numbers.txt".into()],
                    writable_files: vec!["answer.txt".into()],
                    mock_effects: false,
                },
                oracle: FixtureOracle {
                    expected_files: BTreeMap::from([
                        ("numbers.txt".into(), input.into()),
                        ("keep.txt".into(), "unrelated fixture content".into()),
                        ("answer.txt".into(), expected.into()),
                    ]),
                    expected_output: expected.into(),
                    expected_mock_effects: Vec::new(),
                },
            }).collect(),
    }
}

struct Harness {
    state: crate::app::SharedDaemonState,
    service: TaskRegressionService,
    model: Arc<NumericModel>,
    agent_id: String,
    root: tempfile::TempDir,
    store: ControlPlaneStoreConfig,
}

impl Harness {
    async fn new(model: Arc<NumericModel>) -> Self {
        Self::with_adapter(model.clone(), model).await
    }

    async fn with_adapter(model: Arc<NumericModel>, adapter: Arc<dyn ModelAdapter>) -> Self {
        let root = tempfile::Builder::new()
            .prefix("anima-task-regression-test-")
            .tempdir()
            .expect("private test directory");
        std::fs::write(
            root.path().join("canonical.txt"),
            "owner workspace sentinel",
        )
        .expect("canonical sentinel");
        let store = ControlPlaneStoreConfig::Json(root.path().join("control.json"));
        let mut daemon = crate::skills::test_support::with_workspace(
            DaemonState::with_model_adapter(adapter),
            root.path(),
        );
        let mut config = crate::agent_runs::test_support::companion_config("Owner source");
        config.system = Some("Use only the isolated fixture's permitted tools.".into());
        let agent_id = daemon
            .create_agent(config)
            .expect("saved owner source")
            .state
            .id;
        crate::sessions::test_support::seed_messages(
            &mut daemon,
            &agent_id,
            vec![crate::sessions::test_support::message(
                &agent_id,
                "owner-sentinel-message",
                "chat:owner",
                MessageRole::User,
                "Canonical owner transcript sentinel",
                1,
            )],
        );
        daemon.sessions.insert(crate::sessions::SessionRecord::new(
            &agent_id,
            "chat:owner",
            crate::sessions::SessionKind::Chat,
            crate::sessions::SessionOrigin::Web,
            "Owner conversation".into(),
            crate::sessions::TitleSource::Owner,
            1,
        ));
        daemon
            .memory
            .write()
            .await
            .add(anima_memory::NewMemory {
                agent_id: agent_id.clone(),
                agent_name: "Owner source".into(),
                memory_type: anima_memory::MemoryType::Fact,
                content: "Canonical owner memory sentinel".into(),
                importance: 0.8,
                tags: None,
                scope: Some(anima_memory::MemoryScope::Private),
                room_id: None,
                world_id: None,
                session_id: None,
            })
            .expect("owner memory sentinel");
        daemon.set_control_plane_store(Some(store.clone()));
        let state = Arc::new(RwLock::new(daemon));
        let service =
            TaskRegressionService::new(state.clone(), Arc::new(Mutex::new(())), Arc::new(|| true));
        Self {
            state,
            service,
            model,
            agent_id,
            root,
            store,
        }
    }

    async fn request(&self, suite: RegressionSuite, key: &str) -> CompareRequest {
        let suite = self
            .service
            .define(suite)
            .await
            .expect("suite definition is durable");
        let source = self
            .service
            .configuration(&self.agent_id)
            .await
            .expect("owner source selection");
        let selection = |model: &str| VariantSelection {
            agent_id: self.agent_id.clone(),
            config_digest: source.config_digest.clone(),
            model: Some(model.into()),
            provider: None,
            system: None,
            tool_policy: ToolPolicy::Files,
        };
        CompareRequest {
            suite_id: suite.id,
            suite_digest: suite.digest,
            baseline: selection("sum-off-by-one"),
            candidate: selection("sum-correct"),
            repetitions: 2,
            request_key: key.into(),
        }
    }

    async fn retired(&self, id: &str) -> RegressionReport {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let report = self
                    .service
                    .report(id)
                    .await
                    .expect("retained comparison report");
                if report.retired_at_ms.is_some()
                    && self
                        .state
                        .read()
                        .await
                        .task_regression_slots
                        .available_permits()
                        == 1
                {
                    return report;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("comparison physically retires within ten seconds")
    }
}

#[tokio::test]
async fn task_regressions_ordinary_runtime_reads_derives_writes_and_regrades_complete_matrix() {
    let harness = Harness::new(Arc::new(NumericModel::default())).await;
    let request = harness.request(numeric_suite(), "numeric-matrix").await;
    let (canonical_agents, canonical_sessions, canonical_memory) = {
        let state = harness.state.read().await;
        let snapshot = state.control_plane_snapshot();
        let memory = state.memory.read().await.snapshot();
        (
            serde_json::to_value(snapshot.agents).expect("agent snapshot"),
            snapshot.sessions,
            memory,
        )
    };
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("explicit comparison admission");
    let report = harness.retired(&started.id).await;
    assert_eq!(report.status, RunStatus::Completed, "{:?}", report.error);
    assert_eq!((report.planned_trials, report.completed_trials), (8, 8));
    assert!(report.usage_complete);
    assert_eq!(
        report.baseline.config_digest,
        report.candidate.config_digest
    );
    assert_ne!(
        report.baseline.effective_config_digest,
        report.candidate.effective_config_digest
    );
    assert_eq!(report.suite.digest, request.suite_digest);
    assert_eq!(report.evidence_scope, "selectedAdapterTasks");
    let comparison = report
        .comparison
        .as_ref()
        .expect("complete evidence can be independently regraded");
    assert!(comparison.matrix_complete);
    assert_eq!(
        (comparison.baseline.trials, comparison.baseline.correct),
        (4, 0)
    );
    assert_eq!(
        (comparison.candidate.trials, comparison.candidate.correct),
        (4, 4)
    );
    for row in &report.observations {
        let expected = match (row.fixture_id.as_str(), row.variant) {
            ("signed", TraceVariant::Baseline) => "46",
            ("signed", TraceVariant::Candidate) => "45",
            ("subtraction", TraceVariant::Baseline) => "58",
            ("subtraction", TraceVariant::Candidate) => "57",
            other => panic!("unexpected trial {other:?}"),
        };
        assert_eq!(
            row.observed_files.get("answer.txt").map(String::as_str),
            Some(expected)
        );
        assert_eq!(row.final_output.as_deref(), Some(expected));
        assert_eq!(
            row.observed_files.get("keep.txt").map(String::as_str),
            Some("unrelated fixture content")
        );
        assert_eq!(
            (
                row.metrics.model_calls,
                row.metrics.tool_calls,
                row.metrics.tool_errors
            ),
            (3, 2, 0)
        );
        assert_eq!(
            (
                row.metrics.known_usage_calls,
                row.metrics.unknown_usage_calls
            ),
            (3, 0)
        );
    }
    assert_eq!(harness.model.call_count(), 24);
    let state = harness.state.read().await;
    for observation in &state.task_regressions.runs[0].observations {
        assert_eq!(
            observation.graded.trace.settings.fixture_set_id,
            request.suite_digest
        );
        assert_eq!(
            observation
                .graded
                .trace
                .events
                .iter()
                .filter(|event| matches!(
                    event.event,
                    anima_core::trace_evaluation::TraceEventKind::Served { .. }
                ))
                .count(),
            3
        );
    }
    let snapshot = state.control_plane_snapshot();
    assert_eq!(
        serde_json::to_value(snapshot.agents).expect("agent snapshot"),
        canonical_agents
    );
    assert_eq!(snapshot.sessions, canonical_sessions);
    assert_eq!(state.memory.read().await.snapshot(), canonical_memory);
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("canonical.txt"))
            .expect("canonical sentinel"),
        "owner workspace sentinel"
    );
    drop(state);
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("durable report file")
        .expect("persisted report snapshot");
    let saved_run = saved
        .task_regressions
        .runs
        .iter()
        .find(|run| run.id == report.id)
        .expect("saved completed report");
    assert_eq!(saved_run.observations.len(), 8);
    assert_eq!(saved_run.status, RunStatus::Completed);
    assert_eq!(
        saved_run.observations[0].graded.observed_files,
        report.observations[0].observed_files
    );
}

#[tokio::test]
async fn task_regressions_duplicate_request_joins_reserved_worker_without_double_dispatch() {
    let model = Arc::new(NumericModel::default());
    let gate = Gate::new();
    *model.first_call_gate.lock().expect("model gate lock") = Some(gate.clone());
    let harness = Harness::new(model).await;
    let request = harness.request(numeric_suite(), "same-owner-request").await;
    let first = harness
        .service
        .start(request.clone())
        .await
        .expect("first admission");
    gate.entered().await;
    let duplicate = harness
        .service
        .start(request.clone())
        .await
        .expect("duplicate joins admission");
    assert_eq!(duplicate.id, first.id);
    assert_eq!(harness.model.call_count(), 1);
    {
        let state = harness.state.read().await;
        assert_eq!(state.task_regressions.runs.len(), 1);
        assert_eq!(state.task_regressions.reservations.len(), 1);
        assert_eq!(state.task_regression_slots.available_permits(), 0);
    }
    let mut changed = request.clone();
    changed.candidate.model = Some("different-model".into());
    assert!(matches!(
        harness.service.start(changed).await,
        Err(RegressionError::Conflict(_))
    ));
    gate.release();
    let report = harness.retired(&first.id).await;
    assert_eq!(report.status, RunStatus::Completed);
    assert_eq!(harness.model.call_count(), 24);
    let after = harness
        .service
        .start(request)
        .await
        .expect("completed request joins saved report");
    assert_eq!(after.id, first.id);
    assert_eq!(after.completed_trials, 8);
    assert_eq!(harness.model.call_count(), 24);
}

#[tokio::test]
async fn task_regressions_private_oracle_never_enters_model_requests_or_denied_tool_results() {
    const CANARY: &str = "HIDDEN_ORACLE_CANARY_8e1cb410";
    let model = Arc::new(NumericModel {
        probe_private_path: true,
        ..NumericModel::default()
    });
    let harness = Harness::new(model).await;
    let mut suite = numeric_suite();
    suite.fixtures.truncate(1);
    suite.fixtures[0].oracle.expected_output = CANARY.into();
    suite.fixtures[0]
        .oracle
        .expected_files
        .insert("private-oracle.txt".into(), CANARY.into());
    let mut request = harness.request(suite, "hidden-expectation").await;
    request.repetitions = 1;
    let started = harness
        .service
        .start(request)
        .await
        .expect("canary suite admission");
    let report = harness.retired(&started.id).await;
    assert_eq!(report.completed_trials, 2);
    let comparison = report
        .comparison
        .expect("both attempts retain independently regradable observations");
    assert_eq!(
        comparison.candidate.correct, 0,
        "the hidden oracle is not replaced with model output"
    );
    let calls = harness.model.calls.lock().expect("model capture lock");
    assert_eq!(calls.len(), 8);
    let mut denials = 0;
    for (config, request) in calls.iter() {
        assert!(!format!("{config:?}\n{request:?}").contains(CANARY));
        assert!(
            !request.system.contains("Canonical owner"),
            "canonical memory and history are not task inputs"
        );
        for message in &request.messages {
            assert!(!message.content.text.contains("Canonical owner"));
            if message.role == MessageRole::Tool
                && message
                    .content
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("toolCallId"))
                    == Some(&DataValue::String("probe-private".into()))
            {
                assert!(message.content.text.contains("fixture_tool_or_path_denied"));
                assert!(!message.content.text.contains("expected"));
                assert!(!message.content.text.contains("private-oracle.txt"));
                denials += 1;
            }
        }
    }
    assert!(
        denials >= 2,
        "both variants must actually observe the denied public tool response"
    );
    assert!(report
        .observations
        .iter()
        .all(|row| !row.observed_files.contains_key("private-oracle.txt")));
    assert!(report
        .observations
        .iter()
        .all(|row| row.metrics.tool_errors == 1));
}

#[tokio::test]
async fn task_regressions_cancellation_holds_noncooperative_adapter_until_late_retirement() {
    let model = Arc::new(NumericModel::default());
    let gate = Gate::new();
    *model.first_call_gate.lock().expect("model gate lock") = Some(gate.clone());
    let harness = Harness::new(model).await;
    let request = harness.request(numeric_suite(), "cancel-numeric").await;
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("comparison admission");
    gate.entered().await;
    let cancelled = harness
        .service
        .cancel(&started.id)
        .await
        .expect("durable logical cancellation");
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    assert!(cancelled.finished_at_ms.is_some());
    assert!(
        cancelled.retired_at_ms.is_none(),
        "logical cancellation cannot claim physical adapter retirement"
    );
    assert!(cancelled.comparison.is_none());
    assert!(!cancelled.usage_complete);
    assert_eq!(
        harness
            .state
            .read()
            .await
            .task_regression_slots
            .available_permits(),
        0
    );
    let mut replacement = request;
    replacement.request_key = "new-request-before-retirement".into();
    assert!(matches!(
        harness.service.start(replacement).await,
        Err(RegressionError::Conflict(_))
    ));
    assert_eq!(harness.model.call_count(), 1);
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("cancel storage")
        .expect("cancelled snapshot");
    let row = saved
        .task_regressions
        .runs
        .iter()
        .find(|run| run.id == started.id)
        .expect("cancelled run retained");
    assert_eq!(row.status, RunStatus::Cancelled);
    assert!(row.retired_at_ms.is_none());
    gate.release();
    let report = harness.retired(&started.id).await;
    assert_eq!(
        report.status,
        RunStatus::Cancelled,
        "late successful adapter output cannot revive cancellation"
    );
    assert!(report.comparison.is_none());
    assert!(report.completed_trials < report.planned_trials);
    assert_eq!(
        harness.model.call_count(),
        1,
        "late tool-call output must not dispatch another model step"
    );
    assert!(report
        .observations
        .iter()
        .all(|row| !row.observed_files.contains_key("answer.txt")));
    assert_eq!(
        harness
            .state
            .read()
            .await
            .task_regressions
            .reservations
            .len(),
        1
    );
}

#[tokio::test]
async fn task_regressions_restart_retains_suite_quota_and_interrupted_report_without_replay() {
    let model = Arc::new(NumericModel::default());
    let gate = Gate::new();
    *model.first_call_gate.lock().expect("model gate lock") = Some(gate.clone());
    let harness = Harness::new(model).await;
    let request = harness.request(numeric_suite(), "restart-numeric").await;
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("comparison admission");
    gate.entered().await;
    let snapshot = load_control_plane_snapshot(&harness.store)
        .await
        .expect("admission storage")
        .expect("durable admission snapshot");
    let fresh_model = Arc::new(NumericModel::default());
    let mut restored = DaemonState::with_model_adapter(fresh_model.clone());
    restored
        .restore_control_plane_snapshot(snapshot)
        .expect("valid saved owner state restores");
    assert_eq!(restored.task_regressions.suites.len(), 1);
    assert_eq!(restored.task_regressions.reservations.len(), 1);
    let restored_state = Arc::new(RwLock::new(restored));
    let service = TaskRegressionService::new(
        restored_state.clone(),
        Arc::new(Mutex::new(())),
        Arc::new(|| true),
    );
    let report = service.report(&started.id).await.expect("restart report");
    assert_eq!(report.status, RunStatus::Interrupted);
    assert!(report.finished_at_ms.is_some());
    assert!(report.comparison.is_none());
    assert!(!report.usage_complete);
    assert!(report
        .error
        .as_deref()
        .is_some_and(|message| message.contains("will not replay")));
    let duplicate = service
        .start(request)
        .await
        .expect("restart duplicate joins retained historical admission");
    assert_eq!(duplicate.id, started.id);
    assert_eq!(duplicate.status, RunStatus::Interrupted);
    assert_eq!(
        fresh_model.call_count(),
        0,
        "restore and duplicate reads never dispatch uncertain work"
    );
    assert!(restored_state
        .read()
        .await
        .task_regression_cancels
        .is_empty());
    harness
        .service
        .cancel(&started.id)
        .await
        .expect("original test worker stops");
    gate.release();
    harness.retired(&started.id).await;
}

#[tokio::test]
async fn task_regressions_malformed_or_changed_source_selection_refuses_before_dispatch_or_quota() {
    let harness = Harness::new(Arc::new(NumericModel::default())).await;
    let request = harness.request(numeric_suite(), "selection-refusal").await;
    let mut wrong_agent = request.clone();
    wrong_agent.candidate.agent_id = "another-owner-source".into();
    let mut stale_source = request.clone();
    stale_source.baseline.config_digest = "0".repeat(64);
    let mut stale_suite = request.clone();
    stale_suite.suite_digest = "0".repeat(64);
    let mut unsupported = request.clone();
    unsupported.candidate.provider = Some("chatgpt".into());
    let mut missing = request.clone();
    missing.baseline.agent_id = "missing-source".into();
    missing.candidate.agent_id = "missing-source".into();
    let mut unbounded = request.clone();
    unbounded.repetitions = 4;
    let mut empty_key = request.clone();
    empty_key.request_key.clear();
    for (label, invalid) in [
        ("different source agents", wrong_agent),
        ("changed source digest", stale_source),
        ("changed suite digest", stale_suite),
        ("unsupported transport", unsupported),
        ("missing source", missing),
        ("unbounded repetitions", unbounded),
        ("empty request key", empty_key),
    ] {
        assert!(
            harness.service.start(invalid).await.is_err(),
            "{label} must refuse admission"
        );
    }
    let mut unknown_field = serde_json::to_value(request).expect("request wire shape");
    unknown_field["candidate"]["privateOracle"] = serde_json::json!("actor-controlled result");
    assert!(serde_json::from_value::<CompareRequest>(unknown_field).is_err());
    let state = harness.state.read().await;
    assert!(state.task_regressions.runs.is_empty());
    assert!(state.task_regressions.reservations.is_empty());
    assert!(state.task_regression_cancels.is_empty());
    assert_eq!(state.task_regression_slots.available_permits(), 1);
    assert_eq!(harness.model.call_count(), 0);
}

#[tokio::test]
async fn task_regressions_matrix_does_not_reset_saved_source_model_attempt_budget_per_trial() {
    let harness = Harness::new(Arc::new(NumericModel::default())).await;
    {
        let mut state = harness.state.write().await;
        let mut config = state
            .agents
            .get(&harness.agent_id)
            .expect("saved source")
            .config()
            .clone();
        let settings = config.settings.as_mut().expect("source settings");
        let mut limits = anima_core::execution_limits::ExecutionLimits::default();
        limits.max_model_attempts = 1;
        limits.max_retries = 0;
        settings.execution_limits = Some(limits);
        state.restore_agent_config(&harness.agent_id, config);
    }
    let request = harness
        .request(numeric_suite(), "retained-attempt-budget")
        .await;
    let started = harness
        .service
        .start(request)
        .await
        .expect("bounded comparison admission");
    let report = harness.retired(&started.id).await;
    assert_eq!(
        harness.model.call_count(),
        1,
        "the matrix shares one retained source allowance across every trial and variant"
    );
    assert_eq!(report.status, RunStatus::Inconclusive);
    assert!(report
        .comparison
        .as_ref()
        .is_none_or(|comparison| comparison.candidate.correct == 0));
    assert!(report
        .observations
        .iter()
        .all(|row| !row.observed_files.contains_key("answer.txt")));
    assert_eq!(
        harness
            .state
            .read()
            .await
            .task_regressions
            .reservations
            .len(),
        1
    );
}

fn usage_ids(rows: &[crate::usage::UsageRecord]) -> BTreeSet<String> {
    rows.iter().map(|row| row.id.clone()).collect()
}

async fn assert_usage_flushes_once(harness: &Harness, expected: &BTreeSet<String>) {
    let history = harness.state.read().await.history.clone();
    let now = anima_core::primitives::now_millis();
    history
        .flush_once(&harness.state, &harness.service.transactions, now)
        .await
        .expect("retained usage flushes");
    history
        .flush_once(&harness.state, &harness.service.transactions, now)
        .await
        .expect("repeated flush is idempotent");
    let rows = history
        .store()
        .page_usage(&crate::history::UsagePageQuery {
            from_ms: 0,
            to_ms: u64::MAX,
            agent_id: Some(harness.agent_id.clone()),
            session_id: None,
            before: None,
            limit: 10_000,
        })
        .await
        .expect("ordinary owner usage history");
    assert_eq!(
        rows.len(),
        expected.len(),
        "normal usage history retains one row per physical model call"
    );
    assert_eq!(&usage_ids(&rows), expected);
    assert!(history.pending_usage().is_empty());
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("usage acknowledgement storage")
        .expect("saved acknowledgement");
    assert!(
        saved.pending_usage.is_empty(),
        "flushed usage is durably acknowledged"
    );
}

#[tokio::test]
async fn task_regressions_repetition_consumer_drop_holds_producer_and_late_usage_custody() {
    let gate = Gate::new();
    let model = Arc::new(NumericModel {
        repetition_gate: Some(gate.clone()),
        ..NumericModel::default()
    });
    let harness = Harness::new(model).await;
    let request = harness
        .request(numeric_suite(), "repetition-consumer-drop")
        .await;
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("comparison admission");
    gate.entered().await;
    let logical = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let report = harness
                .service
                .report(&started.id)
                .await
                .expect("logical report");
            if report.status != RunStatus::Running {
                return report;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime repetition guard closes its consumer");
    assert_eq!(logical.status, RunStatus::Inconclusive);
    assert!(logical.finished_at_ms.is_some());
    assert!(
        logical.retired_at_ms.is_none(),
        "consumer failure cannot retire an admitted producer"
    );
    assert!(logical.comparison.is_none());
    assert!(!logical.usage_complete);
    assert_eq!(
        harness
            .state
            .read()
            .await
            .task_regression_slots
            .available_permits(),
        0
    );
    let mut another = request.clone();
    another.request_key = "repetition-replacement-before-retirement".into();
    assert!(matches!(
        harness.service.start(another).await,
        Err(RegressionError::Conflict(_))
    ));
    assert_eq!(harness.model.call_count(), 1);
    gate.release();
    let report = harness.retired(&started.id).await;
    assert_eq!(report.status, RunStatus::Inconclusive);
    assert!(
        report.comparison.is_none(),
        "late Final never restores outcome eligibility"
    );
    assert!(!report.usage_complete);
    assert_eq!(
        harness.model.call_count(),
        1,
        "consumer failure never replays the producer or the trial"
    );
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("late usage storage")
        .expect("retired report snapshot");
    let row = saved
        .task_regressions
        .runs
        .iter()
        .find(|run| run.id == started.id)
        .expect("retained failed trial");
    assert_eq!(row.status, RunStatus::Inconclusive);
    assert_eq!(row.usage_rows.len(), 1);
    assert_eq!(
        saved.pending_usage.len(),
        1,
        "post-consumer final provider usage survives retirement"
    );
    let ids = usage_ids(&saved.pending_usage);
    assert_eq!(ids, row.usage_rows.iter().cloned().collect());
    assert_eq!(
        saved.pending_usage[0].total_tokens,
        NumericModel::usage().total_tokens
    );
    assert_usage_flushes_once(&harness, &ids).await;
    let duplicate = harness
        .service
        .start(request)
        .await
        .expect("same failed request joins historical report");
    assert_eq!(duplicate.id, started.id);
    assert_eq!(duplicate.status, RunStatus::Inconclusive);
    assert_eq!(harness.model.call_count(), 1);
}

#[tokio::test]
async fn task_regressions_evicted_report_keeps_request_tombstone_across_restart() {
    let harness = Harness::new(Arc::new(NumericModel::default())).await;
    let request = harness
        .request(numeric_suite(), "evicted-report-replay")
        .await;
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("comparison admission");
    assert_eq!(
        harness.retired(&started.id).await.status,
        RunStatus::Completed
    );
    assert_eq!(harness.model.call_count(), 24);
    let persist = {
        let mut state = harness.state.write().await;
        assert_eq!(state.task_regressions.admissions.len(), 1);
        state.task_regressions.runs.clear();
        state.control_plane_persist_request()
    };
    persist.save().await.expect("report eviction is durable");
    assert!(matches!(
        harness.service.start(request.clone()).await,
        Err(RegressionError::Conflict(_))
    ));
    assert_eq!(
        harness.model.call_count(),
        24,
        "expired report lookup cannot replay already admitted work"
    );
    assert_eq!(
        harness
            .state
            .read()
            .await
            .task_regressions
            .reservations
            .len(),
        1
    );
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("eviction storage")
        .expect("saved eviction snapshot");
    assert!(saved.task_regressions.runs.is_empty());
    assert!(saved
        .task_regressions
        .admissions
        .contains_key(&request.request_key));
    let fresh_model = Arc::new(NumericModel::default());
    let mut restored = DaemonState::with_model_adapter(fresh_model.clone());
    restored
        .restore_control_plane_snapshot(saved)
        .expect("evicted report snapshot restores");
    let state = Arc::new(RwLock::new(restored));
    let service =
        TaskRegressionService::new(state.clone(), Arc::new(Mutex::new(())), Arc::new(|| true));
    assert!(matches!(
        service.start(request).await,
        Err(RegressionError::Conflict(_))
    ));
    assert_eq!(fresh_model.call_count(), 0);
    assert_eq!(state.read().await.task_regressions.reservations.len(), 1);
}

#[tokio::test]
async fn task_regressions_last_trial_save_failure_retains_all_actual_usage_for_exactly_once_flush()
{
    let model = Arc::new(NumericModel::default());
    let last_call = Gate::new();
    *model
        .indexed_call_gate
        .lock()
        .expect("indexed model gate lock") = Some((24, last_call.clone()));
    let harness = Harness::new(model).await;
    let request = harness
        .request(numeric_suite(), "last-trial-usage-save-failure")
        .await;
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("comparison admission");
    last_call.entered().await;
    assert_eq!(harness.model.call_count(), 24);
    let failed_save = harness
        .state
        .write()
        .await
        .install_test_control_plane_non_turn_save_gate(true);
    last_call.release();
    tokio::time::timeout(Duration::from_secs(5), failed_save.entered.acquire())
        .await
        .expect("last trial reaches its ordinary evidence save")
        .expect("save gate stays open")
        .forget();
    failed_save.release.add_permits(1);
    let report = harness.retired(&started.id).await;
    assert_eq!(report.status, RunStatus::Inconclusive);
    assert!(report.comparison.is_none());
    assert!(
        !report.usage_complete,
        "an unacknowledged trial save cannot advertise complete retained accounting"
    );
    assert_eq!(harness.model.call_count(), 24);
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("retirement storage")
        .expect("retirement retries ordinary persistence");
    let row = saved
        .task_regressions
        .runs
        .iter()
        .find(|run| run.id == started.id)
        .expect("saved inconclusive report");
    assert_eq!(row.status, RunStatus::Inconclusive);
    assert!(row.retired_at_ms.is_some());
    let ids = usage_ids(&saved.pending_usage);
    assert_eq!(
        saved.pending_usage.len(),
        24,
        "a failed final trial save must not discard actual provider usage"
    );
    assert_eq!(
        ids.len(),
        24,
        "each actual model call owns one immutable usage ID"
    );
    assert_eq!(row.usage_rows.len(), 24);
    assert_eq!(ids, row.usage_rows.iter().cloned().collect());
    assert!(saved
        .pending_usage
        .iter()
        .all(|usage| usage.total_tokens == NumericModel::usage().total_tokens));
    assert_usage_flushes_once(&harness, &ids).await;
    let duplicate = harness
        .service
        .start(request)
        .await
        .expect("failed request remains retained after usage flush");
    assert_eq!(duplicate.id, started.id);
    assert_eq!(duplicate.status, RunStatus::Inconclusive);
    assert_eq!(harness.model.call_count(), 24);
}

#[tokio::test]
async fn task_regressions_dispatch_retirement_save_failure_still_retains_incurred_raw_usage() {
    let model = Arc::new(NumericModel::default());
    let retired_model = Gate::new();
    let adapter = Arc::new(ModelRetirementGate {
        inner: model.clone(),
        call_index: 3,
        gate: retired_model.clone(),
    });
    let harness = Harness::with_adapter(model, adapter).await;
    let request = harness
        .request(numeric_suite(), "dispatch-retirement-save-failure")
        .await;
    let started = harness
        .service
        .start(request.clone())
        .await
        .expect("comparison admission");
    retired_model.entered().await;
    assert_eq!(harness.model.call_count(), 3);
    let child_id = format!("{}:trial-0-0-Baseline", started.id);
    let scope_id = {
        let state = harness.state.read().await;
        let scope_id = state
            .execution_budgets
            .run_scopes
            .get(&child_id)
            .expect("first trial retains its parent scope");
        let scope = &state.execution_budgets.scopes[scope_id];
        let model_calls = scope
            .reservations
            .values()
            .filter(|reservation| {
                reservation.request.run_id == child_id
                    && reservation.request.operation
                        == anima_core::execution_limits::ExecutionOperation::ModelAttempt
            })
            .collect::<Vec<_>>();
        assert_eq!(model_calls.len(), 3);
        for call in model_calls {
            let settlement = call
                .settlement
                .as_ref()
                .expect("model accounting finishes before the injected host fault");
            assert!(settlement.physical_retired);
            assert_eq!(
                settlement.outcome,
                anima_core::execution_limits::SettlementOutcome::Known(NumericModel::usage())
            );
        }
        let dispatch = scope
            .reservations
            .values()
            .find(|reservation| {
                reservation.request.run_id == child_id
                    && reservation.request.logical_call_id == "retained-child-dispatch"
            })
            .expect("first trial has its orchestration reservation");
        assert!(
            dispatch.settlement.is_none(),
            "only the subsequent host dispatch settlement remains unfinished"
        );
        assert!(state.task_regressions.runs[0].usage_rows.is_empty());
        scope_id.clone()
    };
    let failed_settlement = harness
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    retired_model.release();
    tokio::time::timeout(Duration::from_secs(5), failed_settlement.entered.acquire())
        .await
        .expect("retired child dispatch reaches settlement storage")
        .expect("settlement gate stays open")
        .forget();
    assert_eq!(harness.model.call_count(), 3);
    assert_eq!(
        harness
            .state
            .read()
            .await
            .task_regression_slots
            .available_permits(),
        0
    );
    failed_settlement.release.add_permits(1);
    let report = harness.retired(&started.id).await;
    assert_eq!(report.status, RunStatus::Inconclusive);
    assert!(report.comparison.is_none());
    assert!(!report.usage_complete);
    assert_eq!(
        harness.model.call_count(),
        3,
        "settlement failure cannot dispatch the next trial or replay a model call"
    );
    let saved = load_control_plane_snapshot(&harness.store)
        .await
        .expect("post-failure retirement storage")
        .expect("saved failed comparison");
    let row = saved
        .task_regressions
        .runs
        .iter()
        .find(|run| run.id == started.id)
        .expect("failed report remains durable");
    assert_eq!(row.status, RunStatus::Inconclusive);
    let dispatch = saved.execution_budgets.scopes[&scope_id]
        .reservations
        .values()
        .find(|reservation| {
            reservation.request.run_id == child_id
                && reservation.request.logical_call_id == "retained-child-dispatch"
        })
        .expect("failed settlement keeps its dispatch debit");
    assert!(
        dispatch
            .settlement
            .as_ref()
            .expect("conservative failed-settlement evidence is retained")
            .physical_retired
    );
    assert_eq!(
        saved.pending_usage.len(),
        3,
        "raw provider counters survive a failure before save_trial"
    );
    assert_eq!(row.usage_rows.len(), 3);
    let ids = usage_ids(&saved.pending_usage);
    assert_eq!(ids.len(), 3);
    assert_eq!(ids, row.usage_rows.iter().cloned().collect());
    assert!(saved
        .pending_usage
        .iter()
        .all(|usage| usage.total_tokens == NumericModel::usage().total_tokens));
    assert_usage_flushes_once(&harness, &ids).await;
    let duplicate = harness
        .service
        .start(request)
        .await
        .expect("failed admission still joins its saved report");
    assert_eq!(duplicate.id, started.id);
    assert_eq!(duplicate.status, RunStatus::Inconclusive);
    assert_eq!(harness.model.call_count(), 3);
}
