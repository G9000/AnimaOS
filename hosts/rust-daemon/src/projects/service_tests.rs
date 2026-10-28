use super::*;
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::sessions::{
    test_support::agent_config, SessionKind, SessionOrigin, SessionRecord, TitleSource,
};

fn failing_state(path: &std::path::Path) -> (DaemonState, String, String) {
    let mut state = DaemonState::new();
    let a = state.create_agent(agent_config("Alice")).unwrap().state.id;
    let b = state.create_agent(agent_config("Bob")).unwrap().state.id;
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.to_owned())));
    (state, a, b)
}

#[tokio::test]
async fn failed_create_save_publishes_neither_project_nor_creation_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let (state, a, b) = failing_state(temp.path());
    let state = Arc::new(tokio::sync::RwLock::new(state));
    let service = ProjectService::new(state.clone(), Arc::new(Mutex::new(())));
    let result = service
        .mutate(ProjectMutation::Create(ProjectCreateRequest {
            request_key: "failed-create".into(),
            title: "Not acknowledged".into(),
            member_agent_ids: vec![a, b],
            instructions: String::new(),
        }))
        .await;
    assert!(matches!(result, Err(ProjectError::Persistence(_))));
    let state = state.read().await;
    assert!(state.projects.snapshot().is_empty());
    assert!(state.projects.creation_receipts_snapshot().is_empty());
}

#[tokio::test]
async fn known_failed_membership_save_keeps_old_grants_binding_and_run_authority() {
    let temp = tempfile::tempdir().unwrap();
    let (mut state, a, b) = failing_state(temp.path());
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "saved-project".into(),
                title: "Saved".into(),
                member_agent_ids: vec![a.clone(), b.clone()],
                instructions: String::new(),
            },
            1,
        )
        .unwrap();
    for (agent, room) in [(&a, "chat:a"), (&b, "chat:b")] {
        let mut session = SessionRecord::new(
            agent,
            room,
            SessionKind::Chat,
            SessionOrigin::Web,
            "Native".into(),
            TitleSource::Owner,
            1,
        );
        session.project_id = Some(project.id.clone());
        session.project_binding_revision = 1;
        state.sessions.insert(session);
    }
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    let lease = super::super::member_read_lease(&state, &project.id, &b).unwrap();
    let mut run = crate::runs::RunRecord::running(
        crate::runs::RunStart {
            agent_id: b.clone(),
            session_id: "chat:b".into(),
            source: crate::runs::RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "Work".into(),
            model: "test".into(),
            provider: None,
            parent_run_id: None,
        },
        2,
    );
    run.project_witness = Some(super::super::capture_run_witness(&state, &b, "chat:b").unwrap());
    let run_id = run.id.clone();
    state.runs.insert(run);
    let state = Arc::new(tokio::sync::RwLock::new(state));
    let service = ProjectService::new(state.clone(), Arc::new(Mutex::new(())));
    let result = service
        .mutate(ProjectMutation::Members {
            id: project.id.clone(),
            expected_revision: 1,
            agents: vec![b],
        })
        .await;
    assert!(matches!(result, Err(ProjectError::Persistence(_))));
    let state = state.read().await;
    assert_eq!(state.projects.get(&project.id), Some(&project));
    assert_eq!(
        state
            .sessions
            .get(&a, "chat:a")
            .unwrap()
            .project_id
            .as_deref(),
        Some(project.id.as_str())
    );
    assert!(state.runs.get(&run_id).unwrap().stop.is_none());
    assert!(
        lease.check().is_ok(),
        "a known failed save must not revoke the still-authorized old publication"
    );
}
