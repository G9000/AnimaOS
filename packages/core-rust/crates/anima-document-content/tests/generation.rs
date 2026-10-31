use anima_document_content::{
    inspect_generated_document, render_document, CellValue, DocumentBlock, DocumentSpec,
    GeneratedFormat, MAX_GENERATED_DOCUMENT_BYTES, MAX_GENERATED_SOURCE_BYTES,
};
use serde_json::{json, Value};
use std::io::Read;

fn spec(value: Value) -> DocumentSpec {
    serde_json::from_value(value).unwrap()
}

fn prose(format: &str) -> DocumentSpec {
    spec(
        json!({"format":format,"title":"Quarterly résumé & outlook", "blocks":[
            {"type":"heading","text":"Results <2026>","level":2},
            {"type":"paragraph","text":"Café — revenue grew 12%. Ελληνικά.\nNext line."},
            {"type":"bullets","items":["Review & approve", "Literal [link](https://example.com) <img src=x>"]},
            {"type":"table","headers":["Item", "Value"],"rows":[["Plan",12.5],["Ready",true],["Missing",null]]}
        ]}),
    )
}

fn office_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, String> {
    let archive = rawzip::ZipArchive::from_slice(bytes).unwrap();
    let mut entries = archive.entries();
    let mut parts = std::collections::BTreeMap::new();
    while let Some(entry) = entries.next_entry().unwrap() {
        let file = archive.get_entry(entry.wayfinder()).unwrap();
        assert_eq!(
            entry.compression_method(),
            rawzip::CompressionMethod::DEFLATE
        );
        let decoder = flate2::read::DeflateDecoder::new(file.data());
        let mut body = Vec::new();
        file.verifying_reader(decoder)
            .read_to_end(&mut body)
            .unwrap();
        parts.insert(
            String::from_utf8(entry.file_path().as_bytes().to_vec()).unwrap(),
            String::from_utf8(body).unwrap(),
        );
    }
    parts
}

#[test]
fn all_formats_are_real_deterministic_files_with_verified_source() {
    for format in [
        GeneratedFormat::Docx,
        GeneratedFormat::Xlsx,
        GeneratedFormat::Pptx,
        GeneratedFormat::Pdf,
    ] {
        let document = match format {
            GeneratedFormat::Docx | GeneratedFormat::Pdf => prose(format.extension()),
            GeneratedFormat::Xlsx => spec(
                json!({"format":"xlsx","title":"Budget","sheets":[{"name":"Plan","columns":["Label","Amount","Ready"],"rows":[["=SUM(1,2)",12.5,true],["Café & tea",null,false]]}]}),
            ),
            GeneratedFormat::Pptx => spec(
                json!({"format":"pptx","title":"Brief","slides":[{"title":"A clear plan","blocks":[{"type":"paragraph","text":"Café & tea"},{"type":"table","headers":["Work","Owner"],"rows":[["Launch","Lee"]]}]}]}),
            ),
        };
        let generated = render_document(&document).unwrap();
        assert!(!generated.bytes.is_empty());
        assert!(generated.bytes.len() <= MAX_GENERATED_DOCUMENT_BYTES);
        assert_eq!(generated.bytes, render_document(&document).unwrap().bytes);
        assert_eq!(
            document,
            inspect_generated_document(format, &generated.bytes).unwrap()
        );
        assert!(generated
            .preview
            .contains(&document.title.replace('&', "&amp;")));
        if format == GeneratedFormat::Pdf {
            assert!(generated.bytes.starts_with(b"%PDF-"));
            let text = pdf_extract::extract_text_from_mem(&generated.bytes).unwrap();
            assert!(text.contains("Café"), "{text}");
            assert!(text.contains("Ελληνικά"), "{text}");
        } else {
            assert!(generated.bytes.starts_with(b"PK"));
            let parts = office_parts(&generated.bytes);
            assert!(parts.contains_key("[Content_Types].xml"));
            assert!(parts.contains_key("_rels/.rels"));
            match format {
                GeneratedFormat::Docx => {
                    assert!(parts["word/document.xml"].contains("Results &lt;2026&gt;"));
                    assert!(parts["word/document.xml"].contains("<w:tbl>"));
                    assert!(parts.contains_key("word/styles.xml"));
                }
                GeneratedFormat::Xlsx => {
                    let sheet = &parts["xl/worksheets/sheet1.xml"];
                    assert!(sheet.contains("=SUM(1,2)"));
                    assert!(sheet.contains("t=\"inlineStr\""));
                    assert!(sheet.contains("t=\"b\""));
                    assert!(sheet.contains("<v>12.5</v>"));
                    assert!(!sheet.contains("<f>"));
                    assert!(parts.contains_key("xl/styles.xml"));
                }
                GeneratedFormat::Pptx => {
                    assert!(parts["ppt/slides/slide1.xml"].contains("<a:tbl>"));
                    assert!(parts["ppt/slides/slide1.xml"].contains(
                        "uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\""
                    ));
                    for path in [
                        "ppt/slideMasters/slideMaster1.xml",
                        "ppt/slideLayouts/slideLayout1.xml",
                        "ppt/theme/theme1.xml",
                    ] {
                        assert!(parts.contains_key(path), "missing {path}");
                    }
                }
                _ => unreachable!(),
            }
            for (path, xml) in &parts {
                assert!(!xml.contains("TargetMode=\"External\""), "{path}");
                let mut reader = quick_xml::Reader::from_str(xml);
                loop {
                    if reader.read_event().unwrap() == quick_xml::events::Event::Eof {
                        break;
                    }
                }
            }
        }
    }
}

