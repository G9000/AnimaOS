# M65 owner-configured bearer authentication for HTTP MCP

**Status:** implemented; reviewed source and required local gates pass with synthetic credentials/local TLS. Actual OS-vault/account/remote-server acceptance remains open.

## Goal

Let the local owner explicitly save, replace, and clear one bearer credential for an HTTPS Streamable HTTP MCP connection, stored in the OS credential vault and attached only to that connection's HTTP requests. Existing unauthenticated HTTP/HTTPS and stdio behavior stays intact.

## Evidence and reuse

- `hosts/rust-daemon/src/mcp/transport.rs` (`HttpBackend::post`) receives RMCP's optional `auth` value and currently rejects any `Some`; it also accepts only `mcp-protocol-version` from custom headers. The same backend owns redirect denial, no-proxy behavior, request deadlines/cancellation, wire caps, and shutdown joins; preserve those controls.
- `hosts/rust-daemon/src/mcp/records.rs`: `McpConnectionRecord` persists nonsecret endpoint/owner/review metadata; `CreateMcpConnection` and `UpdateMcpConnection` are strict DTOs. Do not put bearer bytes in this record, snapshot, debug output, or SDK response.
- `hosts/rust-daemon/src/mcp/service.rs`: `McpManager::owned` owns mutations through completion; `connect`/`discover` establish the per-connection generation; endpoint edits retire old work. Extend these existing owner/version/cancellation boundaries rather than opening a parallel transport path.
- `hosts/rust-daemon/src/routes/mcp.rs` and `routes/contracts/mcp.rs`: owner-authorized, no-store MCP routes and response projection already exist. Add explicit credential set/replace/clear operations; responses may expose only a boolean such as `hasBearerCredential`.
- `hosts/rust-daemon/src/connectors/credentials.rs`: reusable pattern for `Zeroizing<String>`, redacted `Debug`, in-memory test store, OS keyring backend, uncertainty-aware reads/writes/deletes, and owned blocking mutations. Use a dedicated MCP keyring service namespace and connection UUID account key; do not reuse Telegram account keys.
- `hosts/rust-daemon/src/app.rs`: production startup installs OS keyring connector stores; deterministic runtime injects in-memory stores. Thread an MCP-specific store through the existing `McpManager` construction seam (`coordinator_with_mcp`) and tests.
- `packages/sdk/src/mcp.ts`, `apps/web/src/components/mcp/McpConnectionsPanel.tsx`: typed connection client and current owner CRUD/connect/discover/review controls. Add a password input with explicit Save/Replace and Clear, clear the local input after settlement, and never re-display the secret.
- Pinned Hermes `1fd18277267152893a8a24792b79213e7ca3f899`: `hermes_cli/mcp_config.py` builds `Authorization: Bearer ${MCP_<SERVER>_API_KEY}` and saves bearer values in its protected `.env`; `tools/mcp_tool_config.py` resolves placeholders and rejects unresolved references; `tools/mcp_tool_transport.py` forwards configured HTTP headers. This supports the selected static bearer capability, not a need to port Hermes config storage or OAuth.
- RMCP 3.5.1 source (`src/transport/common/reqwest/streamable_http_client.rs`) applies `auth_token` with `bearer_auth`, and `src/transport/common/auth/streamable_http_client.rs` implements challenge/refresh OAuth behavior. M65 should use only the static, owner-stored bearer; do not add `AuthClient`, OAuth, refresh, arbitrary header maps, or model-supplied auth values.

## Frozen scope and rules

