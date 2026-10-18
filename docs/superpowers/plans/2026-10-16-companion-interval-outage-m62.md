# M62 finite interval recovery after an outage

Status: locally accepted test-only milestone; publication and merge settlement are recorded in the private progress ledger.

## Gap and smallest useful case

Recurrence/outage evidence is incomplete: controlled clocks must define missed-occurrence behavior and avoid catch-up storms; long outage, before/after durable claim, interrupted shutdown/restart, cancellation/disable and uncertain effects remain listed.

Prove one synthetic workspace-target interval case crossing the JSON control-plane boundary after a long outage. The durable snapshot contains an enabled interval with no prior `last_fired` and a due time in the past relative to the first controlled tick. A new `SchedulerService` is constructed from that snapshot. One explicit `tick_at(N)` must collapse the overdue backlog into exactly one execution at N, durably advance next due to N+I, and never replay missed occurrences. Repeated observation at N must not execute again. A fresh restore must remain quiet until another explicit test tick; exactly one later tick at N+I demonstrates ordinary recurrence resumes without catch-up.

This is narrower than daily/cron/DST and tests orchestration + durable behavior beyond existing arithmetic-only late timing coverage. It makes no external delivery or exactly-once claim.

## Existing evidence and interfaces

- `hosts/rust-daemon/src/schedules/timing.rs`: `next_fire_after_claim` advances interval schedules by `floor((now - previous_due) / interval) + 1`, skipping missed interval steps. `next_fire_after_claim_skips_late_interval_steps` (near line 656) already covers a 70-minute-late active-window case and cron advancement. Arithmetic is covered; durable recovery after an unclaimed interval outage is not.
- `hosts/rust-daemon/src/schedules.rs`: `SchedulerService::tick_at(now)` injects a controlled `u64` timestamp and drains jobs. `tick_inner` reconciles, gathers due enabled schedules, sorts IDs, claims through `claim_due`, and `claim_due` calls `next_due_after_claim` before saving. Existing interval execution near lines 1900-1959 tests two normal same-instance ticks at 2 and 1,002 ms with a 1,000 ms interval; this does not cover outage or fresh restore.
- `hosts/rust-daemon/src/schedules/shutdown_tests.rs`: `fixture()` creates a fake `GatedModel`, isolated `ControlPlaneStoreConfig::Json` under a UUID-named temp file, and returns fresh service/state/agent plus entered/release semaphores. `persisted_unfinished_claim_recovers_in_fresh_states_without_replay` supplies a local `restore(ControlPlaneSnapshot, store, entered, release)` constructor, cleanup via `settle_owned`, serialization comparisons and zero-call assertions. Its accepted scenario persists a claim already advanced by one interval, then startup disables that unresolved claim as `Failed/schedule_run_interrupted`; it deliberately prevents replay. It does not cover an enabled schedule with an old, unclaimed due time. The nested M59 restore/cleanup helpers must be narrowly extracted or repeated for sibling access; no production seam is needed.
- `GatedModel::generate` increments `entered` and blocks on `release`, returning `CHECKIN_SENTINEL` as a test-only response. This gives an exact model-dispatch count and deterministic settlement without any real provider/model/account. Use only a workspace target; do not create a connector.

## Declared finite timeline and assertions

Use `I = 60_000 ms`, set `T` once from the seeded durable record's exact `next_due_at_ms`, then set `N = T + 5I` and `N + I` using checked arithmetic. The test's frozen snapshot records the exact numeric `T`; all later clock movement is only through explicit `tick_at` arguments.

