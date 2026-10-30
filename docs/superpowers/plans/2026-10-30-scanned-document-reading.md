# Scanned PDF and visual-document reading implementation plan

> For agentic workers: use the existing isolated worktree. Keep one owner for the source and citation design. Follow the published pstack Feature, How, Architect and Arena workflow through Codex equivalents.

**Goal:** Read scanned PDF pages, tables and charts in chat, and let the owner open evidence tied to the original page and current source permissions.

**Architecture:** A reusable byte-oriented page-evidence operation renders selected pages and optionally recognizes their text. The daemon owns source authority, the existing physical worker and request-only delivery. SDK and web code display content-free status and authenticated page evidence.

**Worktree:** C:/Users/leoca/.codex/worktrees/scanned-document-reading/animaos-kit
**Branch:** g9000/scanned-document-reading
**Base:** committed main at b563e5ea
**State:** implementation source is prepared. The image-generation chat is the sole native/Cargo/SDK build executor. It verifies independent frozen request revisions using the primary managed validation cache.

## Grounding

Current text extraction accepts PDF sources up to 2 MiB and 100 pages. PDF text snapshots are version 2 and explicitly text-layer-only. Attachment admission verifies ownership and original bytes before extraction and before publication. The Windows document worker shares one physical slot with editing, limits the process to 256 MiB and one active process, and preserves physical capacity until retirement. The existing operation deadline is ten seconds.

The current model adapters serialize images from user messages. Tool outputs are text-only. Page images require a host-created request projection after the complete result group for the assistant tool calls. They must not become durable synthetic user turns or ordinary uploaded-image records.

Project citations already bind passages to source bytes and current membership/resource access. The dialog displays a locator and text. The knowledge cache currently promises literal passage text, so OCR requires explicit provenance. The source fingerprint currently omits extraction-pipeline identity.

## Design comparison and synthesis

Two independent inherited-model Codex runners inspected the existing source and compared whole designs.

| Candidate | Scanned text | Tables/charts | Main consequence |
| --- | --- | --- | --- |
| OCR-first extraction | Searchable text works with text-only models | Labels are available, graphical relationships are lost | Small integration, incomplete visual understanding |
| Source-bound page evidence | OCR projection supports search and text-only models | Raster projection preserves visual evidence | Requires bounded raster framing and deliberate request projection |

Choose source-bound page evidence. Graft OCR-first's searchable text projection into the same evidence object. Reject provider-native whole-PDF upload and unbounded rasterization. Screened against pstack's design red flags: one evidence owner, no client paths as grants, no separate upload-authority path, no staged public protocol that makes callers coordinate capture/render/revalidate.

The Windows.Data.Pdf and Windows.Media.Ocr APIs are the first backend candidate. The Windows worker is already the production boundary. The windows crate 0.62.2 is already present in Cargo.lock. This choice avoids a new bundled PDFium DLL. It remains conditional on real rendering/OCR tests under unchanged job limits. Missing recognizer languages are a typed unavailable outcome; there is no runtime download or cloud OCR fallback.

Cursor Task roles, poteto-agent, prescribed Claude/Grok models and a different-family cross-judge are unavailable in this Codex environment. Both candidates used the inherited chat model. Independent source/design review is retained, without claiming model diversity. No pstack executable or MCP command was installed or run.

## Caller usage and data shape

The host operation takes a typed source and a bounded explicit page selection. It captures verified source bytes, invokes the owned worker, rechecks authority and bytes, and returns evidence. Citation opening resolves its source through the existing citation identity before calling the same operation.

Reusable data structures:
- PdfPageSelection: one-based, unique, ascending page numbers; at most two pages per operation.
- PixelSize: finite positive dimensions, bounded before allocation.
- PdfPageEvidence: original page number, dimensions, bounded PNG bytes, and recognized text.
- RecognizedText: available text with language and line rectangles, empty, or a typed unavailable reason. Do not invent OCR confidence scores.
- PdfEvidence: source digest, pipeline identity, total page count, selected page evidence, and explicit omissions.
- PageCoverage: requested, inspected, omitted and unavailable pages.
- DocumentAuthority, host-private: selected attachment, granted workspace snapshot, or current project source witness.

Raster bytes stay out of public metadata, transcripts and logs. A separate visual envelope preserves the existing version-2 text contract. Source digest plus pipeline/profile identity binds any derived cache entry. Cached pixels never grant access.

