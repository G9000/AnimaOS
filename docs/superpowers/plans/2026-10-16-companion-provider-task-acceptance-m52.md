# M52 useful provider task and bounded delegation implementation plan

Status: controller-bound plan active on
`g9000/companion-provider-task-acceptance-m52`, cut from verified M51/PR57 main
`0e8118ca376354c9ff69d7110c43a0d78f75bab3`. Source
`d53a1000ccbaa33638d549f988ae29b72636c7a5` shares tested tree
`769db1d8c80031787f9773d8b8316b1d918ddb2b`; M51 is committed/pushed/merged.
M51's distinct actual metadata check returned ready/count7 in0.29s; no IDs were
retained or model selected. That release is consumed. M52 has no actual metadata,
service, Exec or generation release yet. Exact release/resume is
`.superpowers/sdd/2026-10-15-companion-provider-task-acceptance-m52/progress.md`.
This plan was bound on2026-10-16MYT after branch/preparation started2026-10-15.

**Goal:** Demonstrate one distinct useful multi-step owner chat repair, one observed
read-only advisory helper, independently correct effects and exact quiet recovery.
**Architecture:** Reuse M47's coordinator, configured model timeout, Windows shell
ownership, native receipts/todos, SDK owner admission and acceptance conventions.
**Stack:** Existing Rust daemon/core, typed SDK and installed Node; no dependency,
new harness framework, worker bridge, runtime, browser or workspace project.

## 1. Evidence and release boundary

- M47's one invoice candidate/task/body and failed provider result stay immutable.
  Do not rerun, repair, copy its live profile, reconstruct its body or seek a pass.
- M48's separate model-access rejection is not M47's historical rejection cause.
- M49 supplies explicit current-account metadata/selection, not generation parity.
  M50 diagnostic evidence found a response over the old cap; M51 addresses only
  that budget. M51 gates/merge and one actual ready/count7 result are recorded; M52 still
  requires its own fresh deliberate selection and release.
- A received catalog entry is a selection aid, never a guarantee of inference,
  tool-schema acceptance, model quality, plan tier or billing entitlement.
- This distinct JSONL fixture is eligible only after verified M51 merge, actual
  current-account selection and root's freeze of all literal acceptance inputs.
- One primary task, one candidate, one attempt; no alternate model/provider,
  regeneration, follow-up verification generation, manual rescue or pass hunting.
- Scripted native journeys establish wiring; only the separately released actual
  provider journey can establish this particular task/delegation quality result.
- PAR-06 remains scoped/partial; M40 quality, broader memory, accounts, production,
  arbitrary coding and full Hermes parity do not become accepted by this case.
- Voice breadth stays held until useful provider work has concrete evidence.

## 2. Reuse and minimal ownership

| Existing source                                                          | Reuse / permitted change after later release                     |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------- |
| `docs/superpowers/plans/2026-10-14-companion-coding-acceptance-m47.md`   | Proven ownership/timeout and failed-attempt boundaries           |
| `docs/superpowers/plans/2026-10-15-companion-subscription-models-m49.md` | Frozen catalog routes, states, deliberate model selection        |
| `docs/superpowers/plans/2026-10-15-companion-catalog-budget-m51.md`      | Bounded catalog budget only; actual metadata is a separate gate  |
| `docs/companion/harness-acceptance.md`                                   | Revision-specific evidence and quality limits                    |
| `docs/companion/parity-roadmap.md`                                       | PAR-06 remains useful-task/delegation acceptance                 |
| `packages/core-rust/acceptance/coding-m47/`                              | Oracle/manifest/control conventions; do not modify M47 inputs    |
| `hosts/rust-daemon/src/agent_runs/coding_tests.rs`                       | Real handlers, scripted gates, receipts and protected witnesses  |
| `hosts/rust-daemon/src/agent_runs/test_support.rs`                       | Existing ScriptedModel, Gate, session/admission/approval helpers |
| `hosts/rust-daemon/src/agent_runs.rs`, `tools/team.rs`                   | Helper inheritance, run links, limits and native coordination    |
| `hosts/rust-daemon/src/runtime_model.rs`                                 | Configured timeout includes credential acquisition and streaming |
| `hosts/rust-daemon/src/tools/process/shell.rs`                           | Existing Windows Job/whole-job wait/joined IO ownership          |
| `packages/sdk/src/{chatgpt,runs,agents,sessions,approvals}.ts`           | Ordinary owner methods; no new production API planned            |

