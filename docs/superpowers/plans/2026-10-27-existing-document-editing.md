# Existing Document Editing Implementation Plan

> **For agentic workers:** Use `superpowers:executing-plans` to implement this plan task-by-task after the coordinator explicitly releases implementation and assigns shared-file ownership. Subagents require explicit authorization; this design phase creates none. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Edit bounded existing DOCX, XLSX formulas/charts and PPTX while preserving supported untouched content and requiring a complete exact owner review before publication.

**Architecture:** Add a portable targeted OOXML editor beside the existing generated-document renderer. The daemon owns upload/workspace custody, isolated preparation, immutable draft/review storage, exact native approvals, checkpointed atomic apply and imported-edit publication receipts. SDK and web components project typed reviews and content previews without running Office content.

**Tech Stack:** Existing Rust rawzip/quick-xml/flate2/serde libraries, host SHA-256 and Windows Job custody, existing control-plane persistence/approvals/checkpoints, TypeScript SDK and React UI. No new package dependencies are initially planned.

**Design:** [Existing document editing design](../specs/2026-10-27-existing-document-editing-design.md).

**Phase rule:** This document is a proposal, not permission to edit shared code. No branch/worktree/commit/push/deploy, real-account operation or concurrent Cargo process. The coordinator assigns a build slot and integration ownership before implementation. Preserve current dirty edits. Use `cmd.exe` with `login:false` for shell operations. Required library guidance comes from Context7; writing Rust also uses `rust-best-practices`. The design's explicit limits and public contracts are normative for each task below.

---

## File structure and integration ownership

Feature-owned code is under:

- `packages/core-rust/crates/anima-document-content/src/editing/`: pure package admission, typed operations, span edits, format adapters, calculations, review and preservation checks.
- `packages/core-rust/crates/anima-document-content/tests/editing_*.rs` and `tests/fixtures/editing/`: observable format, round-trip and adversarial coverage.
- `hosts/rust-daemon/src/document_edits/`: proposal registry, safe blob storage, owner service, isolated worker and exact approval witness.
- `hosts/rust-daemon/src/tools/document_edits.rs`, `src/routes/document_edits.rs`, `src/routes/contracts/document_edits.rs`, `src/routes/tests/document_edits.rs`.
- `packages/sdk/src/document-edits.ts`, `document-edits.spec.ts`.
- `apps/web/src/components/documents/`: typed review, approval and content-preview components with matching tests.

The exact existing shared files and ownership request are enumerated in the design's Dependencies and ownership request section. Shared wiring is a separate integration task; do not edit it until assigned. No changes to `generation/`, generated-document source schemas, existing document extraction wire types, package manifests or the dirty results-panel files are expected. Native worker custody extraction is the only planned refactor, and must preserve current extraction behavior.

## Verification commands and scheduling

Do not run any compile/test/lint in this initial design phase. After release, run one Rust command at a time under the coordinator's build slot. Do not set a new Cargo target directory or exceed the shared 32 GiB ceiling.

Focused portable test wrapper (substitute the exact test target from each task):

```text
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-document-content --test editing_package
```

Focused daemon tests:

```text
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-daemon document_edits
```

After focused tests, the coordinator serializes `bun run rust:test` and `bun run rust:lint` in the current combined tree. For touched TypeScript workspaces run their inspected scripts:

```text
bun run --filter @animaos/sdk test
bun run --filter @animaos/sdk build
bun run --filter @animaos/sdk typecheck
bun run --filter @animaos/web test
bun run --filter @animaos/web build
bun run --filter @animaos/web typecheck
```

Use feature-specific test filters while developing where the existing runner supports them; never assume an uninspected script. Each meaningful behavior task starts with a failing regression test, then minimal implementation, then its passing focused check. Record exact results, restore the build slot, and remove only inspected task-owned scratch artifacts after each verification. Do not change cache tooling or delete Cargo fingerprints.

## Task 1: Lock contracts, fixtures and limits

**Create:** `editing/mod.rs`, `editing/types.rs`; `tests/editing_package.rs`; `tests/fixtures/editing/README.md` under the portable document crate.\
**Shared:** `anima-document-content/src/lib.rs`, export only after assignment.

- [ ] Confirm coordinator release, source ownership and supported-preview/package-policy decisions. Recheck `git status --short`; record unrelated edits without modifying them.
- [ ] Inspect pinned rawzip/quick-xml APIs using local dependency source plus the Context7 references in the spec. Do not assume upstream raw-copy helpers exist in rawzip 0.5.2.
- [ ] Define the exact serde unions from the spec: `OfficeFormat`, `OfficeEditSpec`, `OfficeEditOperation`, `OfficeCellValue`, `OfficeRange`, inventories, review DTOs, calculation states and typed errors. Expand all nested wire projection variants into closed typed fields with explicit bounds; no arbitrary JSON preview/change values. Reject unknown keys/variants, nonfinite numbers, out-of-bound operation counts and text lengths.
- [ ] Write tests for representative JSON requests and invalid extra properties, bad coordinates, operation size, invalid enum states and Unicode scalar offsets. Run `editing_package` to observe failure before implementing validation.
- [ ] Add independent minimal template fixtures with human-readable part manifests for Word/Excel/PowerPoint, internal PNG/JPEG relationships, styles and distinctive unchanged parts. Fixtures must be inert, license/provenance documented and free of real data.
- [ ] Implement constructors/validation and pass the focused contract tests. Do not add editor stubs to the runtime tool catalog yet.

