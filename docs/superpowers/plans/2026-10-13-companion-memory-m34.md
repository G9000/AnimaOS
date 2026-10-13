# Bounded Fact Consolidation Implementation Plan

> **For agentic workers:** Execute the RED checkpoint first, then split core, native, and settings work by the ownership in the milestone ledger. Root owns gates, reviews, and merge.

**Goal:** Let an explicitly authorized agent propose one concise Fact that archives 2–8 exact, same-scope source Facts as historical Reflections in one atomic, provenance-preserving mutation; make exact duplicate default-scope Fact adds idempotent.

**Architecture:** The model supplies the summary and exact source IDs/current text; the host authorizes the operation and checks the complete source cohort under the memory write lock. A narrow core helper adds one new Fact and reclassifies each source to Reflection with one shared cohort tag before a single existing `MemoryMutation` persistence. Existing source IDs, full text, citations, importance, timestamps, scope, and tags remain intact except for the added cohort tag. Exact Fact add is a separate exact-equality/idempotency rule, never fuzzy merge.

**Tech Stack:** Existing `anima-memory` manager, rust-daemon tool registry and `MemoryMutation`, current approval policy and SettingsPanel tool-grant patch, existing embedding index. No schema migration or additional paid summarizer call.

---

## Bound behavior

### Consolidation request and authority

- Register one new `memory_consolidate` operation with snake_case JSON. It accepts `content` for the proposed summary and `sources`, a list of 2–8 `{id, expected_content}` objects. Reject unknown properties, non-string values, blank IDs/summary, empty expected text, duplicate IDs, invalid source count, hidden text, and oversized summary/source text before mutation. Full whitespace-only legacy expected text stays exact. Reuse the existing memory tool content bound (8,000 characters per text field) unless the current descriptor establishes a stricter shared bound.
- This operation is a write. It requires both explicit per-agent `memory_consolidate` grant and the existing `WritePolicy` decision. Add it to the existing write-risk classification. Do not add it to any built-in access profile or default tool list.
- Add an optional SettingsPanel checkbox labeled “Allow consolidating saved facts.” It follows the existing tool grant patch behavior, preserves unrelated custom tool names, and remains off by default. Explicit selection of a standard access profile clears this optional grant; the owner can re-enable it afterward. Unrelated settings saves must not rewrite tools.
- The agent proposes summary content in the ordinary model turn. Do not make a second paid model/summarizer call or infer a source list from embeddings. Similarity is retrieval support, never authorization or a source selector.

### Exact source eligibility and resulting state

- Every source must be the current owner agent's visible `Fact`, durable (`world_id == None` and `session_id == None`), free of hidden text, and match full `expected_content` byte-for-byte with current canonical text. Check all sources and mutate them while holding the same memory write lock; a stale/missing/invalid source rejects the complete operation.
- All sources must have the same exact effective scope tuple (scope, room, world, session). A Room-scoped source must match the current message room. No cross-owner, cross-room, world/session, Reflection, Entity, or fuzzy/partial selection.
- Generate one `cg:<32 lowercase hex>` tag from an existing host UUID. The reusable core helper accepts 32 ASCII hex characters; the host always generates lowercase. Add that same 35-ASCII-character tag to the new summary Fact and every archived source. Preserve each source's existing tags and reject any source already at the existing 20-tag maximum before changing anything. This tag links a group; it is not proof of summary fidelity or a tamper-proof audit record.
- Create one new summary `Fact`; atomically reclassify each source `Fact` to `Reflection`. Preserve source IDs, complete original text, citations/evidence links, importance, timestamps, scope tuple, and existing tags. Add only the shared cohort tag to each source. Do not retarget citations, delete records, edit source text, or remove relationships blindly.
- Summary inherits the exact owner ID/name and source scope tuple, and the maximum importance among sources. Its tags are the normal runtime/tool provenance tags plus the shared cohort tag. It receives a new ID and current creation time. Do not add new persisted fields or migrate the store.
- Require 2–8 sources. Source content remains available in historical search/owner views as Reflection; this is active-Fact consolidation, not data erasure, disk compaction, or a promise that all historical references disappear.

### Exact duplicate `memory_add`

