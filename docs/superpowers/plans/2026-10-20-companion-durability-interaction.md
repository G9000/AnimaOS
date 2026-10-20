# Companion durability and interaction implementation plan

> Execute with the planning, subagent development, test-driven development and verification skills. The owner has approved implementation; do not add another approval gate. Independent module work may be delegated with explicit file ownership. Parent owns integration and final validation. No commits, staging, worktree changes or external sends.

**Goal:** Deliver the seven approved companion improvements through the current Rust runtime, SDK and chat UI.

**Architecture:** Reuse the accepted-run ledger, ordered control-plane saves, scoped tool registry, metered side model, skill draft registry, scheduler and native programmatic broker. Add small focused modules and a single versioned durable format upgrade. Preserve original action evidence and make uncertainty visible.

**Stack:** Rust, Tokio, Axum, serde; TypeScript SDK; React; Nx/Bun.

## Work and ownership

- [x] Recovery: parent adds `agent_runs/recovery.rs`, `routes/run_recovery.rs`, run-ledger/DTO fields, SDK recovery methods, and a focused chat recovery component. Test immutable source, one successor, review-required unknown actions, restart and current permissions.
- [x] Interaction: delegated owner adds bounded detached side questions plus durable structured questions, their route/tool integration, SDK methods and focused chat components. Coordinate shared root modules/schema with parent. Tests cover no main-run mutation, no tools, source bounds, answer validation, stop/restart and stale answers.
- [x] Completion and loops: delegated owner extends core runtime repetition protection and meaningful completion tests, retaining existing guard behavior and upstream notices. Verify false-positive cases and finite failure behavior.
- [x] Learning: delegated owner adds host-neutral `/learn` prompt construction and normal-turn dispatch through existing skill proposal review. Test source framing and unchanged user requests for other commands.
- [x] Event automations: delegated owner extends schedule triggers, durable occurrence admission, owner event endpoint and native outcome event integration. Add SDK/editor support and deduplication, recursion, restart and permission tests.
- [x] Programmatic chains: reuse existing broker; expose setup/availability and selected grants, add a synthetic multi-tool workflow verification and owner documentation. Do not create an extra interpreter/runtime.
- [x] Branding: inspect current product surfaces; use Anima in new copy and remove upstream product branding where encountered. Preserve required license/copyright/provenance files and historical evidence.
- [x] Session notice: consolidate this session's adapted material into the neutral root `NOTICE`, remove optional upstream branding and revision labels from its attribution, and retain the original MIT copyright and permission text. Earlier source notices remain intact.
- [x] Integration review: inspect combined diff, run focused tests first, address independent review findings, then run required Rust/SDK/web/docs gates in the final tree. Record real provider acceptance separately from deterministic validation.

## Verification commands

Before each behavior change, add a meaningful failing regression and verify the failure when the current target can run. Use the existing Nx targets, with the environment workaround stated in the design. Serialize Cargo builds to avoid shared-target contention. Capture logs in `.superpowers/companion-upgrade-2026-10-20/`.

1. Focused core/daemon tests via Nx `--args=<test filter>` where supported by the inspected target.
2. `bun x nx run rust-daemon:test --skipNxCache` (includes the reusable Rust test dependency).
3. `bun x nx run @animaOS-SWARM/sdk:test --skipNxCache`, SDK typecheck/build.
4. `bun x nx run @animaOS-SWARM/web:test --skipNxCache`, web typecheck/build.
5. `bun x nx run rust-daemon:lint --skipNxCache` and `bun x nx run @animaOS-SWARM/docs:build --skipNxCache` after documentation changes.

Do not mark a task complete based on design, compilation alone, or a delegated success report. Final report must identify any unverified live-provider or platform behavior.

## Verified result - 2026-10-21

The final checks below ran after the concurrent dependency upgrade and scheduling work settled. All implementation remains on `main`, unstaged and uncommitted. Logs are in `.superpowers/companion-upgrade-2026-10-20/`.

| Check | Result | Evidence log |
| --- | --- | --- |
| Full `rust-daemon:test --skipNxCache`, including `core-rust:test` | 3,898 passed; 37 opt-in cases ignored | `rust-verified-final.log` |
| Full web test suite, two workers | 1,961 passed; 1 skipped across 155 files | `web-verified-final.log` |
| Full SDK test suite, service tests enabled | 239 passed across 37 files, including all three real-daemon/local-model-stub integration cases | `sdk-verified-final.log` |
| Explicit physical Python chain | Two native reads, one native write, fresh execution approval, four successful receipts, verified process cleanup | `physical-chain-final.log` |
| SDK/web/UI typechecks and SDK build | Passed on the upgraded toolchain | `types-upgraded-final.log` |
| Production web build | Passed | `web-upgraded-build.log` |
| Astro owner guide and Pagefind search index | Passed, 30 pages indexed | `docs-upgraded-final.log` |
| Rust formatting gate | Passed | `rust-verified-lint.log` |

Windows validation used a separate `target/validation-rust-daemon` daemon build because the normal executable was running. Node 23.11.0 and Bun 1.4.2 were selected locally; the updated CI runtime pins were not exercised. No normal daemon restart, profile migration, global interpreter configuration, external send, or live model call was performed by this work. The Python interpreter was selected only for the synthetic acceptance test.

Independent reviews identified and resolved late-receipt recovery fingerprints, dominant text repetition, persistent-question admission, recovery attribution, retention of historical links, and preservation of owner steers. The reload fixture now waits for the public chat input to become enabled before typing or resending. Full validation then passed in the settled tree.

This validates the implemented deterministic boundaries and the synthetic native chain. Live provider/connector behavior, hosted CI and other operating systems remain unverified. The already-running normal daemon must load a new build to expose these changes.
