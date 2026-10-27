# Finished documents

The owner selected capability 1: create and revise DOCX, XLSX, PPTX and PDF files, preview their content, and download the finished result.

## First release

Agents use `write_document` with a workspace-relative `file_path` and a structured `document` specification. A document has `format` (`docx`, `xlsx`, `pptx`, `pdf`) and `title`. DOCX/PDF contain `blocks`; XLSX contains `sheets`; PPTX contains `slides`. Blocks are paragraphs, headings, bullet lists and tables. Sheets have names, column headings and typed scalar rows. Slides have a title and blocks. Text that begins with `=` is literal spreadsheet text; this release does not calculate formulas. This is structured creation and revision of Anima-generated files, not arbitrary Office import/editing.

`read_document_source` returns the exact validated source specification and current SHA-256 of an Anima-generated document. Revision resubmits the complete specification with `expected_sha256`. Creating over an existing path without that hash, or revising stale bytes, fails without changing the file. Native checkpoints and normal write approval apply. No converter, Python installation, external service or model call is required by generation.

## Reusable renderer

Add a portable generation module to `anima-document-content`. Public API: `DocumentSpec`, `GeneratedFormat`, `render_document(&DocumentSpec) -> Result<GeneratedDocument, GenerationError>` and `inspect_generated_document(GeneratedFormat, &[u8]) -> Result<DocumentSpec, GenerationError>`. `GeneratedDocument` exposes `bytes` and `preview` (escaped Markdown content).

The canonical source envelope (`{version: 1, document: spec}`) is bounded to 48 KiB and included as ZIP comment metadata for Office files and a bounded ASCII comment for PDF. Inspection validates the source, regenerates the document deterministically, and compares the complete bytes before displaying a preview or returning editable source. Metadata alone never establishes content or authority. Unknown versions and modified/external files receive an honest unsupported-preview result. Future renderer changes must retain the version-1 renderer or explicitly report the unsupported version. Limits cover input text, collection sizes, rows, slides and output bytes (4 MiB). Office packages contain no macros, executable content, external relationships or formulas. PDF embeds a licensed local font, checks glyph coverage, wraps and paginates text; unsupported glyphs produce an explicit failure.

## Host and clients

Native tools are explicitly granted through the existing read/write abilities. File mutations use capability-relative paths and existing checkpoint/journal/approval controls; sensitive paths and links remain denied. Avoid general binary/base64 write capabilities. Owner workspace preview/download endpoints accept only verified generated binaries in the four formats, preserve existing text-report behavior, recheck current bytes, apply current browsing exclusions and credential checks, and return attachment downloads with no-store/nosniff headers.

Workspace preview adds optional `previewKind: "document"` and `sha256` while keeping `path`, `content` and `truncated`. The web console reuses the Files layout and download control, renders generated content with the existing safe Markdown stack, and labels it **Content preview**. It is not a pixel-identical Office preview. The design editor is unavailable in this session; no new page layout is introduced.

## Acceptance

Test actual Office ZIP/XML relationships and typed cells, independent readers, PDF text and page rendering, empty/oversized/malformed inputs, literal formula text, Unicode/escaping, no overwrite and stale revisions, checkpoint/approval behavior, unauthorized downloads and path/link rejection, tampered metadata/content, SDK blob bounds, and web download/cancellation behavior. Run the full Rust suite and changed SDK/web test/build/typecheck/lint scripts. Record remaining format/layout limits candidly.
