use std::io::Write;

use anima_document_content::extract_docx;
use anima_service_adapters::document_content::{
    render_document_snapshots, validate_document_extraction, DocumentExtractionStatus as Status,
    DocumentLocator, DocumentOmission as Omission, DocumentSnapshot,
};
use rawzip::{CompressionMethod, ZipArchive, ZipArchiveWriter};

const TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
const NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[test]
fn cfb_magic_alone_does_not_prove_encryption() {
    let result = extract_docx(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    assert_eq!(result.status, Status::Unsupported);
    assert!(result.blocks.is_empty());
}

fn xml(body: &str) -> String {
    format!("<w:document xmlns:w=\"{NS}\"><w:body>{body}</w:body></w:document>")
}

fn package(parts: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    let mut output = Vec::new();
    let mut archive = ZipArchiveWriter::new(&mut output);
    for (name, bytes) in parts {
        let method = if deflate {
            CompressionMethod::DEFLATE
        } else {
            CompressionMethod::STORE
        };
        let (mut entry, config) = archive
            .new_file(*name)
            .compression_method(method)
            .start()
            .unwrap();
        if deflate {
            let encoder =
                flate2::write::DeflateEncoder::new(&mut entry, flate2::Compression::default());
            let mut writer = config.wrap(encoder);
            writer.write_all(bytes).unwrap();
            let (encoder, descriptor) = writer.finish().unwrap();
            encoder.finish().unwrap();
            entry.finish(descriptor).unwrap();
        } else {
            let mut writer = config.wrap(&mut entry);
            writer.write_all(bytes).unwrap();
            let (_, descriptor) = writer.finish().unwrap();
            entry.finish(descriptor).unwrap();
        }
    }
    archive.finish().unwrap();
    output
}

fn doc(body: &str, deflate: bool) -> Vec<u8> {
    package(
        &[
            ("[Content_Types].xml", TYPES.as_bytes()),
            ("word/document.xml", xml(body).as_bytes()),
        ],
        deflate,
    )
}

/// The conformant ZIP writer sanitizes paths. Preserve deliberately hostile
/// fixture names by replacing equal-length local/central names only after the
/// library has identified and validated their records.
fn replace_raw_entry_name(bytes: &mut [u8], original: &str, replacement: &str) {
    assert_eq!(original.len(), replacement.len());
    let (central_name, local_name) = {
        let archive = ZipArchive::from_slice(&*bytes).unwrap();
        let mut entries = archive.entries();
        let mut found = None;
        while let Some(entry) = entries.next_entry().unwrap() {
            if entry.file_path().as_bytes() == original.as_bytes() {
                let local = archive.get_entry(entry.wayfinder()).unwrap();
                assert_eq!(
                    local.local_header().file_path().as_bytes(),
                    original.as_bytes()
                );
                found = Some((
                    entry.central_directory_offset() as usize + 46,
                    entry.local_header_offset() as usize + 30,
                ));
                break;
            }
        }
        found.expect("safe placeholder fixture entry exists")
    };
    for start in [central_name, local_name] {
        let target = bytes.get_mut(start..start + original.len()).unwrap();
        assert_eq!(target, original.as_bytes());
        target.copy_from_slice(replacement.as_bytes());
    }
    let archive = ZipArchive::from_slice(&*bytes).unwrap();
    let mut entries = archive.entries();
    assert!(
        (0..3).any(|_| entries
            .next_entry()
            .unwrap()
            .is_some_and(|entry| entry.file_path().as_bytes() == replacement.as_bytes())),
        "fixture must retain literal raw hostile name {replacement:?}"
    );
}

#[test]
fn stored_and_deflate_keep_literal_body_table_order_tabs_breaks_and_ruby_base() {
    let body = r#"<w:p><w:r><w:t>Anchor violet 581.</w:t><w:tab/><w:t>tab</w:t><w:br/><w:t>line</w:t><w:cr/><w:t>return &amp; &#x1F980;</w:t></w:r><w:ruby><w:rt><w:r><w:t>GUIDE_MUST_BE_ABSENT</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>base</w:t></w:r></w:rubyBase></w:ruby></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell two.</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    for deflate in [false, true] {
        let bytes = doc(body, deflate);
        let result = extract_docx(&bytes);
        assert_eq!(result.status, Status::Ready);
        assert_eq!(result.blocks.len(), 2);
        assert_eq!(
            result.blocks[0].text,
            "Anchor violet 581.\ttab\nline\nreturn & 🦀base"
        );
        assert_eq!(
            result.blocks[0].locator,
            DocumentLocator::DocxParagraph { paragraph: 1 }
        );
        assert_eq!(result.blocks[1].text, "Cell two.");
        assert_eq!(
            result.blocks[1].locator,
            DocumentLocator::DocxParagraph { paragraph: 2 }
        );
        assert_eq!(result.omissions, [Omission::MainBodyOnly]);
        validate_document_extraction(&result).unwrap();
    }
}

