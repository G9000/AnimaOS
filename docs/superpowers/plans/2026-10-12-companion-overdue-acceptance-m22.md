# M22 Overdue One-Shot Personal-Use Acceptance Plan

> **For agentic workers:** Use `superpowers:executing-plans` for bounded steps. Root owns binding rulings, branch, services, owner APIs, evidence, validation and Git.

**Goal:** Prove one enabled, never-claimed one-shot fires once after its due time passes with the isolated daemon off, commits its file/action/history, and stays unchanged through another quiet restart.
**Architecture:** Reuse the native scheduler's stored due time, owner APIs, M21 persistent action receipts and retained M16 stores. This is delayed execution of an unclaimed occurrence, followed by observed non-replay of completed work.
**Tech stack:** Existing Rust daemon, configured `gpt-5.5`, owner HTTP client, private control-plane/history/workspace, `write_file`.
**Status:** Real overdue acceptance and quiet completed restart passed, with no native source fix, on g9000/companion-overdue-acceptance-m22, cut from verified main `4d534dd6e941c75e8194317bcbd315683dce87f4` (M21 PR27/source9fea0d0). Root verified prior owned services stopped and binds the same existing binary/stores.

## Scope and authority

- Owner continuation and isolated synthetic local-test authority remain valid; root performs the normal reviewed/unsigned merge gates.
- Exactly one actual provider task/automatic once occurrence and one tiny new synthetic file. Tool continuation belongs to that same task.
- No manual run, chat send, second provider task, automatic retry, repeated acceptance, helper, shell tool, connector/account change or external write.
- No source changes unless root observes a concrete native regression and rules on the minimum correction; no preemptive journal/scheduler feature.
- No dependencies, new target directories, worktrees, stash/reset/restore/path checkout, database servers, containers, port 55432 or deployment.
- Preserve unrelated `nx.json`, existing private settings/stores/schedules, and M16–M21 artifacts. Never stage private profile/log/credential contents.
- Planner produced the ignored lean draft; root maintains this tracked plan and filtered evidence. Raw stores/scripts/logs remain ignored.
- Root uses the existing owner authority privately, disables ordinary environment/profile autoload, and keeps all test connectors disconnected.

## Existing behavior: catch-up is supported

- `hosts/rust-daemon/src/state.rs:872–882`: `restore_allows_an_overdue_schedule_next_due_time` accepts an overdue nonzero stored due time.
- `hosts/rust-daemon/src/state.rs:2225–2230,2590–2648`: restore preserves schedule records/due times; syntax/timing validation does not require stored due to remain future.
- `hosts/rust-daemon/src/schedules/timing.rs:193–226`: stored once needs positive `atMs`; CREATE/PREVIEW additionally require a future time through `next_fire_after`.
- `hosts/rust-daemon/src/schedules.rs:395–458`: startup ticks reconcile unresolved claims, then admit enabled `next_due_at_ms <= now` records that have no unresolved occurrence.
- `hosts/rust-daemon/src/schedules.rs:578–675`: no prior `last_fired` means no interrupted occurrence; a genuinely claimed unfinished run follows M21 disable/review policy instead.
- `hosts/rust-daemon/src/schedules.rs:682–769` and `schedules/timing.rs:357–388`: a once claim is saved before dispatch, retains original due time, records actual fired time and disables itself.
- `hosts/rust-daemon/src/schedules.rs:1665–1703`: `reconciliation_preserves_completed_and_never_claimed_schedules` leaves unclaimed records enabled.
- `hosts/rust-daemon/src/schedules/timing.rs:681–696`: once cannot have active hours; a late claim keeps its original due time.
- Reuse normal owner contracts in `routes/contracts/{schedules,runs,sessions,status}.rs`; M21 `runs/action_receipt.rs` gives bounded handler evidence separate from final chat commit.
- Reuse ignored M19/M21 owner/launch/operator templates, including exact PID/image/start-time ownership and quiet-only cleanup. Quiet force cleanup does not prove graceful wrapper shutdown.
- No source blocker found for this scenario. If current source changes to intentionally drop overdue unclaimed once work, root reports that before arming and does not manufacture a catch-up result.