- Only apply idempotency to an incoming Fact with the existing default add scope tuple: private/default scope, no room, world, or session. Match only an own visible Fact whose full stored content exactly equals the already-trimmed incoming content.
- If duplicate own Facts already exist, choose the deterministic oldest record ID as the existing result; do not change its importance, tags, citations, timestamps, or scope and do not create another row. If no exact duplicate exists, preserve existing add behavior. Non-Fact add behavior stays unchanged.
- A paraphrase, case difference, or partial match remains a separate Fact. Never use embedding similarity, normalization, or a fuzzy duplicate threshold for writes.

## Hermes comparison and claim boundary

The pinned public Hermes `MemoryStore` implements exact duplicate add and `apply_batch` against a working copy, checking final budget and committing atomically; its text-file store also supports batch replace/remove, pinned reviewed entries, and a separate user-profile target. M34 reuses only the narrow idempotent-add and all-or-nothing consolidation intent. Anima's source contract is stronger and different: explicit source record IDs, full exact `expected_content`, same owner/scope cohort, archive-as-Reflection preserving citation IDs, and current per-tool grant plus `WritePolicy`. Do not claim whole-product parity or import Hermes file storage, tolerant substring matching, profile target, staging, or batch API.

## File ownership and responsibilities

- `packages/core-rust/crates/anima-memory/src/memory_manager/edit.rs` and its edit tests — narrow atomic helper that validates owner/type/current-content/cohort/tag capacity, creates the summary, reclassifies sources, and updates the text index in one in-memory mutation.
- `hosts/rust-daemon/src/tools/memory.rs` — parse/validate `memory_consolidate`, perform one write-locked `MemoryMutation`, persist once, and update embeddings only from canonical post-mutation state. Make exact default-scope Fact add return the deterministic existing match without mutation.
- `hosts/rust-daemon/src/tools.rs`, `tools/tests.rs`, and `approvals/policy.rs` — descriptor, executor, grouped runtime tests, and write-risk policy. No default grants.
- `apps/web/src/components/SettingsPanel.tsx` and `SettingsPanel.test.tsx` — optional grant checkbox, profile behavior, and save-patch preservation. Update the existing agent-access helper only if needed to exclude this optional grant from standard-profile derivation.
- No public HTTP schema, database migration, SDK DTO, broad memory API, or new provenance/citation schema.

## Test-first implementation sequence

### Task 1: Native RED at the existing registered tool executor

- Add one meaningful runtime test invoking `memory_consolidate` through the existing tool executor with a current owner and an explicit grant fixture, two valid same-scope Fact sources, and a neighboring Fact.
- Before production registration, assert the valid call succeeds and creates the summarized state; the existing unknown-tool result must make this assertion fail at runtime. Keep fixture construction and unrelated executor tests passing; do not create tests that refer to missing production symbols.
- Root observes and records RED before core/native implementation proceeds.

### Task 2: Add the core atomic state operation

- Write core tests first for a two-source success, source field preservation, Reflection type change, shared cohort tag, summary inheritance, and exactly one new summary.
- Add refusal tests for source count outside 2–8, duplicate IDs, wrong owner/type, stale full text, mismatched scope/room, non-durable world/session sources, hidden text, and source tags at capacity. Assert every refusal leaves the snapshot unchanged.
- Implement the narrow `anima-memory` helper in the existing edit module. Validate the entire batch before mutating any record; update Fact indexes and source indexes coherently. Do not widen owner patch/classification APIs.

### Task 3: Implement the native tool and idempotent add

- Parse only the bound snake_case shape, require explicit grant and existing Write policy, then lock canonical memory once and call the core helper through `MemoryMutation`.
- Persist once. Respect existing rollback behavior for ordinary pre-commit errors/cancellation and `SnapshotDurabilityUncertain` behavior for a committed-but-uncertain snapshot; never claim every returned error means no effect.
- After persistence, re-read canonical summary under the memory lock before embedding it, then follow existing memory-to-embeddings lock order. Source vectors may remain because the source text/IDs still exist; canonical Reflection type prevents automatic context from treating them as active Facts. Explicit search may still find those historical records.
- Add exact Fact-add tests for deterministic oldest duplicate choice, stable existing metadata/ID, no new record on duplicate, and non-idempotent paraphrase/case changes. Verify other memory types keep prior behavior.

