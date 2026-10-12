# M29 Repeated Multi-Call Cycle Advisory — Implementation Plan

**Status:** Root-bound implementation on `g9000/companion-agent-loop-m29`, cut from verified merged M28/PR34 main `9262030949e170aabe670518088dbff3ccc289c0`. Planning began 2026-10-12; binding/execution continue 2026-10-13 local time. Root controls tests/gates/Git; no services or provider calls.

**Goal:** Catch a narrow alternating sequence of successful single-call rounds that M27's consecutive identical-result streak does not see. Add one fixed advisory on the already-next model request while preserving every actual call, result, and control boundary.

**Recommendation:** Adapt only the bounded period/lap comparison from the cached public Hermes helper. Native behavior stays advisory-only. No blocking, suppression, stubbing, replay, automatic recovery, or effect inference.

## Evidence and motivation

- M27's `SuccessfulResultStreak` fingerprints one fresh physical call plus its complete bounded plain-text result. Three identical consecutive fingerprints earn one factual hint.
- That detects period 1 (`A,A,A`). An alternating sequence (`A,B,A,B,A,B`) resets the M27 streak every round and is not recognized as repetition.
- The cached M23 public file `.superpowers/sdd/2026-10-12-companion-agent-loop-m23/upstream-tool_guardrails.py` has `_detect_identical_cycle`, which examines trailing periods 2–4 and matches complete call/result identity per position across three laps.
- The upstream implementation also uses a broad call history, trusted poller exemptions, result stubs, blocking, and halt policy. Do not copy those Hermes-specific behaviors or claim full upstream parity.
- Native has no reliable poller/effect taxonomy. A warning must explicitly allow necessary polling and keep every existing call possible.

## Binding behavior proposal

1. Observe only one fresh, physically dispatched, settled tool call in a round. Reuse the complete eligibility and fingerprint from M27: valid bounded tool name/args; `Success`; no error; nonempty bounded plain text; no nonempty metadata or attachments; and not recovered.
2. Treat that per-call fingerprint as an opaque 32-byte value. Call ID and duration remain in real witnesses but stay out of identity, matching M27.
3. Keep at most 12 fingerprints: maximum period 4 × three complete laps. This is enough to evaluate all candidates without storing raw arguments, text, IDs, or results.
4. Check periods 2, 3, then 4 against the last three complete laps, position by position. Select the smallest matching period.
5. Require at least two distinct fingerprints in a candidate. Constant `A,A,A,...` remains period 1 and belongs exclusively to M27; it cannot produce a second cycle hint.
6. Permit one total cycle advisory per invocation. A `warned` bit remains set after candidate-history resets; a different or rotated cycle later in the same run cannot flood the model with additional hints.
7. Invalid/ineligible evidence, errors, recovered calls, multi-call/parallel rounds, unconfigured-call refusals, evaluator revisions, and intervening non-tool responses clear candidate history. The per-invocation `warned` bit is not cleared by these resets. A new invocation/restore resets both.
8. Do not filter by tool name. Unknown-to-the-detector tools count only if existing configured-tool dispatch actually ran and returned eligible evidence. M25 refusal pairs did not execute and therefore reset state.
9. Do not stop or alter a repeated sequence. After advice, the next ordinary model request may continue with any already-authorized tool. No call is stubbed, muted, repeated, or reauthorized by this feature.
10. Use this exact fixed message, with no interpolation: “The same sequence of tool calls has returned the same results three times. If those results are sufficient, use them; otherwise change the inputs or approach, or continue necessary polling.” Never say “stuck,” “no progress,” “wasted,” or that no effect occurred.

## Loop integration

- Implement a private observer beside the existing M27 logic in `runtime/failure_guard.rs`; reuse M27's bounded fingerprint rather than introducing a second identity scheme.
- In `runtime.rs`, feed it only after the awaited result vector settles and only when the vector contains one fresh call. Parallel rounds reset candidate history.
- The third identical lap sets a pending advisory. Append it once to trusted system guidance on the already-next request after the existing Stop checkpoint.
- At the M23 cap, guidance may accompany the existing no-tools budget wrap-up. Keep its empty tool config, `max_retries: Some(0)`, and single-call accounting unchanged.
- If Stop, provider failure, deadline, or receipt/storage failure prevents continuation, no advisory-specific request occurs. Existing terminal behavior wins.
- M24 exact-failure advice, M27 identical-success advice, M25 refusal pairing, M26 transport retry policy, and run state remain otherwise unchanged.
- The advisory is transient system guidance. Never persist it in conversation history, receipts, run metadata, or agent settings.

