# M85 Live Research Acceptance — Lean Delta Plan

**Goal:** Run one distinct Rust 2018 research task through the actual model, native tools, cited report, explicit read-back, and quiet same-store restart.
**Architecture:** Copy and adapt the M84 private driver. Use existing typed live events plus full pending approvals for in-flight admission; use persisted full content for final grading. No production runtime, SDK, web, dependency, or guard change.
**Execution:** Root binds the five risks below before work, owns all commands, gates, releases, Git and live activity. Workers own source only. One combined source batch and final SDK/web review, then one fix wave; no separate preflight audit or repeated check-ins.

## Evidence and preserved boundaries

- Base: merged M84 `579549d`; branch `g9000/companion-live-research-m85`.
- M84 remains FAILED: one model call, 1,417 tokens, zero search/fetch/write; quiet restart and cleanup passed. Never replay, repair, or regrade it.
- Preserve M84 source snapshots, consumed releases, two archived zero-dispatch attempts, and all earlier outcomes.
- Current `agent_runs.rs:2052-2066` publishes committed history after durable completion. Missing saved Assistant calls during an approval is expected, not evidence of an absent model call.
- Existing `live/observer.rs:207-273` publishes correlated tool start/finish events and host-resolved successful search sources.
- `live/mod.rs:28-29` limits previews to 2 KiB; `approvals/registry.rs:137-148` retains at most 16 KiB of argument JSON with an explicit truncation flag.
- SDK `events.ts` already exposes run/session/step/call identity, sequence numbers, approval events, resync, and search sources. Do not extend it.
- Reuse current Anima admission, oracle, SDK, native tools and unchanged OwnedJob. An unrelated Hermes port or new lifecycle framework adds no needed capability here.

## Ownership and smallest public delta

- Public: `packages/core-rust/acceptance/research-m84/oracle.mjs`, `oracle.test.mjs`, and a short README clarification in that directory.
- Keep the existing `core-rust:research-acceptance-test` target; no project-target change unless root finds an actual need.
- Private M85: copy M84 `owner.ts`, `owner-policy.ts`, `owner-policy.test.ts`, `service.py`, and `controller.py`; adapt only the new task, port, live evidence and grading seams.
- Root owns new release/oracle/freeze/result manifests and the actual driver execution; preserve the copied-source provenance and original hashes.
- Pure admission/projection tests belong in private `owner-policy.test.ts`; no new Rust test module or production test seam.
- Existing `ChatScreen.test.tsx` exact-artifact seam consumes an M85 artifact via the existing environment override. Do not edit or regrade M83 evidence.

## Public oracle interface

- Keep `evaluateResearch(input)` and its existing result shape and failure/partial distinctions.
- Validate `expectations.path` as a bounded simple relative Markdown filename; reject separators, traversal, drive/ADS syntax, controls, empty values and nonstrings.
- Root freezes `rust-2018-research.md`; parameterization does not authorize arbitrary workspace paths or change the native write boundary.
- Use the frozen expected version in the existing narrow negated-version check instead of hardcoded `1.85.0`; escape it before regex use.
- Validate the expected stable version's bounded numeric format before constructing checks. Keep Rust 2024 fixtures passing unchanged.
- Preserve visible-token, entity/whitespace, exact file hash/bytes, real numbered read-back, causal rounds and finite restart/cleanup checks.
- Do not create a generalized truth classifier. Root still reviews substantive prose and limitations against independently frozen official facts.

## Live evidence and admission interfaces

