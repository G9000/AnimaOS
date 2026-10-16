# M87 Fresh Coding Acceptance — Lean Plan

**Goal:** One fresh actual model repairs a duplicate-inventory utility, consults one advisory helper, runs six visible tests, reads the saved output, records truthful CAS progress, and retains the result across a quiet restart.
**Architecture:** Narrow task-specific reuse of the corrected M86 acceptance machinery. No production Rust, SDK, web, dependency or lifecycle-framework change is proposed.
**Execution:** Root binds the five risks, owns commands/releases/Git, freezes sources and authorizes one actual task. Two source-only implementation lanes, one source batch and final SDK review, one combined fix wave, required gates, honest sealing and standing merge; no check-ins or retry task.

## Scope and reuse decision

- Branch: `g9000/companion-coding-acceptance-m87`, from settled M86 main `cded043c5e81275a7a99fabca0e3712effd27c20`.
- Preserve user-owned `design/**` and all unrelated work. Do not read normal profiles, credentials, uploads or ordinary runtime data.
- M86 actual FAILED, its zero-dispatch preflight, raw evidence, source snapshots and consumed releases remain immutable. Never execute or regrade those cases.
- Reuse the **current future-corrected private M86 source**, not an older accepted snapshot, for the new M87 driver. Record source hashes and the small adaptation delta.
- Current `owner-policy.ts:55-123` recognizes one exact linked helper in the parent's contiguous stream; `normalizeCodingRoster` reads native `state.id` and `settings.additional` rather than invented flat summary fields.
- Current `buildCodingEvidence` preserves native arguments and derives separate host phase/stage fields; retain this seam and the actual-shaped projection positive.
- The existing private step validator still caps tool rounds at five (`owner-policy.ts:137-140`); M87 must consistently accept tool steps 1–7 and the tool-free final model step 8.
- Direct reuse: existing native file tools, observed-revision todo CAS, helper coordinator/inheritance, SDK routes/events and unchanged OwnedJob.
- Supported adapter: existing native `bash` runs the installed Node executable for two fixed verifier commands. No executor or protocol is added.
- Hermes public commit `1fd18277267152893a8a24792b79213e7ca3f899` is root-verified unchanged; existing Anima CAS/helper mechanisms are the cheapest fit. No new Hermes port is needed. Retain provenance/notices if substantial code is copied.
- Root ruling favors copied M86 oracle/probe mechanics with narrow names/shape/literal replacements over a generic acceptance-engine refactor. Do not duplicate unrelated research code or widen public APIs to justify reuse.
- New public directory: `packages/core-rust/acceptance/coding-m87/`; add one target to `packages/core-rust/project.json`. Existing M86 public files/targets stay unchanged.
- New private ledger: `.superpowers/sdd/2026-10-17-companion-coding-acceptance-m87/`. Copy/adapt driver, policy, tests, controller/service/release and provenance there; never edit M86 private sources or evidence.

## Small duplicate-inventory contract

- Export exactly pure `findDuplicates(records)` from `src/find-duplicates.mjs`.
- Input is an array of at most 128 records, each with exactly `{ path, bytes, sha256 }`; duplicate paths within the input are invalid.
- Paths are unique, case-sensitive ASCII relative names of at most 128 characters. Each slash-separated segment matches `[A-Za-z0-9][A-Za-z0-9._-]*`; reject `.`/`..`, leading slash, backslash, colon, whitespace and controls.
- `bytes` is an integer from 0 through 1,048,576; `sha256` is exactly 64 lowercase hexadecimal characters.
- Treat the supplied records as inventory facts; do not open paths, hash files or infer that equal hashes with different sizes are duplicates.
- A duplicate group contains at least two records having **both** identical `sha256` and identical `bytes`.
- Return exactly `{ groups, redundantBytes }`. Each group is exactly `{ paths, bytes, sha256 }`.
- Sort `paths` lexicographically using ordinary ASCII order. Sort groups by their first path; unique input paths make that order unambiguous.
- `redundantBytes` is the sum of `(group.paths.length - 1) * group.bytes`. Zero-byte duplicate groups remain visible and contribute zero.
- Omit singleton groups; empty/no-duplicate input returns `{ groups: [], redundantBytes: 0 }`.
- Do not mutate the input array or records. Use Map or an equally safe structure for arbitrary valid path/hash keys.
- Invalid input throws an ordinary Error with a message of at most 200 characters; preserve the original-length witness rather than accepting a truncated error as bounded.
- The module must have no imports, I/O, process/network access, dynamic evaluation or load/call side effects.
- Seed exactly two defects: grouping by bytes alone, and counting `n * bytes` instead of `(n - 1) * bytes`. Validation, ordering and other behavior start correct.
- Only `src/find-duplicates.mjs` is repairable, with exactly one whole-file `write_file`, at most 16 KiB source and full nontruncated approval arguments.
- `duplicates.json` is the sole CLI output. README, CLI, verifier, visible tests, inventory data, sentinel and external neighbor remain protected.

