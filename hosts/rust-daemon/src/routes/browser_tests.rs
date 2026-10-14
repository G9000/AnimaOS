//! Owner missing-route RED retained with the real shared policy constructor.
//! Follow-on cases: missing/helper/foreign/stopped runs; stale scope CAS;
//! Exec Deny/Ask; grant/document/cancel revocation during Ask/receipt save;
//! one physical slot, malformed/oversize IPC/artifacts, Stop and real child joins.
use crate::agent_runs::test_support::{companion_config, ledger_run, ScriptedModel};
use crate::agent_runs::AgentRunCoordinator;
use crate::browser::{
    tests::{policies, RecordingPort},
    BrowserService,
};
use crate::control_plane_store::{ControlPlaneStoreConfig, WorkspaceConfig};
use crate::state::DaemonState;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use tower::util::ServiceExt;

#[tokio::test]
async fn m46_owner_scope_save_for_live_ordinary_run_starts_no_worker() {
    let workspace = tempfile::tempdir().unwrap();
    let model = ScriptedModel::new(vec![]);
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.workspace = Some(WorkspaceConfig {
        root_path: workspace.path().to_path_buf(),
        company_name: "M46 synthetic workspace".into(),
        mission: "Declare a disabled ordinary-run browser scope".into(),
        values: vec![],
    });
    daemon.control_plane_store = Some(ControlPlaneStoreConfig::Json(
        workspace.path().join("control.json"),
    ));
    // Exact numeric origin is metadata only; no server/port reservation or
    // runtime installation/compatibility claim is made by this recording port.
    let policy_id = "controlled-operator-policy";
    let agent_id = daemon
        .create_agent(companion_config("M46 ordinary browser owner"))
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(daemon));
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
    state.write().await.browser_service = Some(browser.clone());
    let run = ledger_run(&coordinator, &agent_id, "chat:m46-owner-red").await;
    let control = state.read().await.live.runs().register(&run.run_id);
    assert!(!control.cancel.is_cancelled());
    assert_eq!(browser.policies(&agent_id).await.unwrap()[0].id, policy_id);
    assert!(state
        .read()
        .await
        .sessions
        .contains(&agent_id, &run.session_id));
    assert_eq!(
        state.read().await.runs.get(&run.run_id).unwrap().status,
        crate::runs::RunStatus::Running
    );
    let grants = state.read().await.agents[&agent_id].config().tools.clone();
    let receipts = state
        .read()
        .await
        .runs
        .get(&run.run_id)
        .unwrap()
        .action_receipts
        .clone();
    let approvals = state.read().await.approvals.pending().len();
    let app = crate::app::app_with_state(state.clone(), crate::DaemonConfig::default());
    let path = format!("/api/agents/{agent_id}/runs/{}/browser-scope", run.run_id);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"policyId":policy_id,"selectedInputIds":[],"enabled":false}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "an owner may save a disabled scope for an exact live ordinary run without starting a worker");
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let saved: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16 * 1024).await.unwrap()).unwrap();
    assert_eq!(saved["scope"]["runId"], run.run_id);
    assert_eq!(saved["scope"]["agentId"], agent_id);
    assert_eq!(saved["scope"]["policyId"], policy_id);
    assert_eq!(saved["scope"]["enabled"], false);
    assert_eq!(saved["scope"]["physicalLeaseHeld"], false);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&path)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let current: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16 * 1024).await.unwrap()).unwrap();
    assert_eq!(current, saved);
    for invalid in [
        r#"{"policyId":"controlled-operator-policy","policyId":"controlled-operator-policy","selectedInputIds":[],"enabled":false}"#,
        r#"{"policyId":"controlled-operator-policy","selectedInputIds":[],"enabled":false,"expectedVersion":null}"#,
        r#"{"policyId":"controlled-operator-policy","selectedInputIds":[],"enabled":false,"expectedVersion":0}"#,
        r#"{"policyId":"controlled-operator-policy","selectedInputIds":[],"enabled":false,"expectedVersion":9007199254740992}"#,
        r#"{"policyId":"controlled-operator-policy","selectedInputIds":[],"enabled":null}"#,
        r#"{"policyId":"controlled-operator-policy","selectedInputIds":[],"enabled":false,"url":"http://127.0.0.1:1"}"#,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&path)
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200")
                    .header("content-type", "application/json")
                    .body(Body::from(invalid))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    }
    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header("host", "127.0.0.1:8080")
                .header("origin", "https://foreign.invalid")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        unauthorized.headers().get("cache-control").unwrap(),
        "no-store"
    );
    assert_eq!(
        browser
            .get(&agent_id, &run.run_id)
            .await
            .unwrap()
            .unwrap()
            .version,
        saved["scope"]["version"].as_u64().unwrap()
    );
    assert_eq!(port.opens.load(Ordering::SeqCst), 0);
    let guard = state.read().await;
    assert_eq!(guard.agents[&agent_id].config().tools, grants);
    assert_eq!(
        guard.runs.get(&run.run_id).unwrap().action_receipts,
        receipts
    );
    assert_eq!(guard.approvals.pending().len(), approvals);
    assert!(model.requests().is_empty());
    assert!(!control.cancel.is_cancelled());
}

