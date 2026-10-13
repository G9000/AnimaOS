# M44 owned stdio MCP: implementation plan

M43/PR49 is verified merged at5580abec15c59e63dd196f13d4a102eb5f6c2f7d with passing gates/controlled acceptance. M44 branch g9000/companion-mcp-stdio-m44 is cut from exact main. Binding rulings below supersede proposed alternatives; M44 behavior/gates remain unverified until recorded.

## Goal and boundary

Add one owner-configured, long-lived MCP stdio transport for an ordinary Anima agent, reusing the existing M42 `McpManager`, reviewed-descriptor grants, dispatch stamps, approval path, action receipts and bounded call policy. Use the already-pinned official `rmcp` 3.5.1 SDK for MCP client protocol and stdio framing. Keep the existing anonymous Streamable HTTP path working unchanged.

This is transport support, not package installation. An owner supplies an explicit executable and argument vector; the host never invokes a shell, package manager, `npx`, `uvx`, or downloaded artifact. New records are disabled by default. No implicit environment inheritance, secrets, OAuth, browser setup, community catalog, auto-install, or tool grants. No endpoint or process starts on create, list, restore, or review. Existing explicit enable/connect/disable/disconnect/owner removal flow remains the authority.

PAR-04 stays partial. This is not full Hermes parity. Selected local fake-process evidence does not establish general package compatibility, production process isolation, or arbitrary community server safety.

## Source-backed reuse and cost choice

`hosts/rust-daemon/Cargo.toml` pins `rmcp = 3.5.1` with `client` and `transport-streamable-http-client`; it does not currently enable child-process transport. The exact cached crate source at `C:/Users/leoca/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.5.1/` exposes `transport::TokioChildProcess` behind `transport-child-process`. Its builder accepts `tokio::process::Command` and piped stdio, and the transport implements `rmcp::transport::Transport<RoleClient>`. That feature brings `transport-async-rw`, Tokio process support and the crate's `process-wrap` dependency. Its source also shows Drop schedules a kill task; explicit `graceful_shutdown` closes transport and waits up to a fixed three seconds before kill. Therefore the SDK is the lowest-cost protocol adapter, while its default process lifecycle alone is not enough to satisfy Anima's tracked cleanup/quarantine guarantees.

Recommendation for root binding: enable only `rmcp/transport-child-process`; use `TokioChildProcessBuilder` for the owned protocol pipes, but retain a narrow Anima MCP child-owner/session wrapper that records child identity, stderr reader and cleanup completion in the existing `McpManager` lifecycle. Do not route MCP children through `bg_start`: `tools/process/background.rs` runs shell strings, captures line-oriented output, exposes process controls and has a separate eight-process allowance, so it cannot provide private framed stdin/stdout or connection-scoped MCP authority. Reuse its explicit child kill/wait and owned-reader cleanup principles, not its shell command surface or storage.

The existing M42 public comparison inspected Hermes at commit `6e7b83a2f1c093e992a8d838ed77ea320fbd8e51` and cited `tools/mcp_tool.py`, `tools/mcp_tool_transport.py`, `tools/mcp_tool_lifecycle.py`, and the upstream MIT license. That evidence supports comparing its one long-lived server task and lifecycle split. It does not show that Hermes' Python implementation is a suitable Rust process owner or that M44 has parity. No Hermes code is copied in this draft. If implementation later copies code, record exact source lines, license and notice before merge; otherwise retain the existing source-comparison citation only.

## Proposed connection record and lifecycle

Keep the existing connection identity, owner, enabled flag, version, reviewed tools and runtime state. Add a strict tagged transport configuration with two variants:

- `streamableHttp { endpoint }` for the existing M42 rows and wire behavior.
- `stdio { executable, args }` for an explicitly supplied executable path/name and ordered argument strings.

No shell string, command template, env map, working-directory override, executable download, or arbitrary launch metadata is accepted. The exact persisted serde migration shape and owner HTTP DTO need root binding before production; preserve all M42 serialized HTTP rows exactly if schema compatibility can do so. If a safe additive record schema cannot preserve existing rows, stop and split a versioned migration into its own plan rather than silently rewriting them.

