# M25 Single Unconfigured-Tool Recovery

**Status:** active on g9000/companion-agent-loop-m25 from merged M24 main7f0ffe1. Root binds this scope below. Before implementation, no production changes,
tests, gates, provider calls, service activity, or runtime acceptance are part of
this draft.

## Goal

Let the model recover once when one model response requests exactly one tool that
is not configured for the current agent. Return a paired, static tool refusal and
allow one bounded ordinary model continuation to correct course using the original
grants. A second unconfigured-tool refusal is terminal.

This is a narrow validation recovery. It does not enable tools, repair names,
select alternatives, retry effects, or grant authority. Existing tools remain the
only dispatchable tools.

## Evidence and current behavior

- Cached public Hermes reference:
  `.superpowers/sdd/2026-10-12-companion-agent-loop-m23/upstream-turn_tool_validation.py`.
  Its `validate_tool_calls` returns `continue` after appending a paired
  tool-role error for an unknown call, and returns a partial terminal result at
  its third consecutive invalid-name response. It also has name repair, mixed
  batch handling, argument normalization, retry counters, provider ID
  normalization, and persistence behavior. Those broader mechanisms are not
  ported here.
- Native capability authority is `AgentConfig::allows_tool` in
  `packages/core-rust/crates/anima-core/src/agent.rs`. It checks the current
  agent's configured descriptor names.
- The relevant native branch is in
  `packages/core-rust/crates/anima-core/src/runtime.rs`, after the model response
  and before tool-round increment, assistant-call recording, pending-step
  persistence, observer tool-start frames, or executor dispatch. Today, any
  unconfigured call immediately enters `fail_step` with
  `tool_not_configured_error`.
- Native `ToolCall` contains the call id, name, and structured arguments. Existing
  `content_with_tool_calls` and `content_from_tool_result` helpers encode the
  assistant request and the matching tool result. Existing room message
  recording and transcript order must be reused.
- `AgentRuntime` owns the tool-round counter and control checks. `anima-harness`
  supplies a `ToolSet` executor; the daemon supplies its own callback and keeps
  receipt/approval checks. The runtime's config grant check remains authoritative
  before either callback.
- Current harness tests already cover a terminal unconfigured-tool denial and
  configured ToolSet dispatch. Current core runtime tests cover control, paired
  tool messages, budget limits, and step lifecycle. Root should inspect the exact
  test names before binding; this draft does not run them.

## Recommended bounded behavior

1. Eligibility: the response has exactly one `ToolCall`, and that call's name
   fails `self.state.config.allows_tool`. This is the only recoverable shape.
2. First eligible refusal in a run: record the model's assistant tool-call
   message and one matching `MessageRole::Tool` result using the original call
   id. The result is a fixed, host-generated sentence explaining that the tool is
   not configured and the model may continue only with configured tools. Do not
   include call arguments, unknown name, registry contents, or guessed
   alternatives.
3. Increment the existing tool-round budget once for this response. The paired
   refusal is a validation round; it does not count as a dispatched tool or a
   completed effect. Then continue through the existing next model turn, using
   unchanged config and tool grants.
4. Any second unconfigured-tool response in the same run is terminal through the
   established incomplete/error path. Do not append a second recovery refusal,
   dispatch it, or exceed the existing tool-round cap.
5. All other current capability checks stay in place. Configured calls follow
   the existing dispatch, receipt, approval, result-pairing and persistence
   path. No automatic tool enablement or reconfiguration occurs.

A valid recovery response may be ordinary text or configured tool calls, subject
solely to the unchanged round budget, stop signal, configured grants, executor,
approval and receipt boundaries. This is a single correction opportunity, not a
promise that the user's task completes.

## Excluded cases

- Empty or absent tool-call arrays with a tool-request stop reason remain terminal
  under the current malformed-response handling.
- Multi-call batches containing any unconfigured name remain terminal as a whole.
  Do not execute valid siblings, add partial batch semantics, or emit result stubs.
- Malformed tool-call data and invalid arguments retain their existing adapter or
  runtime outcomes; M25 adds no JSON repair or schema validation retry.
- A configured name rejected later by a host executor, approval policy, receipt,
  or authorization check is not this case. Preserve that exact failure and its
  no-replay behavior. Do not reinterpret arbitrary callback errors as an
  unconfigured name.