#[test]
fn spreadsheet_escape_sequences_stay_literal_in_cells_headers_and_sheet_names() {
    let document = spec(
        json!({"format":"xlsx","title":"Escapes","sheets":[{"name":"_x0041_","columns":["_x000A_"],"rows":[["_x0041__x005F_"],["_x0041_x0042_"],["_X0041_"],["_xZZZZ_"]]}]}),
    );
    let bytes = render_document(&document).unwrap().bytes;
    let parts = office_parts(&bytes);
    let sheet = &parts["xl/worksheets/sheet1.xml"];
    assert!(sheet.contains("_x005F_x000A_"));
    assert!(sheet.contains("_x005F_x0041__x005F_x005F_"));
    assert!(sheet.contains("_x005F_x0041_x005F_x0042_"));
    assert!(sheet.contains("_x005F_X0041_"));
    assert!(sheet.contains("_xZZZZ_"));
    assert!(parts["xl/workbook.xml"].contains("name=\"_x005F_x0041_\""));
}

#[test]
fn pdf_normalizes_supported_accents_and_rejects_shaping_or_direction_it_cannot_render() {
    let document = spec(
        json!({"format":"pdf","title":"Accent","blocks":[{"type":"paragraph","text":"Cafe\u{301}"}]}),
    );
    let text =
        pdf_extract::extract_text_from_mem(&render_document(&document).unwrap().bytes).unwrap();
    assert!(text.contains("Café"));
    for unsupported in [
        "שלום",
        "العربية",
        "क",
        "a\u{20dd}",
        "A\u{200d}B",
        "a\u{301}\u{301}",
    ] {
        let document = spec(
            json!({"format":"pdf","title":"x","blocks":[{"type":"paragraph","text":unsupported}]}),
        );
        let result = render_document(&document);
        assert!(result.is_err(), "accepted {unsupported}");
        assert!(result.unwrap_err().to_string().contains("shaping"));
    }
    // Office delegates shaping to the native reader and retains this text.
    let document = spec(
        json!({"format":"docx","title":"x","blocks":[{"type":"paragraph","text":"שלום العربية क"}]}),
    );
    assert!(render_document(&document).is_ok());
}

#[test]
fn preview_escapes_literal_html_links_markdown_and_table_delimiters() {
    let document = spec(json!({"format":"docx","title":"Literal title", "blocks":[
        {"type":"paragraph","text":"# header\n<script>alert(1)</script> [click](https://x) ![x](x) *bold*\n1. literal item\nSetext\n===\nhttps://example.com ~strike~"},
        {"type":"table","headers":["A|B"],"rows":[["<img> [x](x) | line\nnext"]]}
    ]}));
    let preview = render_document(&document).unwrap().preview;
    assert!(!preview.contains("<script>"));
    assert!(!preview.contains("<img>"));
    assert!(!preview.contains("[click]("));
    assert!(!preview.contains("\n# header"));
    assert!(preview.contains("&lt;script&gt;"));
    assert!(preview.contains("A\\|B"));
    assert!(!preview.contains("\n1. literal"));
    assert!(!preview.contains("\n===\n"));
    assert!(!preview.contains("https://example.com"));
}

