# Discord Rename Tool Implementation Plan

> Execute the approved design inline with test-driven development and independent review.

**Goal:** Let Anima rename its current Discord thread and confirm only an acknowledged result.

**Architecture:** Add a typed thread capability to native run rooms, a narrow native tool, and exact run-scoped discovery. Reuse the Discord manager and REST transport, serialize title effects, and retain existing action witnesses.

**Tech Stack:** Rust, Tokio, existing reqwest transport, serde, scripted model and fake Discord fixtures.

- [x] Add failing registration and current-thread workflow tests; verify the missing capability.
- [x] Add typed current-thread scope, run selection/discovery, write classification, and tool guidance.
- [x] Implement validated explicit renames, acknowledgement/error handling, metadata persistence, and automatic-title race protection.
- [x] Verify scope, failure, cancellation, delayed acknowledgement, and race regressions; review and address findings.
- [x] Run full Rust tests and formatting, record evidence, clean task scratch, and do not start or restart the ordinary daemon.

## Verification

- Initial registration and current-thread workflow tests failed before implementation; all 14 focused rename regressions now pass.
- Independent review caught stale discovery after connector reconfiguration and ambiguous PATCH server failures. Catalog provenance now retains the exact captured capability, and non-definitive HTTP failures report uncertainty and require inspection before retrying. Follow-up review found no further significant issues.
- Added the tool to the registry's exhaustive schema expectations after the first full run identified the missing entry. A later supervisor fixture hit an occupied temporary port; serialization avoided that fixture race without changing supervisor code.
- `CI=1 CARGO_TARGET_DIR=target/cargo/validation-rust-daemon RUST_TEST_THREADS=1 bun run rust:test`: passed, 4,825 tests across 69 result groups; 47 ignored. Includes core, daemon and packaged supervisor suites.
- Formatting and diff checks passed. Tests used local transports and scripted models; no live Discord rename or model-provider acceptance was attempted.
- Task-wrapper scratch was cleaned by the wrapper; the two task verification logs were removed after recording results. Cargo caches retained. No commit, push, or ordinary-server restart performed. A separately started dev daemon on port 8080 was observed and left untouched; it needs restarting to load this implementation.