#[test]
fn root_selected_fixture_retains_exact_literal_and_excludes_guide_and_header() {
    let bytes = include_bytes!(
        "../../../../../hosts/rust-daemon/tests/fixtures/documents/m54-selected.docx"
    );
    let result = extract_docx(bytes);
    assert_eq!(result.status, Status::Ready);
    let text = result
        .blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("M54 selected record: cobalt harbour 7319."));
    assert!(!text.contains("PHONETIC_GUIDE_NOT_BODY"));
    assert!(!text.contains("HEADER_OMITTED_4096"));
    assert!(!text.contains("NEVER_INLINE_2047"));
}

#[test]
fn excludes_fields_deleted_revisions_drawings_textboxes_and_foreign_namespaces() {
    let body = r#"<w:p><w:r><w:t>visible </w:t></w:r><w:del><w:r><w:t>DELETED</w:t></w:r></w:del><w:fldSimple w:instr="INCLUDETEXT"><w:r><w:t>SIMPLE_FIELD</w:t></w:r></w:fldSimple><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>TOOL_INSTRUCTION</w:instrText><w:fldChar w:fldCharType="separate"/><w:t>FIELD_RESULT</w:t><w:fldChar w:fldCharType="end"/><w:t>tail</w:t></w:r><w:drawing><w:txbxContent><w:p><w:r><w:t>DRAWING_TEXT</w:t></w:r></w:p></w:txbxContent></w:drawing><x:other xmlns:x="urn:foreign"><w:r><w:t>FOREIGN</w:t></w:r></x:other></w:p>"#;
    let result = extract_docx(&doc(body, false));
    assert_eq!(result.status, Status::Ready);
    assert_eq!(result.blocks[0].text, "visible tail");
}

#[test]
fn rejects_corrupt_crc_and_truncated_xml_without_returning_a_valid_prefix() {
    let mut bytes = doc("<w:p><w:r><w:t>CRC_LITERAL</w:t></w:r></w:p>", false);
    let offset = bytes
        .windows(b"CRC_LITERAL".len())
        .position(|w| w == b"CRC_LITERAL")
        .unwrap();
    bytes[offset] ^= 1;
    let result = extract_docx(&bytes);
    assert_eq!(result.status, Status::Malformed);
    assert!(result.blocks.is_empty());
    let broken = xml("<w:p><w:r><w:t>good prefix</w:t></w:r></w:p>").replace("</w:document>", "");
    let bytes = package(
        &[
            ("[Content_Types].xml", TYPES.as_bytes()),
            ("word/document.xml", broken.as_bytes()),
        ],
        false,
    );
    assert_eq!(extract_docx(&bytes).status, Status::Malformed);
}

#[test]
fn rejects_ambiguous_paths_duplicates_macros_and_unsupported_namespace() {
    let main = xml("<w:p><w:r><w:t>visible</w:t></w:r></w:p>");
    for extra in [
        "../escape",
        "word\\document.xml",
        "word/./document.xml",
        "WORD/DOCUMENT.XML",
    ] {
        let placeholder = "x".repeat(extra.len());
        let mut bytes = package(
            &[
                ("[Content_Types].xml", TYPES.as_bytes()),
                ("word/document.xml", main.as_bytes()),
                (&placeholder, b"x"),
            ],
            false,
        );
        replace_raw_entry_name(&mut bytes, &placeholder, extra);
        let result = extract_docx(&bytes);
        assert_eq!(result.status, Status::Malformed, "raw path case {extra:?}");
        assert!(result.blocks.is_empty(), "raw path case {extra:?}");
    }
    let macro_types = TYPES.replace(
        "wordprocessingml.document.main",
        "wordprocessingml.template.macroEnabled.main",
    );
    let bytes = package(
        &[
            ("[Content_Types].xml", macro_types.as_bytes()),
            ("word/document.xml", main.as_bytes()),
        ],
        false,
    );
    assert_eq!(extract_docx(&bytes).status, Status::Unsupported);
    let strict = main.replace(NS, "http://purl.oclc.org/ooxml/wordprocessingml/main");
    let bytes = package(
        &[
            ("[Content_Types].xml", TYPES.as_bytes()),
            ("word/document.xml", strict.as_bytes()),
        ],
        false,
    );
    assert_eq!(extract_docx(&bytes).status, Status::Unsupported);
}

