# M18 Companion Memory Quality Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans; root coordinates implementation, review, validation, acceptance, and integration.

**Goal:** An older relevant durable fact remains available to the Rust companion after irrelevant reflections, with bounded context and clear memory curation habits.
**Architecture:** Add deterministic, owner-scoped fact selection to `anima-memory`; render the selected records through the daemon's existing provider. Keep explicit correction tools and evaluator persistence intact. Selection adds no model call, embedding call, or dependency.
**Tech stack:** Existing Rust memory manager, daemon provider/tool descriptions, in-process tests, and root-owned API acceptance.
**Baseline:** M17 merged at `59f391f2d32f4ebc03be2f8e1a83bf9a65a6437a`; previous counts are historical, not current-tree verification.

## Evidence and scope

- `hosts/rust-daemon/src/components/providers.rs:14-58`: `RecentMemoriesProvider::get` ignores `Message`, reads only three newest owner records, and joins content without type, ID, or hidden-text filtering.
- `hosts/rust-daemon/src/components/evaluators.rs:76-141`: completed responses create reflections and heuristically extracted facts; reflection importance is 0.65, extracted fact importance 0.72.
- `packages/core-rust/crates/anima-memory/src/memory_manager.rs:440-493`: lexical search limits index results before owner/type filters; reusing it alone can let unrelated records starve relevant owner facts.
- `packages/core-rust/crates/anima-memory/src/memory_manager/types.rs:367-425`: records already carry owner, type, importance, scope, room, content, and stable IDs.
- `hosts/rust-daemon/src/tools/memory.rs:91-202`: correction already requires owner ID and exact full `expected_content`, with atomic persistence and canonical reindexing.
- Public Hermes `tools/memory_tool.py:1-5` describes a frozen session-start snapshot; its schema at 346-373 favors compact durable facts over completed-work logs and temporary TODO state.
- Public Hermes `agent/prompt_builder.py:192-232` favors declarative facts and bounded curation; it routes procedures to skills. Borrow the habit, not its storage or snapshot semantics.
- Inspected public Hermes source only; no imports/execution, profile/config/environment/token/keyring reads, or Hermes writes.
- M18 is lexical deterministic context quality, not semantic/vector recall quality or live Hermes parity.
- Exclude new connectors, new memory storage formats, migrations, automatic skill writes, background consolidation, UI work, and independent audits.

## File ownership and interfaces

| File                                                                     | Responsibility                                                                                                                           |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `packages/core-rust/crates/anima-memory/src/memory_manager/types.rs`     | New small `MemoryContextOptions` struct: `agent_id`, `room_id`, `query`, `limit`.                                                        |
| `packages/core-rust/crates/anima-memory/src/memory_manager.rs`           | `select_context(&self, opts: MemoryContextOptions, visible: impl Fn(&Memory) -> bool) -> Vec<Memory>` and deterministic ranking helpers. |
| `packages/core-rust/crates/anima-memory/src/memory_manager/tests.rs`     | Selection behavior regressions using existing store fixtures.                                                                            |
| `hosts/rust-daemon/src/components/providers.rs`                          | Query-aware selection, hidden-text exclusion, bounded rendering, provider tests.                                                         |
| `hosts/rust-daemon/src/tools.rs`                                         | Compact memory curation additions to existing memory tool descriptions.                                                                  |
| `.superpowers/sdd/2026-10-12-companion-memory-quality-m18/acceptance.md` | Root-owned current-tree verification and live evidence.                                                                                  |

Keep reusable selection under `packages/*`; daemon owns prompt formatting and passes its visibility predicate, keeping hidden-text policy out of the reusable crate.
Keep provider name `recent_memories` and existing registration so runtime configuration remains compatible.
If types require an explicit existing export, add only that export after inspecting its declaration; no module restructuring.
The final integration gate also requires a bounded deterministic-adapter repair
in `hosts/rust-daemon/src/model/render.rs` and its unit/API tests: preserve the
multiline quoted fact block through the next provider boundary, with both empty
sentinels supported. Real provider transport is unchanged.

## Root binding rulings: top five risks

