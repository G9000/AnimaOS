# Unified Rust engine: single-player first, persistent agents in both products

Status: proposed design recorded from the user's direction; planning only. No implementation or deployment is authorized by this document.

## Objective

One reusable Rust execution engine must support the Anima personal assistant and a rich single-player agent world. Persistent identity and long-term memory are required for both the assistant and resident game agents. Filesystem workspaces and coding tools remain optional. World membership is required for resident game agents, but not for the assistant. Implement durable execution first, persistent identity/memory next, then prove both consumers and a bounded single-player interaction loop. Multiplayer implementation is deferred; preserve the ownership, identity and storage boundaries needed to add it later.

## Decisions

1. Converge on `anima-core::DurableAgentEngine`. Keep `AgentRuntime` operational as a compatibility path until each consumer has passed explicit migration gates. Do not run both loops for the same task or nest independently accountable loops.
2. Keep `anima-harness` as the ergonomic embedding layer. Add an explicit durable entry point before changing existing callers. It delegates execution to the durable engine rather than implementing another loop.
3. Add a reusable SQLite execution-store adapter under `packages/core-rust/crates/anima-execution-store-sqlite`. Database drivers, filesystem access, and async-runtime integration remain outside `anima-core`.
4. Keep application adapters in their hosts. The daemon owns assistant permissions, connections and tools. The game host owns observations, quest state and application transactions. Neither host supplies a competing execution state machine.
5. The existing daemon is the initial home for a future personal agent space. Keep the existing PoC host as the independent embedding proof. This phase does not move the game into the daemon or create a multiplayer server. A later shared-world server can embed the same crates.
6. Add a reusable durable memory adapter outside `anima-core`, using the existing `MemoryPort` contract and existing recall/relationship algorithms. Both product profiles must bind it before being considered ready. SQLite is the initial local storage implementation; neither product depends on SQLite-specific behavior in its domain logic.
7. Treat execution state, agent memory and world state as separate authorities. Execution checkpoints do not replace remembered experiences, and remembered claims do not override authoritative game state.

## Baseline and limits

Source baseline: `37f4e8d0`, matching local main at the audit. Recheck before implementation.

- `anima-core/src/runtime.rs` is used directly by the daemon and indirectly by the harness.
- `anima-core/src/engine.rs` contains a separate durable execution loop, with definition, policy, capability, event, clock and store boundaries.
- `anima-core/src/execution/store.rs` exports `ExecutionStore` and a public conformance suite.
- `InMemoryExecutionStore` supports validated sealed snapshots and scalar projections; it is currently the reference store, not a disk-backed adapter.
- `anima-harness` already offers cancellation, observers, execution budgets and checkpoint hooks. Its current restore behavior deliberately does not create fresh authority.
- The village creates fresh no-tool harness calls and separately commits game snapshots/request receipts in SQLite.
- Memory recall, relationship and temporal algorithms already exist. Durable `MemoryPort` implementation is missing and is a required milestone in this plan. General-purpose persistent inboxes and common scheduling remain later work; a bounded local interaction proof is included here.

The audit's deterministic tests do not establish live provider quality, concurrent multiplayer capacity, or native game-engine integration.

## Required product capabilities and optional execution capabilities

The minimal execution kernel needs an owner, agent-definition pin, session/run identities, input, finite allowance, and injected host dependencies. It does not need a repository path, working directory, shell, Git or agent-files folder. Kernel-only tests and temporary helpers may run without long-term memory; this is not an allowed substitute for persistent memory in the assistant or resident-character acceptance profiles. Both hosts must register durable agent identity, retrieve authorized memories and persist accepted experiences. Memory-store unavailability is explicit and must not silently produce a supposedly persistent but memoryless agent.

| Concept | Meaning |
| --- | --- |
| Owner | Authorization boundary supplied by the host, never trusted from model output |
| Definition | Versioned model/instruction/capability configuration |
| Agent identity | Stable individual ID linked to definition versions and its memory; survives new sessions, model changes and restart |
| Session | Conversation/activity identity, independent of a directory |
| World / player | Explicit IDs in game state, even with one current player; absent for an assistant outside a world |
| Application scope | Optional explicit application context; an agent/session-scoped context can serve an otherwise context-free run |
| Filesystem grant | Optional authority for a specific directory/resource, required only by capabilities that use it |
| Host data directory | Storage selected by the host; never implicitly exposed as an agent workspace |

Existing policy types use a mandatory `workspace_id` string for matching policy and grants. During this delivery, retain that persisted wire contract. At the new embedding boundary expose an opaque application-scope identifier; a host adapter maps it deterministically to the existing field. Existing assistant workspace IDs remain unchanged. New non-filesystem scopes are allocated and persisted in a host-owned namespace, checked for collisions with legacy scopes. Never substitute an empty ID, global default workspace, filesystem path or model-provided scope. This compatibility mapping does not grant filesystem access. A later wire rename requires its own versioned migration.

