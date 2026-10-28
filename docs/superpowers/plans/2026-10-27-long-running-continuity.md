# Long-running Work Continuity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task after the coordinator releases implementation. Steps use checkbox (`- [ ]`) syntax for tracking. Do not delegate further unless explicitly authorized.

**Goal:** Make canonical workflows and interrupted chat work visibly reviewable, pausable, and safely continuable across restarts.

**Architecture:** Extend the existing core workflow decision history with a dispatch hold, keeping its current lifecycle and finite steps. Derive read-only progress and attempt evidence from canonical workflow/job/run state, and reuse current reviewed recovery, Stop, permissions, and execution accounting. The browser displays and requests decisions; it never runs work.

**Tech Stack:** Existing Rust core/daemon, serde, Tokio, Axum, TypeScript SDK, React, Vitest, repository Bun task wrappers. No new dependencies or persistence store.

**Design:** `docs/superpowers/specs/2026-10-27-long-running-continuity-design.md` is the contract authority for this proposal.

---

## Release and verification constraints

- Implementation was released and integrated in the shared checkout. The unchecked task-level boxes below are the original planned steps, not a claim that each individual case has been independently verified.
- Wait for the coordinator's design review, exact shared-file assignment, and implementation release. Do not ask the human repeated approval questions.
- Keep this shared checkout and branch. No worktrees, branch changes, commits, pushes, deployments, real accounts, credentials, or unrelated cleanup.
- Use cmd.exe with login:false. Re-read Git status at release and preserve all pre-existing edits, including Rust cache/dev tooling, SDK package/test changes, and results-panel work.
- Never start concurrent Cargo processes. Ask the coordinator for the build slot before every compile/test/lint batch, then run Rust checks sequentially with repository wrappers and the shared 32 GiB ceiling. Do not create feature-specific target directories.
- Skills when executing: @superpowers:executing-plans, @superpowers:test-driven-development for meaningful behavior, @rust-best-practices, @superpowers:verification-before-completion. Use Context7 if implementation adds library-specific behavior beyond the checked Tokio custody contract.
- Do not run tests for these two prose files. Later required tests must pass in the final current tree before claiming implementation complete.

## Ownership map

All paths are repository-relative; the root is `C:\Users\leoca\OneDrive\Desktop\anima\animaos-kit`.

Exclusive feature candidates:

- Core: `packages/core-rust/crates/anima-core/src/workflows.rs`, new `packages/core-rust/crates/anima-core/tests/workflow_continuity_contract.rs`.
- Host: `hosts/rust-daemon/src/jobs/workflows.rs`, new `jobs/workflows/continuity.rs`, new `jobs/workflows/continuity_tests.rs`, new `runs/continuity.rs`, `runs/recovery.rs`, `agent_runs/recovery.rs`, `routes/workflows.rs`, `routes/tests/workflows.rs`.
- SDK: `packages/sdk/src/workflows.ts`, `workflows.spec.ts`, `runs.spec.ts`.
- UI: `apps/web/src/components/WorkspaceWorkflows.tsx`, `WorkspaceWorkflows.test.tsx`, new `components/workflows/WorkflowProgress.tsx`, new `WorkflowAttemptInspector.tsx` and its test, new `components/sessions/RecoveryEvidence.tsx`, new `ContinuationStatus.tsx` and its test, `RunRecovery.tsx`, `RunRecovery.test.tsx`, `sessions/ChatWorkflows.test.tsx`.
- Crash acceptance: new `hosts/rust-daemon/src/app/workflow_continuity_crash_tests.rs`.

Small coordinator-owned integration requests:

- `hosts/rust-daemon/src/runs/mod.rs`: register `continuity` projection module.
- `hosts/rust-daemon/src/routes/mod.rs`: register one GET route and OpenAPI path; no generic routing rewrite.
- `hosts/rust-daemon/src/routes/tests/docs_contracts.rs`: add the GET path to route inventory.
- `hosts/rust-daemon/src/app.rs`: register the new crash test module under cfg(test).
- `hosts/rust-daemon/src/app/turn_crash_tests.rs`: reuse `OwnedDaemon`, `profile`, `marker`, and only necessary test stage additions.
- `packages/sdk/src/index.ts`: export new response types.
- `packages/sdk/src/runs.ts`: additive `RunContinuityEvidence` and recovery evidence field; coordinate with other features before editing.
- `apps/web/src/lib/daemon-api.ts`: one `inspectWorkflowAttempt` forwarding method beside `getWorkflow`/`resumeWorkflow`.

