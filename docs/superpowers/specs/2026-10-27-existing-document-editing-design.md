# Editing existing Office documents

Date: 2026-10-27\
Status: architecture accepted with retained inert hyperlinks; portable implementation released and checked. Host registry/witness leaves are drafted; shared runtime integration remains held.\
Feature: 2 of the coordinated Hermes/OpenAI capability implementation.\
Plan: [Implementation plan](../plans/2026-10-27-existing-document-editing.md).

## Outcome and scope

An owner can select an uploaded or workspace DOCX, XLSX, or PPTX, ask for supported changes, inspect a before/after content preview and complete change list, and approve that exact revision before publication. Uploaded originals remain immutable; the result is saved as a workspace copy. Existing workspace documents can be replaced using their expected hash or saved as a new copy. Every path uses the Rust runtime, current grants, native action receipts, configured file checkpoints, atomic publication, and authenticated downloads.

This is a bounded Office editor, not a desktop Office replacement. All three formats ship in the same feature. An unsupported package or operation produces an explanation without changing any target. Editing arbitrary existing PDFs, legacy DOC/XLS/PPT, OCR, and password removal are outside this feature. Current verified generated-document creation/revision, including PDF, remains unchanged.

## Evidence from the current tree

- `packages/core-rust/crates/anima-document-content/src/generation/mod.rs` renders a 48 KiB structured source into at most 4 MiB. `inspect_generated_document` regenerates and compares the entire output. An embedded source marker alone is deliberately insufficient.
- `hosts/rust-daemon/src/tools/generated_documents.rs` exposes `read_document_source` and `write_document`. Existing-file writes require the current SHA-256 and verified generated provenance. It already uses `WorkspaceBytes`, `live_checks`, the action journal, checkpoints, and compare/replace or create-new publication.
- `anima-document-content/src/docx.rs` and `xlsx.rs` provide bounded extraction, not a lossless editing model. The XLSX extractor reads stored data and never calculates formulas. There is no existing PPTX extraction contract.
- `hosts/rust-daemon/src/attachments/files.rs` retains uploads under `uploads/<date>/...`; reads pin workspace identity, expected length, digest, and attachment metadata. `attachments/documents/admission.rs` checks owner/session/run identity before and after parsing.
- `attachments/documents/worker.rs` owns a Windows Job-limited, one-shot parser process, bounded pipes, timeout/cancellation cleanup, and one physical slot. Current non-Windows physical extraction fails closed. This is resource custody, not a general filesystem/network sandbox.
- `tools.rs` and `approvals/gate.rs` already support exact owner decisions for protected instruction writes. `state/approval_state.rs` rejects reusable decisions for those exact records. This is the pattern to extend for document review, with a separate document marker.
- `filesystem_checkpoints/service.rs` currently admits four specific write tools. The new apply tool must be added deliberately. Default checkpoint file size is 10 MiB, sufficient for the proposed 4 MiB editor ceiling.
- `routes/workspace_files.rs` permits Office previews/downloads only after generated-file verification. Imported edits need separate server-held provenance; broadening `inspect_generated_document` would violate the current guarantee.
- `packages/sdk/src/workspace.ts` and `apps/web/src/components/sessions/ApprovalCard.tsx` provide existing download and approval integration. Dirty results-panel, SDK test, and Rust cache/tooling edits are unrelated and must be preserved.

## Alternatives

| Approach | Benefits | Cost / limits | Decision |
| --- | --- | --- | --- |
| Targeted OOXML package edits in the existing Rust document crate | Retains untouched content, works without a desktop Office installation, bounded and reviewable | Requires explicit format subsets; structural previews cannot guarantee layout | Recommended for this feature |
| Rebuild imported files through the current `DocumentSpec` renderer | Small implementation | Discards template layout, images, styles, formulas, and unsupported parts | Reject for imported editing |
| Automate an external Office engine | Broad rendering and calculation coverage | New executable/runtime trust boundary, installation, sandboxing, fidelity and licensing/platform work | Separate optional integration; not an implicit dependency |

## Supported behavior by format

