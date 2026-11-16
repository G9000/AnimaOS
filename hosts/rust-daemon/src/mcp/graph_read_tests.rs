//! Synthetic additive graph reads must preserve independent native/v1 behavior.
use super::*;

#[tokio::test]
async fn memory_provider_mother_personal_health_query_never_expands_the_graph() {
    let (f, agent) = fixture(graph(vec![project()]), vec![v1()], None).await;
    for query in [
        "Is memory working?",
        "记忆连接好了吗？",
        "記憶は接続されていますか？",
        "Memori sudah bersambung?",
    ] {
        let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
            f.state.clone(),
            Some(f.manager.clone()),
            &agent,
            &run(&f),
            CancelSignal::new(),
            query.into(),
        )
        .await;
        let context = source.current(&agent).await;
        assert!(!context.reference.contains("V1-preserved sentinel"));
        assert!(context.identity.is_none());
    }
    assert!(!f
        .factory
        .garden_calls
        .lock()
        .unwrap()
        .iter()
        .any(|(n, _)| n == "mother_get_capabilities" || n.starts_with("mother_garden_")));
    f.manager.shutdown().await;
}
use anima_memory::ExternalMemoryProvider;
use serde_json::Value;

const WORKSPACE: &str = "fdb02e89-a206-4ea0-b7ab-2ebda2627175";
const GENERATION: &str = "55ba8bbb-f30d-401c-b75c-73594d27da8b";

fn graph_record(draft: Value) -> Value {
    json!({"metadata":{"id":uuid::Uuid::new_v4().to_string(),"revision":1,"previousRevision":null,
        "commitSequence":2,"transactionTime":"2026-11-14T00:00:00.000Z","actor":"synthetic-other-harness",
        "contributor":"synthetic-other-harness","approver":null,"lifecycle":"active"},"draft":draft})
}

fn project() -> Value {
    graph_record(
        json!({"kind":"entity","entityType":"synthetic:project","displayName":"AsterOS",
        "aliases":["AsterOS kit"],"extensions":{"future:opaque":{"schemaVersion":99,"payload":{"preserve":"opaque-extension"}}}}),
    )
}

fn assertion(subject: &Value, value: &str) -> Value {
    graph_record(
        json!({"kind":"assertion","subject":{"refType":"existing","kind":"entity","id":subject["metadata"]["id"]},
        "predicate":"synthetic:startup","object":{"type":"value","value":{"type":"string","value":value}},
        "validTime":{"validFrom":null,"validUntil":null,"sourceTimezone":null,"precision":"unknown"},
        "observedTime":{"at":"2026-11-13T00:00:00.000Z","sourceTimezone":"UTC","precision":"instant"},
        "evidence":[],"confidence":{"score":0.8,"basis":"source"},"extensions":{}}),
    )
}

fn graph(records: Vec<Value>) -> GardenGraphFixture {
    GardenGraphFixture {
        capabilities: json!({"version":1,"workspace":{"workspaceId":WORKSPACE,"generation":GENERATION},
            "v1":{"available":true,"contractVersion":1},"garden":{"contractVersion":2,"available":true,
                "capabilities":["garden.entities.v2","garden.assertions.v2","garden.query.v2"],
                "availableTools":["mother_garden_query","mother_garden_get"],"definedTools":["mother_garden_apply"]}}),
        records,
        query_epoch: "opaque-epoch".into(),
        get_epoch: "opaque-epoch".into(),
        deny_reads: false,
        hold_remote: None,
    }
}

fn v1() -> Value {
    json!({"id":uuid::Uuid::new_v4().to_string(),"revision":2,"title":"AsterOS theme","content":"AsterOS native fallback theme note. V1-preserved sentinel.",
        "status":"active","updatedAt":"2026-11-14T00:00:00.000Z","actor":"synthetic-v1-harness"})
}