Acceptance: malformed operation shapes cannot reach package edits; all three formats have source fixtures and explicit preservation assertions.

## Task 2: Build bounded package admission and span editing

**Create:** `editing/package.rs`, `editing/xml_spans.rs`; expand `tests/editing_package.rs`.

- [ ] Write tests covering ZIP traversal, name collisions, central/local mismatches, overlap, CRC failure, encrypted/multidisk/ZIP64 inputs, size expansion, bad XML/namespaces, DTDs, external relationships and active content. Add a valid template with `../media/image1.png` relationships; it must be accepted.
- [ ] Run the test target and observe the missing admission behavior fail.
- [ ] Index and read all admitted parts with shared expanded/XML/event budgets. Reuse the proven checks in existing extractors by small internal helpers only when it does not change their accepted behavior; avoid replacing their independent semantics wholesale.
- [ ] Implement a namespace-aware byte-span index over UTF-8 XML. Retain raw bytes, comments/whitespace and prefixes; record exact boundaries before/after parser events. Reject unsupported instructions/entities and invalid XML before any patching.
- [ ] Validate `[Content_Types].xml`, every relationship part, internal URI resolution and the enumerated inert content/relationship/namespace policy. Retain and enumerate valid ordinary HTTP/HTTPS/mailto hyperlinks without fetching/following; reject active external resources, unsafe schemes/UNC/credentials and executable fields. Scan orphan parts and formula/field sources; no archive extraction or resource fetch.
- [ ] Implement nonoverlapping escaped span patches and bounded repackaging. Preserve untouched uncompressed part bytes and entry order. Use current writer APIs or bounded recompression; do not invent a ZIP header implementation to save compression work.
- [ ] Reopen candidates and enforce exact unchanged-part/spans equality plus the declared change allowlist. A deliberately modified sentinel style/image part must fail validation.
- [ ] Run the complete `editing_package` target. Record archive-container normalization separately from logical part preservation.

Acceptance: output is never based solely on a renderer's success; unsafe/ambiguous packages and accidental collateral changes are refused.

## Task 3: Implement Word text changes without flattening templates

**Create:** `editing/word.rs`; `tests/editing_word.rs` and Word fixtures.

- [ ] Write failing tests for body/table text edits, exact repeated text targeting, Unicode, escaping/whitespace and run-spanning uniform properties. Add refusal tests for mixed styles, tracked changes, fields/controls and nonplain text ranges.
- [ ] Implement inventory paragraphs with internal text-run mapping and opaque IDs bound by the host's source hash. Expose coverage/limitations and preserve-only header/footer/note/image areas.
- [ ] Apply range edits only over supported text nodes; retain paragraph/run properties, unaffected text and every nonedited part. Add `xml:space` only when needed. Refuse overlaps and stale `expected_text`.
- [ ] Reinspect changed paragraphs and compare all unaffected span/part sentinels, including numbering/styles/header/footer/internal image relationships.
- [ ] Run the wrapped `editing_word` target and record its results. Longer content receives a page-layout limitation, not a fake layout pass.

Acceptance: a formatted external template can be edited and round-tripped without losing its unchanged styling/media.

## Task 4: Implement spreadsheet cells and honest formula calculation

**Create:** `editing/spreadsheet.rs`, `editing/formulas.rs`; `tests/editing_spreadsheet.rs`, `tests/editing_formulas.rs`.

- [ ] Write failing tests for string/number/bool/blank edits, formula-looking literal strings, shared strings, existing style preservation, absent-cell insertion and merged-cell anchor restrictions.
- [ ] Implement worksheet/sheet identity, coordinates, stored values/formulas and coverage. Preserve rich shared strings untouched; changed strings become local inline strings. Refuse protected or shared/array/spill edit regions.
- [ ] Write failing expression tests for arithmetic precedence, `$` references, ranges, the six supported aggregate functions, supported blank behavior, errors, cycles and resource budgets. Ensure no process/eval path exists.
- [ ] Implement the small lexer/parser/evaluator with explicit error/deferred states and bounded dependency traversal. Pin finite arithmetic and supported type behavior; do not coerce text/date/locale expressions speculatively.
- [ ] Implement the separate conservative admission tokenizer for unchanged deferred built-ins and internal fixed-range names. Reject unsafe/unknown functions, external/DDE references and macro sheets. Document the literal allowlist in code/tests.
- [ ] On cell/formula changes, clear/recompute formula caches as specified; update `calcPr`; remove calc-chain metadata through declared derived changes. Preserve unchanged formula expression bytes. Include cache invalidation and deferred-cell counts in review data.
- [ ] Conservatively detect downstream unsupported chart/pivot caches; block proposals when affected output cannot be made honest. This requirement is completed with Task 5 before cell edits are exposed to users.
- [ ] Run the wrapped `editing_spreadsheet` and `editing_formulas` targets sequentially. Verify old extraction and generated spreadsheet tests still distinguish literals from formulas.

Acceptance: users can change formulas, see exact supported results or explicit recalculation requirements, and never receive fabricated cached results.

## Task 5: Implement Excel chart edits and affected-cache handling

**Create:** `editing/charts.rs`; `tests/editing_charts.rs` with bar/line/pie fixtures.

