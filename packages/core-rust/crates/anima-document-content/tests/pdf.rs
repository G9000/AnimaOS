use anima_document_content::{extract_document, extract_pdf};
use anima_service_adapters::document_content::{
    render_document_snapshots, validate_document_extraction, DocumentExtractionStatus as Status,
    DocumentFormat, DocumentLocator, DocumentOmission as Omission, DocumentSnapshot, PdfCoverage,
};

const SELECTED: &[u8] =
    include_bytes!("../../../../../hosts/rust-daemon/tests/fixtures/documents/m55-selected.pdf");

/// Independent literal PDF construction, with byte offsets and uncompressed
/// streams. The parser under test and its PDF model/writer do not author it.
fn raw_pdf(pages: &[(&str, bool)]) -> Vec<u8> {
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Count {} /Kids [{kids}] >>", pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    for (index, (content, media_box)) in pages.iter().enumerate() {
        let media = if *media_box {
            "/MediaBox [0 0 612 792]"
        } else {
            ""
        };
        objects.push(format!("<< /Type /Page /Parent 2 0 R {media} /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + index * 2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    raw_objects(&objects)
}

fn raw_objects(objects: &[String]) -> Vec<u8> {
    raw_object_bytes(
        &objects
            .iter()
            .map(|object| object.as_bytes())
            .collect::<Vec<_>>(),
    )
}

fn raw_object_bytes(objects: &[&[u8]]) -> Vec<u8> {
    let mut output = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(output.len());
        output.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        output.extend_from_slice(object);
        output.extend_from_slice(b"\nendobj\n");
    }
    let xref = output.len();
    output
        .extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    output.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    output
}

fn raw_filtered_pdf(filter: &str, payload: &[u8]) -> Vec<u8> {
    let first = text_stream("first-page prefix");
    let mut filtered =
        format!("<< /Length {} /Filter {filter} >>\nstream\n", payload.len()).into_bytes();
    filtered.extend_from_slice(payload);
    filtered.extend_from_slice(b"\nendstream");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 2 /Kids [4 0 R 6 0 R] >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{first}\nendstream", first.len()).into_bytes(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 7 0 R >>".to_vec(),
        filtered,
    ];
    raw_object_bytes(&objects.iter().map(Vec::as_slice).collect::<Vec<_>>())
}

