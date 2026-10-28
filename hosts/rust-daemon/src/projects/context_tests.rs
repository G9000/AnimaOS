use super::*;
use crate::projects::records::ProjectMutationReplay;
use crate::projects::{
    ProjectCreateRequest, ProjectDonorBinding, ProjectPatchRequest, ProjectResourceRole,
};
use crate::runs::{RunSource, RunStart};
use crate::sessions::{
    test_support::{agent_config, message, seed_messages},
    TitleSource,
};

fn fixture() -> (DaemonState, String, String, String) {
    let mut state = DaemonState::new();
    let a = state.create_agent(agent_config("Alice")).unwrap().state.id;
    let b = state.create_agent(agent_config("Bob")).unwrap().state.id;
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "context-fixture".into(),
                title: "One shared project".into(),
                member_agent_ids: vec![a.clone(), b.clone()],
                instructions: "Use selected evidence".into(),
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
            "Project chat".into(),
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
    (state, a, b, project.id)
}
fn run(state: &DaemonState, agent: &str, session: &str) -> RunRecord {
    let mut run = RunRecord::running(
        RunStart {
            agent_id: agent.into(),
            session_id: session.into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "Continue this project".into(),
            model: "test".into(),
            provider: None,
            parent_run_id: None,
        },
        3,
    );
    run.project_witness = Some(capture_run_witness(state, agent, session).unwrap());
    run
}
fn pin(
    state: &mut DaemonState,
    project_id: &str,
    owner: &str,
    session_id: &str,
    ids: &[&str],
) -> String {
    let project = state.projects.get(project_id).unwrap();
    let resource = ProjectResourceRecord {
        id: format!("resource_{}", uuid::Uuid::new_v4()),
        target: ProjectResourceTarget::ChatExcerpt {
            owner_agent_id: owner.into(),
            session_id: session_id.into(),
            clear_generation: 0,
            message_ids: ids.iter().map(|id| id.to_string()).collect(),
        },
        role: ProjectResourceRole::Reference,
        label: Some("Exact owner selection".into()),
        use_in_context: true,
        created_at_ms: 2,
        add_replay: ProjectMutationReplay {
            request_key: format!("pin-{}", uuid::Uuid::new_v4()),
            request_sha256: text_digest("pin"),
        },
        donor: Some(ProjectDonorBinding {
            agent_id: owner.into(),
            member_generation: project.member(owner).unwrap().generation,
        }),
        source_project_binding_revision: Some(1),
        workspace_key: None,
    };
    let id = resource.id.clone();
    state
        .projects
        .add_resource(project_id, project.revision, resource, 2)
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    id
}

#[test]
fn recovery_review_is_limited_to_revision_only_changes_in_the_same_grant() {
    let (mut state, a, b, id) = fixture();
    let original = run(&state, &a, "chat:a");
    assert_eq!(
        context_change_summary(&state, &original),
        ProjectContextChange::Unchanged
    );
    state
        .projects
        .patch(
            &id,
            ProjectPatchRequest {
                expected_revision: 1,
                title: Some("Revised title".into()),
                instructions: None,
                status: None,
            },
            4,
        )
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    assert_eq!(
        context_change_summary(&state, &original),
        ProjectContextChange::ReviewRequired(ProjectChangeReason::ProjectRevisionChanged)
    );
    assert_eq!(
        current_authorized_witness(&state, &original)
            .unwrap()
            .project
            .unwrap()
            .project_revision,
        2
    );
    state
        .projects
        .replace_members(&id, 2, vec![a.clone()], 5)
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    assert_eq!(
        context_change_summary(&state, &original),
        ProjectContextChange::Unavailable(ProjectChangeReason::AccessEpochChanged)
    );
    assert_eq!(
        current_authorized_witness(&state, &original),
        Err(ProjectError::AccessRevoked)
    );
    state
        .projects
        .replace_members(&id, 3, vec![a.clone(), b], 6)
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    assert!(current_authorized_witness(&state, &original).is_err());
}