1. **Ownership and visibility:** Filter owner, Fact type, and caller visibility predicate before ranking/limiting. Daemon excludes room-scoped records from other rooms. Shared/private owner facts may survive fresh sessions; do not widen to another agent or infer world/session authorization.
2. **Prompt authority:** Memory content is quoted evidence, never a new instruction. Exclude `has_hidden_text` records before rendering. Current user instruction and tool approval policy remain authoritative.
3. **Cost and boundedness:** No extra model/embedding call per turn. Normalize at most 512 query characters and 32 distinct terms; output at most six records and 4,800 Unicode characters including headings/guidance. Do not clone a whole snapshot to select records.
4. **Stale and duplicate facts:** Deduplicate identical normalized selected content only; preserve records/history and evaluator behavior. Defer heuristic/model duplicate-capture changes. Corrections still use `memory_search` plus exact IDs/full content with `memory_update`; truncated context is never valid `expected_content`.
5. **Ranking limitations:** Use lexical relevance first, importance next, creation time and ID as deterministic ties. No unrelated reflection/recent fallback. Record lexical misses honestly; do not promise semantic retrieval or ranking equivalence to Hermes.

## Task 1: Select owner facts before limiting

- [x] Add failing tests in `memory_manager/tests.rs` for an old Fact with a unique topic, followed by 20 irrelevant reflections; asking about that topic must select the old Fact.
- [x] Add a stronger crowd-out fixture: many matching facts from another owner precede a matching own Fact; own Fact still survives selection.
- [x] Test zero matching facts, unrelated own facts, other-room facts, deterministic ties, duplicate selected content, and `limit=0`.
- [x] Implement `MemoryContextOptions` and `select_context` using the canonical record map, filtering owner/type/caller visibility before ranking.
- [x] Normalize query/content into lowercase alphanumeric terms; omit a small documented English question/grammar stop-word set. Use distinct query-term overlap/coverage, not substring matches.
- [x] Require positive lexical overlap; preserve useful non-English and short distinctive terms rather than imposing an arbitrary minimum word length.
- [x] Rank positive candidates by coverage, importance, creation time, then stable ID; keep only the best bounded set without copying all record content.
- [x] Deduplicate exact normalized content among selected records; no semantic merging, deletion, importance mutation, or temporal rewriting.
- [x] Generic or empty queries return no automatic facts; existing `memory_search` remains available for explicit broader recall.
- [x] Use the existing Rust host test target for red/green verification; root runs Cargo/Nx serially.

## Task 2: Render bounded memory evidence

- [x] Add provider tests with an actual `Message` topic; verify the old fact appears while reflections, unrelated facts, other-owner facts, and hidden text do not.
- [x] Replace `get_recent(limit=3)` with `select_context` using runtime owner and message room/query.
- [x] Pass a predicate rejecting hidden text and mismatched Room scope before selection limits; hidden entries must never consume the six slots.
- [x] Render each entry with ID, type, and quoted content. Include a compact note that stored content is evidence and current requests take precedence.
- [x] Cap each content excerpt at 800 Unicode characters and the entire provider text at 4,800 characters, counting metadata labels and guidance.
- [x] Mark excerpts explicitly; direct correction to search for full content before using `memory_update`.
- [x] Keep IDs intact or omit the entire entry if it cannot fit; never truncate an ID and present it as usable.
- [x] Test Unicode boundaries, huge content, huge IDs, many matches, and empty selection. Assert total output and record count limits.
- [x] Keep existing provider metadata kind/name; add only bounded selection count metadata if useful to existing diagnostics.

## Task 3: Encourage a small, meaningful memory habit

- [x] Amend existing `memory_add` description: save compact declarative durable facts, preferences, and standing context; skip trivial facts, raw transcripts, temporary task progress, and completed-work logs.
- [x] State that an already represented fact should not be added again; use exact correction tools for changed current facts.
- [x] Preserve M17 correction text requiring adequate search and update of every obsolete current fact; historical reflections need not be rewritten.
- [x] Keep procedural knowledge in existing skill workflows and approvals; the guidance never authorizes an automatic skill write.
- [x] Make guidance concise and static; no extra summarization/nudge model turn, no new tool or tool availability.
- [x] Confirm existing tool descriptor assertions still pass through the full host suite; add one focused description assertion only if it protects the correction contract.

## Task 4: Root validation and API acceptance

