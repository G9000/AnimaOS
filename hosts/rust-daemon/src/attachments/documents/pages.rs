//! Page evidence is derived data, never a new attachment access grant.
use super::{
    admission::read_source_with_slots,
    document_format,
    worker::{DocumentExtractor, WorkerFailure},
};
use crate::attachments::{files::read_attachment_bytes, AttachmentRecord};
use crate::history::HistoryStore;
use anima_core::CancelSignal;
use anima_document_visual::{validate_pdf_evidence, PdfEvidence};
use std::{path::PathBuf, sync::Arc};
use tokio::{sync::Semaphore, time::Instant};

pub(crate) async fn attachment(
    store: &dyn HistoryStore,
    root: PathBuf,
    agent_id: &str,
    session_id: &str,
    record: &AttachmentRecord,
    pages: Vec<u32>,
    extractor: &dyn DocumentExtractor,
    cancel: CancelSignal,
    deadline: Instant,
) -> Result<PdfEvidence, WorkerFailure> {
    attachment_with_slots(
        store,
        root,
        agent_id,
        session_id,
        record,
        pages,
        extractor,
        cancel,
        deadline,
        super::source_read::shared_read_slots(),
    )
    .await
}

pub(crate) async fn attachment_with_slots(
    store: &dyn HistoryStore,
    root: PathBuf,
    agent_id: &str,
    session_id: &str,
    record: &AttachmentRecord,
    pages: Vec<u32>,
    extractor: &dyn DocumentExtractor,
    cancel: CancelSignal,
    deadline: Instant,
    slots: Arc<Semaphore>,
) -> Result<PdfEvidence, WorkerFailure> {
    use anima_service_adapters::document_content::DocumentFormat;
    if record.agent_id != agent_id
        || record.session_id != session_id
        || document_format(record) != Some(DocumentFormat::Pdf)
        || record.size > super::MAX_DOCUMENT_INPUT_BYTES
        || !matches!(tokio::time::timeout_at(deadline, store.get_attachment(agent_id, &record.id)).await, Ok(Ok(Some(ref current))) if current == record)
    {
        return Err(WorkerFailure::Unavailable);
    }
    let reader = Arc::new(read_attachment_bytes) as super::admission::SourceReader;
    let bytes = read_source_with_slots(
        root.clone(),
        record.clone(),
        reader.clone(),
        &cancel,
        deadline,
        slots.clone(),
    )
    .await?;
    let evidence = tokio::select! { biased;
        () = cancel.cancelled() => return Err(WorkerFailure::Cancelled),
        () = tokio::time::sleep_until(deadline) => return Err(WorkerFailure::TimedOut),
        value = extractor.inspect_pdf(bytes, pages.clone(), cancel.clone(), Some(deadline)) => value?,
    };
    if evidence.source_sha256 != record.sha256
        || evidence
            .pages
            .iter()
            .map(|page| page.page)
            .collect::<Vec<_>>()
            != pages
        || validate_pdf_evidence(&evidence).is_err()
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    read_source_with_slots(root, record.clone(), reader, &cancel, deadline, slots).await?;
    if cancel.is_cancelled() {
        return Err(WorkerFailure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(WorkerFailure::TimedOut);
    }
    if !matches!(tokio::time::timeout_at(deadline, store.get_attachment(agent_id, &record.id)).await, Ok(Ok(Some(ref current))) if current == record)
    {
        return Err(WorkerFailure::Unavailable);
    }
    if Instant::now() >= deadline {
        return Err(WorkerFailure::TimedOut);
    }
    Ok(evidence)
}

pub(crate) use anima_service_adapters::document_visuals::{
    workspace_selection, WorkspaceDocumentSelection as WorkspaceSelection,
    WORKSPACE_DOCUMENT_VISUALS_KEY as WORKSPACE_VISUALS_KEY,
};

pub(crate) async fn workspace(
    root: PathBuf,
    path: String,
    expected: Option<String>,
    pages: Vec<u32>,
    extractor: &dyn DocumentExtractor,
    cancel: CancelSignal,
    deadline: Instant,
) -> Result<PdfEvidence, WorkerFailure> {
    workspace_with_slots(
        root,
        path,
        expected,
        pages,
        extractor,
        cancel,
        deadline,
        super::source_read::shared_read_slots(),
    )
    .await
}

pub(crate) async fn workspace_with_slots(
    root: PathBuf,
    path: String,
    expected: Option<String>,
    pages: Vec<u32>,
    extractor: &dyn DocumentExtractor,
    cancel: CancelSignal,
    deadline: Instant,
    slots: Arc<Semaphore>,
) -> Result<PdfEvidence, WorkerFailure> {
    use sha2::{Digest, Sha256};
    let capture_root = root.clone();
    let capture_path = path.clone();
    let bytes = super::source_read::read_retained_with_slots(
        move || {
            crate::tools::workspace_bytes::WorkspaceBytes::open(&capture_root).and_then(
                |workspace| {
                    workspace
                        .read_file_snapshot(&capture_path, super::MAX_DOCUMENT_INPUT_BYTES as usize)
                        .map_err(|_| {
                            std::io::Error::other("workspace document snapshot unavailable")
                        })
                },
            )
        },
        &cancel,
        deadline,
        slots.clone(),
    )
    .await?
    .map_err(|_| WorkerFailure::Unavailable)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if expected
        .as_ref()
        .is_some_and(|expected| expected != &digest)
    {
        return Err(WorkerFailure::Unavailable);
    }
    let value = tokio::select! { biased;
        () = cancel.cancelled() => return Err(WorkerFailure::Cancelled),
        () = tokio::time::sleep_until(deadline) => return Err(WorkerFailure::TimedOut),
        value = extractor.inspect_pdf(bytes, pages.clone(), cancel.clone(), Some(deadline)) => value?,
    };
    if value.source_sha256 != digest
        || value.pages.iter().map(|page| page.page).collect::<Vec<_>>() != pages
        || validate_pdf_evidence(&value).is_err()
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    let current = super::source_read::read_retained_with_slots(
        move || {
            crate::tools::workspace_bytes::WorkspaceBytes::open(&root).and_then(|workspace| {
                workspace
                    .read_file_snapshot(&path, super::MAX_DOCUMENT_INPUT_BYTES as usize)
                    .map_err(|_| std::io::Error::other("workspace document snapshot unavailable"))
            })
        },
        &cancel,
        deadline,
        slots,
    )
    .await?
    .map_err(|_| WorkerFailure::Unavailable)?;
    if hex::encode(Sha256::digest(&current)) != digest {
        return Err(WorkerFailure::Unavailable);
    }
    if cancel.is_cancelled() {
        return Err(WorkerFailure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(WorkerFailure::TimedOut);
    }
    Ok(value)
}
