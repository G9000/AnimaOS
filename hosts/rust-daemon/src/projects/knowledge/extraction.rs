//! Full bounded document extraction, split into independently searchable passages.

use anima_document_store_sqlite::ChunkInput;
use anima_service_adapters::document_content::{
    validate_knowledge_document_extraction, DocumentFormat, DocumentLocator, ExtractedDocumentText,
};

pub(super) fn text_chunks(text: &str, locator: &str) -> Result<Vec<ChunkInput>, String> {
    if text.len() > 1024 * 1024 || unsafe_text(text) {
        return Err("Knowledge document text is sensitive, unsupported or too large".into());
    }
    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut line = 1usize;
    while offset < text.len() {
        let mut end = (offset + 4096).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end < text.len() {
            if let Some((boundary, character)) = text[offset..end]
                .char_indices()
                .rev()
                .find(|(boundary, character)| *boundary >= 2048 && character.is_whitespace())
            {
                end = offset + boundary + character.len_utf8();
            }
        }
        let passage = &text[offset..end];
        let end_line = line + passage.bytes().filter(|byte| *byte == b'\n').count();
        if !passage.trim().is_empty() {
            chunks.push(ChunkInput {
                locator: if locator.starts_with("text#") {
                    format!("text#lines={line}-{end_line};byte={offset}")
                } else {
                    format!("{locator};byte={offset}")
                },
                text: passage.to_owned(),
            });
        }
        line = end_line;
        offset = end;
    }
    Ok(chunks)
}

pub(super) fn document_format(path: &str) -> Option<DocumentFormat> {
    match path.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "docx" => Some(DocumentFormat::Docx),
        "pdf" => Some(DocumentFormat::Pdf),
        "xlsx" => Some(DocumentFormat::Xlsx),
        _ => None,
    }
}

pub(super) fn extracted_chunks(value: &ExtractedDocumentText) -> Result<Vec<ChunkInput>, String> {
    validate_knowledge_document_extraction(value).map_err(|_| "Knowledge extraction is invalid")?;
    let mut chunks = Vec::new();
    for block in &value.blocks {
        let locator = match block.locator {
            DocumentLocator::DocxParagraph { paragraph } => format!("docx#paragraph={paragraph}"),
            DocumentLocator::PdfPage { page } => format!("pdf#page={page}"),
            DocumentLocator::XlsxCell { sheet, row, column } => {
                format!("xlsx#sheet={sheet};row={row};column={column}")
            }
        };
        chunks.extend(text_chunks(&block.text, &locator)?);
    }
    if chunks.len() > anima_document_store_sqlite::MAX_DOCUMENT_CHUNKS {
        return Err("Knowledge extraction contains too many passages".into());
    }
    Ok(chunks)
}

pub(super) fn unsafe_text(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        || crate::tools::generated_documents::credential_fields(text)
        || crate::foreign_sessions::redact_secrets(text) != text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_late_passages_and_utf8_boundaries() {
        let text = format!("{}\nlate marker cobalt harbour", "🦀".repeat(16_384));
        let chunks = text_chunks(&text, "text#line=1").unwrap();
        assert!(chunks.len() > 16);
        assert!(chunks.iter().all(|chunk| chunk.text.len() <= 4096));
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(chunks.last().unwrap().text.contains("cobalt harbour"));
    }

    #[test]
    fn refuses_sensitive_text_and_oversized_extraction() {
        for text in [
            "password=hunter2",
            "-----BEGIN PRIVATE KEY-----",
            "Authorization: Bearer example",
        ] {
            assert!(text_chunks(text, "text#line=1").is_err());
        }
        assert!(text_chunks(&"a".repeat(1024 * 1024 + 1), "text#line=1").is_err());
    }

    #[test]
    fn locates_late_text_lines_and_preserves_words_at_chunk_boundaries() {
        let text = format!("{}latepassage on final line", "ordinary row\n".repeat(5000));
        let chunks = text_chunks(&text, "text#line=1").unwrap();
        let last = chunks.last().unwrap();
        assert!(last.locator.contains("-5001;"), "{}", last.locator);
        assert!(!last.locator.starts_with("text#lines=1-"));
        for adjacent in chunks.windows(2) {
            assert!(adjacent[0].text.ends_with(char::is_whitespace));
            assert!(!adjacent[1].text.starts_with("dinary"));
        }
    }
}
