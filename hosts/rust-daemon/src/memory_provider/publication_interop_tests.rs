//! Opt-in interoperability against an explicitly selected executable and synthetic stores only.
use super::*;
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::mcp::{
    transport::DefaultMcpTransportFactory, McpManager, McpStdioSpec, ReviewMcpTool,
    UpdateMcpConnection,
};
use crate::profile_owner::ProfileResource;
use std::path::{Path, PathBuf};

#[tokio::test]
#[ignore = "Requires ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER pointing to the reviewed Mother executable"]
async fn selected_publication_real_mother_stdio_receipt_mapping_and_revocation() {
    let executable = PathBuf::from(
        std::env::var_os("ANIMA_WORKSPACE_CONTEXT_TEST_MOTHER")
            .expect("Set the explicit Mother fixture executable"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("synthetic-mother.sqlite");
    let soul = dir.path().join("SOUL.md");
    let client = dir.path().join("synthetic-client.private.json");
    let home = dir.path().join("synthetic-mother-home");
    let journal = dir.path().join("publications.sqlite");
    let control = dir.path().join("control.json");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        &soul,
        "Synthetic selected-publication interoperability persona.",
    )
    .unwrap();
    let owner = ProfileOwner::acquire_resources([
        ProfileResource::Sqlite(journal.clone()),
        ProfileResource::File(control.clone()),
    ])
    .unwrap();
    let mut daemon = DaemonState::new();
    let agent = daemon
        .create_agent(crate::sessions::test_support::agent_config(
            "Synthetic real-wire publication owner",
        ))
        .unwrap()
        .state
        .id;
    daemon.profile_owner = Some(owner.clone());
    daemon.control_plane_store = Some(
        ControlPlaneStoreConfig::Json(control)
            .with_owner(owner.clone())
            .unwrap(),
    );
    daemon.approvals.set_policy(
        &agent,
        crate::approvals::ApprovalPolicy {
            exec: crate::approvals::PolicyAction::Allow,
            ..Default::default()
        },
    );
    fixture_cli(
        &executable,
        &home,
        &[
            "init".into(),
            "--db".into(),
            path(&database),
            "--soul".into(),
            path(&soul),
        ],
    )
    .await;
    fixture_cli(
        &executable,
        &home,
        &[
            "grant".into(),
            "anima-publication-interop".into(),
            "--access".into(),
            "companion".into(),
            "--subject".into(),
            agent.clone(),
            "--scopes".into(),
            "read,context:propose,memory:write".into(),
            "--db".into(),
            path(&database),
            "--out".into(),
            path(&client),
        ],
    )
    .await;
    let context = fixture_cli(
        &executable,
        &home,
        &["context".into(), "--config".into(), path(&client)],
    )
    .await;
    let context: Value = serde_json::from_slice(&context).expect("Synthetic workspace context");
    let state = Arc::new(RwLock::new(daemon));
    let publication = state.read().await.mcp_publication.clone();
    let manager = McpManager::new(
        Arc::downgrade(&state),
        Arc::new(Mutex::new(())),
        publication,
        Arc::new(DefaultMcpTransportFactory::default()),
    );
    state.write().await.mcp_manager = Some(manager.clone());
    let row = manager
        .create_stdio(
            &agent,
            "Synthetic publication interoperability".into(),
            McpStdioSpec {
                executable: path(&executable),
                args: vec!["mcp".into(), "--config".into(), path(&client)],
            },
        )
        .await
        .unwrap();
    let mut row = manager
        .update(
            &agent,
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
    row = manager
        .connect(&agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    for remote in [
        "mother_get_context",
        "mother_propose_memory",
        "mother_get_receipt",
        "mother_get_proposal",
        "mother_write_record",
    ] {
        let descriptor_digest = row
            .candidates
            .iter()
            .find(|tool| tool.remote_name == remote)
            .expect("Real Mother exposes the reviewed publication tool")
            .descriptor_digest
            .clone();
        row = manager
            .review(
                &agent,
                &row.record.id,
                ReviewMcpTool {
                    expected_version: row.record.version,
                    discovery_id: row.discovery_id.clone().unwrap(),
                    descriptor_digest,
                },
            )
            .await
            .unwrap();
    }
    let selected_binding: binding::MemoryProviderBinding = serde_json::from_value(json!({
        "version":1,"provider":"mother","connectionId":row.record.id,
        "workspaceId":context["workspace"]["id"],"generation":context["workspace"]["generation"],
        "subject":agent,"publication":"selected","personaSource":"local","accessMode":"companion",
    }))
    .unwrap();
    {
        let mut guard = state.write().await;
        let mut config = guard.agents[&agent].config().clone();
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert(binding::SETTING.into(), selected_binding.portable());
        config.tools = Some(
            row.record
                .reviewed_tools
                .iter()
                .map(|tool| tool.tool_descriptor())
                .collect(),
        );
        guard.restore_agent_config(&agent, config);
        manager.refresh_agent_permissions(&guard, &agent).unwrap();
    }
    let service = PublicationService::open(&state, owner, &journal)
        .await
        .unwrap();
    state.write().await.memory_publications = Some(service.clone());

    let selection = synthetic_selection(
        &state,
        &agent,
        "Synthetic proposal selected over real stdio",
        Delivery::Proposal,
    )
    .await;
    let queued = service.admit(&agent, selection.clone()).await.unwrap();
    assert_eq!(queued.state, PublicationState::Queued);
    assert_eq!(queued.admission.request["record"]["sources"], json!([]));
    // Simulate only the lost acknowledgement: the actual reviewed transport sends the
    // exact persisted request, while local state deliberately records uncertainty.
    let id = queued.admission.publication_id.clone();
    let claim = service
        .db(move |store| store.claim(&id, queued.revision, now_ms()))
        .await
        .unwrap();
    let response = manager
        .invoke_publication(
            &claim,
            "mother_propose_memory",
            claim.admission.request.clone(),
            CancelSignal::new(),
        )
        .await
        .unwrap();
    assert!(
        reconcile::envelope(&response.text).is_ok(),
        "Real Mother rejected the synthetic proposal contract"
    );
    let id = claim.admission.publication_id.clone();
    let unknown = service
        .db(move |store| {
            store.transition(
                &id,
                claim.revision,
                Transition::Unknown {
                    error: "synthetic lost acknowledgement".into(),
                    next_attempt_at_ms: 0,
                },
                now_ms(),
            )
        })
        .await
        .unwrap();
    service.dispatch(unknown).await;
    let pending = service
        .get(&agent, &selection.publication_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pending.state,
        PublicationState::SubmittedForReview,
        "Real receipt reconciliation must retain review status"
    );
    assert!(pending.proposal_id.is_some());
    assert_eq!(pending.receipt.as_ref().unwrap()["status"], "pending");
    let id = selection.publication_id.clone();
    assert!(service
        .db(move |store| store.mapping(&id))
        .await
        .unwrap()
        .is_none());
    // Poll the real proposal without waiting for the production backoff clock.
    let id = selection.publication_id.clone();
    let due = service
        .db(move |store| {
            store.transition(
                &id,
                pending.revision,
                Transition::Submitted {
                    proposal_id: pending.proposal_id.unwrap(),
                    receipt: pending.receipt.unwrap(),
                    next_attempt_at_ms: 0,
                },
                now_ms(),
            )
        })
        .await
        .unwrap();
    service.dispatch(due).await;
    assert_eq!(
        service
            .get(&agent, &selection.publication_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        PublicationState::SubmittedForReview
    );

    let direct = synthetic_selection(
        &state,
        &agent,
        "Separate synthetic direct shared fact",
        Delivery::Direct,
    )
    .await;
    let queued = service.admit(&agent, direct.clone()).await.unwrap();
    service.dispatch(queued).await;
    let saved = service
        .get(&agent, &direct.publication_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.state,
        PublicationState::Saved,
        "Explicit memory:write grant must produce a real canonical receipt"
    );
    assert_eq!(
        saved.receipt.as_ref().unwrap()["operationId"],
        saved.admission.operation_id
    );
    let id = direct.publication_id.clone();
    let mapping = service
        .db(move |store| store.mapping(&id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(Some(mapping.canonical_id), saved.canonical_id);
    assert_eq!(Some(mapping.canonical_revision), saved.canonical_revision);

    let revoked = synthetic_selection(
        &state,
        &agent,
        "Synthetic fact withheld after remote revocation",
        Delivery::Proposal,
    )
    .await;
    let queued = service.admit(&agent, revoked.clone()).await.unwrap();
    fixture_cli(
        &executable,
        &home,
        &[
            "revoke".into(),
            "anima-publication-interop".into(),
            "--db".into(),
            path(&database),
        ],
    )
    .await;
    service.dispatch(queued).await;
    let blocked = service
        .get(&agent, &revoked.publication_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blocked.state, PublicationState::Blocked);
    assert!(blocked.proposal_id.is_none() && blocked.canonical_id.is_none());
    let id = revoked.publication_id;
    assert!(service
        .db(move |store| store.mapping(&id))
        .await
        .unwrap()
        .is_none());
    assert!(state
        .read()
        .await
        .memory_handle()
        .read()
        .await
        .get(&revoked.local_id)
        .is_some());
    service.shutdown().await;
    manager.shutdown().await;
}

async fn synthetic_selection(
    state: &SharedDaemonState,
    agent: &str,
    text: &str,
    delivery: Delivery,
) -> Selection {
    let memory = state.read().await.memory_handle();
    let source = memory
        .write()
        .await
        .add(anima_memory::NewMemory {
            agent_id: agent.into(),
            agent_name: "Synthetic interoperability".into(),
            memory_type: MemoryType::Fact,
            content: text.into(),
            importance: 0.5,
            tags: None,
            scope: Some(MemoryScope::Shared),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap();
    Selection {
        publication_id: uuid::Uuid::new_v4().to_string(),
        local_id: source.id,
        expected_content: source.content,
        title: "Synthetic interoperability fact".into(),
        source: "Explicit owner selection in isolated interoperability test".into(),
        basis: Basis::OwnerStatement,
        delivery,
        target: None,
    }
}
fn path(value: &Path) -> String {
    value.to_str().unwrap().to_owned()
}
async fn fixture_cli(executable: &Path, home: &Path, args: &[String]) -> Vec<u8> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .env("MOTHER_HOME", home)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .expect("Synthetic Mother CLI deadline")
        .expect("Synthetic Mother CLI launch");
    // Grant output may contain a token. Never print stdout, stderr or the private config.
    assert!(
        output.status.success(),
        "Synthetic Mother CLI {} failed with {:?}",
        args[0],
        output.status.code()
    );
    output.stdout
}
