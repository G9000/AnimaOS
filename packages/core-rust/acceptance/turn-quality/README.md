# Ordinary task trace acceptance

`fixtures.json` is a separate six-case task suite. The existing response-only
`skill_evaluation` suite is unchanged. Expected file contents and exact final
output are independent of model claims. Grading compares the complete isolated
workspace tree, so an unrelated green check, skipped edit, stale overwrite,
changed protected file, extra file, or incorrect final answer fails correctness.
Symlinks and non-UTF-8 or oversized trees fail closed.
Fixed acknowledgement wording is explicitly requested in the relevant task
prompts. The large-log task requests the last record identifier without revealing
it; the skill task derives its mapped-word response rule from the input skill.

The `anima-core` integration test `trace_evaluation_ordinary_runtime_battery_and_mutation_oracles`
runs the ordinary `AgentRuntime` with a deterministic local model adapter and
explicitly permitted native-like file/read/check helpers, fresh runtime and
workspace per trial, two repetitions of both variants. Ambiguous replacements
fail; a concurrent owner edit invalidates a stale replacement; a large real log
is returned; repeated helper failures are counted; smoke success does not validate
the target; a skill file must be read again in the fresh session. The model scripts
are intentional correct/incorrect controls, not evidence that a provider can solve
these tasks. Mutating correct output files or adding an unrequested file must make
the independent oracle fail. No owner profile or credential is used.

For a provider-backed run, load the same JSON fixtures; create a separate directory
from each fixture's `initialFiles`, supply `prompt` to an ordinary runtime with
permitted workspace tools, and attach `Arc<TraceCapture>` through
`AgentRuntime::set_run_observer`. Prefer `TraceSettings::from_config` to bind the
actual provider, model and canonical full agent configuration. The experiment
configuration identity must also include host settings absent from that config (all sampling, budgets,
tool definitions/policy, context treatment except the explicit tested variant),
and set `fixtureSetId` to `fixture_set_digest(&fixtures)`. Use repetition indices
`0..repetitions` and both `TraceVariant`s. After awaiting the runtime result, call
`capture.finish(&result)`, then `grade_fixture` against that trial's workspace.
Serialize each `GradedTrace` to preserve normalized model/tool boundary events,
complete filesystem observations and oracle failures. `compare_traces` regrades
those observations against its fixtures and rejects inconsistent stored verdicts.
Call it over the complete matrix. Missing/duplicate,
incomplete or mismatched trials are rejected, including actual fallback models
that differ from the requested matched model. Do not reuse synthetic model
settings for a provider result.
Events must follow the ordinary runtime phases: model start, at most one served
identity before usage, model finish, tool dispatch/results for that current step,
then the next model start after all dispatched tools settle. A historical step
identity cannot be reused to smuggle a late tool or served event into the trace.

`TraceCapture` implements `RunObserver`, buffers bounded data without I/O and must
be dedicated to one run. Hosts already using an observer can fan out each frame
to their observer and this capture. Capture is opt-in and does not change runtime
execution, owner approval, tool grants, stop, or Undo. Save traces only in the
isolated acceptance directory: raw tool results may contain workspace content.

Metrics describe runtime model steps, not hidden transport retries inside an
adapter. Tool calls include recovered results (reported separately); exact repeat
counts use tool name and canonical arguments, ignoring unique call IDs. Result
bytes count UTF-8 tool result payloads. Usage sums only reported nonzero values;
missing/default-zero usage is unknown and remains `None` if all calls are unknown.
Known and unknown call counts accompany the partial sum. Elapsed milliseconds
use a monotonic capture clock and may legitimately be zero for a fast local run.
Terminal runtime errors and tool errors are reported separately. No score grants
approval or makes a skill promotion decision. A deterministic test or a single
provider run establishes only that exact observed execution, not broad quality,
cost improvements, delivery, or live acceptance elsewhere.

For archived deterministic evidence set `ANIMA_TRACE_EVALUATION_OUTPUT_DIR` to
an isolated ignored evidence directory before running the integration test. It
writes `deterministic-runtime-traces.json` and `deterministic-runtime-report.json`.

The daemon also provides the ignored opt-in test
`trace_live_tests::trace_evaluation_live_fresh_skill_reuse`. It requires both
`ANIMA_RUN_TRACE_LIVE=1` and an explicitly forwarded `ANTHROPIC_API_KEY`, reads no
`.env`, and permits only the canonical Anthropic endpoint. It uses a fresh
runtime with `claude-sonnet-4-6`, temperature 0, 512 maximum output tokens, zero
configured retries, no fallbacks, six maximum tool rounds, and a total 30-second
timeout. Its read/write helpers permit only the two fixture inputs and answer
file. The model receives the task and output filename; the expected answer is
withheld. Its system prompt explicitly requires the final reply to match the saved
text without Markdown or added explanation. Successful input reads must precede the write, and exact independent
file/output grading still applies. When the evidence directory is set, it saves
`anthropic-fresh-skill-trace.json` with credential echoes redacted. This individual
live grade does not satisfy the full matched baseline/candidate matrix and does
not establish a comparative improvement.
