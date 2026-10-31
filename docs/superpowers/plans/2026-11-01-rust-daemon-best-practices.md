# Rust daemon best-practices refactor

> **For agentic workers:** Use the executing-plans workflow for implementation and request an independent code review before completion.

**Goal:** Clarify run-admission ownership and remove unnecessary configuration and peer-selection copies without changing daemon behavior.

**Architecture:** A private `RunAdmission` module owns the room-lock, agent-slot, and waiting-budget registries. The coordinator retains orchestration, durable transactions, queue ordering, global permits, and its existing caller-facing methods. Leases continue releasing resources through `Drop`, including cancelled acquisition futures.

**Tech stack:** Rust 2021, existing Tokio synchronization types, repository-managed Bun/Cargo verification.

## Constraints

- Preserve room-before-slot-before-global-permit ordering, helper concurrency limits, refusal messages, and shared registries across coordinator clones.
- Preserve the existing poison-recovery policy in cleanup code: changing panic/recovery behavior is outside this behavior-preserving refactor.
- Preserve the registry-reference drop inside its critical section; existing stress coverage protects against leaked entries from concurrent lease drops.
- Preserve unrelated web and launcher edits. No deployment or running-service restart is needed.
- Use existing wrapped Rust commands and the managed 32 GiB cache ceiling.

## Tasks

- [x] Run the full Rust baseline and inspect current admission and helper/peer tests.
- [x] Add meaningful coverage for cancelled room/slot acquisition and peer selection before refactoring; run the focused cases against the original implementation.
- [x] Create `hosts/rust-daemon/src/agent_runs/admission.rs` with private registry fields, RAII leases, admission modes, and waiting-budget accounting. Keep coordinator forwarding methods where they preserve call-site clarity.
- [x] Move registry-internal stress tests into the admission module, preserving their contention and round counts. Keep coordinator integration coverage and update its inspection helpers.
- [x] Simplify peer selection using iterator pattern matching instead of a second collection and unwrap; borrow helper settings and delegation policy rather than cloning entire option structs.
- [x] Run focused admission/helper/peer tests, full `bun run rust:test`, formatting, daemon Clippy with warnings denied, and daemon rustdoc.
- [x] Obtain an independent diff review, address material findings, record verification evidence and limitations, and inspect the final diff.

## Files

- Modify `hosts/rust-daemon/src/agent_runs.rs`: delegate low-level registry ownership, simplify peer lookup and borrowed configuration.
- Create `hosts/rust-daemon/src/agent_runs/admission.rs`: resource bookkeeping and local tests.
- Create `hosts/rust-daemon/src/agent_runs/admission/tests.rs`: existing registry race and waiting-budget cleanup coverage.
- Modify `hosts/rust-daemon/src/agent_runs/tests.rs`: characterization tests and moved internal stress coverage.
- Modify `hosts/rust-daemon/src/agent_runs/queue.rs` only as needed for queue-held room acquisition.
- Modify `hosts/rust-daemon/src/agent_runs/async_helpers.rs`: route both helper slot acquisitions through the admission registry.

## Verification commands

- Baseline/final: `bun run rust:test`.
- Focused: `bun tools/workspace-dev/scripts/run-rust-daemon-tests.ts --lib agent_runs::`.
- Formatting: `cargo fmt --all --check` (format only touched Rust files if needed).
- Lints: `bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo clippy -p anima-daemon --all-targets --all-features -- -D warnings`.
- Documentation: `bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo doc -p anima-daemon --no-deps`.

Record command output under `test-output/`. Treat unrelated baseline failures as limitations, not passes; do not expand into unrelated repairs.

## Validation

- Full required `bun run rust:test`: **5,700 passed, 0 failed, 57 ignored**, across 111 test result summaries. The supervisor, core, daemon unit/integration, and doc-test suites completed with exit code 0 in the final tree.
- Full-suite setup: normal Windows permissions, `RUST_TEST_THREADS=1`, and `ANIMAOS_TASK_TEMP_ROOT` pointing to a short owner-private temporary folder. The inherited temporary root failed the Office custody privacy check; all four directory tests passed with the private root. Parallel supervisor fixtures also hit temporary port collisions; serialized test execution resolved them without source changes. Internal concurrency tests retained their own parallel execution.
- Focused `agent_runs::` run with the normal parallel harness: **544 passed, 0 failed, 8 ignored**. This includes cancelled acquisition, concurrent lease-drop stress, queue integration, helper workflows, peer selection, and configuration limits. The three new characterization cases also passed against the original implementation.
- `cargo fmt --all --check`: passed.
- Required `bun run rust:lint`: passed using the existing absolute validation target. Existing workspace warnings remain; this command denies Clippy correctness lints rather than every warning.
- Daemon `cargo doc --no-deps`: passed, with existing public-to-private documentation-link warnings in `app.rs` and `logs.rs`.
- The initial strict daemon Clippy run stopped on `clippy::single_match` in `packages/core-rust/crates/anima-worktree/src/git.rs:36`. At the owner's follow-up request, the outer one-pattern `match` was replaced with `if let`, preserving the inner Windows-prefix handling. The affected crate now passes `cargo clippy -p anima-worktree --all-targets --all-features -- -D warnings`; its 17 tests, including Windows drive/UNC/non-Unicode path cases, also pass. Whole-daemon warnings-as-errors cleanliness is not claimed.
- Independent plan and source reviews checked shared registry ownership, cancellation cleanup, helper limits, and admission order. Compilation identified additional queue/helper callers, which were corrected and reviewed. No remaining material review findings.
- `git diff --check`: passed. Unrelated local changes were preserved.
- Task wrappers cleaned their owned scratch. The additional private temporary root was ownership-checked, checked for active users, and removed after validation. Reports remain under `test-output/rust-daemon-refactor-*.log`.

## Integration follow-up

The owner requested committing this refactor and lint fix to `main` and publishing to `origin/main`. The checkout is already on `main`; existing unpublished local history is preserved. Stage only the daemon refactor, worktree lint fix, and this plan. Refresh the remote and verify the full Rust suite and repository lint after the lint fix before committing and pushing normally.

Post-fix verification completed before integration: the full `bun run rust:test` rerun passed **5,700 tests with 0 failures and 57 ignored**. Formatting, required repository lint, strict worktree Clippy, all 17 worktree tests, and the worktree documentation build passed. The documentation build retains an existing public-to-private `git` module link warning. The final review found no issues. The additional private test temporary root was verified and removed; reports are under `test-output/rust-daemon-clippy-*.log`.