A stdio record starts disabled and disconnected. `enable` only saves configuration. Explicit `connect` resolves the executable, starts one child with stdin/stdout piped and stderr separately drained into a bounded redacted diagnostic buffer, then completes initialization and first discovery under the same M42 5-second/256-KiB shared operation budgets. Discovery and descriptor review never execute a tool. Review remains bound to exact discovery ID and full descriptor digest. A changed descriptor gets a new alias and requires a separate grant.

One physical connection owns one child process and one client session. Calls keep M42 per-connection serialization, global two-call slots, queue and deadline, schema/output/argument bounds, approval classification, provenance and receipts. Stop, revoke, timeout, disconnect, disable, delete and daemon shutdown cancel the session and child; no slot or replacement child is allowed until the child is reaped and owned stream/reader cleanup finishes. If kill/wait/reader join cannot establish exit, retain a quarantine and surface static cleanup uncertainty. Never claim a remote effect was undone just because local process termination succeeded.

No automatic restart, crash loop, retry, package install, executable update, or launch after daemon restore. Restored stdio and HTTP records stay disconnected; no process starts on restore. Owner must explicitly connect again. Executable resolution should happen only at explicit connect and should produce a static diagnostic without leaking environment or full argv. Avoid logging arbitrary arguments; they may contain local paths or sensitive values even though env and credentials are disallowed.

## Strict process input and platform rules to bind

- Accept only a bounded executable identifier/path and bounded argv count, per-argument UTF-8 bytes, and total serialized bytes. Reject NUL/control characters and invalid/empty executable values.
- Never interpolate args into a shell or pass MCP args to the shell tool. Spawn directly with `tokio::process::Command` through the rmcp builder.
- Clear inherited environment by default. Add no env configuration in this milestone. Verify the minimum OS runtime behavior without printing env data.
- Pin process working directory to an existing daemon-controlled safe directory; do not inherit agent workspace or honor remote metadata for cwd.
- Enforce workspace-wide connection/child caps jointly across HTTP and stdio; keep four configured connections global, not four per transport. One child per configured connected stdio record, with no background process-manager quota bypass.
- Separate stderr from MCP stdout. Drain it continuously to avoid child deadlock, retain only a small bounded tail, and redact/truncate before owner-visible diagnostics. Never merge stderr into JSON-RPC.
- Treat process exit, malformed protocol output, pipe failures and initialization timeout as unavailable/degraded with a static owner error. No raw stderr or child error strings in model context or API.
- Require an explicit owner confirmation in the future UI before saving/running a stdio executable. Do not make stdio selectable from existing HTTP-only controls until the SDK and console wire are updated in a separately reviewed web lane.

These are proposed defaults, not frozen values. Root must bind which executable identifiers are permitted, whether absolute paths are required, exact byte/count bounds, safe cwd, env policy, Windows process-tree containment, combined transport caps, and UI/API sequencing before implementation.

## Interface and file ownership proposal

Root owns Cargo feature changes/lockfile, record migration and restore, manager dispatch and shutdown integration, route registration/OpenAPI, shared state, and aggregate gates. A bounded daemon implementation would likely touch:

- `hosts/rust-daemon/Cargo.toml` (enable the already-pinned rmcp child transport feature only, unless source proves another dependency necessary).
- `hosts/rust-daemon/src/mcp/records.rs` (strict tagged transport configuration, validation, preserve HTTP restore).
- `hosts/rust-daemon/src/mcp/transport.rs` or a focused `mcp/stdio.rs` (rmcp adapter, child identity, bounded stderr drain, kill/wait cleanup).
- `hosts/rust-daemon/src/mcp/service.rs` (select transport based on validated saved config, shared budgets/slots, retain current grant/approval behavior).
- `hosts/rust-daemon/src/mcp/tests.rs` (manager lifecycle and coexistence cases).
- `hosts/rust-daemon/src/routes/contracts/mcp.rs`, `routes/mcp.rs`, and `routes/tests/mcp.rs` (only after HTTP DTO migration is bound).
- route/OpenAPI registration and state wiring stay with root.

