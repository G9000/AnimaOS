//! Controlled settled outcomes test evidence attribution, not real test execution.
use super::*;
use anima_core::{AgentRuntime, Evaluator, EvaluatorDecision, Message, MessageRole};
use serde_json::{json, Value};

struct Fixture {
    root: tempfile::TempDir,
    review: HostCompletionReview,
}

fn contract(id: &str, command: &str, directory: &str) -> Value {
    json!({"id":id,"command":command,"cwd":".","files":[],"directories":[directory]})
}

fn json_data(value: Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(value) => DataValue::Bool(value),
        Value::Number(value) => DataValue::Number(value.as_f64().unwrap()),
        Value::String(value) => DataValue::String(value),
        Value::Array(values) => DataValue::Array(values.into_iter().map(json_data).collect()),
        Value::Object(values) => DataValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, json_data(value)))
                .collect(),
        ),
    }
}

impl Fixture {
    fn new(contracts: Value) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = crate::agent_runs::test_support::companion_config("change-bound");
        config.settings.as_mut().unwrap().additional.insert(
            "completionVerificationContracts".into(),
            json_data(contracts),
        );
        let review = HostCompletionReview::new(&config, Some(root.path()));
        Self { root, review }
    }

    fn call(name: &str, path: &str) -> ToolCall {
        ToolCall {
            id: name.into(),
            name: name.into(),
            args: [("file_path".into(), DataValue::String(path.into()))].into(),
        }
    }

    fn check_call(command: &str, cwd: &str) -> ToolCall {
        ToolCall {
            id: command.into(),
            name: "bash".into(),
            args: [
                ("command".into(), DataValue::String(command.into())),
                ("cwd".into(), DataValue::String(cwd.into())),
            ]
            .into(),
        }
    }

    fn write(&self, path: &str, bytes: &[u8]) {
        let call = Self::call("write_file", path);
        let prepared = self.review.prepare(Some(self.root.path()), &call);
        let target = self.root.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
        self.review.settle(prepared, &call, &ok());
    }

    fn read(&self, path: &str) {
        let call = Self::call("read_file", path);
        let prepared = self.review.prepare(Some(self.root.path()), &call);
        self.review.settle(prepared, &call, &ok());
    }

    fn check(&self, command: &str, cwd: &str) {
        let call = Self::check_call(command, cwd);
        let prepared = self.review.prepare(Some(self.root.path()), &call);
        self.review.settle(prepared, &call, &ok());
    }

    async fn pending(&self) -> bool {
        let model = crate::agent_runs::test_support::ScriptedModel::new(vec![]);
        let runtime = AgentRuntime::new(
            crate::agent_runs::test_support::companion_config("change-bound"),
            model,
        );
        let message = Message {
            id: "message".into(),
            agent_id: "agent".into(),
            room_id: "room".into(),
            role: MessageRole::User,
            content: Content::default(),
            created_at_ms: 1,
        };
        matches!(
            self.review
                .evaluate(&runtime, &message, &Content::default())
                .await
                .unwrap()
                .decision,
            EvaluatorDecision::Retry { .. }
        )
    }
}

fn ok() -> TaskResult<Content> {
    TaskResult::success(
        Content {
            text: "controlled settled output".into(),
            ..Default::default()
        },
        0,
    )
}

