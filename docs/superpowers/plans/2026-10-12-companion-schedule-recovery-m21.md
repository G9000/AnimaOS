# M21 Durable Action Receipts and Scheduled Recovery

> **For agentic workers:** Use `superpowers:executing-plans` for bounded steps; root owns execution, five binding rulings, validation, services, and Git.

**Goal:** Preserve native action intent/outcome evidence before final conversation commit, and verify interrupted scheduled work recovers without replay.
**Architecture:** Add bounded receipts to the existing native run ledger and ordered control-plane persistence boundary. Await intent and outcome saves, preserve authority after waits, and expose optional owner evidence through the existing SDK/run view. Reuse the retained private M16 profile for one initial crash probe and one justified fresh-path repair acceptance.
**Tech stack:** Existing Rust daemon/scheduler/run ledger/history outbox, configured `gpt-5.5`, existing owner client and `write_file`.
**Status:** Implementation, review/fix wave, local gates and real repaired crash acceptance passed on g9000/companion-schedule-recovery-m21. M0–M20 baseline `5710e6b805ac7c83f3517e345d00fd58ee44426c`; publication identity and exact resume are in the milestone ledger.

## Scope and boundaries

- One workspace `once` occurrence, one new synthetic checkpoint, one actual provider task; its tool continuation is part of that task.
- No manual rerun or automatic retry. One initial live task; a concrete native fix may justify one explicitly bound fresh-path acceptance task after regressions pass. No connectors, accounts, public deployment or browser workaround.
- No dependencies/features, new target directories, worktrees, stash/reset/restore/path checkout, database servers, containers, or port 55432.
- Preserve unrelated `nx.json`, all existing private data, old lead settings, and prior M16–M20 evidence; never stage profile/log/credential contents.
- Planner produced the lean draft; root maintains this tracked plan and sanitized outcome. Raw synthetic stores, scripts and logs remain ignored.
- Root uses saved owner authority without printing credentials, reads only this acceptance's synthetic content, and keeps the web service off. Browser closure remains unverified; no browser-policy workaround.

## Targeted source evidence and reuse

- `hosts/rust-daemon/src/schedules.rs:42,395–408,596–675`: 250 ms tick; unresolved nonactive claim reconciled before admission; failure disables schedule durably, rolling back on save failure.
- `hosts/rust-daemon/src/schedules.rs:1606–1640,1665–1702,1748–1845`: unfinished claim disabled/no replay; completed/unclaimed preserved; failed persistence closes admission; orphan recovery avoids provider reexecution.
- `hosts/rust-daemon/src/runs/ledger.rs:476–490,916–936`: Running/AwaitingApproval restore to Interrupted with `restart_during_run` and warning that started tools may have had effects.
- `hosts/rust-daemon/src/runs/ledger.rs:1139–1189` and `hosts/rust-daemon/src/agent_runs/tests.rs:2562–2615`: deterministic interrupted-ledger/tool preservation coverage; this does not prove live transcript durability.
- `hosts/rust-daemon/src/agent_runs.rs:1645–1701,1738–1846`: runtime owns execution transcript; run delta enters state and history only after final execution/commit/save.
- `hosts/rust-daemon/src/live/observer.rs:232–260`: successful `tool.finished` is live evidence; `history/outbox.rs:1–16,35` mirrors committed records about once per second.
- `hosts/rust-daemon/src/routes/contracts/{schedules,runs,sessions,status}.rs`: exact response names, failure spelling, tool metadata, and `storage.history.pendingFlush`.
- Reuse ignored M19 `plan-draft.md`, `operator.ps1`, and launch template concept; M19 quiet `Stop-OwnedDaemon` is cleanup-only and rejects the intended Running crash.
- Native scheduler already implements recovery; consult/copy upstream only for a concrete demonstrated gap, never as a separate audit or new scheduler.

## Normal owner interfaces

| Purpose              | Operation / required fields                                                                          |
| -------------------- | ---------------------------------------------------------------------------------------------------- |
| Ready/status         | `GET /ready`, `GET /api/status`; record running/queued, history flush count/errors                   |
| Preview              | `POST /api/schedules/preview`, `{trigger:{type:"once",atMs:DUE_MS}}`                                 |
| Create               | `POST /api/agents/AGENT_ID/schedules`, unique `importIdempotencyKey`                                 |
| Inspect/disable      | `GET /api/agents/AGENT_ID/schedules`; `PATCH .../SCHEDULE_ID`, `{enabled:false}`                     |
| History              | `GET .../schedules/SCHEDULE_ID/history?limit=5`; `runs[]` links run/session, `manual`, outcome/error |
| Ledger               | `GET /api/agents/AGENT_ID/runs/RUN_ID`; `run.status`, `source`, `sourceRef`, `error`                 |
| Session evidence     | `GET /api/agents/AGENT_ID/sessions/ENCODED_SESSION_ID/runs` and `.../messages`                       |
| Live tool witness    | `GET /api/agents/AGENT_ID/events`; existing owner SSE stream, correlated by run/tool-call ID         |
| Failure cleanup only | `POST /api/agents/AGENT_ID/runs/RUN_ID/stop`; never use for intended crash injection                 |

