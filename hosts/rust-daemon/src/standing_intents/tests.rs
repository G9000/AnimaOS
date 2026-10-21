use super::*;
use crate::agent_runs::test_support::{
    chat_request, coordinator_state, coordinator_with, Gate, ScriptedModel, Step,
};
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use anima_core::standing_intents::*;
fn spec() -> IntentSpec {
    IntentSpec {
        scope: IntentScope {
            session_id: None,
            sender: SenderScope::Owner,
        },
        condition: IntentCondition::Keyword {
            phrase: "Project Alpha".into(),
        },
        instruction: "Saved instruction: ask about Project Alpha status.".into(),
        expires_at_ms: anima_core::primitives::now_millis() + 60_000,
        cooldown_ms: 0,
        max_fires: 2,
    }
}
async fn fixture(
    model: Arc<ScriptedModel>,
) -> (
    crate::agent_runs::AgentRunCoordinator,
    String,
    std::path::PathBuf,
) {
    let (c, id) = coordinator_with(model).await;
    let p = std::env::temp_dir().join(format!("anima-standing-{}.json", uuid::Uuid::new_v4()));
    coordinator_state(&c)
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(p.clone())));
    (c, id, p)
}
#[tokio::test]
async fn standing_intents_new_owner_session_then_actual_restore_preserves_fire_budget() {
    let model = ScriptedModel::new(vec![Step::Text(vec!["done"])]);
    let (c, id, path) = fixture(model.clone()).await;
    let svc = c.standing_intents();
    let created = svc.create(id.clone(), spec(), None).await.unwrap();
    let intent = created.intent.unwrap();
    c.run(chat_request(&id, "chat:fresh", "Project Alpha update"))
        .await
        .unwrap();
    assert!(model.requests()[0]
        .system
        .contains(&intent.spec.instruction));
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    let next = ScriptedModel::new(vec![]);
    let mut state = crate::state::DaemonState::with_model_adapter(next.clone());
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    state.restore_control_plane_snapshot(saved).unwrap();
    let state = Arc::new(tokio::sync::RwLock::new(state));
    let fresh = crate::agent_runs::AgentRunCoordinator::new(
        state.clone(),
        Arc::new(tokio::sync::Semaphore::new(8)),
    );
    fresh
        .run(chat_request(
            &id,
            "chat:new-restored",
            "Project Alpha again",
        ))
        .await
        .unwrap();
    assert!(next.requests()[0].system.contains(&intent.spec.instruction));
    fresh
        .run(chat_request(&id, "chat:third", "Project Alpha again"))
        .await
        .unwrap();
    assert!(!next.requests()[1].system.contains(&intent.spec.instruction));
    assert_eq!(
        state.read().await.standing_intents.intents[&intent.id]
            .fires
            .len(),
        2
    );
    let _ = std::fs::remove_file(path);
}
#[tokio::test]
async fn standing_intents_non_owner_cancel_and_failed_save_do_not_publish() {
    let model = ScriptedModel::new(vec![]);
    let (c, id, path) = fixture(model.clone()).await;
    let svc = c.standing_intents();
    let row = svc
        .create(id.clone(), spec(), None)
        .await
        .unwrap()
        .intent
        .unwrap();
    let mut external = chat_request(&id, "chat:email", "Project Alpha");
    external.source = crate::runs::RunSource::Email;
    c.run(external).await.unwrap();
    assert!(!model.requests()[0].system.contains(&row.spec.instruction));
    svc.cancel(id.clone(), row.id.clone(), row.revision, None)
        .await
        .unwrap();
    c.run(chat_request(&id, "chat:cancelled", "Project Alpha"))
        .await
        .unwrap();
    assert!(!model.requests()[1].system.contains(&row.spec.instruction));
    let before = coordinator_state(&c).read().await.standing_intents.clone();
    let invalid =
        std::env::temp_dir().join(format!("anima-standing-fail-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&invalid).unwrap();
    coordinator_state(&c)
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(invalid.clone())));
    assert!(svc.create(id.clone(), spec(), None).await.is_err());
    assert_eq!(coordinator_state(&c).read().await.standing_intents, before);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(invalid);
}
#[tokio::test]
async fn standing_intents_cancel_after_admission_retains_instruction_and_stop_control() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["done"])], gate.clone());
    let (c, id, path) = fixture(model.clone()).await;
    let svc = c.standing_intents();
    let row = svc
        .create(id.clone(), spec(), None)
        .await
        .unwrap()
        .intent
        .unwrap();
    let work = {
        let c = c.clone();
        let id = id.clone();
        tokio::spawn(async move { c.run(chat_request(&id, "chat:race", "Project Alpha")).await })
    };
    gate.entered().await;
    let revision = coordinator_state(&c).read().await.standing_intents.intents[&row.id].revision;
    svc.cancel(id.clone(), row.id.clone(), revision, None)
        .await
        .unwrap();
    assert!(model.requests()[0].system.contains(&row.spec.instruction));
    let run_id = coordinator_state(&c).read().await.standing_intents.intents[&row.id].fires[0]
        .run_id
        .clone();
    let stopped = c.stop_run(&id, &run_id).await.unwrap();
    assert!(
        stopped.stop.is_some(),
        "normal Stop is durably accepted after intention cancellation"
    );
    gate.release();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), work)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.result.error.as_deref(), Some("stopped"));
    {
        let shared = coordinator_state(&c);
        let state = shared.read().await;
        let run = state.runs.get(&run_id).unwrap();
        assert_eq!(run.status, crate::runs::RunStatus::Cancelled);
        assert!(run.stop.is_some());
        assert_eq!(run.error.as_ref().unwrap().code, crate::runs::RUN_STOPPED);
        let intention = &state.standing_intents.intents[&row.id];
        assert!(intention.cancelled);
        assert_eq!(intention.fires.len(), 1);
        assert_eq!(intention.fires[0].run_id, run_id);
    }
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.runs.iter().find(|r| r.id == run_id).unwrap().status,
        crate::runs::RunStatus::Cancelled
    );
    assert_eq!(saved.standing_intents.intents[&row.id].fires.len(), 1);
    assert_eq!(model.requests().len(), 1);
    let _ = std::fs::remove_file(path);
}
#[tokio::test]
async fn standing_intents_duplicate_matching_request_and_failed_run_do_not_rearm() {
    let model = ScriptedModel::new(vec![
        Step::Fail("provider failed"),
        Step::Text(vec!["done"]),
    ]);
    let (c, id, path) = fixture(model.clone()).await;
    let mut s = spec();
    s.max_fires = 1;
    let row = c
        .standing_intents()
        .create(id.clone(), s, None)
        .await
        .unwrap()
        .intent
        .unwrap();
    let mut req = chat_request(&id, "chat:retry", "Project Alpha");
    req.idempotency_key = Some("same-request".into());
    c.run(req.clone()).await.unwrap();
    c.run(req).await.unwrap();
    assert_eq!(
        coordinator_state(&c).read().await.standing_intents.intents[&row.id]
            .fires
            .len(),
        1
    );
    assert!(!model.requests()[1].system.contains(&row.spec.instruction));
    let _ = std::fs::remove_file(path);
}