1. Only Streamable HTTP over `https://` may have a bearer credential. Existing unauthenticated HTTPS and numeric-loopback HTTP continue to work. Reject credential set for stdio or HTTP endpoints.
2. Owner must explicitly enter, save/replace, or clear the secret. No connect/discover auto-probe, token acquisition, OAuth flow, refresh, environment expansion, arbitrary headers, URL query credentials, or secret entry in model/tool arguments.
3. Keep secret bytes out of `McpConnectionRecord`, durable control-plane snapshots, logs, errors, tracing, previews, browser storage, page state after settlement, and all public output. A boolean configured marker is the only credential metadata exposed.
4. Store one secret per connection UUID in an MCP-specific OS-keyring namespace. Return `Zeroizing` secret material only to the trusted native transport. The deterministic runtime uses an injected in-memory store. Keyring mutation outcome uncertainty must fail closed and remain visible as a generic unavailable state.
5. Auth is bound to the saved HTTPS endpoint. Editing the endpoint disconnects/retire current generation and clears its bearer before new connection use; require re-entry. Changing or clearing a credential retires that connection's live generation before applying new auth. Respect expected-version/CAS checks and don't broaden grants.
6. At HTTP transport boundary, add only `Authorization: Bearer <owner secret>` to the existing request builder for initialize, POST/SSE, GET, and session DELETE as applicable. Preserve redirect policy `none`, `.no_proxy()`, one-request/no-retry behavior, cancellation, wire/deadline limits, joined close, and outcome-unknown semantics. Do not forward a credential to a different origin or redirect.
7. Stdio launch schema, child env, and stdio transport remain byte-for-byte outside auth scope.

## Owned interfaces and likely paths

Exact owner contract when bound:

- `PUT /api/agents/{agent_id}/mcp-connections/{connection_id}/credential` takes strict `{expectedVersion,bearerToken}` and returns the normal secret-free connection view.
- `DELETE` on that same path takes strict `{expectedVersion}` and returns the normal connection view.
- Connection views add `hasBearerCredential: boolean`, reflecting a saved credential reference only, never vault availability, authorization validity or remote health. List/get must not read the OS vault.
- Private durable connection metadata may add an absent-by-default opaque credential reference. Fresh references identify separate MCP-namespace vault slots, bound to the exact normalized HTTPS endpoint and connection UUID. Never reuse a previous reference on replacement/endpoint change. Preserve existing uncredentialed serialized rows.
- Set/replace is a real versioned change; clear with no reference is a checked no-op. Missing/unreadable referenced secret makes explicit connect unavailable without network or auto-clearing/retrying. Endpoint edits invalidate the current reference; stale/orphan vault entries cannot become live by editing the endpoint back.
- All mutation response outcomes clear the UI password control. Uncertain saves keep the connection unavailable and require explicit refresh/re-entry; no automatic retry or rollback claim. Token values stay only in the password control/request body and native zeroizing material until settlement, never browser storage.

- `hosts/rust-daemon/src/mcp/credentials.rs` (new): `McpBearerToken` (zeroizing/redacted), `McpCredentialStore` async trait, `InMemoryMcpCredentialStore`, OS-keyring store, bounded validation, uncertainty-safe mutation semantics. Mirror connector credential patterns but use MCP service/account namespace and UUID validation.
- `hosts/rust-daemon/src/mcp/mod.rs`, `app.rs`, `mcp/service.rs`: inject store into `McpManager`; add owner-scoped set/clear methods executed through `owned`; disconnect/retire before replacing or clearing. Ensure endpoint update deletes its associated secret before endpoint takes effect.
- `hosts/rust-daemon/src/mcp/transport.rs`: extend the private validated HTTP launch/session construction seam to receive a trusted native bearer token; attach it only in the HTTP backend. Never source auth from `McpCall.arguments` or remote tool descriptors.
- `hosts/rust-daemon/src/mcp/records.rs`, `routes/contracts/mcp.rs`, `routes/mcp.rs`: project only `hasBearerCredential`; add strict versioned credential mutation request(s), owner authorization and `Cache-Control: no-store`. Keep response DTOs secret-free.
- `hosts/rust-daemon/src/mcp/tests.rs`, `mcp/transport.rs` tests, `routes/mcp_tests.rs`: add store, transport, lifecycle and route regressions alongside existing MCP suites.
- `packages/sdk/src/mcp.ts` and `mcp.spec.ts`: typed owner credential operations and boolean projection; no property returns token bytes.
- `apps/web/src/components/mcp/McpConnectionsPanel.tsx` and its test: HTTPS-only password field, explicit save/replace/clear, configured marker, disabled/busy/error behavior, no echo or persistence.
- Update relevant MCP owner docs only after implementation/acceptance are actually bound and passed.

