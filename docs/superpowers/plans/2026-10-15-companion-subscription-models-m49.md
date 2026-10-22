# M49 account-aware ChatGPT model selection implementation plan

> For workers: use the existing lean controller workflow and disjoint releases.

Status: scoped implementation complete; required local gates pass on
`g9000/companion-subscription-models-m49`, cut from verified M48/PR54 main
`317792304772f8cf203f6d6335a1d4a95d4c5cf1`. The one separately released actual
metadata refresh returned unavailable; no catalog or generation is accepted.
Exact source/PR/verified merge settlement and resume are in
`.superpowers/sdd/2026-10-15-companion-subscription-models-m49/progress.md`.
M47's failed useful task and M48's consumed generation release remain unchanged.

**Goal:** Stop selecting a static subscription default as though account access
were known; offer bounded current-account catalog metadata and deliberate selection.
**Architecture:** Reuse existing ChatGPT auth/application adapter, fixed subscription
route and owner API namespace. Add one reusable bounded metadata fetch/parser and
one ephemeral account-scoped host cache. Existing agent model strings and run
configuration semantics remain authoritative; no silent model or account rotation.
**Stack:** Existing Rust/reqwest/serde_json/Tokio, typed SDK, React/Vitest and Nx.
No dependencies, CLI bridge, new runtime, OAuth flow, durable catalog DB or installer.

## 1. Current source findings and minimal change

| Source                                        | Current behavior                                                   | Proposed M49 consequence                                                    |
| --------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| `apps/web/src/lib/daemon-api.ts`              | MODEL_SUGGESTIONS adds two static ChatGPT IDs                      | Stop treating either literal as an account-available/default choice         |
| `components/onboarding/CompanionSetup.tsx:68` | Connected ChatGPT selected, then suggestions[0] becomes model      | Provider may be selected; model stays empty until owner chooses             |
| `components/onboarding/OnboardingFlow.tsx:92` | defaultModel returns first static suggestion                       | Subscription default becomes no selection; other providers unchanged        |
| `components/onboarding/ModelStep.tsx:62`      | Shared model select reads static suggestions                       | Supply account metadata state and explicit custom/current options           |
| `components/SettingsPanel.tsx:133`            | Existing model seeded from static suggestions or custom            | Preserve exact persisted model and dirty draft across metadata changes      |
| `SettingsPanel.tsx:589`                       | Provider change selects options[0]                                 | Entering ChatGPT clears only the new selection, never invents a replacement |
| `components/ChatGptConnection.tsx`            | onChange notices connected boolean transitions                     | Models need connection/principal revision, not connected boolean alone      |
| `packages/sdk/src/chatgpt.ts`                 | Redacted auth status and login/cancel/disconnect only              | Add model metadata methods without breaking these methods/shapes            |
| `routes/contracts/providers.rs`               | Provider entries have configured, no per-model access              | Preserve /api/providers; add focused ChatGPT namespace response             |
| `routes/mod.rs:2215`                          | ChatGPT configured derives from auth status                        | Connected does not imply a model listing or accepted generation             |
| `chatgpt_auth.rs:461`                         | status loads current credential and lifecycle state                | Reuse vault ownership; expose no credential to browser/agent settings       |
| `chatgpt_auth.rs:557`                         | usable_credential serializes refresh and checks generation         | Add smallest private coherent catalog authorization stamp, not new auth     |
| `runtime_model.rs`                            | Explicit provider/model passed to existing application adapter     | Preserve dispatch, timeout, zero-retry and billing route                    |
| `anima-model-adapters/src/chatgpt.rs`         | Fixed subscription responses endpoint/header set                   | Separate fixed metadata GET; never alter inference wire or rotate models    |
| `anima-model-adapters/src/models.rs:1084`     | chatgpt uses OpenAI rows for context/vision                        | Keep static capabilities/pricing distinct from account catalog/access       |
| M48 diagnostics                               | Exact fixed model-access rejection, not a supported-model response | Do not derive an alternative slug or global ban from that one failure       |

