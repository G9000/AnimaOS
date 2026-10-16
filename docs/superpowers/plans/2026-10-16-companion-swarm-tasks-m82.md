# M82: Stable task ownership for legacy swarm members

> **For agentic workers:** Use the approved bounded execution workflow and `superpowers:executing-plans` for assigned steps. The controller has bound the ownership and risk decisions below. No source implementation or validation preceded this plan.

**Goal:** Preserve one logical legacy swarm member's task list across coordinator dispatches without changing its transient runtime identity.

**Architecture:** Give the host tool context an optional task-store owner. Legacy swarm contexts bind that owner to a tagged combination of their coordinator's `swarm_id` and member `agent_id`. Baseline capture, task reads, writes and conflict recovery resolve the same owner; other callers retain `AgentState.id`.

**Tech stack:** Existing Rust daemon, coordinator, deterministic model adapters and durable task CAS store. No dependency, protocol or schema change.

## Evidence and scope

- `state/swarm_runtime.rs:85-86` already computes `stable_swarm_persistence_agent_id(swarm_id, config.name)`.
- `state/swarm_runtime.rs:346-348` defines that ID as `{swarm_id}:agent:{agent_name}`.
- That name-derived database ID is not a sufficient task owner: distinct members can share a name. Leave it unchanged for its existing database purpose.
- `packages/core-rust/crates/anima-swarm/src/coordinator.rs` lines 59–69 supplies host-owned `swarm_id` and `agent_id` to the factory; member creation assigns the latter at line 636.
- `coordinator.rs:700-721` clears pooled shells' task state without replacing their coordinator member IDs. The host then rebuilds only the inner runtime.
- `state/swarm_runtime.rs:145-175` acquires the runtime lock, rebuilds when reset is requested, then captures a per-run task revision using `runtime.state().id`.
- `state/swarm_runtime.rs:330-342` constructs a fresh `AgentRuntime` and separately sets its database persistence ID.
- `tools/todo.rs:61,84,112` still selects the task file using the transient `agent.id` for write, conflict refresh and explicit read.
- Consequently, the next dispatch can read an empty task list at a new path instead of the same member's saved tasks.
- M80's retired additional fixture watched the former runtime's file. Preserve that historical failure and retirement; do not revive it as an acceptance oracle.

This milestone changes only task-store ownership for the legacy factory. It does not change `AgentRuntime` IDs, messages, memories, database keys, run IDs, tool-call idempotency, coordinator reset or pooling.

Do not migrate, delete, discover or adopt orphaned task files. A missing stable file has the ordinary successful empty-list revision. Existing transient files remain untouched.

Continuity is accepted only across resets of the same coordinator member. A new ephemeral member remains distinct. Do not promise process-restart continuity or reinterpret reused name/database keys as task authority.

No REST, SDK, web, task schema, provider, account, profile, service or migration work. No claim of model planning quality or full Hermes parity.

## Source ownership

| File                                           | Responsibility                                                                                                              |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `hosts/rust-daemon/src/tools.rs`               | Optional host-owned task owner, constructor default and one resolver/builder.                                               |
| `hosts/rust-daemon/src/tools/todo.rs`          | Resolve owner consistently for model task operations; retain existing store/CAS and inline boundary tests.                  |
| `hosts/rust-daemon/src/state/swarm_runtime.rs` | Bind stable owner, capture its baseline after lock/reset, and add real coordinator regressions in the existing test module. |

Prefer those three production files with their existing inline tests. `state/swarm_runtime.rs` already tests real `DaemonState::build_swarm`, `start` and `dispatch` using a deterministic adapter (`run_budgeted_supervisor`, lines 533-557).

Use a disposable `WorkspaceConfig` set directly on the test `DaemonState` before building the swarm. The factory captures `self.workspace` at lines 38 and 52. Avoid process-wide environment changes and shared integration-fixture mutexes.

`tests/swarm_api.rs::swarm_event_stream_emits_todo_tool_results` stays an unchanged required integration control. Its original assertions establish SSE success, not cross-dispatch continuity. Additions to that file require a concrete need and controller-recorded scope expansion; prefer the existing internal coordinator seam.

Controller-owned publication updates only the current milestone row/handover/task-use statements that need the new bounded outcome. Preserve unrelated files and historical raw/Git/runtime evidence.

## Interface contract

