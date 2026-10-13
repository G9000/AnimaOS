# Learned Semantic Memory Context Implementation Plan

> **For agentic workers:** Use this plan task-by-task with test-first changes and review checkpoints. Root owns Cargo/Nx gates, branch integration, and publication.

**Goal:** Inject a small, relevant set of the current agent's visible durable facts into each run using the configured embedding backend, with lexical fallback and bounded behavior.

**Architecture:** Keep `RecentMemoriesProvider` as the sole context provider and extend its inputs with the shared embedding runtime. Extend the existing `MemoryManager::select_context` through a portable vector-aware entry, reusing the filtered vector index and exactly the existing automatic-context eligibility rules. Keep explicit hybrid memory_search unchanged. Core builds provider context once before the run loop, so the selected evidence remains frozen for that run. Do not change memory storage, tool grants, run DTOs, events, or model-provider behavior.

**Tech Stack:** Rust daemon, `anima-memory` recall API, existing `MemoryEmbeddingRuntime` backends (local default, operator-configured FastEmbed E5 or existing compatible providers), Tokio bounded blocking work, existing Nx Rust-daemon targets.

---

## Binding behavior

- Retrieve at most 6 records, with at most 800 Unicode scalar values per excerpt and 4,800 total rendered characters. Keep the existing JSON-quoted, injection-resistant format and guidance that excerpts are not exact edit input.
- Candidate records must be the current agent's visible durable Facts and pass existing scope rules: global facts and private/current-room facts may qualify; foreign-agent, hidden-text, Reflection, Entity, Task, session-only, and other-room records must not enter the candidate set. Recheck canonical record eligibility while holding the memory read lock; an embedding index ID alone is never authority.
- Use one bounded query embedding per run-provider call through the existing filtered vector index. Preserve the existing lexical selection and order, then fill remaining context slots with eligible finite positive vector matches in deterministic raw-score order, deduplicating context content. Do not score tags, recency or relationships as relevance in this entry. If embeddings are disabled, unavailable, fail, time out, or cannot obtain a bounded worker slot, return the existing bounded lexical selection.
- Keep the default local embedding configuration unchanged. FastEmbed's public multilingual E5 model is explicitly operator-selected for learned acceptance; no normal boot downloads or silently changes configured models. Existing compatible remote embedding modes remain optional and may add one embedding request per run, never an extra chat completion.
- The provider is called once before the model loop and context is frozen for that run. Do not cache across runs, stream memory mutations into an active run, or add snapshots/events/metadata beyond the existing provider result.
- Bound potentially blocking embedding work: at most two admitted workers, five-second deadline for the provider result, lexical fallback when admission/deadline fails. A timed-out worker retains its permit until it actually exits; discard late results. Do not block the core async event loop or hold the daemon memory write lock across embedding work.
- Do not claim semantic quality from deterministic local aliases, synthetic vectors, or positive cosine alone. Learned acceptance must show the intended fact ranks above declared distractors and is actually rendered for predeclared paraphrase queries, including a multilingual query, while excluded records remain absent.

## Public Hermes comparison (pinned public cache only)

| Behavior                  | Public Hermes helper at pin `1fd18277267152893a8a24792b79213e7ca3f899`                                                                                                                     | Anima M33 target                                                                                                                                                  |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Persistent memory context | `MemoryStore` loads curated memory entries and freezes a system-prompt snapshot; bounded character budgets apply to writes/context.                                                        | Reuse Anima's canonical, scoped Fact store; select relevant records per run and preserve current 6/800/4,800 context bounds.                                      |
| Retrieval                 | Hermes' cached tool/store path is curated-entry context rather than this daemon's filtered learned-vector recall. This plan makes no claim of matching every Hermes memory feature.        | Add real learned semantic ranking to the automatic context already supplied by Anima; retain lexical fallback and strict visibility.                              |
| Consolidation             | Hermes `apply_batch` stages a working copy, supports exact whole-entry replacement/removal semantics, pins reviewed entries, enforces whole-result budget, and commits a batch atomically. | Out of scope for M33. M34 is planned separately for bounded model-authored Fact consolidation with exact source CAS and provenance decisions.                     |
| Safety/authority          | Hermes has target-specific enablement, threat checks, staging behavior, and per-turn consolidation failure limits in its own host.                                                         | Do not port its user-profile target, file storage, batch API, staging workflow, or grant behavior. Keep Anima's existing approval and canonical memory authority. |