The smallest useful change is account-scoped discovery plus honest selection UX.
No current API lists subscription models; no existing static table can establish
the selected account's access. Do not replace one speculative default with another.
Direct owner API requests already specify a model string; keep that explicit path.
Do not add a mandatory catalog preflight to every run or rewrite saved agents.

## 2. Pinned public Hermes comparison

Read public source only at `C:/Users/leoca/AppData/Local/hermes/hermes-agent`,
pin `1fd18277267152893a8a24792b79213e7ca3f899`; no profiles/config/cache or credentials.

| Pinned public source                                   | Useful behavior                                         | Deliberate Anima boundary                                           |
| ------------------------------------------------------ | ------------------------------------------------------- | ------------------------------------------------------------------- |
| `hermes_cli/codex_models.py::_fetch_models_from_api`   | Catalog follows credential route and account headers    | Fixed existing ChatGPT route only; no arbitrary endpoint option     |
| `codex_models.py::_ranked_slugs`                       | Visibility/priority, stable ordering and dedupe         | Preserve exact IDs, bound fields/counts, no executable metadata     |
| `codex_models.py::get_codex_model_ids`                 | Distinguishes live result from curated fallback         | No catalog fallback promoted to account-available/default           |
| `codex_models.py::_auth_cache_key`                     | Principal/route changes invalidate memoized choices     | Use current host auth stamp and opaque public revision              |
| `agent/model_metadata.py::fetch_codex_catalog_entries` | Account catalog endpoint and client_version requirement | One fixed, root-bound protocol query; no two-version fallback probe |
| `model_metadata.py::_codex_oauth_token_fingerprint`    | Credential/route cache separation                       | Never expose raw token or token-derived identifier publicly         |
| `tests/hermes_cli/test_codex_models.py`                | Catalog ranking/account isolation regressions           | Port intent with synthetic Anima DTO/transport tests                |
| `tests/agent/test_model_metadata.py`                   | Endpoint/version/cache behavior                         | No context-window calibration or model-capability rewrite           |

Public source notes `supported_in_api` describes the public API, not subscription
availability; do not remove a subscription row solely because that flag is false.
Hermes also adds static/forward-compatible/context-suffix choices; do not copy them.
Do not read ~/.codex config/models_cache, ~/.hermes state or any private account files.
Catalog presence is metadata, not a generation acceptance or tool-calling guarantee.
Preserve existing MIT provenance if code is adapted; new code uses Anima boundaries.

## 3. Scope and non-goals

- Redacted account-scoped model metadata, explicit bounded Refresh and selection.
- New ChatGPT setup starts with no model selected, including when metadata is ready.
- Owner chooses a listed ID or deliberately enters an unverified custom ID.
- Existing agents keep their exact model/provider until an explicit successful Save.
- Metadata change, account switch, failure and reconnect never rotate a saved model.
- Existing model/context/vision/pricing tables remain static capabilities, not access.
- No model generation to test availability, no M47 replay, helper or actual coding task.
- No plan-tier inference, trial-and-error model scan, account pool, fallback provider,
  synthetic future models, hidden aliases, credential rotation or new auth scope.
- No background refresh loop, startup request, cache-on-disk import or another registry.
- No generic provider model marketplace, API-provider discovery or plugin framework.
- No claim that a listed model will accept the next request or the full tool schema.

## 4. Metadata meaning and states

Use one canonical `ChatGptModelsSnapshot` with a finite status, not a bare string
list that hides offline/empty/error/account state. Root-bound response fields:

```text
{state: unrequested|ready|empty|stale|unavailable|disconnected,
 connectionRevision: opaque-string|null,
 catalogRevision: opaque-string|null,
 fetchedAtMs: integer|null, expiresAtMs: integer|null,
 models: ChatGptModelEntry[{id:string}], error: fixed-safe-string|null}
```

Revision values are host-issued, non-secret and scoped to this daemon lifetime.
No account ID, email, plan claims, token fingerprint, credential, raw upstream JSON,
server URLs, filesystem paths or arbitrary error details are added to this response.
Existing status API fields are not widened or copied into model rows.
Keep display labels equal to bounded exact IDs initially; avoid arbitrary remote
descriptions, links and marketing copy. React renders IDs as text.

