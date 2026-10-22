# Companion Connectors M12 — Lean Plan

> Execution authorized by the owner on 2026-10-12: merge PR #17 and continue. M11 merged at `3721894`, including naming correction `21120aa`. Active branch `g9000/companion-connectors-m12` is cut from that main. Binding five-risk rulings and post-merge interfaces appear below.

## Outcome

Create a small host-agnostic Rust connector contract and compile-in registry,
then route the existing Telegram transport through it without replacing the
proven durable daemon lifecycle. Make community source contributions possible
through documented traits and host registration. Describe installable modules
as future work requiring a genuine isolation, process, and dependency policy.

Active branch: `g9000/companion-connectors-m12`.
It was cut from main `3721894` after the owner-authorized M11 merge. The original
preparation below is retained for design evidence; binding controller interface
decisions at the end supersede draft interfaces.

Priority order remains Telegram, Discord, Gmail, Calendar, Drive.
M12 implements foundation plus Telegram only. Discord REST chat is the next
bounded messaging milestone. Gmail and Calendar reuse their existing native
services in a later migration. Drive begins with explicitly limited reads.
No milestone number beyond M12 is promised before its scope is accepted.

## Evidence and source map

Read `AGENTS.md` and the M8/M10 lean plans and binding rule updates.
The current Telegram implementation is not a blank integration project.
`hosts/rust-daemon/src/connectors/mod.rs` defines durable inbound/outbound rows.
`connectors/runtime.rs` wires focused lifecycle, inbound, delivery, owner,
processing, workers, attachments, and configuration modules.
`runtime/owner.rs` scopes connector lookup to the selected agent and handles
accepted runs and idempotent replay through the durable history boundary.
`runtime/delivery.rs` contains delivery authority and attempt claims introduced
for M10; preserve those fences across adapter calls.
`runtime/tests/` contains lifecycle, startup, processing, transaction, worker,
and delivery tests; adapt those fixtures instead of inventing a parallel runner.
`connectors/telegram.rs` owns Bot API transport and normalized payloads.
`connectors/credentials.rs` owns the existing token store boundary.

`connectors/mail/mod.rs` already supports Gmail and Outlook, local immutable
drafts, OAuth generations, and owner-boundary sending. Draft state includes
unknown effects. It is not a generic installed connector lifecycle.
`connectors/gcalendar/mod.rs` provides on-demand calendar tools and
confirmation-gated writes; it explicitly has no polling loop.
`connectors/oauth_apps.rs` supports Google/Microsoft apps and fixed redirect
allowlists. A Drive callback and scopes are absent from that inspected list.
`routes/connectors.rs`, `routes/mail.rs`, `routes/gcalendar.rs`, and
`routes/oauth_apps.rs` expose existing owner API boundaries.

`packages/core-rust/crates/anima-core` has existing async-trait, serde, futures,
and host-neutral engine capability ports. It has no HTTP framework or host DB.
`packages/core-ts/src/types/mod.ts` defines stateless ModPlugin tools with no
init/cleanup hooks; do not claim those types implement connector lifecycle.
`packages/mod-sdk/src/define.ts` is an identity helper for ModToolHandler.
The older Plugin interface has init/cleanup but is also not a Rust runtime ABI.
`packages/sdk/src/connectors.ts` describes concrete existing native services.
`apps/web/src/components/ConnectorsView.tsx` is the existing connector surface.
`routes/capabilities.rs` explicitly says installation and portable bridge are
unavailable. Keep that claim until a later milestone actually ships them.

Current control-plane snapshot version is 11, independently verified in
`hosts/rust-daemon/src/control_plane_store.rs` and current state tests.
An in-memory adapter descriptor does not require a snapshot version bump.
Any proposed new durable field requires an explicit controller ruling first.
Do not assume that the next snapshot is version 12 or that additive serde
fields remove backup, downgrade, and future-version rejection obligations.

Nx resolved project listing was verified with read-only escalation after
sandbox-native loading failed. Relevant names are `core-rust`, `rust-daemon`,
`@animaos/sdk`, `@animaos/web`, and `workspace-dev`.
Resolved core-rust has build/test/lint; rust-daemon has dev/build/test/lint.
SDK and web have test/build/typecheck targets. No new target is necessary.
No tests were run during planning; task discovery is not validation evidence.

## Hermes source reuse

Installed reference: `C:/Users/leoca/AppData/Local/hermes/hermes-agent`.
Controller verified HEAD `f3ef9db68332063a3625661f3a9b259f60abdb4e`.
Read source files only; never read profiles, environment/config secrets,
keyring, user sessions, or live gateway state. Do not import or run Python.

Inspected `gateway/platform_registry.py`: metadata and adapter factories,
passive availability probes, optional active dependency installers, scoped
registrations, lazy loaders, and explicit source ownership.
Reuse the separation of metadata from activation and source ownership.
Do not port automatic installation, profile environment bridges, or Python
loaders into Rust. Discovery must have no installation or network effects.

Inspected `gateway/platforms/base.py` and `ADDING_A_PLATFORM.md`:
platform adapters are separate from shared authorization/routing behavior.
Hermes has community plugin registration through register(ctx), rather than
requiring every adapter to grow gateway branching. Port that architectural
idea into a Rust source contract, not its complete Python plugin ecosystem.

