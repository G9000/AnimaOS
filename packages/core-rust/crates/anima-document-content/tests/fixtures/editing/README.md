# Inert imported-document fixtures

These hand-authored OOXML sources are independent of Anima's generated-document renderer. They contain synthetic text only and are dedicated to this test suite (MIT, matching the repository). Tests package them with rawzip in memory; they never run Office, scripts, macros or network requests.

- `word-template.xml` has a styled body, table and section/header reference. Its header, styles, internal PNG media and relationships are added by the test and must remain byte-identical after a body edit.
- `pixel.png.base64` is a one-pixel PNG, decoded only into package fixture bytes. The runtime never decodes or renders it.
- Spreadsheet and presentation fixtures are hand-authored in `tests/editing.rs`: fixed cells, explicit stale caches, charts, slide relationships and duplicated per-slide shape IDs. They are deliberately separate from `render_document`.

Tests assert structural round-trips and exact untouched bytes. These fixtures alone do not establish full Office layout fidelity. Native Office/LibreOffice opening and rendering have not been run in this implementation phase.
