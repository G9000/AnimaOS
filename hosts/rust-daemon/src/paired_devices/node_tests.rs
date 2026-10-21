use super::node::*;
use anima_service_adapters::paired_devices::*;
use serde_json::json;

fn setup() -> (tempfile::TempDir, NodeConfig) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("selected");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("ordinary.txt"), b"node fixture oracle").unwrap();
    let config = NodeConfig {
        identity: "controlled-node".into(),
        generation: 1,
        bind: "127.0.0.1:0".parse().unwrap(),
        remote_bind_approved: false,
        root,
        certificate: fixture.path().join("cert.pem"),
        private_key: fixture.path().join("key.pem"),
        state_file: fixture.path().join("state.json"),
    };
    (fixture, config)
}
#[test]
fn paired_devices_ticket_once_token_binding_replay_restart_and_revocation() {
    let (_fixture, config) = setup();
    let (node, ticket) = NodeHost::open(config.clone()).unwrap();
    let manifest = node.manifest().clone();
    assert!(node
        .pair(&ticket, "other-node", &manifest.digest().unwrap())
        .is_err());
    let token = node
        .pair(&ticket, &manifest.identity, &manifest.digest().unwrap())
        .unwrap();
    assert!(node
        .pair(&ticket, &manifest.identity, &manifest.digest().unwrap())
        .is_err());
    let call = NodeInvocation {
        identity: manifest.identity.clone(),
        manifest_digest: manifest.digest().unwrap(),
        generation: 1,
        agent_id: "owner-agent".into(),
        run_id: "host-run".into(),
        session_id: "host-session".into(),
        tool_call_id: "host-call".into(),
        descriptor_digest: manifest.tools[0].descriptor_digest.clone(),
        deadline_unix_ms: now_ms() + 20_000,
    };
    assert!(node
        .invoke("wrong-token", "node_system_info", json!({}), &call)
        .is_err());
    let result = node
        .invoke(&token, "node_system_info", json!({}), &call)
        .unwrap();
    assert_eq!(result["os"], std::env::consts::OS);
    drop(node);
    let (node, _) = NodeHost::open(config).unwrap();
    assert!(node
        .invoke(&token, "node_system_info", json!({}), &call)
        .is_err());
    node.revoke(&token).unwrap();
    assert!(node.authenticate(&token).is_err());
}

fn invocation_for(manifest: &NodeManifest, tool: usize, call: &str) -> NodeInvocation {
    NodeInvocation {
        identity: manifest.identity.clone(),
        manifest_digest: manifest.digest().unwrap(),
        generation: manifest.generation,
        agent_id: "manifest-binding-owner".into(),
        run_id: "manifest-binding-run".into(),
        session_id: "manifest-binding-session".into(),
        tool_call_id: call.into(),
        descriptor_digest: manifest.tools[tool].descriptor_digest.clone(),
        deadline_unix_ms: now_ms() + 20_000,
    }
}

#[test]
fn paired_devices_old_bearer_cannot_forge_fresh_manifest_after_root_or_generation_change() {
    for change_root in [false, true] {
        let (_fixture, mut config) = setup();
        let (node, ticket) = NodeHost::open(config.clone()).unwrap();
        let original = node.manifest().clone();
        let token = node
            .pair(&ticket, &original.identity, &original.digest().unwrap())
            .unwrap();
        let admitted = invocation_for(&original, 0, "must-survive-invalidation");
        node.invoke(&token, "node_system_info", json!({}), &admitted)
            .unwrap();
        drop(node);
        if change_root {
            let root = config.root.parent().unwrap().join("new-owner-root");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(
                root.join("ordinary.txt"),
                b"new root must require fresh pairing",
            )
            .unwrap();
            config.root = root;
        } else {
            config.generation += 1;
        }
        let (node, new_ticket) = NodeHost::open(config.clone()).unwrap();
        let current = node.manifest().clone();
        assert_ne!(original.digest().unwrap(), current.digest().unwrap());
        // A bearer holder can fetch the public manifest and forge matching new
        // metadata. Metadata validation alone must never widen the bearer.
        let forged = invocation_for(&current, 2, "freshly-forged-current-manifest");
        assert!(
            node.invoke(
                &token,
                "node_read_file",
                json!({"path":"ordinary.txt"}),
                &forged
            )
            .is_err(),
            "old bearer gained the changed scope (root changed: {change_root})"
        );
        assert!(node.authenticate(&token).is_err());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config.state_file).unwrap()).unwrap();
        assert!(
            saved["tokenHash"].is_null(),
            "startup must durably invalidate the old bearer before issuing a new ticket"
        );
        assert!(saved["replay"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == &admitted.replay_key()));
        let replacement = node
            .pair(&new_ticket, &current.identity, &current.digest().unwrap())
            .unwrap();
        assert_ne!(replacement, token);
        let fresh = invocation_for(&current, 2, "new-owner-pairing");
        assert!(node
            .invoke(
                &replacement,
                "node_read_file",
                json!({"path":"ordinary.txt"}),
                &fresh
            )
            .is_ok());
        assert!(node.authenticate(&token).is_err());
    }
}

