//! Helper worktrees end to end: a real companion run spawns a helper whose
//! scripted model writes through the native file tool, with real git.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use anima_core::{
    AgentConfig, Content, DataValue, MessageRole, ModelAdapter, ModelGenerateRequest,
    ModelGenerateResponse, ModelStopReason, TokenUsage, ToolCall,
};
use serde_json::Value;
use tokio::sync::{RwLock, Semaphore};

use super::test_support::{accept_web, add_chat, lead_config, wait_for};
use super::AgentRunCoordinator;
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::runs::RunStatus;
use crate::state::DaemonState;
use crate::tools::ToolRegistry;

const HELPER_FILE: &str = "helper-notes.txt";

struct WorktreeModel {
    launch: ToolCall,
    helper_writes: bool,
    owner: AtomicUsize,
    helper: AtomicUsize,
    helper_task: StdMutex<Option<String>>,
    helper_tools: StdMutex<Vec<String>>,
    launch_result: StdMutex<Option<String>>,
}

fn response(text: &str, calls: Option<Vec<ToolCall>>) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: Content {
            text: text.into(),
            ..Default::default()
        },
        stop_reason: if calls.is_some() {
            ModelStopReason::ToolCall
        } else {
            ModelStopReason::End
        },
        tool_calls: calls,
        usage: TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
            ..Default::default()
        },
    }
}

#[async_trait::async_trait]
impl ModelAdapter for WorktreeModel {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
        true
    }
    fn provider(&self) -> &str {
        "worktree-offline"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        if super::is_helper_config(config) {
            let step = self.helper.fetch_add(1, Ordering::SeqCst);
            if step == 0 {
                *self.helper_task.lock().unwrap() = request
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == MessageRole::User)
                    .map(|message| message.content.text.clone());
                *self.helper_tools.lock().unwrap() = config
                    .tools
                    .iter()
                    .flatten()
                    .map(|tool| tool.name.clone())
                    .collect();
                if self.helper_writes {
                    return Ok(response(
                        "",
                        Some(vec![ToolCall {
                            id: "helper-write".into(),
                            name: "write_file".into(),
                            args: BTreeMap::from([
                                ("file_path".into(), DataValue::String(HELPER_FILE.into())),
                                (
                                    "content".into(),
                                    DataValue::String("written in the worktree\n".into()),
                                ),
                            ]),
                        }]),
                    ));
                }
            }
            return Ok(response("Helper finished.", None));
        }
        match self.owner.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(response("", Some(vec![self.launch.clone()]))),
            1 => {
                *self.launch_result.lock().unwrap() = request
                    .messages
                    .iter()
                    .find(|message| {
                        message.role == MessageRole::Tool
                            && message
                                .content
                                .metadata
                                .as_ref()
                                .and_then(|metadata| metadata.get("toolCallId"))
                                == Some(&DataValue::String(self.launch.id.clone()))
                    })
                    .map(|message| message.content.text.clone());
                Ok(response("Companion done.", None))
            }
            _ => Ok(response("Companion done.", None)),
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn launch(tool: &str, repo: bool) -> ToolCall {
    let mut args = BTreeMap::from([
        ("name".into(), DataValue::String("Worktree writer".into())),
        (
            "task".into(),
            DataValue::String("Write the helper notes file.".into()),
        ),
        ("worktree".into(), DataValue::Bool(true)),
    ]);
    if repo {
        args.insert("repo".into(), DataValue::String(".".into()));
    }
    if tool == "spawn_helper_async" {
        args.insert("completion".into(), DataValue::String("manual".into()));
    }
    ToolCall {
        id: "launch-worktree-helper".into(),
        name: tool.into(),
        args,
    }
}

struct Fixture {
    _private: tempfile::TempDir,
    root: PathBuf,
    coordinator: AgentRunCoordinator,
    model: Arc<WorktreeModel>,
    owner: String,
}

