# M42 bounded reviewed MCP client

> Root-approved execution plan. The binding rulings and exact interfaces below govern implementation.
> Implementation and local verification are tracked in the M42 private ledger. No real MCP connection or paid model request is part of acceptance.
> This does not close PAR-04; stdio, packages, credentials and owner UI remain explicit work.
> For implementation workers: follow the root-approved plan, meaningful RED and disjoint ownership.

**Goal:** one owner-selected MCP connection can expose a small reviewed set of tools
to one ordinary agent, and the actual native dispatcher can call an exact granted
tool once with bounded input, output, lifetime and honest cancellation/revocation.
**Architecture:** reuse Anima descriptor/grant/approval/action-journal boundaries,
small protocol-neutral types under packages, and the official Rust MCP client behind
a daemon-owned transport/connection manager. Discovery publishes review candidates only.
**Proposed first cut:** Streamable HTTP, anonymous controlled endpoints, tools only.
**Status:** implementation and local gates complete on `g9000/companion-mcp-m42`, from verified M41 merge `8b816c9bcb12ecf7234340134697b6b4e4c98bee`.

## 1. Locked backlog and proposed sequencing

- M42 proposes one Streamable HTTP client path, owner APIs, reviewed descriptors,
  exact configured grants, actual dispatcher bridge, bounded lifecycle and fake acceptance.
- Next transport milestone: owned stdio commands, safe environment, bounded framing,
  Windows/POSIX child-tree cleanup, startup/shutdown/restart and no shell interpolation.
- Next packaging milestone: versioned manifest, runtime compatibility, digest/license,
  reproducible install/update/disable/remove and one example through the actual package path.
- Next owner-surface milestone: SDK connection/review/grant types and simple web controls,
  with real state/reasons and effective permissions; no readiness claim from saved metadata.
- Credentialed HTTP/OAuth and authenticated real servers follow concrete owner-selected
  accounts/resources. No personal Hermes config, browser profile or secret migration.
- No resources/prompts/sampling/elicitation/tasks/browser/connector execution in M42.
- No external account, deployment or paid generation is activated by this draft.

## 2. Reuse comparison before choosing an adapter

| Approach                                               | Reuse and cost                                                                                                                   | Proposed verdict                                |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| Official `rmcp` client plus thin host transport/policy | Protocol types, negotiation, request IDs, cancellation and HTTP/SSE machinery; local admission and wire limits still required    | Recommended                                     |
| Port current Hermes Python adapter                     | Mature lifecycle examples, but Python thread/anyio/profile globals and broad extension behavior do not fit native Rust authority | Inspiration only; no copied code                |
| Bespoke JSON-RPC/SSE client                            | Initially few files; inherits negotiation, cancellation races, framing and compatibility maintenance                             | Reject unless SDK cannot meet a proven boundary |

- Public Hermes inspected at `6e7b83a2f1c093e992a8d838ed77ea320fbd8e51`,
  fetched from public GitHub on 2026-10-13; MIT, copyright 2025 Nous Research.
- `tools/mcp_tool.py` separates one long-lived server task and bounded discovery;
  transport/lifecycle modules demonstrate owned process cleanup and scoped registration.
- Its profile adoption, trust defaults/readOnlyHint gating, reconnect/SSE fallback,
  environment resolution, sampling and elicitation are intentionally not imported.
- Local schemas/grants and existing Anima dispatcher authority determine permissions;
  server annotations never become a local risk classification or automatic tool grant.
- If any upstream code is later ported, preserve the exact pin, MIT notice and changes.
  A comparison citation alone does not mean implementation was ported.

## 3. Verified library proposal and compatibility decision

- Proposed exact dependency: `rmcp = "=3.5.1"`, official `modelcontextprotocol/rust-sdk`.
  Verified tag `rmcp-v3.5.1`: Apache-2.0, edition 2024, minimum Rust 1.88.
- Root must verify the installed compiler and resolved lockfile at milestone start;
  this draft did not invoke the compiler or dependency resolver.
- Disable rmcp default features (default includes server/macros/base64).
  Proposed host features: `client`, `transport-streamable-http-client`.
- Prefer the generic HTTP backend trait and existing host reqwest 0.12 for bounded
  JSON/SSE decoding; convenience HTTP uses reqwest 0.13.2 and adds a second major minor line.
- An explicit small backend is adaptation of HTTP I/O, not a second MCP protocol engine.
  Compare its bounded body/event implementation with the convenience adapter before binding.
- If direct SSE types are needed, proposed `sse-stream = "=0.2.4"`, verified tag
  `release-0.2.4`, MIT OR Apache-2.0. It is already rmcp's optional dependency.
- Existing core `jsonschema = 0.18` with default features off and draft202012 enabled,
  `serde_jcs = 0.2`, SHA256 and futures are available; reuse rather than adding a second validator.
- Existing `CapabilityManifest` has canonical schema digests and a 64KiB schema-pair cap.
  Its validator currently defaults unspecified dialect to Draft7; MCP defaults to 2020-12.
- Set/validate the MCP schema dialect explicitly before compiling. Do not change old
  native schema defaults or silently reinterpret old callers as 2020-12.
- Proposed initial wire baseline: explicit legacy initialize with `2025-11-25`,
  tools only, because it matches the planned handshake and limits protocol surface.
- Current stable MCP is `2026-07-28`: discovery replaces legacy initialization;
  HTTP sessions/GET disappear, and cancellation uses the request stream's closure.
  Root may instead select explicit modern discovery, but must bind/test that exact behavior.
- Do not silently mix modern discovery with legacy cancellation/session assumptions.
  Unsupported versions are a visible error; no transport/profile/version fallback loop.
- rmcp's high-level `call_tool` may drive input-required follow-up rounds.
  Use one low-level cancellable request or `call_tool_once`; reject input-required/task results.
- Disable client response caching (`ClientCacheConfig::disabled()`), including stale-on-error.
  Use bounded `list_tools` pages; do not call unbounded `list_all_tools`.
- Disable session reinitialization/replay and SSE reconnection policy for the first cut;
  source defaults allow both. A new owner connect/discover operation starts a new generation.

## 4. Current Anima engine/host seams — avoid invented assumptions

- Core `AgentConfig::allows_tool` checks exact configured descriptor names.
  Runtime rejects unconfigured names before host callbacks and preserves paired errors.
- Daemon `ToolRegistry` stores canonical descriptors and function-pointer handlers;
  register/lookup are name-based. Add one shared MCP handler for all reviewed aliases,
  which resolves the alias through the trusted manager; no closure rewrite is required.
