use super::{capture, check, choices};
use crate::connected_search::{providers::document, source_authority, DocumentKind, SourceIndex};
use crate::connectors::drive::model::{DriveFileId, DriveRecord, DriveStatus};
use crate::projects::{
    ProjectCreateRequest, ProjectMutation, ProjectResourceRole, ProjectResourceTarget,
    ProjectService,
};
use crate::state::DaemonState;
use std::sync::Arc;

fn fixture() -> (
    DaemonState,
    String,
    String,
    anima_document_store_sqlite::connected::ConnectedSourceGrant,
) {
    let mut state = DaemonState::new();
    let owner = state
        .create_agent(crate::sessions::test_support::agent_config("Account owner"))
        .unwrap()
        .state
        .id;
    let reader = state
        .create_agent(crate::sessions::test_support::agent_config(
            "Project reader",
        ))
        .unwrap()
        .state
        .id;
    let tools = state
        .tool_registry
        .resolve_descriptors(["drive_read"])
        .unwrap();
    state
        .update_agent(
            &owner,
            anima_core::AgentConfigUpdate {
                tools: Some(tools),
                ..Default::default()
            },
        )
        .unwrap();
    state.drive_records.insert(
        "drive-source".into(),
        DriveRecord {
            id: "drive-source".into(),
            agent_id: owner.clone(),
            status: DriveStatus::Active,
            generation: "native-generation".into(),
            selected_file_ids: vec![DriveFileId::parse("selected").unwrap()],
            account_permission_id: Some("account".into()),
            account_label: Some("owner@example.test".into()),
            oauth_config_revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
            cleanup_generations: Vec::new(),
        },
    );
    let authority = source_authority(&state, &owner, "drive-source").unwrap();
    let mut index = SourceIndex::new(&authority);
    index.enabled = true;
    let mut selected = document(
        DocumentKind::File,
        "selected".into(),
        "selected".into(),
        "Annual report".into(),
        "private body never belongs in picker".into(),
    );
    selected.citation_id = "a".repeat(64);
    index.documents.push(selected);
    state
        .connected_search
        .sources
        .insert("drive-source".into(), index);
    let grant = capture(&state, &owner, "drive-source", &"a".repeat(64)).unwrap();
    (state, owner, reader, grant)
}

#[test]
fn account_generation_selection_and_enrollment_each_fence_old_grants() {
    let (mut state, _, _, grant) = fixture();
    assert!(check(&state, &grant).is_ok());
    let old = state.connected_search.sources["drive-source"]
        .grant_epoch
        .clone();
    state
        .connected_search
        .sources
        .get_mut("drive-source")
        .unwrap()
        .grant_epoch = "replacement-enrollment".into();
    assert!(
        check(&state, &grant).is_err(),
        "enabling search again cannot resurrect a prior project grant"
    );
    state
        .connected_search
        .sources
        .get_mut("drive-source")
        .unwrap()
        .grant_epoch = old;
    state
        .drive_records
        .get_mut("drive-source")
        .unwrap()
        .generation = "replacement-connection".into();
    assert!(check(&state, &grant).is_err());
    state
        .drive_records
        .get_mut("drive-source")
        .unwrap()
        .generation = grant.connection_generation.clone();
    state
        .drive_records
        .get_mut("drive-source")
        .unwrap()
        .selected_file_ids
        .clear();
    assert!(
        check(&state, &grant).is_err(),
        "removing the selected remote ID fences derived content"
    );
}

#[tokio::test]
async fn picker_is_member_bounded_and_never_returns_account_document_bodies() {
    let (mut state, owner, reader, _) = fixture();
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "project".into(),
                title: "Shared".into(),
                member_agent_ids: vec![owner, reader.clone()],
                instructions: String::new(),
            },
            1,
        )
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    let state = Arc::new(tokio::sync::RwLock::new(state));
    let result = choices(&state, &project.id, &reader, "Annual")
        .await
        .unwrap();
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(serialized.contains("Annual report"));
    assert!(serialized.contains("owner@example.test"));
    assert!(!serialized.contains("private body"));
    assert!(choices(&state, &project.id, "outsider", "").await.is_err());
}