| Format | Editable in v1 | Preserved | Unsupported edits |
| --- | --- | --- | --- |
| DOCX | Exact text ranges in plain body paragraphs and table-cell paragraphs, including ranges across adjacent runs whose run properties are byte-identical | Untouched paragraphs/runs, styles, numbering, tables, section/page properties, headers/footers, notes/comments, internal PNG/JPEG images and their relationships | Rich/mixed-style range replacement, fields, tracked changes, content controls, equations, text boxes, new tables/images, layout/style redesign |
| XLSX | Typed cell values; explicit formulas in the calculation subset below; titles and series ranges of existing simple 2D bar/line/pie charts | Cell style indexes, row/column sizing, number formats, merges outside touched cells, themes, internal images/drawings, unrelated chart structure and worksheet features | New charts, pivot/table structure, shared/array/spill formula regions, protected cells/sheets, structural row/column changes, data connections, unsupported chart types/data |
| PPTX | Exact plain text ranges in slide text shapes and slide table cells; permutation of all existing slide IDs | Masters/layouts/themes, shape geometry/style, internal PNG/JPEG images, notes, transitions and relationships | New/deleted slides, notes editing, shape moves/resizing, chart/SmartArt/equation editing, media, embedded Office objects |

Preservation applies only after package admission. Inert content may be retained without being editable, but it must have an explicitly supported content/relationship type. Unknown active content is never copied merely because it is not the edit target. Every inventory identifies both editable targets and preserved-only areas. A text replacement spanning different styles is refused with guidance to choose smaller ranges. New text inherits the existing run properties; no formatting is silently flattened. Empty text is allowed; newline/tab insertion is deferred until explicit element-edit operations exist.

For XLSX, existing cell text is never interpreted as a formula. `set_cell_value` uses an explicit string/number/boolean/blank union and writes literal text, even if it starts with `=`, `+`, `-`, or `@`. Only `set_cell_formula` creates `<f>`. Editing a missing ordinary cell may insert that cell/row in coordinate order and update worksheet dimensions; it does not insert worksheet rows. Merged cells are editable only at the top-left anchor. Changing a shared-string cell uses a local inline string, preserving the original shared-string table and all other references.

Chart operations are `set_chart_title` and `set_chart_series_ranges`. Existing series count, chart type, colors, layout, and drawing anchors remain intact. Series ranges must resolve to rectangular, bounded same-workbook worksheet ranges. Categories and values must have compatible counts and yield known strings/numbers from literals or supported calculations. The editor updates both range formulas and their caches. Unsupported/deferred inputs block a chart-data operation; they never become invented zeroes. A title-only chart edit can retain unchanged data caches, labeled as stored data.

Slide ordering changes only the main presentation slide-ID list. Each existing ID appears exactly once. Part names, numeric IDs, relationship IDs, slide contents, and custom-show membership are retained; the review identifies custom shows as unchanged. Text targets are anchored by slide relationship, shape ID, and paragraph identity, not the slide's current ordinal.

## Calculation contract

New/changed formulas support finite decimal constants, unary `+`/`-`, binary `+ - * /`, parentheses, same-sheet A1 references with optional `$`, rectangular ranges in `SUM`, `AVERAGE`, `MIN`, `MAX`, and `COUNT`, and commas as argument separators. The request uses one leading `=`; stored `<f>` text omits it. Scalar blank references are zero; aggregates skip blanks, with empty `SUM`/`COUNT`/`MIN`/`MAX` returning zero and empty `AVERAGE` reporting a calculation error. Text coercion, dates, boolean arithmetic, locale syntax, exponentiation, and implicit intersection are rejected rather than approximated. Division by zero, missing sheets/invalid references, cycles, and resource limits return explicit calculation states. Cell addresses retain Excel's row/column bounds; an absent ordinary in-bounds cell is blank.

The evaluator is a small bounded parser/interpreter, never `eval`, scripting, or a host process. Use finite IEEE-754 binary64 arithmetic with no text/boolean coercion; a formula depending on an unsupported typed value is deferred. Cap formula length at 4 KiB, nesting at 64, dependency nodes at 10,000, and aggregate range visits at 100,000 per job. Unit tests pin numeric results, blank handling, error propagation, and limits. No function is allowed network/process/filesystem access. Calculation-error/deferred cells receive no fabricated `<v>`; their reason is shown before applying. Invalid syntax for a new formula is refused rather than stored.

Unchanged formulas outside the evaluator subset can remain only if a separate formula admission tokenizer recognizes safe OOXML syntax and a conservative allowlist of inert built-in functions. Initial deferred built-ins: `IF`, `IFERROR`, `AND`, `OR`, `NOT`, `ABS`, `ROUND`, `ROUNDUP`, `ROUNDDOWN`, `SUMIF`, `SUMIFS`, `COUNTIF`, `COUNTIFS`, `VLOOKUP`, `HLOOKUP`, `XLOOKUP`, `INDEX`, `MATCH`, `CONCAT`, `CONCATENATE`, `LEFT`, `RIGHT`, `MID`, `LEN`, `TRIM`, `DATE`, `YEAR`, `MONTH`, `DAY`, and `TEXT`. They are preserved without evaluation; unknown functions/UDFs and unsupported formula constructs fail package admission. Supported shared/array formula metadata may be retained only outside edited regions, without evaluation. Defined names are admitted only as internal fixed ranges; formula-defined names and XLM macro sheets are rejected.

