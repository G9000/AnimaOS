# Long-running work continuity

Date: 2026-10-27

Status: Proposed for coordinated design review. This document does not release implementation.

## Objective and scope

Make saved work understandable and controllable across hours, days, interruptions, and daemon restarts. Owners should see which explicit steps finished, which remain, why work is waiting, and the evidence behind a recovery choice. Continue through the existing workflow, job, run, receipt, and execution-budget machinery.

The proposed slice adds durable workflow pause/continue decisions, a clearer progress and review presentation in Work and chat, a read-only inspector for a workflow attempt, and richer inspection of ordinary chat recovery. It retains the existing bounded step definitions and recovery admission rules. It does not convert an arbitrary conversation into an indefinitely running mission.

The initial phase changes only this spec and its matching plan. No source, tests, manifests, generated inventories, credentials, or live profiles are changed. Implementation must wait for the coordinator to assign shared files and release the work. Use the shared checkout, cmd.exe with login:false, and no branch/worktree/commit/push/deploy operations.

## Evidence from the current tree

| Owner | Existing behavior to reuse |
| --- | --- |
| `packages/core-rust/crates/anima-core/src/workflows.rs` | Host-independent sequential state machine; 1–16 immutable task/approval/input steps; completed results; three task attempts; bounded revisioned owner decisions; reachable-history validation. No pause control currently exists. |
| `hosts/rust-daemon/src/jobs/workflows.rs` | Durable workflow/job linkage, one physical job attempt per workflow attempt, current permission checks, reconciliation, reviewed retry or owner-provided verified result, cancellation plus native Stop. |
| `hosts/rust-daemon/src/jobs.rs` | Transactional snapshot mutations survive a dropped caller; claims are checked in the transaction; restart marks previously running jobs as needing review. Queued work is durable. |
| `hosts/rust-daemon/src/agent_runs/recovery.rs`, `runs/recovery.rs`, `runs/turn.rs` | Owner-reviewed continuation creates one linked successor; exact duplicate requests reuse it. Host-bound checkpoint evidence, original session/input binding, current grants, answered-input checks, and narrowly eligible never-started automatic recovery already exist. |
| `hosts/rust-daemon/src/execution_budgets.rs` | Recovery inherits its original execution scope and deadline. Started legacy work without durable accounting is refused. Independent job runs are separately admitted bounded work. |
| `packages/core-rust/crates/anima-core/src/turn_recovery.rs` | Bounded progress excerpts and pending tool intents are evidence, never authority or proof of a tool effect. |
| `apps/web/src/components/WorkspaceWorkflows.tsx` | Work and chat share workflow cards. Cards show all steps/results and retry/resolve controls, but mainly expose job IDs for inspection. Pending writes retain their exact revision/payload for lost-ack retries. Refresh currently stops after 60 polls. |
| `apps/web/src/components/sessions/RunRecovery.tsx`, `RunOutcomeCard.tsx` | Ordinary recovery has a review checkbox and one continuation action. A saved continuation is reported as saved, without checking its subsequent status. Recorded action previews already have cautious success/failure/unknown labels. |
| `hosts/rust-daemon/src/jobs/workflows/tests.rs`, `app/turn_crash_tests.rs` | Existing restart, interrupted-effect, duplicate-decision, rollback, cancellation, and real process-kill fixtures can be extended. |

These are observations of source, not fresh runtime acceptance results. No Cargo processes were started during design.

## Alternatives

1. **Extend canonical workflows and recovery (recommended).** Add pause as a saved owner decision, expose existing evidence, and improve the two existing surfaces. This provides restart semantics without another scheduler or competing execution record. It requires a bounded change to workflow history validation.
2. **Presentation only.** Count steps, link results, and improve recovery wording. Lowest risk, but an owner still cannot hold later work while preserving a resumable workflow. It does not meet the control requirement.
3. **New mission engine with automatic replanning.** Can express a wider goal but duplicates admission, budgets, approvals, Stop, and recovery. It expands scope and could turn an interrupted effect into a replay. Defer it.

## Canonical state and public contracts

### Workflow pause is a dispatch hold

Extend the existing core types, with camelCase JSON and strict mutation request parsing:

