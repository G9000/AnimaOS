use super::*;
use anima_core::{DataValue, ToolCall};
use std::collections::BTreeMap;

async fn workspace(manager: &DiscordManager) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    manager.state.write().await.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.path().into(),
        ..Default::default()
    });
    temp
}

async fn accept_upload(
    manager: &DiscordManager,
    c: &DiscordConnectorRecord,
    message: DiscordMessage,
) -> Result<(), DiscordManagerError> {
    let cursor = message.id.clone();
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![message],
                next_cursor: cursor,
                boundary_proven: true,
            },
        )
        .await
}

fn upload_message(c: &DiscordConnectorRecord, id: &str, text: &str) -> DiscordMessage {
    let mut value = serde_json::to_value(incoming(c, id)).unwrap();
    value["text"] = serde_json::json!(text);
    value["attachments"] = serde_json::json!([{
        "id":"301", "filename":"report.md", "size":9,
        "url":format!("https://cdn.discordapp.com/attachments/{}/301/report.md", c.channel.id)
    }]);
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn discord_attachment_only_upload_is_admitted_after_participation() {
    let (manager, _, c) = bot_wide().await;
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> start").await;
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    let message = upload_message(&thread, "201", "");
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![message],
                next_cursor: "201".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    let state = manager.state.read().await;
    assert!(
        state
            .discord_inbound
            .contains_key(&(c.id.clone(), "201".into())),
        "an upload is a message even without a caption"
    );
    drop(state);
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_saved_text_reaches_the_model_and_is_reused_after_retry() {
    let (manager, transport, c) = bot_wide().await;
    let temp = workspace(&manager).await;
    transport
        .downloads
        .lock()
        .unwrap()
        .insert("301".into(), b"# report\n".to_vec());
    let model = ScriptedModel::new(vec![Step::Text(vec!["Read report"])]);
    manager.state.write().await.model_adapter = model.clone();
    let parent = discover(&manager, &c, "22", "101").await;
    let message = upload_message(&parent, "101", "<@11> read this");
    accept_upload(&manager, &parent, message.clone())
        .await
        .unwrap();
    let row = manager.state.read().await.discord_inbound[&(c.id.clone(), "101".into())].clone();
    assert_eq!(row.attachment_ids.len(), 1);
    let store = manager.state.read().await.history.store();
    let saved = store
        .get_attachment(&c.agent_id, &row.attachment_ids[0])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.session_id, row.room_id);
    assert_eq!(
        crate::attachments::files::read_attachment_bytes(temp.path(), &saved).unwrap(),
        b"# report\n"
    );
    assert!(saved.path.starts_with("uploads/"));
    let restored: DiscordInboundRecord =
        serde_json::from_value(serde_json::to_value(&row).unwrap()).unwrap();
    assert_eq!(restored.attachment_ids, row.attachment_ids);
    // Simulate retry after a failed cursor commit: the saved source identity is reused.
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    let (ids, text) = manager
        .receive_attachments(&thread, &message)
        .await
        .unwrap();
    assert_eq!(ids, row.attachment_ids);
    assert_eq!(text, row.text);
    assert_eq!(transport.download_calls.load(Ordering::SeqCst), 1);
    processed(&manager, &c, "101").await;
    let requests = model.requests();
    let user = requests[0]
        .messages
        .iter()
        .find(|m| m.role == MessageRole::User)
        .unwrap();
    assert!(user.content.text.contains("# report"));
    assert!(user
        .content
        .text
        .contains("Untrusted Discord attachment text"));
    assert!(user.content.text.contains("user_id=44"));
    assert!(user
        .content
        .attachments
        .as_ref()
        .unwrap()
        .iter()
        .any(|a| a.name == "report.md"));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_authorization_precedes_any_download_and_failures_keep_cursor() {
    let (manager, transport, c) = bot_wide().await;
    let _temp = workspace(&manager).await;
    let parent = discover(&manager, &c, "22", "101").await;
    let mut denied = upload_message(&parent, "101", "<@11> read this");
    denied.sender.id = "99".into();
    accept_upload(&manager, &parent, denied).await.unwrap();
    assert_eq!(transport.download_calls.load(Ordering::SeqCst), 0);
    let current = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("22")
        .unwrap();
    *transport.download_error.lock().unwrap() = Some(DiscordTransportError::Transport);
    assert!(accept_upload(
        &manager,
        &current,
        upload_message(&current, "201", "<@11> read this")
    )
    .await
    .is_err());
    assert_eq!(
        manager
            .get(&c.agent_id, &c.id)
            .await
            .unwrap()
            .destination_for_channel("22")
            .unwrap()
            .cursor,
        "101"
    );
    assert!(!manager
        .state
        .read()
        .await
        .discord_inbound
        .contains_key(&(c.id.clone(), "201".into())));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_intake_save_failure_reuses_saved_upload_without_advancing() {
    let (manager, transport, c) = setup().await;
    let _temp = workspace(&manager).await;
    transport
        .downloads
        .lock()
        .unwrap()
        .insert("301".into(), b"# report\n".to_vec());
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let worker = manager.clone();
    let dest = c.clone();
    let task = tokio::spawn(async move {
        accept_upload(&worker, &dest, upload_message(&dest, "101", "read")).await
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert!(within(task).await.unwrap().is_err());
    assert_eq!(manager.get(&c.agent_id, &c.id).await.unwrap().cursor, "0");
    assert!(!manager
        .state
        .read()
        .await
        .discord_inbound
        .contains_key(&(c.id.clone(), "101".into())));
    accept_upload(&manager, &c, upload_message(&c, "101", "read"))
        .await
        .unwrap();
    assert_eq!(transport.download_calls.load(Ordering::SeqCst), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_unsupported_and_oversize_files_are_reported_truthfully() {
    let (manager, transport, c) = setup().await;
    let _temp = workspace(&manager).await;
    transport
        .downloads
        .lock()
        .unwrap()
        .insert("301".into(), b"# report\n".to_vec());
    let mut message = upload_message(&c, "101", "");
    message.attachments[0].filename = "movie.mp4".into();
    accept_upload(&manager, &c, message).await.unwrap();
    let first = manager.state.read().await.discord_inbound[&(c.id.clone(), "101".into())].clone();
    assert!(first.attachment_ids.is_empty());
    assert!(first.text.contains("unsupported"));
    let mut message = upload_message(&c, "201", "");
    message.attachments[0].size = 26 * 1024 * 1024;
    accept_upload(&manager, &c, message).await.unwrap();
    assert_eq!(transport.download_calls.load(Ordering::SeqCst), 1);
    assert!(
        manager.state.read().await.discord_inbound[&(c.id.clone(), "201".into())]
            .text
            .contains("size limit")
    );
    manager.shutdown().await;
}

fn file_call(path: &str) -> ToolCall {
    ToolCall {
        id: "upload-report".into(),
        name: "send_discord_file".into(),
        args: BTreeMap::from([
            ("path".into(), DataValue::String(path.into())),
            ("caption".into(), DataValue::String("Report".into())),
        ]),
    }
}

#[tokio::test]
async fn discord_attachment_send_waits_for_acceptance_and_uploads_generated_markdown() {
    let (manager, transport, c, model) = rename_tests::rename_fixture(file_call("report.md")).await;
    let temp = workspace(&manager).await;
    std::fs::write(temp.path().join("report.md"), b"# generated report\n").unwrap();
    transport.file_gate.store(true, Ordering::SeqCst);
    rename_tests::request_rename(&manager, &c).await;
    assert!(manager.process_one(c.id.clone()).await.unwrap());
    within(transport.file_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    assert_eq!(
        model.requests().len(),
        2,
        "the model cannot confirm until Discord acknowledges the upload"
    );
    transport.file_release.0.add_permits(1);
    within(async {
        while !manager.state.read().await.discord_inbound[&(c.id.clone(), "201".into())]
            .processing_state
            .is_terminal()
        {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(rename_tests::rename_result(&model).contains("\"sent\":true"));
    assert!(model.configs()[1].allows_tool("send_discord_file"));
    let files = transport.files.lock().unwrap();
    assert_eq!(files[0].0, "101");
    assert_eq!(files[0].1, "report.md");
    assert_eq!(files[0].2, b"# generated report\n");
    drop(files);
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_send_errors_do_not_claim_success_or_retry() {
    for error in [
        DiscordTransportError::PermissionDenied,
        DiscordTransportError::Uncertain,
        DiscordTransportError::RateLimited {
            retry_after_ms: 10,
            global: false,
        },
    ] {
        let (manager, transport, c, model) =
            rename_tests::rename_fixture(file_call("report.md")).await;
        let temp = workspace(&manager).await;
        std::fs::write(temp.path().join("report.md"), b"# generated report\n").unwrap();
        *transport.file_error.lock().unwrap() = Some(error);
        rename_tests::request_rename(&manager, &c).await;
        processed(&manager, &c, "201").await;
        let result = rename_tests::rename_result(&model);
        assert!(!result.contains("\"sent\":true"));
        assert!(result.contains("Discord"));
        assert!(transport.files.lock().unwrap().is_empty());
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn discord_attachment_send_rejects_unsafe_paths_and_revoked_scopes() {
    let (manager, transport, c, _) = rename_tests::rename_fixture(file_call("report.md")).await;
    let temp = workspace(&manager).await;
    std::fs::write(temp.path().join("report.md"), b"# generated report\n").unwrap();
    let (run, scope, _) = rename_tests::active_rename_scope(&manager, &c).await;
    for path in [
        "../report.md",
        "C:/report.md",
        "/report.md",
        "report.md:stream",
        "missing.md",
    ] {
        assert!(scope.send_file(&run, path, "", "call", None).await.is_err());
    }
    manager
        .state
        .write()
        .await
        .discord_connectors
        .get_mut(&c.id)
        .unwrap()
        .configuration_revision += 1;
    assert!(scope
        .send_file(&run, "report.md", "", "call", None)
        .await
        .is_err());
    assert!(transport.files.lock().unwrap().is_empty());
    manager.shutdown().await;
}

struct DocumentFixture;
#[async_trait::async_trait]
impl crate::attachments::documents::worker::DocumentExtractor for DocumentFixture {
    async fn extract(
        &self,
        format: anima_service_adapters::document_content::DocumentFormat,
        bytes: Vec<u8>,
        _: anima_core::CancelSignal,
        _: Option<tokio::time::Instant>,
    ) -> Result<
        anima_service_adapters::document_content::ExtractedDocumentText,
        crate::attachments::documents::worker::WorkerFailure,
    > {
        use anima_service_adapters::document_content::*;
        assert_eq!(format, DocumentFormat::Docx);
        assert!(bytes.starts_with(b"PK"));
        Ok(ExtractedDocumentText {
            version: 2,
            format,
            status: DocumentExtractionStatus::Ready,
            blocks: vec![DocumentBlock {
                locator: DocumentLocator::DocxParagraph { paragraph: 1 },
                text: "Discord document fact 731".into(),
            }],
            omissions: vec![DocumentOmission::MainBodyOnly],
            pdf_coverage: None,
            xlsx_coverage: None,
        })
    }
}

#[tokio::test]
async fn discord_attachment_images_and_documents_reach_central_model_hydration() {
    let (mut manager, transport, c) = setup().await;
    let _temp = workspace(&manager).await;
    manager.runs = manager
        .runs
        .clone()
        .with_document_extractor(Arc::new(DocumentFixture));
    let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x2c\x3b".to_vec();
    let docx = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/documents/m54-selected.docx"
    ))
    .to_vec();
    transport
        .downloads
        .lock()
        .unwrap()
        .extend([("301".into(), gif.clone()), ("302".into(), docx.clone())]);
    let model = ScriptedModel::new(vec![Step::Text(vec!["Read image and document"])]);
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        let mut config = state.agents[&c.agent_id].config().clone();
        config.model = "gpt-4o-mini".into();
        config.provider = Some("openai".into());
        state.restore_agent_config(&c.agent_id, config);
    }
    let mut message = upload_message(&c, "101", "read both");
    message.attachments = vec![
        DiscordAttachment {
            id: "301".into(),
            filename: "photo.gif".into(),
            size: gif.len() as u64,
            url: format!(
                "https://cdn.discordapp.com/attachments/{}/301/photo.gif",
                c.channel.id
            ),
        },
        DiscordAttachment {
            id: "302".into(),
            filename: "notes.docx".into(),
            size: docx.len() as u64,
            url: format!(
                "https://cdn.discordapp.com/attachments/{}/302/notes.docx",
                c.channel.id
            ),
        },
    ];
    accept_upload(&manager, &c, message).await.unwrap();
    processed(&manager, &c, "101").await;
    let requests = model.requests();
    assert!(!requests.is_empty());
    let user = requests[0]
        .messages
        .iter()
        .find(|m| m.role == MessageRole::User)
        .unwrap();
    assert!(
        user.content.text.contains("Discord document fact 731"),
        "central document admission must retain its excerpt"
    );
    assert!(
        user.content
            .attachments
            .iter()
            .flatten()
            .any(|a| a.data.starts_with("data:image/gif;base64,")),
        "vision receives verified saved bytes"
    );
    assert_eq!(
        manager.state.read().await.discord_inbound[&(c.id.clone(), "101".into())]
            .attachment_ids
            .len(),
        2
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_file_tool_cannot_be_granted_to_a_spoofed_web_run() {
    let (manager, transport, c) = setup().await;
    let temp = workspace(&manager).await;
    std::fs::write(temp.path().join("report.md"), b"# private report\n").unwrap();
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![file_call("report.md")]),
        Step::Text(vec!["No Discord destination"]),
    ]);
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        let mut config = state.agents[&c.agent_id].config().clone();
        config.tools = Some(
            state
                .tool_registry
                .resolve_descriptors(["send_discord_file"])
                .unwrap(),
        );
        state.restore_agent_config(&c.agent_id, config);
    }
    let mut request =
        crate::agent_runs::test_support::chat_request(&c.agent_id, "chat:spoof", "send file");
    request.content.metadata = Some(BTreeMap::from([
        ("source".into(), DataValue::String("discord".into())),
        ("connectorId".into(), DataValue::String(c.id.clone())),
    ]));
    manager.runs.run(request).await.unwrap();
    assert!(!model.configs()[0].allows_tool("send_discord_file"));
    assert!(transport.files.lock().unwrap().is_empty());
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_file_tool_discovery_withdraws_when_its_scope_changes() {
    use anima_core::ModelToolSelector;
    let (manager, _, c, _) = rename_tests::rename_fixture(file_call("report.md")).await;
    let (link, scope, mut agent) = rename_tests::active_rename_scope(&manager, &c).await;
    let mut original = agent.config.clone();
    original
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(true));
    agent.config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["send_discord_file"])
            .unwrap(),
    );
    let catalog = crate::tools::catalog::RunToolCatalog::new(
        &manager.state,
        link.clone(),
        anima_core::CancelSignal::default(),
        &original,
        &agent,
        None,
    )
    .with_discord_thread(Some(scope));
    assert!(catalog.visible_tool_names().contains("send_discord_file"));
    assert!(catalog
        .describe(&agent, &link, "send_discord_file")
        .await
        .is_ok());
    manager
        .state
        .write()
        .await
        .discord_connectors
        .get_mut(&c.id)
        .unwrap()
        .configuration_revision += 1;
    assert!(!catalog.visible_tool_names().contains("send_discord_file"));
    assert!(catalog
        .check(&agent, &link, "send_discord_file")
        .await
        .is_err());
    assert!(catalog
        .describe(&agent, &link, "send_discord_file")
        .await
        .is_err());
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_attachment_completed_invalid_download_does_not_block_intake() {
    for error in [
        DiscordTransportError::InvalidResponse,
        DiscordTransportError::HttpStatus { status: 404 },
    ] {
        let (manager, transport, c) = setup().await;
        let _temp = workspace(&manager).await;
        *transport.download_error.lock().unwrap() = Some(error);
        accept_upload(&manager, &c, upload_message(&c, "101", "read this"))
            .await
            .unwrap();
        let row = manager.state.read().await.discord_inbound[&(c.id.clone(), "101".into())].clone();
        assert!(row.attachment_ids.is_empty());
        assert!(row.text.contains("not read"));
        assert_eq!(manager.get(&c.agent_id, &c.id).await.unwrap().cursor, "101");
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn discord_attachment_common_filename_does_not_exhaust_shared_upload_collisions() {
    let (manager, transport, c) = setup().await;
    let temp = workspace(&manager).await;
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    for _ in 0..100 {
        crate::attachments::files::save_upload(temp.path(), &date, "report.md", b"# report\n")
            .unwrap();
    }
    transport
        .downloads
        .lock()
        .unwrap()
        .insert("301".into(), b"# report\n".to_vec());
    accept_upload(&manager, &c, upload_message(&c, "101", "read this"))
        .await
        .unwrap();
    assert_eq!(
        manager.state.read().await.discord_inbound[&(c.id.clone(), "101".into())]
            .attachment_ids
            .len(),
        1
    );
    manager.shutdown().await;
}
