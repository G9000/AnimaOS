# Resumable Workflows Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans task by task. Initial implementation stayed on main, unstaged and uncommitted as requested. On 2026-10-21 the owner subsequently authorized committing the workflow changes on main and pushing them to GitHub; unrelated design edits remain excluded.

**Goal:** Save sequential task workflows with durable approval/input checkpoints and continue without repeating completed work.

**Architecture:** A pure Rust state machine in anima-core drives the existing daemon durable job dispatcher. Atomic control-plane snapshots bind immutable steps to exact jobs; SDK and Work hub expose owner-only decisions.

**Tech Stack:** Existing Rust/serde/Tokio/Axum, TypeScript SDK, React, Nx/Bun; no new runtime or dependency.

## Tasks

- [x] 1. Review the spec and plan for correctness and complete the API/transition contract. Independent review approved after clarifying job retry isolation, cancellation custody, deleted-agent history, step-ID bounds, and capacity behavior.
- [x] 2. Add `packages/core-rust/crates/anima-core/src/workflows.rs` (and a focused tests module) plus its `lib.rs` export. Write and observe failing transition/idempotency/bounds tests first, then implement immutable definitions, task linkage/results, owner decisions, retry history, cancellation, and restore validation.
- [x] 3. Add `hosts/rust-daemon/src/jobs/workflows.rs` and tests. Extend JobService transaction rollback to workflows, reconcile workflows in the existing worker, atomically create/link jobs, honor global capacity and drain, and stop active linked jobs on cancellation. Tests must prove successful steps never run twice across a checkpoint and restart, and uncertain/persistence-failed attempts never advance.
- [x] 4. Extend `control_plane_store.rs` and `state.rs` with version-24 workflow persistence, validation/ownership links, default-empty legacy restore, and exact pre-upgrade backup tests. No live data stores are used.
- [x] 5. Add owner routes in `routes/workflows.rs`, register routes/OpenAPI schemas, and add route tests for guard-before-body, cross-agent identity, strict request schemas, stale/idempotent decisions, and no-store responses. Add bounded ordinary-run `create_workflow`/`list_workflows` tools and discovery/permission wiring, without granting them implicitly or permitting model checkpoint decisions.
- [x] 6. Add SDK `workflows.ts` and tests, client/export wiring, and daemon facade methods. Add Workflows to `WorkspaceHub` with a focused `WorkspaceWorkflows` component and tests. Follow the spec contract and existing styles. Owners must be able to create, inspect, answer/approve, acknowledge/retry/resolve, and cancel workflows.
- [x] 7. Update companion workflow docs with one practical example, limits, permission behavior, restart/review rules, and task-step versus deterministic-pipeline distinction. Update this checklist as tasks finish.
- [x] 8. Run focused tests during implementation and one combined required Rust daemon suite, SDK/web test/typecheck/build, and applicable docs build. Review spec compliance then code quality, fix findings, rerun only affected checks, inspect both staged and unstaged Git diffs, and report verified scope and remaining live-provider uncertainty.

## Validation commands

Confirm names/targets via `bun x nx show projects --json` and `bun x nx show project <name> --json`. In this Windows environment set `NX_SKIP_NATIVE_FILE_CACHE=true`, `NX_DAEMON=false`, `NX_ISOLATE_PLUGINS=false`, `NX_NO_CLOUD=true`, and `CI=1` for the child process when needed. Native plugin sockets exceeded the path limit without these values; no repository configuration change is needed.

- `bun x nx run rust-daemon:test --skipNxCache` (use `CARGO_TARGET_DIR=target/cargo/validation-rust-daemon` if the running daemon locks normal outputs).
- Relevant confirmed Nx test/typecheck/build targets for SDK/web and docs; use bounded focused filters for RED/GREEN feedback.
- `git diff --check`, `git diff --cached --stat`, `git diff --stat`, and `git status --short`.

## Validation result

- Required full Rust daemon Nx target passed, including core dependencies, with isolated Cargo outputs because the live daemon locks its normal executable. Final run used four build jobs and four test threads, with debug symbols disabled only in that validation cache. Daemon suites: 2,887 passed, 35 ignored. Core suites: 1,075 passed, 2 ignored. All workflow, rollback, short provider-truncation, shutdown, inventory, and exact version-23 backup regressions passed.
- SDK unit suite: 244 passed, 3 service tests skipped. SDK/web typecheck and builds passed. Workflow UI: 11 passed; Work hub: 16 passed. The broad web suite retains three failures in unchanged avatar/settings tests (2,055 passed, 1 skipped); it is not a fully passing web acceptance run.
- Astro docs build passed using the bundled Node 24 runtime, including Pagefind. Spec compliance and code quality reviews approved after the recorded fixes.
- No live provider/account acceptance, service restart, staging, commit, or attribution change. All tests use temporary or synthetic data. Existing unrelated design edits were preserved.
- Exact commands, prior failures, limits, and logs are recorded in `target/workflows-validation/validation.md` and `target/workflows-validation/sdk-ui-validation.md`.
