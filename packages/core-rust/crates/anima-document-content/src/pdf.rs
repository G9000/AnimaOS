use std::io::{self, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use anima_service_adapters::document_content::{
    fit_document_snapshots, DocumentBlock, DocumentExtractionStatus as Status, DocumentFormat,
    DocumentLocator, DocumentOmission as Omission, DocumentSnapshot, ExtractedDocumentText,
    PdfCoverage, MAX_DOCUMENT_RENDERED_BYTES, MAX_PDF_PAGES,
};
use flate2::{Decompress, FlushDecompress, Status as InflateStatus};
use pdf_extract::{Document, LoadOptions, Object, ObjectId, PlainTextOutput, Stream};

const MAX_INPUT: usize = 2 * 1024 * 1024;

/// Extract accepted PDF page text in a host-owned physically limited worker.
/// Any reported page error or panic discards the entire document's prefix.
pub fn extract_pdf(bytes: &[u8]) -> ExtractedDocumentText {
    if bytes.len() > MAX_INPUT {
        return ExtractedDocumentText::empty_for(DocumentFormat::Pdf, Status::TooLarge);
    }
    match catch_unwind(AssertUnwindSafe(|| extract(bytes))) {
        Ok(Ok(value)) => value,
        Ok(Err(status)) => ExtractedDocumentText::empty_for(DocumentFormat::Pdf, status),
        Err(_) => ExtractedDocumentText::empty_for(DocumentFormat::Pdf, Status::Malformed),
    }
}

fn extract(bytes: &[u8]) -> Result<ExtractedDocumentText, Status> {
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            strict: true,
            password: None,
            filter: None,
        },
    )
    .map_err(|error| match error {
        pdf_extract::Error::Decryption(_) | pdf_extract::Error::InvalidPassword => {
            Status::Encrypted
        }
        _ => Status::Malformed,
    })?;
    // Loading may internally try an empty password and remove Encrypt. Never
    // release that text: the original encryption marker is authoritative too.
    if document.is_encrypted() || document.was_encrypted() {
        return Err(Status::Encrypted);
    }
    let pages = document.get_pages();
    let page_count = pages.len();
    if page_count > MAX_PDF_PAGES {
        return Err(Status::TooLarge);
    }
    let mut value = ExtractedDocumentText::empty_for(DocumentFormat::Pdf, Status::NoText);
    value.omissions.push(Omission::TextLayerOnly);
    let mut text_page_count = 0;
    let mut retained_bytes = 0;
    let mut text_limited = false;
    for page in 1..=page_count as u32 {
        let page_id = *pages.get(&page).ok_or(Status::Malformed)?;
        validate_page_contents(&document, page_id)?;
        let mut sink = CappedText::new(if text_limited {
            0
        } else {
            MAX_DOCUMENT_RENDERED_BYTES - retained_bytes
        });
        {
            let writer: &mut dyn Write = &mut sink;
            let mut output = PlainTextOutput::new(writer);
            pdf_extract::output_doc_page(&document, &mut output, page)
                .map_err(|_| Status::Malformed)?;
        }
        let (text, nonblank, truncated) = sink.finish()?;
        if nonblank {
            text_page_count += 1;
        }
        text_limited |= truncated;
        if !text.trim().is_empty() {
            retained_bytes += text.len();
            value.blocks.push(DocumentBlock {
                locator: DocumentLocator::PdfPage { page },
                text,
            });
        }
        // Every accepted page is visited even after the retained byte cap.
    }
    value.pdf_coverage = Some(PdfCoverage {
        page_count: page_count as u32,
        text_page_count,
        included_page_count: value.blocks.len() as u32,
    });
    value.status = if value.blocks.is_empty() {
        if text_page_count == 0 {
            Status::NoText
        } else {
            Status::ContextOmitted
        }
    } else if text_limited {
        Status::Partial
    } else {
        Status::Ready
    };
    if text_limited {
        value.omissions.push(Omission::TextLimit);
    }
    // Account for JSON quoting and the largest admitted parent attachment ID.
    // This is an extraction text bound, not a request-specific context decision.
    let snapshot = DocumentSnapshot {
        attachment_id: "a".repeat(128),
        source_sha256: "a".repeat(64),
        extraction: value,
    };
    let mut fitted = fit_document_snapshots(&[snapshot], MAX_DOCUMENT_RENDERED_BYTES)
        .map_err(|_| Status::Malformed)?;
    let mut value = fitted.remove(0).extraction;
    if let Some(index) = value
        .omissions
        .iter()
        .position(|v| *v == Omission::ContextLimit)
    {
        value.omissions.remove(index);
        if !value.omissions.contains(&Omission::TextLimit) {
            value.omissions.push(Omission::TextLimit);
        }
    }
    Ok(value)
}

/// The dependency's convenience content reader skips missing/non-stream
/// references and falls back to raw bytes after decompression errors. Check
/// the active page content objects before that reader can hide these failures.
/// This is deliberately limited to page Contents, not whole-PDF validation.
fn validate_page_contents(document: &Document, page_id: ObjectId) -> Result<(), Status> {
    let page = document
        .get_dictionary(page_id)
        .map_err(|_| Status::Malformed)?;
    if !page.has(b"Contents") {
        return Ok(());
    }
    let contents = page.get(b"Contents").map_err(|_| Status::Malformed)?;
    if !matches!(contents, Object::Reference(_) | Object::Array(_)) {
        return Err(Status::Malformed);
    }
    let (_, contents) = document
        .dereference(contents)
        .map_err(|_| Status::Malformed)?;
    match contents {
        Object::Stream(stream) => validate_content_stream(stream),
        Object::Array(entries) => {
            for entry in entries {
                // PDF content arrays contain indirect stream references. A
                // number, nested array or dictionary must never disappear.
                if !matches!(entry, Object::Reference(_)) {
                    return Err(Status::Malformed);
                }
                let (_, content) = document.dereference(entry).map_err(|_| Status::Malformed)?;
                let stream = content.as_stream().map_err(|_| Status::Malformed)?;
                validate_content_stream(stream)?;
            }
            Ok(())
        }
        _ => Err(Status::Malformed),
    }
}

