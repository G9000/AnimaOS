# Rust Learning and Context Quality Implementation Plan

> Use test-first implementation and bounded subagent spec/quality reviews. User authorization overrides worktree/commit recommendations: stay on main; do not stage, commit or push. Root owns shared integration and serializes Cargo jobs.

**Goal:** Add independently evaluated skill promotion, durable compaction checkpoints, prompt-only tool-result projection and change-bound completion evidence.

**Architecture:** Reuse SkillService approval/Undo transactions, session compaction persistence, core model-request projection and host completion observations. Put reusable grading/projection/evidence primitives in anima-core, host authority and persistence in rust-daemon. Avoid new runtime dependencies and keep clients thin.

**Tech Stack:** Existing Rust, Tokio, Serde, Axum, model adapter and Nx workspace.

Spec: docs/superpowers/specs/2026-10-23-rust-learning-context-quality-design.md

## 1. Skill evaluation (first)

- [x] Review exact promotion/Free/Undo and persistence seams; settle bounded suite/report/service interfaces.
- [x] Add failing core suite/oracle/regression/holdout/digest tests; run through core-rust:test.
- [x] Implement reusable evaluation contracts and deterministic grading without model judging.
- [x] Add failing host fresh trial captures, policy/report persistence, stale result and approval-gate tests: enforced suite without evidence blocks owner/Free/File drafts; owner body/file changes and later model/suite/source changes invalidate evidence; passing evaluation itself grants nothing; legacy absence and exact Undo still work.
- [x] Implement bounded owner-configured evaluation service and exact effective-package gate in common approval transaction; preserve Free/source/Undo checks.
- [x] Add authenticated owner no-store configuration/run/report controls and thin client support where needed.
- [x] Verify real baseline/candidate/fresh-case behavior, exact approval/Undo and restart; report provider limits.
- [x] Obtain separate spec and code-quality review and fix findings.

Expected files: anima-core/src/skill_evaluation.rs and lib.rs; rust-daemon/src/skills/evaluation*.rs, drafts.rs, ledger.rs, registry.rs, service.rs; routes/skill_evaluations.rs; shared state/control-plane/usage/drain integration as required.

## 2. Pre-compaction checkpoint

- [x] Add failing historical-only checkpoint/provenance/bounds/persistence/rollback/restart tests.
- [x] Implement bounded checkpoint data and extraction; persist through existing compaction transaction and render as continuity data.
- [x] Verify current admitted input exclusion, failed save/cancel/stale result behavior and fresh context restoration.
- [x] Obtain separate spec and quality review.

Expected files: sessions/checkpoint.rs, sessions/mod.rs/context.rs, agent_runs/compact.rs and compaction tests; exact shared contract additions only when required.

## 3. Prompt-only tool-output pruning

- [x] Add failing projection tests for old large outputs, recent/current work protection, unknown outcomes, pairing and immutability.
- [x] Implement bounded core policy and per-request integration; default off and parse strict opt-in settings.
- [x] Capture actual outgoing request byte reduction and unchanged canonical/history bytes.
- [x] Obtain separate spec and quality review.

Expected files: anima-core/src/tool_output_pruning.rs, runtime.rs and runtime projection tests; host configuration/prompt integration only if required.

## 4. Change-bound completion evidence

- [x] Add failing path/version/scope tests, including unrelated Nx/Cargo green checks and concurrent external edits.
- [x] Implement per-path core evidence and exact owner command/cwd/coverage contracts, bounded host digest capture before/after settled checks.
- [x] Preserve read-back, advisory reminder, Stop, deadlines, approval receipts and unknown-effect semantics.
- [x] Obtain separate spec and quality review.

Expected files: anima-core/src/completion.rs and tests; rust-daemon/src/tools/completion.rs/completion/* and minimal tools.rs dispatch integration.

## 5. Final integration

- [x] Update owner docs and exact OpenAPI inventory from registered routes.
- [x] Run rust-daemon:test --skipNxCache (core dependency), rust-daemon:lint, daemon/core builds and applicable packaged-host/SDK/client checks.
- [x] Run isolated acceptance for evaluated promotion/fresh context/Undo; separate deterministic and live-provider evidence.
- [x] Confirm main and inspect cached/unstaged changes; leave everything uncommitted and preserve concurrent work.

## Execution evidence

- Implemented and independently reviewed all four changes; review regressions were reproduced and fixed before final validation.
- `rust-daemon:test --skipNxCache` passed, including its full core-rust dependency and host unit/integration/doc tests (host unit suite: 2956 passed, 31 ignored). `rust-daemon:lint`, `rust-daemon:build`, `anima:test`, SDK build/test (253 tests), focused AnimaAppCard tests (21), web typecheck and companion/daemon docs checks passed.
- Web checks used the available Node 24.19 runtime after Node 22.12 failed during Undici/jsdom initialization; no dependency changes were needed. Rust used an isolated Cargo validation directory and the established Windows Nx environment.
- Real Anthropic claude-sonnet-4-6 evaluation: baseline 0/2, candidate 2/2 including heldout, passing report left the draft pending, exact owner approval promoted it, and a separate empty agent/session answered a new case correctly with that skill. This is bounded response-only fixture evidence, not a claim of general learning quality.
- Separate actual-tool acceptance: incorrect baseline net 16500, automatic correction net 12500, independently checked fresh input net 14975; fresh agent loaded the automatically learned package. Settled state survived a real daemon restart, and owner Undo restored exact original body/supporting files and hash. All isolated acceptance processes were identity-checked and stopped.
- Moonshot returned HTTP 401, producing inconclusive evaluation with no promotion. Anthropic supplied the successful live proof. ChatGPT subscription evaluation is explicitly unsupported until its transport can enforce the requested output-token control.
- Context checkpoint, request-copy pruning and file-version completion guarantees were verified with deterministic capture, persistence, cancellation, rollback, multi-iteration and external-edit regressions. Pruning test requests shrink by more than half while canonical history stays unchanged.
- Sanitized logs and combined verification summary: `.tmp/skill-learning-quality-2026-10-23/verification-summary.md`; live evaluation and automatic-learning proofs: `.tmp/skill-learning-quality-live-anthropic-2026-10-23/`. These ignored artifacts are retained locally.
- Owner configuration and limits are documented in `docs/companion/learning-context-quality.md`. No staging, commits, pushes or branch changes were performed.
