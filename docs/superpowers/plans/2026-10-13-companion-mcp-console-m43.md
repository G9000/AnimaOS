# M43 owner MCP console implementation plan

Status: implemented, reviewed and locally gated; controlled browser/restart acceptance passed. The binding controller rulings below supersede draft alternatives. Exact publication is tracked in the M43 ledger.

**Goal:** owners can configure the existing anonymous HTTP MCP client, inspect
its current discovered descriptors, review exact tools and independently grant
or revoke one exact alias without overwriting unrelated agent permissions.
**Architecture:** typed SDK facade over existing M42 owner APIs; a small panel in
Connectors; a narrow grant mutation inside the current native CP transaction.
Existing MCP registry, AgentConfig, approval rules and action receipts stay authoritative.
**Stack:** existing Rust daemon/Axum/serde, SDK TypeScript, React/Vitest/Nx.
**Reuse decision:** reuse Anima's current SDK transport, connector card components,
owner request policy, Settings patterns, agent grant configuration and MCP manager.
No new protocol engine, permission store, package manager, approval engine or UI kit.

## 1. Milestone boundary and prerequisites

- Root must verify M42 merge SHA and its actual final gates before cutting M43.
- M42's final schemas/routes can change during its current review; refresh them once.
- HTTP anonymous connections only: owner-selected HTTPS or literal loopback HTTP.
- M42 client already provides discovery, review, invocation, fencing and cleanup.
- The console invokes owner APIs; it never contacts an MCP endpoint directly.
- Initial saved connection stays disabled; no automatic connect/discover/grant.
- Explicit Connect initializes and performs the existing bounded first discovery.
- Explicit Refresh tool list performs discovery; Refresh status is an owner GET.
- Reviewing an exact candidate saves metadata only; grant is a separate owner action.
- A granted tool still needs a ready exact binding and normal Exec approval policy.
- No UI test button that invokes a remote tool in this milestone.
- No stdio, package install/update, authentication headers, OAuth, credentials,
  resource browsing, prompt import, sampling, tasks, subscriptions or fallback modes.
- Those remain PAR-04 backlog; M43 does not claim complete MCP/Hermes parity.
- No provider request is needed to validate the owner workflow.
- Existing profile defaults and helper restrictions remain unchanged.

## 2. Current source seams to reuse

| Seam                         | Current file                                      | Reuse                                                       |
| ---------------------------- | ------------------------------------------------- | ----------------------------------------------------------- |
| Owner MCP wire DTOs          | hosts/rust-daemon/src/routes/contracts/mcp.rs     | Exact names/nullability/schema objects                      |
| Owner MCP operations         | hosts/rust-daemon/src/routes/mcp.rs               | Auth-first, strict JSON, no-store, redacted errors          |
| Reviewed metadata/live state | hosts/rust-daemon/src/mcp/{records,service}.rs    | Existing version, digest and generation fencing             |
| Typed owner transport        | packages/sdk/src/client.ts:45,146                 | DaemonHttpError, same-origin configured fetch, body parsing |
| SDK resource pattern         | packages/sdk/src/learning.ts:75                   | Encoded owner paths, envelopes and AbortSignal reads        |
| SDK registration             | packages/sdk/src/client.ts:72,99; index.ts        | Add one client and exports                                  |
| Web owner transport          | apps/web/src/lib/daemon-api.ts:68,77              | Existing shared headers/client; avoid a second bare fetch   |
| Connector surface            | apps/web/src/components/ConnectorsView.tsx:815    | Agent-keyed card placement                                  |
| Safe async ownership         | components/learning/LearningReviewPanel.tsx:73    | Generation invalidation and mutation lock                   |
| Status/cancel UI precedent   | components/GoogleDriveCard.tsx:12                 | Explicit unavailable/cleanup wording                        |
| Settings grant draft         | components/SettingsPanel.tsx:128,280              | Keep custom/native/dormant grants on unrelated saves        |
| Native grant semantics       | anima-core/src/agent.rs:58                        | Exact configured descriptor-name membership                 |
| None handling                | rust-daemon/src/state.rs:3461; agent_runs.rs:1612 | None resolves to no configured grant list                   |
| Grant epoch invalidation     | rust-daemon/src/state.rs:3492; mcp/service.rs     | Existing alias-change hook and retirement                   |

