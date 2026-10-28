//! Security descriptors, SIDs and token privileges.
//!
//! A descriptor Windows hands back is a single `LocalAlloc` block whose inner
//! pointers (owner, DACL, ACEs, SIDs) all point inside it. The types in the
//! `acl` submodule turn that block into bounds-checked byte views before
//! anything is read, so a malformed or hostile descriptor is rejected by
//! offset arithmetic rather than dereferenced.
#![allow(unsafe_code)]

mod acl;

use std::ffi::c_void;
use std::io;
use std::marker::PhantomData;
use std::mem::size_of;
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{self, NonNull};

use windows_sys::Win32::Foundation::{
    LocalFree, SetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS, LUID,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, GetSecurityInfo, TreeSetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT, SE_KERNEL_OBJECT, TREE_SEC_INFO_RESET,
};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, CheckTokenMembership, GetSecurityDescriptorControl,
    GetSecurityDescriptorDacl, GetSecurityDescriptorLength, GetSecurityDescriptorOwner,
    GetTokenInformation, LookupPrivilegeValueW, TokenUser, DACL_SECURITY_INFORMATION,
    LUID_AND_ATTRIBUTES, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;
use windows_sys::Win32::System::Threading::OpenProcessToken;

pub use acl::{
    AclValidationError, AllowedAce, OwnedSid, SecurityAclError, ValidatedDacl, ValidatedSid,
};

use crate::bounded_read::{probe_word_aligned, CallOutcome};
use crate::handle::{current_process, owned_from_creation};
use crate::wide::{bounded_units, wide_null};

const MAX_TOKEN_INFORMATION_BYTES: usize = 64 * 1024;
const MAX_SID_STRING_UNITS: usize = 32_768;

/// A security descriptor built from SDDL, for creating objects
/// ([`crate::PipeServer`], [`crate::NamedMutex`], [`crate::Job`]) and for
/// resetting the ACLs of a directory tree.
///
/// ```no_run
/// # fn main() -> std::io::Result<()> {
/// use win_custody::SecurityDescriptor;
///
/// // LocalSystem and the built-in Administrators get full access; nobody
/// // else gets any, and the DACL does not inherit from its parent.
/// let descriptor = SecurityDescriptor::from_sddl("D:P(A;;GA;;;SY)(A;;GA;;;BA)")?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SecurityDescriptor(NonNull<c_void>);

// SAFETY: the descriptor is an immutable LocalAlloc block owned by this value;
// Windows only reads it, so it may be moved to and shared between threads.
unsafe impl Send for SecurityDescriptor {}
// SAFETY: see `Send`; no method mutates the block.
unsafe impl Sync for SecurityDescriptor {}

impl SecurityDescriptor {
    /// Parses an SDDL string. The error carries the Win32 code
    /// (`ERROR_INVALID_PARAMETER` for malformed SDDL).
    pub fn from_sddl(sddl: &str) -> io::Result<Self> {
        let sddl = wide_null(sddl)?;
        let mut descriptor = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; `descriptor` is a live out pointer
        // that receives a LocalAlloc block owned by the returned value.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if converted == 0 || descriptor.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(
            NonNull::new(descriptor).expect("checked non-null descriptor"),
        ))
    }

    /// A `SECURITY_ATTRIBUTES` block pointing at this descriptor, for object
    /// creation calls. It borrows the descriptor and cannot outlive it.
    pub(crate) fn attributes(&self, inherit_handle: bool) -> SecurityAttributes<'_> {
        SecurityAttributes {
            raw: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0.as_ptr(),
                bInheritHandle: i32::from(inherit_handle),
            },
            _descriptor: PhantomData,
        }
    }

    /// Creates one directory with this descriptor at the initial creation.
    ///
    /// Existing paths return `AlreadyExists` and are never modified. The caller
    /// must retain custody of the parent path and validate the resulting object.
    pub fn create_directory(&self, path: &Path) -> io::Result<()> {
        let wide = wide_null(path)?;
        let attributes = self.attributes(false);
        // SAFETY: `wide` is NUL-terminated and `attributes` borrows the live
        // immutable descriptor. Both remain valid for the creation call.
        if unsafe { CreateDirectoryW(wide.as_ptr(), attributes.as_ptr()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Applies this descriptor's DACL to `path` and its whole subtree,
    /// resetting every inherited entry. With `protect` the tree also stops
    /// inheriting from its parent. The error carries the Win32 status.
    pub fn apply_to_tree(&self, path: &Path, protect: bool) -> io::Result<()> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        // SAFETY: `self.0` is a live descriptor and every out pointer is a
        // local; the DACL pointer stays valid while `self` is borrowed.
        let read = unsafe {
            GetSecurityDescriptorDacl(self.0.as_ptr(), &mut present, &mut dacl, &mut defaulted)
        };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || dacl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the descriptor has no DACL to apply",
            ));
        }
        let information = if protect {
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            DACL_SECURITY_INFORMATION
        };
        let wide = wide_null(path)?;
        // SAFETY: `wide` is NUL-terminated; `dacl` points into the live
        // descriptor; owner, group, and SACL are null with matching flags
        // absent; no progress callback is installed.
        let status = unsafe {
            TreeSetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                information,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
                TREE_SEC_INFO_RESET,
                None,
                0,
                ptr::null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(())
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the block came from the converter and is freed exactly once.
        unsafe { LocalFree(self.0.as_ptr()) };
    }
}