Initial bounds retain 2 MiB input, 100 admitted pages, the existing ten-second deadline, 256 MiB process limit and one shared physical worker slot. Proposed output bounds are two pages, 1,600 pixels on the longest edge, and 2 MiB encoded PNG per page. Validate these limits with the backend experiment. Count page images and ordinary uploaded images together under MAX_CONTEXT_IMAGES=4 and reserve image context cost before final selection.

## Throughput checkpoint

- [x] Blocking first steps. Read the actual pstack sources, inspect the existing paths, and compare two structurally distinct designs.
- [ ] Independent workstreams. Renderer/types, host worker and authority integration, then thin SDK/web clients. Native builds wait for the coordinator's lease.
- [x] Shared mutable state. An app-managed worktree and g9000 branch isolate the feature from main and the image-generation chat. Cargo still uses the coordinated absolute managed target and combined 32 GiB ceiling.
- [ ] Smallest safe decomposition. Keep one implementation owner for source/citation authority. Delegate focused renderer or client work only after the contract is fixed; reviewers do not write the same files.

## Task 1: Prove the Windows backend and reusable evidence contract

Files: packages/core-rust/crates/anima-document-visual/Cargo.toml, src/lib.rs, src/windows.rs and tests/pdf_pages.rs; root Cargo.toml; independent PDF fixtures; a minimal rendering opcode and owned-worker test harness in hosts/rust-daemon/src/attachments/documents/worker.rs.

1. Add tests before implementation for a true scanned prose page, a table/chart page, rotation, blank page, malformed/encrypted input, invalid page selection, extreme dimensions and output limits.
2. After the native lease is granted, run the focused test through tools/workspace-dev/scripts/run-task.ts with the coordinator's absolute validation target.
3. Implement byte-oriented rendering with Windows.Data.Pdf and optional Windows.Media.Ocr. No filesystem, daemon state or network policy belongs in the reusable API.
4. Prove actual PNG output, OCR availability outcomes, cancellation and physical retirement through the minimal owned-worker opcode under the unchanged 256 MiB and one-process limits. This experiment gates caller integration. Task 2 completes protocol hardening. If the backend cannot meet those limits, reframe the backend before wiring callers.

## Task 2: Extend the owned worker

Files: hosts/rust-daemon/src/attachments/documents/worker.rs, worker tests, tests/document_worker.rs and packaged worker dispatch tests.

1. Add the page-evidence operation and independently capped binary framing. Keep existing extraction operations intact.
2. Validate all headers, counts, dimensions, page identities and payload lengths before allocation or release. Use checked aggregate lengths, exact EOF, unique requested-page identities, manifest-to-PNG dimension matching, and host-recomputed payload digests. Bound the manifest to 256 KiB, recognized text to 8 KiB per page, and the complete two-page response to 4 MiB plus the manifest bound. Validate finite normalized rectangles inside the page. Reject malformed or encrypted originals before visual fallback.
3. Cover drop, cancellation, timeout, oversized output, trailing data, child failure, busy capacity and physical retirement.
4. Keep the shared process slot and original absolute deadline. Never release a permit merely because the caller stops waiting.

## Task 3: Bind evidence to sources and model requests

Files: hosts/rust-daemon/src/attachments/documents, attachments/model.rs, tools/workspace_document.rs, tools.rs and service context accounting.

1. Add one source-authority owner that reuses attachment records, workspace read grants and project witnesses.
2. Add explicit page selection to document inspection, with current permissions and no arbitrary source URLs.
3. Persist only source-bound descriptors. Each descriptor carries agent/session or project scope, source identity and digest, original pages, and pipeline/render profile. An evidence ID grants nothing. Reload recaptures current authority. Fresh model delivery and page preview always verify current bytes; historical textual tool snapshots retain their existing captured-snapshot semantics. Strip client-supplied private visual metadata.
4. Project verified tool evidence to a clearly labelled, transient user image message after the complete assistant tool-call result group. Preserve tool-call/evidence association, complete all sibling tool results before inserting the projection, and do not persist it. Test multiple calls, mixed visual/nonvisual results, retries, streaming and reasoning metadata.
5. Share the four-image allowance, reserve context cost, and emit an honest text-only fallback.
6. Carry the run cancellation signal and one absolute deadline into controlled visual request preparation. Uncontrolled adapter calls must use an explicit bounded preparation budget. Dropping preparation suppresses publication but retains physical capacity until retirement. Test source deletion/replacement and permission revocation before, during and after rendering, plus restart/history behavior and provider serialization with deterministic adapters.

