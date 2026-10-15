# M50 closed catalog diagnostics implementation plan

Status: scoped diagnostics complete; fresh Rust gates pass on
`g9000/companion-catalog-diagnostics-m50`, from verified M49/PR55 main
`67f047d6d657145b57d8e22c75da216ae0e83e90`. One distinct actual diagnostic
observed HTTP200 rejected by the declared body limit; no catalog was parsed.
Exact source/PR/verified merge and resume are in
`.superpowers/sdd/2026-10-15-companion-catalog-diagnostics-m50/progress.md`.
Useful task/generation and the renumberable JSONL proposal remain held.

**Goal:** Distinguish the next explicitly released catalog failure by safe category,
stage, dispatch certainty and HTTP/body-limit evidence without changing owner UX.
**Architecture:** Small typed core catalog failure plus request-owned native outcome;
reuse the fixed adapter, credential path, timeout/cache fences and ignored driver.
**Stack:** Existing Rust/reqwest/serde/Tokio/Nx. No dependency or diagnostic framework.

## 1. New evidence and its limits

- Controller reports all nine M49 gates passed: Rust 2,709 / 20 ignored,
  SDK 151 / 3 skipped, web 1,320, with required builds/typecheck/lint included.
- One separately root-released actual metadata driver returned fixed unavailable,
  count 0, in approximately 1.45 seconds. No retry or generation followed.
- That result retained no HTTP status, failure stage or dispatch evidence.
  It cannot prove a catalog request was dispatched, an HTTP rejection, a bad
  credential/header, a body limit, a service outage or a specific protocol cause.
- Current unavailable combines authorization refresh, client construction,
  request/transport, non-success HTTP and size/read failures. Invalid-catalog,
  timeout, disconnected, busy and vault results already have distinct safe paths.
- Preserve M49's original failed evidence and consumed release. This proposal
  neither repairs that attempt nor recovers its missing historical detail.
- Metadata receipt cannot establish model entitlement, tool support or generation.

## 2. Existing seam and design choice

| Option                                      | Decision                                                                   |
| ------------------------------------------- | -------------------------------------------------------------------------- |
| Typed safe catalog failure returned by core | Choose: small explicit values, no shared mutable state                     |
| Generic observer/callback/event sink        | Reject for this slice: lifetime/cancellation plumbing exceeds the need     |
| Last-error global/cache field               | Reject: races, cross-account attribution and a new public/durable contract |
| Raw exception/body logging                  | Reject: cannot safely bound secrets by redacting arbitrary strings         |

Current `ChatGptModelsAdapter::new` and `fetch` return String errors; the only
production caller found is `OfficialModelsTransport` in native catalog auth.
Propose a small exported `ChatGptModelsFailure` and closed enums on an additive
detailed path. Preserve existing constructor/fetch String-error compatibility as
thin wrappers around that same implementation and its static public-message map;
there is no SDK, HTTP snapshot, inference API or compatibility-path request change.

Interface sketch only, not implementation:

```text
new(token, account) -> Result<Self, String>                 // unchanged wrapper
fetch() -> Result<Vec<String>, String>                    // unchanged wrapper
new_detailed(token, account) -> Result<Self, ChatGptModelsFailure>
fetch_detailed() -> Result<Vec<String>, ChatGptModelsFailure>
ChatGptModelsFailure = closed stage + catalogDispatch + reason + optional u16 status
public_message() -> exactly one existing static metadata error string
native diagnostic refresh -> { existing result, request-owned safe diagnostic }
ordinary refresh_models() -> discard diagnostic, return unchanged existing result
```

- No upstream error, URL, header, response body, credential or account field lives
  in the failure type; private fields/accessors enforce the finite representation.
- No `source()` wrapping reqwest/auth errors, arbitrary String or raw Debug output.
  Display/static conversion preserves the existing three metadata strings exactly.
- A private native diagnostic variant shares the ordinary refresh implementation;
  it is not a second fetch, test transport bypass, HTTP endpoint or cache producer.
