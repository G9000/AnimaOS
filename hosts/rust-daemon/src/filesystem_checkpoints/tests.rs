use super::*;
use anima_core::filesystem_checkpoints::{
    checkpoint_digest, checkpoint_scope_digest, PathSemantics,
};
use cap_fs_ext::MetadataExt;
use cap_std::{ambient_authority, fs::Dir};
use std::{fs, path::Path};

struct Fixture {
    _temp: tempfile::TempDir,
    workspace: PathBuf,
    store: PathBuf,
    policy: CapturePolicy,
}

fn domain_digest(domain: &[u8], value: &impl Serialize) -> String {
    let mut bytes = domain.to_vec();
    bytes.extend(serde_json::to_vec(value).unwrap());
    checkpoint_digest(&bytes)
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        {
            // The runner's TEMP can carry sandbox/package/shared-group ACEs.
            // The host contract requires an authentic private parent. Establish
            // that precondition only on this fresh empty fixture-owned root;
            // never broaden production policy to accept ambient trustees.
            use win_custody::{current_user_sid_string, SecurityDescriptor};
            let sid = current_user_sid_string().unwrap();
            SecurityDescriptor::from_sddl(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"))
                .unwrap()
                .apply_to_tree(temp.path(), true)
                .expect("fresh empty fixture root gets the declared private-parent ACL");
        }
        let workspace = temp.path().join("workspace");
        let store = temp.path().join("private-store");
        fs::create_dir(&workspace).unwrap();
        let fixture = Self {
            _temp: temp,
            workspace,
            store,
            policy: CapturePolicy {
                profile: CaptureProfile::ContentBasicV1,
                excluded_prefixes: vec![".env".into(), ".git".into(), "node_modules".into()],
                limits: CaptureLimits::default(),
            },
        };
        CheckpointStore::provision_private_root(fixture.config())
            .expect("fixture provisions an actual private store before capture");
        fixture
    }

    fn config(&self) -> CaptureConfig {
        let directory = Dir::open_ambient_dir(&self.workspace, ambient_authority()).unwrap();
        let metadata = directory.dir_metadata().unwrap();
        CaptureConfig {
            workspace_root: self.workspace.clone(),
            store_root: self.store.clone(),
            scope: CheckpointScope {
                owner_id: "owner-a".into(),
                workspace_id: "workspace-a".into(),
                environment_id: "private-test".into(),
                path_semantics: if cfg!(windows) {
                    PathSemantics::WindowsInsensitive
                } else {
                    PathSemantics::PosixSensitive
                },
                root_identity_digest: domain_digest(
                    b"anima-filesystem-checkpoint-root-v1\0",
                    &(metadata.dev(), metadata.ino()),
                ),
                policy_digest: domain_digest(
                    b"anima-filesystem-checkpoint-capture-policy-v1\0",
                    &self.policy,
                ),
            },
            policy: self.policy.clone(),
        }
    }

    fn open(&self) -> CheckpointStore {
        CheckpointStore::new(self.config())
    }

    fn write(&self, path: &str, bytes: &[u8]) {
        let target = self.workspace.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }

    fn object_path(&self, hash: &str) -> PathBuf {
        let scope = checkpoint_scope_digest(&self.config().scope);
        self.store
            .join("scopes")
            .join(scope)
            .join("objects")
            .join(&hash[..2])
            .join(format!("{hash}.blob"))
    }
}

fn paths(paths: &[&str]) -> CaptureSelection {
    CaptureSelection::Paths {
        paths: paths.iter().map(|path| (*path).into()).collect(),
    }
}

fn entry<'a>(record: &'a CaptureRecord, path: &str) -> &'a CaptureState {
    &record
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .unwrap()
        .state
}