This is a narrow comparison against the identified public helpers, not a whole-product parity claim. M33 acceptance means bounded, learned, scope-correct automatic Fact context, not equivalent memory UI, storage, editing, or consolidation.

## Files and responsibilities

- `hosts/rust-daemon/src/components/providers.rs` — add the embedding handle and semantic selection/fallback to `RecentMemoriesProvider`; preserve rendering and candidate visibility.
- `hosts/rust-daemon/src/components.rs` — pass the existing shared memory-embedding runtime through `default_providers`.
- `hosts/rust-daemon/src/state.rs` — wire the shared runtime for created/restored ordinary runtimes.
- `hosts/rust-daemon/src/state/swarm_runtime.rs` — pass it into coordinator/participant runtimes.
- `hosts/rust-daemon/src/state/run_commit.rs` — pass it into the run runtime and summary-provider construction without changing the summary provider.
- `hosts/rust-daemon/src/memory_embeddings.rs` — only if needed for focused test access or reusable bounded-worker support; preserve existing backend behavior and model settings.
- Existing provider, embedding, and runtime tests in those files — add synthetic ranking/visibility/fallback/bounds and a real learned retrieval acceptance test.
- No SDK/web/API/DTO, database schema, tool grant, memory mutation, or public endpoint changes.

## TDD implementation tasks

### Task 1: Prove provider retrieval with an injected filtered index

- Add a provider regression with a query phrased as a paraphrase of a stored Fact that lexical overlap does not select. Provide deterministic test embeddings/vector hits, then assert the actual Fact ID/text is rendered.
- Add controls proving the nearest foreign-agent, other-room, hidden-tag, and non-Fact records are excluded even when their indexed vectors rank highest. Include one eligible private/current-room Fact and one global Fact.
- Preserve old lexical behavior: when no vector index is available or the query yields no vector result, the existing matching Fact is still selected; unrelated query still renders `no relevant memories`.
- Run the focused provider test and observe the expected failure before implementation.

### Task 2: Wire the existing runtime into every provider construction path

- Extend `default_providers` to take the existing `SharedMemoryEmbeddings` handle and pass it into `RecentMemoriesProvider`.
- Update create/restore paths, the `commit_run` runtime copy and summary-provider case, and `build_swarm_runtime`; ensure ordinary, resumed, summary, coordinator, and swarm member runs receive the same configured backend.
- Compile the targeted daemon tests to catch missed construction sites. Do not initialize a second model/index per runtime.

### Task 3: Implement bounded semantic selection and safe fallback

- Under the memory read lock, resolve candidate IDs against canonical records and the same scope/visibility predicate used by current context selection. Use the vector-aware context selector with the existing six-record limit.
- Run inference from a cloned query backend through the bounded worker mechanism with no canonical/index guards. After inference, acquire current memory then index under the remaining deadline and use a precomputed-vector view; never retain a daemon write guard. Do not start an unbounded blocking task for each run.
- On disabled backend, admission denial, backend error, timeout, or late completion, use the existing lexical selection. Keep final formatting/size truncation unchanged.
- Verify selected results are deterministic for tied scores and no candidate outside the canonical allowed set appears.

### Task 4: Verify real learned retrieval, not just vector plumbing