No changes planned to manifests, `state.rs`, `jobs.rs`, `agent_runs.rs`, `execution_budgets.rs`, global CSS, generated inventories, or runtime provider configuration. If implementation proves a need there, report a precise integration patch to the coordinator first.

## Task 1: Add a replay-validated workflow hold

**Files:**
- Modify: `packages/core-rust/crates/anima-core/src/workflows.rs` (types around 50–129; `resume`, `validate_resume_request`, `attach_job`, history replay).
- Create/test: `packages/core-rust/crates/anima-core/tests/workflow_continuity_contract.rs`.

- [ ] Write a public contract test for Pause on a pending task: cursor, attempts, results, and error stay unchanged; `paused=true`; revision/decision count increase once; `attach_job` fails while held.
- [ ] Write a test for Continue: clears only the hold; a pending task becomes attachable without an extra decision; approval/input/review underlying states remain unchanged until their separate decisions.
- [ ] Write replay/restore tests: exact duplicates return no change even after later progress; different payload at the original revision conflicts; old snapshots default false; a fabricated flag or impossible held attach is rejected. At the history cap, Pause must reserve a second decision slot for Continue rather than strand a hold.
- [ ] With the coordinator's Rust slot, run the focused test before implementation and confirm failure on the missing behavior.
- [ ] Add `Pause`/`Continue`, `paused` default false (omit false), strict empty-value/false-ack payload validation, and the new transition branches. Require two free decision slots for a new Pause. Preserve error on hold changes. Reject new checkpoint/retry/resolve decisions while paused; historical exact replay still comes first.
- [ ] Extend reachable-history replay with a paused bit. Permit settlement/review of already attached jobs while held, forbid attachment while held, retain 64 decisions and monotonic timestamp/revision validation. Terminal workflows cannot receive new pause/continue decisions; previously saved hold evidence may remain after completion/cancellation.
- [ ] Run the focused tests again, then existing core workflow tests. Check every existing `WorkflowRun` literal/exhaustive action match in owning files; do not reformat unrelated core files.

Example behavior to encode in tests:

```text
w = Task -> Approval -> Task
Pause(rev1) -> paused, Task pending, no attempt
duplicate Pause(rev1) -> unchanged
Continue(rev2) -> Task pending
attach job1 -> attempt1, Running
Pause(current rev) -> hold, same job1
complete job1 -> Approval waiting, hold retained
Continue(current rev) -> Approval waiting
replay old Continue -> no approval, no task2
Approve(current rev) -> only then next Task
```

## Task 2: Enforce hold at attachment and claim boundaries

**Files:**
- Modify: `hosts/rust-daemon/src/jobs/workflows.rs` (`needs_reconciliation`, `reconcile_workflows`, `dispatch_allowed`, focused test registration).
- Create/test: `hosts/rust-daemon/src/jobs/workflows/continuity_tests.rs`.

- [ ] Write deterministic tests for Pause before attachment and after an attached job is queued. Assert no new model invocation; Continue reuses the queued job/step attempt.
- [ ] Write claim-order tests: Pause wins the transaction before claim → no claim; claim wins before Pause → at most that job may run/settle, never a later step. Reuse existing claim barriers and scripted/waiting model conventions.
- [ ] Write tests for a held active job succeeding/failing, and for account-rule revocation while held. Error/evidence must remain; no unauthorized next job may start.
- [ ] Run focused tests red under the Rust slot.
- [ ] Gate new attachment and queued claims on `!workflow.paused`; let an already durably claimed Running job pass normal admission. Do not override current grants, account-rule checks, Stop, or source binding.
- [ ] Let reconciliation save an existing terminal result/review state while held. Ensure no busy reconciliation loop when the only remaining action is attaching a held step.
- [ ] Run focused tests green, existing workflow cancellation/restart tests, and validate saved workflow/job linkage after each race.

## Task 3: Project bounded canonical evidence