The `packages/sdk` and `apps/web` MCP DTO/panel need a separate additive lane before owners can configure stdio through the console. Do not silently add half of a public transport variant or make the panel send raw command strings against an HTTP-only server. A first daemon-only vertical slice can use strict route tests and fake executable fixtures, then a follow-up SDK/web lane can expose it once lifecycle acceptance passes.

## RED and focused acceptance matrix

Write tests first using only deterministic local child fixtures (small test binary or existing test executable mode), never a downloaded community server, real account, provider, or user config.

1. **Feature/reuse:** compile-time exercise `TokioChildProcessBuilder` through the pinned SDK; fake process exchanges initialize/list/call/close over real stdio framing. Confirm HTTP adapter tests remain unchanged.
2. **Strict launch config:** missing/unknown/null fields, shell injection-shaped strings, NULs, overlong argv, empty executable and bad encoding fail before child spawn. Assert process-start counter stays zero on create/list/restore/review/invalid config.
3. **Authority:** disabled connection cannot start; helper/foreign owner denied; enabled explicit connect starts exactly one child; discovery/review never calls a tool; exact grant and normal Exec approval remain required.
4. **Lifecycle:** stdout contains only JSON-RPC; large stderr is drained but bounded; exit before init, malformed output, stalled initialization and EOF produce static redacted state; no retry or automatic respawn.
5. **Stop and cleanup:** Stop/revoke/disconnect/disable/delete/shutdown while child is blocked cancels local session, waits for child exit and readers, keeps call slot/quarantine until confirmed, and rejects a replacement process during cleanup. Include a child that ignores graceful close and a forced-kill path.
6. **Restore and mixed transports:** restart restores both HTTP and stdio metadata/grants disconnected with zero child/network starts. HTTP and stdio share the global configured/active-call limits and never evict each other's grants or reviews.
7. **Failure settlement:** precommit save failure keeps old record/config; uncertainty is fail-closed and retires child; caller drop does not orphan child or skip settlement. Verify exact receipts/provenance and no endpoint/tool invocation for configuration-only operations.
8. **Bounds and redaction:** initialize/discovery time and raw stdio bytes bounded; oversized frames/events, huge stderr and invalid UTF-8 do not leak into owner errors, task results or logs. Do not claim subprocess sandboxing; these controls bound resources and local visibility only.

Suggested focused names: `m44_stdio_sdk_fake_server_handshake_and_call`, `m44_stdio_create_review_restore_do_not_spawn`, `m44_stdio_invalid_launch_arguments_are_rejected_before_spawn`, `m44_stdio_explicit_connect_requires_review_grant_and_exec_approval`, `m44_stdio_exit_timeout_and_malformed_frames_are_static`, `m44_stdio_stop_revocation_and_shutdown_reap_before_reconnect`, `m44_http_and_stdio_share_global_limits_and_restore_disconnected`, and `m44_stdio_failure_settlement_and_caller_drop_are_owned`.

## Five risks for root binding

| Risk                                                       | Proposed ruling                                                                                                                            | Cost/limit                                                                         |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| rmcp drop is not enough proof of child cleanup             | Keep SDK protocol transport, add Anima child/session owner that waits and retains quarantine through reader/process cleanup                | More lifecycle glue than calling `TokioChildProcess::new`; avoids false slot reuse |
| Owner-supplied executable can run arbitrary local code     | Disabled by default, explicit owner save/connect, no shell, no env map or package manager, visible command identity and strict argv limits | MCP stdio remains a trusted local executable chosen by owner; not a sandbox        |
| M42 durable record/wire compatibility                      | Preserve existing HTTP row shape and default behavior; strict tagged transport only for stdio; reject ambiguous legacy data safely         | Possible migration/version work; don't silently rewrite existing records           |
| Process output/args leak sensitive local data or deadlock  | Piped protocol stdout, continuously drained bounded stderr, no raw argv/stderr in model/API, bounded frames and diagnostics                | Some diagnostics will be generic; avoid logging paths/arguments                    |
| Mixed transport limits or restore cause surprise execution | One workspace-wide cap and shared M42 manager/policy; restore disconnected and no automatic restart                                        | Owner explicitly reconnects after restart; no reconnect promise                    |

