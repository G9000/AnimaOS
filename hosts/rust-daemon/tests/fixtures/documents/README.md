# Independent M54 DOCX fixtures

These small stored-ZIP OOXML documents were generated with Python standard-library
zipfile from literal synthetic XML, independently of the Rust extractor and
upstream tests. No real document or account data is included.

The selected main body starts with `M54 selected record: cobalt harbour 7319.`.
It also contains split runs, tab/break text and a ruby base whose guide is excluded.
The unselected document contains `NEVER_INLINE_2047`; both headers contain
`HEADER_OMITTED_4096`, outside the supported main-body text slice.
Original bytes remain downloadable. Fixtures make no model-quality claim.
