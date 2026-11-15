# M.O.T.H.E.R additive provider validation

Scope: A1 binding and conversion, A2 agent-bound recall/native-memory retention,
A3/A4 durable selected sharing and approval reconciliation, and the A5
configuration/status UI. A7 cross-harness approval acceptance is recorded below.
Pairing-app default changes (G1) and temporal graph capabilities (A6) remain
separate work. Installed native pairing maintenance is recorded below.

## Implemented behavior

- Strict versioned `memoryProvider` setting, independent personality source and
  agent-bound reviewed MCP reader. Legacy bindings retain their previous behavior.
- Owner revision-checked conversion, omitted-versus-null compatibility, durable
  settings rollback, explicit preview and no automatic conversion on upgrade.
- Fresh bounded full-record recall through the MCP authority checks. Workspace,
  generation, role, subject, connection grants and running task remain constrained.
- Native memory tools, task capture and learning stay enabled when the garden is
  unavailable. Helpers do not inherit the garden binding or remote tools.
- Ordinary turns and local memory writes cannot enter the legacy automatic upload
  path for additive agents. A mutation captures that policy before awaiting save,
  so changing the binding during persistence cannot expose earlier local content.
- Personality can remain local or use an explicitly allowed workspace persona.
  Timeout cancels only the garden request, not the native conversation.
- Explicit `memory_share` selections commit to a bounded private SQLite journal
  before acknowledgement. Ordinary memory writes never enter this queue.
- A profile-owned background worker rechecks the selected connection, binding,
  reviewed tools and current persistent permissions. `Ask`/`Deny` block delivery;
  session approvals and fabricated run identities cannot authorize it.
- Interrupted sends reconcile immutable operation receipts before retrying the
  same UUID/request. Known proposals are polled instead of resubmitted. Only
  validated canonical receipts create saved mappings, including owner edits.
- Owner API/SDK/UI distinguish queued, uncertain, awaiting review and saved;
  controls use revision checks and cannot approve Mother proposals.

## First-slice validation record (before durable publication)

All fixture content is synthetic. No installed agent, live key, owner memory or
model-provider account was changed. UI acceptance here uses component tests and
production compilation; installed-host/visual acceptance is not claimed.

Initial route regressions failed before implementation on absent provider metadata
and rejected conversion fields. Binding unit tests then passed. The initial
runtime regression failed before implementation on missing additive fallback.
Integrated runtime testing exposed a fixture without a native-memory tool grant;
the fixture now explicitly grants those tools. Review added timeout isolation and
binding-change-before-persistence regression coverage.

Restricted Windows runs returned path-not-found errors in existing atomic-save
fixtures as well as new tests. Validation uses the same managed commands outside
that restriction, retaining owned temporary directories and the 32 GiB cache cap.

Historical first-slice validation on Windows, 2026-11-14. These counts do not
establish validation of the subsequent queue implementation:

| Check | Outcome |
| --- | --- |
| `bun run build` | Passed, including web production compilation. |
| `bun run typecheck` | Passed after the landing build generated its Astro declarations. |
| `bun run lint` | Passed with existing warnings. |
| `bun run rust:build` | Passed on the final Rust source. |
| `bun run rust:lint` | Passed on the final Rust source, with existing warnings. |
| SDK suite, including synthetic daemon fixtures | 357 passed in 58 files. |
| Web suite | 2,646 passed, one skipped in 217 files. |
| Final managed daemon `--lib memory_provider` filter | 31 passed. |
| Final managed daemon `--lib workspace_context` filter | 24 passed, one existing real-M.O.T.H.E.R integration test ignored. |
| Final managed `cargo test -p anima --test document_editing_acceptance` | One passed on isolated rerun. |
| `bun run rust:test` | Core and supervisor suites passed; daemon library: 4,064 passed, eight failed, 49 ignored. See isolation results below. |
| `bun run test` | SDK, web, CLI and TUI passed; stopped at the packaged Office approval test described below. |

The eight daemon failures were outside the changed modules: three physical
document-directory tests and one Python-runtime quarantine test rejected inherited
Windows temporary-directory permissions; all passed in owner-private disposable
directories. Three drain-counter tests and one document-confirmation test passed
when rerun separately. Production security checks were not relaxed. The broad
suite remains a failed run; isolated passes do not make it an all-green result.

The final workspace run stopped at
`packaged_selected_office_edits_require_owner_and_round_trip`: saving an approval
returned Windows access denied, then the decision endpoint returned 404. This
packaged test had passed in the earlier full Rust run and passed again in the
final isolated rerun. This is an intermittent Windows test limitation; the full
workspace command did not complete successfully.

