use super::catalog::*;
use crate::app::SharedDaemonState;
use crate::runs::RunLink;
use crate::runs::{RunRecord, RunSource, RunStart};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;
use anima_core::{AgentConfig, ModelToolSelector};
use anima_core::{AgentState, CancelSignal};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::RwLock;

async fn fixture(names: &[&str]) -> (SharedDaemonState, AgentState, RunLink, Arc<RunToolCatalog>) {
    let registry = super::ToolRegistry::new();
    let mut config: AgentConfig = serde_json::from_value(serde_json::json!({"name":"catalog","model":"fixture","tools":registry.resolve_descriptors(names.iter().copied()).unwrap()})).unwrap();
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert("toolDiscovery".into(), anima_core::DataValue::Bool(true));
    let mut state = DaemonState::new();
    let agent = state.create_agent(config.clone()).unwrap().state;
    state.sessions.insert(SessionRecord::new(
        &agent.id,
        "catalog-session",
        SessionKind::Chat,
        SessionOrigin::Api,
        "fixture".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let record = RunRecord::running(
        RunStart {
            agent_id: agent.id.clone(),
            session_id: "catalog-session".into(),
            source: RunSource::Api,
            source_ref: None,
            idempotency_key: None,
            text: "fixture".into(),
            model: "fixture".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    let run = RunLink {
        agent_id: agent.id.clone(),
        session_id: record.session_id.clone(),
        run_id: record.id.clone(),
    };
    state.runs.insert(record);
    let state = Arc::new(RwLock::new(state));
    let catalog = Arc::new(RunToolCatalog::new(
        &state,
        run.clone(),
        CancelSignal::default(),
        &config,
        &agent,
        None,
    ));
    (state, agent, run, catalog)
}
#[tokio::test]
async fn tool_discovery_search_is_ranked_bounded_and_hydrates_only_exact_descriptors() {
    let (_state, agent, run, catalog) =
        fixture(&["calculate", "read_file", "write_file", "get_current_time"]).await;
    assert_eq!(
        catalog.visible_tool_names(),
        BTreeSet::from([
            "search_tools".into(),
            "describe_tool".into(),
            "get_current_time".into()
        ])
    );
    let text = catalog.search(&agent, &run, "write_file", 1).await.unwrap();
    assert!(text.len() <= 4096);
    let result: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result["tools"][0]["name"], "write_file");
    assert!(catalog.visible_tool_names().contains("write_file"));
    assert!(!catalog.visible_tool_names().contains("read_file"));
    assert!(catalog
        .search(&agent, &run, &"a".repeat(257), 3)
        .await
        .is_err());
    assert!(catalog.search(&agent, &run, "file", 6).await.is_err());
}
#[tokio::test]
async fn tool_discovery_revoke_before_target_or_discovery_fails_closed() {
    let (state, agent, run, catalog) = fixture(&["calculate", "memory_add"]).await;
    catalog.describe(&agent, &run, "calculate").await.unwrap();
    let mut config = agent.config.clone();
    config
        .tools
        .as_mut()
        .unwrap()
        .retain(|t| t.name != "calculate");
    state.write().await.restore_agent_config(&agent.id, config);
    assert!(catalog.check(&agent, &run, "calculate").await.is_err());
    assert!(catalog.describe(&agent, &run, "calculate").await.is_err());
    assert!(!catalog
        .search(&agent, &run, "calculate", 3)
        .await
        .unwrap()
        .contains("calculate"));
    let mut other = run.clone();
    other.run_id = "other".into();
    assert!(catalog.check(&agent, &other, "memory_add").await.is_err());
    state.write().await.remove_agent(&agent.id);
    assert!(catalog.search(&agent, &run, "memory", 3).await.is_err());
}
#[tokio::test]
async fn tool_discovery_hydration_lru_is_per_run_and_never_revokes_execution() {
    let names = [
        "calculate",
        "read_file",
        "write_file",
        "glob",
        "grep",
        "list_dir",
        "read_document",
        "web_search",
        "web_fetch",
        "memory_search",
        "memory_add",
    ];
    let (_state, agent, run, catalog) = fixture(&names).await;
    for name in names {
        catalog.describe(&agent, &run, name).await.unwrap();
    }
    assert!(!catalog.visible_tool_names().contains("calculate"));
    assert!(catalog.check(&agent, &run, "calculate").await.is_ok());
    assert!(catalog.visible_tool_names().len() <= 11);
    let (_other_state, _, _, other) = fixture(&names).await;
    assert!(!other.visible_tool_names().contains("write_file"));
}
#[tokio::test]
async fn tool_discovery_cancel_and_auto_memory_source_are_rechecked() {
    let (state, mut agent, run, _) = fixture(&["memory_add"]).await;
    let original = agent.config.clone();
    agent.config.tools.as_mut().unwrap().push(
        super::ToolRegistry::new()
            .descriptor("core_memory")
            .unwrap(),
    );
    let cancel = CancelSignal::default();
    let catalog = RunToolCatalog::new(&state, run.clone(), cancel.clone(), &original, &agent, None);
    assert!(catalog.check(&agent, &run, "core_memory").await.is_ok());
    state.write().await.restore_agent_config(
        &agent.id,
        serde_json::from_value(serde_json::json!({"name":"catalog","model":"fixture","tools":[]}))
            .unwrap(),
    );
    assert!(catalog.check(&agent, &run, "core_memory").await.is_err());
    cancel.cancel();
    assert!(catalog.search(&agent, &run, "memory", 3).await.is_err());
}

#[tokio::test]
async fn tool_discovery_native_revocation_during_receipt_save_refuses_before_handler() {
    use crate::agent_runs::{
        action_receipts::RunActionJournal,
        test_support::{gated_context, remember_call, tool_input},
        AgentRunCoordinator,
    };
    let (state, agent, run, catalog) = fixture(&["memory_add"]).await;
    catalog.describe(&agent, &run, "memory_add").await.unwrap();
    let coordinator =
        AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(1)));
    let cancel = CancelSignal::default();
    let journal = RunActionJournal::new(
        &state,
        coordinator.control_plane_transactions(),
        run.clone(),
        cancel.clone(),
        None,
    );
    let save = journal.install_test_before_save_gate();
    let context = gated_context(&coordinator, &run, cancel)
        .await
        .with_action_journal(journal)
        .with_tool_catalog(Some(catalog));
    let input = tool_input(&agent.id, &run.session_id);
    let target = agent.clone();
    let pending = tokio::spawn(async move {
        context
            .execute_tool(
                target,
                input,
                remember_call("receipt-race", "discovery receipt race"),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), save.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let mut config = agent.config.clone();
    config.tools = Some(vec![]);
    state.write().await.restore_agent_config(&agent.id, config);
    save.release.add_permits(1);
    assert_eq!(pending.await.unwrap().status, anima_core::TaskStatus::Error);
    let guard = state.read().await;
    assert!(!guard
        .memory
        .read()
        .await
        .snapshot()
        .memories
        .iter()
        .any(|m| m.content == "discovery receipt race"));
    assert!(guard
        .runs
        .get(&run.run_id)
        .unwrap()
        .action_receipts
        .iter()
        .all(|r| r.status != crate::runs::ActionStatus::Succeeded));
}
#[tokio::test]
async fn tool_discovery_native_revocation_during_owner_approval_refuses_before_handler() {
    use crate::agent_runs::{
        action_receipts::RunActionJournal,
        test_support::{decision, gated_context, pending_approvals, remember_call, tool_input},
        AgentRunCoordinator,
    };
    use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
    let (state, agent, run, catalog) = fixture(&["memory_add"]).await;
    catalog.describe(&agent, &run, "memory_add").await.unwrap();
    state.write().await.approvals.set_policy(
        &agent.id,
        ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Ask),
    );
    let coordinator =
        AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(1)));
    let cancel = CancelSignal::default();
    let journal = RunActionJournal::new(
        &state,
        coordinator.control_plane_transactions(),
        run.clone(),
        cancel.clone(),
        None,
    );
    let context = gated_context(&coordinator, &run, cancel)
        .await
        .with_action_journal(journal)
        .with_tool_catalog(Some(catalog));
    let input = tool_input(&agent.id, &run.session_id);
    let target = agent.clone();
    let pending = tokio::spawn(async move {
        context
            .execute_tool(
                target,
                input,
                remember_call("approval-race", "discovery approval race"),
            )
            .await
    });
    let request = pending_approvals(&coordinator, 1).await.remove(0);
    let mut config = agent.config.clone();
    config.tools = Some(vec![]);
    state.write().await.restore_agent_config(&agent.id, config);
    coordinator
        .decide_approval(
            &request.id,
            decision(ApprovalDecisionKind::AllowOnce, request.revision),
        )
        .await
        .unwrap();
    assert_eq!(pending.await.unwrap().status, anima_core::TaskStatus::Error);
    assert!(!state
        .read()
        .await
        .memory
        .read()
        .await
        .snapshot()
        .memories
        .iter()
        .any(|m| m.content == "discovery approval race"));
}