fn validate_content_stream(stream: &Stream) -> Result<(), Status> {
    if stream.dict.has(b"Filter") {
        // decompressed_content() alone accepts malformed Filter shapes by
        // returning the original bytes; filters() checks name/array-of-names.
        let filters = stream.filters().map_err(|_| Status::Malformed)?;
        // Limit active content codecs to the one whose complete, checked zlib
        // framing we independently verify. The dependency's LZW/ASCII85 and
        // filter-chain error behavior is not sufficient for this contract.
        // Empty arrays are unsupported too: the dependency drops their bytes.
        match filters.as_slice() {
            [filter] if *filter == b"FlateDecode" => validate_zlib_stream(&stream.content)?,
            [filter] if matches!(*filter, b"LZWDecode" | b"ASCII85Decode") => {
                return Err(Status::Unsupported);
            }
            [_] => return Err(Status::Malformed),
            _ => return Err(Status::Unsupported),
        }
        stream
            .decompressed_content()
            .map_err(|_| Status::Malformed)?;
    }
    Ok(())
}

fn validate_zlib_stream(input: &[u8]) -> Result<(), Status> {
    let mut decoder = Decompress::new(true);
    let mut scratch = [0u8; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let offset = usize::try_from(before_in).map_err(|_| Status::Malformed)?;
        let remaining = input.get(offset..).ok_or(Status::Malformed)?;
        let status = decoder
            .decompress(remaining, &mut scratch, FlushDecompress::None)
            .map_err(|_| Status::Malformed)?;
        if status == InflateStatus::StreamEnd {
            return if decoder.total_in() == input.len() as u64 {
                Ok(())
            } else {
                Err(Status::Malformed)
            };
        }
        if decoder.total_in() == before_in && decoder.total_out() == before_out {
            // EOF/BufError is not proof of a complete zlib/checksum trailer.
            return Err(Status::Malformed);
        }
    }
}

/// Retain a bounded UTF-8 prefix while observing every byte. Returning the full
/// input length is intentional: filling the cap must not stop later page work.
struct CappedText {
    bytes: Vec<u8>,
    cap: usize,
    truncated: bool,
    nonblank: bool,
    pending: [u8; 4],
    pending_len: usize,
    invalid: bool,
}

impl CappedText {
    fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::new(),
            cap,
            truncated: false,
            nonblank: false,
            pending: [0; 4],
            pending_len: 0,
            invalid: false,
        }
    }

    fn observe(&mut self, byte: u8) {
        if self.invalid {
            return;
        }
        if self.pending_len == 0 && byte.is_ascii() {
            self.invalid |= byte == 0;
            self.nonblank |= !(byte as char).is_whitespace();
            return;
        }
        self.pending[self.pending_len] = byte;
        self.pending_len += 1;
        match std::str::from_utf8(&self.pending[..self.pending_len]) {
            Ok(text) => {
                self.nonblank |= text.chars().any(|c| !c.is_whitespace());
                self.pending_len = 0;
            }
            Err(error) if error.error_len().is_none() && self.pending_len < 4 => {}
            Err(_) => self.invalid = true,
        }
    }

    fn finish(mut self) -> Result<(String, bool, bool), Status> {
        if self.invalid || self.pending_len != 0 {
            return Err(Status::Malformed);
        }
        if let Err(error) = std::str::from_utf8(&self.bytes) {
            if error.error_len().is_some() {
                return Err(Status::Malformed);
            }
            self.bytes.truncate(error.valid_up_to());
        }
        let text = String::from_utf8(self.bytes).map_err(|_| Status::Malformed)?;
        Ok((text, self.nonblank, self.truncated))
    }
}

impl Write for CappedText {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        for byte in input {
            self.observe(*byte);
        }
        let keep = input.len().min(self.cap - self.bytes.len());
        self.bytes.extend_from_slice(&input[..keep]);
        self.truncated |= keep != input.len();
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_keeps_utf8_prefix_and_observes_discarded_output() {
        let mut sink = CappedText::new(5);
        let text = "é🦀 trailing";
        for byte in text.as_bytes() {
            assert_eq!(sink.write(&[*byte]).unwrap(), 1);
        }
        let (prefix, nonblank, truncated) = sink.finish().unwrap();
        assert_eq!(prefix, "é");
        assert!(nonblank && truncated);
        let mut empty = CappedText::new(0);
        assert_eq!(empty.write("\u{2003}".as_bytes()).unwrap(), 3);
        assert_eq!(empty.finish().unwrap(), (String::new(), false, true));
        let mut discarded = CappedText::new(0);
        discarded.write_all(b"visible").unwrap();
        assert_eq!(discarded.finish().unwrap(), (String::new(), true, true));
    }

    #[test]
    fn invalid_or_nul_output_is_not_released() {
        for bytes in [
            &b"safe\0suffix"[..],
            &b"safe\xffsuffix"[..],
            &b"safe\xc3"[..],
        ] {
            let mut sink = CappedText::new(2);
            sink.write_all(bytes).unwrap();
            assert!(sink.finish().is_err());
        }
    }
}