1. Add an optional task owner to `ToolExecutionContext`, named `todo_owner_id: Option<String>` or equivalently narrow terminology.
2. Initialize it to `None` in `ToolExecutionContext::new`.
3. Add a crate-visible host builder `with_todo_owner(owner_id: String) -> Self`.
4. Add one crate-visible resolver `todo_owner_id(&self, agent: &AgentState) -> &str`, returning the override or `agent.id` with an explicit shared lifetime if Rust requires it.
5. Keep the field private to the tools module where practical. Tool arguments and model messages cannot supply or change it.
6. Binding a new owner must not carry an existing revision from a different owner. The builder starts a fresh `Arc<Mutex<Option<String>>>` containing `None`; it performs no filesystem operation.
7. `with_todo_baseline` remains the per-run constructor of independent revision storage. Clones within one run continue sharing that run's revision only.
8. Resolve owner before calling `read_agent_todos` or `write_agent_todos`; do not duplicate fallback logic at each call site.
9. Keep low-level store signatures and the explicit owner/internal optional-revision writer unchanged.

The resolver should be boring:

```rust
pub(crate) fn todo_owner_id<'a>(&'a self, agent: &'a AgentState) -> &'a str {
    self.todo_owner_id.as_deref().unwrap_or(&agent.id)
}
```

The exact identifier spelling can be adjusted consistently. This is host context, not a new public API or a general persistence identity abstraction.

## Legacy binding and timing

- Build a task-only owner from the factory context's coordinator IDs, for example `format!("swarm:{}:member:{}", context.swarm_id, context.agent_id)`.
- Keep this assembly in one small host helper, such as `swarm_task_owner_id(swarm_id, member_id)`, used by the factory and its in-module tests.
- Pass that owned value to `with_todo_owner` before the shell captures its tool context.
- Keep the separate name-derived `persistence_agent_id` and all `build_swarm_runtime` database arguments unchanged.
- Do not change the format or visibility of `stable_swarm_persistence_agent_id` merely for testing. Do not change core coordinator identity allocation or pooling.
- Keep baseline capture after acquiring the runtime mutex and processing `needs_reset`, before `run_with_tools`.
- Capture from `tool_context.todo_owner_id(runtime.state())`, under the existing `allows_tool("todo_write")` condition.
- Convert read failure to `None` as today. Do not disable the run, repair the file, seed an empty baseline or retry the read automatically.
- Make a fresh run-local baseline context and pass its clones to the tool executor as today.
- For read-only task tools, explicit `todo_read` resolves the stable owner and may populate the run-local revision normally.
- Keep the captured owner immutable through the run. A fresh dispatch recaptures the latest revision for the same owner.

## Task and CAS rules

1. `execute_todo_read` reads the resolved owner and updates the current run's baseline only after success.
2. `execute_todo_write` retains argument validation and M80's missing-baseline refusal before any write, implicit read or directory creation.
3. The successful write path uses the resolved owner with `Some(observed_revision)` and updates the same run-local baseline.
4. The stale-CAS branch reads that same resolved owner, reports the latest tasks and updates the baseline for a later explicit merged write.
5. No automatic merge, repeated model write, per-write baseline capture or retry is introduced.
6. Failed reads preserve malformed bytes and provide no new revision authority. A later external repair alone does not authorize the current run to overwrite it.
7. Missing files retain the existing `[]` revision semantics and ordinary first write.
8. Default contexts keep existing main-companion and direct-tool behavior through `AgentState.id`.
9. Same coordinator member across resets shares one task store; different member IDs or swarm IDs select different stores, including same-name members and new ephemeral members.
10. Use existing hex path encoding, atomic replacement, validation and CAS lock. Do not add a second filename format, identity map or persistence layer.

## Meaningful regression matrix

Use distinctive task contents and exact byte/revision checks, not only directory existence or a generic success string. Each scripted model must inspect actual tool results before returning its next explicit action.

### 1. Real dispatch continuity

- Build and start one real coordinator with one fixed logical swarm identity and deterministic adapter.
- First dispatch writes task A using the ordinary startup baseline.
- Await completion; dispatch again through the same coordinator so normal clear/reset executes.
- Second dispatch explicitly reads and must see A, then explicitly writes A plus B.
- Assert the final list and revision under `swarm_task_owner_id(actual_swarm_id, actual_member_id)` using the coordinator's observed member ID, not a name-derived database ID or the inner runtime ID.
- Assert no extra transient-owner task file is created in the otherwise empty test task directory.
- Record tool request/result counts to prove the expected write sequence ran, rather than inferring success from the dispatch's final text.

This is the primary meaningful RED: the old implementation cannot preserve A through the normal reset and cannot produce the expected stable file.

### 2. Fresh baseline on later dispatch

- After a successful dispatch, make a valid explicit owner/internal update to the stable file.
- The next dispatch must read the updated list and successfully save an explicit merged list against its fresh baseline.
- Assert both the external task and the model task survive.
- Do not mutate or inspect the prior transient runtime's file as the oracle.

### 3. Malformed stable file and explicit recovery

