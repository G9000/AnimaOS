//! Provenance is part of the locator, so it also participates in citation identity.

use serde::{Deserialize, Serialize};

use crate::{valid_metadata, StoreError, MAX_LOCATOR_BYTES};

const MAX_LANGUAGE_BYTES: usize = 128;
const MAX_PIPELINE_BYTES: usize = 64;

/// Distinguishes source text, recognized text, and a page without passage text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PassageOrigin {
    Literal,
    Ocr { language: String, pipeline: String },
    PageImage { pipeline: String },
}

impl<'de> Deserialize<'de> for PassageOrigin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum WireOrigin {
            Literal {},
            Ocr { language: String, pipeline: String },
            PageImage { pipeline: String },
        }

        Ok(match WireOrigin::deserialize(deserializer)? {
            WireOrigin::Literal {} => Self::Literal,
            WireOrigin::Ocr { language, pipeline } => Self::Ocr { language, pipeline },
            WireOrigin::PageImage { pipeline } => Self::PageImage { pipeline },
        })
    }
}

/// Reads provenance from a bounded locator, preserving legacy literal passages.
///
/// # Errors
/// Returns an error for ambiguous, incomplete, unknown, or oversized provenance.
pub fn passage_origin(locator: &str) -> Result<PassageOrigin, StoreError> {
    if !valid_metadata(locator, MAX_LOCATOR_BYTES) {
        return Err(invalid());
    }
    let mut fields = locator.split(';');
    let base = fields.next().ok_or_else(invalid)?;
    let has_provenance = fields.clone().any(|field| {
        let name = field.split_once('=').map_or(field, |(name, _)| name);
        matches!(name, "origin" | "language" | "pipeline")
    });
    if !has_provenance {
        return Ok(PassageOrigin::Literal);
    }
    let page = decimal(base.strip_prefix("pdf#page=").ok_or_else(invalid)?)?;
    if !(1..=100).contains(&page) {
        return Err(invalid());
    }
    let mut origin = None;
    let mut language = None;
    let mut pipeline = None;
    let mut offset = None;
    for field in fields {
        let (name, value) = field.split_once('=').ok_or_else(invalid)?;
        match name {
            "origin" => set_once(&mut origin, value)?,
            "language" => {
                set_once(&mut language, value)?;
                validate_language(value)?;
            }
            "pipeline" => {
                set_once(&mut pipeline, value)?;
                validate_pipeline(value)?;
            }
            "byte" => {
                set_once(&mut offset, value)?;
                decimal(value)?;
            }
            _ => return Err(invalid()),
        }
    }
    match (origin, language, pipeline) {
        (Some("ocr"), Some(language), Some(pipeline)) => Ok(PassageOrigin::Ocr {
            language: language.into(),
            pipeline: pipeline.into(),
        }),
        (Some("page_image"), None, Some(pipeline)) => Ok(PassageOrigin::PageImage {
            pipeline: pipeline.into(),
        }),
        _ => Err(invalid()),
    }
}

/// Builds a canonical PDF locator for recognized text and its recognizer pipeline.
///
/// # Errors
/// Returns an error for invalid page, language, pipeline, or locator bounds.
pub fn pdf_ocr_locator(page: u32, language: &str, pipeline: &str) -> Result<String, StoreError> {
    validate_page(page)?;
    validate_language(language)?;
    validate_pipeline(pipeline)?;
    bounded_locator(format!(
        "pdf#page={page};origin=ocr;language={language};pipeline={pipeline}"
    ))
}

/// Builds a canonical PDF page-image target with no fabricated passage text.
///
/// # Errors
/// Returns an error for invalid page, pipeline, or locator bounds.
pub fn pdf_page_locator(page: u32, pipeline: &str) -> Result<String, StoreError> {
    validate_page(page)?;
    validate_pipeline(pipeline)?;
    bounded_locator(format!(
        "pdf#page={page};origin=page_image;pipeline={pipeline}"
    ))
}

fn validate_page(page: u32) -> Result<(), StoreError> {
    if !(1..=100).contains(&page) {
        return Err(invalid());
    }
    Ok(())
}

fn validate_language(value: &str) -> Result<(), StoreError> {
    if value.is_empty()
        || value.len() > MAX_LANGUAGE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_pipeline(value: &str) -> Result<(), StoreError> {
    if value.is_empty()
        || value.len() > MAX_PIPELINE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid());
    }
    Ok(())
}

fn set_once<'a>(slot: &mut Option<&'a str>, value: &'a str) -> Result<(), StoreError> {
    if slot.replace(value).is_some() {
        return Err(invalid());
    }
    Ok(())
}

