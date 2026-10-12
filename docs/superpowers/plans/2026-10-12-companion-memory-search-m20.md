# M20 Memory Tool Retrieval — Lean Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans; root owns RED proof, binding rulings, review, validation and integration.

**Goal:** Explicit memory tools find eligible visible owner records despite stronger foreign, filtered or hidden matches, and return exact correction input.
**Architecture:** Add predicate-capable retrieval wrappers while preserving existing public methods. Apply eligibility before BM25 candidate limits and visibility before recall/recent truncation; the daemon supplies its existing hidden-text policy.
**Tech stack:** Existing native Rust BM25, memory manager and daemon tools; no new dependencies or provider calls.
**Status:** Active on g9000/companion-memory-search-m20 from verified M19 merge91bc7913585722ee9e5d2be5df2bd4109ff0a483; root rulings below bind the scoped RED-first repair.

## Current evidence and bounded scope

- `packages/core-rust/crates/anima-memory/src/bm25.rs:244-280` scores global documents, sorts BM25 scores and truncates to `limit`.
- `memory_manager.rs:553-605` requests `limit * 3` global lexical hits before owner/name/type/scope/provenance/min-importance filters.
- A stronger foreign or excluded corpus can consume every raw slot and hide an eligible own fact.
- `memory_manager.rs:285-442` combines lexical/vector/recent/relationship/temporal candidates and truncates final weighted results.
- `memory_manager.rs:608-653` filters options, clones every matching recent record, sorts and truncates.
- `hosts/rust-daemon/src/tools/memory.rs:45-69` filters only hidden content after recall has already truncated.
- `tools/memory.rs:318-333` similarly filters hidden content after `get_recent`; hidden IDs/tags are not screened.
- Existing `render_memory` returns full content plus intact ID/type; keep that exact correction surface.
- Existing `memory_matches_search_options` already covers all search options, including minimum importance.
- Existing tool tests include exact CAS, stale refusal, rollback, independent-field preservation and basic hidden-content exclusion.
- M18 automatic context selection/formatting and M18 deterministic rendering remain unchanged.

## Interfaces and ownership

| File                                                                    | Minimal responsibility                                                                     |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `packages/core-rust/crates/anima-memory/src/bm25.rs`                    | Predicate-capable BM25 method and focused eligibility test.                                |
| `packages/core-rust/crates/anima-memory/src/memory_manager.rs`          | Predicate-capable search/recall/recent wrappers; existing option/ranking helpers reused.   |
| `packages/core-rust/crates/anima-memory/src/memory_manager/tests.rs`    | Core candidate-starvation, option and compatibility regressions.                           |
| `hosts/rust-daemon/src/tools/memory.rs`                                 | One local ID/content/tag predicate for explicit reads and exact-update visibility refusal. |
| `hosts/rust-daemon/src/tools/tests.rs`                                  | Hidden crowd-out and exact search-to-correction regressions.                               |
| `.superpowers/sdd/2026-10-12-companion-memory-search-m20/acceptance.md` | Root-owned RED/GREEN, review and gate evidence.                                            |

Proposed additive methods (root may choose equivalent names):

```rust
BM25::search_filtered(&self, query: &str, limit: usize, eligible: impl Fn(&str) -> bool) -> Vec<SearchResult>
MemoryManager::search_filtered(&self, query: &str, opts: MemorySearchOptions, visible: impl Fn(&Memory) -> bool) -> Vec<MemorySearchResult>
MemoryManager::recall_with_vector_index_filtered(&self, query: &str, opts: MemoryRecallOptions, vector_index: Option<&dyn MemoryVectorIndex>, visible: impl Fn(&Memory) -> bool) -> Vec<MemoryRecallResult>
MemoryManager::get_recent_filtered(&self, opts: RecentMemoryOptions, visible: impl Fn(&Memory) -> bool) -> Vec<Memory>
```

Existing signatures delegate with an always-true predicate; no options/type/export restructuring is needed.
BM25 receives record IDs only; the memory manager resolves canonical records and combines option checks with caller visibility.
Predicates are pure read-only eligibility checks; core libraries do not import daemon hidden-text rules.
Existing scope defaults stay intact: explicit tools remain owner-scoped with their current optional room/world/session filters.
Do not copy M18's narrower automatic-context authority rules into these explicit retrieval tools.

## Task 1 — Regression-first core retrieval

