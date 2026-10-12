# M26 ChatGPT Pre-Stream HTTP Retry Plan

**Status:** implemented and locally verified on g9000/companion-agent-loop-m26; publication is recorded in the milestone ledger.

## Goal and bound

Allow at most one retry for a ChatGPT Responses HTTP 502, 503, or 504 received
before SSE parsing. Reuse one client, credential, account, endpoint, request body,
and model configuration. Keep current transport diagnostics and all other
response handling intact.

A failed HTTP attempt has unknown usage. The retry may duplicate provider work;
this is not exactly-once delivery and does not impose a billing ceiling.

## Verified source surface

- `packages/core-rust/crates/anima-model-adapters/src/chatgpt.rs`
  `ChatGptResponsesAdapter::request` builds/authenticates a POST, sends it, then
  passes every non-success response to `http_failure`. `stream` passes a
  successful response to `consume`. `generate` uses `stream` and a `FinalSink`.
- `http_failure` currently reads at most `MAX_ERROR_BODY` (64 KiB), falls back
  when content length is oversized, body reading fails, or the body is not
  recognized JSON, and calls `structured_failure` for a whitelist diagnostic.
  `structured_failure` examines top-level `error` events and `response.error`,
  uses `KNOWN_FAILURES`, and returns only static messages and known parameters.
- `KNOWN_FAILURES` contains terminal request/auth/schema/quota codes alongside
  transient-looking server codes. A retry decision cannot be inferred from the
  status alone or parsed from the formatted diagnostic string.
- `consume` handles successful HTTP responses. SSE `error` and
  `response.failed` events return errors from the stream path. Retry must never
  re-enter `consume` or replay after any stream frame.
- Adapter imports include `Duration`; adapter `Cargo.toml` has `tokio` only in
  dev-dependencies. Do not add production `tokio::sleep` or a new dependency.
- Core `Cargo.toml` already depends on `futures-timer`. Export a tiny generic
  async delay helper from `anima-core`, implemented with that dependency; the
  adapter awaits it. Dropping the adapter future drops the delay future.
- `AgentSettings.max_retries: Option<u32>` is already available on the passed
  `AgentConfig`; the adapter does not use it today. M23 wrap-up uses a shadow
  config and must set its cloned `settings.max_retries` to `Some(0)` without
  changing persistent settings. This prevents HTTP retry during wrap-up.
- Existing tests in `chatgpt/tests.rs` use a local Axum server helper, adapter
  endpoint override, fake token/account, and bounded diagnostic assertions.
  These are adequate for all deterministic cases below.
- Cached M23 comparisons:
  `.superpowers/sdd/2026-10-12-companion-agent-loop-m23/upstream-turn_loop_errors.py`
  and `upstream-turn_retry_state.py` implement broad outer-loop retry state.
  M26 is a private adapter transport retry; no Hermes retry framework is needed.

## Binding retry policy proposed to root

- Eligibility starts with an actual HTTP response status exactly 502, 503, or 504. Only decide before consuming the response as SSE.
- Construct a private typed failure value, for example
  `HttpFailure { diagnostic: String, retryable: bool }`. Keep the diagnostic
  for the existing user-visible error path. The flag is derived from response
  status, content type, bounded body-read outcome, and parsed structured code.
  Never inspect the diagnostic string to decide retry.
- Exclude `Content-Type: text/event-stream` even when paired with 502/503/504.
  Treat it as a phase-ambiguous stream response. No stream event is parsed or
  replayed on the HTTP retry path.
- Preserve the existing 64 KiB bounded read. If declared or observed body size
  exceeds the bound, or body reading fails, return the current fallback
  diagnostic with `retryable=false`.
- When bounded JSON contains a `KNOWN_FAILURES` code, classify by code before
  considering the status. These codes alone may be retryable:
  `server_error`, `internal_server_error`, `service_unavailable`,
  `subscription_sharing_usage_unavailable`, and
  `subscription_sharing_user_unavailable`. Any other recognized code is
  terminal even on HTTP 502/503/504.
- An unrecognized code in structured JSON is terminal. Do not guess its meaning.
  Unknown bounded HTML/plain gateway bodies without a recognized structured
  failure may be retryable for 502/503/504. Unstructured JSON with no recognized
  code is terminal conservatively.
- `None` allows one retry by default; `Some(0)` disables; any positive value
  clamps to one. No unrelated adapter or outer runtime behavior changes.
- Wait exactly 250 ms once, using the core generic delay helper. It must be
  cancellable by dropping the enclosing adapter future before the next POST.