fn actual_basic_mode(path: &Path) -> BasicMode {
    let metadata = fs::metadata(path).unwrap();
    #[cfg(windows)]
    {
        BasicMode::Windows {
            readonly: metadata.permissions().readonly(),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        BasicMode::Posix {
            mode: metadata.permissions().mode() & 0o7777,
        }
    }
}

#[test]
fn fp08_host_capture_exact_basic_content() {
    let fixture = Fixture::new();
    let binary = b"\xff\0private\r\nbytes\n";
    fixture.write("binary.bin", binary);
    fixture.write("empty", b"");
    let original_mode = actual_basic_mode(&fixture.workspace.join("binary.bin"));
    let mut store = fixture.open();
    let record = store
        .capture(paths(&["binary.bin", "empty", "missing"]))
        .expect("real bounded content capture");
    assert_eq!(record.version, 1);
    assert_eq!(record.sequence, 1);
    assert_eq!(
        record.scope_digest,
        checkpoint_scope_digest(&fixture.config().scope)
    );
    assert_eq!(record.profile, CaptureProfile::ContentBasicV1);
    assert_eq!(record.content_bytes, binary.len() as u64);
    assert_eq!(
        entry(&record, "binary.bin"),
        &CaptureState::File {
            content: BlobReference {
                sha256: checkpoint_digest(binary),
                byte_count: binary.len() as u64
            },
            basic_mode: original_mode.clone(),
        }
    );
    assert!(
        matches!(entry(&record, "empty"), CaptureState::File { content, .. } if content.byte_count == 0)
    );
    assert_eq!(entry(&record, "missing"), &CaptureState::Absent {});
    assert_eq!(
        store
            .read_content(&record.checkpoint_id, "binary.bin")
            .unwrap(),
        binary
    );
    assert_eq!(
        store.read_content(&record.checkpoint_id, "empty").unwrap(),
        b""
    );
    assert_eq!(
        fs::read(fixture.workspace.join("binary.bin")).unwrap(),
        binary
    );
    assert_eq!(
        actual_basic_mode(&fixture.workspace.join("binary.bin")),
        original_mode
    );
    assert!(!fixture.workspace.join("missing").exists());
    assert!(serde_json::from_value::<CaptureState>(
        serde_json::json!({"kind":"absent","bytes":[]})
    )
    .is_err());
}

#[test]
fn fp08_host_capture_declared_exclusions_preserve_dirty_sentinels() {
    let fixture = Fixture::new();
    // Deliberately fake Git bytes: this tests preservation, not Git acceptance.
    fixture.write(".git/index", b"dirty-index-sentinel");
    fixture.write(".git/HEAD", b"fake-head-sentinel");
    fixture.write(".env", b"secret-excluded-sentinel");
    fixture.write("node_modules/private", b"excluded-dependency-sentinel");
    fixture.write("dirty.txt", b"dirty tracked-equivalent bytes");
    fixture.write("untracked.txt", b"untracked eligible bytes");
    let outside = fixture._temp.path().join("outside.txt");
    fs::write(&outside, b"unrelated sibling").unwrap();
    let mut store = fixture.open();
    let record = store
        .capture(CaptureSelection::Workspace {})
        .expect("eligible dirty and untracked content capture");
    assert_eq!(
        record
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["dirty.txt", "untracked.txt"]
    );
    assert_eq!(record.excluded, vec![".env", ".git", "node_modules"]);
    assert_eq!(
        fs::read(fixture.workspace.join(".git/index")).unwrap(),
        b"dirty-index-sentinel"
    );
    assert_eq!(
        fs::read(fixture.workspace.join(".git/HEAD")).unwrap(),
        b"fake-head-sentinel"
    );
    assert_eq!(
        fs::read(fixture.workspace.join(".env")).unwrap(),
        b"secret-excluded-sentinel"
    );
    assert_eq!(fs::read(outside).unwrap(), b"unrelated sibling");
    assert_eq!(
        store.capture(paths(&[".env"])),
        Err(CaptureError::UnsafePath)
    );
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn fp08_host_capture_held_read_refuses_writer_and_hardlink() {
    let fixture = Fixture::new();
    fixture.write("ordinary", b"original content");
    let mut store = fixture.open();
    store
        .capture(paths(&["ordinary"]))
        .expect("positive capture before refusal checks");
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let writer = fs::OpenOptions::new()
            .write(true)
            .share_mode(7)
            .open(fixture.workspace.join("ordinary"))
            .unwrap();
        assert_eq!(store.capture(paths(&["ordinary"])), Err(CaptureError::Busy));
        assert_eq!(store.list().unwrap().len(), 1);
        drop(writer);
    }
    fs::hard_link(
        fixture.workspace.join("ordinary"),
        fixture.workspace.join("alias"),
    )
    .unwrap();
    assert_eq!(
        store.capture(paths(&["ordinary"])),
        Err(CaptureError::Linked)
    );
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        fs::read(fixture.workspace.join("ordinary")).unwrap(),
        b"original content"
    );
    assert_eq!(
        fs::read(fixture.workspace.join("alias")).unwrap(),
        b"original content"
    );
}