- Proposed new fixture slice: packages/core-rust/acceptance/provider-task-m52/.
  It contains fixture, independent oracle/self-tests/reference/manifest/neighbor.
- Proposed native file: hosts/rust-daemon/src/agent_runs/provider_task_tests.rs,
  registered in `agent_runs.rs` only if fixture integration cases justify it.
- Add only small reusable helpers to `agent_runs/test_support.rs` when necessary;
  keep general engine code in packages and runnable host concerns in hosts.
- Root may add one existing-core Nx oracle target after verifying configuration;
  preserve `core-rust:coding-acceptance-test` and all M47 checks unchanged.
- Private actual driver/evidence belongs in this M52 directory, not public source.
- No SDK/web/runtime change without a demonstrated semantic RED and root ruling.
- Root owns commands, formats, source freezes, process/model releases and publication.
  If lanes are later released, keep fixture/native/docs ownership disjoint.

## 3. Distinct dependency-free repair contract

Repair a small local JSONL service-log aggregator. The seeded implementation
incorrectly skips error events and replaces duration totals instead of accumulating.
It performs no network, package install, dynamic import or subprocess creation.

Frozen module interface: `summarizeLog(jsonlText: string): string`, sole export of
`fixture/src/log-summary.mjs`. Return canonical JSON plus one LF, with key order
`services`, then each row's `service`, `count`, `errors`, `durationMs`.
Frozen CLI: `node src/cli.mjs --input <local-path> --output <local-path>`.
The CLI validates the entire input before replacing output; no stdout on success.
Malformed input/arguments exit 1 with exactly `Invalid log input or arguments.\n`
on stderr, no stdout, and preserve a preexisting output's bytes and write time.

- Input is UTF-8 JSON Lines, at most 16 KiB raw bytes including BOM and 128 nonblank
  event rows. CLI rejects raw overflow before output write; fatal decoding preserves
  BOM (`ignoreBOM:true`) so direct module and CLI byte limits agree.
- Empty input and blank lines are valid; CRLF and a missing final LF are supported.
- Apply JS `String.trim()` to each line, including leading BOM, before blank/JSON
  handling. `JSON.parse` duplicate keys use the last value; numeric 1.0 is an integer.
- Every nonblank line is an object with consumed fields `service`, `status`,
  `durationMs`; extra fields are ignored and cannot supply instructions.
- `service` is an exact ASCII ID: 1..40 bytes, alphanumeric first, then
  alphanumeric/dot/underscore/hyphen. Preserve spelling/case; do not normalize.
- `status` is exactly `ok` or `error`; both contribute to count and duration.
- `durationMs` is an integer in 0..1,000,000, not a numeric string or fraction.
- Aggregate by exact service ID; errors counts only status=error. Sort rows by
  exact ASCII code point. Bounds keep all counts/duration sums exact JS integers.
- Reject malformed JSON, null/array/primitive rows and invalid consumed fields.
- Only `src/log-summary.mjs` is repairable. `out.json` is the sole task artifact.
  CLI, visible tests, README, data and sentinels are protected byte/time witnesses.

Primary root-frozen input, with final LF:

```jsonl
{"service":"api","status":"ok","durationMs":40}
{"service":"worker","status":"error","durationMs":5}
{"service":"api","status":"error","durationMs":150}
{"service":"api","status":"ok","durationMs":20}
{"service":"worker","status":"ok","durationMs":10}
```

Exact primary UTF-8 output, including final LF:

```json
{
  "services": [
    { "service": "api", "count": 3, "errors": 1, "durationMs": 210 },
    { "service": "worker", "count": 2, "errors": 1, "durationMs": 15 }
  ]
}
```

