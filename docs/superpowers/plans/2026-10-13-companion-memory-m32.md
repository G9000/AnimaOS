# M32: Filter vector recall before top-k

**Status:** Complete on `g9000/companion-memory-m32`, cut from merged M31
`a4f5c647d813550f394433742cc5b7eb245e460c`. Current local gates passed; exact
publication and resume are authoritative in the milestone ledger.

## Goal

Prevent ineligible vector records from consuming the memory-search top-k slots
or depressing scores for eligible results. The core recall path already applies
canonical record, search-option, and owner-visibility checks after vector search;
the gap is that `MemoryVectorIndex::search(query, limit)` truncates first. Add
an object-safe, backward-compatible filtered search method, use it from hybrid
recall, and let the native in-memory index apply eligibility before top-k.

Keep the change limited to retrieval. Do not add vector settings, provider
selection, per-agent filters, default grants, new HTTP/API/SDK shapes, context
provider changes, score weights, persistent-index migration, or a broad index
framework. No service or provider call is required.

## Verified current interfaces and behavior

- `packages/core-rust/crates/anima-memory/src/memory_manager/types.rs` defines
  the object-safe `MemoryVectorIndex` trait with only `search(query, limit)`.
- `MemoryManager::recall_with_vector_index_filtered` in
  `memory_manager.rs` calls `vector_index.search(query, lexical_limit)`, then
  filters canonical IDs, `memory_matches_search_options`, and the supplied
  `visible` predicate. It computes `max_vector_score` from the unfiltered hits
  before those checks.
- Other recall paths already check search options and visibility before adding final candidates. Preserve those paths, separate source limits and current semantics; M32 does not claim universal pre-limit eligibility outside native vector retrieval.
- `InMemoryVectorIndex<E>` stores normalized embeddings and has
  `search_embedding`, which scores all stored vectors, sorts descending with
  memory-ID tie breaking, and truncates. `MemoryVectorIndex for InMemoryVectorIndex`
  embeds its query and delegates to this method.
- The daemon's `MemoryEmbeddingRuntime` in
  `hosts/rust-daemon/src/memory_embeddings.rs` owns `InMemoryVectorIndex` and
  implements only `search`. Its `search` embeds the query once with the selected
  provider and delegates to `search_embedding`.
- `hosts/rust-daemon/src/tools/memory.rs::execute_memory_search` already calls
  the filtered core recall and supplies `visible_memory`, which rejects hidden
  IDs, content, and tags. The candidate filter must also enforce canonical
  existence and the `MemorySearchOptions` constraints inside the vector index
  callback.
- Existing tests include `retrieval_filter_visibility_precedes_limits_on_each_recall_source`
  in core and hidden-content/id/tag tool regressions in daemon tests. The latter
  currently use disabled embeddings; add one explicit local-embedding starvation
  regression without changing those fixtures.
- `core-rust/project.json` has a test target; `rust-daemon: test` depends on it.
  Root owns all Cargo/Nx execution and waits for each native process to finish.

## Contract and compatibility rules

1. Add a default trait method taking `query`, `limit`, and an object-safe
   eligibility callback such as `&dyn Fn(&str) -> bool`. Keep existing
   implementations source-compatible by defaulting to the old `search(query,
limit)` and filtering those returned hits.
2. The default method is deliberately bounded: legacy/third-party indexes only
   test eligibility among their old top-k results. It does not promise to
   recover additional eligible hits below that boundary. Document this limit;
   do not silently overfetch without an index-level bound.
3. Override filtered search in `InMemoryVectorIndex`: apply the callback to
   stored IDs before sorting/truncating. Keep stable descending-score ordering,
   deterministic ID tie breaking, current positive-score behavior, validation,
   and `limit == 0` behavior.
4. The native override must embed the query once, using the same provider/query
   method as current search, then call a filtered embedding-search method. Do
   not re-embed once per record or introduce provider calls in core.
5. Core constructs one callback that looks up each ID in canonical memory state
   and requires all three checks: record exists, it matches every current
   `MemorySearchOptions` field, and caller visibility accepts it. Pass that
   callback to `search_filtered` and keep the current post-search checks as a
   defensive boundary for third-party implementations returning malformed or
   duplicate hits.
6. Compute vector normalization's maximum only from postfiltered, finite,
   positive eligible hits. Ineligible/stale scores must not change an eligible
   result's normalized vector score. Preserve exact lexical/vector weighting
   and every other scoring component.