Capabilities declare their environment requirements. A missing filesystem grant rejects a file capability before invocation; it does not prevent dialogue or unrelated tools from running. Empty capability lists are supported. Model credentials and storage protection keys are injected by hosts and excluded from agent definitions and browser payloads.

## Durable storage design

For the initial bounded adapter, use the existing sealed reference-store snapshot as canonical authority. This avoids independently reimplementing approval, grant, checkpoint and recovery rules in SQL.

Each mutating store operation must:

1. Enter a database write transaction and read the latest generation and sealed snapshot from disk.
2. Restore a fresh reference-store state with host-injected protection and adapter-authoritative time.
3. Apply exactly one public store operation to that isolated state.
4. Export the sealed snapshot and scalar projections, then commit them with the incremented generation atomically.
5. Return success only after commit. On failure, discard the candidate; never expose its mutation through a long-lived in-memory cache.

Reads load a committed generation. Independent connections/processes obey the same transaction boundary. No transaction spans a model call, tool execution, approval wait or external network operation. A bounded storage worker owns connection/transaction custody; cancellation of a caller cannot publish partial state or silently abandon an admitted write. Do not hold async-runtime workers in blocking database work.

Persist store format version, generation, sealed state and a monotonic clock floor. Reconstruct leases from persisted fences; reconnecting cannot extend an expired lease or refill budget. Use the core's snapshot integrity/payload policies. Integrity sealing is not encryption: document plaintext/transcript retention explicitly, keep keys outside snapshots, and test incorrect/missing keys and payload protection. Foreign owners must not read records through any store API.

This full-snapshot design is a first adapter for bounded local deployments, not a multiplayer scaling claim. Respect existing snapshot-size limits and surface capacity errors without destructive pruning. Set a measured latency/size envelope before enabling real workloads. Later indexed/partitioned storage must pass the identical conformance suite.

## Required persistent identity and memory

Use stable host-assigned agent IDs, distinct from definition versions, model runtime instances and session IDs. Preserve existing assistant IDs and create stable save/world-scoped IDs for Tavi and Mira. Changing a personality definition or model does not create a new individual or reset memory. Creating a second world does not silently reuse the first world's NPC memories. Do not copy assistant-private memory into a game just because both use the engine.

The memory adapter stores canonical records, revisions, provenance and lifecycle history in transactional rows, not in the execution snapshot blob. Existing lexical/semantic indexes and relationship projections are derived, rebuildable data. Reuse current retrieval/relationship algorithms without treating the legacy manager as the new durable authority. Relationships must retain the observing agent's perspective and evidence; Tavi's view of Mira is not automatically Mira's view of Tavi.

Read access uses explicit authorized scopes. Because `MemoryPort::write/revise` currently have no access-context parameter, expose only a trusted owner/agent-bound writer through the host; callers and model text cannot choose foreign ownership or visibility. Test read, write, correction, forgetting and derived-index isolation. Per-record revision checks, exact replay/conflict behavior and lifecycle mutations must survive process restart.

Store selected experiences rather than treating every generated sentence as fact. Provenance distinguishes witnessed events, user statements, received messages and model inferences. New model-extracted memory is a candidate validated by host policy; a model claim cannot create an item, grant access or become another agent's observation. Retention and auditable forgetting remain supported; persistent memory does not mean retaining all text forever. Recall uses bounded context and works without an embedding service.

For cross-store consistency, the host commits an accepted user/world/conversation event and a pending memory effect together in its authoritative transaction. A restart-safe projector applies the effect with a stable source-event/agent ID to the memory store, then acknowledges it. A crash between application and acknowledgement must replay without duplicates. Only winning committed game receipts generate authoritative game experiences; abandoned model results do not. Admission of the next dependent agent turn waits for the relevant memory effects to settle, or reports an explicit degraded/unavailable state. Preserve enough authenticated provenance to resume this projection; never infer missed facts from uncommitted output.

Legacy memory/save migration is explicit and idempotent, preserves identities and visibility, and labels unavailable evidence as legacy provenance instead of inventing it. Preserve existing relationship/temporal records and correction semantics during each profile's cutover. Keep backups and the previous reader available; no destructive import or real-data migration occurs during planning or tests.

## Single-player ecosystem and multiplayer seams

