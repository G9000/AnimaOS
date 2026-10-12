# M28 Core/Harness Acceptance and Evidence Matrix — Acceptance Plan

**Status:** accepted on exact merged M27 source304184da765875b90a37104272558ac34f12ebc9; one live task and quiet restart passed. Native source unchanged. Publication is recorded in the milestone ledger. The planned scope below was bound before execution.

**Purpose:** Give the owner one concise map of what the companion has actually demonstrated, what current deterministic tests establish, and what remains unproven. Use it to decide whether one small post-M27 live run adds useful evidence; do not turn the matrix into a broad re-acceptance campaign.

## Evidence labels

- **Live local:** A historical, isolated local run with a real configured model. This proves that specific observed path at that revision and profile only.
- **Deterministic:** Current source/unit/mock/API gates exercise the stated contract. This is not model adherence or provider reliability.
- **Unproven:** Do not infer full Hermes/OpenClaw parity, semantic quality, production uptime, or external effects from the evidence listed.

## Core and harness evidence matrix

| Capability                               | Existing evidence                                                                                                                                                                                                                                                                                                                                                   | What that evidence supports; remaining limit                                                                                                                                       |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Chat and multi-step workspace work       | **Live local M17:** gpt-5.5 completed a multi-step report; independent artifact checks matched the report. **M19/M22:** one-shot workspace writes completed.                                                                                                                                                                                                        | Real but small synthetic tasks. Not broad task success, long-running autonomy, or production workload quality.                                                                     |
| Exact memory edit and relevant retrieval | **Live local M17:** scoped memory search/update corrected the intended synthetic facts while preserving an unrelated fact. **Live M18:** a fresh query found the corrected preference after 20 newer reflections and 8 matching foreign facts. **Deterministic M20:** explicit lexical retrieval eligibility and hidden-record filters pass; no live model was run. | Proves those owner-scoped examples. Semantic/vector recall and general ranking remain unproven.                                                                                    |
| Tasks, helpers, and skills               | **Live local M17:** three saved tasks; one read-only helper independently agreed with arithmetic; an approved skill was read and reused.                                                                                                                                                                                                                            | These specific paths worked. No broad delegation quality, autonomous skill maintenance, or third-party effects.                                                                    |
| Context compaction                       | **Live local M17:** automatic compaction occurred under a temporary bounded context setting; summary persisted.                                                                                                                                                                                                                                                     | The real compaction path worked once. Do not claim summary quality for arbitrary histories or fixed default thresholds from that run.                                              |
| Schedules                                | **Live local M19:** one scheduled write survived pre-due and completed quiet restarts. **Live local M22:** one enabled, unclaimed one-shot that became overdue while daemon-off ran once after restart.                                                                                                                                                             | Two bounded one-shot cases only. Recurrence, arbitrary outage duration, clock changes, and hosted uptime are unproven.                                                             |
| Interrupted action receipts and restart  | **Live local M21:** a schedule crashed after a successful write and native receipt; two restarts retained evidence without replay. **M17:** interrupted run was explicitly resumed after owner review.                                                                                                                                                              | Receipt durability and the selected recovery paths worked. This is not exactly-once external execution; unknown effects still require inspection.                                  |
| Budget wrap-up                           | **Live local M23:** one real read completed, the requested write remained unfinished, and the model returned an honest response after the cap. Same-run quiet restart preserved the accepted reply and `budgetWrapup` marker. **Deterministic:** cap/zero/no-tools/Stop/storage and adapter-shape tests.                                                            | One provider-following example plus native contracts. No hard token ceiling or guaranteed quality of future wrap-ups.                                                              |
| Unconfigured-tool refusal repair         | **Deterministic M25:** first valid single unknown request is paired with a static refusal, no dispatch/receipt, and one correction opportunity; second or malformed/mixed request remains terminal. Web renders request counts neutrally.                                                                                                                           | No live model adherence proof. A requested tool is not an executed action.                                                                                                         |
| Pre-stream transport retry               | **Deterministic M26:** mock HTTP covers one eligible 502/503/504 retry, same body/account, cancellation, zero opt-out, and terminal auth/quota/schema/MIME/read/stream/send cases.                                                                                                                                                                                  | **No live retry acceptance.** Failed first-attempt usage is unknown; no billing cap or provider reliability claim.                                                                 |
| Repeated-result advisory                 | **M27:** source/gates are now verified:6cases/Rust2404/8ignored/lint/PR33. Intended deterministic contract is one factual hint after three identical fresh plain-text successes, with results and effects unchanged.                                                                                                                                                | M27 passed6cases/fullRust2404/8ignored/lint and merged PR33; its actual provider usefulness remains unverified. No no-progress or effect-safety inference.                         |
| Hermes comparison                        | **Deterministic design reference only:** public cached M23 helper files document the specific upstream behaviors compared in M23–M27.                                                                                                                                                                                                                               | Comparison is limited to those cached public helpers. Do not call this complete or current upstream parity, and do not import private profiles/credentials or execute cached code. |

