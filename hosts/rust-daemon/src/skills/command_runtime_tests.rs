use super::*;
use anima_core::skill_packages::SkillSupportingFile;

fn package() -> SkillPackage {
    SkillPackage {
        bytes: b"---\nname: Command fixture\ndescription: Approved local command fixture\n---\nRun the documented setup.\n".to_vec(),
        files: vec![SkillSupportingFile::from_bytes(
            "fixture.txt".into(),
            b"approved original\n".to_vec(),
        )
        .unwrap()],
    }
}

fn request(action: RuntimeAction) -> CommandRuntimeRequest {
    CommandRuntimeRequest {
        action,
        slug: "fixture".into(),
        hash: package().hash(),
        command: None,
        cwd: None,
        state_hash: None,
        timeout_ms: 10_000,
    }
}

fn allow() -> RuntimeAuthority {
    Arc::new(|| Box::pin(async { Ok(()) }))
}

#[test]
fn command_request_requires_exact_state_and_preserves_literal_command() {
    let mut value = request(RuntimeAction::Run);
    value.command = Some("echo first && echo second".into());
    assert!(value.validate().is_err());
    value.state_hash = Some("a".repeat(64));
    assert!(value.validate().is_ok());
    assert_eq!(value.command.as_deref(), Some("echo first && echo second"));
    for cwd in [
        "../escape",
        "/absolute",
        "C:/escape",
        "a\\b",
        "a/../b",
        "a:stream",
    ] {
        value.cwd = Some(cwd.into());
        assert!(value.validate().is_err(), "{cwd}");
    }
    value.cwd = Some("scripts".into());
    assert!(value.validate().is_ok());
    value.command = Some(" ".into());
    assert!(value.validate().is_err());
    value.command = Some("x".repeat(16 * 1024 + 1));
    assert!(value.validate().is_err());
    assert!(
        serde_json::from_value::<CommandRuntimeRequest>(serde_json::json!({
            "action":"prepare", "slug":"fixture", "hash":package().hash(), "executable":"bad"
        }))
        .is_err()
    );
}

#[tokio::test]
async fn command_status_is_read_only_and_prepare_preserves_approved_source() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let installed = package();
    super::super::package::write_package(workspace.path(), "fixture", &installed).unwrap();
    let status = execute(
        root.clone(),
        workspace.path().into(),
        installed.clone(),
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "installed");
    assert!(!root.exists(), "status must not create any runtime storage");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        installed.clone(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.status, "environment_prepared");
    assert!(prepared.state_hash.is_some());
    assert!(prepared.last_check.is_none());
    let cwd = PathBuf::from(prepared.working_directory.unwrap());
    std::fs::write(cwd.join("fixture.txt"), "mutable dependency copy").unwrap();
    assert_eq!(
        super::super::package::read_package(workspace.path(), "fixture")
            .unwrap()
            .unwrap()
            .hash(),
        installed.hash()
    );
    let status = execute(
        root,
        workspace.path().into(),
        installed,
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "needs_prepare");
    assert!(status.state_hash.is_none());
}

#[tokio::test]
async fn command_status_invalidates_mutated_private_home_and_source_snapshot() {
    for changed in ["home/config.txt", "source/skills/fixture/fixture.txt"] {
        let private = crate::document_edits::store::test_support::private_directory();
        let workspace = tempfile::tempdir().unwrap();
        let root = private.path().join("runtimes");
        let prepared = execute(
            root.clone(),
            workspace.path().into(),
            package(),
            request(RuntimeAction::Prepare),
            CancelSignal::new(),
            allow(),
        )
        .await
        .unwrap();
        let manifest = read_manifest(
            &private_handle(&root).unwrap(),
            &manifest_name(&request(RuntimeAction::Status)),
        )
        .unwrap()
        .unwrap();
        std::fs::write(
            root.join(manifest.directory).join(changed),
            "changed out of band",
        )
        .unwrap();
        let status = execute(
            root,
            workspace.path().into(),
            package(),
            request(RuntimeAction::Status),
            CancelSignal::new(),
            allow(),
        )
        .await
        .unwrap();
        assert_eq!(
            status.status, "needs_prepare",
            "{changed}; prepared={prepared:?}"
        );
        assert!(status.last_check.is_none());
    }
}