Schedule bodies have no token/deadline/iteration fields; reuse private lead settings instead. History outcome `failed` maps to schedule `lastOutcome.status:"error"`; run Interrupted serializes as `"interrupted"`.

## Five risks requiring root's binding rulings

1. **Crash ownership/race:** Capture exact daemon PID, image, start time and launch/profile identity. Immediately revalidate identity and Running before killing only that PID; no graceful run Stop or quiet helper for injection. Completion first is INCONCLUSIVE, never PASS or a repeat call.
2. **Transcript durability gap:** Source commits transcript only after execution finishes. `pendingFlush:0` while Running can mean nothing was queued. Captured owner-SSE successful `tool.finished`, direct file effect, and restored native successful tool message/journal witness are distinct. Postrestart needs preserved native success witness for strict PASS; a missing witness is an observed concrete gap, while interruption/no-replay can pass separately. Plan a minimal native journal correction only after actual failure and root scope ruling, never before the live observation.
3. **Bounds/usage:** Private lead `settings.max_tool_iterations=4`, `settings.timeout_ms=20000`; controller deadline due +60 s. Observe 12,000 aggregate reported tokens, stopping on surfaced breach; subscription transport may not enforce a hard token ceiling. Hardkill partial usage can be unknown, even if recovered ledger says zero.
4. **Effect nonduplication:** One new path and exact marker; byte/hash equality alone cannot detect an identical rewrite. Compare hash, modification time, successful write tool-call/message IDs/count and acceptance model-request/step evidence; no appends, helpers, shell, external tools, or hidden rerun.
5. **Isolation/cleanup:** Bind retained M16 private profile, stores, workspace, lead `agent-1791779202818-0`, and `gpt-5.5`. Restore old settings only while stopped; preserve old schedules and evidence. No automated recovery claim extends to recurrence, external delivery, remote uptime or exactly-once actions.

## Task 1: prepare stopped private acceptance (root)

- [ ] Verify current merged revision/binary, exact private launch/store paths, free owned listener, no acceptance work active, and preserved unrelated edits.
- [ ] Save old private lead settings while stopped; set four tool iterations/20 s timeout without exposing settings/credentials in evidence.
- [ ] Bind new destination `m21/checkpoint.txt`, exact ASCII bytes `m21 recovery checkpoint amber-742` with no newline; prove absent. If present, stop and bind a fresh path before arming, preserving old bytes.
- [ ] Prepare capture before due: exact process identity, owner status/list/history/run/messages, owner `GET /api/agents/AGENT_ID/events` SSE via existing curl client into ignored evidence, live `tool.finished`/model-step IDs, file hash/mtime. Record each owned curl PID/image/start time for exact cleanup; no preparatory model call.
- [ ] Record source-backed transcript limitation above before arming; do not add a persistence endpoint or mutate unrelated records to force a save.

## Task 2: arm one actual scheduled write and catch crash boundary (root)

- [ ] Prompt: “Local synthetic acceptance. First call write_file exactly once with file_path m21/checkpoint.txt and content m21 recovery checkpoint amber-742, without newline. After success, write about 1200 words of harmless original description of geometric patterns in this reply, without more tools. No appends, retries, delegation, schedules, shell, connectors, or other files. Stop on tool failure.”
- [ ] Long output creates only an observation window; root stops immediately after verified checkpoint, never waits for its length or adds a fake delay tool.
- [ ] Preview future due at least 15 s ahead, then create once: `{name:LABEL,prompt:PROMPT,trigger:{type:"once",atMs:DUE_MS},target:{type:"workspace"},enabled:true,importIdempotencyKey:LABEL}` with unique `m21-once-<nonce>`.
- [ ] Save exact schedule ID/due; ambiguous creation uses list recovery with same key, never another key. No manual-run/chat request.
- [ ] Poll bounded subsecond/short chunks from due; require matching scheduled run Running, captured owner-SSE successful exact `write_file` event, actual marker/hash/mtime, and `history.pendingFlush:0` with no flush errors before injection. SSE success plus actual file is the precrash proof; flush0 is health evidence only.
- [ ] Separately inspect matching session messages for successful write/tool-call metadata; record persisted-message count, including zero if not committed. Do not count live registry entries as durable messages.
- [ ] Revalidate exact recorded daemon identity and matching run Running; `Stop-Process -Id $M21DaemonPid -ErrorAction Stop` only on that identity. Record last Running observation and kill time; absence of terminal success after restart resolves the remaining race.
- [ ] Verify that PID exited and its listener released; update owned wrapper state using recorded identity only. Do not kill by name/port or widen ownership.
- [ ] If completion wins, do not kill an unrelated/finished run to manufacture interruption; disable if needed, retain evidence and exit INCONCLUSIVE. Timeout/provider failure gets exact-run Stop solely for cleanup, no retry.

