# M40 owner-selected canonical memory views — implementation plan

> Root binds interfaces and observes meaningful RED after verified M39 merge.
> **Goal:** show selected saved facts as user-profile/agent-note views and prove a
> corrected preference reaches both generic and relevant fresh conversations.
> **Architecture:** one canonical MemoryManager; persist only selected source IDs,
> derive displayed/context text from current eligible records, reuse memory CAS.
> **State:** implemented and corrected-preference effect locally accepted. Current Rust2511/9ignored/40suites/lint, SDK131 and web1240/type/build pass. Two fresh replies/one restart/1296 reported tokens; the second reply misread whom to introduce, so overall introduction-task quality is unproven. Exact RED/gates/acceptance/publication are in the M40 ledger.

## Scope and source seams

- PAR-02 explicitly queues selected source-backed views and fresh preference reuse.
- M39 covers procedural skill handoff/reuse; this does not reopen its gate or calls.
- Exclude automatic dialogue learning, unattended curation, new identity instructions,
  Markdown synchronization/import/export, personal Hermes folders and model scouting.
- Existing canonical Memory has ID, owning agent, type, exact content, tags, importance,
  createdAt and scope/room/world/session; it has no edit version or updatedAt field.
- MemoryType::Fact differs from TemporalFact. TemporalFact evidenceMemoryIds exists,
  but TemporalFacts have no direct owning-agent field; do not silently mix stores.
- Start with own visible durable PRIVATE MemoryType::Fact, room/world/session absent.
- “Source-backed” initially means exact canonical record ID/hash and any existing
  citation evidence; do not invent a source message or imply independent verification.
- Owner-attested content is labelled saved fact; model-origin content remains evidence,
  not an instruction, inferred identity or universally true preference.

| Existing seam            | Current source                               | Reuse/change                                             |
| ------------------------ | -------------------------------------------- | -------------------------------------------------------- |
| Canonical fields         | anima-memory/memory_manager/types.rs:353,367 | No new content store                                     |
| Relevant selector        | anima-memory/memory_manager.rs:102,112       | Preserve M33 lexical/vector ranking                      |
| Context/render bounds    | components/providers.rs:20,26,34,880         | Quoted current eligible records                          |
| Exact scoped update      | tools/memory.rs:321,350                      | Full expected-content CAS + MemoryMutation               |
| Owner update             | routes/memory_edits.rs:46,383                | Reuse persistence/embedding order; add guarded view path |
| Owner update DTO         | routes/contracts/memory_edits.rs:12          | Currently no expectedContent field                       |
| Memory owner DTO         | routes/contracts/memories.rs:24              | Existing full text/scope/ID fields                       |
| Provider snapshot timing | anima-core/runtime.rs:468,1432               | Once per run, reused across model/tool rounds            |

Core paths are under packages/core-rust/crates/; native paths under
hosts/rust-daemon/src/. Anchors are planning snapshot, recheck before implementation.

## Smallest proposed interfaces

- Candidate native module memory_views.rs: selection validation, rendering and CAS glue.
- Selection record per live agent: {version, userProfileIds[], agentNoteIds[]}.
- Persist references only in existing control-plane snapshot; canonical MemoryStore
  remains the sole content authority. No copied profile text, embeddings or facts.
- Alternative managed tags is cheaper structurally but existing model tools can write
  arbitrary tags: owner-only selection cannot trust tags without widening all writers.
- Recommend reference-only selection instead; root confirms this is configuration,
  not a second canonical memory store, before RED.
- GET /api/agents/{id}/memory-views returns version, computed viewDigest,
  userProfile/agentNotes arrays and explicit missing/ineligible source diagnostics.
- Each rendered source: id, full content and sourceHash. The console labels these
  saved sources as unverified; no synthetic updatedAt or “verified” boolean.
- PATCH same route: {expectedVersion,userProfile:[{id,sourceHash}],agentNotes:[...]}.
- Strict fields; IDs exact and bounded; sourceHash hashes full authority-bearing
  current fields, including agent/type/content/scope/room/world/session/tags.
- Selection version is server-owned CAS, not a browser counter or createdAt alias.
- Match all requested source hashes under canonical read protection before commit;
  changed/missing/foreign/hidden/Reflection/non-private source refuses fail-closed.
- Selection mutation writes no Fact content and changes no tool grant or identity.
- Correction candidate: PATCH /api/agents/{id}/memory-views/sources/{memoryId}
  {expectedVersion,expectedContent,expectedSourceHash,content}; strict full current text.
- Reuse the existing update/persist/index helper under one memory write lock, with
  added own selected-Fact predicates; no second MemoryMutation implementation.
- Existing owner PATCH is unguarded today; do not claim UI already offers update CAS.
- Existing generic owner APIs retain compatibility; model tools cannot select sources.
- Responses are owner-authenticated/no-store; auth before parsing; 400 invalid,
  generic404 missing/foreign,409 stale version/hash/text,503 failed durability.
