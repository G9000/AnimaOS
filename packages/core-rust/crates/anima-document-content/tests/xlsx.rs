use std::io::Write;

use anima_document_content::extract_document;
use anima_service_adapters::document_content::{
    DocumentExtractionStatus as Status, DocumentFormat, DocumentLocator,
    DocumentOmission as Omission, ExtractedDocumentText, XlsxCoverage, XlsxSheetCoverage,
    XlsxSheetVisibility as Visibility,
};
use rawzip::{CompressionMethod, ZipArchive, ZipArchiveWriter};

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn package(parts: &[(String, String)], deflate: bool) -> Vec<u8> {
    let mut output = Vec::new();
    let mut archive = ZipArchiveWriter::new(&mut output);
    for (name, bytes) in parts {
        let method = if deflate {
            CompressionMethod::DEFLATE
        } else {
            CompressionMethod::STORE
        };
        let (mut entry, config) = archive
            .new_file(name.as_str())
            .compression_method(method)
            .start()
            .unwrap();
        if deflate {
            let encoder =
                flate2::write::DeflateEncoder::new(&mut entry, flate2::Compression::default());
            let mut writer = config.wrap(encoder);
            writer.write_all(bytes.as_bytes()).unwrap();
            let (encoder, descriptor) = writer.finish().unwrap();
            encoder.finish().unwrap();
            entry.finish(descriptor).unwrap();
        } else {
            let mut writer = config.wrap(&mut entry);
            writer.write_all(bytes.as_bytes()).unwrap();
            let (_, descriptor) = writer.finish().unwrap();
            entry.finish(descriptor).unwrap();
        }
    }
    archive.finish().unwrap();
    output
}

fn parts(sheets: &[(&str, &str, &str)], shared: &str) -> Vec<(String, String)> {
    let mut types = String::from("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>");
    let mut workbook = format!("<workbook xmlns=\"{NS}\" xmlns:r=\"{REL}\"><sheets>");
    let mut rels = format!("<Relationships xmlns=\"{PKG}\"><Relationship Id=\"strings\" Type=\"{REL}/sharedStrings\" Target=\"sharedStrings.xml\"/>");
    let mut result = vec![("_rels/.rels".into(), format!("<Relationships xmlns=\"{PKG}\"><Relationship Id=\"root\" Type=\"{REL}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"))];
    for (i, (name, visibility, body)) in sheets.iter().enumerate() {
        let index = i + 1;
        // Reverse part numbers deliberately: workbook order, not filenames, is authoritative.
        let part = sheets.len() - i;
        workbook.push_str(&format!(
            "<sheet name=\"{name}\" sheetId=\"{}\" state=\"{visibility}\" r:id=\"r{index}\"/>",
            50 + index
        ));
        rels.push_str(&format!("<Relationship Id=\"r{index}\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet{part}.xml\"/>"));
        types.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{part}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"));
        result.push((format!("xl/worksheets/sheet{part}.xml"), format!("<worksheet xmlns=\"{NS}\"><dimension ref=\"A1:XFD1048576\"/><sheetData>{body}</sheetData><mergeCells count=\"1\"><mergeCell ref=\"A1:B1\"/></mergeCells></worksheet>")));
    }
    workbook.push_str("</sheets></workbook>");
    rels.push_str("</Relationships>");
    types.push_str("</Types>");
    result.extend([
        ("[Content_Types].xml".into(), types),
        ("xl/workbook.xml".into(), workbook),
        ("xl/_rels/workbook.xml.rels".into(), rels),
        (
            "xl/sharedStrings.xml".into(),
            format!("<sst xmlns=\"{NS}\">{shared}</sst>"),
        ),
    ]);
    result
}

fn simple() -> Vec<(String, String)> {
    parts(
        &[(
            "Ledger",
            "visible",
            "<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>synthetic marker</t></is></c></row>",
        )],
        "",
    )
}

