# M24 Repeated Exact Failure Warning

**Status:** implemented and verified on g9000/companion-agent-loop-m24 from merged M23 main9ee0d50. Publication identities are in the milestone ledger; no services or provider tasks ran for M24.

## Goal

Give the model one concise warning after the same single tool call fails in
three consecutive tool rounds with the same canonical arguments and identical
failure evidence. Keep the warning advisory and transient. The current tool
call's actual result and paired transcript record remain intact.

Do not block or replace calls, classify a run as failed because of the warning,
add a provider/tool turn, or claim repeated errors prove a loop. The ordinary
tool-round cap remains the only loop execution cap.

## Evidence and current contracts

- Cached public reference is
  `.superpowers/sdd/2026-10-12-companion-agent-loop-m23/upstream-tool_guardrails.py`.
  `ToolCallGuardrailController.after_call` increments an exact signature's
  failure count across intervening different failures, warns at `exact_failure_warn_after`, and resets
  the exact record after a success. Its failure classifier can inspect strings,
  result JSON, exit codes and tool-specific conventions.
- Hermes also has per-tool failures, no-progress/read counts, mutating-tool
  progress resets, configurable hard stops, loop caps, cycle guards, polling
  exemptions and result stubs. Those mechanisms are broader and rely on
  Hermes-specific classifications. M24 does not port them.
- Native `AgentRuntime` already awaits each tool executor and gets a typed
  `TaskResult<Content>` before it records the call-linked tool message and
  continues the model loop. The runtime counts tool rounds; one parallel batch
  consumes one round.
- Native `TaskResult` exposes `TaskStatus`, `error`, optional `data`, and
  duration. Treat a failure as valid only when `status == Error` and `error` is
  present. Inconsistent status/error/data combinations are malformed evidence;
  reset the streak and produce no warning.
- `ToolCall` provides tool name and a `BTreeMap` of arguments. Canonicalize
  arguments deterministically; never retain raw arguments or echo them in the
  warning. Do not use provider call IDs as repeat identity.
- The existing tool result is generated from the executor's actual
  `TaskResult`, and is paired with the original assistant tool call. The warning
  should not mutate that result, its call ID, or either transcript message.
- Core already depends on `serde_jcs` and `sha2`; no dependency is needed to
  build a fixed-size fingerprint. No descriptor bit, public config field, DTO,
  daemon registry classification, or harness wiring is needed for this failure
  behavior.
- The native core has no trusted poller map. Do not add a tool-name exclusion
  copied from Hermes. The rule only warns on three exact failures, never blocks
  unknown tools, and leaves polling/execution permission unchanged.

## Recommended narrow behavior

Track only one consecutive streak inside one invocation of
`run_with_context_and_tools_impl`. It consists of one 32-byte digest, a count
saturated at three, and a warning-emitted bit. No per-tool map, raw args, raw
error, result body, or persistent snapshot field is needed.

Eligible observation: a round contains exactly one `ToolCall`, its awaited
executor returns `TaskStatus::Error`, `error: Some(nonempty text)`, and no data.
The exact signature includes tool name, canonical argument bytes, and the full
error text. If any representation is noncanonical, oversized beyond the
existing bounded input/result contract, or otherwise malformed, clear the
streak without warning.

For the first exact failure, set the current signature count to one. An exact
match increments it. A different tool, argument set, or error clears the old
streak and starts a new signature at one because the current failed call begins
that new streak. At count three, set the one-warning bit and prepare one
transient host hint for the next model request. Further exact failures do not
repeat the warning until a reset.

Any successful result resets the streak. So do absent/inconsistent status or
error, missing/extra result data, empty/invalid error text, execution result
serialization failure, zero calls, or a multi-call round. A changed error or
argument starts a fresh one-count failure streak as above. A multi-call round
does not have an ordered single retry path and is deliberately untracked.

Warning wording should stay factual and nonjudgmental, such as: “This same tool
call has returned the same error three times. Inspect the latest error, change
the inputs or approach, or explain the blocker.” It must not include tool
arguments, private result text, IDs, credentials, or a verbatim error that could
contain untrusted data. Do not call the agent stuck or state that no progress
occurred.

Carry the hint only in transient `ModelGenerateRequest.system` guidance on the
next ordinary model call. Do not insert a user message, append text to a tool
result, persist a system transcript row, or make an extra model call to deliver
the hint. If Stop, provider failure, deadline, receipt cancellation, or the
existing budget boundary prevents another request, the hint has no effect on
the terminal outcome.