- [ ] Write failing tests for chart-title changes, series-range rebinding, categories/values/count validation, multi-sheet source identity and preserved drawing anchors/styles.
- [ ] Inventory existing supported charts/series via validated drawing relationships. Keep unsupported charts preserved-only; do not evaluate external/embedded data.
- [ ] Implement title text and fixed-range series operations without changing series count/type or layout. Validate compatible lengths and supported typed results, then update references and cached points together.
- [ ] Determine which charts reference changed worksheet cells; refresh every affected supported cache from staged data. A deferred or unprovable affected chart/pivot cache blocks the proposal before review publication.
- [ ] Distinguish calculated/current caches from retained stored caches in previews; title-only changes preserve unchanged data semantics.
- [ ] Reopen output and compare chart series/caches to reviewed expectations and unrelated ZIP parts exactly.
- [ ] Run the wrapped `editing_charts` target, followed by the spreadsheet/formula targets only if their behavior changed.

Acceptance: existing Excel charts are meaningfully editable, and cell edits cannot silently publish stale chart pictures/data.

## Task 6: Implement PowerPoint text and slide order

**Create:** `editing/presentation.rs`; `tests/editing_presentation.rs` and slide/template fixtures.

- [ ] Write failing shape/table-text tests with Unicode, duplicate per-slide shape IDs, multiple text runs, whitespace and rich/mixed-style refusal.
- [ ] Implement targets anchored by slide relationship, shape and paragraph identity; retain layouts/masters/themes, notes and internal media relationships.
- [ ] Implement exact plain-text range changes with the same byte-span/property rules as Word where applicable, without conflating namespace/schema differences.
- [ ] Write failing permutation tests: duplicate/missing/unknown slide IDs rejected, full valid reorder changes the main slide list only, hidden slides/custom shows retained.
- [ ] Implement slide reorder and before/after ordinal review. No part deletion/reindexing, custom-show rewrite or layout adaptation.
- [ ] Reopen candidates and compare all untouched masters/layouts/notes/images/relationships. Run the wrapped `editing_presentation` target.

Acceptance: uploaded presentations can receive supported text/order revisions while their template content survives intact.

## Task 7: Produce complete typed reviews and preservation proof

**Create:** `editing/review.rs`; expand all format integration tests.

- [ ] Write failing tests that review contains every direct edit and derived formula/cache/type/relationship change, plus explicit untouched/preserved-only coverage.
- [ ] Build `PreparedOfficeEdit` with private candidate bytes and bounded typed review accessors. Never deserialize a model-supplied prepared candidate or trust an input preview.
- [ ] Produce plain content previews: Word paragraphs/tables, spreadsheet cells/formulas/chart point arrays, presentation text/order. Include original vs edited values, hidden-content flags, stored/calculated/deferred labels, and layout limitations.
- [ ] Enforce 64 KiB complete-review and 128-operation limits. Reject/split oversized proposals; never approve a summary with hidden omitted changes.
- [ ] Run targeted review tests in format integration targets. Confirm `generation/` still requires entire regenerated-file equality and accepts no imported-edit marker.

Acceptance: the owner can understand exactly what would change before any workspace file is touched.

## Task 8: Add owned worker preparation and private draft storage

**Create:** `hosts/rust-daemon/src/document_edits/{mod,registry,store,service,worker,tests}.rs`.\
**Shared:** existing document worker custody, host startup/module exports, state and snapshot wiring, only after coordinator assignment.

- [ ] Write failing host tests for authenticated attachment resolution, source hash mismatch, different session/clear generation/workspace, private blob tampering, quotas and expiry.
- [ ] Implement a bounded registry with the spec's states and immutable source/destination/review/candidate binding. Stage candidate bytes only in the owned profile-adjacent safe store; no model storage paths, symlinks or workspace temporary files.
- [ ] Extract/reuse physical document-worker lifecycle custody without weakening current extraction READY/version/frame/resource behavior. New edit jobs use a separate fixed entry mode and bounded request/response schema; share the physical slot. Never send source bytes before verified worker readiness/resource setup.
- [ ] Add worker tests for malformed/wrong-type/oversized/trailing results, cancellation/drop/timeout, pipe floods, memory/process limits, physical retirement and unaffected neighboring processes. Non-Windows resource custody remains unavailable until proven equivalent.
- [ ] Save complete draft metadata and candidate before returning its review. Add default-empty persistence and coordinator-assigned snapshot version. A metadata save failure creates no announced applicable proposal.
- [ ] Implement cancellation/invalidation on stop, expiry, clear/delete, agent/profile/workspace changes and grant revocation; recheck after each await. Cleanup observes owned readers before removing blobs.
- [ ] Run focused daemon `document_edits` tests using the wrapper/build slot. Obtain separate coordinator scheduling for physical worker examples/acceptance using existing repository patterns.

Acceptance: preparation survives client disconnect safely and a dropped request never detaches an unowned parser or admits a second physical job prematurely.

## Task 9: Integrate tools, exact native review and atomic apply

**Create:** `tools/document_edits.rs`, `document_edits/approval.rs`.\
**Shared:** `tools.rs`, approval policy/settlement, checkpoint service, final action authority, completion evidence.

