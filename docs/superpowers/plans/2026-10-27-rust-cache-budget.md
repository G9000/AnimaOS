# Rust Cache Budget Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for the bounded cache-maintenance implementation and review. Preserve unrelated work in the shared checkout.

**Goal:** Reclaim the accumulated Rust build outputs and automatically keep ordinary repository build and validation commands within a 32 GiB cache budget.

**Architecture:** Keep the existing Cargo-owned cache root, use one stable validation target, and put cache lifecycle handling around complete commands through the task wrapper. Coordinate maintenance with PID leases so concurrent commands and running daemons are protected. Clean only verified repository Cargo output paths; never clean source, state, user-selected external directories, or tool download caches.

**Tech stack:** Bun/TypeScript task orchestration, Cargo clean, Cargo profiles, Vitest.

## Task 1: Clear existing caches safely

- [x] Inspect exact Cargo output roots and active executable/build paths.
- [x] Clean inactive Cargo target directories with Cargo's target-scoped cleaner.
- [x] Clear remaining inactive generated files while preserving any locked running executable; record exactly what remains and the actual free-space change.
- [x] Remove only this task's scratch helpers after recording results.

## Task 2: Add automatic bounded maintenance

**Files:** `tools/workspace-dev/src/rust-cache-maintenance.ts`, matching tests, and existing Rust cache status/cleanup helpers.

- [x] First write and run failing tests for the 32 GiB default, inactive-cache pruning, active PID leases, stale leases, concurrent acquisition, symlink/escape refusal, and failure when the budget cannot safely be met.
- [x] Provide a callback-based cache lifecycle API around an absolute repository target directory. Use injectable limits and cleaning for small deterministic tests.
- [x] Check the combined cache budget before and after managed commands. Prune inactive Cargo-owned output, protect live leases, and give actionable failure information instead of silently allowing unbounded growth.
- [x] Keep cleanup scope limited to generated Cargo caches and avoid deleting the output of a concurrently running command.

## Task 3: Integrate and reduce future duplication

**Files:** task wrapper and its CLI/tests; root/core/host/SDK package scripts; SDK integration target selection; Cargo.toml; AGENTS.md and relevant README guidance.

- [x] Add explicit development/validation cache modes to complete task invocations, with absolute `CARGO_TARGET_DIR` values inherited by child commands.
- [x] Use `target/cargo/validation` as the stable validation target, preserving an explicit caller-selected absolute target without automatically deleting external paths.
- [x] Route normal Rust build/test/clippy and daemon development through the cache lifecycle. Hold leases for complete commands, including daemon and SDK service-test lifetimes.
- [x] Disable incremental development compilation while keeping the existing reduced debug settings.
- [x] Update cache commands and agent instructions to describe automatic maintenance, the budget, and the cleanup escape hatch accurately.

## Task 4: Validate and review

- [x] Run focused regression tests red/green, then the full workspace-dev suite, build, typecheck, and lint.
- [x] Validate changed package scripts and Cargo configuration. Run relevant Rust/SDK gates with reduced profiles, recording cache size after the run.
- [x] Review scope and safety independently; fix important findings.
- [x] Report actual disk space recovered, the final cache size, automatic behavior, and any active-file exceptions.

## Validation record

- Initial cleanup increased actual drive free space by 228,873,719,808 bytes (213.16 GiB); subsequent validation rebuilds consume some of that recovered space. Village was stopped with explicit user approval.
- Final workspace-dev suite: 111 passed across 15 files. Build, typecheck, and lint pass. The restricted Windows test run failed on filesystem permissions; rerunning with normal filesystem access passed.
- Village configuration: 3 tests passed; typecheck and web/tool lint passed. SDK typecheck/build passed.
- The required full Rust gate was attempted. Two unchanged supervisor tests fail with profile ownership unavailable/uncertain; an isolated serial retry of stop_then_start reproduces the failure. The separate core/daemon run recorded 4,856 passes and 5 failures involving OwnershipLost or poisoned locks.
- Independent specification and code-quality reviews completed; their Cargo argument/profile and Village relative-target findings were fixed and covered by regression tests.

- Live SDK verification exercised the 32 GiB monitor: it stopped its owned process at 32.3 GiB. Cargo compatibility-report metadata initially blocked pruning; an exact filename allowlist fix and two red/green regressions resolve that gap. Unknown files inside a known target remain protected. The final 111-test suite, build, typecheck, lint, and follow-up safety review pass.
- During initial cleanup, concurrent work used a separate validation-rust-daemon target and an explicit 128 GiB override. That temporary exception was subsequently removed by the user-requested hard-ceiling follow-up below.

- SDK retry: 281 tests passed (including real daemon persistence/restart), 3 skipped. The other service setup failed at its 300-second Cargo build deadline; no full-suite pass is claimed. Removed this task's inactive validation output again after recording results. All task-owned scratch helpers and raw logs were removed; the compact validation JSON is retained.

## User-requested hard ceiling follow-up

- Removed the upward override: environment and programmatic values cannot raise the managed cache budget above 32 GiB. Child commands receive the effective capped value; lower limits remain supported.
- Regression tests first reproduced all three bypass cases, then passed after the fix. Final workspace-dev suite: 113 passed across 15 files; build, typecheck, and lint passed. Independent safety review approved the change.
- Windows User/System settings and repository .env contain no cache-budget override. Replaced the verified old Bun development tree with a normal bun dev --no-open launch. No active lease exceeds 32 GiB; Anima health is ok.
- Cache measured 17.6 GiB after startup. The inspection helper was removed and .tmp is empty. Scanning still permits brief overshoot between 15-second checks; raw Cargo or explicit external targets remain outside managed enforcement.
