use super::service::NativeMutationFault;
use super::*;
use crate::runs::RunLink;
use anima_core::{DataValue, ToolCall};
use std::{collections::BTreeMap, fs};

struct NativeFixture {
    temp: tempfile::TempDir,
    workspace: PathBuf,
    store: PathBuf,
}

impl NativeFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        use win_custody::{current_user_sid_string, SecurityDescriptor};
        let sid = current_user_sid_string().unwrap();
        SecurityDescriptor::from_sddl(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"))
            .unwrap()
            .apply_to_tree(temp.path(), true)
            .unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        Self {
            store: temp.path().join("checkpoint-store"),
            temp,
            workspace,
        }
    }

    fn service(&self) -> NativeCheckpointService {
        NativeCheckpointService::new(self.store.clone())
    }

    fn run(&self, id: &str) -> RunLink {
        RunLink {
            run_id: id.into(),
            session_id: "native-session".into(),
            agent_id: "native-owner".into(),
        }
    }

    fn replace(
        &self,
        service: &NativeCheckpointService,
        run: &RunLink,
        id: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<NativeMutationReceipt, String> {
        service.mutate(&self.workspace, run, &call(id, path), path, |_| {
            Ok(bytes.to_vec())
        })
    }
}

fn call(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "write_file".into(),
        args: BTreeMap::from([
            ("file_path".into(), DataValue::String(path.into())),
            (
                "content".into(),
                DataValue::String("exact admitted transform input".into()),
            ),
        ]),
    }
}

#[test]
fn a6_native_checkpoint_reload_restore_exact_bytes_and_native_binding() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"\xff\0original\r\nbytes").unwrap();
    fs::write(fixture.workspace.join("unrelated"), b"owner sentinel").unwrap();
    let original_mode = fs::metadata(fixture.workspace.join("target"))
        .unwrap()
        .permissions();
    let run = fixture.run("original-native-run");
    let receipt = fixture
        .replace(
            &fixture.service(),
            &run,
            "original-call",
            "target",
            b"native replacement",
        )
        .unwrap();
    let reload = fixture.service();
    let fresh_run = fixture.run("fresh-native-run");
    let listed = reload.list(&fixture.workspace, &fresh_run).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].checkpoint_id, receipt.checkpoint_id);
    assert_eq!(listed[0].native_run_id, run.run_id);
    assert_eq!(listed[0].native_tool_call_id, "original-call");
    assert_eq!(listed[0].path, "target");
    assert_eq!(listed[0].status, "acknowledged");
    let restored = reload
        .restore(&fixture.workspace, &fresh_run, &receipt.checkpoint_id)
        .unwrap();
    assert_eq!(restored.restored_paths, ["target"]);
    assert_ne!(restored.safety_checkpoint_id, receipt.checkpoint_id);
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"\xff\0original\r\nbytes"
    );
    assert_eq!(
        fs::metadata(fixture.workspace.join("target"))
            .unwrap()
            .permissions()
            .readonly(),
        original_mode.readonly()
    );
    assert_eq!(
        fs::read(fixture.workspace.join("unrelated")).unwrap(),
        b"owner sentinel"
    );
}

#[test]
fn a6_native_checkpoint_dirty_refusal_preserves_owner_change() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let run = fixture.run("run");
    let receipt = fixture
        .replace(&fixture.service(), &run, "call", "target", b"native")
        .unwrap();
    fs::write(fixture.workspace.join("target"), b"new owner bytes").unwrap();
    let error = fixture
        .service()
        .restore(&fixture.workspace, &run, &receipt.checkpoint_id)
        .unwrap_err();
    assert!(error.contains("Dirty"), "{error}");
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"new owner bytes"
    );
}

#[test]
fn a6_native_checkpoint_dirty_before_final_lease_has_zero_effects() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let service = fixture.service();
    let target = fixture.workspace.join("target");
    service.set_before_lease(move || fs::write(target, b"owner raced bytes").unwrap());
    let error = fixture
        .replace(&service, &fixture.run("run"), "call", "target", b"native")
        .unwrap_err();
    assert!(error.contains("Dirty"), "{error}");
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"owner raced bytes"
    );
}

