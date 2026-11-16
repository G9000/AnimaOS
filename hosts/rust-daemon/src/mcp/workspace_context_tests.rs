use super::*;
use anima_core::{runtime::prompt_context::PromptContextSource, DataValue};
use sha2::{Digest, Sha256};

#[path = "record_selection_tests.rs"]
mod record_selection_tests;

#[path = "graph_read_tests.rs"]
mod graph_read_tests;

fn context(agent: &str) -> serde_json::Value {
    let persona = "Remote companion persona sentinel";
    json!({"access":{"version":1,"mode":"companion","subject":agent},"workspace":{"id":"workspace","generation":"generation"},"persona":{"content":persona,"hash":hex::encode(Sha256::digest(persona))},"records":[{"content":"Workspace reference sentinel"}]})
}
async fn configured(f: &Fixture) -> McpConnectionView {
    *f.factory.context_result.lock().unwrap() = Some(context(&f.agent));
    let view = f.reviewed().await;
    let _ = f.grant_and_stamp(&view).await;
    let mut state = f.state.write().await;
    let mut config = state.agents[&f.agent].config().clone();
    config.bio = Some("Local bio sentinel".into());
    config.style = Some("Local style sentinel".into());
    config.system = Some("Local system sentinel".into());
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(
            "workspaceContext".into(),
            DataValue::Object(std::collections::BTreeMap::from([
                (
                    "connectionId".into(),
                    DataValue::String(view.record.id.clone()),
                ),
                (
                    "tool".into(),
                    DataValue::String("mother_get_context".into()),
                ),
                ("mode".into(), DataValue::String("companion".into())),
                ("workspaceId".into(), DataValue::String("workspace".into())),
                ("generation".into(), DataValue::String("generation".into())),
                ("subject".into(), DataValue::String(f.agent.clone())),
            ])),
        );
    state.restore_agent_config(&f.agent, config);
    view
}
fn run(f: &Fixture) -> RunLink {
    let guard = f.state.try_read().unwrap();
    let records = guard.runs.for_session(&f.agent, "chat:mcp-fixture");
    let record = records[0];
    RunLink {
        agent_id: f.agent.clone(),
        session_id: record.session_id.clone(),
        run_id: record.id.clone(),
    }
}

async fn additive(f: &Fixture, persona: &str) -> (McpConnectionView, anima_core::AgentState) {
    let view = configured(f).await;
    let mut guard = f.state.write().await;
    let mut config = guard.agents[&f.agent].config().clone();
    let settings = config.settings.as_mut().unwrap();
    settings.additional.remove("workspaceContext");
    let binding: crate::memory_provider::binding::MemoryProviderBinding =
        serde_json::from_value(json!({
            "version":1,"provider":"mother","connectionId":view.record.id,
            "workspaceId":"workspace","generation":"generation","subject":f.agent,
            "accessMode":"companion","publication":"selected","personaSource":persona
        }))
        .unwrap();
    settings
        .additional
        .insert("memoryProvider".into(), binding.portable());
    guard.restore_agent_config(&f.agent, config);
    let agent = guard.get_agent(&f.agent).unwrap().state;
    (view, agent)
}