- Extend the ignored acceptance fixture in `memory_embeddings.rs` with a declared Fact, at least eight varied distractors, two paraphrase queries including a multilingual query, and excluded high-ranking foreign/hidden candidates.
- Assert learned filtered ranking places the target above every distractor for each query, and run the actual `RecentMemoriesProvider` to assert the target Fact is present in rendered context and excluded candidate IDs/text are absent.
- Keep the asset cache isolated under the existing temporary model-test path. Run only when the public model is available or approved for download; record model/provider and observed ranking outcome. This case supplements, but does not replace, deterministic unit tests.
- A pass proves these declared retrieval cases only. It does not establish universal semantic quality, production latency, remote-provider reliability, or parity across all Hermes behaviors.

### Task 5: Verify bounds and compatibility

- Add tests for six-record cap, 800-character excerpts, 4,800-character output cap, JSON escaping, hidden text, no-vector/disabled/error fallback, and timeout/admission fallback. Verify a timed-out worker cannot exceed the two-active-worker bound and its late result is ignored.
- Run focused `RecentMemoriesProvider` and `MemoryEmbeddingRuntime` tests first. Root then runs the required Rust daemon aggregate gate and formatting/lint once after core/native work is ready.
- Do not run services, paid chat providers, APIs, or ordinary configured profiles for this milestone.

## Acceptance and evidence

1. Semantic paraphrase selects and renders the expected stored Fact; the learned test includes multilingual ranking and explicit distractors.
2. Canonical visibility/scope checks exclude foreign, hidden, other-room, and non-Fact records even if vector rank is higher.
3. Existing lexical matches remain available when semantic lookup is unavailable; rendering bounds and instruction-quoted format remain unchanged.
4. Provider context is selected once per run, does not change tool dispatch or mutate memory, and every runtime construction route has the configured shared backend.
5. Default local startup remains download-free. Learned backend use is explicit and reports one configured embedding lookup per run; no additional paid generation call is introduced.
6. Focused tests plus root-owned `bun x nx run rust-daemon:test --skipNxCache` and required lint/format checks pass in the final tree. Tests of the actual provider/runtime are preferred over vector-only assertions.

## Five principal risks

1. **Visibility drift:** candidate selection can accidentally rely on a vector ID. Resolve canonical records and apply the existing filter under the memory read lock; keep adversarial high-ranked excluded records in tests.
2. **Blocking the run loop:** some configured embedding backends block or wait on network. Bound admission and result deadline, preserve permit ownership until worker exit, and fall back lexically.
3. **Misleading semantic claims:** a tiny learned fixture is not general quality evidence. Predeclare queries and distractors, prove ranking and actual provider injection, and limit the claim to those cases.
4. **Default startup cost:** silently selecting FastEmbed could trigger model download/init for existing operators. Keep default local mode and require explicit backend configuration for learned acceptance.
5. **Cost and staleness:** configured remote embeddings may incur one query request per run, and the selected context remains frozen through that run. State both plainly; do not cache across runs or mutate in-flight context.

## Root-owned focused commands

Run focused tests through the existing Nx daemon test target with project-supported filtering as confirmed by `--help`; do not guess flags. Run the ignored learned acceptance through the existing Cargo test target and Cargo's documented `-- --ignored` separator, using the isolated cache. After all source is frozen, root runs `bun x nx run rust-daemon:test --skipNxCache` and the required daemon lint/format checks once. Record exact command/output and model availability; do not claim completion from a synthetic test alone.

## Controller rulings on the risks (binding)