#[tokio::test]
async fn command_queue_wait_observes_cancellation_and_owns_no_files() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let locking = runtime::lease(&root).unwrap();
    let _occupied = locking.lock().await;
    let cancel = CancelSignal::new();
    let task = tokio::spawn(execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        cancel.clone(),
        allow(),
    ));
    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("cancelled"));
    assert!(!root.exists());
}

#[tokio::test]
async fn command_queue_deadline_includes_admission_wait() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let locking = runtime::lease(&root).unwrap();
    let _occupied = locking.lock().await;
    let mut value = request(RuntimeAction::Prepare);
    value.timeout_ms = 1_000;
    let task = tokio::spawn(execute(
        root.clone(),
        workspace.path().into(),
        package(),
        value,
        CancelSignal::new(),
        allow(),
    ));
    let error = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("timed out"));
    assert!(
        !root.exists(),
        "expired admission must not initialize storage"
    );
}

#[tokio::test]
async fn command_changed_shell_identity_and_invalid_manifest_require_preparation() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let name = manifest_name(&request(RuntimeAction::Status));
    let held = private_handle(&root).unwrap();
    let manifest = read_manifest(&held, &name).unwrap().unwrap();
    let mut changed = manifest.clone();
    changed.shell.identity = "0".repeat(64);
    write_manifest(&held, &name, Some(&manifest), &changed).unwrap();
    let status = execute(
        root,
        workspace.path().into(),
        package(),
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "needs_prepare");
    assert!(status.state_hash.is_none());
    assert!(status.last_check.is_none());
}

#[tokio::test]
async fn command_unresolved_custody_blocks_reuse_and_remains_in_drain_accounting() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let physical = runtime::PhysicalLease::admit(&root, runtime::OPERATION_MARKER.into()).unwrap();
    physical.settled.store(false, Ordering::Release);
    drop(physical);
    let status = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "needs_prepare");
    assert_eq!(runtime::persisted_marker_count(&root), 1);
    assert!(execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow()
    )
    .await
    .unwrap_err()
    .contains("unresolved physical custody"));
    assert!(root.join(runtime::OPERATION_MARKER).is_file());
}