## Task 4: Add chat page evidence and project citations

Files: routes/attachments.rs, routes/project_knowledge.rs, projects/knowledge, projects/context.rs, packages/core-rust/crates/anima-document-store-sqlite types/schema/read-write and citation-identity paths, SDK attachments/projects/session types, existing attachment presentation and ProjectCitationDialog.tsx.

1. Add authenticated no-store page-preview delivery using an attachment or citation identity. Resolve the page locator server-side for project citations. Add a typed PageEvidence citation target bound to document identity, source digest, page number and pipeline identity, so charts and pages with unavailable OCR can be cited without manufacturing literal-passage chunks.
2. Return only bounded raster bytes after current source and access checks.
3. Add typed passage origin, OCR recognizer language, render profile and pipeline identity to storage, citation identity and cache invalidation. Project context must describe OCR text as recognized text, not verbatim quotedPassage. Preserve text-layer provenance for legacy cached rows, and test invalidation when the pipeline changes.
4. Extend the existing chat attachment/citation UI with page preview, loading, unavailable and retry states. Abort old requests and revoke object URLs on scope change.
5. Keep image generation/editing outside this feature.

## Task 5: Validate and document

1. Run focused Rust package/daemon/worker checks first under the native lease.
2. Run bun run rust:test and bun run rust:lint against the final tree.
3. For SDK/web changes, run each workspace's test, build and typecheck scripts. Use focused web checks during development.
4. Exercise the chat preview and citation flow with independent fixtures. Confirm no raster bytes enter transcripts or error output.
5. Update docs/companion/documents.md with actual supported behavior and limitations.
6. Record results, remove task-owned scratch artifacts after recording them, and leave the result uncommitted for coordinator review.

## Acceptance limits

Local deterministic checks prove source binding, delivery, limits and citation behavior. They do not prove live-model reading accuracy. Live provider calls, user-daemon restart, commit, push and PR publication remain unauthorized. Native Office layout rendering and larger scan input limits need separate scope after the PDF evidence path is established.


## Native lease invocation

Run the primary checkout''s wrapper with the isolated checkout as its working directory. The wrapper derives its managed cache root from its own module path, so this keeps monitoring and the absolute validation target under the primary repository. Do not invoke a worktree-local wrapper that creates another validation cache.

Example after the coordinator grants the lease:

```powershell
bun C:/Users/leoca/OneDrive/Desktop/anima/animaos-kit/tools/workspace-dev/scripts/run-task.ts --cwd C:/Users/leoca/.codex/worktrees/scanned-document-reading/animaos-kit --rust-cache validation -- cargo test -p anima-document-visual --test pdf_pages
```

The first suite needs the new reusable page evidence tests and the minimal real owned-worker acceptance test. The image-generation chat now executes all native and SDK service checks from independent frozen request files. No further parent approval is required.

## Design-review record

The independent plan review identified and corrected seven gaps: the real backend experiment now precedes caller integration, aggregate raster/OCR framing is bounded, textless pages have their own citation target, image projection follows the complete tool-result group, durable descriptors recapture authority, storage/context provenance is explicit, and visual preparation has a cancellation contract. These are design changes; no native validation result is claimed.

## Implementation and verification record

- Implemented reusable Windows rendering and optional local OCR with strict input, geometry, PNG and output bounds.
- Implemented binary worker framing with page identity, payload digests and exact EOF, preserving process limits and retirement ownership.
- Implemented transient selected-attachment page/OCR input, preserved text-layer excerpts, conservative maximum-calibration context accounting and the four-read bound.
- Implemented explicit workspace PDF page inspection through read_document and grouped request-only image projection.
- Implemented authenticated attachment/project page previews and source-change checks.
- Implemented typed OCR and page-image origins in the existing locator column. Citation identity already includes locator, text and source digest. Legacy rows stay literal. No schema migration or new required constructor fields were added.
- Implemented bounded OCR search and page-only citation references. Context uses recognizedText or pageEvidence instead of quotedPassage for derived origins.
- Focused client checks passed: 23 SDK tests, 86 web tests, a subsequent 28-test provenance check, focused type checks and lint without errors. Final repository builds remain with the verification owner.
- Prototype revision 1 failed compilation on sha2 0.11 hex formatting. Revision 2 compiled and passed 10 tests, with failures in strict unit-variant deserialization and hand-drawn-glyph OCR. Both failures are retained in the coordinator results.
- An independent standard-font scan replaces the block-glyph fixture without weakening OCR assertions. It uses System.Drawing, Arial 46px, black on white, 1000 by 500 RGB pixels: SCAN DOCUMENT 7319, TOTAL 137.42, COBALT HARBOUR. The PDF wrapper contains image objects and no text layer.
- The physical CLI test invokes --document-worker-v2 without daemon services. It still needs the native owner's execution.

