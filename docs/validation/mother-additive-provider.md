# M.O.T.H.E.R additive provider validation

Scope: A1 binding and conversion, A2 agent-bound recall/native-memory retention,
A3/A4 durable selected sharing and approval reconciliation, and the A5
configuration/status UI. A7 cross-harness approval acceptance is recorded below.
Pairing-app default changes (G1) remain separate work. The first read-only A6
temporal-graph slice and installed native pairing maintenance are recorded below.

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

## Publication and restart qualification (2026-11-16)

The owner authorized publication and persistent startup after the pairing repair.
The reviewed 63-file implementation was committed as `019fb26a`, fast-forwarded
into `main` and pushed to `origin/main`. Remote readback confirmed that commit.
The original additive worktree was retained unchanged.

A packaged native supervisor was built with `bundled-ui,rust-embed/debug-embed`
using the managed validation cache. The static embedding feature makes this
development-profile executable carry its web assets instead of loading them from
the source directory. The locked build passed, followed by 134 packaged unit
tests passing, zero failing and one ignored. The installed executable's SHA-256 is
`1276d92094bfd94e3cc26ab73bcc185d5ff9677baadaac822cf8a624c0efe39b`.
Binary inspection confirmed the built web index was embedded; HTTP readback
matched the qualified index after installation and restart.

The executable was installed under the existing profile's versioned `bin`
directory, outside Cargo caches. Native `config` and `autostart` commands set the
existing local API address and enable Windows login startup. The launcher's
workspace was aligned with the already-saved active workspace. Its private data
paths, memory stores and owner tokens were preserved. The existing command-line
shim and Mother's separate native owner-service startup entry were retained.

Installation initially exposed the expected credential isolation between the
historical direct daemon and the packaged `installed` namespace. Explicit owner
maintenance copied only the active profile's existing ChatGPT login manifest and
five immutable chunks, plus its two referenced Discord credential slots. All eight
destinations were checked for conflicts before writes and verified by readback;
source entries were retained. Secrets were never printed or exported. The same
ChatGPT account and both ready Discord connectors were verified after restart.

The direct daemon was drained and gracefully shut down. The installed supervisor
was launched from the Windows system directory with the exact saved login
command, then fully quit and cold-started again. The final workspace alignment
was followed by another full cold start. The supervisor reported running, with
login startup enabled and the same private data directory. Reviewed Mother MCP
sessions were reconnected through the owner API, without changing grants, and
both additive bindings were reread: selected publication, local personality,
current native generation and no primary workspace binding.

After the final restart, byte-for-byte native snapshot rows, all 1,320 existing
messages and nine attachments matched the backups. Both publication tables
retained their empty predeployment state. Existing agent configuration, local
personality, native tool grants and unrelated agents were preserved. A read-only
Mother audit again resolved all 175 original records and all 290 historical
snapshots; intentional checkpoint updates retained their earlier revisions.

This is full application-process restart acceptance using the login command, not
an actual Windows reboot. No model prompt, real sharing proposal, owner approval
or browser visual test was used as a deployment check. Private backups and repair
evidence remain outside Git.

## A6 read-only graph slice (2026-11-16)

Record selection was merged and pushed first at `c9ddf98`; live `origin/main`
readback confirmed that commit. A6 source is on `g9000/mother-graph-read` and is
not installed in the live profile. The installed selection executable and all
existing agent bindings, native memory and explicit selected-sharing behavior
are preserved.

The adapter supplements its existing v1 bootstrap/recall only when capability,
query and Get descriptors were already reviewed on the selected connection and
the available v2 query contract is advertised. Capability availability does not
authorize graph reads; Mother checks independent `garden:read` on every query/Get.
No live grant or tool review is added by the adapter or this validation.

Queries and current Gets retain original typed graph records, valid/observed/
transaction times, attribution and optional extensions. Core schema/semantic
validation, consistent epoch/watermark checks and fresh subject reads reject
malformed or mixed snapshots. Query coverage and next-page status remain visible.
Typed graph locators distinguish graph references from v1 record IDs. Selection
stays within four complete items/8 KiB and the same three-second absolute limit.
There is no graph write, proposal, persistent cursor or graph cache in this slice.

