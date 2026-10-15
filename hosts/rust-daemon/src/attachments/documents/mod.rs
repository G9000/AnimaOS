//! Selected-message document admission; parsing stays behind the owned worker.
pub(crate) mod admission;
pub(crate) mod worker;

pub(crate) const DOCX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub(crate) const MAX_DOCX_INPUT_BYTES: u64 = 2 * 1024 * 1024;

pub(crate) fn is_docx(record: &super::AttachmentRecord) -> bool {
    record.kind == super::AttachmentKind::Document
        && record.mime == DOCX_MIME
        && record
            .name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("docx"))
}
