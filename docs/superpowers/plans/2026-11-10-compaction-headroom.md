# Compaction Headroom Implementation Plan

**Goal:** Stop repeated automatic compaction and finish its visible status promptly.

**Architecture:** Keep normal context selection and its budget unchanged. When history overflows or cold history needs a summary, expand the summary span to retain a smaller recent tail. Preserve at least the newest complete selected exchange. Keep ordinary dropped-message accounting separate from expanded `compaction_input`, and retain the fallback runtime until the summary is saved successfully.

Use the existing durable compaction error for a five-minute automatic retry cooldown; manual compaction bypasses it. Require an automatic summary to reclaim at least 10% of the replaced context's estimated tokens. Measure canonical source context before hydration so source-index overhead cannot distort savings. Clear the compaction phase before main model work, unless cancelled.

**Reference:** Behavioral reference from public [Hermes Agent](https://github.com/NousResearch/hermes-agent) source: `agent/context_compressor.py`, `agent/context_compressor_tail.py`, and `hermes_cli/config_defaults.py`. Retain Anima's source-bound checkpoints, history recovery, cancellation, persistence, and usage accounting. No Hermes runtime or private configuration is imported.

**Tail policy:** 20% of the available history budget, floored at 10k and capped at 25k tokens, never above half the available budget. The latest whole selected exchange may exceed that target. Explicit context budgets and the incoming user message retain their existing behavior.

- [x] Add regressions for headroom across follow-up messages, complete exchange/tool boundaries, cooldown expiry/manual retry, ineffective summaries, and phase completion on success/failure.
- [x] Add bounded tail selection and separate compaction candidates in the daemon's existing session/context code.
- [x] Preserve canonical image attachments for checkpoint source hashes; cover more than four images and cold history newer than an older hot turn.
- [x] Review the final implementation and run focused validation using the repository wrappers and shared validation cache, within the 32 GiB ceiling.

**Validation:**

- 62 compaction tests and the separate original dropped-accounting regression passed (63 focused Rust checks).
- `bun run rust:lint` passed.
- 118 UI/event tests passed; web build, typecheck, and lint passed.
- `bun run rust:test` passed supervisor/core suites, then reported 3,981 passed, 16 failed, and 48 ignored in the daemon unit suite before final corrections. The two compaction/accounting failures were corrected and verified by the focused checks above. Owner-private temporary storage resolved four environment failures; serial execution resolved two drain checks. Eight unchanged Windows command-runtime tests still independently reproduce `Shell process directory is unavailable`. The full suite is not green.
- `bun run --filter rust-daemon build` passed. A local restart smoke check verified backend health, console HTTP 200, proxy health, and preservation of saved agent/session identities.