async fn fixture(
    graph: GardenGraphFixture,
    local: Vec<Value>,
    omit_review: Option<&str>,
) -> (Fixture, anima_core::AgentState) {
    let f = Fixture::new().await;
    *f.factory.garden_graph.lock().unwrap() = Some(graph);
    *f.factory.garden_records.lock().unwrap() = Some(local.clone());
    *f.factory.garden_search_fixture.lock().unwrap() = Some(GardenSearchFixture {
        literal_records: local,
        semantic_records: vec![],
    });
    let (mut view, mut agent) = additive(&f, "local").await;
    for candidate in view.candidates.clone().iter().filter(|c| {
        c.remote_name != "mother_get_context" && Some(c.remote_name.as_str()) != omit_review
    }) {
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
    agent.config.tools = Some(
        view.record
            .reviewed_tools
            .iter()
            .map(|t| t.tool_descriptor())
            .collect(),
    );
    agent.config.tools.as_mut().unwrap().extend(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["memory_search"])
            .unwrap(),
    );
    let mut binding = crate::memory_provider::binding::binding(&agent.config)
        .unwrap()
        .unwrap();
    binding.workspace_id = WORKSPACE.into();
    binding.generation = GENERATION.into();
    agent
        .config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("memoryProvider".into(), binding.portable());
    {
        let mut context = f.factory.context_result.lock().unwrap();
        context.as_mut().unwrap()["workspace"]["id"] = json!(WORKSPACE);
        context.as_mut().unwrap()["workspace"]["generation"] = json!(GENERATION);
    }
    f.state
        .write()
        .await
        .restore_agent_config(&f.agent, agent.config.clone());
    (f, agent)
}

async fn recall(
    f: &Fixture,
    agent: &anima_core::AgentState,
    limit: usize,
) -> Vec<anima_memory::RecalledItem> {
    let provider = crate::memory_provider::mother::MotherProvider::new(
        f.state.clone(),
        f.manager.clone(),
        crate::memory_provider::binding::binding(&agent.config)
            .unwrap()
            .unwrap(),
        agent.clone(),
        run(f),
        CancelSignal::new(),
    );
    provider
        .recall(anima_memory::RecallRequest {
            agent_id: agent.id.clone(),
            query: "AsterOS".into(),
            limit,
            deadline: Duration::from_secs(3),
        })
        .await
        .unwrap()
}

async fn explicit_graph_search(
    source: &crate::workspace_context::WorkspaceRunContext,
    agent_id: &str,
) -> Result<crate::memory_provider::mother::SharedRecall, anima_memory::ExternalMemoryError> {
    source
        .search_shared(
            agent_id,
            crate::memory_provider::mother::RecallPlan::search("AsterOS", "hybrid", None, &[])
                .unwrap(),
            4,
            true,
        )
        .await
}

