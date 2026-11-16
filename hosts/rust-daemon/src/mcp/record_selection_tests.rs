//! Selected garden evidence must match the requested topic, not just semantic proximity.
use super::*;
use anima_memory::ExternalMemoryProvider;
use serde_json::Value;

#[tokio::test]
async fn memory_provider_mother_selection_large_preview_window_retries_smaller_complete_window() {
    let right = record(
        "AsterOS native startup published",
        &format!(
            "AsterOS native login startup and cold restart verified. Budget-topic sentinel. {}",
            "x".repeat(350)
        ),
    );
    let mut records = vec![right];
    records.extend((0..7).map(|i| {
        record(
            &format!("AsterOS reference {i}"),
            &format!("AsterOS reference note. {}", "y".repeat(390)),
        )
    }));
    let (f, agent) = selection_fixture(records.clone(), records, vec![]).await;
    {
        let persona = "p".repeat(11_000);
        let mut context = f.factory.context_result.lock().unwrap();
        context.as_mut().unwrap()["persona"] =
            json!({"content":persona,"hash":hex::encode(Sha256::digest(persona.as_bytes()))});
    }
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
        "Check Mother AsterOS native login startup and cold restart".into(),
    )
    .await;
    let context = source.current(&agent).await;
    assert!(
        context.reference.contains("Budget-topic sentinel"),
        "{}",
        context.reference
    );
    assert!(!context.reference.contains("Shared garden unavailable"));
    assert!(
        context.withheld_tools.is_empty(),
        "partial search must not disable authorized readers after a complete bounded retry"
    );
    let searches: Vec<_> = f
        .factory
        .garden_calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "mother_search_context")
        .map(|(_, args)| args.clone())
        .collect();
    assert!(searches.iter().any(|args| args["limit"] == 8));
    assert!(searches.iter().any(|args| args["limit"] == 4));
    assert!(searches.len() <= 2);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_shared_topic_wins_over_separate_native_query_phrase() {
    let native = record(
        "Native citation note",
        "foundation and repo native memory citation blocker. Separate-query sentinel.",
    );
    let project=record("AsterOS additive startup published","AsterOS additive-provider implementation published; native login startup and cold restart verified. Shared-topic sentinel.");
    let records = vec![native, project];
    let (f, agent) = selection_fixture(records.clone(), records, vec![]).await;
    let query="Read-only recall diagnostic. Check Mother shared memories about publishing the AsterOS additive-provider implementation and verifying native login startup and cold restarts. Also search your own native memory for the exact phrase \"foundation and repo\" and report its citation blocker.";
    let items = recall(&f, &agent, query, 1).await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Shared-topic sentinel"),
        "{}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_event_in_content_beats_unrelated_title() {
    let theme = record(
        "AsterOS theme",
        "AsterOS palette adjusted. Unrelated-theme sentinel.",
    );
    let event = record(
        "Native startup verified",
        "AsterOS native login cold restart verified. Event-content sentinel.",
    );
    let records = vec![theme, event];
    let (f, agent) = selection_fixture(records.clone(), records, vec![]).await;
    let items = recall(&f, &agent, "Check AsterOS native login cold restart", 1).await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Event-content sentinel"),
        "{}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_quoted_topic_beats_protocol_acronym() {
    let wrong = record(
        "MCP transport",
        "MCP native startup verified. Protocol-only sentinel.",
    );
    let right = record(
        "lilt startup",
        "lilt native startup verified. Quoted-topic sentinel.",
    );
    let (f, agent) = selection_fixture(
        vec![wrong.clone(), right.clone()],
        vec![wrong.clone(), right],
        vec![wrong],
    )
    .await;
    let items = recall(&f, &agent, "Use MCP to recall \"lilt\" native startup", 1).await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Quoted-topic sentinel"),
        "{}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_mixed_case_project_beats_protocol_acronyms() {
    let wrong = record(
        "API UI startup",
        "API UI startup verified. Protocol-only sentinel.",
    );
    let right = record(
        "AsterOS startup",
        "AsterOS startup verified. Explicit-project sentinel.",
    );
    let (f, agent) = selection_fixture(
        vec![wrong.clone(), right.clone()],
        vec![wrong.clone(), right],
        vec![wrong],
    )
    .await;
    let items = recall(&f, &agent, "API UI AsterOS startup", 1).await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Explicit-project sentinel"),
        "{}",
        items[0].text
    );
    f.manager.shutdown().await;
}

fn record(title: &str, content: &str) -> Value {
    json!({"id":uuid::Uuid::new_v4().to_string(),"revision":2,"title":title,"content":content,
        "status":"active","updatedAt":"2026-11-14T00:00:00.000Z","actor":"synthetic-harness"})
}