1. Create one synthetic enabled `ScheduleTrigger::Interval { interval_ms: I }`, workspace target, stable synthetic owner, due `T`, no `last_fired` or prior outcome. Save JSON and shut down the initial fixture without starting its worker.
2. Construct fresh state C1 from the saved JSON. No worker is started; no call is admitted before the controlled tick. This models five elapsed interval periods as outage, with no background wall-clock timer involved.
3. Spawn `C1.tick_at(N)`, observe exactly one `GatedModel.entered` permit, release exactly one response, and join. Expect return count 1, one occurrence ID `schedule:<id>:<N>`, `last_fired.fired_at_ms = N`, `next_due_at_ms = N + I`, one successful/silent local outcome and counters.runs = 1. Persisted snapshot and memory state must agree.
4. `C1.tick_at(N)` again returns 0, with no additional entered permit, unchanged fire count 1 and no second record/message. This verifies no same-time catch-up loop.
5. Stop C1 quietly, construct distinct C2 from its saved JSON without starting the worker. Before explicit tick, assert zero additional calls, same next due, and exact schedule/fire snapshot equality.
6. `C2.tick_at(N)` returns 0. Then `C2.tick_at(N + I)` returns 1; observe/release exactly one additional fake call. Expect fire count 2, latest `fired_at_ms = N + I`, next due `N + 2I`, and total fake calls across C1+C2 exactly 2. Save/restore once more only if needed for an exact final disk equality assertion; do not add more recurrence cycles.

Call ceiling: exactly 2 fake-model entries / releases total across the two permitted occurrences; zero connector/outbound effects, zero provider or account calls. The four explicit ticks return `[1, 0, 0, 1]`. No `SchedulerService::start()` or real cadence sleep is used; M59 already separately proves startup reconciliation and no replay of a persisted unresolved claim.

## Ownership, test scope and commands

- Own only a new sibling test in `hosts/rust-daemon/src/schedules/shutdown_tests.rs`, plus any tiny helper extraction needed inside that test module. Do not change scheduler production code unless the new case exposes a concrete mismatch with the declared skip-missed policy; if behavior differs, report evidence first and bind any fix separately.
- Use UUID temp JSON path with exact-file RAII cleanup, finite outer timeout, bounded fake response release, and unconditional service/job settlement on assertion failure, following M59's `FixtureFile`, `settle_owned`, and `catch_unwind` pattern. Keep fixtures synthetic and workspace-only.
- Focused: `bun x nx run rust-daemon:test --skipNxCache --excludeTaskDependencies --args=--lib persisted_interval_outage_collapses_missed_occurrences_without_catch_up -- --test-threads=1`.
- Then run the full required native gate `bun x nx run rust-daemon:test --skipNxCache`; formatting/lint/build only if source/tree or repository gate requires them. Serialize Cargo execution with `CARGO_INCREMENTAL=0`; do not start ordinary daemon, database, container, real connector, provider/model/account or target-host service.

## Top five risks to rule on

1. “At most one per recovery tick” vs “skip every old occurrence without executing” is a semantic policy decision. The proposed existing code's arithmetic advances after one late claim; confirm that late single execution is the desired local behavior.
2. The scheduler's default worker uses wall time. Keep this test on explicit `tick_at` only so controlled timestamps remain deterministic; M59 owns real startup-worker recovery evidence.
3. The fake model may block cleanup if assertions fail between admission and release. Always release its bounded gate and settle every owned scheduler in cleanup.
4. Keep the interval due values internally consistent with creation/update timestamps and the JSON snapshot invariants, or restore validation may reject a malformed fixture.
5. One synthetic workspace run proves scheduler policy and local persistence only. It cannot prove actual model quality, connector delivery, external exactly-once effects, long-duration uptime, daily/cron/DST semantics, or in-flight graceful shutdown.

## Controller rulings on the risks (binding)

1. Ruling: one late execution collapses the overdue unclaimed backlog and advances to N+I — this is the existing skip-missed interval policy, not a new scheduler behavior — cost if wrong: the case fails and a concrete production gap must be separately bound; no catch-up storm or external exactly-once claim.
2. Ruling: use only explicit controlled ticks [1,0,0,1], with no scheduler worker start or wall-clock cadence — M59 already covers startup reconciliation of an unresolved claim — cost if wrong: this case proves only injected-clock recovery, not unattended uptime or cron/DST.
3. Ruling: retain both fresh-state fake-model counters outside the acceptance future and assert exactly two calls after every owned task/service/job joins — pre-cleanup assertions alone miss late dispatch — cost if wrong: focused acceptance fails; no real model or connector activation is permitted.
4. Ruling: freeze T from the input snapshot before execution, use checked N=T+5I and independent N+I/N+2I expectations, exact occurrence IDs/counters and disk-state equality — expectations must not call the timing algorithm under test — cost if wrong: malformed fixture or incorrect persistence fails rather than being regraded.
5. Ruling: one native test-module path, source-only Sol implementation, one native concurrency batch review/fix wave, focused plus full required Rust gates — existing behavior may be immediately GREEN because this adds missing acceptance — cost if wrong: no completion claim, no redundant SDK/web gates for unchanged source, and no ordinary service activation.