- Define a pure reducer over the existing `AgentEvent` union, with fixed case/agent/session/run identity and a bounded complete event prefix.
- State records stream identity, last sequence, healthy/invalid status, start/finish records, requested/resolved approvals and consumed call/approval identities.
- Initial snapshot must be fresh and quiet before task submission. It establishes the stream only; it cannot fabricate earlier causal events.
- Require contiguous sequence numbers over **all** events, not just selected tool events. Resync, reconnect/snapshot replacement, gaps, duplicate sequence, malformed or mismatched evidence invalidates admission.
- Correlate each tool by agent/session/run/step/call/name; exactly one start and at most one finish. A finish without its matching start refuses.
- Unknown tool, recovered/retried call, duplicate call identity, changed arguments or unexpected phase refuses; no best-effort reconstruction from history or preview.
- Keep the watch connected before submit and throughout the run. Publish a bounded complete-prefix checkpoint atomically so a decision cannot read a half-written JSONL event.
- A decision snapshots the immutable event prefix/hash, full pending DTO, matching requested event, current release/identity, prior decisions and computed verdict **before** sending AllowOnce.
- A racing watcher that has not yet recorded the requested event cannot grant. Wait only within the existing task deadline for the same stream prefix, with no new task/model call.
- Re-read the exact pending approval and match identity, revision, expiry, class and full arguments to the captured request; use the existing revision-bound decision API once.
- Preserve uncertain-decision handling: retain the attempt, reconcile only by GET, never repeat an allowance or silently seed evidence.

## Full arguments versus previews

- The pending approval's nontruncated argument JSON is the full authority input; parse a nonarray object and enforce exact known keys and task bounds.
- For an untruncated start preview, require exact serialized argument equality using the daemon's observed serialization representation.
- For a truncated preview, require the same full pending JSON to reproduce the native UTF-8-safe 2 KiB prefix and truncation flag; `live/events.rs:336-344` adds no ellipsis. Do not parse incomplete preview JSON or fill in missing content.
- This permits a legitimate report larger than 2 KiB only when the complete pending JSON remains within 16 KiB and is explicitly nontruncated.
- JSON escaping can exceed 16 KiB even for an 8 KiB report: refuse that approval honestly rather than pretend the report bound guarantees full arguments.
- Never use a live result preview as a complete fetched page, report, or read-back. Search source authority comes only from the successful native finish's typed sources.

## Causal phase rules

- Search: one exact query, `num_results: 3`, matching start/request and Network Ask; no prior search decision or finish.
- Fetch: one or two distinct returned official Rust URLs, matched after fragment-only identity normalization; preserve complete original URLs in evidence.
- Each fetch start must occur after the successful search finish and in a strictly later model step. A guessed same-round URL is insufficient even if search later returns it.
- Require exact URL and integer `max_length` from 1 through 20,000. Reserve each allowed URL/call immediately so concurrent or unfinished decisions cannot duplicate it.
- Write: one exact relative report path and bounded content, after all selected fetches succeeded in prior steps. Reject outstanding fetches, same-round fetch/write, second writes and unknown calls.
- Read-back: existing Read-class native call needs no approval. Observe the later-round exact path/offset 0/limit 200; final persisted evidence must prove full successful read after write and before final.
- Final: later model round, no subsequent tool/write, real report path and actual fetched citations. Missing or uncommitted content remains failed/partial under the oracle, never fabricated from live previews.

## Distinct actual task and fixed limits

- Freeze one query: `site:doc.rust-lang.org/edition-guide Rust 2018 first stable release edition Cargo.toml cargo fix migration`.
- Ask for official Rust 2018 minimum stable release, manifest edition entry and assisted migration command, with limitations; do not supply expected answers to the model.
- Root independently verified and will freeze expected facts `1.31.0`, `edition = "2018"`, and `cargo fix --edition` before release. The Rust 2018 index and current transitioning guide cover the facts in two fetched sources; equivalent actual official pages remain allowed.
- Output: `rust-2018-research.md`, at most 8,192 UTF-8 bytes/100 lines, the existing three-row cited report format, explicit native full read-back and compact linked final.
- Fresh private workspace/store and port **18785**; never use an ordinary profile, daemon, database, account configuration or service.
- Existing subscription resolver only; one catalog-selected candidate and one task attempt. No login, fallback, model trial, helper, retry, steering, extra search or manual report repair.
- Explicit grants remain `web_search`, `web_fetch`, `write_file`, `read_file`; Network/Write Ask with exact AllowOnce, Exec/Delegate Deny and no persistent/session allowance.
- Preserve `maxToolIterations: 5`, `maxRetries: 0`, `maxTokens: 2048`, per-request 30 s, at most six primary calls and zero secondary generation.
- Preserve 240 s task deadline including owner approval time; settings are not a billing cap. One shared Stop attempt, GET reconciliation and physical owned cleanup on expiry/uncertainty.
- Preserve bounded watch/logs, one-use startup/catalog/task markers, source/binary closure, fresh sentinel and exact root-owned release identities.

