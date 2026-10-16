# M84: One model-driven research report acceptance plan

> For agentic workers: use `superpowers:executing-plans` under the controller's bound rulings. Source-only workers prepare the small acceptance slice; root owns validation, selection, releases and publication. No separate preflight audit or lifecycle framework.

**Goal:** Observe one actual subscription-model task search official Rust 2024 guidance, read selected pages, save a cited Markdown report, read it back before replying, and retain its evidence across one quiet same-store restart.

**Architecture:** Reuse M83 native search, existing fetch/file handlers, ordinary owner SDK admission/approvals, and M52 isolated-store/restart conventions. Add a small offline report/evidence oracle, not new production behavior. A scripted fixture validates the oracle and wiring; only the separately released actual model task establishes this case's quality outcome.

**Stack:** Existing Rust daemon/core, SDK, Node built-ins and existing console renderer. No dependency, account, provider, browser or helper addition.

## Scope and inherited evidence

- Root established branch `g9000/companion-research-acceptance-m84` from main `9e8caa3c0c2bc9bb868924cc52c66b4ac0df3a50`, tree `ce4b8c6f66bb270fbe48e12cd28796b2e64ecbb9`. Preserve intervening onboarding work and the two unrelated dirty Astro settings. M83 merged at `fee5e00c510bd9a89d779cb3153a3392c94e068d`.
- M83's successful scripted public endpoint case and earlier failed discovery case remain immutable. They prove endpoint/native/projection behavior, not model research quality.
- M52 remains PARTIAL: its parent marked verification complete while admitting the output contents were unconfirmed. Do not replay, regrade or repair that consumed case.
- One new candidate, one selected model, one owner task, one attempt. No helper, steering, follow-up generation, model trial, fallback, manual report repair or retry after failure.
- This draft authorizes preparation only. Actual metadata/selection and task release are separate root-owned operations after deterministic gates.

## Current source and reuse decision

| Source                                                                | Existing seam / consequence                                                                                                                                               |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tools/web_search.rs`, `mcp/transport.rs`                             | M83 fixed anonymous Exa search; retain current hosted-schema compatibility, bounded transport, cancellation and source projection unchanged.                              |
| `tools/web.rs:26-46,143-185,277-352`                                  | `web_fetch(url,max_length)` uses a 15s client timeout and 4MiB streamed-body cap; `max_length` limits decoded characters afterward. It is not a byte/download cap.        |
| `tools/filesystem.rs:19-66`, `tools/filesystem/search.rs`             | Existing `read_file(file_path,offset,limit)` returns numbered lines. Full read-back must account for that format rather than compare it directly with raw Markdown bytes. |
| `tools/filesystem.rs`, `tools/filesystem/edit.rs`                     | Existing workspace-scoped `write_file`; no new writer/export format.                                                                                                      |
| `approvals/policy.rs:189-221`                                         | Search supports Any matcher, fetch supports Domain, file writes support PathGlob. Use reviewed pending-call AllowOnce, not persistent broad rules.                        |
| `runtime.rs:491-525,615-705`                                          | Five tool rounds plus one tool-free wrap-up; evaluator revisions are an additional producer unless excluded.                                                              |
| `runtime_model.rs:325-337`                                            | Configured timeout bounds each model call; it does not impose a whole owner-task wall deadline.                                                                           |
| `sdk/src/chatgpt.ts:50-67`, `runs.ts:108-158`, `approvals.ts:163-178` | Cache-only catalogue read, explicit refresh, idempotent start, GET reconciliation, Stop, revisioned owner decisions.                                                      |
| `agent_runs/provider_task_tests.rs:169-220`                           | Isolated persisted workspace, explicit grants/settings, disabled autoTitle/autoCompact/profile files. Reuse conventions without importing old actual payloads.            |
| `agent_runs/web_search_tests.rs`; `ChatScreen.test.tsx:159-258`       | M83 real-result capture and exact captured-artifact console rendering. Reuse the artifact projection for this case, retaining its own case identity.                      |

- Direct reuse is cheapest: existing native tools, SDK and acceptance conventions already cover the workflow.
- Supported adapter integration is already supplied by native Exa MCP and the existing subscription model adapter. Do not introduce another transport or provider.
- A Hermes port is unnecessary. M83 already attributes public `plugins/web/keyless_mcp.py` behavior at `1fd18277267152893a8a24792b79213e7ca3f899`; retain that notice. No new upstream code/profile read or copied implementation is needed.

## One candidate and independent oracle

- Candidate: a fresh isolated workspace containing only a synthetic sentinel and no prewritten report, answers, oracle, historical conversation or personal context.
- Sole model-written artifact: `rust-2024-research.md`, UTF-8 Markdown, at most 8KiB and 100 lines. No source code, manifest or additional files may be written.
- Frozen search query: `site:doc.rust-lang.org/edition-guide Rust 2024 first stable release edition Cargo.toml cargo fix migration`.
- Search once with `num_results=3`. Fetch one or two distinct exact returned official Rust URLs selected through root's pending-call review. No result means truthful incomplete outcome, not a replacement search.
- Root independently verified these candidate oracle sources; freeze their official-source observations and hashes outside model authority before release:
  - `https://doc.rust-lang.org/edition-guide/rust-2024/index.html`: first stable release `1.85.0`.
  - `https://doc.rust-lang.org/edition-guide/editions/transitioning-an-existing-project-to-a-new-edition.html`: `cargo fix --edition`, then the manifest edition update to `edition = "2024"`.