- unrequested: no explicit metadata refresh has completed in this connection.
- ready: successful nonempty parsed snapshot for the current authorization stamp.
- empty: successful well-formed zero visible models; do not call it disconnected.
- stale: prior same-connection successful data expired or refresh failed.
- unavailable: no same-connection usable snapshot after bounded failure.
- disconnected: current credentials unavailable/disconnected; expose no prior rows.
- Busy is request/control state, not proof of a different account/model list.
- Authentication failure must remain a fixed failure, not an empty successful list.
- Invalid provider payload must not silently become ready/empty.

Successful TTL:5 minutes; use monotonic expiry internally and safe epoch timestamps
for display. Stale/custom/unlisted rows have a concise unverified status label;
explicit selection or custom entry followed by Save is the owner's deliberate choice.
Cache is memory-only; daemon restart returns unrequested/disconnected with no fetch.
No hidden per-model availability blacklist is inferred from M48 or future failures.

## 5. Reusable fetch/parser interface

Add focused `anima-model-adapters/src/chatgpt/models.rs` and `models_tests.rs`.
Reuse maintained reqwest/TLS and existing redacted credential handling; no new dep.
Frozen core seam: exported `ChatGptModelsAdapter::new(access_token:String,
account_id:String)->Result<Self,String>` and async
`fetch(&self)->Result<Vec<String>,String>`, plus a private pure decoder.
Reuse fixed safe error constants; no engine public DTO/ToSchema/utoipa dependency.
Keep tokens zeroized like the existing adapter, with no secret-bearing Debug output.
Input is host-supplied current subscription credential/account context, never SDK data.
Output contains only bounded parsed metadata or a fixed error category.

Root binds exactly `https://chatgpt.com/backend-api/codex/models?client_version=99.0.0`.
This is the pinned public catalog query, not an Anima/Codex client-feature claim.
One version only: no fallback, alternate query, redirect or account probe.
Fixed bearer, account, originator and User-Agent headers match the current adapter.
Never send a credential to another origin or public API models list; no speculative
residency header, token parser, endpoint override or protocol calibration request.

Bounds: one GET per explicit refresh, overall10 seconds including auth
resolution, connect<=5 seconds, <=512KiB decoded response body, <=512 model rows,
ASCII ID length1..128 bytes, alphanumeric first then alphanumeric or `. _ : / -`.
Priority absent/null becomes10000, otherwise a signed JS-safe integer.
Strict no-redirect/no-retry; Content-Length alone
does not enforce a streamed cap. Body overflow/read failure is unavailable.
Check status before normal parsing; discard bounded/zeroized error bytes and emit
only fixed authorization/unavailable/timeout/invalid-catalog messages.
No raw response/header/token in tracing, panic, assertion output or SDK error body.

Parse the actual models-array shape demonstrated by frozen fixtures.
Ignore unrelated upstream fields; validate every consumed field explicitly.
Visibility absent/null/`list` is visible; `hide`/`hidden` is omitted.
Any other consumed-field mismatch rejects the entire catalog, never partial-ready.
Apply row bounds before filtering; hidden rows cannot bypass resource limits.
Deduplicate exact IDs and sort deterministically by supported priority then code point.
Never synthesize IDs from templates, strip owner suffixes, lowercase IDs or alias them.

## 6. Account ownership, cache and concurrency

Keep the in-memory cache under existing `ChatGptAuth`; a focused private
`chatgpt_auth/models.rs` submodule can hold snapshot/cache types and helpers.
No separate service/registry or lifecycle subsystem. Add only the minimal private
auth-context capture/revalidation seam, factoring usable_credential if necessary
while preserving its current tuple wrapper for inference callers.
Bind lifecycle generation+vault manifest UUID, private account check and fixed route.
Conservatively invalidate on token rotation. Account ID alone is not
enough for A->B->A or same-workspace principal replacement.
Do not cache by connected boolean, selected model, global provider name or plan label.
Use existing credential generation where possible; no new vault format, credential
export, introspection request or CP schema is needed for an ephemeral catalog.
Return an opaque connectionRevision, not the private stamp or token-derived hash.

