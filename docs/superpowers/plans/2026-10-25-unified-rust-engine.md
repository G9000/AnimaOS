# Unified Rust Engine Implementation Plan: Single-Player First

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task after implementation is explicitly requested. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a shared durable Rust engine with persistent identity and memory required for both the assistant and resident game agents; deliver a small single-player ecosystem first. Filesystem workspaces and coding tools remain optional, and only game residents require world membership. Preserve boundaries for future multiplayer without implementing it.

**Architecture:** Converge on `anima-core::DurableAgentEngine`, accessed through an explicit durable harness facade and a reusable SQLite `ExecutionStore`. Add durable `MemoryPort` storage and identity continuity as required milestones after execution; reuse existing recall and relationship machinery. Keep current consumers working until execution and memory parity pass. Hosts retain application permissions, secrets, domain state and transport.

**Tech Stack:** Existing Rust core/harness/model adapters; existing SQLite driver family in a new adapter crate; current Bun task wrappers; deterministic Rust tests and temporary process fixtures. No new server framework or game engine.

**Status:** Planning only. All implementation tasks below are unstarted. Baseline `37f4e8d0`; recheck source and Git status before execution. Do not commit, merge, push, restart services or alter real saves under this planning authorization.

**Design:** [Unified engine design](../specs/2026-10-25-unified-rust-engine-design.md).

## Milestones and sequence

| Milestone | Tasks | Deliverable |
| --- | --- | --- |
| A: durable foundation | 1-4 | Filesystem-independent engine contracts and a crash-tested SQLite adapter |
| B: persistent agent foundation | 5-8 | Durable facade, stable identities and required durable memory/relationship integration |
| C: two persistent consumers | 9-11 | Shared acceptance suite, assistant and village using the same execution and memory components |
| D: bounded single-player ecosystem | 12-13 | Tavi/Mira interaction, persistent consequences, recorded parity and limited cutover decision |

Complete these sequentially where dependencies apply. Code commits are review checkpoints only when separately authorized. A passing milestone does not authorize production cutover.

Milestone A is intermediate execution progress, not a product with sufficient memory. Milestones B-D are required for this single-player target. Multiplayer infrastructure is outside this plan; the compatibility requirements below are acceptance checks, not promises of multiplayer scale.

## Repository ownership

Paths below are repository-relative. New paths are proposals; inspect existing names before creating them.

| Path | Responsibility |
| --- | --- |
| `packages/core-rust/crates/anima-core/src/{engine.rs,definition.rs,policy.rs,execution/}` | Portable execution and authority contracts; no driver, HTTP or game dependencies |
| `packages/core-rust/crates/anima-execution-store-sqlite/` (new) | SQLite connection custody, atomic storage, migrations, protection configuration and process recovery tests |
| `packages/core-rust/crates/anima-core/src/identity.rs` (new, minimal contract) | Stable individual identity distinct from a definition version or session |
| `packages/core-rust/crates/anima-memory-store-sqlite/` (new) | Durable canonical memory records, revisions, source-effect receipts and relationship evidence; no execution-loop ownership |
| `packages/core-rust/crates/anima-memory/src/{access.rs,projection.rs,identity.rs}` (new) | Authorized memory operations, idempotent event projection and identity bindings; reuse current recall/relationship algorithms |
| `packages/core-rust/crates/anima-harness/src/durable/` (new) | Public durable facade, definition/context conversion, control and event adaptation; delegates to the core loop |
| `packages/core-rust/crates/anima-harness/src/conformance.rs` (new, test-support feature) | Shared host acceptance scenarios using host-provided fixtures |
| `hosts/rust-daemon/src/engine_bridge/` (new) | Existing owner/agent identities, policy/receipt mapping and opt-in dispatch integration |
| `poc/village/host/src/engine_bridge.rs` (new) | Save/request-to-run mapping and reconciliation with game transactions |
| `poc/village/host/src/agent_activity.rs` (new) | Bounded single-player event activation, presence expiry and interaction receipts |
| `poc/village/crates/anima-village/src/npc.rs` | NPC prompt/domain contract; live execution can be supplied by the host |