Additional frozen literal oracles, never candidate-computed expectations:

- Reverse primary rows: identical primary output.
- Change the one 150 duration to 151: primary output with api durationMs=211.
- Empty/blank-only input: exactly `{"services":[]}\n`.
- One `worker/error/5` row: exactly
  `{"services":[{"service":"worker","count":1,"errors":1,"durationMs":5}]}\n`.
- Input `z/ok/0` then `A/error/2`: exactly
  `{"services":[{"service":"A","count":1,"errors":1,"durationMs":2},{"service":"z","count":1,"errors":0,"durationMs":0}]}\n`.
- Leading BOM then `bom/ok/1`: exactly
  `{"services":[{"service":"bom","count":1,"errors":0,"durationMs":1}]}\n`.
- One object with service `ignored` then `dup`, status `error` then `ok`, duration
  99 then 3 uses last keys: `{"services":[{"service":"dup","count":1,"errors":0,"durationMs":3}]}\n`.
- `constructor/error/2` then `toString/ok/1`: exactly
  `{"services":[{"service":"constructor","count":1,"errors":1,"durationMs":2},{"service":"toString","count":1,"errors":0,"durationMs":1}]}\n`.
- Literal BOM padding cases freeze raw UTF-8 lengths 16,383/16,384 as valid and
  16,385 as invalid; the invalid CLI case preserves existing output bytes/time.
- Freeze further literal cases for distinct `api`/`API`, extra ignored fields,
  boundary duration and row count before source admission; do not derive them later.
- Invalid cases include bad JSON, missing/null/wrong fields, invalid IDs/status,
  negative/fraction/string/oversized duration, row/body overflow and invalid CLI args.

## 4. Independent oracle and immutable witnesses

- Root freezes buggy source, visible tests, all data, spec, CLI, command strings,
  independent expected manifest, reference and neighbor before any actual call.
- Keep oracle/reference/expected manifest outside candidate workspace/tool authority.
  A directory boundary alone is not OS isolation; record actual tool confinement.
- Oracle tests require buggy baseline FAIL, independent reference PASS and semantic
  mutants FAIL: skip-errors, overwrite-total, zero-errors, unsorted rows,
  hardcoded-primary and prevalidation-output-truncation.
- Expected output comes from literal root-reviewed strings; reference is a positive
  control, not expected-answer authority. Do not import candidate logic to compute it.
- Check sole export/module behavior and actual CLI exit/stdout/stderr/output bytes;
  run candidate code only after root reviews the narrow source diff for side effects.
- Each oracle child has a 2-second timeout and 16-KiB capture limit. Join/retire
  owned children before asserting; inspect unexpected import/process/network code.
- Hash all protected files with byte length and LastWriteTimeUtc before/after task,
  oracle, quiet restart and cleanup. Protect an independent neighbor tree too.
- The todo sidecar is separately declared host-owned runtime evidence; it is not
  an extra model artifact. No model-written reference, test or verdict is trusted.
- Run the final independent oracle once on a frozen candidate copy and repeat
  unchanged after restart. No repair between verdicts and no model re-admission.

## 5. Current model selection: separate HELD operation

- First finish deterministic preparation; do not spend catalog TTL during setup.
- Root records verified M51 merge/source, daemon build and exact owner API origin.
- A local SDK `chatgpt.models({signal})` GET reads cache only. Record its canonical
  state/revisions/times; no direct provider URL, token, account or profile inspection.
- If fresh current ready metadata exists, root deliberately chooses one literal
  listed ID and freezes it in the M52 release manifest; no static suggestion.
- If fresh metadata is needed, root separately releases at most one exact `{}`
  `chatgpt.refreshModels({signal})` operation: the existing fixed upstream metadata GET,
  ten-second bound, no generation, alternate version, scan or fallback.
- This optional metadata operation has its own release/attempt count. An uncertain
  POST permits one cache-only GET reconciliation, never an automatic second POST.
  Previous M47/M48/M49/M50/M51 releases provide no M52 attempt or budget reset.