Resolve usable credentials through existing serialized refresh; capture the stamp
corresponding to those credentials, including any successful normal refresh.
Do not compose an earlier auth revision with a later credential after an await.
Revalidate immediately before request dispatch and after response parsing before
publishing cache state. A switched/disconnected/failed auth context discards late data.
Existing generation access must stay private; no generic credential getter for UI.
No auth/cache/std lock is held across provider network I/O.

One active refresh per auth instance: use try-lock, return fixed busy409 instead of
queuing/coalescing/spawning work. Different-stamp refresh cannot publish old authority;
invalidate old cache publication and keep the finite busy outcome while guard is held.
An older failure cannot erase newer success; publish under exact operation+stamp CAS.
Use a request-owned future+guard. Overall timeout/client drop cancels local I/O and
releases the guard; do not claim that cancellation stopped the upstream server.
No spawned worker/background/shutdown subsystem. Restart performs no automatic fetch.

Catalog refresh is not a transaction that updates agent.model. It never saves an
agent, starts a run, changes grants, approves tools or rewrites a queued run's model.
No durable catalog/history/model-choice provenance registry is introduced.
Current ordinary run snapshots and explicit Settings Save semantics remain intact.

## 7. Minimal public API and SDK additions

Keep `/api/providers`, existing ChatGPT status/login/cancel/disconnect, agent
create/PATCH, workspace/bootstrap and run bodies compatible.
Add owner-only/no-store GET `/api/providers/chatgpt/models` for cached canonical
metadata. GET performs zero upstream catalog requests and no generation.
Add owner-only/no-store POST `/api/providers/chatgpt/models/refresh` with exact `{}`
body for one explicit bounded refresh. No model/account/endpoint/flags in its body.
Both return200 canonical snapshot for metadata failures: unavailable/stale plus a
closed error, never empty-success. Busy409, invalid body400, owner403, vault503 use
the existing static `{error:string}` envelope. No new nested error/code object.
Snapshot error is null or exactly `Model metadata is unavailable.`,
`Model metadata request timed out.` or `Model metadata response is invalid.`.
Disconnected/unrequested/success snapshots use null; refresh auth absence returns
canonical disconnected, while vault I/O failure is503. Never expose provider text.
Reject unknown/null/duplicate body fields consistently with current strict routes.
Use local-owner checks even though the action is metadata-only and no-store on errors.
No production arbitrary endpoint option; tests inject their transport privately.
Host public methods are `models()`/`refresh_models()` and handlers
`models`/`refresh_models`; host maps exact core IDs into its own ToSchema DTO.

Extend `packages/sdk/src/chatgpt.ts` with `models({signal?})` and
`refreshModels({signal?})`; add focused serialization/abort/shape tests and exports.
Use existing shared transport, encoded paths where applicable, no refresh retry.
An uncertain refresh must not be automatically reissued by UI recovery or SDK.
GET-only reconciliation can discover a completed current-stamp result safely.
This is not an inference request key, request replay API or saved model mutation.

## 8. Selection UX and preservation rules

Create focused `hooks/useChatGptModels.ts` and shared
`components/ChatGptModelSelect.tsx`, reused by ModelStep and Settings.
No global cache/dedupe framework: GET per mounted picker is allowed; POST single-flight
is enforced server-side. Consume canonical state by current connectionRevision.
Initial render/status refresh performs GET only; remote metadata fetch requires
the visible Refresh models action, never page load/login completion/background timer.
Explain that refresh contacts the selected subscription service without generating
a reply; it is not a guarantee of model acceptance or zero external activity.

For new ChatGPT setup, remove the static list/default from MODEL_SUGGESTIONS.
The chosen provider can remain ChatGPT; model stays empty until the owner chooses.
A ready catalog offers listed IDs in the established selector plus Custom model.
Unavailable/empty/stale catalogs retain typing and custom entry; no invented fallback.
Use a concise unverified label, without acknowledgement checkboxes/provenance ceremony.
Custom entry or selection followed by Save is deliberate choice, not execution approval.
The existing create/PATCH API still sends the exact explicit model string.