- Duplicate exact selection can return unchanged record/version; stale version cannot
  become an unconditional replacement or automatic retry.

## Current fields, invalidation and context rules

- Selection stores ID membership, not content; explicit correction keeps membership
  and next derived view reads current text/hash. No stale materialized text copy.
- Canonical deletion/type/scope/visibility change makes a selected source unavailable;
  omit it from prompt and expose bounded owner diagnostic, never resolve another ID.
- Snapshot restore keeps selection references/version; missing-agent selections drop.
- Bound total selected sources to six across both views, each ID at most256 bytes,
  each excerpt at most800 characters, total rendered context at most4800 characters.
- Owner sees full canonical source in editor; excerpts are never exact edit input.
- Use fixed deterministic display order (owner array order), reject duplicate IDs
  across both views; retain category distinctions without duplicate prompt text.
- Include selected eligible sources independent of current query overlap so generic
  conversations can see an explicitly selected preference.
- Compose selected sources before relevant M33 results, dedup IDs/normalized content,
  and keep the same TOTAL six-record/4800-character budget, not two unbounded blocks.
- Existing semantic lookup remains one query, two worker permits and five-second
  result deadline; no repeated embedding, oversampling or model call for profiles.
- Do not alter M33 ranking of remaining ordinary retrieval candidates or M36
  current-text vector fingerprint eligibility; stale vectors remain excluded.
- Runtime builds providers once at run start: profile/notes snapshot is frozen for
  that invocation including retries/tool rounds; fresh next owner turn reads current
  canonical source/selection. Do not claim Hermes's session-long freeze parity.
- Quoted view text cannot grant tools or override current request/approval policy.
- Missing/failed profile reads degrade explicitly to available relevant context;
  no fallback to personal directories, another agent or stale exported Markdown.

## Ownership and minimal sequence

1. Root: bind five risks, cut branch only after M39 verified merge, refresh field/API
   shapes, observe existing-boundary RED demonstrating absent selected generic context.
2. Native worker: memory_views.rs, owner DTO/routes/tests, reference-only snapshot
   validation/restore; isolated ownership for shared state/coordinator edits.
3. Context worker: components/providers.rs and compact actual-provider tests;
   core selector changes only if a demonstrated reusable gap requires them.
4. SDK/web worker: additive selected-source API and Memory page view/editor controls;
   source content/hash at click time, preserve edits on conflict and deliberate reconfirm.
5. Reuse existing memory update persistence, detached cancellation-safe settlement,
   lock order, definite rollback and uncertain-durability canonical retention.
6. Selection and correction are separate operations/stores with honest outcomes:
   do not promise atomic rollback across control-plane and MemoryStore commits.
7. No hidden background generation or whole-transcript/profile clone.

## Compact meaningful tests

- Positive select two own canonical Facts into distinct views; rendered source IDs,
  hashes/category/order exact, all content/citations/grants unchanged by selection.
- Full field eligibility: foreign, hidden, Room/Shared, world/session scoped,
  Reflection/missing and duplicate records refuse without scope/content leakage.
- Stale sourceHash/selectionVersion/full-text CAS with queued concurrent correction;
  no partial batch selection or overwrite, no automatic browser retry.
- Correction preserves ID/citations/tags/scope/neighbors, updates canonical lexical
  lookup and M36 current-text embeddings; stale old-vector result cannot reintroduce it.
- Actual RecentMemoriesProvider receives selected preference for a generic prompt
  with no lexical overlap, amid high-score irrelevant distractors and stale vectors.
- Old source text absent after correction; exact prefix/render limits and duplicate
  suppression proven; existing M33 lexical/semantic fallback fixtures preserved.
- Provider invocation freezes one snapshot; next fresh invocation sees correction;
  do not assert a mutable tool-round provider refresh that core does not perform.
- Save failure/uncertainty, cancellation, restart and removed-source behavior retain
  truthful state; no grant/default opt-in change or replay/model request.
- SDK strict CAS/null/optional compatibility; UI save conflict retains edits,
  delayed prior-agent response is ignored and selected source links remain exact.

## Two bounded real acceptance trials

- Root seeds synthetic owner-attested preference + neighbor + small distractor set,
  selects profile/notes, explicitly corrects old preference through guarded API.
- Trial A: one fresh empty generic chat (e.g. Spanish request for a short friendly
  introduction) independently checks the corrected preference's declared observable
  effect; do not disclose the answer in the prompt or count merely quoted source text.
- Trial B: one fresh related practical task independently checks useful application,
  e.g. a concise requested summary uses the selected formatting preference correctly.
- Declare expected outputs before calls; exactly two requests by default, bounded
  steps/time/reported tokens and tools-none where task allows, no helpers/scout/retries.