- Return the diagnostic to its own ignored-driver invocation; no cache, routine
  logging, route/global telemetry, last-result registry, durable DB or UI polling.

## 3. Closed diagnostic vocabulary and certainty

Proposed stage enum: `authorization`, `client`, `request`, `http_status`, `body`,
`decode`, `scope`, `deadline`, `complete`, `control`.
Proposed catalogDispatch enum: `not_started`, `attempted`, `response_received`,
`unknown`. This describes the catalog operation, not credential-refresh dispatch.
Proposed reason enum: `none`, `credential`, `client_build`,
`request_build`, `connect`, `transport`, `http_status`, `declared_limit`,
`streamed_limit`, `body_read`, `invalid_catalog`, `scope_changed`, `timeout`,
`disconnected`, `busy`, `vault`.

Root may remove redundant values before binding, but cannot add arbitrary detail.

| Observed branch                                    | Safe diagnostic                                          | What it does not prove                                |
| -------------------------------------------------- | -------------------------------------------------------- | ----------------------------------------------------- |
| Credential resolution fails before catalog adapter | authorization/not_started                                | Auth HTTP status, bad account or token cause          |
| Client/header/request construction fails locally   | client or request/not_started                            | Any upstream catalog activity                         |
| Catalog send future attempted, no response         | request/attempted, connect or transport                  | Request bytes arrived or provider processed it        |
| HTTP response observed                             | http_status/response_received, exact numeric status      | Meaning of the provider's unretained body             |
| Declared/streamed cap exceeded                     | body/response_received, declared_limit or streamed_limit | A malformed model listing or account access denial    |
| Response body read fails                           | body/response_received, body_read                        | Complete provider body or specific network root cause |
| Bounded decoding rejects payload                   | decode/response_received, invalid_catalog                | An alternative version/model would work               |
| Scope/CAS rejects late result                      | scope, retain only known certainty                       | Authority to publish prior-account rows               |
| Outer deadline drops unfinished work               | deadline/unknown unless separately proven                | Delivery, server cancellation or zero billing         |

- Optional `httpStatus` is a numeric catalog response status only when observed;
  null otherwise. Never synthesize 200, infer auth status or parse error text.
- Reqwest classifier uses structural predicates only; `connect` can include DNS,
  TLS or proxy failure and must not be relabeled as any one of those causes.
- Request-build classification must precede an attempted-dispatch claim. Marking
  a request future polled is not proof that bytes left the machine.
- Success may report complete/response_received and count; absent retained success
  status stays null. Empty successful metadata remains empty, not unavailable.
- Driver output is a fixed bounded object: state, count, stage, catalogDispatch,
  reason, httpStatus and existing static error. No model IDs or raw metadata.
- Exclude raw errors/bodies/tokens/accounts/request IDs/URLs/headers, principal
  stamps, token hashes, paths, arbitrary strings and exception chains everywhere.

## 4. Behavior that must remain exact

- Fixed M49 origin/path/query, one version, existing bearer/account/originator/
  User-Agent conventions; no alternate destination, residency/header experiment,
  auth-flow change, endpoint override, retries, redirects or fallback.
- Ten-second total native deadline including auth/locks/publication, five-second
  connect bound, 512-KiB streamed cap, 512 input rows and exact decoder semantics.
- Non-success responses are classified from status without reading their bodies.
  Bounds still apply while streaming; Content-Length alone is insufficient.
- Cache-only owner GET; explicit exact-{} owner POST; no-store including errors.
- Preserve all existing snapshot fields exactly as the reviewed SDK declares
  them; do not add diagnostics to that wire object.
- Preserve six snapshot states, opaque revisions, nullable times and closed errors;
  409 busy, 400 body, 403 owner and 503 vault envelopes remain unchanged.
- Retain current manifest/lifecycle/account/epoch/operation fences, single-flight
  ownership, cancellation/drop, no lock across HTTP and no background producer.
- No saved model/grant/agent mutation, static default, runtime membership gate,
  inference serializer/model table/retry/deadline change or helper/model call.