#[test]
fn rejects_dtd_custom_entities_depth_and_expansion_bombs() {
    for main in [
        format!(
            "<!DOCTYPE document [<!ENTITY secret 'private'>]>{}",
            xml("<w:p><w:r><w:t>&secret;</w:t></w:r></w:p>")
        ),
        xml("<w:p><w:r><w:t>&unknown;</w:t></w:r></w:p>"),
    ] {
        let bytes = package(
            &[
                ("[Content_Types].xml", TYPES.as_bytes()),
                ("word/document.xml", main.as_bytes()),
            ],
            false,
        );
        assert_eq!(extract_docx(&bytes).status, Status::Malformed);
    }
    let deep = format!(
        "{}{}{}",
        "<w:sdt>".repeat(65),
        "<w:p><w:r><w:t>deep</w:t></w:r></w:p>",
        "</w:sdt>".repeat(65)
    );
    assert_eq!(extract_docx(&doc(&deep, false)).status, Status::TooLarge);
    let bomb = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", "x".repeat(100_000));
    let result = extract_docx(&doc(&bomb, true));
    assert_eq!(result.status, Status::TooLarge);
    assert!(result.blocks.is_empty());
}

#[test]
fn rejects_misplaced_document_events_without_returning_text() {
    let main = xml("<w:p><w:r><w:t>validated body literal</w:t></w:r></w:p>");
    let declaration = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
    for (label, malformed) in [
        ("CDATA before root", format!("<![CDATA[prefix]]>{main}")),
        ("CDATA after root", format!("{main}<![CDATA[suffix]]>")),
        ("entity before root", format!("&amp;{main}")),
        ("character reference after root", format!("{main}&#32;")),
        ("declaration after root", format!("{main}{declaration}")),
        (
            "declaration inside body",
            xml(&format!(
                "{declaration}<w:p><w:r><w:t>body</w:t></w:r></w:p>"
            )),
        ),
        (
            "duplicate declaration",
            format!("{declaration}{declaration}{main}"),
        ),
        (
            "space before declaration",
            format!(" \n{declaration}{main}"),
        ),
        (
            "comment before declaration",
            format!("<!--prefix-->{declaration}{main}"),
        ),
        (
            "reserved XML processing instruction",
            format!("{main}<?XML version='1.0'?>"),
        ),
        (
            "unknown declaration attribute",
            format!("<?xml version='1.0' unknown='x'?>{main}"),
        ),
        (
            "duplicate declaration attribute",
            format!("<?xml version='1.0' version='1.0'?>{main}"),
        ),
        (
            "reversed declaration attributes",
            format!("<?xml version='1.0' standalone='yes' encoding='UTF-8'?>{main}"),
        ),
    ] {
        let bytes = package(
            &[
                ("[Content_Types].xml", TYPES.as_bytes()),
                ("word/document.xml", malformed.as_bytes()),
            ],
            false,
        );
        let result = extract_docx(&bytes);
        assert_eq!(result.status, Status::Malformed, "{label}");
        assert!(result.blocks.is_empty(), "{label}");
    }
}

#[test]
fn content_types_requires_initial_unique_declaration_and_no_outside_root_data() {
    let main = xml("<w:p><w:r><w:t>body literal</w:t></w:r></w:p>");
    let declaration = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
    for (label, types) in [
        (
            "CDATA before types root",
            format!("<![CDATA[prefix]]>{TYPES}"),
        ),
        (
            "CDATA after types root",
            format!("{TYPES}<![CDATA[suffix]]>"),
        ),
        ("entity before types root", format!("&amp;{TYPES}")),
        (
            "character reference after types root",
            format!("{TYPES}&#32;"),
        ),
        (
            "declaration after types root",
            format!("{TYPES}{declaration}"),
        ),
        (
            "declaration inside types root",
            TYPES.replace("<Override", &format!("{declaration}<Override")),
        ),
        (
            "duplicate types declaration",
            format!("{declaration}{declaration}{TYPES}"),
        ),
        (
            "space before types declaration",
            format!(" {declaration}{TYPES}"),
        ),
        (
            "unknown types declaration attribute",
            format!("<?xml version='1.0' unknown='x'?>{TYPES}"),
        ),
    ] {
        let bytes = package(
            &[
                ("[Content_Types].xml", types.as_bytes()),
                ("word/document.xml", main.as_bytes()),
            ],
            false,
        );
        let result = extract_docx(&bytes);
        assert_eq!(result.status, Status::Malformed, "{label}");
        assert!(result.blocks.is_empty(), "{label}");
    }
    // A declaration at byte zero (after an optional UTF-8 BOM) remains valid.
    let types = format!("{declaration}{TYPES}");
    let main = format!("\u{feff}{declaration}{main}");
    let bytes = package(
        &[
            ("[Content_Types].xml", types.as_bytes()),
            ("word/document.xml", main.as_bytes()),
        ],
        false,
    );
    let result = extract_docx(&bytes);
    assert_eq!(result.status, Status::Ready);
    assert_eq!(result.blocks[0].text, "body literal");
}

