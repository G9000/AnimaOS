//! Provider-spy coverage for immutable admission and durable privacy boundaries.
use super::test_support::{calculate_call, chat_request, lead_config, Gate, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::skills::test_support::{temp_workspace, with_workspace};
use crate::state::DaemonState;
use std::sync::Arc;

#[tokio::test]
async fn files_replace_persona_keep_host_guidance_and_survive_tools_without_persistence() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![calculate_call("calc", "6*7")]),
            Step::Text(vec!["42"]),
            Step::Text(vec!["next"]),
        ],
        gate.clone(),
    );
    let root = temp_workspace("agent-files-frozen");
    std::fs::write(
        root.join("SOUL.md"),
        "Identity sentinel alpha; answer clearly.",
    )
    .unwrap();
    std::fs::write(
        root.join("AGENTS.md"),
        "Project sentinel alpha; use concise explanations.",
    )
    .unwrap();
    let mut config = lead_config("companion");
    config.system = Some("Legacy persona sentinel".into());
    config.bio = Some("Legacy biography sentinel".into());
    let mut state = with_workspace(DaemonState::with_model_adapter(model.clone()), &root);
    let id = state.create_agent(config.clone()).unwrap().state.id;
    let coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(tokio::sync::Semaphore::new(4)),
    );
    let worker = {
        let coordinator = coordinator.clone();
        let id = id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(&id, "chat:files", "calculate"))
                .await
        })
    };
    gate.entered().await;
    let first = model.requests()[0].system.clone();
    assert!(first.contains("Identity sentinel alpha"));
    assert!(first.contains("Project sentinel alpha"));
    assert!(first.contains("Live workspace roster"));
    assert!(first.contains("Use spawn_helper"));
    assert!(!first.contains("Legacy persona sentinel"));
    assert!(!first.contains("Legacy biography sentinel"));
    std::fs::write(
        root.join("SOUL.md"),
        "Identity sentinel beta; answer clearly.",
    )
    .unwrap();
    gate.release();
    gate.entered().await;
    assert!(model.requests()[1]
        .system
        .contains("Identity sentinel alpha"));
    assert!(!model.requests()[1]
        .system
        .contains("Identity sentinel beta"));
    gate.release();
    worker.await.unwrap().unwrap();
    {
        let state = coordinator.state.read().await;
        let runtime = state.agents.get(&id).unwrap();
        assert_eq!(runtime.config().system, config.system);
        assert_eq!(runtime.config().bio, config.bio);
        assert!(runtime
            .messages()
            .iter()
            .all(|m| !m.content.text.contains("sentinel")));
    }
    let worker = {
        let coordinator = coordinator.clone();
        let id = id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(&id, "chat:files", "again"))
                .await
        })
    };
    gate.entered().await;
    assert!(model.requests()[2]
        .system
        .contains("Identity sentinel beta"));
    assert!(!model.requests()[2]
        .system
        .contains("Identity sentinel alpha"));
    gate.release();
    worker.await.unwrap().unwrap();
}

#[test]
fn file_reserve_is_counted_once_and_empty_snapshot_preserves_legacy_selection() {
    use anima_core::{AgentFileRenderBudget, AgentFileSection, Content};
    use anima_service_adapters::ServiceTokenEstimator as TokenEstimator;
    let mut state = DaemonState::new();
    let id = state
        .create_agent(lead_config("companion"))
        .unwrap()
        .state
        .id;
    let input = Content {
        text: "hello".into(),
        ..Default::default()
    };
    let empty = state.build_run_runtime(&id, "chat:budget", &input).unwrap();
    let files = anima_core::render_agent_files(
        Some(&AgentFileSection {
            label: "SOUL.md".into(),
            text: "a".repeat(160),
        }),
        &[],
        AgentFileRenderBudget {
            max_identity_chars: 200,
            max_project_chars: 200,
            max_total_chars: 200,
        },
    );
    let with_files = state
        .build_run_runtime_with_files(&id, "chat:budget", &input, None, &files)
        .unwrap();
    assert_eq!(
        empty.context.raw_estimate_tokens,
        TokenEstimator::default().text_tokens("hello")
    );
    assert_eq!(
        with_files.context.raw_estimate_tokens - empty.context.raw_estimate_tokens,
        files.estimated_tokens
    );
}