async fn fixture(tool: &str, helper_writes: bool, init_git: bool) -> Fixture {
    let private = tempfile::tempdir().unwrap();
    let root = private.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("README.md"), "main checkout\n").unwrap();
    if init_git {
        git(&root, &["init", "--quiet"]);
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "--quiet", "-m", "init"]);
    }
    let model = Arc::new(WorktreeModel {
        launch: launch(tool, true),
        helper_writes,
        owner: AtomicUsize::new(0),
        helper: AtomicUsize::new(0),
        helper_task: StdMutex::new(None),
        helper_tools: StdMutex::new(Vec::new()),
        launch_result: StdMutex::new(None),
    });
    let mut state = DaemonState::with_model_adapter(model.clone());
    state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: root.clone(),
        ..Default::default()
    });
    state.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            private.path().join("control-plane.json"),
        ),
    ));
    let mut config = lead_config("Worktree companion");
    let mut tools = vec!["read_file", "write_file", "todo_write"];
    if tool == "spawn_helper_async" {
        tools.push("spawn_helper_async");
    }
    config.tools = Some(ToolRegistry::new().resolve_descriptors(tools).unwrap());
    config.settings.as_mut().unwrap().max_retries = Some(0);
    let owner = state.create_agent(config).unwrap().state.id;
    state.approvals.set_policy(
        &owner,
        ApprovalPolicy::default()
            .with(RiskClass::Delegate, PolicyAction::Allow)
            .with(RiskClass::Write, PolicyAction::Allow),
    );
    let coordinator =
        AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(3)));
    add_chat(&coordinator, &owner, "chat:worktree").await;
    Fixture {
        _private: private,
        root,
        coordinator,
        model,
        owner,
    }
}

impl Fixture {
    async fn run_parent(&self) {
        let run = accept_web(
            &self.coordinator,
            &self.owner,
            "chat:worktree",
            "Use a worktree helper",
        )
        .await;
        wait_for(&self.coordinator, &run, RunStatus::Completed).await;
    }

    fn launch_result(&self) -> String {
        self.model
            .launch_result
            .lock()
            .unwrap()
            .clone()
            .expect("the launch result reached the companion")
    }

    fn worktree_path(&self, report: &Value) -> PathBuf {
        self.root.join(report["path"].as_str().unwrap())
    }
}

