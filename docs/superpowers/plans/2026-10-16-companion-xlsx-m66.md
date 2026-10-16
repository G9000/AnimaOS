# M66 bounded read-only XLSX attachment extraction

**Status:** implemented and locally fixture-tested, including one finite synthetic Windows worker CLI case. Broader workbook and answer-quality acceptance remains open.

## Outcome

When an owner explicitly selects a valid `.xlsx` attachment in a message, extract a bounded, ordered text representation of workbook sheet names and cell values into the existing untrusted-document context. Attach stable sheet/cell locators and honest coverage/omission status. Keep the original upload and download unchanged. Do not edit workbooks, run formulas/macros, resolve external links, or fetch remote data.

XLSX is accepted as an upload type but not understood in selected-message context today: `hosts/rust-daemon/src/attachments/validation.rs` validates the OOXML ZIP envelope and `xl/workbook.xml`; `hosts/rust-daemon/src/attachments/documents/mod.rs::document_format` recognizes only DOCX/PDF. The worker and shared document contract also enumerate only DOCX/PDF. `docs/companion/documents.md` and PAR-08 in `docs/companion/parity-roadmap.md` identify other Office formats as unsupported.

## Reuse/provenance decision

Prefer a narrow extension of `packages/core-rust/crates/anima-document-content` over a new workbook dependency or host-local parser:

- It already depends on pinned `rawzip = 0.5.2`, `quick-xml = 0.41.0`, and `flate2`; `src/docx.rs` implements bounded ZIP central/local-header validation, compression checks, verified bounded entry reads, safe XML event handling, and explicit malformed/unsupported outcomes. `src/pdf.rs` shows the parallel public parser pattern.
- Extend the crate’s closed parser API and its XML/ZIP helpers where their assumptions fit. XLSX-specific relationship resolution and shared-string/table handling need their own narrow logic; do not pretend DOCX package rules are sufficient. Recheck that existing archive overlap/descriptor checks apply to every workbook ZIP part admitted.
- Do not add `calamine`, `umya-spreadsheet`, Python/openpyxl, or another whole-workbook dependency unless the bound implementer proves the maintained parser is materially safer and lower total cost than extending existing parser dependencies. Any new dependency needs license, maintenance, format-coverage and resource-limit review first.
- Existing `HERMES-NOTICE` records a narrow Rust DOCX semantic port from public Hermes `tools/read_extract.py` and retains its MIT notice. For this XLSX work, inspect only the pinned public Hermes tree at commit `1fd18277267152893a8a24792b79213e7ca3f899`: `skills/productivity/xlsx/SKILL.md` and `skills/productivity/xlsx/scripts/xlsx_read.py` demonstrate workbook inventory, sheet reads, JSON/CSV output, and separate formula/cached-value views. This is behavioral reference only; no Hermes XLSX code has been selected or copied. If copying is later chosen, record exact source/license and extend attribution; otherwise retain current notice unchanged.

## Contracts and likely ownership

1. **Shared parser — core crate:** `packages/core-rust/crates/anima-document-content/src/lib.rs`, new `src/xlsx.rs`, and focused `tests/xlsx.rs`. Add a parser entry point returning the existing typed extraction or a format-specific intermediate representation that the existing extraction contract can validate. Keep untrusted source bytes and limits host-neutral.
2. **Shared typed content contract:** `packages/core-rust/crates/anima-core/src/document_content.rs` currently closes `DocumentFormat` to Docx/Pdf and `DocumentLocator` to paragraph/page. Add `Xlsx` plus a stable locator, preferably `{ sheet_index, cell_ref }` with a bounded/control-character-safe sheet name carried as metadata rather than authority. Update strict contract validation, rendering, worker codec and mutation tests together. Do not put cell contents in filenames/paths.
3. **Daemon admission/worker:** `hosts/rust-daemon/src/attachments/documents/{mod.rs,admission.rs,worker.rs}` currently selects, dispatches and decodes only DOCX/PDF. Add XLSX recognition by extension plus MIME, worker format code, per-format input validation and safe result mapping. Reuse current owned worker lifecycle, timeouts, cancellation and whole-process cleanup; do not add a new process or service.
4. **Selection path:** `hosts/rust-daemon/src/agent_runs/attachments.rs` and existing document admission/context tests already ensure only message-selected attachments are read and admitted under shared limits. Extend those seams; do not index all uploaded files or alter standalone download behavior.
5. **Public DTOs:** Attachment upload/list/download SDK and web shapes should remain unchanged because original bytes and metadata do not change. Update an SDK/web DTO only if the existing extraction-format enum is actually public; do not add XLSX editing controls or a separate spreadsheet UI. Native-only extraction metadata stays inside the current run/context boundary.