No known-poller exclusion is proposed: Hermes's name-based poller map has no
native semantic equivalent. A repeated typed failure can be useful for polling
tools too; the advisory never prevents another poll. Unknown tools are never
blocked.

## Interface and result pairing

The loop already has the necessary values, so keep the detector private to
`runtime.rs`:

1. After a one-call round's executor completes, read the `ToolCall` name/args
   and its exact `TaskResult<Content>` status/error/data.
2. Canonicalize the arguments through `serde_jcs` and hash a length-delimited
   tuple of tool name, canonical args and error text with `sha2`.
3. Update or clear the one local streak. Use saturating count and emit a hint
   only once on the third identical eligible failure.
4. Pass the unchanged `TaskResult` to the existing result conversion and
   record exactly one original call-linked `MessageRole::Tool` message.
5. On the next loop iteration only, append the fixed hint to request system
   guidance alongside ordinary system text or M23 wrap-up guidance.
6. Continue through the existing Stop, receipt, evaluator, round-budget and
   commit paths. The detector does not call the executor or adapter.

The detector must see the actual executor result, not a string heuristic over
rendered transcript content. It must not use result status text supplied by a
provider. Tool call IDs remain solely for the existing call/result pairing.

## Bounds and exclusions

- One per-invocation signature digest, count capped at three, warning bit, and
  fixed warning string. State resets at every new run.
- Only single-call rounds qualify. Multi-call rounds reset the streak and
  preserve current parallel dispatch behavior.
- No result stubbing, history scan, cycle/no-progress detector,
  same-tool-different-args threshold, per-tool caps, hard stop, retry, fallback,
  descriptor metadata, or user-facing configuration.
- No blocking, denial, approval change, synthetic failure, retry suppression,
  replay, or outcome status change. Unknown and polling tools continue to run
  under the existing round cap.
- No arguments, raw error body, call ID, or model/provider detail in warning
  text.
- No extra provider/tool call; no warning after terminal Stop, error, deadline,
  receipt failure or completed wrap-up.
- M23's budget wrap-up and adapter capability boundary remain unchanged. The
  warning may accompany a request that is already due; it does not authorize
  that request or extend the budget.

## Five binding risks for root

1. **Failure evidence:** Bind exact native semantics: only `Error` plus a
   nonempty `error` and no data counts. Inconsistent or missing fields reset;
   do not add Hermes's string/JSON failure classifier.
2. **Signature and privacy:** Require canonical name/args/error fingerprint,
   fixed digest state, no raw value retention, and generic warning text that
   cannot re-echo sensitive or attacker-controlled content.
3. **Round ordering:** Require one tool call per round. Parallel batches reset
   state so a failure and a sibling result cannot create an invented ordering.
4. **Pairing and guidance:** Confirm the actual `TaskResult` and call ID pairing
   remain unchanged; deliver the single warning only through the next request's
   transient system guidance, never as a tool result or persisted message.
5. **Boundaries:** Stop, deadline, provider/receipt failure and M23 budget
   behavior stay authoritative. The detector cannot block unknown/polling tools
   or create an extra request.

## Minimum regression set if root binds M24

- Three identical eligible failures in separate single-call rounds produce no
  hint on rounds one/two and one fixed hint on the next existing request.
- Repeating after warning does not duplicate the hint; the count saturates.
- A changed name, canonical args or error starts at one. Any success,
  status/error mismatch, error data, missing/empty error, or malformed digest
  resets and emits no warning.
- A multi-call round resets the streak, keeps each original call ID/result
  pair, and executes every requested call through the unchanged callback path.
- An unclassified or polling tool with three identical typed errors receives
  at most the advisory and remains callable; no block/denial is returned.
- Warning text contains no args, call IDs, raw error/result or private payload;
  the actual `TaskResult` and paired tool message remain unchanged.
- Stop or terminal provider/receipt/deadline failure after the threshold causes
  no additional provider request and preserves the current terminal outcome.
- A new run starts empty; no digest or warning state enters snapshots/history.
- Ordinary successful runs and M23 wrap-up behavior remain unchanged when no
  exact failure threshold is reached.

## Verification if root binds M24

- Add focused scripted-adapter tests in existing
  `packages/core-rust/crates/anima-core/src/runtime/run_tests.rs` or
  `runtime/tests.rs`; reuse current test adapters and callback counters.