/// A `SECURITY_ATTRIBUTES` block that borrows its descriptor.
pub(crate) struct SecurityAttributes<'a> {
    raw: SECURITY_ATTRIBUTES,
    _descriptor: PhantomData<&'a SecurityDescriptor>,
}

impl SecurityAttributes<'_> {
    pub(crate) fn as_ptr(&self) -> *const SECURITY_ATTRIBUTES {
        &self.raw
    }
}

/// The security descriptor of an existing object, with accessors that
/// validate its parts before exposing them. Use it to verify that a pipe,
/// file or directory carries the ACL you expect.
#[derive(Debug)]
pub struct ObjectSecurityDescriptor(NonNull<c_void>);

// SAFETY: the descriptor is a LocalAlloc block owned by this value and only
// read; moving or sharing it across threads is sound.
unsafe impl Send for ObjectSecurityDescriptor {}
// SAFETY: see `Send`.
unsafe impl Sync for ObjectSecurityDescriptor {}

impl ObjectSecurityDescriptor {
    /// The owner and DACL of a retained file or directory handle.
    /// The handle must grant `READ_CONTROL`; its name is never resolved again.
    pub fn query_file_handle(handle: &impl AsHandle) -> io::Result<Self> {
        let raw = handle.as_handle().as_raw_handle();
        let mut descriptor = ptr::null_mut();
        // SAFETY: the retained handle is live for this call; optional outputs
        // are null and the descriptor out pointer transfers its allocation below.
        let status = unsafe {
            GetSecurityInfo(
                raw,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        Self::from_query_result(status, descriptor)
    }

    /// The DACL of the file or directory at `path`. The error carries the
    /// Win32 status, so callers can recognize a vanished path.
    pub fn query_file(path: &Path) -> io::Result<Self> {
        let wide = wide_null(path)?;
        let mut descriptor = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated, every optional output is null, and
        // `descriptor` is a live out pointer whose ownership transfers below.
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        Self::from_query_result(status, descriptor)
    }

    /// The owner and DACL of the kernel object behind `handle`. The handle
    /// needs `READ_CONTROL`.
    pub fn query_kernel_object(handle: &impl AsHandle) -> io::Result<Self> {
        let raw = handle.as_handle().as_raw_handle();
        let mut descriptor = ptr::null_mut();
        // SAFETY: the handle is live for the call; all optional outputs are
        // null and `descriptor` is a live out pointer.
        let status = unsafe {
            GetSecurityInfo(
                raw,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        Self::from_query_result(status, descriptor)
    }

    fn from_query_result(status: u32, descriptor: PSECURITY_DESCRIPTOR) -> io::Result<Self> {
        if status != ERROR_SUCCESS || descriptor.is_null() {
            if !descriptor.is_null() {
                // SAFETY: a non-null descriptor from these APIs is a LocalAlloc
                // block, even on a partial failure; it is freed exactly once.
                unsafe { LocalFree(descriptor) };
            }
            let status = if status == ERROR_SUCCESS {
                io::Error::new(io::ErrorKind::InvalidData, "Windows returned no descriptor")
            } else {
                io::Error::from_raw_os_error(status as i32)
            };
            return Err(status);
        }
        Ok(Self(
            NonNull::new(descriptor).expect("checked non-null descriptor"),
        ))
    }

    /// The descriptor control bits (for example `SE_DACL_PROTECTED`).
    pub fn control(&self) -> io::Result<u16> {
        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: `self.0` is a live descriptor; both outputs are locals.
        let read =
            unsafe { GetSecurityDescriptorControl(self.0.as_ptr(), &mut control, &mut revision) };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(control)
    }

    /// The owner SID. Only present when the query asked for it
    /// ([`query_kernel_object`](Self::query_kernel_object) does).
    pub fn owner(&self) -> Result<ValidatedSid, SecurityAclError> {
        let mut owner = ptr::null_mut();
        let mut defaulted = 0;
        // SAFETY: `self.0` is a live descriptor; both outputs are locals. The
        // returned owner pointer is range-checked before it is read.
        let read =
            unsafe { GetSecurityDescriptorOwner(self.0.as_ptr(), &mut owner, &mut defaulted) };
        if read == 0 {
            return Err(io::Error::last_os_error().into());
        }
        if owner.is_null() {
            return Err(AclValidationError::MissingOwner.into());
        }
        ValidatedSid::from_bounded_bytes(self.bytes_from_pointer(owner.cast())?).map_err(Into::into)
    }

    /// The DACL, provided every entry in it is an access-allowed ACE.
    pub fn dacl(&self) -> Result<ValidatedDacl, SecurityAclError> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        // SAFETY: `self.0` is a live descriptor; all outputs are locals. The
        // returned DACL pointer is range-checked before it is read.
        let read = unsafe {
            GetSecurityDescriptorDacl(self.0.as_ptr(), &mut present, &mut dacl, &mut defaulted)
        };
        if read == 0 {
            return Err(io::Error::last_os_error().into());
        }
        if present == 0 || dacl.is_null() {
            return Err(AclValidationError::MissingDacl.into());
        }
        ValidatedDacl::from_bytes(self.bytes_from_pointer(dacl.cast())?).map_err(Into::into)
    }

    /// The bytes of the descriptor block from `pointer` to its end, provided
    /// `pointer` lies inside the block.
    fn bytes_from_pointer(&self, pointer: *const u8) -> Result<&[u8], AclValidationError> {
        // SAFETY: `self.0` is a live descriptor owned for this call.
        let length = unsafe { GetSecurityDescriptorLength(self.0.as_ptr()) } as usize;
        if length == 0 {
            return Err(AclValidationError::DescriptorRange);
        }
        let offset = (pointer as usize)
            .checked_sub(self.0.as_ptr() as usize)
            .filter(|offset| *offset < length)
            .ok_or(AclValidationError::DescriptorRange)?;
        // SAFETY: Windows reports `length` for this live LocalAlloc block, and
        // the offset was proven to fall inside it.
        let bytes = unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), length) };
        Ok(&bytes[offset..])
    }
}