7. Preserve zero-limit, empty-index, unavailable-embedding, invalid-score,
   stale-ID, and no-vector-index behavior. A vector hit with nonfinite or
   nonpositive score never contributes to normalization or candidates.
8. The callback only answers eligibility by ID. It must not expose hidden
   record text to the index, mutate the manager, or make missing IDs visible.
   Keep it pure and deterministic because native filtering and core's
   defensive postfilter may evaluate the same ID more than once.
9. Public retrieval/search APIs and request/result schemas remain unchanged.
   Do not add a configuration surface or widen candidate limits globally.

## Proposed root risk rulings

1. **Compatibility:** accept the bounded legacy default fallback and document
   that only filtered-aware indexes can fill beyond their previous top-k; do not
   force a breaking trait migration or unbounded overfetch.
2. **Eligibility:** canonical lookup, every search option, and caller visibility
   form one predicate; malformed/stale third-party hits are filtered again in
   core before normalization and result insertion.
3. **Ranking:** normalize against eligible finite positive hits only; update the
   existing visibility test's expected vector score and prove weights remain
   otherwise unchanged.
4. **Native work:** use the existing embedder and in-memory index only; query
   embedding happens once per search and eligibility is applied before native
   top-k. No SQLite/schema/provider/settings work.
5. **Evidence:** one meaningful local-embedding tool RED plus compact core and
   index regressions are sufficient. Do not run live embedding services, model
   calls, datasets, or unchanged SDK/web suites; root owns native aggregate and
   lint gates.

## Implementation sequence (test first)

After root binds the five risks, split production ownership cleanly: one core
worker owns the four `anima-memory` files; one native worker owns
`memory_embeddings.rs` and `tools/tests.rs`. The meaningful daemon-tool RED
precedes both production edits. Root owns test scheduling, combined native
review, gates, ledger, and publication. Do not have both workers edit
`memory_manager/tests.rs` or `vector_index.rs`.

### 1. Native tool starvation RED

Owned test file: `hosts/rust-daemon/src/tools/tests.rs`.

Add one focused async test for the real `memory_search` tool using the existing
`memory_correction_fixture(None)` and the local `MemoryEmbeddingRuntime`. Seed
one eligible record whose content is semantically related to the query through
the existing local semantic groups but has no exact query terms, then seed more
than the current default vector candidate bound of stronger query-matching
records that are rejected by `visible_memory` (for example, a hidden tag). Set
the runtime to `MemoryEmbeddingRuntime::local_default()` and upsert all records.
Issue a one-result memory_search request with the semantic query and assert the
eligible record is returned, the hidden records are absent, and canonical
memory state is unchanged.

Keep the fixture deterministic and local. Do not use HTTP, remote embedding,
normal profiles, provider credentials, test-only production APIs, or a missing
production method in the test. If hand-picked local synonyms do not reliably
create a positive semantic-only hit, use an existing recorded local embedder
test pattern or a small deterministic `MemoryTextEmbedder` fixture while still
exercising the native tool/index path. Root observes the expected RED before
production changes.

### 2. Core filtered-search contract

Owned files:

- `packages/core-rust/crates/anima-memory/src/memory_manager/types.rs`
- `packages/core-rust/crates/anima-memory/src/memory_manager.rs`
- `packages/core-rust/crates/anima-memory/src/memory_manager/tests.rs`

First add/extend core tests using existing `StaticVectorIndex`, which implements
only `search`, to prove the default method preserves old implementations and
filters its bounded result set. Do not require the new method in this legacy
fixture. Add eligible/noneligible scores that prove core postfiltering excludes
stale/option-mismatched/visibility-rejected IDs and normalizes eligible hits
against the eligible-only finite positive maximum. Retain the existing
vector/lexical weighting assertion and snapshot immutability checks.

