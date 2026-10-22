# M53 owner-visible completion boundary implementation plan

Status: implemented and deterministic web acceptance passed on
`g9000/companion-completion-evidence-m53`, from verified M52/PR58 main
`97258b01ecfdffdfcd782c86b907f8b7d151261b`. Exact publication/merge proof and
resume: `.superpowers/sdd/2026-10-16-companion-completion-evidence-m53/progress.md`.

The existing committed message entry now displays **Tool limit reached** for a
strict accepted Assistant marker, without a loaded Run requirement. Live,
pending, revised, stopped, incomplete, non-assistant and tool-call content remain
excluded. Copy/read-aloud/memory-save text, existing history identity and tool/
helper navigation stay unchanged. Four existing web files changed; no native,
SDK, schema, dependency, configuration or runtime/model/service change.

The missing-notice semantic RED was one failed/one passed test. Focused final
chat/transcript/memo tests passed 110 tests in three files. One combined web correctness
and SDK/native scope review preceded the final fix-wave decision and fresh gates:
**1,361 web tests in 115 files**, typecheck/build, explicit formatting/diff and six
docs checks passed. The ledger records any review correction and exact commands.
These are deterministic presentation checks, not a real browser/model trial.
M52's strict quality remains **PARTIAL**, its task/releases immutable/consumed;
this notice does not repair the helper explanation or completed verification todo.
All prior test services remain retired. No M53 actual provider case was admitted.

The original proposal below remains design context; the final rulings are binding.

> Historical draft guidance, superseded by controller rulings below: For agentic workers: use superpowers:subagent-driven-development or executing-plans after root releases this scope. This private proposal grants no implementation, runtime or Git authority.

**Goal:** Show the owner when a committed reply used the accepted budget wrap-up, beside the existing tool evidence, without treating reply completion as task acceptance.
**Architecture:** Reuse the daemon's existing `budgetWrapup: true` message provenance and existing expandable tool evidence. Add one web message predicate and notice; retain the Rust runtime, todo store, provider behavior and owner authority.
**Stack:** Existing React/TypeScript web console, SDK Run DTO and Rust session-message contract; no dependency or project additions.
**State:** Proposal only. Root reports M52 merged at `97258b01ecfdffdfcd782c86b907f8b7d151261b`, tree `15192c476987505a982b66ab98a31ccfb3de0436`, twenty-two owned hashes preserved, services off and releases consumed. Root must create the next authorized branch and bind the five risks before publishing/releasing M53.

## 1. Recommendation and truthful starting point

- M52's useful JSONL repair passed the independent literal module/CLI oracle.
- One helper correctly identified both seeded defects; its edge explanation contradicted itself.
- The parent marked its output-verification todo complete while its reply said file contents were unconfirmed.
- Therefore M52 overall autonomous quality remains **PARTIAL**, not strict full-quality acceptance.
- Evidence: seven recorded calls, 23,065 reported tokens, fourteen durable native receipts and three completed todos.
- The independent output was 129 bytes; quiet restart retained seven calls before/after and four task-result/reflection records.
- M52 gates recorded 2,725 passes, 23 intentional ignores, forty suites, format/build success.
- Those are M52 evidence, not M53 gates or evidence of general model quality.
- The M52 task and metadata releases are consumed; preserve the actual case and its grade unchanged.
- No replay, repair, replacement prompt, extra diagnostic generation or new actual useful task belongs to M53.

The two observed quality misses do **not** establish a broken tool executor, lost result, faulty budget cap or corrupt todo store.
The helper explanation is a model-quality miss. The parent inconsistency is a model/task-reporting miss.
The current todo interface accepts model-declared statuses; neither Anima nor the inspected Hermes todo store verifies arbitrary task semantics.
Do not describe this milestone as making either observed answer correct or guaranteeing truthful future prose.

There is a distinct, fixable product gap: the owner session API exposes accepted budget provenance, but the current web transcript has no `budgetWrapup` consumer.
An owner reading the saved reply cannot see its execution boundary without inspecting API metadata.
M53 makes that existing fact visible and retains existing expandable tool evidence/navigation beside it.
This is a narrow completion-evidence presentation improvement, not a new semantic verifier.

## 2. Alternatives and reuse decision

1. **Stronger instructions only:** reject as the deliverable. `BUDGET_WRAPUP_GUIDANCE` already requires actual settled results and honest remaining work.
2. **Evidence-linked todos or a final claim checker:** defer. These need new schemas/authority and still cannot prove arbitrary prose or a successful task from a successful tool.
3. **Selected: expose existing accepted boundary:** deterministic, useful to the owner, small enough to verify without another model call or evidence surface.