impl Drop for ObjectSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: this value uniquely owns the LocalAlloc block.
        unsafe { LocalFree(self.0.as_ptr()) };
    }
}

/// The calling process's user SID in string form, such as `S-1-5-18`.
pub fn current_user_sid_string() -> io::Result<String> {
    let token = open_current_process_token(TOKEN_QUERY)?;
    let raw = token.as_raw_handle();
    let information = probe_word_aligned(
        size_of::<TOKEN_USER>(),
        MAX_TOKEN_INFORMATION_BYTES,
        |buffer, capacity, needed| {
            // SAFETY: the token is live; the protocol passes either the
            // documented null size probe or word-aligned writable storage
            // described by `capacity`, and `needed` is a live out value.
            let result =
                unsafe { GetTokenInformation(raw, TokenUser, buffer.cast(), capacity, needed) };
            if result != 0 {
                return Ok(CallOutcome::Complete);
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                Ok(CallOutcome::MoreData(error))
            } else {
                Err(error)
            }
        },
    )
    .map_err(|error| error.into_io("token user information"))?;
    // SAFETY: GetTokenInformation succeeded and the protocol verified that its
    // reported byte range holds a complete, word-aligned TOKEN_USER header.
    let user = unsafe { information.read::<TOKEN_USER>() }
        .expect("token information header size and alignment were checked");
    let sid_offset = (user.User.Sid as usize).checked_sub(information.as_ptr() as usize);
    if sid_offset.map_or(true, |offset| offset >= information.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the token user SID lies outside the returned buffer",
        ));
    }
    let mut string = ptr::null_mut();
    // SAFETY: the SID pointer lies inside the live token-information buffer
    // and `string` is a local output receiving a LocalAlloc allocation.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut string) } == 0 || string.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ConvertSidToStringSidW returned a readable NUL-terminated string;
    // it is read within a fixed bound before conversion.
    let units = unsafe { bounded_units(string, MAX_SID_STRING_UNITS) };
    let result = units
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "SID string exceeds the bound"))
        .and_then(|units| {
            String::from_utf16(units).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "SID string is not valid UTF-16")
            })
        });
    // SAFETY: the string was allocated by ConvertSidToStringSidW and is freed
    // exactly once after the bounded copy.
    unsafe { LocalFree(string.cast()) };
    result
}

