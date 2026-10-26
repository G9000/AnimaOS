# Unified engine implementation evidence

> Historical implementation baseline. Current startup is explicitly authorized and opt-in. Subsequent migration, recovery, startup and validation evidence is recorded in [the combined review](2026-10-25-combined-engine-review.md); the blocked-startup notes below describe the earlier approval state.

Baseline: `37f4e8d0`. Implementation is isolated in worktree `349a`. No commit,
push, deployment, real-save migration or user-service restart is authorized or
performed. The approved plan/spec were copied unchanged from the originating
worktree. Luna's two reserved baseline files are not part of this patch.

This records the verified opt-in implementation, not a completed cutover certification. Tests
below use temporary data and deterministic models; provider quality and a human
player walkthrough are separate.

## Final verification

| Command | Current result |
| --- | --- |
| `bun run rust:test` with `RUST_TEST_THREADS=1` | Passed on final code: all core, daemon and packaged supervisor suites. Daemon library: 3,377 passed / 38 ignored; daemon API integrations: 151 passed / 6 ignored. |
| `bun run rust:lint` | Passed: format check and workspace/all-target Clippy correctness. Non-correctness warnings remain. |
| `bun tools/workspace-dev/scripts/run-task.ts --cwd . -- cargo test -p anima-harness --features test-support --test host_conformance` | 2 passed on final dependencies. |
| From `poc/village`: `bun run test` | 38 browser/tool, 18 domain and 46 host tests passed; one peer entry is ignored in discovery and explicitly exercised by its parent. |
| From `poc/village`: `bun run build` | Passed: TypeScript, browser and Rust builds. Nonfatal large-bundle warning. |
| From `poc/village`: `bun run lint` | Passed without warnings on final dependencies. |
| `git diff --check` | Passed. Plan/spec hashes still match the originals. |

Windows tests use normal permissions for private temporary ACLs, loopback test
servers and a scratch registry key. Sequential test execution avoids collisions
between short-lived CLI fixture ports; concurrency scenarios within individual
tests still run. Child execution-store peers use quiet libtest output so their
readiness markers remain independent of test-runner parallelism. The M52 exact
edit fixture now has an explicit LF checkout rule; its checked-in source bytes
are unchanged.

## Library evidence recorded in this worktree

All focused Cargo commands use `bun tools/workspace-dev/scripts/run-task.ts --cwd . --`.

