use std::collections::BTreeMap;
use std::io::{Read, Write};

use anima_document_content::{
    check_office_document_text, inspect_office_document, prepare_office_edit, OfficeEditSpec,
    OfficeFormat,
};
use rawzip::{CompressionMethod, ZipArchive, ZipArchiveWriter};
use serde_json::json;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn package(parts: &[(&str, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut zip = ZipArchiveWriter::new(&mut out);
    for (name, body) in parts {
        let (mut entry, config) = zip
            .new_file(*name)
            .compression_method(CompressionMethod::STORE)
            .start()
            .unwrap();
        let mut writer = config.wrap(&mut entry);
        writer.write_all(body.as_bytes()).unwrap();
        let (_, descriptor) = writer.finish().unwrap();
        entry.finish(descriptor).unwrap();
    }
    zip.finish().unwrap();
    out
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let archive = ZipArchive::from_slice(bytes).unwrap();
    let mut entries = archive.entries();
    let mut out = BTreeMap::new();
    while let Some(entry) = entries.next_entry().unwrap() {
        let local = archive.get_entry(entry.wayfinder()).unwrap();
        let mut body = Vec::new();
        if entry.compression_method() == CompressionMethod::STORE {
            local
                .verifying_reader(local.data())
                .read_to_end(&mut body)
                .unwrap();
        } else {
            local
                .verifying_reader(flate2::bufread::DeflateDecoder::new(local.data()))
                .read_to_end(&mut body)
                .unwrap();
        }
        out.insert(
            std::str::from_utf8(entry.file_path().as_bytes())
                .unwrap()
                .to_owned(),
            body,
        );
    }
    out
}

fn types(overrides: &[(&str, &str)]) -> String {
    format!(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>{}</Types>"#, overrides.iter().map(|(path,kind)|format!(r#"<Override PartName="/{path}" ContentType="application/vnd.openxmlformats-officedocument.{kind}+xml"/>"#)).collect::<String>())
}

fn word(body: &str, extra: &[(&str, String)]) -> Vec<u8> {
    let mut entries = vec![
        (
            "[Content_Types].xml",
            types(&[
                ("word/document.xml", "wordprocessingml.document.main"),
                ("word/styles.xml", "wordprocessingml.styles"),
            ]),
        ),
        (
            "_rels/.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>"#
            ),
        ),
        (
            "word/document.xml",
            format!(
                r#"<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}</w:body></w:document>"#
            ),
        ),
        (
            "word/styles.xml",
            format!(r#"<w:styles xmlns:w="{W}"><w:style w:styleId="Keep"/></w:styles>"#),
        ),
    ];
    entries.extend_from_slice(extra);
    package(&entries)
}

fn spec(value: serde_json::Value) -> OfficeEditSpec {
    serde_json::from_value(json!({"operations":value})).unwrap()
}

#[test]
fn editing_word_preserves_styles_and_replaces_unicode_range() {
    let source = word(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Hi 🦀 world</w:t></w:r></w:p>"#,
        &[],
    );
    let inventory = inspect_office_document(OfficeFormat::Docx, &source).unwrap();
    let target = &inventory.targets[0];
    let edit = spec(
        json!([{"type":"word_replace_text","target_id":target.id,"start":3,"end":4,"expected_text":"🦀","new_text":"<&"}]),
    );
    let prepared = prepare_office_edit(OfficeFormat::Docx, &source, &edit).unwrap();
    let before = parts(&source);
    let after = parts(prepared.bytes());
    assert_eq!(before["word/styles.xml"], after["word/styles.xml"]);
    assert!(String::from_utf8_lossy(&after["word/document.xml"]).contains("Hi &lt;&amp; world"));
    assert_eq!(prepared.review().changes.len(), 1);
    assert!(prepared.review().preservation.unchanged_bytes_verified);
}

#[test]
fn editing_retains_inert_hyperlinks_but_rejects_external_images() {
    for target in [
        "https://example.org/report?q=1&amp;x=2",
        "http://example.org/",
        "mailto:editor@example.org",
    ] {
        let rels = format!(
            r#"<Relationships xmlns="{REL}"><Relationship Id="link1" Type="{R}/hyperlink" Target="{target}" TargetMode="External"/></Relationships>"#
        );
        let source = word(
            r#"<w:p><w:hyperlink r:id="link1"><w:r><w:t>Website</w:t></w:r></w:hyperlink><w:r><w:t>Other</w:t></w:r></w:p>"#,
            &[("word/_rels/document.xml.rels", rels)],
        );
        let inventory = inspect_office_document(OfficeFormat::Docx, &source).unwrap();
        assert_eq!(inventory.retained_hyperlinks.len(), 1);
    }
    let rels = format!(
        r#"<Relationships xmlns="{REL}"><Relationship Id="image" Type="{R}/image" Target="https://example.org/image.png" TargetMode="External"/></Relationships>"#
    );
    assert!(inspect_office_document(
        OfficeFormat::Docx,
        &word("<w:p/>", &[("word/_rels/document.xml.rels", rels)])
    )
    .is_err());
}

fn workbook(cells: &str, extra: &[(&str, String)]) -> Vec<u8> {
    let mut entries = vec![
        (
            "[Content_Types].xml",
            types(&[
                ("xl/workbook.xml", "spreadsheetml.sheet.main"),
                ("xl/worksheets/sheet1.xml", "spreadsheetml.worksheet"),
                ("xl/styles.xml", "spreadsheetml.styles"),
            ]),
        ),
        (
            "_rels/.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml",
            format!(
                r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="Budget" sheetId="1" r:id="s1"/></sheets><calcPr calcMode="manual"/></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="s1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/worksheets/sheet1.xml",
            format!(
                r#"<worksheet xmlns="{S}"><dimension ref="A1:C3"/><sheetData><row r="1">{cells}</row></sheetData></worksheet>"#
            ),
        ),
        (
            "xl/styles.xml",
            format!(
                r#"<styleSheet xmlns="{S}"><cellXfs count="1"><xf numFmtId="0"/></cellXfs></styleSheet>"#
            ),
        ),
    ];
    entries.extend_from_slice(extra);
    package(&entries)
}

#[test]
fn editing_spreadsheet_calculates_formula_and_preserves_literal_equals_and_style() {
    let source = workbook(
        r#"<c r="A1" s="0"><v>2</v></c><c r="B1"><v>3</v></c><c r="C1"><f>A1+B1</f><v>999</v></c>"#,
        &[],
    );
    let edits = spec(json!([
        {"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"number","value":7}},
        {"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"C1","formula":"=SUM(A1:B1)*2"},
        {"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A3","value":{"type":"string","value":"=WEBSERVICE(\"literal\")"}}
    ]));
    let prepared = prepare_office_edit(OfficeFormat::Xlsx, &source, &edits).unwrap();
    let after = parts(prepared.bytes());
    let xml = String::from_utf8_lossy(&after["xl/worksheets/sheet1.xml"]);
    assert!(xml.contains("<v>20</v>"), "{xml}");
    assert!(xml.contains("=WEBSERVICE(&quot;literal&quot;)"));
    assert!(xml.contains("s=\"0\""));
    assert_eq!(parts(&source)["xl/styles.xml"], after["xl/styles.xml"]);
    assert!(prepared.review().changes.iter().any(|c| c.derived));
}

#[test]
fn editing_spreadsheet_rejects_active_formulas_and_new_unsupported_formulas() {
    for formula in [
        "WEBSERVICE(\"https://example.org\")",
        "CMD|'/c calc'!A1",
        "[remote.xlsx]Sheet1!A1",
        "UNKNOWN(A1)",
    ] {
        let escaped = formula.replace('&', "&amp;").replace('<', "&lt;");
        let source = workbook(&format!(r#"<c r="A1"><f>{escaped}</f><v>1</v></c>"#), &[]);
        assert!(
            inspect_office_document(OfficeFormat::Xlsx, &source).is_err(),
            "{formula}"
        );
    }
    let source = workbook(r#"<c r="A1"><v>2</v></c>"#, &[]);
    assert!(prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"B1","formula":"=IF(A1,1,0)"}]))).is_err());
}

fn presentation() -> Vec<u8> {
    package(&[
        (
            "[Content_Types].xml",
            types(&[
                ("ppt/presentation.xml", "presentationml.presentation.main"),
                ("ppt/slides/slide1.xml", "presentationml.slide"),
                ("ppt/slides/slide2.xml", "presentationml.slide"),
            ]),
        ),
        (
            "_rels/.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#
            ),
        ),
        (
            "ppt/presentation.xml",
            format!(
                r#"<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="s1"/><p:sldId id="257" r:id="s2"/></p:sldIdLst></p:presentation>"#
            ),
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="s1" Type="{R}/slide" Target="slides/slide1.xml"/><Relationship Id="s2" Type="{R}/slide" Target="slides/slide2.xml"/></Relationships>"#
            ),
        ),
        (
            "ppt/slides/slide1.xml",
            format!(
                r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/></p:nvSpPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr b="1"/><a:t>First 🦀</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
            ),
        ),
        (
            "ppt/slides/slide2.xml",
            format!(
                r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/></p:nvSpPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Second</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
            ),
        ),
    ])
}

#[test]
fn editing_presentation_preserves_slide_parts_while_reordering_and_edits_text() {
    let source = presentation();
    let inventory = inspect_office_document(OfficeFormat::Pptx, &source).unwrap();
    let target = inventory
        .targets
        .iter()
        .find(|t| t.label.contains("Title") && t.id.contains("s1"))
        .unwrap();
    let edits = spec(json!([
        {"type":"slide_replace_text","target_id":target.id,"start":6,"end":7,"expected_text":"🦀","new_text":"updated"},
        {"type":"reorder_slides","slide_ids":["slide:s2","slide:s1"]}
    ]));
    let prepared = prepare_office_edit(OfficeFormat::Pptx, &source, &edits).unwrap();
    let after = parts(prepared.bytes());
    assert_eq!(
        parts(&source)["ppt/slides/slide2.xml"],
        after["ppt/slides/slide2.xml"]
    );
    let main = String::from_utf8_lossy(&after["ppt/presentation.xml"]);
    assert!(main.find("s2").unwrap() < main.find("s1").unwrap());
    assert!(String::from_utf8_lossy(&after["ppt/slides/slide1.xml"]).contains("First updated"));
}

#[test]
fn editing_rejects_mixed_run_styles_stale_text_and_unsafe_links() {
    let source = word(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r><w:r><w:t>plain</w:t></w:r></w:p>"#,
        &[],
    );
    for (end, expected) in [(9, "boldplain"), (4, "wrong")] {
        assert!(prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":end,"expected_text":expected,"new_text":"change"}]))).is_err());
    }
    for target in [
        "https://user:password@example.org/",
        "file:///C:/secret",
        "javascript:run()",
        "\\\\server\\share",
        "https://example.org/%0aevil",
        "mailto:user@example.org?password=secret",
    ] {
        let rels = format!(
            r#"<Relationships xmlns="{REL}"><Relationship Id="x" Type="{R}/hyperlink" Target="{target}" TargetMode="External"/></Relationships>"#
        );
        assert!(
            inspect_office_document(
                OfficeFormat::Docx,
                &word("<w:p/>", &[("word/_rels/document.xml.rels", rels)])
            )
            .is_err(),
            "{target}"
        );
    }
}

fn rewrite(source: &[u8], name: &str, body: String) -> Vec<u8> {
    let mut entries = parts(source)
        .into_iter()
        .map(|(k, v)| (k, String::from_utf8(v).unwrap()))
        .collect::<BTreeMap<_, _>>();
    entries.insert(name.into(), body);
    let borrowed = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect::<Vec<_>>();
    package(&borrowed)
}

#[test]
fn editing_formula_subset_has_deterministic_arithmetic_ranges_and_blank_rules() {
    let source = workbook(r#"<c r="A1"><v>2</v></c><c r="B1"><v>3</v></c>"#, &[]);
    for (formula, expected) in [
        ("=2+3*4", 14.0),
        ("=(2+3)*4", 20.0),
        ("=-A1+B1", 1.0),
        ("=SUM($A$1:B1)", 5.0),
        ("=AVERAGE(A1:B1)", 2.5),
        ("=MIN(A1:B1)", 2.0),
        ("=MAX(A1:B1)", 3.0),
        ("=COUNT(A1:B1)", 2.0),
        ("=D1", 0.0),
        ("=SUM(D1:D3)", 0.0),
    ] {
        let result=prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"C1","formula":formula}]))).unwrap();
        let value = result
            .review()
            .after
            .targets
            .iter()
            .find(|t| t.id == "sheet:s1:C1")
            .unwrap();
        match &value.value {
            anima_document_content::OfficeTargetValue::Cell {
                value, calculation, ..
            } => {
                assert_eq!(
                    value,
                    &anima_document_content::OfficeCellValue::Number(expected),
                    "{formula}"
                );
                assert_eq!(
                    calculation,
                    &anima_document_content::OfficeCalculationState::Calculated
                );
            }
            _ => panic!("expected a cell"),
        }
    }
}

