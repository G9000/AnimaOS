# M78: Fresh Memory state after reconnect — Implementation Plan

> M77 is merged. Execute this bounded three-file fix with one final web/SDK-contract review and one combined fix wave, then local gates.

**Goal:** Prevent an old Memory request or completed mutation from publishing stale rows, errors or evidence, or initiating reads after its owner context has changed.

**Architecture:** Keep the existing `useMemory` interface and daemon methods. Bind reads, actions and displayed state to the current mounted instance, agent, online state and epoch; use separate read generations for search/refresh ordering. Submitted mutations retain their original target and settle once.

**Tech stack:** React/TypeScript, existing SDK facade, Vitest/Testing Library and Nx web targets. No dependency change.

## Confirmed problem and scope

`useMemory.load` currently checks only agent ID. An edit/delete completing after disconnect still calls four read endpoints; disconnect does not invalidate pending reads or loaded state. Old editable rows can reappear on reconnect before fresh data arrives. Action errors and trace results also lack a captured owner-context check.

This closes a bounded PAR-12 owner-console lifecycle gap. It does not establish memory quality, provider readiness, general restart recovery, or exactly-once server mutations.

Owned public files:

- `apps/web/src/hooks/useMemory.ts`: context fencing, fresh visible state, read/action completion handling.
- `apps/web/src/hooks/useMemory.test.tsx`: deferred read/mutation/evidence regression tests.
- `apps/web/src/pages/MemoryPage.test.tsx`: actual page transitions and stale-row editability checks.

`MemoryPage.tsx` remains unchanged unless the hook cannot supply the required synchronous empty/loading projection; root decides any demonstrated exception before expanding ownership. No daemon, SDK, persisted schema, grants, provider, account, profile, browser or process-ownership changes.

## Reuse and interfaces

Reuse the M76 history boundary fencing and `MemorySearchStatus.tsx` current-context checks as local patterns, without copying their separate request coordination. A Hermes Python port or another adapter adds no value to this React lifecycle fix; pinned Hermes comparison material remains `1fd18277267152893a8a24792b79213e7ca3f899`, with no new upstream copy or dependency.

Keep `MemoryOptions` and `MemoryView` unchanged. Preserve search text, include-replaced preference and existing same-context user interactions. No new auto-save, retry, acknowledgement, or read endpoint.

## Rules

1. Capture a context identity for each mounted lifetime and each change of `[agentId, enabled, epoch]`. Compare structured fields or a collision-safe tuple, plus a generation that prevents an old online context from matching after offline/reconnect. Unmount, null agent and offline invalidate prior work even when they start no replacement GET.
2. Every `load` entry checks the current mounted, online, non-null-agent context before issuing any of the four reads. This includes Refresh, search-triggered loads, conflict/404 recovery and post-mutation refresh. Capture the read context and sequence before dispatch; accept settled data/errors only for that exact current context and latest read sequence. Search and include-replaced ordering remain covered by the read sequence.
3. On reconnect, agent change or epoch change, the hook's returned view synchronously has `loaded=false`, empty memories/facts/entities/relationships and no errors from the prior context. Do not rely only on an effect clearing state after render. Keep that projection until the fresh batch settles. Offline/null-agent views also expose no old editable data.
4. Freeze the action's origin context before dispatch. A submitted mutation keeps its original ID/content/patch, is neither cancelled nor retried, and never gets redirected to a new agent. If its context is no longer current, suppress its success acknowledgement, action error and follow-up reads. Return the existing non-success/null shape to prevent stale completion from driving current UI; this does not mean the server rolled the mutation back. Guard stale callers at invocation as well as completion.
5. Fence evidence/trace success and errors by the same origin context. A late trace cannot populate a new owner's evidence or cause a recovery GET. A stale promise settling after unmount must not update state or dispatch reads.
6. Within the same valid context, preserve ordinary success reload, 404/409 reload, typed gone/conflict messages, and separate read/action errors. Successful reload must not erase a refused action; successful action must not conceal a failed read. Preserve successful sections when another endpoint fails, but never retain another context's rows as a partial-success fallback. A failed batch can finish loading and show its honest error; it does not become an all-success snapshot.
7. Reconnect starts one fresh four-endpoint batch for the current context. Query/filter changes retain existing behavior. An old completion cannot trigger a duplicate batch, overwrite fresh results, or manufacture cancellation of an already-submitted mutation. Do not add SDK cancellation APIs merely for this fix.

## Implementation and meaningful RED tests