| Command suffix | Observed result | Scope and limitation |
| --- | --- | --- |
| `cargo test -p anima-execution-store-sqlite --test open` | 6 passed | Create/reopen, wrong key, future schema, failed migration rollback, corrupted/missing authority. No workspace/config lookup. |
| `cargo test -p anima-execution-store-sqlite --test conformance` | Public suite passed | Real SQLite, all public reference-store authority checks. Dedicated retry/concurrency evidence is additional. |
| `cargo test -p anima-execution-store-sqlite --test atomicity` | 6 passed, also in broad core run | Independent lease/policy and serial-session contention, concurrent retry identity/conflict, expired fence across clock rollback, failed update and real deferred-FK COMMIT rollback. |
| `cargo test -p anima-execution-store-sqlite --test engine_recovery` | 2 parent tests passed; child entry intentionally ignored in discovery | Actual child kills at model checkpoint, pre-effect dispatch, effect-before-result, result commit, terminal commit. Separate physical-effect ledger; exact final output and tool-call correlation; uncertain non-idempotent effect not replayed. |
| `cargo test -p anima-execution-store-sqlite --lib` | 2 passed | Cancelled caller custody and every concurrent shutdown waits for full drain. |
| `cargo test -p anima-execution-store-sqlite --test budget --test open --lib` | 14 passed | Shared finite allowance persistence and binding to owner/store/scope. Missing metadata never creates replacement authority. |
| `cargo test -p anima-execution-store-sqlite --test admission` | 2 passed | Opaque scope mapping/reopen/collisions; payload-free admission retains no private side-table payload. |
| `cargo test -p anima-core --test durable_engine_contract` | 36 passed | Existing engine controls, policy races, approvals, lease heartbeat and recovery behavior retained. |
| `cargo test -p anima-core host_admission_uses` | Targeted protection regression passed | Payload permission, declared-secret replacement, wrong key, owner and run binding. |
| `cargo test -p anima-core --test identity_contract` | 1 passed | Stable individual identity contract, separate optional resident mapping. |
| Final broad core run: `anima-memory` and `anima-memory-store-sqlite` | 155 and 22 passed | Canonical memory, exact lifecycle inspection, scoped projection receipts, directed relationship evidence, copied-manifest import, real COMMIT failure, physical process kill/reopen, stable store identity, concurrent initialization and rejected-write clock floor. |
| `cargo test -p anima-memory-store-sqlite --test memory_contract expired_memory` | Regression failed before fix; passed after fix | Observed expiry survives reopen with earlier host clock. |
| `cargo test -p anima-harness --test durable --test persistent_agent` | 3 passed | Dialogue/persona/settings/exact replay, same-session admission, frozen identity/definition, preference recall after memory reopen and explicit outage/readiness refusal. |
| `cargo test -p anima-harness --test durable_capability` | 3 passed | Non-filesystem tool, admitted actor/scope binding, Stop retains custody/accounting until effect retirement. |
| `cargo test -p anima-harness --features test-support --test host_conformance` | 2 passed | Shared basic contract on reference host; deliberately regenerated retry identity is rejected. Does not stand in for either product host. |
| `cargo test -p anima-execution-store-sqlite --test restart` | 1 passed; 2 explicitly invoked child entries ignored in discovery | Killed writer and fresh verifier: request identity/input, stale writer rejection, foreign-owner denial, cursor continuation and exact completed output. |
| `cargo test -p anima-execution-store-sqlite --test export_clock --test atomicity` | 9 passed after three failing regressions | Both capacity and protection export rejection previously allowed an expired writer to commit after clock rollback. Floor-only persistence now preserves exact snapshot/generation and rejects the stale writer. Actual floor-COMMIT failure returns storage unavailable and rolls back. |
| Execution and memory initialization stress tests | 512 simultaneous first opens per adapter passed | Both adapters previously returned immediate SQLite BUSY while switching WAL mode. Bounded WAL-only BUSY/LOCKED retries retain the existing timeout budgets. Memory preflight also needed one read snapshot to prevent torn schema observations. Actual schema/key/corruption errors are not retried. |
| `cargo test -p anima-execution-store-sqlite --test capacity` | Passed, including broad core run | Last retained image 16,708,208/16,777,216 bytes, 241 runs, 32,768-byte inputs. Eight near-limit admissions in one debug measurement: p50 2,074.994 ms, p95/max 2,900.802 ms. Rejection preserves exact prior generation/blob and retained runs. |
| Harness library + core execution-limits focused regressions | 5 and 30 passed | Tool absence recovery preserves never-sent proof; physical completion waits for actual accounting receipt through Stop; failed accounting cannot publish model success. |

## Host evidence

Both hosts inject the same facade and SQLite adapters through explicit opt-in
constructors. Default executable profiles retain their legacy runner. Fixtures
use actual authenticated daemon HTTP or village HTTP, temporary databases and
scripted models. Neither host nests `AgentRuntime` inside durable execution.

- Daemon bridge: 18 focused tests passed, including the shared basics,
  protected versioned admission, definition/model upgrades with stable identity,
  owner/store attachment refusal, read-only clock capability, semantic/live SSE,
  global capacity and Stop, interrupted HTTP custody, explicit memory outages,
  transaction/ack failure and bounded backlog catch-up. Actual child termination
  covers accepted-source projection and a committed clock result. HTTP lifecycle
  tests cover correction/CAS, exact source replay, forgetting active and inactive
  history, directed relationship evidence, private/foreign-owner refusal, real
  acknowledgement failure followed by reopen without double revision, and a
  competing trusted writer's explicit rejected outcome without poisoned readiness.
  Registration overflow is rejected transactionally and preserves healthy reopen.
