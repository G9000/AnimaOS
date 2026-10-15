# M48 provider-request diagnostics implementation plan

> For agentic workers: use the existing LEAN controller workflow and
> superpowers:executing-plans; root assigns disjoint ownership and all execution.

Status: scoped implementation and controlled wire acceptance complete on
`g9000/companion-provider-diagnostics-m48`, cut from verified M47/PR53 main
`456149927c96e10f1b468e480042d14cb2f8f6e6`. The one separately released actual
zero-tool diagnostic was rejected as model access; its request budget is consumed.
Exact source/publication/merge and continuation proof is in
`.superpowers/sdd/2026-10-15-companion-provider-diagnostics-m48/progress.md`.
The controller rulings below bind the implementation and its evidence boundary.

**Goal:** establish the exact synthetic first-request shape and improve recognized
subscription error diagnostics without exposing upstream text or inventing M47's cause.
**Architecture:** reuse the ordinary native coordinator, portable ModelAdapter port,
existing ChatGPT request serializer, bounded error reader and fixed allowlists.
Test-only native-to-core fixture handoff avoids a new production endpoint/debug API.
**Stack:** existing Rust/serde_json/Axum/Tokio/Nx; no dependency or runtime addition.

## Accepted scope and actual diagnostic (2026-10-15)

- Controlled classifier RED: four pass, two expected failures; narrow implementation
  then passed seven focused core cases with one intentional importer skip.
- Native capture passed two cases with two intentional ignored drivers. The exact
  synthetic ordinary manager request contains 14 tools after real augmentation.
  Its 10,376 bytes, SHA256
  `17a8c79e78738d57b9c27bedee2161cbf287109acd89bdb39cb19a77cf5e15c7`,
  passed separate exporter/importer cases through the existing private serializer
  and joined loopback. This is wire integration evidence, not backend acceptance.
- Known code precedence and retry decisions are preserved. Missing/null code can
  use four known type literals and existing safe parameter labels. Only the exact
  bounded HTTP 400 model-entitlement detail signature receives fixed model-access
  guidance. No upstream text, credential, account or model value is emitted.
- The pinned public Hermes classifier/tests/license were verified at
  `1fd18277267152893a8a24792b79213e7ca3f899`. The adapter notice retains MIT
  provenance. No credential rotation, model fallback or automatic retry was ported.
- One combined native/core review and final affected SDK/web review are clean.
  Required gates passed **2,681 Rust/core tests / 19 intentional ignores /
  40 suites / lint / build**. SDK/web/contracts/manifests/lockfile are unchanged;
  the six reviewed source hashes stayed exact. Owned Prettier and document checks pass.
- Only after those gates, root released one direct synthetic `M48_ROUTE_OK` request
  through the normal application vault adapter: same ChatGPT/gpt-5.5 route,
  zero tools/helpers/retries, 20-second call and 30-second outer bounds. It failed
  in 1.10 seconds with the closed category **model_access**, identifying the
  selected model as unsupported for this account. Failed-call consumption is
  unknown. No second request, alternate model or M47 task replay followed.
- This diagnoses the new zero-tool request only. M47's original HTTP 400 body was
  not retained; its specific cause and useful coding/delegation remain unaccepted.
  Next priority is account-aware subscription model availability/selection. Native
  voice remains held; no full Hermes, real connector or production claim follows.

## 1. Evidence and claim boundary

- M47's one actual ChatGPT/gpt-5.5 task failed HTTP 400 in about 1.1 s before useful work.
- No accepted model-step/usage row, helper, tool, approval, edit or output was observed.
- Upstream consumption/billing for that rejected request remains unknown.
- Exact failed-run/config/files/empty tasks and receipts survived quiet same-store restart;
  no replay occurred. M47 is preserved as partial coding evidence, not rerun for a pass.
- Rust/core 2672 passed/16 intentional ignores/40 suites/lint/build and the real-shell
  ScriptedModel coding flow remain native evidence; they did not validate provider acceptance.
- Static HTTP 400 does not reveal a rejected field, schema, model or account cause.
- The private M47 diagnosis note remains the hypothesis log; it does not prove the cause.
- Native voice remains held/renumberable; useful provider compatibility has priority.

## 2. Scope and exclusions

