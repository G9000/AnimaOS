# M.O.T.H.E.R Additive Provider Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task by task. Use subagent-driven-development only when delegation is authorized. Checkboxes represent implementation work not yet performed.

**Goal:** Keep AnimaOS's native memory and learning active while adding reliable, selected publication to and recall from M.O.T.H.E.R's shared garden.

**Architecture:** Extend the existing external-provider interface with agent-bound M.O.T.H.E.R instances. Keep persona independent, publish only selected candidates through a durable outbox, and merge bounded external recall with native context. Use existing v1 tools first; negotiate later graph capabilities.

**Tech Stack:** Rust daemon and `anima-memory`, existing reviewed MCP manager, durable local outbox, TypeScript SDK and existing web settings/onboarding. M.O.T.H.E.R remains an independent executable.

**Design:** [Shared memory garden](C:/Users/leoca/OneDrive/Desktop/M.O.T.H.E.R/docs/superpowers/specs/2026-11-14-shared-memory-garden.md).

**Garden plan:** [M.O.T.H.E.R implementation](C:/Users/leoca/OneDrive/Desktop/M.O.T.H.E.R/docs/superpowers/plans/2026-11-14-shared-memory-garden.md).

## Boundaries and execution rules

This is an additive provider, not a new canonical AnimaOS backend. Do not replace `MemoryManager`, move learning into M.O.T.H.E.R, mirror all transcripts, or silently reinterpret existing Companion bindings. Existing local privacy scopes still apply. Selecting one agent must not configure every agent on the daemon.

Read current `AGENTS.md` and inspect Git before implementation. At plan authoring an unrelated deletion existed at `hosts/rust-daemon/src/connectors/discord/HERMES-NOTICE`; preserve it. The M.O.T.H.E.R pairing files also had concurrent work. Use isolated implementation worktrees when needed. No live model prompts, user-data imports, installations or service restarts as tests.

For each task: write the listed failing behavioral tests, verify the intended failure, implement, rerun the focused tests and inspect the diff. Use the repository's managed Rust wrappers and 32 GiB cache budget. Do not invent an alternate Cargo target directory.

## A1: Separate additive binding from persona and legacy primary mode

**Modify:** `hosts/rust-daemon/src/workspace_context.rs`, `hosts/rust-daemon/src/workspace_context_tests.rs`, `hosts/rust-daemon/src/routes/workspace_context.rs`, `hosts/rust-daemon/src/routes/tests/workspace_context.rs`, `packages/sdk/src/agent-files.ts`, `packages/sdk/src/agent-files.spec.ts`, `packages/sdk/src/index.ts`.

**Create:** `hosts/rust-daemon/src/memory_provider/binding.rs` and focused `binding_tests.rs` wired into the module.

- [ ] Add a versioned agent setting `settings.additional.memoryProvider` with provider ID, reviewed connection ID, workspace ID, generation, subject, selected-publication policy and independent persona selection.
- [ ] Define `personaSource` as local or the same explicitly approved workspace. Require persona-visible grants for workspace personality; a shared allowlist grant cannot load persona.
- [ ] Keep legacy `workspaceContext` decoding and `primary_memory()` behavior unchanged for unconverted agents. Reject simultaneous additive and primary binding for the same agent.
- [ ] Extend the owner metadata route/SDK to expose both settings and a revision-checked conversion operation that atomically clears legacy primary binding and installs additive binding. Preview preserved identity, newly enabled local memory and unchanged grants.
- [ ] Test malformed config, stale revision, wrong subject/generation, wrong reviewed tool, explicit Deny and unknown-response reread. Defaults remain native-only.
- [ ] Publish the schema for M.O.T.H.E.R G1; endpoint metadata contains no credentials and performs no remote writes.

Proposed setting example, with illustrative identifiers only:

```json
{"version":1,"provider":"mother","connectionId":"reviewed-connection","workspaceId":"workspace-uuid","generation":"generation-uuid","subject":"agent-id","publication":"selected","personaSource":"local"}
```

**Acceptance:** Settings represent local-only, legacy primary and additive unambiguously. Ordinary upgrades do not convert old bindings.

## A2: Implement agent-bound provider and recall

**Modify:** `packages/core-rust/crates/anima-memory/src/external_provider.rs`, `hosts/rust-daemon/src/memory_provider.rs`, `hosts/rust-daemon/src/app.rs`, `hosts/rust-daemon/src/components/providers.rs`, `hosts/rust-daemon/src/tools.rs`, `hosts/rust-daemon/src/agent_runs.rs`.

**Create:** `hosts/rust-daemon/src/memory_provider/mother.rs`, `hosts/rust-daemon/src/memory_provider/mother_tests.rs`.

**Existing tests:** `hosts/rust-daemon/src/memory_provider/tests.rs`, `hosts/rust-daemon/src/agent_runs/memory_provider_tests.rs`, `hosts/rust-daemon/src/mcp/workspace_context_tests.rs`.

