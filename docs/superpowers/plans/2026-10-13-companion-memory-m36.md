# Memory embedding content identity M36 Implementation Plan

> **For agentic workers:** Follow this plan task-by-task with the existing focused Rust workflow. Root owns all Cargo/Nx execution, formatting, Git, and publication.

**Goal:** Prevent a persisted or delayed vector for old memory text from influencing recall after the canonical record changes.

**Architecture:** Bind every indexed vector to SHA-256 of the full exact UTF-8 memory content. Treat absent/legacy or mismatched hashes as cache misses during rebuild, and filter candidate vectors against current canonical content before top-k at each native recall surface. Preserve existing lexical/recent behavior, M33 bounded inference and public memory shapes.

**Tech Stack:** Existing Rust daemon, SQLite embedding cache, `sha2` (already a daemon dependency), `MemoryManager`, and `MemoryVectorIndex` compatibility API. No new dependency, core public trait, `Memory` field, HTTP/SDK DTO, or profile/config surface.

---

## Bound behavior

- The bug is that cache admission checks only memory ID, model and dimension. A saved record can change from text A to B while its old A vector still loads after a failed embedding, crash, or stale delayed upsert.
- Hash the entire `memory.content.as_bytes()` with SHA-256. Do not trim, normalize, tokenize, or hash only the context excerpt. A text mismatch is a cache miss.
- Persist an optional digest beside each cached vector. Existing rows without it are legacy cache misses and rebuild once; a later restart with the same exact text/model/dimension loads the matching vector.
- Rebuild considers only rows whose ID exists, model and dimension match, and content digest exactly matches the canonical record. A mismatched row is never admitted into the in-memory vector index.
- At explicit `memory_search`, owner memory recall, and automatic provider context, current canonical text must be checked against the vector's indexed text digest inside the ID eligibility callback, before the native index chooses top-k. Keep existing option/visibility filtering, final canonical postfilter, lexical results, recent recall and M33 lexical fallback/bounds intact.
- Use a borrowed native adapter over the current `MemoryManager` plus the embedding runtime. Do not add a core public trait method or a serialized memory field. Keep `PreparedVectorIndex` query/model checks and the existing lock-free inference phase; digest ranking uses the current manager only after inference completes.
- A concrete private shape is `CurrentMemoryVectorIndex<'a> { runtime: &'a MemoryEmbeddingRuntime, memories: &'a MemoryManager }`, implementing the existing trait. Its filtered search composes caller eligibility with `current memory.content SHA-256 == indexed vector SHA-256` before the runtime index's top-k; the prepared-query path keeps its existing model-key check and receives the same canonical manager reference.
- An unsuccessful embedding must not replace an older vector's digest with the new content's digest. A delayed upsert of stale text may leave a stale cache row, but current-text checks must exclude it; a later rebuild must miss it.
- Keep existing best-effort embedding error behavior. Canonical writes remain authoritative and lexical/recent recall remains available when vector indexing is missing or fails. No background job, retry queue, revision framework, write-pipeline overhaul, or data deletion.
- SHA equality establishes current byte-text identity only. It does not prove semantic equivalence or distinguish ABA when text changes away and then back to the same bytes.

## File ownership

- `hosts/rust-daemon/src/memory_embeddings.rs` — content digest helper, in-memory digest association, SQLite nullable column/schema-version-2 migration, cache load/upsert/rebuild checks, and storage/search tests.
- `hosts/rust-daemon/src/tools/memory.rs` — wrap explicit tool vector recall with current canonical content eligibility; preserve current owner visibility and search options.
- `hosts/rust-daemon/src/routes/memories.rs` — wrap owner recall with the same current-content check; keep response types and API behavior unchanged.
- `hosts/rust-daemon/src/components/providers.rs` — make `PreparedVectorIndex` filter by current manager text digest without moving embedding inference under store locks or changing the M33 deadline.
- Existing tests in the same native modules, `hosts/rust-daemon/src/tools/tests.rs`, and `hosts/rust-daemon/src/routes/tests/memory_edits.rs` — stale-content recall, fallback, and no-neighbor regression coverage.
- Root alone updates milestone ledger, handover, master-plan row, reviews, runs gates, formats, and publishes. No public docs other than the M36 plan are changed by source workers.

