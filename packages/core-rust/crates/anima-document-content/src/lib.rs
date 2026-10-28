//! Portable bounded DOCX/PDF/XLSX extraction and DOCX/XLSX/PPTX/PDF creation.
//! No filesystem, process, network, OCR or password APIs are exposed here.
//! PDF dependencies materialize internal objects: input/output bounds and panic
//! containment do not bound their memory or wall time. Production callers must
//! execute extraction in a physically limited, owned worker process.
mod docx;
mod editing;
mod generation;
mod pdf;
mod xlsx;
pub use docx::extract_docx;
pub use editing::{
    check_office_document_text, inspect_office_document, prepare_office_edit,
    OfficeCalculationState, OfficeCellValue, OfficeChange, OfficeEditError, OfficeEditOperation,
    OfficeEditReview, OfficeEditSpec, OfficeFormat, OfficeInventory, OfficePreservation,
    OfficeRange, OfficeTarget, OfficeTargetValue, PreparedOfficeEdit, RetainedOfficeHyperlink,
};
pub use generation::{
    document_schema, inspect_generated_document, render_document, CellValue, DocumentBlock,
    DocumentSheet, DocumentSlide, DocumentSpec, GeneratedDocument, GeneratedFormat,
    GenerationError, MAX_GENERATED_DOCUMENT_BYTES, MAX_GENERATED_SOURCE_BYTES,
};
pub use pdf::extract_pdf;
pub use xlsx::extract_xlsx;

use anima_service_adapters::document_content::{
    DocumentFormat, ExtractedDocumentText, MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES,
};

pub fn extract_document(format: DocumentFormat, bytes: &[u8]) -> ExtractedDocumentText {
    match format {
        DocumentFormat::Docx => extract_docx(bytes),
        DocumentFormat::Pdf => extract_pdf(bytes),
        DocumentFormat::Xlsx => extract_xlsx(bytes),
    }
}

/// Extracts bounded indexing text inside the same physically owned document worker.
///
/// Production callers must retain the owned process limits described by this crate.
/// This function preserves every format's input, structure, and literal-data policies.
pub fn extract_knowledge_document(format: DocumentFormat, bytes: &[u8]) -> ExtractedDocumentText {
    match format {
        DocumentFormat::Docx => {
            docx::extract_docx_with_text_limit(bytes, MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES)
        }
        DocumentFormat::Pdf => {
            pdf::extract_pdf_with_text_limit(bytes, MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES)
        }
        DocumentFormat::Xlsx => {
            xlsx::extract_xlsx_with_text_limit(bytes, MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES)
        }
    }
}