Do not introduce an umbrella `anima-agents` crate before proving this composition. Storage adapters are libraries, so they belong under `packages`, not `hosts`. Keep execution snapshots, memory records and game state logically separate even when a host stores them on one device.

## Single-player scope and future multiplayer boundaries

- Ship one player and one village with the existing Tavi/Mira cast. Require their memories, relationship observations and world consequences to survive a real restart.
- Use explicit owner/player/world/agent/session/event IDs; never rely on a global current player or infer authority from an ID supplied by the client/model. An assistant outside a world has no artificial world membership.
- Keep the host authoritative for world actions, version conflicts and command receipts. Test two independent worlds/owners as fixtures without adding a second connected player, login flow or multiplayer UI.
- SQLite is the first local adapter. Domain and host integration rely on portable interfaces and conformance tests. The bounded execution-snapshot adapter must not become the schema for all growing memories/world state. Future PostgreSQL storage needs an explicit adapter, data migration and concurrency validation.
- Do not build matchmaking, replication, distributed queues, shared-world deployment or load scaling. No claim of multiplayer readiness follows from these interfaces alone.

## Task 1: Lock the compatibility contract and workspace-free profiles

**Files:**
- Inspect: `packages/core-rust/crates/anima-core/src/{agent.rs,definition.rs,policy.rs,engine.rs,capability/manifest.rs}`.
- Inspect: `packages/core-rust/crates/anima-harness/src/{config.rs,harness.rs}`.
- Create: `packages/core-rust/crates/anima-core/tests/optional_environment_contract.rs`.
- Modify only when required by failing cases: `src/policy.rs`, `src/definition.rs`, `src/engine.rs` and matching existing contract tests.

- [ ] Record a behavior matrix comparing current runtime/harness with the durable loop: persona/system rendering; provider parameters/fallback; tools/schema validation; streaming; stop/steering; approval/revalidation; finite shared budgets; transcript restoration; checkpoint/effect recovery. Mark supported, missing, and profile-excluded with evidence. No profile is marked migrated from an enum or interface alone.
- [ ] Add tests named `dialogue_requires_no_workspace`, `non_filesystem_capability_requires_no_directory`, `filesystem_capability_requires_explicit_grant`, and `application_scope_cannot_alias_another_owners_grant`. Start from existing definition/policy fixtures, remove all workspace-directory setup, use empty capability lists for dialogue, and assert denied file execution never reaches an executor.
- [ ] Define host-supplied opaque application scopes at the durable facade. Preserve the current policy `workspace_id` wire format through an explicit adapter, retain old workspace IDs, persist new collision-checked scope IDs, and test restart-stable mapping. Do not loosen matching or grant checks to accept empty strings. Storage paths never become model-visible context.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-core --test optional_environment_contract`; fix only boundary behavior exposed by the failures, then run definition and policy contract suites. Expected: all cases pass; conversation executes without any filesystem grant.
- [ ] Record unresolved parity items as explicit blockers for their profiles in this plan's acceptance table. This is the prerequisite to the adapter/facade work, not an excuse for removing existing assistant behavior.

## Task 2: Add an isolated SQLite adapter package and owned storage boundary

**Files:**
- Create: `packages/core-rust/crates/anima-execution-store-sqlite/{Cargo.toml,README.md,src/lib.rs,src/worker.rs,src/schema.rs,src/protection.rs}`.
- Create: `packages/core-rust/crates/anima-execution-store-sqlite/tests/open.rs`.
- Modify: root `Cargo.toml`, `Cargo.lock`, `packages/core-rust/package.json`.
- Reuse contracts: `anima-core/src/execution/{store.rs,memory_store.rs}`.

- [ ] Register only the new library in the root Cargo workspace and existing core build/test/lint package lists. Match current compatible dependency versions. Do not add the PoC to root workspaces or change root dev startup.
- [ ] Add failing cases for create/reopen, unsupported schema version, corrupt snapshot, incorrect/missing protection key, failed migration rollback, and `host_data_path_is_not_agent_workspace`.
- [ ] Implement versioned adapter metadata: schema version, committed generation, sealed authority snapshot, clock floor and only necessary scalar indexes. Inject protection configuration and clock; never mint a replacement key silently on reopen or persist credentials with transcripts. Document integrity versus encryption and payload-retention choices.
- [ ] Give a bounded storage worker ownership of database work. A caller's cancellation must not drop a partially admitted write; complete/rollback the transaction and make the result discoverable. Bound queued work and shutdown draining. Keep model/tool/network work outside the worker and transaction.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-execution-store-sqlite --test open`. Expected: creation/reopen and failure behavior pass with temporary storage, without reading repository/user configuration.

