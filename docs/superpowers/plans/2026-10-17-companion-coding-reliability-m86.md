# M86 Coding and Helper Reliability — Lean Plan

**Goal:** One actual model repairs a small snapshot-comparison module, consults one advisory helper, runs visible tests, reads its output, saves honest progress, and retains the result across a quiet restart.
**Architecture:** New dependency-free public fixture/oracle; copied and adapted M85 private owner policy, driver, service/controller and unchanged OwnedJob. No production runtime/SDK/web change unless an independently demonstrated deterministic RED requires a separately bound narrow fix.
**Execution:** Root binds the five risks, publishes this plan, owns commands/releases/Git and the one actual attempt. Source-only public and private lanes; one source batch plus final SDK/web review, one combined fix wave, required gates and honest publication without check-ins or retries.

## Scope and source evidence

- Branch `g9000/companion-coding-reliability-m86`, base verified M85 merge `51384ab97205fb99500ec6d0c9774879343e4409`.
- Preserve M85 accepted source/case and M52 PARTIAL, every failed/preflight record, original source snapshots and consumed releases. Never replay or regrade them.
- Reuse `tools/team.rs:157-208` ordinary `spawn_helper`; its four-start runtime ceiling is not this case's one-helper authority.
- `agent_runs.rs:104-129` inherits parent model/provider, maxTokens, timeout, retries and tool-round cap, while filtering process/coordination/browser/MCP tools.
- `agent_runs.rs:560-561` reserves another shared run permit while the parent holds one. Set isolated maxConcurrentRuns **2**, maxRunsPerAgent **1**; copied M85 limit 1 would prevent the helper.
- `agent_runs.rs:1633-1645` uses inherited timeout as the helper's whole-run deadline: parent timeout 30,000 ms means a 30 s helper total, not six independent 30 s windows.
- Helpers inherit ordinary read/write/todo descriptors; advisory/read-only behavior is a task constraint, not a newly enforced read-only grant claim. Existing runtime removes shell/process and recursive delegation capability.
- `tools/todo.rs:49-67,106-125` retains observed-revision CAS; no baseline refuses and stale revision saves nothing. Require explicit observed `todo_read` before this task's first `todo_write`, despite startup baseline capture.
- M85 live events plus full pending arguments authorize in-flight actions; committed full history supports terminal quality. Never wait for saved Assistant calls to authorize a pending approval.

## Reuse decision and ownership

- Direct reuse wins: M52 fixture/witness/oracle conventions and private controller **source**, M85 live authority/checkpoints/immutable decisions, existing SDK, file tools, todo store and OwnedJob.
- Supported adapter alternative: existing native `bash` invokes installed Node for two fixed commands; no new executor or transport.
- Narrow port alternative: root compares useful pinned public Hermes coding/todo/helper behavior at commit `1fd18277267152893a8a24792b79213e7ca3f899`; only justified copied code requires provenance/notices. No new port is assumed.
- New public directory: `packages/core-rust/acceptance/coding-m86/` containing README, `fixture/`, `reference/`, literal `expected-cases.json`, oracle/self-tests and a small module probe if needed.
- Modify only `packages/core-rust/project.json` to add `coding-reliability-test` alongside existing acceptance targets; existing slices/targets remain unchanged.
- Private M86 ledger owns copied `owner.ts`, `owner-policy.ts`, policy tests, service/controller/release scripts, releases, source/runtime freeze, immutable witnesses and actual evidence.
- Public fixture lane owns fixture/reference/oracle/tests/README and the target. Private SDK lane owns copied driver/policy/tests; root owns lifecycle/releases and substantive source review before executing candidate code.

## Small repair contract