1. **Canonical authority:** Add portable `select_context_with_vector_index(opts, Option<&dyn MemoryVectorIndex>, visible)`; old `select_context` delegates with None. Resolve every candidate against canonical own Fact, world/session None, exact current-room rule and caller visibility before native vector top-k; defensive postfilter remains mandatory. Preserve lexical selection/order exactly, then fill available slots using finite positive raw vector-score order, deterministic importance/time/ID ties and existing normalized-content dedup. No recency-only or relationship fallback. Cost if wrong: unrelated positive similarities can still fill spare slots; model similarity is evidence retrieval, never truth or calibrated confidence.
2. **Bounded offload:** A shared two-permit admission cap and one inclusive five-second deadline cover lock admission and blocking lookup. A worker owns its permit until exit, late results are discarded, and caller cancellation cannot release a still-running worker's permit. Read locks follow memory then embeddings; no write lock. Lexical fallback must not wait behind a worker-held read lock, so acquire a bounded lexical snapshot before offload and reuse it on timeout/admission failure. Cost if wrong: slow configured embedding providers can keep the two slots occupied until their own timeout, degrading to lexical context without unbounded thread growth.
3. **Quality/evidence:** Real-model RED already passed declared learned ranking/exclusion prerequisites and failed actual provider inclusion. Keep those fixed cases; both English/Spanish queries must rank the target above varied own distractors and render it after implementation. Ordinary gates retain deterministic boundary/fallback tests; run ignored real learned fixture explicitly. Cost if wrong: these two cases establish bounded learned recall, not universal semantic quality or summary faithfulness.
4. **Configuration/cost:** Dependency restriction revoked. Existing fastembed is sufficient for M33; keep configured backend and default local, document fastembed as the learned option. No ordinary profiles/account calls/model switch. At most one query request per provider/run, frozen naturally by existing core run loop. Cost if wrong: remote embeddings incur one configured provider call and cold learned initialization remains an operator concern.
5. **Delivery/reuse:** Source workers own disjoint core/native/test paths; root owns serialized Cargo/Nx/Git. One native batch and final SDK/web contract review, one justified fix wave then aggregate/lint/real learned gates and explicit-path publication under standing merge authority. Preserve upstream MIT notices and Anima product name. M34 follows from merged main and is not omitted. Cost if wrong: release claims must distinguish this semantic-context milestone from pending consolidation and whole-Hermes parity.

### Review refinement (same binding rulings, one fix wave)

Inference must run from a cloned configured query backend without canonical-store
or index guards. After it completes, acquire current canonical memory then index
under the remaining deadline and rank the precomputed vector without another
embedding request. This prevents slow or stuck inference from retaining locks
needed by memory edits after caller fallback/cancellation. Worker permits remain
held until real completion. Already-selected lexical content is excluded from
native vector eligibility before top-k; bounded external legacy shortlists and
pure-semantic duplicate groups retain their documented coverage limits.

## Current verification and publication

The ignored real learned test first passed both fixed-language ranking prerequisites
and then failed at actual context omission. Final source passes the same English
and Spanish ranking/injection checks with genuine cached multilingual E5 weights:
one test passed, 1,573 filtered, 2.41 seconds. No paid chat generation or daemon
was used. Five grouped core and four native cases cover exact lexical ordering,
canonical authority, raw-score/dedup/bounds, pre-top-k lexical duplicate exclusion,
local semantic injection, disabled fallback, admission/lock deadline and blocked
inference timeout/cancellation with writer availability and retained permits.

One native batch review found the slow-inference lock retention and lexical-copy
shortlist issues. One fix wave resolved both; final SDK/web contract review is
clean. Root rejected a proposed lexical truncation regression because the baseline
lexical tokenizer already applies the identical 512-character bound.
Fresh required aggregate passed **2,441 Rust tests / nine ignored / 40 suites**,
then sequential lint and the explicit learned fixture passed. All eight source
hashes stayed unchanged through the gates. SDK/web source is unchanged and its
M31 gates remain historical; no redundant TypeScript gate was run.

The dependency prohibition is revoked; the existing FastEmbed dependency sufficed.
No ordinary profiles, actual owner records/accounts, services, database servers,
containers or new Cargo target directories were used. Similarity is not calibrated
confidence; broad semantic quality, remote latency and full upstream parity are
not established. Publish owned source/docs/notices only, preserve original nx.json,
and record exact source/PR/merge in the ignored milestone ledger. Continue M34
atomic archival Fact consolidation on a fresh branch from verified merged main.
