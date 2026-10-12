# M23 Native Agent-Loop Budget Wrap-Up Plan

> **For agentic workers:** Root executes bounded steps under `superpowers:executing-plans`, owns the five binding rulings, branch, gates, runtime activity and Git. The planner supplied the lean draft; root binds and executes this tracked plan.

**Goal:** After the configured tool-round budget is used, let the reusable Rust loop produce one honest final answer with further tools unavailable, while retaining every existing failure/Stop boundary.
**Architecture:** Change `anima-core`'s next model turn at the budget boundary into a one-shot wrap-up using a cloned call configuration without advertised tools. `anima-harness` inherits the same behavior; daemon receipt/approval/commit logic stays authoritative. Normalize the existing ChatGPT no-tools request shape only where required by that call contract.
**Tech stack:** Existing Rust core/harness/model adapters and deterministic scripted adapters; no dependencies or new service/process.
**Status:** Implemented and verified on g9000/companion-agent-loop-m23. Main baseline: `a1d1af69c2ae1bd23e2194ba6978b29c7c10b810`; baseline had only unrelated `nx.json` dirty. Final services are stopped; publication identities are in the milestone ledger.

## Authority and boundaries

- The user's renewed instruction, “continue please to parity to hermes agent loop haha,” authorizes core-loop progress; earlier broad-parity freeze is superseded for this scope.
- Integration scope stays small: one budget-wrap-up capability and its necessary adapter contract correction, no broad Hermes feature port.
- Planner reads cached tracked public source only; it does not execute Python, start services, inspect normal profiles/credentials or make provider calls.
- Root owns source edits, tests/gates, services/API, external publication and Git. Preserve unrelated `nx.json` and all private acceptance data.
- No new dependencies, public configuration fields, project targets, target directories, worktrees, stash/reset/restore/path checkout or database/connector infrastructure.
- Reusable orchestration remains in `packages/core-rust`; do not move it into `hosts/*` or make core depend on host HTTP/DB/runtime implementations.
- No automatic effect replay, provider fallback chain, tool-schema repair, additional summary retry or evaluator retry expansion.
- Stop, approval denial, native receipt failure, provider failure and explicit deadline interruption remain terminal at their existing boundaries.
- No real-provider acceptance is required to prove deterministic loop behavior. A later live observation needs root's bound cost/timing decision and distinct reporting.
- Existing reviewed-gate/unsigned merge authority stays with root; the plan adds no user approval gate.

## Public reference and evidence limits

Cached Hermes tracked public HEAD: `1fd18277267152893a8a24792b79213e7ca3f899`.
Reference files live under this ignored M23 directory as `upstream-*.py`; do not execute/import them or copy provider/profile machinery.
Source inspection establishes behavior/code paths, not successful current Hermes operation or live-provider quality.
Root additionally cached `upstream-turn_finalizer.py` and the public license for the same revision; the finalizer activation policy is inspected directly.
`upstream-turn_finalizer.py:124–170` only requests budget summary when no final response exists, budget is exhausted, the turn is neither failed nor interrupted, and exit reason is unknown/budget_exhausted.
The cached `handle_max_iterations` implementation is the concrete comparison for wrap-up behavior; its transport behavior and retry count are inspected directly.
If adaptation copies nontrivial upstream text/code, root checks existing attribution/notice requirements; prefer a small native Rust design using the behavior.

## Compact core-loop parity matrix