Inspected actual nested Telegram files: `adapter.py`, `update_admission.py`.
Admission receipts are bot-scoped and bounded; the module warns that durable
receipts do not promise exactly-once external effects. Preserve this distinction.
Use the existing Rust inbound journal for receipts, not a second Hermes ledger.
Do not blindly copy its cursor policy: Telegram IDs can reset after inactivity;
any cursor policy change is separate scope requiring concrete protocol evidence.

Inspected actual nested Discord files: `adapter.py`, `plugin.yaml`, `recovery.py`.
The manifest uses discord.py and includes voice/slash/thread/reaction behavior.
Recovery uses a profile-scoped SQLite ledger. These show the cost of full parity.
Reuse bounded source/message identities and recovery classifications later.
Do not copy voice, media transcoding, slash-command UI, role authorization,
Python SDK installation, or the SQLite recovery schema into M12.

Preserve `packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE`.
If any small source is copied or substantially adapted, retain the full MIT
notice, Nous Research copyright, source paths, revision, and adaptation note
beside the receiving Rust source. Behavior inspiration alone is recorded as
behavior reuse and must not be falsely described as copied implementation.
No wholesale adapter port is justified by the present Telegram pilot.

## Three modularity approaches

### A. Compile-in Rust trait and registry — selected

Put portable DTOs, descriptor validation, and transport ports in an existing
anima-core module. Register reviewed adapter implementations explicitly in the
Rust daemon. The registry is immutable after bootstrap for this milestone.
It maps stable protocol IDs to descriptors and host-owned transport instances.
Telegram is the only runtime implementation in the pilot.

Advantages: no dependencies or features; ordinary Rust compiler checks;
host-neutral engine API; focused community source contribution boundary;
reuses existing lifecycle and host credential storage.
Costs: community contributors build a host binary; source compatibility follows
crate releases; code runs with host privileges; there is no sandbox or stable ABI.
Optional means owner-activated and source-composable, not downloaded at runtime.
Do not add Cargo feature flags while the no-new-features constraint is active.

### B. HTTP sidecar extension boundary — deferred

Existing reqwest and Axum can transport HTTP, but do not establish an approved
plugin lifecycle. No inspected current Rust connector endpoint provides a
complete authenticated callback, credential lease, durable receipt, revocation,
manifest validation, restart, and operator installation contract for sidecars.
The ordinary owner API is not a safe plugin callback merely because it exists.

A later external-process contract could support separately installed community
adapters. It needs endpoint trust, credential separation, message authentication,
network destination policy, bounded callbacks, durable acknowledgement rules,
process ownership, and deployment/backup work. Do not manufacture this in M12.
No subprocess launch or process-supervision feature is currently authorized.

### C. Dynamic library or WASM modules — deferred

Neither a stable Rust plugin ABI nor a WASM runtime is installed and verified.
A loader requires dependency authorization, isolation decisions, signature and
version policy, resource limits, and lifecycle restart semantics. Rust trait
objects cannot be loaded as a stable native ABI by assertion. No loader ships.
Community compile-in adapters and future installable modules remain distinct.

## Portable contract

New proposed module: `anima-core/src/connectors/` with DTOs and registry rules.
No reqwest, Axum, DB drivers, daemon state, vault implementation, global runtime,
process handle, or provider credential enters the core contract.
Expose only the minimum types required by the verified Telegram pilot.
Do not create an unused universal protocol envelope with future fields.

Descriptor fields: stable protocol ID, contract revision, display name,
transport kind, supported operation labels, and source/provenance label.
Descriptor operations describe support; they grant no capability or approval.
Core validates duplicate IDs and supported revisions deterministically.
The host owns registration order and rejects collisions; adapters cannot replace
registered protocols silently. Discovery reads frozen metadata only.

Proposed transport port: bounded receive batch and bounded send request.
Telegram polling remains daemon scheduled and controlled. Do not put a generic
background runtime behind init/cleanup methods that can spawn unmanaged tasks.
A send request includes only destination/content selected by the host and an
attempt identity. A receive result supplies external source IDs and normalized
content; it cannot choose the agent, session, provider, tools, or run policy.
The port returns owned values and classified errors; never daemon lock guards.

Credential handling stays host-owned. The Telegram wrapper resolves the
existing vault token and calls existing transport without exposing it to core.
Do not persist a token in DTOs, metadata, logs, web events, or control-plane JSON.
Future untrusted extensions require a different credential mediation boundary.
Compile-in adapters are reviewed trusted code, with host OS privileges.

## Authority and lifecycle invariants

The host selects the active connector, agent, session/room, provider, and current
permission policy from authoritative state on every accepted inbound event.
External payload IDs are source identity only; adapter-supplied authority is
rejected or ignored according to a deterministic host validator.
Session binding must match the active connector and agent before run acceptance.
A registry descriptor does not allocate a session or resurrect an agent.

Use existing pairing. Unknown Telegram chats produce pairing candidates only.
Approved chat routing is revalidated before durable admission and before reply.
A token-valid bot does not authorize all human senders. Plugins cannot set a
trusted-inbound flag to bypass pairing, permissions, or owner approvals.
Model-generated tool arguments cannot supply connector grants or OAuth scope.

Durable inbound admission precedes model work. Preserve existing update IDs,
run idempotency keys, and terminal Received/Processing/Processed/Rejected/Stopped
semantics. Restart replay uses the existing host recovery path. Terminal stopped
work must never execute again or acquire a new reply through the adapter.
No automatic migration of Telegram journals into a generic journal in M12.

