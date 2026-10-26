use anima_harness::durable::hosted::{HostWorkers, StoragePaths};
#[test]
fn hosted_startup_requires_distinct_absolute_paths_and_preprovisioned_material() {
    let dir = tempfile::tempdir().unwrap();
    let paths = StoragePaths {
        execution: dir.path().join("execution.sqlite"),
        memory: dir.path().join("memory.sqlite"),
        owner: dir.path().join("owner"),
        key: dir.path().join("key"),
    };
    assert!(paths.validate(&[]).is_err());
    std::fs::write(&paths.owner, uuid::Uuid::new_v4().to_string()).unwrap();
    std::fs::write(&paths.key, [17u8; 32]).unwrap();
    let validated = paths.validate(&[]).unwrap();
    assert_eq!(validated.owner, paths.validate(&[]).unwrap().owner);
    assert!(paths.validate(&[paths.memory.clone()]).is_err());
    let mut alias = paths.clone();
    alias.memory = paths.execution.clone();
    assert!(alias.validate(&[]).is_err());
    let mut relative = paths.clone();
    relative.execution = "relative.sqlite".into();
    assert!(relative.validate(&[]).is_err());
    std::fs::write(&paths.key, [17u8; 31]).unwrap();
    assert!(paths.validate(&[]).is_err());
    std::fs::remove_file(&paths.key).unwrap();
    std::fs::write(&paths.execution, []).unwrap();
    assert!(paths.validate(&[]).is_err());
}
#[test]
fn hosted_startup_rejects_parent_child_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let paths = StoragePaths {
        execution: dir.path().join("execution"),
        memory: dir.path().join("execution/child"),
        owner: dir.path().join("owner"),
        key: dir.path().join("key"),
    };
    std::fs::write(&paths.owner, uuid::Uuid::new_v4().to_string()).unwrap();
    std::fs::write(&paths.key, [17u8; 32]).unwrap();
    assert!(paths.validate(&[]).is_err());
}
#[test]
fn hosted_startup_worker_close_is_atomic_and_retains_started_custody() {
    let workers = HostWorkers::default();
    let first = workers.enter().unwrap();
    let second = workers.enter().unwrap();
    workers.close();
    assert!(workers.enter().is_none());
    assert!(!workers.is_empty());
    drop(first);
    assert!(!workers.is_empty());
    drop(second);
    assert!(workers.is_empty());
}

#[test]
fn hosted_startup_lease_excludes_second_owner_and_configured_hardlinks() {
    use anima_harness::durable::hosted::{validate_paths, StorageLease};
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, "fixture").unwrap();
    match std::fs::hard_link(&a, &b) {
        Ok(()) => assert!(validate_paths(&[a.clone(), b]).is_err()),
        Err(error) if cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 1314)) => {
            eprintln!("Hardlink creation is denied for this account; actual hardlink alias fixture was NOT exercised");
        }
        Err(error) => panic!("Unexpected hardlink fixture failure: {error}"),
    }
    let first = StorageLease::acquire(std::slice::from_ref(&a)).unwrap();
    assert!(StorageLease::acquire(std::slice::from_ref(&a)).is_err());
    drop(first);
    assert!(StorageLease::acquire(&[a]).is_ok());
}
#[cfg(unix)]
#[test]
fn hosted_startup_rejects_actual_symlink_alias() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, "fixture").unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    assert!(anima_harness::durable::hosted::validate_paths(&[a, b]).is_err());
}
#[cfg(windows)]
#[test]
fn hosted_startup_rejects_actual_symlink_alias_when_platform_permits() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, "fixture").unwrap();
    match std::os::windows::fs::symlink_file(&a, &b) {
        Ok(()) => assert!(anima_harness::durable::hosted::validate_paths(&[a, b]).is_err()),
        Err(e)
            if e.kind() == std::io::ErrorKind::PermissionDenied
                || matches!(e.raw_os_error(), Some(5 | 1314)) =>
        {
            eprintln!("Symlink creation unavailable for this account; actual reparse fixture was NOT exercised")
        }
        Err(e) => panic!("{e}"),
    }
}