## One optional live check after M27

Recommend at most one fresh task, only if root judges that M27 materially changes the ordinary loop and a model-following observation is useful. Use the existing isolated synthetic local profile and latest merged `main`; do not repeat earlier tasks or reset retained profile data beyond the specified temporary settings and synthetic target edit. Bind one already-present synthetic memory record and one new unique workspace path after checking both are safe.

Use the normal configured provider, `max_tool_iterations=3`, and a 20-second lead timeout. Ask for a relevant memory search, an exact-current-text update of only that selected synthetic record, and one exact small workspace write. Keep each action in its own round; the cap should then permit its already-next no-tools wrap-up. Verify exact memory ID/current content and preserved neighboring fact, file bytes/hash, tool/run/message/receipt pairing, and honest reporting of anything unfinished. A completed reply is not proof that every request succeeded.

Do not deliberately trigger repeated failures, repeated identical successes, invalid tool calls, provider retry, schedule work, helper spawning, compaction, or an interrupted crash in this run. M24–M27 deterministic regressions are the evidence for those branches. No resubmission, retry, prompt repair or auxiliary provider probe to improve a result; one logical task can contain up to four ordinary model steps. Stop and preserve evidence on an unexpected write, receipt error, wrong memory target, timeout, or ambiguous effect.

## Five binding risks for root

1. **Revision and evidence:** Run only after the exact M27 merge/current-main tree is verified. Keep historical real-provider evidence, current gates, and any new live observation in separate categories; a local model task does not replace deterministic regressions.
2. **Scope and cost:** At most one fresh provider task, three tool rounds, 20-second lead timeout, no retry/replay. Report surfaced usage as observational; neither the timeout nor configured round count is a hard provider billing ceiling.
3. **Memory authority:** Select only an already-present synthetic record owned by the lead; require search plus exact current text/ID before updating. Preserve neighboring facts and stop on ambiguous match. No ordinary memory, secret profile, credential, or account-store inspection.
4. **Effects and provenance:** Use one new exact workspace path and correlate actual bytes with the run, messages, requested calls, and native receipts. A success receipt means the handler reported success, not that the complete user goal was verified; inspect partial/unknown effects before any owner-directed retry.
5. **Loop boundaries:** The live task should not manufacture M24/M25/M26/M27 trigger conditions. Verify cap wrap-up does not issue another tool action; changed code remains covered by current scripted tests. No claim of full upstream parity, semantic memory quality, recurrence, external delivery, or uptime.

## Test expectations and resumption

- First confirm M27's own meaningful RED-to-GREEN regressions and required current-tree Rust/harness gates from its ledger; do not duplicate a passing core gate merely because this matrix mentions it.
- Check that M17–M26 focused tests still cover the inherited memory, schedule, receipt, wrap-up, refusal, and transport boundaries. Add a harness regression only if M27 exposes a concrete missing core/harness contract; do not write a test that repeats an existing fixture without new risk coverage.
- If no code changes occur, reuse the recorded M27/M26 source gates and run only the exact owned Markdown formatting/link/diff checks needed for any publication edits. If M28 source changes, root binds the relevant Nx targets before work; no guessed flags or new target directories.
- No provider retry-loop/model grid, no repeated live prompt, no normal services or real connector/account operations, and no provider task to compare model generations.

**Exact next canonical planning path:** `docs/superpowers/plans/2026-10-12-companion-agent-loop-m28.md`. Root should create/bind it only after M27 publication, link it from the master plan, and revise `docs/companion/daily-use.md` or the handover only if actual evidence changes their current claims.

## Controller rulings on the risks (binding)