Paths in the last three rows are under packages/core-rust/crates or hosts as noted.
Numbers are source-read anchors, not promises after M42 formatting/integration.
Existing connector polling helper is not copied blindly: MCP reads/mutations need
the same owner-generation fencing but no interval-triggered connect or discovery.

## 3. Existing M42 wire contract: SDK must match it

Base path: `/api/agents/{agentId}/mcp-connections`.
Encode every agent/connection/alias path segment with encodeURIComponent.

| Method/path suffix      | Body                                             | Result                                |
| ----------------------- | ------------------------------------------------ | ------------------------------------- |
| GET base                | none                                             | 200 `{connections: McpConnection[]}`  |
| POST base               | `{label,endpoint}`                               | 201 `{connection}`; disabled/version1 |
| GET `/{id}`             | none                                             | 200 `{connection}`                    |
| PATCH `/{id}`           | `{expectedVersion,label?,endpoint?,enabled?}`    | 200 `{connection}`                    |
| POST `/{id}/connect`    | `{expectedVersion}`                              | 200 `{connection}`                    |
| POST `/{id}/discover`   | `{expectedVersion}`                              | 200 `{connection}`                    |
| POST `/{id}/review`     | `{expectedVersion,discoveryId,descriptorDigest}` | 200 `{connection}`                    |
| POST `/{id}/disconnect` | `{expectedVersion}`                              | 200 `{connection}`                    |
| DELETE `/{id}`          | `{expectedVersion}`                              | 204, no JSON envelope                 |

- Lifecycle/configuration mutations do not accept an empty body or nullable versions.
- PATCH absent optional keys are preserved; present null is invalid.
- Version is a JS-safe positive integer; runtime wireGeneration is server-owned.
- DiscoveryId is nullable ephemeral UUID, not a durable review record.
- Candidate schemas are JSON objects; outputSchema is object or null.
- Reviewed tool response FLATTENS descriptor fields next to alias.
- Durable native storage nests descriptor; SDK never mirrors that storage shape.
- The wire connection response is flat, not `{record:...}`.
- Unknown body properties fail; keep SDK input types narrow.
- 400 invalid,403 owner auth,404 missing/foreign/helper,409 stale,
  429 busy/cap,503 unavailable/uncertain or incomplete operation.
- A 404 alone cannot prove an old daemon; it may mean the selected agent disappeared.
- Use existing capability metadata where available for feature availability.
- Reads can be aborted; aborting a browser mutation does not undo server settlement.
- SDK never retries CAS conflicts, connect, discovery, review or grant automatically.
- Preserve DaemonHttpError.status/body; UI uses status plus static user-facing copy.

## 4. Proposed SDK types and facade

Create `packages/sdk/src/mcp.ts` and `mcp.spec.ts`.
Add `McpClient` to DaemonClient/client.ts and export its types from index.ts.

`McpConnectionState` = disconnected|connecting|needs_review|ready|degraded|stopping.
`McpToolDescriptor` = {remoteName,description,inputSchema,outputSchema,descriptorDigest}.
`ReviewedMcpTool` = McpToolDescriptor & {alias:string}.
`McpConnection` = {id,agentId,label,endpoint,enabled,version,reviewedTools,state,
wireGeneration,discoveryId,candidates,physicalCallsActive,error}.
`error` = null or {code:string,message:string}; all current native fields required.
Schemas use `Record<string,unknown>`; never expose them as executable validators.
Add `McpCreateInput`, `McpUpdateInput`, `McpVersionInput`, `McpReviewInput`.
Methods: list/get/create/update/connect/discover/review/disconnect/remove.
Owner GET list/get accept `{signal?:AbortSignal}`.
Mutations take the exact frozen body, return a connection; remove returns void.
No caller-provided auth headers/transport flags/protocolVersion/schema digests computed locally.
No request to a supplied endpoint from the SDK client or browser.

## 5. Narrow alias grant operation: root preferred scope

Existing AgentUpdateInput.tools is whole-list replacement without a grant CAS.
A browser read+replace for an instant alias toggle can overwrite a concurrently
changed native/dormant grant. Do not implement instant MCP toggles through that path.
Root selected a narrow server operation as the preferred M43 design; final binding
is still required before implementation. No generic Agent schema/version redesign.