#[cfg(unix)]
#[test]
fn command_seal_covers_permissions_and_replacement_identity() {
    use std::os::unix::fs::PermissionsExt;
    let private = crate::document_edits::store::test_support::private_directory();
    let path = private.path().join("entry.sh");
    std::fs::write(&path, "echo fixture").unwrap();
    let cancel = CancelSignal::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let before = tree_seal(private.path(), deadline, &cancel, &cancel).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_ne!(
        before,
        tree_seal(private.path(), deadline, &cancel, &cancel).unwrap()
    );
    let before = tree_seal(private.path(), deadline, &cancel, &cancel).unwrap();
    std::fs::rename(&path, private.path().join("old.sh")).unwrap();
    std::fs::write(&path, "echo fixture").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_file(private.path().join("old.sh")).unwrap();
    assert_ne!(
        before,
        tree_seal(private.path(), deadline, &cancel, &cancel).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn command_seal_accepts_internal_dependency_links_and_rejects_escape_and_cycles() {
    use std::os::unix::fs::symlink;
    let private = crate::document_edits::store::test_support::private_directory();
    let root = private.path();
    std::fs::create_dir_all(root.join("work/node_modules/.bin")).unwrap();
    std::fs::create_dir_all(root.join("work/node_modules/example")).unwrap();
    std::fs::write(
        root.join("work/node_modules/example/cli.js"),
        "console.log('ok')",
    )
    .unwrap();
    symlink(
        "../example/cli.js",
        root.join("work/node_modules/.bin/example"),
    )
    .unwrap();
    let cancel = CancelSignal::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    assert!(tree_seal(root, deadline, &cancel, &cancel).is_ok());
    symlink("/etc/passwd", root.join("escape")).unwrap();
    assert!(tree_seal(root, deadline, &cancel, &cancel).is_err());
    std::fs::remove_file(root.join("escape")).unwrap();
    symlink("..", root.join("work/cycle")).unwrap();
    assert!(tree_seal(root, deadline, &cancel, &cancel).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn command_mutable_root_cannot_alias_the_original_source_snapshot() {
    use std::os::unix::fs::symlink;
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let manifest = read_manifest(
        &private_handle(&root).unwrap(),
        &manifest_name(&request(RuntimeAction::Status)),
    )
    .unwrap()
    .unwrap();
    let environment = root.join(manifest.directory);
    let work = work_directory(&environment, "fixture");
    std::fs::rename(&work, environment.join("work/saved-original-copy")).unwrap();
    symlink(environment.join("source/skills/fixture"), &work).unwrap();
    assert!(selected_cwd(&environment, "fixture", ".").is_err());
    let status = execute(
        root,
        workspace.path().into(),
        package(),
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "needs_prepare");
}

#[cfg(windows)]
#[tokio::test]
async fn command_exact_shell_execution_reseals_mutations_and_failure_invalidates_success() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let expected_cwd = PathBuf::from(prepared.working_directory.as_ref().unwrap());
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some(
        "echo created>generated.txt && echo \"literal-shell-command\" && echo cwd:%CD%".into(),
    );
    let checked = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        run.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(checked.status, "command_verified", "{checked:?}");
    assert!(checked
        .output
        .as_ref()
        .unwrap()
        .stdout
        .contains("literal-shell-command"));
    let actual_cwd = checked
        .output
        .as_ref()
        .unwrap()
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("cwd:"))
        .expect("CMD must report its actual working directory");
    assert_eq!(
        PathBuf::from(actual_cwd).canonicalize().unwrap(),
        expected_cwd.canonicalize().unwrap(),
        "CMD must not fall back to its Windows directory"
    );
    assert!(expected_cwd.join("generated.txt").is_file());
    assert_eq!(
        checked.last_check.as_ref().unwrap().command,
        run.command.clone().unwrap()
    );
    assert_ne!(checked.state_hash, run.state_hash);
    assert!(execute(
        root.clone(),
        workspace.path().into(),
        package(),
        run.clone(),
        CancelSignal::new(),
        allow()
    )
    .await
    .unwrap_err()
    .contains("state"));
    run.state_hash = checked.state_hash;
    run.command = Some("echo failed>failed.txt && exit /b 7".into());
    let failed = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        run,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(failed.status, "command_failed");
    assert_eq!(failed.output.unwrap().exit_code, Some(7));
    assert!(failed.last_check.is_none());
    assert!(
        failed.state_hash.is_some(),
        "known command failure remains recoverable"
    );
    let status = execute(
        root,
        workspace.path().into(),
        package(),
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "environment_prepared");
    assert!(status.last_check.is_none());
}

#[cfg(windows)]
#[tokio::test]
async fn command_shell_receives_private_home_without_ambient_secrets_or_hooks() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some("(if defined OPENAI_API_KEY exit /b 11) & (if defined PYTHONPATH exit /b 12) & (if defined NODE_OPTIONS exit /b 13) & (if defined ANIMAOS_SKILL_PYTHON exit /b 14) & echo %HOME% & echo %NPM_CONFIG_USERCONFIG%".into());
    let checked = execute(
        root,
        workspace.path().into(),
        package(),
        run,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(checked.status, "command_verified", "{checked:?}");
    assert!(checked.output.unwrap().stdout.contains("home"));
}

#[cfg(windows)]
#[tokio::test]
async fn command_fixed_shell_enables_extensions_and_disables_delayed_expansion() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let manifest = read_manifest(
        &private_handle(&root).unwrap(),
        &manifest_name(&request(RuntimeAction::Status)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(manifest.shell.mode, "cmd-d-eon-voff-s-c-v2");
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some("set ANIMA_LITERAL=expanded& echo !ANIMA_LITERAL!& (if defined HOME echo extensions-enabled)".into());
    let checked = execute(
        root,
        workspace.path().into(),
        package(),
        run,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(checked.status, "command_verified", "{checked:?}");
    let output = checked.output.unwrap().stdout;
    assert!(output.contains("!ANIMA_LITERAL!"), "{output}");
    assert!(output.contains("extensions-enabled"), "{output}");
}

#[cfg(windows)]
#[tokio::test]
async fn command_storage_limit_retires_and_removes_only_its_owned_environment() {
    for fast_exit in [false, true] {
        let private = crate::document_edits::store::test_support::private_directory();
        let workspace = tempfile::tempdir().unwrap();
        let sentinel = private.path().join("owner-sentinel.txt");
        std::fs::write(&sentinel, "keep owner content").unwrap();
        let root = private.path().join("runtimes");
        let prepared = execute(
            root.clone(),
            workspace.path().into(),
            package(),
            request(RuntimeAction::Prepare),
            CancelSignal::new(),
            allow(),
        )
        .await
        .unwrap();
        let working_directory = PathBuf::from(prepared.working_directory.unwrap());
        let mut run = request(RuntimeAction::Run);
        run.state_hash = prepared.state_hash;
        run.command = Some(format!(
            "(for /l %i in (1,1,512) do @echo 01234567890123456789>>oversized.txt){}",
            if fast_exit {
                ""
            } else {
                " & ping -n 60 127.0.0.1 >nul"
            }
        ));
        let failed = execute_with_limits(
            root.clone(),
            workspace.path().into(),
            package(),
            run,
            CancelSignal::new(),
            allow(),
            StorageLimits {
                bytes: 4096,
                entries: 1000,
                depth: MAX_DEPTH,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            failed.status, "command_failed",
            "fast_exit={fast_exit}; {failed:?}"
        );
        assert!(failed.last_check.is_none());
        assert!(failed.state_hash.is_none());
        let output = failed.output.unwrap();
        assert_eq!(
            output.stop.as_deref(),
            Some("storage_limit"),
            "fast_exit={fast_exit}; {output:?}"
        );
        assert!(output.cleanup_confirmed, "{output:?}");
        assert!(
            !working_directory.exists(),
            "known-retired over-budget storage must be removed"
        );
        assert_eq!(runtime::persisted_marker_count(&root), 0);
        assert_eq!(
            std::fs::read_to_string(&sentinel).unwrap(),
            "keep owner content"
        );
        let status = execute(
            root,
            workspace.path().into(),
            package(),
            request(RuntimeAction::Status),
            CancelSignal::new(),
            allow(),
        )
        .await
        .unwrap();
        assert_eq!(status.status, "needs_prepare");
    }
}

#[test]
fn command_storage_cleanup_identity_failure_retains_the_reuse_fence() {
    let private = crate::document_edits::store::test_support::private_directory();
    let root = private.path();
    let directory = format!("r-{}", uuid::Uuid::new_v4().simple());
    crate::document_edits::store::initialize_owned_directory(&root.join(&directory)).unwrap();
    let physical = runtime::PhysicalLease::admit(root, runtime::OPERATION_MARKER.into()).unwrap();
    let result = storage_failure(
        &private_handle(root).unwrap(),
        &directory,
        (0, 0),
        &physical,
        StorageViolation::Limit("fixture storage limit"),
        RuntimeOutput {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            stop: Some("storage_limit".into()),
            cleanup_confirmed: true,
        },
    );
    assert_eq!(result.status, "command_failed");
    assert!(!result.output.unwrap().cleanup_confirmed);
    assert!(root.join(directory).is_dir());
    drop(physical);
    assert_eq!(runtime::persisted_marker_count(root), 1);
    assert!(root.join(runtime::OPERATION_MARKER).is_file());
}

#[cfg(windows)]
#[tokio::test]
async fn command_stop_retires_descendants_and_leaves_no_successful_receipt() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some("ping -n 60 127.0.0.1 >nul".into());
    let cancel = CancelSignal::new();
    let stopping = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        stopping.cancel();
    });
    let stopped = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        run,
        cancel,
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(stopped.status, "command_failed");
    assert!(stopped.last_check.is_none());
    let output = stopped.output.unwrap();
    assert_eq!(output.stop.as_deref(), Some("cancelled"));
    assert!(output.cleanup_confirmed);
    assert_eq!(runtime::persisted_marker_count(&root), 0);
}

#[cfg(windows)]
#[tokio::test]
async fn command_deadline_retires_descendants_and_preserves_invalidation() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some("ping -n 60 127.0.0.1 >nul".into());
    run.timeout_ms = 1_000;
    let stopped = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        run,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(stopped.status, "command_failed");
    let output = stopped.output.unwrap();
    assert_eq!(output.stop.as_deref(), Some("timed_out"));
    assert!(output.cleanup_confirmed);
    assert_eq!(runtime::persisted_marker_count(&root), 0);
    let status = execute(
        root,
        workspace.path().into(),
        package(),
        request(RuntimeAction::Status),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "needs_prepare");
    assert!(status.last_check.is_none());
}

#[cfg(windows)]
#[tokio::test]
async fn command_revoked_authority_retires_process_without_a_receipt() {
    use std::sync::atomic::AtomicBool;
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some("ping -n 60 127.0.0.1 >nul".into());
    let revoked = Arc::new(AtomicBool::new(false));
    let observed = revoked.clone();
    let authority: RuntimeAuthority = Arc::new(move || {
        let allowed = !observed.load(Ordering::Acquire);
        Box::pin(async move {
            if allowed {
                Ok(())
            } else {
                Err("fixture approval revoked".into())
            }
        })
    });
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        revoked.store(true, Ordering::Release);
    });
    let stopped = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        run,
        CancelSignal::new(),
        authority,
    )
    .await
    .unwrap();
    assert_eq!(stopped.status, "command_failed");
    let output = stopped.output.unwrap();
    assert_eq!(output.stop.as_deref(), Some("authority_revoked"));
    assert!(output.cleanup_confirmed);
    assert!(stopped.last_check.is_none());
    assert_eq!(runtime::persisted_marker_count(&root), 0);
}