- No typo suggestions, closest-name matching, model-generated selection,
  automatic grants, registry lookup prompt, or repeated retries.

## Precise native interface and pairing

Keep the behavior private to the existing response-processing branch in
`anima-core/src/runtime.rs`:

- Check cardinality before the current `.find(!allows_tool)` rejection. Only
  `tool_calls.len() == 1` can enter recovery. Empty/missing calls continue into
  current malformed handling; a multi-call batch with any denial takes the
  existing terminal refusal path.
- Use the configured check on the current runtime state. Do not query host
  registries or mutate `AgentConfig`.
- Keep a per-invocation boolean such as `unconfigured_tool_refusal_used`, reset
  for every run. Do not persist it in snapshots, `AgentConfig`, or shared state.
- On the first eligible refusal, create a normal assistant message carrying the
  original `ToolCall`, followed by exactly one tool-role result produced from a
  fixed `TaskResult::error` or an equivalent fixed-content helper. Use the
  original call ID for pairing. Preserve any response text under the existing
  content handling; do not fabricate a successful result.
- Advance the existing round count once. Re-enter the existing control/Stop
  checkpoint before another provider request. If Stop is signaled, Stop wins and
  there is no extra model call.
- On the second denial, retain existing incomplete/terminal semantics. No
  assistant-call row or result row is added for that second refusal unless the
  current terminal behavior already records it; do not leave an unmatched call.
- Never call `prepare_tool_steps`, write a pending DB step, emit tool-start or
  tool-finished frames, invoke a callback, create an action receipt, or claim an
  external effect for the refused call.
- The next request uses the exact persistent tool configuration and original
  adapter. The refusal is visible only through the normal paired conversation
  history. This can add a bounded ordinary model continuation compared with the old terminal denial; there is no separate repair request or effect replay.

Root should confirm helper ordering and cancellation checkpoints against the
current source before implementation. If normal pairing requires a small private
helper, keep it in the same runtime module.

## Five risks and controls

1. **Authority expansion:** A recovery response must not enlarge the original
   configured tool set. Assert config and grants remain unchanged; dispatch only
   configured names.
2. **Confusing validation with host refusal:** This branch applies only to the
   runtime's `allows_tool` pre-dispatch denial. A configured tool's executor,
   approval, receipt, or authorization failure remains terminal as currently
   defined.
3. **Transcript integrity:** Preserve exactly one assistant-call/tool-result
   pair for the first refusal, original ID, and no unmatched second call. Do
   not fabricate action receipts or success frames.
4. **Budget/control priority:** The first refusal consumes one existing round;
   the correction consumes only rounds it actually requests. Existing cap, Stop,
   provider failure, receipt and deadline paths remain authoritative.
5. **Prompt/privacy safety:** Use a fixed, concise error without unknown names,
   raw arguments, registry listings, or suggested substitutes. Never echo model
   supplied content in trusted system guidance.

## Minimum regression set if root binds M25

- One unconfigured call receives one fixed paired refusal, reaches one existing
  next model request, and can finish with text; assert the callback count stays
  zero and the original configured tools are still present.
- First unconfigured call followed by a configured call dispatches that call
  exactly once through the ordinary callback and returns the ordinary paired
  result. The refusal itself produces no DB step, receipt, or tool frames.
- A second unconfigured call ends terminally with no third model request and no
  unpaired assistant tool call. The refusal state resets on the next independent
  run.
- A multi-call batch containing an unconfigured name remains terminal and does
  not dispatch a configured sibling. Empty/malformed responses retain existing
  behavior.
- Stop between the first refusal and the next model request prevents that
  request. The callback remains untouched and no receipt is created.
- A configured tool whose host callback returns an authorization/approval/
  receipt error remains on its existing terminal/no-replay path; it is not
  converted into this model-correction refusal.
- Refusal text contains no unknown name, args, registry contents, or suggested
  alternative. The assistant call and tool result retain the same call ID.

## Verification if bound

- Add focused deterministic tests in the existing core runtime test module and
  harness tests only if the reusable harness surface needs an assertion. Reuse
  scripted adapters, callback counters, existing cancellation controls and
  transcript inspection.