- Capture one fresh synthetic ordinary manager's first adapter-bound request after augmentation.
- Exercise that exact fixture through the existing private ChatGPT serializer and joined loopback.
- Add fixed diagnostics for recognized type-only envelopes and already allowed parameter labels.
- Preserve known-code precedence, error redaction, retry classification and max_retries 0.
- Preserve subscription endpoint, headers, account selection, authorization and billing route.
- Unknown code/type and generic detail/plain/HTML/malformed/oversized/read-failed bodies keep safe fallback. The binding section adds one source-backed, fixed model-access detail signature.
- No captured real prompt/response body, credential/vault/profile inspection or account probe.
- No new API, public serialization helper, arbitrary-endpoint option, producer, process or journal.
- No speculative schema normalization, Harmony rewriting, reserved-name aliasing or validator product.
- No provider/model switch, auth workaround, new coding task or successful coding/Hermes claim.
- The one actual diagnostic followed a separate recorded root release after gates; its budget is consumed. No additional real request is released.

## 3. Source map and proposed ownership

| Owner                     | Paths                                                                                 | Responsibility                                                           |
| ------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Native fixture lane       | `hosts/rust-daemon/src/agent_runs/provider_request_tests.rs`                          | Actual coordinator capture and optional ignored synthetic export         |
| Same native lane          | `hosts/rust-daemon/src/agent_runs.rs`                                                 | Test-module registration only                                            |
| Core diagnostics lane     | `packages/core-rust/crates/anima-model-adapters/src/chatgpt.rs`                       | Narrow demonstrated classifier change only after RED                     |
| Same core lane            | `packages/core-rust/crates/anima-model-adapters/src/chatgpt/tests.rs`                 | Existing bounded HTTP/retry/redaction cases and new type-only cases      |
| Same core lane, if needed | `packages/core-rust/crates/anima-model-adapters/src/chatgpt/request_fixture_tests.rs` | Private fixture decode/serializer/loopback tests; test registration only |
| Root                      | Public plan/docs/private ledger, commands and publication                             | Bind scope, execute gates, settle evidence and exact head                |

Keep `common.rs`, `runtime_model.rs`, core DTOs, production coordinator, SDK/web,
Cargo manifests/lockfile and M47 fixture/candidate unchanged unless a new concrete
RED requires a separately recorded narrow release. Do not add endpoints to share test data.
Root freezes final ownership before writes; no two lanes edit the same registration.

## 4. Exact synthetic native capture

Use current agent_runs test helpers and a fresh TempDir workspace, in-memory model,
ordinary Chat session and normal acceptance/live registration. No server/process is needed.
Register a first-call recording ModelAdapter that clones received AgentConfig and
ModelGenerateRequest, then returns one static no-tool final response with synthetic usage.
That response settles the ordinary run; it is not a fake external provider compatibility proof.
The adapter must reject or count a second call so titles/learning/helper/compaction cannot hide.
Disable those optional secondary paths through existing deterministic construction/configuration.

Select canonical registry descriptors for read_file/list_dir/glob/grep/write_file/edit_file,
todo_read/todo_write/spawn_helper/bash. Configure max_tool_iterations1, max_retries 0,
timeout_ms20000 and max_tokens2048; no MCP/browser/account/background-process descriptors.
Use the manager helper/configuration already exercised by M47, not fabricated owner rows.
Assert the saved config stays exact while the actual run copy includes the existing augmentation:
list_workspace_agents/send_message/broadcast_message/delegate_to_agent/spawn_helper.
Assert exact expected names once each; compare each descriptor to the canonical registry value.
Do not infer that advertising those tools dispatches, grants or approves any action.

Capture first-call system/messages/temperature/max_tokens and the effective config as received.
Assert exactly one user input in the fresh conversation, no assistant/tool/replay messages,
no actual helper start, no tool receipt/approval and no protected fixture-byte mutation;
declare existing host-written runtime journals separately from tool effects.
Use explicit synthetic marker strings only; exact generated IDs/temporary paths remain private.
Assertions must fail on missing augmentation, extra names, duplicate descriptors or extra calls.
No serialized fixture is exported during ordinary default tests. The optional ignored actual diagnostic is separately guarded and is never part of default execution.

## 5. Test-only handoff contract

ModelGenerateRequest has Clone/PartialEq but no Serde; do not add production Serde for this task.
Use this small manually assembled JSON envelope in the optional ignored native exporter:

```text
{fixtureVersion:1, config:<existing AgentConfig JSON>,
 request:{system:string,messages:<existing Message JSON array>,
          temperature:number|null,maxTokens:integer|null}}
```

