# Shared Rust engine: combined review and repair result

Date: 2026-10-25.

## Authorized migration/recovery and startup follow-up (2026-10-26)

In progress in the same isolated integration checkout. The user has now explicitly authorized isolated startup with the existing Anima model connection; the earlier startup authorization block below is historical. No main merge, commit, push or real-save migration is authorized.

Focused migration validation passed 188 memory tests. Both migration reviews are clear; rollback retains the canonical database and its current lifecycle, and the export is not a standalone restore image. Controlled process-death acceptance passed four parent tests plus 40 existing core contracts. Default in-memory capability lineage cannot automatically recover later retries; missing lineage now persists manual recovery without refunding reservations. Unknown provider work remains charged and explicitly host-paused.

Village acceptance passed 65 host tests (plus its parent-invoked child fixture), 30 App/presence tests, typecheck and browser lint. Two review findings were repaired: projection errors no longer hide committed supper output, and initial presence failures retry on the bounded interval. Final and terminal outbox projection and stale restart claims are covered. Both village reviews are clear. Shared daemon/game HTTP lifecycle and privacy checks passed focused runs; the latest game run also checks receipt replay after reopen.

Opt-in startup wiring and the final combined suite are still pending. The game resolver finds ChatGPT / gpt-6-luna, but its read-only credential lookup currently reports unavailable. No live model calls have been made.

**Repair round: complete. All four confirmed defects are repaired, and the final combined checks passed.**

**Merge decision: HOLD for the completed single-player engine plan.** Product activation, historical-memory migration and broader recovery acceptance remain unfinished. Passing this repair round does not certify those milestones.

## Integration and preservation

- Integration checkout: `C:/Users/leoca/.codex/worktrees/shared-engine-review/animaos-kit`, detached and uncommitted at base `37f4e8d0e40788fc3eab1cad75528439d9020b71`.
- Copied and SHA-256 verified 101 Sol files plus Luna's two reserved files. There were no overlapping edits or mechanical conflicts. All 103 original source-file hashes remained unchanged after the repair round.
- Sol source: `C:/Users/leoca/.codex/worktrees/349a/animaos-kit`; Luna source: `C:/Users/leoca/.codex/worktrees/08dc/animaos-kit`.
- Parent and repair agents worked only in the integration checkout. The final manifest covers 108 source/configuration/test and related files: zero hash drift or path changes during final validation; `git diff --check` passed.
- Authoritative plan/spec remain `docs/superpowers/plans/2026-10-25-unified-rust-engine.md` and `docs/superpowers/specs/2026-10-25-unified-rust-engine-design.md`.
- No commit, main merge, push, deployment, live-provider call, real-save migration or main-server/game restart was performed.

## Final verification

All commands below passed on the frozen combined sources. Rust used separate `target/cargo/combined-validation` and `target/cargo/combined-village` targets, temporary fixture databases and isolated local test endpoints. Full Rust validation used `CI=1` and `RUST_TEST_THREADS=1`.

| Check | Result |
| --- | --- |
| Root `bun run rust:test` | PASS: full supervisor, core and daemon suites. Daemon library: 3,377 passed, 38 intentional skips; API integration suite: 47 passed. Both optional-environment baseline tests and all new repair regressions were included. |
| Root `bun run rust:lint` | PASS: formatting and workspace/all-target Clippy correctness. Other warnings remain. |
| `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-harness --features test-support --test host_conformance` | PASS: 2 tests. Run explicitly because ordinary discovery does not enable this feature. |
| From `poc/village`: `bun run test` | PASS: 44 browser/tool, 19 domain, 54 host tests. One process-child fixture is intentionally ignored by ordinary discovery and exercised by its parent. |
| From `poc/village`: `bun run build` | PASS: TypeScript/browser and Rust builds; bundle-size warning remains. |
| From `poc/village`: `bun run lint` | PASS. |
| Frozen-source and whitespace checks | PASS: 108 files, zero source drift or path changes, whitespace exit 0. |

Both new SQLite adapters are included in the core package build/test/lint lists. The daemon test script runs the core suite. Eighteen daemon bridge tests passed, including the actual authenticated HTTP boundary; the village host tests exercised its injectable service/HTTP integration. These are deterministic host-boundary checks, not normal executable activation or live-provider acceptance.

The first baseline Rust run failed an exact-edit fixture because the integration checkout was created before Sol's new LF rule was copied. Applying that intended rule made the fixture byte-identical to Sol. The complete baseline rerun and the final post-repair run both passed. No test was weakened or skipped to address that failure.

## Closed findings

### P1: revoked captured memory reached a new provider call