Proposed route:
`PATCH /api/agents/{agentId}/mcp-connections/{connectionId}/grants/{alias}`.
Strict body: `{expectedVersion:number,descriptorDigest:string,granted:boolean}`.
ExpectedVersion refers to CURRENT connection version, not a browser invented grant epoch.
Alias plus full digest must match one exact own durable reviewed descriptor.
Endpoint/remote name/schema/permission risk are never accepted from this body.
Missing/foreign/helper connection fails generically; stale version/digest yields409.
Grant may be saved for a dormant reviewed alias; it cannot make that tool ready.
Unreviewed candidate cannot be granted and no annotation can substitute for review.

Native method proposal:
`McpManager::set_grant(agent:&str,id:&str,alias:&str,input:McpGrantInput)`.
Return `McpGrantOutcome {connection:McpConnectionView,granted:bool,tool_names:Vec<String>}`.
Wire envelope `{connection,granted,toolNames}` provides an acknowledged canonical
projection for UI reconciliation; it does not create a second grant resource/store.
ToolNames is derived from the current full AgentConfig grant list after settlement.
The connection version need not increment for this agent-only grant change.
Effective changes increment existing runtime grantEpoch and invalidate old stamps.
Identical desired state is a checked no-op after owner/version/digest validation.
Last explicitly submitted desired state for the SAME alias wins; do not invent retry semantics.

Under the shared CP transaction:

1. Check ordinary own agent, current connection version and exact reviewed binding.
2. Clone the CURRENT AgentConfig.tools, not an old browser list or run shadow copy.
3. Add only the canonical reviewed descriptor or remove only that exact alias.
4. Preserve every unrelated descriptor byte-for-byte and its existing ordering.
5. Save the candidate through existing CP settlement; publish only after acknowledgment.
6. Recheck authority after I/O; invalidate grant epochs on effective mutation.
7. Return current canonical tool names/connection; no remote call or policy update.

None semantics verified from actual source:

- AgentConfig::allows_tool returns false when tools is None or Some([]).
- State resolver keeps None; run assembly starts from unwrap_or_default().
- Adding to None produces Some([only this reviewed descriptor]); no profile expansion.
- Revoking absent alias from None keeps None; exact no-op does not normalize it to[].
- Removing the last explicit alias leaves Some([]), preserving explicit empty semantics.
- Host role-specific guidance/coordination overlay remains existing behavior,
  not authority to synthesize standard/native MCP grants.

Failed precommit save retains old grants and known metadata.
Durability uncertainty preserves committed candidate and fails closed, with static503;
UI reloads canonical state, never claims rollback or automatically resubmits a grant.
Prepared ordinary snapshots must not overwrite the acknowledged alias change before
publication. Reuse M42 ordered-settlement machinery; add only a narrow grant witness
if necessary, without overriding a later explicit authoritative Settings tools write.
Require a regression for both lost additions and revived revocations.
Do not assume existing deletion-only removed-alias metadata solves grant additions.

## 6. Simple owner workflow

Place `McpConnectionsPanel` as a separate card section in ConnectorsView.
Title: "External tools (MCP)"; concise description: "Review a server's tools before
allowing your agent to use them. Existing action approval rules still apply."
Display connection list with owner label, endpoint as plain text, saved enabled flag,
live status, grant counts and a bounded static error/unknown-cleanup message.
Zero-state add form has only label and server URL; saving stays disabled/disconnected.
Support one connection edit form at a time, not a table of raw protocol controls.
Plain copy explains anonymous HTTP only; do not suggest pasting secrets into URL.
Server rejection remains authoritative; browser validation is convenience only.

Owner progression:

1. Add disabled saved connection; response records version1, never "Connected".
2. Save enabled state explicitly; remains disconnected until owner clicks Connect.
3. Connect performs first bounded discovery; show working state, then returned readiness.
4. Review one candidate's exact name/description/full schema/full digest.
5. Click "Review this tool" using displayed discoveryId/version/digest snapshot.
6. The returned reviewed alias appears with an unchecked permission control.
7. Owner explicitly enables "Allow this agent to use this tool" via narrow grant route.
8. Show permission separately from readiness and existing approval policy.
9. Disconnect leaves saved configuration/reviews/grants for later explicit reconnect.
10. Disable saves enabled=false and retires transport; old effects may have occurred.
11. Remove connection explicitly removes its saved reviewed aliases/grants only.

