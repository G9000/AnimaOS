# Discord interactions implementation plan

**Goal:** Connect one bot without mandatory channel IDs, offer a verified invite link, support mention-triggered channel conversations, threads and an optional home channel.

**Authorization:** Implement on the existing `main` checkout. Do not stage, commit, create worktrees, publish or send live Discord messages. Preserve unrelated Astro generated-file edits.

**Architecture:** Keep the Rust daemon as execution owner. Retain existing credentials, epoch fencing, durable inbound receipts and outbound uncertainty. A bot connection discovers authorized Discord destinations; each channel/thread has its own durable cursor and native conversation. Existing selected-channel connections keep their stored conversations and receipts. Access policy and response-trigger policy are separate. Home is an explicit outbound destination, never an implicit reply redirect.

## Frozen wire contract

- Creation retains `botToken`, optional `guildId`, optional `channelId`, optional `approvedUserIds` (empty resolves to verified application owners) and accepts optional `settings`. Existing `privateChannelConfirmed` is accepted for compatibility but is no longer mandatory.
- `settings`: `allowedChannelIds: string[]`, `freeResponseChannelIds: string[]`, `approvedRoleIds: string[]`, `homeChannelId: string | null`, `requireMention: boolean` (true), `autoThread: boolean` (true), `threadRequireMention: boolean` (false). Missing settings on existing stored connections preserves their previous inline behavior. New creation applies the defaults above.
- Connector responses keep existing fields and add `settings`, `botWide`, `inviteUrl`, and `conversations` (`channelId`, `guildId`, `parentChannelId`, `name`, `roomId`); the root `channel` is nullable for bot-wide connections on the wire.
- `POST /api/agents/{agent_id}/connectors/discord/check` with `{botToken}` returns `{applicationId, bot, ownerUserIds, messageContentEnabled, inviteUrl}`. Owner-authenticated and bounded; never echoes or persists the token.
- `PUT /api/agents/{agent_id}/connectors/discord/{connector_id}/settings` accepts `{settings, approvedUserIds}` and returns the connector envelope. Configuration changes revoke captured work, validate destinations and restart a previously enabled worker safely.
- Internal manager interfaces: `check_bot(token)`, `create_with_settings(agent, token, guild: Option<String>, channel: Option<String>, users, settings)`, `configure(agent, id, settings, users)`. Runtime owns concrete model definitions.

## Work and ownership

- [x] Runtime: native REST application check, channel/thread discovery and creation; optional channel bot connection; durable destination conversations/cursors; admission by users/roles plus mention/free-response policy; thread participation; per-conversation processing, typing, delivery and recovery; lifecycle/snapshot validation and regressions. Ownership: `hosts/rust-daemon/src/connectors/discord/**`, and required Rust fixture adjustments.
- [x] UI/SDK: typed contract and endpoints, token-check and invite flow, optional channel selection, default home and response/access settings, conversation links and focused regressions. Ownership: `packages/sdk/**`, `apps/web/**` except unrelated generated files.
- [x] Integration: owner routes/contracts/OpenAPI, native session routing, proactive home delivery path and documentation/attribution. Ownership: remaining daemon routes/sessions/automation integration plus this plan.
- [x] Review the combined implementation for requirements, routing isolation, authorization, persistence and restart behavior; fix findings in one bounded wave.
- [x] Run current-tree Nx Rust host tests, relevant SDK/web tests and typechecks, scoped formatting and diff checks. Record scoped passes and broader failures separately; live Discord acceptance stays unverified.

## Behavioral acceptance

No selected channel is required. Only authorized users/roles can trigger execution; a mention never grants authority. Allowed parent-channel rules include their threads. Mentions start conversations and optionally threads; an existing participating thread continues naturally unless configured to require mentions. Free-response channels reply inline. Distinct threads never share history or redirect replies into their parent or home channel. Thread creation failure does not silently change the reply destination. Disable, configuration changes and credentials revoke stale work. Recovery does not re-run completed messages or resend uncertain deliveries. Setup invites request only capabilities used by this implementation.

## Validation commands

Use `NX_DAEMON=false NX_ISOLATE_PLUGINS=false NX_SKIP_NATIVE_FILE_CACHE=true` if needed by this Windows installation. Confirm resolved targets through `bun x nx show project ... --json`.

- `bun x nx run rust-daemon:test --skipNxCache`
- If the running executable is locked, use the documented `CI=1 CARGO_TARGET_DIR=target/validation-rust-daemon` fallback and serialize Rust compilation.
- Relevant SDK/web Nx tests and typechecks, using focused Discord filters before the broader checks.
- Scoped Rust/TypeScript/Markdown formatting and `git diff --check`.

## Implementation and verification record

The implementation remains on `main`, unstaged and uncommitted. Concurrent generic-email changes in this checkout are separate work and have been preserved. No live Discord message, daemon restart or real control-plane store migration was performed.

The independent runtime review found no remaining actionable P1/P2 findings after the bounded fix wave. The combined UI/runtime review also checked the owner boundary, saved conversation destinations and the 16 KiB settings body limit. Static review is separate from the test results below.

- The daemon-only run (`rust-daemon:test --excludeTaskDependencies --skipNxCache`) passed all **95 Discord-related checks**, including owner replies in dynamic threads, history/outbox isolation, role/mention gates, uncertain thread creation, queue recovery, persistence rollback, home delivery and schedule fencing. The complete library result was **2,655 passed, 10 failed, 29 ignored**. One failure was the Discord API inventory count/table, corrected afterward and checked separately below. The remaining failures cover four existing Windows worktree checks, bundled-skill line endings, platform cache paths and three concurrent email/session-contract checks. Cargo stopped at the failed library suite before integration tests ran.
- Required `rust-daemon:test --skipNxCache` remains blocked in its `core-rust:test` dependency: **4 passed, 10 failed** in `anima-worktree`, with Windows `git worktree add` rejecting `//?/C:/...` paths.
- Deterministic SDK suite: **230 passed, 3 service integration tests skipped** with `ANIMA_SKIP_SERVICE_TESTS=1`.
- Focused web suite: **46 passed** across Discord helpers, settings, integration hook, automation editor and automation page. The UI worker separately passed the native channel/thread ViewHarness regressions.
- `@animaos/web:typecheck --skipNxCache` passed, including SDK/UI dependency checks.
- Final API documentation check (`rust-daemon:test --excludeTaskDependencies --skipNxCache --args='--lib docs_contracts'`): **2 passed, 0 failed, 1 inventory-printing helper ignored**. The README lists all 236 registered operations, including both new Discord endpoints, and historical route-preservation checks remain intact.
- Scoped Rust formatting, Markdown formatting and `git diff --check` passed. Global `rust-daemon:lint` still reports formatting drift outside this change's scoped checks.

Known acceptance boundary: offline recovery covers saved conversations. Previously unseen channels are discovered from new Gateway messages. Real Discord permissions, invite acceptance, presence, typing, delivery and model-provider acceptance remain unverified.

Existing selected-channel connections keep their mode. Switching the same bot to bot-wide routing requires deleting the old connection and creating a connection without a selected channel; saved conversation history is retained. The setup guide documents this transition.