## Bounded implementation sequence after root binds

1. Confirm M28 PR #34 is merged and the checked-out tree exactly matches the
   verified latest `main`. Preserve the unrelated `nx.json` change.
2. Read the existing M27 observer and settled-result seam only. Confirm no
   intervening source change altered its eligibility, fingerprint, or resets.
3. Add a runtime test with existing helpers for the missing period-2 hint; root observes meaningful RED before production. Add compact pure tests for the three-lap period detector and
   one-total-warning latch during implementation.
4. Add the smallest private observer using M27's existing digest and a bounded
   twelve-element history. Avoid a new serialization path or name allowlist.
5. Wire exactly one eligible `tool_results` item per round in `runtime.rs`;
   all other result shapes reset candidate history and preserve actual outputs.
6. Add runtime tests for transcript/frame/DB pairing, budget wrap-up, Stop,
   recovery, refusal, parallel, and evaluator boundaries before broad validation.
7. Conduct one native/core/harness contract review. Apply at most one bounded
   fix wave to actionable findings; do not launch separate overlapping Cargo.
8. Run the root-owned aggregate test and lint targets once, after the source is
   frozen. Record actual suite counts and ignored tests from logs.
9. Update attribution and companion docs only after the implementation passes
   review and gates. Keep this planning draft ignored and out of publication.

This sequence intentionally has no source-only implementation agent action
before root's RED observation and binding. It does not require a new worktree,
separate audit, new approval pause, or a live provider call.

## Budget implications

The default tool-round limit is eight. A period-2 cycle needs six single-call rounds and can reach three laps within that default; its hint appears on the next already-scheduled request, which may be wrap-up at the cap.

A period-3 cycle needs nine rounds and a period-4 cycle needs twelve. Those patterns cannot reach threshold under the default eight-round cap. Test them through the existing owner-configurable higher cap without silently changing defaults or auto-extending the run.

Zero rounds allow no tool observation and should produce no cycle hint. No test or acceptance path should raise a limit solely to force a live-provider scenario.

## Bounded state details

- Candidate history contains at most twelve `[u8; 32]` fingerprints; no map keyed by tool names and no persistent storage.
- One boolean tracks whether this invocation has already emitted the cycle advisory.
- Period/lap comparisons operate only on the digest window and return the smallest qualifying period.
- A period-1 constant sequence is rejected by requiring two or more distinct fingerprints in the candidate window.
- A rotated starting point of a valid period remains detectable because matching compares the trailing laps in their observed order.
- After the single hint, later cycles in the same run remain advisory-silent even when candidate history resets and refills.
- Existing M27 byte/depth/node limits and canonical finite argument handling remain the only input-boundary rules.
- Hash or bounds uncertainty clears candidate history; it never changes actual tool results or arguments.
- No new dependency, feature, public setting, descriptor field, DTO, endpoint, SDK/web surface, or general loop-guard framework.

## Files and focused test expectations

| File                                                                | Intended bounded work                                                                                 |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `packages/core-rust/crates/anima-core/src/runtime/failure_guard.rs` | Private cycle state, smallest-period detector, fixed hint, bounded pure tests.                        |
| `packages/core-rust/crates/anima-core/src/runtime.rs`               | Per-invocation wiring at the settled one-result seam and next-request hint assembly.                  |
| `packages/core-rust/crates/anima-core/src/runtime/run_tests.rs`     | Scripted call/request/frame/DB/cap/Stop boundary regressions.                                         |
| `packages/core-rust/crates/anima-core/HERMES-NOTICE`                | Retain complete MIT license and add only accurate narrow attribution if adapting the pattern.         |
| Existing companion plan/master/daily-use/handover                   | Update only after reviewed GREEN and root's evidence decision; distinguish advisory from enforcement. |

Minimum meaningful cases:

1. Periods 2, 3, and 4: no hint before three complete laps; one hint exactly at the threshold.
2. Period-2 sequences starting in either phase (`A,B,...` and `B,A,...`) are recognized; lap comparison remains order-sensitive.
3. Constant `A,A,A,...` remains M27-only; no cycle hint duplicates the M27 single-call hint.
4. Same call fingerprints with fresh IDs/durations still match; emitted frames/results retain their original IDs/durations.
5. One changed name, canonical argument, or complete result breaks the candidate; require three new laps to qualify.
6. An ineligible result (Error, inconsistent status, missing/empty/oversized text, nonempty metadata/attachments, malformed/nonfinite/oversized args) clears candidate history.
7. Recovered result, multi-call/parallel round, M25 refusal, evaluator revision, or intervening text response clears candidate history; none counts as a physical observation.
8. One total warning per run: after warning, a continuing cycle or later rotated/different cycle cannot produce another hint. A fresh invocation can warn once again.
9. Necessary polling and an eligible unknown-to-the-detector name remain dispatchable under existing grants; the advisory never blocks or stubs the action.
10. Original tool call/result pairing, tool frames, DB step rows, action receipts, and round counters are unchanged.
11. At cap 0, no hint is created. At cap 6, a period-2 hint can accompany only the existing no-tools wrap-up; no additional request/tool action is added.
12. Stop before the next request, provider failure, or receipt/storage failure suppresses continuation and preserves existing terminal behavior.
13. Existing M24/M27 tests still pass and their own hints remain distinct; this is not a redesign of their policies.

Compact expected fixture matrix:

| Fixture                                                | Expected detector result                                                       |
| ------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `A,B,A,B,A`                                            | No cycle hint; only two complete period-2 laps exist.                          |
| `A,B,A,B,A,B`                                          | One period-2 hint after the sixth eligible result.                             |
| `B,A,B,A,B,A`                                          | One period-2 hint; the starting phase does not matter.                         |
| `A,B,C,A,B,C,A,B,C`                                    | One period-3 hint after nine eligible results.                                 |
| `A,B,C,D` repeated three times                         | One period-4 hint after twelve eligible results.                               |
| `A,A,A,A,A,A`                                          | No M29 cycle hint; M27 owns the period-1 advisory.                             |
| Same args/name but changed full result at one position | Candidate breaks; no match using an earlier partial lap.                       |
| Same sequence after fresh run IDs and durations        | Digests still match; real IDs and durations remain unmodified.                 |
| Three laps followed by continuing same cycle           | One total advisory for the invocation, no repeated prompt hint.                |
| Hint followed by a different cycle and then three laps | Still one total advisory; warned state survives reset.                         |
| Invalid result or recovery between laps                | Candidate history clears; recovered result never fills a missing lap.          |
| Parallel batch or M25 refusal between laps             | Candidate history clears; refusal has no physical dispatch witness.            |
| Stop or receipt failure after detection                | No extra call; existing stopped/storage failure outcome remains authoritative. |
| Period-2 at `max_tool_iterations=6`                    | Hint may be in the existing no-tools wrap-up only.                             |
| Period-3/4 at explicit cap 9/12                        | Detector reaches the bound without changing default or extending the run.      |

These examples are acceptance targets for future source tests, not claims that
M29 has been implemented or that any model would follow the fixed advice.

## Five binding risks for root

1. **Advisory versus enforcement:** Preserve warning-only behavior. No copied Hermes stop/block/stub semantics, result replacement, tool suppression, action replay, or effect-safety inference. If this value requires a trusted poller/effect taxonomy, defer it.
2. **Identity and eligibility:** Bind exact reuse of M27's full bounded successful-result fingerprint. Only fresh, single, physically dispatched results count; ambiguous or structured evidence resets. Do not classify by model-generated status or tool name.
3. **Threshold and cost:** Three complete laps, periods 2–4, one total hint per invocation. Period 3/4 requires an already-supported higher round setting. Never change default 8 or extend a run automatically; reported usage is not a hard billing cap.
4. **Privacy and memory bounds:** Limit history to twelve digests plus one warned bit. No raw input/result retention or interpolation in trusted guidance; no broader map/ring or persistence.
5. **Control precedence and compatibility:** M23 budget wrap-up/no-tools, M24/M27 warnings, M25 refusal, M26 retries, Stop/deadline/storage/recovery continue to win. Keep callback, frame, DB, receipt, result, and model/tool-call counts honest.

For each binding decision, prefer the conservative failure mode: miss optional
guidance, leave the native action/result untouched, and continue under existing
authority. A detector defect may cost an advisory; it must not create an effect,
hide an effect, or change the run's terminal status.

## Root verification after binding