### Task 4: Add the opt-in settings control

- Add the checkbox under the existing permissions UI. Reuse `AgentUpdateInput`/the current `tools` patch; add no endpoint or capability query.
- Verify default-off behavior, one-time opt-in, opt-out removal, mixed grants preservation, unrelated-save omission, explicit standard-profile reset, owner re-enable after reset, and save failure/reset/saving-state behavior. A failed save must leave the draft visible without implying the daemon grant changed.

### Task 5: Verify atomicity, provenance, and search behavior

- Native tests cover explicit grant and Allow/Ask/Deny policy, no default permission, stale source CAS, duplicate/malformed input, same-room enforcement, tag capacity, one persisted snapshot, and no success receipt on rejection.
- Core/native snapshot and persistence round-trip tests prove source IDs/text/citations/importance/timestamps/scope survive as Reflections, the one new Fact has the expected cohort/provenance, and unrelated neighbors stay unchanged. These are fixture-level restart-equivalent checks, not live provider or owner-data tests.
- Check canonical memory retrieval after archive so archived source Facts no longer appear as automatic durable Fact context; explicit historical search remains possible. Verify embedding upsert uses the canonical saved summary and no stale vector grants visibility.

## Acceptance and evidence

1. A valid explicitly granted two-source operation creates exactly one new summary Fact and archives the exact sources as Reflections atomically.
2. Source IDs, full content, citations/evidence references, importance, time, scope, and existing tags are preserved; only source type and cohort tag are added. Summary metadata follows the bound inheritance rules.
3. Every source is exact-current-text CAS checked against the same owner and scope under one write lock. Any invalid member rejects the whole operation without partial mutation.
4. Cohort tag length/value and 20-tag capacity are checked before mutation. No schema migration or citation retarget occurs.
5. Exact duplicate own default-scope Fact add is idempotent and stable; paraphrases and all other types preserve existing behavior.
6. The operation is unavailable by default and requires explicit per-agent grant plus the current write approval policy. Settings opt-in preserves unrelated grants and standard profiles never enable it.
7. Core/native/web tests, root-owned Rust daemon aggregate/lint/format checks, and relevant SettingsPanel tests pass in the current tree. No live model call is required.

## Five principal risks

1. **Evidence continuity:** archiving rather than deleting preserves source IDs and citations, but the summary's faithfulness is still model-proposed. Keep original evidence intact, associate it only with the discoverable cohort tag, and never describe the summary as verified.
2. **Concurrent edits:** exact text checks and updates must occur under one canonical write lock; do not split validation and mutation across lock acquisitions.
3. **Partial save or uncertain durability:** make one persisted mutation and honor `MemoryMutation`'s committed-but-uncertain case. Report uncertainty honestly and use canonical state before indexing.
4. **Scope or grant escalation:** enforce exact same-scope/current-room durable sources and explicit grant plus existing WritePolicy. Never auto-enable through defaults or profile presets.
5. **Destructive semantic overreach:** only explicit 2–8 source consolidation and byte-exact Fact add dedup. Do not auto-select sources, merge paraphrases, sweep old facts, or silently erase citations/relationships.

## Root-owned verification

Root runs the focused RED first, then the required Rust daemon test aggregate, lint, and format once after source freeze, plus SettingsPanel tests and relevant web typecheck/build. Use synthetic isolated fixtures only; do not inspect private profiles or delete real records, start services, invoke providers, or make account/API calls. Keep unchanged Nx configuration untouched and publish only explicit owned source/docs paths under the standing unsigned-reviewed merge authority.

## Controller rulings on the risks (binding)

1. **Preserve original evidence:** Create a NEW summary Fact and archive each source
   as Reflection, retaining its exact ID/content/citations/importance/time/scope
   and existing tags. Append one host-generated `cg:<32hex>` group tag; reject
   lack of tag capacity first. Never retarget evidence to a model summary. Tags
   link discoverable records, not authenticated audit or proof of fidelity.
   Cost if wrong: this reduces active Facts, not total storage; history can be
   searched, edited, owner-deleted or later pruned by existing retention.
