//! Scoped durable attachment inputs across acceptance, replay, and steering.
use super::test_support::{
    add_chat, calculate_call, coordinator_with, events_until, Gate, ScriptedModel, Step,
};
use super::*;
use crate::attachments::{files::save_upload, AttachmentKind, AttachmentRecord};
use crate::control_plane_store::WorkspaceConfig;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("anima-run-attachment-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.starts_with(std::env::temp_dir())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("anima-run-attachment-")
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
async fn fixture(
    model: Arc<dyn anima_core::ModelAdapter>,
) -> (Temp, AgentRunCoordinator, String, Vec<AttachmentRecord>) {
    let temp = Temp::new();
    let (runs, agent) = coordinator_with(model).await;
    add_chat(&runs, &agent, "chat:files").await;
    runs.state.write().await.workspace = Some(WorkspaceConfig {
        root_path: temp.0.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let mut rows = Vec::new();
    for name in ["one.txt", "two.txt"] {
        let saved = save_upload(&temp.0, "2026-09-23", name, b"hello").unwrap();
        rows.push(AttachmentRecord {
            id: format!("att-{}", uuid::Uuid::new_v4()),
            agent_id: agent.clone(),
            session_id: "chat:files".into(),
            name: name.into(),
            mime: "text/plain".into(),
            size: 5,
            kind: AttachmentKind::Text,
            path: saved.path,
            created_at_ms: 1,
            workspace_root: saved.workspace_root,
            sha256: saved.sha256,
        });
    }
    let store = runs.state.read().await.history.store();
    store.upsert_attachments(&rows).await.unwrap();
    (temp, runs, agent, rows)
}
fn request(agent: &str, key: &str, value: Value) -> Request<Body> {
    crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method("POST")
            .uri(format!("/api/agents/{agent}/sessions/chat%3Afiles/runs"))
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .header("content-type", "application/json")
            .header("idempotency-key", key),
    )
    .body(Body::from(value.to_string()))
    .unwrap()
}
async fn answer(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
fn app(runs: &AgentRunCoordinator) -> axum::Router {
    crate::routes::router_with_runs(
        runs.state.clone(),
        crate::app::DaemonConfig::default(),
        |_| runs.clone(),
    )
}

#[tokio::test]
async fn attachment_only_acceptance_keeps_raw_input_and_scoped_refs() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["done"])], gate.clone());
    let (_temp, runs, agent, rows) = fixture(model.clone()).await;
    let app = app(&runs);
    let mut stream = runs.state.read().await.live.subscribe(&agent).unwrap();
    let response = app
        .oneshot(request(
            &agent,
            "attachment-only",
            json!({"text":"","attachmentIds":[rows[0].id,rows[1].id]}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let value = answer(response).await;
    assert_eq!(value["run"]["input"]["text"], "");
    assert_eq!(
        value["run"]["input"]["attachmentIds"],
        json!([rows[0].id, rows[1].id])
    );
    gate.entered().await;
    let requests = model.requests();
    let message = requests[0]
        .messages
        .iter()
        .find(|m| m.role == anima_core::MessageRole::User)
        .unwrap();
    assert!(message.content.text.contains("read_file"));
    let attachments = message.content.attachments.as_ref().unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0].data, rows[0].path);
    assert_eq!(attachments[0].mime_type.as_deref(), Some("text/plain"));
    gate.release();
    events_until(&mut stream, "run.completed").await;
    let guard = runs.state.read().await;
    let record = guard
        .runs
        .get(value["run"]["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(record.input.text, "");
    assert_eq!(
        record.input.attachment_ids,
        vec![rows[0].id.clone(), rows[1].id.clone()]
    );
    let encoded = serde_json::to_string(record).unwrap();
    assert!(!encoded.contains("workspaceRoot"));
    assert!(!encoded.contains("sha256"));
}
#[tokio::test]
async fn attachment_scope_order_skill_and_loss_replay_are_exact() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["done"])], gate.clone());
    let (temp, runs, agent, rows) = fixture(model).await;
    let app = app(&runs);
    let mut stream = runs.state.read().await.live.subscribe(&agent).unwrap();
    let input = json!({"text":"original","attachmentIds":[rows[0].id,rows[1].id]});
    let response = app
        .clone()
        .oneshot(request(&agent, "key", input.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let first = answer(response).await;
    gate.entered().await;
    std::fs::remove_file(temp.0.join(&rows[0].path)).unwrap();
    let replay = app
        .clone()
        .oneshot(request(&agent, "key", input))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(answer(replay).await["run"]["id"], first["run"]["id"]);
    for changed in [
        json!({"text":"original","attachmentIds":[rows[1].id,rows[0].id]}),
        json!({"text":"original","attachmentIds":[rows[0].id]}),
        json!({"text":"different","attachmentIds":[rows[0].id,rows[1].id]}),
        json!({"text":"original","attachmentIds":[rows[0].id,rows[1].id],"skill":"missing"}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(&agent, "key", changed))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }
    gate.release();
    events_until(&mut stream, "run.completed").await;
    assert_eq!(
        runs.state
            .read()
            .await
            .runs
            .snapshot_records(&std::collections::HashSet::from([agent.clone()]))
            .iter()
            .filter(|r| r.idempotency_key.as_deref() == Some("key"))
            .count(),
        1
    );
}
#[tokio::test]
async fn attachment_inputs_reject_unknown_foreign_duplicate_and_oversized_ids() {
    let (_temp, runs, agent, rows) = fixture(ScriptedModel::new(vec![])).await;
    let app = app(&runs);
    let mut foreign = rows[0].clone();
    foreign.id = "att-foreign".into();
    foreign.agent_id = "other-agent".into();
    let mut wrong_session = rows[0].clone();
    wrong_session.id = "att-other-session".into();
    wrong_session.session_id = "chat:other".into();
    runs.state
        .read()
        .await
        .history
        .store()
        .upsert_attachments(&[foreign, wrong_session])
        .await
        .unwrap();
    let sets = vec![
        vec!["unknown".to_string()],
        vec!["att-foreign".into()],
        vec!["att-other-session".into()],
        vec![rows[0].id.clone(), rows[0].id.clone()],
        vec![String::new()],
        vec!["x".repeat(129)],
        (0..11).map(|n| format!("att-{n}")).collect(),
    ];
    for (n, ids) in sets.into_iter().enumerate() {
        assert_eq!(
            app.clone()
                .oneshot(request(
                    &agent,
                    &format!("bad-{n}"),
                    json!({"text":"hello","attachmentIds":ids})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
#[tokio::test]
async fn attachment_steer_consumption_and_pruned_replay_keep_exact_input() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![calculate_call("calculate-1", "1+1")]),
            Step::Text(vec!["done"]),
        ],
        gate.clone(),
    );
    let (temp, runs, agent, rows) = fixture(model.clone()).await;
    let app = app(&runs);
    let mut stream = runs.state.read().await.live.subscribe(&agent).unwrap();
    let response = app
        .clone()
        .oneshot(request(&agent, "base", json!({"text":"base"})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let base = answer(response).await;
    gate.entered().await;
    let steer = json!({"text":"raw steer","attachmentIds":[rows[0].id],"mode":"steer"});
    let response = app
        .clone()
        .oneshot(request(&agent, "steer", steer.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    gate.release();
    gate.entered().await;
    let requests = model.requests();
    let steered = requests
        .last()
        .unwrap()
        .messages
        .iter()
        .find(|message| {
            message
                .content
                .metadata
                .as_ref()
                .and_then(|m| m.get("clientRequestId"))
                == Some(&anima_core::DataValue::String("steer".into()))
        })
        .unwrap();
    assert_eq!(
        steered.content.attachments.as_ref().unwrap()[0].data,
        rows[0].path
    );
    gate.release();
    let events = events_until(&mut stream, "run.completed").await;
    let event = events
        .iter()
        .find(|event| event["type"] == "run.steered")
        .unwrap();
    assert_eq!(event["text"], "raw steer");
    assert_eq!(event["attachmentIds"], json!([rows[0].id]));
    assert_eq!(event["attachments"][0]["mime"], "text/plain");
    assert!(event["attachments"][0].get("sha256").is_none());
    {
        let mut state = runs.state.write().await;
        let record = state.runs.get(base["run"]["id"].as_str().unwrap()).unwrap();
        let saved = serde_json::to_value(record).unwrap();
        assert_eq!(saved["steeredKeys"][0]["input"]["text"], "raw steer");
        assert_eq!(
            saved["steeredKeys"][0]["input"]["attachmentIds"],
            json!([rows[0].id])
        );
        state
            .agents
            .get_mut(&agent)
            .unwrap()
            .retain_messages(|_| false);
    }
    std::fs::remove_file(temp.0.join(&rows[0].path)).unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(&agent, "steer", steer))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for changed in [
        json!({"text":"different","attachmentIds":[rows[0].id],"mode":"steer"}),
        json!({"text":"raw steer","attachmentIds":[rows[1].id],"mode":"steer"}),
        json!({"text":"raw steer","attachmentIds":[rows[0].id],"skill":"missing","mode":"steer"}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(&agent, "steer", changed))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }
}
#[tokio::test]
async fn attachment_unread_steer_stop_and_restart_preserve_ids_without_notes_duplication() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Hold(vec!["held"])], gate.clone());
    let (_temp, runs, agent, rows) = fixture(model.clone()).await;
    let app = app(&runs);
    let mut stream = runs.state.read().await.live.subscribe(&agent).unwrap();
    let base = app
        .clone()
        .oneshot(request(&agent, "base", json!({"text":"base"})))
        .await
        .unwrap();
    let base = answer(base).await;
    gate.entered().await;
    let response = app
        .oneshot(request(
            &agent,
            "leftover",
            json!({"text":"raw leftover","attachmentIds":[rows[0].id],"mode":"steer"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    runs.stop_run(&agent, base["run"]["id"].as_str().unwrap())
        .await
        .unwrap();
    events_until(&mut stream, "run.cancelled").await;
    let records = runs
        .state
        .read()
        .await
        .runs
        .snapshot_records(&std::collections::HashSet::from([agent.clone()]));
    let left = records
        .iter()
        .find(|r| r.idempotency_key.as_deref() == Some("leftover"))
        .unwrap();
    assert_eq!(left.status, crate::runs::RunStatus::Interrupted);
    assert_eq!(left.input.text, "raw leftover");
    assert_eq!(left.input.attachment_ids, vec![rows[0].id.clone()]);
    let left_id = left.id.clone();
    let restored = crate::runs::RunLedger::restored(
        records,
        &std::collections::HashSet::from([agent]),
        anima_core::primitives::now_millis(),
    );
    assert_eq!(
        restored.get(&left_id).unwrap().input.attachment_ids,
        vec![rows[0].id.clone()]
    );
    assert_eq!(model.requests().len(), 1);
}
#[tokio::test]
async fn attachment_acceptance_cancellation_finishes_and_failure_rolls_back() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["done"])], gate.clone());
    let (_temp, runs, agent, rows) = fixture(model.clone()).await;
    let mut stream = runs.state.read().await.live.subscribe(&agent).unwrap();
    let save = runs
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let mut input = super::test_support::accept(&agent, "chat:files", "cancelled-caller");
    input.text = "raw".into();
    input.attachment_ids = vec![rows[0].id.clone()];
    let start = runs.web_start_with_attachments(
        agent.clone(),
        "chat:files".into(),
        input.text.clone(),
        input.idempotency_key.clone(),
        None,
        input.attachment_ids.clone(),
    );
    let accepting = runs.clone();
    let task = tokio::spawn(async move { accepting.accept_run(input, start).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), save.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(
        runs.state
            .read()
            .await
            .runs
            .find_by_idempotency_key(&agent, "cancelled-caller", 0)
            .is_some(),
        "gate belongs to acceptance"
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    save.release.add_permits(1);
    gate.entered().await;
    gate.release();
    events_until(&mut stream, "run.completed").await;
    assert_eq!(
        runs.state
            .read()
            .await
            .runs
            .find_by_idempotency_key(&agent, "cancelled-caller", 0)
            .unwrap()
            .input
            .attachment_ids,
        vec![rows[0].id.clone()]
    );
    let save = runs
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    save.release.add_permits(1);
    let mut input = super::test_support::accept(&agent, "chat:files", "failed");
    input.text = "raw".into();
    input.attachment_ids = vec![rows[1].id.clone()];
    let start = runs.web_start_with_attachments(
        agent.clone(),
        "chat:files".into(),
        input.text.clone(),
        input.idempotency_key.clone(),
        None,
        input.attachment_ids.clone(),
    );
    assert_eq!(
        runs.accept_run(input, start).await.unwrap_err().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(runs
        .state
        .read()
        .await
        .runs
        .find_by_idempotency_key(&agent, "failed", 0)
        .is_none());
    assert_eq!(model.requests().len(), 1);
}
#[tokio::test]
async fn attachment_new_acceptance_rejects_missing_changed_and_rebound_workspace() {
    let (temp, runs, agent, rows) = fixture(ScriptedModel::new(vec![])).await;
    let app = app(&runs);
    std::fs::write(temp.0.join(&rows[0].path), b"other").unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(
                &agent,
                "changed",
                json!({"text":"","attachmentIds":[rows[0].id]})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    std::fs::remove_file(temp.0.join(&rows[0].path)).unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(
                &agent,
                "missing",
                json!({"text":"","attachmentIds":[rows[0].id]})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let other = Temp::new();
    runs.state
        .write()
        .await
        .workspace
        .as_mut()
        .unwrap()
        .root_path = other.0.clone();
    assert_eq!(
        app.oneshot(request(
            &agent,
            "rebound",
            json!({"text":"","attachmentIds":[rows[1].id]})
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(runs
        .state
        .read()
        .await
        .runs
        .snapshot_records(&std::collections::HashSet::from([agent]))
        .is_empty());
}

#[tokio::test]
async fn legacy_consumed_steer_without_transcript_proof_fails_closed() {
    let (_temp, runs, agent, _rows) = fixture(ScriptedModel::new(vec![])).await;
    let mut record = crate::runs::RunRecord::queued(
        crate::runs::RunStart {
            agent_id: agent.clone(),
            session_id: "chat:files".into(),
            source: crate::runs::RunSource::Web,
            source_ref: None,
            idempotency_key: Some("base".into()),
            text: "base".into(),
            model: "model".into(),
            provider: None,
            parent_run_id: None,
        },
        anima_core::primitives::now_millis(),
    );
    record.steered_keys = serde_json::from_value(json!([{ "idempotencyKey":"old-steer", "acceptedAtMs":anima_core::primitives::now_millis() }])).unwrap();
    runs.state.write().await.runs.insert(record);
    assert_eq!(
        app(&runs)
            .oneshot(request(
                &agent,
                "old-steer",
                json!({"text":"old text","mode":"steer"})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn attachment_retry_waits_for_acceptance_save_before_observing_a_run() {
    let gate = Gate::new();
    let (_temp, runs, agent, rows) = fixture(ScriptedModel::gated(
        vec![Step::Text(vec!["done"])],
        gate.clone(),
    ))
    .await;
    let app = app(&runs);
    let mut stream = runs.state.read().await.live.subscribe(&agent).unwrap();
    let history = runs.state.read().await.history.clone();
    history
        .flush_once(
            &runs.state,
            &runs.control_plane_transactions(),
            anima_core::primitives::now_millis(),
        )
        .await
        .unwrap();
    let save = runs
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let input = json!({"text":"raw","attachmentIds":[rows[0].id]});
    let first = tokio::spawn(
        app.clone()
            .oneshot(request(&agent, "saving", input.clone())),
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), save.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let mut retry = tokio::spawn(app.oneshot(request(&agent, "saving", input)));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut retry)
            .await
            .is_err(),
        "a retry cannot observe uncommitted acceptance"
    );
    save.release.add_permits(1);
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::ACCEPTED);
    assert_eq!(retry.await.unwrap().unwrap().status(), StatusCode::OK);
    gate.entered().await;
    gate.release();
    events_until(&mut stream, "run.completed").await;
}

#[tokio::test]
async fn attachment_disconnected_telegram_replay_matches_full_committed_input() {
    let (_temp, runs, agent, rows) = fixture(ScriptedModel::new(vec![])).await;
    {
        let mut guard = runs.state.write().await;
        guard.sessions.get_mut(&agent, "chat:files").unwrap().kind =
            crate::sessions::SessionKind::Telegram;
        let mut record = crate::runs::RunRecord::queued(
            crate::runs::RunStart {
                agent_id: agent.clone(),
                session_id: "chat:files".into(),
                source: crate::runs::RunSource::Telegram,
                source_ref: Some("disconnected".into()),
                idempotency_key: Some("telegram-key".into()),
                text: "raw".into(),
                model: "model".into(),
                provider: None,
                parent_run_id: None,
            },
            anima_core::primitives::now_millis(),
        );
        record.input.attachment_ids = vec![rows[1].id.clone(), rows[0].id.clone()];
        record.input.skill = Some("raw-skill".into());
        guard.runs.insert(record);
    }
    let app = app(&runs);
    assert_eq!(
        app.clone()
            .oneshot(request(
                &agent,
                "telegram-key",
                json!({"text":"raw","attachmentIds":[rows[1].id,rows[0].id],"skill":"raw-skill"})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for input in [
        json!({"text":"raw","attachmentIds":[rows[0].id,rows[1].id],"skill":"raw-skill"}),
        json!({"text":"raw","attachmentIds":[rows[1].id,rows[0].id]}),
        json!({"text":"raw","skill":"raw-skill"}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(&agent, "telegram-key", input))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }
}