| Loop concern                            | Cached Hermes source anchors                                                                             | Current reusable/native source anchors                                                                               | Assessment / M23 disposition                                                                        |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Model → tool rounds → model             | `upstream-conversation_loop.py:1648–1699`; `upstream-turn_tool_round.py:46`                              | `anima-core/src/runtime.rs:482–946`; `anima-harness/src/harness.rs:95–137`                                           | Present; retain shared loop and host-independent dispatch callback                                  |
| Budget units                            | `upstream-conversation_loop.py:1648–1650` checks API-call count and shared iteration budget              | `runtime.rs:471–480,693`; `agent.rs:35–36`; harness builder `config.rs:132–137`                                      | Different semantics: Anima counts tool-calling rounds, default 8; preserve that meaning             |
| Graceful maximum-budget summary         | `upstream-turn_finalizer.py:124–170` excludes failures/interruption; helpers `2309–2363`                 | `runtime.rs:652–663`; harness `tests/harness.rs:183–204` currently expects hard failure                              | Concrete gap selected for M23; same no-fallback-after-failure rule                                  |
| No-tools wire shape                     | Helpers `2257–2283`: Codex removes tools/choice/parallel; Anthropic tools=None                           | OpenAI-compatible/Anthropic/Google builders omit absent tools; `chatgpt.rs:149–164` always emits array/auto/parallel | Required small companion gap: proper absent-tools ChatGPT shape                                     |
| Summary attempt count                   | Helpers `2334–2347`: up to two attempts for empty text, discards tool calls in `_summary_text:2244–2253` | Native has no distinct wrap-up phase                                                                                 | Native deliberately stricter: one next call only; never dispatch summary tools or retry empty text  |
| Chat-completions summary schemas        | Helpers `2286–2303`: retains tools for cache-prefix compatibility and discards requested calls           | Adapter schemas derive from supplied `AgentConfig.tools`                                                             | Do not claim Hermes uniformly sends no tools; native removes schemas for a hard capability boundary |
| Repeated failures/no progress           | `upstream-tool_guardrails.py:114–137,308–436`; thresholds, mutation progress resets, idempotent reads    | Native iteration cap, typed tool result status and DB recovery at `runtime.rs:772–944`                               | Real separate gap, queued; no generic tool-name heuristics this milestone                           |
| Identical calls/cycles/poll exceptions  | Guardrails `32–49,85–100,323–337`                                                                        | No per-run repeat/cycle detector in inspected core loop                                                              | Queued; distinguish legitimate polling/edit→read from loops                                         |
| Invalid tool calls                      | `upstream-turn_tool_validation.py:45–84` and validation pipeline                                         | `runtime.rs:664–691` rejects missing/unconfigured calls                                                              | Present stricter native denial; no repair/retry widening                                            |
| Parallel rounds and paired results      | Hermes `upstream-turn_tool_round.py:46–175`                                                              | Native `runtime.rs:711–944`, `join_all:799` and one tool result per requested call                                   | Present; wrap-up must not create unmatched assistant calls                                          |
| Persistence failure before continuation | Hermes round `128–175` halts on canonical message/result persistence failure                             | Host `agent_runs/action_receipts.rs`; tools journal; core cancellation checkpoints                                   | Present host-native fail-closed boundary; no wrap-up after receipt cancellation                     |
| Stream interruption / partial text      | Hermes summary helper `2349–2354` propagates interruption                                                | Native `runtime.rs:519–549,1070`; `runtime/control.rs`                                                               | Present; wrap-up uses the same sink/cancel path                                                     |
| Error classification/provider retries   | Hermes `upstream-turn_loop_errors.py:35–58`; `upstream-turn_retry_state.py`                              | Native stream error branch `runtime.rs:938–953`; adapter-specific transport errors                                   | Native fails immediately; retry/fallback policy remains separate                                    |
| Final validation/evaluators             | Hermes validation helpers, finalizer import not cached                                                   | Native `runtime.rs:554–648`, bounded evaluator retry counter                                                         | Preserve validation; wrap-up evaluator rejection gets no second wrap-up                             |
| Embeddable harness and configuration    | Hermes `run_agent.py` hosts its loop/control methods                                                     | `anima-harness/src/config.rs:132–137,207–230`, `harness.rs:95–137`, ToolSet dispatch                                 | Present; harness test demonstrates behavior without daemon or credentials                           |

## Candidate approaches and selection

1. **Recommended: switch the already-next call into a bounded no-tools wrap-up.** Same tool-round budget and existing cancellation/stream/usage pipeline; gives useful final prose rather than a blanket iteration-limit error. Requires one adapter no-tools correction and clear evaluator boundary.
2. **Keep hard failure, add only a better error string.** Very small but does not close Hermes' graceful-answer gap; loses available completed-tool context as an answer.
3. **Implement repeated-tool guards first.** Useful but needs explicit progress, poller and mutation semantics across hosts/tools; higher false-positive and policy scope than this milestone.

Select approach 1. Approach 3 is a future scoped gap, not a second independent feature in M23.
The implementation may use a small private wrap-up state/helper in the existing runtime; do not introduce a generic orchestration framework or public mode API.

## Precise behavior contract

### Enter wrap-up only at the next ordinary call boundary

