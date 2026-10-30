# Semantic project search implementation plan

**Goal.** Retrieve selected project passages by meaning while preserving keyword precision, current authority, citations, and source freshness.

**Scope.** Reviewable uncommitted implementation from a6354d21523564b1ca1da3ce78eec53d4264b11c. No remote document transfer, model downloads, service restarts, commits, pushes, or publication. Native verification is assigned by the parent chat and remains sequential under the primary 32 GiB Cargo budget.

## Workflow

- [x] `how` over the affected subsystem.
- [x] `architect` for parallel design exploration.
- [x] Write the throughput checkpoint as four todo items.
- [x] Delegate code-writing to a subagent with a specific scope and named data shape.
- [ ] Verify on the matching surface.
- [ ] Rebase into small, ordered commits. Skip commits and rebase because this batch explicitly requests an uncommitted result.
- [ ] If the design is contested, `interrogate` before shipping. Skip shipping because this batch is local only.
- [ ] Run Opening a PR. Skip because publication is explicitly excluded.

## Source grounding

The reusable document store owns a custody-protected SQLite derived index. Documents and chunks bind current source hashes and opaque citations. Source replacement, source retention, and corpus removal cascade deleted chunks and FTS entries. Keyword search uses quoted literal terms and BM25 with title weight two.

The host captures project revision, access epoch, membership generation, read lease, and selected source grants. It scans current source bytes before reuse, checks again before publication, searches only ready sources, and constructs a witness that verifies source bytes and authority before delivery. Manual search and automatic project chat retrieval share this path.

FastEmbed 7.2 already runs learned local ONNX embeddings. Its default model is intfloat/multilingual-e5-small with 384 dimensions and separate query and passage prefixes. The existing generic memory query handle also accepts hashed and HTTP providers, so it is not a safe project embedding boundary. A complete local model snapshot exists, but no inference has been run in this task.

## Throughput checkpoint

- [x] **Blocking first steps.** Create a fresh managed worktree from the exact base, ground store/authority/provider flows, compare two independent designs, fix the provider and type contracts before edits.
- [x] **Independent workstreams.** Store/ranking, host local inference/authorization wiring, and thin client metadata proceeded in disjoint files after the contract was fixed. Native verification stays with one shared executor.
- [x] **Shared mutable state.** The primary checkout and other chats remain untouched. All edits use the dedicated managed worktree. The existing SQLite cache owns derived vectors so source deletion and vector deletion share one transaction.
- [x] **Smallest safe decomposition.** One owner integrates store/host semantics. A bounded store implementer and client implementer own disjoint files. Fresh review checked the combined source. No per-feature Cargo output is created.

## Acceptance and verification

- [ ] Paraphrase fixtures distinguish actual local learned retrieval from keyword retrieval without injected-vector quality claims.
- [ ] Exact phrases, codenames, and numeric identifiers retain keyword precedence.
- [ ] Empty allowed sources, outsiders, revoked grants, changed bytes, removed/readded resources, and cross-project passages cannot return stale evidence.
- [ ] Model unavailability, hash-only memory configuration, remote memory configuration, or bounded inference failure returns truthful keyword search.
- [ ] Derived vectors are bounded, finite, profile-bound, revision-bound, and removed with source rows.
- [x] Run pure SDK project tests, SDK build/typecheck, focused web hook/component tests, and web build/typecheck.
- [ ] Parent executor runs wrapped focused store and host tests, full `bun run rust:test`, formatting/lint, and explicitly offline cached-model acceptance when available.

Tests will be written before implementation. Rust execution is held until the parent assigns the shared executor, so test-first authoring must not be described as an observed red/green cycle until those commands run.

## Design comparison and synthesis

Two independent default-model designers inspected the grounded subsystem. Candidate A stores bounded per-window vectors in the existing SQLite cache. Candidate B keeps a transient project index and hydrates citation IDs through SQLite. Both designers preferred A unless avoiding persistence is a product requirement.

