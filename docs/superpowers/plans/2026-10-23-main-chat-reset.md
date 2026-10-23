# Main Chat Reset Implementation Plan

> Execute in this session. The owner authorized implementation and subsequently requested a scoped commit and push. Preserve all pre-existing and concurrent changes; publish only the main-chat reset feature.

**Goal:** Clear the main conversation on request, keeping the companion and saved memories.

**Architecture:** One coordinator-owned reset operation, a durable run request for self-reset, and an idle owner HTTP operation. Reuse history deletion ordering and main-chat/session live updates.

**Tech Stack:** Rust daemon/runtime, React web client, Nx validation.

- [x] Add regression tests for owner main-chat reset, preservation, persistence rollback and a reset requested during a running turn. Run them to establish failure.
- [x] Implement a focused coordinator reset module, native tool registration/exposure, durable run request and post-turn settlement. Preserve existing deletion protections and memory stores.
- [x] Add the idle clear endpoint, web `/clear` and `/reset` handlers, and focused web tests. Verify command dispatch and refreshed history.
- [ ] Run `bun x nx run rust-daemon:test --skipNxCache` using the isolated validation target on Windows; run relevant web Nx tests and build/typecheck. Address failures attributable to the change.
- [x] Review the resulting diff and regression coverage. Update this checklist with actual evidence and report any live-provider acceptance limitation.

Validation so far:

- Web: all 100 session-hook tests, all 45 SessionView tests, and both anchored-main-chat reset journeys pass. The latter exercise slash dispatch and a reset event from another window. Focused slash, event reducer and live-session tests also passed earlier.
- Web typecheck, SDK build/typecheck, and web production build pass. SDK tests passed 253 tests before the final optional session generation field was added; subsequent SDK build/typecheck passed.
- Review addressed queued legacy inputs, unsettled helper lineage, concurrent helper launch, failed final saves, clarification rollback, side-question invalidation and anchored-history invalidation. The final read-only review reported no remaining blockers.
- All 13 Rust reset regressions pass, covering native tool/discovery, owner routes, saved Fact preservation, hot/cold cleanup, rollback, durable deletion acknowledgement, startup/failed-turn recovery, accepted and legacy queued inputs, helper launch races and restorable helper lineage. The API and tool-schema inventory tests also pass in that successfully Nx-built test binary.
- Full Rust gate remains blocked. An earlier run passed the core dependency and 3,021 daemon tests, failing two reset test/inventory issues (now corrected and passing) and three concurrent profile-ownership/document-admission tests. The final required command stopped at seven newly added execution-limit regression failures in the concurrent core work; the daemon task was skipped. This task did not modify those modules.
- Broader ViewHarness integration tests have existing missing-alert failures; one reproduces with the original ViewHarness from HEAD. Temporary baseline copies were removed.
- No live provider conversation was cleared. Natural-language tool dispatch is exercised using the deterministic model; the owner's real chat and saved memories were not used as test data.

Publication verification used a clean checkout at `042f95612839789b64fb085ab51a50d66a753929` plus only the 42 reset-owned paths/hunks. The full Rust core dependency suite passes. The daemon run passed 2,990 tests, including every reset regression, with three unrelated failures: two PDF deadline/cancellation tests pass when rerun individually; the helper fixture test assumes LF inside `include_str!` but the Windows checkout contains CRLF (the exact source match is present after newline normalization). No fixture or runtime reliability changes are included. The exact publication build also passes all 13 reset regressions on a separate rerun. All 100 session-hook tests, both anchored reset journeys, web build/typecheck and SDK build/typecheck pass in the isolated checkout.