## Extraction rules and limits

- Preserve the existing supported-document input ceiling (currently 2 MiB), the existing selected-document count/attempt allowance, and the current shared 16 KiB rendered text budget. Do not increase global limits for XLSX. Add stricter XLSX-internal limits for central-directory entries, selected workbook/rels/sharedStrings/worksheet XML uncompressed bytes, worksheet count, shared-string count, rows/cells, per-cell text, nesting/events and total parse work. Limits must be explicit constants and checked with overflow-safe arithmetic before allocation/iteration.
- Only accept expected OOXML parts and relationships. Resolve internal workbook relationship targets against the package root, normalize safely, and reject traversal, duplicate/ambiguous IDs/targets, external targets, unexpected URI schemes and unsupported content types. Do not open arbitrary ZIP entries. Reject encryption, multi-disk, unsupported compression/flags, corrupt CRC/size metadata, overlapping entry ranges, malformed XML, DTD/entity declarations and invalid XML characters. Reuse rawzip+verified bounded reads and quick-xml’s explicit event parsing; never extract the archive to disk.
- Resolve `xl/workbook.xml` sheet IDs through `xl/_rels/workbook.xml.rels`; process sheets in workbook order and cap sheet count. Read only required shared strings and worksheet cells. Support shared-string and inline-string cells and bounded scalar numeric/boolean/error values. Treat formula cells as **formula text plus optional cached value** only if both can be extracted safely; never calculate or imply freshness. If formula and cached value are both present, preserve both distinctly or render a clear `formula=...; cached=...` representation. Missing cached results remain missing. Define handling for rich shared strings, merged cells, hidden/veryHidden sheets, sparse dimensions and unknown cell types, and test those decisions.
- Report workbook sheet count and included sheet/cell counts separately, plus truncation or unsupported status; do not imply visual layout, chart/pivot/image/comment coverage, formula correctness, hidden-sheet exclusion or complete workbook understanding. Either include hidden-sheet marker and contents under explicit scope or omit them with a typed omission reason; never silently hide their existence.
- Keep text untrusted and source-cited per cell, e.g. `xl/worksheets/sheet2.xml#sheet=2!B7`. Escaping/display cannot turn content into instructions or tool grants. Never use workbook metadata as filesystem paths.
- Keep M54 DOCX and M55 PDF semantics, coverage and serialization backward compatible. Existing old document snapshots must continue to decode; define a safe legacy rule for new optional fields and test it.

## Test-first acceptance

### RED, parser and contract owners

- Add synthetic XLSX fixtures in a new `xlsx.rs` under `packages/core-rust/crates/anima-document-content/tests/`: workbook rels, shared and inline strings, numbers/booleans/errors, formulas with cached/missing values, multiple sheets, hidden and veryHidden sheets, sparse cells, merged cells, and deterministic workbook ordering. Assert exact locator and coverage behavior.
- Add adversarial fixtures for malformed central directory/local headers, duplicate names and relationship IDs, path traversal/external targets, overlapping entries, CRC/size mismatch, encryption, unsupported compression/flags, corrupt/truncated ZIP, DTD/entities, malformed XML, oversized shared strings/sheets/cell counts, expansion bombs, invalid UTF-8/XML characters and cap boundaries. Assert no panic, archive extraction, unbounded allocation or read of unrelated entries.
- Extend `anima-core/src/document_content.rs` strict validation/rendering tests for XLSX status, locator ordering, UTF-8/caps, untrusted annotations, snapshot compatibility and document budget truncation.

### RED, daemon integration owner

- Extend `hosts/rust-daemon/src/attachments/documents/worker_tests.rs` and `admission_tests.rs`: selected XLSX passes to the existing owned worker; unselected XLSX is not read; wrong MIME/extension mismatch, oversized or malformed workbook gets honest refusal; worker timeout/cancel/exit retains ownership and cleanup; DOCX/PDF result serialization and existing worker lifecycle remain intact.
- Add end-to-end context projection coverage in `hosts/rust-daemon/src/attachments/model_tests.rs` or the existing attachment/run context tests: only selected synthetic XLSX contributes source-labeled cells within the existing total budget; formulas are explicitly not executed; original attachment bytes/IDs remain unchanged and the ordinary download path still returns the original.
- Add no model call for extraction tests; any model-facing acceptance must remain a separate scoped case and cannot claim answer quality from fixture parsing.

### Fix and gates after a bound plan