- Record actual provider context IDs/current hashes, outputs, usage/unknown usage,
  untouched neighbors/grants and empty history prerequisites; receipt success is not
  a semantic oracle. If quality fails, report incomplete rather than repeat for a pass.
- One quiet same-store restart retains selections and canonical correction; no paid
  verification task, automatic replay or imported profile is needed for durability.

## Reuse decision/provenance

- Pinned public Hermes1fd18277267152893a8a24792b79213e7ca3f899, MIT/Nous Research2025:
  tools/memory_tool.py:2 explains MEMORY.md notes + USER.md profile/session freeze;
  \_pin_matched_entries pins full reviewed entries, apply_memory_pending applies a
  staged approved write; memory_tool_store.py:141,188,524 implements disk snapshot.
- Direct Python/process adapter would duplicate current canonical content, profiles,
  authority and mutation lifecycle. Managed Markdown mirrors risk conflicting truth.
- Lowest-cost choice: existing Anima canonical/CAS/provider machinery plus small
  reference selection and quoted view glue; preserve run-level freeze deliberately.
- No wholesale upstream copy; substantial adapted helper must retain exact source,
  pin/license/local changes notice. No personal Hermes files are read or imported.

## Top five candidates for root binding

1. Canonical truth: selection references only, not text; Fact versus TemporalFact and
   owner-attested versus genuinely cited provenance labels stay explicit.
2. Authority/scope: owner-only selection, private durable eligible sources, protected
   CAS and no tag-based self-selection or new grants/default curation.
3. Freshness: full sourceHash + exact content + selection version, omit invalid IDs;
   run-frozen context refreshes next invocation, never a second stale profile cache.
4. Bounds: shared six/4800 render budget and unchanged M33/M36 inference limits;
   selected facts do not silently expand profile/context/model cost.
5. Semantic proof: generic and relevant trials independently verify corrected effect;
   two planned calls, no fabricated original-message provenance or broad PAR-02 claim.

## Root-only commands and claim boundary

- Focused actual-owner/provider RED/GREEN, then rust-daemon Nx aggregate test + lint.
- Relevant SDK/web Nx tests/typecheck/build with existing service-test skips.
- Root owns formatting, one native batch/final SDK-web review, justified fix wave,
  bounded actual trials, cleanup, docs and explicit-path publication; preserve nx.json.
- Exact current verification, acceptance and publication are recorded in the ledger.

## M40 frozen native interfaces

- Scope active after M39 merged33c4608.
- Own module after root RED: hosts/rust-daemon/src/memory_views.rs only.
- All types/functions below are pub(crate); DTOs camelCase serde + ToSchema.
- MemoryViewSelection {agent_id:String,version:u64,user_profile_ids:Vec<String>,
  agent_note_ids:Vec<String>} derives Clone,Debug,PartialEq,Eq,Serialize,Deserialize.
- MemoryViewRegistry {rows:BTreeMap<String,MemoryViewSelection>} Clone/Default;
  rows visible to root for single transaction staged save/rollback.
- SharedMemoryViewSelections = Arc<std::sync::RwLock<BTreeMap<String,MemoryViewSelection>>>.
- MemoryViewSource {id:String,content:String,source_hash:String} derives Clone/Serialize/ToSchema.
- MemoryViewUnavailable {id:String,reason:String}: bounded generic diagnostic;
  foreign/missing/hidden/ineligible share unavailable, never quote inaccessible content.
- MemoryViewResponse {version:u64,view_digest:String,user_profile:Vec<MemoryViewSource>,
  agent_notes:Vec<MemoryViewSource>,unavailable:Vec<MemoryViewUnavailable>}.
- MemoryViewPick {id:String,source_hash:String}: strict Deserialize/ToSchema.
- MemoryViewPatchRequest {expected_version:u64,user_profile:Vec<MemoryViewPick>,
  agent_notes:Vec<MemoryViewPick>}: strict Deserialize/ToSchema, all fields required.
- MemoryViewCorrectionRequest {expected_version:u64,expected_content:String,
  expected_source_hash:String,content:String}: strict Deserialize/ToSchema.
- eligible_source(memory:&Memory,agent_id:&str)->bool: own visible private Fact,
  no room/world/session, ID<=256bytes, nonblank content<=8000chars and hidden checks.
- source_hash(memory:&Memory)->String: SHA256 exact full canonical fields, including
  ID/agent/name/type/content/importance/createdAt/tags/scope/room/world/session;
  deterministic full serde JSON representation, no trimmed or partial text hash.
- MemoryViewRegistry::snapshot(&self)->Vec<MemoryViewSelection> returns stable rows.
- MemoryViewRegistry::validate(rows:&[MemoryViewSelection])->Result<(),String>:
  <=1000 rows, unique bounded agent IDs, <=6 total selected IDs per row, duplicates
  across categories refused, IDs nonempty<=256bytes; version0 only empty default.
