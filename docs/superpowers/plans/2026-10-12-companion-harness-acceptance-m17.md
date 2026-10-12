# M17 Harness Acceptance Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for bounded fixes. Root owns live tests, review, serialized gates and publication. User-authorized lean planning replaces full code listings, extra plan audits, worktrees and per-task check-ins.

**Goal:** Demonstrate real multi-step workspace work, correction-aware memory and honest restart recovery; fix only evidenced gaps.

**Architecture:** Exercise the existing Rust daemon through its documented owner HTTP boundary against synthetic files in the retained private M16 profile. Browser access was refused by its URL policy; do not retry through another browser or start web unnecessarily. Verify actual model/tool runs, artifacts and durable records separately from narration, without claiming new browser/UI evidence. Reuse selected public upstream behavior through native Rust ports only when a concrete failure warrants a fix.

**Tech stack:** Existing Rust/Tokio runtime, control-plane JSON, embedded history SQLite, memory JSON, Bun/Nx web/SDK, existing ChatGPT connection.

## Scope and operating rules

- Branch from merged main M16 (`ba781bf` per root activation); one M17 branch.
- Root appends binding controller rulings on the five risks before execution.
- Record decisions as `Ruling: <what> — <why> — <cost if wrong>` in this milestone's `progress.md`.
- Only loopback Rust daemon, synthetic private profile and normal connected-provider inference are needed. User permission also covers web, but this API-first run leaves it stopped.
- No real connectors, sends, account consents, private reference profiles/config/env/keyring, DB server, containers, Docker Postgres55432, purchases or public deployment.
- Retain M16 evidence and independent memory marker; create M17 files only below its private `live/workspace/m17`.
- No new dependencies/features, stash/reset/restore/checkout-path/worktree, broad process kills or unauthorized profile mutation.
- Root alone runs Cargo with `CARGO_INCREMENTAL=0`; stop owned daemon before gates to avoid concurrent compilation and executable locks.
- Explicit stage paths only; never `nx.json`, any `anima.yaml`, ignored profile/evidence or credentials.
- Per-command unsigned commits and reviewed/gated merge are authorized; do not repeat GPG/merge questions.
- One daemon/core batch review, one final web/SDK review if those surfaces change, one combined fix wave, then relevant gates.

## Verified source map and interfaces

- `hosts/rust-daemon/src/tools.rs`: actual registry names `read_file`, `write_file`, `calculate`, `todo_read`, `todo_write`, `spawn_helper`, `memory_add`, `memory_search`, `recent_memories`, `load_skill`, `propose_skill`.
- `apps/web/src/lib/agent-access.ts`: Collaborate permits workspace reads/writes/todos and memory/skills, no process tools.
- `hosts/rust-daemon/src/agent_runs.rs`: Phase B injects helper tools only for a nondelegated/nonpeer workspace manager. Generated helpers cannot recursively delegate, contact other agents or run processes.
- `spawn_helper` args `{name, task}`; max four busy helpers/four starts per parent run, eight tool turns/helper, two-minute deadline. Effects already completed are not rolled back.
- `hosts/rust-daemon/src/tools/todo.rs`: `todo_write` args `{todos:[{content,status,activeForm}]}`; statuses `pending|in_progress|completed`; per-agent workspace persistence and compare-and-swap revision guard.
- `GET/PUT /api/agents/{agent_id}/tasks` exposes the saved task list. Model success text alone is insufficient evidence.
- `hosts/rust-daemon/src/tools/memory.rs`: `memory_add {content,type?,importance?}` and `memory_search {query,limit?}` are agent-scoped. Model currently has no edit/delete-memory tool.
- Owner `PATCH /api/memories/{memory_id}` accepts content/importance/tags; owner `DELETE` and temporal fact correction also exist. They are not automatically model authority.
- `hosts/rust-daemon/src/sessions/context.rs`: configurable `settings.contextBudgetTokens`, default 60% of model window capped at200k, fallback32k; reply reserve <=half budget.
- `POST /api/agents/{agent_id}/sessions/{session_id}/compact`: percent-encode session ID; 202 schedules compaction, 409 active/nothing-to-compact. Verify resulting saved summary, not202 alone.
- `hosts/rust-daemon/src/agent_runs/compact.rs`: summary uses run provider/model and owns room reservation; cancellation/failure must not commit an invented summary.
- `hosts/rust-daemon/src/runs/ledger.rs`: restart converts Running/AwaitingApproval to Interrupted with `restart_during_run` and effects warning; queued work also becomes Interrupted. This is reviewable recovery, not autonomous exactly-once continuation.
- `hosts/rust-daemon/src/tools/skills.rs`: `propose_skill {name,description,body,slug?}` creates a draft; `load_skill {name}` reads only an enabled exact approved hash. Helpers cannot propose.
- `GET /api/skill-drafts`, `POST /api/skill-drafts/{draft_id}/approve`, `GET /api/skills/{slug}` are existing owner interfaces; inspect request schema before mutation.
- Owner API requests use `Origin: http://localhost:4200`; do not bypass local-owner checks or access unrelated records.