- [x] Root confirms project names with `bun x nx show projects --json` and available targets with `bun x nx show project rust-daemon --json`.
- [x] Root sets `CARGO_INCREMENTAL=0` and serializes Cargo. Reuse the existing target directory; no daemon runs during source gates.
- [x] Root runs `bun x nx run rust-daemon:test --skipNxCache`; current-tree success is required. Use project-supported formatting/lint checks only after confirming targets.
- [x] Full web checks are required only if a web file changes; this plan changes no web surface.
- [x] Using only the retained synthetic M16 acceptance profile, reuse the corrected M17 preference facts; add irrelevant synthetic reflections and foreign-owner matching facts through authorized APIs.
- [x] Start a fresh session and ask about the preference without pasting its value or marker; no extra completed model turns are needed to seed noise.
- [x] Normal configured gpt-5.5 connection must recall the correct preference. Capture request/result IDs and store evidence, not credentials or private profile content.
- [x] Fresh recall uses the existing approved M17 exact-ID correction; preserve independent facts. M17 already exercised correction and quiet restart, so do not repeat those model calls absent new correction/storage changes.
- [x] Include a hidden-content and unrelated-owner negative fixture through existing test infrastructure, not production/private data.
- [x] Root repeats fresh recall after daemon restart only if needed to establish changed selection survives the retained persistence path; reuse existing service ownership rules.
- [x] Stop after one useful live pass; if provider behavior varies, record the gap and investigate boundedly rather than rerunning indefinitely.
- [x] Record deterministic tests separately from live API behavior; no browser workaround, connector sends, database/container changes, deployment, or new dependencies.

## Review and integration gate

- [x] Root obtains bounded implementation review of owner filtering, lexical crowd-out, output limits, correction safety, and lack of additional per-turn calls.
- [x] Root records actual commands/results and acceptance limitations in the milestone ledger; baseline M17 counts do not establish M18 success.
- [x] Root inspects final diff for unrelated edits and stages explicit milestone files only; never stage `nx.json` or `anima.yaml`.
- [x] Root handles the authorized milestone commit/merge after required review and current-tree gates; no Git/service/Cargo mutations by this planning worker.
- [x] Completion statement: relevant old facts remain in bounded owner context under tested lexical queries; exact correction remains intact; semantic recall and Hermes parity remain unclaimed.

## Controller rulings on the risks (binding)

1. Ruling: Select only this agent's Shared/Private Fact records without world/session annotations, or the exact current Room scope without those annotations, filtering visibility before limits — unprovided world/session context cannot establish authority — cost if wrong: omit the record and use scoped explicit recall rather than inject unrelated data.
2. Ruling: Render stored facts as quoted data with intact IDs and clearly marked excerpts — stored text and truncated evidence cannot become instructions or exact correction input — cost if wrong: reject unsafe edits and require memory_search for complete current text.
3. Ruling: Limit query normalization, candidate results and rendered characters without extra model or embedding calls — memory relevance must not add a per-turn bill — cost if wrong: lexical misses remain explicit; no semantic-quality claim or context flood.
4. Ruling: Deduplicate prompt content only and preserve existing canonical memories, evaluator history and temporal evidence — this milestone fixes selection, not a full learning lifecycle — cost if wrong: duplicate-capture cleanup stays deferred and no records are deleted or rewritten implicitly.
5. Ruling: Test owner/visibility filtering before ranking and lexical crowd-out with deterministic fixtures plus one real fresh preference query after noise — fluent model replies alone do not establish correct context — cost if wrong: record the failed layer and fix only the concrete gap, without claiming broad parity.

Activation: the owner reiterated autonomous continuation after M17. Root accepts this lean plan, runs the bounded regression/API checks and retains standing reviewed-gate merge authority. No separate preflight audit, approval pause, real account integration or browser-policy workaround.

## Verified outcome (2026-10-12)

The reusable selector and daemon renderer are implemented. Eight new core and
five provider regressions passed after meaningful RED; the integration gate
exposed a first-line-only deterministic adapter, repaired with one RED-proven
regression and strict result-text assertions. One combined review and bounded
parser supplement found no remaining actionable issues.

Fresh required gate: `bun x nx run rust-daemon:test --skipNxCache` passed
2,324 tests with eight ignored; `rust-daemon:lint` passed. Focused renderer
one, agent API fifteen, core selection nine matched and provider five passed.
No web/SDK source changed; M17 remains their last code validation.

Actual gpt-5.5 acceptance: twenty newer irrelevant own reflections and eight
foreign matching facts were seeded through the owner API in the retained private
profile. A fresh query supplied neither answer nor marker, returning compact
table/lime-604 in one step, 2,980 reported tokens and no tools. No additional
correction/restart model probe was needed because those unchanged paths passed
in M17. Private evidence and command logs remain in the milestone ledger.

Only lexical scoped selection, prompt deduplication and curation guidance are
claimed. Canonical records, evaluator lifecycle and correction persistence are
unchanged; semantic/vector quality and automatic consolidation are unverified.
Owned processes are stopped and ports 8080/4200 are free. Publication identifiers
and the exact next branch are appended to the ledger after verification.