#[test]
fn fp08_host_store_corrupt_existing_object_is_not_repaired() {
    let fixture = Fixture::new();
    fixture.write("content", b"original");
    let mut store = fixture.open();
    let first = store
        .capture(paths(&["content"]))
        .expect("publish first immutable object");
    let hash = match entry(&first, "content") {
        CaptureState::File { content, .. } => content.sha256.clone(),
        _ => panic!("ordinary content must be a file"),
    };
    let object = fixture.object_path(&hash);
    fs::write(&object, b"corrupt!").unwrap(); // Same length defeats a length-only check.
    assert_eq!(
        store.capture(paths(&["content"])),
        Err(CaptureError::Corrupt)
    );
    assert_eq!(
        store.read_content(&first.checkpoint_id, "content"),
        Err(CaptureError::Corrupt)
    );
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(fs::read(object).unwrap(), b"corrupt!");
    assert_eq!(
        fs::read(fixture.workspace.join("content")).unwrap(),
        b"original"
    );
}

#[test]
fn fp08_host_store_interrupted_publication_is_not_committed() {
    let fixture = Fixture::new();
    fixture.write("content", b"before");
    let mut store = fixture.open();
    let first = store
        .capture(paths(&["content"]))
        .expect("baseline committed generation");
    fixture.write("content", b"after");
    store.set_publish_fault(TestPublishFault::BeforeIndexCommit);
    assert_eq!(
        store.capture(paths(&["content"])),
        Err(CaptureError::Interrupted)
    );
    let mut reopened = fixture.open();
    let history = reopened.list().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].checkpoint_id, first.checkpoint_id);
    assert_eq!(
        reopened
            .read_content(&first.checkpoint_id, "content")
            .unwrap(),
        b"before"
    );
    reopened.set_publish_fault(TestPublishFault::AfterIndexCommitBeforeAck);
    assert_eq!(
        reopened.capture(paths(&["content"])),
        Err(CaptureError::RecoveryRequired)
    );
    assert_eq!(fixture.open().list(), Err(CaptureError::RecoveryRequired));
    assert_eq!(
        fs::read(fixture.workspace.join("content")).unwrap(),
        b"after"
    );
}

#[test]
fn fp08_host_list_diff_reload_scope_and_limits() {
    let fixture = Fixture::new();
    fixture.write("a", b"first");
    fixture.write("empty", b"");
    let mut store = fixture.open();
    let first = store
        .capture(CaptureSelection::Workspace {})
        .expect("first reloadable generation");
    fixture.write("a", b"second");
    fs::remove_file(fixture.workspace.join("empty")).unwrap();
    fixture.write("b", b"new");
    assert_eq!(
        store.diff(&first.checkpoint_id).unwrap(),
        vec![
            CaptureDiff {
                path: "a".into(),
                kind: DiffKind::Modified
            },
            CaptureDiff {
                path: "b".into(),
                kind: DiffKind::Added
            },
            CaptureDiff {
                path: "empty".into(),
                kind: DiffKind::Deleted
            },
        ]
    );
    let second = store.capture(CaptureSelection::Workspace {}).unwrap();
    let reopened = fixture.open();
    assert_eq!(reopened.get(&first.checkpoint_id).unwrap(), first);
    assert_eq!(reopened.get(&second.checkpoint_id).unwrap(), second);
    let history = reopened.list().unwrap();
    assert_eq!(
        history.iter().map(|item| item.sequence).collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(history[0].entry_count, 2);
    assert_eq!(history[0].content_bytes, 9);
    assert_eq!(history[0].profile, CaptureProfile::ContentBasicV1);
    let mut foreign = fixture.config();
    foreign.scope.owner_id = "owner-b".into();
    let foreign = CheckpointStore::new(foreign);
    assert!(foreign.list().unwrap().is_empty());
    assert_eq!(
        foreign.get(&first.checkpoint_id),
        Err(CaptureError::Unavailable)
    );
    assert_eq!(
        foreign.read_content(&first.checkpoint_id, "a"),
        Err(CaptureError::Unavailable)
    );
    let other = fixture._temp.path().join("different-workspace");
    fs::create_dir(&other).unwrap();
    let mut wrong_root = fixture.config();
    wrong_root.workspace_root = other;
    assert_eq!(
        CheckpointStore::new(wrong_root).capture(paths(&["a"])),
        Err(CaptureError::InvalidScope)
    );
    // A separately bound lower-budget policy cannot partially publish oversized content.
    let mut bounded = Fixture::new();
    bounded.policy.limits.file_bytes = 3;
    bounded.write("too-large", b"four");
    let mut limited = bounded.open();
    assert_eq!(
        limited.capture(CaptureSelection::Workspace {}),
        Err(CaptureError::Budget)
    );
    assert!(limited.list().unwrap().is_empty());
    assert_eq!(
        fs::read(bounded.workspace.join("too-large")).unwrap(),
        b"four"
    );
}

#[test]
fn fp08_host_capture_observed_entry_budget_counts_empty_directories() {
    let mut fixture = Fixture::new();
    fixture.policy.limits.entries = 1;
    fs::create_dir(fixture.workspace.join("first-empty")).unwrap();
    let mut store = fixture.open();
    let first = store
        .capture(CaptureSelection::Workspace {})
        .expect("one observed directory fits the configured ceiling");
    assert!(first.entries.is_empty());
    fs::create_dir(fixture.workspace.join("second-empty")).unwrap();
    assert_eq!(
        store.capture(CaptureSelection::Workspace {}),
        Err(CaptureError::Budget),
        "empty directories consume observed-entry budget even without captured files"
    );
    assert_eq!(
        store.list().unwrap().len(),
        1,
        "failed capture must not publish history"
    );
    assert!(fixture.workspace.join("first-empty").is_dir());
    assert!(fixture.workspace.join("second-empty").is_dir());
    assert_eq!(fs::read_dir(&fixture.workspace).unwrap().count(), 2);
}

#[test]
fn fp08_host_scan_reserves_one_shared_budget_before_buffering_siblings() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.workspace.join("a/deep")).unwrap();
    fs::create_dir(fixture.workspace.join("a/sibling")).unwrap();
    fs::create_dir(fixture.workspace.join("z")).unwrap();
    let root = Dir::open_ambient_dir(&fixture.workspace, ambient_authority()).unwrap();
    let nested = root.open_dir("a").unwrap();
    let mut remaining = 3;
    let root_names = workspace::names_in_budget(&root, &mut remaining).expect("two root names fit");
    assert_eq!(root_names, vec!["a", "z"]);
    // Retain root_names while descending: queued siblings must already have
    // reservations, rather than receiving a new full per-directory allowance.
    assert_eq!(
        workspace::names_in_budget(&nested, &mut remaining),
        Err(CaptureError::Budget)
    );
    assert_eq!(
        remaining, 0,
        "the last available slot was reserved before the overflow name"
    );
    assert_eq!(
        root_names.len(),
        2,
        "the parent buffer remains alive across child enumeration"
    );
    assert_eq!(
        fs::read_dir(fixture.workspace.join("a")).unwrap().count(),
        2
    );
}