**Files:**
- Create: `hosts/rust-daemon/src/runs/continuity.rs`.
- Create: `hosts/rust-daemon/src/jobs/workflows/continuity.rs`.
- Modify: `hosts/rust-daemon/src/runs/recovery.rs`, `hosts/rust-daemon/src/agent_runs/recovery.rs`, `hosts/rust-daemon/src/jobs/workflows.rs`.
- Shared integration: `hosts/rust-daemon/src/runs/mod.rs`.
- Test: unit tests in new projection file and `jobs/workflows/continuity_tests.rs`.

- [ ] Write projection tests for succeeded/failed/unknown receipts, memory-only durability, shortened previews, incomplete checkpoint excerpts, unmatched legacy names with omitted count, and absent evidence. Assert projection performs no mutation or reservation/admission.
- [ ] Write attempt ownership tests for wrong agent/workflow/step/job, mismatched immutable binding, pruned run evidence, an unclaimed queued job, and multiple matching runs.
- [ ] Run focused tests red under the Rust slot.
- [ ] Implement the exact `RunContinuityEvidence` and `WorkflowAttemptEvidence` schemas in the spec. Filter progress roles to assistant/tool; expose no raw binding/config data or hidden reasoning. Do not infer completion from model text or tool intents.
- [ ] Derive recovery evidence under the coordinator transaction/read guard and attach it to the existing GET summary. Keep the fingerprint algorithm and POST admission payload unchanged.
- [ ] Add a JobService read method that validates ownership/linkage under the same transaction and returns one attempt projection. Find the run using the exact Job source/source-ref pair. Missing history is unavailable; it is never proof of no effect.
- [ ] Run focused tests green and existing recovery admission tests, including one-successor replay and shared scope/deadline assertions.

## Task 4: Wire the owner API and SDK

**Files:**
- Modify: `hosts/rust-daemon/src/routes/workflows.rs`, `hosts/rust-daemon/src/routes/tests/workflows.rs`.
- Shared integration: `routes/mod.rs`, `routes/tests/docs_contracts.rs`.
- Modify: `packages/sdk/src/workflows.ts`, `workflows.spec.ts`, `runs.spec.ts`.
- Shared integration: `packages/sdk/src/runs.ts`, `packages/sdk/src/index.ts`.
- Shared integration: `apps/web/src/lib/daemon-api.ts` (one forwarding method).

- [ ] Write HTTP tests: missing owner authorization is rejected before lookup; valid attempt projection is no-store; cross-owned/mismatched step/attempt is not disclosed; malformed pause/continue requests are 400; stale decisions are 409; exact replay survives save/reload.
- [ ] Write SDK request tests for `inspectAttempt`, encoding all IDs, passing AbortSignal, decoding bounded evidence, and forwarding pause/continue with original revision unchanged.
- [ ] Run focused red tests (Rust serialized; SDK wrapper as below).
- [ ] Add GET `/api/agents/{agent_id}/workflows/{workflow_id}/steps/{step_id}/attempts/{job_id}` and register route/OpenAPI inventory through coordinator-owned edits. Return existing error envelopes/status conventions.
- [ ] Mirror response types in the SDK, add `WorkflowsClient.inspectAttempt(...)`, extend action union and optional paused/evidence fields for older daemon responses, and export types. Add `daemon.inspectWorkflowAttempt(agentId, workflowId, stepId, jobId, options)` forwarding to `setupClient.workflows.inspectAttempt(...)`; reuse existing `resumeWorkflow` for decisions.
- [ ] Run focused green tests and SDK build/typecheck. Confirm no manifest/build-output edits are included in the feature patch.

## Task 5: Make Work progress and recovery usable

**Files:**
- Modify: `apps/web/src/components/WorkspaceWorkflows.tsx` and its test.
- Create: `apps/web/src/components/workflows/WorkflowProgress.tsx`.
- Create: `apps/web/src/components/workflows/WorkflowAttemptInspector.tsx` and its test.
- Create: `apps/web/src/components/sessions/RecoveryEvidence.tsx`.
- Modify/test: `apps/web/src/components/sessions/ChatWorkflows.test.tsx`.