Outbox records remain durable before send. Preserve owner generation and attempt
claims, suppression, retry capacity, delivery retention, and history references.
Revalidate active connector/agent/chat authority before dispatch. Network calls
must not hold the control-plane transaction mutex or standard state locks.
Reconcile results only if the claim is still current. Stale results cannot mark
a replacement connector's row delivered or authorize another send.

Disable, delete, credential replacement, re-pair, agent deletion, and shutdown
fence new work. Cancellation cannot undo an external request already accepted.
Existing durable schemas may not encode all ambiguous outcomes explicitly.
Do not silently invent an Unknown enum variant or promise exactly-once delivery.
Document current behavior and seek the explicit durable-format ruling if new
uncertainty representation is necessary. Preserve mail's existing unknown state.

Locks follow control-plane transaction → state → registry/fanout/approval leaf.
No std lock spans await. Bound receive batches, pending work, response bytes,
timeouts, retries, and diagnostic strings using existing named constants where
possible. New constants need a rationale and boundary tests, not arbitrary parity.
Startup must restore only authorized active connectors; no scan of profiles or
automatic connection of every compiled adapter. Discovery does not contact APIs.

## Implementation sequence and file ownership

Work only after controller publishes the accepted plan and binding rulings.
Use disjoint task ownership; serialize Cargo compilation through controller.
No separate preflight audit. One batched runtime review and one final API/UI
review, one fix wave, then the required gates. Stop broadening after gates pass.

Task 1 — portable contract and registry.
Own new core connector files and narrow `anima-core/src/lib.rs` exports.
Keep the module framework-free. Define minimum descriptor and transport DTOs.
Validate revisions, identifiers, collisions, deterministic discovery, and bounds.
Fake transport proves no daemon/provider state is required by the contract.
Do not edit Cargo manifests or TS ModPlugin. Do not create another crate.

Task 2 — Telegram wrapper.
Own new `hosts/rust-daemon/src/connectors/adapters/` modules and narrow mod wiring.
Wrap current TelegramClient and credential store; preserve existing wire behavior.
Map current transport errors to bounded portable categories without losing
host-only uncertainty information. Keep raw HTTP details out of public DTOs.
Do not move reusable Rust definitions into this host adapter directory.

Task 3 — host registry and runtime integration.
Own new `connectors/registry.rs`, focused runtime hooks, and attachment wiring.
Existing runtime.rs receives wiring lines only. Keep lifecycle decisions in
host modules. Existing Telegram rows/API remain authoritative.
Use a fake adapter injection path for tests; no global mutable singleton.
No changes to agent_runs.rs beyond necessary wiring; no provider rewiring.

Task 4 — regression and recovery coverage.
Own new registry/adapters tests and targeted existing runtime test updates.
Retain startup, transactions, processing, worker, delivery, attachment, and stop
regressions. Test failures at persistence and transport boundaries separately.
Use deterministic barriers for races rather than sleeps or timing assertions.
No real Telegram, provider, keyring, gateway, daemon process, DB, or container.

Task 5 — contribution guide and accurate discovery wording.
Controller owns docs/superpowers publication, master plan, handover, and rulings.
Contributor guide explains source registration, ownership, test fixtures, MIT
provenance, build requirements, and limitations. Do not claim runtime installation.
SDK/web changes are conditional: only if the accepted discovery contract changes.
Keep existing connector routes and ConnectorsView behavior stable by default.
Do not build a marketplace, install button, dynamic configuration editor, or
unavailable Discord/Drive activation controls. No broad console redesign.

## Later bounded roadmap

Discord: explicitly select small REST chat behavior in a later accepted plan.
Existing reqwest can support configured-channel message reads and sends without
Gateway transport, but polling correctness, permissions, rate limits, cursors,
bot-self filtering, durable admission/outbox, and uncertainty still need design.
Full Discord Gateway requires websocket dependency authorization; Axum currently
has no ws feature and Tokio has no process feature. Neither is magically added.
Do not describe REST polling as live Gateway, slash commands, voice, or parity.
Require current official protocol checks during that implementation milestone.

Gmail: reuse current mail/OAuth services, immutable draft owner-send policy,
auth-generation fencing, redacted errors, and existing unknown send outcome.
A registry entry is not permission to relax send approvals or broaden scopes.
Migrate one service at a time with existing SDK/web contracts preserved.
Calendar: preserve on-demand behavior, account/calendar allowlists, owner
confirmation, callback revisions, and reauth state. No automatic calendar polling.
Drive: begin bounded metadata/list/read for explicitly authorized account/files.
No write/delete/share/export-all/watch/crawl or broad file-scope grant by default.
New callback/scope/credential policy requires its own accepted design and tests.

Installable community modules: a genuine process policy remains pending.
Future design must distinguish trusted compile-in code from untrusted extensions,
operator install approval from agent tool approval, and metadata discovery from
running code. Existing dependencies alone do not settle those decisions.
No external module deployment or loader implementation is included here.

Agent Markdown files: M11 now owns the confirmed AGENTS.md/SOUL.md Rust port.
Hermes `agent/context_file_sources.py` delegates discovery to prompt_builder,
reports loaded/shadowed/truncated/blocked/flagged sources, and treats owner's SOUL
separately. M12 consumes M11's accepted behavior without redefining precedence,
identity, prompt injection rules, memory, or long-running agent semantics.
If connector metadata later needs an instruction-file reference, use the host's
accepted M11 binding; no adapter-controlled prompt path enters this milestone.

## Validation policy and commands

