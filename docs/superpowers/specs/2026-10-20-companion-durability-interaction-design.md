# Companion durability and interaction

The owner approved implementing all seven proposed companion improvements on 2026-10-20. Extend the current Rust runtime and daemon rather than introducing a second execution engine. Preserve unrelated local changes; leave this work uncommitted. Product copy uses Anima. Retain copyright and license notices for reused substantial code.

## Behavior

1. Interrupted work offers a durable recovery review and one linked continuation. Keep the original evidence immutable. Never automatically repeat uncertain external effects; unknown actions require an explicit review. Reuse accepted-run admission, current grants, native receipts, saved tasks and queue persistence.
2. A side question uses a bounded snapshot of the selected agent/session, no tools and separate usage accounting. It answers while the main task continues and cannot steer, cancel or commit into that task.
3. The agent can ask a structured question with up to four choices and a free-text answer. Save the request before showing it. Scope answers to the actual agent, session, run and request. Wait without holding state locks; stop and restart preserve honest pending/answered status. An interrupted question can be answered and continued through recovery rather than blindly replaying the original tool.
4. Preserve the existing completion evidence reminder and exact-tool-cycle warnings. Add bounded protection against demonstrably degenerate model repetition, with tests for ordinary tables and legitimate repeated data. Completion claims remain tied to settled evidence.
5. `/learn <workflow or source>` starts an ordinary authorized task that gathers the relevant sources and proposes a versioned skill through existing reviewed draft/install controls. No independent memory or skill execution authority is introduced.
6. Event-triggered automations accept authenticated owner events and native task outcomes through the existing scheduler. Bound payloads, durable occurrence deduplication, event recursion and capacity. Do not turn arbitrary payloads into trusted instructions. Existing time triggers and delivery targets remain compatible.
7. Reuse the existing `execute_code` native tool broker. Make its availability, interpreter setup and grants understandable and verify a real multi-tool chain with synthetic data. Preserve native per-call approvals, cancellation, receipts, and physical process custody.

## Boundaries and validation

Reusable policies and prompt construction belong in `packages/core-rust`; daemon state, auth, process custody and HTTP handlers belong in `hosts/rust-daemon`. SDK methods are thin HTTP clients. New UI components use existing chat styling and preserve the current WorkspaceShell changes.

Upgrade the control-plane format once for the new durable fields, with an exact pre-upgrade backup and older-version refusal. Tests must cover duplicate submissions, save failures, restart without replay, cross-agent requests, stale UI results, cancellation, malformed inputs and unchanged legacy behavior. Use isolated stores and deterministic models for runtime tests; do not access personal profiles or make real external sends.

Required gates: `bun x nx run rust-daemon:test --skipNxCache`, relevant SDK/web tests and typechecks/builds, Rust formatting, and the docs build if owner documentation changes. Windows Nx native loading currently needs per-command `NX_SKIP_NATIVE_FILE_CACHE=true`, `NX_DAEMON=false`, `NX_ISOLATE_PLUGINS=false`, `CI=1`. These are validation environment settings, not product changes.
