//! Explicit local interoperability, with a real native owner and no model calls.
use super::*;
use anima_core::{Content, DataValue, TaskResult, TaskStatus};
use serde_json::Value;
use std::path::Path;

const GRAPH_TOOLS: [&str; 7] = [
    "mother_garden_get",
    "mother_garden_query",
    "mother_garden_apply",
    "mother_garden_propose",
    "mother_garden_get_proposal",
    "mother_garden_get_receipt",
    "mother_garden_changes",
];

struct NativeGraphOwner {
    executable: PathBuf,
    home: PathBuf,
    database: PathBuf,
    endpoint: PathBuf,
    process: Option<tokio::process::Child>,
}

impl NativeGraphOwner {
    async fn cli(&self, args: &[&str]) -> Value {
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .args(args)
            .current_dir(&self.home)
            .env("MOTHER_HOME", &self.home)
            .env("MOTHER_MODEL_DIR", self.home.join("absent-model"))
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let output = timeout(Duration::from_secs(15), command.output())
            .await
            .expect("synthetic owner CLI deadline")
            .expect("synthetic owner CLI process");
        assert!(
            output.status.success(),
            "synthetic owner {} failed",
            args[0]
        );
        serde_json::from_slice(&output.stdout).expect("synthetic owner CLI JSON")
    }

    async fn start(&mut self) {
        assert!(self.process.is_none());
        let previous = std::fs::read(&self.endpoint).ok();
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .args([
                "garden",
                "service",
                "--db",
                path(&self.database),
                "--endpoint",
                path(&self.endpoint),
            ])
            .current_dir(&self.home)
            .env("MOTHER_HOME", &self.home)
            .env("MOTHER_MODEL_DIR", self.home.join("absent-model"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().expect("synthetic native owner process");
        timeout(Duration::from_secs(10), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "synthetic native owner exited"
                );
                if let Ok(bytes) = std::fs::read(&self.endpoint) {
                    if Some(&bytes) != previous.as_ref()
                        && serde_json::from_slice::<Value>(&bytes).is_ok()
                    {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("synthetic native owner readiness");
        self.process = Some(child);
    }

    async fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            child.kill().await.expect("stop synthetic native owner");
            child.wait().await.expect("join synthetic native owner");
        }
    }
}

fn path(value: &Path) -> &str {
    value.to_str().expect("synthetic fixture UTF-8 path")
}

// Anima's portable DataValue represents JSON numbers as f64. Preserve the entire
// owner receipt while comparing it in that same representation, with no lossy integers.
fn portable_fixture_json(value: Value) -> Value {
    match value {
        Value::Number(number) => {
            const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
            if let Some(integer) = number.as_u64() {
                assert!(
                    integer <= MAX_SAFE_INTEGER,
                    "fixture integer must be exactly portable"
                );
            } else if let Some(integer) = number.as_i64() {
                assert!(
                    integer.unsigned_abs() <= MAX_SAFE_INTEGER,
                    "fixture integer must be exactly portable"
                );
            }
            json!(number.as_f64().expect("fixture finite JSON number"))
        }
        Value::Array(values) => {
            Value::Array(values.into_iter().map(portable_fixture_json).collect())
        }
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, portable_fixture_json(value)))
                .collect(),
        ),
        other => other,
    }
}

async fn graph_stamp(
    f: &Fixture,
    manager: &McpManager,
    row: &McpConnectionView,
    remote: &str,
    run: &RunLink,
) -> Result<McpDispatchStamp, McpServiceError> {
    let tool = row
        .record
        .reviewed_tools
        .iter()
        .find(|tool| tool.descriptor.remote_name == remote)
        .unwrap();
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    manager.capture_dispatch(
        &agent,
        &ToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            name: tool.alias.clone(),
            args: Default::default(),
        },
        run,
    )
}

