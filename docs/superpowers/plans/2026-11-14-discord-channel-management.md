# Discord channel management implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development for the bounded REST task and inline integration, with independent final review. Preserve unrelated work. Initial authorization covered implementation; after live acceptance, the owner requested server shutdown, commit and push.

**Goal:** Let the verified owner list, create and edit channels through native Discord conversations.

**Architecture:** Add bounded guild-channel REST models separately from conversation routing models. Carry a host-created guild capability alongside current-thread capability through run preparation, catalog discovery and tool execution. Bind it to a verified inbound sender and revalidate ownership before effects.

**Tech stack:** Rust, existing reqwest transport, Tokio, deterministic injected Discord/model fixtures.

## Tasks

- [x] Add native channel models and list/create/edit REST methods in `connectors/discord/channel_model.rs`, `client_channels.rs` and `client.rs`. Add transport tests in `native_tests.rs`; run through managed validation. Extract a single application/team owner into `DiscordBotCheck`. Request Manage Channels in invites.
- [x] Add `guild_tools.rs` capability with receipt/run binding, unique owner check, guild/category validation, cancellation and lifecycle checks, truthful acknowledgements and metadata persistence. Reuse owned effects and tool action journal; never automatically retry writes.
- [x] Wire capability through `RunRoom`, run preparation, `ToolExecutionContext`, tool selection and discovery catalog. Register three narrowly shaped tools in `tools/discord_channels.rs`, classify reads/writes in approval policy and update sender guidance.
- [x] Add failing deterministic integration regressions before implementation, then run them after integration. Cover actual model use, account restrictions, metadata spoofing, guild/category confinement, capability revocation, discovery, provider failures and routing disclosure.
- [x] Review combined diff, fix findings, run `bun run rust:test` and `bun run rust:lint`. Record actual results and limitations. Update this checklist and M.O.T.H.E.R checkpoint.

The default generated invite requests Manage Channels. Deterministic tests do not modify Discord. New channels follow existing routing policy; explicit allowlists remain explicit and tool results disclose whether replies are enabled.

## Validation results, 2026-11-14

- `bun run rust:test` passed, including core, packaged supervisor, daemon integration suites and 4,021 daemon unit tests (49 intentionally ignored). All 17 new channel-management tests and native HTTP regressions passed.
- `bun run rust:lint` passed (formatting and Clippy); existing repository warnings remain. `git diff --check` passed.
- Validation used the standard absolute `target/cargo/validation` cache with `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0` and `RUST_TEST_THREADS=4`. The enforced 32 GiB cache ceiling was never raised. An earlier oversized build was stopped and pruned by the repository wrapper.
- The machine's ordinary Temp ACL grants non-owner sandbox identities access, which existing document/privacy tests correctly reject. The final full run used a fresh short `ANIMAOS_TASK_TEMP_ROOT` with a protected inheritable owner-and-SYSTEM-only DACL. The task-owned directory was removed afterward; global permissions were unchanged. Local HTTP fixtures and a scratch registry test required execution outside the restricted sandbox.
- Two independent reviews found no owner-authority bypass. The reported lock-key issue was fixed so mutations serialize per connector. An existing schema inventory test was updated to include all three new tools.
- Initial implementation validation did not restart the normal runtime or modify Discord. Owner identity comes from the verified bot application owner, or the team owner for a team-owned application. Subsequent live acceptance is recorded below.

## Live acceptance, 2026-11-14

- The owner requested daemon restart and live Discord testing. Port 8080 was initially down; started the updated normal `bun run daemon` with existing persisted control-plane data. Health returned `ok` and M.O.T.H.E.R's connector became ready.
- The first live run listed channels, then received Discord's 403 permission denial for creation and stopped without retrying. After explicit owner approval, enabled only Manage Channels on M.O.T.H.E.R's role in Home; Administrator and Manage Roles remained disabled.
- The owner's Discord account `333667746107621386` (`reaper9000`) is the connector's sole approved user; approved roles are empty. Its actual inbound receipt initiated run `run_199aa62d-62c6-4635-93a0-9ad1e837b118` from message `1571124841640099953` in thread `1571099930171539590`, guild `1571099114232348805`.
- That completed run persisted successful receipts for all three native tools: listed Home channels, created `codex-channel-test-20261114`, then renamed the same channel `1571124943217889411` to `codex-channel-test-passed` and updated its topic. Discord's visible reply and sidebar confirmed the result. The labelled test channel remains available; no deletion was requested.
- Re-ran the wrapped validation filter `discord_channel_management`: 17 passed, zero failed. This covers nonowner, spoofed sender, console, stale owner and receipt/run/connector authority denial. A live second human account was unavailable; those rejection paths were verified by automated tests, alongside the live sole-user/no-role configuration.
- Saved visible evidence at `C:/Users/leoca/.codex/visualizations/2026/11/14/01a1d979-3fc0-7323-bb5e-c2848ca10a5c/discord-channel-live-passed.jpg`. Removed the task-owned focused-test log after recording results. The normal daemon and its runtime logs remain in place.

## Publication preparation, 2026-11-14

- On the owner's subsequent request, stopped the task's daemon launcher and its verified child processes. Confirmed zero remaining owned processes and no listener on port 8080; runtime logs remain available.
- Prepared `g9000/discord-channel-management` from `origin/main` so the two unrelated unpublished settings commits remain on local `main`.
- Re-ran the wrapped `discord_channel_management` validation on that branch: 17 passed, zero failed. No Rust source changed after the full validation or live acceptance. Removed the task-owned publication-check log after recording results.
