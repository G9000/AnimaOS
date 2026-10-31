//! Pure-domain tests. No engine, converter, provider, or native renderer is run.
use std::io::{Read, Write};

use anima_document_content::OfficeFormat;
use anima_document_visual::{
    inspection::{
        admit_office, build_report, ArtifactInspectionReport, CalculationEvidence, FormulaResult,
        FormulaValue, InspectionLimitation, RenderCoverage, RendererIdentity,
    },
    PdfPageEvidence, RecognizedText,
};
use rawzip::{CompressionMethod, ZipArchiveWriter};
use serde_json::json;
use sha2::{Digest, Sha256};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

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

fn types(overrides: &[(&str, &str)]) -> String {
    format!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>{}</Types>"#,
        overrides.iter().map(|(path, kind)| format!(r#"<Override PartName="/{path}" ContentType="application/vnd.openxmlformats-officedocument.{kind}+xml"/>"#)).collect::<String>()
    )
}

fn word(extra_relationship: &str) -> Vec<u8> {
    package(&[
        (
            "[Content_Types].xml",
            types(&[("word/document.xml", "wordprocessingml.document.main")]),
        ),
        (
            "_rels/.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="main" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>"#
            ),
        ),
        (
            "word/document.xml",
            format!(
                r#"<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body><w:p><w:r><w:t>Independent imported bytes</w:t></w:r></w:p></w:body></w:document>"#
            ),
        ),
        (
            "word/_rels/document.xml.rels",
            format!(r#"<Relationships xmlns="{REL}">{extra_relationship}</Relationships>"#),
        ),
    ])
}

fn workbook(formula: &str) -> Vec<u8> {
    package(&[
        (
            "[Content_Types].xml",
            types(&[
                ("xl/workbook.xml", "spreadsheetml.sheet.main"),
                ("xl/worksheets/empty.xml", "spreadsheetml.worksheet"),
                ("xl/worksheets/hidden.xml", "spreadsheetml.worksheet"),
            ]),
        ),
        (
            "_rels/.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="main" Type="{R}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        // The first empty sheet has no cell inventory. IDs deliberately disagree
        // with ordinal order, and the hidden formula is outside any printed view.
        (
            "xl/workbook.xml",
            format!(
                r#"<workbook xmlns="{S}" xmlns:x="{R}"><sheets><sheet name="Empty" sheetId="9" x:id="z9"/><sheet name="Hidden" sheetId="2" state="hidden" x:id="a2"/></sheets><calcPr calcMode="manual"/></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            format!(
                r#"<Relationships xmlns="{REL}"><Relationship Id="a2" Type="{R}/worksheet" Target="worksheets/hidden.xml"/><Relationship Id="z9" Type="{R}/worksheet" Target="worksheets/empty.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/worksheets/empty.xml",
            format!(r#"<worksheet xmlns="{S}"><sheetData/></worksheet>"#),
        ),
        (
            "xl/worksheets/hidden.xml",
            format!(
                r#"<worksheet xmlns="{S}"><sheetData><row r="40" hidden="1"><c r="C40"><f>{formula}</f><v>999</v></c></row></sheetData></worksheet>"#
            ),
        ),
    ])
}

fn renderer() -> RendererIdentity {
    RendererIdentity {
        backend: "libreoffice_uno".into(),
        version: "pinned-test-version".into(),
        executable_sha256: digest(b"engine"),
        configuration_sha256: digest(b"controller and flags"),
        font_manifest_sha256: digest(b"font files"),
        rasterizer: "windows_pdf:test-version".into(),
    }
}

fn page(ordinal: u32, rgba: &[u8]) -> PdfPageEvidence {
    let width = (rgba.len() / 4) as u32;
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgba)
            .unwrap();
    }
    PdfPageEvidence {
        page: ordinal,
        width,
        height: 1,
        png,
        recognized: RecognizedText::Unavailable,
    }
}

fn pdf(count: u32) -> Vec<u8> {
    let mut objects = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()];
    let kids = (0..count)
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!("<< /Type /Pages /Count {count} /Kids [{kids}] >>").into_bytes());
    for i in 0..count {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents {} 0 R >>", 4 + i * 2).into_bytes());
        objects.push(b"<< /Length 0 >>\nstream\n\nendstream".to_vec());
    }
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    bytes
}