## Task 3: Implement the complete ExecutionStore contract atomically

**Files:**
- Create: adapter `src/{store.rs,transaction.rs,projection.rs}` and `tests/{conformance.rs,atomicity.rs}`.
- Inspect: `anima-core/src/execution/memory_store.rs` public snapshot/projection methods; `anima-core/tests/execution_store_external_adapter.rs`.
- Modify core only if a missing public adapter seam is demonstrated by a contract test; never expose private state serialization as an undocumented adapter API.

- [ ] Build a test factory that creates a distinct temporary database and invokes `assert_execution_store_conformance`. Include `retry_run`; its default rejection must not silently stand in for durable retry support.
- [ ] For each write: acquire the database transaction, load the latest sealed state/generation, restore a fresh reference store, apply the public operation, export candidate snapshot/projection, commit atomically, then return. Reads observe committed state. Do not wrap a long-lived mutable reference store and save it after already exposing success.
- [ ] Cover all operations: policy/grant updates and reads; create/retry run; lease acquire/renew; execution commit; run/checkpoint/step/attempt/result/event reads. Preserve exact payload identity, owner scoping, policy revisions, counted-grant consumption, serial-session claims and opaque pagination cursors.
- [ ] Add two-connection concurrency tests for stale versions, competing claims, revoked grants, concurrent retry and expired leases. Inject a failure between candidate creation and durable commit; subsequent reads must expose only the prior generation. Test request cancellation and worker shutdown. Clock rollback cannot resurrect a fence.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-execution-store-sqlite --test conformance` and `... --test atomicity`. Expected: public conformance and race/failure cases pass against real SQLite, not an in-memory-only substitute.

The `...` command abbreviations in this plan mean repeat the same full prefix `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-execution-store-sqlite` with the stated test target; they are not literal commands.

## Task 4: Prove real restart and effect recovery before host migration

**Files:**
- Create: adapter `tests/{restart.rs,engine_recovery.rs,fixtures/execution_peer.rs}` and an explicit Cargo example target for the child fixture.
- Reuse: `anima-core/tests/durable_engine_contract.rs`, public engine boundaries and local scripted model fixtures.

- [ ] Build a parent-controlled fixture with named handshakes and a temporary database. Kill the child process at model checkpoint, dispatch-before-effect, effect-completed-before-result, result-committed-before-response, and terminal-commit boundaries. Do not use sleep timing as the correctness oracle.
- [ ] Place the test side effect in a separate durable fixture ledger keyed by logical invocation ID. For keyed-idempotent/reconcilable capabilities, assert one physical effect and correct recovered result. For uncertain non-idempotent capabilities, assert a review pause and zero automatic replay.
- [ ] Test fresh-process reopen with old fence holders, exact provider transcript correlation, stable request/run mapping, exhausted budget, approval revocation and denied cross-owner reads. Replay events from the last acknowledged durable cursor; distinguish ephemeral token deltas from durable semantic events.
- [ ] Exercise snapshot capacity failure and record bounded latency/size measurements; no silent trimming of recovery evidence. Store limits are delivery limits, not multiplayer capacity claims.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-execution-store-sqlite --test restart` and the same prefix with `--test engine_recovery`. Expected: actual process-death tests pass. Milestone A is incomplete if only snapshot round-trips pass.

## Task 5: Add a durable harness facade without another execution loop

