# Chat suite review guide

Review the chat-suite commit against its parent. This guide supports a later independent Claude review; it does not claim that Claude has reviewed the implementation.

## Scope and boundaries

This change adds conversation model/reasoning preferences, closed interactive results with durable pins, tool-free second opinions, effective-tool discovery and credential-protected read-only transcript sharing. See [the user guide](../companion/chat-suite.md) for behavior and route contracts.

Execution remains in Rust. The SDK and web surface are thin clients. Separate helper/delegation policy and appearance work in the original development checkout is excluded from this commit. The standalone persistence version is 29.

## Review order

1. Trace owner and viewer authentication in `routes/http.rs`, `routes/mod.rs`, `routes/conversation_shares.rs` and `conversation_shares.rs`. Check exact route/method exemptions, global rejection of viewer credentials on owner routes, digest-only storage, fail-closed revocation, cursor isolation and revalidation after asynchronous history reads. Inspect `AppEntry.tsx`, `OwnerApiKeyGate.tsx`, `SharedConversation.tsx`, `owner-api-key.ts` and `useDaemonImage.ts` for transport isolation and secret handling.
2. Trace model resolution from `state/run_commit.rs` through `chat_models.rs`, the run ledger and helper launch hooks. Confirm next-turn scope, inherited/default behavior, effort validation, fallback accounting and unchanged approvals. Check provider request construction in `anima-model-adapters` and the model-preference hook's stale-response handling.
3. Inspect `chat_review.rs` and its routes. Confirm distinct-model checks, bounded and untrusted evidence, empty tool authority, durable start/finish, cancellation before worker launch, persistence rollback, generation/profile ownership fences, shared budgets and restart interruption without replay.
4. Inspect `chat_results.rs`, `agent_runs/chat_results.rs`, `tools/chat_results.rs`, routes and `ChatResultsPanel.tsx`. Check bounded tagged payloads, calculator AST validation, absence of dynamic code execution, direct-owner scope, provenance, idempotency, revision conflicts and stale pin responses.
5. Compare `agent_runs/tool_scope.rs` with execution in `agent_runs.rs` and the frozen descriptor set in `live/registry.rs`. Confirm idle and active capability parity, current revocation/readiness checks, approval-policy caveats and omission of secrets.
6. Review `SessionRecord`, clear/delete/revision flows and `control_plane_store.rs`. Check generation resets, preference preservation, noncopying of grants/results/reviews into a revision, and exact pre-upgrade backup bytes.
7. Follow UI integration through `ViewHarness.tsx`, `SessionView.tsx`, `ChatExtensions.tsx`, composer and slash commands. Switch conversations and clear a conversation while requests are pending. Confirm opening a review panel does not call a provider.

## Regression evidence to inspect

Native tests cover model snapshot semantics, helper inheritance, review persistence/cancellation races, cross-chat authorization, scope selection and revocation, result validation/idempotency/revisions/rollback, sharing authentication/projection/cursors/generation/revocation and version-28 backup preservation. SDK tests cover route encoding, payloads and separate viewer transport. Web tests cover controls, report interactions, viewer gating, owner-key handling, authenticated images and stale asynchronous responses. Browser acceptance exercises the production bundle at desktop and mobile sizes, normal toolbar clicks, interactive reports, viewer revocation and same-page owner/viewer transitions. These browser cases caught and now guard the report-wrapper stacking and owner-route redirect races.

Run serially for Cargo:

```text
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run @animaos/sdk:test --run --skipNxCache
bun x nx run @animaos/web:test --run --maxWorkers=2 --skipNxCache
bun x nx run @animaos/web:typecheck --skipNxCache
bun x nx run @animaos/web:build --skipNxCache
bun x nx run @animaos/web-e2e:typecheck --skipNxCache
bun x nx run @animaos/web-e2e:e2e-console --grep=chat-suite --skipNxCache
```

On Windows, use an isolated `CARGO_TARGET_DIR` when an existing daemon locks a build executable. Record commands, exit codes and any ignored or environment-dependent tests in the delivery report. Deterministic tests do not establish live provider response quality, billing, remote viewer network reachability or acceptance on other operating systems.

The final task-only snapshot passed the complete Rust target (core, 3,213 daemon unit tests, and all integration groups), Rust formatting, 266 SDK tests, 2,260 web tests, all four browser acceptance cases, both web/e2e typechecks and the production web build. One web test and the existing explicit live/installed-runtime Rust cases remained skipped or ignored. Windows validation used Node 24, `CI=1`, four Rust test threads and a separate Cargo target directory; two web workers avoided contention during native compilation.

An existing `agent_api` assertion, `run_agent_does_not_reuse_previous_tool_result_between_runs`, intermittently failed during validation (3 of 100 isolated repetitions). Hybrid memory recall can legitimately return an earlier TaskResult because generated memory-ID tokens collide in the local embedding hash. Its negative text assertion therefore does not establish stale tool-result reuse. The affected recall/model/test sources are unchanged by this feature. Its assertions were preserved; the complete final Rust run passed. This is a baseline test caveat for reviewers, not a claimed memory-quality fix.

## Manual acceptance

- Select an explicit primary model, admit a turn, change selection during it and verify only the next admitted turn changes. Return to inherit and reload.
- Create and pin each result kind, reload, update it using the latest revision, then race two revisions and confirm one conflicts. Switch chats while a pin request is pending.
- Configure a distinct reviewer, request a focused review, cancel it and restart during another review. Confirm no tool activity or automatic replay.
- Compare Tools while idle and active; remove a grant or disconnect MCP during an active turn and confirm availability decreases without adding new descriptors.
- With an owner key enabled, create viewer access. In a separate browser context, verify the link alone grants nothing, correct credentials show only the projected transcript, owner endpoints reject viewer headers, and revoke/clear ends subsequent reads. Verify owner avatars still load.

## Known limits

Sharing uses owner-issued viewer credentials because this daemon has no account identity service. It grants future text access to one conversation until revoke/clear and requires network reachability. Results deliberately support only four bounded data formats. Reviews use limited saved evidence and cannot attest to external side effects. Unsupported provider/model reasoning efforts remain unavailable rather than accepting arbitrary values.
