//! Actual child-process crash acceptance. Only owned temporary profiles are used.
use super::*;
use anima_core::{
    AgentConfig, Content, DataValue, MessageRole, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, TokenUsage, ToolCall,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Instant,
};

const TOKEN: &str = "isolated-turn-crash-owner";
pub(super) fn durable_marker(root: &Path, name: &str, value: &str) {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(name))
        .unwrap();
    writeln!(f, "{value}").unwrap();
    f.sync_all().unwrap();
}
struct CrashModel {
    root: PathBuf,
    after_effect: bool,
}
#[async_trait]
impl ModelAdapter for CrashModel {
    fn provider(&self) -> &str {
        "crash-fixture"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        r: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let input = r
            .messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.text.as_str())
            .unwrap_or("");
        let is_write = input.contains("CRASH_WRITE");
        let effect_id = input
            .split_whitespace()
            .find_map(|word| word.strip_prefix("EFFECT_ID="))
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 80
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
        let has_result = r
            .messages
            .iter()
            .rev()
            .take_while(|m| m.role != MessageRole::User)
            .any(|m| m.role == MessageRole::Tool);
        durable_marker(&self.root,"calls.jsonl",&json!({"effectId":effect_id,"write":is_write,"result":has_result,"safe":input.contains("SAFE_INPUT"),"block":input.contains("BLOCK_INPUT")}).to_string());
        if input.contains("BLOCK_INPUT") {
            durable_marker(&self.root, "blocked", "model-entered");
            std::future::pending::<()>().await;
        }
        if is_write && has_result && self.after_effect {
            durable_marker(&self.root, "effect-settled", "receipt-before-final");
            std::future::pending::<()>().await;
        }
        let calls =
            (is_write && !has_result).then(|| {
                vec![ToolCall {
                    id: effect_id.map_or_else(|| "crash-write".into(), |id| format!("effect-{id}")),
                    name: "write_file".into(),
                    args: BTreeMap::from([
                        (
                            "file_path".into(),
                            DataValue::String(effect_id.map_or_else(
                                || "effect.txt".into(),
                                |id| format!("effect-{id}.txt"),
                            )),
                        ),
                        (
                            "content".into(),
                            DataValue::String(effect_id.map_or_else(
                                || "exact-approved-effect".into(),
                                |id| format!("effect:{id}"),
                            )),
                        ),
                    ]),
                }]
            });
        Ok(ModelGenerateResponse {
            content: Content {
                text: if calls.is_some() {
                    String::new()
                } else {
                    "fixture-complete".into()
                },
                ..Default::default()
            },
            stop_reason: if calls.is_some() {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            },
            tool_calls: calls,
            usage: TokenUsage {
                prompt_tokens: 7,
                completion_tokens: 3,
                total_tokens: 10,
                ..Default::default()
            },
        })
    }
}