No live provider, user-daemon restart, commit, push or PR publication has run.

The first implementation keeps recognized text as one bounded string plus its
language. It does not expose line rectangles because page previews do not
highlight regions. Visual-selection metadata keys carry V1, and the OCR
pipeline identity derives from the renderer evidence version and raster edge.

Prototype revision 3 compiled, passed the strict recognition-schema check and
read SCAN DOCUMENT 7319 and COBALT HARBOUR from the standard-font scan. It
reported 137 42 for the printed amount 137.42, so exact numerical OCR is not
established. The fixture assertion now retains SCAN and 7319 and verifies the
TOTAL heading. It does not turn OCR into a numeric-transcription guarantee.
The observed decimal loss remains a quality limitation in the coordinator
results. Page-image delivery and unverified-recognition labels are required.

Final review separated live workspace authority from the tool schemas advertised
to a model. Tool-free wrap-up can still receive current evidence. Revocation
permanently retires its read lease, and a workspace switch retires the source
generation. A page descriptor must match the actual assistant read_document
call, its toolCallId, source path and selected pages. Only the current trailing
tool-result group can project workspace page images; historical tool metadata
does not override a later owner turn.

Revision 4 passed all 12 renderer tests and 23 store tests, then daemon
compilation stopped on a snapshot-error type mismatch and missing worker-output
equality used by existing tests. Those boundary mappings were corrected.
Independent review also proved that Busy occurs before a second source read
starts. Returning that one unused reservation preserves both text excerpts
when two single-page PDF visual attempts are busy, within four actual reads.
The page-attempt allowance remains consumed, and other failures receive no
refund. A regression covers this exact fallback.

Revision 5 compiled the daemon and ran the document module: 29 passed, one
existing XLSX admission case failed before either source read, and two were
ignored. Parallel fixtures share the four-slot nonqueued source pool. This is
an investigated contention hypothesis, not a confirmed root cause. The next
request runs that exact test alone and the module serially before continuing.
During native tooling Cargo.lock bytes changed; its current Git diff contains
only the intended renderer package and daemon dependency, with existing package
versions unchanged. Revision 7 re-fingerprinted those bytes but never ran. It
was withdrawn while still queued, with no active OCR command, so known contract
and wording corrections could be included in revision 8.

The final documentation review adds the two PDF page routes to the daemon
inventory and excludes them from historical route counts, retaining both old
route assertions. Capability descriptions and attachment framing now distinguish
text-layer snapshots from separate PDF page evidence and optional unverified
OCR. The generic docs-sync skill checker targets Ardenta-specific routes and
framework policies; Anima's owning OpenAPI contract and scoped link checks are
the relevant checks for these corrections.
Scoped docs-sync link and path checks found no issue in the current daemon
README, document guide or implementation plan. The changelog scan identified
two historical references to deliberately removed apps/server and
packages/core-ts paths in its 2026-10-20 removal entry. Those accurate historical
references remain. No Ardenta-specific route/framework policy was applied.

Physical acceptance review found that the initial CLI test used a text-layer
PDF. Before any revision-8 execution, that queued request was retired. Revision
9 uses a literal image-only scan and chart PDF derived from the same independent
standard-font raster. It asserts no PDF text layer, distinct page rasters, and
SCAN/7319/TOTAL recognition when local OCR is available. The physical test logs
only its bounded recognition outcome, never OCR text or pixels. Optional OCR
unavailability remains an explicit outcome; numerical accuracy is not promised.