#[cfg(windows)]
#[test]
fn fp08_host_diff_windows_case_only_rename_keeps_one_identity() {
    let fixture = Fixture::new();
    fixture.write("Alpha.txt", b"same bytes");
    let mut store = fixture.open();
    let first = store
        .capture(CaptureSelection::Workspace {})
        .expect("ordinary pre-rename capture");
    fs::rename(
        fixture.workspace.join("Alpha.txt"),
        fixture.workspace.join("alpha.txt"),
    )
    .unwrap();
    assert!(
        store.diff(&first.checkpoint_id).unwrap().is_empty(),
        "a case-only spelling change is one Windows identity, not an added file"
    );
    fixture.write("alpha.txt", b"new bytes");
    assert_eq!(
        store.diff(&first.checkpoint_id).unwrap(),
        vec![CaptureDiff {
            path: "alpha.txt".into(),
            kind: DiffKind::Modified
        },]
    );
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        fs::read(fixture.workspace.join("alpha.txt")).unwrap(),
        b"new bytes"
    );
}

fn restore_pure_state(state: &CaptureState) -> anima_core::filesystem_checkpoints::FileState {
    use anima_core::filesystem_checkpoints::{
        FileState, MetadataAttribute, MetadataKind, MetadataSnapshot,
    };
    match state {
        CaptureState::Absent {} => FileState::Absent,
        CaptureState::File {
            content,
            basic_mode,
        } => FileState::File {
            content: content.clone(),
            metadata: MetadataSnapshot {
                required: vec![MetadataKind::BasicPermissions],
                attributes: vec![MetadataAttribute {
                    kind: MetadataKind::BasicPermissions,
                    encoding_version: 1,
                    bytes: serde_json::to_vec(basic_mode).unwrap(),
                }],
            },
        },
    }
}