The original reproduction admitted shared memory, narrowed the same identity's read scope, and still observed the secret in a new model request. Captured context now retains exact canonical records and revalidates identity, scopes, projection readiness, active lifecycle and full record equality. The additive `MemoryPort::get_active` defaults to rejection; SQLite intersects the bound authority with caller scopes and uses authoritative store time.

Validation also runs at each supported physical provider attempt after allowance waits. Composed guards survive retries/key rotation; denial cancels further retries. Transport that was never polled settles through the existing budget authority as `ProvenNeverSent`. Completed exact replay needs no new memory read/provider call. Legacy string-only context can replay completed output but cannot authorize new generation.

Permanent evidence: `anima-harness/tests/memory_revalidation.rs` (8 tests), `anima-memory-store-sqlite/tests/active_lookup.rs` (2 tests), and native execution tests in `anima-model-adapters/src/execution_tests.rs`. Original disclosure and native retry failures were observed before repair. A separate read-only reviewer found no remaining concrete defects in this repair scope.

### P2: direct core execution accepted failed accounting

The original direct `run_controlled` path could publish completion or dispatch a tool after settlement acknowledgement failed. The core now checks the shared execution failure before accepting model progress or authorizing another effect. Conservative outstanding reservations remain retained.

Permanent evidence: four tests in `anima-core/tests/durable_engine_contract/accounting.rs`; the complete 40-test durable contract target passed, including success and Stop behavior.

### P2: persistent evening callback lost remembered details

Callbacks now use typed facts from winning validated intents in the existing accepted-event outbox. Each fact selects its own latest source and checks current scoped canonical evidence. A second check after generation prevents newly forgotten facts entering the authored callback, personal cache or callback journal. Reviewed recovery uses the same facts; completed recovery receipts remain exact replays when memory is unavailable.

Six host regressions and one domain regression cover reopen, forgetting/correction, resident/world isolation, in-flight invalidation, latest-fact precedence and reviewed recovery/replay. Unknown correction prose suppresses unsupported authored facts; a fresh validated preference establishes new evidence. Historical outbox rows without typed metadata are not inferred from prose or stale save flags.

### P2: expired browser presence never recovered

The browser now reopens expired presence, bounds failed reopening to one attempt per heartbeat interval, avoids concurrent reopening, and closes late-issued sessions after disposal. The host returns 410 for eligible missing/expired sessions and 409 for replaced/explicitly closed sessions. Two tabs therefore cannot repeatedly steal each other's session through automatic recovery. No schema change or offline world-time catch-up was introduced.

Permanent evidence: eight browser presence cases plus server expiry/restart/replacement/explicit-close cases in the full village host suite.

## Remaining plan and acceptance gates

1. **Normal executable activation:** the daemon exposes `app_with_engine_bridge` and the game exposes `Service::with_durable`, but normal entrypoints still use legacy execution. Automatic approval review rejected village executable startup wiring because it could enable real provider calls and send game data outside the local app. That action was not retried or bypassed.
2. **Historical-memory migration:** imports accept portable memory records and directed individual relationship observations. Complete extraction/preservation of legacy entity, temporal-fact and temporal-relationship data, reversible product migration, and old untyped callback evidence are not established by save-preservation tests.
3. **Broader recovery acceptance:** process-death fixtures cover important dispatch/result/terminal boundaries, but the execution-store death fixture uses uncontrolled `run` and first-attempt effects. Durable physical-budget recovery and retry attempt two with fresh capability lineage need additional acceptance or an explicit profile exclusion. Default registry lineage is in memory.
4. **Shared conformance breadth:** the exported common suite covers basic replay/conflicts/identity/preference provenance/reopen. Broader lifecycle, privacy and crash cases currently use separate host fixtures.
5. **Retention and scale:** the snapshot adapter rewrites a bounded retained image without automatic pruning. Sol reported 241 runs with 32 KiB inputs near its 16 MiB cap and approximately 2–3 seconds per admission in debug; this review did not independently benchmark it. Production latency and multiplayer scale remain unproven.
6. **Delayed resident interaction:** inspection suggests a delayed/paused supper exchange can exhaust its fixed allowance and remain incomplete. Its terminal/recovery behavior should be specified without refilling an admitted budget. This concern was not independently reproduced and is separate from the four closed defects.
7. **Live acceptance:** no live-provider, human-language, player browser walkthrough, native-game binding, multiplayer or production-cutover acceptance was performed.

The combined implementation is preserved for review. The next milestone is to resolve the activation authorization and remaining migration/recovery acceptance scope before claiming the full plan complete or merging it as such.