- Export pure `compareSnapshots(before, after)` from `src/compare-snapshots.mjs`; return `{ added: string[], removed: string[], changed: string[] }`.
- Each input is an array of at most 128 records with exactly `{ path, bytes, sha256 }`; duplicate paths within either snapshot are invalid.
- Paths are case-sensitive ASCII relative names, at most 128 characters; slash-separated nonempty `[A-Za-z0-9][A-Za-z0-9._-]*` segments, no `.`/`..` segments, leading slash, backslash, colon, whitespace or controls.
- `bytes` is a nonnegative safe integer at most 1,048,576; `sha256` is exactly 64 lowercase hexadecimal characters. Empty arrays are valid.
- Added means only in after; removed means only in before; changed means present in both with different bytes **or** hash. Identical records are omitted.
- Sort each array using ordinary ASCII lexicographic order, independent of input order. Do not mutate either input; use Map rather than unsafe prototype-key indexing.
- Invalid input throws a bounded ordinary Error; the module performs no filesystem/network/process work, imports, dynamic evaluation or side effects at load/call time.
- Seed exactly two defects: missing removed entries and bytes-only change detection, which misses same-size hash changes. Existing validation and other behavior start correct.
- Freeze primary literal output as `{"added":["new.txt"],"removed":["old.txt"],"changed":["same.txt","size.txt"]}\n`; root binds concrete input records and independently computes its byte count/SHA before release.
- Only `src/compare-snapshots.mjs` is repairable, at most 16 KiB source. `changes.json` is the sole CLI-generated artifact.
- CLI, `verify.mjs`, visible tests, before/after JSON data, README, sentinel and external neighbor are protected byte/hash/mtime witnesses. Reference/oracle/hidden cases stay outside candidate authority.
- Legitimate daemon-owned parent todo storage is separately recorded, not treated as arbitrary allowed `.animaos-swarm/**` writes. No helper todo mutation is accepted.

## Protected verification interface

- Six visible Node tests cover removed entries, same-size hash changes, add/size changes, unchanged and empty snapshots, sorting/input immutability, and invalid records/duplicate paths.
- Protected CLI reads fixed before/after data, calls the module, writes compact JSON plus one LF to `changes.json` only after validation; invalid inputs preserve existing output bytes/mtime.
- Protected `verify.mjs` runs exactly those six tests and, only on success, invokes the CLI. It reads the actual artifact and prints a bounded structured summary with visible test count/result, added/removed/changed counts, byte length and SHA-256.
- Verification summary must come from actual child exit and artifact bytes, not candidate-provided status. Six tests are explicit fixed cases, not a guessed TAP count.
- Baseline command runs the same protected verifier and must fail on the seeded defects without creating or changing `changes.json`.
- Final command runs it once after repair. No third Exec attempt, hidden-test command, installation or arbitrary shell command is eligible.
- Root freezes two exact command strings using its verified installed Node executable, native shell quoting, `cwd: "."`, timeout 10,000 ms and no extra arguments. They may be identical strings, distinguished by phase and one-use call identity.
- Before baseline AllowOnce, require the module's exact seeded SHA and all protected witnesses; if the model edits first, refuse rather than label a repaired run as baseline.
- Before final AllowOnce, require baseline evidence, completed helper return, actual successful repair, unchanged protected files and root review of the entire small edited module for side effects.
- A directory boundary and an ordinary process Job are not an OS filesystem sandbox. Root source review is required before any edited module executes, including independent oracle probes.

## Actual task, progress and honest final