- [ ] Write failing tools tests for the three exact schemas, granted source scope, source/destination expectations, proposal-only no-effect behavior and rejected arbitrary XML/package paths.
- [ ] Register `inspect_office_document`, `propose_document_edit`, `apply_document_edit` per the spec. Inventory/tool results stay at most 12 KiB with hash-bound cursors; do not bypass generic spill validation.
- [ ] Before the apply tool gate, resolve a host-owned exact witness. Call existing `check_exact` for document edits, retaining original policy-deny evaluation and using the server's review projection. Reject absent native approval/action journal context and helpers/delegations/RPC bypass attempts.
- [ ] Extend exact-record settlement to allow-once/deny only for the new server-owned marker. Reusable policy allows, smart-check results and model flags cannot satisfy review. Preserve the instruction-write branch and tests.
- [ ] Write failing race tests: source/destination/candidate/review changes during approval, stop/revoke/profile/workspace switch during action save and immediately before file publication, and two native writers targeting the same preimage.
- [ ] Reserve destination/receipt capacity in the same durable applying transition before the file effect. Count concurrent reservations toward 512 paths, exclude same-destination concurrent applies, and fail full capacity before mutation. Test full registry, two proposals competing for its last slot, same-path contention, reservation save failure and restart/release/recovery. Only then expose/extract the current generated-document publication lock without changing verification behavior, add the apply tool to checkpoint admission, validate exact captured preimage, and publish stored candidate bytes with final live authority. The action journal's existing authority guard must also compare current applying proposal/witness/reservation state. With checkpoints disabled use the existing compare/replace or create-new authority callback, never ordinary file writes.
- [ ] Read back bytes, persist imported-edit publication receipt, finish native action evidence, and return authenticated URL/checkpoint/calculation status. Classify proposals as no file effect; record successful apply as a document file effect in completion evidence.
- [ ] Inject failures before/after intent, file replacement, readback and receipt save. Restart must never replay the mutation automatically; reconcile proven hashes or surface recovery-required. Idempotent replay returns a still-current receipt only.
- [ ] Run focused daemon tests and existing exact-approval/checkpoint/generated-document regressions in the coordinator's slot.

Acceptance: only the complete reviewed bytes can be applied, exactly once, against the reviewed file state with current native authority.

## Task 10: Add owner review routes and provenance-gated downloads

**Create:** `routes/document_edits.rs`, `routes/contracts/document_edits.rs`, `routes/tests/document_edits.rs`.\
**Shared:** routes/OpenAPI registration, `routes/workspace_files.rs`, related download tests.

- [ ] Write failing owner-route tests for full review fetch/cancel, no-store, wrong profile/session/workspace, expired/cancelled IDs and unknown fields. Proposal IDs are never bearer authority.
- [ ] Implement the spec's GET/DELETE routes. Keep decisions on the existing native approval endpoint; no parallel file-mutation endpoint.
- [ ] Extend workspace preview/download admission to the union of unchanged generated verification or a current server-held imported-edit publication receipt. Never accept arbitrary imported Office files or PDF via this branch.
- [ ] Verify exact stored path/root/hash and current owner authority. Return existing attachment disposition, octet-stream, no-sniff, no-store and bounded-length behavior. Existing credential/path screening remains enforced.
- [ ] Add tests for forged sidecars/embedded markers, tampered output bytes, stale receipt after replace/restore, correct receipt after restart, missing metadata/binary stores and unchanged generated-document rejection tests.
- [ ] Run focused route/download tests and record results. Keep capability/OpenAPI claims accurate and bounded; coordinator owns shared inventory generation.

Acceptance: only published verified edits become downloadable, and the original generated-document guarantee is intact.

## Task 11: Add SDK contracts and a complete owner review UI

**Create:** SDK `document-edits.ts`/tests and web `components/documents/{DocumentEditReview,DocumentEditApproval,OfficeContentPreview}.tsx`/tests.\
**Shared:** SDK client/export/workspace DTOs; web daemon API, approval card, access profiles/settings and workspace file preview hooks.

- [ ] Write SDK tests for owner review/cancel paths, exact request fields, bounded JSON/blob responses, abort/error handling and additive provenance fields. Implement typed clients using existing authenticated request helpers.
- [ ] Write UI behavior tests: load complete matching review before Apply, render Word text/cells/formulas/chart data/slides, show all changes and limits, distinguish recalculation states, safe text rendering, deny/cancel behavior and unavailable-review disabled state.
- [ ] Implement plain React content previews and supported chart plots from typed arrays. Do not use document-supplied HTML or display/decode imported media in this version. Preserve existing approval focus/keyboard conventions.
- [ ] Integrate the approval card by tool/review marker with only **Apply these changes** and **Keep original**. Ordinary approvals retain their existing four-choice behavior. A stale approval revision or changed digest must refresh/refuse, never silently apply.
- [ ] Add minimal source selection guidance for uploaded files/workspace files and **Save a copy** behavior. Expose saved imported-edit content previews/downloads using current workspace UI; do not modify the dirty chat results panel.
- [ ] Wire explicit read/propose/apply grants into access profiles/settings after coordinator assignment; observe profiles cannot apply. Keep end-user wording free of XML/ZIP/hash jargon.
- [ ] Run SDK and web test/build/typecheck scripts listed above. Render the review at narrow/wide widths and check full change list, warnings, focus order and disabled states using existing UI harnesses; record actual evidence and clean task-owned screenshots.