- The budget is still tool-calling rounds, not model calls, tool count, tokens or wall-clock duration.
- A multi-tool batch consumes one round exactly as today; every admitted call settles/records through existing paths.
- After the last allowed round completes and its results join the conversation, the next model call is the one permitted wrap-up call.
- Do not first send the ordinary tool-enabled request and then add another summary call; avoid the existing doomed tool request and avoid an extra round of cost.
- Natural text completion before the budget remains unchanged.
- `max_tool_iterations=0` allows an immediate text-only answer; it admits zero tool dispatches and remains a valid explicit cap.
- If Stop/cancel is already signaled, finish stopped before any wrap-up request, using the existing checkpoint.
- Ordinary handler failure with a returned tool error may be summarized honestly; infrastructure/receipt cancellation or provider failure may not be recovered by wrap-up.
- Do not create tools, appends, retries, writes, helpers or schedules during wrap-up.

### Build a capability-restricted request without mutating persistent configuration

- Clone the per-call `AgentConfig` and set `tools:Some(Vec::new())`; do not clear the runtime's persistent granted tools or tool registry. Explicit empty avoids custom adapters interpreting absent tools as permission to restore defaults.
- Use the same model/provider/settings, authenticated adapter, original system/identity/host guidance and correlated conversation history.
- Append bounded host-generated guidance to the request system text: tool-round budget exhausted, no more tools, explicitly state confirmed completed work and remaining work/limitations, do not claim unfinished actions completed.
- Do not insert a fake user instruction or commit a synthetic successful action into chat history; the provider sees actual settled tool results.
- Preserve the ordinary temperature/max-token values and request model identifier; do not switch providers or increase caps.
- Do not add a public `ModelGenerateRequest` field solely for this mode: the existing adapter contract already receives the call configuration and request separately.
- The no-tools instruction complements a hard dispatch check; model compliance alone never grants another action.
- Root must inspect all existing adapter tool-shape builders in the touched tree, without adding provider connections or new provider-specific fallbacks.

### Finish the single wrap-up safely

- Reuse existing `StepStarted`, stream sink, cancellation select, usage accounting and step IDs.
- Nonempty End text passes existing final evaluators and commits one final reply through the ordinary runtime/host result path.
- A wrap-up ToolCall response is rejected before callback/approval/receipt/tool dispatch; retain bounded partial text as incomplete and fail with the established iteration-limit reason.
- Reject a response that carries tool calls even if its normalized stop reason falsely says End; never mark such a tool request completed or emit unpaired assistant calls.
- Empty/whitespace final text ends with a controlled error and no further model call; do not silently publish successful empty completion.
- MaxTokens during wrap-up retains visible partial text but is incomplete/error; no continuation/retry and no claim that a complete summary was generated.
- Evaluator Abort remains terminal; evaluator Retry during wrap-up becomes the existing bounded rejection/error outcome rather than another model turn.
- Stream/provider error preserves partial text and surfaced usage and fails through the existing path; no summary fallback on an infrastructure failure.
- Owner Stop while wrap-up streams preserves partial text as stopped, settles the run normally and does not dispatch further effects.
- A wrap-up answer ending successfully means a committed reply, not proof that the user's broader task or external effects completed.
- The next independent run resets wrap-up state and can use its normal tool grants/budget; snapshot/restore must preserve original configuration.

### Result provenance without changing host outcome meanings

- Use stable final Content metadata key `budgetWrapup:true` for an accepted wrap-up reply; avoid multiple competing keys.
- Tag only the accepted wrap-up result/reply, never earlier natural answers or an unexecuted tool request.
- Preserve the marker when tagging run/step IDs and through existing final content commit/snapshot paths.
- Keep current natural-response success behavior outside wrap-up unchanged.
- Completed means the reply finished/committed, not that an entire user goal or job was verified complete.
- Do not rewrite job/goal/schedule outcome pipelines or add a new DTO status to represent that semantic limit.

## Files and interfaces

| File                                                                  | Responsibility / expected edit                                                                                                                           |
| --------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `packages/core-rust/crates/anima-core/src/runtime.rs`                 | Private one-shot wrap-up mode at existing loop boundary; call-config clone; safe terminal branches                                                       |
| `packages/core-rust/crates/anima-core/src/runtime/run_tests.rs`       | Scripted/capturing adapter coverage for boundary, request, callbacks, reset and bounded failures                                                         |
| `packages/core-rust/crates/anima-core/src/runtime/control_tests.rs`   | Stop before/during wrap-up; zero further provider/tool callbacks                                                                                         |
| `packages/core-rust/crates/anima-core/src/runtime/observer_tests.rs`  | Only if needed to prove existing stream/usage/step pairing behavior for new path                                                                         |
| `packages/core-rust/crates/anima-harness/tests/harness.rs`            | Retain adversarial hard-cap expectation and add configuration-aware meaningful wrap-up success                                                           |
| `packages/core-rust/crates/anima-harness/src/config.rs`, `README.md`  | Budget meaning/documentation; builder signature stays unchanged                                                                                          |
| `packages/core-rust/crates/anima-model-adapters/src/chatgpt.rs`       | Omit tools/tool_choice/parallel_tool_calls together when supplied config has no tools                                                                    |
| `packages/core-rust/crates/anima-model-adapters/src/chatgpt/tests.rs` | No-tools body preserving paired historical tool results and normal tools shape                                                                           |
| Existing adapter body tests                                           | Narrow assertion that no-tools config does not advertise schemas; no blanket provider rewrite                                                            |
| Host existing receipt/Stop regression files                           | Root selects one bounded regression proving no next model request after receipt save failure/owner Stop if coverage does not already hit budget boundary |
| Established companion plan/daily-use/handover + Hermes notice         | Final behavior, limitation and attribution update only after validated implementation                                                                    |