#[tokio::test]
async fn only_pinned_exact_donor_messages_are_read_and_old_grants_do_not_revive() {
    let (mut state, a, b, id) = fixture();
    seed_messages(
        &mut state,
        &a,
        vec![
            message(
                &a,
                "hidden-a",
                "chat:a",
                MessageRole::User,
                "Unpinned private material",
                1,
            ),
            message(
                &a,
                "chosen-a",
                "chat:a",
                MessageRole::Assistant,
                "Selected donor evidence",
                2,
            ),
            message(
                &a,
                "neighbor-a",
                "chat:a",
                MessageRole::User,
                "Unselected neighboring material",
                3,
            ),
        ],
    );
    pin(&mut state, &id, &a, "chat:a", &["chosen-a"]);
    let turn = run(&state, &b, "chat:b");
    let scope = capture_context_scope(&state, &turn).unwrap().unwrap();
    let shared = Arc::new(tokio::sync::RwLock::new(state));
    let loaded = load_context(
        &shared,
        &scope,
        ProjectRenderBudget::from_remaining(20_000, 1_000),
        CancelSignal::new(),
        Instant::now() + std::time::Duration::from_secs(2),
    )
    .await
    .unwrap();
    let accepted = recheck_context(&*shared.read().await, &turn, &scope, loaded).unwrap();
    assert!(accepted.text().contains("Selected donor evidence"));
    assert!(!accepted.text().contains("Unpinned private material"));
    assert!(!accepted.text().contains("Unselected neighboring material"));
    {
        let mut state = shared.write().await;
        let revision = state.projects.get(&id).unwrap().revision;
        state
            .projects
            .replace_members(&id, revision, vec![b.clone()], 4)
            .unwrap();
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
        assert!(accepted.lease().check().is_err());
        let revision = state.projects.get(&id).unwrap().revision;
        state
            .projects
            .replace_members(&id, revision, vec![a, b.clone()], 5)
            .unwrap();
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
    }
    let state = shared.read().await;
    let fresh = run(&state, &b, "chat:b");
    let fresh_scope = capture_context_scope(&state, &fresh).unwrap().unwrap();
    drop(state);
    let loaded = load_context(
        &shared,
        &fresh_scope,
        ProjectRenderBudget::from_remaining(20_000, 1_000),
        CancelSignal::new(),
        Instant::now() + std::time::Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert!(!loaded.text.contains("Selected donor evidence"));
    assert_eq!(
        loaded.manifest.sources[0].state,
        ProjectSourceState::AccessRemoved
    );
}

#[tokio::test]
async fn own_chat_deduplication_uses_only_selected_history_and_stays_frozen_for_compaction() {
    let (mut state, a, _, id) = fixture();
    let first = message(
        &a,
        "selected-first",
        "chat:a",
        MessageRole::User,
        "Already selected in native history",
        1,
    );
    let second = message(
        &a,
        "selected-second",
        "chat:a",
        MessageRole::Assistant,
        "Pinned but absent from native selection",
        2,
    );
    seed_messages(&mut state, &a, vec![first.clone(), second]);
    pin(
        &mut state,
        &id,
        &a,
        "chat:a",
        &["selected-first", "selected-second"],
    );
    let turn = run(&state, &a, "chat:a");
    let scope = capture_context_scope(&state, &turn).unwrap().unwrap();
    let shared = Arc::new(tokio::sync::RwLock::new(state));
    let loaded = load_context(
        &shared,
        &scope,
        ProjectRenderBudget::from_remaining(20_000, 1_000),
        CancelSignal::new(),
        Instant::now() + std::time::Duration::from_secs(2),
    )
    .await
    .unwrap();
    let accepted = recheck_context(&*shared.read().await, &turn, &scope, loaded)
        .unwrap()
        .for_selected_history(&[first]);
    assert!(!accepted
        .text()
        .contains("Already selected in native history"));
    assert!(accepted
        .text()
        .contains("Pinned but absent from native selection"));
    let compacted = accepted.for_selected_history(&[]);
    assert_eq!(accepted.text(), compacted.text());
    assert_eq!(accepted.manifest(), compacted.manifest());
    let mut state = shared.write().await;
    let revision = state.projects.get(&id).unwrap().revision;
    let source = state.projects.get(&id).unwrap().resources[0].id.clone();
    state
        .projects
        .remove_resource(&id, revision, &source, 6)
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    assert!(accepted.lease().check().is_err());
}

#[test]
fn missing_queued_witness_on_a_project_chat_is_unavailable() {
    let (state, a, _, _) = fixture();
    let mut turn = run(&state, &a, "chat:a");
    turn.project_witness = None;
    assert_eq!(
        context_change_summary(&state, &turn),
        ProjectContextChange::Unavailable(ProjectChangeReason::MissingWitness)
    );
    assert!(current_authorized_witness(&state, &turn).is_err());
}
