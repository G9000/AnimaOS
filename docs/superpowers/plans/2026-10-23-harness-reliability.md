# Harness Reliability Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for bounded implementation and independent spec/quality review. User authorization overrides worktree/commit defaults: stay on main, no stage/commit/push. Root owns shared integration and serial Cargo/Nx validation.

**Goal:** Deliver profile-wide writer ownership, shared durable execution limits, consistent embeddable controls and a permanent reliability gate.

**Architecture:** Native portable core/control contracts composed by the Rust daemon; retain current admission, approval, receipts, persistence and Undo. Avoid a second scheduler or parallel permission system.

**Tech Stack:** Existing Rust standard library locks, Tokio, SQLx/PostgreSQL, core Budget/Usage contracts, Nx and existing trace acceptance. No unnecessary dependencies.

Spec: docs/superpowers/specs/2026-10-23-harness-reliability-design.md

## 1. One writer per profile first

Files: packages/core-rust/crates/anima-process/src/ (portable retained file ownership if reusable), hosts/rust-daemon/src/profile_owner.rs, app/persistence.rs, state.rs, control_plane_store.rs, memory_store.rs, memory_embeddings.rs, history/, and app/profile_owner_tests.rs as needed.

- [x] Capture baseline/references and independently review ownership design.
  Review correction incorporated: explicit embeddings SQLite override/default alias, retained blocking jobs, hard-link handling, PG no-reconnect/lost-session semantics.
- [x] Add a real child-process contention regression and observe RED: 7 meaningful failures, 2 passes; evidence ownership-red.log.
- [x] Implement canonical resource ownership before opening profile data; retain/fence detached writes, PostgreSQL ownership and clear errors.
- [x] Verify contention, path alias/overlap, separate profiles, failure/drop cleanup, process kill/takeover and stale writes; retain approvals/Undo and backups.
- [x] Independent spec and quality reviews; fix material issues and verify affected paths.
  Restart validation exposed calendar registration, idle swarm manager, participant callback and interrupted-dispatch reference cycles. Real-owner and factory-drop regressions reproduced each leak before weak registrations/callbacks fixed retired custody; active work still retains strong custody.

## 2. Shared execution budgets

Files: anima-core/src/execution_limits.rs (or focused runtime module), model.rs, runtime.rs, anima-model-adapters attempt seams, daemon agent_runs/budget.rs, runs/ledger.rs, state/snapshot, usage/metered.rs, secondary producers, runtime_model/fallback adapter, owner contracts if needed.

- [x] Map real model/retry/key/fallback/tool/secondary/helper dispatch and root recovery lineage; controlled adapter calls count physical sends, with durable root reservations.
- [x] Write failing tests for concurrent last-slot reservation, cancellation, failed save, exhausted restart and shared helper/secondary consumption.
- [x] Implement portable finite limits, idempotent reservation/settlement and request-local cancellation.
- [x] Wire persisted host scope before compaction/model/tool dispatch, native physical retry attempts and standalone secondary tasks; recovery cannot mint allowance.
- [x] Verify unknown/late usage, deadline, token reservation, saturation, no hidden retry, parent/helper limits and persistence rollback.
- [x] Independent spec and quality reviews.

## 3. Embeddable harness control parity

Files: anima-harness/src/config.rs, harness.rs, tool.rs, new execution options if needed, tests/harness.rs and README.md; core hooks reused.

- [x] Add failing conformance cases for Stop, failed checkpoint, denied approval, failed approval callback, observer, limits and explicit restore reattachment.
- [x] Expose focused controls and host tool execution/approval seam without serializing authority or adding host dependencies.
- [x] Pass wrapper/core conformance and preserve run/chat/snapshot behavior; independent spec/quality reviews.

## 4. Reliability gate

Files: focused core/host generated sequence tests, actual process acceptance tests, .github/workflows/harness-reliability.yml, core-rust Nx target if useful, acceptance/turn-quality/ and daemon trace live runner.

- [x] Add bounded seeded lifecycle/property sequences with an independent invariant model and deterministic reproduction.
- [x] Exercise production admission/persistence/tool paths, duplicate requests, approval/Stop interleavings, owner loss and budget restore.
- [x] Add required Windows/Linux verification jobs through Nx and a bounded opt-in repeated matched provider acceptance with sanitized traces.
- [x] Run deterministic and available live/isolated-service acceptance and record limits; independent quality review.
  Windows Rust/core/harness and real child-process acceptance passed; the trace driver passed all seven regressions and isolated PostgreSQL takeover/accounting passed. Linux CI is configured but has not run locally. No live provider comparison or provider-backed improvement is claimed.

## 5. Final integration

- [x] Update owner docs/contracts and changelog; run docs sync.
- [x] Pass full rust-daemon:test --skipNxCache: core/harness, 3,154 daemon unit tests and every enabled public integration, including agent/swarm/workspace restart recovery.
- [x] Pass rust-daemon:build, anima:test, isolated PostgreSQL takeover/accounting, SDK build/typecheck and 19 applicable SDK tests; seven trace-driver regressions pass.
- [x] Verify formatting of all 852 Rust files in the frozen source manifest and run git diff --check.
- [ ] Pass the repository-wide rust-daemon:lint after concurrent MCP work settles. The final normal format run failed only in the newly added tools/mcp_management_tests.rs. That file and its tools/mcp_tests.rs module registration were added by another chat after this full run compiled; they remain untouched and are not covered by the full run's result. The other 916 frozen source identities remained unchanged.
- [x] Save exact evidence/file manifest, confirm main and no harness changes staged; preserve any concurrent staging by other chats. No stage, commit or push.
  Evidence: .tmp/harness-hardening-2026-10-23/verified-validation.json, verified-full-rust.log, verified-frozen-format.log, verified-postgres.log and verified-source-final.json. The disposable PostgreSQL fixture was stopped and auto-removed after identity/isolation checks; postgres-cleanup.json records the exact container. Shared source hashes are validation evidence, not an approved commit scope.