- [ ] Test a synthetic provider adds attributed recall while native memory tools, core-memory blocks, task capture and native learning remain active.
- [ ] Add an agent-bound provider resolver. Retain process-wide `none` compatibility, but never place a M.O.T.H.E.R credential or agent binding in the global singleton. Avoid requiring every generic provider to implement garden-specific methods.
- [ ] Implement M.O.T.H.E.R recall through the same reviewed MCP manager used by agent tools, with current authority stamps, cancellation and response bounds. No direct access to its SQLite files or private client tokens.
- [ ] Bootstrap/read authority, search relevant records and fetch full bounded records before reliance. Maintain workspace/generation/subject checks without requiring persona adoption.
- [ ] Extend recalled items with canonical origin and revision metadata using a backward-compatible constructor/default where feasible. Revalidate access/current revision before injecting a cached result. If revalidation fails, omit shared context and continue native memory.
- [ ] Merge local and external results within explicit time/character budgets. Label provenance; deduplicate by canonical mapping, then normalized text. Preserve distinct contradictory assertions and report source/time rather than silently resolving them.
- [ ] Make M.O.T.H.E.R `sync_turn` and automatic mirror callbacks no-ops for publication. Re-enabling local learning must not reactivate transcript upload through another global provider.
- [ ] Test offline, revoked, restored, oversized and malformed responses, simultaneous agents and cancellation. One agent's failure or returned persona cannot affect another.

**Acceptance:** Additive recall works with v1 M.O.T.H.E.R. Native memory remains available during every external failure and no raw turn is published.

## A3: Add selected sharing and durable outbox admission

**Create:** `hosts/rust-daemon/src/memory_provider/publication.rs`, `outbox.rs`, `outbox_tests.rs` under the same directory.

**Modify:** `hosts/rust-daemon/src/tools.rs`, `hosts/rust-daemon/src/memory_store.rs`, `hosts/rust-daemon/src/learning/adoption.rs`, `hosts/rust-daemon/src/learning/records.rs`, `hosts/rust-daemon/src/memory_provider.rs`.

- [ ] Introduce an explicit share/propose operation that captures selected content, basis, source, local ID/revision and target binding. Keep all ordinary local writes local unless explicitly selected by the agent's permitted workflow or owner review.
- [ ] Store candidate and outbox state atomically in a dedicated local durable publication store. The explicit share action is acknowledged only after this commit. Native memory saving is a separate outcome; do not claim an atomic transaction across the native snapshot store and this outbox.
- [ ] For publication triggered by local review, persist the selection intent before reporting it queued. Snapshot selected content and source digest; if the source changed before admission, require a fresh selection.
- [ ] Use an immutable operation UUID and complete serialized request. Include workspace/generation/agent/connection identity, expected target revision and payload hash. Pending records are private local data, never Git files.
- [ ] Implement bounded disk/admission limits. Reject queue admission visibly when full; do not use the existing lossy `try_send` queue for selected publications.
- [ ] Test crash before admission, after commit/before response, duplicate selection, changed source, disk full and local save success with sharing failure.
- [ ] Restrict publication to current tool/grant policy. Background dispatch must not bypass required approval or an explicit Deny; no model text can choose another client identity.

**Acceptance:** Acknowledged queued shares survive restart. Unselected local memories and conversation noise are never enqueued.

## A4: Reconcile writes, review outcomes and shared identities

**Create:** `hosts/rust-daemon/src/memory_provider/reconcile.rs`, `mapping.rs`, `reconcile_tests.rs`.

**Modify:** `outbox.rs`, `mother.rs`, `publication.rs` from A2/A3.

- [ ] Implement the design's state machine and persist every transition. Serialize operations per target record; unrelated targets may proceed under a bounded concurrency limit.
- [ ] Use `mother_propose_memory` for review-only grants. Reconcile operation receipts and `mother_get_proposal`; distinguish submitted, approved, dismissed and conflicted states. Use direct writes only when currently authorized.
- [ ] On timeout or process death, mark uncertain and reconcile with the same UUID/payload. Retry transient failures with bounded backoff; stop automatic retry on stale revision, changed generation, revocation or changed binding.
- [ ] Persist mappings to approved garden IDs and revisions. A proposal receipt without approval must not create an approved-record mapping.
- [ ] Prevent echo loops: recalled/projected garden content carries origin metadata and is not automatically republished. Distinct local publications with similar text are not silently treated as one claim.
- [ ] Handle local deletion explicitly: cancel an unsent share; preserve already committed shared knowledge unless an authorized correction/retraction is selected. Never infer permission to erase another harness's contribution.
- [ ] Test approval with owner-edited content, a cross-harness revision race, response lost after commit, replay after restart, old generation and revoked credentials. In v1 shared allowlist mode, a newly approved record may remain unreadable: report that accurately and do not widen access.

**Acceptance:** Every acknowledged share has a recoverable state and receipt trail; retries cannot create duplicate mutations, and review remains effective.

## A5: Expose additive setup and status