#[test]
fn policy_rejects_multidisk_zip64_encryption_and_unsupported_compression() {
    let original = doc("<w:p><w:r><w:t>literal</w:t></w:r></w:p>", false);
    let archive = ZipArchive::from_slice(original.as_slice()).unwrap();
    let eocd = archive.eocd_offset() as usize;
    let first = archive.entries().next_entry().unwrap().unwrap();
    let central = first.central_directory_offset() as usize;
    let local = first.local_header_offset() as usize;
    let mut multidisk = original.clone();
    multidisk[eocd + 4] = 1;
    assert_eq!(extract_docx(&multidisk).status, Status::Unsupported);
    let mut entrydisk = original.clone();
    entrydisk[central + 34] = 1;
    assert_eq!(extract_docx(&entrydisk).status, Status::Unsupported);
    let mut zip64 = original.clone();
    zip64[eocd + 10..eocd + 12].copy_from_slice(&[255, 255]);
    assert_ne!(extract_docx(&zip64).status, Status::Ready);
    let mut encrypted = original.clone();
    encrypted[central + 8] |= 1;
    encrypted[local + 6] |= 1;
    assert_eq!(extract_docx(&encrypted).status, Status::Encrypted);
    let mut compression = original;
    compression[central + 10] = 99;
    compression[local + 8] = 99;
    assert_eq!(extract_docx(&compression).status, Status::Unsupported);
}

#[test]
fn validated_large_document_has_whole_paragraph_prefix_bounded_for_ipc_and_renderer() {
    let body = "<w:p><w:r><w:t>small literal</w:t></w:r></w:p>".repeat(2_001);
    let result = extract_docx(&doc(&body, false));
    assert_eq!(result.status, Status::Partial);
    assert!(result.omissions.contains(&Omission::TextLimit));
    assert!(result.omissions.contains(&Omission::ParagraphLimit));
    validate_document_extraction(&result).unwrap();
    assert!(result
        .blocks
        .iter()
        .all(|block| block.text == "small literal"));
    let snapshots = [DocumentSnapshot {
        attachment_id: "a".repeat(128),
        source_sha256: "a".repeat(64),
        extraction: result,
    }];
    assert!(render_document_snapshots(&snapshots).unwrap().len() <= 16 * 1024);
}

#[test]
fn empty_body_has_explicit_no_text_and_raw_input_limit_is_closed() {
    assert_eq!(extract_docx(&doc("<w:p/>", false)).status, Status::NoText);
    let result = extract_docx(&vec![b'x'; 2 * 1024 * 1024 + 1]);
    assert_eq!(result.status, Status::TooLarge);
    assert!(result.blocks.is_empty());
}

#[test]
fn knowledge_extraction_retains_late_passages_without_expanding_attachment_preview() {
    use anima_document_content::extract_knowledge_document;
    use anima_service_adapters::document_content::{
        validate_knowledge_document_extraction, DocumentFormat,
    };
    let body = format!(
        "{}<w:p><w:r><w:t>LATE_KNOWLEDGE_ANCHOR_7319</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>Repeated ordinary paragraph padding 4096.</w:t></w:r></w:p>".repeat(800)
    );
    let bytes = doc(&body, false);
    let preview = extract_docx(&bytes);
    assert!(!preview
        .blocks
        .iter()
        .any(|block| block.text.contains("LATE_KNOWLEDGE_ANCHOR_7319")));
    validate_document_extraction(&preview).unwrap();
    let knowledge = extract_knowledge_document(DocumentFormat::Docx, &bytes);
    assert!(knowledge
        .blocks
        .iter()
        .any(|block| block.text.contains("LATE_KNOWLEDGE_ANCHOR_7319")));
    assert!(
        knowledge
            .blocks
            .iter()
            .map(|block| block.text.len())
            .sum::<usize>()
            > 16 * 1024
    );
    validate_knowledge_document_extraction(&knowledge).unwrap();
    assert!(validate_document_extraction(&knowledge).is_err());
}

#[test]
fn knowledge_extraction_stops_at_one_mib_and_remains_an_honest_partial() {
    use anima_document_content::extract_knowledge_document;
    use anima_service_adapters::document_content::{
        validate_knowledge_document_extraction, DocumentFormat, MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES,
    };
    let paragraph = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", "x".repeat(1024));
    let knowledge =
        extract_knowledge_document(DocumentFormat::Docx, &doc(&paragraph.repeat(1200), false));
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
    validate_knowledge_document_extraction(&knowledge).unwrap();
}