- Preserve ordinary usable_credential tuple and its existing public errors. Auth
  refresh failure stays credential category with no claimed HTTP cause; do not
  parse raw auth exceptions or widen auth transport logging for a finer label.

## 5. Source ownership and public Hermes comparison

- Core slice: `packages/core-rust/crates/anima-model-adapters/src/chatgpt/models.rs`,
  `models_tests.rs`, minimal `chatgpt.rs`/`lib.rs` type export and existing notice.
- Native slice: `hosts/rust-daemon/src/chatgpt_auth/models.rs`, `models_tests.rs`,
  only a minimal private authorization seam in `chatgpt_auth.rs` if demonstrated
  necessary; preserve inference wrapper behavior. Root owns actual driver selection.
- Existing `routes/chatgpt.rs`, SDK chatgpt types/methods and UI are read-only
  compatibility boundaries unless a concrete failure requires root scope binding.
- Read-only public Hermes comparison inspected `hermes_cli/codex_models.py`
  `_fetch_models_from_api` and `agent/model_metadata.py::fetch_codex_catalog_entries`.
  Useful intent: retain numeric HTTP status separately from returned entries.
- Do not port its raw exception logging, multi-version probing, default/negative
  caches, token fingerprints, credential/header inference or static model fallback.
- Existing HERMES-NOTICE records M49 reference `1fd18277267152893a8a24792b79213e7ca3f899`.
  Root rechecks public source provenance before adaptation; this draft did not run
  Git or claim a fresh pin verification. Preserve all existing notices/full MIT text;
  add a narrow attribution only if implementation actually adapts upstream material.

## 6. Synthetic REDs, implementation and one review/fix wave

- [ ] Root verifies M49 main, binds five risks and exact interface/ownership release.
- [ ] Core tests distinguish locally injected client/build failure, connect/transport
      failure, 401/403/429/5xx/redirect status, known/chunked overflow, body-read
      failure, timeout, invalid JSON/UTF-8/consumed fields and valid empty/nonempty.
- [ ] Root observes semantic RED: distinct synthetic failures currently collapse
      to unavailable without the required typed stage/status/dispatch evidence.
- [ ] Native REDs prove authorization failure before catalog calls, typed transport
      mapping, outer timeout certainty, no dispatch on disconnected/busy/vault,
      account ABA/late results and independent concurrent diagnostic attribution.
- [ ] Tests put synthetic sensitive markers in fake errors/body/headers/account,
      then assert their absence from safe failures, Debug/Display, driver output,
      public snapshots/envelopes and retained diagnostics. No real vault/default I/O.
- [ ] Implement the smallest typed failure/native result path; preserve all prior
      exact request-count, cache, deadline/lock, drop/CAS and decoder regressions.
- [ ] One combined native/core source review and final affected SDK/web scopecheck:
      confirm unchanged routes/body/snapshot/strings and no auto-fetch/retry/UI drift.
- [ ] One combined fix wave, source formatting/freeze and fresh required gates.
      Add no generic observer, tracing framework, cache schema or broad auth rewrite.

Future root commands, not preparation results:

```powershell
bun --no-env-file x nx show project core-rust --json
bun --no-env-file x nx show project rust-daemon --json
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
bun --no-env-file x nx run @animaOS-SWARM/sdk:test --skipNxCache
bun --no-env-file x nx run @animaOS-SWARM/sdk:build --skipNxCache
bun --no-env-file x nx run @animaOS-SWARM/web:test --skipNxCache
bun --no-env-file x nx run @animaOS-SWARM/web:typecheck --skipNxCache
bun --no-env-file x nx run @animaOS-SWARM/web:build --skipNxCache
```

Inspect target dependencies/forwarding; avoid duplicate core runs only with evidence.
Proposed focused prefix: `m50_catalog_diagnostics_`. All default/focused tests use
controlled in-memory/loopback transports, zero real metadata/model/account I/O.

## 7. One later actual diagnostic attempt: separately HELD

- Only after gates and exact root release may the ignored driver run once through
  the normal application-owned credential/adapter path and unchanged fixed query.