- MemoryViewRegistry::restored(rows:Vec<MemoryViewSelection>,live:&HashSet<String>)->Self:
  retain valid live-agent rows; no content clone or automatic source replacement.
- MemoryViewRegistry::selection(&self,agent:&str)->MemoryViewSelection returns a
  cloned existing row or empty version0 default for that agent.
- MemoryViewRegistry::published(&self)->SharedMemoryViewSelections initializes a
  separate leaf map from acknowledged restored rows; no live canonical text.
- MemoryViewService::new(state:SharedDaemonState,transactions:Arc<tokio::sync::Mutex<()>>)->Self;
  Clone; root fields memory_views:MemoryViewRegistry and
  memory_view_selections:SharedMemoryViewSelections.
- get(&self,agent:&str)->Result<MemoryViewResponse,ApiError>: registry selection
  cloned under the shared transaction, current canonical eligible sources copied,
  deterministic viewDigest. Missing MemoryStore retains version/unavailable references.
- candidates(&self,agent:&str,query:&str)->Result<(Vec<MemoryViewSource>,bool),ApiError>:
  current eligible sources sorted newest createdAt/ID, at most50 plus hasMore;
  optional substring query at most512 characters; no unrestricted transcript read.
- select(&self,agent:&str,input:MemoryViewPatchRequest)->Result<MemoryViewResponse,ApiError>:
  detached shared transaction; copy/validate canonical picks while memory read held,
  release it BEFORE state stage/snapshot/save; admission linearizes at validated read.
- Exact repeated selection preserves version; changed selection checked_add version.
- On save error roll back staged registry and retain old published map; no profile
  publication. On acknowledgement replace leaf rows; do not hold state+memory locks.
- Root correction route holds same transaction, checks selection version/membership,
  source hash/full expected text and existing MemoryMutation under memory write lock.
- Root owns state/snapshot/factory/HTTP/correction; service never invokes a model,
  writes profile files or makes a second canonical content cache.
- Published leaf poisoning is fail-closed: context omits selections and owner
  GET/selection writes return503. GET otherwise derives from the registry.

## Controller rulings on the risks (binding)

1. **Canonical authority:** store only source ID lists and selection version. Current canonical private Fact text/metadata is the sole content authority. Fact and TemporalFact stay distinct; displayed source ID/hash is canonical lineage, not invented original-message verification. Standard Markdown synchronization and unattended curation remain outside this wave.
2. **Scope and publication:** owner-only strict selection/correction; six total current own clean unscoped private Facts, IDs256 bytes and content8000 characters. Staged selection rows persist under the existing shared transaction; the provider's leaf reference handle publishes only after acknowledged save. Runtime reads clone that handle without awaiting under its lock. Older selections remain on reported failure; existing uncertain-durability limits are stated.
3. **Freshness and locks:** sourceHash covers all canonical fields; ranked result scores are excluded; selection version pins configuration, not content ABA. Validate/hash/copy under canonical read protection, then release before state/snapshot staging; that validated read is selection's logical admission point. No simultaneous state/canonical guards. Correction pins selection version plus full source hash/content under one canonical write mutation/save; it cannot overwrite a concurrent edit. Selection and canonical correction are distinct commits, without cross-store rollback claims.
4. **Context budget:** resolve published IDs to eligible current records once per invocation; owner order first, remaining relevance fills spare slots with ID/content dedup before ranking. Share the existing six-record/4800-character budget and800-character quoted excerpts. Keep M33/M36 current-text vector eligibility, inference deadline/permits and no-selection behavior. Run context is frozen for that invocation; the next invocation refreshes canonical content.
5. **Proof and reuse:** reuse current canonical MemoryMutation/save/index/selector/UI, informed by pinned MIT Hermes full-entry pinning and profile/notes behavior; a Python or mirrored-file adapter introduces conflicting content/authority. Two predeclared fresh tools-free actual tasks independently check the effect of an explicitly corrected selected preference in a generic conversation and a relevant task. No scout/probe/retry/repair calls, helpers or default policy/grant changes. Failure records quality unproven rather than repeating for a pass.

## Frozen interface adjustments

Candidates GET uses optional bounded512-character substring query and returns {candidates,hasMore}, maximum50, deterministic newest/ID order; hashes derive from canonical rows with ranking score excluded. UI heading is Profile & agent notes inside existing About you, separate from temporal facts. Selection uses ordered id/sourceHash picks and expectedVersion; correction additionally requires expectedVersion, full expectedContent/expectedSourceHash and content. Response has version/viewDigest/userProfile/agentNotes/unavailable with generic unavailable reasons. Existing generic memory editing behavior stays compatible.
