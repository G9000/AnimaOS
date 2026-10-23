# Test temporary-file cleanup implementation plan

**Goal:** Commit the remaining completed work and remove disposable test scratch files after verification.

**Design:** A local Nx lifecycle plugin gives each invocation a separate directory under an OS-temp `anima-nx-*` directory scoped to this workspace. It sets `TMP`, `TEMP`, and `TMPDIR` for child tasks, then removes only that invocation's directory after all tasks settle, including failures. Invocation IDs identify cleanup ownership; filesystem checks reject redirected roots. Keeping fixtures outside the checkout also prevents Git discovery from inheriting the repository. Test reports and Cargo/Nx caches retain their existing locations. Forced termination can leave a directory for manual inspection.

- [x] Add filesystem regression tests for isolated runs, failure cleanup, parallel ownership, and symlink containment; observe the missing behavior.
- [x] Implement the plugin and register it in `nx.json`; document agent scratch cleanup and ignore `.tmp`.
- [x] Run workspace-dev tests, affected web tests/typecheck, SDK verification, and required Rust validation in the current tree.
- [x] Remove inspected disposable scratch files, verify the complete diff, and create signed commits for all remaining source changes.

The user authorized implementation and committing all current work. The earlier helper/delegation work is already committed separately.

Validation on the combined tree: `rust-daemon:test --skipNxCache` passed its core dependency, 3,247 daemon tests (37 explicitly ignored), and all integration groups. Web tests passed 2,275 tests with one skip; workspace-dev passed 33 tests including eight cleanup regressions. SDK, web, and workspace-dev typechecks passed; Rust formatting passed. The SDK build also ran as the web test dependency. Independent review found a shared-parent creation race; its regression failed before the fix and the full tooling suite passed afterward. An existing removed Vitest 5 `describe.sequential` use was updated to `{ concurrent: false }` so the Windows process test could run.

Removed the inspected repository `.tmp` and `tmp` folders and the completed chat-suite validation snapshot: about 126 MB of disposable files. Confirmed the managed Nx temp root had no remaining invocations. Source files, test reports, and Cargo/Nx caches were retained.
