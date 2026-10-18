# M47 coding and delegated-task acceptance: implementation plan

Status: runtime and deterministic acceptance implemented on `g9000/companion-coding-acceptance-m47`, from verified M46/PR52 main `0f166fea3e7e33d6b1f79c1d937b73cca61a1e76`. Current gates pass. The single actual provider attempt failed with HTTP 400 before tools; useful coding/delegation quality is not accepted. Its quiet process restart passed without replay. Publication and exact source/merge verification are recorded in the milestone ledger.

**Goal:** Prove one useful small repository repair through Anima's ordinary tools,
saved tasks and one bounded helper, with independently checked code and effects.
**Architecture:** Reuse the native coordinator, runtime, tools, approvals, receipts,
ledger, SDK and console. First close the existing Windows shell ownership gap.
**Stack:** Existing Rust core/daemon, typed SDK, Node's installed test runner;
no new runtime, external coding worker, package dependency or workspace project.
**Release gate:** M46 is verified merged and the five rulings below are bound.
M46 gates are not evidence for an M47 implementation.

## 1. Scope and success claims

- Deterministic cases do not close the failed actual-provider case.
- Deliver an isolated coding fixture, independent oracle, native acceptance cases,
  and concise evidence. A narrow Windows shell Job/IO/caller-drop fix is mandatory
  before live coding; other runtime changes still require a demonstrated gap.
- One candidate implementation is assessed once against the frozen final oracle.
- A failed provider task remains failed; no silent reset, new model or repair loop.
- Deterministic scripted dispatch proves wiring and invariants, not coding quality.
- A real configured-provider run proves only the particular repair and delegation.
- A completed run, passing model-authored tests or helper summary is insufficient.
- No SWE-bench score, arbitrary coding quality, multi-repository autonomy or full
  Hermes parity follows. M40's introduction-task quality miss remains open.
- No external coding-worker bridge, helper terminal grant, recursive delegation,
  automatic worktree/commit/push, network dependency install or benchmark campaign.

## 2. Evidence baseline and source map

| M46 baseline source                                      | Reuse / boundary                                                                                |
| -------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `docs/companion/harness-acceptance.md`                   | M17 report/helper and M21/M23/M28 effect, budget and recovery evidence remain revision-specific |
| `docs/companion/harness-acceptance.md`                   | M40 corrected-name oracle passed while whole introduction quality failed                        |
| `packages/core-rust/crates/anima-core/src/runtime.rs`    | Actual AgentRuntime loop, paired tool results and one tool-free budget wrap-up                  |
| `packages/core-rust/crates/anima-harness/src/harness.rs` | Existing embeddable facade; do not create a second coding engine                                |
| `hosts/rust-daemon/src/agent_runs.rs`                    | Four generated helpers, filtered inherited tools, bounded runs and parent linkage               |
| `hosts/rust-daemon/src/tools/team.rs`                    | Existing spawn_helper tool and four-start allowance per companion run                           |
| `hosts/rust-daemon/src/tools/filesystem.rs`              | Existing read/list/search/write/edit/multi-edit handlers                                        |
| `hosts/rust-daemon/src/tools/todo.rs`                    | Per-agent saved todos and revision-based conflict handling                                      |
| `hosts/rust-daemon/src/tools/process.rs`                 | Existing bash tool, workspace cwd and cancellation propagation                                  |
| `hosts/rust-daemon/src/tools/process/shell.rs`           | Cleared allowlisted environment, bounded output; direct-child Stop limit                        |
| `hosts/rust-daemon/src/agent_runs/action_receipts.rs`    | Persisted begin/outcome witnesses; evidence failure stops later actions                         |
| `hosts/rust-daemon/src/agent_runs/stop.rs`               | Saved Stop fans out to linked children; no new approval needed                                  |
| `hosts/rust-daemon/src/runs/ledger.rs`                   | Restart interrupts unfinished runs; no automatic execution resume                               |
| [SDK run operations](../../../packages/sdk/src/runs.ts)  | Existing start/get/list/stop, encoded owner paths and stable request key                        |