Development evidence is retained privately: the first graph regression run had
three passes/four failures; the first implementation had 43 passes/one failure.
That failure demonstrated that a canceled entered MCP call retires its connection.
The adapter therefore withholds all shared results if final authority cannot be
validated, retaining native fallback and existing fresh-turn recovery. A review
finding on core wire validation reproduced as one pass/eight failures and then
passed all nine pure-module checks. An earlier compile-only failure is retained
separately and is not counted as a behavioral regression. Read-only final source
review found no remaining P1/P2 issues. All 55 integrated focused checks passed.
The real native executable acceptance also passed on a disposable synthetic
store: automatic typed neighbor recall, another harness's correction, retained
historical revision, propose-only scope denial and unchanged native fact.
It used no installed private client configuration or live database. The initial
native fixture was rejected because a Windows temp ancestor allowed replacement;
the same test passed under an owner-private root with ownership checks intact.
The full `bun run rust:test` retry, `bun run rust:lint` and locked bundled native
build (`bundled-ui,rust-embed/debug-embed`) passed at the same Rust source hashes
as the focused and native acceptance checks. Existing warnings remain.
The full retry recorded 6,001 passes, zero failures and 68 ignored tests across
115 groups. The native graph acceptance above was selected explicitly in its
separate run. Qualification retained a bundled executable privately; it did not
install A6, widen live graph grants or run a live model conversation for graph
recall. Graph write/proposal mapping and persistent change-feed caching remain
later slices. The completed plan and private qualification manifest record source
hashes, failed trials, preservation checks and completed owned-scratch cleanup.
The first full run ended with code 4 during an existing compaction test, without
an assertion failure, around a temporary tool-host disconnection. That exact
test passed separately on unchanged source. The interrupted log is retained;
its cause is unconfirmed and it is not counted as a passing full run.

Read-only preservation checks confirmed all 375 original native memories, five
entities, 1,363 messages and nine attachments unchanged. The existing profile has
378 memories and 1,408 messages at the first audit. The post-test audit had
382 memories and 1,422 messages, retaining every original entry. Metadata shows
four additional private runtime task results for the existing Mother agent;
their initiating activity was not inspected. This task initiated no live model
conversation. All four agent configurations, profile settings/credentials, selected
sharing queue/mappings and installed selection binary match their snapshots.
Mother MCP verified all 175 original records and 290 original historical snapshots
with no differences, plus successful reads through all seven existing clients.
Its native generation, installed binary and historical archive remain unchanged.

## Bounded reads and citation recall (17 November 2026)

Mother's native direct server and owner-service bridge now bound whole navigation
previews to 12 KiB of compact UTF-8 JSON and read replies to 48 KiB of serialized
MCP output. Required context and canonical records remain whole or return an
explicit size error. Category balance, inventory, search coverage and omission
counts survive byte trimming. The live existing-client `limit: 100` read produced
24,148 wire bytes, with 185 stored records and 140 recallable records reported
independently of its two returned previews. No database migration or grant change
was used. The qualified Mother artifact contains the bounded-read patch on its
native-only base; separate garden-explorer work remains preserved and excluded.

AnimaOS keeps native memory and local intelligence active. Personal queries use
bounded memory-category searches, whole-word and requested-species checks, and
retain explicit named-topic constraints. Citation acronyms such as `record ID`
cannot override an Owner query's subject; genuine technical topics remain intact.
Health/inventory requests avoid discovering unrelated private facts. Size failures
retain their cause and `outcome_unknown`, connection retirement and physical
cleanup; unknown mutations are never replayed automatically.

Windows qualification passed at frozen per-codebase source hashes: Mother fmt,
218 tests (five opt-in tests ignored), locked release build, strict Clippy and the
normal/build dependency audit excluding SQLite; AnimaOS full Rust tests with
6,019 passes, zero failures and 69 ignored tests across 115 groups, lint and the
locked bundled build. Three native interoperability cases were then selected
explicitly and passed. They include real native personal recall with citation
wording, a large synthetic persona and older facts behind substring distractors.
The separate 100-record native fixture verified both direct and service-bridge
wire bounds and whole-record-or-explicit-error reads. Existing lint warnings remain.

The first approved Discord exchange confirmed bounded search and native recall,
but exposed the citation-acronym bug. After its narrow fix was qualified, installed
and restart-verified, the owner explicitly approved one additional exchange. The
identical diagnostic then discovered and read the correct current Mother record,
cited its ID and revision, recalled the stored pet name, successfully used native
memory search and received a settled Discord delivery receipt. Neither the answer,
record ID nor exact Mother tool name was supplied in either prompt. Only discovery
and reads executed; no garden publication or proposal occurred. Private facts,
transcripts and record IDs remain outside this repository.

The final audit verified all 175 original Mother IDs, 290 archival historical
snapshots and seven unchanged clients. Its generation is unchanged. Nine later
excluded checkpoints cannot be enumerated through ordinary MCP; that navigation
coverage limit is recorded privately. All 382 original native memories, five
entities, 1,422 messages and nine attachments remain unchanged. The final native
store has 385 memories and 1,440 messages; new memory entries are private. Agent
configurations and selected-sharing tables remain unchanged. Actual supervisor
and daemon images, both additive pairings and the served bundled UI were verified
after clean supervisor restarts. This was not a full Windows reboot. Existing live
graph grants were preserved. Backups, failed trials, source hashes, binaries and
delivery/preservation receipts are retained privately; source remains unpublished.

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