#[test]
fn editing_formula_errors_cycles_and_deferred_functions_have_no_stale_cache() {
    for formula in ["1/0", "AVERAGE(D1:D2)", "IF(A1,7,8)"] {
        let source = workbook(
            &format!(r#"<c r="A1"><v>2</v></c><c r="B1"><f>{formula}</f><v>999</v></c>"#),
            &[],
        );
        let result=prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"number","value":5}}]))).unwrap();
        let after = parts(result.bytes());
        let xml = String::from_utf8_lossy(&after["xl/worksheets/sheet1.xml"]);
        assert!(!xml.contains("999"), "{formula}");
        assert!(xml.contains(&format!("<f>{formula}</f>")));
    }
    let source = workbook(r#"<c r="A1"><v>1</v></c><c r="B1"><v>2</v></c>"#, &[]);
    let result = prepare_office_edit(
        OfficeFormat::Xlsx,
        &source,
        &spec(json!([
            {"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"A1","formula":"=B1"},
            {"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"B1","formula":"=A1"}
        ])),
    )
    .unwrap();
    assert!(result.review().after.targets.iter().all(|target| matches!(
        &target.value,
        anima_document_content::OfficeTargetValue::Cell {
            calculation: anima_document_content::OfficeCalculationState::CalculationError,
            ..
        }
    )));
}

fn chart_workbook() -> Vec<u8> {
    let source = workbook(
        r#"<c r="A1" t="inlineStr"><is><t>East</t></is></c><c r="B1"><v>3</v></c>"#,
        &[],
    );
    let before = parts(&source);
    let mut source = rewrite(
        &source,
        "xl/worksheets/sheet1.xml",
        String::from_utf8(before["xl/worksheets/sheet1.xml"].clone())
            .unwrap()
            .replace(
                &format!("xmlns=\"{S}\""),
                &format!("xmlns=\"{S}\" xmlns:r=\"{R}\""),
            )
            .replace("</worksheet>", "<drawing r:id=\"d1\"/></worksheet>"),
    );
    let c = "http://schemas.openxmlformats.org/drawingml/2006/chart";
    for (name, body) in [
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="d1" Type="{R}/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/drawings/drawing1.xml",
            format!(
                r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:c="{c}" xmlns:r="{R}"><c:chart r:id="chart1"/></xdr:wsDr>"#
            ),
        ),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="chart1" Type="{R}/chart" Target="../charts/chart1.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/charts/chart1.xml",
            format!(
                r#"<c:chartSpace xmlns:c="{c}" xmlns:a="{A}"><c:chart><c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr b="1"/><a:t>Old title</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:cat><c:strRef><c:f>'Budget'!$A$1:$A$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>West</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>'Budget'!$B$1:$B$1</c:f><c:numCache><c:formatCode>0.00</c:formatCode><c:ptCount val="1"/><c:pt idx="0"><c:v>999</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#
            ),
        ),
    ] {
        source = rewrite(&source, name, body);
    }
    let types=String::from_utf8(parts(&source)["[Content_Types].xml"].clone()).unwrap().replace("</Types>",r#"<Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/><Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/></Types>"#);
    rewrite(&source, "[Content_Types].xml", types)
}

#[test]
fn editing_chart_cell_changes_refresh_categories_values_and_number_format() {
    let source = chart_workbook();
    let result=prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"B1","value":{"type":"number","value":10}}]))).unwrap();
    let after = parts(result.bytes());
    let chart = String::from_utf8_lossy(&after["xl/charts/chart1.xml"]);
    assert!(chart.contains("<c:v>East</c:v>"));
    assert!(chart.contains("<c:v>10</c:v>"));
    assert!(!chart.contains("999"));
    assert!(chart.contains("<c:formatCode>0.00</c:formatCode>"));
    assert_eq!(
        parts(&source)["xl/drawings/drawing1.xml"],
        after["xl/drawings/drawing1.xml"]
    );
}

