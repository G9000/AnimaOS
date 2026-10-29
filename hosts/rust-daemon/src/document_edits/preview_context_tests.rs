use super::*;
use crate::document_edits::preview_context::{PreviewEditContext, REFUSED};

async fn constrain(fixture: &Fixture) -> PreviewEditContext {
    fixture
        .state
        .read()
        .await
        .runs
        .get(&fixture.run.run_id)
        .unwrap()
        .input
        .document_edit_context
        .clone()
        .unwrap()
}

fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
    let DataValue::Object(args) = crate::tools::generated_documents::schema_data(arguments) else {
        unreachable!()
    };
    ToolCall {
        id: format!("blocked-{name}"),
        name: name.into(),
        args,
    }
}

#[tokio::test]
async fn preview_native_gate_blocks_pregranted_generic_writes_processes_delegation_and_discovery() {
    use crate::agent_runs::test_support::{gated_context, tool_input};
    use crate::approvals::PolicyAction;
    let fixture = html_fixture().await;
    constrain(&fixture).await;
    persistent(&fixture).await;
    {
        let mut state = fixture.state.write().await;
        state.approvals.set_policy(
            &fixture.run.agent_id,
            ApprovalPolicy {
                write: PolicyAction::Allow,
                exec: PolicyAction::Allow,
                network: PolicyAction::Allow,
                delegate: PolicyAction::Allow,
            },
        );
        state
            .agents
            .get_mut(&fixture.run.agent_id)
            .unwrap()
            .update_config(AgentConfigUpdate {
                tools: Some(
                    crate::tools::ToolRegistry::new()
                        .resolve_descriptors([
                            "write_file",
                            "edit_file",
                            "multi_edit",
                            "bash",
                            "delegate_to_agent",
                            "spawn_helper",
                            "execute_code",
                            "inspect_office_document",
                            "propose_document_edit",
                            "apply_document_edit",
                        ])
                        .unwrap(),
                ),
                ..Default::default()
            });
    }
    let cancel = CancelSignal::new();
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        cancel.clone(),
        None,
    );
    let mut context = gated_context(&fixture.coordinator, &fixture.run, cancel)
        .await
        .with_action_journal(journal.clone());
    context.document_edits = Some(fixture.service.clone());
    let agent = fixture
        .state
        .read()
        .await
        .get_agent(&fixture.run.agent_id)
        .unwrap()
        .state;
    for call in [
        call(
            "write_file",
            serde_json::json!({"file_path":"source.html","content":"overwritten"}),
        ),
        call(
            "edit_file",
            serde_json::json!({"file_path":"source.html","old_string":"old","new_string":"unreviewed"}),
        ),
        call(
            "multi_edit",
            serde_json::json!({"file_path":"source.html","edits":[{"old_string":"old","new_string":"unreviewed"}]}),
        ),
        call("bash", serde_json::json!({"command":"echo blocked"})),
        call(
            "delegate_to_agent",
            serde_json::json!({"agent_id":"elsewhere","task":"edit the source"}),
        ),
        call(
            "spawn_helper",
            serde_json::json!({"name":"helper","task":"edit the source"}),
        ),
        call(
            "execute_code",
            serde_json::json!({"code":"print('blocked')"}),
        ),
        call("search_tools", serde_json::json!({"query":"write files"})),
    ] {
        let result = context
            .clone()
            .execute_tool(
                agent.clone(),
                tool_input(&fixture.run.agent_id, &fixture.run.session_id),
                call.clone(),
            )
            .await;
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Error,
            "{}",
            call.name
        );
        assert_eq!(result.error.as_deref(), Some(REFUSED), "{}", call.name);
    }
    assert!(
        journal
            .check_preview_tool(&fixture.call, false)
            .await
            .is_err(),
        "helper/RPC custody cannot borrow this owner context"
    );
    assert_eq!(
        std::fs::read(fixture._root.path().join("source.html")).unwrap(),
        b"<p>old</p><script>retained()</script>"
    );
    assert!(!fixture._root.path().join("copy.htm").exists());
    assert!(fixture.state.read().await.approvals.pending().is_empty());
    assert!(fixture
        .state
        .read()
        .await
        .runs
        .get(&fixture.run.run_id)
        .unwrap()
        .action_receipts
        .is_empty());
}