```rust
// Existing enum gains two owner decisions.
pub enum WorkflowResumeAction {
    Approve, Answer, Retry, Resolve, Pause, Continue,
}

// Existing WorkflowRun gains one backward-readable field.
#[serde(default, skip_serializing_if = "is_false")]
pub paused: bool,

fn is_false(value: &bool) -> bool { !*value }
```

The semantic contract is default false, omitted when false. `paused` is the current projection of pause/continue decisions in the existing `decisions` history. Do not add another pause registry or change the meaning of `WorkflowStatus` / `WorkflowStepStatus`.

Reuse `POST /api/agents/{agent_id}/workflows/{workflow_id}/resume`:

```json
{"revision":7,"action":"pause"}
```

```json
{"revision":9,"action":"continue"}
```

Both actions require `value` absent/null and `acknowledgeUncertain` absent/false. They cannot authorize effects, approve a checkpoint, resolve a failure, or replenish any allowance. New checkpoint/retry/resolve decisions require the hold to be cleared first. Exact historical replays are checked before current-state eligibility, as they are today, so replaying an old approval after a pause only returns the current saved workflow.

| Operation | Preconditions and durable result |
| --- | --- |
| Pause | Any nonterminal, currently unpaused workflow. Save the decision at the supplied current revision, set `paused=true`, increment the existing revision once, preserve current cursor, error, results, attempts, approval/input state, and all job/run bindings. |
| Continue | Any nonterminal paused workflow. Save the decision, clear the hold, and increment once. Keep the underlying state: a waiting approval still needs approval; a failed task still needs review. |
| Exact repeat | Same original revision and full payload returns current workflow with no new decision, job, run, attempt, or permission. A different payload at that revision is 409. |
| Stop workflow | Existing `/cancel` behavior: durably cancel remaining steps and request Stop of an active run. The workflow stays terminal. Earlier results and the pause decision history remain inspectable. |
| Completion while held | A task already durably claimed may settle. Reconciliation may save its result and advance the cursor; the hold stays in place. If it was the last step, terminal completion wins in the UI. |

Race boundary: a durable job claim is the boundary for already started work. A paused workflow may retain an attached queued job; it must not claim that job. A claim saved before Pause may proceed through ordinary run admission and finish, even if the HTTP pause response arrives first. State this in the UI. To stop such work, use Stop workflow.

Changes to host dispatch are narrowly scoped:

- `needs_reconciliation` / `reconcile_workflows` may settle an attached terminal job while paused, but must not attach a new job while paused.
- `dispatch_allowed` rejects a workflow's queued job when paused. Its in-transaction recheck in `jobs.rs` closes the selection/claim race. An already claimed `Running` job is allowed through the existing run-start check, subject to current grants and cancellation.
- Account-rule revocation still runs its existing reconciliation and blocks new dispatch. Pause must not hide revocation, drop uncertain evidence, or override `stop_requested_at_ms`.
- On Continue, an already attached queued job is reused; do not allocate another workflow attempt.

Core validation must reconstruct the paused flag from the decision history, not trust it independently. Extend the bounded replay state with `paused`; pause/continue are explicit no-cursor-movement events; attach is forbidden while held; completion/review of an already attached job remains reachable. Validate decisions, revision increments, timestamps, and step indices together. Old snapshots default to unpaused and remain reachable. A history with `paused=true` but no matching decision is rejected. Preserve the existing 64-decision cap. Require two free decision slots before accepting a new Pause, reserving room for its Continue; return a concrete conflict rather than save a hold that cannot be cleared. Other decisions cannot consume that reserved room while paused.

Adding variants and a publicly constructible field is a Rust source compatibility change inside this workspace. Update exhaustive matches and struct literals together. Old readers with strict deserialization will not understand a snapshot written with pause support: do not claim downgrade compatibility, and do not silently discard a hold on load.

### Response-only run evidence

Add host-owned projection types in a focused `hosts/rust-daemon/src/runs/continuity.rs`, mirrored in `packages/sdk/src/runs.ts`. They contain no executor or permission grant:

```ts
export interface RunContinuityEvidence {
  actionReceipts: ActionReceipt[];
  // Existing legacy tool names with no corresponding receipt; not proof of dispatch.
  unreceiptedTools: string[];
  omittedUnreceiptedToolCount: number;
  checkpoint: {
    sequence: number;
    phase: 'before_model' | 'model_responded' | 'tools_settled';
    modelCalls: number;
    excerpts: { role: 'assistant' | 'tool'; text: string }[];
    intendedTools: { callId: string; name: string }[];
  } | null;
}

// Additive field on existing RunRecoverySummary.
evidence?: RunContinuityEvidence;
```

Project from one retained canonical run. Use the complete already bounded receipt vector (at most 50), at most 50 unmatched legacy names with an explicit omitted count, and the existing checkpoint limits (24 excerpts, 1024 UTF-8 bytes each; 64 pending intents). Filter excerpts to assistant/tool roles. Do not expose binding/config hashes, raw provider traffic, hidden reasoning, secrets from raw tool arguments, or a fabricated remaining-task list. Retain the stored preview truncation and durability flags. Checkpoints may be incomplete and untrusted; pending intents are not a to-do queue.

Add this field to the existing owner-only `GET .../runs/{run_id}/recovery`. Derive it with the summary under the coordinator transaction plus state read guard, so provisional rollback candidates do not appear reviewable. Do not change the recovery fingerprint algorithm or POST payload. Before the first saved continuation, the fingerprint already covers stored turn and receipt evidence. Once a continuation is saved, the historical fingerprint stays fixed; later native settlements cannot reopen it.

The existing POST keeps 202 for a newly saved child and 200 for an exact replay. A response proves admission/queueing, not that the child ran or completed. The UI must use `runs.get` to check the linked child's current status and must never send the recovery POST automatically while polling.

### Read-only workflow attempt inspector

Add an owner-authorized GET:

`/api/agents/{agent_id}/workflows/{workflow_id}/steps/{step_id}/attempts/{job_id}`

```ts
export interface WorkflowAttemptEvidence {
  workflowId: string;
  workflowRevision: number;
  stepId: string;
  jobId: string;
  stepAttempt: number; // 1-based position in this step's attempts
  status: AgentJob['status'];
  startedAtMs: number | null;
  finishedAtMs: number | null;
  result: string | null;
  error: string | null;
  resultTruncated: boolean;
  evidenceState: 'not_started' | 'retained' | 'unavailable';
  run: {
    id: string;
    sessionId: string;
    status: RunStatus;
    evidence: RunContinuityEvidence;
  } | null;
}
```

`WorkflowsClient.inspectAttempt(agentId, workflowId, stepId, jobId, {signal?})` returns this object. One attempt per read avoids multiplying receipt payloads. At most three attempts are selectable from the existing step history; no pagination or unbounded query is needed.

Under the same control-plane transaction/read guard, verify the agent owns the workflow, the step exists, the job ID appears in that exact step's attempts, and its immutable `WorkflowJobBinding` matches the workflow, step, index, attempt, and agent. Locate retained runs by exact `RunSource::Job` plus `source_ref = "{job_id}:{job.attempt}"`, never by prompt/title or client-supplied run ID. Workflow jobs have `max_attempts=1`; multiple matching retained runs are a conflict, not a choice to guess. Only report `not_started` for an unclaimed queued job with attempt zero and no run. Missing/pruned evidence is `unavailable`, never proof that no effect happened. A saved job result can remain visible even when its detailed run evidence is unavailable.

Responses: 200 for an owned projection; 403 before lookup for missing owner authorization; 404 for absent/cross-owner workflow, step, or attempt; 409 for inconsistent retained linkage; 503 if a consistent read cannot be obtained. Use existing error envelopes and `Cache-Control: no-store`. Invalid mutation shapes stay 400; stale revision or invalid transition stays 409. No new public error taxonomy is required.

This endpoint grants no retry/approval rights. The UI compares `workflowRevision` with its loaded card revision before enabling a new review decision; a mismatch clears acknowledgement and refreshes. Retry and verified-result mutations keep their existing original-revision idempotency semantics. The owner attests a verified business result; the UI never rewrites native action receipts.