## Task 3: restart identical stores and prove reconciliation (root)

- [ ] Restart recorded private daemon/profile/binary; readiness plus at most the existing bounded recovery wait, no store reset/settings substitution.
- [ ] Require same schedule ID, `enabled:false`, `running:false`, `lastOutcome.status:"error"`, `errorCode:"schedule_run_interrupted"`, counters one run/one failure, and no next claim.
- [ ] Require exactly one history occurrence, `manual:false`, `outcome:"failed"`, matching error and original non-null run/session links.
- [ ] Follow that ledger: `status:"interrupted"`, `source:"schedule"`, matching `sourceRef`, `error.code:"restart_during_run"`, effects warning, no saved owner Stop, no success reply claim.
- [ ] Compare checkpoint exact bytes/hash/mtime to prekill; compare successful write evidence/message IDs/count separately. Do not require `toolsStarted` to survive or report zero ledger usage as zero provider consumption.
- [ ] Require restored native successful write message/journal witness once, linked to captured SSE tool-call/run IDs; compare prekill persisted-message count honestly. Missing native success witness is an actual transcript gap, never replace it with controller's retained SSE file to claim native durability. File-effect/interruption/no-replay remain separately reportable.
- [ ] After ready/reconciliation, observe at least 5 s (20 ticks): unchanged occurrence/run count, disabled/nonrunning schedule, no acceptance model request/tool effect/queued run or replay.

## Task 4: quiet second restart and stopped cleanup (root)

- [ ] Confirm no running/queued acceptance work, no connectors and `pendingFlush:0`; stop only recorded owned daemon/wrapper identities using the quiet cleanup helper.
- [ ] Restore old private lead settings while stopped; restart same stores once more and repeat ready +5 s observation and schedule/history/ledger/file/message comparisons.
- [ ] Require same one occurrence/link, interrupted ledger/error, unchanged checkpoint hash/mtime, no new write/model request, and disabled schedule. Preserve disabled schedule and synthetic evidence.
- [ ] Leave all owned acceptance processes stopped, including each recorded curl SSE PID revalidated by image/start time; verify identities gone/listener free and retain private artifacts without deletion. Restart failure leaves daemon stopped and reports unresolved evidence.

## Gates and final claims

- No source change: root writes only sanitized acceptance/operator limitations to established companion docs/plan/handover; run `bun x prettier --check <exact changed Markdown files>` and `git diff --check`, then normal reviewed docs-only/unsigned merge gates.
- Concrete regression fix only: minimal failing regression, relevant fresh test, then required `bun x nx run rust-daemon:test --skipNxCache`; root serializes Cargo with `CARGO_INCREMENTAL=0`, no new target directory under this scope. A locked binary is stopped first; changing scope needs a ruling.
- Existing tests listed above are source evidence, not newly executed verification. No full Rust gate merely for unchanged-code acceptance.
- PASS requires every strict recovery/effect/message/no-replay criterion; missing persisted tool message is PARTIAL, completion-before-crash INCONCLUSIVE, wrong recovery or repeated effect FAIL. Unknown hardkill usage is explicitly unknown.
- Final evidence states revision/model, IDs, due/kill/restart times, observed live effect vs persisted messages, hash/mtime, recovery codes, quiet windows, settings restoration and owned cleanup; no production/exactly-once or enforced-token-cap claim.

## Controller rulings on the risks (binding)

1. Ruling: Crash only the recorded exact daemon executable/UTC-start-tick PID after a last matching Running observation, never the run-stop API — kernel timing leaves a race that restored Interrupted state must resolve — cost if wrong: record completion-first as inconclusive, preserve effects and do not manufacture interruption.
2. Ruling: Live SSE success, actual file bytes and native persisted execution evidence are separate checks; pendingFlush0 is only health evidence — final-only transcript commits may lose interrupted tool receipts — cost if wrong: record the concrete missing-witness gap and scope a regression-first native persistence fix before claiming recovery acceptance.
3. Ruling: Four iterations/twenty-second private timeout and sixty-second controller bound from due; stop immediately after one observed checkpoint — hard-kill token usage may be unknown, so reported zero is not zero consumption — cost if wrong: record partial usage and stop without a budget-enforcement claim or routine retries.
4. Ruling: One new synthetic file; compare exact bytes/hash/mtime and captured tool-call/run identity through recovery and a quiet second restart — file equality alone does not prove no repeat — cost if wrong: preserve evidence, keep the schedule disabled and fix only the observed replay/persistence defect.
5. Ruling: Reuse only retained synthetic stores, keep web/connectors off, restore two private settings while stopped, and clean up exact recorded daemon/wrapper/curl identities — isolated testing does not authorize normal profile/account startup — cost if wrong: leave owned services stopped and report uncertainty without touching unowned state.