2. **CAS and scope:** The reusable core entry is
   `consolidate_facts(agent_id, room_id, sources, content, group_tag, visible)`.
   `MemoryConsolidationSource` has `id`/`expected_content`; result
   `MemoryConsolidation` has `summary`/`archived_ids`; a small explicit error
   type exposes safe messages. Validate all2–8 unique bounded IDs/texts, own
   visible Fact, durable world/sessionNone, identical scope/room tuple, current
   room for Room sources, exact full text and tag capacity before any mutation.
   Summary uses first source's stored owner name, exact scope and maximum valid
   importance. Cost if wrong: content equality does not detect ABA or metadata
   revision, but current metadata eligibility is revalidated under one write lock.
3. **One durable operation:** Core validates then adds/reclassifies/reindexes.
   Host wraps one MemoryMutation under one write lock and persists once; no
   intermediate saves. Preserve ordinary rollback and committed-durability-
   uncertain semantics. Re-read canonical saved summary before best-effort index
   upsert in memory→embeddings order. Source vectors remain accurate original
   text; Fact-only context rejects archived types. Cost if wrong: indexing can
   lag canonical state and an uncertainty error can accompany committed effects;
   no automatic retry or no-effect claim.
4. **Explicit authority and exact dedup:** New tool requires an explicit grant
   and existing Write policy. Standard profiles/defaults/helper grants stay
   unchanged; Settings independently controls remove/consolidate and preserves
   unrelated grants. Core `find_exact_default_fact(agent_id, content, visible)`
   returns the oldest created_at then ID matching own visible private Fact with
   no room/world/session and byte-exact content. Native add checks under its
   write lock after existing argument validation, returns existing metadata
   unchanged and skips persistence/index mutation on duplicate. Cost if wrong:
   paraphrases/case differences remain separate; no semantic merging is inferred.
5. **Evidence and delivery:** Native existing-executor RED must fail a Success
   assertion for a valid explicitly granted request before production. Disjoint
   core/native/web workers, one native batch plus final SDK/web review, one fix
   wave then fresh required Rust/lint/web gates. Tests use synthetic records,
   provenance round-trip and approval checks; no real data/services/account or
   paid summarizer. Preserve full public Hermes MIT attribution and Anima UI.
   Cost if wrong: faithful model-written summaries and live consolidation/restart
   behavior still require separate acceptance; no whole-product parity claim.

## Current verification and publication

Meaningful native RED reached the existing explicitly granted executor and failed
Error/Unknown tool versus Success before production. Web RED failed the actual
missing accessible checkbox; its first sandbox cache-rename failure ran no tests
and is recorded separately. Four core, seven native and one policy grouped tests
now cover whole-batch CAS/refusal, source/citation/entity preservation and reload,
active-Fact/history/index behavior, all scopes and count/tag/Unicode boundaries,
rollback, concurrent one-winner consolidation, exact-add no-op and concurrency,
strict schema, explicit grant and existing Allow/Ask/Deny policy. Four added web
cases plus extended controls cover optional grant combinations, profile reset,
re-enable, unrelated save and failed-save draft retention.

One independent native/core batch review was clean. Final independent SDK/web
review found one P2 permission-copy issue; one fix wave explains the actual saved
summary and retained memory history. Initial Rust gates then found three fixture
errors: outdated registry count, U+200B incorrectly treated as blocked despite an
explicit existing acceptance test, and raw rather than restored snapshot state.
Diagnosed test-only corrections preserve the production text policy and all
refusal assertions; failed evidence remains in the ledger. The fresh required
rerun passes **2,453 Rust tests / nine ignored / 40 suites**, followed by sequential
lint. Web **1,205 tests / 110 files**, typecheck/build and nine transitive tasks
pass. Eleven source hashes match the final gate snapshot; unchanged web source
keeps its current gate evidence after the Rust-test-only correction.

No schema/dependency/default-grant/profile changes or actual records/services/
accounts/containers/paid model calls were used. Snapshot reload is fixture-level
restart-equivalent evidence, not an ordinary live model consolidation/restart
run. Summary faithfulness, broad semantic quality and autonomous maintenance
remain separate acceptance work. Publish owned source/docs/full MIT notices only,
record exact tested source/PR/merge in the milestone ledger, and continue with one
bounded isolated real-model memory acceptance if the already-authorized synthetic
profile is available without importing or reading ordinary private credentials.