- Seed malformed bytes at the known stable owner before a dispatch.
- The scripted model attempts `todo_write`; assert the distinct missing-revision refusal and exact unchanged bytes.
- An explicit `todo_read` while malformed must fail without creating authority.
- Repair/update the stable list through an explicit fixture owner action while the same run is paused at a deterministic adapter barrier.
- Another blind write still refuses and leaves the repaired bytes unchanged.
- Only after an explicit successful `todo_read` may the script explicitly write a merged list.
- Verify the other owner's task survives, and the model did not write any transient-ID file.

The adapter barrier is test-only synchronization, not a sleep or a production hook. Assert tool status/result evidence; a model can return a final success sentence after a tool error, so final run status alone is insufficient.

### 4. Conflict recovery resolves the same owner

- Capture a valid stable-owner baseline through normal dispatch startup.
- Perform an owner update before the first scripted model write.
- Assert stale-CAS refusal preserves exact bytes and its response contains the latest stable-owner task.
- Follow with an explicit merged write; assert success and both tasks retained.
- This detects updating the write path while accidentally leaving conflict refresh on `agent.id`.

### 5. Isolation across swarm and member

- Build two coordinators with the same member name and different actual swarm IDs in the same disposable workspace.
- Exercise actual task tools through each; neither reads nor overwrites the other's list.
- In one coordinator, exercise two distinct same-name member IDs through real shells with task tools, using existing spawn/run-reference APIs where name routing cannot distinguish them. Assert their lists and revisions remain separate across repeated work.
- Verify a newly spawned ephemeral member cannot inherit an earlier member's list merely by reusing its name. Do not change pool selection or duplicate-name routing to make this test pass.
- Include manager/worker separation if the chosen strategy uses both; exercise the actual factory, not merely string inequality.
- Join all coordinator work before fixture teardown and assert the expected calls occurred for each named member.

### 6. Defaults and first use

- Retain the M80 direct model boundary tests for missing baseline, malformed bytes, repair/read/merge, first-use empty read and explicit unconditional internal writer.
- Add a focused default-context assertion if existing tests do not prove that a task tool still selects `AgentState.id` without the override.
- Add a small context test for owner rebinding discarding a previous revision while preserving the old clone's revision, if the builder is tested directly.
- These controls must not turn the model's missing-baseline refusal into a broad writer restriction.

Group closely related cases where fixtures are shared, but keep assertions diagnostic. Test names should contain `m82` for the controller's bounded filters. Do not duplicate already meaningful M80 tests wholesale.

## Tests-first implementation split

### Phase A — tests only

- [ ] Add the real coordinator continuity and malformed stable-owner regressions against existing public/internal fixture surfaces.
- [ ] Use actual coordinator/member IDs in the in-module tests; independently assemble the expected tagged owner in the initial tests without requiring the proposed production context API.
- [ ] Keep production unchanged and stop for controller validation.
- [ ] Controller records test source hashes and runs the focused tests once, preserving raw output and exit status.
- [ ] Accept only behavioral failures for the observed ownership mismatch as meaningful RED; fix compilation/fixture mistakes before labeling RED.

### Phase B — narrow implementation

- [ ] Add the context field, builder and resolver with safe default/rebinding semantics.
- [ ] Bind the legacy context to its existing stable ID.
- [ ] Route initial baseline, explicit read, write and conflict refresh through the resolver.
- [ ] Complete the isolation/CAS/default controls within the declared files, preserving the primary RED tests.
- [ ] Do not touch core runtime identity, database APIs, tool schemas or main companion startup.
- [ ] Source-only implementer reports changed files and any unresolved issue; controller owns all formatting and validation processes.

### Phase C — one review and one fix wave

- [ ] Controller records formatted initial source freeze and focused GREEN evidence.
- [ ] One independent daemon batch review covers all changed sources, owner/baseline consistency, reset timing, isolation and meaningful assertions.
- [ ] Apply one combined fix wave for confirmed actionable findings and gate-found compile/fixture corrections.
- [ ] Freeze the final source and run the required serialized gates.
- [ ] Record outcomes precisely; no second design audit, redundant acceptance harness or live-provider case.

## Validation commands and limits

Only the controller runs these commands, through the established Windows validation wrapper. Keep `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=2`, and the existing isolated `CARGO_TARGET_DIR` where needed. Serialize Cargo/Nx validation; do not start parallel native builds.

```text
bun x nx run rust-daemon:test --skipNxCache --excludeTaskDependencies --args="--lib m82 -- --test-threads=2"
bun x nx run rust-daemon:test --skipNxCache --excludeTaskDependencies --args="--lib tools::todo::agent_tests -- --test-threads=2"
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run rust-daemon:build --skipNxCache
bun x prettier --check docs/superpowers/plans/2026-10-16-companion-swarm-tasks-m82.md
```

The current `rust-daemon` Nx target defines test as `cargo test -p anima-daemon`, build as `cargo build -p anima-daemon`, and lint as `cargo fmt --all --check`. Lint is the Rust formatting gate; do not silently describe it as clippy.