#[tokio::test]
async fn cancelled_blocked_admission_drops_loader_and_never_reaches_provider() {
    use anima_core::{CancelSignal, Content, RunControl};
    use std::sync::atomic::{AtomicBool, Ordering};
    let model = ScriptedModel::new(vec![Step::Text(vec!["must not run"])]);
    let root = temp_workspace("agent-files-cancel");
    let config = lead_config("companion");
    let mut state = with_workspace(DaemonState::with_model_adapter(model.clone()), &root);
    let id = state.create_agent(config.clone()).unwrap().state.id;
    let coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(tokio::sync::Semaphore::new(4)),
    );
    let link = super::test_support::ledger_run(&coordinator, &id, "chat:cancel-files").await;
    let cancel = CancelSignal::new();
    let entered = Arc::new(tokio::sync::Notify::new());
    let worker_flag = Arc::new(std::sync::Mutex::new(None::<Arc<AtomicBool>>));
    let stopper = {
        let entered = entered.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            entered.notified().await;
            cancel.cancel();
        })
    };
    let input = Content {
        text: "hello".into(),
        ..Default::default()
    };
    let (mut build, accepted) = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::agent_files::runtime::admit_run_with_loader(
            &coordinator.state,
            coordinator.control_plane_transactions(),
            &id,
            &link.session_id,
            &link.session_id,
            &link.run_id,
            &input,
            &config,
            cancel.clone(),
            None,
            {
                let entered = entered.clone();
                let worker_flag = worker_flag.clone();
                move |_scope, _budget, flag, _deadline| {
                    *worker_flag.lock().unwrap() = Some(flag);
                    let entered = entered.clone();
                    Box::pin(async move {
                        entered.notify_one();
                        std::future::pending().await
                    })
                }
            },
        ),
    )
    .await
    .expect("stop wakes a blocked loader promptly")
    .unwrap();
    stopper.await.unwrap();
    assert!(worker_flag
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .load(Ordering::Acquire));
    assert!(accepted.rendered.identity.is_none());
    assert!(accepted.rendered.project_context.is_empty());
    build.runtime.set_run_control(RunControl {
        cancel,
        ..Default::default()
    });
    build.runtime.run(input).await;
    assert!(
        model.requests().is_empty(),
        "cancelled admission cannot publish a model request"
    );
}

