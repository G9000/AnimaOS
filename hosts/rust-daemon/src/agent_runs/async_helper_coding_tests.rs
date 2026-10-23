//! Synthetic delegated coding acceptance. File tools and optional installed
//! compiler are real; scripted answers establish no autonomous coding claim.
use super::*;
use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
use anima_core::{
    AgentConfig, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, ModelStopReason,
    TokenUsage,
};

const RUST_SOURCE: &str = "pub fn total_open_cents(rows: &[(bool, i64)]) -> i64 {\n    rows.iter().filter(|(voided, _)| !voided).map(|(_, cents)| *cents).sum()\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn exact_integer_cents_exclude_voids() {\n        assert_eq!(super::total_open_cents(&[(false, 199), (true, 900), (false, 1)]), 200);\n    }\n    #[test]\n    fn empty_has_zero_total() { assert_eq!(super::total_open_cents(&[]), 0); }\n    #[test]\n    fn all_void_has_zero_total() { assert_eq!(super::total_open_cents(&[(true, 777)]), 0); }\n}\n";

fn file_call(id: &str, name: &str) -> ToolCall {
    let mut args = BTreeMap::from([("file_path".into(), DataValue::String("invoice.rs".into()))]);
    if name == "write_file" {
        args.insert("content".into(), DataValue::String(RUST_SOURCE.into()));
    }
    ToolCall {
        id: id.into(),
        name: name.into(),
        args,
    }
}

struct CodingModel {
    total: std::sync::atomic::AtomicUsize,
    owner: std::sync::atomic::AtomicUsize,
    helper: std::sync::atomic::AtomicUsize,
    gate: super::super::test_support::Gate,
    command: Option<String>,
    requests: std::sync::Mutex<Vec<ModelGenerateRequest>>,
}

#[async_trait::async_trait]
impl ModelAdapter for CodingModel {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }
    fn provider(&self) -> &str {
        "fp05-offline-coding"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let count = self.total.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(
            count <= if self.command.is_some() { 7 } else { 6 },
            "fixed offline recording ceiling"
        );
        self.requests.lock().unwrap().push(request.clone());
        let (text, calls) = if super::super::is_helper_config(config) {
            match self.helper.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    self.gate.entered.add_permits(1);
                    self.gate.release.acquire().await.unwrap().forget();
                    ("".into(), Some(vec![file_call("coding-child-write", "write_file")]))
                }
                1 => ("invoice.rs was written through the native file tool; independent validation remains with the companion.".into(), None),
                _ => panic!("helper exceeded its two recorded calls"),
            }
        } else {
            match self.owner.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    let mut call = launch("coding-parent-launch", "Create invoice.rs with total_open_cents over (voided, integer cents) rows. Exclude voided rows and add empty/all-void/exact-cents tests. Use native write_file only; report the saved path and limits.");
                    call.args
                        .insert("completion".into(), DataValue::String("chat".into()));
                    ("".into(), Some(vec![call]))
                }
                1 => (
                    "The delegated coding task was accepted. Its result is pending.".into(),
                    None,
                ),
                2 => (
                    "".into(),
                    Some(vec![file_call("coding-completion-read", "read_file")]),
                ),
                3 => {
                    let readback = request
                        .messages
                        .iter()
                        .find(|message| {
                            message.role == MessageRole::Tool
                                && message
                                    .content
                                    .metadata
                                    .as_ref()
                                    .and_then(|metadata| metadata.get("toolCallId"))
                                    == Some(&DataValue::String("coding-completion-read".into()))
                        })
                        .expect("actual native readback reaches the model");
                    assert!(
                        readback.content.text.contains("total_open_cents")
                            && readback
                                .content
                                .text
                                .contains("exact_integer_cents_exclude_voids")
                    );
                    if let Some(command) = &self.command {
                        (
                            "".into(),
                            Some(vec![ToolCall {
                                id: "coding-compile-and-test".into(),
                                name: "bash".into(),
                                args: BTreeMap::from([
                                    ("command".into(), DataValue::String(command.clone())),
                                    ("cwd".into(), DataValue::String(".".into())),
                                    ("timeout".into(), DataValue::Number(20_000.0)),
                                ]),
                            }]),
                        )
                    } else {
                        ("Native readback confirmed the generated source. Compiler validation is pending.".into(), None)
                    }
                }
                4 if self.command.is_some() => {
                    let output = request
                        .messages
                        .iter()
                        .find(|message| {
                            message.role == MessageRole::Tool
                                && message
                                    .content
                                    .metadata
                                    .as_ref()
                                    .and_then(|metadata| metadata.get("toolCallId"))
                                    == Some(&DataValue::String("coding-compile-and-test".into()))
                        })
                        .expect("actual owned process output reaches completion");
                    assert!(
                        output
                            .content
                            .text
                            .contains("test result: ok. 3 passed; 0 failed"),
                        "{}",
                        output.content.text
                    );
                    ("Native readback and the explicitly approved installed compiler/test run succeeded.".into(), None)
                }
                _ => panic!("companion exceeded its finite recorded scenario"),
            }
        };
        Ok(ModelGenerateResponse {
            content: Content {
                text,
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
        })
    }
}