## Final grading, restart and evidence

- After terminal durable commit, capture actual persisted full Assistant/Tool content and taskResult metadata, receipts, decided approvals, usage and final report bytes/hash/mtime.
- Do not assume all full tool text lives in one top-level text field; use the current persisted DTO/content representation and reject missing/mismatched status or identity.
- Reconcile every admitted live call/result with persisted records and usage; preserve actual result ordering, not future-result lookups projected into earlier rounds.
- Maintain actual call/token counts and distinguish admission failure, provider failure, incomplete model work and valid research success.
- Quiet flush, capture canonical state, retire first daemon, and restart once with the same isolated store without new input or metadata refresh.
- Observe five finite quiet seconds; compare report bytes/mtime, messages, source links, runs, receipts, usage, config/grants and memory as before.
- Require owned process retirement, logs joined, Job empty and port free; unknown cleanup cannot become success.
- Save an M85-only console artifact from actual live events and reloaded messages; renderer proof covers source links, not independent model reasoning verification.

## Meaningful deterministic tests and sequence

1. Tests first: no saved Assistant history yet, valid live search-start/request admits exactly once; the copied M84 history-dependent policy must fail this control.
2. Correct full write args over 2 KiB plus matching truncated UTF-8 preview admits; wrong prefix, wrong flag, malformed/truncated pending JSON and escape-induced 16 KiB overflow refuse.
3. Gap, resync, duplicate/missing start/finish/request, wrong run/session/step/name/revision and stale pending approval refuse; import-only driver remains request/mutation free.
4. Successful search finish then later-step selected fetch admits; same-step/pre-finish/unreturned/nonofficial/duplicate third fetch refuses, including already-approved unfinished fetches.
5. All selected fetches succeeded then later-step write admits; outstanding/error/same-step fetch, second write, unknown tool or changed full args refuses.
6. Actual-shaped persisted full content/taskResult projects complete evidence; missing, reordered, mismatched or preview-only content cannot pass read-back/provenance checks.
7. Rust 2018 oracle positive and wrong/negated version, filename escape/mismatch controls; retain unchanged Rust 2024 deterministic fixtures without replaying historical cases.
8. Existing timeout/Stop/one-use/import/quiet-restart controls remain; add only a narrow changed-seam control where needed, not another lifecycle framework.

- Root retains meaningful RED, permits minimal source implementation, runs focused GREEN, freezes sources, obtains the two bounded reviews, applies one combined wave and runs final gates.
- Only after gates and root oracle/model releases may the distinct actual task run once; publish its actual outcome even if failed or partial. No corrective second attempt.

## Root-only commands and gates

- `bun x nx run core-rust:research-acceptance-test --skipNxCache` (existing target, all old/new deterministic oracle cases).
- Copied M85 controller `driver-tests` and `driver-check` modes run private pure-policy tests and bundle/import checks; adapt exact paths before execution.
- Use existing web test target for the immutable M85 console artifact after the actual case; retain the current opt-in skipped result before an artifact exists.
- Root runs changed-file formatting checks and the existing required Nx gate set; no worker Cargo/Nx/service execution.
- If a required Rust gate is run, serialize with `CARGO_INCREMENTAL=0`, build jobs 1, test threads 2 and the isolated validation target. No extra compile loop for unchanged runtime.

