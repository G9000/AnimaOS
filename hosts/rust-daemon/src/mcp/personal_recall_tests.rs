//! Personal garden facts must not be replaced by old assistant task-result claims.
use super::*;

#[tokio::test]
async fn multilingual_recall_bootstrap_never_guesses_topics_or_searches_private_records() {
    let pet = memory(
        "Household pets",
        "My cat is called Vega. Private fact sentinel.",
    );
    let (f, agent) = selection_fixture(vec![pet.clone()], vec![pet], vec![]).await;
    for query in [
        "What is my cat's name?",
        "我的猫叫什么？ Please cite its record ID.",
        "猫の名前覚えてる？",
        "Ingat nama kucing saya?",
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
        assert!(
            !context.reference.contains("Private fact sentinel"),
            "bootstrap guessed topic for {query}"
        );
    }
    assert!(f
        .factory
        .garden_calls
        .lock()
        .unwrap()
        .iter()
        .all(|(n, _)| n == "mother_get_context"));
    f.manager.shutdown().await;
}

fn memory(title: &str, content: &str) -> Value {
    let mut value = record(title, content);
    value["kind"] = json!("memory");
    value["basis"] = json!("owner_statement");
    value
}

#[cfg(windows)]
#[tokio::test]
async fn memory_provider_native_manager_oversized_call_retires_without_replay() {
    let mut f = Fixture::new().await;
    f.manager.shutdown().await;
    let factory = Arc::new(super::super::super::transport::DefaultMcpTransportFactory::default());
    f.manager = McpManager::with_credentials(
        Arc::downgrade(&f.state),
        Arc::new(Mutex::new(())),
        f.state.read().await.mcp_publication.clone(),
        factory.clone(),
        f.credentials.clone(),
    );
    f.state.write().await.mcp_manager = Some(f.manager.clone());
    let fixture = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/mcp_stdio_peer.exe");
    let marker = f.folder.join("native-size.json");
    let row = f
        .manager
        .create_stdio(
            &f.agent,
            "Synthetic size boundary".into(),
            McpStdioSpec {
                executable: fixture.to_string_lossy().into_owned(),
                args: vec![
                    "--mode".into(),
                    "oversized-call".into(),
                    "--marker".into(),
                    marker.to_string_lossy().into_owned(),
                ],
            },
        )
        .await
        .unwrap();
    let row = f
        .manager
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
    let row = f
        .manager
        .connect(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    let row = f
        .manager
        .review(
            &f.agent,
            &row.record.id,
            ReviewMcpTool {
                expected_version: row.record.version,
                discovery_id: row.discovery_id.clone().unwrap(),
                descriptor_digest: row.candidates[0].descriptor_digest.clone(),
            },
        )
        .await
        .unwrap();
    let stamp = f.grant_and_stamp(&row).await;
    let error = f
        .manager
        .invoke(
            stamp.clone(),
            json!({"value":"violet"}),
            CancelSignal::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, McpServiceError::OutcomeUnknownSize);
    let view = f.manager.get(&f.agent, &row.record.id).await.unwrap();
    let diagnostic = view.error.unwrap();
    assert_eq!(diagnostic.code, "outcome_unknown");
    assert!(diagnostic.message.contains("size limit"));
    for private in [
        "synthetic-private-sentinel",
        marker.to_str().unwrap(),
        fixture.to_str().unwrap(),
        "oversized-call",
    ] {
        assert!(!diagnostic.message.contains(private));
    }
    assert_eq!(
        f.manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap_err(),
        McpServiceError::Revoked
    );
    f.manager.shutdown().await;
    assert_eq!(factory.stdio_child_count(), 0);
    let peer: Value = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    assert_eq!(peer["counts"]["tools/call"], 1);
}

#[tokio::test]
async fn multilingual_recall_explicit_translated_queries_retrieve_full_personal_facts() {
    let pet = memory(
        "Household pets",
        "My cat is called Vega. Full-personal sentinel.",
    );
    let wrong = memory("Education application", "Unrelated substring sentinel");
    let (f, agent) = selection_fixture(
        vec![wrong.clone(), pet.clone()],
        vec![wrong, pet.clone()],
        vec![pet.clone()],
    )
    .await;
    let before = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    // These model-output fixtures prove routing, not language understanding.
    for (question, query, mode) in [
        ("我的猫叫什么？ cite ID", "Household pets", "literal"),
        ("猫の名前覚えてる？ please recall", "猫の名前", "semantic"),
        ("Nama kucing saya apa?", "cat name", "hybrid"),
    ] {
        assert!(!question.contains("Vega"));
        let r = planned_recall(&f, &agent, query, mode, Some("memory"), &[], 4).await;
        assert!(r
            .items
            .iter()
            .any(|i| i.text.contains("Full-personal sentinel")));
        assert!(r
            .items
            .iter()
            .all(|i| !i.text.contains("Unrelated substring")));
        assert!(r
            .items
            .iter()
            .all(|i| i.source_id.as_ref().unwrap().starts_with("mother:")));
    }
    assert_eq!(
        serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn multilingual_recall_bootstrap_keeps_only_explicit_pins_and_native_guidance() {
    let pin = memory("Owner chosen core", "Owner-pinned sentinel");
    let private = memory(
        "Household pets",
        "My cat Vega. Unrequested-private sentinel",
    );
    let (f, mut agent) =
        selection_fixture(vec![pin.clone(), private.clone()], vec![private], vec![]).await;
    f.factory.context_result.lock().unwrap().as_mut().unwrap()["pinned"] = json!([pin]);
    agent.config.tools.as_mut().unwrap().extend(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["memory_search"])
            .unwrap(),
    );
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
        "我的猫叫什么？".into(),
    )
    .await;
    let context = source.current(&agent).await;
    assert!(context.reference.contains("Owner-pinned sentinel"));
    assert!(!context.reference.contains("Unrequested-private sentinel"));
    assert!(context.identity.is_none() && !context.withheld_tools.contains("memory_search"));
    assert!(
        context.reference.contains("ADDITIVE memory provider")
            && context.reference.contains("task_result")
    );
    assert!(context.reference.contains("source mother"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn memory_provider_mother_size_cause_survives_cancellation_winning_delivery() {
    let f = Fixture::new().await;
    let view = f.reviewed().await;
    let stamp = f.grant_and_stamp(&view).await;
    let session = f.factory.sessions.lock().unwrap()[0].clone();
    session.wire_limit_calls.store(true, Ordering::SeqCst);
    session.hold_after_wire_limit.store(true, Ordering::SeqCst);
    let cancel = CancelSignal::new();
    let manager = f.manager.clone();
    let call_stamp = stamp.clone();
    let call_cancel = cancel.clone();
    let task = tokio::spawn(async move {
        manager
            .invoke(call_stamp, json!({"value":"violet"}), call_cancel)
            .await
    });
    session.entered.acquire().await.unwrap().forget();
    // call_once latched the cause synchronously; its future never delivers an error.
    cancel.cancel();
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error, McpServiceError::OutcomeUnknownSize);
    assert_eq!(
        f.manager
            .get(&f.agent, &view.record.id)
            .await
            .unwrap()
            .error
            .unwrap()
            .code,
        "outcome_unknown"
    );
    assert_eq!(
        f.manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap_err(),
        McpServiceError::Revoked
    );
    f.manager.shutdown().await;
    assert!(session.closed.load(Ordering::SeqCst));
    assert_eq!(session.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn memory_provider_mother_automatic_size_failure_explains_native_fallback() {
    let pet = memory("Household pets", "My cat is called Vega.");
    let (f, agent) = selection_fixture(vec![pet.clone()], vec![pet], vec![]).await;
    let session = f.factory.sessions.lock().unwrap()[0].clone();
    session.wire_limit_calls.store(true, Ordering::SeqCst);
    let source = crate::workspace_context::WorkspaceRunContext::load_for_query(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
        "What is my cat's name?".into(),
    )
    .await;
    let context = source.current(&agent).await;
    assert!(context.reference.contains("Shared garden unavailable"));
    assert!(context.reference.contains("size limit"));
    assert!(context.reference.contains("native memory tools"));
    assert!(!context.withheld_tools.contains("memory_search"));
    f.manager.shutdown().await;
    assert_eq!(session.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn memory_provider_mother_wire_size_failure_preserves_cause_and_uncertain_write_safety() {
    let f = Fixture::new().await;
    let view = f.reviewed().await;
    let stamp = f.grant_and_stamp(&view).await;
    let session = f.factory.sessions.lock().unwrap()[0].clone();
    session.wire_limit_calls.store(true, Ordering::SeqCst);
    let error = f
        .manager
        .invoke(
            stamp.clone(),
            json!({"value":"violet"}),
            CancelSignal::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "outcome_unknown");
    assert!(
        error.to_string().contains("size"),
        "wire-size cause was hidden: {error}"
    );
    assert!(error
        .to_string()
        .contains("remote effects may have occurred"));
    let failed = f.manager.get(&f.agent, &view.record.id).await.unwrap();
    let diagnostic = failed.error.as_ref().unwrap();
    assert_eq!(diagnostic.code, "outcome_unknown");
    assert!(diagnostic.message.contains("size"));
    assert!(f.manager.ready_descriptors(&f.agent).is_empty());
    assert_eq!(
        f.manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap_err(),
        McpServiceError::Revoked
    );
    f.manager.shutdown().await;
    assert_eq!(
        session.calls.load(Ordering::SeqCst),
        1,
        "never auto-replay an uncertain action"
    );
}
