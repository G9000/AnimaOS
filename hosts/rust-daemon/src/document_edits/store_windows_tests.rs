use super::*;
use std::os::windows::fs::OpenOptionsExt;
use win_custody::{
    current_user_sid_string, ObjectSecurityDescriptor, OwnedSid, SecurityDescriptor,
};

fn security(path: &Path) -> ObjectSecurityDescriptor {
    let file = std::fs::OpenOptions::new()
        .access_mode(0x00020000)
        .share_mode(0x1 | 0x2)
        .custom_flags(0x02000000 | 0x00200000)
        .open(path)
        .unwrap();
    ObjectSecurityDescriptor::query_file_handle(&file).unwrap()
}

fn create_parent(container: &Path, extra: &str) -> PathBuf {
    let path = container.join("parent");
    let user = current_user_sid_string().unwrap();
    SecurityDescriptor::from_sddl(&format!(
        "O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY){extra}"
    ))
    .unwrap()
    .create_directory(&path)
    .unwrap();
    path
}

#[test]
fn document_store_read_only_parent_keeps_its_acl_and_creates_a_private_child() {
    let container = test_support::private_directory();
    let parent = create_parent(container.path(), "(A;OICI;GRGX;;;WD)");
    let before = security(&parent);
    let before_acl = format!("{:?}", before.dacl().unwrap());
    let root = parent.join("document-edits");
    initialize_owned_directory(&root).unwrap();
    initialize_owned_directory(&root).unwrap();
    let descriptor = security(&root);
    let user = OwnedSid::from_string(&current_user_sid_string().unwrap()).unwrap();
    let system = OwnedSid::from_string("S-1-5-18").unwrap();
    assert!(descriptor.owner().unwrap().matches(&user));
    assert_ne!(descriptor.control().unwrap() & 0x1000, 0);
    assert!(descriptor.dacl().unwrap().allowed_aces().all(|ace| {
        ace.flags() & 0x10 == 0 && (ace.trustee_matches(&user) || ace.trustee_matches(&system))
    }));
    let after = security(&parent);
    assert_eq!(format!("{:?}", after.dacl().unwrap()), before_acl);
    assert_eq!(after.control().unwrap(), before.control().unwrap());
    assert!(after.owner().unwrap().matches(&user));
    let store = DocumentEditStore::open_existing(&root).unwrap();
    let staged = store.stage(b"synthetic private candidate").unwrap();
    let hash = staged.hash().to_string();
    drop(staged);
    assert_eq!(
        store
            .load(&hash, b"synthetic private candidate".len() as u64)
            .unwrap(),
        b"synthetic private candidate"
    );
    assert!(std::fs::rename(&root, parent.join("retargeted")).is_err());
}