## Persistence, restart, revocation, and limits

- All new execution control lives in `WorkflowRun` and its existing decision vector in the control-plane snapshot. No new database, table, durable loop, clock service, or store version is needed to read older snapshots. Strict old binaries must refuse rather than erase new paused data.
- Acknowledgement loss is not a failure to save. Continue using `JobService::mutate` and coordinator-owned admission. Request disconnection or a dropped future must not abandon a started durable mutation. Use the existing owned task custody and rollback rules.
- Restored queued jobs remain eligible only through existing ownership/grant gates and the pause gate. Previously running jobs follow existing `start()` behavior: mark uncertain, then reconcile to Needs review, even if paused. They are not replayed.
- Never restart an old tool approval, resurrect browser selection, or convert an approval wait into an allow. Continue clears only the workflow hold. Current runtime grants and connector permissions apply at actual dispatch. Revoked account rules/templates keep their existing deny behavior.
- Ordinary chat recovery inherits its original execution scope, deadline, consumption, and outstanding reservations. If expired/exhausted/stale or missing trusted accounting, it is refused. Show the host's reason. Do not offer a fresh-budget workaround from the recovery card.
- A workflow's later explicit step is an independently admitted bounded job, as it is today. A reviewed workflow Retry is a separately and explicitly authorized attempt within the saved three-attempt limit, with old scope evidence retained. Label it as retrying the step, never as extending an expired turn. Pause and Continue create no run and reset no accounting.
- The existing default execution wall limit is 30 minutes per independently admitted scope in the inspected tree. This feature does not raise it. A finite workflow can span days through steps and owner waits; actual execution still requires the daemon to be running and the computer awake. There is no new periodic wakeup or guaranteed uninterrupted overnight execution.
- Keep the 16-step, three-attempt, 64-decision, result-size, history-capacity, and execution-budget caps. Show a review state or concrete limit message when progress cannot continue. Do not evict accounting to make room.

Tokio cancellation behavior is relevant to the acceptance tests: abort requests are asynchronous and started `spawn_blocking` work cannot be aborted. Preserve the repository's physical custody rules instead of treating a closed browser or dropped handle as proof a write/effect stopped. Documentation checked through Context7 on 2026-10-27: [JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html), [spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

## Owner experience

### Work and active workflow cards

Keep the existing Work → Workflows location and shared chat card. Lead each card with the title and `2 of 5 steps finished · 3 remaining`; derive counts from canonical `steps[].status`, never from model prose or tool-call count. Cancelled steps are shown separately and are not called remaining runnable work. Mark the current step and show its blocker. Collapse finished step details while keeping saved results and previous attempts accessible.

Use plain labels: Task, Question, Approval, Finished, Waiting for your answer, Needs review, Paused, Stopped. Move request keys, job IDs, raw decision names, revision numbers, and payload JSON into optional details. Preserve timestamps using the viewer's locale and existing date formatting.

Add Pause work and Continue work controls. Explain beside Pause: “No new task will start. A task already started may finish.” While held, show that message with the underlying waiting/review reason. Disable approval/answer/retry/resolve until the hold is cleared. Stop workflow uses the existing cancellation endpoint and explains that the current task is asked to stop; effects already performed remain possible. A stopped workflow has no Resume button.

For Needs review, offer three distinct choices: inspect the saved attempt; record a verified result and advance without rerunning; or explicitly retry the current task after acknowledging possible repeated effects. Stop remains available. The inspector shows actual saved results, shortened previews, recorded success/failure/unknown tool outcomes, and incomplete saved excerpts. A missing receipt does not become a success badge. Do not claim that a successful tool receipt verifies the entire business goal.

Keep exact pending-write payloads/revisions for lost acknowledgements, but describe them in normal language (“Your pause request may already be saved. Check it or retry the same request.”). Retrying the saved request must not mint a key, use a new revision, or start a different attempt. Clear review acknowledgement when the card revision, evidence, selected attempt, agent, or connection generation changes.

Refresh active visible work while connected with the existing bounded polling strategy and an explicit last-checked time/manual Refresh control. Distinguish “Automatic refresh paused” from a workflow hold and from daemon offline. Browser polling never performs execution or recovery. A page reload always reads the canonical saved workflow before enabling controls.