#[test]
fn editing_chart_titles_and_series_ranges_are_editable() {
    let source = chart_workbook();
    let result=prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([
        {"type":"set_chart_title","chart_id":"chart:0","expected_title":"Old title","title":"Updated <chart>"},
        {"type":"set_chart_series_ranges","series_id":"chart:0:series:0","categories":{"sheet_id":"sheet:s1","start":"A1","end":"A1"},"values":{"sheet_id":"sheet:s1","start":"B1","end":"B1"}}
    ]))).unwrap();
    let after = parts(result.bytes());
    let chart = String::from_utf8_lossy(&after["xl/charts/chart1.xml"]);
    assert!(chart.contains("Updated &lt;chart&gt;"));
    assert!(chart.contains("<a:rPr b=\"1\"/>"));
    assert!(chart.contains("<c:v>3</c:v>"));
}

#[test]
fn editing_cell_changes_block_unsupported_charts_instead_of_stale_caches() {
    let source = chart_workbook();
    let chart = String::from_utf8(parts(&source)["xl/charts/chart1.xml"].clone())
        .unwrap()
        .replace("barChart", "scatterChart");
    let source = rewrite(&source, "xl/charts/chart1.xml", chart);
    assert!(prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"B1","value":{"type":"number","value":10}}]))).is_err());
}

#[test]
fn editing_hyperlink_relationship_bytes_survive_an_unrelated_word_edit() {
    let rels = format!(
        r#"<Relationships xmlns="{REL}"><Relationship Id="link" Type="{R}/hyperlink" Target="https://example.org/" TargetMode="External"/></Relationships>"#
    );
    let source = word(
        r#"<w:p><w:hyperlink r:id="link"><w:r><w:t>Website</w:t></w:r></w:hyperlink></w:p><w:p><w:r><w:t>Edit me</w:t></w:r></w:p>"#,
        &[("word/_rels/document.xml.rels", rels)],
    );
    let inventory = inspect_office_document(OfficeFormat::Docx, &source).unwrap();
    assert!(!inventory.targets[0].editable);
    let result=prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:1","start":0,"end":4,"expected_text":"Edit","new_text":"Read"}]))).unwrap();
    assert_eq!(
        parts(&source)["word/_rels/document.xml.rels"],
        parts(result.bytes())["word/_rels/document.xml.rels"]
    );
}

