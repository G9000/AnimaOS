use super::*;
use anima_core::skill_packages::SkillSupportingFile;

fn request(action: RuntimeAction) -> PythonRuntimeRequest {
    PythonRuntimeRequest {
        action,
        slug: "research".into(),
        hash: "a".repeat(64),
        entrypoint: None,
        args: Vec::new(),
        requirements: None,
        timeout_ms: 30_000,
    }
}

#[test]
fn runtime_requests_bind_exact_package_and_refuse_execution_options() {
    assert!(request(RuntimeAction::Status).validate().is_ok());
    for path in [
        "../escape.py",
        "/escape.py",
        "scripts/x.py:stream",
        "scripts\\x.py",
        "-c",
    ] {
        let mut value = request(RuntimeAction::Run);
        value.entrypoint = Some(path.into());
        assert!(value.validate().is_err(), "{path}");
    }
    let mut value = request(RuntimeAction::Run);
    value.entrypoint = Some("scripts/research.py".into());
    value.args = vec!["--help".into()];
    assert!(value.validate().is_ok());
    value.hash = "latest".into();
    assert!(value.validate().is_err());
}

#[test]
fn dependency_input_accepts_distribution_specs_and_refuses_alternate_sources() {
    assert!(validate_requirements(
        b"# Reviewed dependencies\nrequests>=2.31,<3\nhttpx[socks]==0.28.1\n"
    )
    .is_ok());
    for bytes in [
        b"--extra-index-url https://example.invalid/simple".as_slice(),
        b"-r another.txt",
        b"-e .",
        b"https://example.invalid/a.whl",
        b"thing @ file:///secret",
        b"../dependency",
        b"thing --config-settings=x=y",
        b"thing\\\n--index-url https://example.invalid",
        b"thing\0evil",
    ] {
        assert!(validate_requirements(bytes).is_err());
    }
}

#[test]
fn readiness_is_scoped_to_exact_successful_command_and_environment() {
    let check = RuntimeCheck {
        entrypoint: "scripts/research.py".into(),
        args: vec!["--help".into()],
        exit_code: 0,
        checked_at: 42,
    };
    let value = serde_json::to_value(check).unwrap();
    assert_eq!(value["entrypoint"], "scripts/research.py");
    assert_eq!(value["args"][0], "--help");
    assert!(value.get("ready").is_none());
}

#[test]
fn requirement_selection_uses_reviewed_package_bytes_and_requires_explicit_none() {
    let package = super::super::package::SkillPackage {
        bytes: b"skill".to_vec(),
        files: vec![SkillSupportingFile {
            path: "requirements.txt".into(),
            content: "requests==2.32.5".into(),
            encoding: Default::default(),
        }],
    };
    let selected = select_requirements(&package, None).unwrap();
    assert_eq!(selected.unwrap().0, "requirements.txt");
    assert!(select_requirements(&package, Some("")).unwrap().is_none());
    assert!(select_requirements(&package, Some("missing.txt")).is_err());
}

#[test]
fn interpreter_selection_rejects_workspace_binaries() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("python.exe");
    std::fs::write(&path, b"untrusted workspace executable").unwrap();
    assert!(trusted_interpreter(&path, temp.path()).is_err());
}

#[test]
fn helper_path_keeps_installed_tools_and_excludes_project_shims_and_duplicates() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let runtime = temporary.path().join("runtimes");
    let installed = temporary.path().join("installed-node");
    let nested = workspace.join("bin");
    let managed = runtime.join("r-example/venv/Scripts");
    let shims = temporary.path().join("project/node_modules/.bin");
    for path in [&workspace, &runtime, &installed, &nested, &managed, &shims] {
        std::fs::create_dir_all(path).unwrap();
    }
    let path = std::env::join_paths([
        installed.clone(),
        installed.join("."),
        PathBuf::from("relative-bin"),
        workspace.clone(),
        nested,
        runtime.clone(),
        managed,
        shims,
        temporary.path().join("missing-tool-dir"),
    ])
    .unwrap();
    assert_eq!(
        sanitized_helper_directories(&path, &workspace, &runtime),
        vec![installed.canonicalize().unwrap()]
    );
    let first = installed.canonicalize().unwrap();
    let mut merged = vec![first.clone(), workspace.clone()];
    append_unique_directories(&mut merged, &[first]);
    assert_eq!(
        merged.len(),
        2,
        "installed helpers cannot duplicate earlier fixed directories"
    );
}

