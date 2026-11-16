# Model-directed multilingual recall (17 November 2026)

This slice replaces AnimaOS's English intent and topic heuristics with explicit
searches chosen by the existing conversation model. It preserves the earlier A6
and bounded-read work. Mother remains an additive shared provider, and native
memory remains the default source.

Bootstrap resolves only owner-pinned and assigned records. The model can call
`memory_search` with `source: "mother"`, a concise query in any language, a mode,
optional v1 category, optional exact entity constraints and optional A6 graph
recall. It can reformulate or translate a query when the provider reports literal
fallback or incomplete indexing. Neither Rust nor Mother receives a new English
vocabulary, translation table or species classifier.

The adapter preserves per-read grants, current binding/agent identity, workspace
and restore generation, revisions, complete-record bounds, threat scanning and
cancellation. Shared results have distinct provider locators. They are not native
mutation IDs. Provider errors never silently substitute native results. Graph
text reads report literal mode independently of v1 semantic mode. Coverage counts
rejected fetched records as well as unresolved candidates and output exclusions.

## Evidence boundaries

- Scripted conversation-model fixtures exercise the actual coordinator and tools
  for Chinese/English, Japanese/English and Malay/English requests, followed by
  native recall. They prove routing and preservation, not model language reasoning.
- An explicitly selected offline test loads genuine cached multilingual E5 ONNX
  and tokenizer bytes. Its synthetic corpus includes a cat, a dog and unrelated
  facts. It checks top-ranked cross-language results and the production native
  `memory_search` path. No download or model-hub constructor participates.
- Disposable native Mother tests separately cover full-record retrieval and A6
  graph contracts, corrections, history and independent read/propose scopes.
- Mother retains its own configured model and reports its actual search mode.
  This change does not claim that Mother's MiniLM model became multilingual E5.

## Qualification

| Check | Result |
| --- | --- |
| Focused multilingual regressions | 12 passed; the E5 case was selected separately. |
| Cached multilingual E5 acceptance | One passed; all four queries ranked the correct fact first and returned full native content. |
| Real native Mother/A6 acceptance | Three passed: full personal record, correction/scope isolation, all seven graph tools. |
| Full Rust suites | 6,009 passed; 0 failed; 70 opt-in tests ignored. |
| Rust lint | Passed `bun run rust:lint` (format check and repository Clippy policy); existing workspace warnings remain. |
| Locked bundled build | Passed `cargo build --locked -p anima --features bundled-ui,rust-embed/debug-embed` through the managed validation wrapper. |

All final checks completed against the same 24 frozen Rust source hashes.
Private evidence retains passing and failed logs, source snapshots and the scope
audit. Cleanup verified process ownership and path boundaries before removing
only this task's synthetic stores and helper scripts. The completion checkpoint
was saved successfully.

Earlier failed regressions and the verification interrupted to address review
findings are retained as evidence, not counted as successful qualification. The
first native attempt correctly failed endpoint ownership checks: Python's
temporary-directory ACL included an `OWNER RIGHTS` grant rejected by Mother's
existing strict policy. Only the owned synthetic root was corrected; a disposable
owner service then started successfully. A subsequent debug test hit Rust's
default stack limit. The accepted native rerun used the repository's documented
8 MiB debug-test stack and completed all three tests. Production threads and
Mother's ownership checks were unchanged.

One intermediate full run failed the existing native API isolation assertion
`run_agent_does_not_reuse_previous_tool_result_between_runs`. The cause was not
established. With no source changes, that test passed alone, all 36 API tests
passed together, and the subsequent complete Rust run passed. Native retrieval
and that assertion were not changed to obtain a passing run. Two earlier A6
fixture expectations were updated for explicit model-directed reads; their
budget, full-record, privacy and scope checks remain in place.

All fixtures use synthetic stores. Live memories, sharing queues, grants,
personality/model settings, installed binaries and startup configuration are
outside this source slice. The only live garden write is the explicitly required
technical continuation checkpoint. Neither consumed Discord test allowance is
reused. No paid model call or Discord post is part of these tests. Source changes
remain local and unpublished; the installed application still runs the preceding
qualified version until an explicit deployment.