- Empty, unavailable, disconnected, stale or failed metadata leaves generation
  HELD. Do not substitute a custom/guessed ID to make this acceptance case proceed.
- Immediately before admission, root verifies current revision and unexpired
  snapshot via cache-only GET. Changed principal/revision/expiry returns to HELD.
- After admission, catalog expiry does not mutate the saved model or create a
  runtime membership gate. Account replacement is a stop/fail condition, not rotation.
- The task release binds the chosen provider+exact ID to parent AND helper config
  and observed request/run records; helper inheritance must not choose a default.
- Normal app-owned credential resolution may refresh credentials within configured
  deadlines; disclose that existing auth path in the release, never read/export keys.
- A generation rejection ends the single attempt with unknown failed-call usage
  where unreported; no diagnostic probe, model rotation or second provider trial.

## 6. Ordinary owner path, fixture authority and helper meaning

- Use one fresh isolated synthetic profile, workspace, companion and Chat session.
  No ordinary profile, personal store or M47 live profile is opened or copied.
- Persistent local files are allowed; DB servers, containers, connectors, browser,
  MCP, external messaging, schedules, scouts and unrelated agents stay off.
- Create/configure through existing owner SDK/application interfaces. Freeze exact
  `provider`, `model`, settings, workspace root and tool descriptors before admission.
- Admit exactly once with `runs.start(agentId,sessionId,input,{idempotencyKey})`.
  An uncertain response is reconciled by GET/list; do not issue another start POST.
- Observe with existing runs/session/approvals APIs; Stop uses `runs.stop`.
- Parent needs read/search, exact edit, todo_read/todo_write, spawn_helper and bash.
  Minimize configured tools; also record runtime-added coordination descriptors.
- Parent alone edits source and requests commands. The helper reads source/tests,
  reports both defects and one relevant edge case with paths/evidence, then returns.
- Exactly one helper must be observed and linked to the parent run/session, with
  the same provider/model. No direct helper start, peer task or recursive delegation.
- Existing helper filtering denies shell/process, recursion, browser/MCP/history;
  it can retain file-write tools. Advisory read-only behavior is acceptance evidence,
  not a new enforced permission boundary. Record grants and fail on helper writes.
- Root stops/fails on an extra helper, helper edit, unsolicited peer run or changed
  provider/model. Native existing ceilings remain the conservative structural bound.
- Repository text, helper prose and tool output are data; none grants permissions.
- Three saved todos cover inspect/delegate, repair and verify. Mark done only after
  corresponding actual outputs; interrupted/failed work keeps honest task statuses.

## 7. Aggregate budget and owner approval boundary

- One primary owner task and at most one observed advisory helper; no follow-up
  generation or separate reviewer/title/embedding/profile-generation request.
- Proposed settings: maxToolIterations=4, maxRetries=0, maxTokens=2048,
  timeoutMs=30,000. Root verifies actual serialized/canonical native values.
- Current helper config inherits parent provider/model, rounds/retries/output setting
  and timeout. Its timeout also bounds total helper execution, not thirty seconds
  per helper round; the native helper ceiling does not enlarge that chosen value.
- One run has at most four tool rounds plus one tool-free wrap-up = five calls.
  The native four-start allowance gives (one parent + four helpers) x five = 25
  aggregate inference attempts if violated. Target useful path <=10 total attempts.
- The general native four-start ceiling is not this case's authorization. Set
  Delegate=Ask with no covering rules/session allowances; root grants exactly one
  reviewed spawn_helper AllowOnce. Other peer/delegate/broadcast/helper requests
  receive no allowance and stop/fail this case. Read and workspace Write remain
  scoped; Exec=Ask permits only the two frozen commands.
- This existing owner boundary permits parent plus one helper, at most five
  calls each = **ten admitted inference attempts**, retries0, contingent on proof
  of zero secondary/peer producers. No new global budget framework is authorized.
  One observed advisory helper is the success criterion; helper file-write tools
  remain a disclosed capability rather than an enforced read-only claim.