- `ToolRegistry::validate_tools` currently checks registration presence, not schema equality.
  MCP admission/dispatch must compare the exact reviewed binding digest separately.
- `state.rs` canonicalizes agent tool names from the registry; `agent_runs.rs` also
  creates a fresh native registry when assembling run tools. Both must see only published
  reviewed MCP bindings, or settings validation and actual runs will disagree.
- Provider adapters already accept full JSON Schema-shaped `parameters_schema`.
  `anima-model-adapters/src/common.rs` canonicalizes integer schema keywords because
  `DataValue::Number` is f64. Reject unsafe numeric schema bounds rather than rounding them.
- Daemon risk classification is a static name table; unknown names default to Exec.
  Exec defaults Ask; configured owner policies/rules can explicitly Allow or Deny.
- Read has no ApprovalPolicy field and Read tools bypass prompts. Do not invent Read Ask.
  Proposal: all MCP aliases retain conservative Exec classification in M42.
- Existing `ToolExecutionContext::execute_tool` performs grant/live checks, approval,
  durable action intent, revalidation, handler dispatch and outcome persistence.
- Its action journal persists receipts for read and write tools alike when CP is durable;
  `durability: persistent` describes the witness, not verified remote effects.
- Core `CapabilityExecutor`/Registry have fencing, lineage/recovery and schema validation,
  but no daemon MCP wiring currently consumes them. Forcing that full registry now adds
  a parallel recovery ledger; prefer reusable schema/bound helpers and the existing native journal.
- Existing shell/background-process managers are not an owned MCP child-tree manager.
  Their environment/capture limits are useful precedents, not proof stdio is already supported.
- Telegram's credential vault interface is typed to Telegram tokens, not a generic MCP vault.
  M42 accepts no credentials/headers; a later credential adapter needs its own scoped references.

## 5. Proposed frozen neutral interfaces (root must bind exact names)

- Add a small `packages/core-rust/crates/anima-core/src/external_tools.rs` module,
  or a similarly small package module; no HTTP, rmcp, OS process or database dependency.
- `ReviewedExternalTool`: local alias, original remote name, reviewed descriptor digest,
  canonical input/output schemas and display description; no endpoint/credential values.
- `ExternalToolCall`: exact alias/digest, bounded JSON arguments, runtime-derived
  owner/agent/session/run/call references, deadline and portable cancellation signal.
- `ExternalToolReply`: bounded text/structured data, error flag and recorded provenance;
  timeout/unknown outcome is distinct from remote failure and successful completion.
- `ExternalToolPort`: one async invoke, plus no default retry or model callback.
  No discovery/configuration permission is available through a model tool.
- Reuse `CapabilityManifest` schema canonicalization/bounds where useful via narrow helpers;
  do not expose private validator internals or duplicate JCS algorithms across hosts.
- Host `McpManager` owns configurations, live connections, generations and physical permits.
  A `PublishedMcpBindings` handle contains immutable reviewed metadata only.
- Capture published binding/generation before await; immediately before RPC verify current
  owner, agent, exact grant, enabled connection, unchanged config and reviewed descriptor.
- Admission waits and approval cannot preserve authority across revision/revocation.
  Late results cannot publish after generation, grant or source run validity changes.
- Alias proposal: `mcp_<connection UUID without dashes>_<binding digest prefix>`;
  <=64 ASCII bytes, separate original-name field, reject alias collisions explicitly.
- Full digest covers remote name, schemas, description and locally selected permission;
  transport configuration has a separate generation. A schema/name/description change
  creates an ungranted alias/review candidate rather than inheriting the old grant.
- Root should prefer one agent per connection in the first cut; sharing requires explicit
  per-agent grants and must not inherit another agent's published descriptors.

## 6. Owner configuration/review and durability

- Default off, no built-in endpoints or MCP tools in profiles, helpers or task modes.
- Owner APIs proposed under `/api/agents/{agentId}/mcp-connections` and exact connection ID;
  authenticated GETs use no-store; mutations use the existing strict owner-write check.
- Create/update request: owner-selected label/endpoint, enabled flag, expectedVersion;
  reject unknown fields, userinfo/fragments/secrets in URL, arbitrary headers and auth payloads.
- HTTPS is normal; explicitly selected loopback HTTP supports controlled local fake acceptance.
  Root binds any private-network policy; discovery must never scan or select other destinations.
- Persist configuration version before publishing activation; failed save keeps prior published state.
  Constructor/save-only calls cannot connect. Explicit owner connect authorizes protocol I/O only.
- Connect/discover publishes bounded untrusted candidates with digest and supported/unsupported reason.
  Review exact candidate digest/version separately; review does not grant it to an agent.
- Agent's existing configured tool selection explicitly grants a published reviewed alias.
  Forged/stale descriptor bytes are rejected or canonicalized to the exact reviewed descriptor.
- Delete/disable/revoke is idempotent; save a tombstone/version before retiring publication,
  cancel active calls, and prevent reconnect. Dropped HTTP callers cannot strand a staged config.
- A save failure reports the existing durability uncertainty contract accurately; no false rollback claim.
- CP snapshot bump and backup/restore include config/review rows, never live sessions or secrets.
  Old snapshots default to no connections. Restore keeps reviews but starts disconnected,
  and requires explicit connect plus current exact discovery before advertising/calling tools.
- Health states distinguish off/disconnected/connecting/needs_review/ready/degraded/stopping;
  include bounded static reason, last protocol success and physicalCallsActive.
- Ready means an exact current reviewed set is reachable; a saved config/list alone is not readiness.

## 7. Hard bounds and lifecycle rules (proposed numbers)

- At most 4 connections/workspace and 32 reviewed tools/connection; first acceptance uses one.
- Discovery: one total 5s deadline, at most 4 pages/64 candidates and 256KiB aggregate wire;
  repeated/cyclic cursors or unfinished pagination => incomplete/unavailable, never full success.
- Per descriptor <=16KiB; schema pair <=16KiB for MCP despite core's larger generic limit;
  depth<=16, nodes<=1024, bounded property/name/description/enums and explicit dialect.
- Allow bounded acyclic local $defs references only if proven; reject remote/file refs,
  recursion and unsupported vocabulary before compiling. No schema-induced network/file reads.
- Arguments: object, <=16KiB serialized, depth<=16/nodes<=1024, schema validated before RPC.
- Wire JSON/SSE event <=64KiB before parsing; per call aggregate <=256KiB, including
  progress/comments. Chunked responses and endless small progress cannot evade the cap.
