# M79: Respect active hours after an outage — Implementation Plan

> M78 is merged and its exact source tree verified. Execute the two-file fix with tests first, one daemon batch review, one combined fix wave and the required local gates. No extra preflight audit.

**Goal:** An overdue automatic occurrence must wait for permitted active hours before it creates a claim or starts work.

**Architecture:** Add the missing current-time admission check to `claim_due`, under its existing control-plane transaction and state lock. Reuse the stored active-hours parser and `ActiveWindow::contains`; preserve next-due planning and the separate manual claim path.

**Tech stack:** Existing Rust daemon scheduler, chrono/chrono-tz and controlled-clock fake-model fixtures. No new dependencies.

## Confirmed defect and scope

The editor promises “Only run between.” With active hours 08:00–22:00, an occurrence due at 21:30 is nevertheless claimed by the first automatic tick at 23:00 after an outage. `claim_due` currently checks enabled/due status and calculates the next permitted due time, but does not check whether the current automatic claim is inside active hours.

This is a PAR-10 product fix. It does not establish target-host uptime, external exactly-once effects, arbitrary recurrence correctness, or a new actual acceptance case.

Owned public files:

- `hosts/rust-daemon/src/schedules.rs`: automatic claim admission only.
- `hosts/rust-daemon/src/schedules/run_now_tests.rs`: controlled-clock scheduler regressions and manual-path preservation.

Use this existing test module unless a concrete fixture limitation makes a different existing module necessary; root records that placement ruling. No web, SDK, schema, grant, persistence-format, dependency or public timing API changes.

## Reuse and verified interfaces

- `active_window(Option<&ActiveHours>) -> Result<Option<timing::ActiveWindow>, ScheduleError>` already exists in `schedules.rs`.
- `ActiveWindow::parse(&ActiveHours)` and `pub(crate) fn contains(&self, at_ms: u64) -> bool` already implement timezone, weekday, overnight and exclusive closing-boundary semantics in `schedules/timing.rs`.
- `next_due_after_claim` remains the existing cadence/skip-missed planner. Do not replace it with a new scheduler or wall-clock calculation.
- `run_now_inner` calls `claim_manual`, which records a manual occurrence without consulting active hours and preserves the due time/enabled switch. Keep that explicit owner action unchanged.
- Reuse the scheduler's `tick_at` and gated/fake-model fixtures. Existing timing tests already provide UTC date construction and New York DST examples; construct declared instants without sleeping or changing the machine clock.

Direct reuse of Anima's planner is the narrow option. The pinned public Hermes comparison remains `1fd18277267152893a8a24792b79213e7ca3f899`; no upstream scheduler or budget code is needed for this local admission omission. No new upstream copy, profile access or dependency is proposed.

## Rules and implementation seam

1. Check the latest selected record in `claim_due`, after enabled/due selection and before mutating any claim fields or preparing persistence. Use the passed tick timestamp `now`; do not introduce a fresh system-clock read into the controlled-clock path.
2. For valid configured active hours, if `contains(now)` is false, return `Ok(None)`. Leave the entire schedule record unchanged, including next due, last-fired/outcome, timestamps/version projection and counters. Create no run, history entry, persistence write or automation-updated event for this skipped record.
3. Do not advance an overdue record merely because a tick is outside the window. At the next permitted tick it can be claimed once using the existing planner, which skips missed intervals. Preserve existing unresolved-claim recovery and concurrency guards.
4. A skipped record must not consume an active-job slot or prevent a different eligible schedule in the same tick from running.
5. No active-hours configuration means existing behavior. Manual Run now continues through `claim_manual`, including outside active hours; its original explicit action semantics and due time remain unchanged.
6. Preserve existing malformed-stored-window and timing-error handling. In particular, do not add an early parser `?` that changes an invalid stored window from the existing disable-and-continue path into a failed whole tick that starves peers.
7. This check controls automatic claim admission at the supplied tick instant. It does not cancel already-claimed work when a window closes, add a second check after room/permit waiting, or claim that every effect finishes inside the window.

The only subtle guard is the error-path preservation: a successfully parsed configured window can reject the current claim; parsing failures must continue into the existing `next_due_after_claim` rejection handling. A small local guard may reuse the parser twice rather than refactor the planner. Keep this proportional to the two-file fix.

## Tests-first work

