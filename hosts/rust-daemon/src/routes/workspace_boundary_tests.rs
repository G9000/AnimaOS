use super::*;
use crate::document_edits::store::test_support::private_directory;
use crate::document_edits::DocumentEditStore;
use crate::state::DaemonState;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Copy, Debug)]
enum Relation {
    Equal,
    Ancestor,
    Descendant,
    MissingDescendant,
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Put,
    Bootstrap,
    Resume,
    Restore,
}

fn private_tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            assert!(!metadata.file_type().is_symlink());
            let name = path.strip_prefix(root).unwrap().to_path_buf();
            if metadata.is_dir() {
                entries.insert(name, None);
                visit(root, &path, entries);
            } else {
                assert!(metadata.is_file());
                entries.insert(name, Some(std::fs::read(path).unwrap()));
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

async fn rejects_private_store_workspace(operation: Operation, relation: Relation) {
    let private = private_directory();
    let safe_workspace = tempfile::tempdir().unwrap();
    std::fs::write(safe_workspace.path().join("public.txt"), b"public fixture").unwrap();
    let store_root = private.path().join("document-edits");
    crate::document_edits::store::initialize_owned_directory(&store_root).unwrap();
    let store = DocumentEditStore::open_existing(&store_root).unwrap();
    let target = match relation {
        Relation::Equal => store_root.clone(),
        Relation::Ancestor => private.path().to_path_buf(),
        Relation::Descendant => {
            let path = store_root.join("nested-workspace");
            std::fs::create_dir(&path).unwrap();
            path
        }
        Relation::MissingDescendant => store_root.join("missing-workspace"),
    };
    // Copied proof bytes retain their source fixture binding; generic preview does not validate it.
    let evidence = crate::document_edits::publication_readback_proof_fixture().await;
    std::fs::write(store_root.join("proof_proposal.json"), &evidence).unwrap();
    if matches!(relation, Relation::Descendant) {
        std::fs::write(target.join("proof_proposal.json"), &evidence).unwrap();
    }
    if matches!(operation, Operation::Resume) {
        std::fs::write(
            target.join("anima.yaml"),
            "name: Boundary fixture\nmission: Keep private storage separate\nmodel: boundary-test\nprovider: deterministic\norchestrator:\n  name: Boundary lead\n  bio: Isolated fixture\n  system: Work only on the fixture\n  tools: [read_file]\n",
        )
        .unwrap();
    }
    let control_plane = private.path().join("control-plane.json");
    let mut daemon = DaemonState::new();
    daemon.document_edit_store = Some(store);
    if matches!(operation, Operation::Put | Operation::Restore) {
        daemon.workspace = Some(WorkspaceConfig {
            root_path: safe_workspace.path().to_path_buf(),
            company_name: "Safe fixture".into(),
            mission: "Keep private storage separate".into(),
            values: vec![],
        });
    }
    daemon.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(control_plane.clone()),
    ));
    let state = Arc::new(RwLock::new(daemon));
    let persist_request = state.write().await.control_plane_persist_request();
    persist_request.save().await.unwrap();
    let before_state = serde_json::to_vec(&state.read().await.control_plane_snapshot()).unwrap();
    let before_tree = private_tree(private.path());
    let workspace = serde_json::json!({
        "rootPath": target,
        "companyName": "Boundary fixture",
        "mission": "Keep private storage separate",
        "values": [],
    });
    let rejected = match operation {
        Operation::Put => handle_put_workspace(serde_json::to_vec(&workspace).unwrap(), &state)
            .await
            .is_err(),
        Operation::Bootstrap => {
            let body = serde_json::json!({
                "workspace": workspace,
                "agent": {
                    "name": "Boundary lead",
                    "presetId": "chief-of-staff",
                    "bio": "Isolated fixture",
                    "system": "Work only on the fixture",
                    "provider": "deterministic",
                    "model": "boundary-test",
                    "tools": ["read_file"],
                },
            });
            handle_bootstrap_workspace(serde_json::to_vec(&body).unwrap(), &state)
                .await
                .is_err()
        }
        Operation::Resume => {
            let body = serde_json::json!({ "rootPath": target });
            handle_resume_workspace(serde_json::to_vec(&body).unwrap(), &state)
                .await
                .is_err()
        }
        Operation::Restore => {
            let mut snapshot = state.read().await.control_plane_snapshot();
            snapshot.workspace = Some(WorkspaceConfig {
                root_path: target,
                company_name: "Boundary fixture".into(),
                mission: "Keep private storage separate".into(),
                values: vec![],
            });
            state
                .write()
                .await
                .restore_control_plane_snapshot(snapshot)
                .is_err()
        }
    };
    let evidence_path = match relation {
        Relation::Ancestor => "document-edits/proof_proposal.json",
        Relation::Equal | Relation::Descendant | Relation::MissingDescendant => {
            "proof_proposal.json"
        }
    };
    let evidence_readable =
        match crate::routes::workspace_files::preview(&state, evidence_path.into()).await {
            Ok(preview) => {
                let preview = serde_json::to_value(preview).unwrap();
                let observed: serde_json::Value =
                    serde_json::from_str(preview["content"].as_str().unwrap()).unwrap();
                assert!(observed["proposal_id"] == "proposal");
                assert!(observed["owner_approval"].is_object());
                assert!(
                    observed == serde_json::from_slice::<serde_json::Value>(&evidence).unwrap(),
                    "generic preview returned different private proof bytes"
                );
                true
            }
            Err(_) => false,
        };
    let evidence_listed = crate::routes::workspace_files::list(&state)
        .await
        .ok()
        .is_some_and(|listing| {
            serde_json::to_string(&listing)
                .unwrap()
                .contains("proof_proposal.json")
        });
    assert!(
        rejected,
        "{operation:?} accepted {relation:?} private storage overlap"
    );
    assert!(
        !evidence_readable,
        "generic workspace preview exposed private fixture evidence"
    );
    assert!(
        !evidence_listed,
        "generic workspace listing exposed private fixture evidence"
    );
    assert!(
        serde_json::to_vec(&state.read().await.control_plane_snapshot()).unwrap() == before_state,
        "rejected {operation:?} changed workspace or agent state"
    );
    assert!(
        private_tree(private.path()) == before_tree,
        "rejected {operation:?} changed persisted state or private staging"
    );
}