For existing agents, show the exact stored model even if missing, hidden or stale;
label it unlisted/unverified without replacing or silently clearing it.
Unrelated Settings edits remain saveable with the existing unchanged model.
An explicitly changed model requires a selection or deliberate custom entry; no
automatic substitution on provider selection, account reconnect or refresh response.
Keep other-provider suggestion behavior untouched.
Do not add a runtime catalog membership gate that breaks existing/custom model IDs
or direct owner API clients; explicit choice remains allowed and may still be rejected.

Metadata arrival never reseeds dirty name/system/tools/model/custom input, clobbers
Settings shared busy state, resets active access/grants or submits a save.
Account change while a selection is unsaved preserves the text and marks metadata
unverified; there is no implicit replacement or extra confirmation ceremony.
Reject stale async responses on owner/provider/connection revision, including ABA,
unmount, sign-out/reconnect and pending Save. Functional parent updates preserve
canonical MCP/browser permissions and unrelated agent fields.
Current ChatGptConnection connected-only callbacks are insufficient to prove this;
use canonical model snapshot revision rather than inventing broad UI polling.
Existing old-daemon model-route404 renders unverified/custom state; no fabricated
availability or forced provider switch. No automatic auth login/disconnect follows.

## 9. Ownership proposal after root release

| Lane                  | Planned owned source                                                                                                                         |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Core metadata adapter | `anima-model-adapters/src/chatgpt/models.rs`, cfg(test) models_tests.rs, existing chatgpt.rs then lib.rs export, HERMES-NOTICE if ported     |
| Native catalog/auth   | `hosts/rust-daemon/src/chatgpt_auth.rs`, private chatgpt_auth/models.rs and focused tests; routes/chatgpt.rs owner handlers                  |
| SDK then console      | SDK chatgpt.ts/spec/index; one shared hook/select; daemon-api.ts; CompanionSetup, OnboardingFlow, ModelStep, SettingsPanel and focused tests |
| Root                  | Public plan/ledger, routes/mod.rs and minimal registration wiring, exact split, all commands/Git/release/trials/publication                  |

Freeze actual files before edits; do not let lanes concurrently modify registration.
Keep inference `request_body`, retry classifier, endpoint/auth/billing route, static
capability tables and existing model-call deadline implementation unchanged.
No broad catalog framework, migration, dependency or agent schema change is planned.
If an unexpected interface requires broader ownership, root settles it explicitly.

## 10. Test-first sequence and evidence

- [ ] Root settles M48/merge, verifies main, binds five risks and exact metadata
      ownership and five risks, and publishes a lean plan before implementation.
- [ ] Core lane adds controlled RED for absent bounded account catalog parsing
      and fixed transport/header/query behavior; root observes actual semantic RED.
- [ ] Native lane adds actual owner-route RED and generation/late-response cases
      using an in-memory auth fake; never default tests against the OS vault.
- [ ] Console lane adds a meaningful rendered RED: connected ChatGPT must not
      preselect a static model; preserve deliberate saved/custom selections.
- [ ] Freeze core/native DTOs before SDK/console production; disjoint workers only.
- [ ] Release minimal implementations after intended RED; no speculative model list.
- [ ] One combined native/core batch and one final affected SDK/web review;
      one combined fix wave, then root formatting/freezes/fresh required gates.
- [ ] Only after gates may root separately propose any actual metadata acceptance;
      generation remains outside this milestone's automatic authority.

Core parser/transport tests: visible/hidden/unknown visibility, priority ties,
duplicate IDs, rejected Unicode/control/whitespace IDs, count/length/body boundaries, missing or
wrong models array, empty200, malformed JSON/UTF-8, read error, chunked overflow,
timeouts, status401/403/429/5xx, redirect refusal and zero automatic retries.
Verify exact fixed origin/query and intended account header; no credential in
wrong-origin redirects, logs, diagnostic strings, serialized cache or fake fixtures.
Test supported_in_api:false does not become a public-API availability filter.
Do not derive expected IDs from the parser's own output; use literal synthetic IDs.