Any cell/formula edit conservatively invalidates all workbook formula result caches. Recompute caches only for formulas whose complete dependency closure is supported; clear the others, retain their formula expressions, set workbook recalculation flags, and remove an existing calc chain plus its relationship/content-type entry. The derived changes appear in the review. Dependencies involving shared/array formulas, deferred functions, errors, or unknown results remain `needs_external_recalculation`. Do not claim the workbook is fully calculated. A chart affected by edited cells must either have all series caches refreshed accurately or block the proposal; stale affected chart caches cannot pass unnoticed. Pivot caches/tables whose inputs cannot be proved unaffected block cell/formula edits.

The preview distinguishes literal, calculated by Anima, stored/unverified, and needs recalculation. Saved formula `<v>` values are caches, and omitting them delegates calculation to a reader; see Microsoft's [formula documentation](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-formulas). Setting recalculation flags requests recalculation on opening; it does not calculate the workbook in Anima. Complete Excel compatibility, advanced charts/pivots and reliable visual pagination require an external Office engine.

## Portable editing API

Add `editing/` inside `anima-document-content`, separate from `generation/`. No filesystem/process/network/host types enter this module. Keep the existing generation schema, renderer, verifier and extraction wire formats unchanged.

```rust
pub enum OfficeFormat { Docx, Xlsx, Pptx }
pub struct OfficeInventory { /* version, format, targets, coverage, limitations */ }
pub struct OfficeEditSpec { pub operations: Vec<OfficeEditOperation> }
pub enum OfficeEditOperation {
    WordReplaceText { target_id: String, start: u32, end: u32,
        expected_text: String, new_text: String },
    SetCellValue { sheet_id: String, cell: String, value: OfficeCellValue },
    SetCellFormula { sheet_id: String, cell: String, formula: String },
    SetChartTitle { chart_id: String, expected_title: String, title: String },
    SetChartSeriesRanges { series_id: String,
        categories: OfficeRange, values: OfficeRange },
    SlideReplaceText { target_id: String, start: u32, end: u32,
        expected_text: String, new_text: String },
    ReorderSlides { slide_ids: Vec<String> },
}
pub fn inspect_office_document(format: OfficeFormat, source: &[u8])
    -> Result<OfficeInventory, OfficeEditError>;
pub fn prepare_office_edit(format: OfficeFormat, source: &[u8], edits: &OfficeEditSpec)
    -> Result<PreparedOfficeEdit, OfficeEditError>;
```

All types have explicit serde contracts with unknown fields denied; tagged enums use snake case. `OfficeCellValue` is tagged string/number/boolean/blank. `OfficeRange` contains an inventory sheet ID and two A1 coordinates, not arbitrary formulas or package paths. Text `start`/`end` are Unicode scalar offsets in the inventoried paragraph, end exclusive, with exact preimage text validation. IDs are opaque deterministic identities valid only for the exact source SHA-256 checked by the host. The crate compares original/candidate part bytes directly and does not need a new hashing dependency.

`PreparedOfficeEdit` has private fields and accessors for candidate bytes, before/after content previews, typed changes, warnings, calculation states, and preservation proof. It cannot be deserialized from model input as an approved candidate. `OfficeEditError` has actionable variants: invalid package, unsupported feature, resource limit, stale target, ambiguous range, invalid operation, calculation unavailable, verification failed. No arbitrary XML, XPath, archive path, relationship, stylesheet, script, or byte-patch operation is public.

## Package safety and preservation

Initial limits: 4 MiB compressed input and output; 1,024 entries; 4 MiB per XML part; 16 MiB total expanded bytes; 500,000 XML events; depth 64; 32 sheets; 128 slides; 10,000 materialized cells; 1,000 points per chart series; 128 operations and 64 KiB serialized operations. Inventory/review output is paginated/bounded independently; never serialize the entire unbounded document into a tool result.