### Ordinary interrupted chat

Keep the existing per-run recovery entry point. Expand Review interrupted work to show the evidence from the same response as its fingerprint, the number of recovery attempts remaining, and a clear explanation: a new attempt inspects saved evidence and works only within the original remaining allowance and current permissions. It does not resume the exact interrupted model operation.

Choices are “Continue remaining work” after required review, “Leave interrupted” (close this review locally without changing source authority), and “Refresh saved evidence.” Unknown effects stay unknown until a native receipt settles or the owner verifies them externally. There is no action-by-action “mark succeeded” button or automatic resend.

After acceptance show Queued / Running / Waiting for approval / Finished / Needs attention / Stopped based on `runs.get` of the returned linked run, with bounded cancellable polling and manual Refresh continuation. A failed status lookup says the status could not be checked; it must not offer a second successor. The persisted source link is authoritative after reload and duplicate requests. The final answer stays in its existing chat; do not synthesize a completion from admission or partial streamed progress.

## Ownership proposal and coordination points

The coordinator must assign these paths before implementation. New files below are unique to this feature. Existing paths remain unclaimed until review.

| Proposed owner paths | Responsibility |
| --- | --- |
| `packages/core-rust/crates/anima-core/src/workflows.rs` | Pause/continue decisions, current flag, bounded reachable-history validation. |
| NEW `packages/core-rust/crates/anima-core/tests/workflow_continuity_contract.rs` | Public state-machine and restore invariants. |
| `hosts/rust-daemon/src/jobs/workflows.rs`; NEW `hosts/rust-daemon/src/jobs/workflows/continuity.rs`; NEW `hosts/rust-daemon/src/jobs/workflows/continuity_tests.rs` | Pause-aware attach/claim gates and consistent attempt projection. Register focused submodules in the existing workflow owner. |
| NEW `hosts/rust-daemon/src/runs/continuity.rs`; `hosts/rust-daemon/src/runs/mod.rs`; `hosts/rust-daemon/src/runs/recovery.rs`; `hosts/rust-daemon/src/agent_runs/recovery.rs` | Read-only run evidence and consistent recovery inspection. Recovery admission semantics remain in their existing owner. |
| `hosts/rust-daemon/src/routes/workflows.rs`; `hosts/rust-daemon/src/routes/mod.rs`; `hosts/rust-daemon/src/routes/tests/workflows.rs`; `hosts/rust-daemon/src/routes/tests/docs_contracts.rs` | Attempt GET, route/OpenAPI registration, auth/contract coverage. |
| `packages/sdk/src/workflows.ts`; `workflows.spec.ts`; `runs.ts`; `runs.spec.ts`; `index.ts` | Additive wire types and inspectAttempt client. |
| `apps/web/src/components/WorkspaceWorkflows.tsx`; NEW `apps/web/src/components/workflows/WorkflowProgress.tsx`; NEW `WorkflowAttemptInspector.tsx`; NEW `apps/web/src/components/sessions/RecoveryEvidence.tsx`; NEW `ContinuationStatus.tsx`; `apps/web/src/components/sessions/RunRecovery.tsx`; `apps/web/src/lib/daemon-api.ts` | Existing surface orchestration, readable progress, pause controls, evidence and child status. Add one `inspectWorkflowAttempt` forwarding method beside existing workflow facade methods. Use existing utility classes; no global CSS change planned. |
| `apps/web/src/components/WorkspaceWorkflows.test.tsx`; NEW `apps/web/src/components/workflows/WorkflowAttemptInspector.test.tsx`; `apps/web/src/components/sessions/RunRecovery.test.tsx`; NEW `ContinuationStatus.test.tsx`; existing `sessions/ChatWorkflows.test.tsx` | Behavior, stale-context and duplicate/lost-ack UI coverage. |
| NEW `hosts/rust-daemon/src/app/workflow_continuity_crash_tests.rs`; `hosts/rust-daemon/src/app.rs`; `hosts/rust-daemon/src/app/turn_crash_tests.rs` | Reuse `OwnedDaemon`/profile/marker fixture; add only bounded test-only stages when existing stages do not suffice. |

