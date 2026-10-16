//! Recording-port evidence is metadata-only, never physical-child proof.
use super::*;
use crate::agent_runs::{
    test_support::{companion_config, ledger_run, ScriptedModel},
    AgentRunCoordinator,
};
use crate::app::SharedDaemonState;
use crate::attachments::{files::save_upload, AttachmentKind, AttachmentRecord};
use crate::control_plane_store::{ControlPlaneStoreConfig, WorkspaceConfig};
use crate::runs::{RunLink, RunSource, RunStatus};
use crate::state::DaemonState;
use anima_core::browser::{
    BrowserControl, BrowserEffect, BrowserError, BrowserErrorCode, BrowserLimits, BrowserMethod,
    BrowserPort, BrowserRoute, BrowserScope, BrowserSession,
};
use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};

#[derive(Default)]
pub(crate) struct RecordingPort {
    pub(crate) opens: AtomicUsize,
}
#[async_trait]
impl BrowserPort for RecordingPort {
    async fn open(
        &self,
        _: BrowserScope,
        _: BrowserControl,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Err(BrowserError {
            code: BrowserErrorCode::Unavailable,
            effect: BrowserEffect::NotDispatched,
        })
    }
}
pub(crate) fn policies(scope_ms: u32) -> InstalledBrowserPolicies {
    InstalledBrowserPolicies::new(
        BrowserInstallation {
            id: "controlled-installation".into(),
            digest: "a".repeat(64),
        },
        vec![InstalledBrowserPolicy {
            id: "controlled-operator-policy".into(),
            template: BrowserScope {
                id: "operator-template".into(),
                revision: 1,
                origin: "http://127.0.0.1:43220".into(),
                public_ipv4_pin: None,
                routes: vec![BrowserRoute {
                    id: "page".into(),
                    method: BrowserMethod::Get,
                    path: "/".into(),
                }],
                input_ids: vec![],
                limits: BrowserLimits {
                    actions: 40,
                    requests: 80,
                    scope_ms,
                    frame_bytes: 64 * 1024,
                    wire_bytes: 1024 * 1024,
                    observation_nodes: 128,
                    observation_text_bytes: 8 * 1024,
                    input_bytes: 16 * 1024,
                    output_bytes: 16 * 1024,
                    screenshot_bytes: 256 * 1024,
                },
            },
        }],
    )
    .unwrap()
}