No combined "connect and trust all", "review and grant all" or built-in auto-enable.
Review card names/descriptions/schema values render React text/pre, never raw HTML.
Do not auto-link/fetch schema URLs, render remote icons, or evaluate server annotations.
Collapsed reviewed cards keep the digest/alias available for inspection/copy.
Show changed candidate separately from older dormant review; never relabel old alias.
Every review click binds the exact displayed candidate snapshot, not the newest array row.

## 7. Truthful status and available controls

| Authoritative state          | Owner wording                       | Meaning/control                                                  |
| ---------------------------- | ----------------------------------- | ---------------------------------------------------------------- |
| enabled=false                | Saved, disabled                     | No automatic connect; owner may enable                           |
| disconnected                 | Disconnected                        | Metadata/grants can remain; explicit Connect                     |
| connecting                   | Connecting / reading tool list      | Owner operation pending, not ready                               |
| needs_review                 | New tools need review               | Exact previously reviewed matches may still be usable            |
| ready                        | Connected; reviewed tools available | Grant/approval still required                                    |
| stopping                     | Disconnecting; cleanup pending      | Never permit a replacement connect while stopping                |
| degraded                     | Connection unavailable              | Show static reason; explicit refresh/status reconciliation       |
| physicalCallsActive=true     | Work/cleanup still active           | Disconnect/revoke can stop local access, not undo remote effects |
| GET failed/stale             | Status unavailable                  | Saved last-known metadata is not current readiness               |
| unknown/uncertain diagnostic | Outcome unknown / inspect state     | No success/rollback/retry claim                                  |

"Granted" never means "Read-only", "Safe", "Approved for every call" or "Executed".
Connection ready is not proof a particular old review is available: use exact
candidate digest membership plus reviewed descriptor matching for row availability.
Known old grant remains visible as "Permission saved; currently unavailable".
503 mutation may have committed; keep edits, refresh state read-only, ask explicit
owner reconfirmation before any new mutation. Read refresh is never a replay.
409 refreshes/reconciles current version but does not resend old intended action.
429/busy shows busy state; no queued browser retry or hidden exponential backoff.
Close/delete confirmation uses existing two-button inline confirmation pattern;
actual destructive action remains one explicit owner request, not a new approval engine.

## 8. UI lifetime, drafts and concurrency

Use an injectable `McpConnectionsApi` backed by daemon-api shared owner client.
List initial metadata once; explicit status Refresh and bounded GET-only refresh
after completed owner mutations. No polling that invokes connect/discover.
If a quiet GET timer is retained from connectors, root must bind its finite interval
and skip hidden/busy panels; it cannot restore readiness from stale cache.
Default recommendation: explicit refresh, no new background timer for first cut.
Mutation lock blocks double submission and serializes connection/review/grant edits.
Increment request generation on mutation, agent switch, unmount and explicit refresh.
Capture agentId, connectionId, version, discoveryId/digest/alias at click time.
Old-agent responses are ignored even if a browser abort does not stop server settlement.
Abort superseded reads; never apply a late older GET over a just-acknowledged mutation.
Clear candidate selection on changed discovery token; preserve owner label/URL edits
on conflict/failure and require explicit reconfirmation against refreshed state.
Do not optimistically show a grant checked or a candidate reviewed before acknowledgment.
After acknowledged grant, merge only returned canonical tool names into current agent
permissions via existing parent reconciliation callback; preserve unrelated name/model/system.
Reload accepted parent agent/config state so an already-open Settings draft reseeds safely.
Global parent settings saving/resetting disables panel writes; helper agent hides/refuses controls.
MCP action failure never starts a model task or installs a skill/fact.

## 9. Profile compatibility