async fn graph_call(
    f: &Fixture,
    manager: &McpManager,
    row: &McpConnectionView,
    run: &RunLink,
    remote: &str,
    args: Value,
) -> Value {
    let result = graph_execute(f, manager, row, run, remote, args).await;
    assert_eq!(
        result.status,
        TaskStatus::Success,
        "real native graph executor {remote}: {:?}",
        result.error
    );
    let envelope: Value =
        serde_json::from_str(&result.data.expect("successful graph content").text).unwrap();
    assert_eq!(
        envelope["isError"], false,
        "real native graph {remote}: {envelope}"
    );
    assert_eq!(envelope["shortened"], false);
    envelope["structuredData"].clone()
}

fn fixture_data_value(value: &Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(value) => DataValue::Bool(*value),
        Value::Number(value) => DataValue::Number(value.as_f64().expect("fixture finite number")),
        Value::String(value) => DataValue::String(value.clone()),
        Value::Array(values) => DataValue::Array(values.iter().map(fixture_data_value).collect()),
        Value::Object(values) => DataValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), fixture_data_value(value)))
                .collect(),
        ),
    }
}

async fn graph_execute(
    f: &Fixture,
    manager: &McpManager,
    row: &McpConnectionView,
    run: &RunLink,
    remote: &str,
    args: Value,
) -> TaskResult<Content> {
    let tool = row
        .record
        .reviewed_tools
        .iter()
        .find(|tool| tool.descriptor.remote_name == remote)
        .unwrap();
    let DataValue::Object(args) = fixture_data_value(&args) else {
        panic!("graph fixture arguments must be an object");
    };
    let (context, agent) = {
        let state = f.state.read().await;
        let context = crate::tools::ToolExecutionContext::new(
            state.memory.clone(),
            state.memory_embeddings.clone(),
            None,
            crate::tools::ToolRegistry::new(),
            state.process_manager.clone(),
            None,
            None,
        )
        .with_run_link(Some(run.clone()))
        .with_cancel(Some(CancelSignal::new()))
        .with_mcp(Some(manager.clone()));
        (context, state.get_agent(&f.agent).unwrap().state)
    };
    let message = crate::agent_runs::test_support::tool_input(&f.agent, &run.session_id);
    context
        .execute_tool(
            agent,
            message,
            ToolCall {
                id: uuid::Uuid::new_v4().to_string(),
                name: tool.alias.clone(),
                args,
            },
        )
        .await
}