- Expected facts are root oracle material, never inserted into the model task/system prompt. Root's `official-source-oracle.json` is authoritative and is not edited by workers. The two URLs above establish expected facts, not an exclusive URL-spelling test: a different actual returned official Rust page is eligible if root verifies its authority and relevant source evidence before AllowOnce. URLs used as report citations must have appeared in actual native search results and been successfully fetched in this run; root may ignore URL fragments for response-source equality only if that rule is frozen beforehand.
- The report has a short title, a three-row `Question | Answer | Source` table, and a short limitations paragraph. Row labels: `Minimum stable Rust`, `Manifest entry`, `Automatic migration command`.
- Validate the answer cell belonging to each exact row, including command/manifest tokens; an expected token elsewhere, in a hidden comment, unrelated URL or negated prose cannot satisfy it. Independently review substantive surrounding prose and qualifiers; no general semantic-truth classifier is claimed.
- Report automatic migration as a tool-assisted step with review/testing limitations, not a promise that every migration is fully automatic. Do not claim to run Cargo or modify a Rust project.
- Freeze one task body requesting those questions, exact query, tool limits, path/format, source-linked answers, mandatory read-back and an honest final response. Do not include the expected fact values or ready-made answer.

## Admission, tools and cost bounds

- Use one new isolated profile/store/workspace and one companion Chat session on one root-bound free loopback port. No ordinary daemon, profile, container, database server or connector is opened, stopped or modified.
- Reuse only the already-authorized app-owned subscription credential resolver. Never inspect/export/copy credentials or profiles, start login, change account, or substitute a keyed provider. If the established isolated resolver cannot be used, hold the actual task and report the missing boundary.
- Read catalogue cache via SDK. If needed, root may separately release one bounded refresh using the M52 revision/expiry rules; an uncertain refresh permits cache-only reconciliation, not a second POST.
- Root selects one literal current listed model ID and binds account-connection/catalog revisions, expiry, provider and config. No guessed default, cheapest-model trial sequence or fallback.
- Exact model tool grants: `web_search`, `web_fetch`, `write_file`, `read_file`. No todo, shell, browser, helper, delegate, messaging, owner MCP or keyed `exa_search` grants.
- Freeze/verify effective runtime descriptors, including automatic coordination additions. No peers/helpers or secondary producers are allowed; Delegate/Exec policy Deny closes admission even if an inherited descriptor appears.
- Settings: `maxToolIterations=5`, `maxRetries=0`, `maxTokens=2048`, `timeoutMs=30000`; autoTitle/autoCompact/profile injection disabled. Prove no evaluator revision, embedding, learning, summary, scout or other model producer.
- Structural ceiling: at most six primary inference attempts including the final wrap-up, no additional producer. Normal route/auth refresh is disclosed separately. Settings and reported token totals are not hard token/billing caps.
- Root wall deadline: 240s from task POST, including all approval waits. At expiry, Stop the known run once, reconcile, retire only owned processes and preserve uncertainty. No later model continuation is authorized.
- Network=Ask, Write=Ask; Read-class tools do not ask under the existing policy. No standing/session rules. Root examines the exact pending arguments/revision and issues AllowOnce only for the frozen operation sequence below.
  1. One `web_search` with the exact query/count.
  2. At most two `web_fetch` calls to distinct selected official returned URLs, each `max_length` in 1..20000. No arbitrary URL or follow-on crawl; redirects remain the existing handler behavior and are a disclosed limit, not a claimed strict host allowlist.
  3. One `write_file` to the exact report path; root checks path/size/authority but does not repair the report or supply answer feedback.
  4. One `read_file` for that path, `offset=0`, `limit=200`, after successful write. No overwrite after read-back and no parallel write/read race.