## Task 1: Add the storage/cache RED first

- [x] Add one deterministic SQLite regression in `memory_embeddings.rs`: embed/persist memory ID X with text A, change canonical X to text B, and rebuild using the same model/dimension.
- [x] Assert the A row is not loaded (`loaded_vectors == 0`), B is rebuilt once (`rebuilt_vectors == 1`), and B's embedding/ranking is used. Add a distractor so the test proves the old A vector cannot win rather than merely checking a counter.
- [x] Assert the initial failure is specifically the current ID/model/dimension-only cache accepting A for B. Keep the test target under `memory_embedding_content_identity_` for root's focused RED.
- [x] Add no production code until root observes this meaningful RED.

## Task 2: Persist and enforce exact content identity

- [x] Add a lowercase 64-hex SHA-256 helper over full UTF-8 bytes and store the digest with each embedding row and in the runtime's per-ID in-memory vector metadata.
- [x] Add a nullable SQLite content-digest column with an idempotent migration for existing files and advance the embedding-cache schema marker to version 2. Old rows remain NULL and rebuild as misses. Keep SQLite cache rows local-only; do not migrate the canonical memory store or its schema.
- [x] Extend stored-row loading to parse digest as optional and require exact content/model/dimension match before `upsert_embedding`. A malformed/missing/mismatched digest must rebuild from canonical content once.
- [x] Make successful upsert replace vector and digest together. If embedding itself fails, preserve the old vector/digest pair unchanged. If SQLite persistence fails after an in-memory upsert, retain truthful best-effort behavior; on reload the old persisted digest still fails current-text comparison.
- [x] Clear/remove the in-memory digest association whenever its vector is cleared or removed; never leave a digest-only or vector-only eligible entry.
- [x] Add storage tests for a legacy version-1 table/NULL digest (one rebuild then one load), idempotent schema reopen at version 2, corrupt vector/digest, same-content/model/dimension reuse, and model/dimension mismatch rebuild.

## Task 3: Filter stale candidates before top-k on every native path

- [x] Add a borrowed adapter that can compare each candidate ID's stored indexed-content digest with the full current canonical record text and compose the existing caller eligibility predicate.
- [x] Route explicit `memory_search` through the adapter while retaining its current owner/hidden-text/options checks and lexical fallback.
- [x] Route `routes/memories.rs` recall through the adapter. Do not change owner auth, query parsing, result DTOs or scores for current-vector matches.
- [x] Give `PreparedVectorIndex` a borrow of the current `MemoryManager`; after existing cloned query inference finishes, pass current canonical content-digest eligibility into the prepared-query search. Preserve its model-key guard, current-manager reacquisition, two permits, one five-second deadline, lexical snapshot fallback and 6/800/4,800 context bounds.
- [x] Keep current canonical final filtering and scoring unchanged. Stale/missing-digest vectors contribute no vector candidate or vector score; lexical/recent/other eligible signals remain available.

## Task 4: Prove stale vectors cannot crowd out current matches

- [x] Add focused tests for each surface using a stale high-scoring A vector under an ID whose current text is B, plus a genuine current B match and an unrelated neighbor. Verify stale A is excluded before top-k and the current match/neighbor behavior is correct.
- [x] Test delayed stale A upsert after canonical B: it may write an A-tagged cache row, but explicit tool search, owner recall and automatic context exclude it before top-k; restart rebuilds B instead of loading A.
- [x] Test embedding failure on B after A was indexed: old A's digest is not changed to B, no stale vector is admitted, and lexical/recent fallback still returns current canonical B where appropriate.
- [x] Test missing ID, current ID with legacy NULL digest, hidden/ineligible records and wrong model/dimension. Preserve existing option, visibility, top-k, normalization, deterministic order, and no-query-reembedding assertions.
- [x] Do not assert semantic quality beyond deterministic embedding fixtures; the regression is byte-identity/cache correctness.

## Five principal risks for root binding

