# M81: Fresh owner Tasks across disconnects

> Delegated lean plan with binding controller rulings. Start only after M80 merges. Two disjoint source lanes, tests first, one final web/SDK-contract review, one combined fix wave, then gates. No separate preflight audit or actual acceptance harness.

## Goal and confirmed gap

The Tasks editor and Work hub receive no online state. Their delayed reads/saves can publish after a disconnect, controls still dispatch offline and a same-agent reconnect performs no fresh read. Close this owner-console gap while preserving drafts and the current revision-based API.

No daemon, SDK, persistence, task-execution, provider or broader workspace redesign.

## Ownership and interfaces

- Lane A: `apps/web/src/components/AgentWork.tsx` and `AgentWork.test.tsx` — editor lifecycle, generations, retained saves and draft policy.
- Lane B: `WorkspaceHub.tsx`, `WorkspaceShell.tsx`, `SettingsPanel.tsx` and their existing tests under `apps/web/src/components/`, plus `apps/web/src/ViewHarness.tsx` and `ViewHarness.test.tsx` — minimal connection plumbing and Tasks-list read fencing.
- Add required `online: boolean` props to the Tasks editor, hub and Settings panel; wire actual callers. Do not default missing connectivity to online. Preserve existing `key={agent.id}` editor keys.
- Reuse `daemon.agentTasks(agentId)` and `updateAgentTasks(agentId, { tasks, revision })`; no SDK schema/cancellation API changes.

## Reuse decision

Reuse M78's local current-context checks, synchronous visible-state projection and StrictMode-safe effect ordering, plus the existing task CAS/error/running behavior. Controller compared installed Hermes public `tools/todo_tool.py` at `1fd18277267152893a8a24792b79213e7ca3f899`: its per-session in-memory store does not solve this React/network lifecycle issue. No upstream code copy, process adapter or dependency is needed; existing notices remain.

## Request and context rules

1. Each mount and agent/online transition owns a distinct generation. Identical values after reconnect must not revive an earlier online generation.
2. Initially offline, Refresh and Save dispatch no GET/PUT; disable edits/add/remove/status/save/refresh appropriately and show an offline state.
3. Project current/fresh state synchronously before effects. Old saved rows must not appear current or editable during revalidation.
4. Current online contexts load fresh tasks; reconnect revalidates once. Accept read/save success, errors and callbacks only for the owning context, including invocation checks.
5. An already submitted PUT keeps its original agent, bytes and revision exactly once. No automatic retry, redirect or follow-up read from an obsolete completion; do not claim rollback.
6. Keep physical pending-save ownership across connection transitions. Do not permit another save just because the display context changed. Old completion may release its own pending reservation, never another operation or newer state.
7. Scope `onBusyChange` to its editor/operation and settle parent navigation ownership on cleanup. Preserve same-context save/conflict behavior, running-agent drafts/save restriction and StrictMode usability.
8. Fence only the hub's Tasks reads/results; preserve other sections' behavior.

## Explicit draft policy

- Preserve same-agent unsaved task edits and unfinished New agent task text across a temporary disconnect. Keep drafts visibly read-only and labelled unsaved while offline/revalidating.
- Retain the original draft revision. Reconnect with the same saved revision can restore editing without changing draft bytes.
- If the fresh revision changed, keep the draft and conflict/review notice; keep Save disabled. Never automatically merge or attach a fresh revision to old draft content.
- Offer **Discard draft and load saved tasks**, explicitly replacing the draft with the already fetched snapshot without another GET.
- A dirty manual Refresh must similarly make discard explicit before replacing content. Keep existing clean Refresh behavior.
- Do not carry drafts across agents or add persistent draft/navigation infrastructure.

## Meaningful deferred regressions

1. Initially offline editor/hub: no GET; controls cannot dispatch reads/writes.
2. Old read success/error across disconnect/reconnect cannot replace current loading/error/data; assert one fresh reconnect GET.
3. Dirty unchanged-revision reconnect preserves edits and unfinished input, no auto PUT; editing resumes only after revalidation.
4. Changed-revision reconnect preserves draft, disables Save and requires explicit discard before using fresh data; no silent revision rebasing or second GET.
5. Parameterized pending-save success/conflict/error: one original-target PUT, no stale publication, retry or follow-up read.
6. Busy/unmount ownership: no duplicate pending PUT, stale clear of a newer operation, redirected target or permanently busy parent.
7. Agent switch and root StrictMode: no foreign draft/list; usable current read while discarded replay response stays ignored.
8. Existing same-context success/conflict, running restriction and Refresh remain; integration assertions prove both actual connection entry points.

Use deferred promises/endpoint counts and existing fixtures, with no timers or live services. Run tests-only lanes before releasing production changes; root records meaningful behavior RED and owns validation.

## Gates and publication

Use the verified web Nx test forwarding for the five owned test files, then:

```text
bun x nx run @animaOS-SWARM/web:test --skipNxCache
bun x nx run @animaOS-SWARM/web:typecheck --skipNxCache
bun x nx run @animaOS-SWARM/web:build --skipNxCache
```

Prettier-check touched source/docs and run scoped docs consistency. No Rust/SDK gates or actual browser/daemon/account/provider/profile activation. Explicit paths only; never stage nx.json/anima.yaml/.superpowers. Preserve historical raw/Git/runtime evidence and unrelated work. Standing unsigned per-command commits, PR attachment and merge-and-continue apply.

## Top five risks

1. Incidental reconnect/refresh draft loss.
2. Silent revision rebasing permits unreviewed replacement.
3. Duplicate or redirected pending saves after context transitions.
4. Stale state/errors/busy callbacks and StrictMode lifecycle mistakes.
5. Missing a real entry point or broadening unrelated hub behavior.

## Controller rulings on the risks (binding)

1. Ruling: preserve same-agent drafts and unfinished input read-only through disconnect/revalidation — transient network loss must not destroy owner work — cost if wrong: typed tasks disappear. Any destructive replacement requires the explicit discard action.
2. Ruling: keep the draft's original revision until fresh equality or explicit discard — a newer server revision does not authorize overwriting it with an unseen merge — cost if wrong: another room's tasks can be replaced. Changed revisions keep Save disabled and apply the already fetched snapshot only after deliberate discard.
3. Ruling: retain one physical pending save and its original payload across generations — connection changes cannot authorize another submission or redirect an old one — cost if wrong: duplicate effects or misleading success. No retries/follow-up GET from stale completion; release only that operation's reservation.
4. Ruling: use distinct current generations plus operation ownership at invocation/completion and StrictMode-safe setup — old online values and late callbacks cannot regain authority — cost if wrong: stale rows/errors/busy state appear fresh or block navigation. Test both late outcomes and root StrictMode.
5. Ruling: pass required connectivity through both existing callers and limit hub changes to Tasks — a component-only fix misses actual daily-use routes — cost if wrong: one entry point remains unsafe or unrelated screens regress. Own only the ten listed source/test candidates; publish only actual changes.

Ruling: reuse the integrated Rust revision API and local M78 lifecycle pattern — a new store/SDK or general offline framework adds cost without closing this UI defect — cost if wrong: any required contract expansion must be recorded and reviewed before widening scope.