Before implementation, inspect test setup and confirm no service-spawning suites.
Set PowerShell `$env:CARGO_INCREMENTAL='0'` and
`$env:ANIMA_SKIP_SERVICE_TESTS='1'` for SDK/full Nx validation.
Set NX_DAEMON=false, NX_ISOLATE_PLUGINS=false, NX_NO_CLOUD=true as required by
existing Windows rules. Native loading may need read-only/runtime escalation.
No install is authorized to repair Nx; report a genuine environment blocker.

Required Rust gates:
`bun x nx run core-rust:test --skipNxCache`
`bun x nx run rust-daemon:test --skipNxCache`
`bun x nx run rust-daemon:lint --skipNxCache`
Use controller-selected existing target directory; do not set a new one.
For executable locks, coordinate existing validation output policy first.
AGENTS fallback does not authorize unlimited duplicate compilation trees.
Check touched Rust formatting with cargo fmt --all --check before formatting
only owned sources. Every cargo invocation gets CARGO_INCREMENTAL=0.

Conditional SDK/web gates when touched:
`bun x nx run @animaos/sdk:test --skipNxCache`
`bun x nx run @animaos/sdk:typecheck --skipNxCache`
`bun x nx run @animaos/web:test --skipNxCache`
`bun x nx run @animaos/web:typecheck --skipNxCache`
`bun x nx run @animaos/web:build --skipNxCache`
Run relevant docs build if actual Astro docs sources change.
workspace-dev tests only if launcher changes; launcher changes are not proposed.
Do not run live E2E, deploy gateway service tests, provider requests, or installs.
Local fakes establish contract/recovery behavior, not live Telegram delivery.

## Acceptance scenarios

Each scenario below is a behavior specification, not a demand for a separate
mirror test. Combine scenarios in parameterized or existing focused tests when
that makes evidence stronger. Test actual acceptance/recovery/dispatch seams.
Prefer existing deterministic fixtures. No service may be started for these.

### S1 — Core: duplicate protocol

Fixture:
Two factories declare telegram.

Exercise:
Register the second factory.

Required evidence:
Reject before activation; preserve the first registration.

### S2 — Core: unknown revision

Fixture:
Descriptor uses unsupported revision.

Exercise:
Validate its descriptor.

Required evidence:
Return bounded compatibility error; never construct transport.

### S3 — Core: empty identity

Fixture:
Descriptor protocol is empty.

Exercise:
Validate before registration.

Required evidence:
Reject without mutating the registry.

### S4 — Core: unstable identity

Fixture:
Descriptor contains whitespace/control bytes.

Exercise:
Validate its stable key.

Required evidence:
Reject rather than normalize two distinct names into collision.

### S5 — Core: metadata ordering

Fixture:
Factories register in opposite order.

Exercise:
Read discovery twice.

Required evidence:
Produce deterministic protocol ordering.

### S6 — Core: passive discovery

Fixture:
Fake factory increments activation counter.

Exercise:
Request metadata inventory.

Required evidence:
Counter remains zero; no network or installation occurs.

### S7 — Core: empty registry

Fixture:
Host chooses no adapters.

Exercise:
Construct portable registry.

Required evidence:
Remain valid with an empty discovery list.

### S8 — Core: trusted contribution

Fixture:
Source adapter is registered explicitly.

Exercise:
Read its provenance.

Required evidence:
Show compile-in source ownership without an installable claim.

### S9 — Core: immutable registry

Fixture:
Bootstrap completes successfully.

Exercise:
Attempt later mutation through public API.

Required evidence:
No runtime mutation API is exposed in M12.

### S10 — Core: no daemon type

Fixture:
Core test imports portable contract only.

Exercise:
Implement a fake transport.

Required evidence:
Compile without daemon state or HTTP dependencies.

### S11 — Core: owned payload

Fixture:
Fake transport returns normalized text.

Exercise:
Drop transport before using result.

Required evidence:
Result owns its data; no borrowed lock or secret remains.

### S12 — Core: operation support

Fixture:
Descriptor lacks a send operation.

Exercise:
Host validates a send dispatch.

Required evidence:
Reject locally before invoking the adapter.

### S13 — Core: bounded metadata

Fixture:
Descriptor contains oversized display text.

Exercise:
Validate registration.

Required evidence:
Return validation failure without unbounded inventory output.

### S14 — Core: error redaction

Fixture:
Transport returns a secret-bearing internal detail.

Exercise:
Map to public failure.

Required evidence:
Expose only approved bounded category and safe diagnostic.

### S15 — Core: runtime absence

Fixture:
Adapter is compiled but owner has no connector.

Exercise:
Restore startup state.

Required evidence:
No worker is created solely because code exists.

### S16 — Telegram: wire preservation

Fixture:
Known Bot API fixture succeeds.

Exercise:
Call existing client through wrapper.

Required evidence:
Preserve destination, text, and normalized message identity.

### S17 — Telegram: batch preservation

Fixture:
Known update fixture has multiple updates.

Exercise:
Receive through wrapper.

Required evidence:
Preserve update IDs and existing ordering policy.

### S18 — Telegram: token isolation

Fixture:
Fake credential store holds sentinel token.

Exercise:
Read core descriptor and SDK response.

Required evidence:
Sentinel never appears in metadata or serialized response.

### S19 — Telegram: credential failure

Fixture:
Fake token store rejects lookup.

Exercise:
Host activates transport.

Required evidence:
Return current credential failure; no provider/model work.

### S20 — Telegram: pairing candidate