- Exact opt-in must be checked before auth/client/vault/I/O, with a separate M50
  selector/flag so M49's consumed release cannot accidentally authorize this case.
- Release binds exact binary/source, driver, operation, destination, ten-second
  deadline, zero-or-one catalog GET, bounded safe output and owned cleanup.
- Record one outcome even when authorization fails before catalog dispatch. Do not
  require an HTTP status to declare the attempt consumed or automatically repeat it.
- No error-body capture, alternate version/headers/model/provider, generation or
  M47 replay. No second actual request to clarify an unknown diagnostic.
- Emit only the closed object; root interprets observed category with its limits.
  Failure stays failure; success proves metadata receipt only. Useful JSONL/M51
  remains separately held pending deliberate model selection and its own release.

## 8. Top five choices awaiting controller rulings

1. **Small typed seam:** retain String-error constructor/fetch wrappers, add a safe
   detailed core failure path and request-owned native outcome; no global state.
   Cost if wrong: hold the diagnostic, never SDK/UI/inference compatibility drift.
2. **Closed data:** enums plus optional numeric catalog status; no arbitrary
   strings/raw bodies/errors/IDs/URLs/headers/accounts/tokens or token fingerprints.
   Cost if wrong: withhold diagnostics rather than retain secret-bearing evidence.
3. **Honest certainty:** attempted is not delivered, null/unknown stays unknown,
   body-limit is distinct from parser rejection; auth HTTP causes remain unproven.
   Cost if wrong: limited diagnosis, never a guessed outage/account/protocol claim.
4. **Unchanged operation:** exact M49 route/query/headers/bounds/cache fences,
   no retry/fallback/generation; provenance reviewed and notices preserved.
   Cost if wrong: stop at one failed observation, not an expanded provider probe.
5. **Release and evidence:** synthetic REDs, one combined batch/scopecheck/fix wave,
   fresh gates, then at most one separately released actual metadata diagnostic.
   Cost if wrong: held/failed evidence retained; no M47 recovery or parity claim.

These choices are proposals only. This private file grants no execution authority.

## Controller rulings on the risks (binding)

1. **Small typed seam.** Ruling: retain exact String-error constructor/fetch
   wrappers, add only a closed detailed constructor/fetch failure and request-local
   native diagnostic outcome — the actual M49 receipt lost the distinction needed
   to select a justified fix — cost if wrong: a small additive core seam, never a
   new HTTP/SDK/UI/inference contract. One source author owns both core/native to
   keep the narrow interface coherent; root first observes a real synthetic HTTP
   status RED against a stub detailed path, then releases minimal implementation.
   Existing public wrappers must delegate the same implementation, not diverge.
2. **Closed information.** Ruling: private finite enums/accessors and optional
   numeric catalog response status are the entire diagnostic authority — arbitrary
   errors and redaction cannot establish secret-free evidence — cost if wrong:
   lose detail rather than store credentials or upstream text. Do not retain raw
   strings, bodies, headers, URLs, account/token fingerprints or exception sources.
   Native outcome is private and request-local; no snapshot/cache/log/route fields.
   Debug/Display must be closed and synthetic sensitive-marker assertions literal.
3. **Certainty and lifecycle.** Ruling: distinguish local not-started/attempted/
   observed-response and preserve unknown outer-timeout state — polling a send is
   not server delivery and auth failure supplies no catalog HTTP cause — cost if
   wrong: a limited category that cannot justify an outage/account claim. Build
   request locally before marking attempted, observe numeric status only from the
   actual response, distinguish declared/streamed limit/read/decode failure. Retain
   the existing absolute deadline, synchronous epoch invalidation, operation CAS,
   principal/lifecycle/manifest fences and single-flight cancellation unchanged.
   Scope changes must not misattribute prior-account success to current scope.
4. **Unchanged operation.** Ruling: preserve M49's exact fixed endpoint/query/
   headers, one request, bounds and parser — no status/cause currently establishes
   a protocol defect — cost if wrong: one visibly failed observation, not a fallback
   or model trial. Recheck only pinned public upstream source/license; existing MIT
   notice stays. Do not copy upstream raw exception logging, version fallback,
   residency/header guesses or static model synthesis. Normal credential resolution
   is reused; no private profile reads/copy or auth transport redesign.