Reuse existing Anima metadata, committed-message rendering and expandable tool cards first. Existing native receipt/status views remain unchanged.
Compare upstream public Hermes at the supplied pin `1fd18277267152893a8a24792b79213e7ca3f899`:

- `agent/turn_explainers.py`: pure exit-reason-to-visible-explanation pattern.
- `agent/turn_finalizer.py::_explain_abnormal_exit`: explanation follows an observed runtime reason.
- `tests/agent/test_turn_completion_explainer.py`: normal replies stay quiet; abnormal reasons map deterministically.
- `agent/chat_completion_helpers.py::handle_max_iterations`: summary boundary, already adapted by Anima M23.
- `tools/todo_tool.py` and `tests/tools/test_todo_tool.py`: revisioned model task declarations, not proof of completion.
- `agent/prompt_builder.py::TASK_COMPLETION_GUIDANCE`: confirms additional prose is guidance rather than an enforced semantic oracle.
- `LICENSE`: MIT, copyright 2025 Nous Research.

Direct Python reuse would import a foreign runtime for a small React projection; a process/API adapter would add services and calls without supplying stronger evidence.
Select a narrow behavioral port of the reason-to-notice pattern into existing Anima UI boundaries.
Do not port summary retries, synthetic owner messages, filesystem-stat heuristics, generic file-mutation verification or a global budget framework.
If implementation copies/adapts substantial upstream material, add the separately scoped apps/web/HERMES-NOTICE file with full MIT text, source pin/paths and local differences; preserve existing notices and Anima naming.
If only the general pattern is used, record that fact explicitly in the public plan's provenance section; do not pretend source was copied.
Planner public source reads succeeded; a read-only Git metadata query was denied, so this draft does not claim a newly verified checkout pin.
Root must use its established verified source provenance before attributing an exact copied snippet; this is no reason to run Hermes or inspect private data.

## 3. Current interfaces and ownership

| Existing source                                       | Observed contract / M53 use                                                                                      |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `packages/core-rust/crates/anima-core/src/runtime.rs` | Honest fixed wrap-up guidance already exists; retain it.                                                         |
| `runtime.rs:509–524,606–675`                          | One already-next call, empty tools, retries zero; provider marker stripped and accepted marker added by runtime. |
| `runtime.rs:1638`                                     | Actual tool results retain paired call ID and TaskResult metadata internally.                                    |
| `hosts/rust-daemon/src/routes/contracts/sessions.rs`  | Owner message metadata allowlist includes `budgetWrapup`; no DTO expansion needed.                               |
| `hosts/rust-daemon/src/routes/tests/sessions.rs`      | Existing real-handler scripted route test proves accepted provenance without extra actions.                      |
| `hosts/rust-daemon/src/tools/todo.rs`                 | Todo content/status/activeForm plus revision checks; completion remains a declaration.                           |
| `hosts/rust-daemon/src/agent_runs.rs`                 | Bounded helper config/result and host reporting guidance already exist; retain them.                             |
| `hosts/rust-daemon/src/runs/mod.rs`                   | `Completed` means a successful committed reply, not independent task acceptance.                                 |
| `packages/sdk/src/runs.ts`                            | Native receipts include status/durability/preview/linkage; no M53 schema or consumer expansion.                  |
| `apps/web/src/lib/transcript.ts`                      | Existing typed projection preserves committed messages and run reconciliation.                                   |
| `apps/web/src/components/ChatScreen.tsx`              | Transcript renderer has message/outcome/learning entries, no accepted-budget notice.                             |
| `apps/web/src/components/sessions/RunOutcomeCard.tsx` | Existing recorded-action labels distinguish reported success/failure/unknown.                                    |
| `apps/web/src/lib/daemon-api.ts`                      | `toChatMessage` preserves session-message metadata and role.                                                     |
| `apps/web/src/hooks/useLiveSession.ts`                | Committed message run IDs retire corresponding streamed step/tool cards.                                         |

Keep reusable runtime code in `packages/*`; this proposal needs no runtime move and no host-selection change.
No Rust, SDK, OpenAPI or todo schema change is expected. If a real required contract gap appears, root narrows/rebinds scope before adding one.

## 4. Proposed transcript interface

Keep the existing `TranscriptItem` shape. Render a small `BudgetWrapupNotice` beside the saved assistant bubble in `ChatScreen`'s existing `case 'message'`.
Keep its eligibility predicate pure in the existing transcript module or a small adjacent module.
Suggested contract: `isAcceptedBudgetWrapupMessage(message: ChatMessage): boolean`.
The notice receives no model prose, task list, native receipt payload or proposed completion grade.