fn extract(parts: &[(String, String)]) -> ExtractedDocumentText {
    extract_document(DocumentFormat::Xlsx, &package(parts, false))
}

fn mutate(parts: &mut [(String, String)], name: &str, from: &str, to: &str) {
    let text = &mut parts.iter_mut().find(|(path, _)| path == name).unwrap().1;
    assert!(text.contains(from));
    *text = text.replace(from, to);
}

fn refused(bytes: &[u8], status: Status) {
    let value = extract_document(DocumentFormat::Xlsx, bytes);
    assert_eq!(value.status, status);
    assert!(value.blocks.is_empty());
    assert!(value.xlsx_coverage.is_none());
    assert!(value.pdf_coverage.is_none());
}

#[test]
fn m66_stored_cells_preserve_order_visibility_types_and_formula_cache_honesty() {
    let source = parts(&[
        ("Ledger &amp; notes", "visible", "<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\"><v>45292</v></c><c r=\"C1\" t=\"b\"><v>1</v></c><c r=\"D1\" t=\"e\"><v>#DIV/0!</v></c></row><row r=\"2\"><c r=\"A2\"><f>SUM(A1:B1)</f><v>3</v></c><c r=\"B2\"><f>NOW()</f></c><c r=\"C2\"/></row>"),
        ("Hidden", "hidden", "<row r=\"7\"><c r=\"C7\" t=\"inlineStr\"><is><r><t>hidden </t></r><r><t>value</t></r></is></c></row>"),
        ("Very hidden", "veryHidden", "<row r=\"1048576\"><c r=\"XFD1048576\" t=\"str\"><v>last sparse cell</v></c></row>"),
    ], "<si><r><t>shared </t></r><r><t>rich string</t></r></si>");
    for deflate in [false, true] {
        let value = extract_document(DocumentFormat::Xlsx, &package(&source, deflate));
        assert_eq!(value.status, Status::Partial);
        assert_eq!(value.omissions, vec![Omission::StoredCellsOnly]);
        assert!(value.pdf_coverage.is_none());
        let cells: Vec<_> = value
            .blocks
            .iter()
            .map(|block| (block.locator, block.text.as_str()))
            .collect();
        assert_eq!(
            cells,
            vec![
                (
                    DocumentLocator::XlsxCell {
                        sheet: 1,
                        row: 1,
                        column: 1
                    },
                    "shared rich string"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 1,
                        row: 1,
                        column: 2
                    },
                    "45292"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 1,
                        row: 1,
                        column: 3
                    },
                    "true"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 1,
                        row: 1,
                        column: 4
                    },
                    "#DIV/0!"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 1,
                        row: 2,
                        column: 1
                    },
                    "formula (not evaluated)=SUM(A1:B1); cached (possibly stale)=3"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 1,
                        row: 2,
                        column: 2
                    },
                    "formula (not evaluated)=NOW(); cached=missing"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 2,
                        row: 7,
                        column: 3
                    },
                    "hidden value"
                ),
                (
                    DocumentLocator::XlsxCell {
                        sheet: 3,
                        row: 1048576,
                        column: 16384
                    },
                    "last sparse cell"
                ),
            ]
        );
        assert_eq!(
            value.xlsx_coverage,
            Some(XlsxCoverage {
                sheet_count: 3,
                observed_cell_count: 9,
                included_cell_count: 8,
                sheets: vec![
                    XlsxSheetCoverage {
                        index: 1,
                        name: "Ledger & notes".into(),
                        visibility: Visibility::Visible,
                        observed_cell_count: 7,
                        included_cell_count: 6
                    },
                    XlsxSheetCoverage {
                        index: 2,
                        name: "Hidden".into(),
                        visibility: Visibility::Hidden,
                        observed_cell_count: 1,
                        included_cell_count: 1
                    },
                    XlsxSheetCoverage {
                        index: 3,
                        name: "Very hidden".into(),
                        visibility: Visibility::VeryHidden,
                        observed_cell_count: 1,
                        included_cell_count: 1
                    },
                ],
            })
        );
    }
}