## Reference source and useful port candidates

Public read-only source: `C:/Users/leoca/AppData/Local/hermes/hermes-agent`, clean selected files at `1fd18277267152893a8a24792b79213e7ca3f899`.

- `tools/memory_tool.py::_pin_matched_entries` and `tools/memory_tool_store.py::_find_unique_match/_locate`: add/replace/remove; ambiguity refusal; reviewed edits pin full entry and reject stale target.
- Useful adaptation if natural correction fails: exact agent-owned memory identification and expected-content comparison, atomic persistence/rollback, no arbitrary owner-level all-agent edit authority. Preserve independent memories and refuse hidden/unrelated targets.
- `tools/todo_tool.py::TodoStore.format_for_injection`: inject pending/in-progress tasks after compaction; omit finished work to avoid replay. Anima already has saved task storage and CAS; port behavior only if live continuity loses pending work or repeats completed effects.
- `agent/context_compressor_summary.py::_summarize_window`: cancellation restores only current attempt's owned prior state. Anima already reserves the room and checks cancellation; no new route/fallback subsystem without actual failure.
- Do not copy Python runtime/profile bridges, fail-open approval fallback or permissive cross-agent authority. Any copied/adapted source keeps exact revision and full MIT attribution in its existing/new appropriate notice.

## Task 1: Activate and establish bounded baseline

- [x] Root publishes tracked lean plan plus binding risk rulings and starts only isolated owned Rust daemon using the M16 script.
- [x] Verify agent `Anima validation`, gpt-5.5, Collaborate, selected private workspace, SQLite non-ephemeral history, connectorCount0 and independent `amber-913` memory.
- [x] Record owned process PID/image/UTC start ticks. Never infer ownership from port alone.
- [x] Root-created `m17/orders.csv`: A001 quantity2 at1250cents, B003 quantity3 at700cents, C001 quantity1 at500cents. Exact total5100cents,6units,3rows.
- [x] `m17/requirements.md` requires missing-discounts honesty, one read-only helper, saved report and three accurately completed todos.
- [x] Root keeps oracle totals out of the model prompt. Run no background agents/schedules at baseline.

## Task 2: Real multi-step work and recoverable tool failure

Prompt: `Read m17/requirements.md and m17/orders.csv. Complete the report task using a saved three-step todo list. Ask exactly one bounded helper to independently verify the source rows and totals; the helper must not write anything. Read m17/discounts.csv once, and if missing continue without inventing a discount. Save m17/report.md, read it back, and mark only verified work completed. Do not use shell, network, connectors or external actions.`

- [x] Evidence: actual read_file calls, todo_write persisted states, missing file error, one completed helper child linked to parent, write_file and read-back.
- [x] Artifact: exact5100cents/$51.00,6units,3rows; accurately disclosed missing discount file and actual helper result.
- [x] Task view/API agrees with persisted todos; no claimed completion of missing/external work.
- [x] Missing optional input does not abort entire run or cause repeated blind retries. At most one targeted retry after a concrete fix.
- [x] Preserve artifacts even if a later step fails; ledger/tool evidence must distinguish completed effects from failed overall run.

## Task 3: Memory save, correction and fresh recall

Prompt A: `Remember this synthetic validation preference: my preferred report format is concise bullets, preference marker plum-286. Keep my other saved preferences and independent facts.`

Prompt B: `Correction: my preferred report format is a compact table, marker lime-604. Replace the obsolete plum-286 preference; do not keep both as current. Keep all independent memories.`

- [x] Inspect actual durable memory records after both runs. Contradictory append-only records do not pass correction acceptance.
- [x] Fresh chat asks for current report preference without repeating either value or marker; require lime-604/table and preservation of independent amber-913/cedar-tea fact where requested.
- [x] Distinguish model-facing correction from owner memory API edit. Owner edit is a baseline, not a substitute for an agent claiming a successful replacement.
- [x] If actual append-only gap manifests, add minimal native agent-scoped update behavior with expected current content/revision and static errors; use core reusable mutation layer where needed.
- [x] Regression cases: own edit, unrelated-agent refusal, nonexistent target, stale target, failed persistence rollback, hidden-text rejection, no modification of independent records. Update tool inventory/grants/API/SDK/UI only if their contract changes.
- [x] Embeddings remain disabled; do not claim semantic-vector quality from lexical recall.