- Village focused engine checks: 18 passed, one child entry ignored in discovery
  and explicitly invoked by its parent. They cover the shared basics, distinct
  owners/worlds/private observers, stable stage bindings, winning game receipts,
  losing-writer fencing, reviewed-winner observation/supper admission, corrections
  and forgetting, padding normalization, host-only clock capability, actor/scope
  mismatch and file proposals rejected before executor entry. Process kills cover
  engine-result/game-commit, source/outbox application and the bounded exchange.
  Full game checks passed with 38 browser/tool, 18 domain and 46 host tests,
  build/typecheck and lint, including the final dependency initialization fixes.

The bounded authored supper interaction persists participant/stage/source IDs,
at most two replies and a shared finite allowance. Its public utterances are
frozen authored facts; model output must satisfy the existing response contract,
but no live generated-dialogue quality is claimed. Each observer receives sourced
episode/relationship evidence. Server-owned presence expiry, closed-session
refusal and restart clock behavior prevent offline catch-up.

## Milestone interpretation

| Milestone | Evidence available | Gate retained |
| --- | --- | --- |
| A: execution foundation | Public disk-store conformance, atomicity, real process death, exact result recovery, clock rollback and measured capacity | Luna's reserved compatibility baseline patch still needs origin integration. |
| B: persistent agents | Required-memory facade, stable identities, canonical lifecycle and directed evidence, source/outbox receipts, private recall and store pins | Copied import manifests are covered; automatic legacy extraction and reversible product migration are not certified. |
| C: two consumers | Both real HTTP boundaries use the same facade/adapters; shared basic checks plus specialized host controls, capability, privacy and recovery tests | The common exported suite is basic. Broader scenarios have host-specific fixtures; existing daemon transcript/UI and approval adapters remain legacy. |
| D: bounded ecosystem | Authored two-reply Tavi/Mira exchange, separate private experiences, sourced relationships, presence expiry, restart/reconciliation and unchanged quest authority | Executable activation and a live provider/browser/language walkthrough remain gated. No production cutover was performed. |

## Remaining cutover gates

Both hosts passed the shared basics through their real HTTP boundary plus their
specialized authority, outbox, capability, process-recovery and domain fixtures.
These certify the explicit fixture profiles. Existing product clients, legacy
data migration and live acceptance retain the separate gates below.

The snapshot adapter is bounded and rewrites its full sealed image. The measured
near-limit cost above must inform a later workload envelope; it is not a
production latency target or multiplayer scaling evidence. Budget and clock metadata assume trusted local
administration; backup restoration needs explicit recovery policy.

Copied memory import manifests are transactional and replay-safe, but automatic
extraction of every legacy daemon/entity/temporal record and reversible migration
are not established by those tests. No real memories or saves were imported.

The copied plan/spec remain byte-identical to their source. Luna's reserved
`optional_environment_contract.rs` and compatibility review are absent from this
worktree and were not recreated or modified. Their separate patch still needs
integration by the originating task; this report does not certify that baseline
gate from the other tests.

## Profiles retained on the existing runtime

Existing assistant coding, MCP, connector, helper, schedule, swarm and other
uncertified profiles remain on `AgentRuntime`. Already admitted legacy records
retain their existing reader. The old harness remains available. No automatic
fallback is permitted after a durable effect begins.

## External and live acceptance

No live provider calls, human-language review, browser player walkthrough,
multiplayer server/load test or native engine binding is certified here.

Automatic approval review rejected village executable startup wiring that would
allow opt-in real provider calls because of possible game-data egress under the
current restrictions. The injectable service/HTTP bridge and isolated fixture
paths were validated. Executable startup wiring remains blocked and must not be enabled
as a workaround.