#[test]
fn a6_native_checkpoint_budget_and_missing_parent_have_zero_target_effects() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let service = fixture.service().with_limits(CaptureLimits {
        store_bytes: 1024,
        ..CaptureLimits::default()
    });
    let error = fixture
        .replace(
            &service,
            &fixture.run("run"),
            "budget",
            "target",
            &vec![b'x'; 2048],
        )
        .unwrap_err();
    assert!(error.contains("Budget"), "{error}");
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"original"
    );
    assert!(fixture
        .replace(
            &fixture.service(),
            &fixture.run("run"),
            "missing-parent",
            "missing/target",
            b"new"
        )
        .is_err());
    assert!(!fixture.workspace.join("missing").exists());
}

#[test]
fn a6_native_checkpoint_faults_persist_conservative_recovery_without_replay() {
    for fault in [
        NativeMutationFault::BeforePreimageAck,
        NativeMutationFault::AfterFirstWrite,
        NativeMutationFault::BeforePostimageAck,
    ] {
        let fixture = NativeFixture::new();
        fs::write(fixture.workspace.join("target"), b"original").unwrap();
        let service = fixture.service();
        service.set_fault(fault);
        let run = fixture.run("run");
        let error = fixture
            .replace(&service, &run, "call", "target", b"replacement")
            .unwrap_err();
        assert_eq!(error, NATIVE_RECOVERY_REQUIRED);
        if fault == NativeMutationFault::BeforePreimageAck {
            assert_eq!(
                fs::read(fixture.workspace.join("target")).unwrap(),
                b"original"
            );
        } else {
            assert_ne!(
                fs::read(fixture.workspace.join("target")).unwrap(),
                b"original"
            );
        }
        fs::write(fixture.workspace.join("target"), b"owner after failure").unwrap();
        assert_eq!(
            fixture.service().list(&fixture.workspace, &run),
            Err(NATIVE_RECOVERY_REQUIRED.into())
        );
        assert_eq!(
            fixture.replace(&fixture.service(), &run, "never-replay", "target", b"bad"),
            Err(NATIVE_RECOVERY_REQUIRED.into())
        );
        assert_eq!(
            fs::read(fixture.workspace.join("target")).unwrap(),
            b"owner after failure"
        );
    }
}

#[test]
fn a6_native_checkpoint_owner_root_and_opaque_id_do_not_grant_authority() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let run = fixture.run("run");
    let receipt = fixture
        .replace(&fixture.service(), &run, "call", "target", b"native")
        .unwrap();
    let mut other_owner = fixture.run("other-run");
    other_owner.agent_id = "other-owner".into();
    assert!(fixture
        .service()
        .list(&fixture.workspace, &other_owner)
        .unwrap()
        .is_empty());
    assert!(fixture
        .service()
        .restore(&fixture.workspace, &other_owner, &receipt.checkpoint_id)
        .is_err());
    let other_root = fixture.temp.path().join("other-workspace");
    fs::create_dir(&other_root).unwrap();
    fs::write(other_root.join("target"), b"other workspace sentinel").unwrap();
    assert!(fixture
        .service()
        .restore(&other_root, &run, &receipt.checkpoint_id)
        .is_err());
    assert!(fixture
        .service()
        .restore(&fixture.workspace, &run, &"a".repeat(64))
        .is_err());
    assert!(fixture
        .service()
        .restore(&fixture.workspace, &run, "../escape")
        .is_err());
    assert_eq!(
        fs::read(other_root.join("target")).unwrap(),
        b"other workspace sentinel"
    );
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"native"
    );
    assert!(fixture
        .replace(&fixture.service(), &run, "call", "target", b"replayed")
        .is_err());
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"native"
    );
}

