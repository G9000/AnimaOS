# Reminder delivery evidence implementation plan

**Goal:** Preserve reminder identity and schedule, journal channel send evidence, and let Anima explain a specific reminder without inferring receipt from execution.

**Architecture:** Extend daemon automation records with a bounded, snapshot-persisted delivery journal correlated to the occurrence and outbound message. Record workspace publication and Telegram/Discord transport results under their existing commit and rollback boundaries. A read-only `check_automation_status` tool and owner-authenticated HTTP endpoint derive scheduled/sent/failed/delivery-unconfirmed status from this journal. Channel send acknowledgements never establish device delivery or reading.

**Tech stack:** Rust daemon, serde, existing control-plane snapshots, Nx/Bun validation.

- [x] Write regression tests for successful send, rejection, ambiguous/missing evidence, and execution without a receipt.
- [x] Add delivery event DTOs, snapshot compatibility, bounded retention, exact scheduled time, and conservative status derivation.
- [x] Connect scheduler publication and transport attempts/results to the journal, preserving transaction rollback and uncertainty after persistence failures.
- [x] Register the scoped status tool, expose an owner-authorized status endpoint, and grant the read capability through the existing migration policy and web companion access profiles.
- [x] Add explicit `verifyOnce` creation support: atomically save one reminder and one check after the due time; no implicit recurring watcher or polling loop. Pending checks move with edits to the reminder's time/destination; changing to a recurring trigger disables the pending check.
- [x] Exercise scope/auth, persistence, transport integration, and a scheduled one-time status check. Document sending versus receipt and existing monitor behavior.
- [x] Run `bun x nx run rust-daemon:test --skipNxCache` and Rust formatting validation; review the final diff.

## Behavior and limits

Before the first due time, status is scheduled. After that, no send acknowledgement means delivery unconfirmed, even if a model run says spoke. A definite channel rejection means failed; timeout, malformed/missing receipt, partial chunk completion, and interrupted sending remain unconfirmed. A valid send acknowledgement means sent, with an explicit statement that receipt cannot be verified. Workspace publication confirms saving the assistant message in its thread only. Current native bot channels expose send acknowledgements, not device-delivery/read receipts.

For an explicitly requested reminder test, `verifyOnce: true` retains the returned automation ID and due time and atomically saves one `once` follow-up five seconds after that time. The runtime captures the original reminder's status once and supplies it to an explanation-only Anima run with no tools, including when the original run is still in progress. It cannot perform further lookups or start background work. Existing scheduled monitoring remains available only when the user asks and the required tools and scheduler are available. This change does not start any real reminder or external send during development.

## Verification

- Full `rust-daemon:test --skipNxCache` and its `core-rust:test` dependency passed on the final saved tree. Windows validation used `CI=1`, `CARGO_TARGET_DIR=target/validation-rust-daemon`, and `RUST_TEST_THREADS=8` outside the sandbox for synthetic localhost HTTP fixtures. The normal target was locked by the running daemon. The first unrestricted full run also exposed an unrelated 500 ms document-fixture race under default concurrency; the bounded-concurrency runs passed.
- All 16 reminder regressions passed, including real injected transport success/rejection/ambiguity, multi-chunk uncertainty, failed settlement persistence, scope/auth, snapshot compatibility, one-time verification, retiming rollback, and legacy unknown scheduled times.
- Web access-profile/activity tests: 30 passed. Web typecheck and Rust formatting validation passed.
- Independent code review findings were resolved. No live Telegram or Discord send/device-receipt acceptance was attempted; the running daemon was not restarted.