## Interfaces and evidence fields

| Purpose               | Normal owner operation / relevant evidence                                                           |
| --------------------- | ---------------------------------------------------------------------------------------------------- |
| Readiness/status      | `GET /ready`, `GET /api/status`; running/queued, connectors, history `pendingFlush`/errors           |
| Future preview        | `POST /api/schedules/preview`, `{trigger:{type:"once",atMs:DUE_MS}}`; expect one due                 |
| Create once           | `POST /api/agents/AGENT_ID/schedules`; future once, workspace target, unique import key              |
| Inspect/disable       | `GET /api/agents/AGENT_ID/schedules`; `PATCH .../SCHEDULE_ID`, `{enabled:false}`                     |
| Occurrence history    | `GET .../schedules/SCHEDULE_ID/history?limit=5`; `manual`, outcome/error, run/session links          |
| Ledger                | `GET /api/agents/AGENT_ID/runs/RUN_ID`; `source`, `sourceRef`, status/model, `actionReceipts`, usage |
| Session               | `GET /api/agents/AGENT_ID/sessions/ENCODED_SESSION_ID/runs` and `.../messages`                       |
| Optional live capture | Existing owner `GET /api/agents/AGENT_ID/events`; correlate tool/model-step IDs                      |
| Failure cleanup       | `POST /api/agents/AGENT_ID/runs/RUN_ID/stop`; cleanup only, never substitute for automatic trigger   |

Root compares exact schedule ID rather than latest/global record; URL-encode session IDs. Schedule history uses `failed` while `lastOutcome.status` uses `error` for failures.
History may list only settled occurrences: an empty history alone never proves no claim. Require `lastFiredAtMs:null`, `running:false`, matching source-ref run absence and enabled persisted state.
Once `nextDueAtMs` remains the original due after firing even though disabled; do not require null/future nextDue or reinterpret retained past due as replay.
`actionReceipts` may be absent on older runs; the new M22 run must contain one `write_file` success with persistent durability. Receipt success reports the handler outcome, not an exactly-once guarantee.

## Five binding risks for root

1. **Stop/due race:** Choose future due with generous stop margin. Strictly prove daemon and recorded wrapper exited before due, schedule stayed enabled/unclaimed and no matching run/file existed. If any claim occurred before shutdown, classify INCONCLUSIVE for M22; do not disable/rearm or repeat to hide it.
2. **Identity and store continuity:** Pin main revision/binary, M16 private profile/control-plane/history/workspace, lead `agent-1791779202818-0`, and configured `gpt-5.5`. Restarts reuse every path; no reset/substitution or ordinary profile reads.
3. **Bounds/provider evidence:** Four tool iterations and 20,000 ms private lead timeout; controller at most 60 s from first postdue restart launch, including readiness/completion. One short reply; 12,000 aggregate reported-token observational ceiling. Stop on surfaced breach; no guaranteed hard provider token cap or complete usage inference from zero/unknown values.
4. **Delayed catch-up versus interrupted replay:** Enabled/unclaimed overdue work should execute once; disabled work and unfinished claimed work follow different policies. Capture postdue persisted unclaimed metadata before restart because it may claim before the first ready/API read. Failure to catch that already-fast claim in a GET is not loss of the prelaunch proof.
5. **Effects and cleanup:** Exact bytes/hash/mtime plus one native tool-call/receipt/message and one run/history occurrence detect repeated identical writes better than hash alone. Restore old settings only stopped; leave all owned daemon/wrapper/optional curl processes stopped, with no acceptance work armed.

## Task 1: bind private baseline and one harmless task