## Execution and reuse boundary

Own only hosts/rust-daemon/src/schedules/shutdown_tests.rs; preserve existing M59
assertions. Reuse its JSON restore, exact-file cleanup and bounded ownership
patterns; extract a tiny test-only helper only if needed. No production changes
without a demonstrated failing contract. Reuse our existing scheduler rather than
porting another scheduler; no new upstream copy, license notice or dependency.
M61/PR67 is settled: source2448040f3bc60b778494059ae914163cf267e9e3,
merge61af5dca33799c53e191736bf1f2676800068c9e, exact treeda3f6a2d0ff1b0e39b160d4d7350b5fc22c314ae.
Its selective actual memory acceptance/18 offline gates remain scoped evidence;
all four failed fresh cases stay immutable. M47/M52 cases are never replayed;
M52 PARTIAL and broader provider/semantic/host/cron/DST parity remain open.

Root alone executes commands, formatting, Git and gates. Every Cargo invocation
uses CARGO_INCREMENTAL=0/jobs1/threads2 and runs serially. Use Nx focused then full
rust-daemon:test, rust-daemon:lint and rust-daemon:build with skipNxCache. Keep provider
and service configuration scrubbed by name. No real daemon/model/account/device,
ordinary profile/credential, database server/container/Docker55432 or deployment.
Update master/parity/harness/handover, explicit-path unsigned commit/push/PR/attach,
standing authorized merge and exact tree/raw-hash proof. Never stage nx.json,
anima.yaml or .superpowers; no stash/reset/restore/path checkout/worktree.
Resume .superpowers/sdd/2026-10-16-companion-interval-outage-m62/progress.md.

## Gated outcome (2026-10-16)

One finite synthetic workspace interval case now crosses fresh JSON restores
after an unclaimed outage. The enabled schedule is due at frozen T; explicit
time N=T+5I uses I=60,000 ms. A newly constructed state runs exactly once at N,
skips the missed backlog and advances to N+I. A repeated N tick stays quiet.
After quiet settlement and a second fresh restore, N stays quiet and N+I runs
once, advancing to N+2I. The four controlled tick counts are **[1,0,0,1]**.

Exact occurrence IDs, local silent outcomes, run/failure counters and JSON/state
equality pass. Both fake-model counters survive the acceptance future; exactly
two calls are checked after every owned tick, scheduler and job has settled.
No connector/outbound effect is admitted. The existing M59 recovery test retains
all assertions through a narrow shared test-helper extraction.

One native concurrency batch review is settled. Fresh Rust/core **2,857 tests /
26 intentional ignores / 45 suites / format / build** pass. Only the
existing native test module and evidence docs change; production, SDK/web, APIs,
dependencies and recovery policy remain unchanged. M60 SDK183/three service skips
and web1491/121-file/type/build, and M61 selective actual memory HTTP acceptance
plus 18 offline browser passes, retain revision-specific evidence for unchanged
inputs. No redundant browser/provider run is claimed.

This proves controlled-clock local interval recovery, without a worker start or
wall-clock cadence. Daily/cron/time-zone/DST/clock changes, target-host uptime,
actual scheduled task effects and external exactly-once delivery remain open.
No real model/provider/account/service/device/database server/container/deployment
is activated. M47/M52 consumed cases and all four failed M61 fresh cases remain
immutable; M52 strict quality remains PARTIAL.

See [harness evidence](../../companion/harness-acceptance.md).