No changes are needed to owner schedule routes, schedule claim/recovery policy, Run DTO statuses, SDK/web model types or native action receipt schema.
Host tests may adapt fixtures to the new cap semantics; they must not weaken receipt/approval/Stop assertions or promise automatic effect replay.

## Five binding risks for root

1. **Capability boundary:** Clearing advertised schemas alone does not stop a bespoke/misbehaving adapter from returning tools. Bind hard rejection of every wrap-up tool response, including End+tool-calls, and assert callback count unchanged.
2. **Cost and failure priority:** Wrap-up replaces the already-next call and happens once. Bind no empty/truncated/evaluator retry and no call after owner Stop, receipt cancellation, provider failure or deadline interruption; no hard token/runtime cap overclaim.
3. **Transcript/receipt honesty:** Confirmed tool results feed the answer; final text does not fabricate action witnesses. Preserve incomplete/stopped markers, paired history, native receipts and host final commit behavior; no replay/backfill.
4. **Configuration/provider compatibility:** Clone call config without changing persistent grants. Bind `None`/empty no-tools wire shapes and ChatGPT strict Responses block omission, plus preservation of historical tool-call/result input. No provider-switch or credential read.
5. **Compatibility and evaluation:** Budget units/default remain rounds/8; zero rounds becomes intentional text-only completion. Bind one wrap-up evaluator acceptance/rejection policy and reset across harness runs/restores; report deterministic evidence separately from provider behavior.

### Root binding placeholder

- [x] Root accepts the chosen single capability scope, explicit shadow-config empty tools and one next-call wrap-up.
- [x] Root pins baseline/binary/tree ownership and serializes Cargo, preserving `nx.json` and private data.
- [x] Root binds final End evaluator Accept, wrap-up Retry/rejection fail-closed with no extra request, existing ToolCall iteration-limit failure and stable `budgetWrapup` provenance; empty/MaxTokens wording remains a routine implementation detail.
- [x] Root confirms existing target directories and relevant Nx targets; do not introduce a validation-only target directory.
- [x] Root decides whether deterministic evidence is sufficient for M23; no live test is presumed by this draft.

## Task 1: capture meaningful red regressions

- [x] Read the bounded existing runtime/harness test support and reuse scripted adapters/callback counters; no new framework/dependency.
- [x] Script one allowed tool round then a text answer; capture each supplied model config/request, callback count, model-call count, transcript and original runtime configuration.
- [x] Assert the second request omits tools and contains budget/confirmed-result guidance while prior tool arguments/results are paired in history.
- [x] Add immediate zero-round text-only case and a next-run reset case; the latter must demonstrate tools work again without editing configuration. Assert `budgetWrapup:true` only on the accepted final wrap-up content.
- [x] Keep a malicious ToolCall wrap-up case proving the handler never runs a second time; add End+tool-calls malformed response case.
- [x] Run focused red tests using root-confirmed existing Nx/Cargo test seam; record failure at the missing behavior, not incidental fixtures.

## Task 2: implement the one-shot native mode

- [x] Add private per-run wrap-up state at the existing next-call boundary after cancellation/steering checks, with no new public config field.
- [x] Build the cloned no-tools call configuration and bounded request guidance; keep ordinary provider request construction unchanged outside this mode.
- [x] Use the same stream sink/cancel select/accounting path; add explicit wrap-up response branches before any tool dispatch or evaluator retry.
- [x] Accept nonempty final text through existing evaluator/commit path; fail bounded empty/truncated/rejected/tool-emitting responses without another request.
- [x] Verify mode resets each run and persistent configuration/grants/snapshot remain unchanged.
- [x] Run focused core tests once after the change; extend only for actual failures/new changes.