5. **Evidence and release.** Ruling: one meaningful core transport RED, focused
   synthetic core/native regressions, one combined native/core review plus final
   affected SDK/web scope check, one fix wave and required current Rust gates —
   source inspection alone cannot prove the detailed path — cost if wrong: retain
   failed evidence and keep actual operation held. SDK/web/routes stay source-frozen;
   their M49 gates are prior evidence, not claimed fresh M50 runs. Repeat their
   relevant gates only if concrete changes broaden scope. Root owns all commands/
   formatting/Git/services/models, CARGO_INCREMENTAL=0/jobs1/two full-suite threads.
   Only after gates may root separately release one normal application-adapter
   metadata diagnostic with distinct M50 opt-in/selector, ten-second async bound,
   fifteen-second outer driver, zero-or-one fixed catalog GET, zero generation/
   tools/helpers/retries/fallback. Any outcome consumes that release; no second
   attempt to refine its unknowns. M47/M48/M49 old releases stay consumed.

### Execution ownership and first release

The native/core author owns the two existing catalog source/test pairs, minimal
chatgpt.rs/lib.rs exports and, only if needed, the private chatgpt_auth.rs seam.
Existing HERMES-NOTICE may receive a narrow actual-reuse note without deleting
its MIT text. Do not modify routes, SDK, web, manifests or lockfiles. First release
is detailed-type/test stub plus one synthetic 403 semantic RED only, while existing
fetch behavior stays exact. Root observes RED and freezes assertions before full
production release. The independent reviewer is source-only and root owns public
docs/ledger/commands/Git. No other source writer or actual operation is released.

## Recorded outcome (2026-10-15)

Six source files implement a closed detailed core failure and private request-local
native outcome. Existing constructor/fetch String wrappers share that implementation;
public snapshots, routes, SDK/web, inference, fixed query/headers/parser, auth path,
cache/principal/operation fences and asynchronous deadline remain unchanged.
No dependency, manifest/lockfile, telemetry/cache schema or raw error capture.
Existing MIT notices remain intact; no substantial upstream code copy was added.

The original synthetic HTTP403 test was observed RED after one controlled response
lost its stage/status, then GREEN unchanged in the full suite. The combined native/
core review and final SDK/web scope check found no production issue. One P2 test
assumed a closed Windows port would refuse within one second; the run correctly
returned Timeout. The one fix wave replaced that assumption with a local no-proxy
failing resolver and counter, preserving exact Connect/Timeout assertions. Stripping
the cfg(test) setter reproduces the reviewed production-source hash.

Fresh required gates passed **2,719 Rust/core tests / 21 intentional ignores /
40 suites**, formatting and build, including all ten default M50 cases. Six source
hashes and 25 SDK/web/routes/README/manifest/lock boundary hashes stayed frozen.
Those unchanged TypeScript paths retain M49's evidence; no fresh M50 SDK/web run is
claimed. Final owned Prettier/document/diff checks are recorded in the ledger.

After gates, exactly one separately released actual driver failed availability in
**0.84 seconds** with closed evidence: stage **body**, reason **declared_limit**,
catalogDispatch **response_received**, HTTP **200**, state unavailable and count0.
The current response's declared body exceeds the existing **512KiB** cap; the
adapter discarded it before catalog parsing. No model IDs or raw body were retained.
This is a concrete limit blocker for this new request, not a recovered explanation
of M49's historical unavailable result. No retry, second query/header/model/provider,
generation, helper, daemon, connector, DB server or container followed. Release consumed.

Next is a narrow bounded catalog-budget correction using synthetic valid padded
catalogs and exact boundary checks, then a separately released metadata observation.
Keep the fixed route/query and explicit selection; no speculative inference/model
fallback. The distinct useful JSONL task and native voice stay held/renumberable.
M47's original failed candidate/result remains immutable; full parity is not claimed.