Eligibility rules:

- Only a committed Assistant message from owner session history may create the notice.
- Require the metadata value to be boolean `true`; strings, numbers and truthy objects do not count.
- Exclude revised, incomplete or stopped content and assistant tool-call messages.
- The accepted runtime marker supplies the reason; do not infer exhaustion from step count, elapsed time, token usage, todo text or a model sentence.
- Place exactly one notice immediately after its final saved reply, inside that existing message entry and before later learning-review entries.
- Reuse the existing message key; history pagination and repeated snapshots must not duplicate or reorder replies.
- A live text delta or a terminal ledger row alone does not create the notice; wait for the committed marked message.
- The corresponding Run need not be loaded: the persisted accepted marker supplies the fact; do not fetch extra history.
- Do not attach/aggregate other runs' or helpers' results. Each session's saved marked reply may have its own local notice.
- Keep existing run failure, interruption, cancellation, helper and learning-review behavior intact.

This selector does not inspect the reply's claims or change its text, stored metadata, task statuses, accepted result or memory.
Do not require a todo list, a read-after-write pattern or a successful shell result to let the owner receive a reply.

Marker lifecycle to preserve/test:

1. Core removes provider-supplied `budgetWrapup` on every response; only accepted tool-free wrap-up content receives trusted boolean `true` before final assistant recording.
2. The normal coordinator persistence path and owner message allowlist retain that accepted marker; the existing M23 route regression proves this path.
3. SDK `SessionMessage.metadata` and `toChatMessage` preserve it; `mergeNewest` retains the current committed message page rather than creating local completion metadata.
4. Streamed step text has no accepted history marker. `RunActivity` remains unchanged; only the hydrated saved Assistant bubble can show this note.
5. `useLiveSession` retires streamed cards after committed message reconciliation. The notice stays keyed within that same message entry, so refresh/reconnect yields one note.
6. User/Tool rows may contain arbitrary metadata but fail the role predicate. Model prose, run counters and uncommitted stream contents never synthesize the marker.

## 5. Visible behavior and evidence limits

Render a compact `BudgetWrapupNotice` next to the saved reply, using existing session styling.
Suggested exact heading: **Tool limit reached**.
Suggested body: **The reply is saved. Check its results and remaining work.**
This labels the known execution boundary without declaring that the requested task passed or failed.

- Keep the existing expandable `ToolBlock`, helper-session links and native recorded-action views unchanged.
- If an existing directly usable evidence anchor is available, the notice may link to it using an observed DOM ID; do not invent navigation or add fetches to obtain one.
- No evidence anchor is required for the marker notice. Do not build a new receipt panel, action summary or report schema.
- No **Verified**, **Task accepted**, **All done** or inferred verification percentage appears.
- No automatic retry, Continue, model call, new approval, helper launch or budget extension is added.
- Keep the note outside model-authored reply text, copied answer content, read-aloud content and saved task-result memory.
- Do not append the notice into transcript storage or feed it back as owner instructions.
- Do not add polling/API requests; use the session messages and run data already loaded.
- Keep any existing navigation keyboard-operable; use static accessible text and avoid a repeated live-region announcement.

The M52 contradiction can still exist in a future model answer. This feature exposes an observed boundary and direct witnesses; it does not repair semantics.

## 6. File plan and bounded implementation

One web author owns the source slice; root alone executes commands, format, Git and runtime operations.
Use cheap Codex delegation under existing authorization. One combined review and one bounded fix wave; no repeated planning or check-ins between tasks.

- Modify `apps/web/src/lib/transcript.ts`: the narrow accepted-message predicate only, if that is its natural home.
- Modify `apps/web/src/lib/transcript.test.ts`: semantic projection/identity regressions.
- Keep the factual static notice inline in existing ChatScreen; the historical standalone-component suggestion is superseded by the binding four-file scope.
- Modify `apps/web/src/components/ChatScreen.tsx`: render it beside the existing committed assistant message.
- Extend `apps/web/src/components/ChatScreen.test.tsx` and/or `sessions/RunActivity.test.tsx` for the user-visible behavior.
- Add narrowly scoped styling to existing session styles only if existing classes are insufficient.
- Add the web Hermes notice only for actual substantial adaptation, as specified above.
- Root updates `docs/companion/daily-use.md` and the usual milestone ledger/handover to state the feature and its limits after gates.

