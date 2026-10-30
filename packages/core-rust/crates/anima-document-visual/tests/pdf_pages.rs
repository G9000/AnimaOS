use anima_document_visual::{
    render_pdf_pages, validate_pdf_evidence, PdfEvidence, PdfPageEvidence, PdfPageSelection,
    PdfVisualError, RecognizedText, MAX_OCR_TEXT_BYTES, MAX_PAGE_PNG_BYTES,
};

mod fixtures;

#[test]
fn selection_requires_one_based_unique_ascending_pages() {
    for pages in [
        vec![],
        vec![0],
        vec![2, 1],
        vec![1, 1],
        vec![1, 2, 3],
        vec![101],
    ] {
        assert!(PdfPageSelection::new(pages).is_err());
    }
    assert_eq!(
        PdfPageSelection::new(vec![1, 100]).unwrap().pages(),
        &[1, 100]
    );
}

#[test]
fn evidence_validates_source_binding_page_identity_and_png_dimensions() {
    let valid = evidence();
    validate_pdf_evidence(&valid).unwrap();
    for mutate in [
        |v: &mut PdfEvidence| v.version = 2,
        |v: &mut PdfEvidence| v.source_sha256 = "not-a-source-digest".into(),
        |v: &mut PdfEvidence| v.page_count = 0,
        |v: &mut PdfEvidence| v.page_count = 101,
        |v: &mut PdfEvidence| v.pages[0].page = 0,
        |v: &mut PdfEvidence| v.pages[0].page = 3,
        |v: &mut PdfEvidence| v.pages[0].width = 2,
        |v: &mut PdfEvidence| v.pages[0].height = 1601,
        |v: &mut PdfEvidence| v.pages[0].png[0] = 0,
        |v: &mut PdfEvidence| v.pages[0].png.truncate(20),
        |v: &mut PdfEvidence| v.pages[0].png.resize(MAX_PAGE_PNG_BYTES + 1, 0),
        |v: &mut PdfEvidence| v.pages.push(v.pages[0].clone()),
    ] {
        let mut invalid = valid.clone();
        mutate(&mut invalid);
        assert!(validate_pdf_evidence(&invalid).is_err());
    }
}

#[test]
fn recognized_text_is_bounded_and_not_interchangeable_with_empty_text() {
    let mut value = evidence();
    for (text, language) in [
        (" ".into(), "en-US".into()),
        ("safe\0hidden".into(), "en-US".into()),
        ("a".repeat(MAX_OCR_TEXT_BYTES + 1), "en-US".into()),
        ("recognized".into(), "".into()),
        ("recognized".into(), "en\nUS".into()),
    ] {
        value.pages[0].recognized = RecognizedText::Text { text, language };
        assert!(validate_pdf_evidence(&value).is_err());
    }
    value.pages[0].recognized = RecognizedText::Text {
        text: "Recognized table\nRevenue\t42".into(),
        language: "en-US".into(),
    };
    validate_pdf_evidence(&value).unwrap();
}

#[test]
fn debug_output_does_not_reveal_png_payloads() {
    let value = evidence();
    let debug = format!("{value:?}");
    assert!(debug.contains("redacted"));
    assert!(!debug.contains("137, 80, 78, 71"));
}

#[test]
fn evidence_serializes_with_closed_recognition_variants() {
    let value = evidence();
    let encoded = serde_json::to_value(&value).unwrap();
    assert_eq!(encoded["pages"][0]["recognized"]["kind"], "unavailable");
    let decoded: PdfEvidence = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, value);
    for forged in [
        serde_json::json!({"kind": "unavailable", "text": "invented OCR result"}),
        serde_json::json!({"kind": "empty", "language": "invented language"}),
        serde_json::json!({"kind": "text", "text": "source", "language": "en-US", "extra": "ignored"}),
    ] {
        assert!(serde_json::from_value::<RecognizedText>(forged).is_err());
    }
}

#[test]
fn malformed_and_oversized_sources_release_no_evidence() {
    let selection = PdfPageSelection::new(vec![1]).unwrap();
    assert_eq!(
        render_pdf_pages(b"not a PDF", &selection),
        Err(PdfVisualError::Malformed)
    );
    assert_eq!(
        render_pdf_pages(&vec![0; 2 * 1024 * 1024 + 1], &selection),
        Err(PdfVisualError::TooLarge)
    );
}