#[test]
fn flate_page_content_requires_complete_checked_zlib_m55() {
    use std::io::Write;
    let text = text_stream("valid checked Flate content");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    for filter in ["/FlateDecode", "[/FlateDecode]"] {
        let value = extract_pdf(&raw_filtered_pdf(filter, &compressed));
        assert_eq!(value.status, Status::Ready, "{filter}");
        assert_eq!(value.blocks.len(), 2);
        assert!(value.blocks[0].text.contains("first-page prefix"));
        assert!(value.blocks[1].text.contains("valid checked Flate content"));
        assert_eq!(
            value.pdf_coverage,
            Some(PdfCoverage {
                page_count: 2,
                text_page_count: 2,
                included_page_count: 2,
            })
        );
    }
    let mut checksum_bad = compressed.clone();
    *checksum_bad.last_mut().unwrap() ^= 1;
    let mut trailing = compressed.clone();
    trailing.push(0);
    for (label, bytes) in [
        (
            "missing checksum byte",
            compressed[..compressed.len() - 1].to_vec(),
        ),
        (
            "missing complete checksum",
            compressed[..compressed.len() - 4].to_vec(),
        ),
        ("checksum mismatch", checksum_bad),
        ("trailing compressed garbage", trailing),
    ] {
        let value = extract_pdf(&raw_filtered_pdf("/FlateDecode", &bytes));
        assert_eq!(value.status, Status::Malformed, "{label}");
        assert!(value.blocks.is_empty(), "{label}");
        assert!(value.pdf_coverage.is_none(), "{label}");
    }
    let large = text_stream(&"checked content beyond scratch buffer ".repeat(1024));
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(large.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    let value = extract_pdf(&raw_filtered_pdf("/FlateDecode", &compressed));
    assert_eq!(value.status, Status::Partial);
    assert_eq!(value.blocks.len(), 2);
    assert!(value.omissions.contains(&Omission::TextLimit));
    assert_eq!(
        value.pdf_coverage,
        Some(PdfCoverage {
            page_count: 2,
            text_page_count: 2,
            included_page_count: 2,
        })
    );
}

#[test]
fn unverified_page_content_codecs_and_chains_are_unsupported_m55() {
    // These cases establish the filter-name policy before payload decoding;
    // they do not assert the validity of unimplemented codec payloads.
    for (filter, payload) in [
        ("/LZWDecode", &b"unverified LZW payload"[..]),
        ("/ASCII85Decode", &b"87cURD]j7BEbo80~>"[..]),
        ("[/FlateDecode /ASCII85Decode]", &b"unverified chain"[..]),
        ("[]", &b"unverified empty filter array"[..]),
    ] {
        let value = extract_pdf(&raw_filtered_pdf(filter, payload));
        assert_eq!(value.status, Status::Unsupported, "{filter}");
        assert!(value.blocks.is_empty(), "{filter}");
        assert!(value.pdf_coverage.is_none(), "{filter}");
    }
}

#[test]
fn active_page_content_failures_discard_all_prefixes_m55() {
    let first = text_stream("valid first-page prefix must be discarded");
    let second = text_stream("second-page plaintext must not bypass its filter");
    for (label, contents, filter) in [
        ("missing contents reference", "99 0 R", ""),
        ("non-stream page-tree reference", "2 0 R", ""),
        ("non-stream font dictionary reference", "3 0 R", ""),
        ("wrong contents array element", "[7 0 R 42]", ""),
        ("missing contents array reference", "[7 0 R 99 0 R]", ""),
        ("non-stream contents array reference", "[7 0 R 3 0 R]", ""),
        (
            "unsupported filter with plaintext",
            "7 0 R",
            "/Filter /M55Unsupported",
        ),
        (
            "malformed FlateDecode with plaintext",
            "7 0 R",
            "/Filter /FlateDecode",
        ),
        ("wrong filter type", "7 0 R", "/Filter 42"),
        (
            "wrong filter array element",
            "7 0 R",
            "/Filter [/FlateDecode 42]",
        ),
    ] {
        let bytes = raw_objects(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Count 2 /Kids [4 0 R 6 0 R] >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{first}\nendstream", first.len()),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {contents} >>"),
            format!("<< /Length {} {filter} >>\nstream\n{second}\nendstream", second.len()),
        ]);
        let value = extract_pdf(&bytes);
        assert_eq!(value.status, Status::Malformed, "{label}");
        assert!(value.blocks.is_empty(), "{label}");
        assert!(value.pdf_coverage.is_none(), "{label}");
    }
    let absent = raw_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".into(),
    ]);
    let value = extract_pdf(&absent);
    assert_eq!(
        value.status,
        Status::NoText,
        "absent Contents remains legal"
    );
    assert!(value.blocks.is_empty());
    assert_eq!(
        value.pdf_coverage,
        Some(PdfCoverage {
            page_count: 1,
            text_page_count: 0,
            included_page_count: 0,
        })
    );
}

fn text_stream(text: &str) -> String {
    assert!(!text.contains(['(', ')', '\\']));
    format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET")
}

#[test]
fn independent_selected_invoice_has_numeric_page_locator_and_text_only_coverage() {
    let value = extract_document(DocumentFormat::Pdf, SELECTED);
    assert_eq!(value.version, 2);
    assert_eq!(value.format, DocumentFormat::Pdf);
    assert_eq!(value.status, Status::Ready);
    assert_eq!(value.blocks.len(), 1);
    assert_eq!(
        value.blocks[0].locator,
        DocumentLocator::PdfPage { page: 1 }
    );
    assert!(value.blocks[0]
        .text
        .contains("M55 selected invoice total: USD 137.42."));
    assert_eq!(
        value.pdf_coverage,
        Some(PdfCoverage {
            page_count: 2,
            text_page_count: 1,
            included_page_count: 1,
        })
    );
    assert_eq!(value.omissions, vec![Omission::TextLayerOnly]);
    validate_document_extraction(&value).unwrap();
    let rendered = render_document_snapshots(&[DocumentSnapshot {
        attachment_id: "selected".into(),
        source_sha256: "a".repeat(64),
        extraction: value,
    }])
    .unwrap();
    assert!(rendered.contains("locator=pdf#page=1"));
    assert!(!rendered.contains("paragraph="));
}

