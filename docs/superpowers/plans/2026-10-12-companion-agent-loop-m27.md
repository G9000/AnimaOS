# M27 Repeated Successful Result Advisory — Implementation Plan

**Status:** advance planning only; root binds after M26 merges. No source changes, tests, gates, services, provider calls or Git activity are authorized by this draft.
**Goal:** Give one factual advisory after three consecutive fresh single-call successful rounds return the same bounded result for the same tool/canonical arguments, preserving actual results and all existing authority/budget boundaries.
**Architecture:** Add one private per-invocation success-streak fingerprint beside M24's failure streak at the existing settled-result seam. Deliver a fixed transient system hint only on the already-next model request.
**Technology:** Existing Rust core, SHA-256 and JCS dependencies, existing typed `TaskResult<Content>`, scripted tests and native Nx gates.

## Recommendation and value judgment

Proceed only as **repeated-success advice**, not a “no-progress detector” or general cycle guard.
Identical successful output cannot prove lack of progress: repeated writes may each have effects, reads may intentionally confirm state, and polling may legitimately remain unchanged.
A factual hint after three rounds can redirect a wasteful repeated read before the existing default eight-round cap, leaving several rounds to change approach.
This is a modest usefulness claim, not a reason to broaden tool semantics, block calls or reduce the cap.
The model remains free to continue necessary polling or repeated work; final M23 wrap-up still uses actual settled results.
If root requires inferred progress, automatic suppression, hard stopping or complete A/B-cycle parity, **defer M27** until a separate trusted capability/polling policy exists.
Suggested fixed guidance: “This same tool call has returned the same result three times. If that result is sufficient, use it; otherwise change the inputs or approach, or continue necessary polling.”
No argument, tool name, call ID, result/error text, provider detail or private content is interpolated into trusted guidance.

## Accurate public comparison

- Cached tracked public Hermes HEAD is `1fd18277267152893a8a24792b79213e7ca3f899`; source is the M23 ignored `upstream-tool_guardrails.py` only.
- `ToolCallGuardrailController.observe_call` at lines 441–503 tracks consecutive identical signature/result hashes for plain-string results; any different result/call resets that streak.
- Its notices use `STALL_GUARD_IDENTICAL_CALL_THRESHOLD=3`, but pollers are exempt through `is_stall_guard_repeatable` and known name/suffix conventions.
- `observe_call` also offers identical-result reference stubs for sufficiently large successful strings; it may halt when configured hard-stop policy reaches its threshold.
- `_detect_identical_cycle` at lines 505–533 uses a bounded call-history window and periods 2–4, including poller-only cycle exemptions.
- `after_call` at lines 425–438 has a separate idempotent-tool successful-result repeat count, based on configured mutating/idempotent names and per-signature state.
- Those mechanisms are not interchangeable: M27 chooses the consecutive single-call observation pattern, not the idempotent-name counter, reference stubbing or multi-call cycle detector.
- M24 counts typed Error/no-data results and exact error bytes; M27 would count typed Success/no-error results, avoiding Hermes string/JSON failure classification.
- Native has no trusted poller/mutation taxonomy. Do not copy Hermes names or infer effect safety from names.
- Retain existing MIT notice if adapting this pattern; describe a narrower native advisory rather than claiming full Hermes guardrail parity.
- Do not execute/import Python, inspect upstream private profiles/credentials, or copy provider/runtime framework code.

## Current native seams

- `packages/core-rust/crates/anima-core/src/runtime.rs` initializes M24 streak and pending hint inside each invocation; no streak enters snapshots.
- The settled `tool_results` vector exists after awaited executors/`join_all` and before unchanged result-to-message recording.
- M24 observes only one fresh result; recovered/parallel rounds reset its state, and fixed guidance goes into the next request's system string.
- `runtime/failure_guard.rs::bounded_arguments` checks finite values, depth/node/byte limits before canonical serialization; M25 reuses it without changing original arguments.
- M25 validation refusal bypasses physical execution and explicitly resets M24; M27 must also reset there, never count static refusal as a successful executed result.
- M23 switches the already-next call to no-tools wrap-up after the cap; pending advice can accompany that existing request but cannot extend the budget.
- `runtime/control.rs` and M26 transport policy remain authoritative; provider attempts are not successful physical tool rounds and cannot increment M27.
- `TaskResult<Content>` has status/error/data/duration; `Content` contains text, optional attachments and optional metadata with nested `DataValue`.
- Duration and provider call ID are observation details, not success identity; tool name, canonical args and eligible canonical result form the fingerprint.