fn restore_authority(
    fixture: &Fixture,
    before: &CaptureRecord,
    after: &CaptureRecord,
    changed: &[&str],
) -> BasicRestoreAuthority {
    use anima_core::filesystem_checkpoints::*;
    let scope = fixture.config().scope;
    let checkpoint = FilesystemCheckpoint {
        version: 1,
        checkpoint_id: before.checkpoint_id.clone(),
        sequence: before.sequence,
        scope_digest: before.scope_digest.clone(),
        scope: scope.clone(),
        entries: before
            .entries
            .iter()
            .map(|entry| CheckpointEntry {
                path: entry.path.clone(),
                state: restore_pure_state(&entry.state),
            })
            .collect(),
    };
    let changes = changed
        .iter()
        .map(|path| {
            let previous = restore_pure_state(entry(before, path));
            let next = restore_pure_state(entry(after, path));
            let disposition = match (&previous, &next) {
                (FileState::Absent, FileState::File { .. }) => MutationDisposition::Created,
                (FileState::File { .. }, FileState::Absent) => MutationDisposition::Deleted,
                (FileState::File { .. }, FileState::File { .. }) => MutationDisposition::Replaced,
                _ => panic!("fixture must perform a genuine content/tree mutation"),
            };
            MutationChange {
                path: (*path).into(),
                disposition,
                observed_preimage: previous,
                acknowledged_postimage: Some(next),
            }
        })
        .collect::<Vec<_>>();
    // Synthetic native identities are private test input; actual source and
    // postimage bytes above came from real host captures in private TempDirs.
    let action = MutationAction {
        sequence: 1,
        action_id: "native-test-action".into(),
        native_run_id: "native-test-run".into(),
        native_tool_call_id: "native-test-call".into(),
        request_digest: checkpoint_digest(b"private native test mutation"),
        outcome: MutationActionOutcome::Acknowledged,
        changes,
    };
    let manifest_digest = checkpoint_manifest_digest(&checkpoint);
    let action_digest = checkpoint_action_digest(&action);
    let head = LineageHeadDigestInput {
        version: 1,
        checkpoint_id: before.checkpoint_id.clone(),
        checkpoint_sequence: before.sequence,
        manifest_digest: manifest_digest.clone(),
        scope_digest: before.scope_digest.clone(),
        action_count: 1,
        last_sequence: 1,
        action_digests: vec![action_digest.clone()],
    };
    let head_digest = checkpoint_lineage_head_digest(&head);
    let expected_head = ExpectedLineageHead {
        checkpoint_id: before.checkpoint_id.clone(),
        checkpoint_sequence: before.sequence,
        manifest_digest: manifest_digest.clone(),
        scope_digest: before.scope_digest.clone(),
        action_count: 1,
        last_sequence: 1,
        head_digest: head_digest.clone(),
        action_bindings: vec![ExpectedActionBinding {
            sequence: 1,
            action_id: action.action_id.clone(),
            native_run_id: action.native_run_id.clone(),
            native_tool_call_id: action.native_tool_call_id.clone(),
            request_digest: action.request_digest.clone(),
            action_digest,
        }],
    };
    BasicRestoreAuthority {
        expected_scope: scope,
        lineage: MutationLineage {
            version: 1,
            checkpoint_id: before.checkpoint_id.clone(),
            checkpoint_sequence: before.sequence,
            manifest_digest,
            scope_digest: before.scope_digest.clone(),
            action_count: 1,
            last_sequence: 1,
            actions: vec![action],
            head_digest,
        },
        expected_head,
    }
}

fn replacement_restore_fixture() -> (
    Fixture,
    CheckpointStore,
    CaptureRecord,
    BasicRestoreAuthority,
) {
    let fixture = Fixture::new();
    fixture.write("target", b"ORIGINAL\0RAW\r\nBYTES");
    fixture.write("unrelated", b"unrelated sentinel");
    let mut store = fixture.open();
    let before = store
        .capture(paths(&["target"]))
        .expect("real basic preimage capture");
    fixture.write("target", b"native acknowledged postimage");
    let after = store
        .capture(paths(&["target"]))
        .expect("real postimage capture");
    let authority = restore_authority(&fixture, &before, &after, &["target"]);
    (fixture, store, before, authority)
}

