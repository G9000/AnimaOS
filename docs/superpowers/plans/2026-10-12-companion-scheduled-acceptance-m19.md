# M19 Local Scheduled-Work Acceptance Plan

> **For agentic workers:** Use `superpowers:executing-plans` for bounded steps; root controls all gates, services, API mutations, model calls, validation, and Git.

**Goal:** Observe one harmless real-provider scheduled file task with the web service off, persistence through restart before due, and no repeated completed effect after a second restart.
**Architecture:** Reuse the daemon's existing `once` trigger, workspace target, durable claim, stable schedule conversation, and run ledger. This is acceptance evidence, not a new scheduler or an exactly-once promise.
**Tech stack:** Existing Rust daemon, retained private M16 profile, configured `gpt-5.5`, owner-authorized local HTTP, existing file tool and persistence.
**Status:** Active after M18 verified merge7144181cf53c4c7482ea8e0d710fb4e30476b6ea; root binding rulings below authorize the bounded acceptance.

## Boundaries and evidence

- M17 harness/chat/memory/skill/compaction/inflight restart evidence is inherited context; root must bind current post-M18 revision and binary.
- The preparation worker wrote only the lean plan. Root subsequently executed the authorized isolated acceptance; no source edits or dependency changes were needed.
- Future scope: one synthetic workspace file, one schedule, one occurrence, one successful completion, restart before due and once after completion.
- No shell tool, external destination, connectors, consents/sends, database server/containers/55432, personal secrets/profile/keyring, or public deployment.
- Keep `nx.json`, `anima.yaml`, private profile contents, logs, and fixtures unstaged; no worktree/stash/reset/restore/path checkout.
- Root alone serializes Cargo with `CARGO_INCREMENTAL=0`; retain per-command unsigned commit policy if a reviewed code correction proves necessary.
- Evidence captures IDs, statuses, time, hash, and safe usage summaries; redact credentials and unrelated private content.

## Existing contracts inspected (six targeted files only)

- `hosts/rust-daemon/src/lib.rs`: schedule module registration.
- `hosts/rust-daemon/tests/schedule_api.rs`: owner guard, CRUD, idempotent import, OpenAPI paths.
- `hosts/rust-daemon/src/schedules.rs`: 250 ms worker tick, durable claim, one-shot disable, stable schedule room, run source/ref, reconciliation.
- `hosts/rust-daemon/src/schedules/timing.rs`: `once.atMs` must be future positive milliseconds; once rejects active hours.
- `hosts/rust-daemon/src/routes/schedules.rs`: HTTP routes, owner authorization, history limit 1–50.
- `hosts/rust-daemon/src/routes/contracts/schedules.rs`: request/response schemas and run/session history IDs.

| Interface        | Exact operation / relevant result                                                                                                 |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Preview          | `POST /api/schedules/preview`, `{ "trigger": { "type": "once", "atMs": DUE_MS } }`; `nextRuns:[DUE_MS]`                           |
| Create           | `POST /api/agents/AGENT_ID/schedules`; 201 new or 200 existing idempotent schedule                                                |
| Inspect          | `GET /api/agents/AGENT_ID/schedules`; locate by saved ID, never assume a single global item                                       |
| History          | `GET /api/agents/AGENT_ID/schedules/SCHEDULE_ID/history?limit=5`; `runs[]` includes `runId`, `sessionId`, `manual`, outcome/times |
| Disable          | `PATCH /api/agents/AGENT_ID/schedules/SCHEDULE_ID`, `{ "enabled": false }`                                                        |
| Manual run       | `POST .../SCHEDULE_ID/run` exists; excluded because it would invalidate autonomous trigger evidence                               |
| Run/detail/stop  | `GET /api/agents/AGENT_ID/runs/RUN_ID`; `POST /api/agents/AGENT_ID/runs/RUN_ID/stop`                                              |
| Session evidence | `GET /api/agents/AGENT_ID/sessions/ENCODED_SESSION_ID/runs` and `.../messages`                                                    |
| Task evidence    | `GET /api/agents/AGENT_ID/tasks`; correlate exact run/session/task references, not latest unrelated task                          |

The retained owner client supplies its already-established authentication and origin; tests use `Host:127.0.0.1:8080`, `Origin:http://localhost:4200`. Do not infer production authorization from test headers.
Schedule bodies reject unknown fields: there is no schedule-level `maxTokens`, tool-turn, or deadline field.
Workspace occurrences enter their own stable `schedule:<id>` room through the existing run coordinator, with source `schedule` and source ref equal to the schedule ID.
Responses expose `enabled`, `running`, `lastFiredAtMs`, `lastOutcome`, `nextDueAtMs`, and `counters`; history failure name is `failed`, while `lastOutcome.status` uses `error`.

## Top five risks: root must bind these rulings before arming