## Independent primary literal and six visible tests

- Primary inventory uses six records; each hash letter below repeats 64 times:
  - `alpha.txt`, `copy/alpha.txt`, `copy2/alpha.txt`: 16 bytes, hash `a`.
  - `beta.txt`, `copy/beta.txt`: 7 bytes, hash `b`.
  - `lonely.txt`: 16 bytes, hash `c`.
- Primary expected output has two groups in alpha/beta order with the exact sorted paths above, 39 redundant bytes, and no `lonely.txt` group.
- Freeze the full literal JSON plus one LF in `expected-cases.json`; root independently binds byte length and SHA before actual release. Do not generate expected answers by calling candidate or reference code.
- The verifier reports `counts: { groups: 2, duplicateFiles: 5, redundantBytes: 39 }` for that primary output, plus actual artifact bytes/SHA. `duplicateFiles` counts all files participating in groups, including one retained copy per group.
- Six protected visible test cases:
  1. Same size but distinct hashes does not form a group; catches bytes-only grouping.
  2. Correct group membership excludes a same-size different-hash singleton; catches bytes-only grouping independently.
  3. A three-file nonzero group has two redundant copies; catches `n * bytes`.
  4. Zero-byte duplicates sort paths/groups without mutating input; zero-byte saving remains zero.
  5. Empty, singleton and distinct-size no-duplicate inputs return no groups.
  6. Duplicate/unsafe paths and malformed records/hashes/byte values refuse without mutation.
- Design the seeded baseline for three failures/three passes; root records actual deterministic counts before binding them. Do not manufacture counts to match a guessed baseline.
- Independent literal edge cases additionally cover equal hashes with different sizes, multiple groups in scrambled input order, two vs four copies, boundaries, prototype-like path names, missing/extra fields and oversized arrays.
- Reference implementation is only a positive control; literal outputs remain the oracle authority.
- Mutants: bytes-only grouping, hash-only grouping, `n * bytes`, singleton inclusion, unsorted groups/paths, hardcoded primary output and input mutation.

## Protected verifier and candidate execution

- Copy/adapt the existing probe to require exactly the `findDuplicates` export and one records argument. Preserve bounded child timeout/output, original error overflow witness and input-mutation check.
- Preserve candidate/neighbor inventories and exact bytes/hash/mtime before and after probes, including added neighbor files and the one exact parent todo file exception.
- CLI reads fixed `data/inventory.json`, validates through the module, then writes compact JSON plus one LF to `duplicates.json`. Failed validation preserves any prior artifact bytes/mtime.
- `verify.mjs` runs exactly the six protected tests. Only when all pass does it run the protected CLI and print the summary derived from real child exit and actual output bytes.
- Preserve sanitized Node child environment, including the M86 fix preventing inherited `NODE_TEST_CONTEXT` from suppressing child tests.
- Freeze exact native Exec arguments: command `"C:/nvm4w/nodejs/node.exe" verify.mjs`, `cwd: "."`, `timeout: 10000`. Root verifies installed Node identity; adjust the declared path before freeze only if necessary.
- Baseline and final commands are identical strings but distinct witnessed phases and one-use call IDs. No third command, install, arbitrary shell or hidden-test model command is eligible.
- Baseline requires exact seeded module SHA and protected inventory; it must fail without creating/changing the artifact.
- Final requires actual failed baseline, successful helper return, successful module write, protected witnesses and root's full-source review of the exact edited module.
- Root review records finite future expiry, case ID, SHA, bytes, protected-witness hash and explicit side-effect review before execution. No malformed/missing authority defaults.
- Workspace paths and a Windows Job are not an OS filesystem sandbox. Full source review precedes final command and independent candidate probes.

## Task, helper and CAS progress