#[cfg(windows)]
#[test]
fn fp08_host_provision_private_root_and_refuse_insecure_existing_root_without_acl_reset() {
    use win_custody::{
        current_user_sid_string, ObjectSecurityDescriptor, OwnedSid, SecurityDescriptor,
    };
    let fixture = Fixture::new();
    fixture.write("target", b"private source");
    let private_root = fixture._temp.path().join("new-owned-private-store");
    let mut config = fixture.config();
    config.store_root = private_root.clone();
    assert!(!private_root.exists());
    let mut store = CheckpointStore::provision_private_root(config)
        .expect("provision real private empty root before secret bytes");
    let descriptor = ObjectSecurityDescriptor::query_file(&private_root).unwrap();
    assert_ne!(
        descriptor.control().unwrap() & 0x1000,
        0,
        "private DACL must be protected from inheritance"
    );
    let user = current_user_sid_string().unwrap();
    let user_sid = OwnedSid::from_string(&user).unwrap();
    let system_sid = OwnedSid::from_string("S-1-5-18").unwrap();
    let dacl = descriptor.dacl().unwrap();
    assert!(!dacl.is_empty());
    assert!(dacl
        .allowed_aces()
        .all(|ace| ace.trustee_matches(&user_sid) || ace.trustee_matches(&system_sid)));
    store
        .capture(paths(&["target"]))
        .expect("verified private root stores actual source bytes");
    let insecure = fixture._temp.path().join("existing-insecure-store");
    fs::create_dir(&insecure).unwrap();
    SecurityDescriptor::from_sddl(&format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;WD)"))
        .unwrap()
        .apply_to_tree(&insecure, true)
        .unwrap();
    let sentinel = insecure.join("unrelated");
    fs::write(&sentinel, b"must not repair or delete").unwrap();
    let before = ObjectSecurityDescriptor::query_file(&insecure).unwrap();
    let before_dacl = before.dacl().unwrap();
    let before_masks = before_dacl
        .allowed_aces()
        .map(|ace| (ace.flags(), ace.mask()))
        .collect::<Vec<_>>();
    let mut config = fixture.config();
    config.store_root = insecure.clone();
    assert!(matches!(
        CheckpointStore::provision_private_root(config),
        Err(CaptureError::InsecureStore)
    ));
    let after = ObjectSecurityDescriptor::query_file(&insecure).unwrap();
    assert_eq!(after.control().unwrap(), before.control().unwrap());
    assert_eq!(
        after
            .dacl()
            .unwrap()
            .allowed_aces()
            .map(|ace| (ace.flags(), ace.mask()))
            .collect::<Vec<_>>(),
        before_masks
    );
    let everyone = OwnedSid::from_string("S-1-1-0").unwrap();
    assert!(after
        .dacl()
        .unwrap()
        .allowed_aces()
        .any(|ace| ace.trustee_matches(&everyone)));
    assert_eq!(fs::read(sentinel).unwrap(), b"must not repair or delete");
}

