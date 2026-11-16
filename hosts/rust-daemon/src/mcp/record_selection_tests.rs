//! Model-authored searches retain complete-record and authority boundaries.
use super::*;
use serde_json::Value;
#[path = "multilingual_recall_tests.rs"]
mod multilingual_recall_tests;
#[path = "personal_recall_tests.rs"]
mod personal_recall_tests;

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

async fn planned_recall(
    f: &Fixture,
    agent: &anima_core::AgentState,
    query: &str,
    mode: &str,
    kind: Option<&str>,
    names: &[&str],
    limit: usize,
) -> crate::memory_provider::mother::SharedRecall {
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        agent,
        &run(f),
        CancelSignal::new(),
        String::new(),
    )
    .await;
    source
        .search_shared(
            &agent.id,
            crate::memory_provider::mother::RecallPlan::search(
                query,
                mode,
                kind,
                &names.iter().map(|n| (*n).into()).collect::<Vec<_>>(),
            )
            .unwrap(),
            limit,
            false,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn memory_provider_mother_selection_exact_topic_recovers_from_partial_semantic_index() {
    let wrong = record("Aster browser", "Wrong-project sentinel.");
    let right = record(
        "AsterOS startup",
        "Native login cold restart verified. Correct-project sentinel.",
    );
    let (f, agent) = selection_fixture(
        vec![wrong.clone(), right.clone()],
        vec![wrong.clone(), right.clone()],
        vec![wrong],
    )
    .await;
    let before = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    // The conversation model chooses an exact entity search after incomplete semantics.
    let partial = planned_recall(
        &f,
        &agent,
        "混合语言 startup",
        "semantic",
        None,
        &["AsterOS"],
        4,
    )
    .await;
    assert!(partial.items.is_empty());
    assert_eq!(
        partial.coverage["searches"][0]["search"]["reason"],
        "index_incomplete"
    );
    let found = planned_recall(&f, &agent, "AsterOS", "literal", None, &["AsterOS"], 1).await;
    assert!(found.items[0].text.contains("Correct-project sentinel"));
    assert!(found.items[0]
        .source_id
        .as_ref()
        .unwrap()
        .ends_with(&format!("/{}@2", right["id"].as_str().unwrap())));
    assert_eq!(
        serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_rechecks_topic_in_current_full_record() {
    let preview = record("AsterOS startup", "AsterOS evidence");
    let mut current = preview.clone();
    current["title"] = json!("OtherProject");
    current["content"] = json!("OtherProject replaced the attribution");
    current["revision"] = json!(3);
    let (f, agent) = selection_fixture(vec![current], vec![preview], vec![]).await;
    assert!(
        planned_recall(&f, &agent, "AsterOS", "literal", None, &["AsterOS"], 4)
            .await
            .items
            .is_empty()
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_literal_hit_after_excerpt_still_resolves_full_record() {
    let right = record(
        "Native startup",
        &format!(
            "{} AsterOS verified. Late-topic sentinel.",
            "Background. ".repeat(60)
        ),
    );
    let (f, agent) = selection_fixture(vec![right.clone()], vec![right], vec![]).await;
    let r = planned_recall(&f, &agent, "AsterOS", "literal", None, &["AsterOS"], 1).await;
    assert!(r.items[0].text.contains("Late-topic sentinel"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_complete_record_budget_is_not_a_truncation_budget() {
    let values: Vec<_> = (0..2)
        .map(|i| {
            record(
                "AsterOS",
                &format!("{} Complete-record-{i} sentinel.", "x".repeat(4200)),
            )
        })
        .collect();
    let (f, agent) = selection_fixture(values.clone(), values, vec![]).await;
    let r = planned_recall(&f, &agent, "AsterOS", "literal", None, &[], 4).await;
    assert_eq!(r.items.len(), 1);
    assert!(r.items[0].text.contains("Complete-record-0 sentinel."));
    assert!(r.items.iter().map(|i| i.text.len()).sum::<usize>() <= 8192);
    assert_eq!(r.coverage["excludedResolvedRecords"], 1);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_preserves_attributed_contradictions_and_excludes_retractions(
) {
    let mut a = record("AsterOS", "Port5151");
    a["actor"] = json!("first-harness");
    let mut b = record("AsterOS", "Port6161");
    b["actor"] = json!("second-harness");
    let mut old = record("AsterOS", "Port7171");
    old["status"] = json!("retracted");
    let (f, agent) = selection_fixture(
        vec![old.clone(), a.clone(), b.clone()],
        vec![old, a, b],
        vec![],
    )
    .await;
    let r = planned_recall(&f, &agent, "AsterOS", "literal", None, &[], 4).await;
    assert_eq!(r.items.len(), 2);
    assert!(r
        .items
        .iter()
        .any(|r| r.text.contains("first-harness") && r.text.contains("5151")));
    assert!(r
        .items
        .iter()
        .any(|r| r.text.contains("second-harness") && r.text.contains("6161")));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_large_preview_window_retries_smaller_complete_window() {
    let values: Vec<_> = (0..8)
        .map(|i| record("AsterOS", &format!("Budget-{i} {}", "x".repeat(600))))
        .collect();
    let (f, agent) = selection_fixture(values.clone(), values, vec![]).await;
    let persona = "p".repeat(11000);
    f.factory.context_result.lock().unwrap().as_mut().unwrap()["persona"] =
        json!({"content":persona,"hash":hex::encode(Sha256::digest(persona.as_bytes()))});
    let r = planned_recall(&f, &agent, "AsterOS", "literal", None, &[], 4).await;
    assert!(!r.items.is_empty());
    let calls = f.factory.garden_calls.lock().unwrap().clone();
    let searches: Vec<_> = calls
        .iter()
        .filter(|(n, _)| n == "mother_search_context")
        .collect();
    assert_eq!(searches.len(), 2);
    assert_eq!(searches[0].1["limit"], 8);
    assert_eq!(searches[1].1["limit"], 4);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_selection_pins_do_not_consume_explicit_search_budget() {
    let pin = record("Owner core", "Pinned sentinel");
    let fact = record("AsterOS", "Search sentinel");
    let (f, agent) = selection_fixture(vec![pin.clone(), fact.clone()], vec![fact], vec![]).await;
    f.factory.context_result.lock().unwrap().as_mut().unwrap()["pinned"] = json!([pin]);
    let r = planned_recall(&f, &agent, "AsterOS", "literal", None, &[], 1).await;
    assert_eq!(r.items.len(), 1);
    assert!(r.items[0].text.contains("Search sentinel"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn multilingual_recall_semantic_order_is_not_replaced_by_english_overlap() {
    let correct = record(
        "Household fact",
        "My cat is called Vega. 跨语言正确 sentinel.",
    );
    let wrong = record("叫什么名字", "叫什么名字 language lesson. Wrong sentinel.");
    let (f, agent) = selection_fixture(
        vec![correct.clone(), wrong.clone()],
        vec![],
        vec![correct, wrong],
    )
    .await;
    let r = planned_recall(&f, &agent, "叫什么名字", "semantic", None, &[], 1).await;
    assert!(r.items[0].text.contains("跨语言正确 sentinel"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn multilingual_recall_exact_name_filters_do_not_reorder_semantic_results() {
    let correct = record(
        "Deployment evidence",
        "AsterOS startup was verified. Provider-top sentinel",
    );
    let wrong = record(
        "AsterOS title",
        "Unrelated palette. Lower-provider sentinel",
    );
    let (f, agent) = selection_fixture(
        vec![correct.clone(), wrong.clone()],
        vec![],
        vec![correct, wrong],
    )
    .await;
    for mode in ["hybrid", "semantic"] {
        let result = planned_recall(&f, &agent, "启动验证", mode, None, &["AsterOS"], 1).await;
        assert!(result.items[0].text.contains("Provider-top sentinel"));
    }
    f.manager.shutdown().await;
}

#[tokio::test]
async fn multilingual_recall_coverage_counts_rejected_complete_records() {
    let preview = record("AsterOS", "AsterOS evidence");
    let mut wrong = preview.clone();
    wrong["title"] = json!("OtherProject");
    wrong["content"] = json!("OtherProject correction");
    let mut old = record("AsterOS", "Retracted evidence");
    old["status"] = json!("retracted");
    let (f, agent) = selection_fixture(vec![wrong, old.clone()], vec![preview, old], vec![]).await;
    let result = planned_recall(&f, &agent, "AsterOS", "literal", None, &["AsterOS"], 4).await;
    assert!(result.items.is_empty());
    assert_eq!(result.coverage["candidateRecords"], 2);
    assert_eq!(result.coverage["fetchedRecords"], 2);
    assert_eq!(result.coverage["unresolvedCandidates"], 0);
    assert_eq!(result.coverage["excludedResolvedRecords"], 2);
    f.manager.shutdown().await;
}