## Narrow eligibility recommendation

For the first implementation, prefer **plain-text success only**, matching the usable small Hermes observation seam without building a result-classification framework.
An eligible round contains exactly one fresh physical `ToolCall` and returns `TaskStatus::Success`, `error:None`, `data:Some(Content)`.
Require nonempty bounded text, no nonempty attachments and no nonempty metadata; structured/multimodal outcomes reset conservatively.
Compare complete eligible text exactly, not a prefix, excerpt, redaction, semantic similarity or decoded JSON subset.
Canonicalize the complete original argument map with existing bounded helper; require nonempty bounded tool name and valid finite canonical arguments.
Validate byte/encoded-size uncertainty before allocation/serialization; over-size, invalid or malformed evidence resets without changing real outcomes.
State consists of one 32-byte digest, count saturated at three, and a warned bit; no maps/history rings/raw strings/persistent fields.
Hash a length-delimited tuple of name, canonical argument bytes and complete result text. Omit duration and call ID intentionally.
An exact repeat increments; changed name/args/text starts a new count of one. Warn once at three, then suppress while that exact streak continues.
Any Error, inconsistent Success+error, absent data, empty result, structured/attached result, recovered result or multi-call round resets.
An intervening completed non-tool response/evaluator revision resets; independent runs and restores start empty.
If root instead requires full structured `Content` identity, bind preflight size/depth/node handling for metadata/attachments explicitly before implementation; do not simply serialize arbitrary retained payloads.
Plain-text narrowing misses some useful cases safely; it avoids falsely declaring stable visible text identical when hidden structured fields changed.

## Interface and ordering

1. At the existing result seam, observe only the fresh single awaited executor result; never inspect rendered transcript text or provider-generated status metadata.
2. Feed Error observations to unchanged M24 and Success observations to M27; each kind resets the other streak.
3. Keep original `TaskResult`, call ID, arguments, tool frames, duration and receipt/persistence paths unchanged.
4. Prepare a pending success hint only after the third eligible result settles; do not emit another tool result, synthetic user message or persisted system row.
5. Deliver it once via the next request's trusted system guidance after the existing Stop checkpoint; consume the pending flag on that existing request.
6. If cancellation/provider/receipt/deadline failure prevents continuation, no advice-specific request occurs and the existing terminal result wins.
7. Preserve M23 cap-zero/cap-boundary behavior, M24 Error eligibility and M25 one-refusal recovery; do not add evaluator/model retries.

No new public settings, descriptor metadata, DTO, SDK/web field, daemon endpoint or harness provider connection is needed.
Implementation stays under reusable core; hosts keep approvals, ACLs, native receipts and exact effect boundaries.
Normal successful tool calls remain callable, including unknown-to-the-detector pollers or effectful tools.
The hint is advisory; it never serves as proof of success, effect durability, wasted work or completion of the user's goal.

## Five binding risks for root after M26 merge

1. **Semantic honesty:** Bind “same result” language, never “no progress” or “stuck.” Repeated work/polls may be necessary; advisory permits them and changes no authority/cap.
2. **Identity and privacy:** Bind exact bounded plain-text success identity, canonical args/name and digest-only state; no raw result/argument retention or reflection into trusted hints. Structured/multimodal evidence resets or awaits a separate explicit bound.
3. **Physical execution versus recovery:** Count fresh settled executor Success only; recovered DB results and M25 validation refusals reset. Parallel rounds reset rather than inventing sibling order.
4. **Control and compatibility:** Stop, native receipt failure, provider retry/failure policy, deadlines and M23 wrap-up take precedence. Preserve original results, call IDs, physical frames, receipt semantics, grant configuration and paired transcript.
5. **Value versus cost:** Threshold three/once per streak gives modest guidance within default eight rounds; no live call is needed for deterministic behavior. If requested value requires cycle detection or effect-aware suppression, defer instead of broadening.