#[tokio::test]
async fn m46_owner_chat_selection_is_bound_before_first_model_step() {
    use crate::agent_runs::test_support::{add_chat, Gate, Step};
    use crate::browser::{tests::Fixture, BrowserScopeState};
    use std::time::Duration;

    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![Step::Text(vec!["synthetic browser admission complete"])],
        gate.clone(),
    );
    let fixture = Fixture::with_model(120_000, model.clone()).await;
    let agent_id = fixture.run.agent_id.clone();
    let session_id = "chat:m46-owner-opt-in";
    add_chat(&fixture.coordinator, &agent_id, session_id).await;
    let grants = fixture.state.read().await.agents[&agent_id]
        .config()
        .tools
        .clone();
    let app = crate::app::app_with_state(fixture.state.clone(), crate::DaemonConfig::default());
    let response=app.oneshot(Request::builder().method("POST")
        .uri(format!("/api/agents/{agent_id}/sessions/{session_id}/runs"))
        .header("host","127.0.0.1:8080").header("origin","http://localhost:4200")
        .header("content-type","application/json").header("idempotency-key","m46-browser-opt-in")
        .body(Body::from(json!({"text":"Inspect the controlled page", "mode":"queue", "browserSelection":{
            "policyId":"controlled-operator-policy", "selectedInputIds":[]
        }}).to_string())).unwrap()).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::ACCEPTED,
        "ordinary owner chat must bind its selected browser scope before its first model/tool step"
    );
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let accepted: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 32 * 1024).await.unwrap()).unwrap();
    let run_id = accepted["run"]["id"].as_str().unwrap();
    gate.entered().await;
    let scope = fixture
        .service
        .get(&agent_id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(scope.run_id, run_id);
    assert_eq!(scope.session_id, session_id);
    assert_eq!(scope.policy_id, "controlled-operator-policy");
    assert_eq!(scope.state, BrowserScopeState::Scoped);
    assert!(scope.enabled);
    assert!(!scope.physical_lease_held);
    assert_eq!(fixture.port.opens.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture.state.read().await.agents[&agent_id].config().tools,
        grants
    );
    assert!(fixture
        .state
        .read()
        .await
        .runs
        .get(run_id)
        .unwrap()
        .action_receipts
        .is_empty());
    gate.release();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if fixture
                .state
                .read()
                .await
                .runs
                .get(run_id)
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        model.requests().len(),
        1,
        "one controlled scripted step, no live provider"
    );
    assert_eq!(fixture.port.opens.load(Ordering::SeqCst), 0);
}