- One task asks the parent to inspect the fixture, consult one advisory helper, repair the module, run the two commands, read full `changes.json`, complete three todos from evidence and report actual results.
- Give contract, paths, exact commands and limits; do not reveal hidden cases, reference implementation or a fabricated expected output/hash.
- Freeze one helper name/task: inspect only module, visible tests and README; identify both defects and one edge case with paths, then return advice. Explicitly prohibit edits, todo writes, commands and further delegation.
- The helper result's success and parent/helper links must be actual; the parent must not call the helper's opinion an executed test result.
- Three fixed todos: `Inspect and consult one advisory helper`, `Repair snapshot comparison`, `Verify tests and inspect output`.
- Read tasks first. First todo becomes complete only after actual inspection and successful helper return; repair complete only after successful module edit; verification complete only after successful final Exec **and later full native artifact read**.
- Preserve complete-list CAS updates and exact statuses. Stale/conflicting/malformed task state stays unmodified and nonpassing; no owner seeding or automatic retry to rescue the case.
- Feasible five-tool-round order: (1) independent reads + todo_read + helper + baseline; (2) repair and evidence-appropriate todo state; (3) final verification and repair-complete state; (4) full artifact read; (5) verification-complete todo; then tool-free final.
- Independent calls may share a round; dependent completed-state claims require an earlier successful result. The initial todo_read must precede the first write in a later model round.
- Final must state actual visible tests passed/failed and count, output status and counts, artifact bytes/SHA, exact file changed, helper contribution and limitations.
- No claim of running hidden/root edge cases, extra tests, broad correctness, deployment, general helper reliability or crash recovery. Root reviews both parent and helper substantive claims; do not build a generalized truth classifier.
- A functional repair with false verification/todo/final claims remains nonpassing or PARTIAL under a frozen classification table; never silently correct the model's report.

## Live admission and limits

- Parent grants only required `read_file`, `write_file`, `todo_read`, `todo_write`, `bash`, and `spawn_helper`; do not widen profiles or grant unrelated tools.
- Parent Write/Exec/Delegate Ask, Network Deny; exact AllowOnce only. One helper spawn, exactly one whole-module write_file repair, at most three complete-list todo writes, bounded three-row todo updates, and two Exec decisions.
- Any helper write/todo mutation is a failing observed effect and triggers Stop; detection does not undo it. Process/recursive tools are filtered by the existing runtime. Observe actual inherited permissions/policy, never claim the advisory prompt enforces read-only access; grant no helper approval.
- Reuse M85 contiguous stream, request/start identity, exact full pending JSON and native UTF-8 2 KiB prefix corroboration; pending truncation at 16 KiB refuses. Freeze the full decision context before POST.
- Extend accepted tool/phase vocabulary only for this case; use parent stream for parent approval and helper-return result, plus actual helper run/session inventory for helper effects and final evidence.
- Track both agents/runs and all usage. Preserve requested/resolved DTOs, exact tool identity, no gaps/resync/reconnect substitution and no stale closed-watcher authority.
- One listed subscription model selected once, inherited by the helper; existing app-owned resolver only. Zero retries/fallback/model ladder or secondary title/compaction/profile/learning/embedding/query-expansion generation.
- Parent maxToolIterations 5, maxRetries 0, maxTokens 2048, timeout 30,000 ms. Helper inherits five rounds and 2048 tokens: structural ceiling **6 parent + 6 helper = 12 calls**, target at most nine; no fictional two-call helper cap.
- Root task deadline **300 s**, including approvals; helper whole-run deadline **30 s**. Settings/usage are not a hard billing cap. Stop and reconcile known parent/helper runs once each, then retained-Job cleanup; never resend uncertain operations.
- Fresh synthetic store/workspace at port **18786**; shared run capacity 2 only for parent plus one helper. No normal profiles, raw credentials, login/account changes, connectors, DB server, container, Docker 55432 or ordinary services.

## Independent oracle and persistence proof

