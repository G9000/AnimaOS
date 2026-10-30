# Creating and reading documents

## Create finished files

Enable **Files & documents** in the agent's built-in tool settings, then ask for
a Word document (`.docx`), Excel workbook (`.xlsx`), PowerPoint presentation
(`.pptx`), or PDF. For example:

> Create a quarterly report as a Word document and a PDF, with a summary,
> headings, next steps, and a table of results. Save them under reports.

> Create an Excel workbook with separate Budget and Owners sheets, then make a
> short PowerPoint briefing from the same information.

The native `write_document` tool creates the file from structured content without
an Office installation, Python, a converter, or another service. Word documents
and PDFs support headings, paragraphs, bullet lists, and tables. Workbooks
support named sheets, column headings, and text, numeric, Boolean, or empty
cells. Presentations support titled slides, text, bullets, and tables.

The agent can return a **Download** link in the owner chat. Downloading starts
only when you click it. You can also open **Files**, refresh, select the generated
file, and choose **Download current file**. The **Content preview** shows readable
text and tables; open the downloaded file in its usual application to inspect
the final layout. Original text-file previews remain plain text.

To revise an Anima-generated file, ask for the change and name its path. The
agent uses `read_document_source` to read its complete source and current hash,
then resubmits the revised document with that hash. Creating over an existing
file without the hash, or revising a file that changed since it was read, is
refused. Anima serializes its own document writes; external applications do not
share that lock or its final file checks. Normal file-write approval,
cancellation, and configured native file checkpoints still apply. Enabling a
tool does not approve a particular write.
Existing custom grants are preserved until you explicitly change them.

This creation path revises Anima-generated files. Images, charts, spreadsheet
formulas/calculation, and a visual Office editor are outside its scope. For
supported changes to existing Office files, see the editing section below.
Text beginning with `=` stays literal spreadsheet
text. The document source is limited to 48 KiB and the finished file to 4 MiB;
oversized content is refused. PDF uses an embedded font for supported
left-to-right text and refuses missing glyphs or scripts requiring complex
shaping/bidirectional layout rather than silently rendering them incorrectly.
Word, Excel, and PowerPoint use their applications' text rendering. Slide content
must fit its slide; split a large table or long text across slides when requested.

Generated files include versioned source metadata. A readable preview, editable
source, and binary download require the whole file to match its deterministic
rendered source. Externally modified files, unknown source versions, and files
without matching metadata are refused by this delivery path. Workspace path,
hidden-file, link, and credential-field exclusions remain in effect. These
checks establish content consistency, not authorship or business correctness.

## Edit an existing Office document

On Windows, enable **Edit existing documents** in the agent's built-in tool
settings and save. This gives the agent access to inspect a supported document
and propose edits; it does not approve a change. In your own chat, attach the
Word (`.docx`), Excel (`.xlsx`) or PowerPoint (`.pptx`) file to your message, or
name a file already in the workspace. Ask for the change and the workspace
path where the result should be saved. The document's contents may enter the
conversation and be sent to your selected model.

Choose a workspace that excludes both private profile settings and private
editing storage. A separate editing directory alone does not resolve a profile
settings file inside the workspace. Keep existing editing data for recovery;
do not redirect uncertain work to an empty directory or delete its evidence.

When **Review document changes** appears, check the source, destination, all
changes, and the before and after content. Choose **Apply these changes** to
approve that specific change, or **Keep original** to decline it. Every new
publication needs its own approval. If the review expires or the source or
destination changes, ask for a fresh review. If a decision cannot be confirmed,
refresh the chat and check its status before taking another action.

An uploaded original remains unchanged. To keep an existing workspace source
unchanged, ask for a new output path. The review identifies whether the result
will create a new file or replace the reviewed existing file. After applying,
use the download link or select the file in **Files** and choose
**Download current file**. The content preview is structural; open the downloaded
file in its usual Office application to check layout, appearance and calculations.

Editing supports selected text changes in Word and PowerPoint, slide reordering,
and supported Excel cell values, formulas and simple existing chart titles or
series. Not every document feature or edit is supported. Complex or active
content, unsafe links, protected or unsupported structures, and edits that
cannot safely preserve dependent content can be refused. Formula support is
limited, and some results need recalculation in a spreadsheet application.
Documents and finished edited files are limited to 4 MiB. Existing PDF editing
and a visual Office editor are not available here.

The separate **Allow reading workspace documents** setting enables the reading
workflow below; it is not a substitute for the editing permission.

## Download a generated report

Ask the companion to save a text, Markdown or CSV report using its existing
file-write permission and approvals. Open **Files**, refresh, select the report,
then click **Download current file**. The authenticated action saves the selected
basename and accepts complete UTF-8 files up to 1 MiB, including empty files, BOM
and CRLF. Oversized, excluded or unsupported files are refused, rather than
truncated. A preview can be shorter or unavailable independently of download.