The native writer assembles request fields explicitly from the actually captured request.
The core reader reconstructs ModelGenerateRequest explicitly; no common public DTO is added.
Both sides use only existing serde_json and existing AgentConfig/Message serialization.
Root supplies a fresh absolute private `ANIMAOS_M48_REQUEST_FIXTURE` path to ignored cases;
require version1, exact envelope/request keys, expected synthetic marker, and <=512KiB bytes.
Writer uses create_new; missing/invalid path or input fails, never skips or defaults to a profile.
Reader rejects non-file/oversize/truncated/foreign-version input before constructing a request.
This is a trusted test artifact, not a secure general importer or arbitrary-editor CAS guarantee.
No automatic fixture generation, nested Cargo, subprocess, source parsing or provider call occurs.

Core test feeds the reconstructed values into the existing private request_body function.
Assert typed user input, exact model/tools/order-independent schema equality, strict:false,
stream:true/store:false/tool_choice:auto/parallel_tool_calls:true and absence of API-only controls.
Validate nested todo items/required/enum and structural integer bounds using actual emitted JSON.
Use existing exact expectations, not a handwritten duplicate of the whole registry.
Then use existing in-process HTTP fixture with a locally constructed test adapter to capture
the same emitted JSON and return one bounded synthetic success or controlled rejection.
Loopback override stays inside the adapter crate's existing test module; no production seam.
Assertions/logs expose only synthetic schema/flag/role summaries, counts and digests.
No installed Node/schema validator is required; use one only if an actual unresolved schema
question needs it and root confirms it already exists. Do not install a validator/dependency.

## 6. Minimal safe error diagnostic contract

Reuse MAX_ERROR_BODY=64KiB, bounded chunk accounting, zeroized temporary bytes,
failure_payload wrapper selection, KNOWN_FAILURES and known_parameter/indexed-tool reduction.
No diagnostic may quote message/detail/body/header/URL/account/path or unknown code/type/param.
Keep all current known-code strings and finite parameter normalization behavior unchanged.

Candidate priority, to freeze with the controlled cases before production:

1. A recognized code in the selected error payload keeps the current diagnostic and precedence.
2. If code is absent or JSON null, a recognized type may select the same fixed explanation.
3. A non-null unknown or malformed code is not overridden by a weaker type; keep fallback.
4. Type recognition reuses the applicable existing finite literal mapping, initially
   invalid_request_error/authentication_error/permission_error/server_error only if tested.
5. Render recognized type provenance as fixed `(type: invalid_request_error)` style;
   add only current canonical parameter labels, for example `parameter: tools`.
6. Unknown/malformed type, raw detail and non-object shapes return the original status fallback.

The selected payload may be nested error, response.error, or an existing supported top-level
error event. Retain failure_payload's precedence, including present malformed top-level code.
The envelope class is demonstrated by synthetic tests; no raw envelope/debug API is exposed.
Do not add an SSE error parser: HTTP event-stream/error fallbacks and retry denial stay intact.
Successful response conversion and streaming error paths must retain prior known-code behavior.
The change does not diagnose the historical400: its actual envelope remains unknown.

Diagnostic recognition must be separate from failure_code/retryable logic.
Type-only server_error must not enable a retry previously refused by the code-based classifier.
Only the existing allowed502/503/504 transient conditions may retry, within current limits.
HTTP 400 and max_retries 0 still make one request; partial streams never replay.
No unbounded strings or regex extraction are added. Require safe outputs <=512 UTF-8 bytes
for the new bounded synthetic matrix without truncating arbitrary source text into diagnostics.

## 7. Meaningful RED/GREEN groups

- [ ] Native `m48_provider_request_manager_first_call_captures_augmented_tools`:
      capture actual config/request and prove no handlers/secondary calls/saved-config widening.
- [ ] Ignored `m48_provider_request_export_synthetic_first_call`:
      root-only create_new fixture export, same real native capture path, no provider.
- [ ] Ignored core `m48_provider_request_import_uses_subscription_wire`:
      manual versioned decode, actual private serializer and joined loopback; one HTTP attempt.
- [ ] Core `m48_provider_diagnostic_type_only_errors_are_bounded_and_parameter_safe`:
      known type with missing/null code in nested/response.error envelopes must yield exact safe text;
      current generic status fallback must be the observed RED, not a compile/import failure.
- [ ] `m48_provider_diagnostic_known_code_precedes_type_and_unknown_stays_private`:
      conflicting known code/type preserves code; unknown/malformed code/type, secret-bearing
      parameters/detail/message stay fallback, including adversarial sentinel strings and Unicode.
