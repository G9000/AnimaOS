//! Real native search -> coordinator -> owner HTTP/session/live projection.
use super::test_support::{
    add_chat, chat_request, companion_config, decision, events_until, patient, pending_approvals,
};
use super::*;
use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
use crate::tools::web_search::tests::{labelled, HttpFixture};
use crate::tools::web_search::{sources_from_envelope, SearchExecutor, ENDPOINT};
use anima_core::{
    AgentConfigUpdate, MessageRole, ModelAdapter, ModelGenerateResponse, ModelStopReason,
    ModelStreamFrame, ModelStreamSink,
};
use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tower::ServiceExt;

struct EvidenceModel {
    query: String,
    controlled: bool,
    hold: bool,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
    observed: StdMutex<Option<(String, Value)>>,
    consumption_error: StdMutex<Option<String>>,
    stall_first: AtomicBool,
    calls: AtomicUsize,
}
impl EvidenceModel {
    fn new(query: &str, controlled: bool, hold: bool) -> Arc<Self> {
        Arc::new(Self {
            query: query.into(),
            controlled,
            hold,
            entered: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
            observed: StdMutex::new(None),
            consumption_error: StdMutex::new(None),
            stall_first: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
        })
    }

    fn consume(&self, request: &anima_core::ModelGenerateRequest) -> Result<String, String> {
        let tool = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::Tool)
            .ok_or("Actual tool result missing")?;
        let parsed = serde_json::from_str::<Value>(&tool.content.text)
            .unwrap_or(Value::String(tool.content.text.clone()));
        let status = tool
            .content
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("taskResult"))
            .and_then(|result| match result {
                DataValue::Object(result) => result.get("status"),
                _ => None,
            })
            .and_then(|status| match status {
                DataValue::String(status) => Some(status.as_str()),
                _ => None,
            })
            .unwrap_or("unknown");
        *self.observed.lock().unwrap() = Some((status.to_string(), parsed.clone()));
        if status == "error" {
            return Ok(format!("Search unavailable: {}", tool.content.text));
        }
        if status != "success" {
            return Err("Tool result status missing or invalid".into());
        }
        let sources =
            sources_from_envelope(&tool.content.text).ok_or("Actual source envelope invalid")?;
        if sources.is_empty() {
            return Err("No actual result URLs to cite".into());
        }
        if self.controlled
            && !parsed["results"][0]["excerpt"]
                .as_str()
                .unwrap_or("")
                .contains("observed fixture evidence")
        {
            return Err("Controlled source evidence changed".into());
        }
        let mut answer = String::new();
        for (index, source) in sources.iter().enumerate() {
            let excerpt = parsed["results"][index]["excerpt"]
                .as_str()
                .ok_or("Actual excerpt missing")?;
            if excerpt.is_empty() {
                return Err("Actual excerpt empty".into());
            }
            let brief = excerpt.chars().take(100).collect::<String>();
            answer.push_str(&format!(
                "{}: {} [Returned source {}](<{}>)\n\n",
                source.title,
                brief,
                index + 1,
                source.url
            ));
        }
        Ok(answer)
    }
}
#[async_trait]
impl ModelAdapter for EvidenceModel {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
        true
    }
    fn provider(&self) -> &str {
        "m83-scripted-zero-paid-generation"
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &anima_core::ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        Err("No secondary model generation in M83 fixture".into())
    }
    async fn stream(
        &self,
        _: &AgentConfig,
        request: &anima_core::ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let turn = self.calls.fetch_add(1, Ordering::SeqCst);
        let (text, tool_calls) = if turn == 0 {
            if self.stall_first.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            (
                String::new(),
                Some(vec![anima_core::ToolCall {
                    id: "m83-search-call".into(),
                    name: "web_search".into(),
                    args: BTreeMap::from([
                        ("query".into(), DataValue::String(self.query.clone())),
                        ("num_results".into(), DataValue::Number(3.0)),
                    ]),
                }]),
            )
        } else {
            if turn != 1 {
                return Err("Unexpected additional model request".into());
            }
            let consumed = self.consume(request);
            *self.consumption_error.lock().unwrap() = consumed.as_ref().err().cloned();
            // Every actual consumption outcome wakes the owner before an
            // error can finish the run, including valid empty search results.
            self.entered.add_permits(1);
            if self.hold {
                self.release
                    .acquire()
                    .await
                    .map_err(|_| "Evidence barrier closed")?
                    .forget();
            }
            (consumed?, None)
        };
        let stop_reason = if tool_calls.is_some() {
            ModelStopReason::ToolCall
        } else {
            ModelStopReason::End
        };
        let response = ModelGenerateResponse {
            content: Content {
                text,
                ..Default::default()
            },
            tool_calls,
            usage: Default::default(),
            stop_reason,
        };
        sink.complete_usage(response.usage.clone()).await?;
        sink.emit(ModelStreamFrame::Final(response)).await
    }
}

