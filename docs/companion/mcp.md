# Reviewed external tools

M42 adds the first MCP client path for an ordinary Anima agent. It supports anonymous Streamable HTTP connections with the explicitly selected `2025-11-25` protocol and tools only. The owner API saves, connects, discovers and reviews tool descriptions. A saved or reviewed connection grants no tools and makes no tool calls.

The owner separately adds an exact reviewed alias to the agent's existing tool grants. Every external tool uses the existing Exec approval category; server hints cannot lower its classification. Helpers inherit no MCP grants. The native dispatcher captures the full reviewed descriptor, live connection generation and grant epoch before approval, then rechecks them before dispatch and before accepting the result.

## Owner API

All operations are scoped to one existing ordinary agent at `/api/agents/{agent_id}/mcp-connections`. They use the existing local-owner authorization and return `Cache-Control: no-store`.

| Operation                  | Request                                                             | Result                                                                       |
| -------------------------- | ------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| GET collection             | none                                                                | Own saved configuration and current runtime state                            |
| POST collection            | `label`, `endpoint`                                                 | Disabled version-one connection; no network I/O                              |
| GET connection             | none                                                                | One own connection                                                           |
| PATCH connection           | `expectedVersion` and one or more of `label`, `endpoint`, `enabled` | Saved compare-and-swap edit                                                  |
| POST connection/connect    | `expectedVersion`                                                   | Initialize and complete bounded discovery                                    |
| POST connection/discover   | `expectedVersion`                                                   | Refresh current connection discovery                                         |
| POST connection/review     | `expectedVersion`, `discoveryId`, `descriptorDigest`                | Save one exact reviewed descriptor; no grant                                 |
| POST connection/disconnect | `expectedVersion`                                                   | Retire runtime transport; keep reviewed metadata and grants                  |
| DELETE connection          | `expectedVersion`                                                   | Save removal and remove only this connection's aliases from the owner grants |

Unknown fields, explicit null patch fields and empty patches are rejected. Endpoints allow HTTPS or literal loopback HTTP (`127.0.0.1`/`::1`), without user information, query strings, fragments, authentication headers, redirects or ambient proxy use. Credentials and arbitrary process commands are outside this milestone.

Aliases contain the connection UUID and a descriptor-digest prefix. The full immutable digest is checked as well; a collision is rejected. Changed remote schemas/descriptions produce a new ungranted alias. Endpoint or enabled edits retire live discovery; a label-only edit keeps discovery while advancing the saved version. Old reviews remain dormant when their exact descriptor is absent.

## Limits and recovery

- Four connections across the workspace; 32 durable reviews per connection, including dormant reviews.
- Five seconds for initialization plus complete first discovery; at most four pages and 64 candidate tools.
- Twenty seconds for one call, including queueing; one physical call per connection, two globally and at most four queued callers.
- Aggregate raw wire budget 256 KiB, individual JSON/SSE event 64 KiB, including raw comments and progress traffic.
- Reviewed descriptor/schema pair and arguments each 16 KiB, depth 16 and 1,024 nodes. Schemas use explicit Draft 2020-12, acyclic local `$defs` references and safe numeric values.
- Final model evidence envelope at most 16 KiB and 16 text blocks. Unsupported blocks and shortened content are explicitly marked. Returned text is external evidence with connection, generation, alias and digest provenance.

There is no automatic retry, reconnect, protocol fallback, stale discovery cache or additional model lookup. A timeout, Stop or revocation cannot prove that a remote effect was undone. A retired physical call keeps its slot until owned cleanup joins. Unknown cleanup retains its quarantine; shutdown can report unsettled effects without reusing the slot.

Control-plane version 17 retains reviewed metadata and exact grants, with an independent `.pre-mcp.bak` for version 16. Restart restores connections as disconnected and opens no endpoint. Only an explicit connection and exact rediscovery can make a reviewed grant callable again. Candidate saves settle through the existing ordered control-plane path before shared publication. Precommit failure preserves old metadata; uncertain post-replacement durability retains the candidate fail-closed and reports unavailable.

## Scope still open

This is a partial PAR-04 milestone. Stdio process ownership, community package installation, credentials/OAuth, SDK/web owner controls and real selected-server acceptance remain queued. M42 does not establish whole Hermes parity. The implementation reuses the official Rust MCP SDK; public Hermes source informed lifecycle comparison, without importing private profiles or copying its Python runtime.

See the [execution plan](../superpowers/plans/2026-10-13-companion-mcp-m42.md), [parity roadmap](parity-roadmap.md) and [harness evidence](harness-acceptance.md). Fresh Rust/core validation passed 2,565 tests with nine intentional ignores across40 suites, plus lint and35 focused MCP cases. Controlled fake SDK/dispatcher/approval/restore evidence uses zero runtime provider requests. Exact publication is recorded in the M42 private ledger.