Fixture:
Unknown chat sends a valid bot update.

Exercise:
Host processes its source metadata.

Required evidence:
Create pairing candidate; do not start a companion run.

### S21 — Telegram: approved chat

Fixture:
Owner paired one chat.

Exercise:
Admit update from that chat.

Required evidence:
Bind run to authoritative connector agent/session.

### S22 — Telegram: wrong chat

Fixture:
Owner paired chat A and update names B.

Exercise:
Evaluate admission.

Required evidence:
Reject before model invocation or reply enqueue.

### S23 — Telegram: forged agent

Fixture:
Payload names another existing agent.

Exercise:
Validate host binding.

Required evidence:
Never route using payload agent identity.

### S24 — Telegram: forged session

Fixture:
Payload includes another room identifier.

Exercise:
Validate host binding.

Required evidence:
Ignore or reject it; derive room from connector state.

### S25 — Telegram: forged provider

Fixture:
Payload includes model/provider selection.

Exercise:
Prepare run request.

Required evidence:
Host current provider selection remains authoritative.

### S26 — Telegram: self message

Fixture:
Transport fixture represents unsupported/self content.

Exercise:
Normalize existing behavior.

Required evidence:
Do not introduce bot-loop admission through generic envelope.

### S27 — Telegram: malformed batch

Fixture:
Fake transport yields invalid identifier/content.

Exercise:
Validate before durable insert.

Required evidence:
Bound rejection; preserve existing cursor behavior.

### S28 — Telegram: attachment identity

Fixture:
Fixture has approved attachment metadata.

Exercise:
Use existing attachment ingestion seam.

Required evidence:
Preserve source ownership and existing content limits.

### S29 — Telegram: oversized content

Fixture:
Fixture exceeds existing inbound limit.

Exercise:
Normalize and admit.

Required evidence:
Preserve established bounded behavior without memory growth.

### S30 — Telegram: poll retry

Fixture:
Fake receives transient transport failure then success.

Exercise:
Drive bounded worker steps.

Required evidence:
Use current backoff; do not spawn duplicate pollers.

### S31 — Inbound: first receipt

Fixture:
Durable store is empty.

Exercise:
Admit one update.

Required evidence:
Persist Received before asking model to run.

### S32 — Inbound: duplicate receipt

Fixture:
Existing row shares connector/update identity.

Exercise:
Admit same update again.

Required evidence:
Reuse journal outcome; never enqueue a second run.

### S33 — Inbound: cross connector ID

Fixture:
Two connectors receive equal external update IDs.

Exercise:
Admit both valid updates.

Required evidence:
Keep identities scoped; no cross-connector deduplication.

### S34 — Inbound: processing restart

Fixture:
Processing row exists at simulated restart.

Exercise:
Run existing restoration flow.

Required evidence:
Recover under established policy and idempotency identity.

### S35 — Inbound: processed restart

Fixture:
Processed row exists at restart.

Exercise:
Restore active connector.

Required evidence:
Do not rerun terminal work.

### S36 — Inbound: rejected restart

Fixture:
Rejected row exists at restart.

Exercise:
Restore journal.

Required evidence:
Keep terminal rejection; never reclassify it as Received.

### S37 — Inbound: stopped restart

Fixture:
Stopped row exists with suppressed reply.

Exercise:
Restore state.

Required evidence:
Do not rerun or enqueue its reply.

### S38 — Inbound: durability failure

Fixture:
Fake store fails Received commit.

Exercise:
Try admitting update.

Required evidence:
No model work starts and no acknowledgement falsely claims durability.

### S39 — Inbound: agent deletion

Fixture:
Receipt exists then authoritative agent is deleted.

Exercise:
Attempt processing.

Required evidence:
Fail closed without creating a replacement agent.

### S40 — Inbound: disable before run

Fixture:
Receipt accepted then connector disabled.

Exercise:
Release processing barrier.

Required evidence:
Generation/current-active validation prevents new work.

### S41 — Inbound: re-pair before run

Fixture:
Receipt was from old approved chat.

Exercise:
Replace pairing then process.

Required evidence:
Old source does not inherit new pairing authority.

### S42 — Inbound: idempotency preservation

Fixture:
Existing run key is associated with inbound row.

Exercise:
Pass through adapter seam.

Required evidence:
Retain same accepted-run replay behavior.

### S43 — Inbound: host policy refresh

Fixture:
Tool grant revoked after receipt but before execution.

Exercise:
Prepare actual run.

Required evidence:
Use current host policy; adapter cannot restore old grants.

### S44 — Inbound: bounded queue

Fixture:
Existing pending limit is reached.

Exercise:
Admit another update.

Required evidence:
Use established backpressure without unmanaged task fanout.

### S45 — Inbound: no prompt authority

Fixture:
Payload carries instruction-file path.

Exercise:
Prepare model context.

Required evidence:
Do not let connector choose AGENTS/SOUL files.

### S46 — Outbox: durable before send

Fixture:
Reply generation produces text.

Exercise:
Commit pending reply then dispatch.

Required evidence:
Send occurs only after durable outbox admission.

### S47 — Outbox: claim exclusivity

Fixture:
Two workers contend for one pending record.

Exercise:
Use deterministic claim barrier.

Required evidence:
Only one current attempt owns dispatch.

### S48 — Outbox: no lock across HTTP

Fixture:
Fake send blocks on barrier.

Exercise:
Try independent control-plane mutation.

Required evidence:
Mutation progresses while network is pending.