1. Index the complete archive with rawzip. Check central/local agreement, flags, supported STORE/DEFLATE methods, CRCs, actual expanded sizes, data descriptors and non-overlapping entry ranges. Reject encryption, unsupported extras, ambiguous/case-colliding names, symlink attributes, ZIP64/multidisk, traversal, absolute paths, backslashes, URI-like names and trailing garbage. Do not extract archive entries to disk.
2. Validate every XML and relationship part, including orphan parts. Reject DTD/entity declarations, non-UTF-8 XML, processing instructions other than the XML declaration, unbound namespaces, duplicate attributes, malformed characters and resource overruns. Index namespace-expanded names while retaining original byte spans. Do not normalize the whole XML tree on output.
3. Admit only enumerated inert OOXML content types, relationships and namespaces needed by the support table. Reject macro/VBA/XLM, ActiveX, OLE/embedded packages, altChunk, signatures, executables, scriptable image types, external data connections/images/attached templates and active external links. Ordinary `TargetMode=External` hyperlink relationships using valid HTTP, HTTPS or mailto targets are retained unchanged and enumerated as preserved-only; never fetch, remotely render or automatically follow them. Reject credential-bearing URLs, unsafe schemes, UNC/file paths, encoded control characters and ambiguous targets. Preserve internal PNG/JPEG media without decoding or displaying it in v1. Hyperlink editing is outside this version; no retained link is silently removed.
4. Resolve internal relationships relative to their owning part using bounded package URI normalization. Legitimate `../media/...` relationships are allowed only when resolution remains inside the package and reaches an existing admitted part with matching type. Archive member names themselves never contain `..`. IDs must be unique within each relationship set; shared image relationships remain shared. Missing targets, encoded separators, schemes and root escapes are refused.
5. Inspect field instructions and formula syntax as well as relationships: DOCX fields are preserved only for `PAGE` and `NUMPAGES`; unknown fields, DDE, includes, links and macro actions are refused. Formula admission is independent of the evaluation subset, with external references, DDE, UDFs, `CALL`, `EXEC`, `REGISTER`, `RTD`, `WEBSERVICE`, `HYPERLINK` and unknown functions rejected. The runtime never executes these constructs or emits newly activated content.
6. Resolve typed edits to validated targets; detect overlapping operations and stale/ambiguous text before generating bytes. Apply escaped text/value changes only to approved spans. Add `xml:space=preserve` only when required for edited text. Preserve every untouched span in a changed XML part.
7. Repackage without reconstructing imported files through `DocumentSpec`. Preserve every untouched part's exact uncompressed bytes, original entry order and names; ZIP compression/header details and whole-file hash can change. Reuse validated compressed data when the pinned rawzip API supports it; bounded recompression is an acceptable fallback. Report container normalization, not an impossible byte-identical whole-file claim.
8. Reopen the candidate through the same admission path. Verify relationships, types, expected edited semantics, exact untouched-part equality, unchanged raw spans in touched parts, and an explicit allowlist of derived cache/relationship changes. Fail the whole proposal on any unexpected part addition/removal/change. Unchanged template image/style/master/relationship parts are equality-checked, not merely counted.

Apply existing secret/path screening to inventoried logical text, operations, review and published candidate; do not return content which the current document path would exclude. Preview text is untrusted data rendered as text, not markup supplied by the document.

## Tool and HTTP contracts

Source reference (host only): `{kind:"attachment", attachment_id}` or `{kind:"workspace", file_path}`. Attachment references must belong to the current ordinary owner chat/session and current selected message or already authorized source in that session; arbitrary known IDs are insufficient. Workspace sources use the existing capability-relative ordinary-file policy. Reject edits targeting `uploads/` and other reserved/excluded paths.

| Tool | Arguments | Result / authority |
| --- | --- | --- |
| `inspect_office_document` | `source`, optional bounded opaque `cursor` | Source hash, origin label, coverage, paginated editable IDs and bounded values, preserved-only areas and limitations. Explicit read grant. |
| `propose_document_edit` | `source`, `expected_sha256`, `file_path`, `expected_destination`, `operations` | Creates a private immutable candidate and review, returns `proposal_id`, `review_sha256`, summary and review URL. Explicit grant; read risk because it cannot mutate/publish workspace files. |
| `apply_document_edit` | `proposal_id`, `review_sha256`, `file_path`, `expected_destination` | Exact owner approval for the server-resolved review; publishes only the stored candidate. Explicit write grant; cannot run without native approval/action journal authority. |

`expected_destination` is `{state:"absent"}` or `{state:"present",sha256}`. Replacement defaults to the source hash only when destination equals a workspace source; all other existing destinations require their own current hash. Format must match the destination extension. Source uploads are always saved to a copy. `propose_document_edit` is inert private staging, with per-profile quotas and no model-controlled storage paths; it is not proof of owner approval.

New owner-only routes:

- `GET /api/document-edits/{proposal_id}`: bounded full `DocumentEditReview` including version, immutable review digest, source provenance, destination expectation, before/after previews, every direct and derived change, preserved-only coverage, calculation status and limitations. Fetch requires the same profile/workspace/session authority; IDs confer no access. Use `no-store`.
- `DELETE /api/document-edits/{proposal_id}`: cancel a pending draft; cannot undo a published file. It settles an associated pending approval/stops its waiter and deletes staged bytes after physical readers retire.
- Keep `POST /api/approvals/{id}/decision` for apply/deny. The exact document marker permits `allow_once` or `deny` only. A decision references the approval revision; the approval itself binds proposal/review/candidate/source/destination hashes. No second independent apply endpoint.
- Keep `/api/workspace/file/download?path=...` for published files. Accept either the existing generated verifier or a current server-held imported-edit publication receipt matching workspace, normalized path and exact bytes/hash. A candidate, embedded source marker, sidecar supplied by the model, or arbitrary imported file never qualifies. PDF always uses the existing generated verifier.
- Extend the current workspace preview response additively with imported-edit provenance/preview quality and the same published receipt check. Do not let raw Office bytes enter the HTML artifact iframe.

Tool results stay below the ordinary spill threshold (target 12 KiB). A review is at most 64 KiB; a proposal exceeding it is refused or split into a smaller proposal, never approved with an incomplete change list. Inventory pages are at most 12 KiB, with a cursor bound to source digest and requested scope. No exception to generated-source result validation is necessary.

The versioned public projections are concrete wire objects, separate from private custody records:

```text
OfficeInventoryPage = {
  version: 1, format: "docx"|"xlsx"|"pptx", source_sha256: string,
  source: {kind: "attachment"|"workspace", display_name: string},
  targets: [{id: string, kind: "word_text"|"cell"|"chart"|"series"|"slide_text"|"slide",
    label: string, editable: boolean, value: typed bounded content,
    permitted_operations: string[]}],
  coverage: {total_targets: integer, returned_targets: integer, complete: boolean},
  limitations: string[], next_cursor: string|null
}
DocumentEditReview = {
  version: 1, proposal_id: string, review_sha256: string,
  source: {kind: "attachment"|"workspace", display_name: string, sha256: string},
  destination: {file_path: string, expected: ExpectedDestination},
  candidate_sha256: string, format: "docx"|"xlsx"|"pptx",
  before: OfficeContentPreview, after: OfficeContentPreview,
  changes: [{operation_index: integer|null, kind: string, label: string,
    before: typed bounded content, after: typed bounded content, derived: boolean}],
  preservation: {unchanged_parts: integer, edited_parts: integer,
    unchanged_bytes_verified: boolean, container_normalized: boolean},
  calculation: {state: "not_applicable"|"complete_subset"|"needs_external_recalculation"|"calculation_error",
    calculated_cells: integer, deferred_cells: integer, error_cells: integer},
  limitations: string[], expires_at_ms: integer,
  state: "prepared"|"applying"|"applied"|"cancelled"|"expired"|"invalidated"|"recovery_required"
}
OfficeContentPreview = tagged union:
  word {paragraphs: [{label, text, runs: [{start,end,style_label}]}]}
  spreadsheet {sheets: [{id,name,visibility,cells: [{address,value,formula,calculation_state}]}],
    charts: [{id,title,type,categories,series,calculation_state}]}
  presentation {slides: [{id,ordinal,hidden,title,text_blocks}]}
```

Typed bounded content uses a closed scalar/text/range/order/cache-status union, not arbitrary JSON/HTML. Full Rust/TypeScript DTO declarations must spell out those nested variants and string bounds before dispatch registration. Display labels are escaped untrusted text. The immutable review digest is a domain-separated SHA-256 over a canonical, versioned payload containing proposal/source/destination/candidate identity, operations, complete previews/changes/calculation/limitations and expiry; it excludes the digest itself and mutable lifecycle state. This avoids a self-referential hash and permits status updates without changing the approved bytes. The owner route returns no private filesystem roots or blob paths.

## Approval and publication sequence