#[cfg(windows)]
#[tokio::test]
async fn command_missing_program_reports_actual_failure_and_recoverable_state() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package(),
        request(RuntimeAction::Prepare),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let mut run = request(RuntimeAction::Run);
    run.state_hash = prepared.state_hash;
    run.command = Some("anima-fixture-intentionally-missing-command".into());
    let failed = execute(
        root,
        workspace.path().into(),
        package(),
        run,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(failed.status, "command_failed");
    assert!(failed.state_hash.is_some());
    assert!(failed.last_check.is_none());
    let output = failed.output.unwrap();
    assert_ne!(output.exit_code, Some(0));
    assert!(output
        .stderr
        .contains("anima-fixture-intentionally-missing-command"));
    assert!(output.cleanup_confirmed);
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires installed Node/npm and Bun; installs only the local zero-dependency fixture, no network"]
async fn physical_command_node_npm_bun_sequence_preserves_originals_and_seals_dependencies() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let mut installed = package();
    installed.files.extend([
        SkillSupportingFile::from_bytes("package.json".into(), br#"{"name":"anima-local-fixture","private":true,"version":"1.0.0","scripts":{"check":"node check.js"}}"#.to_vec()).unwrap(),
        SkillSupportingFile::from_bytes("check.js".into(), b"const fs=require('node:fs'); if(fs.readFileSync('fixture.txt','utf8')!=='approved original\\n') process.exit(9); console.log('local-js-fixture-passed');\n".to_vec()).unwrap(),
    ]);
    super::super::package::write_package(workspace.path(), "fixture", &installed).unwrap();
    let mut value = request(RuntimeAction::Prepare);
    value.hash = installed.hash();
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        installed.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    let mut state = prepared.state_hash;
    for command in [
        "npm install --offline --ignore-scripts --no-audit --no-fund",
        "npm run check",
        "bun run check.js",
    ] {
        value.action = RuntimeAction::Run;
        value.command = Some(command.into());
        value.state_hash = state;
        let checked = execute(
            root.clone(),
            workspace.path().into(),
            installed.clone(),
            value.clone(),
            CancelSignal::new(),
            allow(),
        )
        .await
        .unwrap();
        assert_eq!(checked.status, "command_verified", "{command}: {checked:?}");
        assert!(checked.output.as_ref().unwrap().cleanup_confirmed);
        state = checked.state_hash;
    }
    assert_eq!(
        super::super::package::read_package(workspace.path(), "fixture")
            .unwrap()
            .unwrap()
            .hash(),
        installed.hash()
    );
    assert!(PathBuf::from(prepared.working_directory.unwrap())
        .join("package-lock.json")
        .is_file());
}