## Top five risks — root binding rulings required

1. **Live versus durable authority:** bind contiguous live evidence to pending approval for admission, persisted full content for final truth; no substitution between them.
2. **Preview/full-argument mismatch:** bind native UTF-8 prefix reproduction and 16 KiB truncation refusal, including large writes and JSON escaping.
3. **Causality and duplicate allowance:** bind strict finish-before-later-start/step rules, reserved unfinished calls and immutable decision snapshots.
4. **One-use cost and cleanup:** bind one candidate/task, deadlines/Stop reconciliation, no retries and quiet owned restart; unknown evidence stays unknown.
5. **Oracle reuse and historical integrity:** bind safe filename/version parameters, distinct Rust 2018 source oracle, and preservation of M84/M52 outcomes; no generalized semantic classifier or scope expansion.

## Controller rulings on the risks (binding)

1. Ruling: use one contiguous already-connected typed stream plus the exact full pending approval for live admission, and terminal persisted full text for final grading — ordinary saved calls arrive only after commit — cost if wrong: missing/mismatched/resync evidence refuses and the distinct case fails honestly. Preserve original requested/resolved DTOs, sequence and all run/session/step/call/name identities. No runtime/SDK event change or reconstructed history authority.
2. Ruling: reproduce the native exact UTF-8-safe2KiB prefix with no ellipsis against full pending serialized JSON; require nontruncated pending arguments within16KiB — an8KiB report can have a longer escaped JSON representation — cost if wrong: the write is refused without a model retry. Previews establish only corroboration; no preview-as-full-page/report/readback claim.
3. Ruling: require prior successful finish and strictly later start/step for search→fetch→write→read/final; reserve approved unfinished identities and save immutable decision-time prefix/full-DTO/verdict before one revision-bound POST — late results and repeated decisions cannot establish earlier knowledge — cost if wrong: the next case remains nonpassing. Atomic complete-prefix watcher checkpoints avoid half-written JSONL reads; a missing requested event permits bounded same-prefix waiting only within the fixed task deadline. Uncertain POST is GET reconciliation only.
4. Ruling: choose one fresh listed subscription model for one distinct Rust2018 task on isolated18785, max six primary calls/five tool rounds/zero retries or secondary producers and240sec including approvals — no model ladder or old-case improvement loop is authorized — cost if wrong: retain the one actual outcome. Existing resolver only, no login/raw-credential/profile copying/account change/normal services/connectors/DB servers/containers/Docker55432. Disable title/compaction/profile/learning/embeddings/query expansion; deterministic reflection-memory retention is allowed without generation/revision. Owned shutdown and five-second quiet reload prove this finite cancelled/completed case only; settings/accounting are not hard billing caps.
5. Ruling: minimally parameterize the existing oracle's root-frozen simple Markdown filename and expected numeric version, then use independently checked official Rust2018 facts — a different question avoids replaying the consumed Rust2024 task while reusing the same three-row report contract — cost if wrong: oracle controls or actual source sufficiency fail before a quality claim. Root verified rust-2018/index.html1.31.0 and the current transition guide's explicit2018 manifest/assisted command example; no expected values enter the model prompt. Keep original M84 raw source/runtime/failed case and preflights, M83 and M52 outcomes immutable. Substantive prose is independently root-reviewed; no full Hermes parity or universal semantic classifier.

Ruling: retain the lean delta, source-only Codex workers, one batch plus final SDK/web review, one combined fix wave and root-only validation/publication — standing user instructions prioritize progress and low cost — cost if wrong: exact ledger/source/evidence and unsigned explicit-path PR publication remain reviewable. No new dependencies or runtime behavior needed. Existing typed tools/SDK/OwnedJob are direct reuse; M83's attributed Hermes behavior stays preserved, with no new port required.