Activation: root accepts the lean plan under continuing harness/restart acceptance
authority. No separate preflight audit or approval pause. If a native receipt gap
is observed, root binds the minimal interfaces and tests in this plan/ledger;
workers perform bounded source repair, one combined review/fix wave and required
fresh local gates. One new uniquely labelled/path-bound post-fix live acceptance
is allowed only after the initial failure and regression verification, preserving
the initial artifact and disabled schedule. No retroactive fabricated receipt.

## Observed native witness gap and accepted repair

# M21 Native Action Witness Repair Plan

> For implementation: root owns five binding rulings, RED evidence, combined review/fix wave, gates, services and Git. This is a bounded source-backed draft, not implementation authorization beyond root's existing scope.

**Goal:** Preserve a native owner-visible tool action witness before the final model reply, so an interrupted scheduled run reports what it actually attempted and what the handler actually returned.
**Architecture:** Add bounded action receipts to the existing daemon run ledger and owner Run DTO. Await an intent save before native handler dispatch and await its outcome save before returning a tool result to the runtime. Keep the final transcript commit and history outbox unchanged.
**Stack:** Existing Rust/serde/Tokio/control-plane store; no dependency, migration, new Cargo feature, service, target directory or scheduler redesign.
**Baseline:** `5710e6b805ac7c83f3517e345d00fd58ee44426c`, branch `g9000/companion-schedule-recovery-m21`; preserve unrelated `nx.json` and private evidence.

## Demonstrated gap and actual contracts

- Initial real scheduled run `run_ed50e6f4-3be6-494c-976a-02b5ab4eba61` executed `write_file`, call `call_3a8e86fc55914f0d9cc654d0323f4cec`; owner SSE reported success and the exact 33-byte artifact existed before the Running crash.
- Restart correctly disabled the schedule and restored Interrupted/error/effects warning, but native messages and `toolsStarted` were zero. Preserve this original PARTIAL evidence; never backfill its witness.
- `agent_runs.rs:1647-1667,1741-1846` commits isolated runtime delta/messages only after execution returns and the control-plane save succeeds. `pendingFlush:0` while Running does not prove message persistence.
- `tools.rs:188-237` is the actual native execution boundary: `ToolExecutionContext::execute_tool`, registry lookup, live checks, approval, protected-target revalidation, then awaited handler. There is no existing ToolExecutor trait to extend.
- `anima-core/src/runtime.rs:759-891` already has optional DatabaseAdapter pending/result steps, but failures merely emit events and execution continues; prepared results can be replayed. Do not repurpose these hooks into receipt recovery or enable replay.
- Core executes one model batch concurrently with `join_all` (`runtime.rs:798-850`). A receipt failure must close admission for sibling calls that have not dispatched; already-dispatched effects cannot be undone.
- `live/observer.rs:233-260` emits success after the executor returns. Preserve this synchronous live observer; no asynchronous save spawned from RunObserver.
- `state.rs:1678-1706,1889-1898,2005-2010` provides ordered snapshot saves and merges live tool names; `agent_runs.rs:680-690` provides the shared control-plane transaction.
- `runs/ledger.rs:214-267,476-490,877-936` stores/restores whole RunRecords; `routes/contracts/runs.rs:40-109` provides the owner DTO.
- `routes/sessions.rs:454-533` reserves the room, serializes deletion/save/rollback/history queueing; `history/outbox.rs:534-574` mirrors terminal whole RunRecords. Preserve that protocol.
- Existing bounds: `MAX_RUN_TOOLS_STARTED=50`, `MAX_LIVE_TOOL_CARDS=50`, `MAX_PREVIEW_BYTES=2048`; reuse UTF-8-safe `live::preview` behavior.
- Native contracts already suffice. No Hermes source copying is needed for this repair; if root finds a concrete reusable behavior, inspect only clean tracked public files with `git show HEAD:<path>` and preserve MIT attribution for adapted code. Never inspect/import/run private profiles or configuration.

## Five root rulings to bind before source implementation

1. **Evidence semantics:** A receipt proves native handler admission/return, not terminal chat commit or exactly-once effect. A saved intent without saved outcome is explicitly unknown; handler error can still have partial effects.
2. **Authority and concurrency:** All receipt updates use the existing control-plane transaction, then state lock, then release state lock for save while retaining transaction. Revalidate caller/run and permissions after waits; no worker dispatches tools or resurrects deleted records.
3. **Failure/cancellation:** Intent save failure denies the effect; outcome save failure rolls outcome back to intent, closes further admission and stops model continuation. Save/rollback is cancellation-safe, shared across parallel calls, and preserves genuine owner-stop precedence.
4. **Bounds and visibility:** Retain at most 50 calls per run, with 2048-byte argument/result previews and truncation flags. Reject a new call before effect at capacity rather than silently executing without witness. Include the owner API and thin SDK/current interrupted-outcome rendering; no new screen, endpoint or retry buttons.
5. **Acceptance and cleanup:** Regression-first repair, one combined daemon plus final SDK/web review if changed, one fix wave, fresh required gates, then one newly labelled/path-bound real acceptance under existing limits. Preserve initial disabled schedule/artifact, no historical fabrication/replay/retries; root owns exact-process cleanup/settings restoration.

