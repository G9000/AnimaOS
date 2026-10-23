# Chat controls, results, review, capabilities, and sharing

Approved scope: all five additions in the continuing chat brief. Preserve unrelated dirty helper/delegation, personality, authentication, and appearance work. Starting HEAD: `09b42325`; starting state captured privately under `.tmp/chat-suite-start`.

## Contracts

- Session model preferences have explicit inherit/override states. Resolve at durable turn start; queued turns use the preferences at that boundary, admitted turns retain their snapshot. Never write companion defaults from a chat control. Validate supported reasoning efforts.
- Interactive results are closed data payloads (chart, comparison, document, calculator), with session/generation ownership, durable IDs, optimistic revisions, inline rendering and persisted pin state. Generated content has no code execution or host APIs.
- Second opinions are bounded, tool-free calls over selected chat evidence. A distinct configurable reviewer reports findings and limitations in the origin chat. Failure, cancellation and unavailable reviewers cannot imply approval or trigger an action.
- Capabilities use execution's effective tool selection. Running turns have an immutable descriptor ceiling intersected with current grants/readiness. Expose approval requirements as conditional on actual arguments; omit credentials and endpoints.
- Sharing uses separate owner-issued viewer credentials and non-secret links because current daemon auth has no user-account identity system. Digest-only credential persistence, one selected conversation per grant, revoke and generation checks on every read, separate minimal transcript projection and viewer page. All owner routes reject viewer credentials. No execution, tools, attachments, effects, metadata, or private file access through the viewer API.

## Ownership and sequence

1. Parallel read-only audits: auth/sharing; results/previews; models/review. Root inspects runtime tool scope and web integration.
2. Sharing agent owns new sharing service/routes/client/viewer/management components and isolated tests.
3. Results agent owns new results service/tool/routes/client/renderer and isolated tests.
4. Models/review agent owns model resolution/adapters, review lifecycle/routes/client/controls and isolated tests.
5. Root owns shared module/router/client exports, tool registry, session command/navigation glue, capabilities, integration, docs, verification and publishing. Shared SessionRecord additions use separate fields; no broad formatting of unrelated dirty files.
6. Independent reviewer audits integrated scope, permissions, durability, races, invalid input, cross-chat isolation, and UI behavior. Fix findings, then rerun affected checks.

## Validation and delivery

- Confirm Nx project targets (`rust-daemon`, `@animaos/sdk`, `@animaos/web`). Native plugin/socket setup requires execution outside current filesystem sandbox.
- Serialize native builds. Run `bun x nx run rust-daemon:test --skipNxCache`, using isolated target directory only if needed for a running binary lock. Run relevant SDK/web tests, builds and typechecks. Record exact command/results, distinguish live-provider acceptance from deterministic tests.
- Inspect UI where feasible, including current-chat selection, switching chats during pending requests, pinned reports and standalone viewer authorization.
- Prepare a review guide covering contracts, routes and regression evidence. Commit only task-owned hunks with configured signing; push and verify remote SHA under existing authorization.

## Delivery checklist

- [x] Implement all five native/SDK/web feature contracts.
- [x] Complete independent implementation review and fix its findings.
- [x] Validate real browser clicks, reports and viewer isolation on desktop/mobile.
- [x] Write owner documentation and a guide for later Claude review.
- [x] Finish the final isolated native and web verification runs.

Publishing follows this checklist: sign only the reviewed task tree, push it, and record the verified remote commit in the delivery message.
