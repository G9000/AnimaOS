# Discord Attachments Implementation Plan

**Goal:** Let approved Discord participants send readable files/images and receive generated files in the current thread.

**Architecture:** Reuse durable upload records and model hydration for inbound files. Extend the current-thread capability and native transport for an awaited, receipted outbound file tool. Preserve unrelated local changes and the user's running services.

**Tech Stack:** Rust daemon, reqwest 0.12, existing capability-relative workspace I/O, history store and run journals.

## Tasks

- [x] Write native metadata/attachment-only regression tests and observe failure. Add serde-default metadata and intake checks (`model.rs`, `client.rs`, `interactions.rs`, `inbound.rs`).
- [x] Add focused `connectors/discord/attachments.rs` for bounded native CDN downloads and scoped save/reuse. Journal attachment IDs only after files/history save; pass them to acceptance and use attachment_content in processing. Verify failures cannot advance cursor, unsupported files produce notices and prior saves are reused.
- [x] Write multipart/output regression tests and observe failure. Add native send_file primitive in a focused client attachment module, sharing rate-limit/response handling. Test receipt evidence, disabled mentions, nonce and uncertain responses.
- [x] Add `send_discord_file(path, caption?)` through thread scope, tools registry/catalog/scope and write policy. Capture bounded safe workspace bytes, recheck authority/cancellation before effect, use owned lifecycle. Test direct-thread success/ack, invalid paths and withdrawn scope.
- [x] Update exhaustive registry contract, companion docs and invite permission. Review combined diff, run focused tests then `CI=1 CARGO_TARGET_DIR=target/cargo/validation-rust-daemon RUST_TEST_THREADS=1 bun run rust:test`, format and diff checks. Record results and clean only task-owned logs.

No daemon restart or commit/push is part of this request.

## Verification

Full required Rust gate passed: 4,844 passed, 47 ignored across 69 result groups; 559.73 seconds. All 19 new attachment regressions passed. Focused daemon lint (format) and Git diff checks passed. Independent source review found no remaining material issue. No live Discord/model-provider delivery test was performed; native API and runtime acceptance are covered by local fixtures.