- [x] Seed more than the current `limit * 3` pool with stronger foreign matches, plus one lower-ranked own fact; owner search with `limit=1` must return that own ID/content.
- [x] Add independent high-score excluded type, scope/provenance and below-minimum-importance fixtures; each existing filter must operate before lexical limiting.
- [x] Add predicate-hidden lexical crowd-out at `limit=1`, and filtered recall/recent cases where excluded records cannot consume final slots.
- [x] Cover zero limits for BM25/search/recall/recent and empty/no-match behavior without panic or predicate-driven fallback.
- [x] Verify BM25 filtered scores equal unfiltered scores for admitted IDs; preserve corpus statistics, tokenization/query expansion and current tie behavior.
- [x] Compare existing wrappers against predicate wrappers with `|_| true` for ordinary eligible fixtures; assert unchanged weighted ordering/options.
- [x] Reuse existing memory fixtures; avoid a new synthetic evaluator or large test framework.
- [x] Root runs focused RED, recording meaningful assertion failures; only minimal compile interfaces/stubs precede RED.

## Task 2 — Minimal retrieval implementation

- [x] Add BM25 eligibility before scoring/collecting candidates; keep global IDF/average-length statistics and existing sorting unchanged.
- [x] Have search pass canonical option checks plus visibility into BM25, then request the actual result limit; remove the `* 3` starvation workaround.
- [x] Preserve `MemorySearchResult` scores/content and all existing filters through the shared matcher.
- [x] Route filtered recall's lexical and recent sources through the predicate-aware methods.
- [x] Apply the same visibility check at vector, relationship, temporal evidence and fallback candidate admission before final result truncation.
- [x] Keep existing recall weights, score normalization, evidence limits, temporal fallback threshold and vector-index contract/ranking unchanged.
- [x] For recent reads, sort eligible borrowed records with the existing recency/ID-sequence comparator, truncate, then clone only returned records.
- [x] Never fetch an unrestricted snapshot, use `usize::MAX`, repeatedly increase limits or introduce an arbitrary oversampling cap.
- [x] Root reruns focused GREEN; existing search/recall/recent tests must remain intact.

## Task 3 — Explicit tool visibility and correction input

- [x] Before implementation, add daemon tests with hidden ID, content and tag records that would outrank/crowd out an ordinary owner fact at `limit=1`.
- [x] Cover both `memory_search` and `recent_memories`; use isolated canonical fixtures and disabled embeddings for deterministic lexical proof.
- [x] Assert exactly the visible record count/ID/type/full content, absence of all foreign/hidden records and unchanged canonical records.
- [x] Include a multiline visible fact; feed the retrieved exact ID/full content into the existing correction call and verify strict CAS succeeds only for that record.
- [x] Keep stale mismatch and duplicate-current-fact correction regressions unchanged; do not accept excerpts or guessed expected content.
- [x] Add hidden-ID/tag current-record update refusals using existing `MEMORY_TEXT_HIDDEN`; assert missing/foreign targets retain generic not-found ordering and records remain unchanged.
- [x] Root proves these tool tests RED before the worker changes handlers.
- [x] Implement one private host `visible_memory` predicate rejecting hidden text in ID, content or any tag; pass it into filtered recall/recent before limits.
- [x] Reuse that predicate at exact update's existing current-content visibility check, after same-owner/not-found checks; retain existing error, CAS/rollback/indexing and independent fields.
- [x] Remove the redundant post-truncation content-only filters; preserve tool limit validation/defaults, grants, approvals, metadata and full rendering.
- [x] Make no other correction-write, descriptor, evaluator, storage, HTTP route or automatic-provider changes; do not refactor the M18 provider.

## Top five risks — root rulings bind before tests/code

1. **Filter starvation:** Apply all existing search filters and caller visibility before BM25 limiting; prove the foreign/type/scope/min-importance cases separately.
2. **Visibility across recall sources:** Hidden records must not enter any final recall candidates; vector top-k itself remains unchanged and may still starve semantic-only eligible hits. Record that deferred limitation.
3. **Compatibility and authority:** Existing methods/predicates preserve current scope defaults, corpus scoring, weights and order; this milestone does not broaden another owner's authority or redefine explicit scope.
4. **Exact correction safety:** Read/update visibility checks share ID/content/tag rules; preserve foreign/missing not-found ordering, full canonical input, CAS/stale refusal/rollback/evidence fields and duplicate correction guidance.
5. **Bounded cost:** Use existing index scans and eligible borrowed references, clone only returned recent records, add no model/embedding call, dependencies, migration or unrestricted fetch workaround.

## Root verification and integration

