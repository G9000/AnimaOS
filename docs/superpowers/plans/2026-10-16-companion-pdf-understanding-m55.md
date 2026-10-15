# M55 selected PDF text understanding implementation plan

Status: controller-bound lean plan on `g9000/companion-pdf-understanding-m55`,
cut from verified M54/PR60 main `186446cfb996a8f6df085ab6e45eb24c5d0a0b16`.
M54 source `c1d3a7b895f2eece8c35cf1c88d5f76a5e040fc6` matches tested/merged
tree `60e2e5ada6e978fba1c71c95547d77ba1b30be05`. Fresh M54 Rust2766/25intentional
ignores/44suites,12parsercases,two explicit physical cases/fmt/build, SDK151/three
service skips and web1366/115files/type/build passed. Its 51publication/44source/
58boundary hashes and unrelated nx.json were preserved; this is M54 evidence.

Publish this plan/master/handover, then root records a tests-only real-handler
missing-PDF-context RED before dependency/source changes. Root has reviewed the
lean draft, public parser source and the five binding rulings below; this is not
a separate pre-flight audit. Binding rules supersede differing draft interfaces,
loader/encryption assumptions and parent-versus-worker resource wording.
No M55 implementation or acceptance has passed yet. No actual model, metadata
refresh, ordinary daemon/account/DB server/container/deployment is released.
M47/M52 cases remain immutable/consumed and M52 strict quality PARTIAL.
Exact resume: `.superpowers/sdd/2026-10-16-companion-pdf-understanding-m55/progress.md`.

## Historical lean draft

## Deliverable and exclusions

Add bounded text-layer extraction for selected PDFs in the existing owner Web
message flow. Preserve the PDF original and its download. Associate every
included text block with a 1-based PDF page locator. Expose finite extraction
and text-coverage status next to the existing attachment. Use selected text as
quoted, untrusted user data only.

M55 supports Windows only through the already-owned document worker. It does
not render pages, OCR, decrypt/password-prompt, repair malformed files, inspect
images, run converters, contact a service, or provide a general PDF viewer.
Empty text on a page has unknown cause: scanned image, blank page, or unsupported
text encoding. Do not label it as scanned with certainty. Model answer quality,
citations, live providers, accounts, and overall PAR-08 remain outside this
slice.

## Reuse and dependency decision

The checked local source receipts in
`.superpowers/sdd/2026-10-16-companion-pdf-understanding-m55/dependency-source/`
identify `pdf-extract 0.12.1` (MIT; archive SHA-256
`e5c4820f5811e424ce037d08493ac87752ecc54339e0f1e40c3d17da93278861`; edition
2018, no declared MSRV) and its `lopdf 0.42.0` dependency (MIT; MSRV 1.85;
archive SHA-256
`25aab26d99567469098e64a02f42679f8965c6401263eefa31d8f2dcc37a221c`). The
workspace compiler is 1.93 per the root's private receipt. This is source
inspection, not a build or proof of complete transitive MSRV/license
compatibility. Root must retain the lockfile/dependency review as a release
gate; do not upgrade the toolchain to accommodate an unverified transitive.

| Reuse route                     | Decision and reason                                                                                                                                                                                                                                                                                                          |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Direct Hermes component/package | Reject. The inspected Hermes reader delegates PDF to optional `firecrawl-anydoc`, performs whole-document conversion, and has an optional separate `pdftotext` scan-coverage process. This does not provide the selected local PDF page API or M54 process/resource ownership. No Hermes runtime or converter is included.   |
| Supported adapter               | Reject for this slice. Another process/converter adds installation, license, executable and lifecycle surfaces without a need after a local Rust candidate was checked. No remote, OCR, shell or conversion fallback.                                                                                                        |
| Narrow attributed port          | Reject parser port. PDF object/content parsing is a substantial parser, not a small behavior port. Reuse the library and keep only the source pin in this plan. Hermes root MIT notice (Nous Research, 2025) remains relevant only if a future implementation copies substantial Hermes code; no such code is selected here. |
| Rust dependency                 | Proposed: exact `pdf-extract = 0.12.1`, default features disabled, using its checked `lopdf 0.42` integration. Both checked archives are MIT. Freeze exact Cargo feature and lock changes only after root's dependency compatibility review. No alternate parser or hand-written PDF parser in this milestone.               |