- Model-facing result <=16KiB serialized; <=16 text blocks, explicit shortening/provenance.
  Output-schema violation and malformed envelopes are errors, not successful empty text.
- Text/structured content only; unsupported image/audio/resource links stay labelled omitted.
  No implicit URL/resource/attachment fetch and no server text reinjected as system guidance.
- Call deadline 20s absolute including queue, transport and result validation; no progress reset.
  Discovery/close have separate bounded deadlines. No retries, polling or auxiliary model calls.
- Physical limit: one active call/connection, two workspace-wide, queue at most four total.
  One shared manager across coordinator clones; cleanup retains its permit until transport exits.
- Stop/revocation/disconnect close/cancel exact RPC, suppress late publication and settle a
  paired honest tool result. Remote effects may still have happened; no automatic resend.
- For 2025-11-25, send cancellation notification and close abandoned stream/connection.
  For 2026-07-28 HTTP, closing its response stream is the protocol cancellation signal.
- Disable redirects and environment proxies for the selected endpoint; never carry auth
  to a different origin. Preserve TLS verification; raw transport errors are not owner output.
- Refuse unsolicited server sampling/elicitation/roots callbacks and task/MRTR follow-ups.
  Health/discovery never calls tools. Disconnect doesn't imply a remote effect was undone.
- Publish/config locks are leaves and never held over network/CP awaits; validation stages
  data, takes the existing transaction, saves, then publishes one complete generation.

## 8. Disjoint source ownership proposal

| Worker                         | Owned source                                                                                                                   | Responsibility                                                        |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| Neutral/native protocol worker | core external_tools + bounded schema helpers/tests; host mcp/transport.rs                                                      | Neutral input/reply contracts; SDK transport adaptation and wire caps |
| Host lifecycle worker          | host mcp/{mod,records,service,tests}.rs                                                                                        | Config/review/CAS, generations, physical slots, cancel/health/restore |
| Route/test worker              | routes/mcp.rs + routes/tests/mcp.rs                                                                                            | Strict owner endpoints and actual-router persistence/rollback cases   |
| Root                           | Cargo manifests/lock, state/CP/backup, tools/context/registry, approvals/policy, agent_runs/helper/shutdown, registration/docs | Integration and binding authority                                     |

- Root allocates shared core helpers to one worker only and freezes public types before consumers.
- Thin shared handler `tools/mcp.rs` may be delegated after its dispatch contract freezes.
- SDK/web changes are a later step unless root explicitly binds a minimal first surface;
  an absent UI is documented, not rendered as a working connection toggle.
- No Nx/project reorganization, apps/server removal, core HTTP dependency or native plugin loading.

## 9. Meaningful RED and bounded implementation sequence

- [ ] Root approves transport/version/classification/persistence rulings after M41 publication.
- [ ] Write actual-native-dispatch RED: reviewed controlled fake's `echo_record` descriptor,
      exact agent grant, deterministic scripted tool call; expect paired checked result and
      exactly one fake tools/call. Existing unknown registration/absent route must fail concretely.
- [ ] Observe real assertion failure before production changes; no import-only RED or weakened oracle.
- [ ] Freeze neutral contracts and owner wires; implement transport + service in disjoint files.
- [ ] Integrate current registry canonicalization, descriptor/grant checks and native action witness;
      preserve all native defaults, helper restrictions, provider schema serialization and Stop.
- [ ] Add bounded grouped regressions below, then one combined native review and one fix wave.
- [ ] Root runs required current-tree checks and controlled fake acceptance once evidence is stable.
- [ ] Record exact source/lock/version, outputs, limits, follow-ups and resume path in M42 progress.

## 10. Minimum meaningful regression inventory

1. Handshake/list/call over actual HTTP JSON and request SSE; malformed/unsupported negotiation,
   list pagination exhaustion/duplicate remote names/alias collision and no calls from discovery.
2. Schema depth/bytes/nodes/unsafe numbers/remote refs; arguments fail before RPC; output/schema
   errors/huge JSON/huge SSE/endless progress/unsupported blocks do not become accepted success.
3. Exact own strict owner requests, unknown keys, foreign agent/connection, CAS, save failure,
   dropped caller and old snapshot restore; no endpoint/secret leakage in output/logged errors.
4. Ungranted/stale alias denial, configured Exec Deny/Ask/Allow through the existing actual gate,
   schema revision during approval, and no helper inheritance or new grants after refresh.
5. Held call + revoke/Stop/timeout/disconnect + late fake success: one RPC, honest paired outcome,
   permit held until physical close, second connection cap respected, no replay/reconnect.
6. Fake input-required/task/unsolicited sampling requests never start another model or tool call;
   unavailable discovery cannot re-advertise an SDK stale-cached reviewed set.
7. Fresh reload of config/review keeps disconnected state; exact rediscovery re-enables only
   identical reviewed bindings. Changed descriptor stays needs_review with existing grants untouched.
8. One actual executor task reads the declared synthetic record and quotes the checked value;
   server call log, run action receipt and canonical memory/skill/config neighbors agree.

## 11. Root-only command/acceptance plan — not executed in this draft

- Required source gate: `bun x nx run rust-daemon:test --skipNxCache`.
- Set `CARGO_INCREMENTAL=0` for every Cargo-backed gate; serialize each to actual exit.
  On Windows executable lock use `CI=1` and `CARGO_TARGET_DIR=target/validation-rust-daemon`.
- Required format check: `bun x nx run rust-daemon:lint --skipNxCache`.
- Root verifies current toolchain/dependency graph/license metadata and captures resolved lockfile;
  refresh SDK pin if necessary before changing source rather than guessing older APIs.
- Focused RED/regressions use root-chosen supported target/filter syntax; this draft does not
  assert unverified Nx forwarding flags. Required full target includes core-rust dependency tests.
- If SDK/web enters scope, root first resolves their current Nx targets; use
  `ANIMA_SKIP_SERVICE_TESTS=1`, focused behavior gates and actual Settings/Chat call-site checks.
- Controlled fake binds isolated loopback with synthetic payloads; root owns port/process startup,
  shutdown and cleanup. No ordinary daemon/profile, external account, container or remote call.
- Prefer scripted actual executor + independent fake log for M42; optional paid semantic task
  needs a separately declared finite request/time/cost ceiling and root authority.
- No feature completion claim until selected tests, required gate and fake acceptance pass.

## 12. Top five binding risks and remaining root choices

