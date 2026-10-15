//! Selected-message document admission; parsing stays behind the owned worker.
pub(crate) mod admission;
pub(crate) mod worker;

pub(crate) const DOCX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub(crate) const PDF_MIME: &str = "application/pdf";
pub(crate) const MAX_DOCUMENT_INPUT_BYTES: u64 = 2 * 1024 * 1024;
#[allow(dead_code)]
pub(crate) const MAX_DOCX_INPUT_BYTES: u64 = MAX_DOCUMENT_INPUT_BYTES;

#[allow(dead_code)]
pub(crate) fn is_docx(record: &super::AttachmentRecord) -> bool {
    document_format(record) == Some(anima_core::document_content::DocumentFormat::Docx)
}

pub(crate) fn document_format(
    record: &super::AttachmentRecord,
) -> Option<anima_core::document_content::DocumentFormat> {
    use anima_core::document_content::DocumentFormat;
    if record.kind != super::AttachmentKind::Document {
        return None;
    }
    let (_, extension) = record.name.rsplit_once('.')?;
    match record.mime.as_str() {
        DOCX_MIME if extension.eq_ignore_ascii_case("docx") => Some(DocumentFormat::Docx),
        PDF_MIME if extension.eq_ignore_ascii_case("pdf") => Some(DocumentFormat::Pdf),
        _ => None,
    }
}
