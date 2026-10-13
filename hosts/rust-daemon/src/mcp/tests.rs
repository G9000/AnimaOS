use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};

use anima_core::{prepare_external_tool, CancelSignal, ExternalToolResult, ToolCall};
use futures::future::BoxFuture;
use serde_json::json;
use tokio::sync::{Mutex, Notify, RwLock, Semaphore};
use tokio::time::{timeout, Duration};

use super::transport::*;
use super::*;
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::runs::{RunLink, RunRecord, RunSource, RunStart};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;

fn candidate() -> anima_core::ExternalToolDescriptor {
    prepare_external_tool("echo_record","Read a synthetic record",json!({"type":"object",
        "properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}),None).unwrap()
}

#[derive(Default)]
struct FakeFactory {
    connects: AtomicUsize,
    hold_calls: AtomicBool,
    sessions: StdMutex<Vec<Arc<FakeSession>>>,
}
struct FakeSession {
    changed_schema: AtomicBool,
    discoveries: AtomicUsize,
    calls: AtomicUsize,
    hold_calls: AtomicBool,
    entered: Semaphore,
    hold_close: AtomicBool,
    fail_close: AtomicBool,
    close_release: Arc<Semaphore>,
    closed: Arc<AtomicBool>,
    closed_notify: Arc<Notify>,
}
impl FakeSession {
    fn new(hold: bool) -> Self {
        Self {
            changed_schema: AtomicBool::new(false),
            discoveries: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            hold_calls: AtomicBool::new(hold),
            entered: Semaphore::new(0),
            hold_close: AtomicBool::new(false),
            fail_close: AtomicBool::new(false),
            close_release: Arc::new(Semaphore::new(0)),
            closed: Arc::new(AtomicBool::new(false)),
            closed_notify: Arc::new(Notify::new()),
        }
    }
}
impl McpTransportFactory for FakeFactory {
    fn connect(
        &self,
        _endpoint: ValidatedMcpEndpoint,
        _control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        self.connects.fetch_add(1, Ordering::SeqCst);
        let session = Arc::new(FakeSession::new(self.hold_calls.load(Ordering::SeqCst)));
        self.sessions.lock().unwrap().push(session.clone());
        Box::pin(async move { Ok(session as Arc<dyn McpTransportSession>) })
    }
}
impl McpTransportSession for FakeSession {
    fn discover(
        &self,
        _control: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>> {
        self.discoveries.fetch_add(1, Ordering::SeqCst);
        let changed = self.changed_schema.load(Ordering::SeqCst);
        Box::pin(async move {
            let mut descriptor = candidate();
            if changed {
                descriptor = prepare_external_tool(
                    &descriptor.remote_name,
                    "Changed synthetic descriptor",
                    descriptor.input_schema,
                    descriptor.output_schema,
                )
                .unwrap();
            }
            Ok(McpDiscovery {
                protocol_version: "2025-11-25".into(),
                tools: vec![descriptor],
            })
        })
    }
    fn call_once(
        &self,
        _call: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        let hold = self.hold_calls.load(Ordering::SeqCst);
        Box::pin(async move {
            if hold {
                control.cancel.cancelled().await;
                return Err(McpTransportError::OutcomeUnknown);
            }
            Ok(ExternalToolResult {
                text_blocks: vec!["synthetic violet record".into()],
                structured_data: None,
                is_error: false,
                omitted_blocks: 0,
                shortened: false,
            })
        })
    }
    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>> {
        if self.fail_close.load(Ordering::SeqCst) {
            return Box::pin(async { Err(McpTransportError::Unavailable) });
        }
        // Fake owned close state outlives each returned future, as the real transport does.
        if !self.hold_close.load(Ordering::SeqCst) {
            self.closed.store(true, Ordering::SeqCst);
            self.closed_notify.notify_waiters();
            return Box::pin(async { Ok(PhysicalClose::for_test()) });
        }
        let (release, closed, notify) = (
            self.close_release.clone(),
            self.closed.clone(),
            self.closed_notify.clone(),
        );
        Box::pin(async move {
            loop {
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if closed.load(Ordering::SeqCst) {
                    return Ok(PhysicalClose::for_test());
                }
                tokio::select! {_=notified=>{},permit=release.acquire()=>{
                permit.map_err(|_|McpTransportError::Unavailable)?.forget();closed.store(true,Ordering::SeqCst);
                notify.notify_waiters();return Ok(PhysicalClose::for_test())}}
            }
        })
    }
}

struct Fixture {
    state: Arc<RwLock<DaemonState>>,
    manager: McpManager,
    agent: String,
    factory: Arc<FakeFactory>,
    folder: PathBuf,
}
impl Fixture {
    async fn new() -> Self {
        let folder =
            std::env::temp_dir().join(format!("anima-mcp-manager-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut daemon = DaemonState::new();
        let agent = daemon
            .create_agent(crate::sessions::test_support::agent_config("MCP owner"))
            .unwrap()
            .state
            .id;
        daemon.control_plane_store =
            Some(ControlPlaneStoreConfig::Json(folder.join("control.json")));
        let publication = daemon.mcp_publication.clone();
        let state = Arc::new(RwLock::new(daemon));
        let factory = Arc::new(FakeFactory::default());
        let manager = McpManager::new(
            Arc::downgrade(&state),
            Arc::new(Mutex::new(())),
            publication,
            factory.clone(),
        );
        state.write().await.mcp_manager = Some(manager.clone());
        Self {
            state,
            manager,
            agent,
            factory,
            folder,
        }
    }
    async fn create(&self) -> McpConnectionView {
        self.manager
            .create(
                &self.agent,
                CreateMcpConnection {
                    label: "Synthetic".into(),
                    endpoint: "http://127.0.0.1:9123/mcp".into(),
                },
            )
            .await
            .unwrap()
    }
    async fn reviewed(&self) -> McpConnectionView {
        let view = self.create().await;
        let view = self
            .manager
            .update(
                &self.agent,
                &view.record.id,
                UpdateMcpConnection {
                    expected_version: 1,
                    label: None,
                    endpoint: None,
                    enabled: Some(true),
                },
            )
            .await
            .unwrap();
        let view = self
            .manager
            .connect(&self.agent, &view.record.id, view.record.version)
            .await
            .unwrap();
        self.manager
            .review(
                &self.agent,
                &view.record.id,
                ReviewMcpTool {
                    expected_version: view.record.version,
                    discovery_id: view.discovery_id.clone().unwrap(),
                    descriptor_digest: view.candidates[0].descriptor_digest.clone(),
                },
            )
            .await
            .unwrap()
    }
    async fn grant_and_stamp(&self, view: &McpConnectionView) -> McpDispatchStamp {
        let tool = view.record.reviewed_tools[0].tool_descriptor();
        let mut guard = self.state.write().await;
        let runtime = guard.agents.get_mut(&self.agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(vec![tool.clone()]);
        runtime.replace_config(config);
        let sid = "chat:mcp-fixture";
        guard.sessions.insert(SessionRecord::new(
            &self.agent,
            sid,
            SessionKind::Chat,
            SessionOrigin::Web,
            "MCP test".into(),
            TitleSource::Owner,
            1,
        ));
        let run = RunRecord::running(
            RunStart {
                agent_id: self.agent.clone(),
                session_id: sid.into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "synthetic fixture".into(),
                model: "mock".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        let run_id = run.id.clone();
        guard.runs.insert(run);
        let agent = guard.agents[&self.agent].state();
        drop(guard);
        self.manager.note_agent_permissions_changed(&self.agent);
        self.manager
            .capture_dispatch(
                &agent,
                &ToolCall {
                    id: "mcp-call-1".into(),
                    name: tool.name,
                    args: Default::default(),
                },
                &RunLink {
                    agent_id: self.agent.clone(),
                    session_id: sid.into(),
                    run_id,
                },
            )
            .unwrap()
    }
    async fn grant(
        &self,
        view: &McpConnectionView,
        granted: bool,
    ) -> Result<McpGrantOutcome, McpServiceError> {
        let tool = &view.record.reviewed_tools[0];
        self.manager
            .set_grant(
                &self.agent,
                &view.record.id,
                &tool.alias,
                McpGrantInput {
                    expected_version: view.record.version,
                    descriptor_digest: tool.descriptor.descriptor_digest.clone(),
                    granted,
                },
            )
            .await
    }
    async fn persisted(&self) -> crate::control_plane_store::ControlPlaneSnapshot {
        let config = self.state.read().await.control_plane_store.clone().unwrap();
        crate::control_plane_store::load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.folder.starts_with(std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

#[tokio::test]
async fn m43_actual_owner_router_grants_one_exact_reviewed_alias() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let tool = &reviewed.record.reviewed_tools[0];
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    fixture.state.write().await.mcp_manager = Some(fixture.manager.clone());
    let app = crate::app::app_with_state(fixture.state.clone(), crate::DaemonConfig::default());
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/agents/{}/mcp-connections/{}/grants/{}",
                    fixture.agent, reviewed.record.id, tool.alias
                ))
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"expectedVersion":reviewed.record.version,
            "descriptorDigest":tool.descriptor.descriptor_digest,"granted":true})
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the owner must be able to grant the exact reviewed alias through the narrow route"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["granted"], true);
    assert_eq!(body["toolNames"], json!([tool.alias]));
    assert_eq!(body["connection"]["id"], reviewed.record.id);
    assert_eq!(body["connection"]["version"], reviewed.record.version + 1);
    assert_eq!(
        fixture.state.read().await.agents[&fixture.agent]
            .config()
            .tools,
        Some(vec![tool.tool_descriptor()])
    );
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 1);
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    assert_eq!(session.discoveries.load(Ordering::SeqCst), 1);
    assert_eq!(
        session.calls.load(Ordering::SeqCst),
        0,
        "granting an alias must not invoke a remote tool"
    );
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_grants_preserve_current_unrelated_descriptors_noops_and_none_semantics() {
    let fixture = Fixture::new().await;
    let first = fixture.reviewed().await;
    let second = fixture.reviewed().await;
    let native = crate::tools::ToolRegistry::new()
        .resolve_descriptors(["read_file", "write_file"])
        .unwrap();
    let unrelated = vec![
        native[0].clone(),
        second.record.reviewed_tools[0].tool_descriptor(),
        native[1].clone(),
    ];
    {
        let mut state = fixture.state.write().await;
        let runtime = state.agents.get_mut(&fixture.agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(unrelated.clone());
        runtime.replace_config(config);
    }
    let added = fixture.grant(&first, true).await.unwrap();
    assert_eq!(added.connection.record.version, first.record.version + 1);
    let mut expected = unrelated.clone();
    expected.push(first.record.reviewed_tools[0].tool_descriptor());
    assert_eq!(
        fixture.state.read().await.agents[&fixture.agent]
            .config()
            .tools,
        Some(expected.clone())
    );
    let repeat = fixture.grant(&added.connection, true).await.unwrap();
    assert_eq!(repeat.connection.record, added.connection.record);
    let removed = fixture.grant(&repeat.connection, false).await.unwrap();
    assert_eq!(
        removed.connection.record.version,
        added.connection.record.version + 1
    );
    assert_eq!(
        fixture.state.read().await.agents[&fixture.agent]
            .config()
            .tools,
        Some(unrelated)
    );
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &second.record.id)
            .await
            .unwrap()
            .record,
        second.record
    );
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 2);
    assert!(fixture
        .factory
        .sessions
        .lock()
        .unwrap()
        .iter()
        .all(|session| session.calls.load(Ordering::SeqCst) == 0));
    fixture.manager.shutdown().await;

    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let absent = fixture.grant(&reviewed, false).await.unwrap();
    assert_eq!(absent.connection.record.version, reviewed.record.version);
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    let added = fixture.grant(&reviewed, true).await.unwrap();
    let removed = fixture.grant(&added.connection, false).await.unwrap();
    assert_eq!(
        fixture.state.read().await.agents[&fixture.agent]
            .config()
            .tools,
        Some(vec![])
    );
    assert_eq!(
        fixture
            .grant(&removed.connection, false)
            .await
            .unwrap()
            .connection
            .record,
        removed.connection.record
    );
    fixture.manager.shutdown().await;
}

async fn m43_http(
    app: &axum::Router,
    uri: &str,
    origin: Option<&str>,
    body: serde_json::Value,
) -> (axum::http::StatusCode, String, serde_json::Value) {
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::util::ServiceExt;
    let mut request = Request::builder()
        .method("PATCH")
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("content-type", "application/json");
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cache = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (
        status,
        cache,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn m43_grant_http_rejects_unauthorized_stale_foreign_helper_and_non_strict_inputs() {
    use axum::http::StatusCode;
    let fixture = Fixture::new().await;
    let view = fixture.reviewed().await;
    let tool = &view.record.reviewed_tools[0];
    let (foreign, helper) = {
        let mut state = fixture.state.write().await;
        let foreign = state
            .create_agent(crate::sessions::test_support::agent_config("Foreign owner"))
            .unwrap()
            .state
            .id;
        let mut config = crate::sessions::test_support::agent_config("Helper owner");
        config.settings = Some(anima_core::AgentSettings::default());
        config.settings.as_mut().unwrap().additional.insert(
            "workspaceRole".into(),
            anima_core::DataValue::String("helper".into()),
        );
        let helper = state.create_agent(config).unwrap().state.id;
        state.mcp_manager = Some(fixture.manager.clone());
        (foreign, helper)
    };
    let app = crate::app::app_with_state(fixture.state.clone(), crate::DaemonConfig::default());
    let path = format!(
        "/api/agents/{}/mcp-connections/{}/grants/{}",
        fixture.agent, view.record.id, tool.alias
    );
    let valid = json!({"expectedVersion":view.record.version,"descriptorDigest":tool.descriptor.descriptor_digest,"granted":true});
    for origin in [None, Some("https://foreign.invalid")] {
        let (status, cache, _) = m43_http(&app, &path, origin, valid.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(cache, "no-store");
    }
    for body in [
        json!({"expectedVersion":view.record.version,"descriptorDigest":tool.descriptor.descriptor_digest}),
        json!({"expectedVersion":null,"descriptorDigest":tool.descriptor.descriptor_digest,"granted":true}),
        json!({"expectedVersion":view.record.version,"descriptorDigest":tool.descriptor.descriptor_digest,"granted":null}),
        json!({"expectedVersion":view.record.version,"descriptorDigest":null,"granted":true}),
        json!({"expectedVersion":view.record.version,"descriptorDigest":tool.descriptor.descriptor_digest,"granted":true,"tools":[]}),
    ] {
        let (status, cache, _) = m43_http(&app, &path, Some("http://localhost:4200"), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(cache, "no-store");
    }
    for body in [
        json!({"expectedVersion":view.record.version-1,"descriptorDigest":tool.descriptor.descriptor_digest,"granted":true}),
        json!({"expectedVersion":view.record.version,"descriptorDigest":"sha256:wrong","granted":true}),
    ] {
        assert_eq!(
            m43_http(&app, &path, Some("http://localhost:4200"), body)
                .await
                .0,
            StatusCode::CONFLICT
        );
    }
    for agent in [&foreign, &helper] {
        let uri = format!(
            "/api/agents/{agent}/mcp-connections/{}/grants/{}",
            view.record.id, tool.alias
        );
        assert_eq!(
            m43_http(&app, &uri, Some("http://localhost:4200"), valid.clone())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let uri = format!(
        "/api/agents/{}/mcp-connections/{}/grants/unreviewed",
        fixture.agent, view.record.id
    );
    assert_eq!(
        m43_http(&app, &uri, Some("http://localhost:4200"), valid)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &view.record.id)
            .await
            .unwrap()
            .record,
        view.record
    );
    use tower::util::ServiceExt;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["paths"]
        ["/api/agents/{agent_id}/mcp-connections/{connection_id}/grants/{alias}"]["patch"]
        .is_object());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_grant_precommit_failure_keeps_state_and_uncertainty_publishes_fail_closed() {
    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let previous = std::fs::read(fixture.folder.join("control.json")).unwrap();
    let invalid = fixture.folder.join("unwritable.json");
    std::fs::create_dir(&invalid).unwrap();
    fixture.state.write().await.control_plane_store = Some(ControlPlaneStoreConfig::Json(invalid));
    assert_eq!(
        fixture.grant(&reviewed, true).await.unwrap_err(),
        McpServiceError::Unavailable
    );
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &reviewed.record.id)
            .await
            .unwrap()
            .record,
        reviewed.record
    );
    assert_eq!(fixture.manager.ready_descriptors(&fixture.agent).len(), 1);
    assert_eq!(
        std::fs::read(fixture.folder.join("control.json")).unwrap(),
        previous
    );
    fixture.state.write().await.control_plane_store = Some(ControlPlaneStoreConfig::Json(
        fixture.folder.join("control.json"),
    ));
    fixture.grant(&reviewed, true).await.unwrap();
    fixture.manager.shutdown().await;

    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    let manager = fixture.manager.clone();
    let agent = fixture.agent.clone();
    let row = reviewed.clone();
    let pending = tokio::spawn(async move {
        manager
            .set_grant(
                &agent,
                &row.record.id,
                &row.record.reviewed_tools[0].alias,
                McpGrantInput {
                    expected_version: row.record.version,
                    descriptor_digest: row.record.reviewed_tools[0]
                        .descriptor
                        .descriptor_digest
                        .clone(),
                    granted: true,
                },
            )
            .await
    });
    timeout(Duration::from_secs(1), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &reviewed.record.id)
            .await
            .unwrap()
            .record
            .version,
        reviewed.record.version
    );
    gate.release.add_permits(1);
    assert_eq!(
        timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        McpServiceError::Unavailable
    );
    let alias = &reviewed.record.reviewed_tools[0].alias;
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .allows_tool(alias));
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &reviewed.record.id)
            .await
            .unwrap()
            .record
            .version,
        reviewed.record.version + 1
    );
    assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
    let persisted = fixture.persisted().await;
    assert!(persisted
        .agents
        .iter()
        .find(|agent| agent.state.id == fixture.agent)
        .unwrap()
        .state
        .config
        .allows_tool(alias));
    let mut restarted = DaemonState::new();
    restarted.restore_control_plane_snapshot(persisted).unwrap();
    assert!(restarted.agents[&fixture.agent].config().allows_tool(alias));
    assert!(restarted.mcp_publication.read().unwrap().live.is_empty());
    assert_eq!(restarted.mcp_publication.read().unwrap().known.len(), 1);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_dropped_grant_caller_still_settles_after_ack_without_staged_publication() {
    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let manager = fixture.manager.clone();
    let agent = fixture.agent.clone();
    let row = reviewed.clone();
    let pending = tokio::spawn(async move {
        manager
            .set_grant(
                &agent,
                &row.record.id,
                &row.record.reviewed_tools[0].alias,
                McpGrantInput {
                    expected_version: row.record.version,
                    descriptor_digest: row.record.reviewed_tools[0]
                        .descriptor
                        .descriptor_digest
                        .clone(),
                    granted: true,
                },
            )
            .await
    });
    timeout(Duration::from_secs(1), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &reviewed.record.id)
            .await
            .unwrap()
            .record,
        reviewed.record
    );
    pending.abort();
    let _ = pending.await;
    gate.release.add_permits(1);
    timeout(Duration::from_secs(1), async {
        loop {
            if fixture.state.read().await.agents[&fixture.agent]
                .config()
                .allows_tool(&reviewed.record.reviewed_tools[0].alias)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fixture.persisted().await.mcp_connections[0].version,
        reviewed.record.version + 1
    );
    assert_eq!(
        fixture.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        0
    );
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_ordered_grant_projection_survives_newer_stale_snapshots_and_opposite_connections() {
    let fixture = Fixture::new().await;
    let first = fixture.reviewed().await;
    let second = fixture.reviewed().await;
    let native = crate::tools::ToolRegistry::new()
        .resolve_descriptors(["read_file"])
        .unwrap()
        .remove(0);
    let a = first.record.reviewed_tools[0].tool_descriptor();
    let b = second.record.reviewed_tools[0].tool_descriptor();
    let (candidate, ordinary) = {
        let mut state = fixture.state.write().await;
        let mut rows = state.mcp_registry.snapshot();
        rows.iter_mut()
            .find(|row| row.id == first.record.id)
            .unwrap()
            .version += 1;
        let candidate = state
            .control_plane_persist_request()
            .with_mcp_grant_candidate(rows, &fixture.agent, Some(vec![native.clone(), a.clone()]))
            .unwrap();
        let runtime = state.agents.get_mut(&fixture.agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(vec![native.clone()]);
        runtime.replace_config(config);
        let ordinary = state.control_plane_persist_request();
        (candidate, ordinary)
    };
    candidate.save_mcp_candidate().await.unwrap();
    ordinary.save().await.unwrap();
    assert_eq!(
        fixture
            .persisted()
            .await
            .agents
            .iter()
            .find(|row| row.state.id == fixture.agent)
            .unwrap()
            .state
            .config
            .tools,
        Some(vec![native.clone(), a.clone()])
    );
    // Publish the first settled grant, then admit opposite edits on distinct
    // connections. The newer ordinary snapshot still observes both old versions.
    let (candidate, ordinary) = {
        let mut state = fixture.state.write().await;
        state
            .mcp_registry
            .connections
            .get_mut(&first.record.id)
            .unwrap()
            .version += 1;
        let runtime = state.agents.get_mut(&fixture.agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(vec![native.clone(), a.clone()]);
        runtime.replace_config(config);
        let mut rows = state.mcp_registry.snapshot();
        rows.iter_mut()
            .find(|row| row.id == first.record.id)
            .unwrap()
            .version += 1;
        rows.iter_mut()
            .find(|row| row.id == second.record.id)
            .unwrap()
            .version += 1;
        let candidate = state
            .control_plane_persist_request()
            .with_mcp_grant_candidate(rows, &fixture.agent, Some(vec![native.clone(), b.clone()]))
            .unwrap();
        let ordinary = state.control_plane_persist_request();
        (candidate, ordinary)
    };
    candidate.save_mcp_candidate().await.unwrap();
    ordinary.save().await.unwrap();
    let persisted = fixture.persisted().await;
    assert_eq!(
        persisted
            .agents
            .iter()
            .find(|row| row.state.id == fixture.agent)
            .unwrap()
            .state
            .config
            .tools,
        Some(vec![native.clone(), b.clone()])
    );
    assert_eq!(
        persisted
            .mcp_connections
            .iter()
            .find(|row| row.id == first.record.id)
            .unwrap()
            .version,
        first.record.version + 2
    );
    assert_eq!(
        persisted
            .mcp_connections
            .iter()
            .find(|row| row.id == second.record.id)
            .unwrap()
            .version,
        second.record.version + 1
    );
    let (candidate, ordinary) = {
        let mut state = fixture.state.write().await;
        state.mcp_registry = McpRegistry::restored(
            persisted.mcp_connections,
            &HashSet::from([fixture.agent.clone()]),
        )
        .unwrap();
        let runtime = state.agents.get_mut(&fixture.agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(vec![native.clone(), b.clone()]);
        runtime.replace_config(config);
        let mut rows = state.mcp_registry.snapshot();
        rows.iter_mut()
            .find(|row| row.id == second.record.id)
            .unwrap()
            .version += 1;
        let candidate = state
            .control_plane_persist_request()
            .with_mcp_grant_candidate(rows, &fixture.agent, Some(vec![native.clone()]))
            .unwrap();
        // A current-version intentional first-connection grant survives while
        // the second connection's older snapshot receives its acknowledged revoke.
        let runtime = state.agents.get_mut(&fixture.agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(vec![native.clone(), a.clone(), b.clone()]);
        runtime.replace_config(config);
        let ordinary = state.control_plane_persist_request();
        (candidate, ordinary)
    };
    candidate.save_mcp_candidate().await.unwrap();
    ordinary.save().await.unwrap();
    assert_eq!(
        fixture
            .persisted()
            .await
            .agents
            .iter()
            .find(|row| row.state.id == fixture.agent)
            .unwrap()
            .state
            .config
            .tools,
        Some(vec![native, a])
    );
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_current_version_generic_settings_revoke_remains_authoritative() {
    let fixture = Fixture::new().await;
    let first = fixture.reviewed().await;
    let second = fixture.reviewed().await;
    let first = fixture.grant(&first, true).await.unwrap().connection;
    let second = fixture.grant(&second, true).await.unwrap().connection;
    fixture.state.write().await.mcp_manager = Some(fixture.manager.clone());
    let app = crate::app::app_with_state(fixture.state.clone(), crate::DaemonConfig::default());
    assert_eq!(
        m43_http(
            &app,
            &format!("/api/agents/{}", fixture.agent),
            Some("http://localhost:4200"),
            json!({"tools":["read_file"]})
        )
        .await
        .0,
        axum::http::StatusCode::OK
    );
    let native = crate::tools::ToolRegistry::new()
        .resolve_descriptors(["read_file"])
        .unwrap();
    assert_eq!(
        fixture.state.read().await.agents[&fixture.agent]
            .config()
            .tools,
        Some(native.clone())
    );
    assert_eq!(
        fixture
            .persisted()
            .await
            .agents
            .iter()
            .find(|row| row.state.id == fixture.agent)
            .unwrap()
            .state
            .config
            .tools,
        Some(native.clone())
    );
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &first.record.id)
            .await
            .unwrap()
            .record
            .version,
        first.record.version
    );
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &second.record.id)
            .await
            .unwrap()
            .record
            .version,
        second.record.version
    );
    let readded = fixture.grant(&second, true).await.unwrap();
    assert!(!fixture.state.read().await.agents[&fixture.agent]
        .config()
        .allows_tool(&first.record.reviewed_tools[0].alias));
    let mut expected = native;
    expected.push(second.record.reviewed_tools[0].tool_descriptor());
    assert_eq!(
        fixture.state.read().await.agents[&fixture.agent]
            .config()
            .tools,
        Some(expected)
    );
    assert_eq!(readded.connection.record.version, second.record.version + 1);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_deleted_connection_projection_cannot_restore_grants_and_restart_stays_disconnected() {
    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let granted = fixture.grant(&reviewed, true).await.unwrap().connection;
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let manager = fixture.manager.clone();
    let agent = fixture.agent.clone();
    let id = granted.record.id.clone();
    let version = granted.record.version;
    let pending = tokio::spawn(async move { manager.delete(&agent, &id, version).await });
    timeout(Duration::from_secs(1), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let ordinary = fixture.state.write().await.control_plane_persist_request();
    gate.release.add_permits(1);
    timeout(Duration::from_secs(1), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    ordinary.save().await.unwrap();
    let persisted = fixture.persisted().await;
    assert!(persisted.mcp_connections.is_empty());
    assert!(!persisted
        .agents
        .iter()
        .find(|row| row.state.id == fixture.agent)
        .unwrap()
        .state
        .config
        .allows_tool(&granted.record.reviewed_tools[0].alias));
    let mut restarted = DaemonState::new();
    restarted.restore_control_plane_snapshot(persisted).unwrap();
    assert!(restarted.mcp_publication.read().unwrap().known.is_empty());
    assert!(restarted.mcp_publication.read().unwrap().live.is_empty());
    assert_eq!(
        fixture.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        0
    );
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn m43_active_call_revoke_fences_epoch_and_retains_physical_permit_until_close() {
    let fixture = Fixture::new().await;
    fixture.factory.hold_calls.store(true, Ordering::SeqCst);
    let reviewed = fixture.reviewed().await;
    let granted = fixture.grant(&reviewed, true).await.unwrap().connection;
    let stamp = fixture.grant_and_stamp(&granted).await;
    let stale_stamp = stamp.clone();
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    session.hold_close.store(true, Ordering::SeqCst);
    let manager = fixture.manager.clone();
    let pending = tokio::spawn(async move {
        manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
    });
    timeout(Duration::from_secs(1), session.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let revoked = fixture.grant(&granted, false).await.unwrap();
    assert!(!revoked.granted);
    assert!(revoked.connection.physical_calls_active);
    assert!(!fixture.state.read().await.agents[&fixture.agent]
        .config()
        .allows_tool(&granted.record.reviewed_tools[0].alias));
    assert_eq!(
        timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        McpServiceError::OutcomeUnknown
    );
    assert!(fixture
        .manager
        .invoke(stale_stamp, json!({"value":"violet"}), CancelSignal::new())
        .await
        .is_err());
    assert_eq!(session.calls.load(Ordering::SeqCst), 1);
    assert!(
        fixture
            .manager
            .get(&fixture.agent, &granted.record.id)
            .await
            .unwrap()
            .physical_calls_active
    );
    session.close_release.add_permits(1);
    timeout(Duration::from_secs(1), async {
        loop {
            if !fixture
                .manager
                .get(&fixture.agent, &granted.record.id)
                .await
                .unwrap()
                .physical_calls_active
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.manager.shutdown().await;
}

#[test]
fn reviewed_records_round_trip_validate_and_restore_only_known_ordinary_owners() {
    let id = uuid::Uuid::new_v4().to_string();
    let descriptor = candidate();
    let row = McpConnectionRecord {
        id: id.clone(),
        agent_id: "owner".into(),
        label: "Own".into(),
        endpoint: "https://example.com/mcp".into(),
        enabled: true,
        version: 3,
        reviewed_tools: vec![ReviewedMcpTool {
            alias: alias_for(&id, &descriptor.descriptor_digest).unwrap(),
            descriptor,
        }],
    };
    let encoded = serde_json::to_value(&row).unwrap();
    assert!(encoded["reviewedTools"][0]["descriptor"].is_object());
    let decoded: McpConnectionRecord = serde_json::from_value(encoded).unwrap();
    assert_eq!(row, decoded);
    let registry =
        McpRegistry::restored(vec![decoded.clone()], &HashSet::from(["owner".into()])).unwrap();
    let published = registry.published();
    assert_eq!(published.read().unwrap().known.len(), 1);
    assert!(published.read().unwrap().live.is_empty());
    assert!(McpRegistry::restored(vec![decoded.clone()], &HashSet::new()).is_err());
    let mut forged = decoded;
    forged.reviewed_tools[0].descriptor.description = "Changed without review".into();
    assert!(McpRegistry::validate(&[forged]).is_err());
}

#[test]
fn anonymous_endpoint_and_optional_patch_fields_are_strict() {
    for endpoint in [
        "http://localhost/mcp",
        "http://127.1/mcp",
        "https://user:secret@example.com/mcp",
        "https://example.com/mcp?token=secret",
        "https://example.com/mcp#fragment",
        "file:///mcp",
    ] {
        assert_eq!(
            normalized_endpoint(endpoint).unwrap_err(),
            McpServiceError::InvalidRequest
        );
    }
    assert!(normalized_endpoint("http://[::1]:1234/mcp").is_ok());
    assert!(normalized_endpoint("https://example.com/mcp").is_ok());
    assert!(serde_json::from_value::<UpdateMcpConnection>(
        json!({"expectedVersion":1,"label":"good","enabled":null})
    )
    .is_err());
    assert!(serde_json::from_value::<ReviewMcpTool>(
        json!({"expectedVersion":1,"candidateDigest":"old"})
    )
    .is_err());
}

#[tokio::test]
async fn configuration_discovery_review_are_separate_from_grant_and_call() {
    let fixture = Fixture::new().await;
    let view = fixture.create().await;
    assert_eq!(view.record.version, 1);
    assert!(!view.record.enabled);
    assert_eq!(view.state, McpConnectionState::Disconnected);
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .manager
            .connect(&fixture.agent, &view.record.id, 1)
            .await
            .unwrap_err(),
        McpServiceError::Conflict
    );
    let view = fixture.reviewed().await;
    assert_eq!(view.state, McpConnectionState::Ready);
    assert_eq!(view.record.reviewed_tools.len(), 1);
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .tools
        .is_none());
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    assert_eq!(session.calls.load(Ordering::SeqCst), 0);
    let duplicate = fixture
        .manager
        .review(
            &fixture.agent,
            &view.record.id,
            ReviewMcpTool {
                expected_version: view.record.version,
                discovery_id: view.discovery_id.clone().unwrap(),
                descriptor_digest: view.record.reviewed_tools[0]
                    .descriptor
                    .descriptor_digest
                    .clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(duplicate.record.version, view.record.version);
    assert_eq!(
        fixture
            .manager
            .review(
                &fixture.agent,
                &view.record.id,
                ReviewMcpTool {
                    expected_version: 2,
                    discovery_id: view.discovery_id.clone().unwrap(),
                    descriptor_digest: view.record.reviewed_tools[0]
                        .descriptor
                        .descriptor_digest
                        .clone()
                }
            )
            .await
            .unwrap_err(),
        McpServiceError::Conflict
    );
    let stamp = fixture.grant_and_stamp(&view).await;
    let content = fixture
        .manager
        .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&content.text).unwrap();
    assert_eq!(result["textBlocks"][0], "synthetic violet record");
    assert_eq!(session.calls.load(Ordering::SeqCst), 1);
    fixture
        .manager
        .disconnect(&fixture.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
    assert!(fixture
        .manager
        .known_descriptor(&fixture.agent, &view.record.reviewed_tools[0].alias)
        .is_some());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn failed_candidate_save_keeps_registry_and_published_descriptor_unchanged() {
    let fixture = Fixture::new().await;
    let view = fixture.reviewed().await;
    let before = fixture.state.read().await.mcp_registry.snapshot();
    let known = fixture
        .manager
        .known_descriptor(&fixture.agent, &view.record.reviewed_tools[0].alias);
    let invalid = fixture.folder.join("directory.json");
    std::fs::create_dir(&invalid).unwrap();
    fixture.state.write().await.control_plane_store = Some(ControlPlaneStoreConfig::Json(invalid));
    assert_eq!(
        fixture
            .manager
            .update(
                &fixture.agent,
                &view.record.id,
                UpdateMcpConnection {
                    expected_version: view.record.version,
                    label: Some("Changed".into()),
                    endpoint: None,
                    enabled: None
                }
            )
            .await
            .unwrap_err(),
        McpServiceError::Unavailable
    );
    assert_eq!(fixture.state.read().await.mcp_registry.snapshot(), before);
    assert_eq!(
        fixture
            .manager
            .known_descriptor(&fixture.agent, &view.record.reviewed_tools[0].alias),
        known
    );
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn foreign_owner_and_removed_then_readded_grant_cannot_reuse_dispatch_stamp() {
    let fixture = Fixture::new().await;
    let view = fixture.reviewed().await;
    let stamp = fixture.grant_and_stamp(&view).await;
    assert_eq!(
        fixture
            .manager
            .get("other", &view.record.id)
            .await
            .unwrap_err(),
        McpServiceError::NotFound
    );
    fixture
        .manager
        .note_agent_permissions_changed(&fixture.agent);
    fixture
        .manager
        .note_agent_permissions_changed(&fixture.agent);
    assert_eq!(
        fixture
            .manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap_err(),
        McpServiceError::Revoked
    );
    assert_eq!(
        fixture.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        0
    );
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn cancelling_dispatched_call_retains_slot_until_physical_close_joins() {
    let fixture = Fixture::new().await;
    fixture.factory.hold_calls.store(true, Ordering::SeqCst);
    let view = fixture.reviewed().await;
    let stamp = fixture.grant_and_stamp(&view).await;
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    session.hold_close.store(true, Ordering::SeqCst);
    let cancel = CancelSignal::new();
    let child_cancel = cancel.clone();
    let manager = fixture.manager.clone();
    let pending = tokio::spawn(async move {
        manager
            .invoke(stamp, json!({"value":"violet"}), child_cancel)
            .await
    });
    timeout(Duration::from_secs(1), session.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    cancel.cancel();
    assert_eq!(
        timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        McpServiceError::OutcomeUnknown
    );
    assert!(
        fixture
            .manager
            .get(&fixture.agent, &view.record.id)
            .await
            .unwrap()
            .physical_calls_active
    );
    assert_eq!(
        fixture
            .manager
            .connect(&fixture.agent, &view.record.id, view.record.version)
            .await
            .unwrap_err(),
        McpServiceError::Busy
    );
    assert_eq!(session.calls.load(Ordering::SeqCst), 1);
    session.close_release.add_permits(1);
    timeout(Duration::from_secs(1), async {
        loop {
            if !fixture
                .manager
                .get(&fixture.agent, &view.record.id)
                .await
                .unwrap()
                .physical_calls_active
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn dropped_caller_settles_candidate_and_never_publishes_staged_configuration() {
    let fixture = Fixture::new().await;
    let view = fixture.reviewed().await;
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let (manager, agent, id, version) = (
        fixture.manager.clone(),
        fixture.agent.clone(),
        view.record.id.clone(),
        view.record.version,
    );
    let pending = tokio::spawn(async move {
        manager
            .update(
                &agent,
                &id,
                UpdateMcpConnection {
                    expected_version: version,
                    label: None,
                    endpoint: None,
                    enabled: Some(false),
                },
            )
            .await
    });
    timeout(Duration::from_secs(1), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(
        fixture
            .manager
            .get(&fixture.agent, &view.record.id)
            .await
            .unwrap()
            .record
            .enabled
    );
    assert_eq!(fixture.manager.ready_descriptors(&fixture.agent).len(), 1);
    pending.abort();
    gate.release.add_permits(1);
    timeout(Duration::from_secs(1), async {
        loop {
            if !fixture
                .manager
                .get(&fixture.agent, &view.record.id)
                .await
                .unwrap()
                .record
                .enabled
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
    assert!(fixture
        .manager
        .known_descriptor(&fixture.agent, &view.record.reviewed_tools[0].alias)
        .is_some());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn four_connection_limit_is_workspace_wide_without_discovery_or_grant_side_effects() {
    let fixture = Fixture::new().await;
    for _ in 0..4 {
        fixture.create().await;
    }
    let other = fixture
        .state
        .write()
        .await
        .create_agent(crate::sessions::test_support::agent_config("other owner"))
        .unwrap()
        .state
        .id;
    assert_eq!(
        fixture
            .manager
            .create(
                &other,
                CreateMcpConnection {
                    label: "Fifth".into(),
                    endpoint: "https://example.com/mcp".into()
                }
            )
            .await
            .unwrap_err(),
        McpServiceError::Busy
    );
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.manager.list(&fixture.agent).await.unwrap().len(), 4);
    assert!(fixture.manager.list(&other).await.unwrap().is_empty());
}

#[tokio::test]
async fn repeated_idle_disconnect_preserves_stopping_generation_until_physical_close() {
    let fixture = Fixture::new().await;
    let view = fixture.reviewed().await;
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    session.hold_close.store(true, Ordering::SeqCst);
    let first = fixture
        .manager
        .disconnect(&fixture.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    assert_eq!(first.state, McpConnectionState::Stopping);
    assert!(!first.physical_calls_active);
    let second = fixture
        .manager
        .disconnect(&fixture.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    assert_eq!(second.state, McpConnectionState::Stopping);
    assert_eq!(second.wire_generation, first.wire_generation);
    assert_eq!(
        fixture
            .manager
            .connect(&fixture.agent, &view.record.id, view.record.version)
            .await
            .unwrap_err(),
        McpServiceError::Busy
    );
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 1);
    session.close_release.add_permits(1);
    timeout(Duration::from_secs(1), async {
        loop {
            if fixture
                .manager
                .get(&fixture.agent, &view.record.id)
                .await
                .unwrap()
                .state
                == McpConnectionState::Disconnected
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture
        .manager
        .connect(&fixture.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 2);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn agent_retirement_closes_connected_never_reviewed_session() {
    let fixture = Fixture::new().await;
    let created = fixture.create().await;
    let enabled = fixture
        .manager
        .update(
            &fixture.agent,
            &created.record.id,
            UpdateMcpConnection {
                expected_version: 1,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let view = fixture
        .manager
        .connect(&fixture.agent, &enabled.record.id, enabled.record.version)
        .await
        .unwrap();
    assert!(view.record.reviewed_tools.is_empty());
    assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    session.hold_close.store(true, Ordering::SeqCst);
    fixture.manager.retire_agent(&fixture.agent);
    assert_eq!(
        fixture
            .manager
            .get(&fixture.agent, &view.record.id)
            .await
            .unwrap()
            .state,
        McpConnectionState::Stopping
    );
    session.close_release.add_permits(1);
    timeout(Duration::from_secs(1), fixture.manager.shutdown())
        .await
        .unwrap();
    assert!(session.closed.load(Ordering::SeqCst));
    assert_eq!(session.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn terminal_close_failure_keeps_permits_quarantined_but_shutdown_finishes() {
    let fixture = Fixture::new().await;
    fixture.factory.hold_calls.store(true, Ordering::SeqCst);
    let view = fixture.reviewed().await;
    let stamp = fixture.grant_and_stamp(&view).await;
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    session.fail_close.store(true, Ordering::SeqCst);
    let cancel = CancelSignal::new();
    let child = cancel.clone();
    let manager = fixture.manager.clone();
    let pending = tokio::spawn(async move {
        manager
            .invoke(stamp, json!({"value":"violet"}), child)
            .await
    });
    timeout(Duration::from_secs(1), session.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    cancel.cancel();
    assert_eq!(
        timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        McpServiceError::OutcomeUnknown
    );
    timeout(Duration::from_secs(1), fixture.manager.shutdown())
        .await
        .expect("settled unknown cleanup cannot wait for an impossible empty quarantine");
    assert!(
        fixture
            .manager
            .get(&fixture.agent, &view.record.id)
            .await
            .unwrap()
            .physical_calls_active
    );
    assert_eq!(
        fixture
            .manager
            .connect(&fixture.agent, &view.record.id, view.record.version)
            .await
            .unwrap_err(),
        McpServiceError::Unavailable
    );
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn acknowledged_mcp_candidate_survives_a_newer_prepared_stale_ordinary_snapshot() {
    let fixture = Fixture::new().await;
    let view = fixture.reviewed().await;
    let alias = view.record.reviewed_tools[0].alias.clone();
    let (candidate, ordinary) = {
        let mut state = fixture.state.write().await;
        let runtime = state.agents.get_mut(&fixture.agent).unwrap();
        let mut config = runtime.config().clone();
        let read = crate::tools::ToolRegistry::new()
            .resolve_descriptors(["read_file"])
            .unwrap()
            .remove(0);
        config.tools = Some(vec![view.record.reviewed_tools[0].tool_descriptor(), read]);
        runtime.replace_config(config);
        let candidate = state
            .control_plane_persist_request()
            .with_mcp_candidate(
                Vec::new(),
                Some((fixture.agent.clone(), vec![alias.clone()])),
            )
            .unwrap();
        let ordinary = state.control_plane_persist_request();
        (candidate, ordinary)
    };
    candidate.save_mcp_candidate().await.unwrap();
    ordinary.save().await.unwrap();
    let config = fixture.state.read().await.control_plane_store.clone();
    let snapshot =
        crate::control_plane_store::load_control_plane_snapshot(config.as_ref().unwrap())
            .await
            .unwrap()
            .unwrap();
    assert!(snapshot.mcp_connections.is_empty());
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.state.id == fixture.agent)
        .unwrap();
    let names = agent
        .state
        .config
        .tools
        .iter()
        .flatten()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["read_file"]);
    assert!(!names.contains(&alias.as_str()));
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn durability_uncertain_candidate_retains_committed_rows_and_revokes_ready_binding() {
    for deleting in [false, true] {
        let fixture = Fixture::new().await;
        let view = fixture.reviewed().await;
        let alias = view.record.reviewed_tools[0].alias.clone();
        {
            let mut state = fixture.state.write().await;
            let runtime = state.agents.get_mut(&fixture.agent).unwrap();
            let mut config = runtime.config().clone();
            config.tools = Some(vec![view.record.reviewed_tools[0].tool_descriptor()]);
            runtime.replace_config(config);
        }
        let gate = fixture
            .state
            .write()
            .await
            .install_test_control_plane_uncertain_save_gate();
        let (manager, agent, id, version) = (
            fixture.manager.clone(),
            fixture.agent.clone(),
            view.record.id.clone(),
            view.record.version,
        );
        let pending = tokio::spawn(async move {
            if deleting {
                manager.delete(&agent, &id, version).await
            } else {
                manager
                    .update(
                        &agent,
                        &id,
                        UpdateMcpConnection {
                            expected_version: version,
                            label: None,
                            endpoint: None,
                            enabled: Some(false),
                        },
                    )
                    .await
                    .map(|_| ())
            }
        });
        timeout(Duration::from_secs(1), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        assert_eq!(fixture.manager.ready_descriptors(&fixture.agent).len(), 1);
        gate.release.add_permits(1);
        assert_eq!(
            timeout(Duration::from_secs(1), pending)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err(),
            McpServiceError::Unavailable
        );
        assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
        let config = fixture.state.read().await.control_plane_store.clone();
        let snapshot =
            crate::control_plane_store::load_control_plane_snapshot(config.as_ref().unwrap())
                .await
                .unwrap()
                .unwrap();
        if deleting {
            assert_eq!(
                fixture
                    .manager
                    .get(&fixture.agent, &view.record.id)
                    .await
                    .unwrap_err(),
                McpServiceError::NotFound
            );
            assert!(fixture
                .manager
                .known_descriptor(&fixture.agent, &alias)
                .is_none());
            assert!(snapshot.mcp_connections.is_empty());
            assert!(!fixture.state.read().await.agents[&fixture.agent]
                .config()
                .allows_tool(&alias));
            assert!(!snapshot
                .agents
                .iter()
                .find(|agent| agent.state.id == fixture.agent)
                .unwrap()
                .state
                .config
                .allows_tool(&alias));
        } else {
            let current = fixture
                .manager
                .get(&fixture.agent, &view.record.id)
                .await
                .unwrap();
            assert!(!current.record.enabled);
            assert_eq!(current.record.version, view.record.version + 1);
            assert!(fixture
                .manager
                .known_descriptor(&fixture.agent, &alias)
                .is_some());
            assert_eq!(snapshot.mcp_connections[0], current.record);
        }
        fixture.manager.shutdown().await;
    }
}

struct NoTelegram;
#[async_trait::async_trait]
impl crate::connectors::runtime::TelegramTransport for NoTelegram {
    async fn get_me(
        &self,
        _: &crate::connectors::credentials::TelegramBotToken,
    ) -> Result<
        crate::connectors::TelegramBotIdentity,
        crate::connectors::telegram::TelegramTransportError,
    > {
        panic!("agent deletion has no connector transport work")
    }
    async fn get_updates(
        &self,
        _: &crate::connectors::credentials::TelegramBotToken,
        _: i64,
    ) -> Result<
        crate::connectors::telegram::TelegramUpdateBatch,
        crate::connectors::telegram::TelegramTransportError,
    > {
        panic!("agent deletion has no connector transport work")
    }
    async fn send_message(
        &self,
        _: &crate::connectors::credentials::TelegramBotToken,
        _: &str,
        _: &str,
    ) -> Result<
        Vec<crate::connectors::telegram::TelegramSentMessage>,
        crate::connectors::telegram::TelegramTransportError,
    > {
        panic!("agent deletion has no connector transport work")
    }
}
#[tokio::test]
async fn actual_agent_delete_purges_acknowledged_metadata_but_failed_delete_restores_it_disconnected(
) {
    for fail in [false, true] {
        let fixture = Fixture::new().await;
        let view = fixture.reviewed().await;
        {
            let mut state = fixture.state.write().await;
            let runtime = state.agents.get_mut(&fixture.agent).unwrap();
            let mut config = runtime.config().clone();
            config.tools = Some(vec![view.record.reviewed_tools[0].tool_descriptor()]);
            runtime.replace_config(config);
        }
        let session = fixture.factory.sessions.lock().unwrap()[0].clone();
        let runs = crate::agent_runs::AgentRunCoordinator::new(
            fixture.state.clone(),
            Arc::new(Semaphore::new(4)),
        )
        .with_mcp(fixture.manager.clone());
        let connectors = crate::connectors::runtime::ConnectorManager::new(
            fixture.state.clone(),
            runs,
            Arc::new(crate::connectors::credentials::InMemoryCredentialStore::default()),
            Arc::new(NoTelegram),
        );
        if fail {
            let invalid = fixture.folder.join("invalid.json");
            std::fs::create_dir(&invalid).unwrap();
            fixture.state.write().await.control_plane_store =
                Some(ControlPlaneStoreConfig::Json(invalid));
        }
        let result = timeout(
            Duration::from_secs(1),
            connectors.delete_agent(fixture.agent.clone()),
        )
        .await
        .unwrap();
        if fail {
            assert!(result.is_err());
            let state = fixture.state.read().await;
            assert!(state.agents.contains_key(&fixture.agent));
            assert_eq!(state.mcp_registry.snapshot(), vec![view.record.clone()]);
            assert!(state.agents[&fixture.agent]
                .config()
                .allows_tool(&view.record.reviewed_tools[0].alias));
            drop(state);
            assert!(fixture
                .manager
                .known_descriptor(&fixture.agent, &view.record.reviewed_tools[0].alias)
                .is_some());
        } else {
            result.unwrap();
            let state = fixture.state.read().await;
            assert!(!state.agents.contains_key(&fixture.agent));
            assert!(state.mcp_registry.snapshot().is_empty());
            assert!(state.mcp_publication.read().unwrap().known.is_empty());
            drop(state);
            let config = fixture.state.read().await.control_plane_store.clone();
            let snapshot =
                crate::control_plane_store::load_control_plane_snapshot(config.as_ref().unwrap())
                    .await
                    .unwrap()
                    .unwrap();
            assert!(snapshot.mcp_connections.is_empty());
            assert!(snapshot
                .agents
                .iter()
                .all(|agent| agent.state.id != fixture.agent));
        }
        assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
        timeout(Duration::from_secs(1), fixture.manager.shutdown())
            .await
            .unwrap();
        assert!(session.closed.load(Ordering::SeqCst));
        assert_eq!(session.calls.load(Ordering::SeqCst), 0);
        connectors.shutdown().await;
    }
}

#[tokio::test]
async fn rediscovery_changes_require_a_new_review_and_never_transfer_the_old_grant() {
    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    let old = reviewed.record.reviewed_tools[0].clone();
    let stamp = fixture.grant_and_stamp(&reviewed).await;
    let session = fixture.factory.sessions.lock().unwrap()[0].clone();
    session.changed_schema.store(true, Ordering::SeqCst);
    let discovered = fixture
        .manager
        .discover(&fixture.agent, &reviewed.record.id, reviewed.record.version)
        .await
        .unwrap();
    assert_eq!(discovered.state, McpConnectionState::NeedsReview);
    assert!(fixture.manager.ready_descriptors(&fixture.agent).is_empty());
    assert_eq!(
        fixture.manager.known_descriptor(&fixture.agent, &old.alias),
        Some(old.tool_descriptor())
    );
    assert!(fixture.state.read().await.agents[&fixture.agent]
        .config()
        .allows_tool(&old.alias));
    assert!(fixture
        .manager
        .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
        .await
        .is_err());
    let next = fixture
        .manager
        .review(
            &fixture.agent,
            &reviewed.record.id,
            ReviewMcpTool {
                expected_version: discovered.record.version,
                discovery_id: discovered.discovery_id.unwrap(),
                descriptor_digest: discovered.candidates[0].descriptor_digest.clone(),
            },
        )
        .await
        .unwrap();
    let new = next
        .record
        .reviewed_tools
        .iter()
        .find(|tool| tool.alias != old.alias)
        .unwrap();
    assert!(!fixture.state.read().await.agents[&fixture.agent]
        .config()
        .allows_tool(&new.alias));
    assert_eq!(
        fixture.manager.ready_descriptors(&fixture.agent),
        vec![new.tool_descriptor()]
    );
    assert_eq!(session.calls.load(Ordering::SeqCst), 0);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn actual_control_plane_restore_keeps_exact_grants_without_opening_connections() {
    let fixture = Fixture::new().await;
    let reviewed = fixture.reviewed().await;
    fixture.grant_and_stamp(&reviewed).await;
    let snapshot = fixture.state.read().await.control_plane_snapshot();
    let exact = snapshot
        .agents
        .iter()
        .find(|row| row.state.id == fixture.agent)
        .unwrap()
        .state
        .config
        .tools
        .clone();
    let alias = reviewed.record.reviewed_tools[0].alias.clone();
    let mut restarted = DaemonState::new();
    restarted
        .restore_control_plane_snapshot(snapshot.clone())
        .unwrap();
    assert_eq!(restarted.agents[&fixture.agent].config().tools, exact);
    assert_eq!(restarted.mcp_registry.snapshot(), snapshot.mcp_connections);
    assert!(restarted.mcp_publication.read().unwrap().live.is_empty());
    assert_eq!(restarted.mcp_publication.read().unwrap().known.len(), 1);
    let publication = restarted.mcp_publication.clone();
    let state = Arc::new(RwLock::new(restarted));
    let factory = Arc::new(FakeFactory::default());
    let manager = McpManager::new(
        Arc::downgrade(&state),
        Arc::new(Mutex::new(())),
        publication,
        factory.clone(),
    );
    assert_eq!(
        manager
            .get(&fixture.agent, &reviewed.record.id)
            .await
            .unwrap()
            .state,
        McpConnectionState::Disconnected
    );
    assert_eq!(
        manager.known_descriptor(&fixture.agent, &alias),
        Some(reviewed.record.reviewed_tools[0].tool_descriptor())
    );
    assert!(manager.ready_descriptors(&fixture.agent).is_empty());
    assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
    let mut invalid = snapshot;
    invalid.mcp_connections[0].agent_id = "foreign-missing-owner".into();
    assert!(state
        .write()
        .await
        .restore_control_plane_snapshot(invalid)
        .is_err());
    assert_eq!(
        state.read().await.agents[&fixture.agent].config().tools,
        exact
    );
    manager.shutdown().await;
    fixture.manager.shutdown().await;
}