## Task 3: normalize adapter contract and prove harness reuse

- [x] Add failing ChatGPT request-shape test for absent tools with prior assistant tool calls/tool outputs in input.
- [x] Omit the tools/choice/parallel block together when config tools are None/empty, retaining stream=true/store=false and all existing authentication/model/input behavior.
- [x] Verify ordinary configured tools still send their existing schemas/choice/parallel fields; no global forced-none policy.
- [x] Assert other existing adapter no-tools builders remain valid via relevant existing body tests; correct only a demonstrated request-shape defect.
- [x] Preserve existing adversarial harness budget test: a model that ignores empty tools and requests a second effect still fails. Add an adapter that honors shadow-config empty tools and produces meaningful final text with exactly one prior effect.
- [x] Document builder cap as tool-calling rounds followed by at most one no-tools wrap-up; no provider quality or enforced-token-cap claim.

## Task 4: prove terminal/fail-closed boundaries

- [x] Owner Stop before wrap-up: zero next request; Stop during wrap-up: cancelled partial text, no tool callback and no later request.
- [x] Receipt intent failure at the last tool round: cap-specific fixture proves no next call. Outcome-save failure retains inherited M21 cancellation coverage; no new cap-specific outcome fixture. Prior effect/witness remains truthful.
- [x] Provider stream error and missing final response: no recovery summary/retry; partial text/usage retain existing markers.
- [x] Empty, MaxTokens and evaluator Retry/Abort wrap-up cases: bounded error after one wrap-up attempt; no second continuation.
- [x] Parallel last-round batch: all admitted results stay paired, budget increments once and no extra call executes; completed native receipts remain unchanged.
- [x] Approval refusal does not become authority for another tool; stop/cancel priority is retained even if model text arrives during cancellation.
- [x] Review final core/adapter/host diff for reusable boundary and no changes to schedule admission, resume or receipt publication/order.

## Verification commands and gates

- Root first confirms project names/targets with `bun x nx show projects --json` and `bun x nx show project core-rust --json`; these are root actions, not planner execution.
- Core/harness/adapters required gate: `bun x nx run core-rust:test --skipNxCache`, using the existing configured target directory only.
- Required host regression/full gate for reusable Rust changes: `bun x nx run rust-daemon:test --skipNxCache`; Cargo serialized by root with `CARGO_INCREMENTAL=0` as bound.
- Relevant formatting/lint: `bun x nx run core-rust:lint --skipNxCache` after root confirms the resolved target; its existing configuration checks Rust formatting, with no new target.
- Root runs current-tree host lint/format targets required by its merge workflow; stopped services avoid executable locks, no new target directory fallback under this scope.
- Changed Markdown: `bun x prettier --check <exact changed files>`; `git diff --check`; root records exact gate counts and ignored tests.
- No SDK/web full rerun solely for unchanged surfaces; any actual touched TypeScript implementation gets its relevant existing Nx targets.
- Root obtains a bounded final review of the native loop + adapter contract + host failure boundaries; only then normal reviewed/unsigned PR/merge gates.
- No automatic live-provider call or repeat suite after passing gates absent new changes/failures/unresolved concerns.

## Exit criteria and limitations

- Budget exhaustion can produce one meaningful accepted final answer with no further tool capability; all adversarial/failure paths are bounded.
- Model/tool counts, original configuration and paired transcript provide deterministic proof; actual-provider adherence remains untested unless root separately observes it.
- Stop and native storage failure tests prove no new model request after those terminal signals; no action replay or receipt durability regression.
- Normal provider/auth/setup errors remain errors; the milestone does not add provider retry/fallback or hide failures behind fabricated summaries.
- Root's completion report names implemented behavior, relevant current-tree test evidence, remaining provider limits and next bounded gap.
- Any unresolved safety/cancellation/persistence regression blocks merge; do not mark parity complete merely because the wrap-up path works.

## Real acceptance gap and binding fix

The one real gpt-5.5 task read the actual input and honestly reported the write
unfinished after one tool round: two model steps, 3,692 reported tokens, one
persistent successful read receipt and no output file. Native control-plane
storage retained `budgetWrapup: true`, but owner session-message responses hid
the key through their existing metadata allowlist. Preserve that same completed
task; do not replay, backfill or spend another provider request.