fn chat_request(agent: &str, session: &str, key: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/agents/{agent}/sessions/{session}/runs"))
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header("content-type", "application/json")
        .header("idempotency-key", key)
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn m46_chat_selection_busy_race_replay_and_omission_preserve_authority() {
    use crate::agent_runs::test_support::{add_chat, Gate, Step};
    use crate::browser::tests::Fixture;
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Text(vec!["first"]),
            Step::Text(vec!["ordinary queued reply"]),
        ],
        gate.clone(),
    );
    let f = Fixture::with_model(120_000, model.clone()).await;
    let agent = &f.run.agent_id;
    let session = "chat:m46-opt-in-race";
    add_chat(&f.coordinator, agent, session).await;
    let app = crate::app::app_with_state(f.state.clone(), crate::DaemonConfig::default());
    let body = json!({"text":"same criteria task","mode":"queue","browserSelection":{"policyId":"controlled-operator-policy","selectedInputIds":[]}});
    let (one, two) = tokio::join!(
        app.clone()
            .oneshot(chat_request(agent, session, "race-one", body.clone())),
        app.clone()
            .oneshot(chat_request(agent, session, "race-two", body.clone()))
    );
    let one = one.unwrap();
    let two = two.unwrap();
    assert_eq!(
        [one.status(), two.status()]
            .iter()
            .filter(|s| **s == StatusCode::ACCEPTED)
            .count(),
        1
    );
    assert_eq!(
        [one.status(), two.status()]
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1
    );
    let (response, key) = if one.status() == StatusCode::ACCEPTED {
        (one, "race-one")
    } else {
        (two, "race-two")
    };
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 32 * 1024).await.unwrap()).unwrap();
    let run = value["run"]["id"].as_str().unwrap();
    gate.entered().await;
    let original = f.service.get(agent, run).await.unwrap().unwrap();
    let replay=app.clone().oneshot(chat_request(agent,session,key,json!({"text":"same criteria task","browserSelection":{"policyId":"uninstalled-changed-selection","selectedInputIds":["att-missing"]}}))).await.unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(f.service.get(agent, run).await.unwrap().unwrap(), original);
    let steer=app.clone().oneshot(chat_request(agent,session,"new-steer",json!({"text":"new criteria","mode":"steer","browserSelection":{"policyId":"controlled-operator-policy","selectedInputIds":[]}}))).await.unwrap();
    assert_eq!(steer.status(), StatusCode::BAD_REQUEST);
    let ordinary = app
        .clone()
        .oneshot(chat_request(
            agent,
            session,
            "no-selection",
            json!({"text":"ordinary queued reply"}),
        ))
        .await
        .unwrap();
    assert_eq!(ordinary.status(), StatusCode::ACCEPTED);
    let value: Value =
        serde_json::from_slice(&to_bytes(ordinary.into_body(), 32 * 1024).await.unwrap()).unwrap();
    let next = value["run"]["id"].as_str().unwrap();
    assert!(f.service.get(agent, next).await.unwrap().is_none());
    gate.release();
    gate.entered().await;
    assert!(f.service.get(agent, next).await.unwrap().is_none());
    gate.release();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if f.state
                .read()
                .await
                .runs
                .get(next)
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.port.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn m46_chat_selection_changed_workspace_fails_before_any_model_or_open() {
    use crate::agent_runs::test_support::{accept, ScriptedModel};
    use crate::browser::{tests::Fixture, BrowserSelection};
    let model = ScriptedModel::new(vec![]);
    let f = Fixture::with_model(120_000, model.clone()).await;
    let session = "chat:m46-bind-change";
    crate::agent_runs::test_support::add_chat(&f.coordinator, &f.run.agent_id, session).await;
    let prepared = f
        .service
        .prepare_selection(
            &f.run.agent_id,
            session,
            BrowserSelection {
                policy_id: "controlled-operator-policy".into(),
                selected_input_ids: vec![],
            },
        )
        .await
        .unwrap();
    // Hold queued acceptance's durability save; revalidation at bind must see
    // the later workspace change before even compaction can call a model.
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let coordinator = f.coordinator.clone();
    let agent = f.run.agent_id.clone();
    let p = prepared.clone();
    let accepting = tokio::spawn(async move {
        let start = coordinator.web_start_with_browser(
            agent.clone(),
            session.into(),
            "changed-workspace".into(),
            p,
        );
        coordinator
            .accept_browser_run(
                accept(&agent, session, "changed-workspace"),
                start,
                prepared,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    f.state.write().await.workspace.as_mut().unwrap().mission =
        "changed after prevalidation".into();
    gate.release.add_permits(1);
    let accepted = accepting.await.unwrap().unwrap();
    let crate::agent_runs::AcceptedRun::Created(record) = accepted else {
        panic!("new run")
    };
    // Phase A and the explicit failed-start outcome each use the existing save.
    gate.release.add_permits(2);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if f.state
                .read()
                .await
                .runs
                .get(&record.id)
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let state = f.state.read().await;
    let run = state.runs.get(&record.id).unwrap();
    assert_eq!(run.status, crate::runs::RunStatus::Failed);
    assert_eq!(
        run.error.as_ref().unwrap().code,
        "browser_scope_admission_failed"
    );
    assert!(run.action_receipts.is_empty());
    drop(state);
    assert!(f
        .service
        .get(&f.run.agent_id, &record.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(f.port.opens.load(Ordering::SeqCst), 0);
    assert!(model.requests().is_empty());
}
