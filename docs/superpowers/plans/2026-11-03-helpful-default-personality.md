# Helpful default personality implementation plan

> **For agentic workers:** Execute the following steps in order, with focused review and verification.

**Goal:** Give uncustomized agents and newly created workspace managers a warm, attentive, curious default that the owner can replace.

**Architecture:** Keep one Markdown default in the host-independent core. Use it only when configuration has no system personality, and include it in newly generated workspace-manager profiles. Accepted workspace/profile SOUL text continues to replace the entire configured personality. Existing custom system text, including intentionally empty text, remains untouched. Conversation preferences can override default tone, verbosity, initiative, and question frequency; personality never grants permissions. No saved-profile migration or daemon restart.

**Tech stack:** Existing Rust prompt renderer and Vite raw asset imports (verified against Vite 8.3.5 documentation).

**Review clarification:** Configured bio, adjectives, communication style, and initiative explicitly override conflicting default traits even when `system` is absent. Test their preservation together with the default.

## Implementation

- [x] Add runtime regression coverage in `packages/core-rust/crates/anima-core/src/runtime/agent_files_tests.rs`: capture default prompt, custom system replacement, SOUL replacement and removal, retained knowledge/project/host guidance, unchanged saved config. Check the personality editor's renderer agrees with runtime default.
- [x] Run focused core tests through the managed validation wrapper and observe the missing default assertion fail.
- [x] Add `packages/core-rust/crates/anima-core/src/default_personality.md`; expose its text as a documented constant in `agent.rs`; replace the one-line fallback. Wording covers listening/corrections, purposeful questions, initiative, warmth, honesty, follow-through, and owner customization.
- [x] Use that constant in the daemon context estimate fallback in `hosts/rust-daemon/src/sessions/context.rs`; test default/custom/SOUL parity.
- [x] Include the same Markdown in `apps/web/src/lib/workspace-manager.ts`, before role and owner preferences. Keep guided/balanced/proactive options authoritative and test their coexistence.
- [x] Document override order and unchanged existing profiles in `docs/companion/daily-use.md`.
- [x] Run `bun run rust:test`, `bun run rust:lint`, and web `test`, `build`, `typecheck`. Review the diff and record evidence below. Tests establish instruction selection, not live-model personality quality.

## Verification

- New core regression and web profile regression failed for the expected missing-default assertion before implementation, then passed.
- Focused core prompt-selection tests: 4 passed. Focused daemon personality tests: 20 passed, including context estimates, SOUL source replacement, saved configuration, and fresh-session readback after restore.
- Web suite rerun after concurrent, unrelated chat-display changes: 223 files passed; 2,702 tests passed and 1 skipped. Web build and typecheck passed in that tree. Web lint passed with existing warnings.
- A later concurrent ConversationControls edit was preserved. Final focused workspace-manager/ConversationControls tests passed (7 tests); web build and typecheck were rerun successfully after that edit. The full 2,702-test result predates this last unrelated UI edit.
- Managed `rust:lint` passed (format and workspace Clippy); existing workspace warnings remain.
- Full native validation first encountered restricted Windows file/registry/local-server access errors. An approved unrestricted retry passed packaged supervisor and core suites, but three daemon integration-test binaries reported an `ort_sys` rlib linking error. A focused rebuild of the previously failing `step_log_integration` binary subsequently passed without source changes or cache deletion.
- The next full run passed 3,974 daemon unit tests but failed 8 in Windows storage/recovery and drain accounting (48 ignored). Separate integration execution hit a one-second HTTP read timeout. Rerunning the failing unit groups sequentially passed recovery/drain checks and isolated four failures to insufficiently private ambient temporary-directory permissions. A fresh task-owned temporary root with owner/SYSTEM-only ACLs, supplied through `ANIMAOS_TASK_TEMP_ROOT`, made all 10 selected checks pass. No existing directory ACLs or product code changed.
- Final `bun run rust:test` **passed (exit 0)** with that private temporary root and `RUST_TEST_THREADS=1`: full core, packaged supervisor, daemon unit/integration and doc-test suites. The daemon unit suite passed 3,982 tests (48 intentionally ignored); personality/context and the previously failing HTTP integration checks passed. No tests were newly skipped and the 32 GiB managed Cargo ceiling remained enforced. Task-owned logs and the private temporary root were removed after recording results.
- Independent code review approved the change after clarifying that configured legacy style/adjectives override conflicting defaults.
- No live-provider conversation-quality test, saved-profile migration, daemon restart, commit, or push. Concurrent chat-display edits are outside this change and were preserved.