#[tokio::test]
#[ignore = "Requires ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER pointing to the reviewed native Mother executable"]
async fn native_mother_full_graph_reviewed_transport_invokes_all_seven_tools() {
    let executable = PathBuf::from(
        std::env::var_os("ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER")
            .expect("select the native Mother executable explicitly"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let f = Fixture::new().await;
    let home = f.folder.join("native-mother");
    std::fs::create_dir(&home).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut owner = NativeGraphOwner {
        executable,
        database: home.join("garden.redb"),
        endpoint: home.join("service.json"),
        home,
        process: None,
    };
    let identity = owner
        .cli(&["garden", "init", "--db", path(&owner.database)])
        .await;
    let soul = owner.home.join("SOUL.md");
    std::fs::write(&soul, "Synthetic full graph interoperability persona").unwrap();
    owner
        .cli(&[
            "garden",
            "compat-init",
            "--db",
            path(&owner.database),
            "--soul",
            path(&soul),
        ])
        .await;
    let direct = owner.home.join("anima.direct.private.json");
    owner
        .cli(&[
            "garden",
            "grant-v1",
            "anima-graph-interop",
            "--db",
            path(&owner.database),
            "--out",
            path(&direct),
            "--access",
            "companion",
            "--subject",
            &f.agent,
            "--all",
            "--scopes",
            "read,context:propose,garden:read,garden:write,garden:propose",
        ])
        .await;
    owner.start().await;
    let config = owner.home.join("anima.service.private.json");
    owner
        .cli(&[
            "garden",
            "service-config",
            "--config",
            path(&direct),
            "--endpoint",
            path(&owner.endpoint),
            "--out",
            path(&config),
        ])
        .await;
    let publication = f.state.read().await.mcp_publication.clone();
    let manager = McpManager::new(
        Arc::downgrade(&f.state),
        Arc::new(Mutex::new(())),
        publication,
        Arc::new(DefaultMcpTransportFactory::default()),
    );
    f.state.write().await.mcp_manager = Some(manager.clone());
    let row = manager
        .create_stdio(
            &f.agent,
            "Synthetic full graph interoperability".into(),
            McpStdioSpec {
                executable: path(&owner.executable).into(),
                args: vec!["mcp".into(), "--config".into(), path(&config).into()],
            },
        )
        .await
        .unwrap();
    let row = manager
        .update(
            &f.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let mut row = manager
        .connect(&f.agent, &row.record.id, row.record.version)
        .await
        .expect("real full native catalog admission, including nested graph references");
    assert_eq!(row.candidates.len(), 19);
    let original_candidates = row.candidates.clone();
    assert!(row.record.reviewed_tools.is_empty());
    let stale_discovery = row.discovery_id.clone().unwrap();
    let digest = row.candidates[0].descriptor_digest.clone();
    assert!(
        manager
            .review(
                &f.agent,
                &row.record.id,
                ReviewMcpTool {
                    expected_version: row.record.version,
                    discovery_id: stale_discovery.clone(),
                    descriptor_digest: "sha256:wrong".into(),
                }
            )
            .await
            .is_err(),
        "a different digest cannot be reviewed"
    );
    row = manager
        .discover(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    assert_eq!(row.candidates, original_candidates);
    assert!(
        manager
            .review(
                &f.agent,
                &row.record.id,
                ReviewMcpTool {
                    expected_version: row.record.version,
                    discovery_id: stale_discovery,
                    descriptor_digest: digest,
                }
            )
            .await
            .is_err(),
        "a retired discovery cannot authorize review"
    );
    for remote in GRAPH_TOOLS {
        let descriptor_digest = row
            .candidates
            .iter()
            .find(|tool| tool.remote_name == remote)
            .unwrap()
            .descriptor_digest
            .clone();
        row = manager
            .review(
                &f.agent,
                &row.record.id,
                ReviewMcpTool {
                    expected_version: row.record.version,
                    discovery_id: row.discovery_id.clone().unwrap(),
                    descriptor_digest,
                },
            )
            .await
            .unwrap();
    }
    let run = {
        let mut state = f.state.write().await;
        let sid = "chat:real-native-graph";
        state.sessions.insert(SessionRecord::new(
            &f.agent,
            sid,
            SessionKind::Chat,
            SessionOrigin::Web,
            "Synthetic graph invocation".into(),
            TitleSource::Owner,
            1,
        ));
        let record = RunRecord::running(
            RunStart {
                agent_id: f.agent.clone(),
                session_id: sid.into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "Synthetic local graph calls".into(),
                model: "synthetic-no-model-calls".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        let run_id = record.id.clone();
        state.runs.insert(record);
        RunLink {
            agent_id: f.agent.clone(),
            session_id: sid.into(),
            run_id,
        }
    };
    assert!(
        graph_stamp(&f, &manager, &row, GRAPH_TOOLS[0], &run)
            .await
            .is_err(),
        "review must never grant tool execution"
    );
    for tool in row.record.reviewed_tools.clone() {
        row = manager
            .set_grant(
                &f.agent,
                &row.record.id,
                &tool.alias,
                McpGrantInput {
                    expected_version: row.record.version,
                    descriptor_digest: tool.descriptor.descriptor_digest,
                    granted: true,
                },
            )
            .await
            .unwrap()
            .connection;
    }
    let envelope = json!({"version":2,"workspaceId":identity["workspace"]["id"],
        "generation":identity["workspace"]["generation"],"requiredExtensions":{}});
    let mut batch = envelope.clone();
    batch["operationId"] = json!(uuid::Uuid::new_v4().to_string());
    batch["items"] = json!([
        {"itemKey":"person","mutation":"create","expectedRevision":0,"draft":{
            "kind":"entity","entityType":"synthetic:person","displayName":"Synthetic graph person","aliases":[],
            "extensions":{"synthetic:opaque":{"schemaVersion":1,"payload":{"$ref":"ordinary payload","nested":[1,null,true]}}}}},
        {"itemKey":"claim","mutation":"create","expectedRevision":0,"draft":{
            "kind":"assertion","subject":{"refType":"item","kind":"entity","itemKey":"person"},
            "predicate":"synthetic:likes","object":{"type":"value","value":{"type":"string","value":"tea"}},
            "validTime":{"validFrom":null,"validUntil":null,"sourceTimezone":null,"precision":"unknown"},
            "observedTime":{"at":null,"sourceTimezone":null,"precision":"unknown"},"evidence":[],
            "confidence":{"score":0.8,"basis":"owner_statement"},"extensions":{}}}
    ]);
    let applied = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_apply",
        batch.clone(),
    )
    .await;
    assert_eq!(applied["receipt"]["status"], "applied");
    assert_eq!(applied["receipt"]["results"].as_array().unwrap().len(), 2);
    assert_eq!(
        graph_call(
            &f,
            &manager,
            &row,
            &run,
            "mother_garden_apply",
            batch.clone()
        )
        .await,
        applied,
        "identical operation retry must reconcile without another commit"
    );
    let reference = applied["receipt"]["results"][0]["reference"].clone();
    let mut get = envelope.clone();
    get["reference"] = reference;
    get["revision"] = Value::Null;
    get["knownAt"] = Value::Null;
    let fetched = graph_call(&f, &manager, &row, &run, "mother_garden_get", get.clone()).await;
    assert_eq!(
        fetched["record"]["metadata"]["revision"].as_f64(),
        Some(1.0)
    );
    let mut query = envelope.clone();
    for (key, value) in json!({"ids":[],"kinds":[],"entityType":null,"predicate":null,"property":null,
        "validAt":null,"knownAt":null,"text":null,"vector":null,"traversal":null,"limit":1,"cursor":null}).as_object().unwrap() {
        query[key] = value.clone();
    }
    let first = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_query",
        query.clone(),
    )
    .await;
    assert_eq!(first["hits"].as_array().unwrap().len(), 1);
    assert!(first["page"]["nextCursor"].is_string());
    query["cursor"] = first["page"]["nextCursor"].clone();
    let second = graph_call(&f, &manager, &row, &run, "mother_garden_query", query).await;
    assert_eq!(second["hits"].as_array().unwrap().len(), 1);
    assert_ne!(
        first["hits"][0]["record"]["metadata"]["id"],
        second["hits"][0]["record"]["metadata"]["id"]
    );
    let mut changes = envelope.clone();
    changes["limit"] = json!(10);
    changes["cursor"] = Value::Null;
    let committed = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_changes",
        changes.clone(),
    )
    .await;
    assert_eq!(committed["changes"].as_array().unwrap().len(), 2);
    changes["cursor"] = committed["page"]["nextCursor"].clone();
    let mut proposal = batch.clone();
    proposal["operationId"] = json!(uuid::Uuid::new_v4().to_string());
    proposal["items"] = json!([{"itemKey":"pending","mutation":"create","expectedRevision":0,"draft":{
        "kind":"entity","entityType":"synthetic:pending","displayName":"Synthetic unapproved graph item","aliases":[],"extensions":{}}}]);
    let pending = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_propose",
        proposal.clone(),
    )
    .await;
    assert_eq!(pending["status"], "pending");
    let mut outcome = envelope.clone();
    outcome["operationId"] = proposal["operationId"].clone();
    outcome["operationGeneration"] = envelope["generation"].clone();
    let review = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_get_proposal",
        outcome.clone(),
    )
    .await;
    assert_eq!(review["receipt"]["status"], "pending");
    let receipt = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_get_receipt",
        outcome.clone(),
    )
    .await;
    assert_eq!(receipt["receipt"]["status"], "pending");
    assert_eq!(
        graph_call(&f, &manager, &row, &run, "mother_garden_changes", changes).await["changes"],
        json!([]),
        "pending proposals never become canonical changes"
    );
    let mut malformed = get.clone();
    malformed["reference"]["id"] = json!("not-a-uuid");
    let error = graph_execute(&f, &manager, &row, &run, "mother_garden_get", malformed).await;
    assert_eq!(error.status, TaskStatus::Error);
    assert!(
        error.data.is_none(),
        "malformed UUID is rejected by host schema"
    );
    let mut wrong_workspace = get.clone();
    wrong_workspace["workspaceId"] = json!(uuid::Uuid::new_v4().to_string());
    let error = graph_execute(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_get",
        wrong_workspace,
    )
    .await;
    assert_eq!(error.status, TaskStatus::Error, "{:?}", error.error);
    let error: Value = serde_json::from_str(
        &error
            .data
            .expect("schema-valid remote authority failure retains evidence")
            .text,
    )
    .unwrap();
    assert_eq!(error["isError"], true);
    assert_eq!(
        error["structuredData"]["error"]["code"],
        "WORKSPACE_MISMATCH"
    );
    let reviewed = row.record.reviewed_tools.clone();
    row = manager
        .disconnect(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    owner.stop().await;
    let review_request = owner.home.join("synthetic-graph-review.json");
    let owner_operation_id = uuid::Uuid::new_v4().to_string();
    std::fs::write(
        &review_request,
        serde_json::to_vec(&json!({
            "version":2,"workspaceId":envelope["workspaceId"],"generation":envelope["generation"],
            "operationId":owner_operation_id,"proposalId":pending["proposalId"],
            "expectedRevision":1,"action":"approve"
        }))
        .unwrap(),
    )
    .unwrap();
    let approved = owner
        .cli(&[
            "garden",
            "review",
            "--graph",
            "--db",
            path(&owner.database),
            "--request",
            path(&review_request),
        ])
        .await;
    owner.start().await;
    row = manager
        .connect(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    assert_eq!(
        row.candidates, original_candidates,
        "owner restart preserves descriptor digests"
    );
    assert_eq!(row.record.reviewed_tools, reviewed);
    assert_eq!(
        graph_call(
            &f,
            &manager,
            &row,
            &run,
            "mother_garden_get_receipt",
            outcome.clone()
        )
        .await["receipt"],
        receipt["receipt"],
        "admission receipt remains pending after owner approval"
    );
    let approved_outcome = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_garden_get_proposal",
        outcome,
    )
    .await;
    assert_eq!(approved_outcome["receipt"]["status"], "approved");
    assert_eq!(approved["receipt"]["operationId"], owner_operation_id);
    assert_ne!(approved["receipt"]["operationId"], proposal["operationId"]);
    let mut expected = portable_fixture_json(approved["receipt"].clone());
    // Proposer outcome lookup keeps its original operation identity; the owner
    // review receipt has the separate approval operation. All other fields match.
    expected["operationId"] = proposal["operationId"].clone();
    assert_eq!(approved_outcome["receipt"], expected);
    let stale = graph_stamp(&f, &manager, &row, "mother_garden_apply", &run)
        .await
        .unwrap();
    let apply = row
        .record
        .reviewed_tools
        .iter()
        .find(|tool| tool.descriptor.remote_name == "mother_garden_apply")
        .unwrap()
        .clone();
    row = manager
        .set_grant(
            &f.agent,
            &row.record.id,
            &apply.alias,
            McpGrantInput {
                expected_version: row.record.version,
                descriptor_digest: apply.descriptor.descriptor_digest,
                granted: false,
            },
        )
        .await
        .unwrap()
        .connection;
    assert!(
        manager
            .invoke(stale, batch, CancelSignal::new())
            .await
            .is_err(),
        "revoked captured calls fail closed"
    );
    assert!(graph_stamp(&f, &manager, &row, "mother_garden_apply", &run)
        .await
        .is_err());
    assert_eq!(
        graph_call(&f, &manager, &row, &run, "mother_garden_get", get).await["record"],
        fetched["record"]
    );
    manager.shutdown().await;
    owner.stop().await;
    assert!(!owner.home.join("context.sqlite").exists());
    assert!(
        !f.folder.join("native-memory.json").exists(),
        "graph calls never initialize Anima memory persistence"
    );
}
