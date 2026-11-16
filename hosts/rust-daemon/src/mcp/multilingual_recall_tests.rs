//! Scripted intelligence verifies orchestration; cached E5 separately proves learned retrieval.
use super::*;
use anima_memory::{MemoryScope, MemoryType, NewMemory};

async fn seed_native(f: &Fixture, content: &str) -> anima_memory::Memory {
    f.state
        .read()
        .await
        .memory
        .write()
        .await
        .add(NewMemory {
            agent_id: f.agent.clone(),
            agent_name: "Synthetic owner".into(),
            memory_type: MemoryType::Fact,
            content: content.into(),
            importance: 0.8,
            tags: None,
            scope: Some(MemoryScope::Private),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap()
}

#[tokio::test]
async fn multilingual_recall_foreign_agent_cannot_reuse_captured_mother_context() {
    let fact = record("Household pets", "Bound-agent private sentinel");
    let (f, agent) = selection_fixture(vec![fact.clone()], vec![fact], vec![]).await;
    let source = crate::workspace_context::WorkspaceRunContext::load(
        f.state.clone(),
        Some(f.manager.clone()),
        &agent,
        &run(&f),
        CancelSignal::new(),
    )
    .await;
    let registry = crate::tools::ToolRegistry::new();
    let mut foreign = agent.clone();
    foreign.id = "foreign-synthetic-agent".into();
    foreign.config.tools = Some(registry.resolve_descriptors(["memory_search"]).unwrap());
    let state = f.state.read().await;
    let context = crate::tools::ToolExecutionContext::new(
        state.memory.clone(),
        state.memory_embeddings.clone(),
        None,
        registry,
        state.process_manager.clone(),
        None,
        None,
    )
    .with_workspace_context(Some(source));
    drop(state);
    let before = f.factory.garden_calls.lock().unwrap().len();
    let result = context
        .execute_tool(
            foreign.clone(),
            anima_core::Message {
                id: "foreign-request".into(),
                agent_id: foreign.id.clone(),
                room_id: "chat:foreign".into(),
                role: anima_core::MessageRole::User,
                content: Default::default(),
                created_at_ms: 1,
            },
            anima_core::ToolCall {
                id: "foreign-shared".into(),
                name: "memory_search".into(),
                args: std::collections::BTreeMap::from([
                    ("query".into(), anima_core::DataValue::String("pets".into())),
                    (
                        "source".into(),
                        anima_core::DataValue::String("mother".into()),
                    ),
                ]),
            },
        )
        .await;
    assert_eq!(result.status, anima_core::TaskStatus::Error);
    assert_eq!(
        f.factory.garden_calls.lock().unwrap().len(),
        before,
        "foreign agent must cause no Mother I/O"
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn multilingual_recall_conversation_model_selects_queries_with_native_memory_active() {
    use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};
    use anima_core::{DataValue, ToolCall};
    use std::collections::BTreeMap;
    for (question, query) in [
        (
            "我的猫叫什么？ Please check Mother 和自己的记忆。",
            "Household pets",
        ),
        (
            "猫の名前覚えてる？ check shared and local memories.",
            "pets",
        ),
        (
            "Nama kucing saya apa? semak Mother dan native memory.",
            "cat",
        ),
    ] {
        let pet = record(
            "Household pets",
            "My cat is called Vega. Complete-garden sentinel.",
        );
        let (f, mut agent) =
            selection_fixture(vec![pet.clone()], vec![pet.clone()], vec![pet]).await;
        agent.config.tools.as_mut().unwrap().extend(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["memory_search"])
                .unwrap(),
        );
        f.state
            .write()
            .await
            .restore_agent_config(&f.agent, agent.config.clone());
        f.state.write().await.approvals.set_policy(
            &f.agent,
            crate::approvals::ApprovalPolicy {
                exec: crate::approvals::PolicyAction::Allow,
                ..Default::default()
            },
        );
        let native = seed_native(&f, "Native project work. Independent-native sentinel.").await;
        let before_config =
            serde_json::to_value(f.state.read().await.agents[&f.agent].config()).unwrap();
        let before_remote = f.factory.garden_records.lock().unwrap().clone();
        let shared = ToolCall {
            id: "model-shared".into(),
            name: "memory_search".into(),
            args: BTreeMap::from([
                ("query".into(), DataValue::String(query.into())),
                ("source".into(), DataValue::String("mother".into())),
                ("mode".into(), DataValue::String("literal".into())),
                ("include_graph".into(), DataValue::Bool(false)),
            ]),
        };
        let local = ToolCall {
            id: "model-native".into(),
            name: "memory_search".into(),
            args: BTreeMap::from([(
                "query".into(),
                DataValue::String("Independent-native".into()),
            )]),
        };
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![shared]),
            Step::Tools(vec![local]),
            Step::Text(vec!["Synthetic verified response"]),
        ]);
        f.state.write().await.model_adapter = model.clone();
        let runs = crate::agent_runs::AgentRunCoordinator::new(
            f.state.clone(),
            Arc::new(Semaphore::new(4)),
        )
        .with_mcp(f.manager.clone());
        runs.run(chat_request(&f.agent, "chat:mixed-language", question))
            .await
            .unwrap();
        let requests = model.requests();
        assert_eq!(requests.len(), 3);
        assert!(
            !requests[0].system.contains("Complete-garden sentinel"),
            "bootstrap must not supply the answer"
        );
        assert!(requests[1].messages.iter().any(|m| m
            .content
            .text
            .contains("Complete-garden sentinel")
            && m.content.text.contains("mother:")));
        assert!(requests[2]
            .messages
            .iter()
            .any(|m| m.content.text.contains("Independent-native sentinel")));
        assert_eq!(
            serde_json::to_value(f.state.read().await.agents[&f.agent].config()).unwrap(),
            before_config
        );
        assert_eq!(*f.factory.garden_records.lock().unwrap(), before_remote);
        assert_eq!(
            f.state
                .read()
                .await
                .memory
                .read()
                .await
                .get(&native.id)
                .unwrap(),
            native
        );
        assert!(f
            .factory
            .garden_calls
            .lock()
            .unwrap()
            .iter()
            .all(|(n, _)| matches!(
                n.as_str(),
                "mother_get_context" | "mother_search_context" | "mother_get_record"
            )));
        assert!(f
            .factory
            .garden_calls
            .lock()
            .unwrap()
            .iter()
            .any(|(n, a)| n == "mother_search_context" && a["query"] == query));
        f.manager.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "Requires ANIMA_TEST_E5_SNAPSHOT containing cached assets; fully offline synthetic acceptance"]