| Criterion | A. Derived SQLite vectors | B. Transient semantic index |
|---|---|---|
| Current-source ownership | Existing cascading deletion binds vectors to citations. | Requires a second generation/invalidation inventory. |
| Custody and resource bounds | Reuses private cache custody and database cap, plus explicit vector quotas. | Requires charged staging/index memory and eviction accounting. |
| Integration complexity | One store owns evidence and derived vectors. | Cross-store publication and hydration must agree. |
| Restart behavior | Safe reuse requires a verified artifact fingerprint. | Rebuilds after every restart. |
| Local inference | Shares only the loaded FastEmbed model. | Can share that model or create a separate session. |

Choose A. Graft B's strict candidate filtering before ranking, explicit semantic coverage, and single local inference capability. Keep all citations and witnesses unchanged. No different-model cross-judge ran because this environment provides Codex default-model delegation only; a fresh independent default-model review will inspect the implementation.

The current learned model handle does not expose verified model/tokenizer artifact digests. Therefore the first version uses a random model-session identity, with explicit model, dimension, E5 prompt contract, and windowing version. It deliberately rebuilds vectors after a model reload/restart instead of claiming unsafe cross-session artifact reuse. Old session vectors are disposable and bounded.

## Named data shapes and ownership

- The reusable store owns `EmbeddingProfile`, `EmbeddingPassage`, `PassageEmbedding`, `SemanticCoverage`, and `HybridSearch`. It exposes bounded pending windows, conditional vector publication, and hybrid ranking. Vectors key exact citation and window ordinal, with foreign-key cascade from current chunks. The store owns normalization and validation.
- `LocalProjectEmbedder` contains only a loaded FastEmbed backend. It exposes its profile, session identity, document-window embeddings, and query embeddings. It cannot invoke hashing or HTTP. It never creates a model loader.
- Host `knowledge/semantic.rs` owns bounded preparation, current model/scope checks around inference, and truthful fallback metadata. Existing retained reads keep physical inference permits until native work retires. It never holds daemon or SQLite locks during inference.
- `KnowledgeSearch.retrieval` reports `mode` as `hybrid` or `keyword`, a semantic state, local model when eligible, and indexed/eligible/omitted window counts. SDK and UI carry this field without owning retrieval logic. Automatic chat retrieval uses the same result and metadata.

Long source chunks split into overlapping bounded UTF-8 windows and keep the original citation. Scoring uses the best window per citation. The profile includes the window/prefix contract. A cap can reduce semantic coverage but cannot silently reduce keyword coverage.

Ranking protects verified exact phrases, short exact codenames, structured identifiers, and numeric anchors. Remaining candidates use weighted reciprocal-rank fusion with a semantic emphasis, independent BM25 and learned cosine lanes, citation deduplication, and deterministic tie-breaks. Cosine is a ranking signal, not a confidence estimate. Quality acceptance must use actual cached E5 inference.

## Implementation tasks

1. Store worker writes regression tests, the vector table and quota handling, bounded window enumeration, conditional publication, allowed-source vector scoring, and rank fusion. Own only `packages/core-rust/crates/anima-document-store-sqlite`.
2. Host owner writes the FastEmbed-only capability and tests, then integrates bounded semantic work after current source indexing and before the existing witness. Own `memory_embeddings.rs` and `projects/knowledge`.
3. Client worker writes response types, hook state/reset handling, UI status, and focused client regressions. Own SDK project types and project knowledge hook/component/tests.
4. Fresh independent reviewer inspects authority, provenance, cancellation custody, ranking, bounds, and truthful claims. Fix material findings before shared validation.
5. Parent executor runs native checks and explicitly offline real-model fixture. Record evidence and limitations here. Do not claim completion while required verification remains unrun.

## Review and current evidence

The store and client implementations were delegated with disjoint ownership. Two independent design candidates used the configured default Codex model. No Cursor command, unavailable model role, or different-model review ran.