### Root binding placeholder

- [ ] Bind only after verified M26 merge/current revision and services-off state.
- [ ] Accept narrowly factual repeated-success advice or explicitly defer the milestone.
- [ ] Bind eligible plain-text Content shape and exact conservative bounds; final fixed guidance permits necessary polling.
- [ ] Bind once-per-streak/transient behavior, M24/M25 resets and M23/Stop/transport priority.
- [ ] Keep existing dependencies/targets/directories and inherited validation workflow; no production changes are made from this draft.

## Meaningful minimum regressions

- Three identical fresh successes in separate single-call rounds: no hint before threshold, one fixed hint in the already-next request; unchanged callback/model-call counts.
- Repeats after threshold: no repeated prompt hint; counter saturates; a different name/argument/result starts a fresh streak.
- Result differences outside the beginning/end of a string matter; do not hash a preview or claim identity on a truncated result.
- Canonical argument map ordering/negative zero behave consistently with M24; nonfinite/oversized/deep arguments reset without changing executor output.
- Error, Success+error, no data, whitespace-only output, attached content and structured metadata reset; no fabricated result/marker is added.
- A real ordinary Success still contributes its original full TaskResult to paired history; assertions compare call IDs/results rather than only model final text.
- Duration and fresh call IDs change while eligible text/arguments stay identical: same streak; actual per-call IDs/durations remain intact in physical witnesses.
- Parallel success/failure batches reset; all callbacks and result pairs retain their original order and counts.
- Recovered successful DB results reset and do not create evidence of fresh work; independent invocation/snapshot restore has no lingering hint.
- M25 unconfigured refusal between two success streaks resets; there is still no callback/physical receipt for that validation result.
- M24 exact-failure tests remain unchanged and pass; successes/errors do not cross-contaminate the two counters.
- Stop after the threshold prevents the next model request; provider/receipt/deadline failures keep their existing terminal outcomes and make no advice-specific retry.
- Cap three: the pending hint accompanies only the existing M23 no-tools wrap-up; no tool executes beyond cap and no extra summary attempt exists.
- Necessary polling-style successful output gets at most factual advice, never denial/block/result stubbing; no effect-safe name allowlist is introduced.

## Bounded execution seams once root binds

- Modify `runtime/failure_guard.rs` or one adjacent private module for bounded success fingerprint/streak; no broad shared-policy redesign.
- Modify existing `runtime.rs` initialization/result observation/request guidance and reset seams only.
- Add meaningful tests in existing core runtime test modules; reuse scripted adapters/callback/frame counters and native DB-recovery fixtures.
- Keep M24 bounded helper and exact typed Error behavior intact; root reviews the combined result once rather than adding per-task audits.
- Update reusable core Hermes notice and established companion plan/docs only after actual implementation and gates.
- No harness public API/host wiring change unless a concrete regression requires one; root controls all edits and publication.

## Verification commands and limits

- Root confirms existing project/targets with `bun x nx show projects --json` and `bun x nx show project core-rust --json` before execution.
- Focused scripted regressions through the existing target/test path first; run supported arguments only, no guessed CLI flags.
- Required current-tree core/harness/adapter gate: `bun x nx run core-rust:test --skipNxCache`, existing target directory only.
- Required current-tree host dependency gate: `bun x nx run rust-daemon:test --skipNxCache`; root serializes Cargo with `CARGO_INCREMENTAL=0`.
- Relevant format/lint: existing root-confirmed core/host Nx targets; changed Markdown `bun x prettier --check <exact files>` and `git diff --check`.
- Preserve unrelated local edits, original private settings/stores, existing target dirs and services-off cleanup; no worktree/stash/reset/restore operations.
- No SDK/web gate repetition for unchanged surfaces, no new dependencies/features/targets, no model downloads, provider/account/connector tests or live calls.
- PASS demonstrates bounded factual guidance and preserved loop behavior; it does not prove detection of no progress, suppression of effects, general cycles or model responsiveness.
- Implementation, tests, gates, risk rulings and merge are all deferred until M26 completes; this draft alone changes no runtime behavior.

