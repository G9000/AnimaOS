//! Optional real-executable provider acceptance, exclusively on owned synthetic data.
use super::*;
use anima_memory::{ExternalMemoryProvider, MemoryScope, MemoryType, NewMemory};

#[tokio::test]
#[ignore = "Requires ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER; owned synthetic personal-recall garden only"]
async fn native_mother_model_authored_personal_search_resolves_full_fact_without_identifiers() {
    use anima_core::runtime::prompt_context::PromptContextSource;
    let executable = PathBuf::from(
        std::env::var_os("ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER")
            .expect("explicit qualified Mother executable"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let f = Fixture::new().await;
    let home = f.folder.join("native-personal-citation");
    std::fs::create_dir(&home).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut owner = NativeGraphOwner {
        executable,
        database: home.join("synthetic.redb"),
        endpoint: home.join("service.json"),
        home,
        process: None,
    };
    let identity = owner
        .cli(&["garden", "init", "--db", path(&owner.database)])
        .await;
    let soul = owner.home.join("SOUL.md");
    std::fs::write(&soul, format!("Synthetic persona {}", "p".repeat(7000))).unwrap();
    owner
        .cli(&[
            "garden",
            "compat-init",
            "--db",
            path(&owner.database),
            "--soul",
            path(&soul),
        ])
        .await;
    let direct = owner.home.join("reader.private.json");
    owner
        .cli(&[
            "garden",
            "grant-v1",
            "synthetic-personal",
            "--db",
            path(&owner.database),
            "--out",
            path(&direct),
            "--access",
            "companion",
            "--subject",
            &f.agent,
            "--all",
            "--scopes",
            "read,memory:write",
        ])
        .await;
    owner.start().await;
    let manager = McpManager::new(
        Arc::downgrade(&f.state),
        Arc::new(Mutex::new(())),
        f.state.read().await.mcp_publication.clone(),
        Arc::new(DefaultMcpTransportFactory::default()),
    );
    f.state.write().await.mcp_manager = Some(manager.clone());
    let row = connection(&f, &manager, &owner, &direct, "personal-citation").await;
    let run = {
        let mut state = f.state.write().await;
        let sid = "chat:synthetic-personal-citation";
        state.sessions.insert(SessionRecord::new(
            &f.agent,
            sid,
            SessionKind::Chat,
            SessionOrigin::Web,
            "Synthetic citation recall".into(),
            TitleSource::Owner,
            1,
        ));
        let record = RunRecord::running(
            RunStart {
                agent_id: f.agent.clone(),
                session_id: sid.into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "Synthetic personal recall".into(),
                model: "synthetic-no-model-calls".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        let run_id = record.id.clone();
        state.runs.insert(record);
        RunLink {
            agent_id: f.agent.clone(),
            session_id: sid.into(),
            run_id,
        }
    };
    let create = |title: &str, content: &str| {
        json!({"workspaceId":identity["workspace"]["id"],"generation":identity["workspace"]["generation"],
        "operationId":uuid::Uuid::new_v4().to_string(),"expectedRevision":0,"record":{"kind":"memory","status":"active","basis":"owner_statement","title":title,"content":content}})
    };
    let pet = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_write_record",
        create(
            "Household pets",
            "My cat is called Vega. Synthetic-personal-context sentinel.",
        ),
    )
    .await;
    for index in 0..12 {
        graph_call(
            &f,
            &manager,
            &row,
            &run,
            "mother_write_record",
            create(
                &format!("Education application {index}"),
                "Education application note. Unrelated-substring sentinel.",
            ),
        )
        .await;
    }
    let envelope = json!({"workspaceId":identity["workspace"]["id"],"generation":identity["workspace"]["generation"]});
    let mut agent = bind(&f, &row, &envelope).await;
    agent.config.tools.as_mut().unwrap().extend(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["memory_search"])
            .unwrap(),
    );
    f.state
        .write()
        .await
        .restore_agent_config(&f.agent, agent.config.clone());
    let before = serde_json::to_value(&agent.config).unwrap();
    let query="Can you remember my cat's name now? Please verify the saved detail with a fresh read from Mother and cite the record ID and revision. Also search your own native memory for earlier project work, with at most one result. Keep sharing explicit.";
    assert!(!query.contains("Vega") && !query.contains(pet["record"]["id"].as_str().unwrap()));
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(manager.clone()),
        &agent,
        &run,
        CancelSignal::new(),
        query.into(),
    )
    .await;
    let context = source.current(&agent).await;
    assert!(!context
        .reference
        .contains("Synthetic-personal-context sentinel"));
    assert!(context.identity.is_none() && !context.withheld_tools.contains("memory_search"));
    let result = source
        .search_shared(
            &agent.id,
            crate::memory_provider::mother::RecallPlan::search(
                "pets",
                "literal",
                Some("memory"),
                &[],
            )
            .unwrap(),
            4,
            false,
        )
        .await
        .unwrap();
    assert!(result
        .items
        .iter()
        .any(|r| r.text.contains("Synthetic-personal-context sentinel")
            && r.text.contains(pet["record"]["id"].as_str().unwrap())));
    assert!(!result
        .items
        .iter()
        .any(|r| r.text.contains("Unrelated-substring sentinel")));
    assert_eq!(
        serde_json::to_value(f.state.read().await.agents[&f.agent].config()).unwrap(),
        before
    );
    let after = graph_call(
        &f,
        &manager,
        &row,
        &run,
        "mother_get_context",
        json!({"limit":1}),
    )
    .await;
    assert_eq!(after["inventory"]["totalRecords"], 13.0);
    manager.shutdown().await;
    owner.stop().await;
}

async fn connection(
    f: &Fixture,
    manager: &McpManager,
    owner: &NativeGraphOwner,
    direct: &Path,
    label: &str,
) -> McpConnectionView {
    let service = owner.home.join(format!("{label}.service.private.json"));
    owner
        .cli(&[
            "garden",
            "service-config",
            "--config",
            path(direct),
            "--endpoint",
            path(&owner.endpoint),
            "--out",
            path(&service),
        ])
        .await;
    let row = manager
        .create_stdio(
            &f.agent,
            label.into(),
            McpStdioSpec {
                executable: path(&owner.executable).into(),
                args: vec!["mcp".into(), "--config".into(), path(&service).into()],
            },
        )
        .await
        .unwrap();
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
        .unwrap();
    let mut row = manager
        .connect(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    for candidate in row.candidates.clone() {
        row = manager
            .review(
                &f.agent,
                &row.record.id,
                ReviewMcpTool {
                    expected_version: row.record.version,
                    discovery_id: row.discovery_id.clone().unwrap(),
                    descriptor_digest: candidate.descriptor_digest,
                },
            )
            .await
            .unwrap();
    }
    for tool in row.record.reviewed_tools.clone() {
        row = manager
            .set_grant(
                &f.agent,
                &row.record.id,
                &tool.alias,
                McpGrantInput {
                    expected_version: row.record.version,
                    descriptor_digest: tool.descriptor.descriptor_digest,
                    granted: true,
                },
            )
            .await
            .unwrap()
            .connection;
    }
    row
}

async fn bind(f: &Fixture, row: &McpConnectionView, envelope: &Value) -> anima_core::AgentState {
    let mut state = f.state.write().await;
    let mut config = state.agents[&f.agent].config().clone();
    let binding: crate::memory_provider::binding::MemoryProviderBinding=serde_json::from_value(json!({
        "version":1,"provider":"mother","connectionId":row.record.id,"workspaceId":envelope["workspaceId"],
        "generation":envelope["generation"],"subject":f.agent,"publication":"selected","personaSource":"local","accessMode":"companion"
    })).unwrap();
    let settings = config.settings.get_or_insert_with(Default::default);
    settings.additional.remove("workspaceContext");
    settings
        .additional
        .insert("memoryProvider".into(), binding.portable());
    config.system = Some("Synthetic independent local persona".into());
    state.restore_agent_config(&f.agent, config);
    state.get_agent(&f.agent).unwrap().state
}

async fn recall(
    f: &Fixture,
    manager: &McpManager,
    agent: anima_core::AgentState,
    run: &RunLink,
) -> Vec<anima_memory::RecalledItem> {
    let provider = crate::memory_provider::mother::MotherProvider::new(
        f.state.clone(),
        manager.clone(),
        crate::memory_provider::binding::binding(&agent.config)
            .unwrap()
            .unwrap(),
        agent,
        run.clone(),
        CancelSignal::new(),
    );
    provider
        .recall(anima_memory::RecallRequest {
            agent_id: f.agent.clone(),
            query: "AsterOS".into(),
            limit: 4,
            deadline: Duration::from_secs(3),
        })
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "Requires ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER; disposable synthetic native store and grants only"]
async fn native_mother_additive_graph_provider_reads_corrections_and_respects_independent_scope() {
    let executable = PathBuf::from(
        std::env::var_os("ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER")
            .expect("explicit native Mother executable"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let f = Fixture::new().await;
    let home = f.folder.join("a6-native-mother");
    std::fs::create_dir(&home).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut owner = NativeGraphOwner {
        executable,
        database: home.join("garden.redb"),
        endpoint: home.join("service.json"),
        home,
        process: None,
    };
    let identity = owner
        .cli(&["garden", "init", "--db", path(&owner.database)])
        .await;
    let soul = owner.home.join("SOUL.md");
    std::fs::write(
        &soul,
        "Synthetic optional persona; never replace local identity",
    )
    .unwrap();
    owner
        .cli(&[
            "garden",
            "compat-init",
            "--db",
            path(&owner.database),
            "--soul",
            path(&soul),
        ])
        .await;
    let reader = owner.home.join("reader.private.json");
    let writer = owner.home.join("writer.private.json");
    let proposer = owner.home.join("proposer.private.json");
    for (name, config, scopes) in [
        ("a6-reader", &reader, "read,context:propose,garden:read"),
        (
            "a6-propose-only",
            &proposer,
            "read,context:propose,garden:propose",
        ),
    ] {
        owner
            .cli(&[
                "garden",
                "grant-v1",
                name,
                "--db",
                path(&owner.database),
                "--out",
                path(config),
                "--access",
                "companion",
                "--subject",
                &f.agent,
                "--all",
                "--scopes",
                scopes,
            ])
            .await;
    }
    owner
        .cli(&[
            "garden",
            "grant",
            "a6-other-harness",
            "--db",
            path(&owner.database),
            "--out",
            path(&writer),
            "--access",
            "companion",
            "--subject",
            "synthetic-other-harness",
            "--all",
            "--scopes",
            "garden:read,garden:write",
        ])
        .await;
    owner.start().await;
    let publication = f.state.read().await.mcp_publication.clone();
    let manager = McpManager::new(
        Arc::downgrade(&f.state),
        Arc::new(Mutex::new(())),
        publication,
        Arc::new(DefaultMcpTransportFactory::default()),
    );
    f.state.write().await.mcp_manager = Some(manager.clone());
    let reader = connection(&f, &manager, &owner, &reader, "a6-reader").await;
    let writer = connection(&f, &manager, &owner, &writer, "a6-writer").await;
    let proposer = connection(&f, &manager, &owner, &proposer, "a6-proposer").await;
    let run = {
        let mut state = f.state.write().await;
        let sid = "chat:real-native-provider";
        state.sessions.insert(SessionRecord::new(
            &f.agent,
            sid,
            SessionKind::Chat,
            SessionOrigin::Web,
            "Synthetic provider acceptance".into(),
            TitleSource::Owner,
            1,
        ));
        let record = RunRecord::running(
            RunStart {
                agent_id: f.agent.clone(),
                session_id: sid.into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "Synthetic provider recall".into(),
                model: "synthetic-no-model-calls".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        let run_id = record.id.clone();
        state.runs.insert(record);
        RunLink {
            agent_id: f.agent.clone(),
            session_id: sid.into(),
            run_id,
        }
    };
    let envelope = json!({"version":2,"workspaceId":identity["workspace"]["id"],"generation":identity["workspace"]["generation"],"requiredExtensions":{}});
    let mut batch = envelope.clone();
    batch["operationId"] = json!(uuid::Uuid::new_v4().to_string());
    let claim = json!({"kind":"assertion","subject":{"refType":"item","kind":"entity","itemKey":"project"},"predicate":"synthetic:startup",
        "object":{"type":"value","value":{"type":"string","value":"Native startup cold restart initial. Native-graph-before sentinel."}},
        "validTime":{"validFrom":null,"validUntil":null,"sourceTimezone":null,"precision":"unknown"},
        "observedTime":{"at":"2026-11-14T00:00:00Z","sourceTimezone":"UTC","precision":"instant"},"evidence":[],"confidence":null,"extensions":{}});
    batch["items"] = json!([
        {"itemKey":"project","mutation":"create","expectedRevision":0,"draft":{"kind":"entity","entityType":"synthetic:project","displayName":"AsterOS",
            "aliases":[],"extensions":{"future:preserve":{"schemaVersion":99,"payload":{"opaque":"native-extension sentinel"}}}}},
        {"itemKey":"claim","mutation":"create","expectedRevision":0,"draft":claim}
    ]);
    let applied = graph_call(&f, &manager, &writer, &run, "mother_garden_apply", batch).await;
    let results = applied["receipt"]["results"].as_array().unwrap();
    let project = results.iter().find(|r| r["itemKey"] == "project").unwrap()["reference"].clone();
    let claim_ref = results.iter().find(|r| r["itemKey"] == "claim").unwrap()["reference"].clone();
    let memory = f.state.read().await.memory.clone();
    let local = memory
        .write()
        .await
        .add(NewMemory {
            agent_id: f.agent.clone(),
            agent_name: "Synthetic graph acceptance".into(),
            memory_type: MemoryType::Fact,
            content: "Local private graph-acceptance sentinel remains native".into(),
            importance: 0.8,
            tags: None,
            scope: Some(MemoryScope::Private),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap();
    let original = serde_json::to_value(&local).unwrap();
    let agent = bind(&f, &reader, &envelope).await;
    let before = recall(&f, &manager, agent.clone(), &run).await;
    assert!(
        before
            .iter()
            .any(|r| r.text.contains("Native-graph-before") && r.text.contains("a6-other-harness")),
        "actual native graph provider returned no selected assertion"
    );
    assert!(before
        .iter()
        .any(|r| r.text.contains("native-extension sentinel")));
    assert!(before.iter().any(|r| r
        .source_id
        .as_ref()
        .is_some_and(|s| s.contains("/assertion/"))));
    let mut corrected = claim;
    corrected["subject"] = json!({"refType":"existing","kind":"entity","id":project["id"]});
    corrected["object"]["value"]["value"] =
        json!("Native startup cold restart corrected. Native-graph-after sentinel.");
    let mut update = envelope.clone();
    update["operationId"] = json!(uuid::Uuid::new_v4().to_string());
    update["items"] = json!([{"itemKey":"correction","mutation":"update","expectedRevision":1,"id":claim_ref,"draft":corrected}]);
    let changed = graph_call(&f, &manager, &writer, &run, "mother_garden_apply", update).await;
    assert_eq!(
        changed["receipt"]["results"][0]["revision"].as_f64(),
        Some(2.0)
    );
    let after = recall(&f, &manager, agent, &run).await;
    assert!(after.iter().any(|r| r.text.contains("Native-graph-after")));
    assert!(!after.iter().any(|r| r.text.contains("Native-graph-before")));
    let mut historical = envelope.clone();
    historical["reference"] = claim_ref;
    historical["revision"] = json!(1);
    historical["knownAt"] = Value::Null;
    let old = graph_call(&f, &manager, &reader, &run, "mother_garden_get", historical).await;
    assert!(old["record"].to_string().contains("Native-graph-before"));
    let denied_capabilities = graph_call(
        &f,
        &manager,
        &proposer,
        &run,
        "mother_get_capabilities",
        json!({}),
    )
    .await;
    assert_eq!(
        denied_capabilities["garden"]["available"], true,
        "propose-only capability must not be treated as read permission"
    );
    let denied = bind(&f, &proposer, &envelope).await;
    assert!(recall(&f, &manager, denied.clone(), &run).await.is_empty());
    let context = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(manager.clone()),
        &denied,
        &run,
        CancelSignal::new(),
        "Check AsterOS native startup".into(),
    )
    .await;
    use anima_core::runtime::prompt_context::PromptContextSource;
    let prompt = context.current(&denied).await;
    assert!(prompt.identity.is_none() && prompt.reference.contains("ADDITIVE memory provider"));
    assert!(!prompt.reference.contains("Native-graph-after"));
    assert_eq!(
        serde_json::to_value(memory.read().await.get(&local.id).unwrap()).unwrap(),
        original
    );
    assert!(!f.folder.join("native-memory.json").exists());
    assert!(!owner.home.join("context.sqlite").exists());
    manager.shutdown().await;
    owner.stop().await;
}