#[test]
fn paired_devices_legacy_token_missing_pair_manifest_binding_is_invalidated_on_restart() {
    let (_fixture, config) = setup();
    let (node, ticket) = NodeHost::open(config.clone()).unwrap();
    let manifest = node.manifest().clone();
    let token = node
        .pair(&ticket, &manifest.identity, &manifest.digest().unwrap())
        .unwrap();
    let admitted = invocation_for(&manifest, 0, "legacy-replay-must-survive");
    node.invoke(&token, "node_system_info", json!({}), &admitted)
        .unwrap();
    drop(node);
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config.state_file).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("pairManifestDigest");
    std::fs::write(&config.state_file, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let (node, _) = NodeHost::open(config.clone()).unwrap();
    assert!(
        node.authenticate(&token).is_err(),
        "missing durable credential binding must fail closed"
    );
    let forged = invocation_for(node.manifest(), 0, "legacy-fresh-current-metadata");
    assert!(node
        .invoke(&token, "node_system_info", json!({}), &forged)
        .is_err());
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config.state_file).unwrap()).unwrap();
    assert!(saved["tokenHash"].is_null());
    assert!(saved["replay"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == &admitted.replay_key()));
}

#[test]
fn paired_devices_unchanged_restart_keeps_exact_bearer_binding_and_all_replay_keys() {
    let (_fixture, config) = setup();
    let (node, ticket) = NodeHost::open(config.clone()).unwrap();
    let manifest = node.manifest().clone();
    let token = node
        .pair(&ticket, &manifest.identity, &manifest.digest().unwrap())
        .unwrap();
    let admitted = invocation_for(&manifest, 0, "unchanged-replay");
    node.invoke(&token, "node_system_info", json!({}), &admitted)
        .unwrap();
    drop(node);
    let (node, _) = NodeHost::open(config).unwrap();
    assert!(node.authenticate(&token).is_ok());
    assert!(node
        .invoke(&token, "node_system_info", json!({}), &admitted)
        .is_err());
    assert!(node
        .invoke(
            &token,
            "node_system_info",
            json!({}),
            &invocation_for(node.manifest(), 0, "unchanged-fresh-call")
        )
        .is_ok());
}
#[test]
fn paired_devices_reads_selected_ordinary_file_and_refuses_escapes_and_hardlinks() {
    let (fixture, config) = setup();
    let (node, ticket) = NodeHost::open(config.clone()).unwrap();
    let manifest = node.manifest().clone();
    let token = node
        .pair(&ticket, &manifest.identity, &manifest.digest().unwrap())
        .unwrap();
    for (index, path) in [
        "ordinary.txt",
        "../outside.txt",
        "C:/secret",
        "x:stream",
        "a\\b",
    ]
    .into_iter()
    .enumerate()
    {
        let call = NodeInvocation {
            identity: manifest.identity.clone(),
            manifest_digest: manifest.digest().unwrap(),
            generation: 1,
            agent_id: "owner".into(),
            run_id: "run".into(),
            session_id: "session".into(),
            tool_call_id: format!("call-{index}"),
            descriptor_digest: manifest.tools[2].descriptor_digest.clone(),
            deadline_unix_ms: now_ms() + 20_000,
        };
        let result = node.invoke(&token, "node_read_file", json!({"path":path}), &call);
        if index == 0 {
            assert_eq!(result.unwrap()["text"], "node fixture oracle");
        } else {
            assert!(result.is_err(), "{path}");
        }
    }
    let outside = fixture.path().join("outside.txt");
    std::fs::write(&outside, b"private outside fixture").unwrap();
    std::fs::hard_link(&outside, config.root.join("hard.txt")).unwrap();
    let call = NodeInvocation {
        identity: manifest.identity.clone(),
        manifest_digest: manifest.digest().unwrap(),
        generation: 1,
        agent_id: "owner".into(),
        run_id: "run".into(),
        session_id: "session".into(),
        tool_call_id: "hardlink-call".into(),
        descriptor_digest: manifest.tools[2].descriptor_digest.clone(),
        deadline_unix_ms: now_ms() + 20_000,
    };
    assert!(node
        .invoke(&token, "node_read_file", json!({"path":"hard.txt"}), &call)
        .is_err());
}
#[test]
fn paired_devices_config_requires_explicit_remote_bind_and_private_state_outside_root() {
    let (_fixture, mut config) = setup();
    config.bind = "0.0.0.0:8443".parse().unwrap();
    assert!(NodeHost::open(config.clone()).is_err());
    config.bind = "127.0.0.1:0".parse().unwrap();
    config.state_file = config.root.join("private-state.json");
    assert!(NodeHost::open(config).is_err());
}