#[test]
fn m66_empty_cells_are_observed_without_invented_readable_text() {
    let value = extract(&parts(
        &[(
            "Empty",
            "visible",
            "<row r=\"1\"><c r=\"A1\"/><c r=\"B1\" t=\"inlineStr\"><is><t> </t></is></c></row>",
        )],
        "",
    ));
    assert_eq!(value.status, Status::NoText);
    assert!(value.blocks.is_empty());
    let coverage = value.xlsx_coverage.unwrap();
    assert_eq!(
        (
            coverage.sheet_count,
            coverage.observed_cell_count,
            coverage.included_cell_count
        ),
        (1, 2, 0)
    );
}

#[test]
fn m66_cell_text_limit_omits_whole_utf8_cell() {
    let exact = "é".repeat(512);
    let over = "é".repeat(513);
    let body = format!("<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>{exact}</t></is></c><c r=\"B1\" t=\"inlineStr\"><is><t>{over}</t></is></c></row>");
    let value = extract(&parts(&[("Bounds", "visible", &body)], ""));
    assert_eq!(value.status, Status::Partial);
    assert_eq!(value.blocks.len(), 1);
    assert_eq!(value.blocks[0].text, exact);
    assert!(value.omissions.contains(&Omission::CellTextLimit));
    let coverage = value.xlsx_coverage.unwrap();
    assert_eq!(
        (coverage.observed_cell_count, coverage.included_cell_count),
        (2, 1)
    );
}

#[test]
fn m66_relationship_escape_external_duplicate_and_wrong_type_fail_without_prefix() {
    for (from, to) in [
        ("worksheets/sheet1.xml", "../worksheets/sheet1.xml"),
        ("worksheets/sheet1.xml", "https://example.invalid/sheet.xml"),
        (
            "Target=\"worksheets/sheet1.xml\"",
            "Target=\"worksheets/sheet1.xml\" TargetMode=\"External\"",
        ),
        ("Id=\"r1\"", "Id=\"strings\""),
        ("/worksheet\"", "/unknown\""),
    ] {
        let mut source = simple();
        mutate(&mut source, "xl/_rels/workbook.xml.rels", from, to);
        let value = extract(&source);
        assert!(matches!(
            value.status,
            Status::Malformed | Status::Unsupported
        ));
        assert!(value.blocks.is_empty());
        assert!(value.xlsx_coverage.is_none());
    }
}

#[test]
fn m66_xml_invalid_late_cells_and_duplicates_discard_every_prefix() {
    for cell in [
        "<c r=\"A1\"><v>2</v></c>",
        "<c r=\"XFE1\"><v>2</v></c>",
        "<c r=\"B1048577\"><v>2</v></c>",
        "<c r=\"B0\"><v>2</v></c>",
        "<c r=\"B1\" t=\"s\"><v>999</v></c>",
        "<c r=\"B1\"><v>2</c>",
        "<c r=\"B1\" t=\"inlineStr\"><is><t>&unknown;</t></is></c>",
        "<c r=\"B1\" t=\"inlineStr\"><is><t>\u{0001}</t></is></c>",
    ] {
        let mut source = simple();
        mutate(
            &mut source,
            "xl/worksheets/sheet1.xml",
            "</row>",
            &format!("{cell}</row>"),
        );
        refused(&package(&source, false), Status::Malformed);
    }
    let mut source = simple();
    source
        .iter_mut()
        .find(|(path, _)| path == "xl/workbook.xml")
        .unwrap()
        .1
        .insert_str(
            0,
            "<!DOCTYPE workbook [<!ENTITY secret SYSTEM 'file:///not-read'>]>",
        );
    refused(&package(&source, false), Status::Malformed);
}

