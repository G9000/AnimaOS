# Simple account connections with OAuth

The owner can sign into an HTTP MCP server from its existing connection card. This implements the user's selected capability 2. Prior finished-document changes remain in the same working tree and must be preserved.

## Owner experience

Save an HTTP connection, choose **Sign in**, complete the provider's browser consent, and return to a connected status. The panel polls only an active sign-in attempt and stops polling when the attempt settles or the panel lifetime changes. It never starts sign-in on mount. An optional advanced public client ID supports providers requiring pre-registration; otherwise use advertised dynamic registration. Do not invent a hosted client metadata document.

Show waiting, signed in, failed, and needs-sign-in states. Provide a browser-link fallback, cancel, and sign out. Tokens never reach the browser or chat. Sign out stops the transport and removes local credentials; supported remote revocation is bounded and best effort. Plain disconnect keeps the account credential. Existing static bearer and local-process connections continue to work.

## Architecture and authority

- A reusable `anima-mcp-oauth` Rust crate wraps rmcp 3.6 authorization, using bounded reqwest 0.12 requests. It handles protected-resource and issuer discovery, native public-client registration, PKCE S256, state/issuer checks, resource binding, expiry, refresh rotation, and revocation.
- The daemon owns ephemeral loopback callbacks, attempt expiry/cancellation, vault persistence, credential generation fences, and physical connection lifecycle. Only opaque references enter control-plane records. Restart restores the reference without contacting providers.
- Vault payloads are bounded and chunked where necessary for Windows Credential Manager's 2560-byte UTF-16 item limit. Use immutable credential generations and recoverable cleanup; test capacity-limited storage, large tokens, rotation, restart, signout, partial writes/deletes, and persistence failure preventing dispatch.
- Fetch and persist fresh tokens before HTTP dispatch. Serialize refresh, preserve rotated refresh tokens, never retry an MCP effect after an authentication failure, and prevent stale callbacks/refreshes from reviving signed-out or edited connections.
- OAuth authenticates a connection. Existing Free/Custom/Strict tool policy, reviews, grants, version comparisons, and dispatch fencing remain authoritative.
- Owner routes use existing owner authorization and no-store responses. Callback accepts only the bound one-use expiring state; fixed response bodies contain no codes, state, tokens, or provider errors.
- Native pre-registered clients use a stable `/oauth/mcp/callback` path with an ephemeral loopback port. To prevent authorization-server mix-up with this shared registered path, require RFC 9207 issuer-response support in discovered metadata before consent and exact issuer identification on every success/error callback. Older authorization servers without this support are explicitly outside the supported subset.
- HTTPS is required except original loopback MCP resources. Deny redirects and prevent discovered remote endpoints from reaching local/private addresses. Bound response bytes, time, attempts, and retained state. SDK auth tracing must not emit secrets.

## Owner API

Under `/api/agents/{agent_id}/mcp-connections/{connection_id}`:

- `POST /oauth/start` `{ expectedVersion, clientId? }` → `{ authorizationUrl, attemptId, expiresAt }`.
- `GET /oauth/status` → `{ state, attemptId?, error? }`, states `signed_out | pending | connected | needs_sign_in | failed`.
- `POST /oauth/cancel` `{ expectedVersion, attemptId }` → `{ connection }`.
- `DELETE /oauth` `{ expectedVersion }` → `{ connection }`.

Route shapes may be refined during implementation if necessary for a concrete lifecycle guarantee; synchronize SDK and docs with the final contract.

## Validation and limits

Use synthetic authorization/MCP servers for discovery, PKCE, callback denial/replay, expiry, token refresh/rotation, restart persistence, sign-out/edit races, and no-effect-retry tests. UI tests cover browser link, pending polling, lifecycle cancellation, reconnect, and sign out. Test SDK route encoding, exact bodies, abort signals, and no mutation retries.

Run full `bun run rust:test`, `bun run rust:lint`, SDK tests/build/typecheck/lint, and focused web MCP tests/build/typecheck/lint on the final tree. Do not claim real-provider login has been verified without a completed live consent flow. Existing unrelated web test failures from the prior task remain outside scope.

Protocol reference: [MCP authorization specification, 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization). The MCP wire version remains the existing negotiated version; this work adds authentication, not a protocol migration.