async fn setup(
    model: Arc<EvidenceModel>,
    executor: Option<Arc<SearchExecutor>>,
    grant: bool,
    policy: PolicyAction,
) -> (tempfile::TempDir, AgentRunCoordinator, String) {
    let workspace = tempfile::tempdir().unwrap();
    let mut daemon = DaemonState::with_model_adapter(model);
    daemon.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: workspace.path().into(),
        company_name: "Isolated M83".into(),
        mission: String::new(),
        values: vec![],
    });
    let mut config = companion_config("Isolated search fixture");
    config.tools = Some(if grant {
        daemon
            .tool_registry
            .resolve_descriptors(["web_search"])
            .unwrap()
    } else {
        vec![]
    });
    config.settings.as_mut().unwrap().max_retries = Some(0);
    config.settings.as_mut().unwrap().timeout_ms = Some(45_000);
    let agent = daemon.create_agent(config).unwrap().state.id;
    daemon.approvals.set_policy(
        &agent,
        ApprovalPolicy::default().with(RiskClass::Network, policy),
    );
    let mut coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(daemon)),
        Arc::new(Semaphore::new(2)),
    )
    .with_approval_timeouts(patient());
    coordinator.web_search = executor;
    add_chat(&coordinator, &agent, "chat:m83").await;
    (workspace, coordinator, agent)
}

async fn owner_messages(coordinator: &AgentRunCoordinator, agent: &str) -> Value {
    owner_messages_result(coordinator, agent)
        .await
        .expect("owner session messages")
}

async fn owner_messages_result(
    coordinator: &AgentRunCoordinator,
    agent: &str,
) -> Result<Value, String> {
    let app = crate::routes::router(
        coordinator.state.clone(),
        crate::app::DaemonConfig::default(),
    );
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.oneshot(
            Request::builder()
                .uri(format!("/api/agents/{agent}/sessions/chat%3Am83/messages"))
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .map_err(|_| "Owner session HTTP projection deadline exceeded")?
    .map_err(|_| "Owner session HTTP projection failed")?;
    if response.status() != axum::http::StatusCode::OK {
        return Err(format!(
            "Owner session HTTP projection returned {}",
            response.status()
        ));
    }
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .map_err(|_| "Owner session response exceeded its bound")?;
    serde_json::from_slice(&bytes).map_err(|_| "Owner session response was invalid JSON".into())
}

async fn capture_events(
    subscription: &mut crate::live::LiveSubscription,
    stop_at_start: bool,
) -> (Vec<Value>, Option<String>) {
    let mut events = Vec::new();
    let captured = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let event = match subscription.next().await {
                Some(crate::live::LiveDelivery::Event(event)) => event,
                Some(_) => return Err("Live event capture requires resync".to_string()),
                None => return Err("Live event capture channel closed".to_string()),
            };
            let value = event.to_json(events.len() as u64 + 1);
            let stop = value["type"] == "tool.finished"
                || (stop_at_start && value["type"] == "run.started")
                || matches!(
                    value["type"].as_str(),
                    Some("run.completed" | "run.failed" | "run.cancelled" | "run.interrupted")
                );
            events.push(value);
            if stop {
                return Ok(());
            }
        }
    })
    .await;
    let error = match captured {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("Live event capture deadline exceeded".into()),
    };
    (events, error)
}