- Request `.send()` errors, timeouts, 400/401/403/429, every other status,
  redirects, successful statuses, all SSE event errors, parsing errors, partial
  output, sink failures, and final-response validation errors are never retried.
- The second eligible failure returns its normal diagnostic. There is no third
  request, provider fallback, auth refresh, request change, or model switch.

## Implementation steps and interfaces

1. Add only a generic exported async delay helper to `packages/core-rust/crates/anima-core` using existing `futures-timer`. Do not add a policy-specific API or dependency.
2. Refactor private HTTP failure classification in `chatgpt.rs` around a typed result carrying the existing safe `diagnostic` plus a private `retryable` flag. Keep `http_status_failure` and existing diagnostic text behavior.
3. Capture actual status and `Content-Type` before consuming a response body. Reuse the existing size cap and `Zeroizing` body buffer. Derive retryability from typed facts, not displayed strings.
4. Return a retryable typed failure only for allowed 502/503/504 cases above. Explicitly make body read errors, oversized bodies, `text/event-stream`, and terminal/unknown structured codes non-retryable.
5. Build `request_body(config, request)` once before the attempt loop and reuse the same `Value` for both sends. Keep client, headers, token, account, endpoint and request bytes semantically identical.
6. On first retryable status only, await 250 ms through the core delay helper and repeat the existing send path. Return second failure without another wait/request.
7. Read only the call config's `settings.max_retries`: absent => one, zero => none, positive => one. In M23, force `Some(0)` only on the wrap-up shadow clone; original saved settings remain unchanged.
8. Preserve `stream -> consume` as a single pass after the first successful HTTP response. Do not retry any error returned by `consume` or any error from `.send()`.

## Minimum regression set

- Local HTTP server returns 503/plain gateway response then valid completed SSE:
  with `None`, assert two requests, identical headers/body, one final response.
- Same response sequence with `Some(0)`: assert one request and original diagnostic.
- Positive values above one: assert at most two requests. Two retryable responses return the second diagnostic and never produce a third request.
- Parameterize all three eligible statuses; no retry for 400, 401, 403, 429, or other 5xx.
- On 502/503/504 with each recognized terminal code (schema, auth, quota, permission,
  request and other non-allowlisted known values), assert one request. Confirm the
  five explicitly allowed transient codes remain eligible.
- Unknown bounded HTML/plain 502 body may retry. Unknown structured JSON code,
  unstructured JSON, oversized body, content-length over cap, observed overflow,
  and body-read failure do not retry.
- `text/event-stream` on 502/503/504 does not retry. A 200 SSE error event,
  malformed/incomplete SSE, partial output then failure, and sink error each
  return after one request and preserve current errors/frames.
- A `.send()` connection failure or timeout is not retried; avoid assertions on
  unstable reqwest diagnostic wording.
- Cancel during the 250 ms delay through the existing core control path; assert
  no second POST. Do not add a dependency or alternate cancellation mechanism.
- With M23 wrap-up and persistent `max_retries > 0`, assert the adapter receives
  shadow `Some(0)`, makes one eligible 503 request, and does not retry; persistent
  config remains positive. An ordinary call using that config may retry.

## Risks and controls

1. **Unknown duplicate usage:** Eligible gateway responses can still follow
   provider work. Permit at most one retry, retain the opt-out, report usage as
   unknown, and make no billing-cap claim.
2. **Wrong classification:** A 5xx can carry a permanent auth/schema/quota error.
   Use typed status/content-type/body/code facts; recognized terminal codes win.
   Never parse formatted diagnostics.
3. **Stream replay:** An SSE response can be partially processed. Exclude the
   event-stream content type and all errors after `consume` starts.
4. **Unbounded or changed retry:** Reuse one request body and same authority;
   wait once, retry once, clamp settings, and never refresh/fail over.
5. **Cancellation and wrap-up:** The core timer helper must be drop-cancellable.
   M23 shadow config forces `Some(0)` while persistent settings remain intact.

## Verification and command ownership

- Adapter tests belong in
  `packages/core-rust/crates/anima-model-adapters/src/chatgpt/tests.rs` using
  its existing mock HTTP server. Core may add focused tests for the delay
  helper/drop behavior and M23 captured shadow config.
- Focused Rust tests should run through the existing `core-rust` Nx project
  after root inspects supported target filters. The required aggregate gate is
  `bun x nx run rust-daemon:test --skipNxCache`; root owns execution and serializes
  Cargo.
