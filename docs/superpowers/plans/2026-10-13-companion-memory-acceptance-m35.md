# Companion Memory Acceptance M35 Plan

> **For agentic workers:** Use `superpowers:executing-plans` to follow this acceptance plan with checkpoints. Keep evidence and operator artifacts in the ignored M35 folder.

**Goal:** Validate one explicitly granted synthetic memory consolidation with the selected ChatGPT model, verify it survives one quiet same-store restart, then test learned Spanish automatic context without tool use.

**Architecture:** Reuse the retained synthetic M16 lead/workspace and existing local Rust daemon/owner API. Seed three new isolated synthetic Facts, temporarily grant only `memory_search` and `memory_consolidate`, run one bounded accepted task, restart the same store once without replay, and make one fresh-session Spanish context request using the configured local FastEmbed model cache.

**Tech Stack:** Existing Rust daemon and owner API, existing ChatGPT connection with `gpt-5.5`, configured local FastEmbed/E5 cache, PowerShell operator/evidence scripts. No source changes are planned.

---

## Scope and claim boundary

- Use only the retained synthetic profile and validation lead approved for isolated local testing in M16. Do not open ordinary profiles, inspect vault/keyring/environment secrets, export/copy/import credentials, or change account connections.
- Before startup, verify all connector/account collections are empty, no schedules are enabled, no work is pending, and port `8080` is free. If a guard fails, stop before starting or changing state.
- Use the existing internal ChatGPT connection and configured `gpt-5.5`; do not substitute models, perform credential operations, or run a diagnostic prompt.
- Reuse the cached real local FastEmbed model. Do not call a remote embedding provider or request a separate summarizer task.
- Limit the first accepted model task to 2 tool rounds, 20 seconds, zero retries, and at most 3 model steps. Limit the second fresh-session request to zero tool rounds, zero retries, and one model step. This is at most two logical requests and four configured model steps, not a hard provider billing cap.
- Do not resubmit, probe, repair model-created memory directly, or repeat a failed request. Preserve partial effects and report an incomplete result if the model or a guard does not behave as expected.
- A successful tool receipt means its handler reported success. It does not prove the whole goal, exactly-once execution, or faithful summary. Correlate the final canonical records, receipts, messages, and request history.
- Prior M34 native/web gates remain valid for unchanged source. Do not rerun Cargo/Nx gates while the daemon wrapper is active. If a concrete source defect appears, stop and ask root to bind a separate source-fix scope.

## Files and evidence

**Create or update only ignored acceptance artifacts:**

- `.superpowers/sdd/2026-10-13-companion-memory-acceptance-m35/acceptance.ps1` — guards, bounded settings/grant changes, service ownership capture, restore, and cleanup orchestration adapted from the retained M28 operator.
- `.superpowers/sdd/2026-10-13-companion-memory-acceptance-m35/start-daemon.ps1` — hidden isolated daemon launch with explicit synthetic stores, local embeddings and no environment-file loading.
- `.superpowers/sdd/2026-10-13-companion-memory-acceptance-m35/evidence/` — redacted request/run/message/receipt/memory/restart/cleanup captures and concise result notes. Never store credentials, tokens, raw provider secrets, or full private settings.
- Do not change M34 source files or unrelated `nx.json`.

### Task 1: Verify preflight and create synthetic fixture

- [x] Confirm current main/source revision and the prior M34 gates from the M35 progress ledger. Record revision and separate inherited code evidence from new live acceptance evidence.
- [x] Run the existing synthetic profile preflight read-only. Confirm exactly zero connectors/accounts, zero enabled schedules, zero pending/running work, unchanged existing settings/grant baseline, and port `8080` free. Record count-only guard results; do not print unrelated settings or credentials.
- [x] Create three NEW uniquely marked own default-scope Facts through the synthetic owner path: two complementary source Facts with one synthetic citation relationship referencing both source IDs, and one unrelated neighboring Fact. Record their exact IDs, full text, citations, metadata, and before snapshot in the isolated evidence folder.
- [x] Verify the selected sources are visible current Facts, belong to the synthetic lead, have the same durable default scope tuple, have no world/session scope, and have capacity for one cohort tag. Verify the neighbor is distinct and not a source.
- [x] Save only the exact three temporary settings fields and original tool descriptors and names needed for restoration. Do not back up or replace the whole profile.

### Task 2: Run one explicitly authorized consolidation task