### S49 — Outbox: late successful result

Fixture:
Send starts then connector generation changes.

Exercise:
Return transport success.

Required evidence:
Do not settle a replacement generation with stale result.

### S50 — Outbox: disable during send

Fixture:
Request already in flight then owner disables.

Exercise:
Complete external request.

Required evidence:
Document effect may have happened; forbid new unauthorized sends.

### S51 — Outbox: delete during send

Fixture:
Request in flight then connector is deleted.

Exercise:
Reconcile late result.

Required evidence:
No resurrection or replacement worker activation.

### S52 — Outbox: token replacement

Fixture:
Old transport request races credential replacement.

Exercise:
Finish old attempt.

Required evidence:
New credential generation cannot inherit old dispatch authority.

### S53 — Outbox: stop before dispatch

Fixture:
Run is stopped with pending reply.

Exercise:
Attempt delivery.

Required evidence:
Suppress according to existing host stop semantics.

### S54 — Outbox: suppressed restart

Fixture:
Suppressed record is persisted.

Exercise:
Restart fake runtime.

Required evidence:
Never send it or count it as awaiting delivery.

### S55 — Outbox: successful delivery

Fixture:
Fake send returns message identity.

Exercise:
Reconcile current claim.

Required evidence:
Record current Delivered outcome and release active claim.

### S56 — Outbox: retryable failure

Fixture:
Fake send fails under current retry policy.

Exercise:
Run next worker step.

Required evidence:
Use bounded retry without creating another outbox row.

### S57 — Outbox: ambiguous timeout

Fixture:
Provider may have accepted before timeout.

Exercise:
Classify transport outcome.

Required evidence:
Never assert exactly-once; preserve current documented uncertainty policy.

### S58 — Outbox: persistence after send

Fixture:
External success occurs then durable commit fails.

Exercise:
Restore fake state.

Required evidence:
Report uncertainty and obey controller ruling; do not invent schema.

### S59 — Outbox: history replay

Fixture:
Reply has left hot tail but history remains.

Exercise:
Owner replays same idempotency key.

Required evidence:
Return original answer/delivery relation through existing history flow.

### S60 — Outbox: retention

Fixture:
Delivered rows exceed existing retention cap.

Exercise:
Prune host journal.

Required evidence:
Preserve pending/failed/suppressed semantics and bounded delivered retention.

### S61 — Outbox: capacity

Fixture:
Undelivered cap is reached.

Exercise:
Accept owner send attempt.

Required evidence:
Return existing backpressure before unnecessary model work.

### S62 — Outbox: shutdown

Fixture:
Daemon begins graceful shutdown while work exists.

Exercise:
Signal producer/dispatch stop.

Required evidence:
No new poll work starts; accepted drain follows current policy.

### S63 — Snapshot: version eleven

Fixture:
Saved version 11 Telegram fixture exists.

Exercise:
Load via current restoration.

Required evidence:
Existing schema remains compatible after nonpersistent trait extraction.

### S64 — Snapshot: unknown future

Fixture:
Fixture version exceeds accepted current version.

Exercise:
Load snapshot.

Required evidence:
Reject as currently required; no blind field stripping.

### S65 — Snapshot: missing legacy fields

Fixture:
Supported older fixture omits defaulted fields.

Exercise:
Run existing migration path.

Required evidence:
Preserve current defaults without treating migration as adapter privilege.

### S66 — Snapshot: new field proposal

Fixture:
Implementation discovers required durable generation field.

Exercise:
Prepare change proposal.

Required evidence:
Wait for controller schema/backup ruling before source mutation.

### S67 — Snapshot: credential exclusion

Fixture:
Snapshot contains active connector metadata.

Exercise:
Serialize and inspect fixture.

Required evidence:
Never include token, client secret, or refresh token.

### S68 — API: existing route

Fixture:
SDK uses current Telegram endpoint.

Exercise:
Serve in-process fixture router.

Required evidence:
Response and owner auth semantics remain compatible.

### S69 — API: inventory claim

Fixture:
Discovery returns compile-in Telegram entry.

Exercise:
Render SDK metadata fixture.

Required evidence:
Do not state marketplace installation or dynamic module support.

### S70 — API: unknown connector

Fixture:
Owner addresses absent connector ID.

Exercise:
Run authenticated route fixture.

Required evidence:
Return existing bounded error; no implicit factory activation.

### S71 — API: approval preservation

Fixture:
Mail draft exists alongside Telegram registry.

Exercise:
Execute mail owner-send regression.

Required evidence:
Registry does not grant send or weaken approval.

### S72 — API: calendar preservation

Fixture:
Pending calendar write exists.

Exercise:
Run current confirmation regression.

Required evidence:
Same host confirmation and account scoping apply.

### S73 — API: OAuth preservation

Fixture:
Google app config revision changes.

Exercise:
Resolve existing callback fixture.

Required evidence:
Current revision/allowlist fencing remains intact.

### S74 — API: secret-free event

Fixture:
Connector failure emits status event.

Exercise:
Inspect serialized fixture.

Required evidence:
Only bounded nonsecret identity/status fields escape.

### S75 — Community: minimal source adapter

Fixture:
Contributor implements portable fake contract.

Exercise:
Register in test host assembly.

Required evidence:
Compiler verifies contract; no install tooling is needed.

### S76 — Community: provenance copy

Fixture:
Small Hermes-derived helper is adapted.

Exercise:
Review receiving source notice.

