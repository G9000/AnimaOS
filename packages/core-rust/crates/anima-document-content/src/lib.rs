//! Portable bounded DOCX main-body and PDF text-layer extraction.
//! No filesystem, process, network, OCR or password APIs are exposed here.
//! PDF dependencies materialize internal objects: input/output bounds and panic
//! containment do not bound their memory or wall time. Production callers must
//! execute extraction in a physically limited, owned worker process.
mod docx;
mod pdf;
pub use docx::extract_docx;
pub use pdf::extract_pdf;

use anima_core::document_content::{DocumentFormat, ExtractedDocumentText};

pub fn extract_document(format: DocumentFormat, bytes: &[u8]) -> ExtractedDocumentText {
    match format {
        DocumentFormat::Docx => extract_docx(bytes),
        DocumentFormat::Pdf => extract_pdf(bytes),
    }
}