struct TlsPeer {
    _fixture: tempfile::TempDir,
    config: NodeConfig,
    node: std::sync::Arc<NodeHost>,
    ticket: String,
    endpoint: String,
    client: reqwest::Client,
    stop: tokio_util::sync::CancellationToken,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl TlsPeer {
    async fn start(identity: &str) -> Self {
        Self::start_with_hold(identity, None).await
    }
    async fn start_with_hold(
        identity: &str,
        hold: Option<std::sync::Arc<TlsDispatchHold>>,
    ) -> Self {
        let (fixture, mut config) = setup();
        config.identity = identity.into();
        let cert =
            include_bytes!("../../../../packages/core-rust/browser-worker/fixtures/m63/server.pem");
        let key = include_bytes!(
            "../../../../packages/core-rust/browser-worker/fixtures/m63/server-key.pem"
        );
        let ca =
            include_bytes!("../../../../packages/core-rust/browser-worker/fixtures/m63/ca.pem");
        std::fs::write(&config.certificate, cert).unwrap();
        std::fs::write(&config.private_key, key).unwrap();
        let (node, ticket) = NodeHost::open(config.clone()).unwrap();
        let tcp = tokio::net::TcpListener::bind(config.bind).await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let endpoint = format!("https://m63-browser.anima.tools:{}/mcp", addr.port());
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve("m63-browser.anima.tools", addr)
            .add_root_certificate(reqwest::Certificate::from_pem(ca).unwrap())
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let stop = tokio_util::sync::CancellationToken::new();
        let shutdown = stop.clone();
        let mut router = node.router();
        if let Some(hold) = hold {
            router = router.layer(axum::middleware::from_fn_with_state(hold, hold_node_reply));
        }
        let acceptor = tls_acceptor(cert, key).unwrap();
        let server = tokio::spawn(async move {
            axum::serve(TlsListener::new(tcp, acceptor), router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Self {
            _fixture: fixture,
            config,
            node,
            ticket,
            endpoint,
            client,
            stop,
            server,
        }
    }
    async fn offline(&mut self) {
        self.stop.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    async fn finish(mut self) {
        self.offline().await;
    }
}

/// Test boundary after the production node handler has executed a real tool,
/// before its TLS response reaches the normal native MCP client.
struct TlsDispatchHold {
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
    invocations: std::sync::Mutex<Vec<serde_json::Value>>,
    replies: std::sync::Mutex<Vec<serde_json::Value>>,
}
impl TlsDispatchHold {
    fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            invocations: std::sync::Mutex::new(Vec::new()),
            replies: std::sync::Mutex::new(Vec::new()),
        })
    }
}
async fn hold_node_reply(
    axum::extract::State(hold): axum::extract::State<std::sync::Arc<TlsDispatchHold>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if request.method() != axum::http::Method::POST || request.uri().path() != "/mcp" {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_NODE_WIRE_BYTES)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let is_call = value["method"] == "tools/call";
    let response = next
        .run(axum::extract::Request::from_parts(
            parts,
            axum::body::Body::from(bytes),
        ))
        .await;
    if is_call {
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, MAX_NODE_WIRE_BYTES)
            .await
            .unwrap();
        hold.replies
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&bytes).unwrap());
        hold.invocations.lock().unwrap().push(value);
        hold.entered.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(15), hold.release.acquire())
            .await
            .expect("test must release its controlled node response")
            .unwrap()
            .forget();
        return axum::response::Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    response
}