## Minimal persisted shape and interfaces

- Add `ActionReceipt` and `ActionStatus` in `runs/ledger.rs` (or a tiny `runs/action_receipt.rs` if it improves focus), re-export through `runs/mod.rs`.
- Fields: `toolCallId`, `name`, `status: started|succeeded|failed`, `startedAtMs`, optional `finishedAtMs`, optional `durationMs`, `argumentsPreview`, `argumentsTruncated`, optional `resultPreview`, `resultTruncated`.
- Store `RunRecord.action_receipts: Vec<ActionReceipt>` with `#[serde(default, skip_serializing_if="Vec::is_empty")]`; no store version bump or migration solely for this additive defaulted field.
- Success requires `TaskResult.status` success AND no error; never infer it from an empty error alone. Failed is an observed handler failure, not a statement that no effect occurred.
- `started` means dispatch was authorized/prepared; after restart its outcome is unknown, even if a file exists. Do not silently promote it or replay it.
- Call IDs must be nonempty and unique within this run; duplicate/reused IDs must not dispatch a second action. Receipts are correlation records, not idempotency keys or replay instructions.
- Deny before dispatch at capacity; retain all admitted receipts and update existing rows in place. Validate bounds, IDs, names, timestamps/status consistency and preview byte lengths during restore.
- Add `ActionReceiptResponse` and `RunResponse.action_receipts` in `routes/contracts/runs.rs`; owner run detail/list and lifecycle serialization use the same DTO conversion, preserving existing authorization/no-store cache headers.
- No new endpoint, global journal, credential scope, raw full arguments/results, generic checkpoint framework or durable intermediate chat messages.
- Add a small `agent_runs/action_receipts.rs`: `RunActionJournal { weak_state, transactions, RunLink, cancel, shared_failure }`, with `async begin(call) -> Result<ReceiptHandle, JournalError>` and `async finish(handle, result, duration) -> Result<(), JournalError>`.
- `ReceiptHandle` binds exact run/agent/session/call and expected started row. `begin`/`finish` never create a missing agent/session/run and never alter terminal status or source state.
- `weak_state` is `Weak<RwLock<DaemonState>>`; transaction/failure/control handles must not create a new DaemonState/coordinator ownership cycle. Upgrade only inside the bounded operation and drop after save/rollback.
- Add optional `ToolExecutionContext.action_journal` and `with_action_journal(...)`; install only on the admitted native owner run path at `agent_runs.rs:1623-1644`. Direct/swarm tool contexts keep their current behavior.
- A configured durable store is required to call a receipt durable; `save(None)` returning Ok is not durable evidence. Preserve embedding/no-store behavior and label its receipts ephemeral through existing storage status; root binds this compatibility choice explicitly.

## Executor sequence and authority

1. Keep existing lookup, live checks, exact protected-target approval and approval refusal behavior before attempting an intent; denied/unknown tools are not claimed as started actions.
2. Capture call identity and approved-call evidence; call awaited `journal.begin`. Under transaction/state lock verify existing live agent/session/run link, Running state, no owner stop/cancel or shared journal failure, identity uniqueness, capacity and applicable caller authority.
3. Mutate only this receipt row plus `note_tool_started`, capture narrow undo, snapshot, release state lock and await save while holding transaction. Announce nothing durable before save succeeds.
4. Roll back a failed intent save while transaction is retained; latch journal failure and cancel continuation before any next dispatch. Return a persistence error, never run handler.
5. After the await, recheck shared failure/cancel, live run/caller existence, execution deadline/expiry and applicable configured/parent/peer tool permission. Recheck protected target and current policy deny; preserve the exact approval already obtained rather than reopening or converting it into a reusable grant. An approval's existing `expiresAtMs` governs its pending decision window; do not invent expiry for already-settled exact authorization.
6. The added post-save wait requires a small approval-gate revalidation helper carrying the configured-tool baseline and exact approved call. Match current `gate.rs` semantics for synthetic peer/team tools; do not deny solely because frozen augmented tools are absent from canonical configuration.
7. If revalidation refuses, handler is not invoked; settle a failed/refused receipt if storage remains healthy. A surviving started receipt remains conservative, never falsely successful.
8. Explicitly release the transaction before dispatching the existing registry handler once; never hold it across effect execution, which may itself acquire the transaction. Record actual elapsed duration and returned TaskResult; no automatic retry on handler or persistence error.
9. Await `journal.finish`: reacquire transaction/state lock; verify the same row/link and that no deletion/tombstone superseded it. Record result previews/status, capture narrow undo, release state lock and save while holding transaction.
10. Outcome-save failure restores the prior started row; latch failure and cancel continuation, then return error rather than original success. Already-effected work stays unknown in durable storage; later final commit must not fabricate a successful receipt.
11. Owner stop after handler dispatch may still receive the actual returned action outcome; preserve run Cancelled/stop semantics. Missing/deleted run refuses outcome publication; never insert it again or enqueue independent history work.
12. A dropped executor/timeout must not strand a dirty success row. Use a bounded owned save/rollback operation awaited by the caller: cancellation may let that data-only operation settle, but it must never execute effects. Its failure closes the shared run gate even if caller is gone.
13. Final coordinator outcome reads shared failure and treats receipt infrastructure failure as failed, while preserving genuine owner-stop priority. Cancel signal stops additional model calls; normal tool errors continue under current runtime rules only when their failed receipt was saved.
14. Cancellation/revalidation after intent save and before dispatch is unavoidable: the durable started intent can overstate an attempted effect, which is why its semantics stay unknown. Revocation after the last check follows existing tool boundaries; this repair does not claim atomic external effects.

