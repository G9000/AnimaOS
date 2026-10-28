# Local filesystem custody additions

This is the crates.io `win-custody` **0.1.1** source, without a package or
dependency version change. Upstream repository:
https://github.com/115dkk/Win-Custody.rs

- Upstream Git revision (published `.cargo_vcs_info.json`):
  `4a98fe6af9a798af92adb14b8ee53ced514fddee`
- Published upstream crate SHA-256:
  `e9f78c23db90914be58d60cb31a9f4c608cc344f55fc7aa8879937ba308eada3`
- Both upstream `LICENSE-MIT` and `LICENSE-APACHE`, copyright notices, and
  source documentation are retained unchanged.
- Cargo extraction markers and the dependency's standalone lockfile are omitted.

The only implementation changes are in `src/security.rs`:

1. `SecurityDescriptor::create_directory(&self, path: &Path) -> io::Result<()>`
   calls `CreateDirectoryW` with this descriptor in `SECURITY_ATTRIBUTES` during
   initial creation. It does not modify an existing directory, reset an ACL,
   provision parents, or choose a security policy. The caller retains parent
   custody and validates the returned filesystem object.
2. `ObjectSecurityDescriptor::query_file_handle(handle: &impl AsHandle)
   -> io::Result<Self>` calls `GetSecurityInfo` with `SE_FILE_OBJECT` and
   `OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION`, using the retained
   handle. Existing bounded descriptor/SID/ACL parsing and allocation ownership
   remain unchanged.

Focused dependency-local tests cover initial descriptor ownership/protection,
retained-handle queries, and refusal to modify existing paths. The host separately
tests parent custody and its strict private-child policy. Unsafe calls remain in
the upstream security module with explicit safety comments; the workspace's
unsafe-code prohibition is unchanged.
