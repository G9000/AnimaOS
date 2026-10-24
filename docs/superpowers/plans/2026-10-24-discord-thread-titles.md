# Discord Thread Titles Implementation Plan

> Execute inline using the approved two-stage design and test-driven development; use an independent review before completion.

**Goal:** Give new Discord threads readable initial names and a guarded AI title after the first completed reply.

**Architecture:** Keep title lifecycle in connectors/discord/titles.rs, persist its identity/claim in DiscordConversation (creation claims are pruned with the cursor), and reuse the coordinator's existing bounded and metered model call. Guard native renames by fetching the current name, connector authority and routing identity.

**Tech Stack:** Rust, existing reqwest Discord transport, serde journals, Tokio tasks and current scripted model fixtures.

- [x] Add failing transport and runtime regression tests for mention cleanup and the first-reply title.
- [x] Add UTF-16-bounded title cleanup, persisted initial-name/one-shot markers, and native name-only PATCH support.
- [x] Extract the existing metered title call for reuse; schedule Discord title work after completed runs without delaying delivery.
- [x] Cover manual changes during generation, disabled/failed generation, older journals, later replies, and revoked authority.
- [x] Review the combined diff, address findings, run bun run rust:test and formatting, and record results. Preserve unrelated web edits.

## Verification

- Regression tests first reproduced raw mention IDs and missing AI renames.
- Independent review identified creation-journal pruning, delivery serialization, and first-completion crash windows. Title state now lives on the conversation, uses an independent lifecycle fence, and is consumed in the completed reply's source commit. Follow-up review found no remaining significant issues.
- `CI=1 CARGO_TARGET_DIR=target/cargo/validation-rust-daemon bun run rust:test`: passed, 4,811 tests across 69 result groups; 47 ignored. All 11 new regressions passed. Loopback fixtures required execution outside sandbox networking; no live Discord or model-provider acceptance was performed.
- `cargo fmt --all --check` and scoped `git diff --check`: passed.
- Task-wrapper temporary directories were cleaned by the wrapper; the task's exact verification log was removed after recording these results. Cargo caches retained. Unrelated web edits preserved; no commit, push, or ordinary-daemon restart performed.