- [ ] Author writes failing semantic web tests for missing notice/provenance lifecycle; root records RED once.
- [ ] Author implements the smallest projection and presentation; preserve reply and native records byte-for-byte.
- [ ] Root runs affected checks; one reviewer checks behavior, source boundaries, provenance and evidence wording together.
- [ ] Author addresses one consolidated fix wave; root runs final required gates on the resulting tree.
- [ ] Root publishes a bounded completion report and integrates only explicitly owned paths under standing authorization.

## 7. Deterministic test list

Projection and placement:

- Accepted committed marked reply creates one correctly anchored notice; ordinary successful replies create none.
- Missing/false/string/number/object marker, User/Tool role, revised/incomplete/stopped or tool-call content creates none.
- A streamed draft or completed run without its committed marked reply creates none.
- Message-first, run-first and repeated-snapshot arrival yield one stable notice without duplicate final bubbles.
- Older-page insertion and two independent marked replies retain chronological order and separate keys.
- Loaded marked history without a ledger run still shows the reason without creating a receipt section.
- Matching helper-session history is handled locally; the parent's note does not aggregate helper outcomes.

Presentation and non-effects:

- Heading/body describe tool exhaustion and saved reply; they do not assert task correctness or unfinished work as a fact.
- Existing tool disclosures, helper-session navigation and native action labels retain their behavior.
- Malicious-looking model text and user-supplied metadata cannot create a marker note through text matching.
- Any reused evidence link is keyboard-operable and resolves only to an existing observed target.
- Rerender and history hydration issue zero added network calls, approvals, helper starts or model requests.
- Existing reply text, copy/read-aloud behavior, todo declarations and learning-review eligibility remain unchanged.
- A fixture with a completed verification todo and contradictory final prose stays contradictory; no test relabels it a semantic pass.
- Existing M23 route/core tests remain the provenance authority; do not clone their entire matrix into React tests.

Use invented deterministic messages/receipt fixtures, not the private M52 transcript or live account data.
No new actual task, daemon/service launch, browser installation, provider catalog request or network acceptance is needed.

## 8. Future root verification commands

No command below has run for this held proposal. Resolve the current project targets before selecting their exact invocations.

```powershell
bun --no-env-file x nx show projects --json
bun --no-env-file x nx show project @animaos/web --json
bun --no-env-file x nx run @animaos/web:test --skipNxCache
bun --no-env-file x nx run @animaos/web:typecheck --skipNxCache
bun --no-env-file x nx run @animaos/web:build --skipNxCache
```

The test/typecheck/build lines are conditional on the resolved target names; use existing supported targets rather than adding targets to satisfy this draft.
Root confirms test-file forwarding before a focused RED run; do not guess Vitest/Nx flags.
Run the repository's existing formatting check on changed web/docs files and record its exact command/output.
No Rust/SDK production file is proposed; do not rerun native gates merely to inflate M53 evidence.
If root releases a necessary Rust change, run `bun --no-env-file x nx run rust-daemon:test --skipNxCache` and relevant lint/build on the current tree.
For a locked owned daemon executable use the documented isolated validation target directory with CI and `CARGO_INCREMENTAL=0`, jobs one, under root control.
SDK changes, if separately justified, require their resolved fresh test/typecheck/build gates.
Passing web fixtures prove presentation and identity matching, not hosted uptime, model quality or actual provider acceptance.

## 9. Top five proposed risk decisions — root must bind

1. **Semantic overclaim.** Proposed ruling: leave M52 PARTIAL and consumed; show observed boundary/witnesses only. Why: no receipt count or completed todo proves arbitrary acceptance. Cost if wrong: misleading quality claim; reject wording before merge.
2. **Provenance and identity.** Proposed ruling: strict accepted boolean marker on the persisted Assistant message only; preserve existing reconciliation and do not aggregate other results. Why: user metadata, live drafts and model prose cannot establish accepted provenance. Cost if wrong: lifecycle/identity tests block release.
3. **Scope and inference cost.** Proposed ruling: presentation only, existing loaded data, zero new generations/tasks/frameworks. Why: the semantic misses do not justify new paid retries or a generic verifier. Cost if wrong: remove the added producer/scope and keep M53 held.
4. **Persistence and observation.** Proposed ruling: read existing durable records; never edit reply/todo/memory to make the card agree. Why: a view must not rewrite historical evidence. Cost if wrong: retain original records and fail the non-effect/refresh tests.
5. **Reuse and integration.** Proposed ruling: narrow attributed exit-reason pattern with Anima metadata/React, full MIT notice for substantial port, one web author/review/fix wave. Why: Python/process reuse adds unrelated operational cost. Cost if wrong: revise the narrow integration before publication, no upstream parity claim.