- [ ] Write tests for finished/remaining counts, cancelled-step labeling, current blocker, held approval/review, and Pause/Continue/Stop payloads. Counts must come from steps, and no new decision is enabled by polling alone.
- [ ] Write inspector tests for stored result and receipt labels, unavailable details, exact attempt selection, stale workflow revision, delayed old responses, agent switch, offline/reconnect, and cleared review acknowledgement on evidence changes.
- [ ] Extend lost-ack UI coverage: saved Pause/Continue/Retry/Resolve/Stop request retains exact revision/payload across retry/reload; a failed acknowledgement must not create a second attempt or lose the still-pending request.
- [ ] Run focused web tests red.
- [ ] Extract focused presentation components, retain existing canonical read/mutation orchestration, and render clear counts/blockers. Keep IDs, revisions, and raw payloads behind Details. Use utility classes already available; avoid global styling churn.
- [ ] Add Pause work/Continue work help text and controls. Keep Stop terminal. Hold disables new approve/answer/retry/resolve decisions until Continue. An already started step may finish; do not claim immediate process cancellation.
- [ ] Replace job-ID-only inspection with a lazily fetched attempt inspector using the new SDK method. Record verified result and Retry task remain separate existing actions; never write synthetic success receipts.
- [ ] Preserve the existing exact pending-command mechanism and bounded polling. Show last-checked time and differentiate auto-refresh suspension, daemon offline, and a workflow pause.
- [ ] Run focused green tests, including the shared chat workflow surface.

## Task 6: Show evidence and verify the linked chat continuation

**Files:**
- Modify: `apps/web/src/components/sessions/RunRecovery.tsx` and its test.
- Create: `apps/web/src/components/sessions/ContinuationStatus.tsx` and its test.
- Reuse: `apps/web/src/components/sessions/RecoveryEvidence.tsx`.
- Test existing integration through `apps/web/src/components/sessions/RunOutcomeCard.test.tsx` if present; otherwise add the assertions to RunRecovery tests without editing unrelated transcript code.

- [ ] Write tests for evidence and its fingerprint arriving in one response, unknown receipts, historical continuation link after reload, remaining attempts, and the Leave interrupted action performing no mutation.
- [ ] Write status tests: a POST result is Queued/accepted, never immediately Finished; status updates follow only GET of the returned child; failed GET retains its child identity; terminal state stops polling; unmount/agent/run changes fence stale responses.
- [ ] Add a lost-ack test where POST saved its child but the response is lost: Refresh finds `continuationRunId`; no new POST or successor is created by status polling.
- [ ] Run focused web tests red.
- [ ] Render consistent evidence and review acknowledgement; reset acknowledgement on a refreshed fingerprint/evidence change. Explain same original remaining allowance/current permissions and incomplete saved excerpts. Preserve original fingerprint on the explicit POST.
- [ ] Render child status through bounded cancellable GET polling plus manual Refresh continuation. Use existing Run statuses and ordinary final chat output; do not synthesize an answer or imply instruction-pointer resume.
- [ ] Run focused green tests and existing RunOutcome/Chat workflow tests that touch the same surface.

## Task 7: Prove crash, restart, revocation, and no replay

**Files:**
- Create/test: `hosts/rust-daemon/src/app/workflow_continuity_crash_tests.rs`.
- Shared fixture integration: `hosts/rust-daemon/src/app.rs`, `hosts/rust-daemon/src/app/turn_crash_tests.rs`.
- Extend: `hosts/rust-daemon/src/jobs/workflows/continuity_tests.rs`, `hosts/rust-daemon/src/agent_runs/recovery.rs` tests only when needed.

- [ ] Reuse the real `OwnedDaemon` fixture, isolated parent-owned temporary profile, controlled fixture model, file marker, and unique test stage. Never point at the owner's running daemon/profile or real accounts.
- [ ] Test a pause saved before process kill: restart cannot invoke the held queued step; Continue admits it once; earlier step results/job IDs remain identical.
- [ ] Test a kill during a fixture file effect before its receipt is acknowledged: the marker shows one effect; restart requires review; inspection/polling do not replay; Record verified result advances without repeating the effect.
- [ ] Test kill after the first step's durable result but before next step dispatch/HTTP acknowledgement: saved completion is preserved, only remaining work starts, and original-revision decision retries remain no-ops.
- [ ] Test dropped pause/continue request futures and save failure gates: acknowledged state is durable; failed save rolls back the hold/decision together; no dispatch relies on provisional unacknowledged state.
- [ ] Test old tool approval remains a wait, Stop remains terminal, account/profile/grant revocation blocks new effects, and ordinary recovery cannot renew an expired/exhausted scope or settle unknown reservations for free.
- [ ] Under the Rust slot, run new crash/restart tests sequentially and existing actual-process turn crash tests. Use barrier/marker state to control timing, not long sleeps. Check fixture processes exit and task-owned temp output is removed through the wrapper.