- [ ] Group caps/media/read-error cases around existing fixture: >64KiB known-length/chunked,
      invalid UTF-8/JSON, empty/string/array/detail-only/SSE, chunk-read failure; no body echo.
- [ ] Reuse current retry matrix and add type-only server_error/max_retries 0/HTTP 400 exact
      one-attempt checks only where needed; preserve cancellation/backoff/partial no-replay coverage.

Capture coverage may pass unchanged production; record that as a newly closed evidence gap.
Only failing recognized-type/parameter behavior permits its corresponding production change.
Do not manufacture schema/Harmony RED requirements solely to justify a rewrite.

## 8. LEAN execution sequence

1. Root binds five risks, publishes a lean public plan, and releases disjoint tests only.
2. Native lane freezes capture/handoff source; core lane freezes exact failing type/param cases.
3. Root observes compiled controlled RED with original logs and no real provider request.
4. Root releases only the demonstrated classifier delta; authors freeze their source.
5. One combined affected native/core review and one final affected SDK/web source check;
   existing String/static-error rendering should need no SDK/web change.
6. One combined fix wave for actionable findings, then format/freeze and required gates.
7. Root may separately decide the single actual diagnostic below, or settle offline-only.
8. Publish precise evidence, attach PR, verify exact reviewed/tested head and main merge.

## 9. Root-only future commands

These are not preparation evidence. Root first confirms resolved targets/argument forwarding:

```powershell
bun --no-env-file x nx show projects --json
bun --no-env-file x nx show project core-rust --json
bun --no-env-file x nx show project rust-daemon --json
bun --no-env-file x nx run core-rust:test --skipNxCache
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
```

Focused Cargo test selectors passed through confirmed Nx forwarding: `m48_provider_request_`
and `m48_provider_diagnostic_`; ignored exporter/importer separately with `--ignored`, serialized.
Root avoids duplicate core runs only after verifying the target dependency/forwarding behavior.
Use preserved approved environment workaround, `CARGO_BUILD_JOBS=1`/`CARGO_INCREMENTAL=0`,
`RUST_TEST_THREADS=2` and a validation target directory if an owned daemon locks the normal exe.
These denote existing environment variable values, not extra command flags or source edits.
Retain each exit, actual attempt count, fixture SHA/bytes and all source hashes/timestamps.
No real-service skip is counted as provider acceptance; no unnecessary new full SDK/web gate
unless their source/contracts change or the final affected review identifies a real concern.

## 10. Optional distinct first-turn diagnostic — still HELD

At most one separately root-released request after gates, if offline evidence cannot answer
basic route/model acceptance. Use a fresh synthetic run/session, same selected subscription
route/model, max_retries 0, enforced 20 s call deadline and root 30 s wall bound/Stop.
No M47 task, candidate, helper, executable, file operation, account/connector or useful coding.
Choose either one zero-tool adapter request or the full synthetic first-request shape in advance;
do not issue both sequentially to hunt for a pass. Declare the chosen question before admission.
A zero-tool adapter call must actually omit tools/tool_choice/parallel_tool_calls; normal ordinary
coordinator augmentation means simply saving no grants does not establish a zero-tool request.
Use only an existing suitable root-owned application adapter path; if absent, remain offline-only
until root explicitly approves the smallest separate driver, never a public endpoint override.
Success proves that one selected shape/route/model at that time; zero-tool success does not clear
the full tool set, coding, delegation, current account policy generally, or the historical400.
Failure remains failure/unknown with fixed diagnostic and actual/unknown usage; no retry,
new provider/model, account inspection, repair prompt or second M47 quality task follows.
Root records quiet cleanup/exact owned identities; no physical-stop claim from HTTP acknowledgement.

## 11. Top five risk rulings for root to bind

1. **Cause and evidence:** exact synthetic augmented request plus compiled controlled error RED;
   historical generic400 stays unidentified. Why: guessed schema changes can hide the wrong cause.
   Cost if wrong: stop with a bounded evidence gap; do not claim useful provider/coding success.
2. **Confidentiality:** finite known code/type/parameter text only; raw unknown/detail fallback stays.
   Why: arbitrary errors can contain prompts, credentials and paths. Cost if wrong: reject the
   change before actual calls; retain synthetic leak sentinels, never dump real bodies for diagnosis.