#[test]
fn m66_zip_crc_local_header_mismatch_duplicate_and_truncation_refuse() {
    let bytes = package(&simple(), false);
    let mut corrupt = bytes.clone();
    let offset = corrupt
        .windows(16)
        .position(|window| window == b"synthetic marker")
        .unwrap();
    corrupt[offset] ^= 1;
    refused(&corrupt, Status::Malformed);
    refused(&bytes[..bytes.len() - 8], Status::Malformed);
    let mut duplicate = simple();
    duplicate.push(
        duplicate
            .iter()
            .find(|(name, _)| name == "xl/workbook.xml")
            .unwrap()
            .clone(),
    );
    refused(&package(&duplicate, false), Status::Malformed);
    let mut mismatch = bytes.clone();
    mismatch[30] ^= 1; // First local filename disagrees with its central directory.
    refused(&mismatch, Status::Malformed);
}

#[test]
fn m66_zip_encryption_unsupported_method_and_disk_refuse() {
    let original = package(&simple(), false);
    let archive = ZipArchive::from_slice(original.as_slice()).unwrap();
    let mut entries = archive.entries();
    let entry = entries.next_entry().unwrap().unwrap();
    let central = entry.central_directory_offset() as usize;
    let local = entry.local_header_offset() as usize;
    for (central_delta, local_delta, bytes, status) in [
        (8, 6, 1u16.to_le_bytes(), Status::Encrypted),
        (10, 8, 99u16.to_le_bytes(), Status::Unsupported),
    ] {
        let mut changed = original.clone();
        changed[central + central_delta..central + central_delta + 2].copy_from_slice(&bytes);
        changed[local + local_delta..local + local_delta + 2].copy_from_slice(&bytes);
        refused(&changed, status);
    }
    let mut multidisk = original.clone();
    multidisk[central + 34..central + 36].copy_from_slice(&1u16.to_le_bytes());
    refused(&multidisk, Status::Unsupported);
}

#[test]
fn m66_limits_refuse_before_unbounded_part_or_table_growth() {
    refused(&vec![0; 2 * 1024 * 1024 + 1], Status::TooLarge);
    let sheets = vec![("Bound", "visible", ""); 17];
    refused(&package(&parts(&sheets, ""), false), Status::TooLarge);
    let strings = "<si><t>x</t></si>".repeat(4097);
    refused(
        &package(&parts(&[("Bound", "visible", "")], &strings), false),
        Status::TooLarge,
    );
    let mut entries = simple();
    for index in entries.len()..513 {
        entries.push((format!("unused/{index}.xml"), "".into()));
    }
    refused(&package(&entries, false), Status::TooLarge);
    let mut bomb = simple();
    bomb.iter_mut()
        .find(|(path, _)| path == "xl/sharedStrings.xml")
        .unwrap()
        .1 = format!(
        "<sst xmlns=\"{NS}\"><si><t>{}</t></si></sst>",
        "x".repeat(2 * 1024 * 1024)
    );
    refused(&package(&bomb, true), Status::TooLarge);
}

#[test]
fn m66_cell_count_boundary_and_context_budget_preserve_honest_counts() {
    let body: String = (1..=2000)
        .map(|row| format!("<row r=\"{row}\"><c r=\"A{row}\"><v>{row}</v></c></row>"))
        .collect();
    let value = extract(&parts(&[("Count", "visible", &body)], ""));
    assert_eq!(value.status, Status::Partial);
    let coverage = value.xlsx_coverage.as_ref().unwrap();
    assert_eq!(coverage.observed_cell_count, 2000);
    assert_eq!(coverage.included_cell_count as usize, value.blocks.len());
    assert!(coverage.included_cell_count > 0 && coverage.included_cell_count < 2000);
    assert!(value.omissions.contains(&Omission::TextLimit));
    let snapshot = anima_service_adapters::document_content::DocumentSnapshot {
        attachment_id: "a".repeat(128),
        source_sha256: "a".repeat(64),
        extraction: value,
    };
    assert!(
        anima_service_adapters::document_content::render_document_snapshots(&[snapshot])
            .unwrap()
            .len()
            <= 16 * 1024
    );
    let over = format!("{body}<row r=\"2001\"><c r=\"A2001\"><v>2001</v></c></row>");
    refused(
        &package(&parts(&[("Count", "visible", &over)], ""), false),
        Status::TooLarge,
    );
}