- A forbidden/extra request is denied and stops/fails the case. An uncertain approval decision is reconciled by GET. No global automatic approval loop or AllowAlways.
- Search remains bounded by M83's 30s/256KiB wire and four physical leases; fetch by existing 15s/4MiB raw limits. Root task shutdown covers the owned process if a request remains unresolved; cancellation is not rollback.
- Release one `runs.start` with frozen task bytes and unique idempotency key. If acknowledgement is uncertain, GET/list reconciliation only; never submit a second start.

## Evidence and verdict interface

- Private `case-manifest.json`: case ID, exact source/build/fixture/oracle/task hashes, isolated roots/port, model/revisions, settings/grants/policies, operation counts and deadlines; separate one-use metadata/task release records with expiry.
- Private `evidence.json`: version/case ID, actual run/session/agent IDs, ordered tool calls and statuses, approval IDs/revisions, bounded actual search/fetch outputs, report hash/bytes, read-back witness, final message, observed usage/counts and cleanup/restart outcomes.
- Preserve raw bounded artifacts and expose a redacted projection to the public oracle. Never log auth headers, credential material or unrelated state. Missing observations are explicit unknowns, not inferred successes.
- Reuse the M83 console artifact shape for actual search `urls`, exact owner `sessionMessages`, events and optional snapshot. Mark M84 and actual-model provenance; do not copy M83's scripted-answer/model-call/cleanup assertions.
- Oracle inputs are report bytes + root-frozen expectations + controller-collected evidence. The model-written report cannot supply tool success, chronology, provenance, call counts or verdict authority.
- Require successful search then successful official reads; cited table facts supported by the fetched text; successful write then full successful read-back before final assistant message; no later write; report bytes match the post-read frozen file.
- Final reply names the saved report, states what was actually verified, and has no unsupported claim of tests/compilation/migration. A correct report without read-back is PARTIAL, even if root independently reads it afterward.
- Separate verdicts: transport/provenance, factual report, workflow/read-back, console links, budget, persistence/no-replay and cleanup. PASS requires all; otherwise retain FAIL/PARTIAL with precise missing evidence. A completed run alone is insufficient.

## Quiet restart and immutable history

- After terminal state, wait for zero in-flight tools/model work and unresolved approvals. Capture canonical session messages/source projection, run/receipts/config/grants/usage, report bytes+mtime and sentinel witnesses.
- Restart only the owned isolated daemon once against the same store, without new task input or metadata refresh. Use existing verified process ownership; no new Windows guard/framework.
- Read the same session/run/report via ordinary owner interfaces and compare canonical data and exact report/source-link bytes. Reload the captured actual console history through the existing renderer test seam.
- Observe quiet state for a fixed 5s window after readiness; counters/records must show no new run, model/tool call, approval or write. Do not claim indefinite no-replay from finite observation.
- Retire/join owned daemon/launcher/descendants and prove the case port is free. Preserve unconfirmed cleanup as PARTIAL, never target a neighbor by name/PID alone.
- This is quiet reload/no-replay evidence, not crash-atomicity, in-flight effect recovery, durable semantic memory quality or general research/Hermes parity.

## Minimal ownership and deterministic preparation