### Inspected public API and parser caveats

The checked source exposes `lopdf::Document::load_mem`,
`Document::is_encrypted`, `Document::get_pages`, and `pdf_extract::output_doc_page`.
`PlainTextOutput::new` accepts a `&mut dyn std::io::Write` via the library's
`ConvertToFmt` adapter, allowing a bounded writer instead of an unbounded
`String`. The proposed worker flow is: load the bounded bytes, reject encrypted
documents, obtain page IDs/count, validate the strict page range, then invoke
`output_doc_page` once for each page using one bounded sink for that page. The
checked source has `ConvertToFmt` for `&mut dyn Write`, `&mut String`, and
`&mut File`, not a blanket implementation for a custom writer. Therefore the
call site must explicitly coerce the concrete sink:

```rust
let sink: &mut dyn std::io::Write = &mut bounded;
let mut output = PlainTextOutput::new(sink);
output_doc_page(&doc, &mut output, page_number)?;
```

The writer can retain only the bounded valid UTF-8 prefix, discard later bytes,
and report each full input slice as written so page processing continues and
coverage remains complete. Track `truncated` and nonblank-text state separately
from retained bytes. Release the per-page borrow before reading those counters.
Do not construct a full page `String` before writing it.

Do **not** call `extract_text_from_mem_by_pages`: inspected code loops while each
page succeeds and ends at the first error, returning a successful vector with
no error for later pages. That would hide incomplete coverage. `output_doc_page`
returns a page-level result and makes failure visible. The checked `pdf-extract`
source contains multiple `unwrap`/`expect` paths on document/page objects and
glyph decoding; this is why parsing stays only inside the owned, bounded worker.
Catch unwind at the worker boundary as a closed malformed/unavailable result,
without stdout diagnostics, process restart, or usable partial text. The worker
Job and wall deadline remain necessary; a capped output writer alone does not
bound the parser's internal document/object allocations or CPU work.

The source-level API finding does not prove the library safe against hostile
PDFs. The physical 256 MiB process and job committed-memory limits, one-process
kill-on-close, 10-second parent deadline, no inherited secrets, bounded IPC and
observed retirement remain mandatory. Do not claim a CPU quota, filesystem or
network sandbox, or general security isolation.

## Frozen ownership and source authority

Keep the M54 separation:

- `packages/core-rust/crates/anima-core` owns host-neutral versioned bounded
  extraction/snapshot DTOs, validation, rendering and exact cost accounting.
- `packages/core-rust/crates/anima-document-content` owns the pure parser
  bridge. It receives only bounded PDF bytes and fixed limits. It imports no
  daemon, store, HTTP, account, process configuration or provider.
- `hosts/rust-daemon` owns selection, source identity checks, fixed worker mode,
  cancellation and physical resource lifecycle, persistence, reload/hydration,
  public summary projection and the adapter bridge for handler tests.
- SDK/web starts only after native/public summary freeze; change the existing
  session types and attachment display/chip only. Retain original download and
  existing text/image behavior. No new preview, page viewer or CSS redesign.

Only selected own Web-chat attachments may trigger extraction. Before reading,
the parent binds agent, session, selected opaque ID, durable attachment record,
MIME, source size, storage root and SHA-256. Recheck source identity before
parsing and before committing the settled snapshot. The worker never receives
the path, source hash, IDs, user metadata, grants, credentials or caller options;
the parent inserts provenance independently. All client admission paths strip
reserved `documentExtracts` metadata.