Final focused Rust commands reused the managed validation cache, an owner-private
synthetic temporary root, and `RUST_TEST_THREADS=1`:

```powershell
bun tools/workspace-dev/scripts/run-rust-daemon-tests.ts --lib memory_provider
bun tools/workspace-dev/scripts/run-rust-daemon-tests.ts --lib workspace_context
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima --test document_editing_acceptance
```

The final regressions include local capture with an offline garden, complete
attributed records without duplicate bootstrap previews, persona independence,
binding changes during recall/persistence, cancellation isolation, helper
isolation, revision-checked conversion and legacy behavior. `git diff --check`
passed. Task-owned logs and temporary directories are removed after recording
these results; managed build caches remain subject to the repository budget.

## Durable selected-sharing slice (2026-11-15)

This slice adds the SQLite queue, receipt/proposal reconciliation, canonical
mappings, explicit native sharing/status tools and owner API/SDK/UI controls.
All records, grants and conversations used for validation were synthetic.

Initial store and receipt tests failed against their unimplemented boundaries;
SDK and UI regressions likewise failed before their implementations. Review
identified and fixed post-await authority checks, outcome-capacity reservation,
permanent rejection handling, storage-failure reporting and Mother-compatible
source fields. The local source attribution remains in the journal; the adapter
does not invent a network URL for a local memory.

The first full Rust run passed the core and packaged-host suites and 4,117 daemon
tests. Two inventory tests needed the four new routes and two new tool schemas
added to their expectations. Historical route-count assertions remain intact;
the new routes are excluded from those historical counts. The actual OpenAPI
and marked README inventory both contain 384 matching operations.

The real-Mother fixture uses an explicitly selected `mother.exe`, an owned
temporary database, a synthetic SOUL file and a disposable client grant. It
passed proposal submission with simulated lost acknowledgement, real receipt
reconciliation, pending review without a canonical mapping, direct canonical
saving and remote grant revocation. Its first attempt exposed a fixture setup
error, reconnecting an already-open connection; the fixture now connects once
and reviews its tools on that connection. No installed private client config,
real workspace records, model account or running owner agent was changed.

Owner-edited approval, dismissal, hidden approval, revision conflict, restart,
disk/admission bounds, stale source/control revisions, permission changes during
an awaited request, and shutdown during a send are covered by synthetic
transport and store tests. Real owner TUI approval and installed-host/UI
acceptance are not claimed.

| Check | Outcome |
| --- | --- |
| Managed `--lib publication` filter | 51 passed; real-Mother fixture separately selected below. |
| Real-Mother stdio publication fixture | One passed against the explicit installed executable with disposable data. |
| SDK suite | 359 passed in 59 files. |
| Web suite | 2,651 passed, one skipped in 218 files. |
| `bun run typecheck` | Passed. |
| `bun run rust:lint` | Formatting and Clippy passed, with warnings. |
| `bun run rust:build` | Passed. |
| `bun run build` | Passed, including packaged daemon and web output. |
| `bun run test` | Passed across the workspace, including the full core, packaged-host and daemon Rust suites. Daemon library: 4,119 passed, zero failed, 50 ignored. |
| `bun run lint` | Passed with warnings. |
| Final `git diff --check` and staged diff check | Passed. |

The final workspace run supersedes the two inventory failures above: both
inventory regressions passed after their expectations were updated. Rust checks
used the managed `target/cargo/validation` cache, the 32 GiB cache cap, an
owner-private disposable temporary root and serial Rust tests. Type checking
was repeated successfully after the final UI copy changes. Results are recorded
here before removing task-owned logs and scratch directories.

Implementation remains uncommitted in the `g9000/mother-additive` worktree.
It has not been installed or applied to the owner's running main-checkout app.

## Cross-harness approval acceptance (2026-11-15)

A new opt-in Mother integration test launches the built AnimaOS daemon with a
disposable private profile and communicates through its authenticated owner HTTP
API. AnimaOS uses the real Mother MCP transport, while a separate synthetic
harness retrieves and corrects canonical records. Owner review uses Mother's
existing `Store::review_proposal` transaction, the same boundary as its TUI.