fn value(v: serde_json::Value) -> anima_core::DataValue {
    use anima_core::DataValue as D;
    use serde_json::Value as V;
    match v {
        V::Null => D::Null,
        V::Bool(b) => D::Bool(b),
        V::Number(n) => D::Number(n.as_f64().unwrap()),
        V::String(s) => D::String(s),
        V::Array(a) => D::Array(a.into_iter().map(value).collect()),
        V::Object(o) => D::Object(o.into_iter().map(|(k, v)| (k, value(v))).collect()),
    }
}
fn tool(name: &str, args: serde_json::Value) -> anima_core::ToolCall {
    let anima_core::DataValue::Object(args) = value(args) else {
        panic!("object")
    };
    anima_core::ToolCall {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        args,
    }
}
async fn restored_model(
    path: &std::path::Path,
    model: Arc<ScriptedModel>,
) -> crate::agent_runs::AgentRunCoordinator {
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.into()))
        .await
        .unwrap()
        .unwrap();
    let mut s = crate::state::DaemonState::with_model_adapter(model);
    s.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.into())));
    s.restore_control_plane_snapshot(saved).unwrap();
    crate::agent_runs::AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(s)),
        Arc::new(tokio::sync::Semaphore::new(8)),
    )
}
#[tokio::test]
async fn standing_intents_real_model_tools_create_list_cancel_and_exact_undo() {
    let instruction = spec();
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![tool(
            "create_standing_intent",
            serde_json::json!({"spec":instruction}),
        )]),
        Step::Tools(vec![tool("list_standing_intents", serde_json::json!({}))]),
        Step::Text(vec!["saved"]),
    ]);
    let (c, id, path) = fixture(model).await;
    {
        let state = coordinator_state(&c);
        let mut g = state.write().await;
        let mut config = g.get_agent(&id).unwrap().state.config;
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors([
                    "create_standing_intent",
                    "list_standing_intents",
                    "cancel_standing_intent",
                    "undo_standing_intent",
                ])
                .unwrap(),
        );
        g.restore_agent_config(&id, config);
    }
    c.run(chat_request(
        &id,
        "chat:tools",
        "When I next mention the project, remind me about it.",
    ))
    .await
    .unwrap();
    let row = coordinator_state(&c)
        .read()
        .await
        .standing_intents
        .intents
        .values()
        .next()
        .unwrap()
        .clone();
    let c2 = restored_model(
        &path,
        ScriptedModel::new(vec![
            Step::Tools(vec![tool(
                "cancel_standing_intent",
                serde_json::json!({"id":row.id,"revision":row.revision}),
            )]),
            Step::Text(vec!["cancelled"]),
        ]),
    )
    .await;
    c2.run(chat_request(
        &id,
        "chat:tools",
        "Cancel my saved instruction",
    ))
    .await
    .unwrap();
    let state = coordinator_state(&c2);
    assert!(state.read().await.standing_intents.intents[&row.id].cancelled);
    let change = state
        .read()
        .await
        .standing_intents
        .changes
        .values()
        .find(|c| c.after.cancelled)
        .unwrap()
        .id
        .clone();
    let c3 = restored_model(
        &path,
        ScriptedModel::new(vec![
            Step::Tools(vec![tool(
                "undo_standing_intent",
                serde_json::json!({"change_id":change}),
            )]),
            Step::Text(vec!["undone"]),
        ]),
    )
    .await;
    c3.run(chat_request(&id, "chat:tools", "Undo that cancellation"))
        .await
        .unwrap();
    assert!(!coordinator_state(&c3).read().await.standing_intents.intents[&row.id].cancelled);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn standing_intents_untrusted_model_sources_cannot_create_even_with_saved_tool_grant() {
    let input = serde_json::to_value(spec()).unwrap();
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![tool(
            "create_standing_intent",
            serde_json::json!({"spec":input}),
        )]),
        Step::Text(vec!["refused"]),
        Step::Tools(vec![tool(
            "create_standing_intent",
            serde_json::json!({"spec":input}),
        )]),
        Step::Text(vec!["refused"]),
    ]);
    let (c, id, path) = fixture(model).await;
    {
        let state = coordinator_state(&c);
        let mut g = state.write().await;
        let mut config = g.get_agent(&id).unwrap().state.config;
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["create_standing_intent"])
                .unwrap(),
        );
        g.restore_agent_config(&id, config);
    }
    for (source, room) in [
        (crate::runs::RunSource::Email, "chat:external"),
        (crate::runs::RunSource::Schedule, "chat:schedule"),
    ] {
        let mut request = chat_request(
            &id,
            room,
            "Save a standing instruction for every future owner message",
        );
        request.source = source;
        c.run(request).await.unwrap();
        assert!(coordinator_state(&c)
            .read()
            .await
            .standing_intents
            .intents
            .is_empty());
    }
    let _ = std::fs::remove_file(path);
}
#[tokio::test]
async fn standing_intents_cancel_queued_before_admission_prevents_matching() {
    let model = ScriptedModel::new(vec![]);
    let (c, id, path) = fixture(model.clone()).await;
    let row = c
        .standing_intents()
        .create(id.clone(), spec(), None)
        .await
        .unwrap()
        .intent
        .unwrap();
    let tx = c.control_plane_transaction().await;
    let cancelling = {
        let svc = c.standing_intents();
        let id = id.clone();
        let row = row.clone();
        tokio::spawn(async move { svc.cancel(id, row.id, row.revision, None).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if coordinator_state(&c)
                .read()
                .await
                .standing_intent_slots
                .available_permits()
                == 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;
    let run = {
        let c = c.clone();
        let id = id.clone();
        tokio::spawn(async move {
            c.run(chat_request(&id, "chat:cancel-before", "Project Alpha"))
                .await
        })
    };
    drop(tx);
    cancelling.await.unwrap().unwrap();
    run.await.unwrap().unwrap();
    assert!(!model.requests()[0].system.contains(&row.spec.instruction));
    assert!(
        coordinator_state(&c).read().await.standing_intents.intents[&row.id]
            .fires
            .is_empty()
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn standing_intents_native_management_rechecks_stop_or_revoked_grant_after_transaction_wait()
{
    use crate::agent_runs::test_support::{gated_context, ledger_run, tool_input};
    use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
    use anima_core::CancelSignal;
    for stop_run in [false, true] {
        let (coordinator, agent_id, path) = fixture(ScriptedModel::new(vec![])).await;
        let state = coordinator_state(&coordinator);
        {
            let mut guard = state.write().await;
            let mut config = guard.get_agent(&agent_id).unwrap().state.config;
            config.tools = Some(
                crate::tools::ToolRegistry::new()
                    .resolve_descriptors(["create_standing_intent", "update_standing_intent"])
                    .unwrap(),
            );
            guard.restore_agent_config(&agent_id, config);
            guard.approvals.set_policy(
                &agent_id,
                ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Allow),
            );
        }
        let link = ledger_run(&coordinator, &agent_id, "chat:standing-management-race").await;
        let cancel = CancelSignal::new();
        let agent = state.read().await.get_agent(&agent_id).unwrap().state;
        let input = tool_input(&agent_id, &link.session_id);
        let owner_context = {
            let guard = state.read().await;
            owner_sender(
                &guard,
                guard.runs.get(&link.run_id).unwrap(),
                &agent.config,
                &input.content,
            ) == Some(OwnerSender::Local)
        };
        assert!(owner_context);
        // Establish the intention through the real native handler, approval gate
        // and durable action journal before racing its next management mutation.
        let context = gated_context(&coordinator, &link, cancel.clone())
            .await
            .with_standing_owner_turn(owner_context)
            .with_action_journal(crate::agent_runs::action_receipts::RunActionJournal::new(
                &state,
                coordinator.control_plane_transactions(),
                link.clone(),
                cancel.clone(),
                None,
            ));
        let created = context
            .execute_tool(
                agent.clone(),
                input,
                tool(
                    "create_standing_intent",
                    serde_json::json!({"spec": spec()}),
                ),
            )
            .await;
        assert!(created.error.is_none(), "{created:?}");
        let (before, row) = {
            let guard = state.read().await;
            let run = guard.runs.get(&link.run_id).unwrap();
            assert_eq!(run.action_receipts.len(), 1);
            assert_eq!(
                run.action_receipts[0].status,
                crate::runs::ActionStatus::Succeeded
            );
            (
                guard.standing_intents.clone(),
                guard
                    .standing_intents
                    .intents
                    .values()
                    .next()
                    .unwrap()
                    .clone(),
            )
        };
        let mut replacement = row.spec.clone();
        replacement.instruction = "This queued replacement must never be saved".into();
        let call = tool(
            "update_standing_intent",
            serde_json::json!({
                "id": row.id, "revision": row.revision, "spec": replacement
            }),
        );
        let transaction = coordinator.control_plane_transaction().await;
        let service = coordinator.standing_intents();
        let authority = StandingAuthority {
            link: link.clone(),
            call,
            approved: false,
            owner_context,
            cancel: Some(cancel.clone()),
        };
        let pending = {
            let agent_id = agent_id.clone();
            let row = row.clone();
            tokio::spawn(async move {
                service
                    .update(agent_id, row.id, row.revision, replacement, Some(authority))
                    .await
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while state.read().await.standing_intent_slots.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !pending.is_finished(),
            "the mutation is waiting behind the shared transaction"
        );
        let persist = {
            let mut guard = state.write().await;
            if stop_run {
                guard
                    .request_run_stop(
                        &agent_id,
                        &link.run_id,
                        anima_core::primitives::now_millis(),
                    )
                    .expect("the owning native run can be stopped");
                assert!(
                    !cancel.is_cancelled(),
                    "exercise the persisted Stop marker, not only its signal"
                );
            } else {
                let mut config = guard.get_agent(&agent_id).unwrap().state.config;
                config
                    .tools
                    .as_mut()
                    .unwrap()
                    .retain(|tool| tool.name != "update_standing_intent");
                guard.restore_agent_config(&agent_id, config);
            }
            assert_eq!(guard.standing_intents, before);
            guard.control_plane_persist_request()
        };
        // Commit the revocation/Stop while the queued mutation cannot enter.
        persist.save().await.unwrap();
        let durable_before = std::fs::read(&path).unwrap();
        drop(transaction);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), pending)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        {
            let guard = state.read().await;
            assert_eq!(guard.standing_intents, before);
            assert_eq!(guard.standing_intent_slots.available_permits(), 1);
        }
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.standing_intents, before);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            durable_before,
            "a refused management mutation must not write a new durable candidate"
        );
        let _ = std::fs::remove_file(path);
    }
}