Native tests: owner/no-store/default GET zero egress; explicit POST exactly one GET;
disconnected no request; coherent refresh/auth stamp; A->B->A; same account/new
principal; token refresh; old success/failure after disconnect/newer result; busy
try-lock zero queued request; client drop; finite deadline; restart zero request/cache.
Assert raw credentials/account/private principal stamp are absent from all public
fields/errors. Cache expiry/failure preserves only correctly labeled same-stamp data.
Refresh never changes agent config/grants/runs, creates a helper or calls generation.

SDK tests: exact additive methods/body, optional abort, static errors/no mutation
retry and existing auth contracts. UI tests: initial no default, explicit listed
choice, unavailable/empty/stale/custom unverified labels, unchanged existing model,
dirty Settings, provider/account transitions, stale-response ABA, refresh/save
race, unmount, old-daemon404, protected grant merges and other-provider regression.
Render and inspect actual dropdown options; closed selector/source alone is not UX proof.
Use finite fake timers/barriers and actual component journeys, not model requests.

Synthetic acceptance: catalog fake initially offers literal synthetic-model-a and
synthetic-model-b, then hides/removes one, switches connection revision and fails
refresh. New setup stays unselected; explicit choice survives correct transitions;
existing agent's model/tools/system remain exact until a deliberate owner save.
Count metadata requests, generation calls0, auth/device starts0 and no model fallback.
This proves metadata and selection behavior, not supported real model identifiers.

## 11. Optional actual metadata check: still HELD

This plan authorizes no account/vault/profile reads, HTTP requests or generation.
After implementation gates, root may propose one explicit Refresh through the
normal app-owned credential path, with one fixed metadata GET and10-second bound.
Require concrete release of that request/destination and capture only bounded
redacted counts/revision/source/status, not credential/raw response/account data.
Do not invoke a model, start the M47 task, try multiple slugs or rotate credentials.
A nonempty real metadata response proves only catalog receipt for that connection
at that time. Empty/failure stays empty/unavailable; no fallback probe/pass hunt.
If no metadata request is authorized, publish deterministic-only implementation
limits after root settlement; do not claim account model access was verified.
Any later generation trial requires its own explicitly selected task/model/budget.

## 12. Root-only future commands and settlement

