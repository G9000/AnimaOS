# Unified Turn Recovery Implementation Plan

> Use superpowers:subagent-driven-development with bounded ownership and independent spec/quality review. User authorization overrides worktree and commit defaults: stay on main, no stage/commit/push. Root owns shared state/snapshot integration and serializes Nx/Cargo validation.

**Goal:** Unify ordinary turn recovery, add actual crash/restart acceptance and trace evaluations, and close secondary usage crash loss.

**Architecture:** Shared core recovery/trace contracts plus existing host run coordinator, action receipts, transaction/snapshot and history outbox; thin clients.

**Tech Stack:** Existing Rust/Tokio/Serde/Nx, isolated test processes and fixture transport; no unnecessary new dependencies.

Spec: docs/superpowers/specs/2026-10-23-unified-turn-recovery-design.md

## 1. Unified turn recovery first

- [x] Map admission, boundaries, receipts, restore, continuation and final commit; capture failing cases.
- [x] Add portable bounded turn checkpoint/recovery classification types and tests in anima-core.
- [x] Wire checkpoints and root identity through ordinary runtime/coordinator, persistence, owner recovery DTO and current-authority continuation.
- [x] Restore/claim only never-started eligible owner inputs; serialize once-only recovery, preserve stop/unknown/review.
- [x] Verify save failure, stale binding, duplicate startup, no completed replay, continuation lineage and approval/Undo invariants.

## 2. Durable secondary usage

- [x] Reproduce crash loss before implementing persisted pending usage custody.
- [x] Integrate bounded outbox/snapshot/default/validation/upgrade and idempotent history drain; retain rows on failure.
- [x] Verify restart, duplicate ack, late enqueue, deletion, saturation and save failure.

## 3. Crash and restart acceptance

- [x] Add owned child-process/isolated-profile harness with exact bounded barriers and independent oracles.
- [x] Kill/restart at accepted input, pending approval and effect/commit boundaries; verify stable recovery claims, no duplicate unsafe effects and durable usage.
- [x] Cover compaction/history/Undo or use focused Rust integration where the actual process barrier is not practical; record exact coverage and limitations.

## 4. Trace evaluations

- [x] Define independent fixtures, matched comparison configuration and strict trace/outcome grading in Rust.
- [x] Add adversarial task battery and verified correctness/call/error/repetition/byte/usage/latency metrics.
- [x] Run deterministic oracle/trace tests and bounded provider-backed acceptance with explicit evidence limits.

## 5. Review, docs and verification

- [x] Independent spec review and quality review; fix material findings and rerun affected regressions.
- [x] Update owner docs, SDK contract if changed and snapshot/API inventory tests.
- [x] Pass required full Rust/core tests, Rust format/build and applicable packaged/client checks.
- [x] Record isolated crash/live proofs and final exact file manifest; confirm main and empty index without committing.

Verified 2026-10-23: full Rust/core 4,273 passed, packaged supervisor 175 passed, SDK 253 passed; format/build/web types/docs checks passed. Four owned process crash cases and the bounded live Anthropic fresh-skill file/output oracle passed. Final evidence and exact file hashes: .tmp/unified-turn-recovery-2026-10-23/verification-summary.md and final-scope.json. Initial failure evidence is retained. Main and empty index verified; no commit.
