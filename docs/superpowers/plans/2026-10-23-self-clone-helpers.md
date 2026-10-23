# Focused Helper Implementation Plan

> Execute inline with focused regressions and review. Preserve unrelated edits and leave work unstaged.

**Goal:** The companion delegates to same-model helpers with fresh conversations and separate task/playbook instructions, omitting the main personality.

**Architecture:** Reuse existing helper configuration, lifecycle, and durable records. Compose task/playbook input once for both native launch paths, and exclude parent personality from worker prompts, including roster metadata. Keep existing project instructions, grants, approvals, and bounds.

**Tech Stack:** Existing Rust daemon, anima-core prompt composition, native schemas, and scripted-model tests; no new dependencies.

- [x] Inspect focused-worker defaults and current Anima helper behavior. The user selected a focused worker persona that omits the main personality.
- [x] Add bounded task/playbook composition before launch admission in `tools/team.rs`, and update both native schemas and schema inventory expectations.
- [x] Keep focused helper configuration and update main/helper host guidance. Suppress parent persona biographies from helper roster context.
- [x] Add focused regressions in `agent_runs/self_clone_tests.rs` and `self_clone_async_tests.rs` for legacy/SOUL omission, playbooks, reuse, durable async input/replay, invalid input, and native async model requests.
- [x] Complete implementation review and pass current-tree focused verification, `bun x nx run rust-daemon:test --skipNxCache`, `rust-daemon:lint`, `rust-daemon:build`, and scoped diff checks.

Initial regression work observed the native generic helper persona and absence of parent SOUL. Following reference research and the user's explicit choice, the final implementation retains that omission and adds assignment playbooks and roster privacy. Earlier inheritance-prototype test/build results are superseded and are not acceptance for this design.

Do not restart the running demo, commit, or push as part of this request. Current tests use scripted models and no real provider. Record final evidence and limitations here.

Final current-tree verification passed: six focused native helper regressions; the complete `rust-daemon:test` target and `core-rust:test` dependency with cache bypassed; `rust-daemon:lint`; `rust-daemon:build`; and scoped diff whitespace checks. The host library passed 3,176 tests with zero failures and 37 intentional ignores; every host integration suite passed. Final broad verification used `RUST_TEST_THREADS=4` and isolated `target/cargo/sdk-daemon-integration` outputs. A prior superseded broad run encountered an unrelated controlled Docker fixture exceeding its 150 ms success deadline; no execution backend code or test was changed. The full final run passed that case unchanged.

Code and final docs were reviewed with no remaining findings. All seven feature source/test inputs retained their SHA-256 values through the final full test/build checks. No real provider, private profile, or network-connected agent task was used. The running older demo daemon remains untouched and needs a dev-session restart to load the feature. Work is unstaged and uncommitted.