async fn multilingual_recall_cached_e5_ranks_cross_language_facts_and_resolves_complete_records() {
    use anima_memory::MemoryVectorIndex;
    const QUERIES: [&str; 4] = [
        "我的猫叫什么名字？ Please recall.",
        "うちの猫の名前、覚えてる？",
        "Nama kucing saya apa? Please remember.",
        "What is my cat's name?",
    ];
    const FACT: &str = "My pet cat is named Vega.";
    const OTHER: [&str; 5] = [
        "My pet dog is named Rex.",
        "The next language lesson covers Japanese grammar.",
        "Application education deployment was completed.",
        "My favourite hobby is mountain walking.",
        "The bicycle has a red frame.",
    ];
    let path = PathBuf::from(
        std::env::var_os("ANIMA_TEST_E5_SNAPSHOT").expect("explicit existing local E5 snapshot"),
    );
    let mut learned = crate::memory_embeddings::cached_multilingual(&path);
    let f = Fixture::new().await;
    let correct = seed_native(&f, FACT).await;
    learned.upsert_memory(&correct).unwrap();
    for text in OTHER {
        learned.upsert_memory(&seed_native(&f, text).await).unwrap();
    }
    for query in QUERIES {
        let hits = learned.search(query, 6);
        assert_eq!(
            hits.first().map(|h| h.memory_id.as_str()),
            Some(correct.id.as_str()),
            "actual E5 top-1 for {query}"
        );
    }
    let before = f.state.read().await.memory.read().await.snapshot();
    *f.state.read().await.memory_embeddings.write().await = learned;
    // Exercise the production native search/filter path with the genuine vectors.
    let registry = crate::tools::ToolRegistry::new();
    let mut agent = f.state.read().await.get_agent(&f.agent).unwrap().state;
    agent.config.tools = Some(registry.resolve_descriptors(["memory_search"]).unwrap());
    let state = f.state.read().await;
    let context = crate::tools::ToolExecutionContext::new(
        state.memory.clone(),
        state.memory_embeddings.clone(),
        None,
        registry,
        state.process_manager.clone(),
        None,
        None,
    );
    drop(state);
    for query in QUERIES {
        let message = anima_core::Message {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: f.agent.clone(),
            room_id: "chat:offline-e5".into(),
            content: anima_core::Content {
                text: query.into(),
                ..Default::default()
            },
            role: anima_core::MessageRole::User,
            created_at_ms: 1,
        };
        let call = anima_core::ToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            name: "memory_search".into(),
            args: std::collections::BTreeMap::from([
                ("query".into(), anima_core::DataValue::String(query.into())),
                ("limit".into(), anima_core::DataValue::Number(1.0)),
            ]),
        };
        let result = context
            .clone()
            .execute_tool(agent.clone(), message, call)
            .await;
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Success,
            "{:?}",
            result.error
        );
        assert!(
            result.data.unwrap().text.contains(FACT),
            "complete learned recall for {query}"
        );
    }
    assert_eq!(f.state.read().await.memory.read().await.snapshot(), before);
    f.manager.shutdown().await;
}
