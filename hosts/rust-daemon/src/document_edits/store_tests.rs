use super::*;
use std::sync::{Arc, Barrier};

fn private_root(directory: &test_support::PrivateTestDirectory) -> std::path::PathBuf {
    let root = directory.path().join("document-edits");
    initialize_owned_directory(&root).unwrap();
    root
}

#[test]
fn document_edit_store_private_initialization_does_not_reset_an_insecure_existing_root() {
    let directory = test_support::private_directory();
    let root = private_root(&directory);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(initialize_owned_directory(&root).is_err());
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    #[cfg(windows)]
    {
        use win_custody::{current_user_sid_string, SecurityDescriptor};
        let user = current_user_sid_string().unwrap();
        SecurityDescriptor::from_sddl(&format!("D:P(A;OICI;FA;;;{user})(A;OICI;FR;;;WD)"))
            .unwrap()
            .apply_to_tree(&root, true)
            .unwrap();
        assert!(initialize_owned_directory(&root).is_err());
        assert!(DocumentEditStore::open_existing(&root).is_err());
    }
}

#[test]
fn document_edit_store_detects_tampered_candidate_bytes() {
    let directory = test_support::private_directory();
    let root = private_root(&directory);
    let store = DocumentEditStore::open_existing(&root).unwrap();
    let staged = store.stage(b"candidate").unwrap();
    let hash = staged.hash().to_string();
    drop(staged);
    assert_eq!(store.load(&hash, 9).unwrap(), b"candidate");
    std::fs::write(root.join(format!("{hash}.blob")), b"altered!!").unwrap();
    assert!(store.load(&hash, 9).is_err());
    assert!(store.load("../../outside", 9).is_err());
}

#[test]
fn document_edit_store_enforces_quota_even_under_concurrent_staging() {
    let directory = test_support::private_directory();
    let store = DocumentEditStore::open_with_limit(&private_root(&directory), 4).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut tasks = Vec::new();
    for bytes in [b"one!".to_vec(), b"two!".to_vec()] {
        let store = store.clone();
        let barrier = barrier.clone();
        tasks.push(std::thread::spawn(move || {
            barrier.wait();
            store.stage(&bytes)
        }));
    }
    barrier.wait();
    assert_eq!(
        tasks
            .into_iter()
            .map(|task| task.join().unwrap().is_ok())
            .filter(|ok| *ok)
            .count(),
        1
    );
}

#[test]
fn document_edit_store_only_retires_a_hash_after_reference_check() {
    let directory = test_support::private_directory();
    let store = DocumentEditStore::open_existing(&private_root(&directory)).unwrap();
    let staged = store.stage(b"candidate").unwrap();
    let hash = staged.hash().to_string();
    assert!(store.retire_if_unreferenced(&hash, || Ok(())).is_err());
    drop(staged);
    assert!(store
        .retire_if_unreferenced(&hash, || Err::<(), _>("referenced".to_string()))
        .is_err());
    assert!(store.load(&hash, 9).is_ok());
    store.retire_if_unreferenced(&hash, || Ok(())).unwrap();
    assert!(store.load(&hash, 9).is_err());
}

#[test]
fn document_edit_store_separate_handles_share_the_same_quota_lock() {
    let directory = test_support::private_directory();
    let root = private_root(&directory);
    let first = DocumentEditStore::open_with_limit(&root, 4).unwrap();
    let second = DocumentEditStore::open_with_limit(&root, 4).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let tasks = [(first, b"one!".to_vec()), (second, b"two!".to_vec())]
        .into_iter()
        .map(|(store, bytes)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.stage(&bytes)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    assert_eq!(
        tasks
            .into_iter()
            .map(|task| task.join().unwrap().is_ok())
            .filter(|ok| *ok)
            .count(),
        1
    );
}