## Test acceptance list

- Secret wrapper rejects empty, oversized, control/non-header-safe values and has redacted `Debug`; in-memory and keyring-backed adapter tests prove put/load/replace/delete, absent values, backend failures, and uncertain mutation outcomes.
- Transport fixture captures exact `Authorization: Bearer fixture-secret` on initialize, discovery, tool call and close/session-delete request. Verify no auth on unauthenticated connections, no retry, and no credential at redirect target; preserve existing cancellation, no-proxy and joined-close tests. Exercise the RMCP `auth` callback behavior explicitly so the static secret has one trusted source and does not activate OAuth refresh.
- Service tests cover owner isolation, HTTPS-only set, stdio/HTTP refusal, stale versions, replace/clear after retiring active generation, endpoint-change secret invalidation, revocation, failed vault operation, and quiet restart from a fresh serialized registry with secret absent from serialized bytes.
- Route tests cover non-owner denial, request-size/strict DTO rejection, no-store responses, secret-free list/get/error JSON, and set/replace/clear producing only the configured boolean.
- SDK tests assert exact routes/body shape and prove no token appears in response types; web tests cover user-explicit actions, input clearing after every settled outcome, configured/empty state, non-HTTPS/stdIO hiding or refusal, explicit re-entry after failure, and no token echo/local persistence.
- Preserve existing HTTP MCP, reviewed grants, CAS, stdio allowlist/launch, and all unauthenticated transport cases.

## Commands for a future bound implementation