1. Inspect revalidates source custody before/after the isolated parse. Propose requires the same hash, runs preparation/verification in an owned worker, and persists the immutable candidate before announcing its review.
2. Before the normal tool gate, resolve `apply_document_edit` to a server-owned witness. Verify run/profile/session/clear generation, proposal state/expiry, source hash, destination expectation, candidate hash and review digest. The approval projection includes only validated identity and review references, plus a bounded summary; the model cannot create the exact marker.
3. Reuse `ApprovalGate::check_exact` with the original apply call for policy denies and the server-owned projection for the owner. Exact review cannot be satisfied by remembered allows, write-policy allow, smart approval, helper/delegated approval, or a claimed model `approved` flag. Native policy denies still win. Existing exact instruction behavior remains intact.
4. `ApprovalCard` loads the review from the owner route and shows file name, origin, all changes, before/after content, calculation warnings and preservation limits. It disables Apply until the full matching review is available. Labels: **Apply these changes**, **Keep original**, **Content preview**, **Open in Office to check page layout**. No raw ZIP/XML/hashes in the normal UI. The hash remains in the underlying contract.
5. After the owner decision and action-intent receipt save, repeat live grants/deny/profile/workspace/run/source/destination checks. In the same durable transaction that enters `applying`, reserve imported-publication receipt capacity for the destination, accounting for concurrent reservations and the 512-path quota. Reserve the destination exclusively, so competing proposals cannot reuse its receipt/reservation concurrently. A full registry fails before file mutation. Only after this save succeeds, serialize file publication with existing native document writes. Reuse `checkpoint_mutate`; add only the exact new apply tool to its admission. The transform proves the current durable reservation and captured preimage before returning candidate bytes. Commit/release/recover reservations with the intent; unreserved mutation is forbidden.
6. If checkpoints are disabled, use the existing `WorkspaceBytes` compare/replace or create-new atomic path with the action journal's final authority callback. Never fall back to an unchecked write. Recheck attachment record/hash or workspace source identity immediately before publication; no awaited work spans the final destination write lease. The source is a reviewed snapshot, not a promise to lock arbitrary external applications.
7. Read back and verify the published bytes. Save an imported-edit receipt and finish the action result before reporting success/download. The result includes exact `path`, `sha256`, size, `download_url`, provenance kind, calculation status and configured checkpoint ID. Replay of the same proposal returns its receipt only while the destination still matches; it never repeats the write.

## Persistence, restart and revocation

Add a bounded `DocumentEditRegistry` to the host's existing control-plane snapshot (default empty for old snapshots). The coordinator assigns a single next snapshot version across features. Store metadata/receipts there so JSON and PostgreSQL modes behave consistently; binary candidate blobs remain under an owned profile-adjacent `document-edits/` directory, with names derived by the host, safe file handles, digest/size verification, atomic publication and no symlinks. Missing local storage means this feature is unavailable, even with database metadata; it must not improvise workspace staging.

Metadata binds profile identity, canonical workspace identity, agent/session/clear generation, source upload ID or workspace path and source hash, destination and expected state, complete operation digest, candidate digest, review digest/version, creation/expiry, state, approval identity, native action identity, checkpoint and result receipt. Persist no credentials. States: `prepared -> applying -> applied`; terminal `cancelled`, `expired`, `invalidated`, `recovery_required`. There is no reusable `approved` state or durable approval token.

Pending drafts expire after 30 minutes, with at most 8 drafts and 32 MiB candidate storage per profile, and a bounded total metadata budget. Reject new drafts at quota rather than silently evict an approval in flight. Applied receipts retain the latest current imported-edit provenance per workspace path (maximum 512 paths, including reserved new paths; explicit quota error before the effect when full). Reservations for existing published paths consume no additional path slot but still exclude simultaneous applying proposals for that destination. Restart preserves reservations until reconciliation proves commit or safe release; cancellation before an effect releases one durably. Unexpected failures after an effect retain the uncertain/recovery-required path. Test a full registry, competing proposals for the last path slot, same-path contention, restart and failed reservation persistence. Audit history remains in native action/approval history. Discard pending candidates only after all physical readers retire. Applied candidates can be removed once durable publication receipts/readback are complete; downloads read the actual workspace file.

On restart, pending native approvals expire as today, and prepared drafts become non-applicable until re-proposed in a fresh ordinary run. Do not resume a write automatically. Reconcile an `applying` intent using the exact destination preimage/candidate hashes, native journal/checkpoint evidence and stored owner decision: candidate bytes observed plus complete evidence may finalize a receipt without another file mutation; unchanged preimage requires a new review; missing/conflicting/unprovable evidence becomes `recovery_required`. Failure to persist a post-write receipt is an uncertain effect, never a clean failure followed by blind retry.

Stop, session clear/deletion, agent deletion, profile loss, workspace switch, explicit cancellation, tool revocation or new deny invalidates pending application. Check again after every await and at the final synchronous write boundary. Download/preview always requires current owner authentication, matching profile/workspace and exact current bytes; a revoked tool cannot make new edits. Clearing a chat does not itself delete a published workspace artifact. Changing a published file invalidates its receipt and download eligibility until a fresh verified edit or generated verification succeeds.

## Preview and fidelity claims

The always-available preview is a structural content preview: paragraphs/table text for DOCX; changed cells/formulas and supported chart data plots for XLSX; slide order/text for PPTX. Typed React components render plain data. Template images are listed as retained, not decoded/rendered. Hidden sheets/rows and changed hidden content are labeled. Unsupported features and truncated inventory coverage remain visible; every applied edit and derived change must still be in the complete review.