#[test]
fn fp08_host_basic_selected_restore_exact_bytes_empty_mode_and_unrelated_sentinel() {
    let fixture = Fixture::new();
    fixture.write("binary", b"\xff\0original\r\nbytes");
    fixture.write("empty", b"");
    fixture.write("unrelated", b"owner unrelated sentinel");
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(fixture.workspace.join("binary"))
            .unwrap()
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(fixture.workspace.join("binary"), permissions).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.workspace.join("binary"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    let mode = actual_basic_mode(&fixture.workspace.join("binary"));
    let mut store = fixture.open();
    let before = store
        .capture(paths(&["binary", "empty"]))
        .expect("real binary and empty preimage capture");
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(fixture.workspace.join("binary"))
            .unwrap()
            .permissions();
        permissions.set_readonly(false);
        fs::set_permissions(fixture.workspace.join("binary"), permissions).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.workspace.join("binary"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    fixture.write("binary", b"native replacement");
    fixture.write("empty", b"filled after capture");
    let after = store.capture(paths(&["binary", "empty"])).unwrap();
    let authority = restore_authority(&fixture, &before, &after, &["binary", "empty"]);
    let plan = store
        .prepare_basic_restore(
            &before.checkpoint_id,
            &["binary".into(), "empty".into()],
            &authority,
        )
        .expect("prepare genuine basic-profile selected restore without invented time");
    for operation in &plan.proposal.operations {
        if let anima_core::filesystem_checkpoints::FileState::File { metadata, .. } =
            &operation.desired
        {
            assert_eq!(metadata.attributes.len(), 1);
            assert_eq!(
                metadata.required,
                vec![anima_core::filesystem_checkpoints::MetadataKind::BasicPermissions]
            );
        }
    }
    let result = store
        .execute_basic_restore(&plan, &authority)
        .expect("real private selected content restore");
    assert_eq!(result.restored_paths, vec!["binary", "empty"]);
    assert!(!result.safety_checkpoint_id.is_empty());
    assert_eq!(
        fs::read(fixture.workspace.join("binary")).unwrap(),
        b"\xff\0original\r\nbytes"
    );
    assert_eq!(fs::read(fixture.workspace.join("empty")).unwrap(), b"");
    assert_eq!(actual_basic_mode(&fixture.workspace.join("binary")), mode);
    assert_eq!(
        fs::read(fixture.workspace.join("unrelated")).unwrap(),
        b"owner unrelated sentinel"
    );
    assert_eq!(
        fixture.open().restore_status(),
        Ok(BasicRecoveryStatus::Clean)
    );
    #[cfg(windows)]
    {
        // Fixture cleanup only, after proving actual restored readonly mode.
        let mut permissions = fs::metadata(fixture.workspace.join("binary"))
            .unwrap()
            .permissions();
        permissions.set_readonly(false);
        fs::set_permissions(fixture.workspace.join("binary"), permissions).unwrap();
    }
}

#[test]
fn fp08_host_basic_restore_failed_or_uncertain_safety_save_has_zero_target_effects() {
    for fault in [
        TestRestoreFault::BeforeSafetySync,
        TestRestoreFault::AfterPreparedBeforeAck,
    ] {
        let (fixture, mut store, before, authority) = replacement_restore_fixture();
        let plan = store
            .prepare_basic_restore(&before.checkpoint_id, &["target".into()], &authority)
            .expect("valid restore before safety fault");
        store.set_restore_fault(fault);
        let expected = if fault == TestRestoreFault::BeforeSafetySync {
            CaptureError::Interrupted
        } else {
            CaptureError::RecoveryRequired
        };
        assert_eq!(
            store.execute_basic_restore(&plan, &authority),
            Err(expected)
        );
        assert_eq!(
            fs::read(fixture.workspace.join("target")).unwrap(),
            b"native acknowledged postimage"
        );
        assert_eq!(
            fs::read(fixture.workspace.join("unrelated")).unwrap(),
            b"unrelated sentinel"
        );
        if fault == TestRestoreFault::AfterPreparedBeforeAck {
            assert!(matches!(
                fixture.open().restore_status(),
                Ok(BasicRecoveryStatus::RecoveryRequired { .. })
            ));
        }
    }
}

#[test]
fn fp08_host_basic_restore_dirty_change_after_safety_save_preserves_newer_content() {
    let (fixture, mut store, before, authority) = replacement_restore_fixture();
    let plan = store
        .prepare_basic_restore(&before.checkpoint_id, &["target".into()], &authority)
        .expect("valid restore plan before external edit");
    let target = fixture.workspace.join("target");
    store.set_restore_hook(Box::new(move |phase| {
        if phase == TestRestorePhase::BeforeTargetLease {
            fs::write(&target, b"owner edit after safety save").unwrap();
        }
    }));
    assert_eq!(
        store.execute_basic_restore(&plan, &authority),
        Err(CaptureError::Dirty)
    );
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"owner edit after safety save"
    );
    assert_eq!(
        fs::read(fixture.workspace.join("unrelated")).unwrap(),
        b"unrelated sentinel"
    );
}

#[test]
fn fp08_host_basic_restore_interrupted_actual_write_retains_safety_and_never_replays() {
    let (fixture, mut store, before, authority) = replacement_restore_fixture();
    let plan = store
        .prepare_basic_restore(&before.checkpoint_id, &["target".into()], &authority)
        .expect("valid restore before actual write interruption");
    store.set_restore_fault(TestRestoreFault::AfterFirstTargetWrite);
    assert_eq!(
        store.execute_basic_restore(&plan, &authority),
        Err(CaptureError::RecoveryRequired)
    );
    assert_ne!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"native acknowledged postimage",
        "fault must follow a real target write"
    );
    let mut reopened = fixture.open();
    assert!(
        matches!(reopened.restore_status(), Ok(BasicRecoveryStatus::RecoveryRequired { plan_id, .. }) if plan_id == plan.plan_id)
    );
    assert_eq!(
        reopened
            .read_safety_content(&plan.plan_id, "target")
            .unwrap(),
        b"native acknowledged postimage"
    );
    fixture.write("target", b"owner newer after interruption");
    assert_eq!(
        reopened.execute_basic_restore(&plan, &authority),
        Err(CaptureError::RecoveryRequired)
    );
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"owner newer after interruption"
    );
    assert_eq!(
        fs::read(fixture.workspace.join("unrelated")).unwrap(),
        b"unrelated sentinel"
    );
}