#[cfg(windows)]
#[test]
fn shell_search_path_skips_unusable_helpers_but_requires_fixed_directories() {
    let private = crate::document_edits::store::test_support::private_directory();
    let fixed = private.path().join("fixed");
    let helper = private.path().join("helper with spaces");
    let missing = private.path().join("missing");
    let file = private.path().join("file");
    std::fs::create_dir(&fixed).unwrap();
    std::fs::create_dir(&helper).unwrap();
    std::fs::write(&file, "not a search directory").unwrap();
    // Verbatim Windows paths can name directories CMD cannot safely represent.
    let reserved = private.path().canonicalize().unwrap().join("NUL");
    std::fs::create_dir(&reserved).unwrap();
    assert!(shell_process_path(&reserved).is_err());
    let helpers = [
        missing.clone(),
        reserved,
        file,
        helper.clone(),
        fixed.clone(),
    ];
    assert_eq!(
        shell_search_path(std::slice::from_ref(&fixed), &helpers).unwrap(),
        vec![
            shell_process_path(&fixed).unwrap(),
            shell_process_path(&helper).unwrap()
        ]
    );
    assert!(shell_search_path(&[missing], &helpers).is_err());
}

#[cfg(windows)]
#[test]
fn shell_process_paths_use_normal_spellings_without_changing_filesystem_identity() {
    use std::path::{Component, Prefix};
    let private = crate::document_edits::store::test_support::private_directory();
    let directory = private.path().join("normal directory with spaces");
    std::fs::create_dir(&directory).unwrap();
    let canonical = directory.canonicalize().unwrap();
    let normal = shell_process_path(&canonical).unwrap();
    assert!(
        matches!(normal.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
    );
    assert_eq!(
        shell_path_identity(&canonical).unwrap(),
        shell_path_identity(&normal).unwrap()
    );
    assert_eq!(normal.canonicalize().unwrap(), canonical);
    assert_eq!(shell_current_directory(&canonical).unwrap(), normal);
    let file = directory.join("fixture.cmd");
    std::fs::write(&file, "@echo fixture").unwrap();
    let normal_file = shell_process_path(&file.canonicalize().unwrap()).unwrap();
    assert_eq!(
        shell_path_identity(&file).unwrap(),
        shell_path_identity(&normal_file).unwrap()
    );
    for name in [
        "NUL",
        "con.txt",
        "COM1.log",
        "LPT².txt",
        "trailing.",
        "trailing ",
        "name:stream",
        "wild?card",
    ] {
        assert!(!normal_windows_component(name), "{name}");
    }
    assert!(normal_windows_component("ordinary dependency-1.0"));
}

#[test]
fn environment_seal_covers_dependency_bytes_and_python_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let venv = temp.path().join("venv");
    std::fs::create_dir_all(venv.join("Lib/site-packages/example")).unwrap();
    std::fs::write(
        venv.join("pyvenv.cfg"),
        "include-system-site-packages = false",
    )
    .unwrap();
    let dependency = venv.join("Lib/site-packages/example/__init__.py");
    std::fs::write(&dependency, "VALUE = 1").unwrap();
    let original = environment_hash(temp.path()).unwrap();
    std::fs::write(&dependency, "VALUE = 2").unwrap();
    assert_ne!(original, environment_hash(temp.path()).unwrap());
    std::fs::write(&dependency, "VALUE = 1").unwrap();
    assert_eq!(original, environment_hash(temp.path()).unwrap());
    std::fs::write(
        venv.join("pyvenv.cfg"),
        "include-system-site-packages = true",
    )
    .unwrap();
    assert_ne!(original, environment_hash(temp.path()).unwrap());
}