1. **Runtime/provider cost bound:** One occurrence/no automatic retry; stopped private lead gets `settings.max_tool_iterations=4`, `settings.timeout_ms=20000` (serialized snake_case); root deadline 60 s from due. Budget ceiling 12,000 reported aggregate tokens, brief final output (request at most 32 tokens); stop on surfaced breach. Subscription transport may not enforce `maxTokens` and usage may arrive only afterward: explicitly record hard token cap unavailable/total unknown where applicable; root accepts time/iteration controls, no cap claim or new feature.
2. **Owner/profile isolation:** Root binds retained lead `agent-1791779202818-0` (`Collaborate`), configured `gpt-5.5`, private M16 profile, and workspace `.superpowers/sdd/2026-10-12-companion-live-acceptance-m16/live/workspace`. Preserve old private settings, restore them after test with daemon stopped. Never read personal profiles or switch providers opportunistically.
3. **Irreversible or repeated effect:** Permit only one tiny new synthetic file at a unique relative path. Preserve its bytes, hash, modification time and completed file-tool event count; hash equality alone cannot detect a repeated identical write. No helpers, schedules created by the agent, retries, appends, or shell commands.
4. **Claim/stop/restart race:** One-shot claim disables the schedule before work completes. Disabling is not stopping an admitted run; stop exact run on timeout and confirm terminal status. Restart before due, then again only after durable completion; if restart crosses due, report delayed delivery rather than pretend on-time evidence. No intentional inflight crash window.
5. **Evidence overclaim:** A manual-false daemon schedule with the web service off proves the observed daemon trigger; browser closure is unverified. Unclaimed overdue once work, interrupted recovery, clock changes, sleep, recurrence, remote uptime and exactly-once effects remain separate policies/evidence. Report uncertainty directly.

## Task 1: bind preconditions and limits (root, no model call)

- [x] Confirm M18 merged, required M18 verification passed, clean intended source tree apart from preserved unrelated edits, binary matches that revision.
- [x] Confirm daemon owner and retained profile; use M17 verified owner client and only current agent/schedule/run/task metadata.
- [x] File tools already verified by root: `write_file` args `{file_path:"m19/scheduled-proof.txt",content:"scheduled proof marker saffron-318"}`; `read_file` args `{file_path:"m19/scheduled-proof.txt"}`. Do not issue a preparatory model call.
- [x] Root saves old settings while stopped, sets private 4 iterations/20,000 ms timeout, and records 60 s controller bound plus honest token enforcement limitation.
- [x] Prove unique synthetic destination absent and no running/scheduled work from this acceptance ID; preserve unrelated schedules and profile state.
- [x] Prepare unique label `m19-once-<nonce>`; destination `m19/scheduled-proof.txt`, exact ASCII bytes `scheduled proof marker saffron-318` (no newline). If already present, stop and bind a fresh synthetic path before arming, preserving old bytes.

## Task 2: create and arm exactly one one-shot (root)

- [x] Prepare prompt: “Local acceptance task. Call write_file once with file_path m19/scheduled-proof.txt and content scheduled proof marker saffron-318, without a trailing newline. Do not append, delegate, create schedules, use shell/connectors, or access other files. Return only M19_DONE after successful tool result; stop on failure.”
- [x] Set `DUE_MS = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() + 30000`; validate preview before creating. Recalculate if fewer than 10 s remain; never change the system clock.
- [x] Submit once: `{ "name":"LABEL", "prompt":"PROMPT", "trigger":{"type":"once","atMs":DUE_MS}, "target":{"type":"workspace"}, "enabled":true, "importIdempotencyKey":"LABEL" }`.
- [x] Save returned ID and confirm matching due time/target/owner. Never repeat with a different key after an ambiguous create result: list first and recover the original ID.
- [x] Keep the web service off; browser closure is unverified and browser policy must not be bypassed. Root shuts down/restarts only owned daemon on same private profile before due; preserve schedule ID/due; confirm restored enabled/non-running record. Root polling never calls manual-run/chat.
- [x] Poll schedule list/history every 2 s in bounded chunks; stop at due + 60 s, iteration limit, surfaced token ceiling, provider error, or unexpected tool/action. Only one fire; no automatic retry.

## Task 3: verify completion and durable links (root)

- [x] Require one history occurrence with `manual:false`, `outcome:"spoke"`, non-null run/session IDs, no error, `enabled:false`, `running:false`, `counters.runs:1`, failures 0.
- [x] Follow run/session/task interfaces above (URL-encode session ID); require terminal success, source `schedule`, matching source ref, and exactly one completed `write_file` event with exact args. Root checks file directly or owner file API without a new model call.
- [x] Read only the synthetic file: exact expected bytes, hash, modification time; compare task/run evidence to actual file, not assistant text alone.
- [x] Save sanitized evidence with due/fired/finished times, model/usage, IDs and statuses. Any missing separate task object is reported explicitly; do not invent one or claim persisted task history solely from schedule history.
- [x] On timeout/failure: PATCH disabled, stop the admitted run if present, await terminal state and `running:false`; no manual retry or second live model call sequence in this milestone.

## Task 4: second restart after completed effect; leave nothing armed (root)

