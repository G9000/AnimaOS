# Harness reliability gate

This gate combines production owner-API/process tests with portable core and
embeddable harness conformance. It uses private temporary profiles, isolated
ports and deterministic model responses. It never opens or stops an owner's
normal daemon/profile and the default gate needs no provider credentials.

Run the trace-driver regressions through `core-rust:harness-reliability-test`,
and the Rust gate through `rust-daemon:test` (which also runs `core-rust:test`):

```bash
bun run --filter core-rust harness-reliability-test
bun run --filter rust-daemon test
bun run --filter rust-daemon lint
bun run --filter anima test
```

The focused daemon filter is `harness_reliability_`. Its eight fixed seeds are
`1, 7, 19, 41, 97, 313, 20261023, 1592642302`. Each generated sequence has at most
32 logical actions, two owned process restarts and a 150-second total bound.
HTTP requests and process-readiness waits have their own shorter bounds.
Failures print the seed and complete action prefix; setting
`ANIMA_HARNESS_RELIABILITY_OUTPUT_DIR` retains per-seed JSON evidence.

The generator permutes allow-once, denial and Stop decisions around real
run submission, same-key replay, changed duplicate refusal, stale decisions and
terminal restarts. Its independent oracle follows the owner's commands. It
compares the resulting unique effect files and payloads, the fixture adapter's
durably flushed physical dispatch log, successful native action-receipt IDs and
terminal run responses from the owner API. A separate mutation test proves that
an unapproved file or extra model dispatch fails this oracle.

Targeted cases use actual persistence/ownership hooks without constructing
ledger rows: a failed admission save behind a barrier prevents model dispatch;
loss of the retained writer closes a waiting approval and new admission; killing
an actually dispatched model preserves its unknown reservation, scope, counters
and deadline across restore. The last case uses the owner's one-attempt limit,
rejects stale/unreviewed recovery and checks that reviewed recovery cannot mint
another model attempt. Existing `turn_crash_tests` separately cover queued-input
adoption, recovered approval, settled effect preservation and durable usage.
These bounded cases establish their specified invariants; they do not exhaust
all concurrent event interleavings or prove remote-effect delivery.

The workflow at `.github/workflows/harness-reliability.yml` runs the deterministic
gate on Windows 2022 and Ubuntu 22.04 with Node 24, Bun 1.4.3 and stable Rust.
Its separate PostgreSQL 16 job is opt in through the manual workflow's `postgres`
input. That job sets only `ANIMA_TEST_PROFILE_OWNER_POSTGRES_URL` to its disposable
service and serializes the ignored session-death/takeover and acknowledged-save
uncertainty tests. An ordinary run
does not silently substitute a user's database or provider credentials.

## Explicit live comparison

`live-driver.mjs` reuses the six existing `../turn-quality/fixtures.json` tasks.
The default experiment plans two repetitions of six fixtures for both variants:
24 trials. Each trial starts a fresh runtime/workspace in an owned test-binary
process, with the same provider/model, sampling, budget, tool policy and context
treatment. The independent Rust filesystem/output oracle grades every task;
`compare_traces` then validates and regrades the complete matched matrix.

Live execution requires both `ANIMA_RUN_TRACE_LIVE=1` and an explicitly supplied
`ANTHROPIC_API_KEY`. No `.env` file is loaded. Only the canonical Anthropic
endpoint is allowed; credentials are forwarded only to the owned live worker,
and their literal value is redacted from archived artifacts/logs. The worker
has no general shell, network or owner-profile tool. Its read/replace/answer and
check helpers operate only in the trial's disposable fixture workspace.

Build each intended revision separately and retain its build log. The test
binary must contain `app::harness_reliability_tests::live_trace::trace_live_worker`
and `trace_live_compare`. A build-only Cargo invocation can expose its actual
test executable path in its `compiler-artifact` JSON output:

```bash
bun tools/workspace-dev/scripts/run-task.ts cargo test -p anima-daemon --lib --no-run --message-format=json
node packages/core-rust/acceptance/harness-reliability/live-driver.mjs --record-build /absolute/test-binary /absolute/baseline-build.json
```

Copy/retain that binary before building the second revision, record its own
manifest, and create an experiment JSON based on `experiment.example.json`.
Use absolute output/manifests or paths relative to the experiment JSON. Its
output directory must be new, with an existing parent. Run:

```bash
node packages/core-rust/acceptance/harness-reliability/live-driver.mjs /absolute/experiment.json
```

Each manifest records the exact binary SHA-256, Git commit, and digest of the
Rust/Cargo source snapshot observed when recorded. This observation does not
prove how an externally built artifact was produced; retain the corresponding
build log. The plan also records the fixture digest and complete matched
budget/policy/context settings. Raw provider keys never enter this metadata.
Each worker records the hash of its compiled-in fixture bytes; the driver rejects
a stale compiled suite whose hash differs from the archived current fixture set.

The driver durably writes all planned rows before sending any trial and updates
them after each observation. Timeout, process failure, missing grade or mismatched
identity stops the experiment; subsequent rows remain `planned`. It never resumes
an uncertain row or automatically retries a provider request. Missing, duplicate,
incomplete, mismatched-model/configuration or independently misgraded traces fail
matrix validation and produce a nonzero driver exit. Completed wrong answers
remain observed failures in the matrix instead of disappearing from the sample.

Identical binary hashes are labeled `repeatability-control`. Distinct artifacts
are only a matched comparison; neither case automatically claims quality/cost
improvement or grants promotion, approval or Undo authority. No live experiment
runs in ordinary CI.