#[test]
fn a6_native_checkpoint_protected_restore_refuses_present_and_absent_preimages() {
    for present in [false, true] {
        let fixture = NativeFixture::new();
        if present {
            fs::write(fixture.workspace.join("AGENTS.md"), b"original instruction").unwrap();
        }
        let run = fixture.run("run");
        // Controller has already performed exact approval for this original mutation.
        let receipt = fixture
            .replace(
                &fixture.service(),
                &run,
                "approved-call",
                "AGENTS.md",
                b"approved instruction",
            )
            .unwrap();
        let error = fixture
            .service()
            .restore(&fixture.workspace, &run, &receipt.checkpoint_id)
            .unwrap_err();
        assert!(error.contains("protected"), "{error}");
        assert_eq!(
            fs::read(fixture.workspace.join("AGENTS.md")).unwrap(),
            b"approved instruction"
        );
    }
}

#[test]
fn a6_native_checkpoint_history_reserves_actual_restore_safety_capture() {
    let fixture = NativeFixture::new();
    let run = fixture.run("run");
    let service = fixture.service();
    let mut newest = None;
    for index in 0..19 {
        let path = format!("file-{index:02}");
        fs::write(fixture.workspace.join(&path), b"original").unwrap();
        newest = Some(
            fixture
                .replace(&service, &run, &format!("call-{index}"), &path, b"native")
                .unwrap(),
        );
    }
    fs::write(fixture.workspace.join("overflow"), b"overflow sentinel").unwrap();
    let error = fixture
        .replace(&service, &run, "overflow", "overflow", b"bad")
        .unwrap_err();
    assert!(error.contains("Budget"), "{error}");
    assert_eq!(
        fs::read(fixture.workspace.join("overflow")).unwrap(),
        b"overflow sentinel"
    );
    assert_eq!(service.list(&fixture.workspace, &run).unwrap().len(), 19);
    let newest = newest.unwrap();
    let restored = service
        .restore(&fixture.workspace, &run, &newest.checkpoint_id)
        .unwrap();
    assert!(!restored.safety_checkpoint_id.is_empty());
    assert_eq!(
        fs::read(fixture.workspace.join(&newest.path)).unwrap(),
        b"original"
    );
}

#[test]
fn a6_native_checkpoint_final_authority_revalidation_refuses_before_effect() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let service = fixture.service();
    let mut checked = false;
    let error = service
        .mutate_checked(
            &fixture.workspace,
            &fixture.run("run"),
            &call("call", "target"),
            "target",
            |_| Ok(b"must not write".to_vec()),
            || {
                checked = true;
                Err("native authority revoked".into())
            },
        )
        .unwrap_err();
    assert!(checked, "authority must be read after durable admission");
    assert!(error.contains("revoked"), "{error}");
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"original"
    );
}