Extraction starts after durable run start and Stop registration, outside daemon
or history locks, and before context/file admission. A current matching settled
snapshot may be reused; stale/missing/malformed snapshots are omitted without
repair or automatic reparse. A stopped, expired, interrupted, failed or
uncertain run cannot gain a ready snapshot or late excerpt. Helpers, connectors,
steered new documents, arbitrary `read_file`, and other attachment automation
do not start a PDF worker.

## Limits and closed behavior

| Boundary                | Binding proposal                                                                                                                                                                                                                                                                                                                                                 |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Host and worker         | Windows M54 worker only. Reuse checked process/job ownership, 256 MiB process and Job commit limits, one process, kill-on-close, 10-second parent deadline covering spawn/handshake/write/read, process-wide one-slot admission and retained lease until observed retirement. No queued parse or retry.                                                          |
| Input                   | 2 MiB maximum PDF payload, plus one sentinel probe in a host physical read. Above the bound: `too_large`, zero bytes to worker. Preserve original upload/download.                                                                                                                                                                                               |
| Format                  | Require selected record/MIME plus PDF signature and parser validation. No extension-only classification. Reject encryption before page output; do not call `decrypt`, `output_doc_encrypted` or password-taking helper. No repair or fallback.                                                                                                                   |
| Pages                   | Strict range: documents with 1 through 100 pages may be extracted; 0 pages maps to `no_text`; more than 100 pages returns `too_large` with no blocks. Process every accepted page once to report honest coverage. Coverage is actual pages examined and actual pages producing nonblank text, not an image/scanned determination.                                |
| Output                  | At most 16 KiB of combined rendered quoted document data, including labels and page locators, shared by selected DOCX and PDF excerpts. PDF text may be a valid UTF-8 prefix and return `partial`/`text_limit`; malformed page/parser failure discards all blocks for that PDF. Keep bounded page-block count and per-block text under the same aggregate limit. |
| Selection               | At most two **usable documents total** per message, selected order, counting DOCX and PDF together. Status-only rows remain subject to ten snapshots per message. Other selected originals remain saved with `context_omitted`/`selection_limit`; do not parse them.                                                                                             |
| Physical reads          | Across hydration, at most four current/newest document sources total per request, with each source read capped at 2 MiB plus one sentinel (8 MiB plus four sentinel bytes maximum). Retain each read lease until the physical read completes. Omit older sources without reparse or mutation.                                                                    |
| Public and private data | Private snapshot stores normalized format, version, source ID/hash, typed locators, text, status, omissions, and optional PDF coverage. Public projection never contains extracted text, hash, storage root, worker diagnostic or arbitrary parser string.                                                                                                       |
| Cancellation            | Run Stop/deadline/caller Drop wins before worker result commit. Physical slot and source-read leases remain held through actual retirement/completion. No late output is delivered.                                                                                                                                                                              |

Suggested closed outcomes reuse the M54 status enum: `ready`, `partial`,
`unsupported`, `encrypted`, `malformed`, `too_large`, `no_text`, `unavailable`,
`cancelled`, `context_omitted`. Add only a PDF `text_layer_only` omission if the
UI needs the explicit coverage label; keep `selection_limit`, `text_limit` and
`context_limit`. Do not add `page_limit` because page count above 100 refuses
with no blocks. A page output/parser error is `malformed` with no blocks. A
worker panic/timeout is `unavailable`/`cancelled`, with no blocks. An output cap
reached after successful page processing yields `partial` with a valid UTF-8
prefix and `text_limit`.

## Proposed Rust contract (names for plan review; no code/API claim)

Normalize private snapshots to V2, with a strict versioned codec and
`deny_unknown_fields` at each serialized layer. An illustrative shape:

```rust
enum DocumentFormat { Docx, Pdf }
enum DocumentLocator {
    DocxParagraph { paragraph: u32 },
    PdfPage { page: u32 },
}
struct DocumentBlockV2 { locator: DocumentLocator, text: String }
struct PdfCoverage { page_count: u32, text_page_count: u32, included_page_count: u32 }
struct ExtractedDocumentV2 {
    version: 2,
    format: DocumentFormat,
    status: DocumentExtractionStatus,
    blocks: Vec<DocumentBlockV2>,
    omissions: Vec<DocumentOmission>,
    pdf_coverage: Option<PdfCoverage>,
}
struct DocumentSnapshotV2 {
    version: 2,
    attachment_id: String,
    source_sha256: String,
    extraction: ExtractedDocumentV2,
}
```

This shape is conceptual; root freezes exact naming/serde representation. Use a
typed locator enum so PDF page numbers cannot be confused with DOCX paragraph
numbers. Validate format-locator compatibility, sorted unique page/paragraph
locators, nonempty text blocks, per-status empty/nonempty rules, at most two
usable snapshots, at most ten total snapshots, and the exact shared render cap.
All counts are bounded and nonnegative; failure/unknown coverage is `None`, not
fabricated zeroes. V2 stays private under the reserved metadata key.

Backward compatibility is explicit:

- Decode existing strict private V1 DOCX snapshots into the V2 in-memory model.
  On read/rebuild/hydration, do not rewrite persisted V1 records, recalculate
  the source hash, or reparse the original.
- New successful/status writes use private V2. DOCX V1 paragraph locator and
  exact rendered representation/cost remain equivalent. Existing V1 status
  semantics remain intact; when a V1 snapshot is decoded, do not fabricate
  format or PDF coverage beyond the known DOCX contract.
- Keep the current public DOCX summary exactly V1:
  `{version:1,status,paragraphCount,omissions}`. For PDF publish a distinct V2
  summary with format `pdf`, finite status, pageCount/textPageCount/
  includedPageCount nullable on closed failure, and bounded omissions. This
  prevents private V2 from silently changing the existing public DOCX schema.
- SDK types form a discriminated union for existing public DOCX V1 and new PDF
  V2. Runtime guards accept historical missing summary and known DOCX V1; reject
  unknown versions/formats/enums. UI never turns missing page data into a zero
  or a ready status. Original download stays available in every state.

Document worker protocol becomes private frame V2. The fixed internal command
changes to `--document-worker-v2`; parent-selected closed format byte precedes
u32 little-endian payload length, then exactly that many bytes and EOF. Worker
result is strict V2 and bounded by existing 64 KiB stdout / 4 KiB drained
stderr; READY remains first and precedes any payload byte. The parser reads no
more than the frame says. Unknown format/protocol/extra frame/EOF mismatch is a
closed invalid output. Explicitly refuse the old internal v1 flag rather than
letting it fall through to normal daemon startup. No user-selected executable,
command args, environment or alternate ABI.

The V2 process may still parse DOCX via the existing parser route. Preserve DOCX
results and behavior through conversion/validation; the protocol version is
private and does not change DOCX's public summary version. Worker source, fixed
dispatch before normal host initialization, and exact parser/API glue require
root review before implementation. Before the ordinary daemon initializes any
host service, scan arguments and reject every unrecognized argument beginning
`--document-worker-` (including deprecated V1), so a typo or stale flag cannot
silently fall through to normal startup. No-argument daemon startup keeps its
current path.

## Rendering, budget, hydration, persistence

Render one quoted block per source locator, e.g.
`[UNTRUSTED DOCUMENT DATA attachment=<opaque-id> locator=pdf/page=7]`, with the
text JSON-quoted as in M54. Preserve DOCX locator spelling and byte-for-byte
rendered output. Include only selected source blocks. The 16 KiB ceiling counts
the complete rendered representation across both formats. Fit at complete
UTF-8 boundaries for PDF and whole paragraph boundaries for DOCX, then mark
`partial`, `context_omitted` and the appropriate closed omission.