This captures current bytes at the click; it does not lock an earlier preview or
create a chat attachment/artifact. Downloading leaves the workspace file intact.
This text-report path supports `.txt`, `.md` and `.csv`. Verified Anima-generated
Office/PDF files use the finished-file path above. Existing Files
hidden/name/credential exclusions apply to the whole captured file, as
heuristics rather than a complete secrecy guarantee. This adds no public
sharing or general conversion of existing documents. See the
[M68 plan](../superpowers/plans/2026-10-16-companion-report-downloads-m68.md).

## Documents already in the workspace

In the agent's Settings, enable **Allow reading workspace documents**, save, then
ask it to read a workspace-relative DOCX, PDF or XLSX such as
`reports/ledger.xlsx`. The `read_document` tool reads one file up to 2 MiB without
uploading the original. Extracted text enters the conversation and can be sent to
the selected model. The grant is off when absent; standard profiles do not add it.
Existing explicitly granted helpers and connector-triggered runs can use it too.

The result quotes the relative path and SHA-256 of the captured bytes, with typed
locators and coverage under a 16 KiB total serialized limit. It is a captured
snapshot, so it does not promise that the file still has those bytes afterward.
Ordinary tool history can retain that snapshot across restart without opening or
parsing the source again. Ask for another read when current contents are needed.
No attachment or download entry is created and the source stays unchanged.

The tool uses the same Windows worker and format limits described below. A
configured workspace root and run cancellation are required; missing/unsafe,
oversized or unsupported sources can be refused. Symlinks/reparse points and
nonregular files are rejected; hard-link behavior follows existing workspace
reads. Four physical source-read leases are shared with attachment admission, and
one absolute ten-second deadline covers capture/extraction/fitting. Stop does not
release capacity while physical work survives. No automatic retry or reparse for text-only reads. Explicit page inspection
verifies and rerenders bounded pages during fresh model delivery.

See the [M67 plan](../superpowers/plans/2026-10-16-companion-workspace-documents-m67.md).
Local synthetic tool/history tests verify the workflow; they do not establish
independent model answer quality. Explicit PDF page inspection is described below;
editing and full Office visual fidelity remain separate.

## Documents selected in chat

On Windows, send a DOCX, PDF or XLSX attachment with a message in your own web chat.
Anima extracts bounded DOCX main-body text, PDF text-layer content or XLSX
stored-cell data for that message. The saved attachment shows the outcome and keeps its original download.
The original message and document bytes are preserved.

The first two supported documents in selected order can be parsed, each up to
2 MiB, counting all three formats together. A failed or oversized supported file
still consumes an attempt. Together their quoted excerpts and locators fit within
16 KiB, and may be reduced further to fit the chat context. At most ten selected
documents retain an outcome; extra originals remain downloadable.

Admission can read each of those two sources before and after extraction: four
reads maximum. Each later model request or compaction separately checks at most
four current/newest sources, with 2 MiB plus a sentinel byte per read. Initial
admission plus the first model request can therefore make eight bounded reads.
Four physical read leases are shared across these paths. Older saved excerpts
can be omitted from a request. Changing or deleting the original prevents reuse
of its old excerpt. Reload reuses the saved result without reparsing it.

DOCX coverage is **main-body text only**: ordinary paragraphs, table paragraphs,
tabs, line breaks and ruby base text. Headers, footers, footnotes, comments,
drawings, text boxes, field results, deleted revisions and layout are excluded.
A paragraph locator identifies the original selected attachment and its position
in word/document.xml; it does not identify a page.

PDF coverage is **text layer only**, with at most 100 pages. Anima examines every
accepted page, even after the retained text fills. The outcome reports total
pages, pages producing nonblank text, and pages whose excerpts were included.
An empty text result does not establish whether a page is scanned or image-only.
The native locator `pdf#page=N` identifies the source page. This outcome describes the text layer.
The separate page-evidence path below supplies rendered pages and local OCR. A known limit can shorten a PDF excerpt at a
UTF-8 boundary. Invalid page `Contents` references or failed supported page-stream
decoding discard the whole PDF excerpt. Page content supports unfiltered streams
or a single FlateDecode filter with strict zlib completion and checksum checking.
Other filters, chains and empty filter arrays are unsupported in this slice.
The library can tolerate malformed unused objects and some resource/font/form
data, so success does not certify that every PDF object is valid or every visual
element produced text.

## Read scanned PDF pages and inspect tables or charts

On Windows, attach a supported PDF in your own chat and ask about its contents.
Within the context budget, Anima inspects up to the first two selected pages.
An image-capable model receives page images and available local OCR text.
A text-only model receives recognized text and a limitation about visual
relationships. Pages share the four-image allowance with uploaded images.
Omitted pages are identified; a partial view is not a full document review.

For a PDF already in the workspace, enable **Allow reading workspace documents**
and ask for the original page numbers. The read_document tool accepts one or
two unique, ascending numbers in pages, for example pages 7 and 8. Text-only
reads keep their existing behavior. Fresh image delivery rechecks the captured
source hash and current read permission. Its transient user message follows
all sibling tool results and is not saved as a new owner turn. Historical
textual tool results remain captured snapshots.