- [x] Temporarily set the synthetic lead's tool grant list to exactly `memory_search` and `memory_consolidate`; keep the separately saved original list for exact restoration. Do not alter the model/provider or any other settings. Confirm no other tool is granted during the accepted task.
- [x] Submit one clear task asking the model to search the two exact records, create its own concise faithful summary (provide no desired summary text), archive those source Facts, and leave the neighbor unchanged. Provide no desired IDs as an authority shortcut; require the model to retrieve current IDs/text via search.
- [x] Use the existing owner API/local service with `max_tool_iterations=2`, `timeout_ms=20000`, `max_retries=0`, and a 3-model-step ceiling. Submit once only.
- [x] Preserve the actual provider response, every paired message, requested tool call, result, and action receipt. Do not create synthetic tool frames or fill missing evidence.
- [x] Read canonical owner memory after completion. Confirm one new Fact summary is semantically faithful to both source records; the two exact source IDs now remain as Reflections with unchanged full text, citations/evidence references, importance, timestamps, scope, and existing tags plus the same generated `cg:<32 hex>` tag. Confirm one neighbor is unchanged and exactly one new active Fact was created. Compare every preexisting record by ID; record any new ordinary conversation Reflections and exact completed-response TaskResults separately.
- [x] Verify receipts report actual handler-reported outcomes and associate with the observed run/step/tool call. Report the configured model steps and returned token usage as observations, not a billing ceiling. If any source selection is ambiguous, summary is unfaithful, receipt is missing, or an effect is uncertain, stop without a second submission or manual repair.

### Task 3: Restore state and perform one quiet same-store restart

- [x] When the first task is settled and the daemon is stopped, set zero tool rounds, zero retries, a 20-second timeout and an empty grant list before the one restart. Restore original settings/grants only after the final request and shutdown; do not replace the profile wholesale.
- [x] Capture exact daemon, wrapper, and runner PID, executable image, and UTC start ticks. Stop only those recorded process identities; do not use broad process-name or port-owner kills.
- [x] Verify no running/queued work, pending flushes, connectors, enabled schedules, or provider request remains. Restart the same daemon against the exact same synthetic store paths once.
- [x] After readiness, read the original run and canonical memory without resubmitting or replaying the task. Confirm the summary, archived source IDs/content/citations/group tag, neighbor, status, actual messages, and receipts are unchanged across restart.
- [x] Do not claim exactly-once execution from a single observed run or quiet restart. State only that no replay was observed in this bounded check.

### Task 4: Test learned Spanish context without tools

- [x] Start one fresh session against the same restarted store. Ask a short Spanish paraphrase that does not repeat distinctive source wording or expose memory IDs, and asks for the relevant saved preference naturally.
- [x] Set `max_tool_iterations=0`, `max_retries=0`, and a one-model-step ceiling. Do not enable or call `memory_search`; require automatic context selection from the configured real local FastEmbed/E5 index.
- [x] Verify the request contains no tool grant/call, the actual Spanish answer reflects the consolidated Fact accurately, and no unrelated neighbor fact is surfaced. Capture the configured embedding mode and actual request/response evidence without copying secrets.
- [x] Treat a failed or irrelevant answer as a failed acceptance observation. Do not repair the index manually, use explicit search, change embedding provider, or repeat the model call.
- [x] Keep the second request within the four configured model-step total. Its use of local embeddings is not an additional chat completion.

### Task 5: Restore and verify cleanup

- [x] Stop only exact recorded daemon/wrapper/runner identities and verify each captured PID/image/start-tick tuple is gone. Confirm ports `8080` and the owned web port are free.
- [x] Restore original temporary settings and exact tool-name list; verify no new grants, enabled schedules, pending/running work, or connectors remain. Do not delete the synthetic Facts or reset the profile.
- [x] Save a concise cleanup proof and evidence manifest with timestamps, source hashes/IDs, run IDs, receipts, restart identity proof, model-step/token observations, and cleanup results. Redact account identifiers and omit credentials.
- [x] If the wrapper required forceful quiet cleanup, report that fact; do not call it graceful shutdown. Do not leave services running.

## Acceptance matrix

| Claim                             | Required evidence                                                                               | Limit                                                                                  |
| --------------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Exact-source consolidation worked | Actual search and consolidate calls/results; canonical summary + two archived sources           | Faithfulness is judged in this one synthetic example, not generalized semantic quality |
| Evidence was preserved            | Exact source IDs/text/citations/metadata and shared group tag before/after                      | Tags are a discoverability link, not proof or tamper-proof audit                       |
| Neighbor was preserved            | Exact neighboring Fact unchanged in canonical owner view                                        | Does not cover other users or production records                                       |
| Quiet restart retained effects    | Same run/messages/receipts and memory after one same-store restart; no replay observed          | Does not establish exactly-once behavior or uptime                                     |
| Learned Spanish context works     | Fresh session, zero tools/rounds, one answer reflects the summary, configured local E5 evidence | One paraphrase is not a benchmark or broad multilingual-quality claim                  |
| Environment was restored          | Exact original grants/settings plus zero processes/ports/connectors/schedules/pending work      | Does not validate hosted deployment or external connectors                             |