Crash recovery verified all 66 revision-9 paths and raw file hashes against the
saved request, with no extra changed paths. Its native checks had not started;
the persisted result still described queued revision 8. A fresh read-only review
confirmed the physical worker's limit/READY ordering and found one failure-only
logging gap: direct Vec comparisons could print complete PNG buffers. Revision
10 replaces both physical and renderer test comparisons with content-free
boolean assertions. No rendering, custody, model, source-access or OCR quality
behavior changes. All native verification remains with the designated owner.
Revision 10 passed the isolated XLSX diagnostic, the document module serially
and in parallel, the exact API inventory, all 12 renderer tests and all 23 store
tests. The earlier admission failure did not reproduce. The model-input suite
then passed 18 tests and failed four new visual cases. Investigation found that
their visual_content helper reused an image-only fixture constructor, marking
PDF references as images and metadata kind=image. Strict document binding
correctly rejected these mismatched references. The fixture now uses the real
attachment_content constructor, retaining all positive/negative assertions and
all production authority, rendering, context and source-read rules unchanged.
The tiny fake PNG's chunk CRCs and compressed stream were independently checked
and valid; no PNG validator was relaxed. Final model and remaining suites must
run against the corrected revision.
Revision 11's parallel model run passed 17 and failed five varying cases,
including existing document hydration. Revision 12 then passed the calibration
case alone and all 22 model cases serially on identical source. Pool acquisition
happens before the read callback, and the fixtures shared the production's
four-slot nonqueued pool. The model fixtures now inject independent four-slot
pools, matching the existing explicit retained-read seam. Production adapters,
attachment previews and workspace tools still default to the same shared pool.

Text, compaction, attachment-page and workspace-page model preparation carry the
adapter's pool through both source reads. The retained-read implementation,
nonqueued acquisition, cancellation checks and physical permit lifetime are
unchanged. A regression holds all four permits and requires visual, text and
compaction hydration to stay unavailable without invoking the legacy reader.
Parallel model checks and the full suite remain required for this correction.
Revision 13's document module reproduced the earlier XLSX admission failure:
29 passed, one failed and two were ignored; the selected source callback was
never reached. Its independent admission fixtures also used the shared pool.
Admission now accepts the existing explicit read-pool seam internally. The
public production entry still selects the shared four-slot pool; unit fixtures
supply their own four-slot pools to both read checkpoints. The shared read_source
compatibility wrapper remains for voice handling. A new exhausted-capacity admission regression
requires zero file-reader and extractor calls, unchanged owner text, and one
Unavailable snapshot with no derived blocks. No authority, deadline, cancellation
or physical retained-read policy is relaxed. Parallel native confirmation is
still required before claiming this correction passes.
Revision 14 stopped at compilation before any test ran: the removed shared
read_source wrapper still had a local import in voice handling. Its existing
signature and delegation to the global pool are restored; the admission unit
pool injection remains. No voice behavior is changed. The compile failure is
recorded, and the same parallel and complete native checks remain required.
Revision 15 passed both parallel document runs (31 each), both parallel model
runs (23 each), all 23 project-knowledge tests and all 17 workspace-document
tests. The physical scan worker rendered both pages and verified the expected
SCAN/7319/TOTAL OCR markers under its existing limits, then failed its final
empty-temp-directory assertion with one entry. The production physical owner
already supplies an owned per-job TEMP/TMP directory and retires the worker
before directory release. The next diagnostic retains the failing assertion
and reports bounded entry name/type/size metadata only, never file contents,
OCR text or raster bytes, to identify the platform behavior before any change.
Revision 16 identified the remaining entry as a directory named NVIDIA
Corporation inside the worker-owned temporary directory, while raster and OCR
marker checks again succeeded. Native graphics can create driver cache state;
requiring an untouched directory was an incorrect acceptance assumption. The
physical test now explicitly closes its owned TempDir after child retirement
and checks the whole directory is gone. Cleanup errors fail the test, rather
than being ignored by TempDir Drop. It also supplies an invalid host-only
configuration value, which the private worker entry must ignore. All physical
limits, raster/source/framing/EOF and OCR assertions remain. No production
cleanup path or driver-specific whitelist is added.

Revision 17 passed physical scan raster/OCR marker verification and checked
temporary-directory cleanup, Rust lint, SDK tests and build, all 216 web test
files (2558 tests passed, one skipped), web build/typecheck and all four offline
browser cases. The full Rust run found one stale tool-registry expectation:
read_document now has optional pages. Its existing schema contract now includes
that optional field and verifies one or two unique integer pages bounded 1..100.
SDK typecheck also found the new test's HeadersInit name unavailable in the
SDK's test configuration; the fixture now uses a string-record type. Both fixes
are test-only. The designated owner must rerun the registry contracts, full Rust
suite, Rust lint and SDK checks on the next frozen revision. Unchanged production,
web and physical acceptance results retain their exact source hashes.