#[tokio::test]
async fn paired_devices_real_tls_cancelled_dispatch_keeps_unknown_evidence_and_revokes_offline() {
    use crate::agent_runs::{
        action_receipts::RunActionJournal,
        test_support::{decision, gated_context, ledger_run, pending_approvals, tool_input},
        AgentRunCoordinator,
    };
    use crate::{
        mcp::{
            credentials::{InMemoryMcpCredentialStore, McpBearerToken, McpCredentialStore},
            transport::DefaultMcpTransportFactory,
            McpGrantInput, McpManager, ReviewMcpTool,
        },
        paired_devices::{PairDeviceInput, PairedDevicesService},
        state::DaemonState,
    };
    use anima_core::{CancelSignal, DataValue, TaskStatus, ToolCall};
    use std::{collections::BTreeMap, sync::Arc, time::Duration};
    use tokio::sync::{Mutex, RwLock, Semaphore};
    let hold = TlsDispatchHold::new();
    let mut peer = TlsPeer::start_with_hold("controlled-cancel-peer", Some(hold.clone())).await;
    let mut daemon = DaemonState::new();
    let agent = daemon
        .create_agent(crate::sessions::test_support::agent_config(
            "Cancellation owner",
        ))
        .unwrap()
        .state
        .id;
    let owner_store = peer._fixture.path().join("owner-cancel-control.json");
    daemon.control_plane_store = Some(crate::control_plane_store::ControlPlaneStoreConfig::Json(
        owner_store.clone(),
    ));
    let publication = daemon.mcp_publication.clone();
    let state = Arc::new(RwLock::new(daemon));
    let credentials = Arc::new(InMemoryMcpCredentialStore::default());
    let manager = McpManager::with_credentials(
        Arc::downgrade(&state),
        Arc::new(Mutex::new(())),
        publication,
        Arc::new(DefaultMcpTransportFactory::with_http_client(
            peer.client.clone(),
        )),
        credentials.clone(),
    );
    state.write().await.mcp_manager = Some(manager.clone());
    let service = PairedDevicesService::new(Arc::downgrade(&state), manager.clone())
        .with_client(peer.client.clone());
    let paired = service
        .pair(
            &agent,
            PairDeviceInput {
                label: "Cancellation fixture".into(),
                endpoint: peer.endpoint.clone(),
                expected_identity: peer.node.manifest().identity.clone(),
                expected_manifest_digest: peer.node.manifest().digest().unwrap(),
                ticket: McpBearerToken::parse(peer.ticket.clone()).unwrap(),
            },
        )
        .await
        .unwrap();
    assert!(paired.discovery_ready);
    let device = paired.device;
    let mut view = manager
        .get(&agent, &device.mcp_connection_id)
        .await
        .unwrap();
    let descriptor = view
        .candidates
        .iter()
        .find(|candidate| candidate.remote_name == "node_read_file")
        .unwrap()
        .clone();
    view = manager
        .review(
            &agent,
            &view.record.id,
            ReviewMcpTool {
                expected_version: view.record.version,
                discovery_id: view.discovery_id.clone().unwrap(),
                descriptor_digest: descriptor.descriptor_digest,
            },
        )
        .await
        .unwrap();
    let tool = view.record.reviewed_tools[0].clone();
    view = manager
        .set_grant(
            &agent,
            &view.record.id,
            &tool.alias,
            McpGrantInput {
                expected_version: view.record.version,
                descriptor_digest: tool.descriptor.descriptor_digest.clone(),
                granted: true,
            },
        )
        .await
        .unwrap()
        .connection;
    let credential_ref = view.record.bearer_credential_ref.clone().unwrap();
    let token = manager
        .paired_bearer(&agent, &view.record.id)
        .await
        .unwrap()
        .unwrap();
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)))
        .with_mcp(manager.clone());
    let run = ledger_run(&coordinator, &agent, "chat:node-cancel").await;
    let cancel = CancelSignal::new();
    let journal = RunActionJournal::new(
        &state,
        manager.control_plane_transactions(),
        run.clone(),
        cancel.clone(),
        None,
    );
    let context = gated_context(&coordinator, &run, cancel.clone())
        .await
        .with_mcp(Some(manager.clone()))
        .with_action_journal(journal);
    let agent_state = state.read().await.agents[&agent].state();
    let call = ToolCall {
        id: "cancelled-real-node-read".into(),
        name: tool.alias.clone(),
        args: BTreeMap::from([("path".into(), DataValue::String("ordinary.txt".into()))]),
    };
    let stale_stamp = manager.capture_dispatch(&agent_state, &call, &run).unwrap();
    let original_generation = stale_stamp.wire_generation;
    let message = tool_input(&agent, &run.session_id);
    let task = tokio::spawn(async move { context.execute_tool(agent_state, message, call).await });
    let approval = pending_approvals(&coordinator, 1).await.remove(0);
    coordinator
        .decide_approval(
            &approval.id,
            decision(
                crate::approvals::ApprovalDecisionKind::AllowOnce,
                approval.revision,
            ),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), hold.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let invocation: NodeInvocation = serde_json::from_value(
        hold.invocations.lock().unwrap()[0]["params"]["_meta"][NODE_META_KEY].clone(),
    )
    .unwrap();
    assert_eq!(invocation.run_id, run.run_id);
    assert_eq!(invocation.tool_call_id, "cancelled-real-node-read");
    assert!(
        hold.replies.lock().unwrap()[0]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("node fixture oracle")
    );
    let node_state = std::fs::read_to_string(&peer.config.state_file).unwrap();
    assert!(
        node_state.contains(&invocation.replay_key()),
        "real remote read was admitted durably before its response was withheld"
    );
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, TaskStatus::Error);
    assert!(
        result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("outcome is unknown"),
        "{:?}",
        result.error
    );
    let receipt = state
        .read()
        .await
        .runs
        .get(&run.run_id)
        .unwrap()
        .action_receipts[0]
        .clone();
    assert_eq!(receipt.status, crate::runs::ActionStatus::Failed);
    assert!(receipt
        .result_preview
        .as_deref()
        .unwrap_or("")
        .contains("outcome is unknown"));
    assert!(receipt.finished_at_ms.is_some());
    let cancelled_view = manager.get(&agent, &view.record.id).await.unwrap();
    assert_ne!(cancelled_view.wire_generation, original_generation);
    assert!(manager.ready_descriptors(&agent).is_empty());
    assert!(manager
        .invoke(
            stale_stamp,
            json!({"path":"ordinary.txt"}),
            CancelSignal::new()
        )
        .await
        .is_err());
    assert_eq!(
        hold.invocations.lock().unwrap().len(),
        1,
        "stale generation cannot reach the TLS peer again"
    );
    // Cancellation cannot undo the completed remote read, revoke its token, or
    // establish a successful result whose TLS response was never received.
    assert!(peer.node.authenticate(token.expose()).is_ok());
    hold.release.add_permits(1);
    let endpoint = peer.endpoint.clone();
    peer.offline().await;
    let revoked = service.revoke(&agent, &device.id).await.unwrap();
    assert!(revoked.revoked);
    assert!(
        !revoked.remote_revocation_confirmed,
        "offline node invalidation must remain unconfirmed"
    );
    assert!(manager
        .get(&agent, &device.mcp_connection_id)
        .await
        .is_err());
    assert!(manager.known_descriptor(&agent, &tool.alias).is_none());
    assert!(manager.ready_descriptors(&agent).is_empty());
    assert!(credentials
        .load(&device.mcp_connection_id, &endpoint, &credential_ref)
        .await
        .unwrap()
        .is_none());
    assert!(!state.read().await.agents[&agent]
        .config()
        .allows_tool(&tool.alias));
    let saved = std::fs::read_to_string(owner_store).unwrap();
    assert!(!saved.contains(token.expose()));
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert!(saved["pairedDevices"]["records"][0]["revoked"]
        .as_bool()
        .unwrap());
    assert_eq!(
        saved["pairedDevices"]["records"][0]["remoteRevocationConfirmed"],
        false
    );
    // Offline revocation has not changed the node's durable bearer hash.
    assert!(peer.node.authenticate(token.expose()).is_ok());
    manager.shutdown().await;
}