- [ ] Add a deferred/outage regression to the existing scheduler test module: an enabled unclaimed occurrence due at 21:30, active 08:00–22:00 UTC, tick at 23:00. Assert zero claims, fake-model calls, runs and history entries, and exact before/after record equality. Root records meaningful RED from the present unwanted claim.
- [ ] Tick again outside hours: still zero work and unchanged record. Tick at the next 08:00 opening: exactly one claim and settled fake-model invocation, one history outcome, expected next due/counters. Repeat that timestamp: zero additional claim. Pre-release fixture gates so positive ticks can settle; never await an entered-model notification in a zero-work case.
- [ ] Test the exact exclusive end boundary (22:00 refused) and inclusive opening (08:00 allowed). Cover interval, daily and cron overdue records with the same automatic admission rule, using small table-driven fixtures where practical.
- [ ] Test an overnight Friday 22:00–Saturday 06:00 window: Saturday 05:59 allowed, 06:00 refused; Saturday night remains outside the Friday-only window. Test a weekday window across a weekend with the next Monday opening.
- [ ] Reuse declared UTC instants for a New York DST transition: verify one inside-window and one outside-window automatic claim across the offset change using the existing wall-clock rules. No new DST policy or exhaustive timezone suite.
- [ ] In one tick, place the outside-window overdue record before an eligible peer and assert only the peer runs; the blocked record stays byte-for-byte equal in its stored fields. Retain/add the malformed-window peer test so parser rejection cannot starve it.
- [ ] Test explicit manual Run now outside active hours: one manual occurrence, unchanged due/enabled fields and existing outcome semantics. Keep no-window, disabled and unresolved-claim cases unchanged.
- [ ] Implement the narrow guard, run focused GREEN, freeze the two source files and request one final daemon batch review. Apply one combined fix wave, then final gates.

## Root-owned gates and publication

Root chooses supported focused-test arguments from the current Nx configuration/help before running. Use the existing Rust validation target directory/isolation settings; preserve the exact RED/GREEN outputs. Final required gates:

- `bun x nx run rust-daemon:test --skipNxCache`
- `bun x nx run rust-daemon:lint --skipNxCache`
- `bun x nx run rust-daemon:build --skipNxCache`
- Rust formatting check through the established host/workspace command; scope changes to the two owned files and inspect any formatter changes.

No web/SDK gate, actual browser, provider/account/device call, daemon service, container, machine-clock change, real-time wait or new acceptance supervisor is required. Root owns all commands, formatting, Git, published plan/status updates and private evidence. Report deterministic native behavior only.

## Top five risks for root controller rulings

1. Advancing or marking an outside-window occurrence as claimed loses the intended deferred run: return before any mutation/persistence and assert whole-record equality. Cost if wrong: a missed occurrence can disappear or acquire fabricated history.
2. Checking only the initial due-list snapshot races an owner update: enforce the window on the latest record inside the existing transactional claim. Cost if wrong: work can start under obsolete active hours.
3. Accidentally applying the automatic restriction to Run now changes explicit owner authority: leave `claim_manual` untouched and test it outside hours. Cost if wrong: the existing manual action stops working.
4. New parser/error exits can fail the whole tick or starve peers: preserve existing malformed-window disable/continue behavior and no-slot consumption. Cost if wrong: one record can block unrelated work.
5. Invented DST/cadence semantics can duplicate or skip work: reuse `contains(now)` and the existing next-due planner, with controlled boundary/overnight/weekend/DST tests. Cost if wrong: active-hour enforcement changes recurrence beyond this fix.

Binding root rulings follow; no further preflight pass.

## Controller rulings on the risks (binding)

1. Ruling: defer an automatic occurrence with no record or persistence mutation when the current tick is outside valid active hours — an occurrence remains available at the next opening instead of acquiring false claim/history — cost if wrong: overdue work may disappear or execute at the wrong time. Assert full record equality and zero runs/history/fake-model calls.
2. Ruling: evaluate the latest record inside the existing transactional claim — the earlier due-list snapshot cannot authorize work after an owner update — cost if wrong: changed active hours could be ignored. Reuse the passed tick time and current lock ordering.
3. Ruling: preserve explicit manual Run now through its existing claim_manual path — the owner action is already a deliberate override and keeps due/enabled state — cost if wrong: manual execution changes unexpectedly. No manual-path source change.
4. Ruling: preserve malformed stored-window disable/continue handling and peer admission — a parse error must not become an early whole-tick failure, and skipped records consume no slot — cost if wrong: one record can starve unrelated jobs. Test the eligible peer and invalid restored record.
5. Ruling: reuse existing timezone/window and skip-missed cadence semantics at automatic claim admission — no new DST policy, scheduler, cancellation rule or post-permit check — cost if wrong: broader recurrence or in-flight behavior can change. Already-claimed work keeps its normal lifecycle; this milestone proves admission at the tick instant, not that every later effect finishes within active hours.

Use bounded representative boundary tables for interval/daily/cron, overnight,
weekday/weekend and one DST transition. Pre-release fake model permits and await
owned jobs before assertions so a failing RED cannot strand a fake task. No
actual service, model provider, account, device, database or container activation.