#[test]
fn fp08_host_basic_restore_post_write_metadata_failure_reports_recovery_and_never_replays() {
    let (fixture, mut store, before, authority) = replacement_restore_fixture();
    let plan = store
        .prepare_basic_restore(&before.checkpoint_id, &["target".into()], &authority)
        .expect("valid restore before metadata I/O failure");
    store.set_restore_fault(TestRestoreFault::ModeMetadataReadFails);
    assert_eq!(
        store.execute_basic_restore(&plan, &authority),
        Err(CaptureError::RecoveryRequired),
        "a metadata failure after actual content effects must report recovery immediately"
    );
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"ORIGINAL\0RAW\r\nBYTES",
        "injection occurs after a real complete content write"
    );
    let mut reopened = fixture.open();
    assert!(
        matches!(reopened.restore_status(), Ok(BasicRecoveryStatus::RecoveryRequired { plan_id, .. }) if plan_id == plan.plan_id)
    );
    assert_eq!(
        reopened
            .read_safety_content(&plan.plan_id, "target")
            .unwrap(),
        b"native acknowledged postimage"
    );
    fixture.write("target", b"owner newer after metadata failure");
    assert_eq!(
        reopened.execute_basic_restore(&plan, &authority),
        Err(CaptureError::RecoveryRequired)
    );
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"owner newer after metadata failure"
    );
    assert_eq!(
        fs::read(fixture.workspace.join("unrelated")).unwrap(),
        b"unrelated sentinel"
    );
}

#[test]
fn fp08_host_basic_restore_absent_and_created_races_preserve_actual_unknown_objects() {
    for created in [false, true] {
        let fixture = Fixture::new();
        if !created {
            fixture.write("target", b"original deleted by native action");
        }
        fixture.write("unrelated", b"unrelated sentinel");
        let mut store = fixture.open();
        let before = store
            .capture(paths(&["target"]))
            .expect("real original present or absent preimage");
        if created {
            fixture.write("target", b"native created content");
        } else {
            fs::remove_file(fixture.workspace.join("target")).unwrap();
        }
        let after = store.capture(paths(&["target"])).unwrap();
        let authority = restore_authority(&fixture, &before, &after, &["target"]);
        let plan = store
            .prepare_basic_restore(&before.checkpoint_id, &["target".into()], &authority)
            .expect("genuine Created or Deleted restore plan");
        let target = fixture.workspace.join("target");
        let retained = fixture.workspace.join("agent-retained-unselected");
        store.set_restore_hook(Box::new(move |phase| {
            if (!created && phase == TestRestorePhase::BeforeAbsentCreate)
                || (created && phase == TestRestorePhase::BeforeCreatedMove)
            {
                if created {
                    fs::rename(&target, &retained).unwrap();
                }
                fs::write(&target, b"raced unknown owner object").unwrap();
            }
        }));
        let expected = if created {
            CaptureError::RecoveryRequired
        } else {
            CaptureError::Dirty
        };
        assert_eq!(
            store.execute_basic_restore(&plan, &authority),
            Err(expected)
        );
        if created {
            assert_eq!(
                fixture
                    .open()
                    .preserved_restore_content(&plan.plan_id, "target")
                    .unwrap(),
                b"raced unknown owner object"
            );
            assert_eq!(
                fs::read(fixture.workspace.join("agent-retained-unselected")).unwrap(),
                b"native created content"
            );
            if fixture.workspace.join("target").exists() {
                assert_eq!(
                    fs::read(fixture.workspace.join("target")).unwrap(),
                    b"raced unknown owner object"
                );
            }
        } else {
            assert_eq!(
                fs::read(fixture.workspace.join("target")).unwrap(),
                b"raced unknown owner object"
            );
        }
        assert_eq!(
            fs::read(fixture.workspace.join("unrelated")).unwrap(),
            b"unrelated sentinel"
        );
    }
}

#[cfg(windows)]
#[test]
fn a6_created_file_restore_rechecks_revocation_after_move_preparation() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let before = store.capture(paths(&["created"])).unwrap();
    fixture.write("created", b"native created bytes");
    let after = store.capture(paths(&["created"])).unwrap();
    let authority = restore_authority(&fixture, &before, &after, &["created"]);
    let plan = store
        .prepare_basic_restore(&before.checkpoint_id, &["created".into()], &authority)
        .unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let hook_stop = stopped.clone();
    store.set_restore_hook(Box::new(move |phase| {
        if phase == TestRestorePhase::BeforeCreatedMove {
            hook_stop.store(true, Ordering::SeqCst);
        }
    }));
    let result = store.execute_basic_restore_checked(&plan, &authority, &mut || {
        if stopped.load(Ordering::SeqCst) {
            Err(CaptureError::Interrupted)
        } else {
            Ok(())
        }
    });
    assert_eq!(result, Err(CaptureError::Interrupted));
    assert_eq!(
        fs::read(fixture.workspace.join("created")).unwrap(),
        b"native created bytes"
    );
    assert_eq!(
        fixture.open().restore_status(),
        Ok(BasicRecoveryStatus::Clean)
    );
}