- Focused Rust: `CARGO_INCREMENTAL=0 bun x nx run rust-daemon:test --skipNxCache` (serialize Cargo and use the repository's alternate target-dir instruction if the daemon binary is locked).
- SDK: `bun x nx run @animaOS-SWARM/sdk:test --skipNxCache` and the repository's SDK typecheck target after confirming target names with Nx.
- Web: `bun x nx run @animaOS-SWARM/web:test --skipNxCache` and web typecheck/build targets after confirming names with Nx.
- Formatting and final source gates: `bun x nx run rust-daemon:lint --skipNxCache`, owned Prettier on the exact changed TS files, then Rust/core, SDK and web gates on the final tree. No actual remote server or account is needed; use loopback-only fixture servers with synthetic tokens.

## Top five risks

1. **Credential leakage:** metadata serialization, debug traces, `Authorization` errors or UI state could expose the token. Mitigate with zeroizing/redacted type, redaction tests and strict response projection.
2. **Credential reaches wrong host:** endpoint edits, redirects, origin changes or session deletion could forward it. Bind secret to current endpoint, clear on endpoint change, retain redirect denial, assert destination capture.
3. **Vault mutation races/lost outcome:** cancellation may drop a caller while keyring mutation completes. Reuse owned mutation semantics; treat uncertain write/delete as unavailable and don't connect until verified.
4. **Auth callback ambiguity:** RMCP `auth: Option<String>` is Bearer-token input, and OAuth's client wrapper can refresh/retry. Keep static secret sourced only from the MCP vault; explicitly prove no refresh, duplicate request, challenge-driven auth, or model-controlled influence.
5. **Authority/scope creep:** arbitrary header and OAuth support would enlarge credential policy substantially. Keep one owner-entered bearer on HTTPS only, no secret in connection JSON/tool arguments, and leave stdio unchanged.

**Resolved by binding ruling 3:** the manager retires and joins the current generation before vault mutation, with version CAS and a bounded response deadline. Uncertain retirement refuses before changing stored material; the retained manager task settles independently of a dropped caller.

## Controller rulings on the risks (binding)

1. Ruling: expose only explicit owner PUT/DELETE credential operations with expectedVersion and a secret-free configured boolean; use a bounded zeroizing/redacted opaque token, and clear the password input after every settled response including failure — secret entry belongs to the owner and never durable/browser/model state — cost if wrong: block publication on leakage; failures require deliberate re-entry, never automatic resubmission. Hermes is the pinned behavioral reference for static Bearer forwarding; preserve notices for any literal port and do not import its private env/config.
2. Ruling: bind each credential to the exact saved HTTPS endpoint and a fresh opaque nonsecret credential reference; durable records may contain only that optional reference, with absent/default preserving old unauthenticated rows — endpoint edits or replacement must never resurrect a previously stored credential — cost if wrong: fail closed and leave the connection unavailable. OS-keyring and control-plane writes are not one transaction: prepare new vault material before reference publication, compensate/remove orphaned material where possible, and report uncertain outcomes honestly without rollback/atomicity claims. No bearer byte in snapshots/DTOs/Debug/errors.
3. Ruling: manager-owned versioned mutations retire and join the affected generation before changing auth, and refuse credential mutation if retirement remains uncertain — dropped HTTP callers must not detach writes or permit a stale in-flight dispatch after settlement — cost if wrong: keep quarantine and old material unchanged where known; explicit reconnect/re-entry required, no retry/replay. Existing exact descriptor/grant authority stays separate.
4. Ruling: credential forwarding remains one native static source on HTTPS only, including applicable session requests; retain verified TLS, no proxy/redirect/retry/refresh and exact endpoint binding — adding OAuth or arbitrary headers would widen scope beyond the demonstrated missing feature — cost if wrong: refuse auth before network; HTTP/stdio stay unauthenticated, challenge-driven token exchange and model-chosen headers are unavailable.
5. Ruling: Astra owns only concurrency/security-sensitive native lifecycle/auth implementation, Sol owns SDK/web after interface freeze, and Luna provides source-only test/planning help; one native batch review and one final web/SDK review with one fix wave precede required fresh gates — bounded delegation improves speed without duplicating audits — cost if wrong: no merge or completion claim. Use synthetic tokens/injected stores and local fixtures only; no real keyring/account/remote MCP/profile/provider/daemon/container/database/deploy activation. Dependencies may be added only when needed and recorded; no live-account acceptance or full parity claim.

## Execution and resume

Root owns commands/fmt/Git and serialized Cargo CARGO_INCREMENTAL=0/jobs1/threads2. Source-only implementers, one native batch and final web/SDK review, one fix wave then fresh relevant Nx gates. Per-command unsigned commits and merge-and-continue authorized. Explicit stage paths only; never nx.json/anima.yaml/.superpowers, no stash/reset/restore/path checkout/worktree. No real keyring/credential/profile/account/provider/daemon/device/remote MCP/DB server/container/deployment activation. M64 public browser NOT RUN; M52 PARTIAL and consumed cases unchanged. Resume .superpowers/sdd/2026-10-16-companion-mcp-bearer-m65/progress.md.

## Recorded outcome

Rust/core **2,886 passed / 27 intentional ignores / 45 suites / lint/build**, including **20 focused M65 native tests**; SDK **185 passed / three intentional service skips / typecheck/build**; web **1,497 passed / 121 files / typecheck/build**.

Compiled route inventory: 202 operations. One native batch review and final web/SDK review, one combined pre-gate fix wave, docs/format/hash checks. Bundle removal deletes only exact owned slots after retirement; partial/uncertain deletion preserves references and intent. One dev-only tokio-rustls edge at existing locked version 0.26.4, zero new package versions. No actual OS-vault/account/remote-server activation or full-parity claim. M64 public browser NOT RUN; M52 strict quality PARTIAL. See [harness evidence](../../companion/harness-acceptance.md).