#[test]
fn empty_blank_and_sparse_pages_report_actual_nonblank_coverage() {
    for count in [0, 1, 100] {
        let pages = vec![("", true); count];
        let value = extract_pdf(&raw_pdf(&pages));
        assert_eq!(value.status, Status::NoText, "page count {count}");
        assert!(value.blocks.is_empty());
        assert_eq!(
            value.pdf_coverage,
            Some(PdfCoverage {
                page_count: count as u32,
                text_page_count: 0,
                included_page_count: 0,
            })
        );
        validate_document_extraction(&value).unwrap();
    }
    let text = text_stream("literal page three");
    let blank = text_stream("   ");
    let value = extract_pdf(&raw_pdf(&[("", true), (&blank, true), (&text, true)]));
    assert_eq!(value.status, Status::Ready);
    assert_eq!(
        value.blocks[0].locator,
        DocumentLocator::PdfPage { page: 3 }
    );
    assert_eq!(
        value.pdf_coverage,
        Some(PdfCoverage {
            page_count: 3,
            text_page_count: 1,
            included_page_count: 1,
        })
    );
}

#[test]
fn page_and_input_limits_are_closed_and_release_no_prefix() {
    let value = extract_pdf(&raw_pdf(&vec![("", true); 101]));
    assert_eq!(value.status, Status::TooLarge);
    assert!(value.blocks.is_empty());
    assert!(value.pdf_coverage.is_none());
    let value = extract_pdf(&vec![b'x'; 2 * 1024 * 1024 + 1]);
    assert_eq!(value.status, Status::TooLarge);
    assert!(value.blocks.is_empty());
    assert!(value.pdf_coverage.is_none());
    let value = extract_pdf(b"not a pdf");
    assert_eq!(value.status, Status::Malformed);
    assert!(value.blocks.is_empty());
    assert!(value.pdf_coverage.is_none());
}

#[test]
fn retained_cap_still_counts_later_text_pages_and_visits_later_malformed_pages() {
    let large = text_stream(&"x".repeat(30_000));
    let later = text_stream("later text must still be counted");
    let value = extract_pdf(&raw_pdf(&[(&large, true), (&later, true), ("", true)]));
    assert_eq!(value.status, Status::Partial);
    assert!(value.omissions.contains(&Omission::TextLimit));
    assert!(!value.omissions.contains(&Omission::ContextLimit));
    assert_eq!(
        value.pdf_coverage,
        Some(PdfCoverage {
            page_count: 3,
            text_page_count: 2,
            included_page_count: 1,
        })
    );
    validate_document_extraction(&value).unwrap();
    let rendered = render_document_snapshots(&[DocumentSnapshot {
        attachment_id: "a".repeat(128),
        source_sha256: "a".repeat(64),
        extraction: value,
    }])
    .unwrap();
    assert!(rendered.len() <= 16 * 1024);
    // Missing MediaBox triggers an inspected dependency panic after the first
    // page filled the retained cap; the safe prefix must still be discarded.
    let value = extract_pdf(&raw_pdf(&[(&large, true), (&later, false)]));
    assert_eq!(value.status, Status::Malformed);
    assert!(value.blocks.is_empty());
    assert!(value.pdf_coverage.is_none());
}

#[test]
fn malformed_later_page_discards_an_otherwise_usable_early_page() {
    let first = text_stream("safe first-page literal");
    let value = extract_pdf(&raw_pdf(&[(&first, true), ("", false)]));
    assert_eq!(value.status, Status::Malformed);
    assert!(value.blocks.is_empty());
    assert!(value.pdf_coverage.is_none());
    assert!(value.omissions.is_empty());
}