#[tokio::test]
async fn memory_provider_mother_graph_recalls_typed_neighbor_with_current_subject() {
    let subject = project();
    let fact = assertion(
        &subject,
        "Native startup cold restart verified. Graph-fact sentinel.",
    );
    let (f, agent) = fixture(graph(vec![subject.clone(), fact.clone()]), vec![v1()], None).await;
    let before = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    let all = recall(&f, &agent, 4).await;
    let items: Vec<_> = all
        .into_iter()
        .filter(|r| {
            r.source_id
                .as_ref()
                .is_some_and(|s| s.contains("/assertion/"))
        })
        .collect();
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Graph-fact sentinel"),
        "{}",
        items[0].text
    );
    let data: Value = serde_json::from_str(&items[0].text).unwrap();
    assert_eq!(
        data["record"],
        crate::routes::data_value_to_json(&test_data_value(&fact))
    );
    assert!(items[0].text.contains("opaque-extension"));
    assert!(items[0].source_id.as_ref().unwrap().contains("/assertion/"));
    assert_eq!(
        serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
    let calls = f.factory.garden_calls.lock().unwrap().clone();
    assert!(calls
        .iter()
        .any(|(name, args)| name == "mother_garden_query" && args["traversal"].is_object()));
    assert!(
        calls
            .iter()
            .filter(|(name, _)| name == "mother_garden_get")
            .count()
            <= 4
    );
    assert!(!calls
        .iter()
        .any(|(name, _)| name.contains("apply") || name.contains("propose")));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_rereads_another_harness_correction_without_cache() {
    let subject = project();
    let fact = assertion(&subject, "Startup-before-correction sentinel.");
    let (f, agent) = fixture(graph(vec![subject, fact.clone()]), vec![], None).await;
    assert!(recall(&f, &agent, 4)
        .await
        .iter()
        .any(|r| r.text.contains("Startup-before-correction")));
    {
        let mut fixture = f.factory.garden_graph.lock().unwrap();
        let graph = fixture.as_mut().unwrap();
        graph.records[1]["metadata"]["revision"] = json!(2);
        graph.records[1]["metadata"]["previousRevision"] = json!(1);
        graph.records[1]["metadata"]["actor"] = json!("synthetic-correcting-harness");
        graph.records[1]["draft"]["object"]["value"]["value"] =
            json!("Native startup cold restart corrected. Startup-after-correction sentinel.");
    }
    let items = recall(&f, &agent, 4).await;
    assert!(items
        .iter()
        .any(|r| r.text.contains("Startup-after-correction")
            && r.text.contains("synthetic-correcting-harness")));
    assert!(!items
        .iter()
        .any(|r| r.text.contains("Startup-before-correction")));
    assert!(items.iter().any(
        |r| r.text.contains("validFrom\":null") && r.text.contains("2026-11-13T00:00:00.000Z")
    ));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_missing_or_unsupported_capability_keeps_v1() {
    for available in [false, true] {
        let mut provider = graph(vec![project()]);
        provider.capabilities["garden"]["available"] = json!(available);
        if available {
            provider.capabilities["garden"]["contractVersion"] = json!(3);
        }
        let (f, agent) = fixture(provider, vec![v1()], None).await;
        assert!(recall(&f, &agent, 4)
            .await
            .iter()
            .any(|r| r.text.contains("V1-preserved")));
        assert!(!f
            .factory
            .garden_calls
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name.starts_with("mother_garden_")));
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn memory_provider_mother_graph_propose_only_denial_never_becomes_read_authority() {
    let mut provider = graph(vec![project()]);
    provider.deny_reads = true;
    let (f, agent) = fixture(provider, vec![v1()], None).await;
    assert!(recall(&f, &agent, 4)
        .await
        .iter()
        .any(|r| r.text.contains("V1-preserved")));
    let calls = f.factory.garden_calls.lock().unwrap().clone();
    assert!(calls.iter().any(|(name, _)| name == "mother_garden_query"));
    assert!(!calls.iter().any(|(name, _)| name == "mother_garden_get"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_requires_reviewed_readers_without_granting_them() {
    let (f, agent) = fixture(
        graph(vec![project()]),
        vec![v1()],
        Some("mother_garden_get"),
    )
    .await;
    let before = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    assert!(recall(&f, &agent, 4)
        .await
        .iter()
        .any(|r| r.text.contains("V1-preserved")));
    assert!(!f
        .factory
        .garden_calls
        .lock()
        .unwrap()
        .iter()
        .any(|(name, _)| name == "mother_get_capabilities" || name.starts_with("mother_garden_")));
    assert_eq!(
        serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_changed_epoch_discards_graph_without_poisoning_v1() {
    let subject = project();
    let mut provider = graph(vec![
        subject.clone(),
        assertion(&subject, "Stale-epoch secret sentinel."),
    ]);
    provider.get_epoch = "changed-epoch".into();
    let (f, agent) = fixture(provider, vec![v1()], None).await;
    let items = recall(&f, &agent, 4).await;
    assert!(items.iter().any(|r| r.text.contains("V1-preserved")));
    assert!(!items.iter().any(|r| r.text.contains("Stale-epoch secret")));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_timeout_retires_shared_authority_and_recovers_next_turn() {
    for remote in [
        "mother_get_capabilities",
        "mother_garden_query",
        "mother_garden_get",
    ] {
        let subject = project();
        let mut provider = graph(vec![
            subject.clone(),
            assertion(&subject, "Unreturned graph sentinel."),
        ]);
        provider.hold_remote = Some(remote.into());
        let (f, agent) = fixture(provider, vec![v1()], None).await;
        let cancel = CancelSignal::new();
        let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
            f.state.clone(),
            Some(f.manager.clone()),
            &agent,
            &run(&f),
            cancel.clone(),
            "AsterOS".into(),
        )
        .await;
        assert!(explicit_graph_search(&source, &agent.id).await.is_err());
        assert!(agent.config.allows_tool("memory_search"));
        assert!(!cancel.is_cancelled());
        assert!(
            f.factory
                .garden_calls
                .lock()
                .unwrap()
                .iter()
                .any(|(name, _)| name == remote),
            "optional {remote} not exercised"
        );
        f.factory
            .garden_graph
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .hold_remote = None;
        let connection = crate::memory_provider::binding::binding(&agent.config)
            .unwrap()
            .unwrap()
            .connection_id;
        timeout(Duration::from_secs(1), async {
            while f
                .manager
                .get(&agent.id, &connection)
                .await
                .unwrap()
                .physical_calls_active
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let fresh = crate::workspace_context::WorkspaceRunContext::load_for_query(
            f.state.clone(),
            Some(f.manager.clone()),
            &agent,
            &run(&f),
            CancelSignal::new(),
            "AsterOS".into(),
        )
        .await;
        let recovered = explicit_graph_search(&fresh, &agent.id).await.unwrap();
        assert!(recovered
            .items
            .iter()
            .any(|r| r.text.contains("V1-preserved")));
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn memory_provider_mother_graph_prompt_context_has_typed_locators_and_local_identity() {
    let subject = project();
    let fact = assertion(
        &subject,
        "Current graph startup evidence. Context-graph sentinel.",
    );
    let (f, agent) = fixture(graph(vec![subject, fact]), vec![v1()], None).await;
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
        "AsterOS".into(),
    )
    .await;
    let bootstrap = source.current(&agent).await;
    assert!(
        bootstrap.identity.is_none() && !bootstrap.reference.contains("Context-graph sentinel")
    );
    let result = explicit_graph_search(&source, &agent.id).await.unwrap();
    assert!(result
        .items
        .iter()
        .any(|r| r.text.contains("Context-graph sentinel")
            && r.text.contains("garden.v2")
            && r.source_id.as_ref().unwrap().contains("/assertion/")));
    assert_eq!(result.coverage["graph"]["mode"], "literal");
    assert_eq!(result.coverage["graph"]["status"], "queried");
    assert!(agent.config.allows_tool("memory_search"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_cancellation_cannot_publish_late_shared_context() {
    let subject = project();
    let mut graph = graph(vec![
        subject.clone(),
        assertion(&subject, "Late-graph sentinel."),
    ]);
    graph.hold_remote = Some("mother_garden_query".into());
    let (f, agent) = fixture(graph, vec![v1()], None).await;
    let cancel = CancelSignal::new();
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        cancel.clone(),
        "AsterOS".into(),
    )
    .await;
    let executing_agent = agent.clone();
    let pending =
        tokio::spawn(async move { explicit_graph_search(&source, &executing_agent.id).await });
    timeout(Duration::from_secs(1), async {
        while !f
            .factory
            .garden_calls
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "mother_garden_query")
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    assert!(pending.await.unwrap().is_err());
    assert!(agent.config.allows_tool("memory_search"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_wrong_capability_identity_invalidates_shared_context() {
    let mut provider = graph(vec![project()]);
    provider.capabilities["workspace"]["generation"] = json!("restored-generation");
    let (f, agent) = fixture(provider, vec![v1()], None).await;
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
        "AsterOS".into(),
    )
    .await;
    assert!(explicit_graph_search(&source, &agent.id).await.is_err());
    assert!(agent.config.allows_tool("memory_search"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_defined_future_contract_is_not_available_query() {
    let mut provider = graph(vec![project()]);
    provider.capabilities["garden"]["capabilities"] = json!([]);
    provider.capabilities["garden"]["definedCapabilities"] = json!(["garden.query.v2"]);
    let (f, agent) = fixture(provider, vec![v1()], None).await;
    assert!(recall(&f, &agent, 4)
        .await
        .iter()
        .any(|r| r.text.contains("V1-preserved")));
    let calls = f.factory.garden_calls.lock().unwrap().clone();
    assert!(calls
        .iter()
        .any(|(name, _)| name == "mother_get_capabilities"));
    assert!(!calls
        .iter()
        .any(|(name, _)| name.starts_with("mother_garden_")));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_keeps_owner_precedence_complete_bounds_and_coverage() {
    let subject = project();
    let mut records = vec![subject.clone()];
    records.extend((0..6).map(|i| {
        assertion(
            &subject,
            &format!("Native startup cold restart graph claim {i}. Complete-claim-{i} sentinel."),
        )
    }));
    let local = v1();
    let id = local["id"].clone();
    let (f, agent) = fixture(graph(records), vec![local.clone()], None).await;
    f.factory.context_result.lock().unwrap().as_mut().unwrap()["pinned"] = json!([local]);
    let items = recall(&f, &agent, 10).await;
    assert_eq!(items.len(), 4);
    assert!(items[0]
        .source_id
        .as_ref()
        .unwrap()
        .contains(id.as_str().unwrap()));
    assert!(items.iter().map(|r| r.text.len()).sum::<usize>() <= 8 * 1024);
    let graph_items: Vec<Value> = items
        .iter()
        .filter_map(|r| {
            let data: Value = serde_json::from_str(&r.text).unwrap();
            (data["format"] == "garden.v2").then_some(data)
        })
        .collect();
    assert!(!graph_items.is_empty());
    assert!(graph_items.iter().any(|item| item["queryCoverage"]
        .as_array()
        .unwrap()
        .iter()
        .any(|q| q["coverage"]["truncated"] == true)));
    assert!(graph_items
        .iter()
        .any(|item| item["record"]["draft"]["kind"] == "assertion"));
    for item in graph_items {
        assert!(item["record"]["metadata"]["id"].is_string());
        assert_eq!(item["temporalSelection"], "current");
        match item["record"]["draft"]["kind"].as_str() {
            Some("assertion") => assert!(item["record"]["draft"]["object"]["value"]["value"]
                .as_str()
                .unwrap()
                .ends_with("sentinel.")),
            Some("entity") => assert_eq!(item["record"]["draft"]["displayName"], "AsterOS"),
            kind => panic!("unexpected synthetic graph kind: {kind:?}"),
        }
    }
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_graph_inactive_and_oversized_data_do_not_replace_valid_v1() {
    for oversized in [false, true] {
        let subject = project();
        let mut fact = assertion(&subject, "Must-not-inject sentinel.");
        if oversized {
            fact["draft"]["extensions"] =
                json!({"future:large":{"schemaVersion":1,"payload":"x".repeat(20_000)}});
        } else {
            fact["metadata"]["lifecycle"] = json!("retracted");
        }
        let (f, agent) = fixture(graph(vec![subject, fact]), vec![v1()], None).await;
        let items = recall(&f, &agent, 4).await;
        assert!(items.iter().any(|r| r.text.contains("V1-preserved")));
        assert!(!items.iter().any(|r| r.text.contains("Must-not-inject")));
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn memory_provider_mother_graph_insufficient_deadline_reserve_skips_optional_work() {
    let mut graph = graph(vec![project()]);
    graph.hold_remote = Some("mother_get_capabilities".into());
    let (f, agent) = fixture(graph, vec![v1()], None).await;
    let provider = crate::memory_provider::mother::MotherProvider::new(
        f.state.clone(),
        f.manager.clone(),
        crate::memory_provider::binding::binding(&agent.config)
            .unwrap()
            .unwrap(),
        agent.clone(),
        run(&f),
        CancelSignal::new(),
    );
    let result = provider
        .recall(anima_memory::RecallRequest {
            agent_id: agent.id.clone(),
            query: "AsterOS".into(),
            limit: 4,
            deadline: Duration::from_millis(500),
        })
        .await
        .unwrap();
    assert!(result.iter().any(|r| r.text.contains("V1-preserved")));
    assert!(!f
        .factory
        .garden_calls
        .lock()
        .unwrap()
        .iter()
        .any(|(name, _)| name == "mother_get_capabilities"));
    f.manager.shutdown().await;
}