#[test]
fn inspection_admits_actual_imported_bytes_and_maps_hidden_source_formulas() {
    let source = workbook("1/0");
    let admission = admit_office(OfficeFormat::Xlsx, &source).unwrap();
    assert_eq!(admission.source_sha256(), digest(&source));
    assert_eq!(admission.source_byte_length(), source.len() as u64);
    assert_eq!(admission.formulas().len(), 1);
    let identity = &admission.formulas()[0];
    assert_eq!(identity.sheet_id, "sheet:a2");
    assert_eq!(identity.sheet_ordinal, 1);
    assert_eq!(identity.address, "C40");
    assert_eq!(identity.formula_sha256, digest(b"1/0"));
    assert_ne!(
        admission.source_sha256(),
        admission.formula_inventory_sha256()
    );
}

#[test]
fn inspection_closed_subset_includes_the_existing_basic_office_outputs() {
    for (format, value) in [
        (
            OfficeFormat::Docx,
            json!({"format":"docx","title":"Bytes","blocks":[{"type":"paragraph","text":"A rendered paragraph"}]}),
        ),
        (
            OfficeFormat::Xlsx,
            json!({"format":"xlsx","title":"Bytes","sheets":[{"name":"Data","columns":["Value"],"rows":[[1]]}]}),
        ),
        (
            OfficeFormat::Pptx,
            json!({"format":"pptx","title":"Bytes","slides":[{"title":"Actual slide","blocks":[{"type":"paragraph","text":"A rendered slide"}]}]}),
        ),
    ] {
        let spec = serde_json::from_value(value).unwrap();
        let source = anima_document_content::render_document(&spec).unwrap();
        // Generation metadata is neither inspected nor used as provenance.
        let admission = admit_office(format, &source.bytes)
            .unwrap_or_else(|error| panic!("generated {format:?} source rejected: {error:?}"));
        assert_eq!(admission.source_sha256(), digest(&source.bytes));
    }
}

#[test]
fn inspection_fill_pattern_admission_keeps_values_and_element_scope_closed() {
    let spec = serde_json::from_value(json!({
        "format":"xlsx","title":"Fills",
        "sheets":[{"name":"Data","columns":["Value"],"rows":[[1]]}]
    }))
    .unwrap();
    let source = anima_document_content::render_document(&spec).unwrap();
    assert!(admit_office(OfficeFormat::Xlsx, &source.bytes).is_ok());
    for (before, after) in [
        ("patternType=\"solid\"", "patternType=\"unknown\""),
        (
            "<fill><patternFill patternType=\"solid\">",
            "<fill patternType=\"solid\"><patternFill>",
        ),
    ] {
        let archive = rawzip::ZipArchive::from_slice(&source.bytes).unwrap();
        let mut entries = archive.entries();
        let mut parts = Vec::new();
        let mut replaced = false;
        while let Some(entry) = entries.next_entry().unwrap() {
            let name = std::str::from_utf8(entry.file_path().as_bytes())
                .unwrap()
                .to_owned();
            let local = archive.get_entry(entry.wayfinder()).unwrap();
            assert_eq!(
                entry.compression_method(),
                rawzip::CompressionMethod::DEFLATE
            );
            let mut decoded = Vec::new();
            local
                .verifying_reader(flate2::read::DeflateDecoder::new(local.data()))
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut decoded)
                .unwrap();
            assert!(decoded.len() <= 4 * 1024 * 1024);
            let body = String::from_utf8(decoded).unwrap();
            replaced |= body.contains(before);
            parts.push((name, body.replace(before, after)));
        }
        assert!(replaced, "fixture must contain {before}");
        let source = package(
            &parts
                .iter()
                .map(|(name, body)| (name.as_str(), body.clone()))
                .collect::<Vec<_>>(),
        );
        assert!(
            anima_document_content::inspect_office_document(OfficeFormat::Xlsx, &source).is_ok()
        );
        assert!(
            admit_office(OfficeFormat::Xlsx, &source).is_err(),
            "must reject {after}"
        );
    }
}