#[tokio::test]
async fn paired_devices_verified_two_tls_peers_pair_review_grant_real_tools_restart_and_revoke() {
    use crate::agent_runs::{
        action_receipts::RunActionJournal,
        test_support::{decision, gated_context, ledger_run, pending_approvals, tool_input},
        AgentRunCoordinator,
    };
    use crate::{
        mcp::{
            credentials::{InMemoryMcpCredentialStore, McpBearerToken},
            transport::DefaultMcpTransportFactory,
            McpGrantInput, McpManager, ReviewMcpTool,
        },
        paired_devices::{PairDeviceInput, PairedDevicesService},
        state::DaemonState,
    };
    use anima_core::{CancelSignal, DataValue, TaskStatus, ToolCall};
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::sync::{Mutex, RwLock, Semaphore};
    let a = TlsPeer::start("controlled-peer-a").await;
    let b = TlsPeer::start("controlled-peer-b").await;
    let mut daemon = DaemonState::new();
    let agent = daemon
        .create_agent(crate::sessions::test_support::agent_config("Node owner"))
        .unwrap()
        .state
        .id;
    daemon.control_plane_store = Some(crate::control_plane_store::ControlPlaneStoreConfig::Json(
        a._fixture.path().join("owner-control.json"),
    ));
    let publication = daemon.mcp_publication.clone();
    let state = Arc::new(RwLock::new(daemon));
    let manager = McpManager::with_credentials(
        Arc::downgrade(&state),
        Arc::new(Mutex::new(())),
        publication,
        Arc::new(DefaultMcpTransportFactory::with_http_client(
            a.client.clone(),
        )),
        Arc::new(InMemoryMcpCredentialStore::default()),
    );
    state.write().await.mcp_manager = Some(manager.clone());
    let service = PairedDevicesService::new(Arc::downgrade(&state), manager.clone())
        .with_client(a.client.clone());
    let input = |endpoint: String, identity: &str, digest: String, ticket: &str| PairDeviceInput {
        label: "Controlled node".into(),
        endpoint,
        expected_identity: identity.into(),
        expected_manifest_digest: digest,
        ticket: McpBearerToken::parse(ticket.into()).unwrap(),
    };
    let digest = a.node.manifest().digest().unwrap();
    assert!(service
        .pair(
            &agent,
            input(
                a.endpoint.clone(),
                "foreign-identity",
                digest.clone(),
                &a.ticket
            )
        )
        .await
        .is_err());
    assert!(service
        .pair(
            &agent,
            input(
                a.endpoint.clone(),
                "controlled-peer-a",
                b.node.manifest().digest().unwrap(),
                &a.ticket
            )
        )
        .await
        .is_err());
    assert!(service
        .pair(
            &agent,
            input(
                b.endpoint.clone(),
                "controlled-peer-a",
                digest.clone(),
                &a.ticket
            )
        )
        .await
        .is_err());
    let outcome = service
        .pair(
            &agent,
            input(a.endpoint.clone(), "controlled-peer-a", digest, &a.ticket),
        )
        .await
        .unwrap();
    assert!(outcome.discovery_ready);
    let device = outcome.device;
    assert!(service
        .pair(
            &agent,
            input(
                a.endpoint.clone(),
                "controlled-peer-a",
                a.node.manifest().digest().unwrap(),
                &a.ticket
            )
        )
        .await
        .is_err());
    let mut view = manager
        .get(&agent, &device.mcp_connection_id)
        .await
        .unwrap();
    assert!(view.record.reviewed_tools.is_empty());
    assert_eq!(view.candidates.len(), 3);
    for candidate in view.candidates.clone() {
        view = manager
            .review(
                &agent,
                &view.record.id,
                ReviewMcpTool {
                    expected_version: view.record.version,
                    discovery_id: view.discovery_id.clone().unwrap(),
                    descriptor_digest: candidate.descriptor_digest,
                },
            )
            .await
            .unwrap();
    }
    assert!(state.read().await.agents[&agent]
        .config()
        .tools
        .as_ref()
        .is_none_or(|tools| tools.iter().all(|tool| !tool.name.starts_with("mcp_"))));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)))
        .with_mcp(manager.clone());
    let run = ledger_run(&coordinator, &agent, "chat:controlled-node").await;
    let ungranted = view.record.reviewed_tools[0].tool_descriptor();
    assert!(manager
        .capture_dispatch(
            &state.read().await.agents[&agent].state(),
            &ToolCall {
                id: "no-grant".into(),
                name: ungranted.name,
                args: BTreeMap::new()
            },
            &run
        )
        .is_err());
    for tool in view.record.reviewed_tools.clone() {
        let granted = manager
            .set_grant(
                &agent,
                &view.record.id,
                &tool.alias,
                McpGrantInput {
                    expected_version: view.record.version,
                    descriptor_digest: tool.descriptor.descriptor_digest,
                    granted: true,
                },
            )
            .await
            .unwrap();
        view = granted.connection;
    }
    let token = manager
        .paired_bearer(&agent, &view.record.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        a.client
            .post(a.endpoint.clone())
            .bearer_auth("wrong-token")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
            .unwrap()
            .status()
            == reqwest::StatusCode::FORBIDDEN
    );
    for (index, tool) in view.record.reviewed_tools.clone().into_iter().enumerate() {
        let cancel = CancelSignal::new();
        let journal = RunActionJournal::new(
            &state,
            manager.control_plane_transactions(),
            run.clone(),
            cancel.clone(),
            None,
        );
        let context = gated_context(&coordinator, &run, cancel)
            .await
            .with_mcp(Some(manager.clone()))
            .with_action_journal(journal);
        let agent_state = state.read().await.agents[&agent].state();
        let args = if tool.descriptor.remote_name == "node_read_file" {
            BTreeMap::from([("path".into(), DataValue::String("ordinary.txt".into()))])
        } else {
            BTreeMap::new()
        };
        let call = ToolCall {
            id: format!("controlled-call-{index}"),
            name: tool.alias,
            args,
        };
        let message = tool_input(&agent, &run.session_id);
        let task =
            tokio::spawn(async move { context.execute_tool(agent_state, message, call).await });
        let approval = pending_approvals(&coordinator, 1).await.remove(0);
        coordinator
            .decide_approval(
                &approval.id,
                decision(
                    crate::approvals::ApprovalDecisionKind::AllowOnce,
                    approval.revision,
                ),
            )
            .await
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.status, TaskStatus::Success, "{:?}", result.error);
        let text = result.data.unwrap().text;
        match tool.descriptor.remote_name.as_str() {
            "node_system_info" => assert!(text.contains(std::env::consts::OS)),
            "node_list_root" => assert!(text.contains("ordinary.txt")),
            "node_read_file" => assert!(text.contains("node fixture oracle")),
            _ => unreachable!(),
        }
    }
    assert_eq!(
        state
            .read()
            .await
            .runs
            .get(&run.run_id)
            .unwrap()
            .action_receipts
            .len(),
        3
    );
    // Tear down the serving host, reload only its durable state, and prove a
    // replay remains rejected. No live replay map is carried into the restart.
    let replay = NodeInvocation {
        identity: a.node.manifest().identity.clone(),
        manifest_digest: a.node.manifest().digest().unwrap(),
        generation: 1,
        agent_id: agent.clone(),
        run_id: run.run_id.clone(),
        session_id: run.session_id.clone(),
        tool_call_id: "controlled-call-0".into(),
        descriptor_digest: view.record.reviewed_tools[0]
            .descriptor
            .descriptor_digest
            .clone(),
        deadline_unix_ms: now_ms() + 20_000,
    };
    let mut changed = a.config.clone();
    changed.generation = 2;
    let (widened, _) = NodeHost::open(changed).unwrap();
    assert!(widened
        .invoke(
            token.expose(),
            &view.record.reviewed_tools[0].descriptor.remote_name,
            json!({}),
            &replay
        )
        .is_err());
    drop(widened);
    let revoked = service.revoke(&agent, &device.id).await.unwrap();
    assert!(revoked.remote_revocation_confirmed);
    assert!(manager
        .get(&agent, &device.mcp_connection_id)
        .await
        .is_err());
    assert!(a.node.authenticate(token.expose()).is_err());
    assert!(state.read().await.agents[&agent]
        .config()
        .tools
        .as_ref()
        .is_none_or(|tools| tools.iter().all(|tool| !tool.name.starts_with("mcp_"))));
    let saved = std::fs::read_to_string(a._fixture.path().join("owner-control.json")).unwrap();
    assert!(!saved.contains(token.expose()));
    manager.shutdown().await;
    a.finish().await;
    b.finish().await;
}