- Reuse M52 `captureProtected(candidateRoot, neighborRoot)` / `verifyCandidate(candidateRoot, witnesses)` concepts; witnesses must be frozen before task admission, not reconstructed afterward.
- Literal primary and edge-case outputs are independent of candidate/reference execution. Reference is a positive control, not expected-answer authority.
- Independent cases include empty sides, all removed, same-size hash change, size change with same hash, reordered inputs, unchanged entries, prototype-like names, duplicate/unsafe paths and malformed hashes/bytes.
- Mutants must fail for missing removals, bytes-only comparison, unsorted results, treating unchanged as changed, hardcoded primary output, and modifying input/protected output during invalid-input handling.
- Probe candidate in bounded child processes with capped output and deadlines; use existing ownership evidence. A timeout or missing child retirement is not a pass.
- Capture terminal full parent/helper histories, usage step identities, receipts, approvals, helper config/model/provider and parent/run/session links, todos and revision, actual module diff, artifact bytes/hash/mtime and protected witnesses.
- Terminal history contains full Tool text; public `taskResult` may be hidden. Never substitute receipt/live previews for full output or read-back.
- Join all run completion/history flushes; compare actual helper advice and parent final to the frozen evidence. Record target-budget exceedance separately from the 12-call structural ceiling.
- Retire the first daemon, restart the same isolated store once with no new input/catalog refresh, observe five finite quiet seconds and compare parent/helper sessions/messages, linked runs, usage, todos, file bytes/mtime and permissions.
- Require both physical process trees retired, logs joined, Job empty and port free. This proves finite quiet retention, not active-run crash recovery or indefinite no replay.
- Seal actual success, partial or failure once. No corrective second task, extra helper, model rerun, artifact repair or historical outcome rewrite.

## Tests first, reviews and root-only gates

1. Public tests establish buggy baseline fails, independent reference passes and each listed mutant fails with preserved protected/neighbor witnesses.
2. Driver tests prove live pending admission without saved history, exact one spawn/two commands, seeded-SHA baseline guard, edited-source final guard, unknown/extra/helper effects refused and immutable decisions before POST.
3. Progress tests reject todo completion before evidence, missing initial todo_read/CAS, verification-before-read and fabricated helper links or model identity.
4. Projection/report controls reject preview-only or missing artifact read, wrong counts/hash/test status, unrun-test claims and missing helper terminal evidence; valid actual-shaped DTOs pass.
5. Retain M85 import purity, closed-watch/gap/refusal, uncertain POST GET-only reconciliation, deadline and quiet-restart controls. Do not create another general guard framework.
6. Root retains focused RED, enables minimal source implementation, runs focused GREEN, freezes both lanes, obtains one source batch and final SDK/web review, applies one combined fix wave, then runs required gates.

- Add and verify `bun x nx run core-rust:coding-reliability-test --skipNxCache`; preserve and run relevant existing provider-task/research acceptance targets once for changed shared assumptions.
- Root uses copied controller modes for private policy tests, typecheck, bundle/import checks and Python static compilation; no worker command execution.
- Required Rust gates remain root-only `bun x nx run rust-daemon:test --skipNxCache`, `rust-daemon:lint`, `rust-daemon:build`, serialized with `CARGO_INCREMENTAL=0`, build jobs 1, test threads 2 and isolated target.
- Root runs applicable SDK/web type/build, changed-file formatting and docs gates from the established Nx targets; no production SDK/web change is proposed.
- Only after gates, source/binary/witness freeze and exact root model/task releases may the one actual task execute. Root records proof and publishes/merges under standing authorization.

## Top five risks — root binding rulings

1. **Helper capacity and scope:** two shared slots, one exact spawn, real inheritance/linkage, 30 s helper total and 12-call structural ceiling; advisory behavior must not be mislabeled an enforced read-only grant.
2. **Candidate execution and protected targets:** exact seeded baseline before any edit; only the small module repairable; root reviews full candidate before final/oracle execution; byte/hash/mtime witnesses cover protected files and neighbor.
3. **Live authority and one-use effects:** M85 stream/full-pending corroboration, immutable decisions, exact commands and CAS todos; never terminal-history authority while awaiting approval or automatic uncertain retries.
4. **Truthful progress/report:** later full native output read precedes verify completion/final; actual tests/counts/hash and helper evidence govern acceptance, with substantive human-controller review instead of generic claim classification.
5. **Cost, persistence and historical integrity:** one model/task/helper, 300 s root deadline, finite same-store restart and full owned cleanup; no historical replay or rescue, no normal profile/account/service activation, retain every failed/partial outcome.

## Controller rulings on the risks (binding)