#[tokio::test]
async fn ordinary_resource_admission_cannot_forge_a_selected_account_grant() {
    let (mut state, owner, reader, grant) = fixture();
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "project".into(),
                title: "Shared".into(),
                member_agent_ids: vec![owner, reader.clone()],
                instructions: String::new(),
            },
            1,
        )
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    let state = Arc::new(tokio::sync::RwLock::new(state));
    let service = ProjectService::new(state.clone(), Arc::new(tokio::sync::Mutex::new(())));
    let result = service
        .mutate(ProjectMutation::AddResource {
            id: project.id.clone(),
            input: crate::projects::AddResourceRequest {
                expected_revision: project.revision,
                request_key: "forged".into(),
                as_agent_id: reader,
                target: ProjectResourceTarget::ConnectedSource { grant },
                role: ProjectResourceRole::Reference,
                label: None,
                use_in_context: true,
            },
        })
        .await;
    assert!(result.is_err());
    assert!(state
        .read()
        .await
        .projects
        .get(&project.id)
        .unwrap()
        .resources
        .is_empty());
}

#[tokio::test]
async fn persisted_account_revocation_immediately_fences_old_project_runs_without_starving_new_ones(
) {
    let (mut state, owner, reader, grant) = fixture();
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "project".into(),
                title: "Shared".into(),
                member_agent_ids: vec![owner.clone(), reader.clone()],
                instructions: String::new(),
            },
            1,
        )
        .unwrap();
    let donor = crate::projects::ProjectDonorBinding {
        agent_id: owner,
        member_generation: project
            .members
            .iter()
            .find(|m| m.agent_id == grant.owner_agent_id)
            .unwrap()
            .generation,
    };
    let resource = crate::projects::ProjectResourceRecord {
        id: format!("resource_{}", uuid::Uuid::new_v4()),
        target: ProjectResourceTarget::ConnectedSource { grant },
        role: ProjectResourceRole::Reference,
        label: Some("Annual report".into()),
        use_in_context: true,
        created_at_ms: 2,
        add_replay: crate::projects::records::ProjectMutationReplay {
            request_key: "selected".into(),
            request_sha256: "a".repeat(64),
        },
        donor: Some(donor),
        source_project_binding_revision: None,
        workspace_key: None,
    };
    let project = state
        .projects
        .add_resource(&project.id, project.revision, resource, 2)
        .unwrap();
    state
        .project_access
        .publish_records(&state.projects)
        .unwrap();
    let old = state.project_access.acquire_run(&project, &reader).unwrap();
    state.drive_records.get_mut("drive-source").unwrap().status = DriveStatus::Disconnected;
    let _snapshot = state.control_plane_persist_request();
    assert!(
        old.check().is_err(),
        "the real account persistence hook must close already admitted project runs"
    );
    let healthy = state.project_access.acquire_run(&project, &reader).unwrap();
    let _snapshot = state.control_plane_persist_request();
    assert!(
        healthy.check().is_ok(),
        "later unrelated saves must not cancel a fresh turn again for the same unavailable source"
    );
    assert!(
        old.check().is_err(),
        "fresh project leases do not revive old ones"
    );
}

#[test]
fn removing_and_readding_account_owner_cannot_revive_a_previous_donor_grant() {
    let (mut state, owner, reader, grant) = fixture();
    let (project, _) = state
        .projects
        .create(
            ProjectCreateRequest {
                request_key: "project".into(),
                title: "Shared".into(),
                member_agent_ids: vec![owner.clone(), reader.clone()],
                instructions: String::new(),
            },
            1,
        )
        .unwrap();
    let resource = crate::projects::ProjectResourceRecord {
        id: format!("resource_{}", uuid::Uuid::new_v4()),
        target: ProjectResourceTarget::ConnectedSource {
            grant: grant.clone(),
        },
        role: ProjectResourceRole::Reference,
        label: None,
        use_in_context: true,
        created_at_ms: 2,
        add_replay: crate::projects::records::ProjectMutationReplay {
            request_key: "selected".into(),
            request_sha256: "a".repeat(64),
        },
        donor: Some(crate::projects::ProjectDonorBinding {
            agent_id: owner.clone(),
            member_generation: project.member(&owner).unwrap().generation,
        }),
        source_project_binding_revision: None,
        workspace_key: None,
    };
    assert!(
        crate::projects::context::validate_resource_access(&state, &project, &resource).is_ok()
    );
    let project = state
        .projects
        .add_resource(&project.id, project.revision, resource.clone(), 2)
        .unwrap();
    let removed = state
        .projects
        .replace_members(&project.id, project.revision, vec![reader.clone()], 3)
        .unwrap();
    let readded = state
        .projects
        .replace_members(&project.id, removed.revision, vec![owner, reader], 4)
        .unwrap();
    assert!(
        check(&state, &grant).is_ok(),
        "the original native account remains active throughout"
    );
    assert!(
        crate::projects::context::validate_resource_access(&state, &readded, &resource).is_err(),
        "new project membership cannot revive an older explicit donor grant"
    );
}