Initial experience: one player, one persistent village, Tavi and Mira. An event activates a bounded NPC conversation, a deliberately shared fact becomes a sourced memory for the recipient, a relationship observation is retained, and a later encounter recalls it after a full restart. Preserve world time, quest/inventory state and delivery receipts. Do not add combat, large crowds, economic simulation or unrestricted continuous thinking.

Game activity is enabled while its single-player session is open, with host-owned presence expiry for disconnects. Closing the game stops new NPC activity and safely settles/checkpoints admitted work; reopening resumes saved world time without simulating offline hours. The assistant's normal daemon-owned lifecycle remains independent. The single-player host owns event ordering and validates all proposed world changes.

Keep owner, player, world, agent, session, request and event IDs explicit. Use version checks and idempotent commands rather than global player state. Test isolation using two worlds/owners as fixtures without building multiplayer UI or networking. Scope authority is supplied by the host, not by client/model parameters. Database adapters are replaceable and must satisfy the same semantics; future PostgreSQL/distributed storage is a separate implementation and migration project, not a connection-string switch.

No multiplayer authentication, matchmaking, replication, distributed scheduling, shared-world server or load acceptance is included. These seams reduce later redesign; they do not justify calling the product multiplayer-ready.

## Host integration and migration

Persist the selected execution version with a new run before dispatch. A retry, reconnect or restart resolves the existing run and continues with its original version. Unsupported capabilities fail or use the legacy path only before durable admission; never fall back after a model/tool effect may have started.

The durable facade maps definition/personality inputs, provider configuration, tools, streaming events and controls explicitly. Preserve personality rendering; do not assume the `persona` field is automatically injected by the current durable engine. Finite attempts, tokens, tool calls, elapsed deadlines and shared child allowances need a parity gate: turn-count limits alone are insufficient. Missing current-runtime behavior is implemented or deliberately keeps that profile on the legacy path.

The first daemon profile is conversational with persistent memory and identity, but no workspace or coding tools. The next profile uses one bounded non-filesystem capability. Existing coding, MCP, connectors, schedules and helper workflows remain on the current path until their parity cases pass. This phase does not claim the entire daemon has migrated.

The game profile retains the current HTTP responses, save IDs, quest rules, separate NPC context and reviewed fallback semantics. Map every request ID to stable execution identities, including distinct interpretation and narration subruns when both occur. Their request-level allowance is shared. Retrying a completed subrun reuses its committed result.

Game-state transactions and engine transactions remain separate. Persist the request-to-run mapping before dispatch. On recovery, reconcile committed engine results into the existing idempotent game transaction; never blindly regenerate because a game commit was interrupted. A final reviewed fallback or recovery revokes the pending generation's authority to commit game state. Late completions remain accounted for but cannot overwrite the winning game receipt. Preserve all existing saves without claiming that a snapshot is an execution checkpoint.

## Acceptance gate

Both consumers must exercise the same durable facade, execution adapter and persistent-memory implementation with separate host data and capabilities. They do not share private data or credentials. Execution-only milestones are intermediate progress; product acceptance requires persistent memory in both consumers.

Required scenarios: no-workspace dialogue; non-filesystem tool; denied filesystem tool without a grant; exact retry; conflicting payload rejection; stop before dispatch; stop during an effect; approval denial/revocation; finite budget exhaustion; actual child-process death and reopen; stale writer rejection; completed-effect reconciliation; uncertain non-idempotent effect paused for review; scoped event replay; no cross-owner reads; failure during database commit; preserved personality and streaming.

Also required: identity continuity across definition/model/session changes; remembered preference after restart in both products; private/shared memory access; sourced agent-to-agent learning; relationship perspective; stale revision rejection; correction/forgetting reflected in recall; retry-safe memory projection; rollback-safe legacy import; explicit memory outage behavior; independent worlds in fixtures; and a bounded single-player interaction that pauses when the game closes.

Use deterministic models and temporary host data. Include local HTTP provider fixtures separately. Live provider acceptance, human language review, multiplayer load and native bindings are separately reported and are not implied by fixture success. Do not stop or migrate the user's running main server or game during validation.

## Follow-on boundaries

After this single-player delivery: generalize durable messaging and schedule admission beyond the bounded local proof; add richer optional spaces and embodiments; then separately design multiplayer hosting. Persistent identity and memory are already delivery requirements, not follow-on options. A game/Unreal/Unity integration can initially use a narrow service protocol; native bindings require a separate plan.

## Non-goals

No new daemon deployment, all-agent migration, automatic import of uncertain legacy runs, replacement of game transactions, large-scale world simulation, character UI redesign, replacement of existing recall/relationship algorithms, general scheduler rewrite, native SDK or multiplayer performance claims in this delivery. Durable memory storage/integration and a small single-player interaction are in scope. No implementation, commit, merge or push is performed by updating this design.