Required evidence:
Retain MIT copyright, revision, source path, and adaptation statement.

### S77 — Community: unsupported loader

Fixture:
Contributor asks for runtime Python/WASM loading.

Exercise:
Read contribution guide.

Required evidence:
State deferred process/dependency policy without promising support.

## Exactly five risks requiring controller binding rulings

1. **Modularity promise versus trust boundary.**
   Compile-in trusted Rust code offers a source contribution API, not a sandbox
   or installable ABI. A misleading optional-module claim creates architecture
   debt and grants unjustified expectations to operators and contributors.
   Proposed ruling: select approach A and expressly defer B/C until authorized.
   Proposed ruling (binding decision appended below): controller must accept scope — cost if wrong: unsafe loader
   expectations or a second runtime before its authority contract exists.

2. **Extraction regression in authority and lifecycle ownership.**
   A portable adapter must not choose agent/session/provider or bypass pairing,
   tool grants, approvals, current-generation checks, and durable host admission.
   Proposed ruling: host lifecycle remains authoritative; pilot is Telegram
   transport extraction only; retain existing routes and journals.
   Proposed ruling (binding decision appended below): controller must bind ownership — cost if wrong: cross-agent
   routing, stale revocation, duplicate runs, or an approval bypass.

3. **Durability and ambiguous external effects.**
   Existing rows may not encode every late-result or uncertain-send case.
   A refactor must preserve honest classifications and never imply exactly-once.
   Proposed ruling: no new durable fields by default; if needed, stop that seam
   for explicit schema version/upgrade/downgrade/backup decision. Future version
   12 is a possibility for a reviewed change, not a preapproved number.
   Proposed ruling (binding decision appended below): controller owns format decision — cost if wrong: replayed
   side effects, silent lost replies, or unsafe snapshot rollback.

4. **Protocol/dependency scope and roadmap feasibility.**
   Discord Gateway lacks websocket support in current manifests; subprocess
   and WASM loading also lack the required authorized foundation. Drive lacks
   the inspected callback/scopes. Existing HTTP libraries are not full adapters.
   Proposed ruling: foundation+Telegram only; later REST Discord, existing
   Gmail/Calendar migration, and limited Drive reads need separate accepted plans.
   Proposed ruling (binding decision appended below): controller binds roadmap labels — cost if wrong: hidden
   dependency additions, incomplete parity, or expanded account access.

5. **Hermes reuse and M11 instruction-file coupling.**
   Hermes registry is Python and its adapters rely on SDKs and profile rules.
   Copying whole integrations can import unrelated authority, memory, prompts,
   automatic installers, and source licensing omissions into the Rust engine.
   Proposed ruling: focused behavior/pure-helper reuse with MIT provenance;
   M11 owns AGENTS/SOUL semantics and must merge before this branch exists.
   Proposed ruling (binding decision appended below): controller binds reuse boundary — cost if wrong: a second
   instruction precedence model, secret leakage, or unreviewed broad parity.

## Controller publication and finish criteria

The controller handles master plan, handover, accepted rulings, and publication.
This published preparation must be reverified against merged M11 before implementation.
Workers never stage or commit; controller uses explicit owned paths and unsigned
commits under the owner's current authorization. Old signing instructions do
not override that authorization. Never stage nx.json, anima.yaml, or unrelated
local edits. No stash/reset/restore/worktree/branch changes by workers.

Implementation completion requires the current-tree relevant gates to pass,
a bounded review/fix cycle to finish, and accurate documentation of limitations.
Report commands and pass counts, changed paths, unresolved limitations, and
whether Hermes code was copied or behavior only reused. Do not substitute
mock evidence for live provider readiness or hosted deployment evidence.

Preparation completion means this draft exists, reflects inspected source,
contains five proposed risks with binding rulings appended, and proposes no source implementation.
Preparation adds documentation only; no connector source, dependency metadata or service state changes are introduced. Reverify the implementation plan against merged M11 before dispatching workers.

## Controller rulings on the risks (binding)

1. Ruling: Ship a trusted compile-in Rust connector trait/registry foundation and Telegram pilot first — current no-dependency/features rule and available runtime ports support source-level modularity, not a genuine dynamic loader — cost if wrong: users rebuild reviewed community integrations until an installable-module architecture is explicitly approved; never claim sandboxing or drop-in installation.
2. Ruling: Preserve host-owned pairing, agent/session binding, permissions, approval, credential generations, durable admission and settlement — adapter events are input data, not authority grants, and the M10 lifecycle already solves important races — cost if wrong: extraction can break routing or revocation; reuse existing gated regressions and make the pilot behavior-equivalent.
3. Ruling: Default to no durable schema change; any demonstrated new persistent field requires a concrete version/backup/upgrade/downgrade decision before implementation — ephemeral registry metadata cannot justify a speculative snapshot bump — cost if wrong: preserve existing at-least-once and uncertainty classifications rather than fabricate exactly-once or silently replay effects.
4. Ruling: Keep roadmap bounded to Telegram, Discord, Gmail, Calendar and Drive with separately verified protocols/scopes — current manifests lack Discord Gateway/process/WASM dependencies and Drive consent surfaces, and the owner does not want broad integration parity — cost if wrong: delivery is phased; never hide dependency additions or promise unverified REST/Gateway equivalence. Existing Gmail/Calendar services are reused; Drive starts with selected read/search access.
5. Ruling: M11 owns Markdown identity/instruction loading and must merge before M12 implementation branch creation; focused Hermes code/behavior reuse retains MIT provenance — avoid duplicate precedence/memory/authority models and copying Python orchestration wholesale — cost if wrong: integration starts later but has one established Rust prompt/lifecycle authority. Prefer current regression reuse over mechanically adding every listed scenario as a new test.

