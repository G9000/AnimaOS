//! Abrupt process termination around acknowledged durable selected-publication admission.
use super::*;
use crate::{
    control_plane_store::{
        load_control_plane_snapshot, save_control_plane_snapshot, ControlPlaneStoreConfig,
    },
    mcp::{McpConnectionRecord, ReviewedMcpTool},
    memory_store::{load_memory_snapshot, save_memory_manager, MemoryStoreConfig},
    profile_owner::ProfileResource,
};
use anima_core::prepare_external_tool;
use anima_memory::NewMemory;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Instant,
};

const CHILD_TEST: &str =
    "memory_provider::publication::crash_tests::publication_admission_crash_child";

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stores(root: &Path) -> (ProfileOwner, ControlPlaneStoreConfig, MemoryStoreConfig) {
    let owner = ProfileOwner::acquire_resources([
        ProfileResource::Sqlite(root.join("publications.sqlite")),
        ProfileResource::File(root.join("control.json")),
        ProfileResource::File(root.join("native-memory.json")),
    ])
    .unwrap();
    let control = ControlPlaneStoreConfig::Json(root.join("control.json"))
        .with_owner(owner.clone())
        .unwrap();
    let memory = MemoryStoreConfig::Json(root.join("native-memory.json"))
        .with_owner(owner.clone())
        .unwrap();
    (owner, control, memory)
}

fn signal(root: &Path, report: &Value) {
    let mut file = tempfile::NamedTempFile::new_in(root).unwrap();
    file.write_all(&serde_json::to_vec(report).unwrap())
        .unwrap();
    file.as_file().sync_all().unwrap();
    file.persist_noclobber(root.join("barrier.json")).unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "Synthetic child entry point; selected by the abrupt admission parent test"]