#[test]
fn failed_environment_cleanup_never_removes_an_unowned_or_redirected_path() {
    let private = crate::document_edits::store::test_support::private_directory();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("owner-file.txt");
    std::fs::write(&sentinel, "keep").unwrap();
    assert!(remove_owned_environment(private.path(), "../owner-file.txt").is_err());
    assert!(remove_owned_environment(private.path(), "r-not-a-uuid").is_err());
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "keep");
}

#[test]
fn runtime_cleanup_failure_persists_marker_and_refuses_admission() {
    let private = crate::document_edits::store::test_support::private_directory();
    let root = private.path();
    let name = OPERATION_MARKER;
    assert_eq!(persisted_marker_count(root), 0);
    let physical = PhysicalLease::admit(root, name.into()).unwrap();
    physical.settled.store(false, Ordering::Release);
    drop(physical);
    assert!(root.join(name).is_file());
    assert_eq!(persisted_marker_count(root), 1);
    assert!(PhysicalLease::admit(root, name.into()).is_err());
    // This fixture has no child. Remove only its synthetic marker and quarantine
    // entry after proving that fact, so it does not alter other tests' drain state.
    std::fs::remove_file(root.join(name)).unwrap();
    assert_eq!(
        persisted_marker_count(root),
        1,
        "native quarantine survives external marker deletion"
    );
    QUARANTINED
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&operation_key(root).unwrap());
    assert_eq!(persisted_marker_count(root), 0);
}

#[test]
fn runtime_quarantine_survives_aliases_removed_marker_and_root_recreation() {
    let private = crate::document_edits::store::test_support::private_directory();
    let root = private.path();
    let alias = root
        .parent()
        .unwrap()
        .join(".")
        .join(root.file_name().unwrap());
    #[cfg(windows)]
    let alias = PathBuf::from(alias.as_os_str().to_string_lossy().to_uppercase());
    let logical_key = operation_key(root).unwrap();
    assert_eq!(operation_key(&alias).unwrap(), logical_key);
    assert!(Arc::ptr_eq(&lease(root).unwrap(), &lease(&alias).unwrap()));
    let physical = PhysicalLease::admit(root, OPERATION_MARKER.into()).unwrap();
    physical.settled.store(false, Ordering::Release);
    drop(physical);
    std::fs::remove_file(root.join(OPERATION_MARKER)).unwrap();
    assert_eq!(persisted_marker_count(&alias), 1);
    std::fs::remove_dir(root).unwrap();
    assert_eq!(persisted_marker_count(&alias), 1);
    crate::document_edits::store::initialize_owned_directory(root).unwrap();
    assert_eq!(operation_key(&alias).unwrap(), logical_key);
    assert_eq!(persisted_marker_count(&alias), 1);
    assert!(PhysicalLease::admit(&alias, OPERATION_MARKER.into()).is_err());
    QUARANTINED
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&logical_key);
    assert_eq!(persisted_marker_count(root), 0);
}

#[tokio::test]
async fn delayed_authority_cannot_launch_or_publish_after_deadline_or_stop() {
    let cancel = CancelSignal::new();
    let call = CancelSignal::new();
    assert!(ensure_operation_live(
        tokio::time::Instant::now() - Duration::from_millis(1),
        &cancel,
        &call
    )
    .unwrap_err()
    .contains("timed out"));
    cancel.cancel();
    assert!(ensure_operation_live(
        tokio::time::Instant::now() + Duration::from_secs(1),
        &cancel,
        &call
    )
    .unwrap_err()
    .contains("cancelled"));
    let pending: RuntimeAuthority = Arc::new(|| Box::pin(std::future::pending()));
    assert!(require_authority(
        &pending,
        tokio::time::Instant::now() + Duration::from_millis(20),
        &CancelSignal::new(),
        &call
    )
    .await
    .unwrap_err()
    .contains("timed out"));
}

