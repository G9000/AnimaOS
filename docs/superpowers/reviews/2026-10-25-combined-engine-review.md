# Shared Rust engine: final integration verification

Updated: 2026-10-26 (Asia/Kuala_Lumpur).

**Deterministic merge checks passed.** The user explicitly authorized merging both the shared Rust engine and development-command changes into main and pushing to origin. Normal Windows execution cleared the earlier restricted-runner permission, ownership, link and linker failures. Historical evidence remains in the earlier review documents; it does not describe the current test outcome.

## Verified implementation

- Shared durable execution, persistent identity and scoped memory are reusable Rust packages. Assistant conversation profiles and resident game agents use the same core with optional coding tools/workspaces.
- Protected SQLite execution, budget and memory adapters preserve exact requests, conservative uncertain reservations, effect reconciliation and source-scoped memory projection.
- Legacy import preserves typed memory/entity/relationship/temporal archives and idempotent receipts. Auxiliary unknown legacy evidence is inspection-only. Rollback retains the canonical database; the export is not a standalone restore image.
- Controlled process-death fixtures cover provider, effect, checkpoint/result and terminal boundaries, including later retry attempts. Missing durable capability lineage enters manual recovery instead of replaying an uncertain effect.
- Daemon and game executable activation is explicit and default-off. Selected daemon HTTP profiles, game residents, worker shutdown custody, draining, store binding, memory corrections/forgetting and cross-world privacy are covered by tests.
- The game retains the bounded Tavi/Mira supper interaction, terminal/retryable projection handling, presence lifecycle and restart persistence. The resident-activity future allocation repair passed the original normal-stack regression and the complete game suite.
- Root development commands now select app, docs, game, playground or all services. Docs and game skip assistant-owner startup. Existing owner-console authentication remains intact.

## Fresh verification before commit

All commands ran in the isolated review checkout with temporary fixture data and dedicated Cargo targets. No test or ownership guard was disabled to obtain these results.

| Check | Result |
| --- | --- |
| Root bun run rust:test | PASS: full core, daemon and packaged supervisor suites. Daemon library: 3,384 passed, 38 intentional skips. API integration: 47 passed. |
| Root bun run rust:lint | PASS: formatting and all-target Clippy correctness; existing warnings remain. |
| Harness tests with test-support enabled | PASS, including shared host conformance, deliberate invalid-adapter checks, privacy/lifecycle checks and hosted startup. |
| Village bun run test | PASS: 48 browser/tool, 19 domain, 69 host tests. A child-process fixture is invoked by its parent. |
| Village bun run build and bun run lint | PASS. |
| workspace-dev test | PASS: 59 tests across 13 files. Earlier Windows junction/temp/process failures cleared. |
| workspace-dev build, typecheck and lint | PASS. |
| Staged whitespace check | PASS. A trailing blank line in the new memory-store Cargo manifest was corrected without changing configuration semantics. |
| Existing Anima connection status | Available: ChatGPT / gpt-6-luna through the read-only game resolver. This checks credential access, not generation. |

The six daemon startup regressions all passed, including real profile ownership, selected adapter/provider/model/persona, omitted-profile drain/replay, configuration drift rejection and externally owned store preservation. Source files were staged from an explicit 137-file scope; generated fixture authority, credentials, databases, reports and caches are excluded.

## Acceptance boundaries

Durable startup remains opt-in for individually selected HTTP conversation profiles and resident game agents. Connector, schedule, helper and coding entrypoints retain their existing runner until separately certified. A copied legacy save is not automatically activated by executable startup; fixture migration remains explicit.

No real-save migration, real provider generation, player browser walkthrough, native-game binding or multiplayer/load acceptance was performed. Unknown remote requests retain their reservation, and automatic restart-safe later capability retries require durable lineage/results. Snapshot capacity/latency and general autonomous simulation remain separate work.

The isolated startup fixture and its fresh authority remain in test-output for further live acceptance. Local machine-readable results are in test-output/shared-engine-review/merge-validation-results.json. Historical baselines are preserved in 2026-10-25-combined-engine-review-before-startup.md and 2026-10-25-unified-engine-parity.md.
