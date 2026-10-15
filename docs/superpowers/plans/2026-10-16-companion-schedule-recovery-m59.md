# M59 persisted schedule-claim recovery

## Scope

Add one finite synthetic acceptance case for PAR-10: load a persisted unresolved
schedule claim into a fresh daemon state and prove startup reconciliation refuses
replay, disables the schedule for owner review, and durably records the local
interrupted outcome. Preserve the next interval due time. This validates existing
behavior; it does not introduce another recovery policy or claim full PAR-10.

M58/PR64 is merged at119ab161902ca840eb57003c6545d1cb21bfe6ce; its tested tree is
5d48f05cda6eb8238c84618f13093b9648fb9f21. Local gates: Rust2855/26 intentional
ignores/45 suites/lint/build, SDK183/three service skips, web1468/120files/type/build,
actual200-route inventory, docs and formatting. All protected hashes preserved.

## Lean plan and reuse evidence

The source-only planner inspected current PAR-10/PAR-12 and existing scheduler/UI.
Automations already has run-now, pause/resume, edit, history and delete. Existing
late-claim timing tests cover skipped interval steps. The restart case in
schedules.rs uses the original shared state without a store; shutdown tests check
settlement in the same state. The missing evidence is unresolved-claim disk load
into a newly constructed daemon and a second fresh reload after reconciliation.

Reuse fixture(), due_schedule(), GatedModel, claim_due, JSON control-plane snapshot
load/restore, SchedulerService and existing safe outcome types. No new runtime,
queue, API, UI, dependency or third-party port. This acceptance work checks Anima's
existing Rust recovery contract; copying another scheduler adds no useful behavior.
Retain existing Hermes notices unchanged. The public installed source pin remains
1fd18277267152893a8a24792b79213e7ca3f899; no profile/credential data is imported.

## Interfaces and ownership

Only source path released after plan publication:
hosts/rust-daemon/src/schedules/shutdown_tests.rs. Reuse sibling test helpers via
super::tests and actual load_control_plane_snapshot / restore_control_plane_snapshot.
No production helper or extra test seam is authorized without a concrete failure.
Root owns all commands, formatting, Git, docs, fixtures and decision ledger.

Read-only references: schedules.rs, schedules/timing.rs, control_plane_store.rs,
state.rs, routes/tests/automations.rs, SDK automations and existing owner UI.
All M58 source, fixtures, Cargo.lock/manifests and unrelated nx stay exact.

## Finite oracle

1. Create one synthetic JSON-backed schedule using the existing test fixture.
   Record its identity and declared due T, a60,000ms interval.
2. Persist exactly one claim using claim_due at T, without starting a model job.
   Load the actual JSON and prove it contains that unresolved claim: last-fired T,
   enabled, no safe outcome, next due T+60,000. Do not quietly save a settled case.
3. Stop/quiesce the original service without settling the synthetic claim. Restore
   a newly constructed daemon with a fake model from those saved bytes. Use the
   production restore and scheduler startup paths, preserving the original owner.
4. Observe bounded startup reconciliation. Expect disabled, local Failed with
   schedule_run_interrupted, same occurrence/next-due identity, no model calls,
   no messages/effects or duplicate occurrence. Explicit future tick must not run it.
5. Load the JSON again; assert the reconciled record matches memory. Restore a
   second new daemon from this settled snapshot and prove no second execution or
   changed receipt. Reconciliation is durable rather than an in-memory-only pass.
6. Always settle owned test tasks and remove only the exact temporary fixture.
   No cadence sleeps, real service/account/model/provider or unlimited loops.

If a step conflicts with actual type semantics, report exact evidence to root;
do not change the oracle or broaden policy silently. Existing correct production
may pass immediately: this is acceptance coverage, not an invented RED claim.

## Test list and commands

- One new fresh-state disk-roundtrip claim/reconciliation test, exact independent
  timestamp/outcome/identity/zero-call assertions and second reload.
- Existing shutdown/claim/storage-recovery/cadence cases stay unchanged.
- Root first formats touched Rust, then focused new case. One native batch review,
  one bounded fix wave if needed, then required test/lint/build gates.
- SDK/web behavior is unchanged: retain the fresh M58 results and exact hashes.
  No redundant web review/test wave unless a relevant source/input changes.