**Files:**
- Create: `anima-harness/src/durable/{mod.rs,config.rs,definitions.rs,controls.rs,events.rs}` and `anima-harness/tests/durable.rs`.
- Modify: `anima-harness/src/lib.rs`, `anima-harness/Cargo.toml`, `anima-harness/README.md`.
- Modify only for parity gaps: `anima-core/src/{engine.rs,definition.rs,execution/state.rs}` and corresponding tests.

- [ ] Specify a facade accepting an injected `ExecutionStore`, definition resolver, model adapter, policy/capability adapters, clock and event sink. Admit stable owner/agent/session/run identities before invoking `DurableAgentEngine`. No required cwd, filesystem workspace or world object. Kernel-only execution fixtures can omit memory; assistant/resident-character profiles must bind required memory in Task 8 and fail admission explicitly if it is unavailable. No mandatory SQLite dependency in the facade's production API.
- [ ] Add failing tests for empty-tool dialogue, preserved persona/system rendering, stop/pause, rejected capability before effect, shared finite allowance, and restore without renewed allowance. Treat proposed facade names as new API, not pre-existing methods.
- [ ] Map actual existing capabilities: rendered personality, model settings and conversation input; immutable definition/manifest versions; exact policy binding; model events and terminal outcomes. Freeze accepted instructions per run and revalidate live authority before effects. A model call or tool operation must be accounted for by one authority, not separately reset facade and host budgets.
- [ ] Close the finite-execution gaps before exposing the new path: attempts, retries, tokens, tools, in-flight work and deadlines must have enforceable shared accounting. A response timeout or turn-count cap alone does not satisfy this gate. Retain custody of started effects through Stop and reconcile unknown outcomes.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-harness --test durable` and `bun run --filter core-rust test`. Expected: new behavior and legacy harness tests pass. Existing `Harness::run/chat` callers remain unchanged until explicitly migrated.

## Task 6: Establish persistent individual identity and memory authority

**Files:**
- Create: `packages/core-rust/crates/anima-core/src/identity.rs`, `tests/identity_contract.rs`; export from the existing `src/lib.rs`.
- Create: `packages/core-rust/crates/anima-memory/src/{identity.rs,access.rs}`, `tests/identity_access.rs`; update that crate's `src/lib.rs`.
- Reuse: core `src/memory.rs`, `tests/knowledge_contract.rs`, existing agent/definition contracts and legacy memory mappings.

- [ ] Add tests for an individual retaining its ID across new sessions, changed model/definition, and restart; a new world's NPC receiving a separate identity; an assistant not requiring any world ID; and no filesystem access arising from identity registration.
- [ ] Define the smallest stable owner/individual/definition binding needed by both consumers. Preserve assistant IDs and persist explicit save/world/NPC mappings. Record optional player/world associations separately from individual identity; do not make names or model-generated IDs authoritative.
- [ ] Define trusted, scoped writers around `MemoryPort::write/revise`, which currently lack an access-context argument. Authorize ownership, scope and correction targets before every mutation; bind read contexts server-side. Reject cross-owner/world reads and writes even when the caller knows a record ID. Test derived indexes and relationship endpoints under the same boundary.
- [ ] Keep private experiences scoped to their observer. Shared sessions/world facts require explicit authorized scope membership. Mapping existing `MemoryScope::Workspace` to a non-filesystem space must be explicit and collision-safe; its name never grants directory access. No global shared memory across the assistant and game.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-core --test identity_contract` and the same task wrapper with `cargo test -p anima-memory --test identity_access`. Expected: continuity and access tests pass independently of SQLite, a game world and agent files.

## Task 7: Implement required durable memory and relationship storage

**Files:**
- Create: `packages/core-rust/crates/anima-memory-store-sqlite/{Cargo.toml,README.md,src/lib.rs,src/store.rs,src/schema.rs,src/relationships.rs}` and `tests/{memory_contract.rs,restart.rs,migration.rs}`.
- Modify: root `Cargo.toml`, `Cargo.lock`, `packages/core-rust/package.json` to include the adapter in required validation.
- Reuse: `anima-core/src/memory.rs`, `anima-memory/src/{core_port.rs,memory_manager.rs,memory_manager/types.rs}`. Keep SQL drivers outside the core and reusable memory algorithms.