- [ ] Add deferred-promise tests before changing the hook. Root records RED from the actual current behavior, not a missing import or malformed fixture.
- [ ] Cover online edit and delete followed by offline, then success or 409/404: exactly one original mutation; no follow-up GET while offline; no stale action error/acknowledgement. Repeat representative success/conflict cases after unmount.
- [ ] Start a list read, disconnect or set agent null, settle it, then reconnect: old completion cannot publish; reconnect starts exactly one fresh batch. Assert the returned view is immediately unloaded/empty before fresh promises settle.
- [ ] Independently supersede agent and epoch while reads/actions are pending. Deliver old success and error after the new result; only new rows/errors survive. Include online -> offline -> online with the same agent/epoch to catch tuple reuse.
- [ ] Verify Refresh/search while offline issue zero GETs; explicit reads after unmount and stale action callbacks cannot dispatch. Verify late trace success/error is ignored and late trace 404 makes no recovery GET.
- [ ] Retain same-context success/conflict/404 reload tests; add a partial batch case proving successful current sections remain usable and read/action errors stay independent. Assert exact endpoint and mutation counts so a no-op implementation cannot pass.
- [ ] In `MemoryPage.test.tsx`, load an editable row, disconnect, reconnect with the new batch deferred, and assert the old row/edit/delete controls are absent while Loading is shown; then publish only fresh rows. Add an independent agent-switch case and keep existing offline/profile/evidence behavior passing.
- [ ] Implement the smallest hook-only fix using captured context and visible-state projection. No general cache, new hook framework, or unrelated cleanup.
- [ ] Freeze the three changed sources, perform one bounded combined review, apply one justified fix wave, then run final affected gates.

## Root-owned validation and publication

Root verifies focused-test forwarding against the existing web project configuration before choosing a filter. Run the two owned test files once for RED and focused GREEN; retain actual outputs. Final gates:

- `bun x nx run @animaOS-SWARM/web:test --skipNxCache`
- `bun x nx run @animaOS-SWARM/web:typecheck --skipNxCache`
- `bun x nx run @animaOS-SWARM/web:build --skipNxCache`
- `bun x prettier --check apps/web/src/hooks/useMemory.ts apps/web/src/hooks/useMemory.test.tsx apps/web/src/pages/MemoryPage.test.tsx`

No Rust, SDK, E2E browser, provider or ordinary-service activation is required. Root owns formatting, commands, Git, the published plan/status links and private evidence. Record this as local deterministic UI behavior, with no model-quality or live-host claim.

## Top five risks for root controller rulings

1. An old context can become equal again after reconnect: use a fresh lifetime/generation, not only value equality. Cost if wrong: stale rows or evidence can revive.
2. Clearing state only in effects leaves one render with editable stale data: project visible state against current inputs synchronously. Cost if wrong: owner actions can target data from the previous selection.
3. Treating obsolete UI work as cancelled server work can duplicate mutations: retain one original submission and suppress only stale UI/follow-up reads. Cost if wrong: edits/deletes could be retried or misreported as rolled back.
4. A late mutation/trace completion can contaminate another agent or epoch: freeze and check its origin before every state update and recovery read. Cost if wrong: current errors/evidence or network activity can belong to old work.
5. Overbroad reset logic can erase legitimate partial data or action errors: preserve same-context partial successes and separate error channels, with negative and positive deferred tests. Cost if wrong: freshness fixes can hide actual mutation/read failures.

No extra preflight audit. Root commands and explicit publication remain serialized.

## Controller rulings on the risks (binding)

1. Ruling: give every owner-context transition a fresh generation, including offline and null-agent transitions — equal agent/epoch values after reconnect cannot revive prior requests — cost if wrong: stale rows, evidence or actions may be accepted as current.
2. Ruling: synchronously project an empty unloaded view until the current context has a result — effect-only clearing leaves an editable stale render — cost if wrong: the owner can act on the previous selection before fresh reads settle.
3. Ruling: preserve every submitted mutation exactly once and suppress obsolete UI acknowledgements only — stale success returns false/null to the obsolete caller and does not imply server rollback — cost if wrong: duplicate edits/deletes or misleading cancellation may occur. Never retry, redirect or re-submit a mutation.
4. Ruling: fence action invocation, completion, errors, trace evidence and recovery reads by the captured context — old callbacks and late promises must not create work or state in a new context — cost if wrong: cross-context UI contamination or offline GETs remain.
5. Ruling: preserve same-context partial reads and independent action/read errors — fresh empty context state cannot borrow failed categories from an older scope — cost if wrong: legitimate current data or refused actions can disappear. Verify deferred negative and positive behaviors with endpoint/mutation counts.

Use the existing daemon facade; no cancellation API, general cache or hook framework.
Only the hook and its two existing test files are released. A demonstrated need for
production page changes requires a recorded root ruling before expanding ownership.
No actual browser, daemon, provider, account, device, database or container is planned.