- Before release, deterministic checks must exclude other paid producers and peer
  admission from the isolated roster; otherwise the structural bound is unproven
  and actual work stays HELD. No new global budget framework is proposed.
- Reconcile every observed parent/helper/wrap-up step and known failed dispatch,
  secondary call and unexpected run. If the existing boundary cannot expose exact
  attempted counts, retain honest unknowns plus the proven structural bound;
  do not invent a per-dispatch trace or hard billing cap. No real release unless
  producer/approval/loop bounds are deterministic and independently checked.
- Disable autoTitle/autoCompact where available and all learning/evaluator/scout/
  embedding/profile producers. Helpers do not inherit arbitrary additional flags:
  prove zero secondary calls for the bounded fresh helper room, never assume it.
- Proposed root task deadline: 180 seconds including approval waits. On expiry or
  violation, Stop parent and linked children; no further model call is authorized.
- At most two task Exec approvals: exact baseline test and exact final test+CLI.
  Each command has a 10-second tool deadline and bounded existing output capture.
- Model settings/request limits are not hard token or billing caps. Subscription
  adapter output-token controls may be omitted; cancellation cannot undo billing.
- Optional metadata count, normal auth refresh and root offline oracle processes
  are reported separately from inference attempts and the two task Exec actions.

## 8. Exact task and shell release material

Root freezes the final owner task text before admission, covering these directives:
repair the JSONL aggregator; inspect tests/source; save the three todos; ask exactly
one advisory helper to inspect only; observe initial failing tests; edit only the
module; run the declared final test/CLI; report actual result and remaining limits.
Do not provide the reference implementation or permit test/data/CLI modification.

Root resolves an installed absolute Node executable and the actual selected shell
before replacing the placeholders below. Freeze exact quoting, cwd, arguments and
timeouts; these examples are not executable authority or guessed shell syntax.

```text
baseline: <absolute-node> --test test/log-summary.test.mjs
final:    <absolute-node> --test test/log-summary.test.mjs && <absolute-node> src/cli.mjs --input data/events.jsonl --output out.json
cwd:      <new-owned-M52-candidate-root>
```

- Existing Exec Ask remains active. Root reviews pending exact command/cwd/body
  and source diff, then issues only AllowOnce for these frozen operations.
- No command-prefix wildcard, any matcher, AllowSession/Always, model-selected
  script, extra arguments, install, arbitrary shell or auto-approval loop.
- A mismatched command is denied and the case stops truthfully; it is not rewritten
  by the driver until accepted. Approvals and tool receipts must correlate by ID.
- Root offline oracle/self-test commands have a distinct predeclared release;
  they are not permission for the model to execute the oracle or its reference.

## 9. Deterministic sequence and checks before any actual release

- [ ] Root verifies M51 main/clean relevant tree, binds five risks and publishes plan.
- [ ] Freeze literal spec/oracles/commands/fixture/neighbors; prove baseline failure.
- [ ] Prove independent reference passes and each semantic mutant fails as intended.
- [ ] Reuse ScriptedModel/Gate to exercise real read/edit/todo/bash/helper handlers,
      exact approvals, failed-before/correct-after output and persistent paired receipts.
- [ ] Assert parent/helper model identity, four-start aggregate bound, zero retries,
      no peer/secondary producer, no helper edits and no protected byte/time changes.
- [ ] Cover rejection before tools, failed command, budget exhaustion and owner Stop
      using existing regressions plus only missing fixture-specific integration checks.
- [ ] Reuse M47 physical shell lifecycle evidence on the new verified build; rerun
      focused ownership checks only if changed source/installation leaves a real concern.
- [ ] Demonstrated gaps get semantic RED, smallest fix, one combined affected review
      and one fix wave; no speculative harness or broad permission rewrite.
- [ ] Root freezes final source, runs required current-tree gates, then separately
      decides whether to release metadata selection and the one actual task.

Future root commands; none has run for this draft:

```powershell
bun --no-env-file x nx show projects --json
bun --no-env-file x nx show project core-rust --json
bun --no-env-file x nx show project rust-daemon --json
bun --no-env-file x nx run core-rust:provider-task-acceptance-test --skipNxCache
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
```