#[test]
fn encryption_is_refused_even_when_an_empty_password_could_open_it() {
    let selection = PdfPageSelection::new(vec![1]).unwrap();
    for password in ["protected", ""] {
        assert_eq!(
            render_pdf_pages(&fixtures::encrypted_scan(password), &selection),
            Err(PdfVisualError::Encrypted)
        );
    }
}

#[test]
fn raw_source_geometry_is_checked_before_platform_normalization() {
    let selection = PdfPageSelection::new(vec![1]).unwrap();
    for geometry in ["[0 0 0 100]", "[0 0 1000000000 100]", "[0 0 -100 100]"] {
        assert_eq!(
            render_pdf_pages(&fixtures::blank_page(geometry, 0), &selection),
            Err(PdfVisualError::Malformed)
        );
    }
}

#[cfg(not(windows))]
#[test]
fn valid_pdf_fails_closed_without_a_platform_renderer() {
    let selection = PdfPageSelection::new(vec![1]).unwrap();
    assert_eq!(
        render_pdf_pages(&fixtures::scan_and_chart(), &selection),
        Err(PdfVisualError::Unsupported)
    );
}

#[cfg(windows)]
#[test]
fn real_renderer_reads_image_only_scan_and_preserves_requested_chart_page() {
    let bytes = fixtures::scan_and_chart();
    let text = anima_document_content::extract_pdf(&bytes);
    assert!(
        text.blocks.is_empty(),
        "fixture must contain no PDF text layer"
    );
    let evidence = render_pdf_pages(&bytes, &PdfPageSelection::new(vec![1, 2]).unwrap()).unwrap();
    validate_pdf_evidence(&evidence).unwrap();
    assert_eq!(evidence.page_count, 2);
    assert_eq!(
        evidence
            .pages
            .iter()
            .map(|page| page.page)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(evidence
        .pages
        .iter()
        .all(|page| page.width.max(page.height) == 1600));
    assert!(
        evidence.pages[0].png != evidence.pages[1].png,
        "scan and chart pages must have distinct rasters"
    );
    match &evidence.pages[0].recognized {
        RecognizedText::Text { text, .. } => {
            assert!(
                text.to_ascii_uppercase().contains("SCAN"),
                "unexpected OCR text: {text:?}"
            );
            assert!(
                text.contains("7319"),
                "scan identifier was not recognized: {text:?}"
            );
            assert!(
                text.to_ascii_uppercase().contains("TOTAL"),
                "scan heading was not recognized: {text:?}"
            );
        }
        RecognizedText::Unavailable => {}
        RecognizedText::Empty => panic!("the installed recognizer found no text on the scan"),
    }
}

#[cfg(windows)]
#[test]
fn rendering_rejects_an_unavailable_page_without_returning_a_prefix() {
    let selection = PdfPageSelection::new(vec![1, 3]).unwrap();
    assert_eq!(
        render_pdf_pages(&fixtures::scan_and_chart(), &selection),
        Err(PdfVisualError::InvalidSelection)
    );
}

#[cfg(windows)]
#[test]
fn blank_rotated_page_and_extreme_geometry_have_explicit_outcomes() {
    let selection = PdfPageSelection::new(vec![1]).unwrap();
    let blank = render_pdf_pages(&fixtures::blank_page("[0 0 240 400]", 90), &selection).unwrap();
    assert!(matches!(
        blank.pages[0].recognized,
        RecognizedText::Empty | RecognizedText::Unavailable
    ));
    assert!(blank.pages[0].width > blank.pages[0].height);
    for geometry in ["[0 0 0 100]", "[0 0 1000000000 100]", "[0 0 -100 100]"] {
        assert!(render_pdf_pages(&fixtures::blank_page(geometry, 0), &selection).is_err());
    }
}

fn evidence() -> PdfEvidence {
    PdfEvidence {
        version: 1,
        source_sha256: "a".repeat(64),
        page_count: 2,
        pages: vec![PdfPageEvidence {
            page: 1,
            width: 1,
            height: 1,
            png: fixtures::one_pixel_png(),
            recognized: RecognizedText::Unavailable,
        }],
    }
}