## Task 1: RED native regression and contract tests

**Files:** create `hosts/rust-daemon/src/agent_runs/action_receipt_tests.rs`; register module in `agent_runs.rs`; add focused `runs/ledger.rs`/DTO tests.

- [ ] Use deterministic/scripted adapter: return one actual local `write_file`, then hold the next model reply with an existing test barrier; no live provider/services.
- [ ] Before releasing the reply, load the configured temporary control-plane store and assert Running run has call-linked succeeded receipt, exact tool name/result preview, `toolsStarted` and file bytes, while committed session messages/replyMessageId remain absent.
- [ ] Restore saved state, reconcile schedules, and assert Interrupted plus same receipt and disabled schedule; release/quiet second restore has no new handler invocation. Preimplementation failure must be missing receipt, not unrelated compilation/runtime failure.
- [ ] DTO/default tests: legacy snapshot without field loads; owner run detail exposes receipts; nonowner request denied; preview UTF-8 truncation/50 capacity; duplicate IDs do not cause a second effect; malformed snapshot rejected without partial mutation.
- [ ] Existing history terminal-record mirroring retains receipt JSON after ledger pruning; actual session/agent deletion removes it, and failed deletion restores it.
- [ ] Root captures RED evidence with the existing Nx Rust test target; avoid adding a fake test-only persistence shortcut.

## Task 2: implement ledger/journal/executor minimally

**Files:** `runs/ledger.rs`, `runs/mod.rs`, `routes/contracts/runs.rs`, new `agent_runs/action_receipts.rs`, `agent_runs.rs`, `tools.rs`; tiny `approvals/gate.rs` helper only for required postwait authority.

- [ ] Implement defaulted shape/bounds/DTO, then journal transaction/save/rollback and weak ownership, then executor integration and final infrastructure-error outcome handling.
- [ ] Reuse `live::preview`; preserve original ToolCall/TaskResult and normal tool/result transcript generation after successful journal settlement.
- [ ] Keep `commit_run`, `rollback_run`, source commit callback, schedule/job CAS, pending steers, terminal history queue/deletion order, approval consumption and protected-file semantics unchanged.
- [ ] Ensure final commit/rollback starts from current RunRecord and preserves independently saved action receipts rather than replacing them from frozen start state.
- [ ] Do not change synchronous RunObserver persistence or add partial `MessageRole::Tool` into canonical transcript/outbox.

## Task 3: focused concurrency/failure regressions

**Files:** same new test module, plus existing `agent_runs/approval_stop_tests.rs`, `history/outbox.rs` tests only where existing fixtures fit.

- [ ] Inject begin save failure using `install_test_control_plane_save_gate`: handler count zero, no success receipt, no next model request; old row state restored.
- [ ] Inject finish save failure after a real effect: file exists, durable started remains, returned tool is not success, continuation stopped, later final save/restore never manufactures success.
- [ ] Parallel batch barriers: one save failure closes other not-yet-dispatched calls; independently dispatched sibling effects are honestly represented, receipts neither overwrite nor disappear and only one row per call survives.
- [ ] Revoke configured/parent/peer permission, owner stop, execution deadline/expiry or protected target during intent-save wait; after release no handler effect. Approved exact call revalidation neither duplicates an approval prompt nor loses one-time authority.
- [ ] Drop executor during begin/finish save; release gate and prove save/rollback settles, no dirty successful row leaks into a later snapshot, no new effect and transaction is released.
- [ ] Race agent deletion with receipt save under shared transaction; durable deletion wins afterward without resurrected receipt/history row. Failed deletion rollback preserves saved receipt.
- [ ] Commit reject/final persistence failure retains prior saved receipts while rolling back only final transcript; owner stop remains Cancelled with honest action witness.
- [ ] No-store embedding compatibility is explicit; configured-store failure is fail closed. Receipt handles do not keep state alive after owner teardown.

## Task 4: visibility, guidance and final gates

