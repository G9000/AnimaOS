# Complete Discord channel management implementation plan

> Use bounded implementation agents and independent review, as required by the brainstorming and writing-plans skills. Owner selected complete channel management; implement without an additional approval round for the already authorized scope. Preserve existing work. Apply the owner's existing commit-and-push instruction to this extension after verification; keep the daemon stopped.

**Goal:** Complete native channel administration while keeping every added operation restricted to the verified owner and current guild.

**Architecture:** Extend the existing capability with a bounded advanced-operation dispatcher and explicit REST routes. Keep the existing basic tool contracts compatible. Split transport, payload validation, capability effects and tool schema registration into focused files.

**Stack:** Rust, reqwest, Tokio, existing native tool action receipts and injected Discord/model test fixtures.

- [x] Add failing tool-visibility regression for the complete operation names before implementation. Run it through the managed validation wrapper and confirm the missing-tool failure.
- [x] Add typed internal route/method models in `connectors/discord/channel_admin_model.rs`, bounded REST dispatch in `client_channel_admin.rs`, and native HTTP regressions. Extend `DiscordTransport` with a default-denied advanced method. Keep 204 success and PUT/DELETE uncertainty correct.
- [x] Add `guild_admin.rs` for owner-capability execution and `channel_admin_validation.rs` for bounded schema and same-guild/parent validation. Reuse the existing lifecycle, serialization and uncertain-write lock, and persist deletion metadata changes safely.
- [x] Extend `tools/discord_channels.rs` and new schema helpers to expose bounded inspection/deletion/reordering/permissions/thread/voice/follow operations. Register/filter/catalogue every added tool under the verified guild capability, including saved-descriptor revocation. Keep old basic create/update calls compatible and extend them with a bounded settings object and additional channel kinds.
- [x] Extend runtime fakes and model tests in `channel_admin_tests.rs`: owner success, nonowner/spoof/console denial, cross-server/parent/thread rejection, delete exact-name checks, permission validation, empty acknowledgements, uncertainty fencing and metadata failure. Keep routing allowlists unchanged.
- [x] Review integrated code and scope coverage. Run focused checks, then `bun run rust:test`, `bun run rust:lint` and `git diff --check`. Use managed Cargo cache and protected task temp root if required by existing document/privacy tests. Remove task-owned scratch after recording results.
- [x] Update plan validation evidence and shared M.O.T.H.E.R checkpoint. Keep the server stopped and leave live bot permissions unchanged; report any Discord permission requirements and remaining API limits truthfully.

## Native tool coverage

| Tool | Capability |
| --- | --- |
| `list_discord_channels`, `get_discord_channel` | Guild inventory and current channel/thread details |
| `create_discord_channel`, `update_discord_channel` | Text, announcement, voice, stage, category, forum and media configuration |
| `delete_discord_channel` | One verified channel/thread, with exact current-name precondition |
| `reorder_discord_channels` | Bounded bulk ordering and one category move per request |
| `set_discord_channel_permissions`, `delete_discord_channel_permission` | Explicit role/member permission overwrites |
| `get_discord_channel_permissions` | Computed member or role permissions |
| `list_discord_threads` | Active guild threads and paginated public/private/joined archived threads |
| `create_discord_thread`, `update_discord_thread` | Public/private/message/forum thread creation; rename, archive, lock, slowmode, tags and pinning |
| `manage_discord_thread_members` | List/get/join/leave/add/remove thread membership |
| `set_discord_voice_status` | Set or clear voice status |
| `follow_discord_announcement` | Follow into a text channel verified in the same guild |
| `rename_discord_thread` | Compatible current-thread alias, now requiring both thread and owner-guild capabilities |

New post starters are bounded text with mentions suppressed; attachment upload stays in file-delivery tooling. Server-wide roles/moderation/settings/invites are outside the selected scope. The stopped daemon has not been restarted, and no live roles or channels have been modified by this implementation.

## Validation results, 2026-11-14

- The added missing-tool regression first failed with `missing get_discord_channel`, before implementation.
- Seven native HTTP contract tests passed in the initial focused run and are included in the final full pass.
- Integration tests exposed model integer-to-JSON conversion and fixture assumptions; exact integer normalization and corrected fixtures are covered by the final full pass.
- Independent review found an automatic/manual thread-title race. Thread operations now share the actual-thread title lock, refresh provider metadata after locking, and persist manual-name intent before the network effect. A deterministic lock regression and persisted-intent regression cover the new path.
- `bun run rust:test` passed: reusable core, packaged supervisor, daemon integration/doc suites and 4,059 daemon unit tests (49 intentionally ignored). All 21 new runtime administration regressions passed, including owner/nonowner, console, stale scopes, identity mismatches, metadata failure, uncertainty and shared title locks.
- Final `bun run rust:lint` and `git diff --check` passed. Existing repository warnings remain. No Rust source changed after the successful full test and lint runs.
- Validation used the wrapped absolute `target/cargo/validation` cache, debug information disabled, incremental compilation disabled, and four Rust test threads. Final combined Cargo cache size was 25.0 GiB; the 32 GiB ceiling was never raised.
- Existing document/privacy tests require a private owner-and-SYSTEM-only temp root on this machine. An initial long temp path hit an existing Windows worktree filename limit; the successful full run used the protected short task-owned `C:/act-52d6d354` root. Both private roots were removed afterward, without changing global directory permissions. Task-owned verification logs and script were removed after recording results.
- Spec, plan and independent code reviews found no remaining blocking issues. Complete controls have not been exercised against live Discord, and additional provider permissions may still be needed by the installed bot. No normal daemon restart or live channel/role changes occurred.