- Preferred public slice: `packages/core-rust/acceptance/research-m84/README.md`, `packages/core-rust/acceptance/research-m84/oracle.mjs` and `packages/core-rust/acceptance/research-m84/oracle.test.mjs`. Keep literal positive/negative fixtures inline or in that directory; no independent project/dependency.
- Public oracle value: reproducible claim/provenance/chronology checks for this narrow task, with meaningful mutants, independent of private provider credentials and the controller's verdict text.
- Add only `research-acceptance-test` to existing `packages/core-rust/project.json`, matching current Node-oracle targets (cache false, parallelism false). Preserve M47/M52 targets.
- Root private ownership: minimal driver/release/oracle-manifest/evidence files in this M84 ledger. Reuse the public-source M52 controller owner catalogue/setup/submit/capture/quiet-restart conventions and existing OwnedJob, without copying its old PowerShell lifecycle or reading consumed case payloads. No worker builds a lifecycle framework.
- No new Rust test module by default: production handlers are unchanged and already gated. Add a focused native regression only for a demonstrated missing behavior/RED with root ruling.
- No SDK/web production change. Reuse current exact-artifact renderer acceptance; a tiny M84 test wrapper is optional only if its required artifact contract cannot be reused honestly.
- [ ] Write oracle controls first; establish semantic failures, then smallest oracle implementation.
- [ ] Root verifies one valid constructed fixture passes and the following independent mutations fail:
  1. Wrong minimum version, edition literal or migration command in its designated row, including correct-token decoys elsewhere.
  2. Fabricated/nonofficial/unreturned/unfetched citation, changed source identity or missing successful fetch evidence.
  3. Correct report but absent/partial/pre-write read-back; final-before-read or post-read mutation.
  4. Claimed success with failed/empty/truncated-insufficient external evidence, missing report or malformed/oversized input.
  5. Seventh model call, retry/secondary producer/helper, extra search/fetch/write, wrong path or rejected approval presented as success.
  6. Quiet restart changes report/messages/sources or records a new model/tool/run action; cleanup unknown must not become PASS.
- Positive controls include two fetched official sources, repeated citations to one source, numbered full read-back, correct same-store restore and an honest PARTIAL result.
- Any proposed production repair requires an actual deterministic RED first, one affected source batch review, one combined fix wave and fresh required gates; no prompt-only claim that model quality is fixed.

## Root commands and release sequence

These are future root-owned commands, not executed for this draft. Inspect targets before using a newly added target or forwarding focused arguments.

```text
bun --no-env-file x nx show project core-rust --json
bun --no-env-file x nx run core-rust:research-acceptance-test --skipNxCache
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
```

- Root determines relevant fresh gates from final scope: oracle-only changes need oracle tests/format/docs checks and exact unchanged production build provenance; any Rust change requires all three Rust targets. Never label reused M83 gates as fresh M84 tests.
- Serialize all Cargo-backed root commands with `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=2`; use the established isolated target directory if needed. No worker validation.
- Existing web artifact renderer replay uses exact retained M84 evidence and the established Nx test target; it performs no new provider call. Distinguish renderer replay from actual browser interaction.
- Sequence: bind risks/current baseline → public oracle tests/freeze + private driver checks → one combined affected review/fix wave → scope-required gates → root oracle/source freeze → metadata/selection release → one task release → independent verdict → one quiet restart → cleanup/seal → honest publication/merge.
- No extra mandatory audit. Do not reopen a consumed actual case to improve its score. If blocked before actual release, publish the blocker and retain the held state.

## Top five risks for controller binding rulings

1. **Actual model selection and six-call scope:** bind one fresh listed model and existing resolver; require zero secondary/evaluator producers. If unavailable, HELD, not a fallback model trial.
2. **Source-selection/oracle sufficiency:** freeze official facts, query/count and official-page selection rules before release; bind each actual selected returned URL at pending-call review. Equivalent official pages are eligible when evidence supports the frozen facts. Missing ranked sources/evidence yields PARTIAL, not a wider search.
3. **Read-back truth:** require complete read after sole write and before final, plus independent report correctness. This establishes the observed workflow, not internal model comprehension or universal claim verification.
4. **Authority/time limits:** bind exact grants/AllowOnce checks, report bounds, request/task deadlines and unknown-cleanup handling; current fetch redirects/raw limit and subscription settings are disclosed practical limits.
5. **Scope and history integrity:** choose the small public oracle/private driver option, preserve current onboarding/unrelated work and all M52/M83 consumed artifacts, and accept only quiet same-store recovery claims.

Lean alternatives: (A) recommended public offline oracle + private actual driver using existing tools; (B) add a narrow native regression only after meaningful RED; (C) if subscription/ownership/source admission is unavailable, retain held/partial evidence and stop this case. No new provider, runtime or general research framework.

## Controller rulings on the risks (binding)