- First observe one meaningful RED in the existing core runtime tests: an alternating period-2 sequence reaches three laps without the new hint. Fixtures must use the existing scripted adapter/callback helpers and require no new production symbols.
- Add compact pure tests for period selection, rotated start, constant period-1 rejection, threshold timing, single-warning latch, and bounded/reset state.
- Use the existing verified Nx filtering convention for focused tests; do not guess flags or launch an independent Cargo process.
- Review the runtime/core/harness contract once, apply at most one bounded fix wave, then run the aggregate Rust host test target once. `bun x nx run rust-daemon:test --skipNxCache` already includes the reusable core tests; do not duplicate a full core aggregate gate.
- Run the existing Rust daemon lint target after confirming the current project target/flags. Root serializes Cargo with `CARGO_INCREMENTAL=0` and uses only existing target directories.
- No SDK/web gates unless their source or public contract changes. No live model request is needed to establish the deterministic detector contract.
- Any optional usefulness check must be separately bound as one authorized isolated task; no provider grid, repeated prompt, retry loop, or attempt to make periods 3/4 fit the default budget.

The focused test report should distinguish:

- Pure fingerprint-window behavior from runtime integration behavior.
- The existing M27 period-1 hint from the new period-2/3/4 advisory.
- A real dispatched action from an M25 validation refusal or recovered result.
- The default-eight-round period-2 reachability from period-3/4 tests using
  explicitly configured existing higher limits.
- The unchanged no-tools wrap-up from any ordinary model request after a hint.
- Preserved results/frames/receipts from the fact that trusted guidance was
  present in a request system string.

Do not turn these checks into a provider-response rubric. The model may ignore
the advisory; success means the native runtime reported it once at the right
boundary and left call authority, actions, transcript, and budgets unchanged.

**Canonical plan:** This root-bound document governs execution. The ignored planner draft remains historical; the controller rulings and ownership below are binding.

## Controller rulings on the risks (binding)

1. Ruling: Adapt only trailing three-lap periods2–4 comparison, with at least two distinct complete fingerprints and one total fixed cycle advisory per invocation — identical results establish repetition, not no progress or effect absence; polling remains permitted — cost if wrong: optional advice may be unhelpful, never block/suppress/stub/replay/reauthorize calls.
2. Ruling: Reuse M27 complete bounded typed Success/plain-content fingerprint and retain at most12digest32 entries plus warned bit — full finite canonical args/name/text must match, no preview/raw data in trusted advice — cost if wrong: omit advisory on uncertain evidence; bounds/privacy regressions block publication.
3. Ruling: Ineligible/error/recovered/parallel/refused/evaluator/text boundaries clear history, preserving the once-warned latch; fresh invocation or restore starts fully empty — do not join cached or unrelated physical evidence or flood rotated/different cycles — cost if wrong: miss a helpful notice rather than change effects or persist private state.
4. Ruling: Insert the exact fixed hint on the already-next request only, including existing cap6 tool-free wrap-up; all Stop/provider/deadline/receipt failures win and no extra request or budget extension — default8 permits period2, periods3/4 only existing explicit larger caps — cost if wrong: native request/config/frame/pair/DB accounting tests and full gate must fail before merge.
5. Ruling: Source-only lean implementer, one native batch and final public-contract review, one fix wave then host aggregate test+lint — stable SDK/web/host contracts require no extra gates or live provider task; user autonomy supersedes skill approval/worktree/audit defaults — cost if wrong: bounded source fix and rerun affected checks, no costly provider test loops or unsupported parity claim.

## Root execution ownership

One worker owns failure_guard.rs/runtime.rs/runtime/run_tests.rs. First add a
runtime assertion exercising six alternating fresh physical successes using only
existing helpers; root observes meaningful missing-hint RED before production.
Then implement and compact pure/runtime matrix. No independent Cargo/Nx/services/
Git/format loop from worker. Root owns notice/docs/ledger/serial CARGO_INCREMENTAL=0
existing-target focused and required gates, explicit publication and standing
reviewed merge. Preserve nx.json and all prior synthetic stores/settings. Full
native host test target already includes core dependencies, no duplicate aggregate.
No public settings/DTO/endpoints/SDK/web/host source changes intended.

## Verified result

Seven compact new tests (three pure, four runtime) pass after the meaningful missing-hint RED. One native batch review and final SDK/web contract review found no actionable defects. Required host aggregate passed 2,411 tests, eight ignored, across 40 suites; formatting passed on the same frozen source. The initial restricted-network mock-HTTP failures are retained in ignored logs; the unchanged authorized rerun passed. The controller also recorded one read-only formatting-check scheduling overlap and returned to strict serialization. No source fix, SDK/web/host contract change, service or provider request was needed. This verifies deterministic advice and preserved controls, not model responsiveness or full upstream parity. Exact source/PR/merge and next resume are in the milestone ledger.