- First make the parser/contract and host integration tests fail for the absent XLSX behavior; then implement the narrow extension and keep all new tests focused on the real safety/selection boundaries above.
- `cargo test --manifest-path packages/core-rust/crates/anima-document-content/Cargo.toml` for parser-focused iteration.
- `bun x nx run rust-daemon:test --skipNxCache` for required Rust host/core validation after focused tests; follow repo Windows target-lock guidance if needed.
- Run formatting through the root-owned agreed workspace command and inspect only the owned diff. SDK/web gates should be unnecessary unless a public DTO changes; if one does, root must bind exact affected targets first.

## Top five binding risks

1. OOXML relationship targets and ZIP entry validation create traversal/expansion/security risk; permit only a strict normalized allowlist of required internal parts.
2. Formula cached values may be absent or stale; do not compute formulas and state exactly what was observed.
3. Hidden sheets can contain relevant or sensitive content; make their inclusion/omission explicit and test both visible and hidden cases.
4. Extending a shared strict serialized format may affect old worker snapshots; preserve DOCX/PDF wire compatibility and prove legacy decode.
5. Added parser limits must fit inside existing attachment/document/context budgets and retain joined worker cancellation/cleanup; no hidden larger allocation or process lifetime.

Root has selected this direction from verified M65 main. Source-only execution starts after plan publication; no completed behavior or acceptance is claimed.

## Controller rulings on the risks (binding)

1. Ruling: extend the existing host-neutral rawzip/quick-xml document crate and retain the physically limited owned worker; admit only bounded internal OOXML parts and never unpack an archive — a second runtime or whole-workbook engine adds cost and does not remove resource risk — cost if wrong: return malformed/unsupported/too_large without text; no network, files, external target or formula execution. Keep existing dependencies; an alternative requires concrete evidence before source changes, not an inferred user approval gate.
2. Ruling: XLSX output is stored-cell data only, with literal formulas labeled not evaluated and cached values labeled possibly stale; preserve raw scalar/date serial values and mark visual/style/chart/merge omissions — cached workbook values cannot establish current computation or visual fidelity — cost if wrong: partial/unsupported rather than a false calculated answer. All document text remains untrusted and never establishes grants.
3. Ruling: include visible, hidden and very-hidden sheets in workbook order, carrying explicit visibility and safe sheet names in bounded coverage metadata — silently excluding hidden sheets would conceal relevant source content — cost if wrong: reject malformed sheet metadata and record truncation, never silently claim complete coverage. Locators are numeric sheet/row/column, never filesystem paths or workbook-controlled IDs.
4. Ruling: retain document content version 2 and old DOCX/PDF wire shapes; add Xlsx, XlsxCell {sheet,row,column}, and an absent-by-default optional XLSX coverage field; strict validators require matching coverage/ordered unique locators and reject cross-format metadata — existing persisted snapshots must remain readable and metadata is no attachment authority — cost if wrong: fail validation before context, no legacy rewrite or public attachment schema change.
5. Ruling: keep 2 MiB input, two usable selected documents and shared 16 KiB rendered budget; XLSX adds at most 512 ZIP entries, 16 sheets, 8 MiB total expanded relevant XML, 2 MiB per XML part, 4,096 shared strings, 2,000 observed cells, 250,000 XML events and depth 64, with per-cell text capped at 1,024 UTF-8 bytes and explicit omission — sparse dimensions must not induce empty-range iteration or larger global context — cost if wrong: deterministic bounded refusal/partial output and joined worker retirement, not larger limits or a retry. Root freezes exact coverage fields/ownership before source tests; no real models/accounts/profiles/services are released. Fresh required Nx gates and one native batch review/fix wave precede publication; final SDK/web review is needed only for actual changes there.

## Execution amendments

- Prefix every Cargo command with CARGO_INCREMENTAL=0; root serializes jobs1/testthreads2 and uses Nx targets when present. The direct Cargo example above is only for a focused test when Nx cannot forward the package selection; verify targets first.
- Root alone commands/fmt/Git/tests/activation. Source-only workers: one contract owner, one security-sensitive parser owner and one daemon integration owner after contract freeze; no shared edits.
- First RED may use the existing public extract_document entry point only after minimal Xlsx contract exists; assert actual missing extraction/admission rather than compilation failure. Contract old-shape validation cases can run alongside this RED. Root binds a small test-first staging sequence before implementation.
- No real workbook/profile data, Python runtime, provider/model, credential/device/account, ordinary daemon, DB server/container or deployment activation. Synthetic workbooks only. Hermes is a behavioral reference; preserve all existing notices and record any actual literal port separately.

## Frozen shared interfaces