Reserve the exact rendered UTF-8 bytes before history selection, at least one
token per document byte and calibration no lower than 1.0. Apply identically to
current message, selected history, rebuild, compaction, wrap-up and transient
hydration. No post-selection text injection. Compaction counts the same content
and the actual truncation marker within its own cap. Tool-pair boundaries and
ordinary text/image accounting remain unchanged.

At hydration, validate current attachment ownership/session/source ID/hash and
the normalized private V1/V2 snapshot before rendering. Do this for generate
and stream. A malformed V2 or stale source produces omission; never recover
from caller metadata or worker echo. Do not mutate stored messages during
transient compaction hydration. Restart reuses an exact settled snapshot only;
interrupted parsing does not replay automatically. The existing four-source
physical read bound is global to the request, not four per format or per call.

## File ownership after release

Root owns root `Cargo.toml`/`Cargo.lock`, `core-rust`/`rust-daemon` Nx target
inputs, branch and publication. Do not alter `nx.json`/`anima.yaml`.

Core/parser ownership: `anima-core/src/document_content.rs`, `lib.rs`,
`context_window.rs`; `anima-document-content/src/{lib.rs,docx.rs,pdf.rs}`,
Cargo manifest, tests and any required notice. Keep process/parser dependency
out of `anima-core`; the parser crate must not depend on the daemon.

Worker ownership: daemon `attachments/documents/{mod.rs,worker.rs}` and focused
tests; early worker dispatch in `main.rs` and minimal library export. Use no
second executable. The parent assigns/queries exact resources before READY and
bytes. Worker mode opens no listener/store/provider/credentials/config.

Host integration owns `attachments/documents/admission.rs`,
`agent_runs/attachments.rs`, `agent_runs.rs`, `state/run_commit.rs`,
`attachments/model.rs`, compaction/rebuild glue only where the frozen contract
requires, reserved metadata stripping, persistence and handler tests. Reuse
M54 behavior and minimize cross-file changes.

SDK/web ownership begins only after public projection freezes: session DTO/types
and focused tests, existing `MessageAttachments`/`AttachmentChips` and tests.
No overlap with unrelated console transcript work. Preserve download, text,
copy and speech interactions.

## Release and acceptance sequence

1. Root publishes/binds the public M55 plan and top-five risks with exact
   source/dependency provenance and this version/limit policy. This draft is
   not implementation authorization.
2. Test-only owner adds a meaningful real-handler RED on current code using an
   independently generated selected PDF and recording model adapter. Prove
   upload/admission succeeds but the selected PDF literal is absent from user
   data; prove an unselected PDF remains absent. Do not import new APIs or touch
   production source/dependency manifests for RED. No provider call, M47/M52
   replay, or fixture edits to M54 DOCX.
3. After root releases source work, freeze V2 core/worker/private codec and
   public summary/SDK contract. Root handles dependency manifests/lockfile and
   compatibility/license gate before implementation authors consume it.
4. Implement/parser tests: independently generated small one-page/multipage
   PDFs; exact page text/locators; mixed nonblank and empty pages; all-empty;
   encrypted; malformed header/xref/trailer/page content; unsupported filters;
   text output over cap; malformed page after earlier valid page; zero-page and
   101-page refusal. Assert all-page coverage for accepted inputs; on any page
   structural failure, no blocks escape. For >100 pages assert too_large/no
   blocks. Verify no OCR, decrypt, render, network, subprocess or provider path.
5. Worker tests: READY before bytes; V1/V2/unknown format frames; exact lengths,
   EOF, multiple/extra frames, malformed JSON, stderr/stdout floods; CPU spin,
   256 MiB memory pressure, 10-second parent deadline and earlier Stop; cancel
   during spawn/write/read; panic/child failure; pipe-held and retirement cases.
   Show the permit stays occupied until OS process and pipes retire; neighbor
   process survives and unrelated processes remain untouched.