Acceptance: a human can review and approve a concrete proposed document change without reading raw tool arguments or misleading visual/calculation claims.

## Task 12: Combined verification and coordinator handoff

**Shared:** capability description and focused user documentation under coordinator-assigned ownership.

- [x] Exercise all three formats with inert selected uploads through the packaged daemon: inspect, propose, owner review, AllowOnce, guarded publication, authenticated download, archive content/preservation checks and unchanged source. The DOCX case also denies once and prepares a fresh proposal. The model endpoint is loopback-only; no live provider, account, shell or Office fallback is required.
- [x] Run the final focused document-edit, strict metadata-boundary and persistence checks: 60, 27 and 38 tests passed respectively. The unchanged packaged Office acceptance test also passed again after those changes.
- [ ] Verify checkpoint restore for an imported edit when checkpoints are configured, and separately validate a complete browser-to-native document flow.
- [ ] Recheck old generated DOCX/XLSX/PPTX/PDF creation, source verification, expected-hash revision, preview and download regressions.
- [ ] Audit every claim in the review/capability UI: supported formats/operations, 4 MiB editor ceiling, retained inert hyperlinks versus refused active external resources, content preview quality and formula limitations are explicit.
- [x] Run `bun run rust:test` and `bun run rust:lint` under the coordinator's build slot in the final combined tree. Both exited successfully; the daemon suite reported 3661 passed, 0 failed and 38 ignored. The targeted vendor custody test passed 1/1.
- [x] Run the full default SDK test suite: 51 files and 291 tests passed, including three real-daemon integration tests and one personality restart test.
- [ ] Resolve or account for the full web suite failures before claiming the combined web gate passes: 2284 passed, 165 failed and 1 skipped. Focused UI and synthetic browser results remain separate evidence.
- [ ] If an Office engine is explicitly available in a controlled test environment, run independent open/re-save/render checks on representative fixture outputs and record engine/version/results. Otherwise report visual acceptance as unverified and retain the content-preview-only product claim; do not download/install/invoke an engine implicitly.
- [ ] Inspect task-owned scratch paths/active users, remove only those verified scratch artifacts, retain configured test-output reports and leave tool/Cargo download caches alone.
- [ ] Hand the coordinator changed paths, exact commands/results, remaining limitations and integration risks. Do not commit, switch branches, push, deploy, message another chat or alter real accounts.

Completion gate: all three Office formats have supported end-to-end edits with preservation checks, complete exact owner review, restart/concurrency/revocation coverage and authenticated published downloads. Full Office rendering/calculation remains a separately named dependency, not an unimplemented implied promise.

## Implementation checkpoint — 2026-10-28

The coordinator released the portable editor and new leaf files while holding every existing shared host/UI/SDK integration file. The hyperlink revision and pre-effect receipt-capacity reservation revision have been incorporated. No worktree/branch/commit/push/deploy or real-account action occurred.

Implemented and exercised in the portable crate:

- `src/editing/{mod,types,package,xml_spans,word,formulas,spreadsheet,charts,presentation}.rs` and the exclusively assigned `src/lib.rs` exports.
- `tests/editing.rs`: 26 tests covering all three Office formats, immutable media/styles/headers/relationships, inert hyperlinks versus active external consumers, typed values and literal formula-looking strings, supported calculations and deferred/error caches, chart title/series/cache edits, slide text/order, malformed packages, raw traversal/case collisions, formula range budgets, shared strings, calc-chain removal, empty worksheets and hidden rows/columns.
- `tests/fixtures/editing/{README.md,word-template.xml,pixel.png.base64}`. Fixtures are independent hand-authored OOXML, not source generated by Anima's renderer. No Office engine/rendering acceptance has been run.

Verification (one owned Cargo command at a time, shared `validation` target, no cache-ceiling changes):

1. Initial red check: new editing exports were absent (`E0432`).
2. First implemented pass: 19 new editing tests passed.
3. Added regressions exposed self-closing empty-sheet insertion and missing hidden row/column labels: 20 passed, 2 failed. Each root cause was corrected and its targeted test passed separately.
4. Final command: `bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima-document-content --test editing --test generation --test docx --test xlsx` — **72 passed, 0 failed** (26 editing + 13 generation + 12 DOCX extraction + 21 XLSX extraction). The owned wrapper physically exited with code 0 and the Cargo slot was explicitly released.

Owned Rust files were formatted directly with rustfmt, without formatting unrelated files. No daemon test/build/lint, full workspace check, or external Office engine was run. The repository's task wrapper owns/cleans verification scratch; this phase created no scratch scripts, screenshots or ad-hoc reports to remove.

### Actual portable API available now

`inspect_office_document(OfficeFormat, &[u8]) -> Result<OfficeInventory, OfficeEditError>` and `prepare_office_edit(OfficeFormat, &[u8], &OfficeEditSpec) -> Result<PreparedOfficeEdit, OfficeEditError>` are exported from `anima-document-content`.

`PreparedOfficeEdit::bytes() -> &[u8]` and `::review() -> &OfficeEditReview` return a sealed candidate and typed content review. It deliberately does not deserialize. `OfficeEditReview` has `version`, `before: OfficeInventory`, `after: OfficeInventory`, `changes: Vec<OfficeChange>`, and `preservation: OfficePreservation`. Changes have closed, bounded plain-text `before`/`after` fields; the inventories carry typed cells/formulas/chart ranges/slides. The future HTTP `DocumentEditReview` wraps this content with host provenance, destination, expiry and digests. Do not mistake the portable review for owner approval or published-file provenance.