#[tokio::test]
async fn tool_discovery_new_grants_cannot_expand_a_captured_catalog() {
    let (state, agent, run, catalog) = fixture(&["calculate"]).await;
    let mut config = agent.config.clone();
    config
        .tools
        .as_mut()
        .unwrap()
        .push(super::ToolRegistry::new().descriptor("write_file").unwrap());
    state.write().await.restore_agent_config(&agent.id, config);
    assert!(catalog.describe(&agent, &run, "write_file").await.is_err());
    assert!(catalog.check(&agent, &run, "write_file").await.is_err());
    assert!(!catalog
        .search(&agent, &run, "write file", 3)
        .await
        .unwrap()
        .contains("write_file"));
}

#[tokio::test]
async fn tool_discovery_changed_native_descriptor_is_not_reused() {
    let (state, agent, run, catalog) = fixture(&["calculate"]).await;
    catalog.describe(&agent, &run, "calculate").await.unwrap();
    let mut config = agent.config.clone();
    config.tools.as_mut().unwrap()[0]
        .description
        .push_str(" changed canonical descriptor");
    state.write().await.restore_agent_config(&agent.id, config);
    assert!(catalog.check(&agent, &run, "calculate").await.is_err());
    assert!(catalog.describe(&agent, &run, "calculate").await.is_err());
}

