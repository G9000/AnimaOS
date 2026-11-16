# Model-directed multilingual Mother recall

> Execute inline using the debugging, test-driven development and verification skills. Preserve the existing dirty A6 and bounded-read work; do not commit or publish this slice without a request.

**Goal:** Remove English intent/topic rules and let AnimaOS's conversation model formulate multilingual provider searches while keeping native memory active and sharing explicit.

**Architecture:** Bootstrap reads only owner-pinned/assigned complete records. The existing `memory_search` tool defaults to native recall and gains an explicit `source: mother` read path on the current additive binding. The conversation model supplies a concise query, literal/hybrid/semantic mode, optional memory category and optional exact entity names. Rust validates these arguments, bounds work, preserves current-record and authority checks, and reports actual retrieval coverage. It never infers personal topics, species, inventory intent or translations from English words. A6 graph recall remains available behind the same read grants when the model explicitly requests provider recall.

**Alternatives considered:** A separate preflight model adds latency, charges and a second instruction context; expanding language dictionaries repeats the defect. Reuse the already admitted conversation model and its existing read tool instead.

**Preservation:** No live store edits, reindex, grants, model settings, autostart or installation changes are needed. All tests use disposable synthetic stores. Neither prior Discord allowance is reused. Native search remains the default and is never silently replaced with Mother. Provider errors never cause publication or replay.

- [x] Snapshot current source state and add failing mixed-language/provider-boundary regressions.
- [x] Replace `memory_provider/selection.rs` with bounded language-neutral search arguments and optional exact-name checks; retain A6 graph contracts and revision/authority validation.
- [x] Route explicit model-selected Mother searches through `workspace_context.rs` and `tools/memory.rs`; extend the `memory_search` descriptor. Bootstrap must not perform query-derived private searches in any language.
- [x] Update existing selection tests to exercise explicit model search plans, retaining budget, wrong-project, stale-record, retraction, pin and scope regressions.
- [x] Test English/Chinese/Japanese/Malay mixed queries and translated model searches, coverage/fallback, native default, unavailable binding, cancellation and unchanged selected sharing. Distinguish scripted model routing from learned semantic evidence.
- [x] Run an actual cached multilingual E5 acceptance case on synthetic facts if assets are available; never download a model or call a paid provider silently.
- [x] Run full `bun run rust:test`, `bun run rust:lint`, focused real native Mother/A6 cases, and required bundled build through managed wrappers with sequential Cargo/jobs=1 and the 32GiB cache cap.
- [x] Update validation docs/checklist, review the diff, save a Mother checkpoint, and remove only owned scratch after checking process/path ownership. Preserve prior source, all memories, backups and unrelated edits.

**Acceptance:** No English stop-word/owner/species classifier remains. Query interpretation belongs to the model; quoted entity names and model-selected exact names remain data constraints. Every provider result is a complete current authorized record or is omitted with bounded-coverage metadata. Native memory stays independently usable. No new write authority or implicit sharing. No claim that Unicode tokenization alone proves cross-language semantics or that a scripted model proves natural-language reasoning.

**Review refinements:** Each explicit search accepts one model-authored query (1–200 UTF-16 units), mode, optional v1 kind, up to two exact names, and an optional graph-read flag. It fetches at most eight previews and four complete records under a three-second deadline and an 8KiB complete-record budget; a partial envelope may receive one smaller read-only retry. Pins do not consume explicit search results. Machine-readable coverage preserves server search/selection/budget information and reports omitted complete records and graph availability with its actual literal mode. Source locators remain distinct from native memory IDs and cannot authorize local mutations. Exact names use Unicode normalization, Latin/alphanumeric boundaries and substring matching for names within continuous CJK text; they are model-selected constraints, never automatic translations. Tests must reject stale/wrong-project full records and distinguish model-routing fixtures from real learned retrieval.