Use CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=2 CI=1,
ANIMA_SKIP_SERVICE_TESTS=1 and the existing provider/service-env-clearing runner.
NX_DAEMON=false NX_ISOLATE_PLUGINS=false NX_NO_CLOUD=true; serialized Cargo.
Focused known forwarding: bun --no-env-file x nx run rust-daemon:test --skipNxCache
--excludeTaskDependencies --args="--lib persisted_unfinished_claim -- --test-threads=1".
Required: bun --no-env-file x nx run rust-daemon:test --skipNxCache;
rust-daemon:lint --skipNxCache; rust-daemon:build --skipNxCache. Use documented
validation target directory only if an observed Windows executable lock requires it.
Local gates are verification, without an Nx Cloud success claim.

## Controller rulings on the risks (binding)

1. Ruling: preserve interrupted-claim disable-and-review semantics — a durable
   local claim cannot prove an external effect did not happen — cost if wrong:
   owner must review before resuming; never advertise remote exactly-once delivery.
2. Ruling: assert unresolved bytes at the first disk boundary and use two genuinely
   new daemon states — same-state restart tests already exist — cost if wrong:
   acceptance is rejected as duplicate or in-memory-only evidence.
3. Ruling: one claim, zero permitted model calls, bounded reconciliation and explicit
   ticks only — recovery evidence needs neither inference nor wall-clock cadence
   — cost if wrong: fail the case and drain its owned tasks; no real provider call.
4. Ruling: preserve exact owner, schedule/occurrence identity, due timestamp and
   local outcome through JSON reload — equal-looking new fixtures can hide replay
   — cost if wrong: strict independent assertions fail; no silent regrade or rerun
   of consumed actual M47/M52 cases.
5. Ruling: release only the existing test module, review native changes once and
   preserve all production/SDK/web inputs by hash — this closes an evidence gap,
   not a scheduler rewrite — cost if wrong: publication blocks until a concrete
   failing contract justifies a narrow controller-bound correction.

## Publication and resume

One branch g9000/companion-schedule-recovery-m59 from verified M58 main. Root
records rulings/results in the private M59 progress ledger, updates master status,
parity/harness evidence and handover, then explicit-path unsigned commit/push/PR,
attach and standing authorized merge with exact tree/hash proof. Never stage
nx.json/anima.yaml/.superpowers. No stash/reset/restore/path checkout/worktree.
No ordinary profiles/credentials, actual models/accounts/services/devices,
database servers, containers/Docker55432 or deployment. M52 remains PARTIAL.

## Gated outcome (2026-10-16)

One finite synthetic acceptance case now crosses the unresolved-claim JSON disk
boundary into a freshly constructed daemon, then reloads a second new daemon
after reconciliation. The first snapshot contains exactly one enabled unresolved
claim at declared T and next due T + 60,000 ms. Startup preserves its identity,
disables it for review and records local Failed / schedule_run_interrupted.
An explicit later tick does not run it; the saved reconciled record matches memory
and survives the second fresh restore without another execution or changed receipt.

The fake-model call count stays zero. No chat message, duplicate occurrence or
external effect is produced. This reuses the existing scheduler, claim, JSON
load/restore and safe-outcome code. Production, SDK/web, APIs, owner controls,
dependencies and all 30 frozen M58 source hashes remain unchanged. The useful
new evidence is fresh-state disk recovery; existing same-state restart and late
interval advancement tests remain distinct.

One native batch review is settled. Fresh Rust/core **2,856 tests / 26
intentional ignores / 45 suites / format / build** pass. The current unchanged
SDK/web inputs retain M58's 183 SDK tests / three service skips and 1,468 web tests /
120 files / typecheck / build evidence; no redundant browser/provider run is claimed.
Owned docs, formatting and preservation checks complete publication.

This is local claim recovery, without a remote exactly-once guarantee. Broader
recurrence, daily/cron/DST/clock changes, long outages, real target-host recovery
and checked scheduled task effects remain open under PAR-10/PAR-11. No actual
model/provider/account/service/device/database server/container/deployment is
activated. M52 strict quality remains PARTIAL and consumed cases are unchanged.

See [harness evidence](../../companion/harness-acceptance.md).