The separate `anima-core/src/engine.rs` durable-engine abstractions are not evidence
that this daemon uses every portable recovery primitive. Follow actual coordinator
and AgentRuntime wiring; do not migrate the host to another engine for this case.

## 3. Pinned public Hermes comparison

Read-only comparison root: `C:/Users/leoca/AppData/Local/hermes/hermes-agent`.
Its public HEAD ref resolves to `1fd18277267152893a8a24792b79213e7ca3f899`.
Only public source was read; no profile, account, config, DB or credential was opened.

| Public source at that pin                                                                                                                                       | Useful intent / Anima disposition                                                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| [tools/delegate_tool_toolsets.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/delegate_tool_toolsets.py)   | Child tools intersect parent authority; retain Anima's stricter process/MCP/browser/coordination exclusions                      |
| [tools/delegate_tool_child_run.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/delegate_tool_child_run.py) | Late child attachment inherits a pending parent Stop; reuse existing native late-helper regression                               |
| [tools/delegate_tool_child_run.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/delegate_tool_child_run.py) | Child status/exit reason, raw summary and usage differ; retain actual native receipts instead of treating prose as completion    |
| [tools/delegate_tool_results.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/delegate_tool_results.py)     | Bounded actual tool-output tails and sanitized target summaries; no unbounded trace dump                                         |
| [tools/todo_tool.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/todo_tool.py)                             | Saved status should follow observed work, not intent; keep native per-agent CAS format                                           |
| [tools/file_tools_write_guards.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/file_tools_write_guards.py) | Stale full-overwrite refusal; test Anima's existing exact edit/no-neighbor behavior, do not port a guard framework speculatively |
| [tools/checkpoint_manager.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/tools/checkpoint_manager.py)           | Explicit filesystem evidence/attribution for recovery; use fixture byte manifests, not a new shadow-Git store                    |
| [mini_swe_runner.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/mini_swe_runner.py)                             | Record actual command result and finite calls; a final marker is not an independent correctness oracle                           |