## Task 4: Compaction and skill reuse

- [x] Attempted manual compaction once; it correctly returned Nothing to compact yet. The automatic threshold scenario below then verified a durable summary.
- [x] Poll boundedly until summary saved or compactionError appears. Record summary coverage and lack of active duplicate run; 202 alone fails evidence.
- [x] Ask to recall report location, exact result, missing-input caveat and current task completion without replaying file writes; verify saved artifact unchanged.
- [x] If automatic compaction needs proof, temporarily lower only private agent `contextBudgetTokens`, add bounded synthetic context, exercise one actual auto-compacting run, and restore original setting. Do not claim manual compaction proves automatic threshold behavior.
- [x] Propose a small reusable `Synthetic report check` skill based on the verified read/count/total/missing-input/readback workflow. Verify proposal remains a draft until reviewed.
- [x] Root reviews exact synthetic instructions and approves via existing owner API within authorized acceptance scope; verify enabled exact hash.
- [x] Fresh chat loads the approved skill and uses it to check existing report without rewriting/re-running completed work. Evidence includes load_skill and actual source/file checks.
- Not applicable: the root reviewed and approved only the synthetic read-only draft within the authorized owner acceptance scope; no runtime approval was bypassed.

## Task 5: Quiet and in-flight restart recovery

- [x] Quiet restart same stores; reread owner API records; verify saved report, completed tasks, corrected memory, independent fact, summary and approved skill survive.
- [x] Start one synthetic bounded run: saved pending task plus harmless sufficiently long answer; interrupt only while actual ledger reports Running, before final completion.
- [x] Root stops exact owned daemon after verifying PID/image/start ticks. In-flight interruption is intentional and must not masquerade as a graceful quiet shutdown.
- [x] Restart exact stores; inspect prior run Interrupted/restart_during_run, effects warning, no hidden automatic replay and persisted partial artifacts/tasks.
- [x] Owner-directed follow-up asks agent to inspect existing artifacts/tasks and finish only remaining work; verify no duplicate completed effects.
- [x] No exact-once/automatic recovery claim: successful manual resumption plus honest interrupted record is the acceptance contract.
- Not exercised: optional local one-shot automation was unnecessary; no schedule was created.

## Task 6: Fix, review, gates and publication

- [x] Turn each actual failure into one minimal regression test before implementation; preserve reusable core/host boundaries.
- [x] Record fixture evidence separately from actual model/tool/HTTP/restart evidence; no new browser evidence or broad upstream-parity claim.
- [x] One batch daemon/core review, final web+SDK review if changed, one fix wave; re-exercise only affected live scenario.
- [x] Stop owned daemon before gates; check its loopback listener absent, retain synthetic evidence/profile. Do not fabricate a screenshot for an API-first milestone.
- [x] Rust changes: `CARGO_INCREMENTAL=0`; `bun x nx run rust-daemon:test --skipNxCache`, then `bun x nx run rust-daemon:lint --skipNxCache`. Root sets known local Nx Cloud-off environment and redirects logs to milestone ledger.
- [x] Web/SDK changes: relevant existing Nx test/typecheck/build targets, discovered before invocation. No full rerun for untouched surfaces.
- [x] Format explicit touched Rust/Markdown/TS paths; `git diff --check`. Do not format/stage unrelated `nx.json`.
- Publication: master status and handover updated; final commit/push/PR/verified merge completion is recorded with exact SHAs in the milestone ledger.
- Publication resume: after verified merge, fast-forward main and append exact source/PR/merge SHA to the ledger; that final record closes publication.

## Top five risks for controller rulings

1. **False success:** fluent model claims can hide wrong totals, absent helper work or unstored tasks. Independent files, tool records and durable DTOs are mandatory.
2. **Correction authority:** adding model edit/delete could accidentally expose unrelated memories or silently overwrite concurrent owner edits. Agent ownership, expected current content and persistence rollback must bind every mutation.
3. **Restart effects:** killed work may already have written files or started tools. Interrupted warning plus inspection/manual continuation is required; no blind replay/exact-once promise.
4. **Token and context cost:** broad parity probes, long context floods and repeated full gates could exhaust budget. Use one small oracle task, one helper, bounded compaction, isolated targeted retries and only changed-source gates.
5. **Private/reference contamination:** reused account auth or upstream code scans could touch real secrets/workspaces or activate channels. Runtime credential use stays internal, public source reads only, selected private paths and recorded owned process identities.

## Execution outcome

Root fills verified outcomes and remaining limits after actual execution. This plan contains no asserted live success or gate result.

## Controller rulings on the risks (binding)