#[test]
fn m66_oversized_shared_values_and_formulas_are_omitted_whole() {
    let shared = format!("<si><t>{}</t></si>", "x".repeat(1025));
    let body = format!("<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>0</v></c><c r=\"C1\"><f>{}</f><v>7</v></c><c r=\"D1\"><v>9</v></c></row>","A1+".repeat(340));
    let value = extract(&parts(&[("Bound", "visible", &body)], &shared));
    assert_eq!(value.status, Status::Partial);
    assert_eq!(value.blocks.len(), 1);
    assert_eq!(value.blocks[0].text, "9");
    assert_eq!(
        value.blocks[0].locator,
        DocumentLocator::XlsxCell {
            sheet: 1,
            row: 1,
            column: 4
        }
    );
    assert!(value.omissions.contains(&Omission::CellTextLimit));
    let coverage = value.xlsx_coverage.unwrap();
    assert_eq!(
        (coverage.observed_cell_count, coverage.included_cell_count),
        (4, 1)
    );
}

#[test]
fn m66_empty_formula_cache_is_missing_and_strings_keep_literal_escapes() {
    let source=parts(&[("Cache","visible","<row r=\"1\"><c r=\"A1\"><f>SUM(1,2)</f><v/></c><c r=\"B1\" t=\"inlineStr\"><is><t>&lt;tool&gt;&amp; &#x1F642; _x0041_</t></is></c></row>")],"");
    let value = extract(&source);
    assert_eq!(value.status, Status::Partial);
    assert_eq!(
        value.blocks[0].text,
        "formula (not evaluated)=SUM(1,2); cached=missing"
    );
    assert_eq!(value.blocks[1].text, "<tool>& 🙂 _x0041_");
}

#[test]
fn m66_unknown_cells_and_shared_formula_inference_are_unsupported() {
    for cell in [
        "<c r=\"B1\" t=\"unknown\"><v>7</v></c>",
        "<c r=\"B1\" t=\"d\"><v>2026-10-16</v></c>",
        "<c r=\"B1\"><f t=\"shared\" si=\"0\"/><v>7</v></c>",
        "<c r=\"B1\"><f t=\"array\" ref=\"B1:C1\">A1</f><v>7</v></c>",
    ] {
        let mut source = simple();
        mutate(
            &mut source,
            "xl/worksheets/sheet1.xml",
            "</row>",
            &format!("{cell}</row>"),
        );
        refused(&package(&source, false), Status::Unsupported);
    }
}

#[test]
fn m66_unhandled_worksheet_relationship_features_are_not_silently_admitted() {
    let mut source = simple();
    source.push(("xl/worksheets/_rels/sheet1.xml.rels".into(),format!("<Relationships xmlns=\"{PKG}\"><Relationship Id=\"external\" Type=\"{REL}/hyperlink\" TargetMode=\"External\" Target=\"https://example.invalid/private\"/></Relationships>")));
    refused(&package(&source, false), Status::Unsupported);
}