- [ ] Add adapter tests for persistent write/query, stable-ID duplicate replay versus conflicting payload, exact scope enforcement, stale revision rejection, correction/supersession, forgetting/retention, commit failure and fresh-process reopen. Build a reusable contract fixture rather than treating the legacy in-memory manager as a conforming `MemoryPort`.
- [ ] Persist canonical memory rows, revision/lifecycle history, provenance and source-effect receipts transactionally. Preserve current validated wire types. Scope the adapter/writer to a trusted owner; a model cannot choose ownership or arbitrary access contexts. Support all memory kinds required by the current contract, including preferences and episodes.
- [ ] Persist relationship perspectives, evidence IDs and temporal observations using existing semantics. Apply visibility before recall ranking, including relationship evidence. Rebuild lexical/semantic indexes from authorized canonical data; absent embeddings must still allow bounded lexical/recent recall. Corrections/forgetting must invalidate stale derived hits.
- [ ] Define an idempotent import manifest for daemon snapshots and game witnessed facts. Preserve prior IDs/visibility, facts, relationship/temporal records and source links; mark unknown evidence as legacy provenance. Test retry, interrupted import, collisions, rollback and old-reader preservation using copied fixtures only. Never infer private knowledge merely because a player/save had a global flag.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-memory-store-sqlite` and `bun run --filter core-rust test`. Expected: memory contracts, physical restart and migration tests pass. A snapshot round-trip or execution checkpoint alone cannot satisfy this milestone.

## Task 8: Bind memory to accepted events and agent turns

**Files:**
- Create: `anima-memory/src/projection.rs`, `anima-memory/tests/projection.rs`, `anima-harness/src/durable/memory.rs`, `anima-harness/tests/persistent_agent.rs` (crate-relative paths under `packages/core-rust/crates`).
- Modify: the new durable facade/definition conversion from Task 5 and memory adapter receipt methods from Task 7.

- [ ] Define required-memory product profiles for the assistant and resident NPCs. Retrieve authorized context before generation and preserve individual identity across sessions. Test explicit startup/turn failure on unavailable memory instead of silently running a memoryless profile. Kernel/helper fixtures may be memoryless but cannot satisfy host acceptance.
- [ ] Model selected observations as sourced memory candidates: witnessed event, user statement, received message or inference. Validate before persistence; model text is not authority for game facts or permissions. Bound candidate count/context size and preserve existing host consent/retention policy. Include a false/unsupported candidate rejection test.
- [ ] Specify a host event/outbox contract: commit an accepted interaction/world event and its pending memory effect together; apply the memory effect with stable `(owner, source event, observing agent)` identity; acknowledge afterward. Corrections and relationships use the same idempotent protocol. The next dependent turn waits for relevant projections or surfaces unavailability. Do not claim atomicity across separate databases.
- [ ] Test failure after source commit, after memory commit but before acknowledgement, and before source acceptance. Restart/retry must neither lose accepted observations nor duplicate memories/relationships; rejected or losing game generations cannot produce authoritative experience. Test read-after-projection and both agents' different perspectives.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-memory --test projection` and the same wrapper with `cargo test -p anima-harness --test persistent_agent`. Expected: an accepted preference is recalled after a fresh process/session with private scope intact. Milestone B requires this in addition to successful execution recovery.

## Task 9: Define one executable acceptance suite for both hosts

**Files:**
- Create: `anima-harness/src/conformance.rs` behind a test-support feature, `anima-harness/tests/host_conformance.rs`.
- Create: `docs/superpowers/reviews/2026-10-25-unified-engine-parity.md` during implementation, recording evidence per consumer.