Future checks after source release; none are preparation results:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='1'
bun --no-env-file x nx run core-rust:test --skipNxCache
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
bun --no-env-file x nx run @animaos/sdk:test --skipNxCache
bun --no-env-file x nx run @animaos/sdk:build --skipNxCache
bun --no-env-file x nx run @animaos/web:test --skipNxCache
bun --no-env-file x nx run @animaos/web:typecheck --skipNxCache
bun --no-env-file x nx run @animaos/web:build --skipNxCache
```

Root confirms resolved targets/forwarding and avoids duplicate core runs only after
inspecting the host target dependency. Proposed focused prefix `m49_chatgpt_models_`.
All default/focused gates use controlled local/in-memory transports and zero real
credentials/providers/models. No DB server/container/daemon/profile import is needed.
Root retains exact source hashes, actual RED/GREEN exits, metadata request counts,
selection/grant snapshots and complete publication/verified merge settlement.

## 13. Top five risk proposals for root binding

1. Evidence/meaning: account metadata informs choices but cannot prove generation
   or tool compatibility. Remove automatic static defaults; no replacement literal.
   Cost if wrong: unverified/custom UX and visible rejection remain, no silent rotation.
2. Credential/account boundary: bind fixed route, exact protocol query/headers and
   coherent private principal/generation stamp, including ABA and refresh. Cost if
   wrong: discard metadata and refuse refresh; never cross-account cache/secret egress.
3. Finite offline behavior:10-second/512KiB/512-row, single-flight, five-minute TTL,
   explicit refresh/try-lock only and no retry/background fetch. Cost if wrong: stale or
   unavailable state, no fabricated ready/empty result or hidden request amplification.
4. Owner choice/concurrency: preserve exact saved and dirty models/grants, require
   deliberate selection/custom entry without a runtime membership gate or ceremony.
   Cost if wrong: conflict/unverified state, never lost edits or implicit agent rewrite.
5. Scope/acceptance: additive existing-provider namespace, maintained HTTP/auth path,
   deterministic tests then at most one separately released metadata check, no
   generation/M47 recovery. Cost if wrong: partial evidence, not another model trial,
   account probe, fallback framework or claim of useful coding/provider acceptance.

Root's narrowed interface decisions are incorporated above. The five risk proposals
await its appended binding section and public plan/source ownership release.

## Controller rulings on the risks (binding)

1. **Meaning and owner choice.** Ruling: remove subscription auto-defaults and
   offer only received metadata plus explicit custom/current values — M48 proved
   this account rejects the old gpt-5.5 choice; a catalog entry still cannot prove
   generation or tool compatibility — cost if wrong: an extra deliberate model
   choice, never a silent provider/model switch or successful-coding claim.
   New setup starts with an empty model. Preserve every saved/dirty model string;
   metadata, account transitions and failures cannot write agents or change runs.
   Explicit custom entry and Save suffice; use concise unverified text, no new
   acknowledgement checkbox or execution/membership gate. API-provider defaults
   and existing direct/custom owner model strings remain supported.
2. **Principal and credential boundary.** Ruling: reuse the normal application
   auth path, binding lifecycle generation plus vault manifest UUID and private
   account check to the fixed route; never expose that stamp or a token hash —
   connected/account ID alone misses replacement and ABA — cost if wrong: discard
   late metadata and show unavailable, without cross-account cache or secret egress.
   A minimal private coherent credential/stamp seam may factor usable_credential
   while preserving its tuple wrapper. Revalidate immediately before dispatch and
   cache publication, with no auth/cache lock across HTTP. GET reconciles current
   scope without token refresh/network; opaque random revisions are display fences.
3. **Finite protocol/cache.** Ruling: one explicit GET to the exact fixed
   `https://chatgpt.com/backend-api/codex/models?client_version=99.0.0`, the pinned
   Hermes catalog query; no redirect, retry, alternate version, endpoint override
   or client-feature claim — a fallback probe hides failed evidence and spend —
   cost if wrong: metadata stays unavailable. Use existing bearer/account/originator/
   User-Agent conventions, 10-second total including auth, five-second connect,
   512KiB body/512 input rows, five-minute monotonic TTL. IDs are exact ASCII,
   1..128 bytes, alphanumeric first then alphanumeric or dot/underscore/colon/slash/
   hyphen. Priority absent/null becomes10000, otherwise JS-safe signed integer.
   Visibility absent/null/list is visible; hide/hidden omitted; any other consumed
   field mismatch rejects the entire catalog. Ignore supported_in_api. Sort by
   priority then exact ID and deduplicate; never synthesize or normalize IDs.
   Request-owned future/try-lock has no queue or spawned/background producer;
   drop/timeout cancels local I/O without claiming server cancellation. Restart
   discards the cache, with zero automatic fetch. Failed refresh labels previous
   same-scope data stale; unknown data is never promoted to ready or empty.
4. **Contract and UI races.** Ruling: freeze the additive snapshot and two owner
   routes from sections4/7, cache-only GET and exact-empty-object POST; use200
   snapshot for metadata unavailable/stale, fixed409 busy/400 body/403 owner/503
   vault envelopes, no-store everywhere — one canonical contract avoids client
   guesses — cost if wrong: explicit unavailable/reconciliation, never draft/grant
   loss, account rotation or repeated POST. Shared hook/select is enough; no global
   cache framework. A mounted picker may GET locally; only Refresh may POST.
   Preserve unrelated Settings fields/permissions and existing auth APIs, static
   context/vision/pricing and inference serialization/retry/deadlines exactly.
5. **Release and evidence.** Ruling: root publishes this plan, observes meaningful
   core-decoder/native-route/rendered-UI REDs, then releases minimal implementation
   in three disjoint lanes — source inspection or catalog receipt is not useful
   task acceptance — cost if wrong: retain failed evidence and no parity claim.
   One combined native/core batch, one final SDK/web review, one fix wave, then
   required gates. No new dependency. Generation and M47 replay are excluded.
   Only after gates may root separately release at most one10-second actual
   metadata refresh via the application adapter, with no generation/fallback.

### Frozen execution ownership and first release