async fn coding_acceptance(compiler: Option<std::path::PathBuf>) {
    let private = tempfile::tempdir().unwrap();
    let root = private.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("sentinel.txt"),
        b"preserve this independent file\n",
    )
    .unwrap();
    let command = compiler.map(|compiler| {
        assert!(compiler.is_absolute() && compiler.is_file(), "root selects an explicit installed compiler");
        let executable = compiler.to_str().unwrap().replace('\\', "/");
        assert!(!executable.chars().any(|c| matches!(c, '\r' | '\n' | '\0')));
        let quoted = format!("'{}'", executable.replace('\'', "'\"'\"'"));
        format!("test -n \"$BASH_VERSION\" || exit 90\n{quoted} --edition=2021 --test invoice.rs -o invoice-tests.exe && ./invoice-tests.exe --nocapture")
    });
    let gate = super::super::test_support::Gate::new();
    let model = Arc::new(CodingModel {
        total: 0.into(),
        owner: 0.into(),
        helper: 0.into(),
        gate: gate.clone(),
        command: command.clone(),
        requests: Default::default(),
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
    let mut config = lead_config("Offline Rust coding companion");
    config.tools = Some(
        ToolRegistry::new()
            .resolve_descriptors(["spawn_helper_async", "read_file", "write_file", "bash"])
            .unwrap(),
    );
    config.settings.as_mut().unwrap().max_retries = Some(0);
    let owner = state.create_agent(config).unwrap().state.id;
    state.approvals.set_policy(
        &owner,
        ApprovalPolicy::default()
            .with(RiskClass::Delegate, PolicyAction::Allow)
            .with(RiskClass::Write, PolicyAction::Allow)
            .with(RiskClass::Exec, PolicyAction::Ask),
    );
    let coordinator =
        AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(3)));
    add_chat(&coordinator, &owner, "chat:offline-coding").await;
    let parent = super::super::test_support::accept_web(
        &coordinator,
        &owner,
        "chat:offline-coding",
        "Delegate the bounded Rust invoice module",
    )
    .await;
    gate.entered().await;
    wait_for(&coordinator, &parent, RunStatus::Completed).await;
    assert!(
        !root.join("invoice.rs").exists(),
        "the parent finished while actual child file work was still pending"
    );
    let child = coordinator
        .state
        .read()
        .await
        .control_plane_snapshot()
        .runs
        .into_iter()
        .find(|record| record.async_helper.is_some())
        .unwrap();
    let continuation = child
        .async_helper
        .as_ref()
        .unwrap()
        .completion
        .run_id()
        .unwrap()
        .to_string();
    gate.release();
    if command.is_some() {
        let approvals = super::super::test_support::pending_approvals(&coordinator, 1).await;
        assert_eq!(approvals[0].run_id, continuation);
        assert_eq!(approvals[0].tool_call_id, "coding-compile-and-test");
        assert_eq!(approvals[0].class, RiskClass::Exec);
        assert!(
            !root.join("invoice-tests.exe").exists(),
            "the compiler has not run before this new approval"
        );
        coordinator
            .decide_approval(
                &approvals[0].id,
                super::super::test_support::decision(
                    ApprovalDecisionKind::AllowOnce,
                    approvals[0].revision,
                ),
            )
            .await
            .unwrap();
    }
    if command.is_some() {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let record = coordinator
                    .state
                    .read()
                    .await
                    .runs
                    .get(&continuation)
                    .unwrap()
                    .clone();
                if record.status.is_terminal() {
                    assert_eq!(record.status, RunStatus::Completed, "{record:?}");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("20-second owned compiler command and bounded cleanup settle within 30 seconds");
    } else {
        wait_for(&coordinator, &continuation, RunStatus::Completed).await;
    }
    // Independent file/output oracle, separate from generated model prose.
    assert_eq!(
        std::fs::read(root.join("invoice.rs")).unwrap(),
        RUST_SOURCE.as_bytes()
    );
    assert_eq!(
        std::fs::read(root.join("sentinel.txt")).unwrap(),
        b"preserve this independent file\n"
    );
    assert_eq!(
        model.total.load(Ordering::SeqCst),
        if command.is_some() { 7 } else { 6 }
    );
    let guard = coordinator.state.read().await;
    let source = guard.runs.get(&child.id).unwrap();
    let write = source
        .action_receipts
        .iter()
        .find(|receipt| receipt.tool_call_id == "coding-child-write")
        .unwrap();
    assert_eq!(write.status, crate::runs::ActionStatus::Succeeded);
    assert!(
        matches!(&source.async_helper.as_ref().unwrap().completion, crate::runs::HelperCompletionState::Settled { run_id } if run_id == &continuation)
    );
    if command.is_some() {
        let callback = guard.runs.get(&continuation).unwrap();
        let receipt = callback
            .action_receipts
            .iter()
            .find(|receipt| receipt.tool_call_id == "coding-compile-and-test")
            .unwrap();
        assert_eq!(receipt.status, crate::runs::ActionStatus::Succeeded);
        let output = receipt.result_preview.as_ref().unwrap();
        for literal in [
            "exact_integer_cents_exclude_voids",
            "empty_has_zero_total",
            "all_void_has_zero_total",
            "test result: ok. 3 passed; 0 failed",
        ] {
            assert!(
                output.contains(literal),
                "owned compiler/test output must contain {literal}: {output}"
            );
        }
    }
}

#[tokio::test]
async fn async_helper_coding_native_write_and_completion_readback_have_independent_literal_oracle()
{
    coding_acceptance(None).await;
}

#[tokio::test]
#[ignore = "root explicitly selects installed rustc after owned shell custody validation; no Cargo/provider/network"]
async fn async_helper_coding_installed_rustc_tests_use_owned_custody_and_fresh_exec_approval() {
    coding_acceptance(Some(std::path::PathBuf::from(
        std::env::var_os("ANIMAOS_FP05_TEST_RUSTC").expect("root selects explicit installed rustc"),
    )))
    .await;
}
