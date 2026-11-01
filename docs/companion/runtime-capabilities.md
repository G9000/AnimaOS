# Native runtime capabilities

These controls run in Rust. Tool selection and stored instructions do not grant extra permissions. Existing Strict/Custom approvals, action receipts, Stop and file checkpoint controls continue to apply.

## Background memory maintenance

Maintenance is separately disabled by default. The owner must enable the policy and approve exact Fact sources before background consolidation. Strict/Custom produce a pending decision; Free can apply only while the current policy and source approvals still permit it. Manual source review can inspect older untrusted facts without granting automatic trust. The operation history keeps exact before-images for Undo and restart recovery. See [learning](learning.md).

## Dedicated authenticated browser profiles

The browser profile owner endpoints create a dedicated profile, start sign-in, finish sign-in and revoke it. Select its profileId on the run's browser scope. The operator must configure a separate authenticated policy with the exact origin, paths and methods. The ordinary public browser policy remains GET-only.

Login state is encrypted outside the workspace using an OS credential-store key. A fresh browser worker restores that state; only one physical profile lease may be held at once. An uncertain worker close keeps that lease. Revocation closes the active context and invalidates captured approvals.

Drain and shutdown refuse new sign-in work. Pending sign-ins and physical contexts still awaiting confirmed closure remain in the drain count; finishing or revoking a profile stays available.

This first version supports direct same-origin sign-in. It refuses redirects, SSO, popups, iframes, parent/sibling-domain cookies and personal-browser attachment. Use an installed supported browser engine and the existing verified worker configuration.

## Deferred tool discovery

Set settings.toolDiscovery to true or grant both search_tools and describe_tool on an agent to opt in. The Run commands access profile includes both discovery controls and web search. The first model request exposes the permitted essentials and search_tools/describe_tool. Search or describe hydrates exact selected schemas into the next request. A run retains at most eight hydrated names and 64 KiB of schema data. Discovery results are bounded to 4 KiB.

The canonical grants remain intact. Actual calls still pass ordinary native or MCP permission, approval, generation and receipt checks. Reconnection, changed schemas and revoked grants invalidate captured catalog entries. Disabled discovery preserves the existing direct-tool behavior; no universal token or cost saving is promised.

## Research and set up integrations from chat

Direct owner web/API chats expose `manage_mcp` when its manager is available, `import_github_skill` and `manage_skill_runtime`, including with deferred discovery. Setup guidance directs the agent to research official documentation, inspect the original package and continue supported setup. External installation instructions remain untrusted content. They cannot grant tools, bypass owner review or authorize unrelated commands.

MCP setup uses the existing connection manager and approval policy. Browser OAuth sign-in stays in **Connectors > MCP > Sign in**, and static Bearer secrets stay in its password control. Chat reports saved OAuth credential presence separately from current connection/discovery state. A missing service-specific tool is not proof that its MCP server cannot be configured. See [MCP setup](mcp.md).

`import_github_skill` accepts `repository` as `owner/repository`, optional `revision`, optional `path` to the skill directory, and optional local `slug`. The agent derives these from the GitHub URL it inspected. Omitted revision resolves the repository's default HEAD once; every file then comes from that immutable commit's tree and blob IDs. Only public repositories are supported, without credentials, redirects or ambient proxies.

The package preserves `SKILL.md`, portable root files, nested code, references and binary media. Hard limits are 256 supporting files, 8 MiB each, 32 MiB decoded total, and a 36 KiB main skill file. Text keeps its existing representation; binary assets use base64 in the API and original bytes on disk. The package hash covers decoded bytes and paths. Acquisition caps 300 requests, 48 MiB wire data, 1024 tree entries and 120 seconds. Pinned raw GitHub downloads verify Git blob identities and sizes without one REST API request per file. Symlinks, submodules, LFS pointers, unsafe paths, executable binaries, truncated listings or a missing file fail the entire import. Excluded hidden repository metadata is explicitly reported. README, license and `.skillignore` files are preserved.

Imports use the existing pending **Import** draft even in Free. The Skills card shows the repository, pinned commit, original instructions/metadata and excluded files before the owner chooses **Keep**. Keeping unchanged content preserves the original SKILL.md bytes, including extra front matter. Editing the body preserves its front matter; metadata remains inert and cannot grant tools. The stored source hash identifies the original package; owner edits may produce different approved bytes. Import does not run scripts, install dependencies or grant tools. Stop, changed workspace and current write policy are checked again before draft admission. A changed replacement base requires a fresh GitHub import before approval.

### Prepare and verify a Python skill

After Keep, `load_skill` returns the approved package hash and reviewed file list.
`manage_skill_runtime` accepts `action`, `slug`, `hash`, optional `requirements`,
`entrypoint`, literal `args` and a bounded `timeoutMs`.

- `status` inspects installation and the exact recorded check without running code.
- `prepare` creates a private source snapshot and dedicated Python venv outside
  the workspace. Select an installed Python 3.12+ with `ANIMAOS_SKILL_PYTHON`, or
  use an installed interpreter discovered outside the workspace. Reviewed ordinary
  PyPI requirements install as wheels from the fixed public index. Recursive,
  editable, URL and source-build dependencies are refused with a specific error.
- `run` executes an approved relative `.py` entrypoint in that snapshot. Use the
  package's actual preflight and record its exact arguments and native exit code.