- Core lane: packages/core-rust/crates/anima-model-adapters/src/chatgpt/models.rs,
  packages/core-rust/crates/anima-model-adapters/src/chatgpt/models_tests.rs,
  minimal `chatgpt.rs` registration and `lib.rs` export, existing adapter notice.
  Final portable interface is ChatGptModelsAdapter::new(String,String) and async
  fetch()->Result<Vec<String>,String>. First release: pure decoder error stub and
  meaningful literal parser RED tests only; no HTTP/export implementation yet.
- Native lane: `hosts/rust-daemon/src/chatgpt_auth.rs`, private
  `hosts/rust-daemon/src/chatgpt_auth/models.rs` and focused tests,
  `hosts/rust-daemon/src/routes/chatgpt.rs`, minimal routes/mod.rs wiring and
  README route inventory. First release: two actual route REDs in the existing
  route test module only; no new handlers/cache/auth seam/wiring yet.
- SDK/web lane: SDK chatgpt.ts/spec/index exports; shared useChatGptModels hook and
  ChatGptModelSelect component, ChatGptConnection, daemon-api, CompanionSetup,
  OnboardingFlow, ModelStep, SettingsPanel and focused tests. First release: one
  actual rendered CompanionSetup RED against the static auto-default only; no SDK
  or component implementation yet.
- Root: public plan/status/handover, private ledger, all commands/format/Git,
  root releases, reviews and optional actual operation. No overlapping writers.

Core success/status/body bounds are tested with joined synthetic HTTP peers.
Native auth/cache late results and default routes use in-memory auth/fake metadata,
never OS vault. The ignored optional actual driver must check its exact opt-in
before auth/client/vault/I/O and stay held until root release after gates.

## Recorded outcome (2026-10-15)

Implemented the reusable fixed-route bounded catalog adapter and ephemeral
principal-bound host cache, cache-only owner GET, explicit owner POST Refresh,
typed SDK and shared web selector. New ChatGPT setup requires deliberate selection.
Saved/custom/dirty models and grants survive cache/account transitions; metadata
never changes inference dispatch or proves generation entitlement. Pinned public
Hermes ranking/query behavior is narrowly reused with the existing MIT notice.
No new dependency, manifest/lockfile, ordinary profile or service was introduced.

Meaningful decoder, actual-route and rendered-default REDs preceded implementation.
The final SDK/web review was clean. The combined native/core review found one
asynchronous deadline gap; two contention regressions failed against original
production, then passed after the one fix wave. One absolute deadline now covers
initial lock acquisition through publication. Expiry invalidates the private cache
epoch before single-flight release, without awaited or spawned cleanup. Existing
synchronous OS-vault operations are not newly promised physically preemptible.

Required fresh gates passed: **2,709 Rust/core tests / 20 intentional ignores /
40 suites**, Rust formatting/build, **151 SDK tests / three real-service skips**,
**1,320 web tests / 115 files**, SDK/web type checks and builds. Both deadline
regressions and the compiled 194-operation README/OpenAPI inventory passed.
All 29 reviewed source/README hashes remained unchanged throughout the gates.
Owned Markdown/TypeScript formatting and final document checks are recorded in
the private ledger. Local gates remain authoritative given the known Nx Cloud step.

After gates, the single metadata-only driver selected exactly one ignored test
through normal application vault access. It returned state **unavailable**,
zero IDs, fixed `Model metadata is unavailable.` in **1.45 seconds**. The
empty-ID JSON digest is
`4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945`.
This is a failed availability check, not a successful empty catalog. The fixed
category does not distinguish transport, non-success HTTP, credential resolution
or a size-bound failure; no raw error body/cause was retained. No retry, query
fallback, generation, helper, daemon, connector, DB server or container followed.
At most one catalog GET was authorized; whether every pre-dispatch condition
succeeded is not proven by this redacted receipt. The release is consumed.

Useful provider coding/delegation remains unaccepted. A distinct JSONL task and
literal independent oracle may be prepared next, while generation remains held
until deliberate current-account selection has evidence. Preserve the original
M47 failed candidate/result; do not repair/replay it or substitute a guessed slug.
Native voice and broader account/host/quality parity stay open.