### Actual host leaves drafted now (not compiled or runtime-wired)

New `hosts/rust-daemon/src/document_edits/{mod,registry,approval}.rs` establishes these concrete metadata and witness interfaces. Its 7 unit tests have been written but **not run** because the Cargo lease excluded daemon checks. Do not register apply tools or enable downloads from these leaves alone.

Persisted field for the coordinator's version-33 snapshot: `document_edits: DocumentEditRegistry`, with `#[serde(default)]` on the containing snapshot field. Runtime state uses the same type. The registry derives `Default + Clone + Serialize + Deserialize`; private maps contain pending drafts, latest publication receipts and exclusive destination reservations. Its serde keys are snake case internally. Preserve its maps; do not expose them as owner/model mutation APIs.

Actual value types:

- `DocumentEditScope { profile_key: String, workspace_key: String, agent_id: String, session_id: String, clear_generation: u64, run_id: String }`. Every value is host-derived, with profile/workspace keys from current owner custody; no model-supplied scope is authoritative.
- `DocumentEditSource::Attachment { attachment_id, sha256 }` or `::Workspace { file_path, sha256 }`.
- `ExpectedDestination::Absent` or `::Present { sha256 }`, tagged by `state`.
- `DocumentEditDraft { id, scope, source, file_path, expected_destination, candidate_sha256, candidate_bytes, operations_sha256, content_review: OfficeEditReview, created_at_ms, expires_at_ms }`.
- `DocumentEditRecord { draft, review_sha256, status }`, with `Prepared`, `Applying { approval_id, tool_call_id }`, or `RecoveryRequired { approval_id, tool_call_id }`. Published drafts compact into `DocumentPublicationReceipt`; full before/after review persistence after publication is not implemented yet and must be resolved before the final review route contract is frozen.

Registry methods available in source:

```rust
validate(&self) -> Result<(), String>
insert(&mut self, draft: DocumentEditDraft, now_ms: u64) -> Result<String, String> // review digest
get(&self, id: &str) -> Option<&DocumentEditRecord>
begin_apply(&mut self, id: &str, review_sha256: &str, scope: &DocumentEditScope,
    approval_id: &str, tool_call_id: &str, now_ms: u64) -> Result<(), String>
reserved_apply(&self, id: &str, review_sha256: &str, scope: &DocumentEditScope,
    tool_call_id: &str, now_ms: u64) -> bool
confirm_publication(&mut self, id: &str, tool_call_id: &str, observed_sha256: &str,
    checkpoint_id: Option<String>, now_ms: u64) -> Result<DocumentPublicationReceipt, String>
cancel(&mut self, id: &str, no_effect_proven: bool) -> Result<Option<DocumentEditDraft>, String>
recover_after_restart(&mut self) -> Vec<String> // IDs needing reconciliation, no file mutations
publication(&self, profile_key: &str, workspace_key: &str, file_path: &str,
    sha256: &str) -> Option<&DocumentPublicationReceipt>
```

Call `validate()` during snapshot validation. `recover_after_restart()` invalidates prepared authority, turns applying intents into recovery-required and retains their reservations; persist that result before accepting new actions. Reconciliation must compare owned destination bytes plus native action/checkpoint/approval evidence before `confirm_publication` or cancellation with `no_effect_proven=true`. That boolean is a private host assertion, never a model/HTTP input. No file is automatically rewritten by registry recovery.

`begin_apply` reserves the same-path exclusion and the 512-path publication capacity together, counting other reservations. It validates metadata and rolls back its in-memory change on validation failure. The coordinator must hold its existing durable control-plane transaction, snapshot/restore the registry on save failure, and complete the save before a file effect. `reserved_apply` is not permission until that save has succeeded. Tests cover full capacity, the last-slot race, same-path contention, restart retention, failed-save rollback and seal tampering; they remain unexecuted until a daemon lease is granted.

Actual exact-review source APIs in `document_edits::approval`:

```rust
capture(registry: &DocumentEditRegistry, scope: &DocumentEditScope,
    call: &ToolCall, now_ms: u64) -> Result<Option<DocumentEditWitness>, String>
DocumentEditWitness::approval_call(&self, call: &ToolCall) -> ToolCall
DocumentEditWitness::revalidate(&self, registry: &DocumentEditRegistry,
    scope: &DocumentEditScope, call: &ToolCall, now_ms: u64) -> bool
DocumentEditWitness::native_file_authorized(&self, registry: &DocumentEditRegistry,
    scope: &DocumentEditScope, call: &ToolCall, now_ms: u64) -> bool
exact_record(arguments: &str) -> bool
```

For `apply_document_edit` only, call `capture` before the normal gate **after** establishing a direct ordinary owner run and source/session custody. It accepts only `proposal_id`, `review_sha256`, `file_path`, `expected_destination`; model `approved`/exact-marker keys are rejected. Pass original call plus `witness.approval_call(original)` to `gate.check_exact`. The projection's private marker is `_anima_exact_document_edit`. Extend exact settlement's existing instruction-record branch with `document_edits::approval::exact_record`; permit allow-once/deny only. Repeat `witness.revalidate` after approval and receipt awaits while still prepared. After the durable applying/reservation save, enforce `witness.native_file_authorized` inside the existing synchronous `native_file_authority` state guard, alongside current owner/run/tool grants/deny/stop/source checks. It verifies the original arguments, exact scope/review/source/candidate and current same-call reservation. It never substitutes for current source bytes, owner identity or an actual owner decision.