Shared files most likely to need coordinator integration: `hosts/rust-daemon/src/routes/mod.rs`, `hosts/rust-daemon/src/routes/tests/docs_contracts.rs`, `hosts/rust-daemon/src/runs/mod.rs`, `hosts/rust-daemon/src/app.rs`, `hosts/rust-daemon/src/app/turn_crash_tests.rs`, `packages/sdk/src/index.ts`, `packages/sdk/src/runs.ts`, `apps/web/src/lib/daemon-api.ts`, and `apps/web/src/components/WorkspaceWorkflows.tsx`. Feature-specific host projection must stay outside generic `state.rs`, `agent_runs.rs`, `jobs.rs`, and execution-budget implementations unless a proven integration issue requires a separately reviewed change. No manifest or dependency changes are planned.

Integration with the other three designs is limited to shared registration/export files and reuse of any agreed evidence component/types. Do not bundle project search, account setup, files, or new tool permissions into this feature.

## Acceptance and meaningful verification

| Case | Required observable result |
| --- | --- |
| Pause before attach/claim | No model/tool invocation, no extra job/attempt; an attached unclaimed job remains held and is reused on Continue. |
| Pause after durable claim | At most the claimed step proceeds; its actual result may settle; no later step starts. Stop can still cancel it. |
| Pause/Continue across restart | Saved hold and decision revision survive. No earlier finished result/job/run identity changes. Continue clears only the hold. |
| Crash during effect with lost acknowledgement | Real fixture effect marker occurs once; restart shows review/unknown evidence; automatic processing and read-only inspection never repeat the effect. Verified-result resolution advances without repeating it. |
| Crash after durable completion, before UI acknowledgement | Restart reads the same saved result and advances only remaining steps. Duplicate decision does not create another task. |
| Duplicate/lost-ack Pause, Continue, Retry, Resolve, Stop | Retry original revision/payload; one saved decision and at most one admitted successor/attempt. Changed payload at that revision conflicts. |
| Review state while held | Error and evidence remain visible. Continue does not approve/retry/resolve. Late receipts can settle evidence but never reopen a saved continuation. |
| Permission/account-rule/profile ownership revoked | No new unauthorized work, no revived approvals, and no scope/accounting reset after Continue or restart. |
| Expired/exhausted ordinary recovery | Original scope/deadline and outstanding reservations survive; POST fails, no child/scope created, UI shows concrete reason. |
| Legacy started work without accounting | Inspection is possible, recovery still refuses unsafe admission. Missing detailed evidence is labeled unavailable. |
| Bounded/malformed state | Oversized history, invented paused flag, impossible attach while held, wrong step/job/agent binding, and altered decision replay fail validation. |
| UI disconnect, agent switch, reload, old read, failed save | Stale responses cannot enable controls or clear a different draft/acknowledgement. Accepted child status is checked without a second POST. Persisted state survives browser closure. |

Use fake/scripted models and fixture file effects only. No real sends, providers, credentials, production profile, or live services. Extend existing restart fixtures with deterministic barriers rather than sleeps or manufactured success receipts.

After release, run focused contract/host/SDK/UI checks, then required `bun run rust:test` and applicable workspace test/build/typecheck scripts in the current tree. Rust compile/test/lint requires the coordinator's exclusive build slot, repository task wrappers, the shared `target/cargo/validation` location, and the 32 GiB ceiling. Do not run Cargo during this design phase. Record unrelated failures separately without modifying their files.

## Product decisions for coordinator review

Recommended defaults are concrete and do not require another human approval question: Pause holds unclaimed work and lets an already claimed step settle; Stop stays terminal; continuing a paused review/approval never decides it; finite existing workflow limits stay unchanged; no automatic retry of uncertain steps or sends; no background schedule is installed.

If “multiday mission” is intended to mean automatic dynamic step generation, automatic daily budget renewal, calendar-delayed steps, or resuming stopped work, that is a separate expansion. This proposal delivers multiday persistence and independently admitted bounded steps with explicit review, rather than promising uninterrupted or instruction-pointer resume.