async fn assert_queued_request_settles(cancel_queued: bool) {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let root = temporary.path().join("runtimes");
    let locking = lease(&root).unwrap();
    let _occupied = locking.lock().await;
    let package = super::super::package::SkillPackage {
        bytes: b"reviewed skill".to_vec(),
        files: Vec::new(),
    };
    let mut value = request(RuntimeAction::Prepare);
    value.hash = package.hash();
    value.timeout_ms = 1000;
    let called = Arc::new(AtomicBool::new(false));
    let observed = called.clone();
    let authority: RuntimeAuthority = Arc::new(move || {
        observed.store(true, Ordering::Release);
        Box::pin(async { Ok(()) })
    });
    let cancel = CancelSignal::new();
    let task = tokio::spawn(execute(
        root.clone(),
        workspace.path().into(),
        package,
        value,
        cancel.clone(),
        authority,
    ));
    if cancel_queued {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    }
    let error = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("a queued request must settle while the other operation still holds its lease")
        .unwrap()
        .unwrap_err();
    assert!(error.contains(if cancel_queued {
        "cancelled"
    } else {
        "timed out"
    }));
    assert!(
        !called.load(Ordering::Acquire),
        "queued requests never acquire effect authority"
    );
    assert!(
        !root.exists(),
        "a cancelled or expired queued request creates no runtime files"
    );
    // The owned worker joined and released its admission/Active guards. Counting
    // this unique lease avoids depending on unrelated concurrent tests' jobs.
    assert_eq!(Arc::strong_count(&locking), 1);
}

#[tokio::test]
async fn queued_runtime_request_observes_stop_before_other_operation_finishes() {
    assert_queued_request_settles(true).await;
}

#[tokio::test]
async fn queued_runtime_request_deadline_includes_global_admission_wait() {
    assert_queued_request_settles(false).await;
}

#[cfg(windows)]
fn physical_package(script: &str) -> super::super::package::SkillPackage {
    assert!(std::env::var_os("ANIMAOS_SKILL_PYTHON").is_some(), "select an installed Python >=3.12 with ANIMAOS_SKILL_PYTHON before running this physical acceptance test");
    super::super::package::SkillPackage {
        bytes: b"---\nname: Runtime acceptance\ndescription: Physical managed runtime fixture\n---\nRun the reviewed Python entrypoint.\n".to_vec(),
        files: vec![
            SkillSupportingFile::from_bytes("scripts/check.py".into(), script.as_bytes().to_vec()).unwrap(),
            SkillSupportingFile::from_bytes("scripts/helper.py".into(), b"VALUE = 42\n".to_vec()).unwrap(),
            SkillSupportingFile::from_bytes("assets/example.png".into(), vec![137,80,78,71,0,1,2]).unwrap(),
        ],
    }
}