### Current gates before declaring this feature complete

- Blob storage, the isolated worker, host service, tools, routes, SDK and UI wiring now exist in the combined source tree. Their presence does not establish successful native compilation or full-flow acceptance.
- Source re-review cleared the four reported host P2 findings and the pending-run evidence-retention correction after their fixes. Native compilation, regression tests, worker custody and end-to-end publication checks remain with the coordinated Rust driver.
- Current portable compatibility limits to surface in the eventual UI: content previews only; 4 MiB packages; common inert HTTP/HTTPS/mailto hyperlinks retained; unsupported active/complex content refused; only existing simple chart titles/series are editable (adding a missing title is currently refused); unsupported charts/pivots block cell edits conservatively; unchanged named formulas outside the safety syntax can be refused; full Office rendering/calculation remains external.

### Portable-core review fixes — 2026-10-28

The independent review requested five corrections. Tests were added first and reproduced all five failures; the corrected tree then passed `bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima-document-content --test editing --test generation --test docx --test xlsx`: **83 passed, 0 failed** (37 editing, 13 generation, 12 DOCX, 21 XLSX). The owned command physically exited with code 0 and the Cargo slot was released.

1. Pivot protection now derives from admitted content types and relationship kinds rather than filenames. Renamed-cache fixtures cover each independent signal.
2. New formula ASTs permit ranges only as direct supported-aggregate arguments; bare ranges and ranges nested in arithmetic/unary expressions are rejected before preparation.
3. Presentation custom-show slide relationships are accepted only in the main presentation's proper custom-show ancestry and only when the referenced slide is in its main slide list. Reordering preserves the custom-show bytes; an unlisted relationship is rejected.
4. Fixed-range defined names are validated/resolved before package-wide formula admission. Safe unchanged formulas using those names are retained with external recalculation status; executable names remain rejected.
5. Word, PowerPoint and chart-title text edits now map decoded Unicode scalars to their original XML byte spans. Unselected character references, entity spellings, comments, tag attributes and quote choices are retained. `xml:space` changes only when necessary and is recorded as a derived review change. CDATA-containing text is preserved-only. Multi-run, entity, Unicode, self-closing text and whitespace cases are covered.

Independent portable source re-review subsequently cleared all five findings. The 83-test run predates the retained-text helper described below; it does not validate that helper or host approval, custody, routes or UI.

### SDK and review interface handoff — 2026-10-28

The earlier “Still pending” list is a historical checkpoint, not current ownership. The host writer now owns `document_edits/**`, native document-edit tool/route leaves and extraction-worker custody. Generic SDK client/index and `daemon-api.ts` remain with the shared integration writer. The SDK/UI writer owns the new document-edit SDK and review component, `ApprovalCard`, and the explicitly released settings/files surfaces.

The new `packages/sdk/src/document-edits.ts` exports `DocumentEditsClient` and the actual snake-case host review/inventory/cancellation types. The class constructor takes `DaemonClient`; it exposes `review(proposalId, agentId, sessionId, { signal? })` and `cancel(...)`, using `/api/document-edits/{proposal}?agent_id=...&session_id=...`. The shared writer now exports the module, adds `documentEdits` to `DaemonClient`, and forwards the same client as `daemon.documentEdits` using the existing owner-authenticated transport. `ApprovalCard` uses `daemon.documentEdits.review` directly.

`ApprovalCard` routes every `apply_document_edit` request to `DocumentEditCard`, even malformed requests; the latter never falls back to generic reusable approval. It reads only for an interactive owner, validates the complete host DTO against the exact witness and scopes asynchronous review/decision results to the current approval identity. Only allow-once and deny are offered. A lost decision acknowledgement requires refreshing rather than replaying an uncertain decision. The built-in access switch is separate and opt-in; no access profile or helper is granted document editing automatically. JS tests cover the owner route, stale proposal/source/review/destination/session responses, read-only no-fetch, unsafe text, late decisions and lost acknowledgements. Full host and UI integration is not yet validated.

SDK/UI verification so far: 285 SDK source tests passed; 26 focused document/approval UI tests passed; the three new explicit-access checks passed separately; SDK typecheck, web typecheck, web build and both lint scripts passed. The full `SettingsPanel.test.tsx` run has 60 failures and 10 passes in this combined tree, primarily missing existing control/label selectors; do not report the full suite as passing. An initial attempt at the full SDK suite inadvertently reached its two daemon integration tests, which invoke Cargo internally. It was interrupted, and a process listing showed no remaining Cargo, rustc or daemon processes. No Rust result was obtained from that interrupted run. The subsequent SDK run was restricted to `src` and did not invoke service tests. Final combined checks remain coordinated.

The full `bun run --filter @animaos/web test` subsequently exited 1: 2,228 passed, 217 failed, one skipped, and one unhandled test-run error (204 files, 16 failing). `ViewHarness.test.tsx` alone had 88 failures; settings, skill and other suites also failed. Its focused document review tests passed separately. This run cannot establish a passing full-flow or workspace-wide UI gate.

