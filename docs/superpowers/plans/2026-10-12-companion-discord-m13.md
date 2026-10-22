# Anima Discord M13 Implementation Plan

> For agentic workers: use superpowers:subagent-driven-development or superpowers:executing-plans task by task. This is a lean executable plan: interfaces, invariants, meaningful tests, commands and ownership; full code only for tricky algorithms. Controller binding rulings supersede draft choices. Checkboxes track implementation, not planning completion.

**Goal:** Let the owner connect Anima to one selected private Discord guild text channel, receive permitted user text through durable polling, converse in its native session, and deliver Anima replies through the native Discord REST API.

**Architecture:** Keep M12's host-neutral associated-type contract and passive compile-in registry. Add typed Discord transport and native durable journals inside the daemon, using the existing run coordinator's commit/rollback boundary and equivalent native authority/claim rules. Preserve Telegram wire formats, claim implementation and behavior. Owner-authenticated SDK/web setup selects the channel and permitted users; no public adapter chooses authority.

**Tech stack:** Existing Rust tokio/axum/reqwest/serde/sha2/zeroize/keyring stack; existing React/TypeScript SDK and web; Bun/Nx. No added dependencies or dependency features.

**Branch/base:** `g9000/companion-discord-m13`, cut from merged main `f7a58084bc6a36273f528882463b4f516ef96bb1` after owner-authorized PR #18 merge. Owner now grants standing continuation/merge authorization for gated milestones serving the selected assistant use case. Root owns branch operations and publication.

## Read first and evidence

- `AGENTS.md`.
- Start-here section of `docs/superpowers/handover/2026-09-28-companion-console.md`.
- Master `docs/superpowers/plans/2026-09-23-companion-console.md`, global constraints and current execution protocol.
- `docs/superpowers/plans/2026-10-12-companion-connectors-m12.md`, especially controller interface freeze and execution outcome.
- Lean task shape in `docs/superpowers/plans/2026-09-23-companion-console-m8.md`.

M12 is implemented: core `ConnectorTransport` has native associated Identity, InboundBatch, SendReceipt and classified Error types; host registry binds the already-captured Telegram credential privately. Core has no HTTP or storage dependency. Host Telegram manager, vault, journal, generation, claim, cancellation, rollback and tests are existing working code. Discord is absent from that durable runtime, owner API and SDK/web. Snapshot version is currently 11.

Reusable core files are `packages/core-rust/crates/anima-core/src/connectors/{mod.rs,registry.rs,tests.rs}`. Existing host references: `connectors/{mod.rs,registry.rs,credentials.rs,telegram.rs}`, `connectors/adapters/telegram.rs`, `connectors/runtime/{configuration,lifecycle,inbound,processing,owner,delivery,workers}.rs`; tests under `connectors/runtime/tests/`. Read narrowly; do not re-audit all M0–M12.

Current sessions and runs explicitly recognize Telegram. Add Discord deliberately to session kind/origin/source, room routing, stop suppression, history and owner-run hooks; never identify a Discord message as Telegram to reuse a branch. Existing mail/calendar are separate native services, not polling transports. Gmail already offers `mail_list_messages` and `mail_create_draft`, local immutable drafts and owner-only sending; scopes are `gmail.readonly` plus `gmail.send`. Calendar already offers reads and reviewed writes with `calendar.events`; it has no polling loop. Drive consent/tools are absent.

### Source adaptation, exact provenance

Local source-only reference: `C:/Users/leoca/AppData/Local/hermes/hermes-agent`.
Verified `.git/HEAD` selects main and `.git/refs/heads/main` is `f3ef9db68332063a3625661f3a9b259f60abdb4e`; root independently verified git HEAD. `LICENSE` is MIT, Copyright (c) 2025 Nous Research. No profile, config, env, credential, session or live process was read; Python was not imported/run.

Actual Discord sources are **`plugins/platforms/discord/`**, not `gateway/platforms/discord.py`:

| Upstream source/symbol                                                                                                                                    | Rust destination/adaptation                                                                                                                                    | Deliberate boundary                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `adapter.py`: `DiscordAdapter.MAX_MESSAGE_LENGTH`, `MAX_SPLIT_MESSAGES`, `_cap_split_chunks`, `send`                                                      | `connectors/discord/text.rs`: scalar-safe 2000-character splitting, cap eight deliveries, final plain Anima truncation notice; complete reply stays in history | Substantial small algorithm adaptation with MIT notice; no discord.py formatting/thread/voice machinery                   |
| `adapter.py`: `_build_allowed_mentions`                                                                                                                   | `connectors/discord/client.rs`: explicit `allowed_mentions: {parse: [], replied_user: false}` on every POST                                                    | Adapt safe mention suppression; Anima also disables user pings; no env/config overrides                                   |
| `adapter.py`: `_discord_message_admission`, `_dispatch_recovered_message`, `_iter_missed_message_backfill_candidates`, `_advance_discord_recovery_cursor` | `connectors/discord/{client,inbound}.rs`: source identities, selected-channel/allowed-user checks, receipts before cursor movement                             | Architectural/behavior adaptation; bot/self/webhook messages never admitted; no role, multiplex-profile or mention bypass |
| `recovery.py`: `DiscordRecoveryStore`, messages/cursors schema                                                                                            | `connectors/discord/model.rs` and existing snapshot store: native receipts, native string snowflake cursor, uncertainty                                        | Behavior reuse only; do not copy SQLite schema/WAL/profile fallback                                                       |
| `plugin.yaml`                                                                                                                                             | Compile-in host descriptor `discord`, display name `Discord`, source `anima-native`                                                                            | Metadata only; no installer, Python loader or env bridge                                                                  |
| `tests/gateway/test_discord_{allowed_mentions,bot_auth_bypass,missed_message_backfill,split_cap}.py`                                                      | Rust fake transport and HTTP fixtures                                                                                                                          | Adapt relevant edge cases; do not execute upstream tests                                                                  |

Receiving `connectors/discord/HERMES-NOTICE` contains full upstream MIT notice, copyright, revision, exact source paths/symbols and adaptation description. Cite it in module headers. Preserve existing `anima-model-adapters/HERMES-NOTICE`. Product names/UI/new public symbols remain Anima/AnimaOS; upstream name appears only in attribution and historical references. Do not claim wholesale adapter parity or copied code for behavior-only reuse.

### Verified protocol sources