#[tokio::test]
async fn memory_provider_mother_timeout_cancels_only_its_read_not_the_native_run() {
    use anima_memory::ExternalMemoryProvider;
    let f = Fixture::new().await;
    let (_, agent) = additive(&f, "local").await;
    let cancel = CancelSignal::new();
    let provider = crate::memory_provider::mother::MotherProvider::new(
        f.state.clone(),
        f.manager.clone(),
        crate::memory_provider::binding::binding(&agent.config)
            .unwrap()
            .unwrap(),
        agent,
        run(&f),
        cancel.clone(),
    );
    f.factory.sessions.lock().unwrap()[0]
        .hold_calls
        .store(true, Ordering::SeqCst);
    let result = provider
        .recall(anima_memory::RecallRequest {
            agent_id: f.agent.clone(),
            query: "garden".into(),
            limit: 4,
            deadline: Duration::from_millis(30),
        })
        .await;
    assert!(result.is_err());
    assert!(
        !cancel.is_cancelled(),
        "garden timeout must leave native conversation running"
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_additive_persona_is_independent_and_binding_changes_fail_closed() {
    for persona in ["local", "workspace"] {
        let f = Fixture::new().await;
        let (_, agent) = additive(&f, persona).await;
        let source = crate::workspace_context::WorkspaceRunContext::load(
            f.state.clone(),
            Some(f.manager.clone()),
            &agent,
            &run(&f),
            CancelSignal::new(),
        )
        .await;
        let result = source.current(&agent).await;
        assert_eq!(result.identity.is_some(), persona == "workspace");
        assert!(result.reference.contains("ADDITIVE memory provider"));
        assert!(
            !result.reference.contains("Workspace reference sentinel"),
            "bootstrap previews must not bypass full-record recall"
        );
        assert!(!result.reference.contains("PRIMARY durable memory"));
        assert!(!crate::workspace_context::primary_memory(&agent.config));
        let mut config = agent.config.clone();
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .remove("memoryProvider");
        f.state.write().await.restore_agent_config(&f.agent, config);
        let fallback = source.current(&agent).await;
        assert!(fallback.identity.is_none());
        assert!(fallback.reference.contains("Shared garden unavailable"));
        assert!(!fallback.reference.contains("Workspace reference sentinel"));
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn memory_provider_mother_full_recall_checks_revision_authority_and_keeps_local_identity() {
    let f = Fixture::new().await;
    let id = uuid::Uuid::new_v4().to_string();
    *f.factory.garden_records.lock().unwrap() = Some(vec![
        json!({"id":id,"revision":2,"content":"Full garden memory sentinel","status":"active","updatedAt":"2026-11-14T00:00:00.000Z","actor":"other-harness"}),
    ]);
    let (mut view, mut agent) = additive(&f, "local").await;
    let candidates = view.candidates.clone();
    for candidate in candidates
        .iter()
        .filter(|t| t.remote_name != "mother_get_context")
    {
        view = f
            .manager
            .review(
                &f.agent,
                &view.record.id,
                ReviewMcpTool {
                    expected_version: view.record.version,
                    discovery_id: view.discovery_id.clone().unwrap(),
                    descriptor_digest: candidate.descriptor_digest.clone(),
                },
            )
            .await
            .unwrap();
    }
    // Review mode explicitly grants every read tool, as ordinary owner setup does.
    let tools = f
        .state
        .read()
        .await
        .mcp_registry
        .connections
        .values()
        .next()
        .unwrap()
        .reviewed_tools
        .iter()
        .map(|t| t.tool_descriptor())
        .collect();
    agent.config.tools = Some(tools);
    f.state
        .write()
        .await
        .restore_agent_config(&f.agent, agent.config.clone());
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
        "garden".into(),
    )
    .await;
    let bootstrap = source.current(&agent).await;
    assert!(bootstrap.identity.is_none());
    assert!(!bootstrap.reference.contains("Full garden memory sentinel"));
    let plan = || {
        crate::memory_provider::mother::RecallPlan::search("garden", "hybrid", None, &[]).unwrap()
    };
    let result = source
        .search_shared(&agent.id, plan(), 4, false)
        .await
        .unwrap();
    assert!(result
        .items
        .iter()
        .any(|r| r.text.contains("Full garden memory sentinel")
            && r.text.contains("other-harness")
            && r.text.contains("revision")));
    f.factory.context_result.lock().unwrap().as_mut().unwrap()["workspace"]["generation"] =
        json!("restored");
    assert!(source
        .search_shared(&agent.id, plan(), 4, false)
        .await
        .is_err());
    let result = source.current(&agent).await;
    assert!(result.reference.contains("Shared garden unavailable"));
    assert!(!result.reference.contains("Full garden memory sentinel"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_additive_online_then_revoked_keeps_native_recall_and_capture_isolated() {
    use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};
    use crate::approvals::autonomy::{AgentAutonomy, AutonomyMode};
    use crate::memory_provider::publication::{Basis, Delivery, PublicationService, Selection};
    use crate::profile_owner::{ProfileOwner, ProfileResource};
    use anima_memory::{ExternalMemoryProvider, MemoryScope, MemoryType, NewMemory};

    let f = Fixture::new().await;
    let remote_id = uuid::Uuid::new_v4().to_string();
    *f.factory.garden_records.lock().unwrap() = Some(vec![json!({
        "id":remote_id,"revision":3,"content":"Garden qualification attributed fact",
        "status":"active","updatedAt":"2026-11-15T00:00:00.000Z","actor":"synthetic-other-harness"
    })]);
    let (mut view, _) = additive(&f, "local").await;
    for candidate in view
        .candidates
        .clone()
        .iter()
        .filter(|candidate| candidate.remote_name != "mother_get_context")
    {
        view = f
            .manager
            .review(
                &f.agent,
                &view.record.id,
                ReviewMcpTool {
                    expected_version: view.record.version,
                    discovery_id: view.discovery_id.clone().unwrap(),
                    descriptor_digest: candidate.descriptor_digest.clone(),
                },
            )
            .await
            .unwrap();
    }
    let journal = f.folder.join("qualification-publications.sqlite");
    let owner = ProfileOwner::acquire_resources([
        ProfileResource::Sqlite(journal.clone()),
        ProfileResource::File(f.folder.join("control.json")),
    ])
    .unwrap();
    let native_tools = crate::tools::ToolRegistry::new()
        .resolve_descriptors(["memory_add", "memory_search"])
        .unwrap();
    let (agent, foreign_agent) = {
        let mut state = f.state.write().await;
        state.profile_owner = Some(owner.clone());
        state.control_plane_store = Some(
            state
                .control_plane_store
                .clone()
                .unwrap()
                .with_owner(owner.clone())
                .unwrap(),
        );
        // The fixture explicitly supplies owner-reviewed publication metadata. No write is dispatched.
        let row = state
            .mcp_registry
            .connections
            .get_mut(&view.record.id)
            .unwrap();
        row.reviewed_tools.push(ReviewedMcpTool {
            alias: "mcp_qualification_selected_proposal".into(),
            descriptor: prepare_external_tool(
                "mother_propose_memory",
                "Synthetic selected proposal",
                json!({"type":"object","additionalProperties":true}),
                None,
            )
            .unwrap(),
        });
        let mut tools: Vec<_> = row
            .reviewed_tools
            .iter()
            .map(ReviewedMcpTool::tool_descriptor)
            .collect();
        tools.extend(native_tools.clone());
        let mut config = state.agents[&f.agent].config().clone();
        config.tools = Some(tools);
        state.restore_agent_config(&f.agent, config);
        state.approvals.set_policy(
            &f.agent,
            crate::approvals::ApprovalPolicy {
                exec: crate::approvals::PolicyAction::Allow,
                ..Default::default()
            },
        );
        // Explicit tool removal revokes authority in review mode; automatic mode uses enabled connections.
        state.autonomy.set(AgentAutonomy {
            agent_id: f.agent.clone(),
            mode: AutonomyMode::Strict,
            ..Default::default()
        });
        let foreign = state
            .create_agent(crate::sessions::test_support::agent_config(
                "Synthetic isolated qualification agent",
            ))
            .unwrap()
            .state
            .id;
        (state.get_agent(&f.agent).unwrap().state, foreign)
    };
    f.manager
        .refresh_agent_permissions(&*f.state.read().await, &f.agent)
        .unwrap();
    assert_eq!(f.manager.tool_access(&f.agent), McpToolAccess::Review);
    let memory = f.state.read().await.memory_handle();
    let mut selected = None;
    for (agent_id, content, scope) in [
        (
            f.agent.clone(),
            "Native qualification selected fact",
            MemoryScope::Shared,
        ),
        (
            foreign_agent.clone(),
            "Foreign qualification private fact",
            MemoryScope::Private,
        ),
    ] {
        let fact = memory
            .write()
            .await
            .add(NewMemory {
                agent_id,
                agent_name: "Synthetic qualification".into(),
                memory_type: MemoryType::Fact,
                content: content.into(),
                importance: 0.9,
                tags: None,
                scope: Some(scope),
                room_id: None,
                world_id: None,
                session_id: None,
            })
            .unwrap();
        if fact.agent_id == f.agent {
            selected = Some(fact);
        }
    }
    let selected = selected.unwrap();
    let provider = crate::memory_provider::mother::MotherProvider::new(
        f.state.clone(),
        f.manager.clone(),
        crate::memory_provider::binding::binding(&agent.config)
            .unwrap()
            .unwrap(),
        agent,
        run(&f),
        CancelSignal::new(),
    );
    let calls_before = f.factory.sessions.lock().unwrap()[0]
        .calls
        .load(Ordering::SeqCst);
    assert!(provider
        .recall(anima_memory::RecallRequest {
            agent_id: foreign_agent.clone(),
            query: "qualification".into(),
            limit: 4,
            deadline: Duration::from_secs(1),
        })
        .await
        .is_err());
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        calls_before,
        "another agent must be rejected before any garden transport call"
    );
    let publications = PublicationService::open(&f.state, owner, &journal)
        .await
        .unwrap();
    f.state.write().await.memory_publications = Some(publications.clone());
    let admitted = publications
        .admit(
            &f.agent,
            Selection {
                publication_id: uuid::Uuid::new_v4().to_string(),
                local_id: selected.id.clone(),
                expected_content: selected.content.clone(),
                title: "Selected qualification fact".into(),
                source: "Explicit synthetic owner selection".into(),
                basis: Basis::OwnerStatement,
                delivery: Delivery::Proposal,
                target: None,
            },
        )
        .await
        .unwrap();
    f.manager
        .publication_preflight(&admitted, "mother_get_context", &json!({"limit":1}))
        .await
        .unwrap();
    let search_call = |id: &str| ToolCall {
        id: id.into(),
        name: "memory_search".into(),
        args: std::collections::BTreeMap::from([(
            "query".into(),
            DataValue::String("qualification".into()),
        )]),
    };
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![search_call("online-native-search")]),
        Step::Text(vec!["online complete"]),
        Step::Tools(vec![search_call("revoked-native-search")]),
        Step::Text(vec!["revoked complete"]),
    ]);
    // This fixture explicitly selects a core record; bootstrap no longer guesses
    // a topic from conversation wording.
    f.factory.context_result.lock().unwrap().as_mut().unwrap()["pinned"] =
        json!([{"id":remote_id}]);
    f.state.write().await.model_adapter = model.clone();
    let runs =
        crate::agent_runs::AgentRunCoordinator::new(f.state.clone(), Arc::new(Semaphore::new(4)))
            .with_mcp(f.manager.clone());
    runs.run(chat_request(
        &f.agent,
        "chat:additive-qualification",
        "qualification online",
    ))
    .await
    .unwrap();
    let requests = model.requests();
    assert!(requests[0]
        .system
        .contains("Garden qualification attributed fact"));
    assert!(requests[0].system.contains("synthetic-other-harness"));
    assert!(requests[0].system.contains("Local system sentinel"));
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content.text.contains(&selected.content)),
        "online native memory_search must return the local source"
    );
    {
        let mut state = f.state.write().await;
        let mut config = state.agents[&f.agent].config().clone();
        config.tools = Some(native_tools);
        state.restore_agent_config(&f.agent, config);
    }
    f.manager.note_agent_permissions_changed(&f.agent);
    let calls_before = f.factory.sessions.lock().unwrap()[0]
        .calls
        .load(Ordering::SeqCst);
    assert!(
        f.manager
            .publication_preflight(&admitted, "mother_get_context", &json!({"limit":1}))
            .await
            .is_err(),
        "selected sharing must recheck current owner grants"
    );
    runs.run(chat_request(
        &f.agent,
        "chat:additive-qualification",
        "qualification revoked",
    ))
    .await
    .unwrap();
    let requests = model.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[2].system.contains("Shared garden unavailable"));
    assert!(requests[2].system.contains("Local system sentinel"));
    assert!(!requests[2]
        .system
        .contains("Garden qualification attributed fact"));
    assert!(
        requests[3]
            .messages
            .iter()
            .any(|message| message.content.text.contains(&selected.content)),
        "revoked garden grants must leave native memory_search available"
    );
    assert!(requests
        .iter()
        .flat_map(|request| &request.messages)
        .all(|message| !message
            .content
            .text
            .contains("Foreign qualification private fact")));
    for config in model.configs() {
        assert!(config.allows_tool("memory_add") && config.allows_tool("memory_search"));
    }
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        calls_before,
        "revoked reads and publication preflight must not contact the provider"
    );
    let records = memory
        .read()
        .await
        .get_recent(anima_memory::RecentMemoryOptions {
            agent_id: Some(f.agent.clone()),
            ..Default::default()
        });
    assert!(
        records
            .iter()
            .filter(|record| record.memory_type == MemoryType::TaskResult)
            .count()
            >= 2,
        "both completed turns must retain native task-result capture"
    );
    assert_eq!(
        memory.read().await.get(&selected.id).unwrap().content,
        selected.content
    );
    assert!(publications
        .get(&foreign_agent, &admitted.admission.publication_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        publications
            .get(&f.agent, &admitted.admission.publication_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        crate::memory_provider::outbox::PublicationState::Queued
    );
    publications.shutdown().await;
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_mcp_pins_identity_and_rejects_wrong_authority_and_legacy() {
    let f = Fixture::new().await;
    let view = configured(&f).await;
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let run = run(&f);
    for field in [
        "valid",
        "subject",
        "workspace",
        "generation",
        "legacy",
        "hash",
        "version",
    ] {
        let mut value = context(&f.agent);
        match field {
            "subject" => value["access"]["subject"] = json!("foreign"),
            "workspace" => value["workspace"]["id"] = json!("foreign"),
            "generation" => value["workspace"]["generation"] = json!("foreign"),
            "legacy" => value["access"]["mode"] = json!("legacy"),
            "hash" => value["persona"]["hash"] = json!("bad"),
            "version" => value["access"]["version"] = json!(2),
            _ => {}
        }
        *f.factory.context_result.lock().unwrap() = Some(value);
        let source = crate::workspace_context::WorkspaceRunContext::load(
            f.state.clone(),
            Some(f.manager.clone()),
            &agent,
            &run,
            CancelSignal::new(),
        )
        .await;
        let result = source.current(&agent).await;
        assert_eq!(result.identity.is_some(), field == "valid", "{field}");
        assert_eq!(
            result
                .withheld_tools
                .contains(&view.record.reviewed_tools[0].alias),
            field != "valid",
            "{field}"
        );
        if field != "valid" {
            assert!(!result.reference.contains("Workspace reference sentinel"));
        }
    }
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_mcp_revocation_and_cancel_remove_previously_admitted_persona() {
    let f = Fixture::new().await;
    let view = configured(&f).await;
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let run = run(&f);
    let cancel = CancelSignal::new();
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run,
        cancel.clone(),
    )
    .await;
    assert!(source.current(&agent).await.identity.is_some());
    cancel.cancel();
    assert!(source.current(&agent).await.identity.is_none());
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run,
        CancelSignal::new(),
    )
    .await;
    assert!(source.current(&agent).await.identity.is_some());
    f.manager
        .disconnect(&f.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    let fallback = source.current(&agent).await;
    assert!(fallback.identity.is_none());
    assert!(fallback.reference.contains("Workspace context unavailable"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_actual_model_online_offline_recovery_keeps_same_chat() {
    use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};
    let f = Fixture::new().await;
    let binding_view = configured(&f).await;
    let external = Arc::new(
        crate::memory_provider::test_support::FakeProvider::with_items(
            "legacy-external",
            &["Competing external memory sentinel"],
        ),
    );
    let _external = crate::memory_provider::test_support::inject(external.clone());
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["one"]),
        Step::Text(vec!["two"]),
        Step::Text(vec!["three"]),
    ]);
    f.state.write().await.model_adapter = model.clone();
    let runs =
        crate::agent_runs::AgentRunCoordinator::new(f.state.clone(), Arc::new(Semaphore::new(4)))
            .with_mcp(f.manager.clone());
    for (index, prompt) in ["first", "second", "third"].iter().enumerate() {
        if index == 1 {
            f.factory.sessions.lock().unwrap()[0]
                .hold_calls
                .store(true, Ordering::SeqCst);
        }
        if index == 2 {
            timeout(Duration::from_secs(2), async {
                loop {
                    let view = f
                        .manager
                        .get(&f.agent, &binding_view.record.id)
                        .await
                        .unwrap();
                    if matches!(
                        view.state,
                        McpConnectionState::Disconnected | McpConnectionState::Degraded
                    ) && !view.physical_calls_active
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        *f.factory.context_result.lock().unwrap() = Some(context(&f.agent));
        runs.run(chat_request(&f.agent, "chat:workspace-context", prompt))
            .await
            .unwrap();
    }
    let requests = model.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        f.factory.connects.load(Ordering::SeqCst),
        2,
        "reconnect only on the next turn after an offline read"
    );
    for index in [0, 2] {
        assert!(requests[index]
            .system
            .contains("Remote companion persona sentinel"));
        assert!(requests[index]
            .system
            .contains("Workspace reference sentinel"));
        assert!(!requests[index].system.contains("Local bio sentinel"));
        assert!(!requests[index].system.contains("Local style sentinel"));
        assert!(!requests[index].system.contains("Local system sentinel"));
    }
    assert!(requests[1].system.contains("Workspace context unavailable"));
    for local in [
        "Local bio sentinel",
        "Local style sentinel",
        "Local system sentinel",
    ] {
        assert!(requests[1].system.contains(local));
    }
    assert!(!requests[1]
        .system
        .contains("Remote companion persona sentinel"));
    assert!(!requests[1].system.contains("Workspace reference sentinel"));
    assert!(requests[2]
        .messages
        .iter()
        .any(|m| m.content.text == "first"));
    assert!(requests[2]
        .messages
        .iter()
        .any(|m| m.content.text == "second"));
    let snapshot =
        serde_json::to_string(&f.state.read().await.get_agent(&f.agent).unwrap()).unwrap();
    assert!(!snapshot.contains("Remote companion persona sentinel"));
    assert!(external.recall_requests.lock().unwrap().is_empty());
    assert!(external.turns.lock().unwrap().is_empty());
    assert!(external.writes.lock().unwrap().is_empty());
    let memory = f.state.read().await.memory_handle();
    assert!(memory
        .read()
        .await
        .get_recent(anima_memory::RecentMemoryOptions {
            agent_id: Some(f.agent.clone()),
            ..Default::default()
        })
        .is_empty());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_remote_policy_change_between_model_steps_removes_identity_and_tools() {
    use crate::agent_runs::test_support::{
        calculate_call, chat_request, Gate, ScriptedModel, Step,
    };
    let f = Fixture::new().await;
    let bound = configured(&f).await;
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![calculate_call("sum", "1+1")]),
            Step::Text(vec!["done"]),
        ],
        gate.clone(),
    );
    {
        let mut state = f.state.write().await;
        let mut config = state.agents[&f.agent].config().clone();
        config.tools.as_mut().unwrap().extend(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["calculate"])
                .unwrap(),
        );
        state.restore_agent_config(&f.agent, config);
        state.model_adapter = model.clone();
    }
    let runs =
        crate::agent_runs::AgentRunCoordinator::new(f.state.clone(), Arc::new(Semaphore::new(4)))
            .with_mcp(f.manager.clone());
    let request = chat_request(&f.agent, "chat:midrun-revoke", "Calculate");
    let pending = tokio::spawn(async move { runs.run(request).await });
    gate.entered().await;
    let mut revoked = context(&f.agent);
    revoked["access"]["mode"] = json!("shared");
    *f.factory.context_result.lock().unwrap() = Some(revoked);
    gate.release();
    gate.entered().await;
    let requests = model.requests();
    assert!(requests[0]
        .system
        .contains("Remote companion persona sentinel"));
    assert!(requests[1].system.contains("Workspace context unavailable"));
    assert!(!requests[1]
        .system
        .contains("Remote companion persona sentinel"));
    assert!(!model.configs()[1].allows_tool(&bound.record.reviewed_tools[0].alias));
    gate.release();
    pending.await.unwrap().unwrap();
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_explicit_exec_deny_never_calls_transport() {
    let f = Fixture::new().await;
    configured(&f).await;
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    f.state.write().await.approvals.set_policy(
        &f.agent,
        crate::approvals::ApprovalPolicy::default().with(
            crate::approvals::RiskClass::Exec,
            crate::approvals::PolicyAction::Deny,
        ),
    );
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
    )
    .await;
    assert!(source.current(&agent).await.identity.is_none());
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        0
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_reconnect_rechecks_policy_after_blocked_credential_read() {
    let f = Fixture::new().await;
    let view = configured(&f).await;
    let view = f
        .manager
        .set_bearer(
            &f.agent,
            &view.record.id,
            view.record.version,
            McpBearerToken::parse("synthetic-only-token".into()).unwrap(),
        )
        .await
        .unwrap();
    f.manager
        .connect(&f.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    f.factory.sessions.lock().unwrap()[1]
        .fail_calls
        .store(true, Ordering::SeqCst);
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let failed = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
    )
    .await;
    assert!(failed.current(&agent).await.identity.is_none());
    timeout(Duration::from_secs(2), async {
        loop {
            let current = f.manager.get(&f.agent, &view.record.id).await.unwrap();
            if current.error.is_some()
                && matches!(
                    current.state,
                    McpConnectionState::Disconnected | McpConnectionState::Degraded
                )
                && !current.physical_calls_active
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    *f.credentials.read_gate.lock().unwrap() = Some((entered.clone(), release.clone()));
    let connects = f.factory.connects.load(Ordering::SeqCst);
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let worker = {
        let state = f.state.clone();
        let manager = f.manager.clone();
        let run = run(&f);
        let agent = agent.clone();
        tokio::spawn(async move {
            crate::workspace_context::WorkspaceRunContext::load(
                state,
                Some(manager),
                &agent,
                &run,
                CancelSignal::new(),
            )
            .await
        })
    };
    timeout(Duration::from_secs(2), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    f.state.write().await.approvals.set_policy(
        &f.agent,
        crate::approvals::ApprovalPolicy::default().with(
            crate::approvals::RiskClass::Exec,
            crate::approvals::PolicyAction::Deny,
        ),
    );
    release.add_permits(1);
    let source = worker.await.unwrap();
    assert!(source.current(&agent).await.identity.is_none());
    assert_eq!(
        f.factory.connects.load(Ordering::SeqCst),
        connects,
        "revocation after vault admission must prevent physical connection start"
    );
    assert!(view.record.enabled);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_explicit_disconnect_stays_offline_until_owner_connects() {
    let f = Fixture::new().await;
    let view = configured(&f).await;
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let run = run(&f);
    f.manager
        .disconnect(&f.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if f.manager
                .get(&f.agent, &view.record.id)
                .await
                .unwrap()
                .state
                == McpConnectionState::Disconnected
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let connects = f.factory.connects.load(Ordering::SeqCst);
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run,
        CancelSignal::new(),
    )
    .await;
    assert!(source.current(&agent).await.identity.is_none());
    assert_eq!(
        f.factory.connects.load(Ordering::SeqCst),
        connects,
        "owner Disconnect must prevent automatic reconnect"
    );
    f.manager
        .connect(&f.agent, &view.record.id, view.record.version)
        .await
        .unwrap();
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run,
        CancelSignal::new(),
    )
    .await;
    assert!(source.current(&agent).await.identity.is_some());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_remote_revocation_latches_fallback_until_next_turn() {
    let f = Fixture::new().await;
    configured(&f).await;
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let run = run(&f);
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run,
        CancelSignal::new(),
    )
    .await;
    assert!(source.current(&agent).await.identity.is_some());
    let mut revoked = context(&f.agent);
    revoked["access"]["mode"] = json!("legacy");
    *f.factory.context_result.lock().unwrap() = Some(revoked);
    assert!(source.current(&agent).await.identity.is_none());
    let calls = f.factory.sessions.lock().unwrap()[0]
        .calls
        .load(Ordering::SeqCst);
    *f.factory.context_result.lock().unwrap() = Some(context(&f.agent));
    assert!(source.current(&agent).await.identity.is_none());
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        calls,
        "no retries during failed run"
    );
    let next = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run,
        CancelSignal::new(),
    )
    .await;
    assert!(next.current(&agent).await.identity.is_some());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_populated_duplicated_sdk_payload_preserves_unicode_persona() {
    let f = Fixture::new().await;
    configured(&f).await;
    let persona = "温暖、诚实、清晰。与我一起思考。".repeat(190);
    let mut value = context(&f.agent);
    value["persona"] =
        json!({"content":persona,"hash":hex::encode(Sha256::digest(persona.as_bytes()))});
    value["records"] = json!((0..20)
        .map(|index| json!({"id":format!("record-{index}"),"excerpt":"上下文资料".repeat(80)}))
        .collect::<Vec<_>>());
    *f.factory.context_result.lock().unwrap() = Some(value);
    let agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
    )
    .await;
    let accepted = source.current(&agent).await;
    assert_eq!(accepted.identity.as_deref(), Some(persona.as_str()));
    assert!(accepted.reference.contains("record-0"));
    assert!(!accepted.reference.contains("record-1\""));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn workspace_context_offline_preserves_unrelated_mcp_advertisement_and_execution() {
    use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};
    let f = Fixture::new().await;
    let bound = configured(&f).await;
    *f.factory.context_result.lock().unwrap() = None;
    let unrelated = f.reviewed().await;
    let other = unrelated.record.reviewed_tools[0].tool_descriptor();
    let mut offline = context(&f.agent);
    offline["workspace"]["generation"] = json!("wrong");
    *f.factory.context_result.lock().unwrap() = Some(offline);
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![ToolCall {
            id: "unrelated-call".into(),
            name: other.name.clone(),
            args: std::collections::BTreeMap::from([(
                "value".into(),
                DataValue::String("hello".into()),
            )]),
        }]),
        Step::Text(vec!["done"]),
    ]);
    {
        let mut state = f.state.write().await;
        let mut config = state.agents[&f.agent].config().clone();
        config.tools.as_mut().unwrap().push(other.clone());
        state.restore_agent_config(&f.agent, config);
        state.approvals.set_policy(
            &f.agent,
            crate::approvals::ApprovalPolicy::default().with(
                crate::approvals::RiskClass::Exec,
                crate::approvals::PolicyAction::Allow,
            ),
        );
        state.model_adapter = model.clone();
    }
    let runs =
        crate::agent_runs::AgentRunCoordinator::new(f.state.clone(), Arc::new(Semaphore::new(4)))
            .with_mcp(f.manager.clone());
    runs.run(chat_request(
        &f.agent,
        "chat:unrelated-mcp",
        "Use the unrelated tool",
    ))
    .await
    .unwrap();
    assert!(model.configs()[0].allows_tool(&other.name));
    assert!(!model.configs()[0].allows_tool(&bound.record.reviewed_tools[0].alias));
    assert_eq!(
        f.factory.sessions.lock().unwrap()[1]
            .calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(model.requests().len(), 2);
    f.manager.shutdown().await;
}

/// Explicitly selected interoperability fixture; never reads an installed client configuration.
#[tokio::test]
#[ignore = "Requires ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER pointing to the reviewed Mother executable"]
async fn workspace_context_real_mother_stdio_identity_and_remote_revocation() {
    use crate::agent_runs::test_support::{
        calculate_call, chat_request, Gate, ScriptedModel, Step,
    };
    let executable = PathBuf::from(
        std::env::var_os("ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER")
            .expect("Set the explicit Mother fixture executable"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let f = Fixture::new().await;
    let db = f.folder.join("mother-context.sqlite");
    let soul = f.folder.join("SOUL.md");
    let client_config = f.folder.join("client.private.json");
    let home = f.folder.join("mother-home");
    std::fs::create_dir_all(&home).unwrap();
    let persona = "Synthetic stdio companion persona. 温暖、诚实、清晰。";
    std::fs::write(&soul, persona).unwrap();
    let path = |value: &std::path::Path| value.to_str().unwrap().to_owned();
    mother_cli(
        &executable,
        &home,
        &[
            "init".into(),
            "--db".into(),
            path(&db),
            "--soul".into(),
            path(&soul),
        ],
    )
    .await;
    mother_cli(
        &executable,
        &home,
        &[
            "grant".into(),
            "anima-interop".into(),
            "--access".into(),
            "companion".into(),
            "--subject".into(),
            f.agent.clone(),
            "--scopes".into(),
            "read,context:propose".into(),
            "--db".into(),
            path(&db),
            "--out".into(),
            path(&client_config),
        ],
    )
    .await;
    let context = mother_cli(
        &executable,
        &home,
        &["context".into(), "--config".into(), path(&client_config)],
    )
    .await;
    let context: serde_json::Value =
        serde_json::from_slice(&context).expect("Synthetic owner context JSON");
    let workspace = context["workspace"]["id"].as_str().unwrap();
    let generation = context["workspace"]["generation"].as_str().unwrap();
    let publication = f.state.read().await.mcp_publication.clone();
    let manager = McpManager::new(
        Arc::downgrade(&f.state),
        Arc::new(Mutex::new(())),
        publication,
        Arc::new(DefaultMcpTransportFactory::default()),
    );
    f.state.write().await.mcp_manager = Some(manager.clone());
    let row = manager
        .create_stdio(
            &f.agent,
            "Synthetic Mother interoperability".into(),
            McpStdioSpec {
                executable: path(&executable),
                args: vec!["mcp".into(), "--config".into(), path(&client_config)],
            },
        )
        .await
        .expect("Synthetic stdio metadata");
    let row = manager
        .update(
            &f.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .expect("Enable synthetic connection");
    let row = manager
        .connect(&f.agent, &row.record.id, row.record.version)
        .await
        .expect("Real Mother SDK discovery");
    let reader = row
        .candidates
        .iter()
        .find(|tool| tool.remote_name == "mother_get_context")
        .expect("Real context reader")
        .descriptor_digest
        .clone();
    let row = manager
        .review(
            &f.agent,
            &row.record.id,
            ReviewMcpTool {
                expected_version: row.record.version,
                discovery_id: row.discovery_id.clone().unwrap(),
                descriptor_digest: reader,
            },
        )
        .await
        .expect("Review only selected reader");
    let reader = row
        .record
        .reviewed_tools
        .iter()
        .find(|tool| tool.descriptor.remote_name == "mother_get_context")
        .unwrap()
        .tool_descriptor();
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![calculate_call("sum", "1+1")]),
            Step::Text(vec!["Synthetic complete"]),
        ],
        gate.clone(),
    );
    {
        let mut state = f.state.write().await;
        let mut config = state.agents[&f.agent].config().clone();
        config.system = Some("Synthetic complete local fallback personality".into());
        config.tools = Some(vec![reader.clone()]);
        config.tools.as_mut().unwrap().extend(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["calculate"])
                .unwrap(),
        );
        config.settings.get_or_insert_with(Default::default).additional.insert("workspaceContext".into(),test_data_value(&json!({"connectionId":row.record.id,"tool":"mother_get_context","mode":"companion","workspaceId":workspace,"generation":generation,"subject":f.agent})));
        state.restore_agent_config(&f.agent, config);
        state.model_adapter = model.clone();
    }
    let runs =
        crate::agent_runs::AgentRunCoordinator::new(f.state.clone(), Arc::new(Semaphore::new(4)))
            .with_mcp(manager.clone());
    let request = chat_request(
        &f.agent,
        "chat:real-mother-stdio",
        "Synthetic interoperability request",
    );
    let pending = tokio::spawn(async move { runs.run(request).await });
    gate.entered().await;
    assert!(model.requests()[0].system.contains(persona));
    assert!(!model.requests()[0]
        .system
        .contains("Synthetic complete local fallback personality"));
    mother_cli(
        &executable,
        &home,
        &[
            "revoke".into(),
            "anima-interop".into(),
            "--db".into(),
            path(&db),
        ],
    )
    .await;
    gate.release();
    gate.entered().await;
    let requests = model.requests();
    assert!(requests[1].system.contains("Workspace context unavailable"));
    assert!(requests[1]
        .system
        .contains("Synthetic complete local fallback personality"));
    assert!(!requests[1].system.contains(persona));
    assert!(!model.configs()[1].allows_tool(&reader.name));
    gate.release();
    pending.await.unwrap().unwrap();
    manager.shutdown().await;
}

async fn mother_cli(
    executable: &std::path::Path,
    home: &std::path::Path,
    args: &[String],
) -> Vec<u8> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .env("MOTHER_HOME", home)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .expect("Synthetic Mother CLI deadline")
        .expect("Synthetic Mother CLI launch");
    // Do not put stdout/stderr into assertion messages: grant commands may contain credentials.
    assert!(
        output.status.success(),
        "Synthetic Mother CLI {} failed with {:?}",
        args[0],
        output.status.code()
    );
    output.stdout
}