- [x] Stop only owned acceptance daemon; restore old private lead settings while stopped, then restart on same profile/revision. This is the second restart; keep output/schedule evidence intact.
- [x] After ready, wait 5 s (20 worker ticks), re-read schedule/history/run/task and synthetic file metadata; retain same run/session IDs, one occurrence/write, exact bytes/hash/mtime, disabled/non-running state.
- [x] Require no new scheduled occurrence, model request, queued task, or file effect from the acceptance ID. Preserve the disabled schedule/history as evidence; do not delete unrelated or private state.
- [x] If restart fails or state is ambiguous, leave owned daemon stopped, record the unresolved evidence and verify nothing can fire; do not re-arm.
- [x] Record observed completed-effect non-duplication only. Unfinished claims are expected to become `schedule_run_interrupted` and disabled for review, not replayed; this run does not test that crash boundary.

## Bounded tests / minimal gap handling

- Existing test evidence to inspect: `schedule_crud_is_agent_scoped_and_mutations_require_local_owner`, `openapi_registers_schedule_crud_and_import_contracts`, `once_fires_once_and_refuses_active_hours`, `restart_disables_unfinished_claim_without_replaying_it`, `reconciliation_preserves_completed_and_never_claimed_schedules`.
- If an observed source gap requires correction, root scopes one small regression first; preserve host/core boundaries, no dependencies/features. Root runs `bun x nx run rust-daemon:test --skipNxCache` in the current corrected tree, with documented Windows validation target fallback if locked.
- Prefer existing scheduler/timing/history/run coordinator. No Hermes source needed for this acceptance; any later comparison is limited to public local cron/todo source, never execution/config/env/profile/token reads, and only to explain a demonstrated gap.
- Exit report: PASS only when all four tasks' required evidence exists and no acceptance work remains armed; otherwise PARTIAL/FAIL with exact limits. Do not broaden into recurrent/live-connector/cloud uptime claims.

## Controller rulings on the risks (binding)

1. Ruling: One occurrence, four tool iterations, twenty-second private agent timeout and sixty-second controller deadline from due, with no retry — reported token usage is observational and the subscription transport supplies no proven hard token cap — cost if wrong: stop the exact admitted run and report failure/unknown usage rather than inventing a budget guarantee.
2. Ruling: Reuse only the retained synthetic lead/profile and restore its two original settings while stopped — personal accounts/files and unrelated service state are outside scope — cost if wrong: leave the owned daemon stopped and record restoration uncertainty without changing other profiles.
3. Ruling: Permit one new synthetic file and verify exact bytes, hash, modification time and one completed write event — an identical hash alone cannot show non-duplication — cost if wrong: preserve evidence and stop, without a second model attempt.
4. Ruling: Due time is thirty seconds after preview/create to allow the owned pre-due restart; second restart occurs only after durable completion — disabling does not cancel an admitted run, and tool startup timing needs margin — cost if wrong: report delayed delivery or interruption honestly, disable/stop the exact run and do not re-arm.
5. Ruling: Web service stays off; do not claim browser closure, exactly-once recovery, recurrence or hosted uptime — browser access remains policy-blocked and a successful one-shot is narrower evidence — cost if wrong: the unobserved boundary remains explicitly unverified.

Activation: root accepts the prepared lean plan after verified M18 merge. Root owns
services, schedule APIs, the single model occurrence and evidence; no new source,
Cargo/dependencies, connectors, account actions or browser workaround are needed
unless a concrete acceptance failure justifies a scoped regression-first repair.
Documentation-only gates reuse unchanged M18 source verification and validate
owned Markdown/local links plus one source/evidence review before publication.

## Verified outcome (2026-10-12)

PASS on M18 merge7144181cf53c4c7482ea8e0d710fb4e30476b6ea and its freshly
built daemon. One schedule restored enabled/idle 10.7 seconds before due after
an owned quiet restart, fired 137 ms after due without a manual trigger, then
completed on gpt-5.5 in two steps/6,156 reported aggregate tokens. Exactly one
write_file call and successful correlated tool event produced the requested
34 UTF-8 bytes without a newline; final reply was M19_DONE.

After durable completion, root restored the two original private settings while
stopped and restarted the same stores. After twenty worker ticks, the same one
fire/run/session, completed status, tool event count, message count, token usage,
file bytes/hash/modification time were retained. Schedule disabled/non-running,
zero queued/running runs, zero pending history flushes and zero connectors.
Both recorded owned daemon/wrapper identities were stopped; ports8080/4200 free.
The private task API retained two completed earlier todos; this scheduled prompt
did not create an independent todo object. Durable scheduled-work evidence is
its schedule history and linked completed run/session, not an invented task row.

IDs: schedule-1791789424431-1; run_6d758702-07f5-475e-b238-c8fa4fe5fed0;
session schedule:schedule-1791789424431-1. Synthetic evidence, settings backup,
owned process records and file remain ignored in the milestone/M16 directories.

No runtime/UI/SDK changes were needed, so unchanged M18 Rust2324/8ignored/lint
verification remains current source evidence. This docs-only milestone requires
source/evidence review, owned Markdown/local links and diff checks, not another
full test suite or model call. No browser closure, exactly-once effects,
unclaimed overdue policy, intentional inflight schedule crash, recurrence,
external messaging or hosted uptime claim. Publication SHA/PR/merge and resume
are appended to the private ledger after tools verify them.