1. Ruling: Test only exact verified mergedM27/main304184d in retained syntheticM16 profile; historical real-provider examples, fresh deterministicgates and this new observation stay distinct — no inferred current upstream or production parity — cost if wrong: preserve failure/evidence and revise claims, not source or historical results.
2. Ruling: One accepted logical provider task, max3toolrounds/20sectimeout/explicitmax_retries0/manualsessiontitle, no resubmission or auxiliary probe — one task may contain up to4modelsteps and failed/compaction usage is not a billing cap — cost if wrong: stop and inspect partialeffects; no quota-consuming test loop.
3. Ruling: Select one existing visible lead-owned synthetic Fact through owner API, append one distinct marker after model searches full current text/exactID; preserve another own Fact — exercise actual CAS without touching ordinary/private owner data or granting tools — cost if wrong: reject stale/ambiguous input and report unfinished; no direct fixture repair to fake model success.
4. Ruling: One absent exact workspace output under retained synthetic root; correlate bytes/hash/mtime, actual paired messages and persistent receipts, then quiet same-store restart once — handler success is not proof of goal completion or exactly-once effects — cost if wrong: retain uncertain outcomes and inspect, never replay/backfill.
5. Ruling: No deliberate errors/cycles/fallback/messaging/accounts or browser workaround; restore only three temporary fields while stopped and clean exact owned process identities — selected ordinary loop integration is sufficient for this cheap check — cost if wrong: abort unexpected connector/queued/schedule state, no broad kill/data reset/secret read/DB/container/public deployment.

## Root execution decision

User's isolated-testing/continue authority persists. This M28 introduces no native
source change unless an actual source failure is established. Keep original
max_tool_iterations/timeout_ms/max_retries plus model/provider/grant names;
restore just those three fields on freshly loaded synthetic control-plane data.
Do not restore an old whole profile. Profile must have zero connector/account
records and no enabled schedules/pending runs before launch. Same normal internal
ChatGPT connection, no credential/vault/env inspection/import/copies. CARGO0,
existingtarget, no gates while daemon cargo-run wrapper active. Operator source
is copied only into ignored evidence folder; root owns all effects/process/Git.

Existing synthetic lime-604 preference is the target, preserving its prior content
and appending exactly newline + M28 acceptance marker: cedar729. A separate
amber-913 fact remains unchanged. New file m28/acceptance.txt must initially be
absent and contain exactly m28 proof cedar729 without newline. One sequential
memory_search, exact memory_update and write_file are requested; inspect whatever
actually happens, do not force a pass. M24–M27 rare trigger behavior remains
scripted-test evidence; this task does not establish their model responsiveness.

RequiredM27Rust2404/8ignored/lint source gates already passed on the same code;
SDK130/web1192/typecheck/build are M25historical checks for unchanged surfaces.
No redundantfullgates for docs-only acceptance; review concrete evidence/docs,
exact touchedMarkdown formatting/links/diff before unsigned publication/PR/merge.

## Verified M28 result

One actual configured gpt-5.5 task, run_5637f9bb-26dc-4d2f-8602-9f71ff5ff71f,
session chat:bb4bc177-2b7c-4e7c-a9f3-5fab3dabc53c: memory_search, exact-current
memory_update on mem-1791783806248-4, then one exact18byte write. Separate
amber-913 Fact unchanged. Fourth model step returned a factual accepted
budgetWrapup reply;12595 reported tokens and3 persistent succeeded receipts.
No resubmission, auxiliary model probe, title generation or deliberate rare
recovery trigger. Temporary3round/20second/0retry settings restorednull/null/null;
original model/provider and21 existing grant names preserved.

One quiet same-store restart retained the same completed run/usage, all8 original
message IDs/roles/text/run-step-call-status metadata/accepted marker, every scalar
receipt field, edited Fact/unrelated Fact and exact file bytes/SHA256/mtime.
No observed replay. All14 recorded daemon/wrapper/runner identities are gone;
8080/4200free,0connectors/0running/0queued/0pendingFlush. Forceful quiet cleanup
is not graceful-wrapper proof. Browser and hosted uptime remain unverified.

Controller-only preflight/verifier corrections did not alter native source or
rerun the model: CamelCase snapshot keys/null-array guard, API lowercase fact,
actual hyphenated synthetic markers, and native toolStatus for successful plain
Content (only errors require JSON fallback). Initial capture retained. A restart
permission review timed out before execution; one authorized retry succeeded.
No hidden failed provider task, source fix or direct effect repair.

M27 requiredRust2404/8ignored/40suites/lint remains unchanged verified source;
M25SDK130/web1192/typecheck/build is historical for unchanged surfaces. Fresh
M28 evidence/docs review/format/local-link/diff checks precede publication.
See [harness acceptance](../../companion/harness-acceptance.md) for the concise
current evidence and limits. No complete upstream parity assertion.
