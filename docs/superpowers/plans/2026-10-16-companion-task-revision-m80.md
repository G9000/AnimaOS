# M80: Require an observed task revision before model writes

> Lean plan from the delegated native reader, bound by the controller below. Start this branch only after M79 merges. One daemon batch review, one combined fix wave, then gates; no separate preflight audit or actual acceptance case.

## Goal and confirmed defect

Prevent model-driven `todo_write` from overwriting tasks when the run could not read its initial revision. At startup `agent_runs.rs` converts `read_agent_todos` errors to `None`. The model tool forwards that value to the lower-level writer, where `None` intentionally bypasses revision checking. A malformed/unreadable list, or another room's later repair, can therefore be replaced without observing its contents.

A normally missing file already reads as an empty list with a valid revision. Preserve that first-use behavior, existing compare-and-swap conflicts and explicit merged retries.

## Ownership and interfaces

Own `hosts/rust-daemon/src/tools/todo.rs`, including its existing inline tests. Keep `AgentTodos`, `read_agent_todos`, `write_agent_todos(..., Option<&str>)`, `ToolExecutionContext.todo_revision`, tool schemas and owner HTTP contracts unchanged. No production change to `agent_runs.rs` or `tools.rs` is expected.

## Rules

1. After validating tool arguments, require a captured revision before calling the writer.
2. With no baseline, return a distinct truthful error: nothing was saved; use `todo_read`, merge the current list, then write explicitly.
3. Refusal does not write, create directories, read automatically, seed a revision or retry. The context remains without a baseline.
4. Do not use `TASKS_CHANGED_PREFIX` for this refusal: that prefix enters the existing conflict/read-refresh branch.
5. Successful `todo_read` supplies the baseline; a failed read supplies no new authority. Do not repair malformed bytes or treat them as empty.
6. With a baseline, retain stale-write refusal, latest-list response, baseline refresh and explicit merged retry.
7. Keep direct `write_agent_todos(..., None)` behavior for explicit owner/internal callers and its existing path/item validation.
8. Do not block the whole run or add automatic task repair, inference, schema changes or new dependencies.

## Reuse decision and provenance

Controller inspected public `tools/todo_tool.py` at installed Hermes commit `1fd18277267152893a8a24792b79213e7ca3f899`. Its `TodoStore` is per-session in-memory revisioned state with replacement/merge, snapshots and compression injection. Anima's existing per-agent durable list already integrates workspace containment, atomic writes, revisions and cross-room conflict recovery. Reusing that Rust path and closing the model-boundary bypass is smaller than porting a different store or adding a process adapter. No upstream code is copied; existing notices remain.

## Meaningful regression list

1. Replace the existing model test that accepts blind writes: existing valid tasks and absent baseline must return an error, preserve exact bytes and retain `None`.
2. Malformed initial bytes: baseline read fails; attempted model write and failed `todo_read` preserve exact bytes and absent baseline.
3. Another room repairs/updates after the failed read: the old run still refuses. Explicit `todo_read` followed by a merged write succeeds and retains the other room's task.
4. Ordinary missing file: a successful empty read supplies a revision; first model write succeeds. A blind write with no baseline creates no task directory.
5. Existing stale revision: preserve the current concurrent-room conflict and explicit merged-retry regression.
6. Direct unconditional writer remains supported; assert its explicit update still succeeds independently of the model refusal.

Use bounded disposable test roots, exact byte/revision assertions and no network, providers, services or wall-clock sleeps. Existing test fixture cleanup must settle before assertions where applicable.

## Implementation sequence

- Implementer writes the regression changes only, reports ownership and stops before production behavior changes.
- Controller records meaningful RED using the established Nx filtered-test forwarding.
- Implementer adds the narrow model-boundary refusal; retain lower-level writer/conflict logic.
- Controller freezes source, requests one daemon batch review, closes one combined fix wave and runs final gates.
- Update the master row, handover and selected task-use docs; explicit unsigned commit/push/PR/attach/standing merge with exact-tree proof.

## Validation commands

Use the current isolated Windows controller and verified Nx project targets. All Cargo stays incremental0/jobs1/test-threads2 and serialized.

```text
bun x nx run rust-daemon:test --skipNxCache --excludeTaskDependencies --args="--lib tools::todo::agent_tests -- --test-threads=2"
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run rust-daemon:build --skipNxCache
```

The lint target checks Cargo formatting. Keep touched Markdown Prettier-clean and run scoped docs consistency checks. No web/SDK changes or gates are planned. Preserve unrelated `nx.json`, `anima.yaml` and historical raw/Git/runtime evidence; never stage `.superpowers`.

## Top five risks

1. Missing file confused with read failure, preventing ordinary first-use tasks.
2. Automatic baseline seeding makes an unread list writable again.
3. Narrow model check accidentally changes owner/internal writer authority.
4. Existing stale-write/merged-retry handling regresses.
5. Misleading error implies repair or rollback while hidden effects occur.

## Controller rulings on the risks (binding)

1. Ruling: preserve the successful missing-file empty revision and require it for first model writes — absence is already represented by a real read result — cost if wrong: normal first-use task creation could fail. Test both blind refusal and explicit empty-read success.
2. Ruling: refuse at the model entry point without reading or seeding a baseline automatically — a repaired/updated list must be observed before replacement — cost if wrong: another room's task progress can be lost. Preserve exact bytes and `None` until explicit `todo_read` succeeds.
3. Ruling: leave the low-level optional-revision writer and owner API unchanged — their explicit authority differs from a model run's missing observation — cost if wrong: legitimate owner/internal saves can break. Keep a direct unconditional-writer regression.
4. Ruling: reuse the existing CAS/conflict/read-refresh/explicit merged retry — the integrated Rust mechanism already isolates concurrent rooms — cost if wrong: conflicts may overwrite or permanently block task progress. Retain the existing concurrency test and exercise repair/read/merge.
5. Ruling: return a distinct actionable refusal and preserve failed-read bytes without recovery or retry — the model needs a truthful next step, not a silent repair claim — cost if wrong: corrupted or uncertain state can be mistaken for an empty successfully saved list. Keep all effects and result assertions deterministic.

Ruling: keep this delegated plan proportionate to a one-file boundary fix — the user requests cheap continuous progress and a lean plan — cost if wrong: broader API/concurrency requirements must be recorded before expanding ownership. No 1000-line boilerplate, new acceptance harness or redundant audit is needed.