- [x] Root confirms the reported merged revision/binary, no owned acceptance processes, recorded private paths, listener free, no test connectors, and preserved unrelated edits.
- [x] Save old private lead settings while stopped; apply `settings.max_tool_iterations=4`, `settings.timeout_ms=20000` without printing private contents.
- [x] Bind new relative path `m22/overdue-proof.txt`, exact ASCII bytes `m22 overdue proof cobalt-529` without newline; prove absent. Existing bytes stop arming until root binds a fresh unique path, preserving them.
- [x] Start only the recorded isolated daemon and wait bounded readiness; save its exact PID/image/start time and wrapper identity. No browser or web service is necessary; browser closure remains unverified and no browser-policy workaround is allowed.
- [x] Bind unique label/import key `m22-once-<nonce>` and prompt: “Local synthetic acceptance. Call write_file exactly once with file_path m22/overdue-proof.txt and content m22 overdue proof cobalt-529, without trailing newline. Do not append, retry, delegate, create schedules, use shell/connectors or access other files. Return only M22_DONE after success; stop on failure.”
- [x] No preparatory inference or provider switch. Root may use existing safe provider/status metadata; configuration alone is not provider success evidence.

## Task 2: create future enabled schedule and stop strictly before due

- [x] Compute `DUE_MS = UtcNowMilliseconds + 45000`; preview must return exactly that future due. Require at least 30 s remain before create; otherwise recalculate before any schedule exists.
- [x] Create once: `{name:LABEL,prompt:PROMPT,trigger:{type:"once",atMs:DUE_MS},target:{type:"workspace"},enabled:true,importIdempotencyKey:LABEL}`; save ID/owner/trigger/target.
- [x] Ambiguous create result: list and recover same original import key/ID, never submit another key. Never create an already-overdue once or edit `atMs` to the past.
- [x] Before stop, require `enabled:true`, `running:false`, `lastFiredAtMs:null`, `lastOutcome:null`, zero counters/history and no ledger run with source schedule/ref ID; file still absent.
- [x] Require global private running/queued zero, connectors empty, `pendingFlush:0`, no history errors and at least 10 s left before due. Begin quiet exact-identity stop; do not PATCH disabled.
- [x] Verify exact daemon/wrapper exited and reserved listener free at timestamp strictly before due. If deadline is missed, inspect actual persisted claim state and record INCONCLUSIVE; no repeat acceptance.
- [x] While stopped, root reads only filtered acceptance schedule/run metadata from the recorded control-plane store: same ID/due, enabled, no `last_fired`, no outcome, counters zero, no matching run. Never output whole private stores.
- [x] Save sanitized before-stop/off evidence, including UTC samples, due, process-exit time and file absence. The schedule remains armed in its stopped store deliberately.

## Task 3: cross due fully off, then observe one automatic delayed completion

- [x] Keep exact processes absent and listener free until UTC is at least due +5 s; bounded small waits, no system-clock edits, sleep/suspend or drift assumptions.
- [x] Before launch at this postdue timestamp, recheck same stored enabled/unclaimed schedule/due, zero matching run and absent file. This is strict proof of an overdue unclaimed record; history absence is only supporting evidence.
- [x] Record first restart launch UTC as controller start and restart identical revision/binary/profile/stores. Do not call schedule run, send chat, reenable, edit trigger or force a tick.
- [x] Poll short bounded chunks and require one automatic fire/completion within launch +60 s. Capture actual fire/start/finish timestamps and delay from original due; readiness may arrive after claiming.
- [x] Require schedule disabled/nonrunning, counters runs 1/failures 0, `lastOutcome.status:"spoke"`, no error, original due unchanged, actual `lastFiredAtMs >= postdue launch`.
- [x] Require exactly one settled history occurrence with `manual:false`, `outcome:"spoke"`, original schedule ID and non-null matching run/session IDs; fired time is postdue and after the recorded off interval.
- [x] Follow ledger: completed, source schedule/ref ID, actual configured model/provider, no Stop/error, committed `replyMessageId`; session run/messages confirm the same task and one successful exact write call.
- [x] Require one native `write_file` receipt, `status:"succeeded"`, `durability:"persistent"`, exact call ID/argument/result preview and no truncation for this tiny task. Verify actual exact file bytes and save hash/mtime.
- [x] Wait for `pendingFlush:0` with no new flush error after completion. Root treats native receipt, committed message/history and file effect as separate correlated evidence; no success based only on assistant text.
- [x] Failure/timeout/unexpected action: disable exact schedule, Stop admitted exact run if present and settle before owned shutdown; preserve evidence and do not retry/live-call again unless root observes and scopes a concrete source gap.

## Task 4: completed-work quiet restart and final cleanup