## Five binding risks for root

1. **Synthetic isolation:** only the retained approved synthetic lead/profile is eligible. Cost if wrong: abort before mutation; never inspect or restore an ordinary profile.
2. **Provider cost and retries:** one first task plus one context request, fixed model-step ceilings, 20-second first-task timeout, zero retries, and no resubmissions. Usage remains observational, not a billing guarantee.
3. **Memory authority and fidelity:** model must search current full text/IDs; host enforces exact CAS and scope. Cost if source selection or summary is uncertain: preserve it and report incomplete; no repair or retry.
4. **Effects and restart:** handler receipts are not full-task proof, and one restart does not prove exactly-once effects. Compare canonical records, paired messages, actual receipts, and precise bytes/IDs; never replay to make a proof pass.
5. **Cleanup and source evidence:** restore only saved fields/grants and stop exact owned identities. Keep new real-model evidence separate from inherited M34 gates and earlier M16/M28 provider evidence; no source changes or live-service claims.

## Verification and resumption

- No code or runtime target changes are expected. Reuse the current M34 source gates as inherited evidence; no fresh Cargo/Nx gate is required for a docs/evidence-only result.
- Review the exact evidence files and claims, touched Markdown formatting/local links, and `git diff --check` before any publication. Preserve unrelated `nx.json` changes.
- Record actual counts and outcomes without altering earlier evidence. A failure remains a useful acceptance result; it must not trigger extra prompts or direct memory repair.
- **Exact next resume path:** `.superpowers/sdd/2026-10-13-companion-memory-acceptance-m35/progress.md`.

## Controller rulings on the risks (binding)

1. Ruling: Use only the explicitly approved M16 synthetic stores and one existing validation lead, with all 13 account arrays, schedules and pending work guarded before effects — prior local-testing authority covers this fixture and connection — cost if wrong: abort the experiment and preserve evidence; no ordinary profile or credential operations.
2. Ruling: Submit once for search/consolidation/final and once for fresh recall; maximum four configured model steps, zero retries, no diagnostic requests — this bounds the cheap acceptance case without pretending to cap provider billing — cost if wrong: incomplete evidence; no resubmission.
3. Ruling: Let the model author the summary from two retrieved exact Facts about a fictional rare dragonfruit allergy; independently judge full claims and retained source metadata/citations — a supplied summary or common guessable allergy would weaken the test — cost if wrong: report failed or uncertain acceptance, never repair model memory directly.
4. Ruling: Compare all preexisting records by ID, allow only source type/cohort changes and one new active summary Fact, and separately record ordinary new conversation Reflections and exact completed-response TaskResults; restart once with tools disabled — normal journal entries are not illicit extra batch Facts and quiet restart must not replay — cost if wrong: preserve an incomplete result and stop the experiment.
5. Ruling: Record daemon, wrapper and every intervening runner by absolute image, PID and UTC start ticks; stop only those identities, then restore only the saved three settings and original descriptors on current state — avoids stale-PID cleanup and wholesale rollback — cost if wrong: leave precise cleanup/resume evidence and report the remaining owned process. M34 source gates are inherited, real M35 observations are distinct.

## Observed result (2026-10-13)

Both logical requests passed on unchanged merged M34 source. The first used three model steps and reported 4,923 tokens: search retrieved both exact synthetic Facts, the model authored a faithful summary, and consolidation retained original IDs/text/citations and the neighboring Fact. One new summary Fact, one ordinary response Reflection and one exact completed-response TaskResult were recorded. A quiet same-store restart retained the original run, six messages, receipts and memory with no replay observed.

The fresh empty Spanish session had no granted tools and used one model step, reporting 614 tokens. Its question contained no ingredient, source ID, marker or supplied answer; the actual answer named dragonfruit/pitahaya and anaphylaxis. Total: four steps, 5,537 reported tokens, two logical requests, zero retries/resubmissions/probes. The configured local FastEmbed multilingual E5 backend had dimension 384; cached model assets were verified before launch.

The first private verifier missed the normal runtime TaskResult record. Its source-confirmed correction checks exact completed-response text and metadata; only the read-only proof was rerun on the same task. No model request or memory effect was repaired or repeated. All 14 exact owned process identities are gone, ports 8080/4200 are free, and the three nullable settings plus all 21 original descriptors were restored on current synthetic state. No source change or new Cargo gate was needed; the unchanged M34 gate hashes were rechecked. Publication and exact resume remain in the private milestone ledger.