#[test]
fn inspection_rejects_links_active_content_and_unknown_formulas_before_engine_use() {
    let hyperlink = format!(
        r#"<Relationship Id="link" Type="{R}/hyperlink" Target="https://example.org/" TargetMode="External"/>"#
    );
    assert!(
        anima_document_content::inspect_office_document(OfficeFormat::Docx, &word(&hyperlink))
            .is_ok()
    );
    assert!(admit_office(OfficeFormat::Docx, &word(&hyperlink)).is_err());
    let active = format!(r#"<Relationship Id="active" Type="{R}/oleObject" Target="object.bin"/>"#);
    assert!(admit_office(OfficeFormat::Docx, &word(&active)).is_err());
    assert!(admit_office(OfficeFormat::Xlsx, &workbook("WEBSERVICE(1)")).is_err());
    assert!(admit_office(OfficeFormat::Html, b"<p>inert html</p>").is_err());
    assert!(admit_office(OfficeFormat::Docx, b"not a package").is_err());
}

#[test]
fn inspection_rejects_unknown_elements_and_attributes_in_known_office_namespaces() {
    let source = word("");
    let replace = |before: &str, after: &str| {
        let archive = rawzip::ZipArchive::from_slice(&source).unwrap();
        let mut entries = archive.entries();
        let mut parts = Vec::new();
        while let Some(entry) = entries.next_entry().unwrap() {
            let name = std::str::from_utf8(entry.file_path().as_bytes())
                .unwrap()
                .to_owned();
            let local = archive.get_entry(entry.wayfinder()).unwrap();
            let body = String::from_utf8(local.data().to_vec())
                .unwrap()
                .replace(before, after);
            parts.push((name, body));
        }
        package(
            &parts
                .iter()
                .map(|(name, body)| (name.as_str(), body.clone()))
                .collect::<Vec<_>>(),
        )
    };
    for source in [
        replace("<w:p>", "<w:unknownBehavior/><w:p>"),
        replace("<w:p>", "<w:p w:unknownBehavior=\"enabled\">"),
    ] {
        assert!(
            anima_document_content::inspect_office_document(OfficeFormat::Docx, &source).is_ok()
        );
        assert!(admit_office(OfficeFormat::Docx, &source).is_err());
    }
}

#[test]
fn inspection_hashes_actual_pdf_png_and_immutable_report_and_rejects_mutation() {
    let source = word("");
    let admission = admit_office(OfficeFormat::Docx, &source).unwrap();
    let pdf = pdf(1);
    let page = page(1, &[255, 255, 255, 255, 0, 0, 0, 255]);
    let report = build_report(
        "captured-version",
        &admission,
        renderer(),
        &pdf,
        &[page.clone()],
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    assert_eq!(report.source().sha256, digest(&source));
    assert_eq!(report.source().source_version, "captured-version");
    assert_eq!(report.rendering().pdf_sha256, digest(&pdf));
    assert_eq!(report.rendering().pages[0].png_sha256, digest(&page.png));
    assert_eq!(report.rendering().pages[0].width, 2);
    assert!(matches!(
        report.rendering().coverage,
        RenderCoverage::CompleteExportedView
    ));
    assert_eq!(report.rendering().pages[0].ink.non_background_pixels, 1);
    assert_eq!(report.rendering().pages[0].ink.total_pixels, 2);
    report.validate().unwrap();
    assert!(report
        .limitations()
        .contains(&InspectionLimitation::FontManifestCoversDirectorySubset));
    let encoded = serde_json::to_value(&report).unwrap();
    let decoded: ArtifactInspectionReport = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded.sha256(), report.sha256());
    for pointer in [
        "/source/sha256",
        "/source/source_version",
        "/renderer/version",
        "/renderer/executable_sha256",
        "/renderer/configuration_sha256",
        "/renderer/font_manifest_sha256",
        "/rendering/pdf_sha256",
        "/rendering/pages/0/png_sha256",
    ] {
        let mut forged = encoded.clone();
        *forged.pointer_mut(pointer).unwrap() = json!("a".repeat(64));
        assert!(
            serde_json::from_value::<ArtifactInspectionReport>(forged).is_err(),
            "{pointer}"
        );
    }
    let mut unknown = encoded;
    unknown["pass"] = json!(true);
    assert!(serde_json::from_value::<ArtifactInspectionReport>(unknown).is_err());
}

#[test]
fn inspection_two_of_three_and_ten_page_cap_remain_partial() {
    let admission = admit_office(OfficeFormat::Docx, &word("")).unwrap();
    let pages = [page(1, &[255; 4]), page(2, &[255; 4])];
    let report = build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(3),
        &pages,
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    match &report.rendering().coverage {
        RenderCoverage::Partial { missing_pages, .. } => assert_eq!(missing_pages, &[3]),
        _ => panic!("two pages cannot cover a three-page export"),
    }
    let ten = (1..=10)
        .map(|ordinal| page(ordinal, &[255; 4]))
        .collect::<Vec<_>>();
    let report = build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(11),
        &ten,
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    assert!(matches!(
        report.rendering().coverage,
        RenderCoverage::Partial { .. }
    ));
    let eleven = (1..=11)
        .map(|ordinal| page(ordinal, &[255; 4]))
        .collect::<Vec<_>>();
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(11),
        &eleven,
        CalculationEvidence::NotApplicable
    )
    .is_err());
}