1. **Legacy cache compatibility:** old SQLite rows have no digest. Make migration nullable/idempotent and rebuild those rows once; do not treat NULL as a wildcard. Cost if wrong: old vectors may contaminate recall or existing cache startup can break.
2. **Stale writes and failed embedding:** keep vector+digest paired and never mark old vector as current for new text. Current canonical text must gate eligibility even if a stale upsert wins a race. Cost if wrong: wrong-content ranking can persist across restart.
3. **Top-k placement:** stale candidates must be filtered before top-k on explicit search, owner recall, and automatic context; postfilter alone cannot backfill. Cost if wrong: a current match remains crowded out.
4. **M33 execution invariants:** digest checks must not move model inference under canonical/index locks, extend the five-second deadline, create unbounded workers, or lose lexical fallback. Cost if wrong: memory updates block or context silently disappears on slow embedding.
5. **Scope and claim limits:** this hashes bytes, not semantics; it cannot detect ABA and does not add a canonical revision. No new DTO/schema/profile semantics, background repair system, remote call or private-data test. Cost if wrong: describe only content-identity correctness and keep runtime guarantees bounded.

## Verification and publication

- First root runs focused `memory_embedding_content_identity_` tests to observe RED with the existing Nx wrapper (root uses `bun x nx run rust-daemon:test --excludeTaskDependencies --args='--lib TESTFILTER'` for focused library tests), then source work begins. Native storage and integration tests stay fixture-only; no real memory/profile or embedding service is used.
- After one native source batch, root performs one independent native review and one final SDK/web contract review. A single justified fix wave precedes the full required Rust daemon aggregate and lint. No SDK/web gate is needed if their source remains unchanged.
- Root serializes Cargo with `CARGO_INCREMENTAL=0`, one job, existing target directory, and waits for actual exit before another Rust command. Use repository targets: `bun x nx run rust-daemon:test --skipNxCache` and `bun x nx run rust-daemon:lint`. Use the existing exact formatting check for owned Rust paths; do not invent flags or create a target directory.
- No services, browser, credentials, ordinary profile, model request, DB server/container, or Git operation is part of this plan. Preserve unrelated `nx.json`.
- Root publishes this plan from its explicit owned path, links it from the master-plan status table/handover, records the actual RED/GREEN/gate evidence and exact source revision, then uses standing reviewed merge authority.

## Controller rulings on the risks (binding)

1. Ruling: Migrate only the derived embedding cache, with a nullable exact full-content SHA-256 field and idempotent version-2 initialization; legacy/malformed/mismatched rows are misses — current canonical text must establish cache reuse — cost if wrong: old cache rows rebuild once, while canonical memory and public store/DTO schemas remain untouched. No compatibility claim for concurrent older binaries writing the same cache.
2. Ruling: Replace each in-memory vector and its source digest as a pair after successful embedding; an embedding failure preserves the truthful old pair, and a stale late upsert is excluded against current canonical content — avoids relabeling old semantics as new — cost if wrong: semantic coverage can temporarily fall back to lexical/recent signals, not automatic repair or retries.
3. Ruling: Apply the native canonical-aware adapter before top-k on tool search, owner recall and prepared automatic context — postfiltering cannot recover a crowded-out fresh candidate — cost if wrong: meaningful native regression must fail; existing owner/visibility/options and lexical ranking remain unchanged.
4. Ruling: Keep M33 query inference outside canonical/index guards with existing permits and inclusive five-second returned-result deadline; digest comparison is pure ranking after reacquisition — this fix must not undo bounded inference — cost if wrong: retain the established lexical fallback and document finite ranking work rather than physical cancellation guarantees.
5. Ruling: Use deterministic temporary-file/mock fixtures and existing sha2; no new provider requests, real services, profiles, broad background maintenance or source revision framework — the demonstrated bug is cache text identity, not generalized semantic quality — cost if wrong: exact same-byte ABA is indistinguishable and full Hermes parity remains unproven. Root runs the required Rust aggregate/lint, one batch native review and final contract review before explicit publication.

## Execution evidence

The original deterministic regression failed after its ranking prerequisites: the changed canonical record rebuilt zero vectors instead of one. The focused final run passes all eight grouped content-identity tests, including the three actual recall surfaces. One native batch review and the final SDK/web contract review were clean; no fix wave was needed. The required full Rust aggregate, sequential lint and existing cached learned-context control are recorded in the milestone ledger before publication. No SDK/web source or public contract changed.