- One fresh task asks the parent to inspect README/module/tests, consult the helper, repair the two defects, run baseline/final, read full output, save progress and report only observed results.
- Give the interface, exact paths/commands and limits, but never the reference, hidden cases or expected primary hash/output.
- One exact helper, proposed name `Duplicate inventory review`; task: read README, `src/find-duplicates.mjs` and `test/find-duplicates.test.mjs`, identify defects and one edge case with paths, return advice only.
- The helper must not write files/todos, run commands or delegate. This is observed behavior, not an enforced read-only grant. Actual helper effects/pending approvals cause Stop and nonpassing evidence; do not undo them.
- Parent grants only `read_file`, `write_file`, `todo_read`, `todo_write`, `bash`, `spawn_helper`; Write/Exec/Delegate Ask, Network Deny, exact AllowOnce only.
- Three fixed todo titles: `Inspect and consult one advisory helper`, `Repair duplicate inventory`, `Verify tests and inspect output`, with frozen matching active forms.
- Require successful explicit `todo_read` before the first write in a later model round; preserve native CAS and complete-list semantics. No owner seeding, merge rescue, silent baseline refresh or retry.
- Exactly three successful complete-list saves represent inspection, repair and verification. Native arguments contain only `todos`; stage labels are host-derived evidence, not invented tool parameters.
- Inspection completion follows actual parent inspection and helper success. Repair completion follows successful module write. Verification completion follows successful final command and later full native `read_file` of `duplicates.json` with offset 0/limit 200.
- Suggested seven-round sequence: independent reads/todo read/helper/baseline; repair plus justified inspection progress; repair progress; final command after review; full artifact read; verification progress; one spare non-effect observation round if needed; final tool-free answer by model call 8.
- Spare round does not authorize another helper/write/Exec/todo save. Dependencies require earlier successful results; independent calls may share a round.
- Final states actual six-test count/pass/fail, group count, duplicate-file count, redundant bytes, artifact byte count/SHA, sole changed module, helper contribution and limits.
- Parent/helper substantive report review is root-owned. No generalized truth classifier, claims of unrun hidden tests, deployment, broad correctness or helper reliability beyond this task.

## Live driver, accounting and restart

- Use the current corrected roster normalizer and exact linked-helper participant discovery before parsing global owner frames.
- Preserve the single contiguous parent stream including linked child/session events. Reject unknown/second helpers, wrong lineage/run/session, missing/duplicate sequences, resync and helper effects; child completion does not close parent authority.
- End-to-end offline fixtures must include real native-shaped roster, linked helper session/run/read/final events and a pending parent baseline approval after those child frames.
- Pending admission uses actual starts plus full nontruncated Approval JSON and exact UTF-8 2 KiB prefix corroboration; pending JSON limit remains 16 KiB.
- Freeze immutable raw decision context, prior decisions, prefix/hash and exact case/releases before one POST. Uncertain submit/approval/Stop is GET-reconciled without resubmission.
- Terminal projection correlates full saved calls/results with live identities, receipts and approvals; preserve spawn intent < child final < successful parent spawn finish as separate observations.
- Match every usage row by agent/session/run/step/provider/model and actual source (`chat` parent, `helper` child); refuse missing/duplicate/unknown secondary usage.
- Parent `maxToolIterations: 7`, retries 0, maxTokens 2048, timeout 30,000 ms. Helper inherits seven rounds and the **30 s whole-helper deadline**.
- Structural ceiling is eight calls per agent, sixteen total; target at most twelve. Record target exceedance separately; settings and observed usage are not a hard billing cap.
- Change all private step validators, projection bounds, oracle model limits, configured-setting checks and fixtures consistently; no retained five/six-call assumption.
- Fresh isolated workspace/store/neighbor at port **18787**, two global run slots and one per agent. Existing app-owned subscription/current catalog/model selection only; no new account/login/provider fallback or model trials.
- Root task deadline **420 s including approvals**. Start watchdog at actual task attempt, use matching service cutoff and a longer bounded watcher allowance for pre-submit setup; no old 300 s cutoff or watcher expiry before the task deadline.
- At expiry or violation Stop known parent/helper once each, then retained Job cleanup. No ordinary services, profiles, accounts, connectors, DB server, Docker/container or historical workspace is touched.
- After terminal completion and history/usage flush, freeze full messages, linked runs/sessions, config/policies, usage, todos/revisions, files/hash/mtime, inventories and memory witness.
- Retire first daemon, restart the same isolated store once, issue no task/catalog refresh, observe five finite quiet seconds and compare the complete canonical snapshot including artifact hash.
- Require both physical trees retired, logs joined, Job empty and port free. This proves finite quiet retention, not active-run crash recovery or indefinite no replay.
- Seal one accepted/partial/failed outcome. Failed admission/task is not repaired into success; no second task, extra helper, artifact/report rewrite or M86 regrade.