Structural round-trip proof is mandatory in the runtime. Pixel fidelity is a separate claim: v1 does not reproduce Word pagination, Excel layout or PowerPoint text fitting. Longer text may overflow an existing slide box; preserve its geometry and state this before applying. Any future visual renderer must be a separately configured, isolated Office engine with no macros/network/external links; it cannot be a hidden shell fallback. It must return trusted render receipts, not document-supplied HTML. An unavailable renderer never becomes a claimed successful visual check.

## Meaningful verification

- Independent template fixtures for each format with styles, internal PNG/JPEG relationships and preserved-only content; edited output reparses and all untouched parts/spans compare exactly. Include several editors' namespace/relationship layouts and legitimate parent-relative targets.
- DOCX: Unicode ranges, XML escaping, whitespace preservation, repeated phrases, uniform/mixed run styles, table text, tracked-change/field/control refusal, and unchanged headers/footers/styles/images.
- XLSX: literal formula-looking strings, shared-string isolation, typed values/style preservation, absent cells, merged anchors, subset calculations/dependencies/errors/cycles/budgets, deferred formula caches, calc-chain removal, affected charts, title/series edits and cache/range consistency. Reject edits which could stale an unsupported pivot/chart cache.
- PPTX: shape/table text, duplicate shape IDs scoped per slide, Unicode offsets, exact slide permutation, hidden slides/custom shows, preserved masters/layouts/notes/images/relationships, unsupported rich text and chart edits.
- Adversarial corpus: ZIP bombs/traversal/overlap/CRC/local-central disagreement, case collisions, malformed/DTD XML, relationship escapes/external references, hidden macros/objects, unsafe fields/formulas and unknown content types. Rejection has zero target effects.
- Worker: wrong readiness/version/type, oversized/trailing frames, malformed responses, timeout/cancel/drop, memory/process limits, pipe flood and physical slot retirement. Non-Windows fails closed pending an equivalent proven resource supervisor.
- Host: cross-agent/session/upload IDs, workspace/profile changes, upload/destination/candidate/review tampering, policy deny/revocation during approval/save/final fence, remembered allows, helper/RPC attempts, concurrent writes, stop/delete, failed persistence, restart at every publication phase, exact replay and checkpoint restore.
- Download/preview: old generated files remain accepted, tampered generated source remains rejected, arbitrary imported files remain rejected, published edited receipts survive restart, stale/forged/wrong-workspace receipts fail, authentication and existing response headers remain enforced.
- UI/SDK: a complete review precedes Apply, safe plain-text rendering, unavailable/changed review disables Apply, allow-once/deny only, ordinary approval behavior unchanged, honest formula and layout labels, keyboard/focus behavior and authenticated downloads.
- Independent render acceptance: on a controlled machine with an explicitly available Office engine, open/re-save/render representative fixtures and compare expected text, formulas, chart series, styles and slide ordering. Record engine/version and actual results; these are release evidence for the tested corpus only, not universal fidelity. No real accounts or provider calls are needed.

## Dependencies and ownership request

Reuse the existing pinned `rawzip 0.5.2`, `quick-xml 0.41.0`, `flate2`, `serde`, `serde_json`, and host `sha2`, workspace file custody and Windows process custody. No new dependency, manifest edit, office automation service or formula crate is proposed initially. Confirm raw-copy API availability in the pinned rawzip source before using it; Context7's upstream docs can be newer. The fallback is bounded recompression plus exact uncompressed-part checks.