Prepare and run each require an exact owner approval even in Free. They use
local OS execution with the daemon user's access; a venv isolates dependencies,
not filesystem or network access. The general host-shell gate stays separate.
Ambient account credentials, user homes and Python/pip configuration are not
inherited. Installed helper programs can use a filtered startup PATH; workspace
directories, runtime staging directories and project `node_modules` shims are
excluded. Stop, deadlines, current policy, live owner authority, package identity,
process-tree custody and durable native action receipts still apply. Other
languages and unsupported dependencies require an available approved execution
tool. Credentials remain in their supported connector controls.

The managed local Python path currently requires Windows Job Object custody.
Hosts without the required strong process custody refuse execution before launch;
use an operator-configured supported execution target there.

Report imported, owner-kept, environment-prepared, exact-command-verified and
account-connected as separate observations. A passing `--help` verifies that
command only. Failed setup or a changed source/environment cannot establish
readiness. Binary assets stay out of model text and browser execution; review
shows metadata and bounded literal text previews.

## Selected execution backends

An operator-owned profile file selects installed Local, Docker or SSH transports. execution_targets lists profile IDs; execution_run accepts only targetId, command, cwd and timeoutMs. It requires ordinary execution permission and approval. An unavailable target never falls back to local execution.

Docker uses an installed digest-pinned image, no host mounts or network, a read-only root and bounded resources. SSH requires an operator endpoint, identity key, host pins and the fixed Rust worker. Unknown effects or cleanup keep the target occupied until authoritative reconciliation. Arbitrary command effects are not Undo operations. See [execution profiles](../../hosts/rust-daemon/EXECUTION-BACKENDS.md).

## Doctor

GET /api/doctor and doctor_report use the same bounded redacted observations. Configuration and local runtime facts are separate from unprobed remote reachability. A diagnostic report makes no model, shell or external request.

Doctor can pause one repeatedly failing recurring workspace automation using an exact candidate fingerprint. The durable repair history supports Undo only while the paused record is unchanged. Undo retains outcomes/counters and calculates a new future due time. These writes use normal approvals in native runs, or authenticated owner controls.

The only cache repair rebuilds a bounded local RAM memory index. It does not change canonical memories or the persisted vector cache, and reports undoSupported false. Nonlocal, busy or over-budget index repairs are refused.

## Standing conversational intentions

The owner can save a keyword condition or one of the host events conversation.started/conversation.message, an instruction, scope, expiry, cooldown and total fire budget. Management is available through the owner endpoints and the standing-intention tools in proven local-owner runs.

Matching uses no model call. Up to three matches, ordered by creation time and ID, append the saved instructions to the same admitted owner run. Normal tool approvals remain authoritative. The admission durably consumes each match once; request retries and failed runs do not re-arm it. Cancel prevents future admissions. A run already admitted retains Stop. Revisioned edits and cancellation have exact Undo handles that refuse intervening changes.

The registry bounds records, edit history, matching admissions and instruction text. Only authenticated owner inputs may create or widen an intention; connector matching requires current private-owner inbound proof.

## Paired nodes

Run anima node serve --config with an absolute operator configuration path. It requires a selected root, persistent state outside that root and a valid TLS certificate/key. Loopback is the default; remote binding requires explicit operator approval. Startup prints the identity, manifest digest, TLS address and an expiring one-use ticket.

The owner pairs the endpoint by checking that identity and manifest digest. Pairing creates no tool grants. Use the existing MCP discovery, individual descriptor review and grant controls afterward. Calls then use the ordinary native action approval and receipt lane.

The declared capabilities are system information, listing the selected directory and bounded reading of an ordinary UTF-8 file inside it. Node invocations carry host-derived run/call identity, exact capability bindings and deadlines. Replay receipts persist across node restart. The stored credential is bound to the paired manifest; changing the root or generation invalidates it before startup succeeds and requires owner review and pairing again.

Cancellation retires the captured transport generation. If remote completion cannot be established, the action retains an unknown outcome. Revocation blocks local dispatch and removes MCP aliases and credentials. remoteRevocationConfirmed records whether the node also confirmed invalidation; an unreachable peer does not count as confirmed remote revocation. Replay storage fails closed at its bound rather than forgetting old request IDs.

## Running instance and outcome evidence

The capability inventory reports a `runtime` observation of the responding daemon instance, with its process-lifetime identity, observation time, version and optional build revision. Versioned image-history and PDF-page API contracts describe registered support. Image configuration describes local workspace, durable-storage and credential prerequisites. Endpoint/model access and provider health remain unchecked until dispatch. Chat capabilities retain their separate captured tool ceiling and current approval policy.

The same observation is available in `doctor_report`, `search_tools` and `describe_tool`. An unavailable local-state observation remains unknown. A source edit or green code check does not deploy a new daemon or establish live feature acceptance. Compare the responding instance and advertised contracts when reporting that a backend can perform an action. For example, report that code checks passed while the running backend does not advertise image-history support when that is the observed state.

Outcome verification begins with a bounded saved-image adapter. It reports exact local byte readback with target/version identity and time. It does not verify arbitrary external effects, image meaning, remote persistence or billing. Model-written assertions never become host observations. Existing completion file/version checks and native receipts retain their current settlement and authority rules.