1. Ruling: use one deliberately selected currently listed subscription model through the existing app-owned credential resolver, five tool rounds plus at most one final call, no retries/helpers/evaluator revisions or secondary generation — this tests the new search workflow without a provider trial ladder or raw credential access — cost if wrong: metadata/task remains held or its one actual attempt fails honestly. AutoTitle/autoCompact/profile injection and learning are disabled; check descriptors and current config before admission. The existing deterministic reflection-memory evaluator can retain its ordinary isolated task memories; it makes no extra generation and must not revise this answer. No login, credential copy/readout, fallback or normal-profile mutation. Model settings/accounting are not a hard billing cap.
2. Ruling: freeze the three independently checked official facts, one exact query and citation/evidence rules before any task — positive source transport alone cannot prove factual research — cost if wrong: insufficient ranked results yield an incomplete outcome. Root may AllowOnce at most two distinct actual-returned official Rust pages after checking their exact URLs and relevance; strip URL fragments only for source identity comparison, preserve full returned URLs elsewhere. Do not insert expected answer literals into model instructions or rewrite a candidate report.
3. Ruling: require one successful report write followed by a complete numbered native read-back before the final reply, and independently judge the actual answer cells and surrounding prose — M52's reporting gap must be checked on a distinct task rather than regraded — cost if wrong: correct file content can still be PARTIAL workflow evidence. Read-class tools never ask: their narrow grant and isolated workspace bound access, and observed call records establish chronology. Network/Write Ask use only exact pending-call AllowOnce; Exec/Delegate Deny. No fabricated Read policy or post-read edits.
4. Ruling: use one 240-second owner-task deadline with existing 30-second model/search and 15-second fetch limits, current bounded streams and owned-process shutdown — waiting, cancellation and uncertainty must retain evidence without unbounded background work — cost if wrong: the task or cleanup fails/partials and no continuation is attempted. Existing web_fetch may follow validated redirects and its 20,000-character output choice does not reduce the 4MiB raw cap; do not claim strict destination pinning or rollback.
5. Ruling: reuse existing native tools, typed owner SDK, public oracle conventions and Windows OwnedJob; add only a narrow reusable offline oracle/brief/tests and existing-project target — a new runtime or guard framework adds cost without proving this workflow — cost if wrong: a demonstrated production gap requires a focused regression and affected gates. Preserve current onboarding, unrelated files, all prior source/Git/runtime cases and the original M52 PARTIAL/M83 outcomes. Quiet reload plus five observed seconds proves only this case's finite no-replay result; no in-flight recovery, broad quality or full Hermes parity claim.

Ruling: retain the user's standing lean delegated workflow, unsigned per-command commits and merge-and-continue without new task-by-task check-ins — those choices were explicitly authorized earlier and remain in the handover — cost if wrong: exact progress/source/evidence remain inspectable in this ledger and PR. No worktree/stash/reset/restore/path checkout, new account/provider/device/ordinary-service/database/container or Docker55432 activity.

## Recorded outcome (2026-10-17 MYT)

M84 adds an offline cited-report oracle and77 positive/negative controls, plus an existing-project Nx target; no production runtime, SDK, web or dependency change. Fresh Rust2989passed/31ignored across46suites, lint/build, SDK/web typecheck/build and10 private-driver tests pass. One daemon/oracle batch review, final SDK/web-driver review and one reviewed fix wave are complete.

Actual research acceptance FAILED at the first approval. One gpt-6-luna subscription call reported1,417tokens; the private verifier required a matching full saved Assistant call before it would AllowOnce. Live tool/approval events preceded terminal-only saved message events, so the verifier denied and stopped the run. No remote search, fetch or report write ran; research quality, report read-back and research-source rendering were unassessed. Five quiet same-store seconds preserved the cancelled run/history/usage exactly; both owned daemons retired and18784 was free. This is cancellation/recovery evidence, not research-quality acceptance.

Two earlier private preflights are separately preserved: runs-list limit200 exceeded native maximum50 before any task POST, then an existing original log was refused before process launch. Their outcomes were not regraded; both used zero model/search calls. The runs limit was corrected before the sole model task. The decision-time current capture was later overwritten; exact original page contents are unavailable, while raw stream/source timing supports the diagnosis. Frozen source, exact binaries, raw receipts and original failures remain immutable. No retry, fallback, steering, report repair, account/login/profile change, normal-service/database/container activity or historical-case replay. M52 remains PARTIAL and M83 remains revision-specific.

Next: repair private admission to use actual typed live tool/approval events with exact run/session/step/call identity and full nontruncated pending arguments; retain immutable decision snapshots and validate native taskResult metadata against terminal full text for the oracle. Prove those contracts deterministically before a separately frozen, distinct bounded task. Do not replay M84 or infer full Hermes parity.