## Task 8: Integrated validation and handoff

Run only after implementation release. The coordinator owns the Rust build slot and final shared-tree verification timing.

Focused Rust command templates, executed one at a time from the root:

```bat
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-core --test workflow_continuity_contract
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-daemon continuity
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-daemon workflows
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-daemon actual_process_kill
```

Expected: all selected tests pass; all bounded no-replay assertions hold. Confirm the daemon test target/filter still matches current wrappers at release; do not bypass wrapper limits if the tree has changed.

SDK and UI commands (inspected workspace script names):

```bat
bun run --filter @animaos/sdk test -- src/workflows.spec.ts src/runs.spec.ts
bun run --filter @animaos/sdk build
bun run --filter @animaos/sdk typecheck
bun run --filter @animaos/web test -- src/components/WorkspaceWorkflows.test.tsx src/components/workflows/WorkflowAttemptInspector.test.tsx src/components/sessions/RunRecovery.test.tsx src/components/sessions/ContinuationStatus.test.tsx src/components/sessions/ChatWorkflows.test.tsx
bun run --filter @animaos/web build
bun run --filter @animaos/web typecheck
```

- [x] Run the required full `bun run rust:test` after obtaining the exclusive build slot. Run applicable Rust lint/format through repository commands sequentially. Do not raise or bypass the 32 GiB ceiling to finish a check.
  - Final current-tree `bun run rust:test` exited 0; the daemon library recorded 3,661 passed and 38 ignored. Both `actual_process_kill_after_file_effect_never_replays_the_completed_action` and `continuity_process_kill_preserves_hold_and_never_replays_a_verified_effect` passed using the isolated process fixtures. Report: `test-output/continuity-full-rust-formatted.log`.
  - `bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- bun run rust:lint` exited 0, with warnings. The separate offline Windows private-directory regression also passed 1/1. Reports: `test-output/continuity-rust-lint-final.log` and `test-output/continuity-win-custody-final.log`.
  - `bun run --filter @animaos/sdk test --no-file-parallelism` exited 0: 51 files and 291 tests passed, including the daemon integration and personality restart tests. Report: `test-output/continuity-sdk-full-final.log`.
  - These process-kill cases do not establish every planned crash timing or live-provider acceptance. The coordinator reported 165 failures in the broader web suite; that gate remains open.
- [ ] Run SDK/web workspace tests, build, typecheck, and relevant lint after shared integration is stable. Broaden only when a failure or new change justifies it.
- [ ] Review the feature-only diff for unauthorized shared-file edits, manifests, generated inventories, unrelated formatting, and accidental secrets.
- [ ] Record exact validation commands/results and separate pre-existing unrelated failures. If a relevant required check fails, report the limitation and do not claim implementation complete.
- [ ] Inspect and clean only task-owned scratch files/logs/screenshots after recording results; preserve configured test-output reports, active users, tool caches, and Cargo bookkeeping.
- [ ] Hand the coordinator a concise summary, exact changed paths, validation evidence, and remaining integration issues. No commit/push/deploy or message to another chat is part of this plan.

## Final review checklist

- [ ] Progress reflects canonical explicit steps; tool success or prose is never a completed-step claim.
- [ ] Pause persists and blocks new claims; Stop preserves terminal authority; Continue renews neither approval nor accounting.
- [ ] Ordinary recovery retains the original scope/deadline and single successor; workflow retries remain explicit bounded independent attempts with prior evidence retained.
- [ ] Unknown effects, missing/pruned evidence, stale data, and interrupted execution are labeled honestly.
- [ ] Real kill/restart and duplicate/lost-ack cases pass using isolated fixtures with no external sends.
- [ ] All relevant required checks pass in the final shared tree, or limitations are stated without claiming completion.
