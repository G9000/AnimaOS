# Reading documents

## Download a generated report

Ask the companion to save a text, Markdown or CSV report using its existing
file-write permission and approvals. Open **Files**, refresh, select the report,
then click **Download current file**. The authenticated action saves the selected
basename and accepts complete UTF-8 files up to 1 MiB, including empty files, BOM
and CRLF. Oversized, excluded or unsupported files are refused, rather than
truncated. A preview can be shorter or unavailable independently of download.

This captures current bytes at the click; it does not lock an earlier preview or
create a chat attachment/artifact. Downloading leaves the workspace file intact.
It supports `.txt`, `.md` and `.csv` only. Existing Files hidden/name/credential
exclusions apply to the whole captured file, as heuristics rather than a complete
secrecy guarantee. No PDF/Office conversion, public sharing or external spreadsheet
formula-safety promise is added. See the [M68 plan](../superpowers/plans/2026-10-16-companion-report-downloads-m68.md).

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
release capacity while physical work survives. No automatic retry or reparse.

See the [M67 plan](../superpowers/plans/2026-10-16-companion-workspace-documents-m67.md).
Local synthetic tool/history tests verify the workflow; they do not establish
independent model answer quality or add OCR, editing or visual fidelity.

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
The native locator `pdf#page=N` identifies the source page; there is no visual
layout, OCR or page rendering. A known limit can shorten a PDF excerpt at a
UTF-8 boundary. Invalid page `Contents` references or failed supported page-stream
decoding discard the whole PDF excerpt. Page content supports unfiltered streams
or a single FlateDecode filter with strict zlib completion and checksum checking.
Other filters, chains and empty filter arrays are unsupported in this slice.
The library can tolerate malformed unused objects and some resource/font/form
data, so success does not certify that every PDF object is valid or every visual
element produced text.

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

OCR, other Office formats and native extraction on other operating systems
remain unsupported in this slice; their originals can still be stored and
downloaded. Browser dictation, compatible-model image input and [selected audio transcription](voice.md) are
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
[M66 XLSX plan](../superpowers/plans/2026-10-16-companion-xlsx-m66.md) for the stored-cell reader. Public Hermes XLSX informed the behavior; its Python runtime was not imported. See the [parity roadmap](parity-roadmap.md) for the remaining media work.