The test passed (one test, zero failures). It verifies repeated admission keeps
the same operation ID, pending review survives graceful daemon restart without a
duplicate proposal, and pending content is absent from the other harness's
canonical context/search. Owner-edited approval at revision 1 remains the saved
mapping after another harness corrects the record to revision 2. A second,
targeted proposal approved at revision 3 still maps to revision 3 when Mother's
`currentRecord` has already advanced to revision 4. Native source memories remain
unchanged, and an unselected fact never enters the queue or garden.

Recovery uses the background worker's automatic reconnect, with no fixture
connect/resume after restart. Client-scoped receipts remain private: the second
harness receives `NOT_FOUND` for the first client's operation receipt. The
original client can retrieve its immutable submission receipt.

The fixture lives in the separate `g9000/mother-garden-acceptance` Mother worktree
at `tests/mcp/additive_anima.rs`; its command and validation details are in that
checkout's `docs/additive-animaos-acceptance.md`. No AnimaOS production source
changed in this acceptance slice. Mother's final locked suite passed 283 tests
(three optional tests ignored), and formatting, release build and strict Clippy
passed; the new ignored integration test was explicitly run separately. This is
synthetic two-executable acceptance,
not model-generated recall, abrupt crash injection, actual TUI interaction,
browser acceptance or installation into the running owner app. G1 pairing and
the temporal graph work remain separate.

## Installed native pairing repair (2026-11-16)

Owner-authorized maintenance integrated the existing additive worktree changes
into the active main checkout, preserving the intervening UI changes. The source
worktree, including its staged and unstaged state, was preserved. These integrated
changes remain local and uncommitted. The historical results above describe their
original validation and installation status; this section records the later repair.

| Check on the integrated checkout | Outcome |
| --- | --- |
| `bun run rust:test` | Passed across the full Rust workspace; daemon library: 4,122 passed, zero failed, 52 ignored. |
| Managed locked daemon build | Passed. |
| `bun run rust:lint` | Formatting and Clippy passed, with existing warnings. |
| SDK suite | 359 passed in 59 files. |
| Web suite | 2,654 passed, one skipped in 218 files. |
| SDK/web type checks and lint | Passed. |
| SDK/web production builds | Passed. |

Rust validation used the managed validation cache within the 32 GiB cap, serial
tests and a short owner-private temporary root. Four Windows ACL-sensitive tests
passed with that root, followed by a successful full suite. No production
permission checks were relaxed.

The qualified native daemon was installed outside the disposable build cache and
started against the existing profile. Its SHA-256 is
`24fd54b633de0142aae1737af49e5f4b5143b479c9b9ea4f76b6e13ae2f6e770`.
The two agents that already had Mother connections were repaired through the
authenticated AnimaOS owner API. Two obsolete entries pointing at historical
executables were archived and replaced within the four-connection limit. Existing
per-agent credentials and scopes were reused; unrelated connections were retained.

Both native connections reached `ready`, discovered 12 tools, and reviewed/granted
six read tools plus `mother_propose_memory`. Each saved binding has selected
publication, local personality and the current native workspace generation, with
no primary workspace binding. Fresh stdio clients using those same agent credentials
read current context and historical records within the adapter's 16 KiB envelope
budget. No model prompt or real proposal was submitted for verification.

Private backups preceded deployment. After repair, the native memory snapshot,
all 1,320 existing messages and nine attachments matched the backup. Original
agent settings, personality and native tool grants were retained, and unrelated
agent configurations matched exactly. A separate read-only Mother audit resolved
all 175 original records and all 290 historical snapshots without differences;
an intentionally updated installation checkpoint retained its original revision
in history. The archived SQLite source was unchanged. Backup paths, credentials
and private record content are intentionally excluded from this report.

This establishes installed connection and preservation acceptance. It does not
claim model-generated recall, actual owner TUI approval, browser visual acceptance
or completion of every additive-provider release gate.

## Limits

Automatic recall takes at most four full records, with an 8 KiB content budget
and 16 KiB per MCP envelope. Missing read grants, incompatible authority or failed
reads fall back to native memory. Pinned records are prioritized within this
bounded selection; this is not an exhaustive view of the garden. Canonical source
IDs and revisions are preserved. Publication mappings record confirmed receipts;
they do not overwrite native facts or automatically resolve disagreements with
owner-edited garden content. Queue publication currently accepts only own shared
native facts and local MCP connections, with bounded selected payloads. Other
native privacy scopes and paired-node publication remain unsupported.

Disabling automatic provider use does not revoke its key or remove separately
granted MCP tools. Existing legacy bindings require explicit conversion; native-only
Mother no longer ships the historical pairing application.
