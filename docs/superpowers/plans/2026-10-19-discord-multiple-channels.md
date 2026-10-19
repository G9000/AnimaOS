# Multiple Discord Channels Implementation Plan

**Goal:** Allow one agent to connect to multiple Discord text channels, with replies delivered only through the connection that received the message.

**Architecture:** Keep one immutable channel, private credential, conversation room, inbound cursor, and outbox per connection. Replace the per-agent uniqueness rule with per-agent/channel uniqueness in creation and snapshot validation. Preserve existing channel, sender, epoch, and credential checks; do not broadcast replies. Owner messages sent from a specific Discord conversation continue to target that conversation's channel.

**Tech stack:** Rust daemon, existing run coordinator and Discord transport, React/TypeScript console, Nx verification.

- [x] Add regression coverage for creating three connections for one agent, duplicate channel requests, independent conversation/delivery routing, isolated disable/delete, and snapshot restore.
- [x] Run focused regressions and confirm the current single-connection restriction fails them.
- [x] Update connection and snapshot uniqueness; give duplicate channels a specific safe API/UI error. Explain multiple channels and reply routing in setup help.
- [x] Share native Gateway identify queues by verified bot identity, so starting several channels respects the bot's login budget. Cover token replacement and cancelled queued logins.
- [x] Run Rust host tests, focused Discord web tests, web typecheck, and Rust formatting through the existing Nx targets. Review the final diff and preserve unrelated Astro settings edits. Record the unrelated full-suite failures below.

Validation uses fake transports in this checkout. It does not send live Discord messages, change stored credentials, or restart the running daemon. The change is prepared for integration into main, with the scoped checks passing and broader check failures recorded below.

Initial evidence: the original Rust implementation failed four of the five new regressions (create and restore restriction). After the uniqueness fix, all five passed. The focused web suite passed 20 tests, and the web Nx typecheck plus its dependencies passed. Review found that independent native Gateway queues could issue simultaneous bot logins; the queue fix is part of this scope.

Full pipeline constraints observed during verification: sandboxed loopback tests were denied sockets; the unsandboxed core suite passed those tests but failed 10 existing `anima-worktree` tests on Windows verbatim paths (`//?/C:/...`). The global formatting target also reports existing formatting differences in unrelated approval, clock, automation, and utility files. Preserve those files and report these separately from the changed Discord paths.

Final validation:

- PASS: `bun x nx run rust-daemon:test --excludeTaskDependencies --skipNxCache --args=discord` with `CI=1 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/validation-rust-daemon CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUST_TEST_THREADS=1`: 58 Discord tests passed. Executed outside the sandbox to permit loopback fixture servers. Includes the Gateway pacing regression, which failed before the shared-queue fix.
- PASS: `bun x nx run @animaOS-SWARM/web:test --excludeTaskDependencies --skipNxCache --args='--run src/lib/discord.test.ts src/components/DiscordSettings.test.tsx src/hooks/useDiscordIntegrations.test.tsx'`: 20 passed. Workspace-local `TEMP`/`TMP` avoided sandbox temporary-file rename restrictions.
- PASS: `bun x nx run @animaOS-SWARM/web:typecheck --skipNxCache`, including nine dependency tasks.
- PASS: scoped `rustfmt --check`, Prettier checks, and `git diff --check` for the changed files. Focused code review found no remaining actionable issues after the Gateway queue and active-worker test fixes.
- FAIL outside this feature: the full required `rust-daemon:test` pipeline stopped at the core dependency's 10 Windows worktree failures. Running the full daemon target separately with dependencies excluded produced 2,428 passed, 5 failed, and 29 ignored: four unchanged worktree tests and one unchanged bundled-skill manifest comparison (LF versus CRLF). Integration binaries compiled, but Cargo stopped after the failing unit suite. Full `rust-daemon:lint` remains blocked by the unrelated formatting differences listed above.

Live Discord delivery remains unverified. The running daemon and credentials were not changed; restart the dev launcher to load the implementation.