Settings currently displays unknown tool sets as Custom and preserves them on
unrelated scalar edits. Keep that behavior for MCP aliases; do not strip aliases
just to claim the agent matches Observe/Collaborate/Operate.
Current standard profiles contain no MCP alias and helpers inherit none.
Choosing a standard profile remains a deliberate whole-list replacement through
the existing Settings save, which removes MCP aliases from that selected profile.
Give a clear brief notice that choosing a standard profile replaces external grants.
MCP review records remain dormant after that grant reset; no automatic regrant.
Independent grant route preserves all current native/dormant grants even if the
browser last loaded the connection before a concurrent unrelated Settings change.
Owner-requested ordinary whole-list Settings semantics remain existing behavior.

## 10. Disjoint implementation ownership after root release

1. Root native integration: grant DTO/route/manager settlement/epoch/CP acknowledgment.
   Files mcp/{records,service,tests}.rs; routes/{mcp,contracts/mcp,mod}.rs;
   narrow state.rs ordered-persistence hooks if regression demonstrates necessity.
2. SDK worker: sdk/src/{mcp.ts,mcp.spec.ts,client.ts,index.ts}; exact transport only.
3. Web worker: lib/daemon-api.ts/test; new components/mcp/McpConnectionsPanel.tsx/test;
   components/ConnectorsView.tsx/test; parent ViewHarness.tsx/test for reconciliation.
4. Root coordinates SettingsPanel.tsx/test only if profile notice/busy wiring is needed.
5. No worker edits the native transport/core schemas, Cargo, nx.json or other connectors.
6. Root owns all commands, actual acceptance, final review/fix wave and publication.

## 11. Meaningful RED and minimum behavior tests

- [ ] Native first: existing owner router with valid own reviewed binding receives
      the new exact-grant request; expect acknowledged changed sole alias vs current404.
- [ ] Prove original native/dormant grants before asserting new alias mutation.
- [ ] UI first: existing Connectors surface lacks owner MCP workflow; test actual
      create→enable→connect→review→grant clicks and exact request bodies, not import errors.
- [ ] Root observes meaningful assertion failures before releasing production ownership.
- [ ] Add SDK path escaping/all methods/envelopes/nullability and DELETE204 tests.
- [ ] Native CAS: foreign/helper/stale version/changed digest/unreviewed alias refuse.
- [ ] Native grantNone/empty/add/remove/no-op semantics; no profile/default widening.
- [ ] Concurrent native/dormant/second-alias changes survive a narrow alias toggle.
- [ ] Save-before-publication, stale ordinary snapshot, failed save and uncertain-save tests.
- [ ] Explicit revoke invalidates old approval/in-flight stamps; no ABA reuse or new remote call.
- [ ] UI no implicit connect/discover/review/grant on mount/status refresh/agent switch.
- [ ] Create remains disabled; enabled/disconnected and ready/ungranted copy stay distinct.
- [ ] Needs-review still displays usable exact old match plus changed dormant/ungranted row.
- [ ] Safe schema/description text, no fetched URLs/icons or trusted annotation claims.
- [ ] Review uses click-time version/discoveryId/full digest; stale409 retains edits/no resend.
- [ ] Grant request changes one alias, no full AgentUpdateInput.tools write from MCP panel.
- [ ] Disabled/helper/busy/double-click gates; failed grant never optimistic permission.
- [ ] Agent switch, out-of-order reads, reset and unmount cannot alter another owner's panel.
- [ ] Concurrent parent settings fields survive acknowledged permission reconciliation.
- [ ] Disconnect/reload retain reviewed/dormant grant metadata; no automatic reconnect.
- [ ] Removal confirmation targets exact owner/id/version, preserves other cards/grants.

## 12. Root-only commands and bounded acceptance

Root resolves inferred Nx targets once from current workspace metadata; names are
verified packages @animaOS-SWARM/sdk and @animaOS-SWARM/web. Do not guess forwarding flags.
Required commands after concrete changes (not run during this draft):

