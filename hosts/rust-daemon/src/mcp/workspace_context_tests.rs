use super::*;
use anima_core::{runtime::prompt_context::PromptContextSource, DataValue};
use sha2::{Digest, Sha256};

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
