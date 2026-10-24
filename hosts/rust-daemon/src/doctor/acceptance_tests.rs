use super::*;
use crate::agent_runs::test_support::{
    companion_config, coordinator_state, decision, gated_context, ledger_run, pending_approvals,
    tool_input,
};
use crate::agent_runs::AgentRunCoordinator;
use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
use anima_core::{AgentState, CancelSignal, DataValue, ToolCall};
use std::collections::BTreeMap;
struct Fixture {
    _dir: tempfile::TempDir,
    state: SharedDaemonState,
    coordinator: AgentRunCoordinator,
    agent: AgentState,
    link: crate::runs::RunLink,
    original: ScheduledPromptRecord,
    cancel: CancelSignal,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut daemon = crate::state::DaemonState::new();
        daemon.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                dir.path().join("control.json"),
            ),
        ));
        let mut config = companion_config("Doctor controlled owner");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors([
                    "doctor_report",
                    "doctor_repairs",
                    "doctor_repair",
                    "doctor_undo",
                ])
                .unwrap(),
        );
        let agent = daemon.create_agent(config).unwrap().state;
        daemon.approvals.set_policy(
            &agent.id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Allow),
        );
        let mut original = super::tests::record();
        original.agent_id = agent.id.clone();
        daemon
            .schedules
            .insert(original.id.clone(), original.clone());
        let state = Arc::new(tokio::sync::RwLock::new(daemon));
        let coordinator =
            AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(1)));
        let link = ledger_run(&coordinator, &agent.id, "chat:doctor-owner").await;
        Self {
            _dir: dir,
            state,
            coordinator,
            agent,
            link,
            original,
            cancel: CancelSignal::new(),
        }
    }
    fn call(&self) -> ToolCall {
        ToolCall {
            id: "doctor-approved-pause".into(),
            name: "doctor_repair".into(),
            args: BTreeMap::from([
                ("id".into(), DataValue::String(self.original.id.clone())),
                (
                    "fingerprint".into(),
                    DataValue::String(fingerprint(&self.original)),
                ),
            ]),
        }
    }
}
#[tokio::test]
async fn doctor_native_pause_waits_for_ordinary_write_approval_and_records_receipt() {
    let f = Fixture::new().await;
    f.state.write().await.approvals.set_policy(
        &f.agent.id,
        ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Ask),
    );
    let context = gated_context(&f.coordinator, &f.link, f.cancel.clone())
        .await
        .with_action_journal(crate::agent_runs::action_receipts::RunActionJournal::new(
            &f.state,
            f.coordinator.control_plane_transactions(),
            f.link.clone(),
            f.cancel.clone(),
            None,
        ));
    let (owner, input, call) = (
        f.agent.clone(),
        tool_input(&f.agent.id, &f.link.session_id),
        f.call(),
    );
    let pending = tokio::spawn(async move { context.execute_tool(owner, input, call).await });
    let asks = pending_approvals(&f.coordinator, 1).await;
    assert_eq!(asks[0].tool, "doctor_repair");
    assert_eq!(f.state.read().await.schedules[&f.original.id], f.original);
    f.coordinator
        .decide_approval(
            &asks[0].id,
            decision(ApprovalDecisionKind::AllowOnce, asks[0].revision),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let g = f.state.read().await;
    assert!(!g.schedules[&f.original.id].enabled);
    let run = g.runs.get(&f.link.run_id).unwrap();
    assert_eq!(run.action_receipts.len(), 1);
    assert_eq!(
        run.action_receipts[0].status,
        crate::runs::ActionStatus::Succeeded
    );
}
#[tokio::test]
async fn doctor_rechecks_late_stop_grant_cancel_and_drain_while_physical_work_is_counted() {
    for race in 0..4 {
        let f = Fixture::new().await;
        let tx = f.coordinator.control_plane_transaction().await;
        let service = f.coordinator.doctor();
        let authority = DoctorAuthority {
            link: f.link.clone(),
            call: f.call(),
            approved: true,
            cancel: Some(f.cancel.clone()),
        };
        let (agent, id, fp) = (
            f.agent.id.clone(),
            f.original.id.clone(),
            fingerprint(&f.original),
        );
        let pending =
            tokio::spawn(async move { service.repair(agent, id, fp, Some(authority)).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.state.read().await.doctor_slots.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        match race {
            0 => {
                f.state
                    .write()
                    .await
                    .runs
                    .get_mut(&f.link.run_id)
                    .unwrap()
                    .stop = Some(crate::runs::RunStopRequest { requested_at_ms: 1 });
            }
            1 => {
                let mut g = f.state.write().await;
                let runtime = g.agents.get_mut(&f.agent.id).unwrap();
                let mut config = runtime.config().clone();
                config.tools = Some(vec![]);
                runtime.replace_config(config);
            }
            2 => f.cancel.cancel(),
            _ => f.coordinator.begin_drain(),
        }
        drop(tx);
        assert!(tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        let g = f.state.read().await;
        assert_eq!(g.schedules[&f.original.id], f.original);
        assert!(g.doctor.repairs.is_empty());
        assert_eq!(g.doctor_slots.available_permits(), 1);
    }
}
#[tokio::test]
async fn doctor_owner_routes_reject_cross_origin_requests_and_share_the_report() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::util::ServiceExt;
    let f = Fixture::new().await;
    let app =
        crate::app::app_with_state_for_owner_test(f.state.clone(), crate::DaemonConfig::default());
    for origin in ["https://attacker.invalid", "http://localhost:4200"] {
        let mut request = Request::builder()
            .uri("/api/doctor")
            .header("host", "127.0.0.1:8080")
            .header("origin", origin);
        if origin == "http://localhost:4200" {
            request = crate::routes::owner_test_support::authenticate(request);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        if origin.starts_with("https") {
            assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN)
        } else {
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(body["externalProbes"], false);
            assert!(body["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "providers" && c.get("configured").is_some()));
        }
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/agents/{}/doctor/repairs", f.agent.id))
                .header("host", "127.0.0.1:8080")
                .header("origin", "https://attacker.invalid")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"id":f.original.id,"fingerprint":fingerprint(&f.original)})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    assert!(f.state.read().await.doctor.repairs.is_empty());
}
