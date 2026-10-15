# Selected documents in chat

On Windows, send a DOCX attachment with a message in your own web chat. Anima
extracts a bounded portion of its main-body text for that message. The saved
attachment shows the extraction outcome and keeps its original download action.
The original message and document bytes are preserved.

The first two selected DOCX files can be parsed, each up to 2 MiB. Together their
quoted excerpts and paragraph locators fit within 16 KiB, and may be reduced
further to fit the chat context. Newest selected history has priority; at most
four document sources are checked for each model request. Older saved excerpts
can be omitted from that request. Changing or deleting the original prevents
reuse of its old excerpt. Reload reuses the saved result without reparsing it.

Coverage is **main-body text only**: ordinary paragraphs, table paragraphs,
tabs, line breaks and ruby base text. Headers, footers, footnotes, comments,
drawings, text boxes, field results, deleted revisions and layout are excluded.
A paragraph locator identifies the original selected attachment and its position
in word/document.xml; it does not identify a page.

“Main body text extracted” means extraction succeeded. “Some main body text
extracted” means a known limit shortened coverage. Other outcomes explain an
unsupported, encrypted, malformed, oversized, empty, unavailable, cancelled or
context-omitted result. Historical attachments without an outcome have no
extraction evidence. An extraction outcome does not verify the assistant's
answer or the completeness of the document.

Parsing runs in a fixed, one-shot worker with checked Windows Job memory and
process limits before document bytes are sent, a ten-second parent wall deadline,
bounded pipes and an owned cancellation/retirement path. These are resource and
lifetime limits, not a filesystem/network sandbox or a kernel CPU quota. File
read waits also yield to Stop and the extraction deadline; late work cannot
publish a usable result. A stuck physical worker retains its slot until its
retirement is proved.

PDF, OCR, other Office formats and native extraction on other operating systems
remain unsupported in this slice; their originals can still be stored and
downloaded. Browser dictation, compatible-model image input and native voice are
separate capabilities. Extraction itself makes no model request, enables no
tool permission and writes no Fact or knowledge collection. Compaction can
summarize verified excerpts as untrusted conversation data under its existing
bounds.

The Rust parser narrowly ports useful DOCX behavior from the pinned public
Hermes source, with its MIT notice retained. See the
[M54 plan](../superpowers/plans/2026-10-16-companion-document-understanding-m54.md)
for scope, limits and verification, and the
[parity roadmap](parity-roadmap.md) for the remaining media work.
