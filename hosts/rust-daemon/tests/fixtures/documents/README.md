# Independent M54 DOCX fixtures

These small stored-ZIP OOXML documents were generated with Python standard-library
zipfile from literal synthetic XML, independently of the Rust extractor and
upstream tests. No real document or account data is included.

The selected main body starts with `M54 selected record: cobalt harbour 7319.`.
It also contains split runs, tab/break text and a ruby base whose guide is excluded.
The unselected document contains `NEVER_INLINE_2047`; both headers contain
`HEADER_OMITTED_4096`, outside the supported main-body text slice.
Original bytes remain downloadable. Fixtures make no model-quality claim.

## Independent M55 PDF fixtures

These literal synthetic PDFs were written independently with Python standard
library byte serialization, checked object/xref offsets and stream lengths,
without a PDF-library writer. The selected fixture has two pages, one with
`M55 selected invoice total: USD 137.42.` and instruction-looking data
`DOCUMENT_DATA_NOT_AUTHORITY_M55`, and one blank. The unselected file contains
`NEVER_INLINE_PDF_M55_9081`. Their immutable SHA-256 receipts are in the private
M55 ledger. The original M54 DOCX fixture bytes and assertions remain unchanged.
Supplemental codec/encryption fixtures are synthetic; no real document/account
data or model-quality claim is included.