## Controller rulings on the risks (binding)

1. Ruling: Three consecutive fresh single physically executed typed Success/no error/plain-text results with identical full bounded text/name/canonical args get one factual hint on the already-next request — repeated output can suggest wasted work but cannot prove no progress or no effects — cost if wrong: optional advice may be unhelpful; never block/suppress/stub/replay/change outcomes or disallow necessary polling.
2. Ruling: Eligibility requires valid bounded name, existing finite/depth/node/byte-bounded arguments, nonempty non-whitespace text at most existing64KiB before scanning, and no nonempty metadata/attachments — compare complete known evidence with conservative exclusions — cost if wrong: miss a useful hint, never hash a preview or treat structured evidence as identical.
3. Ruling: Local digest32/count saturated3/warned only, length-framed domain-separated SHA256; no raw name/args/result/private text in the fixed trusted hint — reuse M24 private bounds with minimal new helper, no new framework/settings/DTO — cost if wrong: privacy/reset/bounds regression blocks merge; original content/results remain intact.
4. Ruling: Errors/inconsistent/empty/structured/multimodal/recovered/parallel/text-evaluator outcomes and M25 refusal reset the success streak, independent invocations/restores start empty; M24 behavior unchanged — successful repetition and failures are different evidence — cost if wrong: fail closed by missing advice rather than joining unrelated or cached effects.
5. Ruling: Same Stop/receipt/provider/deadline and M23 cap/no-tools/M26zero-retry precedence; append one transient system hint only after settled fresh results, no extra call/event/result/marker — preserve actual effect and request accounting — cost if wrong: strict callback/frame/paired-result/DB/budget tests and full Rust gate prevent publication; no automatic effect replay.

## Root execution boundaries

Branch g9000/companion-agent-loop-m27 cut from verified mergedM26PR32/main
4f25d044094ba6c43ecc590f48279d234ff8ba00. Draft128lines/five risks are adopted
only as factual repeated-result advice; broader cycle/progress inference deferred.
Suggested exact fixed hint above is binding. Fresh tool IDs/durations do not enter
fingerprint but retain actual execution witnesses. Empty metadata/attachments
are equivalent to absent; all nonempty structured data resets. Bound text before
trim/hash; argument bounds remain existing M24 helper. No serialization of
unbounded Content, no trusted poller/mutator name list, no cancellation changes.

One native worker owns runtime/failure_guard.rs/runtime.rs/runtime/run_tests.rs;
root owns notices/docs/Cargo/Nx/services/Git. Reuse existing scripted adapters,
result/frame/callback helpers. Add one meaningful runtime RED first without
production-helper dependency, then minimal code and compact pure/runtime matrix.
One native batch/final compatibility review, one fix wave, then required
rust-daemon:test (already includes core tests) and lint; do not duplicate core
aggregate gates. CARGO_INCREMENTAL=0; no newdeps/features/targetdirs/services or
live provider request. Source-only workers/explicit paths/unsigned reviewed
merge-and-continue remain authorized; original nx.json is preserved.

## Verified result

Implemented the controller-bound factual advice. Required Nx Rust2404passed/
8ignored/40suites and lint pass, including6compact pure/runtime cases after a
meaningful0-versus1-hint RED. One native/final contract review had no findings;
no needless fix wave. No SDK/web/API/public settings change or live provider task
for this milestone. Actual responsiveness/usefulness and broad cycle detection
remain unproven; receipt/provider priorities use unchanged inherited gate paths.
Exact tested source/PR/merge/next resume are recorded in the milestone ledger.