#[tokio::test]
async fn workspace_boundary_put_rejects_equal_private_store() {
    rejects_private_store_workspace(Operation::Put, Relation::Equal).await;
}
#[tokio::test]
async fn workspace_boundary_put_rejects_private_store_ancestor() {
    rejects_private_store_workspace(Operation::Put, Relation::Ancestor).await;
}
#[tokio::test]
async fn workspace_boundary_put_rejects_private_store_descendant() {
    rejects_private_store_workspace(Operation::Put, Relation::Descendant).await;
}
#[tokio::test]
async fn workspace_boundary_bootstrap_rejects_equal_private_store() {
    rejects_private_store_workspace(Operation::Bootstrap, Relation::Equal).await;
}
#[tokio::test]
async fn workspace_boundary_bootstrap_rejects_private_store_ancestor() {
    rejects_private_store_workspace(Operation::Bootstrap, Relation::Ancestor).await;
}
#[tokio::test]
async fn workspace_boundary_bootstrap_rejects_private_store_descendant() {
    rejects_private_store_workspace(Operation::Bootstrap, Relation::Descendant).await;
}
#[tokio::test]
async fn workspace_boundary_resume_rejects_equal_private_store() {
    rejects_private_store_workspace(Operation::Resume, Relation::Equal).await;
}
#[tokio::test]
async fn workspace_boundary_resume_rejects_private_store_ancestor() {
    rejects_private_store_workspace(Operation::Resume, Relation::Ancestor).await;
}
#[tokio::test]
async fn workspace_boundary_resume_rejects_private_store_descendant() {
    rejects_private_store_workspace(Operation::Resume, Relation::Descendant).await;
}