#[test]
fn inspection_formula_results_require_all_original_identities_and_keep_errors_separate() {
    let admission = admit_office(OfficeFormat::Xlsx, &workbook("1/0")).unwrap();
    let pages = [page(1, &[255; 4])];
    let result = FormulaResult {
        identity: admission.formulas()[0].clone(),
        engine_formula: "=1/0".into(),
        value: FormulaValue::Error { code: 532 },
    };
    let report = build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &pages,
        CalculationEvidence::Recalculated {
            results: vec![result.clone()],
        },
    )
    .unwrap();
    assert_eq!(report.calculation().total_formulas, 1);
    assert_eq!(report.calculation().inspected_formulas, 1);
    assert_eq!(report.calculation().errors.len(), 1);
    assert_eq!(report.calculation().errors[0].identity.sheet_ordinal, 1);
    assert_eq!(report.calculation().errors[0].code, 532);
    assert!(report.calculation().result_sha256.is_some());
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &pages,
        CalculationEvidence::Recalculated { results: vec![] }
    )
    .is_err());
    let mut wrong = result.clone();
    wrong.identity.sheet_ordinal = 0;
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &pages,
        CalculationEvidence::Recalculated {
            results: vec![wrong]
        }
    )
    .is_err());
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &pages,
        CalculationEvidence::NoFormulas
    )
    .is_err());
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &pages,
        CalculationEvidence::NotApplicable
    )
    .is_err());
    let mut nonfinite = result;
    nonfinite.value = FormulaValue::Number { value: f64::NAN };
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &pages,
        CalculationEvidence::Recalculated {
            results: vec![nonfinite]
        }
    )
    .is_err());
}

#[test]
fn inspection_pixel_measurements_are_deterministic_and_alpha_composites_over_white() {
    let admission = admit_office(OfficeFormat::Docx, &word("")).unwrap();
    let page = page(1, &[0, 0, 0, 0, 0, 0, 0, 255, 250, 250, 250, 255]);
    let first = build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &[page.clone()],
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    let second = build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &[page],
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    assert_eq!(first.sha256(), second.sha256());
    assert_eq!(first.rendering().pages[0].ink.non_background_pixels, 1);
    assert_eq!(first.rendering().pages[0].ink.threshold, 245);
    assert_eq!(first.rendering().pages[0].ink.measurement_version, 1);
    assert!(!serde_json::to_string(&first).unwrap().contains("layout_ok"));
}

#[test]
fn inspection_rejects_duplicate_pages_bad_dimensions_and_malformed_pdf() {
    let admission = admit_office(OfficeFormat::Docx, &word("")).unwrap();
    let page = page(1, &[255; 4]);
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(2),
        &[page.clone(), page.clone()],
        CalculationEvidence::NotApplicable
    )
    .is_err());
    let mut wrong = page.clone();
    wrong.width = 2;
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        &pdf(1),
        &[wrong],
        CalculationEvidence::NotApplicable
    )
    .is_err());
    assert!(build_report(
        "v1",
        &admission,
        renderer(),
        b"%PDF-malformed",
        &[page],
        CalculationEvidence::NotApplicable
    )
    .is_err());
}