- [x] Once completed and quiet, record full sanitized acceptance comparison baseline: IDs/counts/statuses, receipt/call/message IDs, due/fire times, bytes/hash/mtime and surfaced provider usage.
- [x] Stop only recorded owned daemon/wrapper/optional curl identities, restore old lead settings while stopped, then restart identical stores once more.
- [x] After ready, observe at least 5 s (20 native 250 ms ticks). Require same disabled/nonrunning schedule, one completed run/history/receipt/write message, identical file bytes/hash/mtime and no acceptance model request, queued run or new occurrence/effect.
- [x] Final quiet status: no running/queued acceptance work, no connectors, zero pending history flush and no new errors. Leave disabled schedule/history/artifact as evidence, no deletion/backfill.
- [x] Stop all recorded owned processes, including optional SSE curl clients verified by image/start time; verify exact identities gone and listener free. Restart failure leaves daemon stopped and reports unresolved evidence.

## Existing tests and bounded verification

- Source tests: `restore_allows_an_overdue_schedule_next_due_time`; `once_fires_once_and_refuses_active_hours`; `reconciliation_preserves_completed_and_never_claimed_schedules`; `restart_disables_unfinished_claim_without_replaying_it`; `failed_restart_reconciliation_keeps_all_admission_closed`.
- Owner contracts: `schedule_crud_is_agent_scoped_and_mutations_require_local_owner`, `openapi_registers_schedule_crud_and_import_contracts`; M21 receipt restore/order regressions remain inherited evidence.
- Root's baseline current merged gates: Rust 2354 passing/8 ignored, SDK 130 passing, web 1191 passing. This plan has not executed them; absent source changes, do not duplicate full gates.
- Docs-only PASS: root updates established companion plan/daily-use/handover with sanitized delay and non-replay results; `bun x prettier --check <exact changed Markdown files>` and `git diff --check`, then reviewed/unsigned docs merge gates.
- Concrete native fix only: minimal failing regression, focused fresh check and required `bun x nx run rust-daemon:test --skipNxCache`; root serializes Cargo with `CARGO_INCREMENTAL=0`, stops owned locked binary first, no new target directory within current scope.
- Use relevant existing Nx SDK/web checks only if their implementation changes; no new test/dependency merely to restate this docs-only observation. Upstream comparison only for an actual useful native gap.
- PASS requires strict pre-stop/postdue-unclaimed proof, one actual delayed provider success with committed persistent evidence, unchanged 20-tick restart and stopped/restored cleanup. Race/lost timing proof is INCONCLUSIVE; actual wrong behavior/effect is FAIL; missing evidence is PARTIAL.
- Final report distinguishes delayed one-shot execution from interrupted-action recovery, recurrent catch-up, clock/sleep behavior, always-on hosting and exactly-once external effects; claim only what this local observation proves.

## Controller rulings on the risks (binding)

1. Ruling: Create with45seconds future margin, stop with at least10seconds left and prove exact process exit before due plus persisted unclaimed state after due+5seconds — a pre-stop claim would test interruption instead of offline catch-up — cost if wrong: classify inconclusive, preserve evidence and do not rearm or hide the race with another task.
2. Ruling: Reuse the identical synthetic M16 stores/workspace, exact lead/model and verified M21 source/binary; one new M22 path/label — changing identity or storage could manufacture persistence — cost if wrong: stop owned testing and retain the existing stores without resets, imports or real profile/credential reads.
3. Ruling: One short real gpt-5.5 task, four iterations/twenty-second private timeout, launch-to-completion controller60seconds and reported-token12,000 observational ceiling — this tests a concrete cheap workflow rather than a sustained model loop — cost if wrong: disable/stop only the acceptance task and report the actual failure/usage uncertainty; do not claim a hard I/O/provider billing cap or retry.
4. Ruling: Capture overdue enabled/no-lastFired/no-run/no-file proof while completely stopped, then require one automatic manualfalse completed occurrence with original due time — delayed unclaimed once execution is distinct from replaying an interrupted action and disabled once retains a past due value — cost if wrong: classify wrong recovery or partial evidence, not a passing catch-up claim; no manual trigger/chat/reenable/clock edits.
5. Ruling: Correlate actual bytes/hash/mtime with one run/history/tool-call/receipt and committed write/reply, then verify20quiet ticks after restored-settings restart and exact owned cleanup — identical bytes alone cannot prove no repeated write — cost if wrong: preserve the artifact, leave automation disabled and report the concrete gap; no extra task/backfill or graceful-wrapper/production claim.