- [ ] Define a fixture interface for admitting a run, observing events, requesting controls, approving/revoking a capability, reopening a host, inspecting an effect ledger and verifying persisted memory/relationships. It must invoke each host's real integration boundary, not a direct engine shortcut inside both tests.
- [ ] Implement common scenarios listed in the acceptance table below. Use fresh owners/identities and temporary stores per case; test refusal to mix identities and stores. Include read-only non-filesystem capabilities and a fixture file capability with explicit grant.
- [ ] Prove the fixture catches a deliberately misconfigured local test adapter (wrong authority or duplicate effect), then retain normal assertions; do not commit an intentionally failing test.
- [ ] Keep scripted-model results, HTTP provider fixtures and live-provider acceptance separate in reporting. Add a process-restart fixture in addition to in-process object recreation.
- [ ] Run `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-harness --test host_conformance`. Expected: the common suite passes with a reference host and fails invalid fixture behavior.

## Task 10: Integrate a gated workspace-free assistant with persistent memory

**Files:**
- Create: `hosts/rust-daemon/src/engine_bridge/{mod.rs,definitions.rs,policy.rs,capabilities.rs,events.rs,tests.rs}`.
- Modify: daemon `Cargo.toml`, `src/{state.rs,agent_runs.rs,runs/mod.rs,runs/ledger.rs}` and the run persistence schema/codec located by tracing `RunRecord` in `runs/ledger.rs` before editing.
- Inspect and reuse: daemon `src/{execution_budgets.rs,approvals/,memory_store.rs,tools/memory.rs}`, `src/execution_budgets/tests.rs`, and existing action-receipt tests.

- [ ] Add tests through the real daemon request boundary for a no-workspace conversation with durable memory and a non-filesystem capability. Select execution and memory-format versions during admission and persist them with the run. Never let retries change engines or memory authority.
- [ ] Adapt owner/agent/session identity, credentials, accepted personality, current permissions, events and existing request receipts. Explicitly select supported profiles before any model work; preserve legacy coding, helper, connector, schedule and MCP profiles until individually certified.
- [ ] Bridge durable capability execution to existing effect receipts and recovery. Do not run `AgentRuntime` inside a durable capability to simulate a migrated model loop. Do not maintain two independently authoritative budget ledgers; migrate or adapt accounting with a defined single owner.
- [ ] Bind the same persistent identity/memory service used by the game. Migrate only fixture/profile-scoped records first; commit accepted user/conversation events and memory-effect outbox entries through daemon-owned durable storage. On cutover there is one canonical memory writer per profile, not divergent legacy/new writes.
- [ ] Reopen an isolated daemon fixture after termination and run the shared suite. Check HTTP/SSE compatibility, stop semantics, approval changes, replay, remembered preferences, relationship perspectives, correction/forgetting and private-memory isolation. Existing user state and the running server are excluded from tests.
- [ ] Run `bun tools/workspace-dev/scripts/run-rust-daemon-tests.ts engine_bridge` and `bun run rust:test`. Expected: new persistent profile and existing core/daemon/supervisor suites pass. The durable path remains opt-in until Task 13; do not label all daemon agents migrated.

## Task 11: Integrate persistent single-player residents as the independent consumer

**Files:**
- Create: `poc/village/host/src/engine_bridge.rs`, `poc/village/host/src/engine_bridge_tests.rs`.
- Modify: `poc/village/host/{Cargo.toml,src/main.rs,src/service.rs,src/store.rs,src/tests.rs}`, `poc/village/crates/anima-village/src/npc.rs`, PoC `Cargo.lock` and root dependency lock only if required.
- Keep shared domain libraries free of host-owned database handles and credential lookup.