Until root binds these decisions after the verified M52 merge/new branch, this remains a private held proposal.
Completion criterion: fresh relevant web gates plus deterministic owner-visible marker behavior, intact existing evidence navigation, honest documentation and no new model activity.
It does not close M52's strict semantic-quality gap or authorize another actual acceptance case.

## Controller rulings on the risks (binding)

1. Ruling: preserve M52's partial semantic grade and exhausted immutable case;
   expose only the accepted execution limit — neither a completed todo nor a
   settled tool result proves arbitrary task correctness — cost if wrong: a
   misleading acceptance claim; scope/wording tests must block publication.
   Exact owner text: **Tool limit reached** and **This saved reply could not run
   further tools. Review the tool results and any remaining work.** No Verified,
   failed-task classification, retry/Continue action, new model call or todo rewrite.
2. Ruling: a pure shape predicate is used only from existing committed
   `TranscriptEntry` message rendering — boolean `budgetWrapup === true` is native
   accepted provenance, whereas prose/live frames/user metadata cannot establish
   it — cost if wrong: false notices/duplicate replies; role/lifecycle/rendering
   tests block release. Require Assistant, object metadata and exact true; reject
   revised/stopped/incomplete true flags and tool-call content. Missing/null/empty
   tool-call arrays may be ordinary final content; malformed/nonempty tool calls
   are excluded. The predicate does not authenticate arbitrary browser objects;
   its committed-history caller supplies context. Pending, live, revised and
   non-assistant paths never render this notice, even with lookalike data.
   No Run fetch/link requirement: loaded older accepted replies remain useful.
3. Ruling: implement inline in the existing committed message entry, using
   existing styling and evidence navigation — a new receipt panel/anchor/extractor
   adds scope without strengthening this observation — cost if wrong: added
   authority/fetch/maintenance cost; remove it before gates. Source ownership is
   exactly `ChatScreen.tsx`, `ChatScreen.test.tsx`, `lib/transcript.ts` and
   `lib/transcript.test.ts` under existing web source. No new component, CSS,
   schema, SDK/native, dependency or configuration change is initially released.
   Keep note outside copied/read-aloud/saved model text, static/non-live accessible
   markup, and no tool/approval/budget/session state mutation or extra network.
4. Ruling: preserve existing history/memoization/reconciliation and persistent
   bytes — this is a view, not a repair of model history — cost if wrong: duplicates
   or rewritten evidence; stable identity/hydration/action tests must fail. Add
   meaningful missing-notice UI tests first for semantic RED against unchanged
   implementation; only then release the four-file implementation. Reuse current
   tool disclosures/helper links. Keep loaded-message order and render count
   protections; do not add a transcript item kind or aggregate other runs.
5. Ruling: use only the general Hermes exit-reason-to-notice pattern and Anima's
   existing metadata/React code, with one source author, combined web+SDK scope
   review, one fix wave and current resolved web gates — direct Python/process
   integration and repeated actual tasks add cost without proving semantic truth —
   cost if wrong: broader unverified product/attribution scope; hold release and
   rebind the smallest necessary change. Root reverified local HEAD
   `1fd18277267152893a8a24792b79213e7ca3f899` and selected public sources/LICENSE
   clean. No upstream implementation/text is copied here; no new notice file is
   required for that general pattern. Retain all existing MIT notices/Anima naming.
   If substantial upstream code is later adapted, record exact source/local
   changes and full MIT notice before publication. No M53 actual provider case.

### Verified targets and command ownership

Root resolved Nx web/sdk targets on this branch before source release. Web test
uses Vitest and depends on upstream builds; typecheck has upstream type checks,
build uses the existing Vite script. Existing `--args` forwarding is confirmed by
Nx executor help. Root must confirm Vitest's focused/run/name flags through its
help before the RED run. Use `bun --no-env-file`, CI/disabled real-service flags,
Nx daemon/plugins/cloud off and existing cache path. Full final web test,
typecheck and build are required with `--skipNxCache`; dependency tasks may run.
Explicit Prettier/diff/docs checks are required. SDK/native remain frozen; their
M52/M49 evidence stays historical unless actually rerun. Root alone executes
commands/fmt/Git, unsigned per command; stage explicit owned paths only, never
nx.json/anima.yaml/private evidence. No worktree/stash/reset/restore/path checkout,
ordinary profile/credential reads/copy, real connector/DB server/container/55432,
daemon/browser installation, model call or deployment is released.