#[tokio::test]
async fn preview_proposal_cannot_change_source_hash_or_selected_range() {
    let fixture = html_fixture().await;
    let context = constrain(&fixture).await;
    for (path, hash, start, end, expected) in [
        ("elsewhere.html", context.sha256().to_owned(), 3, 6, "old"),
        ("source.html", "b".repeat(64), 3, 6, "old"),
        ("source.html", context.sha256().to_owned(), 3, 7, "old<"),
    ] {
        let input: ProposeRequest = serde_json::from_value(serde_json::json!({"source":{"kind":"workspace","file_path":path},"expected_sha256":hash,"file_path":"copy.htm","expected_destination":{"state":"absent"},"operations":[{"type":"html_replace_text","start_utf8":start,"end_utf8":end,"expected_text":expected,"new_text":"new"}]})).unwrap();
        let error = fixture
            .service
            .propose(fixture.run.clone(), input, CancelSignal::new())
            .await
            .unwrap_err();
        assert_eq!(error, REFUSED);
    }
    assert!(!fixture._root.path().join("copy.htm").exists());
}

#[tokio::test]
async fn preview_idempotency_keeps_context_in_the_exact_input_identity() {
    let fixture = html_fixture().await;
    let context = constrain(&fixture).await;
    fixture
        .state
        .write()
        .await
        .runs
        .get_mut(&fixture.run.run_id)
        .unwrap()
        .idempotency_key = Some("preview-key".into());
    let replay = fixture
        .coordinator
        .replayed_run_with_document_context(
            &fixture.run.agent_id,
            &fixture.run.session_id,
            "use a tool",
            "preview-key",
            &[],
            None,
            Some(&context),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        replay,
        crate::agent_runs::AcceptedRun::Replayed(_)
    ));
    assert!(fixture
        .coordinator
        .replayed_run_with_document_context(
            &fixture.run.agent_id,
            &fixture.run.session_id,
            "use a tool",
            "preview-key",
            &[],
            None,
            None
        )
        .await
        .unwrap()
        .is_err());
    let mut changed = context;
    let crate::document_edits::preview_context::PreviewSelection::HtmlText {
        expected_text, ..
    } = &mut changed.selection
    else {
        unreachable!()
    };
    *expected_text = "new".into();
    assert!(fixture
        .coordinator
        .replayed_run_with_document_context(
            &fixture.run.agent_id,
            &fixture.run.session_id,
            "use a tool",
            "preview-key",
            &[],
            None,
            Some(&changed)
        )
        .await
        .unwrap()
        .is_err());
}

#[tokio::test]
async fn preview_context_roundtrips_pending_recovery_and_rejects_steer_downgrades() {
    let fixture = html_fixture().await;
    let context = constrain(&fixture).await;
    let mut record = fixture
        .state
        .read()
        .await
        .runs
        .get(&fixture.run.run_id)
        .unwrap()
        .clone();
    record.pending_steers.push(crate::runs::RunSteer {
        idempotency_key: "preview-steer".into(),
        text: "continue the selected edit".into(),
        attachment_ids: vec![],
        skill: None,
        document_edit_context: Some(context.clone()),
        accepted_at_ms: anima_core::primitives::now_millis(),
    });
    crate::runs::RunLedger::validate(&[record.clone()]).unwrap();
    let restored: crate::runs::RunRecord =
        serde_json::from_value(serde_json::to_value(&record).unwrap()).unwrap();
    assert_eq!(restored.input.document_edit_context, Some(context.clone()));
    let ledger = crate::runs::RunLedger::restored(
        vec![restored],
        &std::collections::HashSet::from([fixture.run.agent_id.clone()]),
        anima_core::primitives::now_millis(),
    );
    let pending = ledger
        .find_by_idempotency_key(&fixture.run.agent_id, "preview-steer", 0)
        .unwrap();
    assert_eq!(pending.input.document_edit_context, Some(context));
    record.pending_steers[0].document_edit_context = None;
    assert!(crate::runs::RunLedger::validate(&[record]).is_err());
}

#[tokio::test]
async fn preview_transcript_context_is_stamped_from_durable_input_for_retry() {
    let fixture = html_fixture().await;
    let context = constrain(&fixture).await;
    fixture
        .state
        .write()
        .await
        .runs
        .get_mut(&fixture.run.run_id)
        .unwrap()
        .status = crate::runs::RunStatus::Queued;
    let content = crate::agent_runs::attachments::accepted_content(
        &fixture.state,
        &fixture.run.agent_id,
        &fixture.run.session_id,
        &fixture.run.run_id,
        std::collections::BTreeMap::from([(
            "documentEditContext".into(),
            DataValue::String("forged".into()),
        )]),
    )
    .await
    .unwrap();
    let stamped = content
        .metadata
        .unwrap()
        .remove("documentEditContext")
        .unwrap();
    let DataValue::String(stamped) = stamped else {
        panic!("native context stamp must be JSON data")
    };
    let stamped: PreviewEditContext = serde_json::from_str(&stamped).unwrap();
    assert_eq!(stamped, context);
}

