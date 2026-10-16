//! Portable bounded DOCX main-body, PDF text-layer and XLSX stored-cell extraction.
//! No filesystem, process, network, OCR or password APIs are exposed here.
//! PDF dependencies materialize internal objects: input/output bounds and panic
//! containment do not bound their memory or wall time. Production callers must
//! execute extraction in a physically limited, owned worker process.
mod docx;
mod pdf;
mod xlsx;
pub use docx::extract_docx;
pub use pdf::extract_pdf;
pub use xlsx::extract_xlsx;

use anima_core::document_content::{DocumentFormat, ExtractedDocumentText};

pub fn extract_document(format: DocumentFormat, bytes: &[u8]) -> ExtractedDocumentText {
    match format {
        DocumentFormat::Docx => extract_docx(bytes),
        DocumentFormat::Pdf => extract_pdf(bytes),
        DocumentFormat::Xlsx => extract_xlsx(bytes),
    }
}