## Controller interface freeze after M11 merge (binding)

Ruling: Use a portable async transport trait with associated identity, inbound-batch, receipt and classified-error types — Telegram has vetted host-native metadata and retry/uncertainty semantics that must survive this pilot; a fabricated universal JSON envelope adds unnecessary conversion and authority fields — cost if wrong: future adapters provide their own typed payload validators and a later shared normalization contract; source modularity is real but not a dynamic binary ABI.

Core names: ConnectorDescriptor, ConnectorOperation (Identify/Receive/Send), ConnectorTransportKind (Polling), ConnectorRegistry, ConnectorReceiveRequest {cursor:String}, ConnectorSendRequest {destination:String,text:String,attempt_id:String}, ConnectorErrorKind and ConnectorFailure (standard Error+Send+Sync with kind()). ConnectorTransport is async_trait Send+Sync; associated Identity, InboundBatch, SendReceipt:Send and Error:ConnectorFailure. Methods identify(), receive(request), send(request). Immutable core registry holds bounded descriptors in stable protocol-ID order; no registration executes transport code.

Ruling: Bind Telegram credentials privately inside a short-lived host adapter at the existing verified-token dispatch point — the host already loads/fences the precise token before polling or claiming delivery; a second vault lookup could substitute a newer credential after the claim — cost if wrong: the adapter is host-created per dispatch and cannot be persisted/serialized; reviewed compile-in code retains host privileges. Core DTOs and descriptors contain no token, agent/session, provider/model, permissions or instruction paths.

Host registry owns registered native transport plus core metadata. Preserve ConnectorManager::new(...Arc<dyn TelegramTransport>) injection and its existing fake tests; this constructor builds the immutable Telegram pilot registry. Every identify/receive/send (including pairing notice) dispatch goes through the registered adapter/portable trait, using the already captured token. Core-associated Telegram error is the existing error type with a redacted portable category; retain native status/timeout semantics. Delivery attempt_id uses the existing durable outbound/claim identity; notices use an explicit host-selected non-durable identity. No new idempotency promise or lifecycle/schema change.

Ruling: Freeze registration at bootstrap with trusted source contributions, no public discovery/API/schema changes in M12 — existing Telegram owner surfaces already provide activation; capabilities must keep installable plugins unavailable — cost if wrong: community contribution requires reviewed host registration/rebuild until a later lifecycle/transport integration is implemented. No SDK/web production changes are needed unless concrete drift is found; final API/UI review verifies preserved contracts and docs.

Ruling: Run required rust-daemon:test once with its core-rust:test dependency, plus lint/format — resolved Nx already runs the core task; separate standalone full core execution duplicates work — cost if wrong: inspect both task outcomes/counts instead of assume dependency success. Serialize cargo; no new target directories, dependencies/features, real services/providers/databases/containers, GPG prompts, or automatic PR merge. Existing unsigned per-command commits and explicit paths persist.

Bounds: registry/descriptor/request constants and meaningful tests are owned by the portable-contract implementer; preserve valid Telegram cursor/send behavior and existing splitting. New validation errors must be before any transport effect, and no raw payload/credential appears in generic public errors. Document any new upper bound and its cost explicitly in the ledger before wiring.

## M12 execution outcome (2026-10-12)

Core descriptor catalog and `ConnectorTransport` are implemented without new
dependencies. The daemon routes Telegram identify, poll, durable delivery and
pairing notices through the registered credential-borrowing adapter. Native
payload/error/chunking behavior and existing host lifecycle fences are retained.
Reviewed source contributions and their host privileges/rebuild requirements are
documented in `hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md`.

One daemon batch review and one final API/SDK/web/docs review found no actionable
issues. The single fix wave reconciled host import aliases and formatting.
Current-tree Nx Rust gate: **2,230 passed, 8 ignored**, comprising core dependency
630 and host 1,600. Ignored cases: five Postgres, one live subscription smoke, one
model download and one inventory printer. All six new consolidated tests pass.
Nx Rust lint passes. HTTP/SDK/web, durable schema v11 and manifests are unchanged;
conditional TypeScript/Astro gates were not repeated. No services, databases,
containers, providers, user profiles or secrets were accessed.

Ruling: Metadata bounds apply to fields, not a fixed catalog count — reviewed
compile-in source registration is passive and not external discovery — cost if
wrong: a future external catalog needs its own discovery and resource bounds.
Ruling: Portable text is capped at 1,048,576 Unicode scalars, with cursor128,
destination256 and attempt128 — bound requests before effects while retaining
native splitting — cost if wrong: enormous output fails closed with static native
`InvalidRequestBounds`; valid text whitespace/control handling remains native.
Ruling: The host test target's core dependency run satisfies both required Rust
test suites — avoid duplicate compilation while inspecting both outcomes — cost
if wrong: failed dependency or host outcomes must block publication independently.

Publication remains on the M12 branch against main. M12 merge needs a new explicit
owner instruction; next connector work begins from main after that merge.

Anima/AnimaOS is our product naming. Upstream names appear only in attribution or source-reference evidence. The 77 acceptance scenarios guide consolidated behavioral coverage and regression reuse, not 77 mandatory new tests.