#[cfg(windows)]
fn allow() -> RuntimeAuthority {
    Arc::new(|| Box::pin(async { Ok(()) }))
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires explicitly selected real Python >=3.12; creates venv, no network"]
async fn physical_python_preparation_and_exact_preflight_preserve_assets_and_clean_environment() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let package = physical_package("import os, sys, pathlib\nfrom helper import VALUE\np = pathlib.Path(__file__).parents[1]\nassert VALUE == 42\nassert (p/'assets/example.png').read_bytes() == bytes([137,80,78,71,0,1,2])\nassert sys.prefix != sys.base_prefix\nassert os.environ['HOME'] != os.environ.get('ORIGINAL_HOME')\nassert os.environ['PIP_CONFIG_FILE'] == os.devnull\nassert 'ANIMAOS_SKILL_PYTHON' not in os.environ\nassert 'PYTHONPATH' not in os.environ\nassert sys.argv[1:] == ['--help']\nprint('actual-preflight-passed')\n");
    let mut value = request(RuntimeAction::Status);
    value.hash = package.hash();
    value.requirements = Some(String::new());
    value.timeout_ms = 120_000;
    let status = execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "installed");
    assert!(!root.exists(), "status does not create runtime directories");
    value.action = RuntimeAction::Prepare;
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.status, "environment_prepared");
    assert!(prepared.last_check.is_none());
    value.action = RuntimeAction::Run;
    value.entrypoint = Some("scripts/check.py".into());
    value.args = vec!["--help".into()];
    let checked = execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(checked.status, "command_verified");
    assert_eq!(checked.last_check.unwrap().args, ["--help"]);
    let output = checked.output.unwrap();
    assert_eq!(output.exit_code, Some(0));
    assert!(output.cleanup_confirmed);
    assert!(output.stdout.contains("actual-preflight-passed"));
    value.action = RuntimeAction::Status;
    value.entrypoint = None;
    value.args.clear();
    let status = execute(
        root,
        workspace.path().into(),
        package,
        value,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(status.status, "command_verified");
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires explicitly selected real Python >=3.12; creates venv, no network"]
async fn physical_python_mutated_source_invalidates_preflight_and_future_execution() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let package =
        physical_package("from pathlib import Path\nPath(__file__).write_text('print(123)')\n");
    let mut value = request(RuntimeAction::Prepare);
    value.hash = package.hash();
    value.requirements = Some(String::new());
    value.timeout_ms = 120_000;
    execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    value.action = RuntimeAction::Run;
    value.entrypoint = Some("scripts/check.py".into());
    assert!(execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow()
    )
    .await
    .unwrap_err()
    .contains("snapshot changed"));
    value.action = RuntimeAction::Status;
    value.entrypoint = None;
    let status = execute(
        root,
        workspace.path().into(),
        package,
        value,
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
#[ignore = "requires explicitly selected real Python >=3.12; creates venv, no network"]
async fn physical_python_stop_retires_process_and_records_no_success() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let package = physical_package("import time\nprint('started', flush=True)\ntime.sleep(60)\n");
    let mut value = request(RuntimeAction::Prepare);
    value.hash = package.hash();
    value.requirements = Some(String::new());
    value.timeout_ms = 120_000;
    execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    value.action = RuntimeAction::Run;
    value.entrypoint = Some("scripts/check.py".into());
    let cancel = CancelSignal::new();
    let stopped = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        stopped.cancel();
    });
    let result = execute(
        root,
        workspace.path().into(),
        package,
        value,
        cancel,
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(result.status, "command_failed");
    assert!(result.last_check.is_none());
    let output = result.output.unwrap();
    assert_eq!(output.stop.as_deref(), Some("cancelled"));
    assert!(output.cleanup_confirmed);
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "downloads pinned idna==3.10 wheel from public PyPI into an isolated fixture environment"]
async fn physical_python_wheel_dependencies_are_installed_and_actually_importable() {
    let private = crate::document_edits::store::test_support::private_directory();
    let workspace = tempfile::tempdir().unwrap();
    let root = private.path().join("runtimes");
    let mut package = physical_package("from importlib.metadata import version\nimport idna\nassert version('idna') == '3.10'\nassert idna.encode('example.com') == b'example.com'\nprint('dependency-preflight-passed')\n");
    package.files.push(
        SkillSupportingFile::from_bytes("requirements.txt".into(), b"idna==3.10\n".to_vec())
            .unwrap(),
    );
    let mut value = request(RuntimeAction::Prepare);
    value.hash = package.hash();
    value.timeout_ms = 180_000;
    let prepared = execute(
        root.clone(),
        workspace.path().into(),
        package.clone(),
        value.clone(),
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.status, "environment_prepared", "{prepared:?}");
    assert_eq!(prepared.requirements.as_deref(), Some("requirements.txt"));
    assert!(prepared.last_check.is_none());
    value.action = RuntimeAction::Run;
    value.entrypoint = Some("scripts/check.py".into());
    let checked = execute(
        root,
        workspace.path().into(),
        package,
        value,
        CancelSignal::new(),
        allow(),
    )
    .await
    .unwrap();
    assert_eq!(checked.status, "command_verified", "{checked:?}");
    assert!(checked
        .output
        .unwrap()
        .stdout
        .contains("dependency-preflight-passed"));
}