#[tokio::test]
async fn a_worktree_helper_writes_into_its_own_checkout_and_the_worktree_is_kept() {
    let fixture = fixture("spawn_helper", true, true).await;
    fixture.run_parent().await;
    let result: Value = serde_json::from_str(&fixture.launch_result()).unwrap();
    assert_eq!(result["status"], "success", "{result}");
    let report = &result["worktree"];
    assert_eq!(report["removed"], false, "{report}");
    assert!(report["path"]
        .as_str()
        .unwrap()
        .starts_with(".worktrees/anima-helper-"));
    assert!(report["branch"]
        .as_str()
        .unwrap()
        .starts_with("anima-helper/"));
    assert_eq!(
        report["baseCommit"],
        git(&fixture.root, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        report["changedFiles"],
        serde_json::json!([{"path": HELPER_FILE, "status": "untracked"}])
    );
    assert!(report["note"].as_str().unwrap().contains("git merge"));
    // The file landed in the worktree, never in the main checkout.
    let worktree = fixture.worktree_path(report);
    assert_eq!(
        std::fs::read_to_string(worktree.join(HELPER_FILE)).unwrap(),
        "written in the worktree\n"
    );
    assert!(!fixture.root.join(HELPER_FILE).exists());
    assert_eq!(git(&fixture.root, &["status", "--porcelain"]), "");
    // The helper was told where it works, and lost the shared task list.
    let task = fixture.model.helper_task.lock().unwrap().clone().unwrap();
    assert!(
        task.contains("[Worktree]") && task.contains("cannot run git"),
        "{task}"
    );
    let tools = fixture.model.helper_tools.lock().unwrap().clone();
    assert!(tools.contains(&"write_file".to_string()), "{tools:?}");
    assert!(
        !tools.iter().any(|tool| tool.starts_with("todo_")),
        "{tools:?}"
    );
    // The repository is remembered for the startup sweep.
    let repos = fixture
        .coordinator
        .state
        .read()
        .await
        .helper_worktree_repos
        .clone();
    assert_eq!(repos.len(), 1);
}

#[tokio::test]
async fn a_worktree_helper_that_changes_nothing_has_its_worktree_removed() {
    let fixture = fixture("spawn_helper", false, true).await;
    fixture.run_parent().await;
    let result: Value = serde_json::from_str(&fixture.launch_result()).unwrap();
    let report = &result["worktree"];
    assert_eq!(report["removed"], true, "{report}");
    assert_eq!(report["changedFilesTotal"], 0);
    assert!(!fixture.worktree_path(report).exists());
    let branches = git(&fixture.root, &["branch", "--list", "anima-helper/*"]);
    assert_eq!(branches, "");
}

#[tokio::test]
async fn a_worktree_outside_a_git_repository_is_refused() {
    let fixture = fixture("spawn_helper", true, false).await;
    fixture.run_parent().await;
    let result = fixture.launch_result();
    assert!(
        result.contains("Could not create a helper worktree")
            && result.contains("not inside a git repository"),
        "{result}"
    );
    assert_eq!(fixture.model.helper.load(Ordering::SeqCst), 0);
    assert!(!fixture.root.join(HELPER_FILE).exists());
    let guard = fixture.coordinator.state.read().await;
    assert!(
        guard
            .list_agents()
            .iter()
            .all(|agent| super::helper_parent(&agent.state).is_none()),
        "no helper was created"
    );
}

#[tokio::test]
async fn an_async_worktree_assignment_saves_its_report_with_the_helper_run() {
    let fixture = fixture("spawn_helper_async", true, true).await;
    fixture.run_parent().await;
    let handle: Value = serde_json::from_str(&fixture.launch_result()).unwrap();
    let child = handle["runId"].as_str().unwrap().to_string();
    wait_for(&fixture.coordinator, &child, RunStatus::Completed).await;
    let record = fixture
        .coordinator
        .state
        .read()
        .await
        .runs
        .get(&child)
        .cloned()
        .unwrap();
    let worktree = record.async_helper.unwrap().worktree.unwrap();
    assert!(record.input.text.contains("[Worktree]"));
    let report = worktree.public_json(false);
    assert_eq!(report["removed"], false, "{report}");
    assert_eq!(report["changedFiles"][0]["path"], HELPER_FILE);
    assert!(worktree.worktree.path.join(HELPER_FILE).is_file());
    assert!(!fixture.root.join(HELPER_FILE).exists());
}

#[test]
fn run_root_only_trusts_helper_worktree_roots() {
    let fixture = tempfile::tempdir().unwrap();
    let repo_root = fixture.path().join("repo");
    let worktree_path = repo_root.join(".worktrees").join("anima-helper-abc12345");
    assert!(worktree_path.is_absolute());
    let parent = anima_core::AgentState {
        id: "lead".into(),
        name: "Lead".into(),
        status: anima_core::AgentStatus::Idle,
        config: lead_config("Lead"),
        created_at_ms: 0,
        token_usage: Default::default(),
    };
    let worktree = super::worktrees::HelperWorktree {
        worktree: anima_worktree::Worktree {
            name: "abc12345".into(),
            repo_root,
            path: worktree_path.clone(),
            branch: "anima-helper/abc12345".into(),
            base_commit: "0123abcd".into(),
        },
        root: worktree_path.join("sub"),
        display_path: ".worktrees/anima-helper-abc12345".into(),
        report: None,
    };
    let helper = super::helper_config_in(&parent, "Helper".into(), Some(&worktree));
    assert_eq!(
        super::worktrees::run_root(&helper),
        Some(worktree.root.clone())
    );
    assert_eq!(
        super::worktrees::run_root(&super::helper_config(&parent, "Plain".into())),
        None
    );
    // A non-helper config, or a root outside an anima-helper worktree, is ignored.
    let mut lead = parent.config.clone();
    lead.settings
        .as_mut()
        .unwrap()
        .additional
        .extend(super::worktrees::config_entries(&worktree));
    assert_eq!(super::worktrees::run_root(&lead), None);
    let mut forged = worktree.clone();
    forged.worktree.path = fixture.path().join("outside");
    forged.root = forged.worktree.path.clone();
    assert_eq!(
        super::worktrees::run_root(&super::helper_config_in(
            &parent,
            "Helper".into(),
            Some(&forged)
        )),
        None
    );
}

#[tokio::test]
async fn profile_owner_custody_revocation_preserves_sweep_worktrees() {
    let fixture = fixture("spawn_helper", false, true).await;
    let worktree = super::worktrees::create(Some(fixture.root.clone()), ".".into(), None)
        .await
        .unwrap();
    let owner = crate::profile_owner::ProfileOwner::acquire([fixture
        ._private
        .path()
        .join("custody-profile.json")])
    .unwrap();
    fixture.coordinator.state.write().await.profile_owner = Some(owner.clone());
    let before = git(&fixture.root, &["worktree", "list", "--porcelain"]);
    owner.mark_lost();

    super::worktrees::startup_sweep(fixture.coordinator.state.clone()).await;

    assert!(
        worktree.worktree.path.exists(),
        "a revoked profile removed the helper worktree during startup sweep"
    );
    assert_eq!(
        git(&fixture.root, &["worktree", "list", "--porcelain"]),
        before
    );
}

#[test]
fn profile_owner_custody_queued_sweep_retains_lease_after_waiter_drop() {
    let private = tempfile::tempdir().unwrap();
    let resource = private.path().join("custody-profile.json");
    let owner = crate::profile_owner::ProfileOwner::acquire([resource.clone()]).unwrap();
    let mut daemon = DaemonState::new();
    daemon.profile_owner = Some(owner.clone());
    let state = Arc::new(RwLock::new(daemon));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let retained = runtime.block_on(async move {
        let (started, running) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = started.send(());
            let _ = held.recv_timeout(std::time::Duration::from_secs(10));
        });
        running.await.unwrap();
        let mut sweep = Box::pin(super::worktrees::startup_sweep(state));
        assert!(futures::poll!(sweep.as_mut()).is_pending());
        // The worker is queued, but its caller and daemon state are gone.
        drop(sweep);
        drop(owner);
        let retained = crate::profile_owner::ProfileOwner::acquire([resource.clone()]).is_err();
        release.send(()).unwrap();
        blocker.await.unwrap();
        retained
    });
    // Runtime drop waits for the detached blocking worker to actually exit.
    drop(runtime);
    assert!(
        retained,
        "dropping the sweep waiter released a queued worker's profile lease"
    );
    crate::profile_owner::ProfileOwner::acquire([private.path().join("custody-profile.json")])
        .expect("the lease releases after the worker exits");
}