- DocumentFormat::Xlsx; DocumentLocator::XlsxCell { sheet: u32, row: u32, column: u32 }, with one-based sheet/row/column and strict workbook-order lexical uniqueness, rows <= 1048576, columns <= 16384. Do not use a workbook-provided part name as an external path.
- Add optional xlsx_coverage to ExtractedDocumentText with default/skip-if-None, preserving version 2 and old DOCX/PDF JSON. XlsxCoverage contains sheet_count, observed_cell_count, included_cell_count, and sheets: Vec<XlsxSheetCoverage>. Each sheet contains index, name (<=128 UTF-8 bytes, valid XML text/no controls), visibility (visible/hidden/very_hidden), observed_cell_count and included_cell_count. Counts must agree with blocks, array and totals; all <= existing bounded constants. Fit/context omission updates included counts without inventing observed cells or losing sheet metadata.
- Add typed StoredCellsOnly, CellLimit and CellTextLimit omissions as necessary; do not reuse paragraph/page omissions for XLSX. Existing strict omission count cap stays five. XLSX stored-cell extraction is Partial when usable, NoText when there are no readable cells, or a typed failure without blocks. Non-XLSX formats reject XLSX coverage and XLSX-only omissions. Unusable XLSX may omit coverage when package validation never established it.
- Render safe sheet names/visibility as quoted untrusted data under the same total rendered byte budget, with numeric cell locators and explicit formula/cache labels. Context fitting drops whole XLSX cells; no partial formula/cache representation. Coverage/status/metadata size remain bounded, including metadata-only omitted snapshots.
- No new public attachment/SDK/web contract is planned. Source owners must report any actually exposed extraction enum before changing it. Existing unselected uploads are never read, and original bytes/download IDs remain unchanged.

## Execution and resume

### Conditional public-format amendment (bound before SDK/web source)

The existing selected-chat attachment response at `hosts/rust-daemon/src/routes/contracts/sessions.rs` has a closed document extraction union, projected to `packages/sdk/src/sessions.ts` and the read-only badge in `apps/web/src/components/attachments/MessageAttachments.tsx`. XLSX therefore requires the plan's conditional public variant. Add `SessionMessageXlsxExtraction` with `version: 2`, `format: 'xlsx'`, existing closed status, nullable `sheetCount`, `observedCellCount`, `includedCellCount`, `hiddenSheetCount`, and omissions. Hidden count includes hidden and very-hidden sheets. Public projection contains no sheet names, cells/formulas, raw text, source hash, private paths or errors. Keep existing DOCX/PDF response bytes and selected-record/snapshot validation unchanged. SDK exports and omissions add only this variant and the three typed XLSX omissions; UI renders bounded counts/limitations through the existing badge with no new controls.

Ruling: extend the existing safe extraction-status response and badge when adding XLSX — root's targeted contract trace found a real closed public union, so suppressing every XLSX status would leave the console incomplete — cost if wrong: return no badge for invalid/mismatched metadata; never show raw document data or claim calculation/answer quality. One final SDK/web review and relevant fresh SDK/web gates now apply, after the frozen response and implementation. This supersedes the earlier expectation of no public DTO change and adds no route or account activation.

Root alone commands/fmt/Git, serialized Cargo CARGO_INCREMENTAL=0/jobs1/testthreads2. First source-only contract/RED stage, then frozen-contract disjoint parser/native integration owners. One native batch review, one combined fix wave, focused and required fresh Nx native/core gates. SDK/web review/gates only if actually changed; no duplicate broad audits. Per-command unsigned commit/push/PR/attach/standing merge and exact tree/hash proof remain authorized. Never stage nx.json/anima.yaml/.superpowers; no stash/reset/restore/path checkout/worktree. No real profiles/providers/models/accounts/daemons/devices/DB servers/containers/deployments. M64 public browser NOT RUN, M52 strict quality PARTIAL and consumed cases unchanged. Resume .superpowers/sdd/2026-10-16-companion-xlsx-m66/progress.md.

## Recorded outcome

Rust/core **2,918 passed / 28 intentional ignores / 46 suites / lint/build**; SDK **186 passed / three intentional service skips / typecheck/build**; web **1,508 passed / 121 files / typecheck/build**.

Focused core/native, one native batch and final SDK/web review, finite Windows worker CLI, docs/format/frozen hashes pass. Reused ZIP/XML dependencies unchanged; public Hermes XLSX informed behavior without a literal Python port. No real workbook/profile/model/provider/account/daemon/service/deployment activation. See [harness evidence](../../companion/harness-acceptance.md) for limits and exact scope.