6. Handler/context tests: selected versus unselected, foreign agent/session,
   changed hash, deleted session, MIME/format mismatch, forged reserved key,
   stale and unknown-version snapshot. Assert no private text/hash/root in public
   response. Cover current + history selection, compaction, transient hydration,
   generate and stream, wrap-up/rebuild/reload, exact V1 DOCX compatibility,
   V2 PDF projection, and no restart reparse. Assert 2 usable mixed-format docs,
   10 status rows, 16 KiB full rendered cap, 4 physical reads total and 8 MiB +
   four sentinels. Check tool pairs, Stop and no late result.
7. SDK/web tests: historical absent extraction field; current DOCX V1; PDF V2
   with ready/partial/failure/nullable coverage; unknown version/status; no false
   page numbers or ready state; source label is the selected attachment/page;
   unchanged download, keyboard actions and stale-owner suppression.
8. One combined native/core review, final SDK/web review, one justified fix
   wave, root-run fresh gates and exact revision/output evidence. Root decides
   whether limitations require a follow-up instead of weakening assertions.

## Root-run commands after targets are resolved (not run by planner)

First resolve exact project/task targets with `bun x nx show projects --json`
and `bun x nx show project <resolved-name> --json`; do not assume argument
forwarding or that the current target includes the new parser crate. Retain
`CARGO_INCREMENTAL=0`, one Cargo build job and two test threads for every
Cargo-backed command. The expected task set after resolution is:

```text
bun x nx run core-rust:test --skipNxCache
bun x nx run core-rust:lint --skipNxCache
bun x nx run core-rust:build --skipNxCache
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run rust-daemon:build --skipNxCache
```

Add focused parser/physical worker/handler filters only after checking the
resolved target and supported forwarding flags. For a locked running daemon,
use the repository's isolated Windows Rust validation target directory. Run
changed SDK/web `test`, `typecheck`, `build` and formatting targets with
`ANIMA_SKIP_SERVICE_TESTS=1`; resolve their exact names first. Run touched-file
format and docs/diff checks. No Astro or dev-launcher checks unless those files
change. Do not run these commands until root releases them.

## Top five risks for binding

1. **Parser panics on malformed content.** Multiple inspected unwrap/expect
   paths mean catch unwind in the fixed worker, return no text, retain the lease
   until process exit, and add malformed fixture coverage. Job/wall bounds are
   not a general security sandbox.
2. **The library materializes document structure.** 2 MiB input and 256 MiB
   Job/process commit limits are hard requirements. The writer cap bounds output
   only; `Document::load_mem`, page map creation, stream decoding and glyph work
   still require hostile synthetic resource tests and physical limits.
3. **Coverage truth and page failure.** Convenience per-page extraction hides
   first page errors. Use explicit `output_doc_page` errors for every page;
   process all pages only in the accepted 1..100 range and discard every block
   if any structural page fails. Empty page means no extracted text, not proven
   scan.
4. **Private/public version drift.** Private V1 DOCX must decode to normalized
   V2 without rewriting/reparsing; public DOCX stays V1 while PDF has a closed
   V2 discriminator. Strict codec and SDK runtime-guard regressions are release
   blockers.
5. **Cost, reads and retirement drift.** Two usable documents are DOCX+PDF
   combined; four reads are global per request. Exact render bytes, history,
   compaction, transient hydration, cancellation and actual process/read
   retirement must agree. Any uncertainty omits data and retains its lease.

**Held pending root binding:** exact Cargo lock diff and full transitive license/
MSRV compatibility; strict private V2 shape; SDK PDF V2 contract; V2 worker
frame; missing/outcome/null coverage policy. No implementation, tests, builds,
formatting, process use, or completion/parity claim occurred in this planning
step.

## Controller rulings on the risks (binding)

