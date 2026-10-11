#[tokio::test]
async fn accepted_owner_attachment_uses_ledger_refs_and_keeps_text_delivery() {
    use crate::agent_runs::test_support::{events_until, Gate, ScriptedModel, Step};
    use crate::agent_runs::{
        AcceptRun, AcceptedRun, QueuedRunStart, QueuedStartError, SessionRunMode,
    };
    use crate::attachments::{files::save_upload, AttachmentKind, AttachmentRecord};
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["textual reply"])], gate.clone());
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(
        state.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent.clone(),
            TelegramBotToken::parse("42:attachments").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    manager.approve_pending(connector.id.clone()).await.unwrap();
    let temp = std::env::temp_dir().join(format!(
        "anima-telegram-attachment-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp).unwrap();
    state.write().await.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let saved = save_upload(&temp, "2026-09-23", "owner.txt", b"owner data").unwrap();
    let session = crate::sessions::session_id_for_room(&connector.room_id);
    state
        .write()
        .await
        .sessions
        .insert(crate::sessions::SessionRecord::new(
            &agent,
            &session,
            crate::sessions::SessionKind::Telegram,
            crate::sessions::SessionOrigin::Telegram,
            "Telegram".into(),
            crate::sessions::TitleSource::Owner,
            1,
        ));
    let row = AttachmentRecord {
        id: "att-telegram".into(),
        agent_id: agent.clone(),
        session_id: session.clone(),
        name: "owner.txt".into(),
        mime: "text/plain".into(),
        kind: AttachmentKind::Text,
        size: 10,
        path: saved.path,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
        created_at_ms: 1,
    };
    let store = state.read().await.history.store();
    store.upsert_attachments(&[row.clone()]).await.unwrap();
    let mut stream = state.read().await.live.subscribe(&agent).unwrap();
    let sending = manager.clone();
    let sending_agent = agent.clone();
    let connector_id = connector.id.clone();
    let start = QueuedRunStart::new(move |run_id| {
        Box::pin(async move {
            sending
                .send_from_owner_accepted(
                    sending_agent,
                    connector_id,
                    "untrusted closure text".into(),
                    "owner-files".into(),
                    run_id,
                )
                .await
                .map_err(|error| QueuedStartError::Failed(error.to_string()))
        })
    });
    let accepted = manager
        .runs
        .accept_run(
            AcceptRun {
                agent_id: agent.clone(),
                session_id: session,
                text: "raw owner text".into(),
                attachment_ids: vec![row.id.clone()],
                skill: None,
                idempotency_key: "owner-files".into(),
                mode: SessionRunMode::Queue,
                source: RunSource::Telegram,
                source_ref: Some(connector.id.clone()),
            },
            start,
        )
        .await
        .unwrap();
    assert!(matches!(accepted, AcceptedRun::Created(_)));
    gate.entered().await;
    let requests = model.requests();
    let user = requests[0]
        .messages
        .iter()
        .find(|message| message.role == anima_core::MessageRole::User)
        .unwrap();
    assert!(user.content.text.starts_with("raw owner text"));
    assert!(!user.content.text.contains("untrusted closure"));
    assert_eq!(user.content.attachments.as_ref().unwrap()[0].data, row.path);
    assert_eq!(
        user.content.attachments.as_ref().unwrap()[0]
            .mime_type
            .as_deref(),
        Some("text/plain")
    );
    gate.release();
    events_until(&mut stream, "run.completed").await;
    let snapshot = state.read().await.get_agent(&agent).unwrap();
    assert!(snapshot
        .messages
        .iter()
        .any(|message| message.room_id == connector.room_id
            && message
                .content
                .attachments
                .as_ref()
                .is_some_and(|refs| refs[0].data == row.path)));
    manager
        .deliver_pending_once(connector.id.clone())
        .await
        .unwrap();
    assert_eq!(
        transport.sent.lock().unwrap().last().unwrap().1,
        "textual reply"
    );
    manager.shutdown().await;
    std::fs::remove_dir_all(temp).unwrap();
}