Then add the default object-safe trait method. In `recall_with_vector_index_filtered`,
construct eligibility from canonical `memories`, `memory_matches_search_options`,
and the caller's `visible` predicate. Call `search_filtered(query,
lexical_limit, &eligible)`. Recheck eligibility on returned IDs before scoring
and normalize only after that defense. Keep all non-vector sources unchanged.

Update `retrieval_filter_visibility_precedes_limits_on_each_recall_source`:
the surviving vector hit is now normalized by the maximum eligible hit, so the
expected vector score is 1.0 in the one-survivor fixture. Add a second eligible
hit with a lower score when useful to assert relative normalization rather than
only the one-hit identity case.

### 3. Native index filtered top-k

Owned files:

- `packages/core-rust/crates/anima-memory/src/vector_index.rs`
- `packages/core-rust/crates/anima-memory/src/vector_index.rs` unit tests
- `hosts/rust-daemon/src/memory_embeddings.rs`

Add `search_embedding_filtered` to `InMemoryVectorIndex`, retaining existing
embedding validation, positive-dot filtering, deterministic sort, and truncation
while skipping IDs rejected by the callback before top-k. Keep
`search_embedding` behavior unchanged; reuse one internal implementation only
when it remains clearer than duplicating its short scoring loop.

Override `MemoryVectorIndex::search_filtered` in `MemoryEmbeddingRuntime`. Return
no hits while disabled; otherwise call `embedder.embed_query(query)` once and
delegate to `index.search_embedding_filtered`. Preserve current empty/unavailable
embedding behavior by returning an empty vector on the same errors as `search`.
No new dependency or setting is needed.

Add a small direct index test where an ineligible highest hit precedes an
eligible lower hit at limit one; filtered search returns the eligible hit.
Retain/extend existing tests for limit zero, score order, tie-breaking, invalid
vectors and dimension checks. The native tool starvation RED becomes green only
after the core callback and native override both work together.

## Test matrix and evidence boundaries

- Existing `StaticVectorIndex` compiles unchanged and uses the trait's bounded
  default fallback.
- Bounded legacy fallback filters returned top-k IDs but does not promise
  further eligible coverage below top-k; assert and document that compatibility
  limit rather than implying the old adapter is fully starvation-proof.
- Native in-memory filtered search finds an eligible lower-ranked vector when
  stronger ineligible vectors would otherwise fill the limit.
- Core filter checks canonical existence, agent, agent name, type, scope,
  room/world/session, min importance, and caller visibility before final scoring.
- Ineligible maximum scores do not depress normalized eligible scores; invalid
  scores do not contribute; remaining score weights and lexical suppression
  remain as before.
- Native `memory_search` tool returns the vector-only visible semantic match
  despite hidden-vector crowdout, with no hidden ID/text and no memory mutation.
- Disabled index, unavailable local embedding, empty/zero limit, no index, and
  ordinary no-match behavior remain empty/unchanged.
- Existing lexical/recent/relationship/temporal/fallback retrieval regressions
  remain untouched and pass in root's aggregate.
- No claim is made for third-party vector backends' ability to retrieve eligible
  records below their bounded legacy top-k, real embedding-provider quality,
  external vector stores, or production relevance beyond these fixtures.

## Files expected to change

- `packages/core-rust/crates/anima-memory/src/memory_manager/types.rs`
- `packages/core-rust/crates/anima-memory/src/memory_manager.rs`
- `packages/core-rust/crates/anima-memory/src/memory_manager/tests.rs`
- `packages/core-rust/crates/anima-memory/src/vector_index.rs`
- `hosts/rust-daemon/src/memory_embeddings.rs`
- `hosts/rust-daemon/src/tools/tests.rs`

No SDK, web, daemon route, capability, profile, model adapter, database schema,
embedding persistence, or global configuration files should change. If review
identifies a needed exception, stop and bind it explicitly before broadening.

## Root-owned verification

Root runs the meaningful focused RED first. After source freezes and reviews, the required aggregate verifies all core/index/native cases; avoid another focused GREEN recompile unless a failure justifies it. Use existing target directories only. Do not overlap Cargo
processes; use the repository's established serial `CARGO_INCREMENTAL=0`
workflow and wait for exit before the next command. Once the combined native
batch is reviewed, do a read-only SDK/web contract check confirming no public
surface changed, then run the required aggregate and lint once:

- `bun x nx run rust-daemon:test --skipNxCache`
- `bun x nx run rust-daemon:lint --skipNxCache`

The daemon test target composes the `core-rust:test` target; do not separately
repeat an aggregate it already ran. Skip unchanged SDK/web gates and unnecessary
locomo/baseline datasets. No services, model/provider calls, normal profiles,
credentials, HTTP daemon, or external index are needed. Record the observed RED,
focused outcomes, aggregate counts, and any legacy fallback limitation in the
M32 progress ledger; plan assertions are not test evidence.

## Controller rulings on the risks (binding)

1. **Compatibility/fallback:** Add an object-safe filtered entry with a bounded default using one old search plus returned-hit filtering; no oversampling loop, index migration or required new method for existing adapters. The native in-memory index and daemon override eligibility before scoring/top-k. Cost if wrong: external legacy adapters remain limited to their old shortlist and cannot promise backfill below it.
2. **Complete eligibility:** Resolve canonical records and reuse all existing MemorySearchOptions and caller visibility under the current read locks. An indexed ID/score never grants access. Defensively check returned hits again and reject missing/invalid scores. Cost if wrong: stale/noncompliant adapters may still produce fewer results; no broad recall source rewrite or new authority framework.
3. **Score normalization:** Normalize over eligible finite positive vector hits only. Preserve descending score/ID tie order, fusion weights and exact-lexical vector suppression. The historical hidden-max score0.5 fixture becomes1.0 intentionally. Cost if wrong: rankings change in cases formerly influenced by rejected hits; deterministic tests must make that change explicit.
4. **One embedding/current scope:** Embed query once per nonzero native search; zero limit/disabled/unavailable fail empty. Keep providers/configuration/grants/context injection/HTTP/SDK persistence formats unchanged. No downloads, accounts, model calls or new dependencies in fixtures. Cost if wrong: existing synchronous providers remain slow; this is selection correctness, not semantic quality or consolidation.
5. **Evidence/cheap verification:** One meaningful real-local-index tool RED precedes production; disjoint core/native workers, one combined native batch plus final SDK/web contract review, one justified fix wave then required aggregate and sequential lint. Source hashes/current gates and synthetic snapshot invariance bound claims; no duplicate unchanged SDK/web tests or new target directories. Cost if wrong: real embedding/provider/browser quality and third-party backfill remain unverified.

Root owns every Cargo/Nx/service/Git operation and waits for actual session exit
before another Cargo. Standing user autonomy overrides additional approval/audit
or worktree flows; all rulings and exact publication/resume live in the ledger.
This native index fix is not represented as copied Hermes source: reuse pinned
public code where applicable, keep existing notices, and make no whole-product
parity claim.

Additional binding bound: the legacy default and defensive core examination take
at most the requested candidate count before eligibility checks, even if an
adapter violates its limit contract. This is a safety/compatibility fallback,
not full eligible backfill; the native override applies eligibility before top-k.
No full-corpus adaptive oversampling or repeated query embedding is allowed.

## Current verification and publication

The meaningful local-tool RED established no own lexical hit, positive actual
local vector similarity, a strictly stronger global top4 that omitted the own
record, executorSuccess and unchanged canonical snapshot before matchCount0
failed against1. No missing-symbol test or model/provider call preceded production.

Five grouped core and four native cases pass, including all option fields,
visibility/canonical-missing IDs, bounded legacy/noncompliant output, invalid
scores, eligible-only normalization, score/ID ties, one filtered query embedding,
zero/disabled/unavailable paths and the formerly failing native tool. The old
hidden-max fixture intentionally changes normalized vector0.5 to1.0; weights and
exact-lexical suppression remain tested.

One native batch and final SDK/web contract review found no retrieval/transport
P1/P2. One P2 documentation fix narrows zero-no-embedding behavior to the filtered
entry and native pre-top-k coverage; direct legacy portable search keeps its old
zero-limit embedding call. There was no further source fix or audit wave.
Fresh required Rust **2,432 passed / eight ignored / 40 suites**, followed by
strictly sequential formatting, passed. All6reviewed source hashes are unchanged.
SDK/web source matches M31; no redundant unchanged TypeScript gate was run.
M31 web1,201 and SDK128unit tests/3servicecases skipped/type/build are historical.

No actual memory/account/profile/provider/model/service, downloads, dependency,
index migration or new target directory was used. Fixtures prove native selection
with existing fixed local synonym groups, not general semantic quality, latest
text/vector revision agreement, automatic context injection/consolidation or full
external legacy-index backfill. Exact tested source/PR/merge/resume are in
`.superpowers/sdd/2026-10-13-companion-memory-m32/progress.md`. Publish only13owned
public paths, preserving original `nx.json`, manifests and private evidence.