#[test]
fn xml_noncharacters_sheet_controls_and_unrenderable_slides_are_rejected() {
    for value in [
        json!({"format":"docx","title":"x","blocks":[{"type":"paragraph","text":"\u{fffe}"}]}),
        json!({"format":"xlsx","title":"x","sheets":[{"name":"Line\nbreak","columns":["A"],"rows":[]}]}),
        json!({"format":"pptx","title":"x","slides":[{"title":"x","blocks":[{"type":"paragraph","text":"W".repeat(8192)}]}]}),
        json!({"format":"pptx","title":"x","slides":[{"title":"x","blocks":[{"type":"table","headers":["A"],"rows":vec![vec!["too many rows"];100]}]}]}),
    ] {
        assert!(
            render_document(&spec(value.clone())).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn wide_slide_text_is_measured_before_accepting_its_layout() {
    for value in [
        json!({"format":"pptx","title":"x","slides":[{"title":"W".repeat(140),"blocks":[]}]}),
        json!({"format":"pptx","title":"x","slides":[{"title":"x","blocks":[{"type":"paragraph","text":"W".repeat(1100)}]}]}),
        json!({"format":"pptx","title":"x","slides":[{"title":"x","blocks":[{"type":"table","headers":["A"],"rows":[["W".repeat(1000)]]}]}]}),
    ] {
        assert!(render_document(&spec(value)).is_err());
    }
}

#[test]
fn collection_limits_are_enforced_even_for_tiny_values() {
    for value in [
        json!({"format":"docx","title":"x","blocks":vec![json!({"type":"paragraph","text":""});129]}),
        json!({"format":"docx","title":"x","blocks":[{"type":"bullets","items":vec!["x";129]}]}),
        json!({"format":"xlsx","title":"x","sheets":[{"name":"Plan","columns":vec!["x";33],"rows":[]}]}),
        json!({"format":"xlsx","title":"x","sheets":[{"name":"Plan","columns":["x"],"rows":vec![vec![Value::Null];1001]}]}),
        json!({"format":"pptx","title":"x","slides":vec![json!({"title":"x","blocks":[]});65]}),
    ] {
        assert!(render_document(&spec(value)).is_err());
    }
}

#[test]
fn pdf_refuses_columns_narrower_than_their_glyphs() {
    let document = spec(
        json!({"format":"pdf","title":"x","blocks":[{"type":"table","headers":["A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z","AA","AB","AC","AD","AE","AF"],"rows":[vec!["W".repeat(100);32]]}]}),
    );
    assert!(render_document(&document).is_err());
}

#[test]
fn compact_source_cannot_expand_past_the_binary_output_limit() {
    let slide = json!({"title":"S","blocks":[{"type":"table","headers":vec!["A";8],"rows":vec![vec![Value::Null;8];11]}]});
    let document = spec(json!({"format":"pptx","title":"T","slides":vec![slide;64]}));
    assert!(serde_json::to_vec(&document).unwrap().len() < MAX_GENERATED_SOURCE_BYTES - 64);
    let failure = render_document(&document).unwrap_err();
    assert!(failure.to_string().contains("4 MiB"), "{failure}");
}

#[test]
fn input_is_strict_and_rejects_ignored_content_and_invalid_structure() {
    assert!(serde_json::from_value::<DocumentSpec>(
        json!({"format":"docx","title":"x","unknown":true})
    )
    .is_err());
    assert!(serde_json::from_value::<DocumentSpec>(
        json!({"format":"docx","title":"x","blocks":[{"type":"paragraph","text":"x","level":2}]})
    )
    .is_err());
    for value in [
        json!({"format":"docx","title":"x"}),
        json!({"format":"xlsx","title":"x","blocks":[{"type":"paragraph","text":"ignored"}],"sheets":[{"name":"Plan","columns":["A"],"rows":[]}]}),
        json!({"format":"docx","title":"x","blocks":[{"type":"heading","text":"x","level":7}]}),
        json!({"format":"xlsx","title":"x","sheets":[{"name":"../bad","columns":["A"],"rows":[[1,2]]}]}),
        json!({"format":"xlsx","title":"x","sheets":[{"name":"Plan","columns":["A"],"rows":[]},{"name":"plan","columns":["A"],"rows":[]}]}),
        json!({"format":"docx","title":"x","blocks":[{"type":"table","headers":["A"],"rows":[[1,2]]}]}),
        json!({"format":"docx","title":"x\u{0}","blocks":[{"type":"paragraph","text":"x"}]}),
        json!({"format":"docx","title":"x","blocks":[{"type":"paragraph","text":"\u{7f}"}]}),
    ] {
        assert!(
            render_document(&spec(value.clone())).is_err(),
            "accepted {value}"
        );
    }
    let mut nonfinite = prose("docx");
    nonfinite.blocks = vec![DocumentBlock::Table {
        headers: vec!["A".into()],
        rows: vec![vec![CellValue::Number(f64::NAN)]],
    }];
    assert!(render_document(&nonfinite).is_err());
}

#[test]
fn bounded_source_and_output_and_unsupported_glyphs_fail_without_partial_results() {
    let mut oversized = prose("docx");
    oversized.blocks = (0..10)
        .map(|_| DocumentBlock::Paragraph {
            text: "x".repeat(7_000),
        })
        .collect();
    assert!(serde_json::to_vec(&oversized).unwrap().len() > MAX_GENERATED_SOURCE_BYTES);
    assert!(render_document(&oversized).is_err());
    let missing_glyph = spec(
        json!({"format":"pdf","title":"Unsupported","blocks":[{"type":"paragraph","text":"\u{10ffff}"}]}),
    );
    assert!(render_document(&missing_glyph)
        .unwrap_err()
        .to_string()
        .contains("glyph"));
    assert!(inspect_generated_document(
        GeneratedFormat::Pdf,
        &vec![0; MAX_GENERATED_DOCUMENT_BYTES + 1]
    )
    .is_err());
    assert!(inspect_generated_document(GeneratedFormat::Docx, b"PK malformed").is_err());
}

#[test]
fn inspection_rejects_changed_bytes_forged_source_and_unknown_source_version() {
    for format in [GeneratedFormat::Docx, GeneratedFormat::Pdf] {
        let output = render_document(&prose(format.extension())).unwrap();
        let mut changed = output.bytes.clone();
        changed[20] ^= 1;
        assert!(inspect_generated_document(format, &changed).is_err());
        let mut appended = output.bytes.clone();
        appended.extend_from_slice(b"extra");
        assert!(inspect_generated_document(format, &appended).is_err());
    }
    let output = render_document(&prose("docx")).unwrap();
    let mut forged = output.bytes.clone();
    let offset = forged
        .windows(b"Quarterly".len())
        .rposition(|x| x == b"Quarterly")
        .unwrap();
    forged[offset] = b'X';
    assert!(inspect_generated_document(GeneratedFormat::Docx, &forged).is_err());
    let mut future = output.bytes;
    let offset = future
        .windows(b"\"version\":1".len())
        .rposition(|x| x == b"\"version\":1")
        .unwrap();
    future[offset + b"\"version\":".len()] = b'2';
    assert!(inspect_generated_document(GeneratedFormat::Docx, &future).is_err());
    assert!(inspect_generated_document(GeneratedFormat::Xlsx, &future).is_err());
}

#[test]
fn pdf_wraps_and_paginates_without_losing_text() {
    let text = (0..180)
        .map(|n| format!("Line {n}: café and the next milestone.\n"))
        .collect::<String>();
    let document = spec(
        json!({"format":"pdf","title":"Long report","blocks":[{"type":"paragraph","text":text}]}),
    );
    let bytes = render_document(&document).unwrap().bytes;
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();
    assert!(pdf.get_pages().len() >= 3);
    let text = pdf_extract::extract_text_from_mem(&bytes).unwrap();
    assert!(text.contains("Line 0:"));
    assert!(text.contains("Line 179:"));
    assert!(bytes
        .windows(b"/FontFile2".len())
        .any(|x| x == b"/FontFile2"));
    assert!(bytes
        .windows(b"/ToUnicode".len())
        .any(|x| x == b"/ToUnicode"));
}