- No live ChatGPT account/provider request is useful for classifying these
  branches. No service, browser, account, credential, API mutation, Cargo or Nx
  action is part of this planning task.

## Root decision

Bind the private typed classifier, the exact known-code allowlist, unknown-body
handling, 250 ms one-shot delay, `None` default one / `Some(0)` opt-out / positive
clamp, and M23 wrap-up shadow override before production work. Do not expand into
send-error replay, auth refresh, quota/429 retry, SSE restart, failover, or a
generic retry framework.

## Controller rulings on the risks (binding)

1. Ruling: Default one retry only for actual pre-stream502/503/504, explicit Some(0) disables and positive settings clamp to one — small transient availability improvement without a retry loop — cost if wrong: up to one extra provider attempt with unknown first-attempt usage; no exactly-once or billing-cap claim.
2. Ruling: Use private typed status/content-type/bounded-read/structured-code classification; known permanent and unknown JSON errors, event-stream/oversize/read failure are terminal — auth/schema/quota must override gateway status and ambiguity fails closed — cost if wrong: miss transient recovery; no credential refresh/account/model/failover or raw diagnostic escape.
3. Ruling: Build request body once, reuse exact client/endpoint/credential/account/headers/model; no send-error/timeout or post-consume retry — transport ambiguity and partial output may represent accepted work — cost if wrong: one conservative failure, preserve visible output and existing error; no tool/effect replay.
4. Ruling: Await one250ms executor-neutral core helper using existing futures-timer under existing cancellation select; no detached/blocking delay/dependency/feature/targetdir — dropping the adapter future must stop its wait and continuation — cost if wrong: cancellation regression blocks publication; concurrent transport completion is not an exactly-once/no-billing promise.
5. Ruling: M23 wrap-up shadow clone forces existing max_retries Some(0); the three existing API-provider stream retry predicates must honor explicit zero only while preserving their default policy — inspection found nested old retries ignored the setting — cost if wrong: cap0/positive-setting and all-provider mock regressions fail; persistent config/grants and ordinary prior policies remain unchanged.

## Root implementation boundaries

Baseline553e020e01e9074e468dc21dd8cdae5947d02416/mergedM25PR31. Root cut
the branch after exact tested-tree verification. No live provider/service needed.
Root owns all Cargo/Nx/Git; CARGO_INCREMENTAL=0 and existing target dirs only.

Worker core owns model.rs/lib.rs generic async model_transport_delay(Duration),
runtime.rs shadow max_retries Some(0), existing run_tests.rs captured-config
regressions (None/zero/positive persistent setting unchanged). Keep schema/DTO,
host wiring and cancellation controls unchanged. Adapter worker owns chatgpt.rs,
chatgpt/tests.rs, adapter.rs and tests.rs: typed classifier/one retry plus exact
three existing stream predicates honoring explicit zero. Shared source files
are not assigned twice. Add tests first; root observes actual meaningful RED
before approving production. One batch native/adapter review, one contract review
if needed, one fix wave, required full Rust test and lint before publication.

Preserve existing diagnostic text and existing structured selection precedence;
use one private extracted payload error selector, no formatted-string matching.
Only the five proposed transient structured codes can retry on eligible status.
Invalid/unknown parsed JSON is terminal, bounded non-JSON gateway text may retry.
Other providers' ordinary default retry statuses/delays remain as before; only
explicit zero disables their existing retries. No broader vendor retry refactor.

Public pinned Hermes1fd18277267152893a8a24792b79213e7ca3f899 architecture
references: agent/turn_loop_errors.py::handle_outer_loop_error/OuterErrorVerdict
and agent/turn_retry_state.py::TurnRetryState. Native private transport logic
adapts bounded phase-aware recovery, not credential refresh/fallback/outer-loop
framework. Retain full MIT notice with accurate limited provenance.

## Verified result

Required Nx Rust2398passed/8ignored/40suites and lint pass. This includes2new
captured-config core tests and15new mock-HTTP tests, original provider controls
and full native receipt/restart coverage. Actual RED preceded production; one
batch/final contract review and one fix wave corrected private helper composition
and exact MIME eligibility. Structured diagnostic precedence remains unchanged.
No actual provider/service/account/browser test for M26; failed-attempt usage
and simultaneous cancellation/transport completion remain uncertain. SDK/web
schemas/source unchanged; no redundant TypeScript gate. Exact source/PR/merge
and next branch resume are recorded in the ignored milestone ledger.