/// Whether the calling thread's token (or the process token, when the thread
/// is not impersonating) has `group` as an enabled group.
pub fn current_token_is_member_of(group: &OwnedSid) -> io::Result<bool> {
    let mut is_member = 0;
    // SAFETY: a null token means the current thread or process token; the
    // SID is a live LocalAlloc block owned by `group`; the output is a local.
    let checked = unsafe { CheckTokenMembership(ptr::null_mut(), group.as_ptr(), &mut is_member) };
    if checked == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(is_member != 0)
}

/// A privilege enabled in the process token for this value's lifetime, and
/// disabled again when it is dropped.
pub struct PrivilegeGuard {
    token: OwnedHandle,
    luid: LUID,
}

impl std::fmt::Debug for PrivilegeGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivilegeGuard")
            .field("token", &self.token)
            .field("luid_high", &self.luid.HighPart)
            .field("luid_low", &self.luid.LowPart)
            .finish()
    }
}

impl PrivilegeGuard {
    /// Enables the privilege `name`, such as `SeBackupPrivilege`. A token
    /// that does not hold the privilege fails with
    /// [`io::ErrorKind::PermissionDenied`]; enabling only works for
    /// privileges the token already has.
    pub fn enable(name: &str) -> io::Result<PrivilegeGuard> {
        let token = open_current_process_token(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY)?;
        let name_wide = wide_null(name)?;
        let mut luid = LUID::default();
        // SAFETY: `name_wide` is NUL-terminated and `luid` is a local output.
        if unsafe { LookupPrivilegeValueW(ptr::null(), name_wide.as_ptr(), &mut luid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let privileges = token_privileges(luid, SE_PRIVILEGE_ENABLED);
        // SAFETY: clearing last-error is required because a successful
        // AdjustTokenPrivileges reports partial assignment through last-error.
        unsafe { SetLastError(ERROR_SUCCESS) };
        // SAFETY: the token has adjust rights and `privileges` is a complete,
        // live one-entry TOKEN_PRIVILEGES value; previous state is not requested.
        let adjusted = unsafe {
            AdjustTokenPrivileges(
                token.as_raw_handle(),
                0,
                &privileges,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if adjusted == 0 {
            return Err(io::Error::last_os_error());
        }
        let assignment = io::Error::last_os_error();
        if assignment.raw_os_error() == Some(ERROR_NOT_ALL_ASSIGNED as i32) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("the current token does not hold {name}"),
            ));
        }
        Ok(Self { token, luid })
    }
}

impl Drop for PrivilegeGuard {
    fn drop(&mut self) {
        let privileges = token_privileges(self.luid, 0);
        // SAFETY: the token remains live through drop and `privileges` is a
        // complete one-entry value; disabling is best effort by contract.
        unsafe {
            AdjustTokenPrivileges(
                self.token.as_raw_handle(),
                0,
                &privileges,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
    }
}

fn token_privileges(luid: LUID, attributes: u32) -> TOKEN_PRIVILEGES {
    TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: attributes,
        }],
    }
}