fn public_policy() -> InstalledBrowserPolicy {
    let mut baseline = policies(120_000);
    let mut policy = baseline
        .policies
        .remove("controlled-operator-policy")
        .unwrap();
    policy.id = "public-operator-policy".into();
    let mut value = serde_json::to_value(&policy.template).unwrap();
    value["origin"] = serde_json::json!("https://m63-browser.anima.tools");
    value["publicIpv4Pin"] = serde_json::json!("93.184.216.34");
    value["routes"] = serde_json::json!([
        {"id": "page", "method": "GET", "path": "/"},
        {"id": "article", "method": "GET", "path": "/article"}
    ]);
    policy.template = serde_json::from_value(value)
        .expect("operator public pin must deserialize as scope admission data");
    policy
}
pub(crate) fn fixture_policies(
    installation: &InstalledBrowserPolicies,
    origin: &str,
    routes: Vec<BrowserRoute>,
) -> InstalledBrowserPolicies {
    let mut table = policies(120_000);
    let mut policy = table.policies.remove("controlled-operator-policy").unwrap();
    policy.template.origin = origin.into();
    policy.template.routes = routes;
    InstalledBrowserPolicies::new(
        installation
            .installation
            .clone()
            .expect("verified configured installation"),
        vec![policy],
    )
    .unwrap()
}
pub(crate) struct Fixture {
    workspace: tempfile::TempDir,
    pub(crate) state: SharedDaemonState,
    pub(crate) service: BrowserService,
    pub(crate) port: Arc<RecordingPort>,
    pub(crate) coordinator: AgentRunCoordinator,
    pub(crate) run: RunLink,
    model: Arc<ScriptedModel>,
}
impl Fixture {
    async fn new(scope_ms: u32) -> Self {
        let model = ScriptedModel::new(vec![]);
        Self::with_model(scope_ms, model).await
    }
    pub(crate) async fn with_model(scope_ms: u32, model: Arc<ScriptedModel>) -> Self {
        Self::with_tools_model(scope_ms, model, &["calculate"]).await
    }
    pub(crate) async fn with_tools_model(
        scope_ms: u32,
        model: Arc<ScriptedModel>,
        tools: &[&str],
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let mut daemon = DaemonState::with_model_adapter(model.clone());
        daemon.workspace = Some(WorkspaceConfig {
            root_path: workspace.path().into(),
            company_name: "M46 controlled workspace".into(),
            mission: "Metadata admission".into(),
            values: vec![],
        });
        daemon.control_plane_store = Some(ControlPlaneStoreConfig::Json(
            workspace.path().join("control.json"),
        ));
        let mut config = companion_config("M46 ordinary owner");
        config.tools = Some(
            daemon
                .tool_registry
                .resolve_descriptors(tools.iter().copied())
                .unwrap(),
        );
        let agent = daemon.create_agent(config).unwrap().state.id;
        let state = Arc::new(RwLock::new(daemon));
        let coordinator = crate::app::coordinator_with_mcp(
            &state,
            AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1))),
        );
        let port = Arc::new(RecordingPort::default());
        let service = BrowserService::new(
            &state,
            coordinator.control_plane_transactions(),
            policies(scope_ms),
            port.clone(),
        );
        state.write().await.browser_service = Some(service.clone());
        let coordinator = coordinator.with_browser(service.clone());
        let run = ledger_run(&coordinator, &agent, "chat:m46-domain").await;
        state.read().await.live.runs().register(&run.run_id);
        Self {
            workspace,
            state,
            service,
            port,
            coordinator,
            run,
            model,
        }
    }
    fn input(enabled: bool, version: Option<u64>) -> SaveBrowserScope {
        SaveBrowserScope {
            policy_id: "controlled-operator-policy".into(),
            selected_input_ids: vec![],
            enabled,
            expected_version: version,
        }
    }
    async fn save(
        &self,
        enabled: bool,
        version: Option<u64>,
    ) -> Result<BrowserScopeView, BrowserServiceError> {
        self.service
            .save(
                &self.run.agent_id,
                &self.run.run_id,
                Self::input(enabled, version),
            )
            .await
    }
    pub(crate) async fn attachment(
        &self,
        id: &str,
        session: &str,
        bytes: &[u8],
    ) -> AttachmentRecord {
        let saved = save_upload(
            self.workspace.path(),
            "2026-10-14",
            &format!("{id}.csv"),
            bytes,
        )
        .unwrap();
        let row = AttachmentRecord {
            id: id.into(),
            agent_id: self.run.agent_id.clone(),
            session_id: session.into(),
            name: format!("{id}.csv"),
            mime: "text/csv".into(),
            size: bytes.len() as u64,
            kind: AttachmentKind::Text,
            path: saved.path,
            created_at_ms: 1,
            workspace_root: saved.workspace_root,
            sha256: saved.sha256,
        };
        let store = self.state.read().await.history.store();
        store
            .upsert_attachments(std::slice::from_ref(&row))
            .await
            .unwrap();
        row
    }
    fn zero_dispatch(&self) {
        assert_eq!(self.port.opens.load(Ordering::SeqCst), 0);
        assert!(self.model.requests().is_empty());
    }
}

