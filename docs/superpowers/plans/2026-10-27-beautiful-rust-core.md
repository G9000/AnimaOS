# BeautifulRust Core Refactor Implementation Plan

> **For agentic workers:** Use the installed BeautifulRust `rust-best-practices` and `rust-review` skills. Execute the tasks in order, retaining the existing public API and serialized representations. Request an independent review before delivery.

**Goal:** Refactor the host-independent `anima-core` library using BeautifulRust without changing runtime behavior or disturbing the existing document and OAuth changes.

**Architecture:** Keep runtime execution in `packages/core-rust/crates/anima-core`. Separate execution-store conformance checks from store contracts, remove unnecessary owned copies, reduce the private command enum's size, and make the entire crate pass Clippy with warnings denied. Existing public constructors, re-exports, error variants, serialized data, ordering, and lock-poisoning policy remain compatible.

**Tech Stack:** Rust 2021, the existing Cargo workspace, Serde, and the repository's Bun validation wrapper. No new dependencies or global lint-policy changes.

## Task 1: Establish baseline and pin publication behavior

- [x] Load the installed BeautifulRust skills and inspect repository instructions and current local changes.
- [x] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-core`; baseline passes.
- [x] Run strict Clippy for all `anima-core` targets and features; record the 51 existing diagnostics.
- [x] Add a public-contract test to `packages/core-rust/crates/anima-core/tests/definition_contract.rs` covering profile overrides, override-only capabilities, configuration ownership, deterministic ordering, and retained publication snapshots.
- [x] Run that contract test before implementation to confirm it describes current behavior. Also pin approval-resume command serialization and digest/accessor round trips before boxing its private binding.

## Task 2: Refactor ownership and private representations

**Files:** `anima-core/src/definition.rs`, `anima-core/src/execution/state.rs`, and existing core files identified by Clippy.

- [x] Consume capability overrides from the owned draft with `std::mem::take` rather than cloning their JSON configurations. Use borrowed capability IDs in validation sets when ownership is unnecessary.
- [x] Accept `&mut [HostRequirement]` in the private canonicalization helper.
- [x] Box the large private command payload field(s) identified by Clippy. Retain public signatures and Serde bytes; update construction/access internally.
- [x] Apply behavior-preserving Clippy fixes within `anima-core` only. Remove temporary slice clones and redundant default implementations, simplify collection operations, and scope test locks before async work.
- [x] Move `runtime_serde` unit tests to `src/runtime_serde/tests.rs`, leaving production conversion logic together.
- [x] Keep intentional poison recovery and public bool parameters intact because changing those contracts exceeds a behavior-preserving refactor.

## Task 3: Separate execution-store contracts from conformance checks

**Files:** `anima-core/src/execution/store.rs`, new `anima-core/src/execution/store/conformance.rs`.

- [x] Move `assert_execution_store_conformance`, its private checks/fixtures, and `execution_store_conformance_manifest_inventory` into the private `store::conformance` module.
- [x] Retain the two existing public paths using explicit re-exports from `store.rs`; do not expose implementation helpers.
- [x] Move conformance-only constants and imports with their users. Use explicit imports rather than production glob imports.
- [x] Verify the external-adapter contract test still compiles and runs, proving downstream use of the public port remains supported.

## Task 4: Verify and review

- [x] Format only `anima-core`, then run `cargo clippy -p anima-core --all-targets --all-features -- -D warnings` through the task wrapper.
- [x] Run the required Rust coverage, including the reusable crates, daemon, and packaged supervisor. `bun run rust:test` passed the core/supervisor suites before hitting the development daemon's executable lock; `bun run --filter rust-daemon test` then passed with the documented isolated validation cache.
- [x] Run `bun run rust:lint` (exit code 0).
- [x] Run `cargo doc -p anima-core --no-deps` through the task wrapper with `RUSTDOCFLAGS=-D warnings`.
- [x] Request an independent review of this task's diff and address material findings. Review approved; all extracted conformance bodies match their originals after module-path and formatting normalization.
- [x] Check `git diff --check`, public compatibility, and preservation of the initial unrelated edits.
- [x] Record verification results here.
- [x] Remove task-owned scratch files and logs after recording results; preserve configured reports and build caches. Removed 13 verification logs; task-wrapper temporary directories were already cleaned, and Cargo caches/configured reports were retained.

## Verification notes

The initial checkout has existing edits in Cargo manifests/lockfile, the daemon, SDK, web UI, documentation, and `anima-document-content`, plus an untracked `anima-mcp-oauth` crate. This task must not overwrite, stage, commit, or revert those changes. Validation runs against this current working tree. If a live daemon locks its executable, use the documented isolated validation target. Prefer retaining the normal cache unless a lock actually occurs.

- Final `anima-core` test run: **573 passed, 0 failed**, including unit, integration, external-adapter, and documentation checks.
- Strict `anima-core` Clippy (`--all-targets --all-features -- -D warnings`): passed.
- Core documentation with warnings denied: passed.
- All 14 reusable-crate suites: **1,348 passed, 0 failed, 6 ignored**.
- Supervisor suites: **175 passed, 0 failed, 1 ignored** with `RUST_TEST_THREADS=1`. The initial parallel run encountered a temporary-port collision; the serialized rerun passed the affected test and all supervisor suites.
- Workspace `bun run rust:lint`: **exit code 0**. Existing warnings remain in other crates and the daemon; strict `anima-core` linting is clean.
- The normal `bun run rust:test` reached the daemon build after the core and supervisor suites passed, then encountered the running development daemon's locked `target/cargo/debug/anima-daemon.exe`. Host validation passed in `target/cargo/validation-rust-daemon` using the documented isolated-cache approach; the development daemon was left running.
- Isolated `bun run --filter rust-daemon test`: **exit code 0**, covering **4,924 passed, 0 failed, 50 ignored** (1,348 reusable-crate tests and 3,576 daemon tests).
- Combined coverage without counting repeated crate runs: **5,099 passed, 0 failed, 51 ignored** across the reusable crates, daemon, and supervisor. Validation used `CI=1`, `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2`, and `RUST_TEST_THREADS=1`; host validation also used `CARGO_TARGET_DIR=target/cargo/validation-rust-daemon` and normal Windows process access for native build tools/local test servers.
- Independent plan and implementation reviews: approved, no material findings.
- Source checks: conformance function bodies are unchanged after module-path/formatting normalization; the existing public exports and command serialization remain intact.
- Concurrent work continues to update some existing MCP OAuth files. This refactor has written only `anima-core` sources/tests and this plan; it has not edited the existing changes.