Root accepts this bounded personal-use acceptance under the continuing harness
authority. No separate audit, worktree, specification approval pause or provider
configuration change. A demonstrated native gap needs a minimum regression-first
repair ruling before source changes or a separately justified fresh retest. Existing
M21 gates remain evidence for unchanged source; this milestone only claims freshly
executed live acceptance and owned documentation checks when no code changes.

## Verified outcome (2026-10-12)

The unchanged native scheduler passed this short-outage once-task acceptance.
No source fix, upstream transplant, dependency or additional model task was needed.
The original future trigger was created through the owner API, never edited or
manually triggered.

| Observation                | Evidence                                                                                                                                                                                        |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Source/binary              | M21 merge4d534dd6e941c75e8194317bcbd315683dce87f4; runtime/SDK/web source unchanged                                                                                                             |
| Schedule/run               | schedule-1791799707663-1 / run_634be5a8-a312-4d6e-b151-d3aeb7669919                                                                                                                             |
| Original due               | 2026-10-12T10:09:12.649Z                                                                                                                                                                        |
| Exact processes stopped    | 2026-10-12T10:08:28.185Z,44,464ms before due                                                                                                                                                    |
| Off proof after due        | 2026-10-12T10:09:17.693Z: enabled, original due, no lastFired/outcome/matching run/fire/file                                                                                                    |
| First postdue launch       | 2026-10-12T10:09:19.5895447Z                                                                                                                                                                    |
| Automatic fire             | 2026-10-12T10:09:21.788Z:9,139ms late,2,199ms after launch, manualfalse                                                                                                                         |
| Actual completed run       | gpt-5.5/chatgpt,2steps,6,157 reported tokens; completed10:09:27.287Z within7,698ms of launch                                                                                                    |
| Effect and native evidence | exactly28bytes `m22 overdue proof cobalt-529` without newline at `m22/overdue-proof.txt`; one persistent succeeded write receipt and matching committed tool result; committed reply `M22_DONE` |
| Completed quiet restart    | same disabled/nonrunning schedule and retained original due, one spoke run/history/receipt/tool-result, identical messages and exact file bytes/SHA256/mtime after20ticks                       |
| Cleanup                    | original two settings restorednull/null; every exact recorded owned daemon/wrapper identity gone, ports8080/4200free, connectors0; completed disabled task/evidence retained                    |

The first verifier misparsed a JSON-decoded DateTime through a local-format string,
then the second compared artifact JSON property ordering across PowerShell
processes. Root corrected only the private UTC/value comparison code, retained both
original diagnostic captures and rechecked the same completed task without another
model call, trigger, store reset or recorded-time edit. The initial evidence capture
was32,933ms after the actual launch; the run finished7,698ms after launch. These were
controller verification defects, not scheduler defects or a production source fix.

Fresh verification is this actual local experiment plus owned documentation checks.
M21 Rust2354passed/8ignored/lint, SDK130/web1191/test/typecheck/build evidence remains
applicable to identical source; those suites were not rerun for this docs-only
milestone. Source/evidence review and Markdown/local-link checks precede publication.

This proves one short-outage, unclaimed one-time task. It does not prove recurrence
catch-up policy, arbitrary outages, clock changes, system suspend, external delivery,
exactly-once effects or hosted uptime. An already claimed interrupted task still
follows M21 disabled/review behavior. Normal owner review and scoped account/deploy
authority remain required for other workflows.
Final bounded documentation/evidence review found one retained-evidence omission: settings and free ports had been tool-verified but were not in the supplied JSON set. Root captured fresh sanitized cleanup-proof.json with exact owned identities gone, both ports free and original null/null settings restored; all assertions passed. The other reviewed chronology, provider/token figures, delayed-once behavior and limits matched the retained evidence. No additional product issue or new model task.