- [ ] Existing owner Run API exposes saved receipts; update established companion recovery/operator guidance to distinguish action witness from committed chat, success from overall run success, started/failed from absence of effects, and no replay.
- [ ] After daemon contract is frozen, add only optional `Run.actionReceipts?: ActionReceipt[]` in `packages/sdk/src/runs.ts` and a compact receipt list in `apps/web/src/components/sessions/RunOutcomeCard.tsx`; existing interrupted card currently shows only tool names. Render as text with truncation indication; no new actions/retry buttons or transcript injection. Re-export the receipt type through the existing SDK barrel only if needed.
- [ ] Thin UI test in existing `apps/web/src/components/sessions/RunActivity.test.tsx` covers succeeded/failed/unknown statuses and legacy absent array; current run status, effects warning and scheduled no-retry behavior remain intact. Include final SDK/web files with daemon in the single combined review.
- [ ] Inspect current Nx resolved project names/targets before gates; use `bun x nx show projects --json` and exact `bun x nx show project <name> --json` for changed SDK/web targets.
- [ ] Root runs serial Cargo validation with existing directory and `CARGO_INCREMENTAL=0`: `bun x nx run rust-daemon:test --skipNxCache`, then `bun x nx run rust-daemon:lint --skipNxCache`; stop locked owned daemon first under current scope.
- [ ] If SDK/web changed, run only their actual resolved relevant test/build/typecheck targets; format exact changed files and `git diff --check`. One combined review/fix wave, repeat checks only for changed/failing concerns.
- [ ] After deterministic GREEN and root's reviewed gates, one new scoped actual-provider scheduled-write/crash acceptance must observe successful tool SSE only after durable receipt, restore the same linked succeeded receipt via owner API while Interrupted, and prove disabled schedule/no replay across two restarts.
- [ ] Capture new IDs/path independently; preserve old PARTIAL evidence and artifact; keep limits/settings restoration/exact-process cleanup from the existing M21 plan. No repeat acceptance if completion wins without a new explicit root ruling.

## Remaining crash window and honest final claim

An effect and its control-plane receipt are separate transactions. A crash after the handler causes an effect but before its outcome is durably saved leaves only started/unknown; a crash after saved intent but before dispatch can leave the same unknown. This is deliberately visible and never retried automatically.

After saved outcome, the interrupted run can retain actual native success/failure evidence without a final model reply or committed chat message. Owner SSE success now follows the awaited receipt save; it is still no exactly-once guarantee, atomic effect rollback, provider budget guarantee, durable full intermediate transcript, or production-availability claim.

## Controller rulings on the repair risks (binding)

1. Ruling: Store bounded native admission/outcome receipts on existing runs, separate from final chat history — a started intent is unknown, handler failure may still have effects and success is not whole-run success — cost if wrong: retain conservative unknown evidence and require effect review, never promote or replay it.
2. Ruling: Await intent and outcome durability under the existing serialized control-plane transaction, release it before effect dispatch, and revalidate existing authority/protected target/deadline after waits — parallel calls and deletions must not overwrite or resurrect receipt rows — cost if wrong: deny further dispatch and preserve independent admitted effects without a broader concurrency redesign.
3. Ruling: Use cancellation-safe data-only save/rollback and one shared fail-closed latch/cancel/final-status bridge; intent-save failure means no dispatch, outcome-save failure retains started and aborts continuation — runtime errors must not turn an unrecorded effect into success or a retry — cost if wrong: mark infrastructure failure, retain the effects warning and stop rather than fabricate durability. Genuine owner-stop takes precedence.
4. Ruling: Limit fifty unique calls per run, existing UTF-8-safe 2048-byte argument/result previews and truncation indicators; expose thin SDK/interrupted-card evidence without new retry actions — owners need bounded visible receipts, not a second transcript or an unbounded journal — cost if wrong: refuse before the unrecorded effect; previews remain partial evidence. A no-store embedded runtime remains supported with explicitly memory-only receipt durability; configured-store saves are fail-closed and never mislabeled durable.
5. Ruling: Initial tests exercise actual executor/save/restore/owner JSON APIs to prove absent receipts before implementation, then cover parallel failure, cancellation, revocation/deletion and final rollback; one combined daemon/final web-SDK review and relevant fresh gates precede one new path-bound live retest — the initial real failure justifies the repair and retest cost — cost if wrong: preserve initial failed evidence, stop owned services and report the remaining concrete gap. No retroactive backfill or automatic retries.