- `bun x nx run rust-daemon:test --skipNxCache` for narrow native grant mutation.
- `bun x nx run rust-daemon:lint --skipNxCache` after root formatting.
- `bun x nx run @animaOS-SWARM/sdk:test --skipNxCache` with intentional service skips.
- `bun x nx run @animaOS-SWARM/sdk:build --skipNxCache`.
- `bun x nx run @animaOS-SWARM/web:test --skipNxCache` with intentional service skips.
- `bun x nx run @animaOS-SWARM/web:typecheck --skipNxCache`.
- `bun x nx run @animaOS-SWARM/web:build --skipNxCache`.
  Set ANIMA_SKIP_SERVICE_TESTS=1 where current targets use it; document skip counts.
  Rust gates serialize, CARGO_INCREMENTAL=0; reuse current target directory only.
  No new target directory/worktree/dependency/service retry is part of this plan.

One controlled fake UI acceptance, root-owned after gates:

- Isolated synthetic agent/CP/HTTP fake and existing UI, no ordinary profile/accounts.
- Save disabled card, enable, connect, inspect exact candidate, review, grant one alias.
- Independent fake log proves discovery/review/grant made zero tools/call requests.
- Existing deterministic executor may exercise the granted synthetic alias once;
  scripted adapter costs zero provider requests and normal journal witnesses are checked.
- Disconnect and quiet same-store reload retain metadata/grants, show disconnected,
  make zero spontaneous HTTP MCP reconnect/discovery/tool calls.
- Owner explicitly reconnects; unchanged digest can become ready without re-review;
  changed fake descriptor appears unreviewed and gains no new permission.
- Preserve configured native/dormant grant witnesses and unrelated canonical stores.
- Finite controller/process cleanup; no second semantic task or paid probe/retry.
- Report fake/scripted/browser evidence as such, not live third-party service parity.

## 13. Five risk candidates for root binding

| Risk                                                  | Recommended ruling                                                                                                       | Cost/limit                                                                |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| Grant updates overwrite concurrent permissions        | Narrow per-alias owner mutation from current config; exact connection version/alias/digest; no browser whole-list toggle | Small native CP change and Rust gate required                             |
| Saved/reviewed/granted/ready are conflated            | Separate status/permission/review; explicit enable/connect/review/grant; no defaults changed                             | Owner performs deliberate steps; helper/offline refusal stays visible     |
| Acknowledged grant lost or leaked around persistence  | Candidate save-before-publication + bounded existing ordered acknowledgment; fail closed uncertainty                     | Add only narrow witness if required; never second grant store             |
| Old-agent/conflict async completion changes wrong UI  | Shared mutation lock, generation fencing, exact click snapshots, read refresh/no mutation retry                          | Owner reconfirms stale action; unknown commit cannot be labelled rollback |
| Remote schema/copy trusted or expensive scope expands | Plain React text/schema inspection; conservative existing Exec rules; fake zero-provider acceptance                      | Anonymous HTTP only; stdio/packages/auth remain explicit backlog          |

## Controller rulings on the risks (binding)

Ruling: Use the narrow owner PATCH connection/{connectionId}/grants/{alias} with expectedVersion, full descriptorDigest and explicit granted boolean, mutating only that alias from the current AgentConfig — a browser read plus whole-list replacement can overwrite unrelated concurrent grants — cost if wrong: one small native settlement operation and a fresh Rust gate, with an explicit refresh required on conflict. None grants only the added alias; absent revoke keeps None, last removal keeps Some([]).

Ruling: Every effective grant edit increments the existing connection version, while checked no-ops preserve it; retain exact discovery but invalidate dispatch stamps through existing epochs — reuse an existing publication fence instead of inventing a public agent schema version or permission store — cost if wrong: the owner refreshes a stale version before another review/grant action. This overrides the draft's optional unchanged-version proposal.

Ruling: Save grant candidates before publication and extend the existing bounded ordered acknowledgement with per-owner MCP grant projections and their connection-version fences — a newer ordinary snapshot prepared before publication must not erase an acknowledged addition or resurrect a revoke — cost if wrong: races could change tool authority across restart. Before overwriting a stale ordinary catalog, compare its own connection versions to the acknowledged projection; merge only MCP aliases for a genuinely older projection, preserving native grants and intentional current-version owner edits. Retain only current eligible owners/known reviewed aliases; at most four connections and 128 aliases bound the witness. No second durable grant ledger. Precommit failure keeps old state; typed uncertainty publishes candidate fail-closed and retires the affected transport.