#[tokio::test]
async fn preview_admission_and_dispatch_refuse_an_identical_source_in_a_different_workspace() {
    let fixture = html_fixture().await;
    let context = constrain(&fixture).await;
    let bytes = b"<p>old</p><script>retained()</script>";
    let inventory =
        anima_document_content::inspect_office_document(OfficeFormat::Html, bytes).unwrap();
    let old_scope = super::super::readonly_scope_in(
        &*fixture.state.read().await,
        &fixture.run.agent_id,
        &fixture.run.session_id,
    )
    .unwrap();
    super::super::owner_inspection::validate_inspected_preview_context(
        &context,
        &inventory,
        &digest(bytes),
        &old_scope,
    )
    .unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("source.html"), bytes).unwrap();
    fixture
        .state
        .write()
        .await
        .workspace
        .as_mut()
        .unwrap()
        .root_path = other.path().to_path_buf();
    let changed_scope = super::super::readonly_scope_in(
        &*fixture.state.read().await,
        &fixture.run.agent_id,
        &fixture.run.session_id,
    )
    .unwrap();
    assert_ne!(
        changed_scope
            .source_scope("source.html", &digest(bytes))
            .unwrap(),
        context.source_scope()
    );
    assert!(
        super::super::owner_inspection::validate_inspected_preview_context(
            &context,
            &inventory,
            &digest(bytes),
            &changed_scope
        )
        .is_err(),
        "a stale inspected source scope must not be accepted despite identical path and bytes"
    );
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        CancelSignal::new(),
        None,
    );
    assert!(
        journal
            .check_preview_tool(&fixture.call, true)
            .await
            .is_err(),
        "queued native context must not rebind to the replacement workspace"
    );
    assert_eq!(
        std::fs::read(other.path().join("source.html")).unwrap(),
        bytes
    );
}

#[tokio::test]
async fn preview_raw_instruction_supplies_the_native_selection_to_the_model_without_rewriting_input(
) {
    use crate::agent_runs::test_support::{ScriptedModel, Step};
    use crate::agent_runs::{AgentRunRequest, RunRoom};
    let fixture = html_fixture().await;
    persistent(&fixture).await;
    let selected = constrain(&fixture).await;
    let model = ScriptedModel::new(vec![Step::Text(vec!["ready for the requested edit"])]);
    let raw_instruction = "use a tool";
    {
        let mut state = fixture.state.write().await;
        state.model_adapter = model.clone();
        let record = state.runs.get_mut(&fixture.run.run_id).unwrap();
        record.status = crate::runs::RunStatus::Queued;
        record.started_at_ms = None;
        record.idempotency_key = Some("preview-model-input".into());
        state.live.runs().register(&fixture.run.run_id);
    }
    fixture
        .coordinator
        .run_accepted(
            AgentRunRequest {
                agent_id: fixture.run.agent_id.clone(),
                content: anima_core::Content {
                    text: raw_instruction.into(),
                    metadata: Some(std::collections::BTreeMap::from([(
                        "documentEditContext".into(),
                        DataValue::String("forged-prompt-context".into()),
                    )])),
                    ..Default::default()
                },
                room: RunRoom::Stable(fixture.run.session_id.clone()),
                idempotency_key: Some("preview-model-input".into()),
                source: crate::runs::RunSource::Web,
                source_ref: None,
                parent: None,
            },
            fixture.run.run_id.clone(),
        )
        .await
        .unwrap();
    let requests = model.requests();
    let request = requests.first().expect("native run reached the model");
    assert!(request.system.contains("propose_document_edit"));
    assert!(request.system.contains("apply_document_edit"));
    assert!(request.system.contains("quoted document reference data"));
    assert!(
        request
            .system
            .contains(&serde_json::to_string(&selected).unwrap()),
        "model must receive the exact native path/hash/scope/selected range"
    );
    assert!(
        !request.system.contains("forged-prompt-context"),
        "arbitrary content metadata is not selection authority"
    );
    let owner_message = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == anima_core::MessageRole::User)
        .unwrap();
    assert_eq!(owner_message.content.text, raw_instruction);
    let state = fixture.state.read().await;
    let record = state.runs.get(&fixture.run.run_id).unwrap();
    assert_eq!(record.input.text, raw_instruction);
    assert_eq!(record.input.document_edit_context, Some(selected));
    assert_eq!(
        record.idempotency_key.as_deref(),
        Some("preview-model-input")
    );
}