After the shared owner facade landed, the card was switched to `daemon.documentEdits.review` and the SDK public type exports. The updated focused review/approval run passed 28 tests, SDK and web typechecks and the web production build passed. The shared writer added receipt-backed workspace preview/download wiring and the paginated SDK response; `WorkspaceFiles` now renders those pages as plain content and checks path, fingerprint, source label, coverage and cursor progress before appending. All 16 focused Files-view tests pass, including cancellation of a late page on file selection change. Host compilation and native end-to-end validation are still pending.

The host source now includes a fix and regression for the same-path cancellation reservation race. The host source review also cleared its four P2 findings and the pending-run evidence-retention correction after fixes. These are source-review outcomes; their new native regressions are not claimed as run here.

### Private retained-content screening handoff

The portable crate now exports `check_office_document_text(format: OfficeFormat, source: &[u8], policy: impl FnMut(&str) -> bool) -> Result<bool, OfficeEditError>`. The callback returns **true for allowed text**; `Ok(false)` means reject without exposing the triggering text. In the isolated edit worker, screen both source and prepared candidate with `|text| !generated_documents::credential_fields(text) && foreign_sessions::redact_secrets(text) == text`, and return only a generic policy rejection. Never invoke the parser on the daemon's untrusted input thread. `Package::open` retains the existing 4 MiB per part, 16 MiB expanded, 500,000 event and depth limits; the visitor checks every admitted XML/relationship part, decoded text joined across runs within paragraphs or spreadsheet cell/shared-string/comment blocks, text-bearing attributes, and XML comments. It has no output containing the private content. New regressions cover Word header/footer/comments/footnotes/endnotes, XLSX shared text, PPTX notes, split runs, attributes, XML comments, safe separate paragraphs, invalid input and screening the preserved candidate. At this handoff the source had not yet been compiled or run; later focused native evidence is recorded below.

### Current coordinated verification evidence

- The document browser test passed **7 of 7** cases using synthetic responses: desktop and mobile receipt-backed previews, complete matching review with one-time Apply, mismatched-review refusal, and a read-only viewer with no private review fetch. It is browser/UI evidence, not native publication acceptance.
- The missing SDK public type export was corrected; current coordinated typechecks passed. Earlier SDK source tests and focused UI results are recorded above.
- The earlier 83 portable tests predated the retained-text helper. The subsequent full Rust run included the newer retained-content fixtures and passed all 42 editing tests.
- Final focused runs passed 60 document-edit tests, 27 metadata-boundary tests and 38 persistence tests. Earlier targeted coverage included seven Windows custody ACL tests and one ordinary and one continuity process-kill test.
- The full `bun run rust:test` and `bun run rust:lint` commands exited successfully. The daemon suite reported 3661 passed, 0 failed and 38 ignored; the packaged three-format acceptance test passed within that run. The targeted vendor custody test passed 1/1.
- The full default SDK suite passed 51 files and 291 tests, including three real-daemon integration tests and one personality restart test.
- The latest full web suite reported 2284 passed, 165 failed and 1 skipped; it is not a passing suite. No live provider calls were made. The seven synthetic browser cases do not establish live-provider behavior or an end-to-end browser-to-native flow.

### Packaged acceptance and decoder handoff

The packaged startup health check passed after the coordinated startup correction.
The new packaged document acceptance test compiled after test-only ZIP and hash
API corrections, then reached real DOCX attachment inspection. Its first proposal
was refused before approval: model tool offsets became floating-point values in
the host argument conversion and could not deserialize as unsigned integers.
The document-local checked decoder correction passed all three focused regression
tests (`test-output/continuity-document-decoder-tests.log`, 3 passed, 0 failed).
The subsequent packaged acceptance test passed unchanged
(`test-output/continuity-packaged-document-acceptance-decoder.log`, 1 passed,
0 failed, 22.07 seconds). It exercises real selected DOCX, XLSX and PPTX uploads
through the isolated worker, review, approval, publication and download, verifies
candidate hashes, edited archive contents and preservation, and checks denial and
a fresh proposal for DOCX. It uses a loopback scripted model; it does not exercise
the browser, an Office application's rendering or configured checkpoint restore.
The final full Rust, Rust lint, targeted vendor custody and full default SDK
results are recorded in `test-output/continuity-full-rust-formatted.log`,
`test-output/continuity-rust-lint-final.log`,
`test-output/continuity-win-custody-final.log`, and
`test-output/continuity-sdk-full-final.log`, respectively. No live provider
acceptance or real Office application visual acceptance is claimed.

The strict boundary requires both configured private editing storage and the
actual local control-plane metadata file to remain outside the effective
workspace. Unsafe or uncertain placement refuses startup even with empty metadata;
there is no legacy overlap exception or automatic relocation. The final focused
results are recorded in `test-output/continuity-document-edits-final.log` (60/60),
`test-output/continuity-metadata-boundary-green.log` (27/27), and
`test-output/continuity-metadata-persistence-green.log` (38/38). The unchanged
packaged Office acceptance rerun in `test-output/continuity-document-acceptance-final.log`
passed 1/1 in 23.67 seconds. The later full Rust run passed the same acceptance
test again. These results do not establish the full web suite or an end-to-end
browser-to-native flow.