#[tokio::test]
async fn workspace_boundary_restore_rejects_equal_private_store() {
    rejects_private_store_workspace(Operation::Restore, Relation::Equal).await;
}
#[tokio::test]
async fn workspace_boundary_restore_rejects_private_store_ancestor() {
    rejects_private_store_workspace(Operation::Restore, Relation::Ancestor).await;
}
#[tokio::test]
async fn workspace_boundary_restore_rejects_private_store_descendant() {
    rejects_private_store_workspace(Operation::Restore, Relation::Descendant).await;
}
#[tokio::test]
async fn workspace_boundary_put_rejects_before_creating_private_store_descendant() {
    rejects_private_store_workspace(Operation::Put, Relation::MissingDescendant).await;
}
#[tokio::test]
async fn workspace_boundary_bootstrap_rejects_before_creating_private_store_descendant() {
    rejects_private_store_workspace(Operation::Bootstrap, Relation::MissingDescendant).await;
}

async fn metadata_workspace_change(operation: Operation, excludes_metadata: bool) {
    let private = private_directory();
    let safe_workspace = tempfile::tempdir().unwrap();
    let staging_parent = private.path().join("staging-area");
    let metadata_parent = private.path().join("work-area");
    std::fs::create_dir(&staging_parent).unwrap();
    std::fs::create_dir(&metadata_parent).unwrap();
    let store_root = staging_parent.join("document-edits");
    crate::document_edits::store::initialize_owned_directory(&store_root).unwrap();
    let store = DocumentEditStore::open_existing(&store_root).unwrap();
    let metadata_file = metadata_parent.join("meeting-notes.json");
    let target = if excludes_metadata {
        let target = metadata_parent.join("team-workspace");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("public.txt"), b"public fixture").unwrap();
        target
    } else {
        metadata_parent
    };
    assert!(!crate::document_edits::workspace_boundary::overlaps(
        store.canonical_root().unwrap(),
        Some(&target),
    )
    .unwrap());
    if matches!(operation, Operation::Resume) {
        std::fs::write(
            target.join("anima.yaml"),
            "name: Metadata boundary fixture\nmission: Keep private reviews separate\nmodel: boundary-test\nprovider: deterministic\norchestrator:\n  name: Boundary lead\n  bio: Isolated fixture\n  system: Work only on the fixture\n  tools: [read_file]\n",
        )
        .unwrap();
    }
    let mut daemon = DaemonState::new();
    daemon.document_edit_store = Some(store);
    daemon.document_edits = crate::document_edits::metadata_review_registry_fixture().await;
    if matches!(operation, Operation::Put | Operation::Restore) {
        daemon.workspace = Some(WorkspaceConfig {
            root_path: safe_workspace.path().to_path_buf(),
            company_name: "Safe fixture".into(),
            mission: "Keep private reviews separate".into(),
            values: vec![],
        });
    }
    daemon.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(metadata_file.clone()),
    ));
    let state = Arc::new(RwLock::new(daemon));
    let request = state.write().await.control_plane_persist_request();
    request.save().await.unwrap();
    let persisted: crate::control_plane_store::ControlPlaneSnapshot =
        serde_json::from_slice(&std::fs::read(&metadata_file).unwrap()).unwrap();
    let review = &persisted
        .document_edits
        .get("proposal")
        .unwrap()
        .draft
        .content_review;
    assert!(review.changes[0].before == "Private fixture original paragraph");
    assert!(review.changes[0].after == "Private fixture revised paragraph");
    assert_eq!(
        metadata_file
            .canonicalize()
            .unwrap()
            .starts_with(target.canonicalize().unwrap()),
        !excludes_metadata,
    );
    let before_state = serde_json::to_vec(&state.read().await.control_plane_snapshot()).unwrap();
    let before_tree = private_tree(private.path());
    let before_staging = private_tree(&staging_parent);
    let workspace = serde_json::json!({
        "rootPath": target,
        "companyName": "Metadata boundary fixture",
        "mission": "Keep private reviews separate",
        "values": [],
    });
    let accepted = match operation {
        Operation::Put => handle_put_workspace(serde_json::to_vec(&workspace).unwrap(), &state)
            .await
            .is_ok(),
        Operation::Bootstrap => {
            let body = serde_json::json!({
                "workspace": workspace,
                "agent": {
                    "name": "Boundary lead",
                    "presetId": "chief-of-staff",
                    "bio": "Isolated fixture",
                    "system": "Work only on the fixture",
                    "provider": "deterministic",
                    "model": "boundary-test",
                    "tools": ["read_file"],
                },
            });
            handle_bootstrap_workspace(serde_json::to_vec(&body).unwrap(), &state)
                .await
                .is_ok()
        }
        Operation::Resume => {
            let body = serde_json::json!({ "rootPath": target });
            handle_resume_workspace(serde_json::to_vec(&body).unwrap(), &state)
                .await
                .is_ok()
        }
        Operation::Restore => {
            let mut snapshot = state.read().await.control_plane_snapshot();
            snapshot.workspace = Some(WorkspaceConfig {
                root_path: target.clone(),
                company_name: "Metadata boundary fixture".into(),
                mission: "Keep private reviews separate".into(),
                values: vec![],
            });
            state
                .write()
                .await
                .restore_control_plane_snapshot(snapshot)
                .is_ok()
        }
    };
    let metadata_readable =
        match crate::routes::workspace_files::preview(&state, "meeting-notes.json".into()).await {
            Ok(preview) => {
                let preview = serde_json::to_value(preview).unwrap();
                let observed: crate::control_plane_store::ControlPlaneSnapshot =
                    serde_json::from_str(preview["content"].as_str().unwrap()).unwrap();
                let change = &observed
                    .document_edits
                    .get("proposal")
                    .unwrap()
                    .draft
                    .content_review
                    .changes[0];
                assert!(change.before == "Private fixture original paragraph");
                assert!(change.after == "Private fixture revised paragraph");
                true
            }
            Err(_) => false,
        };
    let metadata_listed = crate::routes::workspace_files::list(&state)
        .await
        .ok()
        .is_some_and(|listing| {
            serde_json::to_string(&listing)
                .unwrap()
                .contains("meeting-notes.json")
        });
    assert_eq!(
        accepted, excludes_metadata,
        "{operation:?} did not enforce the configured metadata file boundary"
    );
    assert!(
        !metadata_readable,
        "generic preview exposed private review text"
    );
    assert!(
        !metadata_listed,
        "generic listing exposed configured metadata"
    );
    assert!(
        private_tree(&staging_parent) == before_staging,
        "workspace change altered separate private staging"
    );
    if excludes_metadata {
        let guard = state.read().await;
        assert!(guard.workspace.as_ref().unwrap().root_path == target.canonicalize().unwrap());
        drop(guard);
        let public = crate::routes::workspace_files::preview(&state, "public.txt".into())
            .await
            .unwrap();
        assert!(serde_json::to_value(public).unwrap()["content"] == "public fixture");
    } else {
        assert!(
            serde_json::to_vec(&state.read().await.control_plane_snapshot()).unwrap()
                == before_state,
            "rejected {operation:?} changed workspace, agents, or document metadata"
        );
        assert!(
            private_tree(private.path()) == before_tree,
            "rejected {operation:?} changed metadata or private staging"
        );
    }
}

#[tokio::test]
async fn workspace_boundary_metadata_put_rejects_workspace_containing_configured_file() {
    metadata_workspace_change(Operation::Put, false).await;
}
#[tokio::test]
async fn workspace_boundary_metadata_bootstrap_rejects_workspace_containing_configured_file() {
    metadata_workspace_change(Operation::Bootstrap, false).await;
}
#[tokio::test]
async fn workspace_boundary_metadata_resume_rejects_workspace_containing_configured_file() {
    metadata_workspace_change(Operation::Resume, false).await;
}
#[tokio::test]
async fn workspace_boundary_metadata_restore_rejects_workspace_containing_configured_file() {
    metadata_workspace_change(Operation::Restore, false).await;
}
#[tokio::test]
async fn workspace_boundary_metadata_allows_subdirectory_excluding_configured_file() {
    metadata_workspace_change(Operation::Put, true).await;
}