#[tokio::test]
async fn quality_change_bound_completion_owner_plain_json_contracts_survive_route_and_storage() {
    use crate::{
        app::DaemonConfig,
        control_plane_store::{
            load_control_plane_snapshot, ControlPlaneStoreConfig, WorkspaceConfig,
        },
        state::DaemonState,
    };
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    let root = tempfile::tempdir().unwrap();
    let store = ControlPlaneStoreConfig::Json(root.path().join("control-plane.json"));
    let model = crate::agent_runs::test_support::ScriptedModel::new(vec![]);
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.workspace = Some(WorkspaceConfig {
        root_path: root.path().to_path_buf(),
        company_name: "Contract roundtrip".into(),
        mission: "Verify settings conversion without execution".into(),
        values: vec![],
    });
    daemon.set_control_plane_store(Some(store.clone()));
    let state = Arc::new(RwLock::new(daemon));
    let app = crate::routes::router(state.clone(), DaemonConfig::default());
    let contracts = json!([contract("a", "verify-a", "a")]);
    let request = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method("POST")
            .uri("/api/agents")
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .header("content-type", "application/json"),
    )
    .body(Body::from(
        json!({
            "name":"Contract roundtrip", "model":"offline",
            "settings":{"completionReview":true,"completionVerificationContracts":contracts},
        })
        .to_string(),
    ))
    .unwrap();
    let created = app.clone().oneshot(request).await.unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value =
        serde_json::from_slice(&to_bytes(created.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert_eq!(
        created["agent"]["state"]["config"]["settings"]["additional"]
            ["completionVerificationContracts"],
        contracts
    );
    let id = created["agent"]["state"]["id"].as_str().unwrap();
    let loaded = app
        .oneshot(
            crate::routes::owner_test_support::authenticate(
                Request::builder()
                    .uri(format!("/api/agents/{id}"))
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(loaded.status(), StatusCode::OK);
    let loaded: Value =
        serde_json::from_slice(&to_bytes(loaded.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert_eq!(
        loaded["agent"]["state"]["config"]["settings"]["additional"]
            ["completionVerificationContracts"],
        contracts
    );

    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    let config = &saved
        .agents
        .iter()
        .find(|agent| agent.state.id == id)
        .unwrap()
        .state
        .config;
    assert!(enabled(config));
    assert_eq!(
        crate::routes::data_value_to_json(
            &config.settings.as_ref().unwrap().additional["completionVerificationContracts"]
        ),
        contracts
    );
    let fixture = Fixture {
        review: HostCompletionReview::new(config, Some(root.path())),
        root,
    };
    fixture.write("a/lib.rs", b"a");
    fixture.read("a/lib.rs");
    fixture.check("verify-a", ".");
    assert!(
        !fixture.pending().await,
        "persisted owner contracts reach the production evidence parser"
    );
    assert!(
        model.requests().is_empty(),
        "settings roundtrip must not execute provider work"
    );
}

#[tokio::test]
async fn quality_change_bound_completion_contract_scope_does_not_cover_sibling_project() {
    let fixture = Fixture::new(json!([contract("b", "cargo test -p b", "b")]));
    fixture.write("a/lib.rs", b"a");
    fixture.read("a/lib.rs");
    fixture.check("cargo test -p b", ".");
    assert!(fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_per_file_union_requires_both_projects() {
    for both in [false, true] {
        let fixture = Fixture::new(json!([
            contract("a", "cargo test -p a", "a"),
            contract("b", "bun x nx run b:test", "b"),
        ]));
        fixture.write("a/lib.rs", b"a");
        fixture.write("b/lib.rs", b"b");
        fixture.read("a/lib.rs");
        fixture.read("b/lib.rs");
        fixture.check("cargo test -p a", ".");
        if both {
            fixture.check("bun x nx run b:test", ".");
        }
        assert_eq!(fixture.pending().await, !both);
    }
}

#[tokio::test]
async fn quality_change_bound_completion_exact_command_cwd_and_component_boundary() {
    for (path, command, cwd) in [
        ("a/lib.rs", "cargo test -p a filtered", "."),
        ("a/lib.rs", "cargo test -p a", "a"),
        ("another/lib.rs", "cargo test -p a", "."),
    ] {
        let fixture = Fixture::new(json!([contract("a", "cargo test -p a", "a")]));
        fixture.write(path, b"saved");
        fixture.read(path);
        fixture.check(command, cwd);
        assert!(fixture.pending().await, "{path} {command} {cwd}");
    }
}

#[tokio::test]
async fn quality_change_bound_completion_all_matching_contracts_remain_required() {
    for fail in [false, true] {
        let fixture = Fixture::new(json!([
            contract("tests", "cargo test -p a", "a"),
            contract("lint", "cargo clippy -p a", "a"),
        ]));
        fixture.write("a/lib.rs", b"a");
        fixture.read("a/lib.rs");
        if fail {
            let call = Fixture::check_call("cargo clippy -p a", ".");
            let prepared = fixture.review.prepare(Some(fixture.root.path()), &call);
            fixture.review.settle(
                prepared,
                &call,
                &TaskResult::error("failed or uncertain", 0),
            );
        }
        fixture.check("cargo test -p a", ".");
        assert!(fixture.pending().await);
    }
}

#[tokio::test]
async fn quality_change_bound_completion_all_matching_contracts_can_complete_together() {
    let fixture = Fixture::new(json!([
        contract("tests", "cargo test -p a", "a"),
        contract("lint", "cargo clippy -p a", "a"),
    ]));
    fixture.write("a/lib.rs", b"a");
    fixture.read("a/lib.rs");
    fixture.check("cargo test -p a", ".");
    fixture.check("cargo clippy -p a", ".");
    assert!(!fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_owner_contract_requires_docs_and_data_checks() {
    for path in ["report.md", "report.csv"] {
        for check in ["absent", "failed", "passed"] {
            let fixture = Fixture::new(json!([contract("report", "verify-report", ".")]));
            fixture.write(path, b"saved report\n");
            fixture.read(path);
            if check != "absent" {
                let call = Fixture::check_call("verify-report", ".");
                let prepared = fixture.review.prepare(Some(fixture.root.path()), &call);
                let result = if check == "passed" {
                    ok()
                } else {
                    TaskResult::error("verification failed or unknown", 0)
                };
                fixture.review.settle(prepared, &call, &result);
            }
            assert_eq!(fixture.pending().await, check != "passed", "{path} {check}");
        }
        let fixture = Fixture::new(json!([]));
        fixture.write(path, b"saved report\n");
        fixture.read(path);
        assert!(
            !fixture.pending().await,
            "uncontracted docs retain the read-back exemption"
        );
    }
}

#[tokio::test]
async fn quality_change_bound_completion_cwd_uses_the_native_executors_trim_rules() {
    for declared_cwd in ["a", " a "] {
        let mut definition = contract("a", "verify-a", "a");
        definition["cwd"] = json!(declared_cwd);
        let fixture = Fixture::new(json!([definition]));
        std::fs::create_dir_all(fixture.root.path().join(" a ")).unwrap();
        fixture.write("a/lib.rs", b"a");
        fixture.read("a/lib.rs");
        fixture.check("verify-a", " a ");
        assert_eq!(
            fixture.pending().await,
            declared_cwd != "a",
            "executor runs in trimmed a, contract declares {declared_cwd:?}"
        );
    }
}

#[tokio::test]
async fn quality_change_bound_completion_later_failed_attempt_beats_late_old_pass() {
    let fixture = Fixture::new(json!([contract("a", "cargo test -p a", "a")]));
    fixture.write("a/lib.rs", b"a");
    fixture.read("a/lib.rs");
    let call = Fixture::check_call("cargo test -p a", ".");
    let old = fixture.review.prepare(Some(fixture.root.path()), &call);
    let newer = fixture.review.prepare(Some(fixture.root.path()), &call);
    fixture
        .review
        .settle(newer, &call, &TaskResult::error("outcome unknown", 0));
    fixture.review.settle(old, &call, &ok());
    assert!(fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_external_edits_during_check_or_before_final_are_stale() {
    for during in [true, false] {
        let fixture = Fixture::new(json!([contract("a", "cargo test -p a", "a")]));
        fixture.write("a/lib.rs", b"before");
        fixture.read("a/lib.rs");
        let call = Fixture::check_call("cargo test -p a", ".");
        let prepared = fixture.review.prepare(Some(fixture.root.path()), &call);
        if during {
            std::fs::write(fixture.root.path().join("a/lib.rs"), b"during").unwrap();
        }
        fixture.review.settle(prepared, &call, &ok());
        if !during {
            std::fs::write(fixture.root.path().join("a/lib.rs"), b"after").unwrap();
        }
        assert!(fixture.pending().await);
    }
}

#[tokio::test]
async fn quality_change_bound_completion_changed_read_and_native_reedit_are_stale() {
    for native in [false, true] {
        let fixture = Fixture::new(json!([contract("a", "cargo test -p a", "a")]));
        fixture.write("a/lib.rs", b"a");
        let call = Fixture::call("read_file", "a/lib.rs");
        let reading = fixture.review.prepare(Some(fixture.root.path()), &call);
        if native {
            fixture.write("a/lib.rs", b"b");
        } else {
            std::fs::write(fixture.root.path().join("a/lib.rs"), b"b").unwrap();
        }
        fixture.review.settle(reading, &call, &ok());
        fixture.check("cargo test -p a", ".");
        assert!(fixture.pending().await);
    }
}

#[tokio::test]
async fn quality_change_bound_completion_new_native_version_needs_its_own_check() {
    let fixture = Fixture::new(json!([contract("a", "cargo test -p a", "a")]));
    fixture.write("a/lib.rs", b"a");
    fixture.read("a/lib.rs");
    fixture.check("cargo test -p a", ".");
    fixture.write("a/lib.rs", b"b");
    fixture.read("a/lib.rs");
    assert!(fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_absolute_and_dot_aliases_share_actual_file_version() {
    let fixture = Fixture::new(json!([contract("a", "cargo test -p a", "a")]));
    fixture.write("a/lib.rs", b"a");
    fixture.read(fixture.root.path().join("a/lib.rs").to_str().unwrap());
    fixture.read("./a/./lib.rs");
    fixture.check("cargo test -p a", fixture.root.path().to_str().unwrap());
    assert!(!fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_missing_or_oversized_contracts_cannot_claim_coverage() {
    for contracts in [
        json!([]),
        json!(vec![contract("duplicate", "cargo test", "."); 33]),
    ] {
        let fixture = Fixture::new(contracts);
        fixture.write("a/lib.rs", b"a");
        fixture.read("a/lib.rs");
        fixture.check("cargo test", ".");
        assert!(fixture.pending().await);
    }
}

#[tokio::test]
async fn quality_change_bound_completion_outside_workspace_call_cannot_cover_owned_file() {
    let fixture = Fixture::new(json!([contract("a", "cargo test", "a")]));
    fixture.write("a/lib.rs", b"a");
    fixture.read("a/lib.rs");
    let foreign = tempfile::tempdir().unwrap();
    let call = Fixture::check_call("cargo test", ".");
    let prepared = fixture.review.prepare(Some(foreign.path()), &call);
    fixture.review.settle(prepared, &call, &ok());
    assert!(fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_file_contract_does_not_cover_other_files() {
    let fixture = Fixture::new(json!([{
        "id":"one", "command":"cargo test", "cwd":".",
        "files":["a/lib.rs"], "directories":[],
    }]));
    fixture.write("a/other.rs", b"other");
    fixture.read("a/other.rs");
    fixture.check("cargo test", ".");
    assert!(fixture.pending().await);
}

#[tokio::test]
async fn quality_change_bound_completion_snapshot_limits_leave_visible_gap() {
    let fixture = Fixture::new(json!([contract("all", "cargo test", ".")]));
    fixture.write("large.rs", &vec![b'x'; 1024 * 1024 + 1]);
    fixture.read("large.rs");
    fixture.check("cargo test", ".");
    assert!(fixture.pending().await);
}

#[test]
fn quality_change_bound_completion_contract_paths_are_relative_and_bounded() {
    for path in ["../a", "a/../b", "/a", "a*", "a//b", "a\\..\\b"] {
        let fixture = Fixture::new(json!([contract("a", "cargo test", path)]));
        assert!(
            fixture.review.contracts.is_empty(),
            "invalid declared scope: {path}"
        );
    }
}