The proposed oracle target must first be added/inspected on existing core-rust;
it runs Node's oracle self-tests, preserves M47 target and adds no project/dependency.
Inspect target dependencies before omitting duplicate core runs. Confirm focused
args forwarding before using proposed `m52_provider_task_` selectors. Use the
documented Windows validation target directory if an owned daemon locks its exe.
SDK/web changes, if later justified, require their relevant fresh Nx gates too.

## 10. Actual verdict, memory, quiet restart and physical cleanup

- Root binds exact release manifest: source/merge hashes, selected ID+revisions,
  fixture/oracle/command hashes, roots/port, attempt/time/Exec budgets and Stop rule.
- Record first actual failing test output, helper's correct bounded finding,
  source diff, final passing tests/CLI, independent literal oracle and final reply.
- Completed run or a model-authored PASS is insufficient. Helper failure, missing
  verification, unchanged bug or oracle miss is failed/incomplete quality evidence.
- No manual repair/reset/new task after a miss; retain failed state and consumption.
- Default minimal profile has no seeded canonical memory. State that explicitly.
  Only if root deliberately seeds a synthetic Fact/context item and observes its
  actual canonical source/context use may this case claim that narrow preservation.
- Wait for parent/helper terminal state and zero in-flight calls/tools/producers;
  freeze configs/grants, messages, todo revisions, receipts, links, usage and files.
- Perform one quiet same-store owned process restart with no new owner message.
  Verify exact canonical records, candidate/output/neighbor bytes+write times,
  no new calls/tools/helpers/approvals and no automatic replay after readiness.
- A forced quiet restart proves reload/no-replay, not graceful shutdown or recovery
  of an in-flight effect. Do not relabel it as broad memory or crash-atomicity proof.
- Capture owned PID/image/creation-time/ancestry for daemon, launcher and actual
  shell/Node children. Retire only verified owned identities, including descendants.
- Prove joined owned process retirement and port free; do not infer cleanup from
  a returned Stop, direct-child exit, stale PID or mere process-name search.
- Preserve uncertain ownership/effects and stop cleanup on mismatch. Neighbor
  processes/files must survive; no ordinary profile deletion or global kill action.
- Root records actual gates, independent verdict, usage unknowns, restart/cleanup,
  exact published source/PR/merge and remaining parity limits after settlement.

## 11. Top five risk proposals (superseded by binding rulings below)

1. **Current selection and evidence.** Choose one fresh current-account listed ID,
   separately releasing metadata if necessary, and freeze it for parent/helper.
   Why: static guesses and catalog presence do not prove generation/tool acceptance.
   Cost if wrong: held or visibly failed one-shot quality case, never another slug.
2. **Literal correctness and immutable history.** Use the distinct JSONL fixture,
   protected tests/neighbors, literal oracle/reference/mutants and actual tool output.
   Why: transcript completion and M47 replay cannot prove a new repaired utility.
   Cost if wrong: failed quality retained; no manual rescue credited to the model.
3. **Aggregate exposure and helper authority.** Bind five calls/run x five possible
   runs=25 conservative attempts, zero retries/producers, one observed advisory helper.
   Why: per-parent limits and read-only instructions are not aggregate/isolation caps.
   Cost if wrong: Stop/fail with actual/unknown spend and inherited writes disclosed.
4. **Exact execution and physical effects.** Only two declared AllowOnce shell actions,
   source review, existing Job/IO ownership and PID/image/time/ancestry cleanup proof.
   Why: model text cannot authorize shell breadth or make process retirement true.
   Cost if wrong: task stays incomplete/held with uncertain effects preserved.
5. **Quiet recovery and scope.** Reuse existing stores/coordinator and exact quiet
   restart with no replay; claim seeded memory only when actually observed.
   Why: scripted wiring and forced reload do not prove graceful shutdown or parity.
   Cost if wrong: narrow evidence/limits remain explicit; voice and breadth stay held.

No proposal above is a controller ruling or authority to execute. Root must bind
these five risks and every concrete real operation on the next verified main branch.