#[test]
fn profile_owner_custody_queued_sweep_rechecks_revocation_before_git() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = fixture("spawn_helper", false, true).await;
        let worktree = super::worktrees::create(Some(fixture.root.clone()), ".".into(), None)
            .await
            .unwrap();
        let owner = crate::profile_owner::ProfileOwner::acquire([fixture
            ._private
            .path()
            .join("custody-profile.json")])
        .unwrap();
        fixture.coordinator.state.write().await.profile_owner = Some(owner.clone());
        let (started, running) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = started.send(());
            let _ = held.recv_timeout(std::time::Duration::from_secs(10));
        });
        running.await.unwrap();
        let mut sweep = Box::pin(super::worktrees::startup_sweep(
            fixture.coordinator.state.clone(),
        ));
        assert!(futures::poll!(sweep.as_mut()).is_pending());
        // Admission succeeded, then authority was revoked while the worker queued.
        owner.mark_lost();
        release.send(()).unwrap();
        blocker.await.unwrap();
        sweep.await;
        assert!(
            worktree.worktree.path.exists(),
            "the queued sweep used authority checked only before spawn_blocking"
        );
    });
}

#[tokio::test]
async fn profile_owner_custody_revocation_refuses_worktree_create() {
    let fixture = fixture("spawn_helper", false, true).await;
    let owner = crate::profile_owner::ProfileOwner::acquire([fixture
        ._private
        .path()
        .join("custody-profile.json")])
    .unwrap();
    owner.mark_lost();

    let result =
        super::worktrees::create(Some(fixture.root.clone()), ".".into(), Some(owner)).await;

    assert!(result.is_err());
    assert!(!fixture.root.join(".worktrees").exists());
    assert_eq!(
        git(&fixture.root, &["branch", "--list", "anima-helper/*"]),
        ""
    );
}

#[tokio::test]
async fn profile_owner_custody_revocation_preserves_finished_worktree() {
    let fixture = fixture("spawn_helper", false, true).await;
    let worktree = super::worktrees::create(Some(fixture.root.clone()), ".".into(), None)
        .await
        .unwrap();
    let owner = crate::profile_owner::ProfileOwner::acquire([fixture
        ._private
        .path()
        .join("custody-profile.json")])
    .unwrap();
    owner.mark_lost();

    let result = super::worktrees::finish(&worktree, Some(owner)).await;

    assert_eq!(result["inspectionFailed"], true);
    assert_eq!(result["removed"], false);
    assert!(worktree.worktree.path.exists());
    assert_eq!(
        git(
            &fixture.root,
            &[
                "branch",
                "--format=%(refname:short)",
                "--list",
                "anima-helper/*"
            ]
        ),
        worktree.worktree.branch
    );
}