These rules supersede differing proposal text. M54/PR60 is merged at
`186446cfb996a8f6df085ab6e45eb24c5d0a0b16`, source
`c1d3a7b895f2eece8c35cf1c88d5f76a5e040fc6`, tested/merged tree
`60e2e5ada6e978fba1c71c95547d77ba1b30be05`. This releases a tests-only
meaningful missing-PDF-context RED after publication. Dependencies and source
implementation follow that recorded RED and a frozen contract, without another
user check-in. It releases no actual model/service/account/deployment case.

1. Ruling: reuse exact pdf-extract0.12.1 and its lopdf0.42.0 integration inside
   the existing owned Windows worker — public archive checksums/licenses/APIs
   were checked, and a new converter or custom PDF parser adds unnecessary
   installation/maintenance surfaces — cost if wrong: the dependency/lock/source/
   compiler gates block integration, without a toolchain upgrade or fallback.
   Both archives are MIT; lopdf declares MSRV1.85, pdf-extract no MSRV. Current
   compiler1.93 remains fixed. Root alone reviews/resolves the minimal lock diff
   after RED; new transitive compatibility/licenses remain a gate. No new Hermes
   parser code is copied; retain existing DOCX notices and Anima product naming.
2. Ruling: retain checked256MiB process+job committed-memory/one-process limits,
   ten-second parent wall deadline and independent physical owner — the PDF
   library materializes objects and has panic paths, so output/input limits alone
   do not bound internal work — cost if wrong: return no text and retain physical
   leases until retirement is proved. The fixed worker self-installs and queries
   its resource Job before READY; the parent owns its separate outer Job. Keep
   global four nonqueued source-read leases and existing Stop/deadline behavior.
   No kernel CPU quota, filesystem/network sandbox or graceful-shutdown claim.
3. Ruling: process every accepted PDF page explicitly with a bounded writer and
   report text coverage without guessing scan/image content — the convenience
   per-page vector helper silently stops on an error — cost if wrong: omit the
   document on a reported page failure rather than publish a misleading prefix.
   Use LoadOptions{strict:true,password:None,filter:None} with the public
   load_mem_with_options API, then reject is_encrypted() OR was_encrypted()
   before page output. Source inspection found that lopdf may internally try an
   empty password during loading and remove Encrypt; is_encrypted alone is
   insufficient. No password is supplied/requested and no encrypted text is
   released; do not claim the library performs no internal decryption. Strict
   loading is not proof of complete PDF validity: the library can skip unused
   malformed objects. Coverage concerns accepted page text only.
4. Ruling: normalize private snapshots to V2 while retaining strict V1 DOCX
   reading and the public DOCX V1 summary — page locators must not masquerade as
   paragraphs, and reload cannot rewrite/reparse old messages — cost if wrong:
   refuse inconsistent snapshots and hold publication on compatibility tests.
   Keep the outer DocumentSnapshot shape unchanged: attachmentId/sourceSha256/
   extraction; version lives inside extraction only. Use existing Rust names
   DocumentBlock/ExtractedDocumentText with a u32 version constant2, format
   docx|pdf, typed locator, text, closed status/omissions and optional PdfCoverage.
   DocumentLocator uses a strict `kind` tag: docx_paragraph{paragraph:u32} or
   pdf_page{page:u32}. PdfCoverage has page_count/text_page_count/
   included_page_count; included equals retained distinct page blocks, text
   count records all nonblank page outputs. Counts0..100, included<=text<=pages;
   zero pages only no_text with zero counts. Ready/partial need nonempty blocks,
   PDF coverage and page_count>=1. Failed/unknown coverage is absent privately
   and all three nulls publicly. context_omitted may retain known coverage with
   zero included pages. Docx has no PDF coverage. Validate format/locator/omission
   compatibility and strict unknown/version/count/order/size rules.