## Implementation division and tests

1. **Public lane:** new coding-m87 fixture, reference, literal cases, narrowly adapted oracle/probe/self-tests/README and the added Nx target. Preserve old slices byte-for-byte.
2. **Private SDK lane:** copied current corrected driver/policy/tests, M87 constants and bounds, actual-shaped linked-child integration through admission and final projection. Root owns service/controller/release adaptation and freezes.
3. Both lanes write meaningful tests first; root records focused RED/GREEN. Do not force a production regression or change runtime merely to obtain a RED.
4. Public controls: seeded defects fail independently, reference/literal cases pass, all mutants fail, invalid CLI input preserves output, extra candidate/neighbor/helper-todo files fail, oversized error remains rejected.
5. Driver controls: linked child frames before pending baseline accepted; unknown/second/wrong-lineage child, child effects, closed/gapped stream, truncated/mismatched pending arguments refused.
6. Integration positive: native-shaped parent/helper DTOs and usage → immutable decisions → full terminal projection → oracle pass, including top-level phases/stages, native args unchanged, actual artifact hash and exact restart snapshot.
7. Integration negatives: helper usage source/identity/model mismatch, missing or contradictory live/saved call, child final outside spawn interval, missing/expired review, early todo completion, absent readback, wrong tests/counts/hash, changed restart and unknown cleanup.
8. Boundary controls: tool step 7 accepted, tool step 8 refused; model final call 8 accepted, ninth call refused; zero retry/fallback, one helper/write and two Execs unchanged.
9. One public/root-source review plus final SDK/web-contract review; freeze once, one combined fix wave, then root-required gates. No separate preflight audit or repeated design review.

## Root-only commands and release order

- Add and inspect target `core-rust:coding-acceptance-m87-test` using the existing noncached serialized Node-test target pattern.
- Run `bun x nx run core-rust:coding-acceptance-m87-test --skipNxCache`; retain relevant M86 `coding-reliability-test` and legacy acceptance target results once.
- Run copied private controller policy/typecheck/bundle/import-purity/Python-static targets; workers do not execute validation or actual modes.
- Required native gates: `bun x nx run rust-daemon:test --skipNxCache`, `rust-daemon:lint`, `rust-daemon:build`, serialized with `CARGO_INCREMENTAL=0`, jobs 1, test threads 2 and isolated validation target.
- Run applicable SDK/web typecheck/build, changed-file format checks and required docs gates using existing Nx targets. No dependency installation or ordinary service activation.
- Freeze reviewed source/runtime/Node identity and public literal contract; prepare fresh synthetic case offline; bind protected witnesses.
- Only then root releases isolated startup, bounded current catalog selection, exact setup and one actual task. Review the full edited module at the pending final Exec before its AllowOnce.
- Root owns bounded actual monitor/decisions, restart/cleanup, semantic final review, evidence seal and standing unsigned commits/merge. No check-in is required between authorized stages.

## Top five risks — root binding rulings

1. **Correct linked-helper stream:** bind real roster/parent-child event identity before approval; test the exact formerly blocking transition offline. Wrong/unknown child must refuse without losing the legitimate child's frames.
2. **Candidate and oracle trust:** only one pure module writable; seeded baseline and full reviewed final SHA precede execution; independent literals and protected inventory remain authoritative, with no broad shared-engine refactor.
3. **One-use authority and causal progress:** exact full pending DTOs, finite review expiry, immutable markers, observed todo CAS and prior-result completion; never retry uncertain POSTs or fabricate phase/stage arguments.
4. **Real accounting and budgets:** seven tool rounds/eight calls per agent, actual helper usage source, structural sixteen/target twelve, one helper, two slots, 30 s whole-helper and 420 s root deadline; no stale cap or misleading billing guarantee.
5. **Truthful persistence/outcome:** full artifact read precedes verification/final; root reviews actual tests/counts/hash/helper claims, canonical quiet restart and physical cleanup. One fresh outcome only; preserve every historical failure and unrelated file.

## Controller rulings on the risks (binding)