Independent static review found false quoted-phrase protection, blank semantic windows that could stall batch progress, and unbounded memory-runtime lock acquisition. The current source requires contiguous quoted anchors, omits blank windows consistently from ordinals/counts, advances migration cursors, retains the best semantic excerpt, and uses immediate model-state `try_read` fallback. A busy memory write regression holds the write guard throughout keyword search.

Fresh client evidence includes 36 focused web tests, eight SDK project tests, SDK build/typecheck, web build/typecheck, and diff checks. These prove the client state/contract path, not learned retrieval quality. Scoped lint completed with two existing hook dependency warnings; Vite retains existing configuration and bundle warnings.

Semantic native request revision 1 was withdrawn before any native command acquired it. Revision 2 passed formatting and all 38 store tests with verified unchanged source hashes. Host tests stopped during linking before execution because `CARGO_NET_OFFLINE` caused the installed ort-sys build script to skip cached runtime selection. This was not a test assertion failure.

Revision 3 freezes 1290 relevant source, Cargo, fixture, and wrapper files. It requeues only incomplete host/context/provider-guard and actual cached-E5 checks. Cargo uses CLI `--offline` instead of that environment variable; the coordinator verifies the exact already-cached ONNX runtime. The parent remains the only executor. Queuing is not a passing result.

The independently labeled meaning cases require the expected document ranked first. Keyword baseline results are recorded separately. The late clause must also rank first and its selected excerpt must contain the late termination text. Current-witness checks remain. This tiny synthetic English fixture is not a production-quality estimate. There is no calibrated relevance threshold; results are nearest ranked candidates.

A fresh bounded integration review compared the assembled result on newer main baseline a31a9aac against the frozen semantic source. It found no missing wiring, export, guard, SDK/UI serialization mismatch, or displacement of the newer chat-control UI. This is static evidence only; combined compilation and browser/runtime acceptance remain required.

The 32768-row and 64-MiB vector payload quotas cover all projects and sessions. Superseded profiles retire when their corpus is next processed. Inactive corpora can retain bounded derived rows and reduce semantic capacity for another project; keyword evidence remains available and partial coverage is explicit.

## Actual-model failure and general correction

The first combined tree passed the full Rust suite, including 3784 daemon tests with 41 ignored, all core and supervisor suites, and 38 store tests. Rust correctness lint and 344 SDK tests also passed. These are baseline results before the following product correction, not final current-tree certification.

Actual cached E5 inference ranked every simple preserved label first. The keyword baseline returned no hits for "How can we back out?", while learned retrieval ranked termination first. The stronger long-prefix case failed. Its 3119-byte chunk had a complete 148-byte termination passage starting at byte 2971, but the only relevant raw window started at 2880 and included 91 unrelated bytes. Recovery ranked first. The red evidence is preserved in the coordinator's `integration-r1-c3.log`.

Source inspection found no scorer-order or missing-window defect. Embedding dilution from the unrelated prefix is the supported hypothesis. Increasing overlap does not isolate the clause. Sentence-only segmentation could lose useful cross-sentence context, so the chosen correction retains raw byte coverage and adds bounded standalone-sentence and adjacent-sentence-pair spans only to long chunks. Short chunks retain exactly their original inputs. Exact-text dedup removes repetitive spans. All raw coverage remains, and optional spans are capped and sampled deterministically with tail coverage.

This recovers the exact original 148-byte termination input without changing the six short competitor inputs, labels, query, or assertions. The host profile uses the shared new window-contract identity. Store initialization invalidates derived vectors and recomputes all persisted counts when that contract changes, including previously nonzero counts.

The actual learned host witness fixture passed when executed alone. Two model loaders in one test process caused an ONNX environment initialization failure, so subsequent real-model fixtures run in separate Cargo test processes. No product fallback or assertion was weakened.

The parent executes revised source in the integration copy only to avoid stale cross-worktree Cargo reuse. A fresh independent correction review and actual-model verification remain required. Full Rust verification reruns after the product window-policy change.