| Risk                                        | Proposed ruling / cost if wrong                                                                                                                                 |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Transport/version breadth                   | HTTP first, one explicit protocol baseline; broad compatibility and owned stdio later. Wrong choice adds migration work, not silent fallback                    |
| Descriptor/grant/approval split             | Immutable reviewed digest and generation, alias-specific exact grant, conservative Exec class. Refuse drift rather than reuse another descriptor's approval     |
| SDK hidden work and unbounded wire          | No stale cache, MRTR, list-all, reinit/replay or implicit fetch; host raw-byte caps. Wrong adapter can inflate work or accept stale permissions                 |
| Cancellation versus physical/remote effects | Local deadline suppresses publication; keep permit through cleanup and never retry unknown RPC. No claim cancellation rolled back the server                    |
| Durable publication and later packaging     | Save-before-publish/CAS, restore disconnected, neutral ports + host lifecycle. Wrong ordering can expose staged config; full package/UI completion remains open |

- Root still selects HTTP-first versus stdio-first/both, legacy initialize versus modern discovery,
  one-agent connection ownership, initial numeric caps, and the generic HTTP backend cost tradeoff.
- Root still selects whether M42 needs a minimal owner UI or may ship reviewed HTTP API first.
- Root must bind result redaction, existing receipt semantics and disable durability uncertainty;
  do not create a second generic recovery engine just to connect an MCP server.

## 13. Primary source evidence