## Reuse attribution and validation status

- Official MCP SDK: pinned local `rmcp` 3.5.1 source and the M42 crate manifest. Its `TokioChildProcessBuilder` is the proposed stdio transport; enabling its child-process feature is not yet implemented or verified in this milestone.
- Anima ownership: current M42 `McpManager` owns per-connection lifecycle, global call slots, cleanup, grant epochs and quarantine. `tools/process/background.rs` is inspected as a lifecycle reference only and is unsuitable as the MCP transport because it executes shell commands and owns line-based background output.
- Hermes: public source provenance recorded in the M42 plan is `NousResearch/hermes-agent` commit `6e7b83a2f1c093e992a8d838ed77ea320fbd8e51`, files `tools/mcp_tool.py`, `tools/mcp_tool_transport.py`, `tools/mcp_tool_lifecycle.py`, license MIT. M42 recorded a lifecycle comparison; this draft does not assert stdio code reuse or behavior parity. Reinspect those exact public files if implementation needs a substantive comparison. No private installation data was read.
- No code, tests, dependencies or gates changed for M44. M43 merge, gates and acceptance are prerequisites; after they are recorded, root should bind this scope and choose a branch before implementation.

## Controller rulings on the risks (binding)

Ruling: Reuse pinned rmcp3.5.1 AsyncRwTransport for JSON-RPC framing/lifecycle and process-wrap10.0.1 for direct process group/Windows Job Object ownership, with a narrow Anima bounded-pipe/cleanup wrapper — TokioChildProcess hides raw pipes and its default drop schedules cleanup without a joined witness, so it cannot enforce M42 pre-decode wire limits or alone prove physical exit — cost if wrong: adapter glue and one direct dependency already required by SDK child-process mode; no new protocol engine or raw platform FFI. Enable transport-async-rw plus explicit Tokio process/io-util, not unused child-process transport. All cleanup results remain conservative/quarantined.

Ruling: Accept only a strict stdio object with an absolute executable path and an ordered argv array; no PATH lookup/shell/env/cwd/package manager, and launch configuration is immutable after save — explicit local owner chooses trusted executable code, but mixed or silently retargeted configurations undermine review authority — cost if wrong: owner must remove/recreate a changed launch and cannot use npx/uvx or ambient-secret servers yet. Windows requires .exe; reject known shell/package-manager launchers. Bound executable2048bytes,32args,each2048bytes,total16KiB, reject NUL/controlchars. Clear inherited environment; preserve only source-proven minimal OS system variables by fixed names when required, never provider/key/PATH/user-profile/config variables. Use an owned tempfile3.x private cwd (existing locked transitive crate, exact version resolved before edits); no owner cwd override. This is process resource ownership, not an OS sandbox or arbitrary installed-server safety proof.

Ruling: Preserve legacy HTTP persisted rows exactly and add optional stdio launch with serde default/skip-None, using empty durable endpoint only for validated stdio; bump CP17→18 with independent .pre-mcp-stdio.bak and validate before publication — explicit versioning distinguishes new process records while existing aliases/reviews/grants survive — cost if wrong: one narrow backup/migration and current-version assertion updates. HTTP create remains {label,endpoint}; stdio create is {label,stdio:{executable,args}} via strict mutually exclusive DTO variants. Public response adds transport:'streamableHttp'|'stdio' and stdio:null|launch; endpoint remains string (empty for stdio). Existing operations, CAS/epochs and grant shapes remain. PATCH allows label/enabled for stdio; endpoint or launch replacements are invalid. No optional null ambiguity, helper grants or auto-launch on restore.