- Inspect current Nx targets before using them. Root owns all Cargo/Nx gates and
  serializes validation; this planning task runs none.
- No provider, HTTP service, database, browser, connector or API acceptance is
  required. Deterministic tests prove native branch behavior only.

## Decision for root

Bind or reject the one-call, one-correction scope before implementation. The
conservative value is one because it permits a small model self-correction while
placing a hard ceiling on validation chatter. Do not expand this into Hermes's
name repair, mixed-batch execution, malformed-argument retry, strike framework,
configuration surface, or host-specific policy map.

## Controller rulings on the risks (binding)

1. Ruling: Recovery is only the first single unconfigured call with a nonempty bounded name/ID, bounded canonical finite arguments and a fresh ID absent from current conversation calls/results — do not widen malformed/batched requests or ambiguous history — cost if wrong: retain existing terminal denial, zero callbacks/receipts/effects.
2. Ruling: Exactly one paired assistant-call/static Error result, original ID/arguments, no ToolStarted/ToolFinished/native action receipt/pending DB step or executor invocation — refusing a requested action is not performing it — cost if wrong: transcript/frame/callback regressions block merge; no fake outcome witness.
3. Ruling: Consume one existing tool round and permit one correction opportunity with unchanged tools/config/provider; the second unconfigured denial stays terminal — this intentionally adds bounded model continuation relative to the old failure, never an independent repair request or effect replay — cost if wrong: budget0/max1/second-denial tests fail with no unauthorized effect or retry loop.
4. Ruling: Configured-tool host authorization/approval/receipt/provider failures retain their current paths; no name guessing, auto-registration/grant, raw private argument/error in the refusal or system hint — the model can choose only authority already granted — cost if wrong: preserve denial/Stop and fail closed, no account or credential action.
5. Ruling: Per-invocation refusal state only; reuse existing pairing helpers and private argument bounds, reset M24 advisory on this non-executed round, same Stop checkpoint before any continuation/M23 no-tools priority — avoid a new framework/DTO/persistent policy — cost if wrong: miss recovery and retain truthful prior effects; meaningful core/harness and host-scope regressions plus batch review/current-tree gates required.

## Root execution decisions

Baseline7f0ffe11acbbbb88bcadd02214055b5cd2cb879f; branch cut from verified
merged M24 after its gates/review/PR30. Only original nx.json dirty; all services
off, original synthetic settings restored. No dependency/feature/new targetdir,
new public settings/DTO/host wiring, real provider test or service/account action.
Every Cargo CARGO_INCREMENTAL=0, serialized by root; workers source-only.

Use existing capability bounds for eligible recovery name/ID/arguments. Invalid,
over-size/nonfinite/deep arguments or a reused ID remain terminal. Any private
extraction of M24 bounded-argument helper must preserve its existing guard tests;
no broad refactor. Static refusal does not include tool name, raw args or arbitrary
provider errors. Original request history retains its actual call as data.

M23 budget exhaustion takes precedence: max0 never recovers a tool request;
max1 recovery consumes the round and the next call is its no-tools wrap-up.
Normal handler/host permission errors are not this validation repair. No simulated
ToolBefore/ToolAfter event claiming an admitted/executed action. Finish the model
step using its existing StepFinished convention and keep usage/text truthful.

Public upstream revision1fd18277267152893a8a24792b79213e7ca3f899:
agent/turn_tool_validation.py::validate_tool_calls. Adapt only paired validation
refusal/continue; upstream third-consecutive-name cutoff, name correction and
mixed-batch machinery are not imported. Native is single recovery per invocation.
Retain full MIT attribution, Anima product naming, no Python/profile/token bridge.

## Verified result

Implemented the bound above. Focused8core/1harness and required Rust2381passed/
8ignored/40suites/lint pass. SDK130 and web1192/typecheck/build pass. Native batch
review had no findings; final SDK/web review led to one caption/fixture fix wave:
chat summaries and interrupted warnings now describe requests rather than
claiming execution. A transcript-backed static refusal test confirms error status,
no recorded action and no reported success. No live provider/service run for M25.
Publication source/PR/exact merge/resume are recorded in the ignored milestone
ledger; this document is committed with the tested implementation.
