//! Portable bounded DOCX main-body extraction; no filesystem or process API.
mod docx;
pub use docx::extract_docx;