## Controller rulings on the risks (binding)

1. **Selection.** Ruling: use a fresh current-account catalog via the ordinary owner
   SDK only after deterministic preparation, then deliberately bind one exact ID to
   parent and helper — M51 proved metadata parsing, not generation entitlement —
   cost if wrong: one failed/held task, never another model/provider trial. No current
   model literal/default is authorized. Cache-only GET may reconcile; at most one
   separately released Refresh uses the fixed M51 path. Preserve principal/revision/
   TTL evidence before admission. Every earlier actual release stays consumed.
2. **Independent correctness.** Ruling: promote only the distinct JSONL fixture and
   literal oracle/reference/mutants, with the129-byte primary output frozen outside
   candidate authority — a completed transcript or M47 replay does not prove a
   repaired utility — cost if wrong: failed/incomplete quality retained, no manual
   rescue credited to the model. Bound JS trim/BOM, JSON.parse last-key-wins and raw
   UTF8 bytes as specified. Keep exactly the two seeded aggregation defects; CLI/
   validation/tests/data/neighbor are protected witnesses. Root reviews edited code
   for side effects before execution; directory separation is not OS isolation.
3. **Cheap bounded delegation.** Ruling: use existing Delegate Ask and exactly one
   owner-reviewed spawn_helper AllowOnce, plus max4tool rounds/retries0/30s timeout
   per configured run — unconditional Delegate Allow would permit extra admitted
   work despite a one-helper instruction — cost if wrong: hold generation until
   deterministic integration proves the boundary, with no generic budget subsystem.
   Parent+one helper x five calls = ten admitted inference attempts, only with
   secondary/peer/title/compact/profile/learning/embedding producers excluded.
   General native four-start exposure is25, not this case's released budget. No
   additional Delegate allowance; any extra/mismatched action is denied and case
   stops. Report known usage/call evidence and uncertainty honestly. The request's
   maxTokens2048 is not a hard provider billing/output cap. Root deadline180s includes
   approvals; Stop all linked work on expiry/violation, no re-admission.
4. **Execution.** Ruling: at most two Exec AllowOnce decisions for the frozen exact
   baseline and final test+CLI,10s each, plus the one separately reviewed helper
   AllowOnce — model prose cannot authorize shell breadth — cost if wrong: stop
   with preserved effects and uncertain process ownership. No wildcard/prefix/
   session allowance or auto-approval loop. Root resolves Node/shell/cwd/hashes and
   checks pending bodies, candidate diff, Windows Job/IO and PID/image/creation/
   ancestry evidence. Retire verified owned identities only; preserve neighbors.
5. **Evidence and recovery.** Ruling: deterministic oracle controls and real-handler
   scripted integration precede one combined native/core/fixture review, final SDK/
   web scope check, one fix wave and fresh required gates — fixtures and scripts
   cannot establish provider quality — cost if wrong: held/failed evidence and no
   parity claim. Only afterward may root separately release synthetic local startup,
   one metadata operation/selection and one useful task under exact manifests.
   Default new profile has no canonical Fact seeded; this case claims record/file/
   todo/receipt retention only. One quiet same-store restart must produce no new
   call/action; it is not graceful shutdown/in-flight exactly-once proof. No M47
   profile copy/replay/repair, extra diagnostic generation or post-miss rescue.

### Source ownership and first releases

After root publishes this plan, fixture author owns only the new existing-core
acceptance/provider-task-m52 slice and its one oracle target in existing core-rust
project.json. Preserve all M47 files/targets. Native author owns new
agent_runs/provider_task_tests.rs and minimal registration/test helpers in existing
agent_runs.rs/test_support.rs. Reuse real read/edit/todo/helper/approval/receipt
handlers; demonstrate any missing behavior with semantic RED before production fix.
Only these deterministic source/preparation releases exist initially. Root owns
public docs/ledger/commands/fmt/Git/services/models/private actual orchestration.
No new project/dependency, broad runtime rewrite, API/SDK/web change, actual daemon,
metadata or generation is released without the later bound operation.