Ruling: Expose the accepted marker as one optional key in the existing opaque
message metadata map and name it in OpenAPI — owner clients must be able to
verify stored provenance — cost if wrong: a coordinator-to-route regression
and same-task quiet restart fail before publication. No new DTO status, SDK/web
implementation or tool authority. Relevant current-tree Rust gates must rerun
after this actual source correction.

## Future queue: three separate gaps, no implementation now

1. **Per-run repeated-tool/no-progress guard.** Cached guardrail source has exact failure counts, idempotent result repeats, mutation resets, identical cycles and poller exemptions. Scope a typed/native policy first; avoid general name-based inference or blocking legitimate polling.
2. **Structured invalid-tool recovery.** Hermes tool validation supplies paired error results with bounded repair strikes; Anima currently terminates on missing/unconfigured requests. A later design must preserve denial/auth boundaries, never auto-enable tools and never rerun previous effects.
3. **Phase-aware provider retry/continuation.** Hermes separates deterministic local/persistence failures from transient API retry state and interrupted streams. A later native policy needs bounded cost, safe transcript/usage handling and no provider failover/credentials expansion without explicit scope.

## Verified execution record

- Initial RED caught advertised tools instead of explicit empty tools in core
  and harness, and ChatGPT's invalid no-tools request block. Focused GREEN:
  nine core, one harness and seventeen subscription controls. The original
  adversarial harness overflow test remains unchanged and passes.
- Final required Nx Rust gate: 2,367 passed, eight ignored, forty suites;
  Rust formatting passes. One combined native review and one final SDK/web
  contract review found no actionable issues. No TypeScript source changed.
- One real task used two steps and 3,692 reported tokens, read actual synthetic
  content and honestly reported the write unfinished. One durable read receipt,
  no output. Saved provenance survived a quiet restart; the API allowlist gap
  above was repaired without replay or another provider request. Settings and
  grants restored, exact owned processes gone, ports free.
- Dedicated new tests cover before/during wrap-up cancellation, provider
  failure/partial usage, rejected tools/empty/truncation/evaluator Retry,
  parallel pairing, zero/default/reset, spoofed provenance and owner API
  exposure. Existing Abort/missing-final/approval/outcome-save boundaries are
  inherited, reviewed and included in the full suite; not every failure race
  has a new cap-specific fixture.
- No full parity or guaranteed goal completion claim. Next: separately bound
  M24 exact-failure advisory, then invalid-call repair and provider retry policy.

## Controller rulings on the risks (binding)

1. Ruling: The next model turn at the tool-round cap uses a cloned call config with explicit empty tools and a hard rejection of any returned tool calls, including End+calls — prompt guidance alone cannot enforce no effects — cost if wrong: fail with the existing iteration-limit reason before dispatch; no unmatched tool-call message or replay.
2. Ruling: Reuse the already-next call once, with no empty/truncated/evaluator/provider retry; owner Stop, deadline and receipt failure keep their terminal priority — graceful progress reporting must not create an expensive fallback loop or continue after infrastructure failure — cost if wrong: fail/stay stopped with partial usage/text, and meaningful no-next-call tests block merge.
3. Ruling: Honest system guidance and accepted final budgetWrapup:true metadata use actual settled tool results; completed remains a committed reply, not verified whole-goal completion — no fabricated action success/history or job/schedule pipeline rewrite — cost if wrong: retain incomplete markers/receipts and leave task verification to its existing owner boundary.
4. Ruling: Preserve persistent settings, model/provider, permissions and original transcript; normalize only the existing no-tools request shape needed by ChatGPT while checking other adapters — strict Responses backends should not receive tool-only fields without tools — cost if wrong: no extra provider switch/auth action, fail visibly and preserve original configuration for the next run.
5. Ruling: Keep rounds/default8 and support explicit0 as text-only; evaluators still accept or terminate, state resets per run/restore — this is one reusable native behavior, not a new host framework — cost if wrong: core/harness/adapter and host Stop/receipt tests fail before publication. Deterministic evidence and any separately bounded real test are reported separately.

Root accepts the lean plan under the latest explicit agent-loop parity request and
existing unsigned/gated-merge continuation authority. No separate audit/spec
approval/worktree. M24 no-progress detection is the next bounded loop gap; provider
retry and invalid-call recovery follow separately without broad connector expansion.
One optional real isolated max1 read-then-write task may test wrap-up after gates;
its first read must be actual, no further write is allowed during wrap-up, the final
reply must report remaining work and carry persisted budgetWrapup provenance. This
is one task with the existing normal internal ChatGPT auth; no source/profile/secret
bridge or billing/I/O cap claim.