#[test]
fn m66_metadata_ambiguity_and_macro_content_types_refuse() {
    for (part, from, to, status) in [
        (
            "xl/workbook.xml",
            "name=\"Ledger\"",
            "name=\"bad&#1;name\"",
            Status::Malformed,
        ),
        (
            "xl/workbook.xml",
            "state=\"visible\"",
            "state=\"secret\"",
            Status::Malformed,
        ),
        (
            "xl/workbook.xml",
            "r:id=\"r1\"",
            "r:id=\"missing\"",
            Status::Malformed,
        ),
        (
            "[Content_Types].xml",
            "spreadsheetml.sheet.main+xml",
            "ms-excel.sheet.macroEnabled.main+xml",
            Status::Unsupported,
        ),
        (
            "[Content_Types].xml",
            "spreadsheetml.worksheet+xml",
            "spreadsheetml.chartsheet+xml",
            Status::Unsupported,
        ),
    ] {
        let mut source = simple();
        mutate(&mut source, part, from, to);
        refused(&package(&source, false), status);
    }
    let mut source = parts(&[("One", "visible", ""), ("Two", "hidden", "")], "");
    mutate(
        &mut source,
        "xl/workbook.xml",
        "sheetId=\"52\"",
        "sheetId=\"51\"",
    );
    refused(&package(&source, false), Status::Malformed);
    let mut source = parts(&[("One", "visible", ""), ("Two", "hidden", "")], "");
    mutate(
        &mut source,
        "xl/_rels/workbook.xml.rels",
        "worksheets/sheet1.xml",
        "worksheets/sheet2.xml",
    );
    refused(&package(&source, false), Status::Malformed);
}

#[test]
fn m66_xml_depth_events_and_late_hidden_sheet_errors_refuse() {
    let mut deep = simple();
    let nested = format!("{}{}", "<extension>".repeat(64), "</extension>".repeat(64));
    mutate(
        &mut deep,
        "xl/worksheets/sheet1.xml",
        "</worksheet>",
        &format!("{nested}</worksheet>"),
    );
    refused(&package(&deep, false), Status::TooLarge);
    let mut events = simple();
    mutate(
        &mut events,
        "xl/worksheets/sheet1.xml",
        "</worksheet>",
        &format!("{}</worksheet>", "<x/>".repeat(125001)),
    );
    refused(&package(&events, false), Status::TooLarge);
    let source = parts(
        &[
            (
                "Visible",
                "visible",
                "<row r=\"1\"><c r=\"A1\"><v>7</v></c></row>",
            ),
            (
                "Late hidden",
                "hidden",
                "<row r=\"1\"><c r=\"A1\"><v>9</c></row>",
            ),
        ],
        "",
    );
    refused(&package(&source, false), Status::Malformed);
    for invalid in [
        "<bad^name/>",
        "<!-- bad -- comment -->",
        "<x xmlns:a=\"urn:a\" xmlns:b=\"urn:a\" a:k=\"1\" b:k=\"2\"/>",
    ] {
        let mut source = simple();
        mutate(
            &mut source,
            "xl/worksheets/sheet1.xml",
            "</worksheet>",
            &format!("{invalid}</worksheet>"),
        );
        refused(&package(&source, false), Status::Malformed);
    }
}

fn entry_offsets(bytes: &[u8], name: &str) -> (usize, usize) {
    let archive = ZipArchive::from_slice(bytes).unwrap();
    let mut entries = archive.entries();
    while let Some(entry) = entries.next_entry().unwrap() {
        if entry.file_path().as_bytes() == name.as_bytes() {
            return (
                entry.central_directory_offset() as usize,
                entry.local_header_offset() as usize,
            );
        }
    }
    panic!("synthetic entry missing");
}

#[test]
fn m66_zip_raw_traversal_size_descriptor_and_reserved_flags_refuse() {
    let original = package(&simple(), false);
    let (central, local) = entry_offsets(&original, "xl/workbook.xml");
    let mut traversal = original.clone();
    for start in [central + 46, local + 30] {
        traversal[start..start + 15].copy_from_slice(b"../workbook.xml");
    }
    refused(&traversal, Status::Malformed);
    for field in [16, 20, 24] {
        let mut changed = original.clone();
        changed[central + field] ^= 1;
        refused(&changed, Status::Malformed);
    }
    let mut reserved = original.clone();
    for start in [central + 8, local + 6] {
        let old = u16::from_le_bytes([reserved[start], reserved[start + 1]]);
        reserved[start..start + 2].copy_from_slice(&(old | 0x10).to_le_bytes());
    }
    refused(&reserved, Status::Unsupported);
    // A second central record claiming the first entry's local bytes must never
    // allow a logically distinct package part to alias an already owned range.
    let (second, _) = entry_offsets(&original, "xl/_rels/workbook.xml.rels");
    let mut overlap = original.clone();
    overlap[second + 42..second + 46].copy_from_slice(&(local as u32).to_le_bytes());
    refused(&overlap, Status::Malformed);
}