fn decimal(value: &str) -> Result<u64, StoreError> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}

fn bounded_locator(locator: String) -> Result<String, StoreError> {
    if !valid_metadata(&locator, MAX_LOCATOR_BYTES) {
        return Err(invalid());
    }
    Ok(locator)
}

fn invalid() -> StoreError {
    StoreError::InvalidInput("invalid passage provenance")
}

#[cfg(test)]
mod tests {
    use crate::{
        passage_origin, pdf_ocr_locator, pdf_page_locator, source_sha256, ChunkInput,
        DocumentInput, DocumentStore, PassageOrigin,
    };

    #[test]
    fn legacy_passages_keep_literal_origin() {
        for locator in [
            "word/document.xml#paragraph=2",
            "pdf#page=3;byte=4096",
            "xlsx#sheet=1;row=2;column=3;byte=0",
            "text#lines=20-30;byte=200",
            "line=1",
        ] {
            assert_eq!(passage_origin(locator).unwrap(), PassageOrigin::Literal);
        }
    }

    #[test]
    fn literal_wire_variant_rejects_unexpected_evidence_fields() {
        use serde::{
            de::value::{Error, MapDeserializer},
            Deserialize,
        };

        let valid = MapDeserializer::<_, Error>::new([("kind", "literal")].into_iter());
        assert_eq!(
            PassageOrigin::deserialize(valid).unwrap(),
            PassageOrigin::Literal
        );
        let forged = MapDeserializer::<_, Error>::new(
            [("kind", "literal"), ("text", "invented passage")].into_iter(),
        );
        assert!(PassageOrigin::deserialize(forged).is_err());
    }

    #[test]
    fn canonical_pdf_provenance_survives_chunk_offsets() {
        let ocr = pdf_ocr_locator(3, "en-US", "winrt-pdf-ocr-v1").unwrap();
        assert_eq!(
            ocr,
            "pdf#page=3;origin=ocr;language=en-US;pipeline=winrt-pdf-ocr-v1"
        );
        let expected = PassageOrigin::Ocr {
            language: "en-US".into(),
            pipeline: "winrt-pdf-ocr-v1".into(),
        };
        assert_eq!(passage_origin(&ocr).unwrap(), expected);
        assert_eq!(
            passage_origin(&format!("{ocr};byte=4096")).unwrap(),
            expected
        );
        let page = pdf_page_locator(100, "winrt-pdf-page-v1").unwrap();
        assert_eq!(
            page,
            "pdf#page=100;origin=page_image;pipeline=winrt-pdf-page-v1"
        );
        assert_eq!(
            passage_origin(&format!("{page};byte=0")).unwrap(),
            PassageOrigin::PageImage {
                pipeline: "winrt-pdf-page-v1".into(),
            }
        );
    }

    #[test]
    fn malformed_or_ambiguous_provenance_fails_closed() {
        for locator in [
            "pdf#page=0;origin=page_image;pipeline=v1",
            "pdf#page=101;origin=page_image;pipeline=v1",
            "pdf#page=+1;origin=page_image;pipeline=v1",
            "pdf#page=01;origin=page_image;pipeline=v1",
            "pdf#page=1.0;origin=page_image;pipeline=v1",
            "text#lines=1-2;origin=ocr;language=en-US;pipeline=v1",
            "pdf#page=1;origin=unknown;pipeline=v1",
            "pdf#page=1;origin=literal;pipeline=v1",
            "pdf#page=1;origin=ocr;pipeline=v1",
            "pdf#page=1;origin=ocr;language=en-US",
            "pdf#page=1;origin=page_image",
            "pdf#page=1;pipeline=v1",
            "pdf#page=1;language=en-US",
            "pdf#page=1;origin;pipeline=v1",
            "pdf#page=1;origin=page_image;pipeline=v1;origin=page_image",
            "pdf#page=1;origin=page_image;pipeline=v1;pipeline=v2",
            "pdf#page=1;origin=ocr;language=en-US;language=en-US;pipeline=v1",
            "pdf#page=1;origin=page_image;language=en-US;pipeline=v1",
            "pdf#page=1;origin=page_image;pipeline=v1;extra=ignored",
            "pdf#page=1;origin=page_image;pipeline=v1;byte=0;byte=0",
            "pdf#page=1;origin=page_image;pipeline=v1;byte=-1",
            "pdf#page=1;origin=page_image;pipeline=v1;byte=01",
            "pdf#page=1;origin=page_image;pipeline=v1;byte=18446744073709551616",
            "pdf#page=1;origin=page_image;pipeline=v1;",
        ] {
            assert!(passage_origin(locator).is_err(), "accepted {locator:?}");
        }
    }