struct PeerProcess(std::process::Child);
impl Drop for PeerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn process_peer(config: &std::path::Path) -> (PeerProcess, reqwest::Client, String, String) {
    use std::io::BufRead;
    let harness = std::env::current_exe().unwrap();
    let executable = harness
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join(if cfg!(windows) {
            "paired_node_peer.exe"
        } else {
            "paired_node_peer"
        });
    assert!(
        executable.is_file(),
        "build paired_node_peer example before paired_devices tests"
    );
    let mut child = PeerProcess(
        std::process::Command::new(executable)
            .arg(config)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let lines = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            std::io::BufReader::new(stdout)
                .lines()
                .take(4)
                .collect::<Result<Vec<_>, _>>()
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        lines.len(),
        4,
        "production node must report bounded startup information"
    );
    let addr: std::net::SocketAddr = lines[2]
        .strip_prefix("TLS address: ")
        .unwrap()
        .parse()
        .unwrap();
    let ticket = lines[3]
        .strip_prefix("One-use pair ticket (expires in 5 minutes): ")
        .unwrap()
        .to_owned();
    let ca = include_bytes!("../../../../packages/core-rust/browser-worker/fixtures/m63/ca.pem");
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve("m63-browser.anima.tools", addr)
        .add_root_certificate(reqwest::Certificate::from_pem(ca).unwrap())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    (
        child,
        client,
        format!("https://m63-browser.anima.tools:{}/mcp", addr.port()),
        ticket,
    )
}