- [ ] Add a migration fixture containing old saves and pending/completed receipts. Add stable mappings from `(save, request, execution version, interpretation-or-narration)` to engine IDs before dispatch. Different stages share the request's allowance; a committed interpretation is never regenerated on a narration retry.
- [ ] Supply a host-owned NPC executor using the same durable facade, execution adapter and persistent-memory service as the daemon. Require stable individual/world IDs and persistent memory for Tavi and Mira. Preserve target language, private witnessed facts, deterministic game authority and existing HTTP payloads. No filesystem workspace or coding capabilities are registered.
- [ ] Test each boundary between engine-result commit and game-state commit. Reconcile by persisted mapping and idempotent existing receipts; do not attempt a pretend atomic transaction spanning two independent stores. A completed game receipt wins, and reviewed recovery fences out late live commits. Retain known usage for late/abandoned work.
- [ ] Commit the winning game receipt/world observation and memory-effect outbox entry together in the game transaction. Replay accepted effects into the memory service without duplicate facts/relationships. Test a private preference, intentional sharing, recipient provenance, correction and complete restart; global save flags cannot imply every NPC witnessed an event.
- [ ] Run the shared suite through the game's host boundary, plus existing save reload, concurrent receipt, disconnect and NPC recovery tests. Include a host-only non-filesystem capability fixture and two independent world/owner fixtures. Do not reset real saves or change ports/startup scripts.
- [ ] From `poc/village`, run `bun run test:host`; run `bun run test`, `bun run build`, `bun run lint` before acceptance. If a temporary isolated preview is needed, use the existing launcher with separate ports and test data. Live Anima-provider and browser checks are separately labeled; deterministic tests cannot establish their result.

## Task 12: Prove a bounded single-player agent ecosystem

**Files:**
- Create: `poc/village/host/src/{agent_activity.rs,agent_activity_tests.rs}`; wire into the existing host module tree.
- Modify: PoC `host/src/{service.rs,store.rs}`, domain `crates/anima-village/src/{npc.rs,lib.rs}` and existing browser client only as needed to present the interaction and signal session presence.
- Reuse: durable facade, memory projection, existing relationship logic and host validation. Do not extract a general distributed scheduler or introduce another model loop.

- [ ] Use one authored supper-preparation trigger: Tavi may deliberately share a relevant fact with Mira; Mira responds; both retain their own sourced episode/relationship observation. A later player interaction recalls it after restart. Keep private, unshared facts out of Mira's context. The trigger is authored; dialogue can be generated within existing language/authority rules.
- [ ] Persist interaction/participant/event IDs and admission/processing receipts. Allow at most two NPC replies per trigger, one active conversation per affected resident, a finite shared token/time budget, and one activation per source event. Reuse durable runs for each turn; crash/retry resumes or reconciles the same interaction instead of starting a duplicate.
- [ ] Activate NPC activity only while the single-player session is present. Use host-owned presence expiry to handle an abruptly closed browser; last-session closure stops new starts and safely settles/checkpoints current work. Persist world clock/progress and resume without offline catch-up. Assistant schedules remain governed by their existing independent lifecycle.
- [ ] Test: restart between replies and between reply/memory commit; private-memory leak attempts; duplicate triggers; budget exhaustion; presence expiry; no post-close new starts; stable inventory/quest state; and no second-world crossover. World actions remain server-validated proposals, never trusted narration.
- [ ] From `poc/village`, run `bun run test`, `bun run build`, `bun run lint`; use an isolated preview for a player walkthrough of preference, NPC exchange, close/reopen and recall. Record actual live-generation results separately from deterministic interaction tests. This demonstrates a small rich loop, not general autonomous simulation or multiplayer scale.

## Task 13: Record execution, memory and single-player parity before limited cutover

**Files:**
- Update: the parity review document from Task 9, harness/memory adapter READMEs, `packages/core-rust/README.md`, `poc/village/README.md`, and relevant daemon runtime docs.
- Runtime changes are limited to enabling individually certified profiles after explicit implementation/cutover authority.