#[test]
fn m66_total_expanded_xml_budget_is_shared_across_sheets() {
    // Low-entropy numbered comments compress below the input ceiling while
    // staying below the independent 100:1 DEFLATE ratio and 2MiB part limits.
    let mut padding = String::new();
    for number in 0..90000 {
        padding.push_str(&format!("{number:06}             "));
    }
    let body = format!("<!--{padding}--><row r=\"1\"><c r=\"A1\"><v>1</v></c></row>");
    let source = parts(
        &[
            ("One", "visible", &body),
            ("Two", "hidden", &body),
            ("Three", "visible", &body),
            ("Four", "visible", &body),
            ("Five", "veryHidden", &body),
        ],
        "",
    );
    let bytes = package(&source, true);
    assert!(bytes.len() < 2 * 1024 * 1024);
    refused(&bytes, Status::TooLarge);
}

#[test]
fn m66_raw_cdata_terminator_in_late_text_discards_prior_cells() {
    let mut source = simple();
    mutate(
        &mut source,
        "xl/worksheets/sheet1.xml",
        "</row>",
        "<c r=\"B1\" t=\"inlineStr\"><is><t>late ]]> text</t></is></c></row>",
    );
    refused(&package(&source, false), Status::Malformed);
}

#[test]
fn m66_invalid_late_processing_instruction_target_discards_prior_cells() {
    let mut source = simple();
    mutate(
        &mut source,
        "xl/worksheets/sheet1.xml",
        "</sheetData>",
        "<?1bad payload?></sheetData>",
    );
    refused(&package(&source, false), Status::Malformed);
}

#[test]
fn m66_colon_and_reserved_pi_targets_discard_prior_cells() {
    for instruction in [
        "<?:leading payload?>",
        "<?ordinary:colon payload?>",
        "<?many:more:colons payload?>",
        "<?XML payload?>",
        "<?Xml payload?>",
        "<?xMl payload?>",
    ] {
        let mut source = simple();
        mutate(
            &mut source,
            "xl/worksheets/sheet1.xml",
            "</sheetData>",
            &format!("{instruction}</sheetData>"),
        );
        refused(&package(&source, false), Status::Malformed);
    }
}

#[test]
fn m66_escaped_terminator_cdata_and_ncname_pi_targets_remain_valid() {
    // Namespaces in XML conformance requires colon-free PI targets. Ordinary
    // and Unicode NCNames remain valid without any namespace declaration.
    for instruction in ["<?ordinary?>", "<?处理 données?>"] {
        let body = format!(
            "<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>escaped ]]&gt; text</t></is></c><c r=\"B1\" t=\"inlineStr\"><is><t><![CDATA[raw <& text]]></t></is></c><c r=\"C1\" t=\"inlineStr\"><is><t><![CDATA[]]]]><![CDATA[>]]></t></is></c></row>{instruction}"
        );
        let value = extract(&parts(&[("Valid XML", "visible", &body)], ""));
        assert_eq!(value.status, Status::Partial, "{instruction}");
        assert_eq!(
            value
                .blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            vec!["escaped ]]> text", "raw <& text", "]]>"]
        );
        let coverage = value.xlsx_coverage.unwrap();
        assert_eq!(
            (coverage.observed_cell_count, coverage.included_cell_count),
            (3, 3)
        );
    }
}