fn open_current_process_token(access: u32) -> io::Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    // SAFETY: the process pseudo-handle is always valid and `token` is a local
    // out pointer whose handle is owned immediately below.
    if unsafe { OpenProcessToken(current_process(), access, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    owned_from_creation(token)
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;

    use super::{
        current_token_is_member_of, current_user_sid_string, ObjectSecurityDescriptor, OwnedSid,
        PrivilegeGuard, SecurityDescriptor,
    };

    #[test]
    fn a_tree_reset_from_sddl_reads_back_as_the_same_allowed_aces() {
        let directory = tempfile::tempdir().unwrap();
        let child = directory.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let current_user = current_user_sid_string().unwrap();
        let sddl = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{current_user})");
        let descriptor = SecurityDescriptor::from_sddl(&sddl).unwrap();
        descriptor.apply_to_tree(directory.path(), true).unwrap();

        let system = OwnedSid::from_string("S-1-5-18").unwrap();
        let queried = ObjectSecurityDescriptor::query_file(&child).unwrap();
        let dacl = queried.dacl().unwrap();
        assert_eq!(dacl.len(), 3);
        assert!(dacl.allowed_aces().any(|ace| ace.trustee_matches(&system)));
        assert!(ObjectSecurityDescriptor::query_file(&directory.path().join("missing")).is_err());
    }

    #[test]
    fn malformed_sddl_reports_invalid_parameter() {
        let error = SecurityDescriptor::from_sddl("D:(not sddl").unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_PARAMETER as i32));
    }

    #[test]
    fn directory_creation_is_private_initially_and_existing_paths_are_unchanged() {
        use std::os::windows::fs::OpenOptionsExt;
        let container = tempfile::tempdir().unwrap();
        let path = container.path().join("atomic-private-directory");
        let sid = current_user_sid_string().unwrap();
        let user = OwnedSid::from_string(&sid).unwrap();
        let system = OwnedSid::from_string("S-1-5-18").unwrap();
        SecurityDescriptor::from_sddl(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"))
            .unwrap()
            .create_directory(&path)
            .unwrap();
        // Observe immediately after creation: no ACL application or tree reset
        // occurs between the creation primitive and this retained-handle query.
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x02000000 | 0x00200000)
            .open(&path)
            .unwrap();
        let before = ObjectSecurityDescriptor::query_file_handle(&handle).unwrap();
        assert!(before.owner().unwrap().matches(&user));
        assert_ne!(before.control().unwrap() & 0x1000, 0);
        let dacl = before.dacl().unwrap();
        assert_eq!(dacl.len(), 2);
        assert!(dacl.allowed_aces().all(|ace| {
            ace.flags() & 0x10 == 0
                && (ace.trustee_matches(&user) || ace.trustee_matches(&system))
                && ace.mask() == 0x001f01ff
        }));
        let error = SecurityDescriptor::from_sddl("D:P(A;OICI;FA;;;WD)")
            .unwrap()
            .create_directory(&path)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        let after = ObjectSecurityDescriptor::query_file_handle(&handle).unwrap();
        assert!(after.owner().unwrap().matches(&user));
        assert_eq!(after.control().unwrap(), before.control().unwrap());
        assert_eq!(after.dacl().unwrap().len(), 2);
        assert!(after
            .dacl()
            .unwrap()
            .allowed_aces()
            .all(|ace| { ace.trustee_matches(&user) || ace.trustee_matches(&system) }));
        assert!(std::fs::rename(&path, container.path().join("retargeted")).is_err());
    }

    #[test]
    fn the_current_user_sid_has_a_decimal_sid_shape() {
        let sid = current_user_sid_string().unwrap();
        assert!(sid.starts_with("S-1-"), "unexpected SID {sid}");
        assert!(sid[4..].split('-').all(|component| !component.is_empty()
            && component.bytes().all(|byte| byte.is_ascii_digit())));
    }

    #[test]
    fn every_token_is_a_member_of_everyone() {
        let everyone = OwnedSid::from_string("WD").unwrap();
        assert!(current_token_is_member_of(&everyone).unwrap());
    }

    #[test]
    fn held_privileges_enable_and_unknown_names_fail() {
        drop(PrivilegeGuard::enable("SeChangeNotifyPrivilege").unwrap());
        assert!(PrivilegeGuard::enable("SeNotARealPrivilege").is_err());
    }
}