- [ ] Fill the evidence table with command, result, host, fixture/live classification and unresolved gaps. Execution-only success cannot mark a profile complete: identity, memory, privacy and recovery must pass in both hosts, plus the single-player loop in the game. Run `bun run rust:test`, `bun run rust:lint`, and the PoC checks from Tasks 11-12 after final changes. Do not reuse the earlier audit's test count as new verification.
- [ ] Keep legacy runs pinned and allow them to finish. Preserve their records; do not fabricate durable checkpoints from snapshots. For a selected new profile, switch only new admissions. Rollback stops new durable admissions but keeps the durable runner/storage available to finish or reconcile already admitted runs.
- [ ] Never shadow-execute real side effects or retry an uncertain durable run through the old engine. Failed execution or memory protection/schema checks keep admission closed with explicit recovery information. Memory rollback preserves the current canonical store and migrated reader until accepted post-cutover memories have a defined reversible mapping; never silently revert to a stale legacy snapshot.
- [ ] Remove obsolete paths only after every intended profile has a passing migration gate and retained data has a supported reader. Full removal of `AgentRuntime` is a later explicit milestone, not a requirement to ship the first shared slice.
- [ ] Inspect and remove only task-owned temporary reports/fixtures after recording results, using the repository task wrapper. Retain caches and user data. Report what remains on the old path and which live checks were not performed.

## Acceptance table to populate during implementation

| Scenario | Engine + disk adapter | Daemon boundary | Game boundary |
| --- | --- | --- | --- |
| Dialogue without a filesystem workspace or coding tools | Required | Required, with memory | Required, with memory |
| Kernel-only memoryless execution fixture | Allowed, not product acceptance | Not sufficient | Not sufficient |
| Persistent individual identity across session/model/definition changes | Required | Required | Required |
| Accepted preference/episode recalled after fresh-process restart | Required | Required | Required |
| Scoped recall/writes, sourced sharing and relationship perspective | Required | Required | Required |
| Stale revision, correction/forgetting and derived-index consistency | Required | Required | Required |
| Retry-safe event-to-memory projection; explicit memory outage | Required | Required | Required |
| Idempotent legacy memory import without changing visibility | Required | Required | Required |
| Non-filesystem capability runs; file capability without grant never executes | Required | Required | Required |
| Persona/instructions and provider settings preserved | Required | Required | Required |
| Exact request replay; mismatched payload rejected | Required | Required | Required |
| Stop/approval revocation prevents undispatched effects | Required | Required | Required |
| Started effects remain owned and uncertain effects do not auto-replay | Required | Required | Required |
| Finite allowance survives retry, child work and restart | Required | Required | Required |
| Actual process death, reopen and stale-writer rejection | Required | Required | Required |
| Owner/session/scope isolation; no key or private transcript leakage | Required | Required | Required |
| Semantic event replay and compatible streaming | Required | Required | Required |
| Commit failure exposes no uncommitted state | Required | Required | Required |
| Existing assistant behavior / existing saves and quest receipts | N/A | Required | Required |
| No required world membership for assistant | Required | Required | N/A: residents belong to world |
| Explicit player/world identities and two-world fixture isolation | Contract | Owner/scope isolation | Required |
| Bounded Tavi/Mira exchange, close/reopen, persistent consequences | Reusable execution/memory | N/A | Required |

## Validation operations

All commands are future implementation checks, not actions performed while writing this plan. Prefer existing Bun scripts. Focused Cargo invocations use the repository task wrapper where no matching focused script exists. Register both new adapters in core scripts before relying on `bun run rust:test` to cover them.

If the normal daemon executable is locked on Windows, set `CI=1` and a dedicated `CARGO_TARGET_DIR=target/cargo/validation-rust-daemon` before the required Rust suites. Do not stop the user's server. Mock HTTP fixture networking may require normal local-network permissions; diagnose environment failures separately from product failures. Never call real providers merely to make deterministic tests pass.

This plan was authored before implementation. Current implementation, verification results and remaining acceptance boundaries are recorded in [the combined engine review](../reviews/2026-10-25-combined-engine-review.md).

## Later plans, deliberately outside this delivery

1. General-purpose durable inboxes and peer coordination beyond the bounded local interaction.
2. Shared scheduling/lifecycle admission and richer optional personal agent spaces.
3. Multiplayer authentication, replication, concurrent-player rules, database migration and load testing.
4. Separate multiplayer host deployment and external-engine protocol/native bindings.

Persistent identity and memory are in this delivery and required for both products. Later modules remain optional: the assistant can work without a world, and game agents can work without a repository or terminal. Stable IDs, host authority, versioned writes and replaceable stores preserve a multiplayer path, but multiplayer is neither implemented nor certified here.