1. Ruling: retain the corrected native parent/helper stream checks, exact linked helper DTO, wrapped roster normalization and original global sequence — M86 failed by rejecting legitimate helper frames — cost if wrong: withhold approval and retain failure. Prove full native-shaped owner-to-oracle success and unknown/wrong-lineage/effect-frame refusals before the sole new model task. Never replay or regrade M86.
2. Ruling: one pure duplicate-inventory module repair, six protected visible tests, independent literal edge cases and exact report bytes — this is a distinct useful coding task and avoids broad runtime work — cost if wrong: reject the case. One module write <=16KiB, two identical Node verifier Execs, root reviews the entire actual module before final execution; no imports/eval/I/O/process/network/load-time effects.
3. Ruling: authorize each Write/Exec/Delegate once from the contiguous active stream and full pending arguments; preserve immutable decision witnesses — terminal-only saved history cannot authorize pending work — cost if wrong: deny/Stop with GET-only uncertain-POST reconciliation. Require successful todo_read and at most three fixed complete-list CAS updates grounded in actual inspection, repair and verification/read-back.
4. Ruling: exactly one advisory helper, two global slots/one per agent, seven tool rounds plus one final call per agent, zero retries,2048tokens/call,30s helper whole-run timeout and420s owner deadline including approvals — realistic parent steps need more room than M86 — cost if wrong: retain the single bounded nonpassing outcome. Structural ceiling16 model calls; target<=12, not an enforced billing cap. No model ladder, steering, rescue or final-report repair.
5. Ruling: acceptance requires actual six-test success, independent module/report/protected-file checks, complete native report read-back, completed persisted todos, substantive parent/helper prose review and five quiet same-store seconds preserving bytes/mtime/history/usage/lineage/config/memory — each claim needs independent evidence — cost if wrong: classify PARTIAL/FAILED honestly. This does not establish in-flight crash recovery, indefinite uptime, broad repository quality or full Hermes parity.

Exact task API is findDuplicates(entries), each entry {path,bytes,sha256}, returning {groups:[{paths,bytes,sha256}],redundantBytes}. Group by both hash and bytes; include only groups with >=2 distinct validated paths; sort paths and groups by their first path using ordinary string ordering. Redundant bytes are (count-1)\*bytes summed across groups; declared array/size bounds keep the total safe. Duplicate paths, malformed hashes/relative paths and invalid sizes are refused. Editable src/find-duplicates.mjs; artifact duplicates.json. Fixed Exec arguments: command `"C:/nvm4w/nodejs/node.exe" verify.mjs`, cwd `.`, timeout10000. Literal primary dataset, bytes/SHA and seeded baseline are fixed and checked offline before release, never inferred from model claims. New isolated port18787; native Windows Job guard unchanged. Existing native CAS/helper implementation is reused after comparison with installed Hermes public tools/todo_tool.py and tools/delegate_tool.py at1fd18277267152893a8a24792b79213e7ca3f899; no Python adapter or source port is needed.

## M87 settled outcome

One distinct actual duplicate-inventory repair is FAILED: gpt-6-luna made5parent +3helper subscription calls,25249reportedtokens in95.9seconds. One read-only helper identified both defects and the zero-byte edge case. Parent ran the expected failing baseline (3/6 passed), repaired only the module and saved one CAS task list. Owner denied the next save because its third row omitted content, then cancelled once. Root independent six-test and literal candidate checks pass. The repaired file, partial progress, history, usage and memory survived five quiet same-store seconds unchanged; both owned daemons retired and18787 was free.

Fresh Rust test/lint/build, SDK/web typecheck/build, public coding oracle, retained legacy oracle targets, private policy/type/bundle/Python checks, touched formatting/docs and one batch/final SDK review with one combined fix wave pass. No production Rust/SDK/web/dependency change; this milestone adds the reusable offline coding acceptance fixture/oracle and target.

The actual workflow FAILED before model final verification, report creation/read-back, completed progress or final reply; supplemental root tests do not turn it into acceptance. Quiet recovery proves retained terminal state, not in-flight recovery, indefinite uptime, browser interaction, broad repository quality or full Hermes parity. The frozen projector also reports semantically equal approval DTOs as different because JSON key order differs; this separate private-verifier limitation is diagnosed without changing or regrading the case. No retry, rescue, manual model/output repair, fallback, profile/account/login/connector/DB server/container/Docker55432 activity or historical replay. Settings and reported tokens are not hard billing caps.