#[test]
fn encrypted_and_empty_password_auto_open_documents_never_release_text() {
    use pdf_extract::{
        Document, EncryptionState, EncryptionVersion, LoadOptions, Object, Permissions,
    };
    for password in ["protected", ""] {
        // This supplemental encryption wrapper uses the pinned library's public
        // writer; its unencrypted source is still the independently built PDF.
        let text = text_stream("encrypted literal must never escape");
        let mut document = Document::load_mem(&raw_pdf(&[(&text, true)])).unwrap();
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal(b"M55-independent-file-id"),
                Object::string_literal(b"M55-independent-file-id"),
            ]),
        );
        let state = EncryptionState::try_from(EncryptionVersion::V1 {
            document: &document,
            owner_password: "owner",
            user_password: password,
            permissions: Permissions::all(),
        })
        .unwrap();
        document.encrypt(&state).unwrap();
        let mut encrypted = Vec::new();
        document.save_to(&mut encrypted).unwrap();
        let loaded = Document::load_mem_with_options(
            &encrypted,
            LoadOptions {
                strict: true,
                password: None,
                filter: None,
            },
        )
        .unwrap();
        if password.is_empty() {
            assert!(
                !loaded.is_encrypted(),
                "fixture exercises automatic empty-password opening"
            );
            assert!(
                loaded.was_encrypted(),
                "original encryption remains observable"
            );
        } else {
            assert!(loaded.is_encrypted());
        }
        let value = extract_pdf(&encrypted);
        assert_eq!(value.status, Status::Encrypted);
        assert!(value.blocks.is_empty());
        assert!(value.pdf_coverage.is_none());
        assert!(value.omissions.is_empty());
        validate_document_extraction(&value).unwrap();
    }
}

#[test]
fn knowledge_pdf_retains_late_pages_beyond_the_attachment_preview() {
    use anima_document_content::extract_knowledge_document;
    use anima_service_adapters::document_content::validate_knowledge_document_extraction;
    let large = text_stream(&"ordinary padding ".repeat(1800));
    let later = text_stream("LATE_PDF_KNOWLEDGE_ANCHOR_7319");
    let bytes = raw_pdf(&[(&large, true), (&later, true)]);
    let preview = extract_pdf(&bytes);
    validate_document_extraction(&preview).unwrap();
    assert!(!preview
        .blocks
        .iter()
        .any(|block| block.text.contains("LATE_PDF_KNOWLEDGE_ANCHOR_7319")));
    let knowledge = extract_knowledge_document(DocumentFormat::Pdf, &bytes);
    validate_knowledge_document_extraction(&knowledge).unwrap();
    assert!(knowledge
        .blocks
        .iter()
        .any(|block| block.text.contains("LATE_PDF_KNOWLEDGE_ANCHOR_7319")));
    assert!(
        knowledge
            .blocks
            .iter()
            .map(|block| block.text.len())
            .sum::<usize>()
            > 16 * 1024
    );
    assert_eq!(knowledge.pdf_coverage.unwrap().included_page_count, 2);
}

#[test]
fn knowledge_pdf_caps_one_mib_while_observing_later_page_coverage() {
    use anima_document_content::extract_knowledge_document;
    use anima_service_adapters::document_content::{
        validate_knowledge_document_extraction, MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES,
    };
    let large = text_stream(&"x".repeat(MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES + 1024));
    let later = text_stream("later page must still be counted");
    let bytes = raw_pdf(&[(&large, true), (&later, true)]);
    let knowledge = extract_knowledge_document(DocumentFormat::Pdf, &bytes);
    validate_knowledge_document_extraction(&knowledge).unwrap();
    assert_eq!(knowledge.status, Status::Partial);
    assert!(knowledge.omissions.contains(&Omission::TextLimit));
    assert!(
        knowledge
            .blocks
            .iter()
            .map(|block| block.text.len())
            .sum::<usize>()
            <= MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES
    );
    assert_eq!(knowledge.pdf_coverage.unwrap().text_page_count, 2);
}