#[test]
fn editing_rejects_dtd_malformed_xml_unknown_parts_and_control_text() {
    let source = word("<w:p><w:r><w:t>Safe</w:t></w:r></w:p>", &[]);
    for body in [
        format!(
            r#"<!DOCTYPE x [<!ENTITY bomb "bad">]><w:document xmlns:w="{W}"><w:body/></w:document>"#
        ),
        format!(r#"<w:document xmlns:w="{W}"><w:body>&unknown;</w:body></w:document>"#),
        format!(r#"<w:document xmlns:w="{W}"><w:body><w:bad$name/></w:body></w:document>"#),
    ] {
        assert!(inspect_office_document(
            OfficeFormat::Docx,
            &rewrite(&source, "word/document.xml", body)
        )
        .is_err());
    }
    assert!(inspect_office_document(
        OfficeFormat::Docx,
        &rewrite(&source, "word/vbaProject.bin", "macro".into())
    )
    .is_err());
    assert!(prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":4,"expected_text":"Safe","new_text":"bad\u{0000}"}]))).is_err());
}

#[test]
fn editing_rejects_overlapping_requests_and_invalid_permutations() {
    let source = word("<w:p><w:r><w:t>Safe</w:t></w:r></w:p>", &[]);
    let operation = json!({"type":"word_replace_text","target_id":"word:0","start":0,"end":4,"expected_text":"Safe","new_text":"New"});
    assert!(prepare_office_edit(
        OfficeFormat::Docx,
        &source,
        &spec(json!([operation, operation]))
    )
    .is_err());
    for ids in [
        json!(["slide:s1"]),
        json!(["slide:s1", "slide:s1"]),
        json!(["slide:s1", "slide:unknown"]),
    ] {
        assert!(prepare_office_edit(
            OfficeFormat::Pptx,
            &presentation(),
            &spec(json!([{"type":"reorder_slides","slide_ids":ids}]))
        )
        .is_err());
    }
}

#[test]
fn editing_rejects_unknown_operation_fields_and_coordinate_smuggling() {
    assert!(serde_json::from_value::<OfficeEditSpec>(json!({"operations":[{"type":"set_cell_value","sheet_id":"s","cell":"A1","value":{"type":"blank"},"xml":"bad"}]})).is_err());
    let source = workbook("", &[]);
    for address in [
        "A0",
        "XFE1",
        "A1048577",
        "A$1$",
        "../A1",
        "Sheet1!A1",
        "A1:B2",
    ] {
        assert!(prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":address,"value":{"type":"number","value":1}}]))).is_err(),"{address}");
    }
}

#[test]
fn editing_external_word_template_retains_header_styles_media_and_relationships() {
    use base64::Engine;
    let body = include_str!("fixtures/editing/word-template.xml");
    let source = word("", &[]);
    let mut entries = parts(&source);
    entries.insert("word/document.xml".into(), body.as_bytes().to_vec());
    let types=String::from_utf8(entries["[Content_Types].xml"].clone()).unwrap().replace("</Types>",r#"<Default Extension="png" ContentType="image/png"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#);
    entries.insert("[Content_Types].xml".into(), types.into_bytes());
    entries.insert("word/header1.xml".into(),format!(r#"<w:hdr xmlns:w="{W}" xmlns:a="{A}" xmlns:r="{R}"><w:p><w:r><w:t>HEADER_SENTINEL</w:t></w:r><w:r><w:drawing><a:blip r:embed="image"/></w:drawing></w:r></w:p></w:hdr>"#).into_bytes());
    entries.insert("word/_rels/document.xml.rels".into(),format!(r#"<Relationships xmlns="{REL}"><Relationship Id="header" Type="{R}/header" Target="header1.xml"/></Relationships>"#).into_bytes());
    entries.insert("word/_rels/header1.xml.rels".into(),format!(r#"<Relationships xmlns="{REL}"><Relationship Id="image" Type="{R}/image" Target="../media/pixel.png"/></Relationships>"#).into_bytes());
    entries.insert(
        "media/pixel.png".into(),
        base64::engine::general_purpose::STANDARD
            .decode(include_str!("fixtures/editing/pixel.png.base64").trim())
            .unwrap(),
    );
    let mut bytes = Vec::new();
    let mut zip = ZipArchiveWriter::new(&mut bytes);
    for (name, body) in &entries {
        let (mut entry, config) = zip
            .new_file(name)
            .compression_method(CompressionMethod::STORE)
            .start()
            .unwrap();
        let mut writer = config.wrap(&mut entry);
        writer.write_all(body).unwrap();
        let (_, descriptor) = writer.finish().unwrap();
        entry.finish(descriptor).unwrap();
    }
    zip.finish().unwrap();
    let result=prepare_office_edit(OfficeFormat::Docx,&bytes,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":8,"expected_text":"Original","new_text":"Revised"}]))).unwrap();
    let after = parts(result.bytes());
    for (name, body) in entries {
        if name != "word/document.xml" {
            assert_eq!(after[&name], body, "preserved part {name}");
        }
    }
    assert!(String::from_utf8_lossy(&after["word/document.xml"]).contains("Revised template text"));
    assert!(String::from_utf8_lossy(&after["word/document.xml"]).contains("Retained table cell"));
}

#[test]
fn editing_rejects_bad_crc_trailing_zip_bytes_and_expanded_limits() {
    let source = word("<w:p><w:r><w:t>CRC_SENTINEL</w:t></w:r></w:p>", &[]);
    let mut bad = source.clone();
    let offset = bad.windows(12).position(|v| v == b"CRC_SENTINEL").unwrap();
    bad[offset] ^= 1;
    assert!(inspect_office_document(OfficeFormat::Docx, &bad).is_err());
    let mut trailing = source;
    trailing.extend_from_slice(b"garbage");
    assert!(inspect_office_document(OfficeFormat::Docx, &trailing).is_err());
    assert!(inspect_office_document(OfficeFormat::Docx, &vec![0; 4 * 1024 * 1024 + 1]).is_err());
}

#[test]
fn editing_rejects_external_template_dde_fields_and_active_actions() {
    let source = word(
        r#"<w:p><w:fldSimple w:instr="DDE AUTO"><w:r><w:t>Unsafe field</w:t></w:r></w:fldSimple></w:p>"#,
        &[],
    );
    assert!(inspect_office_document(OfficeFormat::Docx, &source).is_err());
    let rels = format!(
        r#"<Relationships xmlns="{REL}"><Relationship Id="template" Type="{R}/attachedTemplate" Target="https://example.org/template.dotm" TargetMode="External"/></Relationships>"#
    );
    assert!(inspect_office_document(
        OfficeFormat::Docx,
        &word("<w:p/>", &[("word/_rels/document.xml.rels", rels)])
    )
    .is_err());
    let source = presentation();
    let slide = String::from_utf8(parts(&source)["ppt/slides/slide1.xml"].clone())
        .unwrap()
        .replace(
            "<a:rPr b=\"1\"/>",
            "<a:rPr b=\"1\"><a:hlinkClick action=\"ppaction://macro?name=run\"/></a:rPr>",
        );
    assert!(inspect_office_document(
        OfficeFormat::Pptx,
        &rewrite(&source, "ppt/slides/slide1.xml", slide)
    )
    .is_err());
}

#[test]
fn editing_external_image_cannot_masquerade_as_a_safe_hyperlink() {
    let rels = format!(
        r#"<Relationships xmlns="{REL}"><Relationship Id="remote" Type="{R}/hyperlink" Target="https://example.org/image.png" TargetMode="External"/></Relationships>"#
    );
    let body = format!(
        r#"<w:p><w:r><w:drawing><a:blip xmlns:a="{A}" r:link="remote"/></w:drawing></w:r></w:p>"#
    );
    assert!(inspect_office_document(
        OfficeFormat::Docx,
        &word(&body, &[("word/_rels/document.xml.rels", rels)])
    )
    .is_err());
}

#[test]
fn editing_empty_sheet_data_accepts_new_rows_in_coordinate_order() {
    let source = workbook("", &[]);
    let source = rewrite(
        &source,
        "xl/worksheets/sheet1.xml",
        format!(r#"<worksheet xmlns="{S}"><sheetData/></worksheet>"#),
    );
    let result = prepare_office_edit(OfficeFormat::Xlsx, &source, &spec(json!([
        {"type":"set_cell_value","sheet_id":"sheet:s1","cell":"C3","value":{"type":"number","value":3}},
        {"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"number","value":1}}
    ]))).unwrap();
    let after = parts(result.bytes());
    let xml = String::from_utf8_lossy(&after["xl/worksheets/sheet1.xml"]);
    assert!(xml.find("r=\"1\"").unwrap() < xml.find("r=\"3\"").unwrap());
}

#[test]
fn editing_hidden_rows_and_columns_are_identified_in_content_review() {
    let source = workbook(r#"<c r="A1"><v>1</v></c>"#, &[]);
    let worksheet = String::from_utf8(parts(&source)["xl/worksheets/sheet1.xml"].clone())
        .unwrap()
        .replace(
            "<sheetData>",
            "<cols><col min=\"1\" max=\"1\" hidden=\"1\"/></cols><sheetData>",
        )
        .replace("<row r=\"1\">", "<row r=\"1\" hidden=\"1\">");
    let source = rewrite(&source, "xl/worksheets/sheet1.xml", worksheet);
    let inventory = inspect_office_document(OfficeFormat::Xlsx, &source).unwrap();
    assert!(inventory.targets[0].label.contains("hidden"));
}

#[test]
fn editing_new_plain_strings_preserve_shared_string_table_and_other_cells() {
    let source = workbook(
        r#"<c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>0</v></c>"#,
        &[],
    );
    let source = rewrite(
        &source,
        "xl/sharedStrings.xml",
        format!(
            r#"<sst xmlns="{S}" count="2" uniqueCount="1"><si><r><rPr><b/></rPr><t>Shared original</t></r></si></sst>"#
        ),
    );
    let rels = String::from_utf8(parts(&source)["xl/_rels/workbook.xml.rels"].clone()).unwrap().replace("</Relationships>", &format!(r#"<Relationship Id="strings" Type="{R}/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#));
    let source = rewrite(&source, "xl/_rels/workbook.xml.rels", rels);
    let result = prepare_office_edit(OfficeFormat::Xlsx, &source, &spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"string","value":"Changed"}}]))).unwrap();
    let after = parts(result.bytes());
    assert_eq!(
        parts(&source)["xl/sharedStrings.xml"],
        after["xl/sharedStrings.xml"]
    );
    let xml = String::from_utf8_lossy(&after["xl/worksheets/sheet1.xml"]);
    assert!(xml.contains(r#"<c r="B1" t="s"><v>0</v></c>"#));
}

#[test]
fn editing_calc_chain_and_its_relationship_and_type_are_removed_together() {
    let source = workbook(
        r#"<c r="A1"><f>B1</f><v>999</v></c><c r="B1"><v>2</v></c>"#,
        &[],
    );
    let source = rewrite(
        &source,
        "xl/calcChain.xml",
        format!(r#"<calcChain xmlns="{S}"><c r="A1" i="1"/></calcChain>"#),
    );
    let rels = String::from_utf8(parts(&source)["xl/_rels/workbook.xml.rels"].clone()).unwrap().replace("</Relationships>", &format!(r#"<Relationship Id="chain" Type="{R}/calcChain" Target="calcChain.xml"/></Relationships>"#));
    let source = rewrite(&source, "xl/_rels/workbook.xml.rels", rels);
    let types = String::from_utf8(parts(&source)["[Content_Types].xml"].clone()).unwrap().replace("</Types>", r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#);
    let source = rewrite(&source, "[Content_Types].xml", types);
    let result = prepare_office_edit(OfficeFormat::Xlsx, &source, &spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"B1","value":{"type":"number","value":3}}]))).unwrap();
    let after = parts(result.bytes());
    assert!(!after.contains_key("xl/calcChain.xml"));
    for name in ["[Content_Types].xml", "xl/_rels/workbook.xml.rels"] {
        assert!(!String::from_utf8_lossy(&after[name]).contains("calcChain"));
    }
    assert!(String::from_utf8_lossy(&after["xl/worksheets/sheet1.xml"]).contains("<v>3</v>"));
}

#[test]
fn editing_uniform_runs_preserve_properties_and_support_multirun_ranges() {
    let source = word(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>He</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>llo</w:t></w:r></w:p>"#,
        &[],
    );
    let result = prepare_office_edit(OfficeFormat::Docx, &source, &spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":5,"expected_text":"Hello","new_text":" Hi "}]))).unwrap();
    let after = parts(result.bytes());
    let xml = String::from_utf8_lossy(&after["word/document.xml"]);
    assert_eq!(xml.matches("<w:rPr><w:b/></w:rPr>").count(), 2);
    assert!(xml.contains("xml:space=\"preserve\"> Hi </w:t>"));
}

#[test]
fn editing_formula_range_budget_defers_without_iterating_full_excel_grid() {
    let source = workbook(r#"<c r="A1"><v>1</v></c>"#, &[]);
    let result = prepare_office_edit(OfficeFormat::Xlsx, &source, &spec(json!([{"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"B1","formula":"=SUM(C1:C1048576)"}]))).unwrap();
    let target = result
        .review()
        .after
        .targets
        .iter()
        .find(|t| t.id == "sheet:s1:B1")
        .unwrap();
    assert!(matches!(
        &target.value,
        anima_document_content::OfficeTargetValue::Cell {
            calculation: anima_document_content::OfficeCalculationState::NeedsExternalRecalculation,
            ..
        }
    ));
}

#[test]
fn editing_raw_archive_traversal_and_case_collision_are_rejected() {
    for hostile in [
        "../escape.xml",
        "word/./document.xml",
        "WORD/DOCUMENT.XML",
        "word\\document.xml",
    ] {
        let placeholder = format!("{}.xml", "z".repeat(hostile.len() - 4));
        let mut source = word(
            "<w:p/>",
            &[(&placeholder, format!(r#"<w:styles xmlns:w="{W}"/>"#))],
        );
        assert!(inspect_office_document(OfficeFormat::Docx, &source).is_ok());
        let (central, local) = {
            let archive = ZipArchive::from_slice(source.as_slice()).unwrap();
            let mut entries = archive.entries();
            let mut offsets = None;
            while let Some(entry) = entries.next_entry().unwrap() {
                if entry.file_path().as_bytes() == placeholder.as_bytes() {
                    offsets = Some((
                        entry.central_directory_offset() as usize + 46,
                        entry.local_header_offset() as usize + 30,
                    ));
                    break;
                }
            }
            offsets.unwrap()
        };
        for offset in [central, local] {
            source[offset..offset + hostile.len()].copy_from_slice(hostile.as_bytes());
        }
        assert!(
            inspect_office_document(OfficeFormat::Docx, &source).is_err(),
            "{hostile}"
        );
    }
}

#[test]
fn editing_renamed_pivot_parts_block_cell_edits_by_type_and_relationship() {
    let base = workbook(r#"<c r="A1"><v>1</v></c>"#, &[]);
    let base = rewrite(
        &base,
        "xl/cache/data.xml",
        format!(r#"<pivotCacheDefinition xmlns="{S}"/>"#),
    );
    for via_type in [true, false] {
        let source = if via_type {
            let types = String::from_utf8(parts(&base)["[Content_Types].xml"].clone()).unwrap().replace("</Types>", r#"<Override PartName="/xl/cache/data.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml"/></Types>"#);
            rewrite(&base, "[Content_Types].xml", types)
        } else {
            let rels = String::from_utf8(parts(&base)["xl/_rels/workbook.xml.rels"].clone()).unwrap().replace("</Relationships>", &format!(r#"<Relationship Id="cache" Type="{R}/pivotCacheDefinition" Target="cache/data.xml"/></Relationships>"#));
            rewrite(&base, "xl/_rels/workbook.xml.rels", rels)
        };
        assert!(inspect_office_document(OfficeFormat::Xlsx, &source).is_ok());
        assert!(prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"number","value":2}}]))).is_err(),"via_type={via_type}");
    }
}

#[test]
fn editing_rejects_ranges_outside_aggregate_arguments() {
    let source = workbook(r#"<c r="A1"><v>1</v></c>"#, &[]);
    for formula in ["=A1:B2", "=A1:B2+1", "=SUM(A1:B2+1)", "=-A1:B2"] {
        assert!(prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"C1","formula":formula}]))).is_err(),"{formula}");
    }
    assert!(prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_formula","sheet_id":"sheet:s1","cell":"C1","formula":"=SUM(A1:B2)"}]))).is_ok());
}

#[test]
fn editing_custom_show_is_preserved_byte_for_byte_when_slide_order_changes() {
    let source = presentation();
    let original = String::from_utf8(parts(&source)["ppt/presentation.xml"].clone()).unwrap();
    let show = r#"<p:custShowLst><p:custShow name="Sales" id="1"><p:sldLst><p:sld r:id="s2"/><p:sld r:id="s1"/></p:sldLst></p:custShow></p:custShowLst>"#;
    let source = rewrite(
        &source,
        "ppt/presentation.xml",
        original.replace("</p:presentation>", &format!("{show}</p:presentation>")),
    );
    let result = prepare_office_edit(
        OfficeFormat::Pptx,
        &source,
        &spec(json!([{"type":"reorder_slides","slide_ids":["slide:s2","slide:s1"]}])),
    )
    .unwrap();
    let after = parts(result.bytes());
    let main = String::from_utf8_lossy(&after["ppt/presentation.xml"]);
    assert!(main.contains(show));
}

#[test]
fn editing_safe_fixed_range_names_are_admitted_but_executable_names_are_rejected() {
    let source = workbook(
        r#"<c r="A1"><v>1</v></c><c r="B1"><f>SUM(Totals)</f><v>1</v></c>"#,
        &[],
    );
    let original = String::from_utf8(parts(&source)["xl/workbook.xml"].clone()).unwrap();
    let name = r#"<definedNames><definedName name="Totals">'Budget'!$A$1:$A$1</definedName></definedNames>"#;
    let source = rewrite(
        &source,
        "xl/workbook.xml",
        original.replace("<calcPr", &format!("{name}<calcPr")),
    );
    assert!(inspect_office_document(OfficeFormat::Xlsx, &source).is_ok());
    let result=prepare_office_edit(OfficeFormat::Xlsx,&source,&spec(json!([{"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"number","value":2}}]))).unwrap();
    assert!(result.review().after.targets.iter().any(|t|matches!(&t.value,anima_document_content::OfficeTargetValue::Cell{formula:Some(f),calculation:anima_document_content::OfficeCalculationState::NeedsExternalRecalculation,..} if f=="SUM(Totals)")));
    let malicious = String::from_utf8(parts(&source)["xl/workbook.xml"].clone())
        .unwrap()
        .replace("name=\"Totals\"", "name=\"_xlnm.Auto_Open\"");
    assert!(inspect_office_document(
        OfficeFormat::Xlsx,
        &rewrite(&source, "xl/workbook.xml", malicious)
    )
    .is_err());
}

#[test]
fn editing_word_keeps_unselected_raw_entities_and_attribute_quoting() {
    let source = word(
        r#"<w:p><w:r><w:t xml:space='default' custom='keep'>A&#x41;🐈 &amp;#x41; tail</w:t></w:r></w:p>"#,
        &[],
    );
    let inventory = inspect_office_document(OfficeFormat::Docx, &source).unwrap();
    let anima_document_content::OfficeTargetValue::Text { text } = &inventory.targets[0].value
    else {
        panic!("text target")
    };
    let start = text[..text.find("tail").unwrap()].chars().count();
    let result=prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":start,"end":start+4,"expected_text":"tail","new_text":"end"}]))).unwrap();
    let after = parts(result.bytes());
    let xml = String::from_utf8_lossy(&after["word/document.xml"]);
    assert!(
        xml.contains("<w:t xml:space='default' custom='keep'>A&#x41;🐈 &amp;#x41; end</w:t>"),
        "{xml}"
    );
}

#[test]
fn editing_word_reports_required_xml_space_change_and_keeps_tag_quotes() {
    let source = word(
        r#"<w:p><w:r><w:t custom='keep' xml:space='default'>old</w:t></w:r></w:p>"#,
        &[],
    );
    let result=prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":3,"expected_text":"old","new_text":" new "}]))).unwrap();
    let after = parts(result.bytes());
    let xml = String::from_utf8_lossy(&after["word/document.xml"]);
    assert!(
        xml.contains("<w:t custom='keep' xml:space='preserve'> new </w:t>"),
        "{xml}"
    );
    assert!(result
        .review()
        .changes
        .iter()
        .any(|change| change.derived && change.label.contains("whitespace")));
}

#[test]
fn editing_powerpoint_text_keeps_unselected_entities_and_original_tags() {
    let source = presentation();
    let slide = String::from_utf8(parts(&source)["ppt/slides/slide1.xml"].clone())
        .unwrap()
        .replace(
            "<a:t>First 🦀</a:t>",
            "<a:t custom='keep'>First &#x1F980; &amp;#x41; tail</a:t>",
        );
    let source = rewrite(&source, "ppt/slides/slide1.xml", slide);
    let inventory = inspect_office_document(OfficeFormat::Pptx, &source).unwrap();
    let target = inventory
        .targets
        .iter()
        .find(|t| {
            t.id.contains("s1")
                && matches!(
                    t.value,
                    anima_document_content::OfficeTargetValue::Text { .. }
                )
        })
        .unwrap();
    let anima_document_content::OfficeTargetValue::Text { text } = &target.value else {
        panic!("text target")
    };
    let start = text[..text.find("tail").unwrap()].chars().count();
    let result=prepare_office_edit(OfficeFormat::Pptx,&source,&spec(json!([{"type":"slide_replace_text","target_id":target.id,"start":start,"end":start+4,"expected_text":"tail","new_text":"end"}]))).unwrap();
    let after = parts(result.bytes());
    let xml = String::from_utf8_lossy(&after["ppt/slides/slide1.xml"]);
    assert!(
        xml.contains("<a:t custom='keep'>First &#x1F980; &amp;#x41; end</a:t>"),
        "{xml}"
    );
}

#[test]
fn editing_multirun_text_preserves_unselected_entities_and_comments() {
    let source = word(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t custom='one'>before&#x41;猫</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t custom='two'>犬&#x42;after</w:t></w:r></w:p>"#,
        &[],
    );
    let result=prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":7,"end":9,"expected_text":"猫犬","new_text":"X"}]))).unwrap();
    let output = parts(result.bytes());
    let text = String::from_utf8_lossy(&output["word/document.xml"]);
    assert!(
        text.contains("<w:t custom='one'>before&#x41;X</w:t><"),
        "{text}"
    );
    assert!(
        text.contains("<w:t custom='two'>&#x42;after</w:t>"),
        "{text}"
    );
    let source = word(r#"<w:p><w:r><w:t>ab<!--keep-->cd</w:t></w:r></w:p>"#, &[]);
    let result=prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":1,"end":3,"expected_text":"bc","new_text":"X"}]))).unwrap();
    assert!(
        String::from_utf8_lossy(&parts(result.bytes())["word/document.xml"])
            .contains("aX<!--keep-->d")
    );
}

#[test]
fn editing_text_only_adds_whitespace_attribute_when_edge_semantics_require_it() {
    let source = word(
        r#"<w:p><w:r><w:t custom='keep'>before end</w:t></w:r></w:p>"#,
        &[],
    );
    let result=prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":6,"expected_text":"before","new_text":"after"}]))).unwrap();
    assert!(
        String::from_utf8_lossy(&parts(result.bytes())["word/document.xml"])
            .contains("<w:t custom='keep'>after end</w:t>")
    );
    assert!(!result.review().changes.iter().any(|change| change.derived));
    let empty = word(r#"<w:p><w:r><w:t xml:space='default'/></w:r></w:p>"#, &[]);
    let result=prepare_office_edit(OfficeFormat::Docx,&empty,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":0,"expected_text":"","new_text":" new "}]))).unwrap();
    assert!(
        String::from_utf8_lossy(&parts(result.bytes())["word/document.xml"])
            .contains("<w:t xml:space='preserve'> new </w:t>")
    );
    assert!(result.review().changes.iter().any(|change| change.derived));
}

#[test]
fn editing_cdata_text_is_preserved_but_not_silently_rewritten() {
    let source = word(
        r#"<w:p><w:r><w:t><![CDATA[original text]]></w:t></w:r></w:p>"#,
        &[],
    );
    let inventory = inspect_office_document(OfficeFormat::Docx, &source).unwrap();
    assert!(!inventory.targets[0].editable);
    assert!(prepare_office_edit(OfficeFormat::Docx,&source,&spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":8,"expected_text":"original","new_text":"changed"}]))).is_err());
}

#[test]
fn editing_custom_show_cannot_reference_an_unlisted_or_external_slide() {
    let source = presentation();
    let xml=String::from_utf8(parts(&source)["ppt/presentation.xml"].clone()).unwrap().replace("</p:presentation>",r#"<p:custShowLst><p:custShow name="unsafe" id="1"><p:sldLst><p:sld r:id="elsewhere"/></p:sldLst></p:custShow></p:custShowLst></p:presentation>"#);
    let source = rewrite(&source, "ppt/presentation.xml", xml);
    let rels=String::from_utf8(parts(&source)["ppt/_rels/presentation.xml.rels"].clone()).unwrap().replace("</Relationships>",&format!(r#"<Relationship Id="elsewhere" Type="{R}/slide" Target="slides/slide1.xml"/></Relationships>"#));
    assert!(inspect_office_document(
        OfficeFormat::Pptx,
        &rewrite(&source, "ppt/_rels/presentation.xml.rels", rels)
    )
    .is_err());
}

fn test_credential_policy(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    !normalized.contains("password=") && !normalized.contains("access_token")
}

#[test]
fn editing_private_policy_screen_binds_custom_property_names_to_values() {
    fn document(name: &str, value: &str) -> Vec<u8> {
        let source = word(r#"<w:p><w:r><w:t>Public</w:t></w:r></w:p>"#, &[]);
        let content_types = String::from_utf8(parts(&source)["[Content_Types].xml"].clone())
            .unwrap()
            .replace(
                "</Types>",
                r#"<Override PartName="/docProps/custom.xml" ContentType="application/vnd.openxmlformats-officedocument.custom-properties+xml"/></Types>"#,
            );
        let source = rewrite(&source, "[Content_Types].xml", content_types);
        let relationships = format!(
            r#"<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="word/document.xml"/><Relationship Id="r2" Type="{R}/custom-properties" Target="docProps/custom.xml"/></Relationships>"#
        );
        let source = rewrite(&source, "_rels/.rels", relationships);
        rewrite(
            &source,
            "docProps/custom.xml",
            format!(
                r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="2" name="{name}"><vt:lpwstr>{value}</vt:lpwstr></property></Properties>"#
            ),
        )
    }

    let unsafe_source = document("pass&#x77;ord", "synthetic-&#x74;est");
    assert!(!check_office_document_text(
        OfficeFormat::Docx,
        &unsafe_source,
        test_credential_policy
    )
    .unwrap());
    let candidate = prepare_office_edit(OfficeFormat::Docx, &unsafe_source, &spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":6,"expected_text":"Public","new_text":"Changed"}]))).unwrap();
    assert!(!check_office_document_text(
        OfficeFormat::Docx,
        candidate.bytes(),
        test_credential_policy
    )
    .unwrap());
    assert_eq!(
        parts(&unsafe_source)["docProps/custom.xml"],
        parts(candidate.bytes())["docProps/custom.xml"]
    );
    assert!(!serde_json::to_string(candidate.review())
        .unwrap()
        .contains("synthetic-test"));

    let safe_source = document("department", "synthetic-test");
    assert!(
        check_office_document_text(OfficeFormat::Docx, &safe_source, test_credential_policy)
            .unwrap()
    );
    let separated = document("label", "password");
    let custom = String::from_utf8(parts(&separated)["docProps/custom.xml"].clone())
        .unwrap()
        .replace("</Properties>", r#"<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="note"><vt:lpwstr>= separate item</vt:lpwstr></property></Properties>"#);
    let separated = rewrite(&separated, "docProps/custom.xml", custom);
    assert!(
        check_office_document_text(OfficeFormat::Docx, &separated, test_credential_policy).unwrap()
    );
    let separated_candidate = prepare_office_edit(OfficeFormat::Docx, &separated, &spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":6,"expected_text":"Public","new_text":"Changed"}]))).unwrap();
    assert!(check_office_document_text(
        OfficeFormat::Docx,
        separated_candidate.bytes(),
        test_credential_policy
    )
    .unwrap());
}

#[test]
fn editing_private_policy_screen_visits_retained_word_parts_and_run_fragments() {
    for (name, root) in [
        ("word/header1.xml", "hdr"),
        ("word/footer1.xml", "ftr"),
        ("word/comments.xml", "comments"),
        ("word/footnotes.xml", "footnotes"),
        ("word/endnotes.xml", "endnotes"),
    ] {
        let extra = format!(
            r#"<w:{root} xmlns:w="{W}"><w:p><w:r><w:t>pass</w:t></w:r><w:r><w:t>word=synthetic-test</w:t></w:r></w:p></w:{root}>"#
        );
        let source = word(
            "<w:p><w:r><w:t>Public body</w:t></w:r></w:p>",
            &[(name, extra)],
        );
        assert!(
            !check_office_document_text(OfficeFormat::Docx, &source, test_credential_policy)
                .unwrap(),
            "{name}"
        );
        let inventory = inspect_office_document(OfficeFormat::Docx, &source).unwrap();
        assert!(!serde_json::to_string(&inventory)
            .unwrap()
            .contains("synthetic-test"));
    }
}

#[test]
fn editing_private_policy_screen_visits_shared_strings_and_presentation_notes() {
    let source = workbook(
        r#"<c r="A1"><v>1</v></c>"#,
        &[(
            "xl/sharedStrings.xml",
            format!(r#"<sst xmlns="{S}"><si><r><t>access_</t></r><r><t>token</t></r></si></sst>"#),
        )],
    );
    assert!(
        !check_office_document_text(OfficeFormat::Xlsx, &source, test_credential_policy).unwrap()
    );
    let source = rewrite(
        &presentation(),
        "ppt/notesSlides/notesSlide1.xml",
        format!(
            r#"<p:notes xmlns:p="{P}" xmlns:a="{A}"><a:p><a:r><a:t>pass</a:t></a:r><a:r><a:t>word=synthetic-test</a:t></a:r></a:p></p:notes>"#
        ),
    );
    assert!(
        !check_office_document_text(OfficeFormat::Pptx, &source, test_credential_policy).unwrap()
    );
}

#[test]
fn editing_private_policy_screen_keeps_safe_text_and_block_boundaries_distinct() {
    let source = word(
        r#"<w:p><w:r><w:t>password</w:t></w:r></w:p><w:p><w:r><w:t>= separate paragraph</w:t></w:r></w:p>"#,
        &[(
            "word/header1.xml",
            format!(
                r#"<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>Public heading</w:t></w:r></w:p></w:hdr>"#
            ),
        )],
    );
    assert!(
        check_office_document_text(OfficeFormat::Docx, &source, test_credential_policy).unwrap()
    );
    assert!(check_office_document_text(
        OfficeFormat::Docx,
        b"not a document",
        test_credential_policy
    )
    .is_err());
}

#[test]
fn editing_private_policy_screen_includes_comments_attributes_and_candidate_bytes() {
    let comment = word(
        r#"<w:p><w:r><w:t>visible<!--password=synthetic-test-->text</w:t></w:r></w:p>"#,
        &[],
    );
    assert!(
        !check_office_document_text(OfficeFormat::Docx, &comment, test_credential_policy).unwrap()
    );
    let attribute = word(
        r#"<w:p><w:r><w:t custom="password=synthetic-test">visible</w:t></w:r></w:p>"#,
        &[],
    );
    assert!(
        !check_office_document_text(OfficeFormat::Docx, &attribute, test_credential_policy)
            .unwrap()
    );

    let header = format!(
        r#"<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>password=synthetic-test</w:t></w:r></w:p></w:hdr>"#
    );
    let source = word(
        r#"<w:p><w:r><w:t>Public</w:t></w:r></w:p>"#,
        &[("word/header1.xml", header)],
    );
    let candidate = prepare_office_edit(OfficeFormat::Docx, &source, &spec(json!([{"type":"word_replace_text","target_id":"word:0","start":0,"end":6,"expected_text":"Public","new_text":"Changed"}]))).unwrap();
    assert!(!check_office_document_text(
        OfficeFormat::Docx,
        candidate.bytes(),
        test_credential_policy
    )
    .unwrap());
    assert_eq!(
        parts(&source)["word/header1.xml"],
        parts(candidate.bytes())["word/header1.xml"]
    );
}