1. Ruling: Compare saved artifacts/tasks/run records against a fixed synthetic oracle — narration is not completion — cost if wrong: mark the scenario failed and fix the observed layer.
2. Ruling: Any memory correction tool edits only this agent's visible memory with expected-current-content matching and transactional persistence — owner APIs do not grant model all-agent authority — cost if wrong: reject stale/hidden/unrelated targets rather than overwrite them; retain independent facts.
3. Ruling: Intentional in-flight restart requires exact recorded process identity and an observed Running ledger record — completed effects may survive interruption — cost if wrong: inspect records/artifacts before manual continuation; never blind replay or claim exactly-once recovery.
4. Ruling: Use one small report/helper, short memory turns, one manual compaction and one bounded restart interruption — selected quality matters more than breadth — cost if wrong: automatic compaction/semantic vectors/external delivery remain explicitly unverified instead of context floods or repeated provider grids.
5. Ruling: Reuse only the synthetic profile and normal internal provider connection; inspect public reference source only — local auth and upstream profile data must remain private — cost if wrong: abort on unexpected agents/connectors/processes; no browser-policy workaround, external consent/send or real database/container.

Activation: the owner approved the proposed harness acceptance milestone. The controller accepts the lean plan and risks above. No separate preflight audit or approval pause; root owns API acceptance and services, workers own bounded source fixes, and gated publication/verified-head merge follow standing authorization.

## Verified execution outcome (2026-10-12)

Real acceptance used the actual gpt-5.5 runtime through documented owner HTTP APIs,
with the retained synthetic profile, SQLite history and JSON memory. No browser
workaround was used after the browser URL-policy refusal; no new UI evidence is claimed.

- Multi-step task completed with actual file reads, calculation, exactly one helper,
  task writes, report write and readback. Independent oracle: 3 rows, 6 units,
  5100 cents/$51.00. The missing optional discounts file was read once, failed
  honestly and did not prevent completion. Saved helper result and three completed
  tasks agree with the artifact.
- Natural memory correction initially failed: the model appended a new fact and
  claimed replacement, leaving both old fact records current. A minimal native
  memory_update tool now requires this agent's visible record ID and exact reviewed
  current text, rejects stale/unrelated/hidden targets and rolls back failed saves.
  Search/recent results provide IDs, types and full safe content. Existing grants
  stay unchanged; Collaborate/Operate can explicitly grant the tool.
- Live retest updated both old fact IDs to compact table/lime-604. The original
  independent amber-913 fact stayed unchanged. Fresh chat after quiet restart
  used memory_search and recalled the corrected preference without either answer
  appearing in its prompt. Historical reflections remain historical records.
- The agent proposed a short read-only report-check skill as pending. Root reviewed
  and approved its exact synthetic instructions within the authorized acceptance
  scope. Actual load_skill/read_file/todo_read reuse verified the report and saved
  tasks; report hash stayed unchanged.
- Manual compact returned Nothing to compact yet, correctly refusing empty work.
  A temporary private contextBudgetTokens=6000 then exercised automatic compaction:
  a real saved summary covered all 22 original messages and retained the exact
  report, totals, caveat, helper and task outcomes. The original budget setting
  was returned after testing, preserving new work.
- An intentional interruption occurred while the ledger was Running after an actual
  checkpoint file and completed/pending tasks were durable. Restart restored
  Interrupted/restart_during_run with the effects warning, without automatic replay.
  Explicit owner-directed continuation read existing state and wrote only the pending
  recovery note. The completed checkpoint hash stayed unchanged. Quiet restart then
  retained completed tasks, approved skill, summary, corrected memory and artifacts.

Useful upstream pinned-entry replacement behavior was adapted into Rust, with
exact revision 1fd18277267152893a8a24792b79213e7ca3f899 and full MIT notice retained.
No Python/profile/token-file bridge, arbitrary global edits or removal tool was added.

Validation: six targeted Rust regressions failed before implementation and passed
six after it; focused profile tests went from expected failures to 16 passing.
Fresh Nx rust-daemon:test: 2310 passed / 8 ignored (core636, host1674); Rust lint
passed. Web1188 tests/110files, typecheck and build passed. One daemon batch and
one final web/SDK contract review found no remaining actionable issues. SDK source
and protocol did not change; dependency builds passed through the web targets.

All recorded validation services stopped; ports8080/4200 were free. Private evidence
and exact publication/merge/resume facts remain in this milestone's ignored ledger.
These results cover selected local harness behavior, not overall upstream parity,
semantic-vector quality, autonomous skill improvement, external delivery/consent,
mic/vision, live schedules, public deployment or offsite recovery. Optional one-shot
scheduling was unnecessary and was not exercised.