**Modify:** `apps/web/src/components/personality/WorkspaceContextPanel.tsx`, `WorkspaceContextPanel.test.tsx`, `apps/web/src/components/onboarding/MotherOnboarding.tsx`, `MotherOnboarding.test.tsx`, `packages/sdk/src/agent-files.ts`, `packages/sdk/src/agent-files.spec.ts`, `hosts/rust-daemon/src/routes/workspace_context.rs`.

**Coordinate:** M.O.T.H.E.R G1 pairing UI and configuration writer.

- [ ] Add independent controls for shared garden connection and personality source. New connections use additive mode; old primary connections show an explicit conversion action.
- [ ] Show native memory as active and shared context availability separately. Display pending/reviewed/saved/conflict counts without leaking record content into generic health endpoints.
- [ ] Show a concrete preview before converting the selected agent. Include exact connection/grant role and the fact that local recall/capture will resume; preserve chats and existing local data.
- [ ] Separate disable-provider, cancel pending shares and revoke-key operations. Disabling dispatch does not discard pending data; re-enabling revalidates the same binding before retrying.
- [ ] Test offline setup, uncertain setting saves, read-only grants, review-only grants, unsupported server capabilities and switching between agents.
- [ ] Update SDK tests and run the UI checks below.

**Acceptance:** The owner can understand whether a memory is local, queued, awaiting review or saved in the garden. Connection readiness never claims a model has recalled a record.

## A6: Integrate richer garden capabilities when available

**Depends on:** M.O.T.H.E.R G3–G7; not required to ship Release A.

**Modify:** `mother.rs`, `mapping.rs`, `reconcile.rs`, `publication.rs` and associated tests. Reuse `packages/core-rust/crates/anima-memory/src/memory_manager/types.rs` as semantic reference, not a shared storage dependency.

- [ ] Negotiate v2 capabilities without assuming a particular M.O.T.H.E.R version. Keep v1 publication fully usable on older servers.
- [ ] Require owner-granted `garden:read` and `garden:propose` (or explicit `garden:write`) before using graph tools. Never reinterpret v1 read/propose scopes as graph authority. Reconcile graph batches through `mother_garden_get_proposal` and `mother_garden_get_receipt`, preserving the same uncertainty rules as v1.
- [ ] Map selected entities/assertions/evidence to bounded graph proposals. Preserve unknown extensions and source attribution. Never fabricate graph relationships from v1 references or absent temporal dates.
- [ ] Add temporal and relationship candidate retrieval while keeping final ranking in AnimaOS. Use opaque change cursors and visibility epochs to invalidate local derived context.
- [ ] Keep v1 exact record revalidation as fallback where change tracking is unavailable. If required graph semantics cannot be represented, report unsupported rather than flattening them into misleading records.
- [ ] Test a fact contributed by another harness, later corrected, with AnimaOS seeing the current authorized revision and preserving the historical evidence.

**Acceptance:** The adapter gains graph capabilities without changing native learning behavior or silently degrading old servers.

## A7: Cross-harness acceptance and documentation

**Modify:** `docs/workspace-context.md`, `docs/rust-daemon-architecture.md`, `packages/core-rust/crates/anima-memory/README.md` and relevant onboarding help.

**Create:** `docs/validation/mother-additive-provider.md`.

- [ ] Run a synthetic MCP peer through the normal tool transport and a temporary M.O.T.H.E.R store. Avoid real grants, live chats, real user records and paid model calls.
- [ ] Demonstrate native-only behavior, additive recall, selected publication, owner approval, cross-client retrieval, correction, offline continuation and reconnection.
- [ ] Inject restart/timeout at each outbox boundary and assert one proposal/write. Check cancellation, binding changes, Deny and stale generations.
- [ ] Demonstrate that ordinary local task-result capture and learning continue, but no unselected turn reaches the garden. Old primary mode retains its behavior until explicit conversion.
- [ ] Document Hermes-style additive behavior and the deliberate difference: M.O.T.H.E.R is a context keeper, not an automatic transcript extraction service.
- [ ] Record protocol/synthetic results separately from installed-host acceptance. Only test a live owner agent later with explicit authorization and a prepared rollback.

## Verification commands

From the AnimaOS repository root, use the existing managed runner for focused Rust filters while developing:

```powershell
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima-daemon memory_provider
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima-daemon workspace_context
bun run --filter @animaos/sdk test
bun run --filter @animaos/web test
bun run --filter @animaos/web typecheck
bun run --filter @animaos/web build
bun run --filter @animaos/web lint
```

Confirm the package's actual test names and include new modules under matching filters; a zero-test result is not validation. Before integration run the required full Rust suite and workspace checks:

```powershell
bun run rust:test
bun run rust:lint
bun run rust:build
bun run build
bun run test
bun run typecheck
bun run lint
```

Expected: all relevant checks pass; record unrelated baseline failures without attributing them to this change. Use owned temporary stores and the managed cache target. No tests may configure live harnesses or execute external actions.

## Completion criteria

Release A is done when native memory stays active, the garden is additive, selected shares are durable and review-aware, current permissions are enforced, and explicit conversion is tested. A6 adds graph support later; it must not block useful v1 integration. No database replacement is necessary to fix the current integration.