#[tokio::test]
async fn root_changes_retry_once_with_frozen_config_and_original_deadline() {
    use crate::agent_files::{AcceptedAgentFiles, Availability};
    use anima_core::{Content, DataValue, RuntimePromptIdentity};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let model = ScriptedModel::new(vec![]);
    let root = temp_workspace("agent-files-stale");
    let frozen = lead_config("companion");
    let mut state = with_workspace(DaemonState::with_model_adapter(model), &root);
    let id = state.create_agent(frozen.clone()).unwrap().state.id;
    let coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(tokio::sync::Semaphore::new(4)),
    );
    let link = super::test_support::ledger_run(&coordinator, &id, "chat:stale-files").await;
    {
        let mut state = coordinator.state.write().await;
        let mut changed = frozen.clone();
        changed.model = "later-model".into();
        changed
            .settings
            .as_mut()
            .unwrap()
            .additional
            .insert("agentFilesProfileEnabled".into(), DataValue::Bool(true));
        state.agents.get_mut(&id).unwrap().replace_config(changed);
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let deadlines = Arc::new(std::sync::Mutex::new(Vec::new()));
    let input = Content {
        text: "hello".into(),
        ..Default::default()
    };
    let (build, accepted) = crate::agent_files::runtime::admit_run_with_loader(
        &coordinator.state,
        coordinator.control_plane_transactions(),
        &id,
        &link.session_id,
        &link.session_id,
        &link.run_id,
        &input,
        &frozen,
        anima_core::CancelSignal::new(),
        None,
        {
            let state = coordinator.state.clone();
            let calls = calls.clone();
            let deadlines = deadlines.clone();
            move |scope, _budget, _flag, deadline| {
                assert!(!scope.profile_enabled, "PATCH applies only to later runs");
                deadlines.lock().unwrap().push(deadline);
                let index = calls.fetch_add(1, Ordering::SeqCst);
                let state = state.clone();
                Box::pin(async move {
                    state
                        .write()
                        .await
                        .workspace
                        .as_mut()
                        .unwrap()
                        .root_path
                        .push(format!("changed-{index}"));
                    let mut files = AcceptedAgentFiles::empty(Availability::Available);
                    files.rendered.identity = Some(RuntimePromptIdentity {
                        text: "stale identity".into(),
                    });
                    files
                })
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let deadlines = deadlines.lock().unwrap();
    assert_eq!(deadlines[0], deadlines[1]);
    assert_eq!(accepted.manifest.availability, Availability::StaleScope);
    assert!(accepted.rendered.identity.is_none());
    assert_eq!(build.runtime.config().model, frozen.model);
}

#[tokio::test]
async fn admission_reserves_retained_summary_before_long_soul() {
    use crate::sessions::SessionSummary;
    use anima_core::{CancelSignal, Content, DataValue};
    use anima_service_adapters::ServiceTokenEstimator as TokenEstimator;
    let root = temp_workspace("agent-files-summary-budget");
    std::fs::write(root.join("SOUL.md"), "Speak clearly. ".repeat(1000)).unwrap();
    let mut config = lead_config("companion");
    let settings = config.settings.as_mut().unwrap();
    settings.max_tokens = Some(50);
    settings
        .additional
        .insert("contextBudgetTokens".into(), DataValue::Number(400.0));
    let mut state = with_workspace(DaemonState::new(), &root);
    let id = state.create_agent(config.clone()).unwrap().state.id;
    let coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(tokio::sync::Semaphore::new(4)),
    );
    let link = super::test_support::ledger_run(&coordinator, &id, "chat:summary-budget").await;
    let input = Content {
        text: "hello".into(),
        ..Default::default()
    };
    let estimator = TokenEstimator::new(1.5);
    for summary_chars in [400, 2000] {
        let summary_text = "s".repeat(summary_chars);
        {
            let mut guard = coordinator.state.write().await;
            let session = guard.sessions.get_mut(&id, &link.session_id).unwrap();
            session.context_calibration_permille = Some(1500);
            session.summary = Some(SessionSummary {
                text: summary_text.clone(),
                through_message_id: "old".into(),
                created_at_ms: 1,
                source_message_count: 2,
            });
        }
        let (build, files) = crate::agent_files::runtime::admit_run(
            &coordinator.state,
            coordinator.control_plane_transactions(),
            &id,
            &link.session_id,
            &link.session_id,
            &link.run_id,
            &input,
            &config,
            CancelSignal::new(),
            None,
        )
        .await
        .unwrap();
        let mandatory =
            50 + estimator.text_tokens(&input.text) + estimator.text_tokens(&summary_text);
        if mandatory <= 400 {
            assert!(
                files.rendered.identity.is_some(),
                "remaining budget admits truncated SOUL"
            );
            assert!(
                mandatory + (files.rendered.estimated_tokens as f64 * 1.5).ceil() as u64 <= 400
            );
        } else {
            assert_eq!(
                files.rendered.estimated_tokens, 0,
                "legacy oversized summary admits no files"
            );
            assert!(files.rendered.identity.is_none());
        }
        assert!(
            build.context.raw_estimate_tokens
                >= TokenEstimator::default().text_tokens(&summary_text)
        );
    }
}