Read on 2026-10-12 from official Discord docs. Primary source equivalents for reproducible review:

- [Messages](https://docs.discord.com/developers/resources/message), [source message.mdx](https://github.com/discord/discord-api-docs/blob/main/developers/resources/message.mdx): get channel messages, newest-to-oldest response, before/after/around filters, limit 1–100; create content at most 2000 characters; explicit allowed mentions; nonce/enforce_nonce is recent-message dedup, not durable exactly-once.
- [Channels](https://docs.discord.com/developers/resources/channel): selected channel type and guild identity; GET metadata is not proof that only the owner can view the channel.
- [Rate limits](https://docs.discord.com/developers/topics/rate-limits), [source rate-limits.mdx](https://github.com/discord/discord-api-docs/blob/main/developers/topics/rate-limits.mdx): use returned retry delay and rate headers; bucket/global rules vary; do not invent a fixed per-channel quota.
- [Gateway/intents](https://docs.discord.com/developers/events/gateway): MESSAGE_CONTENT restrictions also affect HTTP responses. REST does not avoid consent/intent restrictions.

No Gateway/WebSocket dependency exists within accepted manifests. M13 is selected-channel **REST polling**, with seconds of latency; it does not observe deleted messages, edit events, reactions, voice, slash commands or threads. Short-lived messages deleted before polling can be missed. Docs/UI state these limits.

## Global rules

- No new dependencies/features, services, providers, keyring reads, databases, containers, Docker Postgres55432 actions or live external sends during implementation validation. Fakes/loopback HTTP only. Operational acceptance remains separately evidenced.
- Preserve unrelated `nx.json` and `anima.yaml`; explicit staging only; no stash/reset/restore/worktree/checkout-path. Root commits with existing per-command unsigned override, never global signing changes or GPG prompts.
- Cargo compilation is serialized and prefixed by `CARGO_INCREMENTAL=0`; use existing target directories. Workers do not run Cargo/Nx Rust tasks or Git operations; root owns all compilation/formatting/gates/commits. Workers prepare source/tests concurrently and provide exact requested focused tests for root's red/green checks.
- Core contains portable contract/metadata only. Discord REST, credentials, durable lifecycle, agent/session binding remain host code. No universal JSON authority envelope, Python orchestration, dynamic ABI, sidecar or fake installed-module feature.
- Every new route authorizes before reading owner state/networking; reads `authorize_read`, mutations `authorize`; `Cache-Control: no-store` including errors; bounded body; OpenAPI paths/schemas. Host errors are static/redacted; upstream text, raw payloads, tokens and authorization headers never reach API, traces or snapshots.
- Registry construction/discovery has no network/vault effects. Reviewed compile-in community adapters have host privileges and require rebuilding; document honestly.
- No preflight audit. Bounded implementers, daemon batch review, final SDK/web review, single combined fix wave, relevant gates. Reuse existing meaningful regressions; do not manufacture one test per checklist sentence.

## Frozen native model and interfaces

Create `hosts/rust-daemon/src/connectors/discord/{mod.rs,model.rs,client.rs,text.rs,credentials.rs,lifecycle.rs,inbound.rs,processing.rs,delivery.rs,tests.rs}` and `connectors/adapters/discord.rs`. Split test file if fixtures grow; do not add a crate/project.

```text
DiscordSnowflake: decimal nonzero u64; serde/API string, numeric comparison.
DiscordBotIdentity { id: String, username: Option<String>, display_name: Option<String> }
DiscordChannelMetadata { id: String, guild_id: String, name: Option<String>, kind: GuildText }
DiscordSenderMetadata { id: String, username: Option<String>, display_name: Option<String> }
DiscordMessage { id, channel_id, guild_id?, sender, text, is_bot, webhook_id?, message_type }
DiscordMessageBatch { messages: Vec<DiscordMessage>, next_cursor: String, boundary_proven: bool }
DiscordSendReceipt { message_id: String, channel_id: String }
DiscordTransportError: static variants with kind(), status/category, retry_after_ms?
  Configuration | InvalidInput | InvalidResponse | Authentication | PermissionDenied
  RateLimited{retry_after_ms,global} | Transport | Uncertain | BacklogLimit
```

Native fields contain necessary provider metadata, never agent/session authority. Bot false/self/webhook/system checks are independent of permitted-user matching. `guild_id` omitted from REST messages is acceptable only because the captured channel metadata proves guild; a present mismatching guild fails the entire batch.

```text
#[async_trait] trait DiscordTransport: Send + Sync {
  get_me(&DiscordBotToken) -> Result<DiscordBotIdentity, DiscordTransportError>
  get_channel(&DiscordBotToken, channel_id:&str) -> Result<DiscordChannelMetadata, ...>
  latest_cursor(&DiscordBotToken, channel_id:&str) -> Result<String, ...>
  receive(&DiscordBotToken, channel_id:&str, cursor:&str) -> Result<DiscordMessageBatch, ...>
  send_chunk(&DiscordBotToken, channel_id:&str, text:&str, attempt_id:&str)
      -> Result<DiscordSendReceipt, ...>
}
DiscordAdapter<'a> privately borrows transport/token/selected channel.
impl ConnectorTransport for DiscordAdapter<'_>:
  Identity=DiscordBotIdentity; InboundBatch=DiscordMessageBatch;
  SendReceipt=DiscordSendReceipt; Error=DiscordTransportError.

#[async_trait] trait DiscordCredentialStore: Send + Sync {
  load(connector_id:&str, generation:u64, bot_id:&str)
      -> Result<Option<DiscordBotToken>, DiscordCredentialError>
  put(connector_id:&str, generation:u64, bot_id:&str, token:DiscordBotToken)
      -> Result<(), DiscordCredentialError>
  delete(connector_id:&str, generation:u64) -> Result<(), DiscordCredentialError>
}
DiscordCredentialError: InvalidToken|InvalidBinding|BackendUnavailable|
  CredentialStateUncertain|InvalidPayload|UnsupportedPayloadVersion|OperationCancelled;
  Display/Debug are static/redacted.
```

`receive` validates the portable request and native snowflake. `send` validates portable bounds then requires destination equals the captured selected channel and text at most 2000 scalars (a single durable chunk). attempt_id is host correlation/stable nonce input, not authority or an exactly-once promise. Each dispatch uses the precise captured token and credential generation; never reload vault inside the adapter. Keep `ConnectorManager::new(...Arc<dyn TelegramTransport>)` compatibility. Host registry adds a constructor for both transports or `with_discord(...)`, retains its original Telegram constructor/fakes, and exposes credential-borrowing `discord(...)`; descriptors remain stable protocol-ID order.

```text
DiscordConnectorRecord {
  id, agent_id, room_id="discord:<id>", bot, channel, approved_user_ids: Vec<String>,
  credential_generation:u64, cursor:String, activated_at_ms:Option<u64>,
  enabled:bool, deleted_at_ms:Option<u64>,
  created_at_ms, updated_at_ms
}
DiscordCredentialIntent {
  connector_id, generation:u64, operation:Create|Replace|Delete, created_at_ms
}
DiscordInboundRecord {
  connector_id, message_id:String, agent_id, room_id, sender, channel_id, text,
  received_at_ms, processing_state:Received|Processing|Processed|Rejected|Stopped,
  run_idempotency_key="discord:<connector>:message:<id>", run_id:Option<String>
}
DiscordOutboundRecord {
  id, connector_id, agent_id, room_id, assistant_message_id, text,
  created_at_ms, delivery_state:Pending|Sending|Delivered|Failed|Unknown|Suppressed,
  chunks:Vec<DiscordOutboundChunk>, message_pruned:bool
}
DiscordOutboundChunk {
  index:u8, text, state:Pending|Sending|Delivered|Failed|Unknown|Suppressed,
  attempt_id:String, provider_message_id:Option<String>, attempts:u32,
  retry_at_ms:Option<u64>
}
```

Frozen selection/channel/allowlist are immutable after create. Delete/recreate changes selection. One non-deleted Discord connector per agent. Agent may have both Telegram and Discord. Record IDs unique across protocols. State holds native `discord_connectors`, `discord_credential_intents`, `discord_inbound` keyed by (connector,String snowflake), `discord_outbound`; snapshot vectors default empty. Discord vault payload is version1 `{version,generation,botId,token}`, binds connector generation and bot identity, and rejects zero/mismatching generation. Generation-specific keyring accounts `connector:<id>:generation:<generation>` under `animaos.connector.discord` prevent stale put/delete overwriting newer credentials; cleanup targets an exact generation. Room/source/session kind is `discord`, never `telegram`. Tokens never derive Debug/Serialize; clone zeroizes; leave Telegram service/payload untouched.

### Authority and lifecycle

Create is owner-only `{botToken, guildId, channelId, approvedUserIds, privateChannelConfirmed:true}`. IDs canonical nonzero u64 strings, allowlist 1–16 unique IDs; only guild text channel type 0. Owner declares channel private after checking Discord permissions; API must not claim verified privacy from channel metadata. Client refuses empty/invalid token (ASCII header-safe, 1–256 bytes, alphanumeric/`._-`; no prefix heuristic). REST get_me must report a bot. get_channel must match selected IDs/type. Test read proves history permission; no silent account/guild substitution.

Create stores selection and vault intent, owns vault mutation to completion, verifies outcome, then persists disabled connector/session. No worker or pairing candidate is created from unsolicited channel messages. Status is `disabled` until explicit activation.

Activate revalidates bot/channel/history with captured token, captures latest cursor, and persists enabled+cursor+activated_at_ms before starting worker. Initial activation consumes **new messages from that point**, not historical chat. Disabled reactivation resumes existing durable cursor; never resets it to latest. `activated_at_ms=None` means never activated; Some is immutable initial activation time, allowing empty-channel sentinel `cursor="0"` without mistaking later reactivation for initial setup. Channel changes need delete/recreate.

Token replacement verifies the same bot, advances generation, revokes old authority and owns vault mutation/persistence to completion. Preserve cursor/selection/journals. Invalid/different bot fails without replacement. Unknown vault outcome is Reconciling with durable intent and no worker/effect; never fall back to the old token. Delete/disable revoke generation before drain; no later admission/delivery settlements under old authority. Delete tombstones before vault cleanup, retains historical session; orphan cleanup intent stays durable on vault failure. Enabled restored connectors start automatically once credentials/generation validate; missing credential is CredentialRequired, never panic.

Native Discord authority owns generation+captured-credential matching, one active claim per connector, RAII release and idle/drain notification, adapting the equivalent vetted Telegram rules. Owner B owns this logic; Telegram claim implementation stays untouched. Shared abstractions are the core transport contract, registry and run coordinator commit/rollback boundary. A generic claim extraction is deferred because it is unnecessary for this usable milestone; never rewrite Telegram lifecycle/runner logic into a generic orchestrator.

`DiscordManager::new(state, runs, credentials, registry)` mirrors existing ownership boundaries and exposes create, activate, replace_token, set_enabled, delete, restart, start_restored, request_shutdown, shutdown, accept_batch, process_one and deliver_one. It uses run coordinator's shared control-plane transaction for every persist/rollback operation; rollback is scoped compare-and-restore, never a whole stale state replacement. Worker waits/networking occur outside state locks. Public owner mutations detach owned work from HTTP cancellation, just as Telegram does. Controlled injectable transports/vaults must permit deterministic startup and mutation failures.

### Intake invariants and bounded scan

Constants: REST_PAGE_LIMIT=100; MAX_SCAN_MESSAGES=1000; MAX_LIVE_INBOUND=100; MAX_TERMINAL_INBOUND=1000; POLL_INTERVAL=5s; transient receive backoff 1s up to30s. These are application bounds, not claimed Discord quotas.

Algorithm independent of undocumented after-window selection:

1. GET selected `/channels/<id>/messages?limit=1`, establishing latest high-water anchor H.
2. If H<=cursor or empty, return empty proven batch; validate all IDs numerically.
3. Collect H, then GET `before=<smallest_seen>&limit=100` repeatedly until reaching/exceeding lower durable cursor boundary or an empty page proves channel start. Always verify strict descending progress; responses may arrive shuffled, so sort/deduplicate valid native IDs. Reject conflicting duplicate contents/channel/author.
4. Stop after 1000 scanned messages. If lower boundary unproven, return BacklogLimit; manager sets Degraded/visible owner alert, persists no new cursor/receipts. Do not leap to H. Do not hide cap as successful empty poll. Channel is documented for low-volume owner use; later bounded prefix-drain may solve larger backlogs.
5. Once proof complete, sort numerically ascending and return every message cursor<id<=H. New messages newer than H wait for next scan. History deletions do not imply an identity conflict; boundary proof can use numeric <=cursor even if exact old message is deleted.
6. Host filters exact channel, matching guild, approved user IDs, nonbot/nonself/nonwebhook, type0/19 and nonempty readable text. Attachments-only/system/empty messages are unsupported and not dispatched; the proven cursor may cross them. Expose a content-unavailable/unsupported warning for empty allowed-user messages; do not assert intent is enabled merely because HTTP returned200.
7. Transaction atomically persists all permitted receipts plus cursor H. Duplicate equivalent receipts do not consume capacity or create runs. Capacity includes Received/Processing; if projected total exceeds100, admit nothing and move no cursor. Persistence failure restores only this batch's changes; no model call before successful save.

No durable receipt depends on terminal retention forever: monotonically persisted cursor prevents old IDs being readmitted after terminal pruning. Identical ID with changed text observed within a batch or pending receipt conflicts; edits of already-terminal IDs do not rerun the agent. Cursor never moves backward or past an unproven window.

### Runs, outbox, uncertainty

Use `RunSource::Discord`, `SessionKind::Discord`, `SessionOrigin::Discord`; native room `discord:<connector_id>` via `RunRoom::Stable`. Session capabilities match Telegram: owner send, stop/rename/archive/compact/export; no steer/deletion while attached. Update classification/room helpers, session groups/transcript source labels and allowed room guards. Helpers/peer delivery cannot acquire connector-room authority merely by passing that ID. Refuse skills/attachments for M13 Discord session owner turns with clear message; no attachment ingestion scope. Metadata stores native source, message/channel/sender IDs; no token or numeric/f64 snowflake conversions. Untrusted message text is input, never instruction/config authority.

Journal Processing admission and durable run ID using existing coordinator accepted-run/idempotency primitives. A persisted Received record can resume once; Processing with a surviving run follows its terminal result; restart-interrupted runs are marked terminal/reviewable and never execute tools again automatically. Do not turn interrupted execution into a new run with a new key. Model final commit hook atomically stores inbound terminal state, native outbound and session/transcript; save rollback removes only inserted rows and restores only matching row state. Reply identity comes from `RunOutcome.reply_message_id`, never transcript scanning.

Owner session turns go through existing session `/runs` admission hook, use `send_from_owner_accepted`, and produce native outbound in the same final commit. Repeated owner keys replay committed run/history without model/send duplication. Stop maps pending reply to Suppressed and intake to Stopped through `state/run_stop.rs`, including undo/rollback; already-started send cannot be unsent, settlement remains fenced and acknowledged as potentially external. Initial owner stop is through web session controls; no Discord `/stop` command is claimed. Poll cancellation/revocation must not orphan child model runs: pass existing cancellation control and await/drain admitted work through the coordinator shutdown policy.

Agent deletion and session history pruning handle native journals and cleanup intents consistently: busy-agent deletion is refused, eventual deletion revokes/drains native claims/workers and queues independent Discord vault cleanup, native outbox never survives with live orphan authority, archived history cannot keep a deleted agent alive, mirrored/pruned messages retain native outbound correlation without re-execution or accidental cross-protocol cleanup.

Outbox chunks are prepared durably before any POST. Adapt upstream eight-chunk cap: first seven 2000-scalar chunks plus a bounded plain Anima notice when text exceeds16000 scalars; full output remains in session history. Document truncation; do not mark delivery failure merely due to intentional cap. Empty output creates no outbound.

For each pending chunk, acquire current generation+credential+selected-channel claim, persist Sending **before** POST, then recheck authority immediately before dispatch. No persistence success => no effect. POST success stores provider message ID and Delivered; response channel mismatch/malformed success is Unknown. Crash with Sending or response/timeout/connection uncertainty becomes Unknown on restore, never Pending. Persist failure after a successful POST leaves Sending/Unknown evidence, never retries automatically. Delivered chunks never resend. Partial output retains delivered prefix plus unknown chunk, stops later chunks and marks outbound Unknown.

429 is a confirmed rejection: save Failed/absolute retry_at_ms deadline and honor returned delay across restart before retry; header/body delays must be finite positive and bounded in representation (large values retain paused state; never clamp downward and send early). 400/401/403/404 are terminal safe failures with static category, stop repeated invalid requests; 401 requires credentials, 403 requires permission review. 5xx, timeout, cancelled POST or malformed success are uncertain. Transport GET failures are retryable; transport POST failures default Unknown unless provably no request was sent. No blanket reqwest retry policy.

Per-bot shared rate limiter in native client coordinates poll+send/connectors with bucket/global returned deadlines (token-derived in-memory identity, never printed). Request pacing is cancellable and outside state locks. Stable nonce <=25 chars can be derived from sha256(outbound id, chunk index) with enforce_nonce=true; recent dedup improves duplicate resistance but does not authorize Unknown automatic resend. Full native send attempts are owned to completion; shutdown/revoke stop new dispatches and drain in-flight claims with existing bounded shutdown budget.

Owner sees Unknown plus delivered provider IDs and instruction to inspect Discord before deciding. Unknown blocks subsequent FIFO output until owner review. Owner explicit `/retry` rejects Unknown with409; only safely failed/pending chunks can retry. Owner `/outbound/suppress` with literal `acknowledgeUncertainty:true` suppresses the currently blocked Unknown output and unsent tail without sending anything, preserves all delivered receipts/Unknown audit/full history, and clears the FIFO block. In-flight Sending cannot be suppressed; await settlement/drain or return409. No automatic retry, resend-all or reset cursor action for Unknown. A later explicit resend/reconciliation API can be separately scoped.

## Exact owner API/SDK/web contract

New namespace preserves existing Telegram list/routes verbatim:

```text
GET    /api/agents/{agent_id}/connectors/discord
  200 {connectors: DiscordConnectorResponse[]}
POST   /api/agents/{agent_id}/connectors/discord
  body {botToken,guildId,channelId,approvedUserIds,privateChannelConfirmed:true}
  201 {connector: DiscordConnectorResponse} (disabled, selection reviewed)
POST   /api/agents/{agent_id}/connectors/discord/{connector_id}/activate
  200 {connector}; initialize boundary only on first activation
PUT    /api/agents/{agent_id}/connectors/discord/{connector_id}/credential
  body {botToken}; 200 {connector}
PATCH  /api/agents/{agent_id}/connectors/discord/{connector_id}
  body {enabled:boolean}; 200 {connector}; no selection edits
POST   /api/agents/{agent_id}/connectors/discord/{connector_id}/restart
  200 {connector}; preserve cursor, claims, delivered/unknown state
POST   /api/agents/{agent_id}/connectors/discord/{connector_id}/retry
  200 {connector}; only safe Failed/Pending, Unknown =>409
POST   /api/agents/{agent_id}/connectors/discord/{connector_id}/outbound/suppress
  body {acknowledgeUncertainty:true}; 200 {connector}
  suppress current Unknown output/unsent tail, retain delivered receipts, no sends
DELETE /api/agents/{agent_id}/connectors/discord/{connector_id}
  200 {deleted:true}; tombstone persisted, cleanup failures explicitly503/reconciling
```

Generic errors use existing `{code,error}` shape. Invalid IDs/unknown fields/missing private confirmation400; owner403; wrong agent/connector404; duplicate/different-bot/unknown-retry409; provider token rejection422; vault/store/transport503. Body8KiB maximum. Request token struct no derived Debug. Errors must never include request/upstream body.

```text
DiscordConnectorResponse {
  id,agentId,type:"discord",roomId,
  bot:{id,username,displayName}, channel:{id,guildId,name,kind:"guildText"},
  approvedUserIds:string[], enabled:boolean,
  status:"disabled"|"ready"|"credentialRequired"|"error"|"degraded"|"reconciling",
  createdAtMs,updatedAtMs,
  pendingInbound:number,pendingOutbound:number,unknownOutbound:number,
  deliveredMessageIds:string[], warning:string|null
}
```

Delivered IDs/warning are bounded safe summaries (last20 IDs, static category strings). No token, token fingerprint or raw native errors in responses. Existing sessions messages/history/SSE supply conversation; avoid a duplicate Discord messages endpoint.

SDK adds exported DTOs and `ConnectorsClient` methods `discordConnectors`, `createDiscordConnector`, `activateDiscordConnector`, `replaceDiscordCredential`, `setDiscordEnabled`, `restartDiscordConnector`, `retryDiscordDelivery`, `suppressDiscordUnknownDelivery` (body literal acknowledgment), `deleteDiscordConnector`, encoding all path segments. Add `discord` to SDK run/session unions. New web `DiscordSettings` card uses those methods in existing ConnectorsView; create form token/password, guild/channel/owner user IDs and private-channel confirmation. Explain Discord Developer Portal bot installation, View Channel/Read Message History/Send Messages permissions, Message Content intent (and verified-app approval where applicable), and initial new-message baseline. No asking for Administrator permission.

UI distinguishes saved disabled vs listening vs missing credentials/permissions/content vs backlog/unknown. Explicit Activate/Disable/Restart/Replace/Delete; safe Retry unavailable for Unknown with review instruction and an explicit confirmation action to acknowledge uncertainty and suppress unsent remainder after checking Discord. Clear token field immediately after any submit and on agent change; never browser persistence or query-string token. Stale load/save results cannot mutate another agent's card. Ready means transport/lifecycle is listening, not verified intent or private visibility. Owner can open Discord session and send via ordinary SessionView, see native source labels and stop. Update all exhaustive session-kind/source maps required by typecheck; reuse existing run/session components.

## Tasks and worker boundaries

### Task 1 — Native transport, adapter, credentials and provenance

**Owner A (independent):** new `connectors/discord/{client,text,credentials,model}.rs`, `adapters/discord.rs`; `registry.rs`, `adapters/mod.rs`, module wiring. Avoid native manager/authority, Telegram claim, host routes/state/UI files owned by B/C. Freeze interfaces above before B integrates.

- [ ] Write focused failing fake HTTP/native tests: header authentication with no secret in URLs/errors, fixed API v10 origin, redirects disabled, bounded streamed response (1MiB), malformed IDs/JSON/mismatched channel/bot rejection; identify/receive/send associated types.
- [ ] Test numeric snowflake ordering, 0 internal cursor only, shuffled/duplicate pages, 101+ message backlog, boundary deletion, cap/no-progress failure; no after-window assumption.
- [ ] Test Unicode splitting/cap/full history retained; exact allowed_mentions on every chunk, stable nonce/enforce_nonce; success receipt; 429 decimal/body/header delay/global bucket; malformed success/POST timeout=>Unknown; GET errors retryable;401/403 halted.
- [ ] Adapt small upstream algorithms into Rust, add full provenance notice, implement fixed-origin REST client using existing reqwest only. Test-only loopback constructor is private/cfg(test), never owner-controlled base URL.
- [ ] Add native zeroizing credential and store using existing backend conventions and independent service; preserve Telegram keyring format. Shared backend code can move only if behavior/visibility remains equal. Vault fakes, not actual vault.
- [ ] Keep Telegram claim source untouched; passive registry registration tests prove no network calls and sorted telegram/discord descriptors. Credential tests prove old generation write/delete cannot alter newer generation account.
- [ ] Report changed paths, red/green evidence and interface freeze to controller; controller compiles with B when module wiring is ready.

Commit suggestion: `feat(connectors): add native Discord REST transport and typed registry adapter`.

### Task 2 — Durable native lifecycle, intake, run/session integration, owner routes

**Owner B:** `discord/{mod,lifecycle,inbound,processing,delivery,tests}.rs`, `state.rs`, `state/{run_stop,session_state}.rs`, `control_plane_store.rs`, `sessions/{mod,migration,views,pruning}.rs` as necessary, `runs/ledger.rs`, `routes/{discord,mod,runs,agents}.rs`, `routes/contracts/{discord,mod}.rs`, `agent_runs/{queue,attachments}.rs` exhaustive source/room branches, `app.rs` bootstrap/start/shutdown; host tests. Coordinate any shared module export with A. Preserve schedules' Telegram destination behavior; Discord schedule delivery is later scope.

- [ ] Write failing v11->v12 upgrade/reload/native validation tests first. Bump snapshot to12; add `ATTACHMENTS_STORE_VERSION=11`, `PRE_DISCORD_BACKUP_SUFFIX=".pre-discord.bak"`, `pre_discord_backup_path`, and branch loaded11 to this suffix while preserving older suffix decisions. Postgres backup key stays `control_plane.backup.<version>`; test fixture only, no database. Reject future13; backup must precede save. Record downgrade requires old backup.
- [ ] Add default-empty native fields, initialization/restoration/snapshot serialization and validation: duplicate/malformed IDs, generation0, unknown agent or binding mismatch, duplicate allowlist, unsupported channel, cursor/journal inconsistencies fail closed. Agent deletion/busy gates include Discord and credential cleanup; old Telegram fixtures reload unchanged.
- [ ] Implement detached create/activate/replace/disable/delete transactions and durable intents. Tests cover caller cancellation, vault put/delete unknown, persistence rollback, same-bot verification, generation races, missing token restore, no worker before activate, preserved cursor on reactivation, both protocol connectors coexist.
- [ ] Add bounded native scan admission with atomic cursor+receipts. Tests use complete/bad/backpressure batches, duplicate replay, save failure, unauthorized/bot/self/webhook/channel mismatch, snapshot restore. No model called before persisted receipt.
- [ ] Integrate coordinator accepted-run and commit/rollback hook: intake dedup and run linkage, owner session idempotent replay/history, interrupted restart no tools replay, final reply native outbox, stop suppression/undo, deleted/replaced connector fencing, busy-agent delete gate and native journal/vault cleanup, mirrored-history pruning. Reserve `discord:` in legacy run room validation; prevent helper/peer bypass and orphan model children on poll cancellation.
- [ ] Implement native credential/generation RAII authority, per-chunk Sending-before-POST, receipt settlement and Unknown restore. Tests: success prefix then uncertain failure; POST success then save failure; restart during Sending; safely rejected429 retry timing across restart; owner retry rejects Unknown; explicit uncertainty acknowledgment/suppression clears block without resending/preserves receipts; sending suppression rejected; token rotation/delete/shutdown while claim in flight; no stale settlement or subsequent send.
- [ ] Wire app start_restored/request_shutdown/shutdown in producer-first order, same shared run coordinator. Deterministic test-mode factory injects fake Discord boundary; no default live Discord call for unrelated tests.
- [ ] Implement exact routes/OpenAPI and owner authorization tests before status/network reads;401/403 vs404 scope, bounded invalid requests, every no-store branch, safe statuses and warnings; existing Telegram API unchanged.
- [ ] Freeze host contracts to C; request one daemon batch review after native transport + host sources ready. Root owns final rulings/fix wave.

Commit suggestion: `feat(daemon): add durable owner-scoped Discord channel assistant`.

### Task 3 — SDK and usable web setup/session experience

**Owner C:** `packages/sdk/src/{connectors,connectors.spec,sessions,runs,index}.ts`; `apps/web/src/{components/DiscordSettings.tsx,components/DiscordSettings.test.tsx,components/ConnectorsView.tsx,components/ConnectorsView.test.tsx,lib/daemon-api.ts}`; necessary exhaustive session/source labels in `apps/web/src/components/sessions/`, `hooks/useCompanionSessions.ts`, `lib/{session-events,session-groups,transcript}.ts`; fake API fixtures. No Rust/state files.

- [ ] Add SDK contract tests for every exact route/body/encoded ID; export DTOs; no token in read summaries; add native Discord unions and request typings.
- [ ] Add settings tests for setup->disabled->activate->ready, Message Content/private channel instructions, empty/invalid allowed users, stale agent switch, token clearing, replacement/delete errors, unknown delivery blocks Retry and review confirmation calls safe suppression, backlog warning; preserving Telegram/Gmail/Calendar cards.
- [ ] Build Discord card and native session source labeling; owner session send uses existing run path and session ID, no new chat API/implicit arbitrary outbound destination. No fake install/community store controls.
- [ ] Existing ConnectorsView may retain Outlook card to preserve behavior; selected roadmap expansion is limited to Discord/Telegram/Gmail/Calendar/Drive and does not broaden Outlook support.
- [ ] Run relevant SDK/web test/typecheck/build after host DTO freeze; final SDK/web review verifies setup reaches actually registered native transport and statuses accurately reflect server behavior.

Commit suggestion: `feat(web): add Anima Discord connector setup and native session UX`.

### Task 4 — Contribution/docs/validation/publication (controller)

**Root:** `hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md`, `README.md`/host setup docs only where required, master plan/handover/current M13 tracked plan, rules/progress/gate evidence. If Astro content changes, run its relevant build. No broad documentation rewrite.

- [ ] Document trusted compile-in community contribution with native types, immutable metadata registration, owner binding, credential generation, receipt/cursor admission, per-chunk uncertainty, fake tests and activation UI requirements. Installation remains unavailable.
- [ ] Record adapted/copy vs behavior source mapping and full MIT notice; set runtime/API/UI product naming Anima/AnimaOS.
- [ ] Batch review native/daemon source then final SDK/web review, one controller-approved fix wave. Verify all code actually traverses registered trait and durable path; no hidden direct Discord sends.
- [ ] Run bounded targeted tests while implementing; after final fix wave run required fresh full gates below once. Repeat only for new failures/source changes.
- [ ] Update master/handover with exact branch/head/PR/gate evidence, standing continuation authorization, live acceptance pending and next selected milestones. Stage explicit owned paths; commit/push/create/attach PR; merge under existing standing owner authorization once gates/reviews pass. Next branch only from updated merged main.

## Verification commands

Planning inspected sources only; no tests were run here. Controller discovers resolved target names with `bun x nx show projects --json` and `bun x nx show project <name> --json` before executing unfamiliar target/flag. Existing confirmed project names: `core-rust`, `rust-daemon`, `@animaos/sdk`, `@animaos/web`, `workspace-dev`.

PowerShell, one cargo process at a time:

```powershell
$env:CARGO_INCREMENTAL = '0'
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run-many -t test,typecheck,build -p @animaos/sdk @animaos/web --skipNxCache
```

Host test target already includes core dependency; inspect both successful outputs rather than repeat full core tests. Expected: all required current-tree tests/checks pass; ignored PostgreSQL/live-provider/download cases remain explicitly unverified. Prefix isolated focused Cargo filters through existing Nx target's resolved forwarding syntax only after `--help` confirmation. No guessed flags, no new targets/deps.

If existing daemon locks executable, AGENTS permits already-established `CI=1` and `CARGO_TARGET_DIR=target/validation-rust-daemon`; controller verifies this existing directory before use and records fallback. Do not start/kill a daemon during planning. Web browser tests require approved deterministic test server only; if no service execution authorization, component/SDK fakes suffice for implementation and browser/live evidence stays pending.

Conditional docs: `bun x nx run @animaos/docs:build --skipNxCache` or contributor-docs target only for changed Astro sources. Launcher tests only if launcher changed (not expected). Required formatting checks use existing configured tools on explicit changed files. Do not claim live delivery or operational daily use from fake tests.

## M13 acceptance

- [ ] Owner can save bot + selected guild text channel + allowed users, see disabled state, activate, disable/re-enable, replace same-bot token and delete through working SDK/web.
- [ ] Authorized new channel text is admitted once, calls Anima's existing coordinator, appears in a Discord session and generates durable native outbox; unauthorized/bot/self/webhook messages do not call model.
- [ ] Pagination/cursor save is proved before advance; <=1000 backlog works; above bound displays Degraded without skipping; capacity/save failures consume no input.
- [ ] Discord reply uses captured credential + selected channel, explicit no-ping allowed_mentions, bounded chunks and durable receipts. Partial/uncertain send never auto-retries; owner sees review status.
- [ ] Restart resumes enabled connectors and pending safe work; never replays interrupted tool effects or Sending/Unknown delivery. Owner stop/revoke/shutdown fences and scoped rollback are covered.
- [ ] Telegram regressions and owner HTTP/wire behavior stay green; old snapshots backed up/upgraded to12; future snapshots rejected; unrelated edits preserved.
- [ ] Native source/run/session label is Discord; Anima naming, MIT attribution, community compile-in guide and REST/content/privacy limitations match implementation.
- [ ] Current-tree host/core + SDK/web gates pass, bounded reviews/fix wave recorded; PR merged under standing authorization before next milestone branch.

## Usable daily Anima assistant: remaining acceptance beyond M13

These are concrete readiness checks, not promises that M13 implements all of them:

| Surface          | Existing baseline                                                                                                              | Remaining real acceptance/gap                                                                                                                                                                                                                           |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Web companion    | Durable runs/sessions, streaming, stop, approvals, history, Markdown identity, memory, schedules, usage/logs/health are merged | Owner creates/opens companion with selected real model; chat/save/restart/approval/stop/identity continuity exercise; setup errors actionable. Local fakes do not verify provider/subscription execution                                                |
| Telegram         | Native vault/pairing/poll/run/outbox/restart path through M12 contract                                                         | Owner-selected test bot/chat send/receive + stop + restart delivery once; no actual token/provider calls during code gates                                                                                                                              |
| Discord          | M13 selected-channel REST implementation                                                                                       | Portal bot install/permissions/Message Content setup, selected private low-volume channel + actual owner ID, receive and native delivery, restart/revoke; inspect unknown-send UX without duplicate resend; latency/unsupported edits/deletes disclosed |
| Gmail            | Existing OAuth app config/connect, readonly list, immutable local draft, owner approve/reject/send with uncertainty            | Real Google app/account consent + refresh/revoke; reading from Anima and draft preview; owner approval sends exactly intended immutable draft. Fix only demonstrated functional gaps; no registry-parity migration                                      |
| Calendar         | Existing Google app/connector read, reviewed create/update/delete; native on-demand tools                                      | Real consent/read/refresh/revoke; Anima prepares proposed event, owner reviews exact write, applied result appears; verify timezone and unknown-write behavior. Preserve native confirmation; no migration just for registry parity                     |
| Drive            | No implemented consent/scopes/native tools at inspected baseline                                                               | Next bounded selected Drive milestone (M14) implements selected read/search, least required consent scope, explicit file provenance and bounded content; no generic write/delete/share, sync-everything or fake installed module                        |
| Always available | VPS deployment/backup/recovery package exists from M10                                                                         | Actual chosen host setup, persistent volumes/keyring/model auth, HTTPS/owner access, reboot/reconnect, backup+restore drill and monitoring; manual deployment acceptance remains unperformed                                                            |

Sequence after gated M13: selected Drive read/search (M14) -> existing Gmail/Calendar demonstrated acceptance gaps only -> deterministic cross-surface assistant acceptance -> authorized actual-account/provider/hosting smoke tests. Progress autonomously within standing scope, distinguish shipped code from real-provider/live readiness, and ask only for genuinely missing external setup/credentials/host choice when needed. Never read Hermes live profiles/secrets to fill those gaps.

## Top five risks for controller binding rulings

1. **No-skip intake:** latest-anchor backward scan, numeric IDs and complete lower-bound proof; >1000 backlog Degraded/no advance. Cost: very busy channel pauses rather than silently losing messages.
2. **Selected authority:** exact guild text channel + nonempty approved-user list, nonbot/self/webhook rejection; privacy is owner-confirmed, not inferred. Captured credential/generation and host session binding constrain every effect. Cost: selection changes require recreate.
3. **Unknown delivery:** native per-chunk Sending persists before POST; Unknown/partial states never auto-retry, nonce only recent dedup. Owner inspects provider; M13 retry refuses Unknown. Cost: some recoverable network failures need manual reconciliation later.
4. **Durability without rewrite:** dedicated native journals/authority + existing shared transport and coordinator hooks; snapshot12/backups/future-version rejection, exact Telegram wire/claims preservation. Cost: native manager has explicit protocol integration; generic claim extraction and broad orchestration deferred.
5. **Real REST scope:** content restrictions, permissions, five-second poll, low-volume bound and unsupported Gateway events are explicit setup/status/docs limits. Community source is reviewed compile-in, not installed. Cost: full Gateway needs separately authorized dependencies and scope.

Controller appends accepted binding section here; no additional audit/confirmation gate is introduced by this draft.

## Controller rulings on the risks (binding)

1. Ruling: Prove catch-up by bounded backward paging from a captured latest anchor to the durable cursor, then numeric snowflake ordering — official REST docs do not establish the nearest-after selection assumption needed for burst safety — cost if wrong: more than 1000 messages or an unprovable boundary pauses visibly without advancing; no silently skipped messages.
2. Ruling: Connect only an owner-selected guild text channel with a nonempty exact sender-ID allowlist, and reject self/bot/webhook messages — a channel ID alone does not establish private access or agent authority — cost if wrong: broader group/DM/thread support needs later design; no arbitrary outbound destination. Privacy is owner confirmation, not a platform-verified guarantee.
3. Ruling: Persist each chunk Sending before effects and settle ambiguous or partial sends Unknown without automatic replay — remote sends cannot be rolled back and recent nonce dedup is not exactly-once — cost if wrong: owner must inspect and explicitly suppress uncertain output before continuing; delivered receipts and full history remain. Unknown resend stays forbidden.
4. Ruling: Use native Discord journals and snapshot12 with version11 pre-Discord backup and future-version rejection; defer Telegram shared-claim extraction — existing Telegram-specific types cannot disguise another provider, and working Telegram claims need no refactor to ship Discord — cost if wrong: some native claim bookkeeping is adapted/duplicated until a concrete reusable seam is justified; old binaries require their backup to roll back. Owner B owns native claims; A must not touch Telegram runtime/delivery or add speculative authority.rs.
5. Ruling: REST scope exposes Message Content intent, permission, polling and unavailable Gateway features in setup/status — current dependency rules exclude genuine Gateway/voice coverage — cost if wrong: owner configures bot intent/channel access for text and uses web Stop initially; no full upstream parity claim.

### Additional interface freeze (supersedes draft ambiguity)

- Credential store: new `DiscordCredentialStore:Send+Sync` uses async_trait methods `load(&self, connector_id:&str, generation:u64, bot_id:&str) -> Result<Option<DiscordBotToken>, DiscordCredentialError>`, `put(&self, connector_id:&str, generation:u64, bot_id:&str, token:&DiscordBotToken) -> Result<(), DiscordCredentialError>`, `delete(&self, connector_id:&str, generation:u64) -> Result<(), DiscordCredentialError>`. Static native error variants `InvalidInput`, `Unavailable`, `Uncertain`, `InvalidPayload`. Generation-specific accounts under independent Discord service validate version/connector/generation/bot payload identity. No token Debug/Display leaks, no token in API/state. A owns types/store and shares exact interface with B. No changes to Telegram store/claim shapes.
- Owner B owns `discord/mod.rs` and connectors/mod.rs declaration, native model state integration/routes/manager. A owns `discord/{model,client,text,credentials}.rs`, adapters/discord.rs + adapters/mod.rs, registry.rs, its own new transport/text tests and `discord/HERMES-NOTICE`; B owns module test wiring and uses explicit separate test filenames for A's fixtures. C owns SDK/web only. Root owns contribution docs/README/master/handover/plan. No workers run Cargo or Git; controller serializes gates and formats.
- Model fields additionally include `activated_at_ms:Option<u64>` on connector (distinguishes an empty-channel activation from never activated) and `retry_at_ms:Option<u64>` on chunks (absolute persisted provider delay; not reset on restart). All snowflakes remain decimal strings in APIs, journals and model metadata.
- Add owner-only `POST /api/agents/{agent_id}/connectors/discord/{connector_id}/outbound/suppress`, body `{acknowledgeUncertainty:true}`, response `{connector}`. Suppress only currently Unknown output and its unsent tail, retaining delivered provider IDs/full history. Never resend; reject missing acknowledgement or unresolved active Sending. SDK `suppressDiscordUnknownDelivery(agentId,connectorId)` sends explicit acknowledgement, UI requires a review confirmation and describes inspection of Discord. Tests prove no native send and subsequent new work can continue. This overrides the draft's deferred reconciliation statement and prevents a permanently stuck daily conversation.
- Retry accepts safely Failed/Pending only; Unknown remains409. A failed chunk has a static error category and durable deadline; runtime response reports actionable warning. Conservatively split provider text at at most2000 UTF-16 units as well as2000 Unicode scalars (never bytes), cap8 with plain truncation notice; full history remains. Cost: emoji-heavy replies use more chunks; valid scalar boundaries preserved.
- Do not migrate functional Gmail/Calendar merely for registry parity. After M13 merge, selected Drive read/search is M14 unless inspection identifies a real blocking daily-use defect; final cross-surface deterministic acceptance follows. Actual account consent/live delivery/hosting remain outside the standing no-services constraint.
- Planner draft stays concise rather than expanding into code-heavy filler. Its source-grounded interfaces/rules/commands and test lists are executable; retain the user-requested cheap batch review/fix protocol.

## Execution outcome (2026-10-12)

Selected Discord REST setup and native durable conversation path are implemented.
The exact selected channel/user authority, private generation-specific credentials,
first-activation baseline, anchored catch-up, coordinator/session source, owner
Stop and per-chunk receipts/uncertainty are wired through registered transport.
Source adaptation scope and full MIT notice are in discord/HERMES-NOTICE; the
credited files are unchanged from the verified source revision.

One final SDK/web review and one daemon batch review are complete. Combined fixes
cover status reconciliation and Stop availability; tombstone+vault intent atomicity,
activation cursor checks, independent long-input bounds, prompt rate deferral,
Stop during Sending, terminal poll pause, whole-batch identity rejection and
snapshot delivery consistency. Thirty-six focused native/registry/route cases
pass, supplemented by startup migration/backup and existing host regressions.

Fresh current-tree gates: Rust **2,268 passed, 8 ignored** (core dependency630,
host1,638); SDK **125 passed, 3 service-skipped**; web **1,158 passed /107files**.
SDK/web test/typecheck/build, Rust lint and explicit owned Markdown/TypeScript
formatting pass. Eight Rust opt-in cases are five Postgres, one live subscription,
one model download and one inventory printer. Initial Rust gate failures were
15 obsolete current-version fixture expectations; current assertions now use the
store constant while legacy inputs/backup cases are preserved. Full rerun passes.
Web warning origins match the existing baseline; no new focused act warnings.
Rust retains existing unused-code warnings plus the three fixture entry wrappers.

Snapshot12 backs up version11 as .pre-discord.bak; native fields are default-empty
for earlier formats, future versions refused. README inventory covers154 owner
operations. Manifests/features/lockfiles are unchanged. No real services, accounts,
provider delivery, databases, containers, user profiles or credentials were used.

Ruling: Native user input is bounded separately at65,536 Unicode scalars, with
streamed1MiB limits — a bot's 2000-character reply ceiling must not reject valid
user messages — cost if wrong: oversized unsupported input consumes no cursor.
Ruling: Provider pacing returns RateLimited without dispatch while its deadline
is future — a long rate delay cannot retain a delivery claim indefinitely — cost
if wrong: native host persists remaining delay and resumes only after it expires.
Ruling: Persist stop_requested and suppress unsent chunks while preserving an
in-flight receipt — Stop cannot undo a remote effect but must prevent the tail —
cost if wrong: uncertain in-flight effects remain visible for owner review.

Publish and merge the verified branch under standing owner continuation, then cut
M14 from updated main. The prepared selected Drive plan is ignored until activated.