Click **Preview PDF page** beside an attachment to inspect an original page.
Page links in the answer use the authenticated preview. Project citations also
show their original PDF page after current access and source checks. Changed
or deleted originals and revoked grants can make fresh delivery unavailable.

Rendering uses Windows' built-in PDF APIs and installed OCR languages.
No OCR provider or language package is downloaded. Recognition can contain
errors. Chart labels alone do not establish a chart's visual meaning or an
exact value. A local fixture read 137.42 as 137 42, so verify punctuation and
amounts against the page image. The page image is the evidence for a visual
interpretation.

Sources retain the 2 MiB and 100-page admission bounds. Each operation renders
at most two pages, with a longest edge of 1,600 pixels and a 2 MiB PNG limit per
page. The ten-second deadline, shared physical worker slot, 256 MiB process
limit, and retained read capacity still apply. Rendering reserves its before
and after reads within the four-read model-request bound. Pixels stay out of
public session metadata, logs, and saved conversation turns.

Project knowledge indexes recognized text from up to two pages without
retained text per indexing pass. Results identify OCR language and pipeline.
A page without recognized text can have a page-only citation searchable by
document title, with no invented passage. Other content remains partial.
Derived cache entries never grant source access.

This PDF path does not render native DOCX, XLSX, or PPTX layout. Their existing
text and stored-cell readers retain their documented coverage.

## Read stored workbook cells

XLSX coverage is **stored cells only**, in workbook sheet order with numeric
sheet/row/column locators. Visible, hidden and very-hidden sheets are labeled and
included within the limits. Sheet names are quoted as untrusted source metadata.
Shared/inline strings and scalar values are read; formulas are literal text labeled
not evaluated, and cached values are labeled possibly stale or missing. Anima does
not calculate formulas, resolve external relationships, run macros or open workbook
connections. Raw numbers/date serials do not imply formatted dates, currencies or
visual layout. Charts, pivots, images, comments and merged-cell presentation are
outside this text view. The reader supports transitional OOXML; shared/array
formulas, date-string cell types and additional relationship-bearing features are
unsupported. OOXML `_xNNNN_` escape tokens remain literal stored text.

The XLSX reader caps 512 archive entries, 16 sheets, 8 MiB expanded relevant XML,
2 MiB per XML part, 4,096 shared strings, 2,000 observed cells, 250,000 XML events
and depth 64. Individual cell text is limited to 1,024 UTF-8 bytes and is omitted
whole if too long. Over-limit workbooks can be refused without text; sparse
dimensions never cause iteration over an empty grid. Malformed supported parts,
including late invalid cells, discard the excerpt; ignored drawing/style payloads
are not validated. Usable results are partial; the chat badge shows
counts and limitations without exposing names or cell contents. Context fitting
drops whole cells, preserving observed totals and updating included totals. The
original remains downloadable. This is bounded fixture-tested reading, not complete
workbook fidelity or independently verified answer quality.

“Main body text extracted” means extraction succeeded. “Some main body text
extracted” means a known limit shortened coverage. Other outcomes explain an
unsupported, encrypted, malformed, oversized, empty, unavailable, cancelled or
context-omitted result. Historical attachments without an outcome have no
extraction evidence. An extraction outcome does not verify the assistant's
answer or the completeness of the document.

PDF outcomes show text-layer coverage instead of paragraph counts. Failed or
unavailable extraction has unknown page counts. A context-omitted result can
retain known coverage while including zero pages. Encrypted PDFs, including
ones that the library internally opens with an empty password, release no text.
No password is supplied or requested. Original downloads remain available.

Parsing runs in a fixed, one-shot worker with checked Windows Job memory and
process limits before document bytes are sent, a ten-second parent wall deadline,
bounded pipes and an owned cancellation/retirement path. These are resource and
lifetime limits, not a filesystem/network sandbox or a kernel CPU quota. File
read waits also yield to Stop and the extraction deadline; late work cannot
publish a usable result. A stuck physical worker retains its slot until its
retirement is proved.

Office page rendering, Office OCR and native document extraction on other
operating systems remain unsupported in this slice; their originals can still
be stored and downloaded. Browser dictation, compatible-model image input and [selected audio transcription](voice.md) are
separate capabilities. Extraction itself makes no model request, enables no
tool permission and writes no Fact or knowledge collection. Compaction can
summarize verified excerpts as untrusted conversation data under its existing
bounds.

The Rust parser narrowly ports useful DOCX behavior from the pinned public
Hermes source, with its MIT notice retained. See the
[M54 DOCX plan](../superpowers/plans/2026-10-16-companion-document-understanding-m54.md)
and [M55 PDF plan](../superpowers/plans/2026-10-16-companion-pdf-understanding-m55.md)
for scope, limits and verification. PDF extraction directly uses pinned Rust
libraries rather than a converter process. See the
[M66 XLSX plan](../superpowers/plans/2026-10-16-companion-xlsx-m66.md) for the stored-cell reader. Public Hermes XLSX informed the behavior; its Python runtime was not imported.