5. Ruling: count two selected supported documents and four current/newest source
   reads across both formats, reserve the exact shared rendered bytes before
   selection, then run one review/fix/gate sequence — per-format limits would
   silently multiply IO/context and source-only reports are not verification —
   cost if wrong: omit older/extra text and hold the merge. The first two supported
   documents in declared selection order consume the attempt allowance even
   when too large or failed; there are never ten parse attempts. Keep at most ten
   status snapshots/two usable excerpts/16KiB combined quoted data, at most four
   reads of2MiB+one sentinel per request. Preserve DOCX whole-paragraph fitting
   and byte-identical V1 rendering; PDF can retain a UTF-8 prefix with explicit
   text_limit/context_limit. No cross-message cache, Fact/knowledge write or
   automatic interrupted replay. Batch native/core review, final SDK/web review,
   one fix wave and fresh local gates; root alone executes/fmts/Git/processes.

### Exact interface refinements

- Input2MiB, strict maximum100pages: above100 is too_large/no blocks; zero pages
  is no_text/known zero coverage. For1..100 process all pages exactly once, even
  after retained text fills. Each page uses PlainTextOutput::new with an explicit
  `&mut dyn std::io::Write` coercion to a capped sink; keep only a valid UTF-8
  prefix, count nonblank output/truncation separately and discard excess writes
  while reporting their full input length. No whole-page/document String helper.
  Reported structural page errors or caught panic release no usable prefix.
- New private writes use extraction V2; existing strict V1 DOCX is normalized on
  read only, with no saved-byte/hash/canonical-record changes. A present malformed
  version never falls back to legacy. The reserved documentExtracts key remains
  stripped at every client admission boundary. Recheck parent selected ownership,
  exact source hash and expected format; child echoes confer no authority.
- Public DOCX remains exactly {version:1,status,paragraphCount,omissions}. PDF is
  {version:2,format:"pdf",status,pageCount,textPageCount,includedPageCount,omissions}.
  Its three counts are integers0..100 or all null on unknown/failure, with the
  relationships above; ready/partial require positive included/text/page counts.
  Add text_layer_only for PDF; never emit main_body_only/paragraph_limit on PDF
  or text_layer_only on DOCX. Public/private version domains are distinct. The
  web validates each closed branch, stays quiet for absent/invalid historical
  fields and keeps downloads/text/copy/speech unchanged. No new page/action/API.
- Render PDF locators exactly `pdf#page=N` inside the existing UNTRUSTED DOCUMENT
  DATA/JSON-quoted user-data frame. These are native numeric locators, not links,
  arbitrary filenames or a claim about visual/page completeness or answer quality.
- Private worker V2 READY is `ANIMA-DOCUMENT-READY-v2\n`; after it, input is one
  closed format byte (0 DOCX,1 PDF), u32LE byte length, exact payload and EOF.
  Output is u32LE strict V2 JSON length/payload and EOF, with existing total64KiB
  stdout/4KiB stderr bounds. Parent validates expected format as well as shape.
  The exact mode is --document-worker-v2, before tracing/Tokio/config/host init.
  Reject every other --document-worker-\* flag, including V1, before host init;
  preserve normal no-argument startup. No public internal-ABI compatibility claim.
- Existing M54 original DOCX fixtures/assertions and M47/M52 evidence stay
  immutable. M52 strict quality stays PARTIAL. Independent PDF fixtures, real
  handler/recording-adapter RED, private codec compatibility, current/history/
  compaction/reload/stream and both actual worker formats require fresh tests.
  All process cases are finite controlled peers/worker CLI, never daemon service.

Primary source: [pdf-extract0.12.1 API](https://docs.rs/pdf-extract/0.12.1/pdf_extract/),
[pdf-extract repository](https://github.com/jrmuizel/pdf-extract),
[lopdf0.42.0 API](https://docs.rs/lopdf/0.42.0/lopdf/), and
[lopdf repository](https://github.com/J-F-Liu/lopdf). Published source checksums
and exact selected source observations remain in the private milestone ledger.