1. Ruling: two isolated global run slots and one exact advisory helper, inheriting five rounds and a 30 s whole-helper deadline — parent holds a slot while awaiting the child; truthful accounting includes up to six calls per agent — cost if wrong: stop and retain the bounded failure, never widen capacity or retry. Read-only helper behavior is observed, not enforced by a new tool grant.
2. Ruling: one whole-module write and two identical fixed verifier commands, distinguished by actual phase — exact seeded SHA must precede baseline; full source review and protected byte/mtime checks precede final execution — cost if wrong: withhold execution and retain the nonpassing case. Module max16KiB, source-only worker review, no candidate imports/eval/I/O; original guards stay unchanged.
3. Ruling: full M85 live-prefix/full-pending witnesses authorize every Write/Exec/Delegate once; no saved-history prerequisite at admission — native history commits only at terminal and previews are abbreviated — cost if wrong: deny/Stop without repeated POST. Parent todo_read must actually succeed before first write; model updates at most three fixed rows with exact statuses and no extra fields beyond native schema.
4. Ruling: verification completion requires successful final verifier and a later full native changes.json read; parent final names six actual tests, counts, bytes/SHA and helper advice accurately — independent root oracle and substantive parent/helper prose review settle truthfulness — cost if wrong: functional code alone stays PARTIAL, false causal verification is FAIL. No repair of the model final or generic truth classifier.
5. Ruling: one distinct case, current listed low-cost subscription model, one helper, zero retries, structural12calls, max2048tokens/call and300s wall deadline — this isolates practical task quality cheaply — cost if wrong: publish the sole failure honestly. Five quiet same-store seconds prove retained files/tasks/history/usage/lineage/memory without replay; they do not prove in-flight crash recovery or full parity. Preserve M52PARTIAL, M84FAIL and M85ACCEPTED unchanged.

Concrete fixture records use path old.txt/hash a/10bytes, same.txt/hash b/20bytes, size.txt/hash c/30bytes and stable.txt/hash d/40bytes before; after omits old.txt, changes same.txt to hash e/20bytes, size.txt to hash c/31bytes, retains stable.txt and adds new.txt/hash f/5bytes. Hash letters repeat64times. Literal primary artifact is78bytes including LF; root computes its SHA independently before actual release. Exact native Exec arguments are command `"C:/nvm4w/nodejs/node.exe" verify.mjs`, cwd `.`, timeout10000 for each of the two calls. The existing native Windows shell adapter receives that exact command. Root verifies installed Node identity before release. The model receives ordinary task/data/tests, never the reference or hidden oracle.

## M86 settled outcome

One distinct actual snapshot-comparison repair is FAILED: gpt-6-luna made3parent +2helper subscription calls,12542reportedtokens in116.4seconds. One helper completed three read-only inspections and returned grounded advice. The driver withheld the first Exec after seeing linked helper frames; zero shell commands/module writes/todo saves/output files. Owner cancelled once; five quiet same-store seconds preserved all observed state, both owned daemons retired and18786 was free.

Fresh Rust test/lint/build, SDK/web typecheck/build, public coding oracle, retained legacy oracle targets, private policy/type/bundle/Python checks, touched formatting/docs and one batch/final SDK review with one combined fix wave pass. No production Rust/SDK/web/dependency change; this milestone adds the reusable offline coding acceptance fixture/oracle and target. A future-use private driver correction now recognizes only the exact linked helper in the owner stream, with synthetic refusal controls. The sealed actual result is unchanged and has not been rerun or regraded. The initial zero-dispatch native-summary preflight failure is separately archived and remains FAILED.

One actual task FAILED at its private helper-stream approval guard; coding repair, six tests, output read-back, completed saved progress and final reporting remain unassessed. The cancelled parent/helper history, empty tasks, unchanged files, usage/config/grants and memory survived five quiet same-store seconds without replay. This does not prove successful coding, in-flight recovery, indefinite uptime, browser interaction or full Hermes parity. No retry/fallback/report repair, normal profile/account/login/service/connector/database server/container/Docker55432 action or historical-case replay; settings/accounting are not hard billing caps.