Ruling: Show saved/enabled, reviewed, granted and connected status separately, with explicit connect/refresh/review/grant/disconnect/remove actions — metadata and a grant do not prove ready execution — cost if wrong: owners perform several deliberate steps, but unsupported/disconnected/stopping/unknown states stay truthful. No automatic endpoint contact or test-tool button.

Ruling: Fence reads and mutations by current agent/generation and a shared mutation lock, pin click-time version/digest, render remote text/schema as plain data and never retry mutations automatically — stale completion must not affect another agent and remote metadata cannot choose local authority — cost if wrong: the owner reconfirms after conflicts or unavailable/uncertain outcomes. Reuse the SDK transport and connector surface, fake zero-provider acceptance, conservative Exec rules; stdio/packages/auth remain queued.

## Final implementation and verification (2026-10-14)

The final grant input is `{expectedVersion,descriptorDigest,granted}`; response is `{connection,granted,toolNames}`. Native `McpGrantInput`, `McpGrantOutcome` and `McpManager::set_grant` reuse the current store/transaction. The bounded acknowledgement records canonical per-owner MCP descriptors and connection versions; capture snapshot versions before catalog merge, then repair only stale connection aliases. Current-version generic Settings revocation wins. No separate durable schema or grant store was added.

The browser reads actual canonical configured descriptors after a grant/delete and merges only tools into the latest matching agent through `acceptAgentTools`, with the existing collection-mutation fence. Tool names never reconstruct schemas. Permission uncertainty is marked before a permission-affecting request and survives unmount; explicit read-only Refresh reconciles the parent across remount. Busy/unknown state blocks Settings. Dirty editor fields survive conflict; untouched fields follow refreshed canonical metadata and acknowledged saves clear dirtiness. Native, SDK and web implementers owned disjoint files; root owned commands/integration/docs/publication. Settings and the narrow bootstrap helper were included in the web lane.

Meaningful RED was observed before production: missing native exact-grant route returned404 rather than200; Connectors lacked the External tools heading; SDK lacked client.mcp. A sandbox transform-cache failure was separate environment evidence. Nine final native cases and five SDK cases pass. One native batch review was clean; final SDK/web review found remount uncertainty, unmounted grant settlement and stale untouched endpoint fields. One fix wave plus focused regressions addressed them. Subsequent gate-only fixture corrections fixed the existing Saved flash label and an unsupported matcher generic; building SDK fixed stale output after the deliberately dependency-skipped focused command. Runtime behavior and exact assertions were retained.

Root ran existing Nx targets with `bun --no-env-file`, Nx daemon/cloud off, intentional service skips, serialized `CARGO_INCREMENTAL=0` and one Cargo build job. Fresh gates passed: rust-daemon:test including core-rust dependencies, **2,574 passed / nine ignored / 40 suites**; rust-daemon:lint and build. SDK:test **136 passed / three real-service skips**, build/typecheck; five focused cases after the matcher correction. Web:test **1,264 passed / 112 files**, typecheck/build. Sources and compiled README inventory were frozen. Logs and exact command/exit evidence are private in `.superpowers/sdd/2026-10-13-companion-mcp-console-m43/`.

One actual browser workflow used the built console, typed SDK and real Rust owner routes against a local fake MCP server. A fresh synthetic profile contained no accounts, jobs, schedules, sessions or task submissions; embeddings were disabled. A private allowlisted proxy blocked provider/status/auth/account/task routes to avoid ordinary subscription-vault reads. Saving disabled made zero MCP calls. Review/grant preserved native read_file. Disconnect and a quiet stopped-daemon restart restored the exact review/grant disconnected with unchanged RPC counts. Explicit reconnect restored readiness; changed metadata became a new ungranted alias with the old grant dormant. Explicit revoke/removal preserved native permission. Final fake counts were initialize2, tools/list3, tools/call0; runtime provider requests0. All exact owned processes/listeners and the temporary tab were cleaned up. Forced-stop recovery does not prove graceful shutdown. No deterministic executor task was needed because M42 already exercised the dispatcher and M43 changes owner settlement only.

This accepts the scoped anonymous owner workflow. Stdio/process ownership, packages, credentials and selected real servers remain open; M40's task-quality miss and whole Hermes parity remain unclaimed. M44 is only a private read-only draft until verified M43 merge.