async fn workflow(model: Arc<EvidenceModel>, executor: Option<Arc<SearchExecutor>>) -> Value {
    workflow_with_timeout(model, executor, std::time::Duration::from_secs(40)).await
}

async fn workflow_with_timeout(
    model: Arc<EvidenceModel>,
    executor: Option<Arc<SearchExecutor>>,
    consumption_timeout: std::time::Duration,
) -> Value {
    let (_workspace, coordinator, agent) =
        setup(model.clone(), executor.clone(), true, PolicyAction::Allow).await;
    let hub = coordinator.state.read().await.live.clone();
    let mut subscription = hub.subscribe(&agent).unwrap();
    let owner = coordinator.clone();
    let id = agent.clone();
    let mut running = tokio::spawn(async move {
        owner
            .run(chat_request(
                &id,
                "chat:m83",
                "Research using the granted search tool",
            ))
            .await
    });
    let mut early_run = None;
    let readiness_error = tokio::select! { biased;
        entered = model.entered.acquire() => match entered {
            Ok(permit) => { permit.forget(); None },
            Err(_) => Some("Result consumption signal closed".to_string()),
        },
        joined = &mut running => { early_run = Some(joined); Some("Run ended before result consumption was signalled".to_string()) },
        _ = tokio::time::sleep(consumption_timeout) => Some("Result consumption deadline exceeded".to_string()),
    };
    let (events, capture_error) =
        capture_events(&mut subscription, readiness_error.is_some()).await;
    let run_id = events
        .iter()
        .rev()
        .find_map(|event| event["runId"].as_str())
        .map(str::to_owned);
    let snapshot = if let Some(run_id) = &run_id {
        let record = coordinator.state.read().await.runs.get(run_id).cloned();
        record
            .map(|record| {
                crate::live::snapshot_json(
                    &agent,
                    events.len() as u64,
                    &[crate::live::SnapshotRun {
                        record,
                        live: hub.runs().view(run_id),
                    }],
                    &[],
                )
            })
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let mut diagnostics = Vec::new();
    if let Some(error) = &readiness_error {
        diagnostics.push(error.clone());
    }
    if let Some(error) = capture_error {
        diagnostics.push(error);
    }
    // A failed consumption is held at exactly the same checkpoint as success,
    // so it can be captured before the run commits its truthful failure.
    model.release.add_permits(1);
    if readiness_error.is_some() && early_run.is_none() {
        if let Some(run_id) = &run_id {
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                coordinator.stop_run(&agent, run_id),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => diagnostics.push(format!("Run stop: {}", error.message())),
                Err(_) => {
                    diagnostics.push("Run stop deadline exceeded; owned stop continues".into())
                }
            }
        }
    }
    let joined = if let Some(joined) = early_run {
        joined
    } else {
        match tokio::time::timeout(std::time::Duration::from_secs(5), &mut running).await {
            Ok(joined) => joined,
            Err(_) => {
                diagnostics
                    .push("Run did not settle after stop; aborted and joined run task".into());
                running.abort();
                running.await
            }
        }
    };
    let run = match joined {
        Ok(Ok(run)) => json!({"status":run.result.status,"error":run.result.error}),
        Ok(Err(error)) => json!({"status":"error","error":error.message()}),
        Err(error) => json!({"status":"error","error":error.to_string()}),
    };
    let native_joined = if let Some(executor) = &executor {
        tokio::time::timeout(std::time::Duration::from_secs(5), executor.join())
            .await
            .is_ok()
    } else {
        true
    };
    if !native_joined {
        diagnostics.push(
            "Native cleanup deadline exceeded; executor retains owned workers and physical leases"
                .into(),
        );
    }
    let page = match owner_messages_result(&coordinator, &agent).await {
        Ok(page) => page,
        Err(error) => {
            diagnostics.push(error.clone());
            json!({"messages":[],"nextBefore":null,"error":error})
        }
    };
    let observed = model.observed.lock().unwrap().clone();
    let (status, result) = observed
        .clone()
        .unwrap_or_else(|| ("unobserved".into(), Value::Null));
    let urls = if status == "success" {
        result["results"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["url"].as_str())
                    .map(|url| Value::String(url.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };
    let consumption_error = model
        .consumption_error
        .lock()
        .unwrap()
        .clone()
        .or(readiness_error);
    let consumed = observed.is_some() && consumption_error.is_none();
    json!({"schemaVersion":1,"query":model.query,"endpoint":ENDPOINT,
        "search":{"status":status,"urls":urls,"result":result},
        "consumption":{"status":if consumed {"success"}else{"error"},"error":consumption_error},
        "run":run,"sessionMessages":page,"events":events,"snapshot":snapshot,
        "transport":executor.as_ref().map(|executor|executor.evidence()),"modelCalls":model.calls.load(Ordering::SeqCst),
        "diagnostics":diagnostics,"cleanup":{"runTaskJoined":true,"nativeWorkersJoined":native_joined,"paidModelCalls":0}})
}

fn assert_successful_artifact(artifact: &Value) {
    assert_eq!(artifact["search"]["status"], "success");
    assert_eq!(artifact["consumption"]["status"], "success", "{artifact}");
    assert_eq!(artifact["run"]["status"], "success", "{artifact}");
    assert_eq!(artifact["cleanup"]["runTaskJoined"], true);
    assert_eq!(artifact["cleanup"]["nativeWorkersJoined"], true);
    assert_eq!(artifact["transport"]["unknownClose"], 0);
    assert_eq!(artifact["diagnostics"], json!([]));
    let messages = artifact["sessionMessages"]["messages"].as_array().unwrap();
    let tool = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    let sources = tool["metadata"]["webSources"]
        .as_array()
        .expect("actual persisted search sources");
    let urls = &artifact["search"]["urls"];
    assert_eq!(
        json!(sources
            .iter()
            .map(|source| source["url"].clone())
            .collect::<Vec<_>>()),
        *urls
    );
    let finished = artifact["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "tool.finished")
        .unwrap();
    assert_eq!(finished["sources"], tool["metadata"]["webSources"]);
    assert_eq!(
        artifact["snapshot"]["runs"][0]["tools"][0]["sources"],
        finished["sources"]
    );
    let answer = messages
        .iter()
        .rev()
        .find(|message| {
            message["role"] == "assistant" && !message["text"].as_str().unwrap_or("").is_empty()
        })
        .unwrap();
    for url in urls.as_array().unwrap() {
        assert!(answer["text"]
            .as_str()
            .unwrap()
            .contains(url.as_str().unwrap()));
    }
    assert!(tool["metadata"].get("taskResult").is_none());
    assert_eq!(artifact["modelCalls"], 2);
}

#[tokio::test]
async fn m83_controlled_http_native_harness_live_and_owner_session_sources_agree() {
    let fixture = HttpFixture::new(
        true,
        json!({"content":[{"type":"text","text":labelled()}],"isError":false}),
        None,
    )
    .await;
    let artifact = workflow(
        EvidenceModel::new("fixture query", true, true),
        Some(fixture.executor.clone()),
    )
    .await;
    assert_successful_artifact(&artifact);
    assert_eq!(artifact["search"]["status"], "success");
    assert_eq!(
        fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, request)| request["method"] == "tools/call")
            .count(),
        1
    );
    assert_eq!(
        artifact["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["type"] == "tool.finished")
            .unwrap()["truncated"],
        true
    );
    fixture.close().await;
    if let Ok(path) = std::env::var("ANIMA_M83_CONTROLLED_EVIDENCE_PATH") {
        std::fs::write(path, serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    }
}

#[tokio::test]
async fn m83_denied_missing_and_revoked_grants_never_connect_and_owner_approval_dispatches_once() {
    for mode in ["missing", "denied", "revoked", "rejected", "allowed"] {
        let fixture = HttpFixture::new(
            false,
            json!({"content":[{"type":"text","text":labelled()}],"isError":false}),
            None,
        )
        .await;
        let model = EvidenceModel::new("fixture query", true, false);
        let (_workspace, coordinator, agent) = setup(
            model.clone(),
            Some(fixture.executor.clone()),
            mode != "missing",
            if mode == "denied" {
                PolicyAction::Deny
            } else if mode == "missing" {
                PolicyAction::Allow
            } else {
                PolicyAction::Ask
            },
        )
        .await;
        let owner = coordinator.clone();
        let id = agent.clone();
        let running =
            tokio::spawn(async move { owner.run(chat_request(&id, "chat:m83", "search")).await });
        if matches!(mode, "revoked" | "rejected" | "allowed") {
            let pending = pending_approvals(&coordinator, 1).await.remove(0);
            assert!(
                fixture.records.lock().unwrap().is_empty(),
                "no connect before ordinary approval"
            );
            if mode == "revoked" {
                coordinator
                    .state
                    .write()
                    .await
                    .update_agent(
                        &agent,
                        AgentConfigUpdate {
                            tools: Some(vec![]),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            coordinator
                .decide_approval(
                    &pending.id,
                    decision(
                        if mode == "rejected" {
                            ApprovalDecisionKind::Deny
                        } else {
                            ApprovalDecisionKind::AllowOnce
                        },
                        1,
                    ),
                )
                .await
                .unwrap();
        }
        running.await.unwrap().unwrap();
        let count = fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, request)| request["method"] == "tools/call")
            .count();
        assert_eq!(count, usize::from(mode == "allowed"), "{mode}");
        if mode != "allowed" {
            assert!(fixture.records.lock().unwrap().is_empty(), "{mode}");
        }
        let page = owner_messages(&coordinator, &agent).await;
        if mode != "allowed" {
            assert!(page["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["metadata"].get("webSources").is_none()));
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn m83_error_and_altered_actual_results_cannot_fabricate_a_successful_cited_answer() {
    let error = HttpFixture::new(
        false,
        json!({"content":[{"type":"text","text":"quota exceeded"}],"isError":true}),
        None,
    )
    .await;
    let artifact = workflow(
        EvidenceModel::new("fixture query", true, true),
        Some(error.executor.clone()),
    )
    .await;
    assert_eq!(artifact["search"]["status"], "error");
    assert_eq!(artifact["search"]["urls"], json!([]));
    let tool = artifact["sessionMessages"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["role"] == "tool")
        .unwrap();
    assert!(tool["metadata"].get("webSources").is_none());
    let finished = artifact["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "tool.finished")
        .unwrap();
    assert!(finished.get("sources").is_none());
    error.close().await;
    let altered=HttpFixture::new(false,json!({"content":[{"type":"text","text":"Title: Changed result\nURL: https://example.com/altered\nHighlights: evidence was changed"}],"isError":false}),None).await;
    let model = EvidenceModel::new("fixture query", true, false);
    let (_workspace, coordinator, agent) = setup(
        model,
        Some(altered.executor.clone()),
        true,
        PolicyAction::Allow,
    )
    .await;
    let run = coordinator
        .run(chat_request(&agent, "chat:m83", "consume real evidence"))
        .await
        .unwrap();
    assert_eq!(run.result.status, "error");
    let page = owner_messages(&coordinator, &agent).await;
    assert!(!page["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["role"] == "assistant"
            && row["text"]
                .as_str()
                .unwrap_or("")
                .contains("Returned source")));
    altered.close().await;
}

#[tokio::test]
#[ignore = "one bounded anonymous public query; explicit controller opt-in after gates"]
async fn m83_public_web_search_workflow() {
    assert_eq!(
        std::env::var("ANIMA_M83_PUBLIC_SEARCH").as_deref(),
        Ok("1"),
        "explicit public search opt-in required"
    );
    let path =
        std::env::var("ANIMA_M83_EVIDENCE_PATH").expect("private evidence output path required");
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("Public evidence output must be a fresh file before searching");
    let executor = crate::tools::web_search::public_executor();
    let artifact = workflow(
        EvidenceModel::new(
            "Rust programming language official documentation",
            false,
            true,
        ),
        Some(executor),
    )
    .await;
    std::io::Write::write_all(&mut output, &serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    println!(
        "M83 public anonymous search evidence: {path}; status={}",
        artifact["search"]["status"]
    );
    assert_eq!(
        artifact["search"]["status"], "success",
        "actual public failure retained at {path}"
    );
    assert_successful_artifact(&artifact);
}

#[tokio::test]
async fn m83_empty_results_and_excerpts_retain_actual_success_and_failed_consumption_artifacts() {
    for (text, error, expected_sources) in [
        ("No results found.", "No actual result URLs to cite", 0),
        (
            "Title: Actual source\nURL: https://example.com/empty-excerpt\nHighlights:",
            "Actual excerpt empty",
            1,
        ),
    ] {
        let fixture = HttpFixture::new(
            false,
            json!({"content":[{"type":"text","text":text}],"isError":false}),
            None,
        )
        .await;
        let artifact = workflow_with_timeout(
            EvidenceModel::new("fixture query", false, true),
            Some(fixture.executor.clone()),
            std::time::Duration::from_secs(2),
        )
        .await;
        assert_eq!(artifact["search"]["status"], "success");
        assert_eq!(
            artifact["search"]["result"]["results"]
                .as_array()
                .unwrap()
                .len(),
            expected_sources
        );
        assert_eq!(artifact["consumption"]["status"], "error");
        assert_eq!(artifact["consumption"]["error"], error);
        assert_eq!(artifact["run"]["status"], "error");
        assert_eq!(artifact["cleanup"]["runTaskJoined"], true);
        assert_eq!(artifact["cleanup"]["nativeWorkersJoined"], true);
        assert_eq!(artifact["transport"]["ownedWorkers"], 0);
        assert_eq!(artifact["transport"]["logicalSearchCalls"], 1);
        assert_eq!(artifact["diagnostics"], json!([]));
        assert!(!artifact["sessionMessages"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["role"] == "assistant"
                && row["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Returned source")));
        let encoded = serde_json::to_vec(&artifact).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&encoded).unwrap()["search"]["result"],
            artifact["search"]["result"]
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn m83_evidence_timeout_stops_and_joins_run_without_fabricating_a_search_result() {
    let fixture = HttpFixture::new(
        false,
        json!({"content":[{"type":"text","text":labelled()}],"isError":false}),
        None,
    )
    .await;
    let model = EvidenceModel::new("fixture query", true, true);
    model.stall_first.store(true, Ordering::SeqCst);
    let artifact = workflow_with_timeout(
        model,
        Some(fixture.executor.clone()),
        std::time::Duration::from_millis(30),
    )
    .await;
    assert_eq!(artifact["search"]["status"], "unobserved");
    assert_eq!(artifact["search"]["result"], Value::Null);
    assert_eq!(artifact["consumption"]["status"], "error");
    assert_eq!(
        artifact["consumption"]["error"],
        "Result consumption deadline exceeded"
    );
    assert_eq!(artifact["cleanup"]["runTaskJoined"], true);
    assert_eq!(artifact["cleanup"]["nativeWorkersJoined"], true);
    assert_eq!(artifact["transport"]["logicalSearchCalls"], 0);
    assert!(fixture.records.lock().unwrap().is_empty());
    assert!(artifact["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "run.started"));
    fixture.close().await;
}