async fn selection_fixture(
    full: Vec<Value>,
    literal: Vec<Value>,
    semantic: Vec<Value>,
) -> (Fixture, anima_core::AgentState) {
    let f = Fixture::new().await;
    *f.factory.garden_records.lock().unwrap() = Some(full);
    *f.factory.garden_search_fixture.lock().unwrap() = Some(GardenSearchFixture {
        literal_records: literal,
        semantic_records: semantic,
    });
    let (mut view, mut agent) = additive(&f, "local").await;
    for candidate in view
        .candidates
        .clone()
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
    agent.config.tools = Some(
        view.record
            .reviewed_tools
            .iter()
            .map(|t| t.tool_descriptor())
            .collect(),
    );
    f.state
        .write()
        .await
        .restore_agent_config(&f.agent, agent.config.clone());
    (f, agent)
}

async fn recall(
    f: &Fixture,
    agent: &anima_core::AgentState,
    query: &str,
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
            query: query.into(),
            limit,
            deadline: Duration::from_secs(3),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn memory_provider_mother_selection_exact_topic_recovers_from_partial_semantic_index() {
    let wrong = record(
        "Aster v1 browser review",
        "Aster browser acceptance is underway. Wrong-project sentinel.",
    );
    let right = record("AsterOS additive startup published", "AsterOS additive-provider implementation published; native login startup and cold restart verified. Correct-project sentinel.");
    let (f, agent) = selection_fixture(
        vec![wrong.clone(), right.clone()],
        vec![wrong.clone(), right.clone()],
        vec![wrong],
    )
    .await;
    let before = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    let query = format!("Read-only recall diagnostic. {} Check Mother shared memories about publishing the AsterOS additive-provider implementation and verifying native login startup and cold restarts.", "Find the appropriate shared evidence without guessing. ".repeat(5));
    let items = recall(&f, &agent, &query, 1).await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Correct-project sentinel"),
        "{}",
        items[0].text
    );
    assert!(!items[0].text.contains("Wrong-project sentinel"));
    assert!(items[0]
        .source_id
        .as_ref()
        .unwrap()
        .ends_with(&format!("/{}@2", right["id"].as_str().unwrap())));
    let calls = f.factory.garden_calls.lock().unwrap().clone();
    assert!(calls
        .iter()
        .any(|(name, args)| name == "mother_search_context"
            && args["mode"] == "literal"
            && args["query"] == "AsterOS"));
    assert!(
        calls
            .iter()
            .filter(|(name, _)| name == "mother_search_context")
            .count()
            <= 3
    );
    assert!(calls.iter().all(|(name, _)| matches!(
        name.as_str(),
        "mother_get_context" | "mother_search_context" | "mother_get_record"
    )));
    assert_eq!(
        serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_intent_beats_newer_same_project_preview() {
    let theme = record(
        "AsterOS theme updated",
        "AsterOS theme palette adjusted. Unrelated-theme sentinel.",
    );
    let mut release = record("AsterOS native login startup published", "AsterOS additive-provider implementation published; native login startup and cold restart verified. Older-release sentinel.");
    release["updatedAt"] = json!("2026-11-01T00:00:00.000Z");
    let (f, agent) = selection_fixture(
        vec![theme.clone(), release.clone()],
        vec![theme.clone(), release],
        vec![theme],
    )
    .await;
    let items = recall(
        &f,
        &agent,
        "What did we publish for AsterOS additive-provider native login startup and cold restart?",
        1,
    )
    .await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Older-release sentinel"),
        "{}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_similar_project_name_is_not_exact_evidence() {
    let wrong = record(
        "Aster v1 native startup",
        "Aster implementation published and cold restart verified. Wrong-project sentinel.",
    );
    let (f, agent) = selection_fixture(vec![wrong.clone()], vec![wrong.clone()], vec![wrong]).await;
    let items = recall(
        &f,
        &agent,
        "Find AsterOS native startup and cold restart evidence",
        4,
    )
    .await;
    assert!(
        items.is_empty(),
        "similar project must not establish recall: {}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_literal_hit_after_excerpt_still_resolves_full_record() {
    let right = record(
        "Native startup evidence",
        &format!(
            "{} AsterOS native login and cold restart verified. Late-topic sentinel.",
            "Synthetic background. ".repeat(30)
        ),
    );
    let wrong = record(
        "Aster browser service",
        "Aster browser acceptance underway.",
    );
    let (f, agent) =
        selection_fixture(vec![wrong.clone(), right.clone()], vec![right], vec![wrong]).await;
    let items = recall(
        &f,
        &agent,
        "Check AsterOS native login and cold restart evidence",
        1,
    )
    .await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Late-topic sentinel"),
        "{}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_rechecks_topic_in_current_full_record() {
    let preview = record("AsterOS native startup", "AsterOS native startup evidence.");
    let mut current = preview.clone();
    current["title"] = json!("OtherProject startup");
    current["content"] = json!("OtherProject replaced the old attribution. Stale-topic sentinel.");
    current["revision"] = json!(3);
    let (f, agent) = selection_fixture(vec![current], vec![preview.clone()], vec![preview]).await;
    let items = recall(&f, &agent, "Check AsterOS native startup evidence", 1).await;
    assert!(
        items.is_empty(),
        "stale preview must not authorize unrelated current evidence: {}",
        items[0].text
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_prompt_context_contains_current_topic_without_record_id()
{
    let wrong = record(
        "Aster v1 service",
        "Aster browser review only. Wrong-project sentinel.",
    );
    let right = record("AsterOS additive implementation published", "AsterOS additive-provider native login startup and cold restart verified. Full-topic sentinel.");
    let (f, agent) = selection_fixture(
        vec![wrong.clone(), right.clone()],
        vec![wrong.clone(), right],
        vec![wrong],
    )
    .await;
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),Some(f.manager.clone()),&agent,&run(&f),CancelSignal::new(),
        "Check Mother shared memories about AsterOS additive-provider native login startup and cold restart".into()).await;
    let context = source.current(&agent).await;
    assert!(
        context.identity.is_none(),
        "local identity must remain independent"
    );
    assert!(context.reference.contains("ADDITIVE memory provider"));
    assert!(
        context.reference.contains("Full-topic sentinel"),
        "{}",
        context.reference
    );
    assert!(!context.reference.contains("Wrong-project sentinel"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_older_exact_topic_survives_newer_prefix_previews() {
    let mut records: Vec<_> = (0..5)
        .map(|index| {
            record(
                &format!("AsterOS{index} startup"),
                "Similar prefix implementation only.",
            )
        })
        .collect();
    let right = record(
        "AsterOS native startup published",
        "AsterOS native login and cold restart verified. Exact-topic sentinel.",
    );
    records.push(right);
    let (f, agent) =
        selection_fixture(records.clone(), records.clone(), vec![records[0].clone()]).await;
    let items = recall(&f, &agent, "Find AsterOS native login and cold restart", 1).await;
    assert_eq!(items.len(), 1);
    assert!(
        items[0].text.contains("Exact-topic sentinel"),
        "{}",
        items[0].text
    );
    assert!(
        f.factory
            .garden_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "mother_get_record")
            .count()
            <= 4
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_owner_context_keeps_precedence_and_deduplicates_ids() {
    let pinned = record("Owner core", "Owner-pinned sentinel.");
    let assigned = record("Owner assignment", "Owner-assigned sentinel.");
    let right = record(
        "AsterOS startup",
        "AsterOS native startup verified. Topic sentinel.",
    );
    let (f, agent) = selection_fixture(
        vec![pinned.clone(), assigned.clone(), right.clone()],
        vec![right.clone(), right.clone()],
        vec![],
    )
    .await;
    {
        let mut context = f.factory.context_result.lock().unwrap();
        let context = context.as_mut().unwrap();
        context["pinned"] = json!([pinned]);
        context["assignedToYou"] = json!([assigned, right]);
    }
    let items = recall(&f, &agent, "Check AsterOS native startup", 4).await;
    assert_eq!(items.len(), 3);
    for (item, sentinel) in items.iter().zip([
        "Owner-pinned sentinel",
        "Owner-assigned sentinel",
        "Topic sentinel",
    ]) {
        assert!(item.text.contains(sentinel), "{}", item.text);
    }
    assert_eq!(
        f.factory
            .garden_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "mother_get_record")
            .count(),
        3
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_complete_record_budget_is_not_a_truncation_budget() {
    let records: Vec<_> = (0..2)
        .map(|index| {
            record(
                "AsterOS native startup",
                &format!(
                    "AsterOS native startup {} Complete-record-{index} sentinel.",
                    "x".repeat(4_200)
                ),
            )
        })
        .collect();
    let (f, agent) = selection_fixture(records.clone(), records, vec![]).await;
    let items = recall(&f, &agent, "Check AsterOS native startup", 4).await;
    assert_eq!(items.len(), 1);
    assert!(items.iter().map(|item| item.text.len()).sum::<usize>() <= 8 * 1024);
    assert!(items[0].text.contains("Complete-record-0 sentinel."));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_preserves_attributed_contradictions_and_excludes_retractions(
) {
    let mut first = record("AsterOS native startup", "AsterOS startup uses port5151.");
    first["actor"] = json!("synthetic-first-harness");
    let mut second = record("AsterOS native startup", "AsterOS startup uses port6161.");
    second["actor"] = json!("synthetic-second-harness");
    let mut old = record(
        "AsterOS native startup",
        "AsterOS startup uses retired port7171.",
    );
    old["status"] = json!("retracted");
    let records = vec![old, first, second];
    let (f, agent) = selection_fixture(records.clone(), records, vec![]).await;
    let items = recall(&f, &agent, "Check AsterOS native startup", 4).await;
    assert_eq!(items.len(), 2);
    assert!(
        items[0].text.contains("port5151") && items[0].text.contains("synthetic-first-harness")
    );
    assert!(
        items[1].text.contains("port6161") && items[1].text.contains("synthetic-second-harness")
    );
    f.manager.shutdown().await;
}