Repair activation: the initial real source5710e6b probe confirmed zero native
receipts despite a successful file effect. Root accepts this minimal native
ledger/executor repair and thin owner visibility. Additive defaulted run fields
need no store-version bump solely for this addition; reject malformed receipt
payloads on restore without mutating state. Terminal receipt updates must preserve
current whole-run status and mirror the durable run through existing history
ordering when needed; agent/session deletion retains the shared transaction and
never resurrects records. New public metadata must clearly distinguish durable
configured-store receipts from memory-only embedding mode. Product stays Anima;
no upstream runtime/profile/secret bridge or new service/model dependency.
Ruling: Prepare a candidate persisted snapshot, keep the prior receipt public until save acknowledgement, then publish only the receipt/tool-name patch — owner reads do not take the control-plane transaction and a provisional succeeded row would falsely claim durability — cost if wrong: never publish success, retain started/unknown and stop continuation; test reads during the gated outcome save and after failure/cancellation. No global state write lock across I/O; stop/status/independent fields stay current.
Ruling: Retain acknowledged action-only patches in the existing ordered persistence state and merge them into existing run rows before later snapshots are written — newer ordinary settings snapshots can be prepared before public receipt publication without taking the journal transaction — cost if wrong: report no durable success, stop continuation and preserve unknown evidence; never resurrect absent rows, replace whole-run fields or modify every writer route. A deterministic gated stale-snapshot race regression must fail before the fix and pass afterward.
Ruling: Bound lock admission but let owned data-only storage settlement retain ordering through actual acknowledgement — synchronous file I/O and asynchronous database settlement cannot safely be abandoned to a later writer — cost if wrong: infrastructure work may wait beyond the admission deadline; no hard I/O or billing cap is claimed, and no further effects may start.
Ruling: An individually approved call may proceed while a sibling keeps its run AwaitingApproval; all own authority/stop checks remain. Existing helper-stop tests must gate original helper-creation save after new intent, observer tests separate stream-start/text independence from durable tool completion, and independent scripted calls use distinct IDs — preserve meaningful original coverage under receipt semantics — cost if wrong: required focused/full gates fail before publication.

## Verified outcome (2026-10-12)

- [x] Initial real crash reproduced the native evidence gap while preserving the actual file effect, interrupted identity and disabled schedule without replay. That original artifact/schedule remains unchanged; no historical receipt was fabricated.
- [x] Additive native receipt/owner DTO/SDK shape, UTF-8-safe bounds, weak journal ownership, intent-before-dispatch/outcome-before-continuation, public publication after acknowledgement and configured-store versus temporary labels are implemented.
- [x] Shared storage-failure gate denies undispatched parallel effects and model continuation. Data-only settlement survives caller cancellation; active storage keeps ordering through acknowledgement. Ordinary newer snapshots merge acknowledged action patches into existing rows only, preserve independent fields and retire acknowledged removed rows.
- [x] Native RED2 reproduced missing witness; additional P1 race RED reproduced a newer snapshot replacing success with started. Final focused12 GREEN and full required Rust2354passed/8ignored, Rust lint pass. The full gate also caught/corrected an individually approved call while its sibling awaited approval; the unchanged batch-approval regression now passes. Existing helper-stop/policy/stream tests retain meaningful original coverage with updated save barriers/independent call IDs.
- [x] One daemon batch review found the corrected P1 race; final web/SDK review found no actionable issues. Owner display focused40 GREEN; SDK130/web1191 and both test/typecheck/build targets pass. Owned formatting/local links/diff checked before publication.
- [x] Fresh actual gpt-5.5 once task wrote exactly39bytes to `m21/repaired-checkpoint.txt`: `m21 saved outcome checkpoint cobalt-913`, without newline. Schedule `schedule-1791796742170-1`, run `run_7ebf4146-6c79-41cb-8f58-94390d70c120`, call `call_U9ePcFtztuO6iMObeiDNCg4R`. Owner SSE success and the call-linked persistent succeeded receipt were observed while Running and before any final conversation messages; exact owned daemon PID30572 was hard-killed at2026-10-12T09:19:21.2367213Z.
- [x] First and second same-store restarts each remained quiet for20scheduler ticks: identical receipt fields/call identity, run Interrupted/restart_during_run/no owner Stop/no final reply/messages, schedule disabled/nonrunning/schedule_run_interrupted, exactly one failed/manualfalse occurrence linked to the original run/session. Actual checkpoint bytes/SHA256/mtime remained unchanged; no automatic replay.
- [x] Original two private settings restored to null/null before the second restart. All exact recorded daemon/wrapper/curl processes stopped, ports8080/4200free, connectors0. Quiet forced cleanup is not graceful-wrapper shutdown proof. No browser-policy workaround or live browser claim.

Targeted new tests cover actual write/save/restore, publication during gated I/O,
intent/outcome failure, parallel refusal, dropped outcome caller, permission
revocation, duplicate identities, legacy/malformed restore, temporary mode,
stale ordinary snapshots, failed removal, acknowledged removal, and final failed
coordinator/no next model. Broader approval/Stop/deletion/retention fixtures pass
in the full suite. Dedicated new receipt races for every protected/deadline,
deleted session/agent, history pruning and live PostgreSQL settlement permutation
were not added; no database service test was authorized. The live test proves the
configured JSON-store checkpoint, not production availability or all storage modes.

The effect-to-outcome-save crash window still yields started/unknown and requires
owner review. Final intermediate chat and partial provider usage are not made
durable by these receipts; hard-killed provider consumption is unknown. This adds
neither exactly-once effects, automatic replay nor a hard I/O/billing cap.