- Locked Anima parity roadmap.
- [Official SDK tagged manifest: version/license/MSRV](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/Cargo.toml).
- [Tagged feature/dependency manifest](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/Cargo.toml).
- [SSE dependency tagged version/license](https://github.com/4t145/sse-stream/blob/release-0.2.4/Cargo.toml).
- [Tagged client lifecycle and call/list helpers](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/service/client.rs).
- [Tagged stale-cache defaults and disabled configuration](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/service/client/cache.rs).
- [Tagged HTTP limits/reinit/cancel machinery](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/transport/streamable_http_client.rs).
- [Tagged child transport cleanup](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/transport/child_process.rs).
- [Current HTTP protocol](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http) and [cancellation](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/cancellation).
- [Legacy HTTP/stdio baseline](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) and [current tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools).
- [Public Hermes adapter pin](https://github.com/NousResearch/hermes-agent/blob/6e7b83a2f1c093e992a8d838ed77ea320fbd8e51/tools/mcp_tool.py),
  [transport](https://github.com/NousResearch/hermes-agent/blob/6e7b83a2f1c093e992a8d838ed77ea320fbd8e51/tools/mcp_tool_transport.py),
  [lifecycle](https://github.com/NousResearch/hermes-agent/blob/6e7b83a2f1c093e992a8d838ed77ea320fbd8e51/tools/mcp_tool_lifecycle.py),
  [MIT notice](https://github.com/NousResearch/hermes-agent/blob/6e7b83a2f1c093e992a8d838ed77ea320fbd8e51/LICENSE).

## Controller rulings on the risks (binding)

Ruling: Streamable HTTP first, explicitly fixed 2025-11-25 protocol, tools only, one existing agent per owner-selected anonymous connection — this baseline supports the mature initialization path without adopting the distinct 2026 discovery/MRTR semantics halfway through implementation — cost if wrong: newer-only servers require the separate protocol compatibility wave; never silently downgrade/retry. Stdio, credentials and owner UI/package workflows remain explicit follow-ups. Installed rustc1.93.0 satisfies the official pinned SDK1.88 minimum.

Ruling: Reuse official rmcp3.5.1 (Apache-2.0), client/generic HTTP transport with defaults off, adapting existing reqwest0.12 and pinned sse-stream0.2.4 when needed — SDK owns protocol mechanics while a narrow bounded backend preserves raw-byte control and avoids a second reqwest line — cost if wrong: prove a concrete SDK incompatibility before changing the adapter/pin; no bespoke second protocol engine or silent convenience defaults. No copied Hermes code is currently required; retain upstream source comparisons/pins and notices if a later substantive port occurs.

Ruling: Full immutable descriptor digest plus exact alias grant, conservative existing Exec policy, no helper inheritance or authority from server annotations — remote tool metadata cannot choose trusted classification or transplant approval across descriptor revisions — cost if wrong: owners must explicitly allow low-risk tools through existing policy until a later reviewed local classification design. Discovery/review/configuration never call a tool or grant it.

Ruling: One physical call per connection/two globally, finite queue/deadline/wire/schema/result budgets, cancelled RPCs retire before their slot is reusable, no automatic retries/reconnect/reinit/list-all/stale cache/MRTR — returned timeout or Stop does not prove a remote effect was undone — cost if wrong: unavailable or incomplete outcomes remain visible and may need owner reconciliation. Controlled fake acceptance adds zero model requests.

Ruling: Existing control-plane settlement saves before shared publication; restore durable reviews and exact grants but remain disconnected until explicit connect and exact rediscovery; no second recovery ledger — reuse existing action witnesses without turning durable receipts into verified remote effects — cost if wrong: host integration must retain known own reviewed descriptor metadata for restore without advertising ready tools or stripping saved grants. Dynamic registry/config canonicalization and actual dispatch must use the same trusted binding source.

## Active scope and next concrete work

M41/PR47 merged at `8b816c9bcb12ecf7234340134697b6b4e4c98bee`; `g9000/companion-mcp-m42` starts from that verified main. Root froze the interfaces below and observed an actual-router RED (404 instead of 201) before releasing three disjoint workers. Dependency pins resolved; integration and review fixes are in progress. Root owns Cargo/Nx/Git, cross-file integration, gates and publication. No paid models or real MCP endpoints are used. The private M42 ledger records current results and resume.

## Frozen consumer interfaces and owner wires

The exact contracts below supersede the earlier draft sketches. Root observed the actual new in-process owner-route test fail at404expected201 (actual exit1, no missing imports) before production. Use connected/disconnected metadata views separately: native restore rejects unknown names, so validated own known metadata must exist before both validation phases; ready publication and dispatch remain separate.

# M42 exact neutral and transport contract proposal

Status: frozen by root before production. Historical contract preparation had no gates,
dependency resolution, source edits, services, packages or server/model calls.
Public M42 root rulings govern: anonymous Streamable HTTP, protocol2025-11-25,
rmcp=3.5.1/defaults off, existing reqwest0.12, one agent per connection.

## Ownership

- Neutral worker: anima-core/src/external_tools.rs and contained tests only.
- Transport worker: rust-daemon/src/mcp/transport.rs and contained tests only.
- Root: lib/module exports, dependencies/lock, shared integration and notices.
- Manager worker: records/CAS/publication/restore, aliases, permits, authority.
- No new portable invocation/recovery framework or CapabilityRegistry instance.

## Public portable core names (no HTTP/rmcp/Tokio types)

`ExternalToolDescriptor` contains remote_name:String, description:String,
input_schema:Value, output_schema:Option<Value>, descriptor_digest:String.
It is a validated candidate, never proof of owner review or a grant.
Host alias/connection/generation belong to manager records, not this value.

`ExternalToolResult` contains text_blocks:Vec<String>, structured_data:Option<DataValue>,
is_error:bool, omitted_blocks:u32, shortened:bool. All emitted data is evidence.
`ExternalToolError` static variants: DescriptorBounds, SchemaRejected,
ArgumentBounds, ArgumentSchema, OutputSchema, ResultBounds, UnsupportedResult.
Display strings never interpolate endpoints, schema paths, args or server errors.

Public functions:

- `prepare_external_tool(remote_name:&str,description:&str,input:Value,output:Option<Value>)
->Result<ExternalToolDescriptor,ExternalToolError>`.
- `validate_external_arguments(tool:&ExternalToolDescriptor,args:&Value)
->Result<(),ExternalToolError>`.
- `validate_external_output(tool:&ExternalToolDescriptor,output:&Value)
->Result<(),ExternalToolError>`; no output schema means bounds still apply.
- `external_descriptor_digest(name:&str,description:&str,input:&Value,output:Option<&Value>)
->Result<String,ExternalToolError>`; caller supplies normalized validated schemas.
- `bound_external_result(result:ExternalToolResult,reserved_envelope_bytes:usize)
->Result<ExternalToolResult,ExternalToolError>`; final manager wrapper remains<=16KiB.

Bounds are constants, not model-controlled knobs. Descriptor serialized JSON<=16KiB;
schema pair<=16KiB including dialect insertion; maximum depth16 and nodes1024.
Name nonblank/control-free<=128bytes; description<=2048bytes, deterministic refusal,
not silent clipping of digest-covered metadata. Input root must be object schema;
output schema when present object schema. Boolean subschemas remain allowed.
Missing $schema is normalized to https://json-schema.org/draft/2020-12/schema;
any other explicit dialect is rejected. Compile explicitly as Draft202012.
Walk before compilation: reject external/file refs, unresolved/cyclic local refs,
$dynamicRef/$recursiveRef, unsupported $vocabulary and unsafe/nonfinite numbers.
Permit only acyclic #/$defs/... local JSON-pointer refs; count referenced traversal
within the same budgets. No resolver is allowed to perform network/file reads.
Instance JSON is object,<=16KiB/depth16/nodes1024 for arguments; output structural
validation applies to complete received structured data before any shortening.
Use serde_json Value until strict finite/safe-number conversion to portable DataValue;
never silently round large integer constraints or structured result integers.
Digest = sha256:<64 lowercase hex>, JCS of exact remote name/description/normalized
input/output (null absent) plus classification:"exec". Aliases derive in manager.
Reuse existing serde_jcs/SHA256/jsonschema; CapabilityManifest's private helpers and
Draft7 default cannot be called directly or changed globally for this milestone.
Cheapest extraction is shared private schema compilation with explicit dialect;
do not create a synthetic manifest requiring irrelevant compatibility/recovery fields.

## Host interface consumed by manager (pub(crate), object-safe)

`TransportControl {deadline:tokio::time::Instant,cancel:CancellationToken,wire_budget:Arc<WireBudget>}`.
`TransportControl::new(deadline,cancel)` creates the private atomic256KiB budget;
clones share it across connect+handshake+all discovery pages (new budget per call).
`ValidatedMcpEndpoint` is manager-validated anonymous endpoint String newtype;
transport still uses redirect-disabled reqwest client and never adds auth/custom headers.
`McpCall {remote_name:String,arguments:Value,descriptor:ExternalToolDescriptor}`.
`McpDiscovery {protocol_version:String,tools:Vec<ExternalToolDescriptor>}`.
`PhysicalClose` is a private-field unit witness created only by joined cleanup.
`McpTransportError {Unavailable,UnsupportedProtocol,WireLimit,Malformed,
SchemaRejected,RemoteError,UnsupportedResult,Cancelled,TimedOut,OutcomeUnknown}`.
All diagnostics are static. Remote error data/bodies are never owner diagnostics.

Use existing futures::future::BoxFuture; no new async framework required:

- `McpTransportFactory:Send+Sync`:
  `connect(&self,endpoint:ValidatedMcpEndpoint,control:TransportControl)
->BoxFuture<'static,Result<Arc<dyn McpTransportSession>,McpTransportError>>`.
- `McpTransportSession:Send+Sync`:
  `discover(&self,control:TransportControl)
->BoxFuture<'static,Result<McpDiscovery,McpTransportError>>`;
  `call_once(&self,call:McpCall,control:TransportControl)
->BoxFuture<'static,Result<ExternalToolResult,McpTransportError>>`;
  `close(&self)->BoxFuture<'static,Result<PhysicalClose,McpTransportError>>`.
  Methods own cloned task state. No locks held across manager CP snapshots/HTTP I/O.
  Discovery deadline5s covers connect+handshake+<=4 tools/list pages/64 candidates.
  Reject repeated cursor/duplicate name/extra candidates; no silent partial publication.
  Call deadline20s absolute includes queue/dispatch/validation; progress cannot extend it.
  Manager acquires one per connection/two global permits, queue<=4, validates reviewed
  digest/live exact grant/generation immediately before call; transport cannot grant.
  Already-cancelled/expired control sends no RPC. Each admitted call sends one tools/call.

## SDK adaptation and raw limits

Implement tagged StreamableHttpClient using reqwest0.12, not convenience backend.
Its post_message returns SDK StreamableHttpPostResponse::{Accepted,Json,Sse}; SDK
still owns JSON-RPC correlation/lifecycle. Set reinit_on_expired_session(false),
max_concurrent_requests=1, max_sse_event_size=65536, retry_config to a policy whose
retry always returns None. Override raw POST SSE parsing; decoded event caps alone
do not bound accumulating wire. Backend GET returns ServerDoesNotSupportSse without
network I/O (no standalone subscriptions/resumption); DELETE may perform one bounded
session cleanup request. 404 is terminal; no reconnect/fallback/replay.
Each operation tracks all raw JSON/SSE bytes<=256KiB and one raw event<=64KiB,
including comments/progress/ignored events before parsing; chunk crossing cap aborts.
Bound headers/session IDs/cursors too; do not trust Content-Length alone.
Pin ClientLifecycleMode::Initialize and offered protocol2025-11-25; require exact
negotiated version before publishing connection. No Auto/discover protocol probing.
Set peer response cache disabled before list; paginate list_tools, never list_all_tools.
Use peer.send_cancellable_request(ClientRequest::CallToolRequest(...),options),
timeout plus max_total_timeout and reset_timeout_on_progress=false; match only final
CallToolResult. call_tool_once is available but low-level handle supplies cancellation.
Reject InputRequired/Task/unsupported structured envelopes; never MRTR/polling.
Client advertises no sampling/elicitation/roots/subscriptions; static unsupported replies.
Map text blocks<=16; omit image/audio/resource/link payloads without fetching and count
omissions. Validate full structured content before conversion/shortening. Preserve
remote isError as error, never success with empty text after schema/parse failure.
Manager builds evidence-only final envelope with connection ID/generation/alias/digest,
known-vs-unknown outcome and omissions; reserve its bytes before UTF8 shortening.

## Physical completion and cancellation

Owned operation stays alive after HTTP caller drops; cancel stops publication and
initiates request cancellation/connection retirement. Once dispatch may have begun,
timeout/Stop/transport loss can mean OutcomeUnknown, never remote effect rollback.
close awaits RunningService::close plus all backend HTTP/SSE workers; no permit release
on signalling a token or SDK close_with_timeout(None). Timed cleanup retains permit
and quarantines connection until detached cleanup actually joins. No reconnect/reuse.
Normal completed call proves its request stream settled; cancellation retires session.

## Tagged primary sources and concrete adaptation risks

- [HTTP backend/config](https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.5.1/crates/rmcp/src/transport/streamable_http_client.rs): trait391; config1969; reinit2084. Custom raw bounds and no-network GET required.
- [Client](https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.5.1/crates/rmcp/src/service/client.rs): Initialize603; call_tool_once1836. Avoid high-level MRTR.
- [Service](https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.5.1/crates/rmcp/src/service.rs): cancellable request800; close1054; close_with_timeout1073 detaches on timeout. Cleanup witness required.
- [Cache](https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.5.1/crates/rmcp/src/service/client/cache.rs): disabled65.
- [Retry policy](https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.5.1/crates/rmcp/src/transport/common/client_side_sse.rs): SseRetryPolicy157; return None.

No proven pin incompatibility found. Contract tests must prove exact handshake/version,
JSON/request-SSE limits, single call/no replay on404, disabled cache/reconnect,
InputRequired refusal, physical slot retirement and manager late-publication suppression.
Top risks: raw limits before decode; cleanup witness versus timeout; schema resolver
side effects; finite portable number fidelity; digest/generation publication agreement.

# M42 exact manager/service contract

Status: frozen source contract. The binding controller rulings govern implementation.
Fixed 2025-11-25 anonymous Streamable HTTP; default off; one live ordinary agent/connection.
Maximum four workspace connections/32 reviewed tools each; no helper, credential or package API.
Transport/core names below match `core-transport-contract.md`; no second recovery ledger.

## Durable records and runtime state

- `McpConnectionRecord {id:String,agent_id:String,label:String,endpoint:String,
enabled:bool,version:u64,reviewed_tools:Vec<ReviewedMcpTool>}`; serde camelCase/strict input.
- `ReviewedMcpTool {alias:String,descriptor:ExternalToolDescriptor}` stores the full validated
  core name/description/input/output schemas/digest; public reviewedTools flattens descriptor fields.
- Create `{label,endpoint}` produces enabled=false/version=1/empty reviews; save never connects.
- `McpRegistry {connections:BTreeMap<String,McpConnectionRecord>}` owns durable rows only.
  Snapshot field `mcp_connections` defaults empty for pre-v17; never serialize live session/candidates.
- `McpConnectionState`: disconnected|connecting|needs_review|ready|degraded|stopping; off
  is derived from enabled=false, while new disabled response state remains disconnected per RED.
- `McpConnectionView` flattens durable record plus state, wireGeneration:u64, discoveryId:Option<String>,
  candidates:Vec<ExternalToolDescriptor>, physicalCallsActive:bool, error:Option<StaticMcpError>.
- `McpLiveConnection` owns optional Arc<dyn McpTransportSession>, epoch cancellation,
  wireGeneration, complete discovery/candidates, active request/cleanup state; never saved.
- `SharedMcpPublication = Arc<std::sync::RwLock<McpPublishedCatalog>>`; clones alias this same handle.
  Catalog has separate `known` own reviewed descriptors and `live` verified dispatch bindings.
- `McpDispatchStamp {agent_id,connection_id,alias,descriptor_digest,connection_version,
wire_generation,grant_epoch,run_id,session_id,tool_call_id}` is host-derived, never model args.
- Alias = `mcp_<UUID without dashes>_<first16 descriptor digest hex>`; <=64 ASCII bytes.
  Compare full `sha256:<64 lowercase hex>` digest; reject every alias collision, never overwrite.

## Version, generation and discovery CAS

- Durable version starts 1; checked increment for effective owner update/review/removal only.
  Identical mutation is a no-op after expectedVersion checks; stale expectedVersion always409.
- `wireGeneration` is a manager-wide monotonic runtime u64 token, never restored or reused;
  new connect/retirement/config change/disable/review replacement invalidates prior dispatch stamps.
- Complete discovery gets a fresh opaque UUID `discoveryId`, bound to connection/version/generation.
  Failed/partial/old-generation results publish no candidates or ready bindings; no stale cache.
- Review input `{expectedVersion,discoveryId,descriptorDigest}` identifies one exact current candidate.
  Missing/old discovery or digest conflict409; candidate count/review count bounds apply before save.
- The 32 cap counts all durable reviews, including dormant aliases; never evict a granted old review
  to admit a 33rd. Different digest creates a new ungranted alias; identical review is a checked no-op.
- A review persists the exact descriptor and deterministic alias, increments version, then publishes.
  It neither edits AgentConfig.tools nor calls a remote tool. Retain other own reviewed metadata.
- Review/label-only save carries unchanged complete discovery/token forward to the new version;
  endpoint/enabled changes or retirement invalidate it. Reviewing several tools needs no rediscovery.
- Rediscovery enables only exact reviewed digest matches; changed or absent tool remains dormant.
  Never rewrite a saved grant/alias to another descriptor, nor mark an unmatched review ready.
- Endpoint edits retire transport and clear live discovery; keep old reviewed descriptors dormant.
  Each successful new discovery is authoritative only for its own version/generation; no rollback.
- Grant epochs are runtime-only per agent; root invalidates active stamps whenever MCP alias grants
  change, including remove/re-add, agent deletion/helper conversion. Fingerprints alone miss ABA.

## Exact manager methods (pub(crate), clones share lifecycle/slots)

- `McpManager::new(state:Weak<RwLock<DaemonState>>,transactions:Arc<tokio::sync::Mutex<()>>,
publication:SharedMcpPublication,factory:Arc<dyn McpTransportFactory>)->Self` does no I/O.
- `list(agent:&str)->Result<Vec<McpConnectionView>,McpServiceError>`;
  `get(agent:&str,id:&str)->Result<McpConnectionView,McpServiceError>`; no transport dispatch.
- `create(agent:&str,input:CreateMcpConnection)->Result<McpConnectionView,McpServiceError>`;
  create input label/endpoint, fixed disabled/version1. Effective rows scoped to live own agents.
- `update(agent:&str,id:&str,input:UpdateMcpConnection)->Result<McpConnectionView,McpServiceError>`;
  input expectedVersion and optional label/endpoint/enabled; reject empty/unknown-field patch.
- `connect(agent:&str,id:&str,expected_version:u64)->Result<McpConnectionView,McpServiceError>`;
  owner PATCH enabled=true first; disabled409, no auto-toggle; initialize+complete first discovery
  with one shared 5s/256KiB control, fresh generation; no tools/call/grant; busy/stopping refuses.
- `discover(agent:&str,id:&str,expected_version:u64)->Result<McpConnectionView,McpServiceError>`;
  needs connected enabled generation, complete bounded tools/list only; never tools/call or grant.
- `review(agent:&str,id:&str,input:ReviewMcpTool)->Result<McpConnectionView,McpServiceError>` as above.
- `disconnect(agent:&str,id:&str,expected_version:u64)->Result<McpConnectionView,McpServiceError>`;
  idempotent runtime retirement, durable enabled/grants/reviews retained; no automatic reconnect.
- `delete(agent:&str,id:&str,expected_version:u64)->Result<(),McpServiceError>`;
  transactionally remove row and only this connection's aliases from that agent's saved config;
  retire before response, preserve unrelated grants. Absent ID remains generic404 without side effects.
- `capture_dispatch(agent:&AgentState,call:&ToolCall,run:&RunLink)->Result<McpDispatchStamp,...>`;
  before approval/admission waits, require the run's exact advertised full descriptor binding.
- `invoke(stamp:McpDispatchStamp,args:Value,cancel:CancelSignal)->Result<Content,McpServiceError>`;
  after existing approval/receipt, queue bounds/deadline then authoritative pre-RPC/post-result checks.
- `note_agent_permissions_changed(agent:&str)` / `retire_agent(agent:&str)` synchronously invalidate
  leaf epochs/cancel tokens; async cleanup is owned, never spawned untracked per callback.
- `known_descriptor(agent:&str,alias:&str)->Option<ToolDescriptor>` preserves own dormant reviews;
  `ready_descriptors(agent:&str)->Vec<ToolDescriptor>` selects live exact matches only (not grants).
- `request_shutdown()` prevents new connect/discover/call; `shutdown().await` joins owned retirement.
  All above I/O methods async unless expressly marked synchronous/known/ready/constructor.

## Errors, transactions and publication

- `McpServiceError`: InvalidRequest(400),NotFound(404),Conflict(409),Busy(429),Unavailable(503),
  Revoked,Cancelled,TimedOut,OutcomeUnknown; last four become static failed native TaskResults.
  Display never includes endpoint/server bodies/arguments/schema paths. Foreign IDs generic404.
- Owner GET/list are authenticated/no-store; all mutations use existing strict owner-write auth.
  JSON envelopes `{connection}` / `{connections}`; delete204. No model control endpoint is registered.
- Lifecycle POST bodies `{expectedVersion}`; create version1; PATCH/review bodies as above.
  Lifecycle version CAS is durable; generation fencing is server-captured, review token binds it exactly.
- `POST .../{connectionId}/discover` is explicit expectedVersion refresh on connected enabled session.
- Validate endpoint/schema/count/owner inputs before staging: owner HTTPS normal validated host,
  HTTP only literal 127.0.0.1/::1; no query/userinfo/fragment/auth headers/redirect/proxy/TLS bypass.
- Use existing global CP transaction -> brief daemon state -> leaf publication order.
  Clone leaf data and release synchronous read/write guards before every state/CP/network await.
- Stage next registry plus any exact grant removal without exposing it through state or publication;
  root adds candidate snapshot settlement on existing ControlPlanePersistRequest, not another store.
- Save before publishing known/live metadata atomically; known/live readers see only settled generation.
  Failed save keeps old catalog unless durability is explicitly uncertain; then fail closed and report it.
- Dropped owner caller does not drop admitted settlement; owned task finishes save/publish or retirement.
  A late connect/discovery result must match current owner/version/generation before leaf publication.
- CP v17 backs up existing v16 before upgrade; validate exact row owners/IDs/digests/aliases/counts
  against persisted ordinary agent identities. No live sessions/candidates/secrets in backup.

## Critical restore and actual root integration hooks

- Build validated own known catalog from snapshot MCP rows BEFORE `state.rs:2147` snapshot validation,
  including its `:2401` restored-tool resolution, and BEFORE `:3139` restore_agent_snapshot resolution.
- Current `resolve_restored_agent_tools` (:3260) deduplicates then rejects unknown static names;
  extend with agent_id and own validated known descriptors. Never accept arbitrary saved MCP bytes.
- `resolve_agent_tools` (:3238), create_agent(:3051)/update_agent(:3088) also need owner-aware known
  resolution; unrelated config edits and disconnected grants survive canonicalization unchanged.
- Restore canonical grant descriptors from known metadata; leave publication.live empty and runtime
  disconnected. Persist the same alias grants in subsequent snapshots; do not erase them for offline state.
- `ToolRegistry` register/lookup/resolve in tools.rs:1160–1224 needs a shared MCP function-pointer handler,
  plus canonical own known descriptors for validation; callback still requires manager live stamp.
- Fresh `ToolRegistry::new()` in agent_runs.rs:1597 and ToolExecutionContext registry must use the
  same manager binding source. Filter run-advertised MCP tools to configured names AND ready exact matches.
- Filtering affects the run copy only; never commit stripped offline grants into canonical AgentConfig.
  Swarm/other-agent canonicalization cannot use another agent's known aliases; reject unsupported scope.
- Wire context manager+RunLink+cancel; capture stamp before existing tools.rs approval wait, recheck after
  existing action intent save and inside invoke. Do not bypass Exec Ask/Deny or create a Read Ask policy.
- On agent config update/restore/rollback/delete and helper construction, invalidate affected MCP epochs;
  helper_config excludes all MCP aliases, invoke independently refuses helper runtimes.
- Root owns state/CP migration/backup/registry/runtime/approval/helper/shutdown hooks, not transport.

## Transport consumption, physical lifetime and result acceptance

- Use exact factory.connect(endpoint,control), session.discover(control),
  session.call_once(McpCall{remote_name,arguments,descriptor},control), session.close() contract.
- One control/budget across connect+discovery<=5s; call control<=20s absolute incl queue, new256KiB budget.
  No CP/grants in transport. Manager reserves final provenance envelope bytes within16KiB result cap.
- Shared manager owns one call permit/connection, two globally, max four waiters; no unbounded queue.
  Successful call_once must settle owned request I/O before returning. Cancel/error/timeout retains
  permits until close returns PhysicalClose; signalling cancel or returned timeout cannot release them.
- Cleanup timeout quarantines connection and retains permits until owned detached join finishes;
  no reconnect/retry. Retired generation can never republish a late success, even if grant is re-added.
- Before RPC and final result, recheck owner/live ordinary agent, exact current configured alias,
  full digest/version/wireGeneration/grantEpoch, live run/session/Stop and current reviewed discovery.
- Accepted content identifies alias/connection/full digest/generation, evidence-only guidance,
  isError/omissions/shortening; unknown remote effect is explicit failure, never success or rollback.
- Existing action journal remains sole dispatch witness; finish paired errors/receipts through normal
  executor. No remote result directly mutates canonical memory/skills/config or becomes system authority.

# M42 anonymous Streamable HTTP owner API contract

This is the first owner-configuration slice. It stores an endpoint without
network access, leaves it disabled, and does not create a local grant. All
routes are scoped to an existing owner agent and use the existing local-owner
write authorization and `Cache-Control: no-store` behavior. A foreign origin,
missing agent, or non-owner request must not reveal or mutate a connection.

`POST /api/agents/{agentId}/mcp-connections` accepts exactly `{label, endpoint}`
and returns `201 {connection}`. A new record has a generated `id`, `agentId`,
the normalized label and endpoint, `enabled:false`, `version:1`,
`state:"disconnected"`, and an empty reviewed-tool list. Creation performs no
DNS lookup, connect, initialize, discovery, review, tool call, or grant change.

`GET /api/agents/{agentId}/mcp-connections` returns
`{connections:[connection...]}` for that agent only. `GET
/api/agents/{agentId}/mcp-connections/{id}` returns `{connection}` or the same
not-found response for a foreign/missing ID.

`PATCH .../{id}` accepts exactly `{expectedVersion,label,endpoint,enabled}`.
It increments the version only after successful persistence. A version
mismatch is `409`; malformed or unknown fields are `400`. Editing an endpoint
invalidates live discovery and leaves the connection disconnected. A label-only edit preserves complete discovery and reviewed metadata while incrementing the durable version; existing dispatch stamps must still revalidate.
Turning `enabled` on is configuration only; explicit connect is still required.

`POST .../{id}/connect` accepts `{expectedVersion}` and starts one bounded
connect/initialize/discovery generation. It does not automatically call tools.
`POST .../{id}/review` accepts `{expectedVersion,discoveryId,descriptorDigest}` and records
owner review for exactly the current candidate set; review does not grant a
tool to any agent. `POST .../{id}/disconnect` accepts `{expectedVersion}` and
retires that connection generation. `DELETE .../{id}` accepts
`{expectedVersion}` and removes the owner configuration/review while revoking
only the corresponding aliases. These lifecycle calls return the updated
connection record and do not claim that a remote operation was undone.

The selected endpoint is explicit, anonymous HTTP only for loopback IPv4/IPv6
or HTTPS otherwise. Reject credentials/userinfo, query, fragment, custom
headers, redirects, proxies, and
unbounded bodies. No secrets are accepted or returned. Limit the workspace to four
connections; each connection has at most 32 bounded reviewed tool candidates.
Never inherit grants from profiles, helper agents, server annotations, or a
previous descriptor digest. Actual invocation requires a current reviewed
descriptor, the exact alias grant, existing Anima approval policy, and the
ordinary action receipt path.

Errors are bounded and redacted: `400` invalid input, `403` owner required,
`404` unknown/foreign ID, `409` stale version or candidate digest, `413`
over-limit remote payload, `503` unavailable or durability uncertain. A
saved connection is disconnected after restart; durable metadata is not a
readiness claim. Discovery is review-only. No resources, prompts, sampling,
elicitation, tasks, browser access, stdio, credentials, or connector execution
are in this slice.

Ruling: Connect combines initialize and complete first discovery under one5s/256KiBcontrol; refresh is explicit/discovery is never a tool call. Review identifies one candidate by expectedVersion/discoveryId/descriptorDigest and retains at most32durable reviews including dormant — preserve exact saved grants without silently replacing schema authority — cost if wrong: owners must revoke/remove obsolete reviews before exceeding the cap; no eviction/retry.

Dependency declaration: direct host rmcp=3.5.1/defaultsOff/client+transport-streamable-http-client, sse-stream=0.2.4 and existing transitive tokio-util=0.7.18/rt for CancellationToken. Core SHA256/JCS/jsonschema dependencies already exist; no second validation/async framework. Root alone edits Cargo/lock/exports.

## Final validation and next milestone

Meaningful owner-router RED preceded production. Three lanes, one native batch/final SDK-web review and one justified fix wave completed. Fresh required Rust/core: 2,565 passed, nine intentional ignores, 40 suites and lint; 35 focused MCP cases pass. Actual owner router, SDK fake HTTP/SSE, dispatcher/approvals/action receipts, Stop/revoke, changed descriptors, uncertainty, deletion and exact disconnected restore are covered. All22 Rust sources and the compiled README input are frozen. SDK/web source is unchanged; review is clean. No runtime provider request, real MCP endpoint, private profile, database server or container.

Exact publication and evidence remain in the private M42 ledger. M43 owner SDK/web controls follow the verified merge; making this client reviewable and usable costs less than expanding transports first. The 351-line draft and five binding risks are prepared. Stdio, community packages, credentials and real selected-server acceptance remain queued; PAR-04 and full Hermes parity are open.