Run scoped documentation consistency checks on the published plan and controller-touched outcome/handover/roadmap statements. Expand the Prettier path list to those actual Markdown files. No invented docs target is required; if an Astro app source is changed, run its documented Nx build as required by AGENTS.md.

The full Rust suite includes the unchanged original swarm todo SSE test, token-reset/pooling controls and task CAS tests. Do not weaken them after a failure. Preserve primary failure output before secondary fixture-lock poisoning; do not add lock-poison recovery to hide assertions.

No web/SDK gates are required without a scope change. Do not run the daemon, browsers, containers, providers, accounts, normal profiles or ordinary services.

## Reuse and provenance

Reuse the integrated Rust task store, path encoding, atomic writer, CAS baseline and conflict behavior. Reuse existing coordinator identities in a task-only namespace rather than creating another identity allocator. The existing name-derived database persistence key remains untouched.

The M80 plan records a controller comparison to pinned public Hermes `tools/todo_tool.py` at `1fd18277267152893a8a24792b79213e7ca3f899`: its per-session revisioned task state is a useful behavioral comparison, while Anima already has the needed durable store and CAS. That recorded comparison was not revalidated for this draft; no upstream inspection or copied code is needed for this owner-binding fix.

Direct reuse of the current Rust store is smaller than adapting Hermes persistence or porting unrelated session/compression behavior. If substantial upstream code is later copied, record the exact source/commit/local changes and preserve notices before expanding scope.

## Top five risks for controller rulings

1. **Mixed owner and revision:** startup captures one path while explicit read/write/conflict uses another. Bind once and use one resolver at every seam; test stale CAS and explicit repair/read/merge.
2. **Authority or default drift:** override becomes model-controlled or changes ordinary companion behavior. Keep it host-only, default to state ID, clear revision when rebinding and retain existing direct-tool controls.
3. **Identity/isolation drift:** change runtime identity or accidentally share a list across same-name members. Use coordinator swarm/member IDs, not the name-derived database key; verify real duplicate-name/member isolation without changing message/database ownership. Limit continuity to the same coordinator member lifetime.
4. **Unsafe continuity claims or migration:** infer an old orphan file belongs to this member, or call malformed/missing files equivalent. No migration; preserve bytes, require observed revision, and state that old files remain untouched.
5. **False regression evidence:** use a prior transient file, skip real reset, accept generic final text, or let fixture failures mask the result. Use real repeated dispatch, stable-owner bytes, tool results/counts, bounded deterministic barriers and joined cleanup.

## Completion evidence

- Meaningful ownership RED retained with exact test source.
- Final source freeze and one daemon batch review/fix-wave outcome.
- Fresh focused tests and required full Rust test/lint/build/format/docs receipts.
- Stable logical member continuity and isolation demonstrated through real deterministic coordinator dispatch.
- M80 refusal, owner/internal writer authority and explicit CAS recovery retained.
- No provider/account/host acceptance claim, orphan migration, dependency or protocol change.

## Controller rulings on the risks (binding)

1. Ruling: use the host-generated coordinator swarm/member identity, never the display name or the database's name-derived key — duplicate-name members must not share tasks — cost if wrong: one helper can replace another helper's work. A pooled member retains tasks through runtime resets; newly created ephemeral members remain distinct owners.
2. Ruling: change only the task owner in ToolExecutionContext, defaulting to the existing runtime agent ID — changing runtime, memory, database, message or idempotency identity would broaden this fix — cost if wrong: unrelated histories or recovered effects can become conflated. Startup baseline, todo_read, todo_write and conflict recovery must resolve exactly the same owner.
3. Ruling: retain the existing failed-read refusal and CAS contract — a stable owner does not authorize overwriting an unread or newer list — cost if wrong: saved work is lost. Preserve malformed bytes, missing baseline, explicit successful read/merge recovery, valid empty first save and independent other owners.
4. Ruling: a host task-scope builder starts with no observed revision; each run gets a fresh baseline while clones within that run share it — a revision observed for another owner or prior run supplies no authority — cost if wrong: a stale or cross-owner context can replace tasks. Keep the existing serialized runtime and task-store lock; no new retry mechanism.
5. Ruling: do not adopt or migrate old ephemeral task files or claim daemon-level swarm restoration — source inspection proves dispatch-time runtime replacement, not persisted coordinator membership — cost if wrong: arbitrary orphan files could be attributed to the wrong member or unsupported recovery promised. Test real coordinator resets plus same-name/member isolation without changing historical fixtures or activating accounts/services.

Ruling: reuse the current Rust task store and revision API — the pinned public Hermes per-session in-memory TODO store does not solve this host identity mismatch — cost if wrong: a needed contract change must be demonstrated and reviewed before widening scope. No upstream code port or dependency is needed.
