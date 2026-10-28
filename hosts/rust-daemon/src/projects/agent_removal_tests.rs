use super::*;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::projects::{
    capture_run_witness, member_read_lease, restore_projects, ProjectCreateRequest,
};
use crate::sessions::{
    test_support::agent_config, SessionKind, SessionOrigin, SessionRecord, TitleSource,
};

fn fixture() -> (DaemonState, String, String, String, String) {
    let mut state = DaemonState::new();
    let a = state.create_agent(agent_config("Alice")).unwrap().state.id;
    let b = state.create_agent(agent_config("Bob")).unwrap().state.id;
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "agent-lifecycle".into(),
                title: "Retained project".into(),
                member_agent_ids: vec![a.clone(), b.clone()],
                instructions: "Retained brief".into(),
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
            "Retained native chat".into(),
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
    let mut run = crate::runs::RunRecord::running(
        crate::runs::RunStart {
            agent_id: b.clone(),
            session_id: "chat:b".into(),
            source: crate::runs::RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "Active shared task".into(),
            model: "test".into(),
            provider: None,
            parent_run_id: None,
        },
        2,
    );
    run.project_witness = Some(capture_run_witness(&state, &b, "chat:b").unwrap());
    let run_id = run.id.clone();
    state.runs.insert(run);
    (state, a, b, project.id, run_id)
}

#[tokio::test]
async fn acknowledged_agent_removal_preserves_project_receipt_remaining_member_and_deleted_sessions(
) {
    let temp = tempfile::tempdir().unwrap();
    let config = ControlPlaneStoreConfig::Json(temp.path().join("control-plane.json"));
    let (mut state, a, b, id, run_id) = fixture();
    state.set_control_plane_store(Some(config.clone()));
    let before = state.projects.get(&id).unwrap().clone();
    let receipts = state.projects.creation_receipts_snapshot();
    let lease = member_read_lease(&state, &id, &b).unwrap();
    let candidate = ProjectAgentRemovalCandidate::prepare(&mut state, &a, 3).unwrap();
    assert_eq!(
        state.projects.get(&id),
        Some(&before),
        "prepare cannot publish a member change"
    );
    assert!(lease.check().is_ok());
    assert!(state.runs.get(&run_id).unwrap().stop.is_some());
    state.remove_agent(&a);
    state.sessions.remove_for_agent(&a);
    candidate
        .persistence_request(&mut state)
        .unwrap()
        .save_project_candidate()
        .await
        .unwrap();
    candidate.publish(&mut state).unwrap();
    assert!(lease.check().is_err());
    assert!(!state
        .projects
        .get(&id)
        .unwrap()
        .members
        .iter()
        .any(|member| member.agent_id == a));
    assert_eq!(
        state
            .projects
            .get(&id)
            .unwrap()
            .member(&b)
            .unwrap()
            .generation,
        before.member(&b).unwrap().generation
    );
    assert_eq!(state.projects.creation_receipts_snapshot(), receipts);
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert!(!saved.sessions.iter().any(|session| session.agent_id == a));
    assert_eq!(saved.projects, state.projects.snapshot());
    assert_eq!(saved.project_creation_receipts, receipts);
    restore_projects(saved.projects, saved.project_creation_receipts, &state).unwrap();
}

#[test]
fn known_failure_rolls_back_project_stops_without_publishing_or_fencing() {
    let (mut state, a, b, id, run_id) = fixture();
    let original = state.projects.get(&id).unwrap().clone();
    let lease = member_read_lease(&state, &id, &b).unwrap();
    let candidate = ProjectAgentRemovalCandidate::prepare(&mut state, &a, 3).unwrap();
    candidate.rollback(&mut state);
    assert_eq!(state.projects.get(&id), Some(&original));
    assert!(state.runs.get(&run_id).unwrap().stop.is_none());
    assert!(lease.check().is_ok());
}

#[test]
fn uncertain_agent_removal_closes_all_old_member_access_and_keeps_stop_requests() {
    let (mut state, a, b, id, run_id) = fixture();
    let lease = member_read_lease(&state, &id, &b).unwrap();
    let candidate = ProjectAgentRemovalCandidate::prepare(&mut state, &a, 3).unwrap();
    candidate.fence_uncertain(&state);
    assert!(lease.check().is_err());
    assert!(member_read_lease(&state, &id, &b).is_err());
    assert!(state.runs.get(&run_id).unwrap().stop.is_some());
}

#[test]
fn removing_the_final_agent_keeps_zero_member_project_and_original_creation_receipt() {
    let (mut state, a, b, id, _) = fixture();
    let receipts = state.projects.creation_receipts_snapshot();
    ProjectAgentRemovalCandidate::prepare(&mut state, &a, 3)
        .unwrap()
        .publish(&mut state)
        .unwrap();
    ProjectAgentRemovalCandidate::prepare(&mut state, &b, 4)
        .unwrap()
        .publish(&mut state)
        .unwrap();
    assert!(state.projects.get(&id).unwrap().members.is_empty());
    assert_eq!(state.projects.creation_receipts_snapshot(), receipts);
}