Differences to retain deliberately: Hermes config defaults include ten concurrent
children and no default child wall-clock timeout (`delegate_tool_config.py`),
with optional worktree isolation (`115–119`). Anima already has finite helper limits.
Hermes iteration budgets are per agent, not aggregate ([agent/iteration_budget.py](https://github.com/NousResearch/hermes-agent/blob/1fd18277267152893a8a24792b79213e7ca3f899/agent/iteration_budget.py)).
Do not copy those defaults, Python runtime, credentials, terminal backends or profiles.
Read source for behavior; preserve upstream notices if any actual code is adapted.
Anima naming and native library/host boundaries remain authoritative.

## 4. Minimal file ownership after release

- Proposed new reusable fixture/oracle files under
  `packages/core-rust/acceptance/coding-m47/`: `fixture/src/invoice-summary.mjs`,
  `fixture/src/cli.mjs`, `fixture/test/invoice-summary.test.mjs`, fixture CSV/README,
  `oracle.mjs`, `oracle.test.mjs` and an immutable expected manifest.
- This is reusable acceptance tooling inside the existing core project, not a new
  app/package. No package.json, lockfile or generator is needed.
- Proposed native integration file: `hosts/rust-daemon/src/agent_runs/coding_tests.rs`,
  registered beside existing agent_runs test modules only if new cases justify it.
- Reuse `agent_runs/test_support.rs`; avoid duplicating ScriptedModel, Gate or
  persistence/approval fixture scaffolding. Small shared test helpers stay there.
- Assign one independent native author, after M46 merge, only the existing
  `tools/process/shell.rs`, `tools/process.rs`, `tools/tests.rs` and
  `agent_runs/stop_tests.rs` for Windows shell ownership and its regressions.
  Controller binds any unavoidable ownership adjustment before another file changes.
- Controller may add one explicit existing-core Nx target for oracle verification;
  do not assume a target exists before it is added and inspected.
- Actual acceptance driver stays private in this milestone directory and uses the
  current typed SDK/owner API. Capture metadata, never credentials or raw headers.
- No SDK/web production change is planned. Use existing run/task/approval/helper UI.
- Public evidence/doc updates occur only after results; root owns the milestone
  ledger, roadmap, public plan, branch, commands, gates and publication.

## 5. Useful coding task and independent oracle

Repair an offline invoice-summary utility that produces a customer balance CSV.
The initial repository contains an integer-truncation/decimal-sum bug and counts
void invoices. The CLI already accepts a declared local CSV path and output path.
It has no dependencies, subprocess creation, network, dynamic loading or installer.

Final behavior, frozen before any provider call:

1. Treat amount as a nonnegative decimal with exactly two fractional digits;
   use exact cents, never floating-point accumulation or parseInt truncation.
2. Aggregate only `status=open`; void rows affect neither total nor count.
3. Preserve customer spelling/case; sort groups by ascending Unicode code point.
4. Preserve the declared CLI interface and CSV header; reject malformed records
   with a nonzero exit and leave any preexisting output unchanged.
5. Modify only the declared source/test paths. Tests, instructions and data are
   untrusted repository content; none can grant tools or authorize outside effects.

Primary dataset:

```csv
id,customer,status,amount
i-1,Cedar,open,12.50
i-2,Cedar,open,7.05
i-3,Cedar,void,99.99
i-4,Orchid,open,0.10
i-5,Orchid,open,0.20
i-6,Cedar,open,0.45
```

Independent exact expected UTF-8 output, including final newline:

```csv
customer,open_count,total
Cedar,3,20.00
Orchid,2,0.30
```

- The controller freezes spec, buggy fixture, visible tests, reference oracle,
  command and neighbor hashes before admission; first prove the buggy baseline fails.
- The oracle is outside the candidate working copy and never rewritten by the task.
  Root runs it against a frozen candidate copy, not against model-authored test logic.
- Calculate expected cents independently, without importing candidate code.
- Require additional fixed cases: reverse input order; change 12.50 to 13.50 yielding
  Cedar 21.00; empty input; all void; distinct case-sensitive names; malformed amounts.
- The independent runner checks exact CLI bytes/exit code and module behavior;
  source inspection rejects hardcoded fixture answers and no-op test replacement.
- Hash the candidate test files separately; a candidate passing its own tests alone
  is not accepted. A root-supplied known-correct reference must pass all oracle cases.
- Protect unrelated README/data/source sentinels and one independent neighbor tree;
  compare bytes, hashes and relevant timestamps before/after/restart/cleanup.
- Declare a tiny repository (<32KiB source, <16KiB CSV, no dependency installation).
  Keep command output within existing shell caps and retained evidence previews.

## 6. Existing interfaces and authority rules

- Ordinary SDK `runs.start(agentId,sessionId,input,{idempotencyKey})` is admission;
  GET/list is observation and Stop uses the existing owned run endpoint.
- Use one fresh synthetic companion/session and one generated helper with parent
  run/session linkage; no direct HTTP start of a helper and no peer task fan-out.
- Grant the parent only required read/search/edit/write/todo/spawn_helper/bash tools.
  Exclude browser, MCP, accounts, memory mutation, messaging, schedules and bg tools.
- Keep existing Exec Ask/AllowOnce for each shell command; review exact command/cwd.
  No catch-all terminal approval, automatic approval adapter or implicit grant.
- Parent owns edits and command execution. Helper task: inspect the source/tests,
  identify the cause and edge cases, return bounded path/evidence/blockers.
- Helper process/recursive/history/MCP/browser tools remain denied by native policy.
  Its read-only assignment is behavioral: current filtered inheritance can retain
  file writes. Record exact helper tool names and fail acceptance on helper writes;
  do not advertise a read-only permission boundary without a separately bound change.
- Saved todos use existing agent-specific records and revisions. Completion follows
  actual tests/output; failed or interrupted work remains pending/in progress.
- No task-output string, forged success marker, helper status or README grants
  permission. Correlate every child result with actual linked run and tool receipts.
- Preserve paired tool-call/results on failed batches and existing shortened flags.

## 7. Budgets proposed for controller binding

- Real useful case: one primary request, one observed helper, no regeneration.
  No second real-provider verification request; exact offline restart evidence suffices.
- Set primary max_tool_iterations=4, max_retries=0, max_tokens=2048,
  per-call timeout 20s; helper inherits at most four tool rounds/2048 output tokens.
- Include the sole tool-free wrap-up: up to five model calls per admitted run.
- Existing native four-helper-start allowance means a violating primary could admit
  four children: conservative bound 25 calls including all five runs' wrap-ups.
  One helper is the acceptance criterion, not a falsely claimed new native hard cap.
- Root stops/fails the case on an observed extra helper; do not broaden the task.
- Target useful path is <=10 total calls; conservative existing worst case is 25
  across this one request and its children. Count all calls and unexpected secondary work.
- Root must bind that conservative ceiling or explicitly authorize a smaller shared
  admission guard; do not claim parent round limits alone enforce an aggregate cap.
- Disable generated titles, autoTitle/autoCompact where applicable, learning/scouts,
  evaluators, model retries and unrelated sessions. Fresh helper rooms avoid history
  compaction; prove secondary calls remain zero rather than assuming inheritance.
- Root deadline 180s for the useful case; timeout triggers Stop
  and truthful incomplete evidence, not another model request or automatic retry.
- Record actual input/output/reported usage and unknown usage. Request/token settings
  are not hard provider billing bounds, and cancellation cannot undo charged work.

## 8. Execution sequence after release

- [ ] Root binds risks, verifies M46 main, freezes sources/spec/oracle and ownership.
- [ ] Independent native author adds the mandatory Windows shell descendant/held-pipe
      and caller-drop regressions below. Root observes their actual isolated RED.
- [ ] Implement the narrow existing-shell Job/IO owner; root proves real GREEN and
      preserved non-Windows behavior before any live coding/provider admission.
- [ ] Add oracle tests: buggy baseline fails; known-correct reference passes; wrong
      decimal/void/sort/CLI implementations each fail the intended semantic assertion.
- [ ] Add one scripted native end-to-end case through real read/edit/bash/todo/helper
      handlers and durable receipts. No production success stub or mocked file outcome.
- [ ] Observe meaningful RED only for a demonstrated missing behavior; otherwise
      retain existing passing invariants and avoid manufacturing a runtime feature gap.
- [ ] Make only necessary narrow fixes, with reusable library logic in packages/\*.
- [ ] Freeze implementation; perform one native batch and final affected SDK/web
      review if their code changes, then one combined fix wave and fresh required gates.
- [ ] Root prepares fresh private profile/workspace, fixed installed Node, no accounts,
      restores declared settings afterward and records exact owned process identities.
- [ ] Run the actual useful task once: read source/tests, save todos, ask one helper,
      edit code, explicitly approve the finite test command, verify output and wrap up.
- [ ] Freeze candidate immediately; root runs the independent oracle/neighbor check.
- [ ] Quiet same-store restart: verify exact code/todos/receipts/child-parent links,
      no replay/model/helper/process start. Re-run only the offline independent oracle.
- [ ] Capture final task result separately from oracle verdict; do not repair code
      manually and then attribute the repaired result to the provider task.

## 9. Failure, correction, Stop and recovery matrix

| Case                       | Required independent witness                                                                                                    | Evidence class                                                      |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| Corrected requirement      | Gated existing steer joins before next model call; final frozen requirement used; old/new text and key retained                 | Scripted routing; real steering quality only if separately admitted |
| Command fails once         | Actual finite command exits nonzero; result paired; task does not claim passing tests or auto-repeat an uncertain command       | Scripted provider with real tool                                    |
| Budget exhausted           | Completed edit/test facts retained, unfinished todo remains; exactly one tool-free wrap-up and no later effects                 | Deterministic native                                                |
| Stop during helper startup | Late helper inherits Stop, zero late child model call, no new edit or helper replacement                                        | Existing native Gate pattern                                        |
| Stop while helper active   | Parent/child cancelled, paired settled results/receipts, capacity released; stopping child alone leaves parent policy intact    | Deterministic native                                                |
| Windows shell Stop/drop    | Exact shell/Node descendant identities exit; inherited pipes/readers join before disposal is reported; no further marker writes | Real isolated zero-model prerequisite                               |
| Stale todo/edit            | Owner neighbor update survives; todo CAS conflicts; unmatched exact edit changes no bytes                                       | Deterministic native/filesystem                                     |
| Receipt failure            | Failed intent means zero dispatch; failed outcome means unknown effects and no later tool/model continuation                    | Existing native persistence gates                                   |
| Interrupted edit           | Actual file hash and started/succeeded/unknown receipt survive immediate restore; run interrupted; no automatic replay          | Deterministic, plus isolated restart                                |
| Settled-task restart       | Same candidate/todos/receipts/linkage and zero new model/tool/helper calls                                                      | Actual quiet restart                                                |

Reuse `agent_runs/tests.rs`, `stop_tests.rs`,
`steer_tests.rs`, `action_receipt_tests.rs` and
`runs/ledger.rs` instead of duplicating their entire matrices.
New grouped cases should assert integration with this fixture or fill a proven gap.

### Mandatory narrow Windows shell ownership prerequisite

Current cancellation kills only the direct child (`shell.rs`) and may
detach output readers. Ordinary Windows selected-shell-to-Node already has a descendant;
this is a useful-coding prerequisite, not merely an optional hostile test case.

- Reuse maintained process-wrap 10.0.1 and the recorded M44 stdio/M46 browser
  Windows Job ownership seams. No new dependency, process registry or harness.
- Keep shell tool name/schema, command/cwd validation, explicit Exec approval,
  allowlisted cleared environment, output caps and result semantics intact.
- Own shell and all inherited descendants in the Job before execution can escape;
  reject missing/ambiguous containment and retain ownership on uncertain cleanup.
- Stop, timeout and caller drop close logical authority and trigger an uncancelled
  owned retirement path. One whole-Job wait plus joined stdout/stderr readers is
  disposal evidence; direct-child exit, a dropped future or a detached thread is not.
- Never cancel/drop an in-flight writer/reader owner merely to return success.
  Preserve unknown effects and bounded static failure when cleanup lacks proof.
- Preserve current non-Windows behavior and disclose its existing physical limits;
  Windows evidence must not silently become a portable whole-tree guarantee.
- RED fixture: use an explicit installed absolute Node in a fresh private root;
  ordinary shell launches Node, which starts an owned grandchild holding inherited
  output pipes and updating a marker. Record exact PID/image/start-time identities.
- Cover owner Stop, timeout, caller drop during startup/live execution, leader exit
  with a surviving descendant, repeated cleanup and uncertain ownership. Use the
  smallest real cases plus existing gates; zero provider/model calls and no server.
- GREEN requires stopped markers and exact owned descendants gone, bounded output,
  joined IO/Job completion and no cleanup affecting a neighboring process/tree.
- Root alone runs isolated RED/GREEN and required gates. Native author freezes this
  bounded slice for the one combined review/fix wave before useful live acceptance.

## 10. Root-only commands and evidence settlement

Commands below are future instructions, not executed preparation evidence.
From the repo root, use the already configured Nx projects:

```powershell
bun --no-env-file x nx run core-rust:test --skipNxCache
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
```

The host test target depends on core tests; root may avoid duplicate executions
only with an explicitly inspected target/flag and retained evidence. On Windows
use the documented validation target directory if a running daemon locks its exe.
For focused new cases, root first confirms the existing target's args forwarding;
the proposed test prefix is `m47_coding_`, not a preexisting passing test claim.
Run fixture Node commands only through the explicit added existing-core target or
the actually approved task bash command; no npm/Bun install or implicit browser use.

Retain command exits, oracle cases, task IDs, call counts/usage, paired results,
approvals/receipts, todo revisions, fixture/candidate/neighbor hashes, restart
captures and exact process cleanup. Separate driver repairs from runtime failures.
No ordinary profile/accounts/DB server/container, model evaluator, hosted deploy
or public message is needed. Publication requires root's exact source/PR/merge.

## 11. Top five risk proposals

1. **Oracle and quality:** require independent frozen module/CLI/cents/void cases,
   source inspection and unchanged neighbors; one useful failure stays visible.
   Cost if wrong: a passing transcript/test rewrite could masquerade as repaired code.
2. **Helper authority:** keep native filtered inheritance and no helper terminal;
   require one observed advisory helper and zero helper edits, explicitly disclose
   inherited write capability. Cost if wrong: instructions get mistaken for isolation.
3. **Aggregate cost:** bind the source-backed 25-call worst-case ceiling or authorize
   one narrow shared admission guard before real calls; target<=10, no pass hunting.
   Root stops/fails on any observed extra helper; no new hard one-helper cap is claimed.
   Cost if wrong: child/secondary/wrap-up calls escape the parent's apparent budget.
4. **Stop and physical effects:** require the narrow existing-shell Windows Job plus
   joined IO/caller-drop fix and real descendant RED/GREEN before live coding. Reuse
   maintained ownership, preserve non-Windows behavior, and claim no effect rollback.
   Cost if wrong: a cancelled run row hides an executing process or duplicated edit.
5. **Scope and recovery:** acceptance-first, current interfaces and exact restart
   inspection; change native/core only after a concrete RED, no new coding runtime.
   Cost if wrong: a small evidence gap becomes a broad rewrite without quality proof.

The binding controller rulings below supersede these proposals.

## Controller rulings on the risks (binding)

The five proposals above are bound by root. M46 merge and this milestone branch are verified; execute only the individually released ownership slice.

Additional implementation rulings: enable only the existing process-wrap 10.0.1 `std` feature, with no new version or dependency. A genuinely unproven Windows Job/IO retirement retains available resources on its owner thread and latches a minimal shared broken-cleanup fuse before reporting uncertainty. Later shell admissions are refused until daemon restart; this is not a process registry, lease, reaper or retry. The existing run-action failure/cancel seam retains the durable Started receipt and stops further effects/model continuation, without forging an owner Stop or a successful outcome.

The todo sidecar under `.animaos-swarm/agent-tasks/` is separately declared host-owned runtime evidence; `out.csv` remains the sole model-generated artifact. The actual coding-shell test has a distinct ignored `m47_coding_real_shell_` selector so the `m47_coding_windows_` prerequisite cannot execute it early. Baseline leader-exit coverage already passed on the installed Git Bash; only demonstrated Stop/drop/startup/timeout failures count as fixed gaps.

`maxTokens=2048` is requested runtime configuration. The current ChatGPT subscription adapter deliberately omits output-token controls for reasoning models, so it is not a provider output or billing cap. The one-task, per-call/root deadlines, observed helper count, reported usage and no-retry rules remain binding; a failed quality/cost result stays visible.

Ruling: Freeze a controller-owned independent cents/void/sort/module/CLI oracle and candidate/neighbor hashes before the single useful task — a transcript, helper summary or model-edited tests cannot establish repaired behavior — cost if wrong: the task remains a visible failed-quality case, no reset/manual repair/pass hunting attributed to the model.

Ruling: Use exactly one observed advisory helper with current filtered inherited permissions, record its actual grants, and fail/Stop on helper edits or observed extra helpers — an instruction to inspect only is not a native read-only permission boundary — cost if wrong: failed acceptance and truthful inherited-write capability, no unsupported isolation claim or new permission engine for this case.

Ruling: One primary real task only, max_tool_iterations4/max_retries0/max_tokens2048/20s call deadline, target at most10 total calls and conservative existing four-helper bound25 including wrap-ups; no second verification, title/evaluator/learning/scout/compaction side requests — per-parent settings alone omit child/secondary spend — cost if wrong: stop and record actual/unknown usage and a cost miss, never claim a hard one-helper or provider billing cap. Parent/root deadline180s remains conservative and no automatic retry is authorized.

Ruling: Fix the existing Windows shell's physical ownership before useful coding acceptance, reusing process-wrap10.0.1 and the M44/M46 Job/uncancelled-wait/joined-IO approach — ordinary selected-shell-to-Node already spawns a descendant and direct-child cancellation is insufficient — cost if wrong: live acceptance stays held, quarantine/occupied ownership remains truthful, and actual cleanup RED/GREEN must pass; preserve current non-Windows behavior without extending Windows proof to it.

Ruling: Reuse current runtime/tools/todos/receipts/helper links and exact quiet restart inspection; add only demonstrated native gaps — coding parity needs useful effects and recovery evidence, not a new engine — cost if wrong: bound scope and failed-quality evidence remain reviewable, no automatic replay, new journal, registry, dependency or speculative SDK/web rewrite.

## Released interfaces and ownership (binding)

- `summarizeInvoices(csvText)` is the sole module export from the declared source file and returns the canonical output CSV string. The CLI is `node src/cli.mjs --input <path> --output <path>`; validate completely before replacing output. No dependency install, process creation or network in the utility.
- Supported input is the declared unquoted CSV subset: exact `id,customer,status,amount` header, nonempty id/customer, open or void status, nonnegative digit amounts with exactly two fractional digits; reject malformed rows, quotes and embedded field delimiters. Header-only input is valid; a missing header is invalid. Preserve spelling/case, compare Unicode code points explicitly and retain exact cents for large values. CRLF input may normalize to LF output. Fixtures stay within16KiB input/32KiB source.
- Shell author owns only `hosts/rust-daemon/src/tools/process/shell.rs`, `tools/process.rs`, `tools/tests.rs` and `agent_runs/stop_tests.rs`. Read the existing process-wrap10.0.1 features/APIs and propose any necessary Cargo feature adjustment before editing its manifest. Tests first, no production before root-observed RED. Git Bash is currently installed/preferred on Windows; preserve actual launcher selection rather than assume cmd.
- Oracle author owns only `packages/core-rust/acceptance/coding-m47/**`: deliberately buggy fixture, visible tests/data/readme, independent oracle/self-tests/reference and immutable expected manifest. Freeze literal expected outputs and neighbors; actual task runs on a separate synthetic copy. Do not place the reference/oracle inside candidate authority.
- Native acceptance author owns only `hosts/rust-daemon/src/agent_runs/coding_tests.rs`, `agent_runs.rs` test registration and necessary small `agent_runs/test_support.rs` helpers. Reuse current real handlers/model test support. No production runtime fixes without a concrete RED and controller ruling.
- Root owns the existing-core Nx target, all commands/formatting/Git/services/models, plan/handover/master/ledger and final review settlement. No worker commands, compiler, browser, daemon, model or external messages.
- Real shell RED fixtures must have self-expiring owned children and guaranteed cleanup before assertion propagation; prove the vulnerable behavior before waiting out the cleanup TTL. Never terminate a PID without exact owned process identity. A passing synthetic baseline is not useful-task quality acceptance.

## Demonstrated gaps and bounded corrections

The existing selected Git Bash path failed actual saved Stop, live caller Drop,
queued caller Drop and direct Stop/timeout cases before cleanup. Windows now uses
maintained process-wrap Job ownership, an uncancelled whole-job wait and joined
bounded readers. A caller dropping its future cannot abandon the owner or cancel
an unrelated shared run. Leader-exit coverage already passed at baseline and is
retained without claiming it as a newly fixed defect. Non-Windows behavior is
preserved; Windows whole-tree evidence does not establish portable containment.

The configured model timeout previously bounded helpers but not parent adapter
calls. Two controlled loopback tests demonstrated the missing generate/stream
deadline. The small shared adapter wrapper now bounds the complete configured
call, including credential acquisition and partial response bodies, and keeps
`None` unchanged. Four focused cases pass, with no retry or new dependency.
Dropping an HTTP future does not establish remote-provider cancellation or a
billing refund.

The full gate also exposed startup producers contending with synchronous router
installation. Both real startup paths now finish construction before starting
restored jobs/connectors/scheduler/scanner workers. The independent HTTP timeout
regression test now uses a gated model and an explicit completed/zero-in-flight
barrier, retaining first 408 / retry 200 evidence without a 10 ms scheduling assumption.
The original failures remain in the private ledger.

Focused native acceptance passed 11 cases with five intentionally ignored real
shell selectors. The separately selected real-shell coding case passed actual
failed tests, repair, passing tests/CLI, exact approvals and six persistent paired
action receipts. Its 54-byte output SHA256 is
`fa5a0c0d4f17139c2f8a9dfc60ed771134702954d1b1929f339731f18dc6091e`.
ScriptedModel selected those steps; this proves wiring rather than model quality.

Four physical tests cover six lifecycle cases. Independent Windows kernel
PID/image/creation-time/ancestry sampling observed 32 controlled identities and
none remaining; all six private roots were removed and neighbors survived the
relevant observations. Optional test-only readiness acknowledgements capture live
kernel identities before Stop/drop, without delaying post-action assertions. An
earlier functional pass missed a short-lived child in OS sampling and is retained
as incomplete physical evidence, superseded by the final witness run.

The independent oracle self-tests passed 7/7: reject the buggy baseline, accept
an independently implemented reference and reject four semantic mutants. Literal
cases cover eight valid inputs, 18 malformed inputs and four invalid CLI argument
shapes. Actual provider quality, complete current gates and quiet process restart
are recorded separately after the single authorized task; no blanket parity
claim follows from these deterministic checks.

## Final current-tree gates and actual outcome (2026-10-15)

Required local gates passed: **2,672 Rust/core tests, 16 intentional ignores,
40 suites, lint and build**. The final full suite used two test threads; native
concurrency tests retain their explicitly spawned concurrency. Owned Prettier and
five-document link/path checks pass. SDK/web and Cargo.lock remain unchanged;
the final affected SDK/web review found no actionable issue. Windows physical
and real-shell coding selectors were also run separately as described above.

Exactly one real primary task was admitted through the ordinary ChatGPT
subscription adapter (`gpt-5.5`). Run
`run_c760eca7-1ca4-4603-afa6-628ddc13201a` failed after approximately 1.1 seconds with
static `ChatGPT subscription request failed (HTTP 400)`. The server's detailed
body was not retained or exposed; the underlying rejection cause is unproven.
There were **zero helpers, model steps, action receipts, Exec approvals or output
files**, and zero usage rows. One failed provider attempt occurred; zero recorded
tokens do not establish zero billed tokens. No retry, alternate provider, second
verification request or manual candidate repair was performed.

All six candidate files and both neighbor files retained exact bytes and
modification times. The independent oracle rejected the unchanged buggy source
at 14 module/CLI output checks, identically before and after restart. This is a
failed useful-task attempt, not autonomous coding or delegation quality evidence.

An actual quiet same-store process restart retained the failed run, its one
original user message, empty saved todos/receipts, agent configuration, session
links and empty usage exactly, without replay. Canonical memory was absent in
this fresh profile; no memory-content recovery is claimed. Both recorded daemon
and PowerShell-wrapper identities retired and port 18747 is free. This forced
quiet restart is not graceful-shutdown proof. The synthetic evidence remains
private and is never committed.

**Next priority:** diagnose subscription request compatibility with controlled
wire fixtures and safe static errors before another useful provider task. The
327-line native-voice draft remains held; it does not supersede this blocker.
No general Hermes parity claim or supported-provider removal follows from one
HTTP 400. M40 introduction quality, broader coding, accounts and host acceptance
remain open.