3. **Behavior preservation:** keep endpoint/auth/headers/retry/code precedence and zero-retry semantics.
   Why: type recognition is explanatory, not authority to resubmit. Cost if wrong: an extra request
   is an acceptance failure with unknown spend, not a retry policy feature.
4. **Minimal reusable seam:** test-only explicit versioned handoff into the existing serializer;
   no public import/endpoint/producer/validator or speculative normalization. Why: an easy debug
   interface creates durable authority surface. Cost if wrong: stay offline and simplify the tests.
5. **Actual-call budget and claims:** zero or one separately released diagnostic, selected upfront;
   no M47 replay or two-shape pass hunt. Why: another tool-free success cannot prove full coding.
   Cost if wrong: record failed/partial evidence and reprioritize; voice remains independently held.

Preparation completion means only this private plan is frozen. Root owns risk binding,
public plan, source/test releases, all commands, any real request and publication.

## Controller rulings on the risks (binding)

1. **Evidence:** capture the actual synthetic manager request after augmentation,
   then hand it to the existing private serializer and joined HTTP fixture. The
   historical HTTP 400 remains unidentified. Ruling: change only behavior proven
   by a compiled controlled RED — local wire conformance cannot establish backend
   acceptance — cost if wrong: retain a bounded evidence gap and no coding claim.
2. **Confidentiality and exact diagnostic precedence:** preserve recognized-code
   output first; an explicit non-null unknown/malformed code blocks weaker
   inference. Missing/null code may use only the four tested type literals
   `invalid_request_error`, `authentication_error`, `permission_error` and
   `server_error`, rendered with type provenance and existing parameter reduction.
   No standalone parameter-only explanation is added. Ruling: emit fixed constants
   only — provider text can contain secrets and cannot establish authority — cost
   if wrong: reject the change on the leak-sentinel matrix before any real call.
3. **Narrow Hermes reuse:** the pinned public source's
   `agent/error_classifier.py::CODEX_ACCOUNT_MODEL_ENTITLEMENT_MARKER` and
   `tests/agent/test_codex_model_entitlement_rotation.py` demonstrate a detail-only
   model-access rejection. Add this one HTTP 400 signature only: bounded top-level
   detail string or detail.message string, containing the exact ASCII-normalized
   marker `model is not supported when using codex with a chatgpt account`.
   It is eligible only with absent/null code and absent/null or recognized type;
   unknown type remains fallback. A matched signature precedes generic type-only
   explanation but emits one fixed model-access message, with no captured model,
   account or detail text. Generic detail remains fallback. Ruling: reuse the
   demonstrated classification with closed output and retained MIT provenance in
   the adapter notice — it is useful diagnosis, not proof this occurred in M47 —
   cost if wrong: a guidance-only category is wrong; no credential/model rotation,
   retry, permission change or raw-body exposure follows.
4. **Behavior and seams:** failure_code/retry classification, endpoint, headers,
   credentials, successful conversion and all existing known-code strings stay
   unchanged. Type/detail recognition cannot enable another attempt. Root uses a
   private version 1, 512 KiB-bounded handoff; no production serialization/endpoint/
   importer/validator/Harmony rewrite is released. Ruling: keep the delta in the
   existing classifier and test seams — debug convenience must not create an
   authority surface — cost if wrong: the fixture/retry matrix fails and actual
   execution stays held. No dependency is needed.
5. **Distinct diagnostic budget:** only prepare one ignored, opt-in native direct
   RuntimeModelAdapter diagnostic with normal ChatGPT vault access through the
   application adapter, same gpt-5.5 route/model, no tools or coordinator
   augmentation, synthetic fixed `M48_ROUTE_OK` request, retries 0 and enforced 20 s
   call/root 30 s bound. It prints only safe error category or marker match,
   response digest/size and reported usage. It never reads/copies a profile or
   credential, executes a tool, creates a helper, sends an account message, or
   reuses M47's workspace/task. The default test suite must not touch the vault
   or network. Ruling: real execution remains held until root explicitly releases
   this one case after gates — a basic-route result cannot clear the full coding
   request — cost if wrong: record unknown/failed spend and stop, with no retry,
   alternate model/provider, second shape or manufactured quality pass.

Core lane owns the existing adapter notice for its narrow attributed classifier
port. Native lane owns only the new provider_request_tests module and one test
registration. Root owns plans, status/handover/ledger, commands and all releases.
The single native/core batch, final affected SDK/web review and one combined fix
wave remain the review process; no separate pre-flight audit is added.