async fn publication_admission_crash_child() {
    let root = PathBuf::from(std::env::var_os("ANIMA_TEST_PUBLICATION_CRASH_ROOT").unwrap());
    let root = fs::canonicalize(root).unwrap();
    assert!(root.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
    let phase = std::env::var("ANIMA_TEST_PUBLICATION_CRASH_PHASE").unwrap();
    assert!(matches!(phase.as_str(), "before" | "after"));
    let (owner, control, memory_store) = stores(&root);
    let mut daemon = DaemonState::new();
    let agent = daemon
        .create_agent(crate::sessions::test_support::agent_config(
            "Synthetic admission crash owner",
        ))
        .unwrap()
        .state
        .id;
    let connection = uuid::Uuid::new_v4().to_string();
    let binding: binding::MemoryProviderBinding = serde_json::from_value(json!({
        "version":1,"provider":"mother","connectionId":connection,
        "workspaceId":uuid::Uuid::new_v4().to_string(),
        "generation":uuid::Uuid::new_v4().to_string(),"subject":agent,
        "publication":"selected","personaSource":"local","accessMode":"companion"
    }))
    .unwrap();
    let descriptor = prepare_external_tool(
        "mother_propose_memory",
        "Synthetic admission-only metadata",
        json!({"type":"object","additionalProperties":true}),
        None,
    )
    .unwrap();
    let alias = crate::mcp::alias_for(&connection, &descriptor.descriptor_digest).unwrap();
    daemon.mcp_registry.connections.insert(
        connection.clone(),
        McpConnectionRecord {
            id: connection,
            agent_id: agent.clone(),
            label: "Synthetic admission-only connection, never started".into(),
            endpoint: "http://127.0.0.1:9999/mcp".into(),
            stdio: None,
            bearer_credential_ref: None,
            oauth_credential_ref: None,
            enabled: true,
            version: 1,
            reviewed_tools: vec![ReviewedMcpTool { alias, descriptor }],
        },
    );
    let mut config = daemon.agents[&agent].config().clone();
    config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert(binding::SETTING.into(), binding.portable());
    daemon.restore_agent_config(&agent, config);
    daemon.profile_owner = Some(owner.clone());
    daemon.control_plane_store = Some(control.clone());
    let memory = daemon.memory_handle();
    let source = memory
        .write()
        .await
        .add(NewMemory {
            agent_id: agent.clone(),
            agent_name: "Synthetic crash owner".into(),
            memory_type: MemoryType::Fact,
            content: "Synthetic selected source survives an abrupt admission crash".into(),
            importance: 0.8,
            tags: None,
            scope: Some(MemoryScope::Shared),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap();
    save_memory_manager(Some(&memory_store), &*memory.read().await)
        .await
        .unwrap();
    save_control_plane_snapshot(Some(&control), &daemon.control_plane_snapshot())
        .await
        .unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let service = PublicationService::open(&state, owner, &root.join("publications.sqlite"))
        .await
        .unwrap();
    let selection = Selection {
        publication_id: uuid::Uuid::new_v4().to_string(),
        local_id: source.id.clone(),
        expected_content: source.content.clone(),
        title: "Synthetic abrupt admission selection".into(),
        source: "Explicit synthetic owner selection before process termination".into(),
        basis: Basis::OwnerStatement,
        delivery: Delivery::Proposal,
        target: None,
    };
    let acknowledged = if phase == "after" {
        Some(service.admit(&agent, selection.clone()).await.unwrap())
    } else {
        None
    };
    assert!(state.read().await.mcp_manager.is_none());
    signal(
        &root,
        &json!({
            "phase":phase,"agentId":agent,"selection":selection,
            "source":source,"acknowledged":acknowledged
        }),
    );
    // Keep the live SQLite handle, source and owner lease until the parent kills this process.
    tokio::time::sleep(Duration::from_secs(45)).await;
    panic!("parent did not terminate the synthetic admission child at its barrier");
}

#[tokio::test(flavor = "current_thread")]
async fn publication_admission_survives_abrupt_kill_after_ack_and_is_absent_before_admission() {
    for phase in ["before", "after"] {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
            .env("ANIMA_TEST_PUBLICATION_CRASH_ROOT", &root)
            .env("ANIMA_TEST_PUBLICATION_CRASH_PHASE", phase)
            .env(
                "ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR",
                crate::profile_owner::test_registry_path().unwrap(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = ChildGuard(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(20);
        let report = loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "{phase}: child exited before barrier"
            );
            if let Some(report) = fs::read(root.join("barrier.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            {
                break report;
            }
            assert!(
                Instant::now() < deadline,
                "{phase}: admission barrier deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let native_before = fs::read(root.join("native-memory.json")).unwrap();
        child
            .0
            .kill()
            .expect("abruptly terminate the live admission process");
        let status = child.0.wait().unwrap();
        assert!(
            !status.success(),
            "child must be killed without normal teardown"
        );
        assert_eq!(
            fs::read(root.join("native-memory.json")).unwrap(),
            native_before
        );

        let agent = report["agentId"].as_str().unwrap();
        let selection: Selection = serde_json::from_value(report["selection"].clone()).unwrap();
        let (owner, control, memory_store) = stores(&root);
        let mut daemon = DaemonState::new();
        daemon.profile_owner = Some(owner.clone());
        daemon.control_plane_store = Some(control.clone());
        daemon
            .restore_control_plane_snapshot(
                load_control_plane_snapshot(&control)
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        let memory = daemon.memory_handle();
        memory
            .write()
            .await
            .replace_snapshot(load_memory_snapshot(&memory_store).await.unwrap().unwrap());
        let restored_source = memory.read().await.get(&selection.local_id).unwrap();
        assert_eq!(
            serde_json::to_value(&restored_source).unwrap(),
            report["source"]
        );
        let state = Arc::new(RwLock::new(daemon));
        assert!(state.read().await.mcp_manager.is_none());
        let service = PublicationService::open(&state, owner, &root.join("publications.sqlite"))
            .await
            .unwrap();
        let recovered = service.get(agent, &selection.publication_id).await.unwrap();
        let rows = service.list(agent).await.unwrap();
        if phase == "before" {
            assert!(
                recovered.is_none(),
                "a killed pre-admission process must not leave a queued selection"
            );
            assert_eq!(rows["items"], json!([]));
        } else {
            let acknowledged: Publication =
                serde_json::from_value(report["acknowledged"].clone()).unwrap();
            assert_eq!(recovered, Some(acknowledged));
            assert_eq!(rows["items"].as_array().unwrap().len(), 1);
        }
        let admitted = service.admit(agent, selection.clone()).await.unwrap();
        assert_eq!(
            service.admit(agent, selection.clone()).await.unwrap(),
            admitted
        );
        assert_eq!(admitted.admission.operation_id, selection.publication_id);
        assert_eq!(admitted.admission.local_id, restored_source.id);
        assert_eq!(
            admitted.admission.request["record"]["content"],
            restored_source.content
        );
        assert_eq!(admitted.state, PublicationState::Queued);
        assert_eq!(admitted.attempts, 0);
        assert!(admitted.proposal_id.is_none() && admitted.receipt.is_none());
        let rows = service.list(agent).await.unwrap();
        assert_eq!(rows["items"].as_array().unwrap().len(), 1);
        assert_eq!(rows["counts"]["queued"], 1);
        assert_eq!(rows["omitted"], 0);
        assert_eq!(
            memory
                .read()
                .await
                .get(&selection.local_id)
                .unwrap()
                .content,
            restored_source.content
        );
        service.shutdown().await;
    }
}