#[tokio::test]
async fn tool_discovery_two_runs_of_one_agent_have_separate_hydration() {
    let (state, agent, first, catalog) = fixture(&["write_file", "calculate"]).await;
    catalog
        .describe(&agent, &first, "write_file")
        .await
        .unwrap();
    let record = RunRecord::running(
        RunStart {
            agent_id: agent.id.clone(),
            session_id: first.session_id.clone(),
            source: RunSource::Api,
            source_ref: None,
            idempotency_key: None,
            text: "second run".into(),
            model: "fixture".into(),
            provider: None,
            parent_run_id: None,
        },
        2,
    );
    let run = RunLink {
        agent_id: agent.id.clone(),
        session_id: record.session_id.clone(),
        run_id: record.id.clone(),
    };
    state.write().await.runs.insert(record);
    let second = RunToolCatalog::new(
        &state,
        run.clone(),
        CancelSignal::default(),
        &agent.config,
        &agent,
        None,
    );
    assert!(!second.visible_tool_names().contains("write_file"));
    second.describe(&agent, &run, "calculate").await.unwrap();
    assert!(!catalog.visible_tool_names().contains("calculate"));
    assert!(catalog.check(&agent, &run, "write_file").await.is_err());
}

#[tokio::test]
async fn tool_discovery_visibility_drops_currently_revoked_native_and_fails_closed_on_lock() {
    let (state, agent, run, catalog) = fixture(&["calculate", "get_current_time"]).await;
    catalog.describe(&agent, &run, "calculate").await.unwrap();
    assert!(catalog.visible_tool_names().contains("calculate"));
    let mut config = agent.config.clone();
    config.tools = Some(vec![]);
    state.write().await.restore_agent_config(&agent.id, config);
    let visible = catalog.visible_tool_names();
    assert!(!visible.contains("calculate"));
    assert!(!visible.contains("get_current_time"));
    let guard = state.write().await;
    assert!(
        catalog.visible_tool_names().is_empty(),
        "synchronous visibility must not wait on a state lock"
    );
    drop(guard);
    assert!(catalog.visible_tool_names().contains("search_tools"));
}
#[tokio::test]
async fn tool_discovery_auto_memory_pins_the_exact_source_descriptor() {
    let (state, mut agent, run, _) = fixture(&["memory_add"]).await;
    let original = agent.config.clone();
    agent.config.tools.as_mut().unwrap().push(
        super::ToolRegistry::new()
            .descriptor("core_memory")
            .unwrap(),
    );
    let catalog = RunToolCatalog::new(
        &state,
        run.clone(),
        CancelSignal::default(),
        &original,
        &agent,
        None,
    );
    catalog.describe(&agent, &run, "core_memory").await.unwrap();
    let mut changed = original.clone();
    changed.tools.as_mut().unwrap()[0]
        .description
        .push_str(" changed source");
    state.write().await.restore_agent_config(&agent.id, changed);
    assert!(catalog.check(&agent, &run, "core_memory").await.is_err());
    assert!(!catalog.visible_tool_names().contains("core_memory"));
}
