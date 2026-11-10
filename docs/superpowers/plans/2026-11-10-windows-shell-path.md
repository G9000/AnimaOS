# Windows Shell Search Path Fix

**Goal:** Allow approved CMD commands to run when inherited PATH contains optional directories that cannot pass Anima's filesystem identity checks.

**Cause:** Windows Store application directories can be accessible by their full path while protected ancestors deny the capability traversal used for identity verification. Treating every inherited helper directory as mandatory aborts shell launch before CMD starts.

**Design:** Validate fixed shell search directories strictly. Convert optional helper directories independently and omit entries that are missing, are not directories, or fail normal-spelling/identity verification. Preserve ordering, deduplication, and strict executable, cwd, and private-home checks.

- [x] Add Windows regressions for optional invalid directories, preserved valid helpers, and mandatory-directory failure.
- [x] Apply the search-path policy only to Windows shell launches.
- [x] Run command-runtime/path regressions, the full Rust suite, formatting, and lint; inspect the final diff.

The running Anima daemon is left in place during validation.

**Validation:**

- Command-runtime suite: 16 passed, 0 failed, 1 existing physical-tool test ignored. This includes all eight previously failing Windows shell tests.
- Windows shell-path regressions: 2 passed, 0 failed.
- `bun run rust:test`: 5,822 passed, 0 failed, 63 ignored across 113 test groups. The daemon unit group passed 3,998 tests, with 48 ignored. Tests ran serially with a task-owned owner-private temporary root, avoiding the previously identified temp-directory and drain-test interference.
- `bun run rust:lint` and `git diff --check` passed. Existing workspace lint warnings remain outside this fix.
- Review found no outstanding issues.