#[test]
fn native_checkpoint_retains_authority_until_published_bytes_are_read_back() {
    struct Authority {
        path: PathBuf,
        observed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl Drop for Authority {
        fn drop(&mut self) {
            assert_eq!(fs::read(&self.path).unwrap(), b"reviewed candidate");
            self.observed
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    for tool in ["write_file", "apply_document_edit"] {
        let fixture = NativeFixture::new();
        let path = fixture.workspace.join("target");
        fs::write(&path, b"original").unwrap();
        let observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut request = call("call", "target");
        request.name = tool.into();
        fixture
            .service()
            .mutate_with_authority(
                &fixture.workspace,
                &fixture.run("run"),
                &request,
                "target",
                |_| Ok(b"reviewed candidate".to_vec()),
                || {
                    Ok(Authority {
                        path: path.clone(),
                        observed: observed.clone(),
                    })
                },
            )
            .unwrap();
        assert!(observed.load(std::sync::atomic::Ordering::Acquire));
    }
}

fn persisted_bytes(root: &std::path::Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(
        root: &std::path::Path,
        directory: &std::path::Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn document_checkpoint_publication_evidence_requires_exact_native_binding_and_is_read_only() {
    use anima_core::filesystem_checkpoints::checkpoint_digest;
    for existing in [false, true] {
        let fixture = NativeFixture::new();
        if existing {
            fs::write(fixture.workspace.join("report.docx"), b"original").unwrap();
        }
        let run = fixture.run("document-run");
        let mut request = call("document-apply", "report.docx");
        request.name = "apply_document_edit".into();
        request
            .args
            .insert("proposal_id".into(), DataValue::String("proposal".into()));
        request
            .args
            .insert("review_sha256".into(), DataValue::String("a".repeat(64)));
        let service = fixture.service();
        let receipt = service
            .mutate(&fixture.workspace, &run, &request, "report.docx", |_| {
                Ok(b"candidate".to_vec())
            })
            .unwrap();
        let preimage = existing.then(|| checkpoint_digest(b"original"));
        let postimage = checkpoint_digest(b"candidate");
        let before = persisted_bytes(fixture.temp.path());
        let reloaded = fixture.service();
        let proof = reloaded
            .publication_evidence(
                &fixture.workspace,
                &run,
                &request,
                "report.docx",
                preimage.as_deref(),
                &postimage,
                9,
            )
            .unwrap()
            .unwrap();
        assert_eq!(proof.checkpoint_id, receipt.checkpoint_id);
        assert_eq!(persisted_bytes(fixture.temp.path()), before);
        let mut other_run = run.clone();
        other_run.run_id = "other-run".into();
        let mut other_call = request.clone();
        other_call.id = "other-call".into();
        let mut changed_request = request.clone();
        changed_request
            .args
            .insert("review_sha256".into(), DataValue::String("b".repeat(64)));
        for (run, call, path, preimage, postimage, bytes) in [
            (
                &other_run,
                &request,
                "report.docx",
                preimage.as_deref(),
                postimage.as_str(),
                9,
            ),
            (
                &run,
                &other_call,
                "report.docx",
                preimage.as_deref(),
                postimage.as_str(),
                9,
            ),
            (
                &run,
                &changed_request,
                "report.docx",
                preimage.as_deref(),
                postimage.as_str(),
                9,
            ),
            (
                &run,
                &request,
                "other.docx",
                preimage.as_deref(),
                postimage.as_str(),
                9,
            ),
            (
                &run,
                &request,
                "report.docx",
                Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"),
                postimage.as_str(),
                9,
            ),
            (
                &run,
                &request,
                "report.docx",
                preimage.as_deref(),
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                9,
            ),
            (
                &run,
                &request,
                "report.docx",
                preimage.as_deref(),
                postimage.as_str(),
                10,
            ),
        ] {
            assert!(reloaded
                .publication_evidence(
                    &fixture.workspace,
                    run,
                    call,
                    path,
                    preimage,
                    postimage,
                    bytes
                )
                .ok()
                .flatten()
                .is_none());
        }
        assert_eq!(persisted_bytes(fixture.temp.path()), before);
        fs::write(fixture.store.join("capture.lock"), b"busy").unwrap();
        assert!(reloaded
            .publication_evidence(
                &fixture.workspace,
                &run,
                &request,
                "report.docx",
                preimage.as_deref(),
                &postimage,
                9
            )
            .is_err());
        assert_eq!(
            fs::read(fixture.store.join("capture.lock")).unwrap(),
            b"busy"
        );
    }
}

#[test]
fn document_checkpoint_evidence_never_provisions_missing_storage_or_admits_partial_publication() {
    use anima_core::filesystem_checkpoints::checkpoint_digest;
    let fixture = NativeFixture::new();
    let run = fixture.run("document-run");
    let mut request = call("document-apply", "report.docx");
    request.name = "apply_document_edit".into();
    let hash = checkpoint_digest(b"candidate");
    assert!(fixture
        .service()
        .publication_evidence(
            &fixture.workspace,
            &run,
            &request,
            "report.docx",
            None,
            &hash,
            9
        )
        .is_err());
    assert!(
        !fixture.store.exists(),
        "read-only evidence lookup cannot initialize storage"
    );
    let service = fixture.service();
    service.set_fault(NativeMutationFault::BeforePostimageAck);
    assert!(service
        .mutate(&fixture.workspace, &run, &request, "report.docx", |_| Ok(
            b"candidate".to_vec()
        ))
        .is_err());
    assert_eq!(
        fs::read(fixture.workspace.join("report.docx")).unwrap(),
        b"candidate"
    );
    let before = persisted_bytes(fixture.temp.path());
    assert!(fixture
        .service()
        .publication_evidence(
            &fixture.workspace,
            &run,
            &request,
            "report.docx",
            None,
            &hash,
            9
        )
        .is_err());
    assert_eq!(
        persisted_bytes(fixture.temp.path()),
        before,
        "partial evidence cannot be repaired by inspection"
    );
}

#[test]
fn a6_native_checkpoint_restore_revalidates_authority_after_safety_before_effect() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let service = fixture.service();
    let run = fixture.run("run");
    let receipt = fixture
        .replace(&service, &run, "call", "target", b"native replacement")
        .unwrap();
    let mut checked = false;
    let error = service
        .restore_checked(&fixture.workspace, &run, &receipt.checkpoint_id, || {
            checked = true;
            Err("native restore authority revoked".into())
        })
        .unwrap_err();
    assert!(checked);
    assert!(error.contains("revoked"), "{error}");
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"native replacement"
    );
}

#[test]
fn a6_native_checkpoint_saved_pre_and_post_blobs_are_verified_before_list_or_mutation() {
    for content in [b"original".as_slice(), b"native".as_slice()] {
        for remove in [false, true] {
            let fixture = NativeFixture::new();
            fs::write(fixture.workspace.join("target"), b"original").unwrap();
            let run = fixture.run("run");
            fixture
                .replace(&fixture.service(), &run, "call", "target", b"native")
                .unwrap();
            // Positive control: a genuine committed group is available before
            // faulting a real persisted preimage or postimage object.
            assert_eq!(
                fixture
                    .service()
                    .list(&fixture.workspace, &run)
                    .unwrap()
                    .len(),
                1
            );
            let scope = fs::read_dir(fixture.store.join("scopes"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let digest = anima_core::filesystem_checkpoints::checkpoint_digest(content);
            let object = scope
                .join("objects")
                .join(&digest[..2])
                .join(format!("{digest}.blob"));
            if remove {
                fs::remove_file(&object).unwrap();
            } else {
                fs::write(&object, b"corrupt object").unwrap();
            }
            assert_eq!(
                fixture.service().list(&fixture.workspace, &run),
                Err(NATIVE_RECOVERY_REQUIRED.into())
            );
            fs::write(fixture.workspace.join("next"), b"next owner sentinel").unwrap();
            assert_eq!(
                fixture.replace(&fixture.service(), &run, "next-call", "next", b"bad"),
                Err(NATIVE_RECOVERY_REQUIRED.into())
            );
            assert_eq!(
                fs::read(fixture.workspace.join("next")).unwrap(),
                b"next owner sentinel"
            );
        }
    }
}

#[test]
fn a6_native_checkpoint_preindex_fault_retains_visible_admission_without_capture_commit() {
    let fixture = NativeFixture::new();
    fs::write(fixture.workspace.join("target"), b"original").unwrap();
    let run = fixture.run("run");
    let service = fixture.service();
    service.set_fault(NativeMutationFault::BeforePreimageAck);
    assert_eq!(
        fixture.replace(&service, &run, "call", "target", b"replacement"),
        Err(NATIVE_RECOVERY_REQUIRED.into())
    );
    let scope = fs::read_dir(fixture.store.join("scopes"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read_dir(scope.join("commits")).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(scope.join("native-operations"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        fixture.service().list(&fixture.workspace, &run),
        Err(NATIVE_RECOVERY_REQUIRED.into())
    );
    assert_eq!(
        fs::read(fixture.workspace.join("target")).unwrap(),
        b"original"
    );
}