- [x] Confirm project/targets via `bun x nx show projects --json` and `bun x nx show project rust-daemon --json` before gates.
- [x] Root sets `CARGO_INCREMENTAL=0`, serializes Cargo/Nx and reuses the existing target directory; no service runs during gates.
- [x] Name new core tests `retrieval_filter_*`, host tests `memory_tool_retrieval_*`; root verifies exact filters from the worker before running.
- [x] Focused core command: `cargo test -p anima-memory retrieval_filter_` (expected RED then GREEN).
- [x] Focused host command: `cargo test -p anima-daemon --lib memory_tool_retrieval_` (expected RED then GREEN).
- [x] Required final gate: `bun x nx run rust-daemon:test --skipNxCache`; root runs the established core gate and supported formatting checks too.
- [x] One combined implementation review covers eligibility timing, all recall admissions, preserved rankings/defaults, exact correction and cost; fix concrete findings in one bounded wave.
- [x] Record deterministic evidence and the deferred vector candidate limitation; no new live model, browser, connector, service or database acceptance run is required.
- [x] Root stages explicit M20 files only after M19 publication and passing review/gates; never stage `nx.json` or `anima.yaml`.
- [x] Claim completion only for tested explicit lexical/visible retrieval and preserved correction behavior; semantic/vector candidate starvation remains unclaimed.

## Controller rulings on the risks (binding)

1. Ruling: Apply canonical owner/type/scope/provenance/importance filters before lexical limits and prove the original starvation with existing APIs — stronger ineligible matches must not make exact own facts undiscoverable — cost if wrong: keep failed retrieval evidence and do not claim corrected recall.
2. Ruling: Pass host visibility for ID/content/tags through lexical/recent and each combined recall admission before final limits — hidden entries cannot consume output slots or be returned as edit IDs — cost if wrong: refuse the unsafe current record and investigate that path; vector adapter top-k completeness remains deferred.
3. Ruling: Existing methods delegate to always-true wrappers, retaining corpus statistics, weights, rankings, explicit scope defaults and grants — this fixes eligibility timing without silently redefining authority — cost if wrong: compatibility tests fail before publication; no broader scope or migration is accepted.
4. Ruling: Full canonical text/intact IDs remain correction input; only current-record visibility refusal is extended to ID/tags after same-owner/not-found checks — stale CAS, mutation locking, rollback and persistence are unchanged — cost if wrong: refuse the edit instead of weakening exact-text checks or rewriting unrelated history.
5. Ruling: Reuse existing scans, clone only returned recent records, no unlimited fetch/oversampling loop or new model/embedding call — user cost constraints favor deterministic reproduction and existing index paths — cost if wrong: record the limitation; do not expand into semantic/vector or autonomous lifecycle work.

Activation: root accepts the lean plan under the standing harness acceptance and
continue authority. Initial tests exercise existing search/tool APIs so RED is a
meaningful behavioral assertion failure, not an absent-method compile failure.
Root owns Cargo, one combined review/gates and verified publication/merge; workers
own the five bounded source files. No live-service/model repetition is needed.

## Verified outcome (2026-10-12)

Five bounded Rust files implement the repair; no dependency, model/provider,
HTTP/SDK/UI, evaluator, migration or automatic-context change. Existing public
methods delegate to predicate-aware wrappers. Eligible canonical records are
filtered before BM25 limits, with global statistics preserved. Hidden ID/content/
tag records are excluded across all six recall admission paths and recent reads;
exact updates use the same current-record visibility check after owner/not-found.
Full-text CAS, locking, durable rollback and independent fields are unchanged.

Meaningful existing-API RED: core four failures/one control pass; host nine
failures/one control pass. GREEN: eight core and ten host regressions, including
an independent score/order/weighted-recall oracle, all six source admissions,
zero/empty behavior, hidden crowd-out, multiline exact correction and refusal of
hidden current IDs/tags. One combined review found no actionable issues.

Fresh required Nx Rust gate: 2,342 passed/eight ignored; Rust lint passed. Owned
Markdown/local links and diff checks precede publication. No web/SDK source changed;
M17 remains their last code gates. No repeated live model/service acceptance was
needed: this native retrieval repair is covered by deterministic core and actual
tool-handler tests. M17–M19 real-model harness evidence remains distinct.

Only explicit lexical/visible retrieval is proven. The vector adapter's global
top-k contract/normalization is unchanged; semantic-only candidate starvation and
semantic quality remain unverified. No automatic consolidation or broad upstream
parity claim. All validation services/wrappers remain stopped. Exact publication
source/PR/merge SHA and daily-use resume are appended to the private ledger.