/// Executed only as an owned child by the tests below; never touches an owner profile.
#[test]
#[ignore = "isolated child worker; parent tests supply a new private profile"]
fn turn_crash_worker() {
    let root = PathBuf::from(std::env::var_os("ANIMA_TURN_CRASH_ROOT").expect("parent-owned root"));
    assert!(root.is_absolute() && root.is_dir());
    let stage = std::env::var("ANIMA_TURN_CRASH_STAGE").unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let config = DaemonConfig {
            max_concurrent_runs: 1,
            max_runs_per_agent: 1,
            ..Default::default()
        };
        let state = Arc::new(RwLock::new(configured_state(
            &config,
            Arc::new(CrashModel {
                root: root.clone(),
                after_effect: stage == "after-effect",
            }),
        )));
        configure_persistence(
            &state,
            &config,
            crate::app::persistence::prepare_persistence(&config).unwrap(),
        )
        .await
        .unwrap();
        skip_memory_model_load(&state).await;
        if stage == "reliability-controls" {
            // Test-only fault commands act on real persistence/ownership hooks;
            // no run, approval, receipt or budget row is manufactured.
            let state = state.clone();
            let root = root.clone();
            tokio::spawn(async move {
                let deadline = Instant::now() + Duration::from_secs(120);
                while Instant::now() < deadline {
                    if root.join("arm-save-failure").exists() {
                        let gate = state
                            .write()
                            .await
                            .install_test_control_plane_save_gate(true);
                        durable_marker(&root, "save-armed", "next actual save");
                        tokio::time::timeout(Duration::from_secs(10), gate.entered.acquire())
                            .await
                            .unwrap()
                            .unwrap()
                            .forget();
                        durable_marker(&root, "save-entered", "actual persistence waiting");
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while !root.join("release-save").exists() {
                            assert!(
                                Instant::now() < deadline,
                                "parent did not release owned save gate"
                            );
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        gate.release.add_permits(1);
                        return;
                    }
                    if root.join("lose-owner").exists() {
                        state
                            .read()
                            .await
                            .profile_owner()
                            .expect("owned fixture profile")
                            .mark_lost();
                        durable_marker(&root, "owner-lost", "host writer revoked");
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });
        }
        if stage == "usage-before-flush" {
            let row = crate::usage::usage_record(
                &crate::usage::UsageCall {
                    id: "crash-secondary-usage".into(),
                    agent_id: "retained-usage-owner".into(),
                    session_id: None,
                    run_id: None,
                    source: crate::usage::UsageSource::Title,
                    provider: "fixture".into(),
                    model: "fixture".into(),
                    duration_ms: 1,
                    created_at_ms: anima_core::primitives::now_millis(),
                },
                &TokenUsage {
                    prompt_tokens: 7,
                    completion_tokens: 3,
                    total_tokens: 10,
                    ..Default::default()
                },
                &[],
            );
            let history = state.read().await.history.clone();
            let result = history
                .enqueue_usage_durable(&state, Arc::new(tokio::sync::Mutex::new(())), vec![row])
                .await
                .unwrap();
            assert_eq!(format!("{result:?}"), "Durable");
            durable_marker(&root, "usage-saved", "durable-before-history-worker");
            std::future::pending::<()>().await;
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        fs::write(
            std::env::var_os("ANIMA_TURN_CRASH_READY").unwrap(),
            listener.local_addr().unwrap().to_string(),
        )
        .unwrap();
        serve_with_state(listener, state, config, std::future::pending())
            .await
            .unwrap();
    });
}
pub(super) struct OwnedDaemon {
    child: Child,
    url: String,
    client: reqwest::Client,
}
impl Drop for OwnedDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl OwnedDaemon {
    pub(super) async fn start(root: &Path, stage: &str) -> Self {
        let ready = root.join(format!("ready-{}", uuid::Uuid::new_v4()));
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("worker.log"))
            .unwrap();
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "app::turn_crash_tests::turn_crash_worker",
            "--ignored",
            "--nocapture",
        ])
        .env_clear();
        for key in ["SystemRoot", "PATH", "TEMP", "TMP"] {
            if let Some(v) = std::env::var_os(key) {
                c.env(key, v);
            }
        }
        for key in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
            c.env(key, root.join("home"));
        }
        c.env("ANIMA_TURN_CRASH_ROOT", root)
            .env("ANIMA_TURN_CRASH_STAGE", stage)
            .env("ANIMA_TURN_CRASH_READY", &ready)
            .env("ANIMA_LOCAL_ADMIN_TOKEN", TOKEN)
            .env("ANIMAOS_RS_MEMORY_EMBEDDINGS", "local")
            .env(
                "ANIMAOS_RS_CONTROL_PLANE_FILE",
                root.join("control-plane.json"),
            )
            .env(
                "ANIMAOS_RS_HISTORY_SQLITE_FILE",
                root.join("history.sqlite"),
            )
            .env("ANIMAOS_RS_MEMORY_FILE", root.join("memory.json"))
            .env("ANIMAOS_WORKSPACE_ROOT", root.join("workspace"))
            .env(
                "ANIMAOS_RS_KEYCHAIN_NAMESPACE",
                format!("turn-crash-{}", uuid::Uuid::new_v4()),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000);
        }
        let child = c.spawn().unwrap();
        let mut d = Self {
            child,
            url: String::new(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
        };
        if stage == "usage-before-flush" {
            return d;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(address) = fs::read_to_string(&ready) {
                d.url = format!("http://{address}");
                if d.client
                    .get(format!("{}/health", d.url))
                    .send()
                    .await
                    .is_ok()
                {
                    break;
                }
            }
            assert!(
                d.child.try_wait().unwrap().is_none(),
                "worker exited: {}",
                fs::read_to_string(root.join("worker.log")).unwrap_or_default()
            );
            assert!(Instant::now() < deadline, "worker readiness timeout");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        d
    }
    pub(super) fn kill(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
    pub(super) async fn request(
        &self,
        method: &str,
        route: &str,
        body: Value,
        key: Option<&str>,
    ) -> (u16, Value) {
        let mut r = self
            .client
            .request(method.parse().unwrap(), format!("{}{route}", self.url))
            .bearer_auth(TOKEN);
        if method != "GET" {
            r = r.json(&body);
        }
        if let Some(k) = key {
            r = r.header("Idempotency-Key", k);
        }
        let response = r.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let v = serde_json::from_str(&text).unwrap_or_else(|_| json!({"body":text}));
        (status, v)
    }
    pub(super) async fn api(
        &self,
        method: &str,
        route: &str,
        body: Value,
        key: Option<&str>,
    ) -> Value {
        let (s, v) = self.request(method, route, body, key).await;
        assert!((200..300).contains(&s), "{method} {route}: {s} {v}");
        v
    }
    pub(super) async fn setup(&self, root: &Path) -> (String, String) {
        self.api("PUT","/api/workspace",json!({"rootPath":root.join("workspace"),"companyName":"isolated crash acceptance","mission":"deterministic synthetic state only","values":[]}),None).await;
        let a=self.api("POST","/api/agents",json!({"name":"crash-companion","model":"fixture","provider":"deterministic","tools":["write_file"],"settings":{"agentFilesProfileEnabled":false,"maxToolIterations":3},"fallbacks":[]}),None).await;
        let id = a["agent"]["state"]["id"].as_str().unwrap().to_string();
        self.api(
            "PUT",
            &format!("/api/agents/{id}/autonomy"),
            json!({"mode":"strict","smartCheck":false}),
            None,
        )
        .await;
        self.api(
            "PATCH",
            &format!("/api/agents/{id}/learning-policy"),
            json!({"autoEnabled":false}),
            None,
        )
        .await;
        let s = self
            .api(
                "POST",
                &format!("/api/agents/{id}/sessions"),
                json!({}),
                None,
            )
            .await;
        (id, s["session"]["id"].as_str().unwrap().to_string())
    }
    pub(super) async fn submit(&self, agent: &str, session: &str, text: &str, key: &str) -> Value {
        self.api(
            "POST",
            &format!("/api/agents/{agent}/sessions/{session}/runs"),
            json!({"text":text,"mode":"queue"}),
            Some(key),
        )
        .await["run"]
            .clone()
    }
    pub(super) async fn run(&self, agent: &str, id: &str) -> Value {
        self.api(
            "GET",
            &format!("/api/agents/{agent}/runs/{id}"),
            Value::Null,
            None,
        )
        .await["run"]
            .clone()
    }
    pub(super) async fn wait_run(&self, agent: &str, id: &str, status: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let r = self.run(agent, id).await;
            if r["status"] == status {
                return r;
            }
            assert!(Instant::now() < deadline, "run did not reach {status}: {r}");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
}
pub(super) fn profile() -> tempfile::TempDir {
    let d = tempfile::Builder::new()
        .prefix("anima-turn-crash-")
        .tempdir()
        .unwrap();
    fs::create_dir_all(d.path().join("workspace")).unwrap();
    fs::create_dir_all(d.path().join("home")).unwrap();
    d
}
pub(super) async fn marker(root: &Path, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !root.join(name).exists() {
        assert!(Instant::now() < deadline, "missing process barrier {name}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_process_kill_recovers_only_bound_never_started_input_once() {
    let p = profile();
    let root = p.path();
    let mut d = OwnedDaemon::start(root, "run").await;
    let (a, s) = d.setup(root).await;
    let running = d.submit(&a, &s, "BLOCK_INPUT", "first").await;
    marker(root, "blocked").await;
    let queued = d.submit(&a, &s, "SAFE_INPUT", "second").await;
    assert_eq!(queued["status"], "queued");
    let q = queued["id"].as_str().unwrap();
    let first = running["id"].as_str().unwrap();
    d.kill();
    let mut restored = OwnedDaemon::start(root, "run").await;
    let deadline = Instant::now() + Duration::from_secs(30);
    let child = loop {
        let r = restored
            .api(
                "GET",
                &format!("/api/agents/{a}/runs/{q}/recovery"),
                Value::Null,
                None,
            )
            .await;
        if let Some(id) = r["continuationRunId"].as_str() {
            break id.to_string();
        }
        assert!(
            Instant::now() < deadline,
            "unstarted input not adopted: {r}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    };
    let completed = restored.wait_run(&a, &child, "completed").await;
    assert_eq!(completed["turn"]["rootRunId"], q);
    assert_eq!(restored.run(&a, first).await["status"], "interrupted");
    let replay = restored.submit(&a, &s, "SAFE_INPUT", "second").await;
    assert_eq!(replay["id"], q);
    restored.kill();
    let again = OwnedDaemon::start(root, "run").await;
    assert_eq!(again.run(&a, &child).await["status"], "completed");
    let calls = fs::read_to_string(root.join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls.iter().filter(|c| c["safe"] == true).count(), 1);
    assert_eq!(calls.iter().filter(|c| c["block"] == true).count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_process_kill_preserves_approval_and_requires_fresh_owner_decision() {
    let p = profile();
    let root = p.path();
    let mut d = OwnedDaemon::start(root, "run").await;
    let (a, s) = d.setup(root).await;
    let r = d.submit(&a, &s, "CRASH_WRITE", "write").await;
    let id = r["id"].as_str().unwrap();
    d.wait_run(&a, id, "awaiting_approval").await;
    assert!(!root.join("workspace/effect.txt").exists());
    d.kill();
    let restored = OwnedDaemon::start(root, "run").await;
    let summary = restored
        .api(
            "GET",
            &format!("/api/agents/{a}/runs/{id}/recovery"),
            Value::Null,
            None,
        )
        .await;
    assert_eq!(summary["reviewRequired"], true);
    let route = format!("/api/agents/{a}/runs/{id}/recovery");
    let (status, _) = restored
        .request(
            "POST",
            &route,
            json!({"fingerprint":summary["fingerprint"],"reviewed":false}),
            None,
        )
        .await;
    assert_eq!(status, 409);
    assert!(!root.join("workspace/effect.txt").exists());
    let child = restored
        .api(
            "POST",
            &route,
            json!({"fingerprint":summary["fingerprint"],"reviewed":true}),
            None,
        )
        .await;
    let child = child["run"]["id"].as_str().unwrap();
    restored.wait_run(&a, child, "awaiting_approval").await;
    let pending = restored
        .api(
            "GET",
            &format!("/api/approvals?agentId={a}"),
            Value::Null,
            None,
        )
        .await;
    let approval = pending["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["runId"] == child && x["status"] == "pending")
        .unwrap();
    assert_eq!(approval["tool"], "write_file");
    assert!(!root.join("workspace/effect.txt").exists());
    restored
        .api(
            "POST",
            &format!(
                "/api/approvals/{}/decision",
                approval["id"].as_str().unwrap()
            ),
            json!({"decision":"allow_once","revision":approval["revision"]}),
            None,
        )
        .await;
    let completed = restored.wait_run(&a, child, "completed").await;
    assert_eq!(completed["turn"]["rootRunId"], id);
    assert_eq!(
        fs::read(root.join("workspace/effect.txt")).unwrap(),
        b"exact-approved-effect"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_process_kill_retains_secondary_usage_before_any_flush() {
    let p = profile();
    let root = p.path();
    let mut seed = OwnedDaemon::start(root, "usage-before-flush").await;
    marker(root, "usage-saved").await;
    seed.kill();
    let saved: Value =
        serde_json::from_slice(&fs::read(root.join("control-plane.json")).unwrap()).unwrap();
    assert_eq!(saved["pendingUsage"].as_array().unwrap().len(), 1);
    let mut d = OwnedDaemon::start(root, "run").await;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let state: Value =
            serde_json::from_slice(&fs::read(root.join("control-plane.json")).unwrap()).unwrap();
        if state["pendingUsage"].as_array().unwrap().is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pending usage was not acknowledged"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    d.kill();
    let c = rusqlite::Connection::open(root.join("history.sqlite")).unwrap();
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM usage WHERE id = 'crash-secondary-usage'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let mut again = OwnedDaemon::start(root, "run").await;
    again.kill();
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM usage WHERE id = 'crash-secondary-usage'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_process_kill_after_file_effect_never_replays_the_completed_action() {
    let p = profile();
    let root = p.path();
    let mut d = OwnedDaemon::start(root, "after-effect").await;
    let (a, s) = d.setup(root).await;
    let r = d.submit(&a, &s, "CRASH_WRITE", "settled-write").await;
    let id = r["id"].as_str().unwrap();
    d.wait_run(&a, id, "awaiting_approval").await;
    let pending = d
        .api(
            "GET",
            &format!("/api/approvals?agentId={a}"),
            Value::Null,
            None,
        )
        .await;
    let approval = pending["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["runId"] == id && x["status"] == "pending")
        .unwrap();
    d.api(
        "POST",
        &format!(
            "/api/approvals/{}/decision",
            approval["id"].as_str().unwrap()
        ),
        json!({"decision":"allow_once","revision":approval["revision"]}),
        None,
    )
    .await;
    marker(root, "effect-settled").await;
    assert_eq!(
        fs::read(root.join("workspace/effect.txt")).unwrap(),
        b"exact-approved-effect"
    );
    let before = d.run(&a, id).await;
    assert!(before["actionReceipts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["name"] == "write_file" && r["status"] == "succeeded"));
    d.kill();
    let calls = fs::read_to_string(root.join("calls.jsonl"))
        .unwrap()
        .lines()
        .count();
    let restored = OwnedDaemon::start(root, "run").await;
    let summary = restored
        .api(
            "GET",
            &format!("/api/agents/{a}/runs/{id}/recovery"),
            Value::Null,
            None,
        )
        .await;
    assert_eq!(summary["reviewRequired"], true);
    assert!(summary["continuationRunId"].is_null());
    assert_eq!(restored.run(&a, id).await["status"], "interrupted");
    assert_eq!(
        fs::read_to_string(root.join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        calls
    );
    assert_eq!(
        fs::read(root.join("workspace/effect.txt")).unwrap(),
        b"exact-approved-effect"
    );
}