Relevant current documentation fetched through Context7: rawzip [writer/compressed-entry model](https://github.com/nickbabcock/rawzip/blob/master/docs/02-performance.md), quick-xml 0.41 [byte positions](https://docs.rs/quick-xml/0.41.0/quick_xml/reader/struct.Reader.html#method.buffer_position) and [namespace-aware reading](https://docs.rs/quick-xml/0.41.0/quick_xml/reader/struct.NsReader.html). These support a byte-span editing design; they do not supply an Office editor or a fidelity guarantee.

Feature-owned new paths:

- `packages/core-rust/crates/anima-document-content/src/editing/{mod,types,package,xml_spans,word,spreadsheet,formulas,charts,presentation,review}.rs`
- `packages/core-rust/crates/anima-document-content/tests/editing_{package,word,spreadsheet,formulas,charts,presentation}.rs` and `tests/fixtures/editing/`
- `hosts/rust-daemon/src/document_edits/{mod,registry,store,service,worker,approval,tests}.rs`
- `hosts/rust-daemon/src/tools/document_edits.rs`, `hosts/rust-daemon/src/routes/document_edits.rs`, `hosts/rust-daemon/src/routes/tests/document_edits.rs`
- `packages/sdk/src/document-edits.ts` and `document-edits.spec.ts`
- `apps/web/src/components/documents/{DocumentEditReview,DocumentEditApproval,OfficeContentPreview}.tsx` and matching `.test.tsx` files

Exact existing shared files requested (coordinator must assign before edits):

1. Portable export: `packages/core-rust/crates/anima-document-content/src/lib.rs`.
2. Worker custody reuse: `hosts/rust-daemon/src/attachments/documents/worker.rs` and its `worker_tests.rs`; add an extracted `worker_custody.rs` beside them if needed. Keep legacy extraction READY/schema behavior unchanged while a separate edit job uses its own versioned protocol and the same physical slot.
3. Host setup/persistence: `hosts/rust-daemon/src/lib.rs`, `src/main.rs`, `src/state.rs`, `src/control_plane_store.rs`, `src/app/persistence.rs`. The registry is feature-owned; snapshot wiring/version is coordinated.
4. Dispatch/grants: `hosts/rust-daemon/src/tools.rs`, `src/approvals/policy.rs`, `src/state/approval_state.rs`, `src/filesystem_checkpoints/service.rs`, `src/state/run_commit.rs`, `src/agent_runs.rs`. Wire the registry/service into the ordinary run context; contexts without owner custody fail closed. Reuse `gate.check_exact`; do not rework the general gate. Extend final authority validation in `src/agent_runs/action_receipts.rs` to compare the live applying proposal and exact witness under the existing state authority guard.
5. Evidence: `hosts/rust-daemon/src/tools/completion.rs` and `src/tools/completion/evidence.rs` to recognize successful apply and inspected source/candidate evidence; do not count a proposal as a file write.
6. HTTP/contracts: `hosts/rust-daemon/src/routes/mod.rs`, `src/routes/contracts/mod.rs`, `src/routes/workspace_files.rs`, `src/routes/capabilities.rs`; shared route registration/OpenAPI/capability inventory updates should be centralized. A feature-owned `routes/contracts/document_edits.rs` carries new DTOs.
7. SDK/UI glue: `packages/sdk/src/client.ts`, `src/index.ts`, `src/workspace.ts`; `apps/web/src/lib/daemon-api.ts`, `src/lib/agent-access.ts`, `src/components/SettingsPanel.tsx`, `src/components/sessions/ApprovalCard.tsx`, `src/components/WorkspaceFiles.tsx`. Feature-owned components keep shared edits small. No edit to the already-dirty results panel is required.
8. Focused existing regression tests as needed: `hosts/rust-daemon/src/tools/tests.rs`, `src/agent_runs/checkpoint_admission_tests.rs`, `src/routes/tests/workspace_download.rs`, `packages/sdk/src/workspace-files.spec.ts`, `apps/web/src/lib/agent-access.test.ts`, `apps/web/src/components/SettingsPanel.test.tsx`, `apps/web/src/components/sessions/ApprovalCard.test.tsx`, `apps/web/src/components/WorkspaceFiles.test.tsx`. Keep new cases primarily in feature-owned files.
9. Shared publication serialization: `hosts/rust-daemon/src/tools/generated_documents.rs` needs a narrowly scoped helper/lock visibility change, or extraction of that lock into a new shared `tools/document_publication.rs`. Its generated-source inspection, schema, renderer and validation behavior must remain identical. Both writers use the same publication serialization; compare/replace remains the protection against other ordinary native file writers.

## Coordinator decisions and integration boundaries

- Confirm that content previews plus explicit layout limits meet this implementation phase. Faithful rendered pages/slides and full spreadsheet recalculation would require a separate external-engine project; they must not be silently claimed.
- Coordinator accepted the conservative package policy with a revision: inert standard HTTP/HTTPS/mailto hyperlink relationships must be retained without fetching/following. Active external resources, unsafe fields, unsupported media and complex formula constructs remain refused; ordinary linked documents and internal image templates are supported within their other limits.
- Assign one writer for snapshot version, route/OpenAPI registration, tool registry/access-profile entries, SDK export/client and capability presentation. Other feature chats can supply additive integration snippets.
- File editing does not authorize sending documents, accessing connectors/remote drives, borrowing credentials, invoking a shell/Office engine, or changing real accounts. If another feature supplies a remote file, it must first produce a locally owned authenticated attachment with its provenance; this editor does not inherit that connector's authority.

The initial design phase changed only the two assigned documents. The subsequent partial implementation release added the portable editor, regression tests/fixtures and draft host registry/approval leaves. See the plan's implementation checkpoint for actual verification and the remaining held integration. No manifests, real accounts, credentials, commits, deployments or generated inventories were changed by this feature chat.