#[tokio::test]
async fn m46_metadata_cas_fixed_expiry_stop_and_canonical_grants_never_open_port() {
    let f = Fixture::new(120_000).await;
    let before = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    assert!(f
        .service
        .get(&f.run.agent_id, &f.run.run_id)
        .await
        .unwrap()
        .is_none());
    let first = f.save(false, None).await.unwrap();
    assert_eq!(first.tool_names, vec!["calculate"]);
    assert!(!first.physical_lease_held);
    assert_eq!(f.save(true, None).await, Err(BrowserServiceError::Conflict));
    let enabled = f.save(true, Some(first.version)).await.unwrap();
    assert_eq!(enabled.id, first.id);
    assert_eq!(enabled.expires_at_ms, first.expires_at_ms);
    assert_eq!(enabled.version, first.version + 1);
    assert_eq!(enabled.state, BrowserScopeState::Scoped);
    assert_eq!(
        f.service.policies(&f.run.agent_id).await.unwrap()[0].worker_available,
        false
    );
    assert_eq!(
        f.service.list(&f.run.agent_id).await.unwrap(),
        vec![enabled.clone()]
    );
    assert_eq!(
        f.service
            .stop(&f.run.agent_id, &f.run.run_id, first.version)
            .await,
        Err(BrowserServiceError::Conflict)
    );
    let closed = f
        .service
        .stop(&f.run.agent_id, &f.run.run_id, enabled.version)
        .await
        .unwrap();
    assert_eq!(closed.state, BrowserScopeState::Closed);
    assert!(!closed.enabled);
    assert_eq!(
        f.service
            .stop(&f.run.agent_id, &f.run.run_id, closed.version)
            .await
            .unwrap(),
        closed
    );
    assert_eq!(
        f.save(true, Some(closed.version)).await,
        Err(BrowserServiceError::Conflict)
    );
    assert_eq!(
        serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
    f.zero_dispatch();
}

#[tokio::test]
async fn m46_only_live_owned_ordinary_chat_runs_accept_scopes() {
    let f = Fixture::new(120_000).await;
    assert_eq!(
        f.service
            .save(&f.run.agent_id, "missing-run", Fixture::input(false, None))
            .await,
        Err(BrowserServiceError::NotFound)
    );
    let other = f
        .state
        .write()
        .await
        .create_agent(companion_config("Unrelated owner"))
        .unwrap()
        .state
        .id;
    assert_eq!(
        f.service
            .save(&other, &f.run.run_id, Fixture::input(false, None))
            .await,
        Err(BrowserServiceError::NotFound)
    );
    for source in [
        RunSource::Delegation,
        RunSource::Peer,
        RunSource::Schedule,
        RunSource::Job,
        RunSource::Telegram,
        RunSource::Discord,
    ] {
        f.state
            .write()
            .await
            .runs
            .get_mut(&f.run.run_id)
            .unwrap()
            .source = source;
        assert_eq!(
            f.save(false, None).await,
            Err(BrowserServiceError::Conflict)
        );
    }
    {
        let mut state = f.state.write().await;
        let run = state.runs.get_mut(&f.run.run_id).unwrap();
        run.source = RunSource::Web;
        run.status = RunStatus::Queued;
    }
    assert_eq!(
        f.save(false, None).await,
        Err(BrowserServiceError::Conflict)
    );
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .status = RunStatus::Running;
    f.state.read().await.live.runs().remove(&f.run.run_id);
    assert_eq!(
        f.save(false, None).await,
        Err(BrowserServiceError::Conflict)
    );
    f.state.read().await.live.runs().register(&f.run.run_id);
    f.state
        .write()
        .await
        .sessions
        .get_mut(&f.run.agent_id, &f.run.session_id)
        .unwrap()
        .archived = true;
    assert_eq!(
        f.save(false, None).await,
        Err(BrowserServiceError::Conflict)
    );
    f.state
        .write()
        .await
        .sessions
        .get_mut(&f.run.agent_id, &f.run.session_id)
        .unwrap()
        .archived = false;
    let mut helper = companion_config("M46 excluded helper");
    helper.settings.as_mut().unwrap().additional.insert(
        "workspaceRole".into(),
        anima_core::DataValue::String("helper".into()),
    );
    let helper_id = f.state.write().await.create_agent(helper).unwrap().state.id;
    let helper_run = ledger_run(&f.coordinator, &helper_id, "chat:m46-helper").await;
    f.state
        .read()
        .await
        .live
        .runs()
        .register(&helper_run.run_id);
    assert_eq!(
        f.service
            .save(&helper_id, &helper_run.run_id, Fixture::input(false, None))
            .await,
        Err(BrowserServiceError::NotFound)
    );
    f.state.write().await.workspace = None;
    assert_eq!(
        f.save(false, None).await,
        Err(BrowserServiceError::Conflict)
    );
    f.zero_dispatch();
}

#[tokio::test]
async fn m46_expiry_is_a_tombstone_and_run_cancel_never_reopens_scope() {
    let f = Fixture::new(10).await;
    let scope = f.save(true, None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let closed = f
        .service
        .get(&f.run.agent_id, &f.run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(closed.state, BrowserScopeState::Closed);
    assert_eq!(
        f.save(true, Some(closed.version)).await,
        Err(BrowserServiceError::Conflict)
    );
    assert_eq!(closed.expires_at_ms, scope.expires_at_ms);
    let fresh = Fixture::new(120_000).await;
    fresh.save(true, None).await.unwrap();
    fresh
        .state
        .read()
        .await
        .live
        .runs()
        .control(&fresh.run.run_id)
        .unwrap()
        .cancel
        .cancel();
    assert_eq!(
        fresh
            .service
            .get(&fresh.run.agent_id, &fresh.run.run_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        BrowserScopeState::Closed
    );
    f.zero_dispatch();
    fresh.zero_dispatch();
}

#[tokio::test]
async fn m46_selected_inputs_pin_owned_session_workspace_bytes_and_total_cap() {
    let f = Fixture::new(120_000).await;
    let row = f
        .attachment(
            "att-owned",
            &f.run.session_id,
            b"quantity,max_delivery_days\n3,2\n",
        )
        .await;
    let mut input = Fixture::input(false, None);
    input.selected_input_ids = vec![row.id.clone()];
    let scope = f
        .service
        .save(&f.run.agent_id, &f.run.run_id, input.clone())
        .await
        .unwrap();
    assert_eq!(scope.selected_input_ids, vec![row.id.clone()]);
    std::fs::write(
        f.workspace.path().join(&row.path),
        b"quantity,max_delivery_days\n9,9\n",
    )
    .unwrap();
    input.expected_version = Some(scope.version);
    assert_eq!(
        f.service.save(&f.run.agent_id, &f.run.run_id, input).await,
        Err(BrowserServiceError::Conflict)
    );
    let foreign = f
        .attachment("att-foreign", "chat:another", b"quantity\n3\n")
        .await;
    let mut input = Fixture::input(false, Some(scope.version));
    input.selected_input_ids = vec![foreign.id];
    assert_eq!(
        f.service.save(&f.run.agent_id, &f.run.run_id, input).await,
        Err(BrowserServiceError::Conflict)
    );
    let one = f
        .attachment("att-one", &f.run.session_id, &vec![b'a'; 9000])
        .await;
    let two = f
        .attachment("att-two", &f.run.session_id, &vec![b'b'; 9000])
        .await;
    let mut input = Fixture::input(false, Some(scope.version));
    input.selected_input_ids = vec![one.id, two.id];
    assert_eq!(
        f.service.save(&f.run.agent_id, &f.run.run_id, input).await,
        Err(BrowserServiceError::Conflict)
    );
    assert_eq!(
        f.service
            .get(&f.run.agent_id, &f.run.run_id)
            .await
            .unwrap()
            .unwrap(),
        scope
    );
    f.zero_dispatch();
}

#[tokio::test]
async fn m46_acknowledged_run_stop_retires_only_its_browser_scope() {
    let f = Fixture::new(120_000).await;
    let first = f.save(true, None).await.unwrap();
    let neighbor = ledger_run(&f.coordinator, &f.run.agent_id, "chat:m46-neighbor").await;
    f.state.read().await.live.runs().register(&neighbor.run_id);
    let neighbor_scope = f
        .service
        .save(
            &f.run.agent_id,
            &neighbor.run_id,
            Fixture::input(false, None),
        )
        .await
        .unwrap();
    f.coordinator
        .stop_run(&f.run.agent_id, &f.run.run_id)
        .await
        .unwrap();
    let stopped = f
        .service
        .get(&f.run.agent_id, &f.run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stopped.state, BrowserScopeState::Closed);
    assert!(stopped.version > first.version);
    assert_eq!(
        f.service
            .get(&f.run.agent_id, &neighbor.run_id)
            .await
            .unwrap()
            .unwrap(),
        neighbor_scope
    );
    assert!(f
        .state
        .read()
        .await
        .runs
        .get(&f.run.run_id)
        .unwrap()
        .stop
        .is_some());
    f.zero_dispatch();
}

#[tokio::test]
async fn m46_failed_run_stop_preserves_scope_until_durability_settles() {
    let f = Fixture::new(120_000).await;
    let scope = f.save(true, None).await.unwrap();
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let coordinator = f.coordinator.clone();
    let link = f.run.clone();
    let stopping =
        tokio::spawn(async move { coordinator.stop_run(&link.agent_id, &link.run_id).await });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(f
        .state
        .read()
        .await
        .runs
        .get(&f.run.run_id)
        .unwrap()
        .stop
        .is_some());
    let service = f.service.clone();
    let link = f.run.clone();
    let inspecting = tokio::spawn(async move { service.get(&link.agent_id, &link.run_id).await });
    tokio::task::yield_now().await;
    assert!(
        !inspecting.is_finished(),
        "inspection waits for the stop durability decision"
    );
    gate.release.add_permits(1);
    assert!(stopping.await.unwrap().is_err());
    assert_eq!(inspecting.await.unwrap().unwrap().unwrap(), scope);
    assert!(f
        .state
        .read()
        .await
        .runs
        .get(&f.run.run_id)
        .unwrap()
        .stop
        .is_none());
    assert!(!f
        .state
        .read()
        .await
        .live
        .runs()
        .control(&f.run.run_id)
        .unwrap()
        .cancel
        .is_cancelled());
    f.zero_dispatch();
}

#[test]
fn m46_installed_policy_constructor_refuses_non_numeric_origins_and_invalid_caps() {
    let table = policies(120_000);
    let installation = table.installation.unwrap();
    let mut policy = table.policies.into_values().next().unwrap();
    policy.template.origin = "http://localhost:43220".into();
    assert!(matches!(
        InstalledBrowserPolicies::new(installation.clone(), vec![policy.clone()]),
        Err(BrowserServiceError::Invalid)
    ));
    policy.template.origin = "http://127.0.0.1:43220".into();
    policy.template.limits.actions = 41;
    assert!(matches!(
        InstalledBrowserPolicies::new(installation, vec![policy]),
        Err(BrowserServiceError::Invalid)
    ));
}

#[test]
fn m63_installed_policy_allows_only_one_pinned_get_only_public_template() {
    let baseline = policies(120_000);
    let installation = baseline.installation.unwrap();
    let loopback = baseline.policies.into_values().next().unwrap();
    let public = public_policy();
    assert!(InstalledBrowserPolicies::new(
        installation.clone(),
        vec![loopback.clone(), public.clone()]
    )
    .is_ok());
    let mut second = public.clone();
    second.id = "second-public-policy".into();
    assert!(matches!(
        InstalledBrowserPolicies::new(installation.clone(), vec![public.clone(), second]),
        Err(BrowserServiceError::Invalid)
    ));

    let mut missing_pin = public.clone();
    let mut value = serde_json::to_value(&missing_pin.template).unwrap();
    value.as_object_mut().unwrap().remove("publicIpv4Pin");
    missing_pin.template = serde_json::from_value(value).unwrap();
    assert!(matches!(
        InstalledBrowserPolicies::new(installation.clone(), vec![missing_pin]),
        Err(BrowserServiceError::Invalid)
    ));
    let mut post = public.clone();
    post.template.routes[1].method = BrowserMethod::Post;
    assert!(matches!(
        InstalledBrowserPolicies::new(installation.clone(), vec![post]),
        Err(BrowserServiceError::Invalid)
    ));
    let mut inputs = public.clone();
    inputs.template.input_ids.push("operator-input".into());
    assert!(matches!(
        InstalledBrowserPolicies::new(installation.clone(), vec![inputs]),
        Err(BrowserServiceError::Invalid)
    ));
    let mut private_pin = public;
    let mut value = serde_json::to_value(&private_pin.template).unwrap();
    value["publicIpv4Pin"] = serde_json::json!("169.254.169.254");
    private_pin.template = serde_json::from_value(value).unwrap();
    assert!(matches!(
        InstalledBrowserPolicies::new(installation, vec![private_pin]),
        Err(BrowserServiceError::Invalid)
    ));
}

#[tokio::test]
async fn m63_public_metadata_does_not_grant_browser_tools_or_accept_inputs() {
    let f = Fixture::new(120_000).await;
    let installed = InstalledBrowserPolicies::new(
        policies(120_000).installation.unwrap(),
        vec![public_policy()],
    )
    .unwrap();
    let service = BrowserService::new(
        &f.state,
        f.coordinator.control_plane_transactions(),
        installed,
        f.port.clone(),
    );
    let input = SaveBrowserScope {
        policy_id: "public-operator-policy".into(),
        selected_input_ids: vec![],
        enabled: true,
        expected_version: None,
    };
    let view = service
        .save(&f.run.agent_id, &f.run.run_id, input.clone())
        .await
        .unwrap();
    assert_eq!(view.origin, "https://m63-browser.anima.tools");
    assert_eq!(view.tool_names, vec!["calculate"]);
    assert!(view.selected_input_ids.is_empty());
    assert_eq!(f.port.opens.load(Ordering::SeqCst), 0);
    let listed = serde_json::to_value(service.policies(&f.run.agent_id).await.unwrap()).unwrap();
    assert!(!listed.to_string().contains("93.184.216.34"));
    assert!(!listed.to_string().contains("publicIpv4Pin"));

    let call = anima_core::ToolCall {
        id: "attempt-public-navigation".into(),
        name: "browser_navigate".into(),
        args: std::collections::BTreeMap::new(),
    };
    assert_eq!(
        service
            .capture_dispatch(
                &f.run,
                &call,
                anima_core::browser::BrowserOperation::Navigate {
                    route_id: "page".into(),
                },
                None,
            )
            .await
            .err()
            .expect("a public destination must not grant a browser tool")
            .code,
        BrowserErrorCode::Denied
    );
    assert_eq!(f.port.opens.load(Ordering::SeqCst), 0);
    let mut selected = input.clone();
    selected.selected_input_ids = vec!["unselected-input".into()];
    selected.expected_version = Some(view.version);
    assert_eq!(
        service.save(&f.run.agent_id, &f.run.run_id, selected).await,
        Err(BrowserServiceError::Invalid)
    );
    f.zero_dispatch();

    let granted =
        Fixture::with_tools_model(120_000, ScriptedModel::new(vec![]), &["browser_navigate"]).await;
    let installed = InstalledBrowserPolicies::new(
        policies(120_000).installation.unwrap(),
        vec![public_policy()],
    )
    .unwrap();
    let granted_service = BrowserService::new(
        &granted.state,
        granted.coordinator.control_plane_transactions(),
        installed,
        granted.port.clone(),
    );
    let granted_view = granted_service
        .save(&granted.run.agent_id, &granted.run.run_id, input)
        .await
        .unwrap();
    assert_eq!(granted_view.tool_names, vec!["browser_navigate"]);
    assert!(granted_service
        .capture_dispatch(
            &granted.run,
            &call,
            anima_core::browser::BrowserOperation::Navigate {
                route_id: "page".into(),
            },
            None,
        )
        .await
        .is_ok());
    granted.zero_dispatch();
}

#[tokio::test]
async fn m46_snapshot_restore_preserves_grants_without_browser_scope_or_replay() {
    let f = Fixture::new(120_000).await;
    f.save(true, None).await.unwrap();
    let snapshot = f.state.read().await.control_plane_snapshot();
    let original_grants = f.state.read().await.agents[&f.run.agent_id]
        .config()
        .tools
        .clone();
    let model = ScriptedModel::new(vec![]);
    let mut restored = DaemonState::with_model_adapter(model.clone());
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert!(restored.browser_service.is_none());
    assert_eq!(
        restored.agents[&f.run.agent_id].config().tools,
        original_grants
    );
    assert_eq!(
        restored.runs.get(&f.run.run_id).unwrap().status,
        RunStatus::Interrupted
    );
    let state = Arc::new(RwLock::new(restored));
    let coordinator = crate::app::coordinator_with_mcp(
        &state,
        AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1))),
    );
    let port = Arc::new(RecordingPort::default());
    let browser = BrowserService::new(
        &state,
        coordinator.control_plane_transactions(),
        policies(120_000),
        port.clone(),
    );
    assert!(browser.list(&f.run.agent_id).await.unwrap().is_empty());
    assert!(browser
        .get(&f.run.agent_id, &f.run.run_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        browser
            .save(&f.run.agent_id, &f.run.run_id, Fixture::input(true, None))
            .await,
        Err(BrowserServiceError::Conflict)
    );
    assert_eq!(port.opens.load(Ordering::SeqCst), 0);
    assert!(model.requests().is_empty());
    f.zero_dispatch();
}
