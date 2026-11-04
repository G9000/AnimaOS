# Runtime persistence admission fix

**Goal:** Stop tool dispatch when the reusable runtime cannot establish that a persisted step is safe to execute.

**Design:** Keep the existing `DatabaseAdapter` boundary. Make tool preparation fallible: only a missing record permits fresh execution, while readable terminal results remain reusable. Pending records, lookup errors and unreadable terminal records fail the run. Resolve the entire batch before writing pending records, and acknowledge all pending writes before dispatch. A preparation failure records protocol error results for the whole batch without overwriting earlier evidence or executing callbacks. This conservative policy requires host reconciliation of unknown attempts; it does not promise exactly-once execution or add concurrent dispatch claims.

**Files:**
- `packages/core-rust/crates/anima-core/src/runtime.rs`: fallible preparation, pending admission and failed-batch recording.
- `packages/core-rust/crates/anima-core/tests/runtime_persistence.rs`: public-boundary fault injection and batch assertions.
- `packages/core-rust/crates/anima-core/README.md`: adapter failure and reconciliation contract.

**Validation and tasks:**
- [x] Add regressions for lookup failure, unreadable terminal output, pending prior attempt and pending-write failure; observe failures before the fix.
- [x] Implement fail-closed preparation and preserve well-formed transcripts and Stop precedence.
- [x] Run focused persistence and existing runtime success/failure tests; the final full run includes all five persistence regressions and exact call-ID assertions.
- [x] Review the change, run formatting/lint checks and `bun run rust:test`.

No storage migration, daemon restart, live-provider calls or publishing is required.

## Validation

- Four batch regressions failed before the fix because both callbacks executed. After the fix all four passed, including missing/malformed output for both terminal statuses. The added Stop regression also passed.
- Independent code review found no actionable issues; follow-up coverage asserts exact call/result pairing and Stop during failed lookup.
- `bun run rust:test`: passed, **5,804 passed / 0 failed / 63 ignored**, across 113 result summaries in the final tree. Windows setup used normal permissions, `RUST_TEST_THREADS=1`, and a fresh owner/SYSTEM-private short temporary root supplied through `ANIMAOS_TASK_TEMP_ROOT`. This avoids the observed restricted-permission failures, parallel supervisor port collision, and ambient-temp privacy failures without changing unrelated code or existing directory permissions.
- `bun run --filter core-rust lint`: passed.
- Wrapped `cargo clippy -p anima-core --all-targets -- -D warnings` using `--rust-cache validation`: passed.
- `git diff --check`: passed.
- Full and focused environment verification logs are retained under `test-output/runtime-persistence-2026-11-04/`.