#[tokio::test]
async fn paired_devices_actual_process_restart_preserves_replay_and_reapproval_fence() {
    let (fixture, config) = setup();
    std::fs::write(
        &config.certificate,
        include_bytes!("../../../../packages/core-rust/browser-worker/fixtures/m63/server.pem"),
    )
    .unwrap();
    std::fs::write(
        &config.private_key,
        include_bytes!("../../../../packages/core-rust/browser-worker/fixtures/m63/server-key.pem"),
    )
    .unwrap();
    let config_path = fixture.path().join("node-config.json");
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (first, client, endpoint, ticket) = process_peer(&config_path).await;
    let base = endpoint.trim_end_matches("/mcp");
    let manifest: serde_json::Value = client
        .get(format!("{base}/manifest"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let manifest: NodeManifest = serde_json::from_value(manifest["manifest"].clone()).unwrap();
    let paired:serde_json::Value=client.post(format!("{base}/pair")).json(&json!({"ticket":ticket,"expectedIdentity":manifest.identity,"expectedManifestDigest":manifest.digest().unwrap()})).send().await.unwrap().json().await.unwrap();
    let token = paired["bearerToken"].as_str().unwrap().to_owned();
    let mut call = NodeInvocation {
        identity: manifest.identity.clone(),
        manifest_digest: manifest.digest().unwrap(),
        generation: 1,
        agent_id: "host-owner".into(),
        run_id: "host-run".into(),
        session_id: "host-session".into(),
        tool_call_id: "host-tool-call".into(),
        descriptor_digest: manifest.tools[0].descriptor_digest.clone(),
        deadline_unix_ms: now_ms() + 25_000,
    };
    let request = |call: &NodeInvocation| json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"node_system_info","arguments":{},"_meta":{"animaNode":call}}});
    let result: serde_json::Value = client
        .post(&endpoint)
        .bearer_auth(&token)
        .json(&request(&call))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(result.get("result").is_some(), "{result}");
    drop(first);
    let (second, client, endpoint, new_ticket) = process_peer(&config_path).await;
    assert_ne!(ticket, new_ticket);
    let replay: serde_json::Value = client
        .post(&endpoint)
        .bearer_auth(&token)
        .json(&request(&call))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        replay.get("error").is_some(),
        "durable replay must survive actual process restart"
    );
    call.tool_call_id = "fresh-host-call".into();
    call.deadline_unix_ms = now_ms() + 25_000;
    let fresh: serde_json::Value = client
        .post(&endpoint)
        .bearer_auth(&token)
        .json(&request(&call))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(fresh.get("result").is_some());
    drop(second);
    let mut changed = config;
    changed.generation = 2;
    std::fs::write(&config_path, serde_json::to_vec(&changed).unwrap()).unwrap();
    let (third, client, endpoint, _) = process_peer(&config_path).await;
    call.tool_call_id = "fresh-but-unreviewed-generation".into();
    call.deadline_unix_ms = now_ms() + 25_000;
    let widened = client
        .post(&endpoint)
        .bearer_auth(&token)
        .json(&request(&call))
        .send()
        .await
        .unwrap();
    assert_eq!(
        widened.status(),
        reqwest::StatusCode::FORBIDDEN,
        "changed startup scope invalidates the bearer before MCP dispatch"
    );
    let persisted = std::fs::read_to_string(&changed.state_file).unwrap();
    assert!(!persisted.contains(&token));
    drop(third);
}