#[test]
fn document_store_atomic_primitive_has_private_owner_and_acl_before_admission() {
    let container = test_support::private_directory();
    let parent = create_parent(container.path(), "(A;OICI;GRGX;;;WD)");
    let parent_custody = WorkspaceBytes::open(&parent).unwrap();
    assert!(std::fs::rename(&parent, container.path().join("retargeted-parent")).is_err());
    let path = parent.join("document-edits");
    let sid = current_user_sid_string().unwrap();
    let user = OwnedSid::from_string(&sid).unwrap();
    SecurityDescriptor::from_sddl(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"))
        .unwrap()
        .create_directory(&path)
        .unwrap();
    // This is the first observation after CreateDirectoryW. No host initializer
    // or ACL repair has run; the object must already meet the strict policy.
    verify_windows_descriptor(&security(&path), &user, false).unwrap();
    let before = format!("{:?}", security(&path).dacl().unwrap());
    assert_eq!(
        SecurityDescriptor::from_sddl("D:P(A;OICI;GA;;;WD)")
            .unwrap()
            .create_directory(&path)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(format!("{:?}", security(&path).dacl().unwrap()), before);
    drop(parent_custody);
}

#[test]
fn document_store_mutable_or_unknown_parent_grants_fail_before_child_creation() {
    let everyone = OwnedSid::from_string("S-1-1-0").unwrap();
    for (flags, mask, stored_mask) in [
        ("OICI", 0x2, 0x2),               // write/add file
        ("OICI", 0x40, 0x40),             // delete child
        ("OICI", 0x00010000, 0x00010000), // delete
        ("OICI", 0x00040000, 0x00040000), // change DACL
        ("OICI", 0x00080000, 0x00080000), // change owner
        ("OICI", 0x40000000, 0x00120116), // mapped generic write
        ("OICI", 0x10000000, 0x001f01ff), // mapped generic all
        // Windows strips unknown effective rights; inherit-only preserves them.
        ("OICIIO", 0x200, 0x200),
        ("OICIIO", 0x2, 0x2), // inherit-only write is still unsafe
        ("OICIIO", 0x00040000, 0x00040000),
    ] {
        let container = test_support::private_directory();
        let parent = create_parent(container.path(), &format!("(A;{flags};0x{mask:08x};;;WD)"));
        let before = security(&parent);
        let before_acl = format!("{:?}", before.dacl().unwrap());
        let stored_grants: Vec<_> = before
            .dacl()
            .unwrap()
            .allowed_aces()
            .filter(|ace| ace.trustee_matches(&everyone))
            .map(|ace| (ace.flags(), ace.mask()))
            .collect();
        let stored_flags = if flags == "OICIIO" { 0x0b } else { 0x03 };
        assert_eq!(
            stored_grants,
            vec![(stored_flags, stored_mask)],
            "native fixture grant for {flags} mask {mask:#x}"
        );
        let root = parent.join("document-edits");
        assert!(initialize_owned_directory(&root).is_err(), "mask {mask:#x}");
        assert!(
            !root.exists(),
            "unsafe parent mask {mask:#x} created a child"
        );
        let after = security(&parent);
        assert_eq!(format!("{:?}", after.dacl().unwrap()), before_acl);
    }
}

#[test]
fn document_store_parent_rights_mapping_rejects_unknown_or_mutating_generic_bits() {
    assert!(parent_read_only_access(0x80000000 | 0x20000000));
    assert!(parent_read_only_access(0x001200a9));
    for mask in [0x40000000, 0x10000000, 0x02000000, 0x01000000, 0x200, 0x40] {
        assert!(!parent_read_only_access(mask), "mask {mask:#x}");
    }
}

#[test]
fn document_store_checks_the_actual_owner_even_when_the_dacl_admits_the_expected_user() {
    let container = test_support::private_directory();
    let path = container.path().join("different-owner");
    let actual = current_user_sid_string().unwrap();
    let expected = "S-1-5-21-111111111-222222222-333333333-4444";
    SecurityDescriptor::from_sddl(&format!(
        "O:{actual}D:P(A;OICI;FA;;;{actual})(A;OICI;FA;;;{expected})(A;OICI;FA;;;SY)"
    ))
    .unwrap()
    .create_directory(&path)
    .unwrap();
    let descriptor = security(&path);
    let actual = OwnedSid::from_string(&actual).unwrap();
    let expected = OwnedSid::from_string(expected).unwrap();
    assert!(descriptor.owner().unwrap().matches(&actual));
    assert!(descriptor
        .dacl()
        .unwrap()
        .allowed_aces()
        .any(|ace| { ace.trustee_matches(&expected) && ace.mask() == 0x001f01ff }));
    assert_eq!(
        verify_windows_descriptor(&descriptor, &expected, true)
            .err()
            .as_deref(),
        Some("Document storage owner is untrusted")
    );
    assert_eq!(
        verify_windows_descriptor(&descriptor, &expected, false)
            .err()
            .as_deref(),
        Some("Document storage owner is untrusted")
    );
    std::fs::remove_dir(&path).unwrap();
}

#[test]
fn document_store_existing_insecure_child_is_never_repaired_or_repopulated() {
    let container = test_support::private_directory();
    let root = container.path().join("document-edits");
    let user = current_user_sid_string().unwrap();
    SecurityDescriptor::from_sddl(&format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;GR;;;WD)"))
        .unwrap()
        .create_directory(&root)
        .unwrap();
    std::fs::write(root.join("sentinel"), b"unchanged").unwrap();
    let before = security(&root);
    let before_acl = format!("{:?}", before.dacl().unwrap());
    assert!(initialize_owned_directory(&root).is_err());
    assert!(DocumentEditStore::open_existing(&root).is_err());
    assert_eq!(format!("{:?}", security(&root).dacl().unwrap()), before_acl);
    assert_eq!(std::fs::read(root.join("sentinel")).unwrap(), b"unchanged");
}

#[test]
fn document_store_parent_unsupported_ace_types_fail_before_creation() {
    let container = test_support::private_directory();
    let parent = create_parent(container.path(), "(D;;FW;;;AN)");
    assert!(security(&parent).dacl().is_err());
    let root = parent.join("document-edits");
    assert!(initialize_owned_directory(&root).is_err());
    assert!(!root.exists());
}