Ruling: Bound stdout before SDK decoding and drain stderr continuously to a discard-only sink with no owner/model/raw-log exposure; require joined process, protocol and reader cleanup before PhysicalClose — output framing or a dropped initialization future must not leak memory, hold open pipes or falsely free a slot — cost if wrong: diagnostics are static and failed containment/kill/wait/join retains quarantine. Reuse256KiB operation/64KiB rawline limits,5secondinit/discovery,20secondcall and existing schema/results/caps. Count idle unsolicited bytes under a fixed256KiB idle allowance, reset only at an explicit owned operation; no unbounded idle reader. Use process-wrap KillOnDrop shim plus JobObject on Windows and ProcessGroup leader on Unix. Missing containment fails before SDK startup; test a spawned descendant and reader-held pipes, forced exit, caller drop and repeated close. Unsupported platforms fail closed; local Windows evidence cannot claim Unix runtime acceptance.

Ruling: Share all four global configured connections, physical slots, review/grant/approval/receipt and disconnected restore rules across transports, and expose a separately explicit trusted-local-process choice in the existing panel — stdio must not bypass M42 limits or surprise-start during save/reload — cost if wrong: owner confirms local-code trust, starts/reconnects explicitly and receives static unavailable on failure. No tools/call button, installs, credentials, automatic restart/retry, new defaults or extra model requests. HTTP console flow/dirty fields/navigation reconciliation remains exact. One native batch/final SDK-web review/fix wave and fresh required gates; controlled synthetic child only, no ordinary profile/account/services.

## Frozen lane interfaces and RED

- McpStdioSpec {executable:String,args:Vec<String>}; record.stdio:Option<McpStdioSpec> omitted for HTTP. ValidatedMcpLaunch distinguishes existing validated HTTP endpoint and validated stdio spec plus connection identity; factory receives launch, not an unchecked body. Existing fake factories adapt explicitly. No arbitrary command from remote descriptors.
- Stdio child owns tempfile cwd, process-wrap child handle, cancellation, bounded stdout and stderr task plus SDK close witness. Shared SDK discovery/result helpers may be extracted narrowly from current transport.rs; static error mapping, protocol2025-11-25, disabled cache, no fallback/retry remain.
- Native record/service/routes/migration lane owns records.rs/service.rs/tests.rs/routes MCP DTO+handler, exact CP version/migration restore hooks and affected assertions; transport lane owns stdio.rs/transport.rs/mod.rs and transport regressions; SDK/web lane owns SDK mcp types/tests and existing MCP panel/tests. Root owns Cargo/lock/dependency fetch, all commands/gates/integration/docs/Git/services.
- First native RED: actual owner router accepts syntactically valid nonexistent absolute synthetic executable in a disabled stdio save, expects201/current400, proves save performs no spawn. First UI RED: current MCP section lacks explicit Connection type selector, expects it present. Root observes actual behavior assertion failure before releasing production. SDK create already serializes typed bodies; extend wire types without inventing a redundant convenience method solely to manufacture RED.
- Minimum cases: strict mixed/null/unknown input; bounds/no shell; save/list/review/restore zero starts; descendant kill/reap/pipe close; cancel/dropped startup/call and repeated cleanup; raw line/no-newline/idle/stderr bounds; same protocol and exact reviewed approval before dispatch; caller-owned effects unknown; HTTP+stdio shared limits/grants; CP backup/restore and uncertainty; current UI explicit trust/start/changed agent/permissions preservation. Real task quality and real packages are not inferred.

Reuse provenance: official pinned [rmcp3.5.1 source](https://docs.rs/rmcp/3.5.1/src/rmcp/transport/async_rw.rs.html); maintained [process-wrap10.0.1](https://docs.rs/process-wrap/10.0.1/process_wrap/) and [upstream](https://github.com/watchexec/process-wrap), process groups/Job Objects and shims instead of custom platform code. Root must retain actual fetched crate licenses, MSRV/lock/cost and precise cleanup semantics before claiming acceptance. The private draft's public Hermes comparison remains behavioral provenance only; no private installation or copied Python runtime.