    #[test]
    fn constructor_components_cannot_escape_the_locator_envelope() {
        for page in [0, 101, u32::MAX] {
            assert!(pdf_ocr_locator(page, "en-US", "v1").is_err());
            assert!(pdf_page_locator(page, "v1").is_err());
        }
        for language in ["", "en US", "en;origin=literal", "en=US", "中文", "en\nUS"] {
            assert!(pdf_ocr_locator(1, language, "v1").is_err());
        }
        for pipeline in ["", "v 1", "v1;origin=literal", "v=1", "管道", "v1\0"] {
            assert!(pdf_ocr_locator(1, "en-US", pipeline).is_err());
            assert!(pdf_page_locator(1, pipeline).is_err());
        }
        assert!(pdf_ocr_locator(1, &"a".repeat(129), "v1").is_err());
        assert!(pdf_page_locator(1, &"a".repeat(65)).is_err());
        assert!(passage_origin(&"x".repeat(257)).is_err());
    }

    #[test]
    fn page_only_title_search_and_reopened_citation_preserve_empty_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("knowledge.sqlite");
        let document = document("", pdf_page_locator(2, "winrt-pdf-page-v1").unwrap());
        let id = {
            let store = DocumentStore::open(&path).unwrap();
            store
                .replace_source("project", "source", &[document])
                .unwrap();
            let hits = store
                .search("project", &["source".into()], "cobalt chart", 8)
                .unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].text, "");
            assert_eq!(hits[0].excerpt, "");
            assert_eq!(hits[0].chunk_sha256, source_sha256(b""));
            hits[0].citation_id.clone()
        };
        let store = DocumentStore::open(&path).unwrap();
        let hit = store.citation("project", &id).unwrap().unwrap();
        assert_eq!(hit.text, "");
        assert_eq!(
            passage_origin(&hit.locator).unwrap(),
            PassageOrigin::PageImage {
                pipeline: "winrt-pdf-page-v1".into(),
            }
        );
    }

    #[test]
    fn empty_text_is_exclusive_to_valid_page_only_evidence() {
        let store = DocumentStore::open(":memory:").unwrap();
        for (text, locator) in [
            ("", "line=1".into()),
            (" ", "line=1".into()),
            ("", pdf_ocr_locator(1, "en-US", "v1").unwrap()),
            ("\n\t", pdf_ocr_locator(1, "en-US", "v1").unwrap()),
            (" ", pdf_page_locator(1, "v1").unwrap()),
            ("invented passage", pdf_page_locator(1, "v1").unwrap()),
            (
                "",
                "pdf#page=1;origin=page_image;pipeline=v1;extra=x".into(),
            ),
            ("recognized", "pdf#page=1;origin=ocr;pipeline=v1".into()),
        ] {
            assert!(store
                .replace_source("project", "source", &[document(text, locator)])
                .is_err());
        }
        store
            .replace_source(
                "project",
                "source",
                &[document(
                    "recognized table value",
                    pdf_ocr_locator(1, "en-US", "v1").unwrap(),
                )],
            )
            .unwrap();
    }

    #[test]
    fn changing_ocr_pipeline_changes_the_citation_identity() {
        let store = DocumentStore::open(":memory:").unwrap();
        let mut document = document(
            "Recognized cobalt revenue",
            pdf_ocr_locator(1, "en-US", "v1").unwrap(),
        );
        store
            .replace_source("project", "source", &[document.clone()])
            .unwrap();
        let first = store
            .search("project", &["source".into()], "revenue", 8)
            .unwrap()[0]
            .citation_id
            .clone();
        document.chunks[0].locator = pdf_ocr_locator(1, "en-US", "v2").unwrap();
        store
            .replace_source("project", "source", &[document])
            .unwrap();
        let second = store
            .search("project", &["source".into()], "revenue", 8)
            .unwrap()[0]
            .citation_id
            .clone();
        assert_ne!(first, second);
        assert!(store.citation("project", &first).unwrap().is_none());
        assert!(store.citation("project", &second).unwrap().is_some());
    }

    fn document(text: &str, locator: String) -> DocumentInput {
        DocumentInput {
            document_id: "report".into(),
            relative_path: "report.pdf".into(),
            title: "Cobalt chart quarterly report".into(),
            source_sha256: source_sha256(b"same original PDF bytes"),
            chunks: vec![ChunkInput {
                locator,
                text: text.into(),
            }],
        }
    }
}
