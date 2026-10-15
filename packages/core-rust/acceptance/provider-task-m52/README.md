# M52 JSONL useful-task acceptance fixture

This dependency-free deterministic fixture follows the bound
[M52 plan](../../../../docs/superpowers/plans/2026-10-16-companion-provider-task-acceptance-m52.md).
Its presence grants no task, metadata, generation, service or shell execution
release. The controller records validation and any later provider evidence
separately. M47 files and targets remain unchanged.

`fixture/` is the synthetic workspace template. Only
`fixture/src/log-summary.mjs` is repairable; it deliberately skips error events
and replaces accumulated durations. `out.json` is the sole task-generated artifact.
The CLI, visible tests, data, README and sentinel are protected bytes/write times.
`reference/`, `expected-cases.json`, `oracle.mjs`, `oracle.test.mjs`,
`module-probe.mjs`, witnesses and `neighbor/` stay outside candidate authority.
Directory separation is not OS isolation. Root must bind actual tool confinement,
review edited source for side effects, and own any execution/process cleanup.

The specification and primary output preserve the bound plan's JSONL contract.
Expected strings in `expected-cases.json` are literal authority;
the independently written reference is only a positive control. No candidate
implementation computes expected answers. The self-tests propose baseline FAIL,
reference PASS and six mutant failures: skip-errors, overwrite-total, zero-errors,
unsorted rows, hardcoded primary and prevalidation output truncation.

Root-owned interfaces, for later exact release only:

- `captureProtected(candidateRoot, neighborRoot)` returns explicit protected
  paths with SHA-256, byte length and `mtimeNs`. Freeze the returned object outside
  candidate authority before task admission, with its own file hash.
- `verifyCandidate(candidateRoot, witnesses?)` checks module/CLI behavior and
  protected witnesses. Without supplied witnesses, protection covers only oracle
  execution; this cannot establish preservation across the actual task.
- Future CLI: `node oracle.mjs <absolute-candidate-root> [<frozen-witnesses.json>]`.
  Exit 0 means semantic/protection checks passed, 1 verdict failure, 2 runner misuse.
- Self-tests: `core-rust:provider-task-acceptance-test` runs
  Node's `--test` on this slice's `oracle.test.mjs` from the repository root.

The oracle uses installed Node directly, no shell/install/dependency, a two-second
deadline per child and 16-KiB output capture. Module probes are subprocesses too.
These limits do not establish whole-tree OS containment; reuse M47's owned process
proof and root review before executing arbitrary edited code. No provider is used.

The bound semantics use JS `String.trim()` per line, including BOM, and standard
`JSON.parse` duplicate-key last-value-wins semantics. Numeric 1.0 is an integer.
The 16 KiB cap counts raw UTF-8 bytes including BOM before trimming. The CLI checks
raw byte length before output writes and uses fatal UTF-8 decoding with
`ignoreBOM:true`; the module probe preserves BOM too, matching direct module calls.
These are fixture preparations, not model-authored repairs or test results.

Added literal valid cases, each output ending in one LF:

- BOM + `bom/ok/1` => `{"services":[{"service":"bom","count":1,"errors":0,"durationMs":1}]}\n`.
- Duplicate service `ignored` then `dup`, status `error` then `ok`, duration 99 then
  3 => `{"services":[{"service":"dup","count":1,"errors":0,"durationMs":3}]}\n`.
- `constructor/error/2` and `toString/ok/1` =>
  `{"services":[{"service":"constructor","count":1,"errors":1,"durationMs":2},{"service":"toString","count":1,"errors":0,"durationMs":1}]}\n`.
- BOM + padded `bom/ok/1` has literal inputs at 16,383 and 16,384 raw bytes with
  the same BOM-case output; 16,385 bytes must fail module and CLI. The oracle checks
  unchanged existing output bytes and modification time for that invalid case.

Source inspection verifies the CLI raw-length guard precedes decoding, module
invocation and output write. Manifest byte lengths are checked as file preparation;
actual CLI rejection remains unexecuted and requires root's later exact release.

Literal output, invalid diagnostics, limits and all source/oracle/witness hashes
must be reviewed/frozen before any task. No success or test pass is claimed.