- Inspect the existing Nx project/target first. The captured M23 project is
  `core-rust`; its existing aggregate test command is
  `bun x nx run core-rust:test --skipNxCache`. Root owns serial Cargo execution
  and validation; no new target directory.
- Run the focused reproducer through the existing core test path where its
  supported arguments permit; then root runs the required full `core-rust:test`
  gate. Do not run tests or gates during planning.
- No service, API, provider, database, connector, or live acceptance is needed.

## Source anchors

- Native tool-round processing, awaited executor result, `join_all` batch, and
  next request system construction:
  `packages/core-rust/crates/anima-core/src/runtime.rs`.
- Typed result/status and call/argument structures:
  `packages/core-rust/crates/anima-core/src/primitives.rs` and `model.rs`.
- Cached public exact-failure behavior and limits of the comparison:
  `.superpowers/sdd/2026-10-12-companion-agent-loop-m23/upstream-tool_guardrails.py`.
- Existing native loop test helpers:
  `packages/core-rust/crates/anima-core/src/runtime/run_tests.rs` and
  `runtime/tests.rs`.

**Deferred decision:** Root binds exact eligibility for malformed failures,
fingerprint length bounds, warning wording, and M23 wrap-up placement before
source changes. The binding rulings below authorize the bounded implementation.

## Controller rulings on the risks (binding)

1. Ruling: Count only fresh single-call Error results with nonempty error and no data; success, malformed/inconsistent results, recovered records and parallel rounds reset the streak — typed failures avoid guessing tool effects or classifying normal successful polling as stalled — cost if wrong: miss a warning but retain actual results and the existing round cap.
2. Ruling: Compare canonical arguments, tool name and exact error with a fixed digest; ignore call IDs and durations; keep only digest/count/warned state per invocation — changed attempts and timing must not invent identity, and raw private data must not be retained or echoed — cost if wrong: reset on serialization/size uncertainty, never fail or block a tool.
3. Ruling: Warn once on the next existing request after three identical consecutive failed rounds, then suppress for that streak — tolerate a retry and avoid prompt spam; upstream default exact-failure warning threshold is two, this narrower local threshold is deliberate — cost if wrong: delayed guidance until the ordinary budget wrap-up, no extra request.
4. Ruling: Guidance is fixed trusted system text, original call/result pairs remain intact; Stop/provider/receipt failure and M23 no-tools boundary retain precedence — an advisory cannot authorize another effect or claim no partial effects — cost if wrong: meaningful callback/transcript/no-next-call regressions block publication.
5. Ruling: No descriptor bit, public config/DTO, host wiring or generic guard framework; use existing sha2/serde_jcs and private per-run state, bounded fingerprint input and reset after restore/new run — cheaper reuse should improve a concrete loop behavior — cost if wrong: conservative missed warnings, never expand authority. Required Rust Nx dependency gate once, one batch review/fix wave, no live provider repeat.

## Execution boundaries

Main baseline9ee0d50f9575b10fb4b78c5a7289df3cd3f8784d; only original
nx.json dirty, all services off/settings restored. Native owns reusable policy.
Limit tool-name/error/canonical-argument fingerprint sizes; over-size or invalid
canonical serialization resets without affecting the actual callback/result.
At most one transient warning is consumed by the next existing request. No raw
failure text, tool name, arguments or call ID is interpolated into it. No
long-lived runtime fields or snapshot changes. Root owns serial Cargo with
CARGO_INCREMENTAL=0 and Git; workers do not run tests, services or mutations.

Upstream public revision1fd18277267152893a8a24792b79213e7ca3f899:
agent/tool_guardrails.py::ToolCallGuardrailController.after_call. Cached tracked
source from M23 is reference only, no Python/profile/secret bridge. Retain MIT
attribution; product naming stays Anima. Unknown tools and pollers remain callable,
this feature is advisory only. Successful-call/cycle detection, broad failure
classifiers/hard stops and provider retries are separate deferred work.

## Verified execution record

Root observed warning count0-versus1 RED before implementation. Six focused pure/runtime tests pass, and fresh required Nx Rust2373passed/8ignored plus lint pass. Independent batch review found no implementation issue; one docs correction distinguishes upstream per-signature counts from native consecutive rounds. Original result pairs, private evidence exclusion, reset/restore, bounds, Stop at the cap and parallel behavior are covered. No SDK/web/API change, actual provider probe, service, account action, dependency or feature. M23 budget-wrapup controls and host cancellation/storage cases pass in the full suite. Next: separately bound M25 one unconfigured-tool correction opportunity.
