//! One-time extraction after durable run start. No disk/process await holds daemon state.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anima_core::{
    document_content::{
        document_snapshots_metadata, fit_document_snapshots, validate_document_extraction,
        DocumentExtractionStatus as Status, DocumentFormat, DocumentOmission, DocumentSnapshot,
        ExtractedDocumentText, DOCUMENT_EXTRACTS_METADATA_KEY, MAX_DOCUMENT_RENDERED_BYTES,
    },
    AgentConfig, CancelSignal, Content, TokenEstimator,
};

use super::{
    document_format,
    worker::{DocumentExtractor, WorkerFailure},
    MAX_DOCUMENT_INPUT_BYTES,
};
use crate::{
    app::SharedDaemonState,
    attachments::{
        files::read_attachment_bytes, metadata::matching_metadata, AttachmentError, AttachmentKind,
        AttachmentRecord,
    },
    runs::{RunSource, RunStatus},
    sessions::{SessionKind, SessionOrigin},
};

pub(crate) type SourceReader =
    Arc<dyn Fn(&Path, &AttachmentRecord) -> Result<Vec<u8>, AttachmentError> + Send + Sync>;

/// Four retained physical read leases, no queue. Cancellation drops only the
/// await; the bounded blocking task keeps its lease until the OS read returns.
pub(crate) async fn read_source(
    root: PathBuf,
    record: AttachmentRecord,
    reader: SourceReader,
    cancel: &CancelSignal,
    deadline: tokio::time::Instant,
) -> Result<Vec<u8>, WorkerFailure> {
    super::source_read::read_retained(move || reader(&root, &record), cancel, deadline)
        .await?
        .map_err(|_| WorkerFailure::Unavailable)
}

/// Only a newly selected owner Web message can acquire a derived snapshot.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn admit(
    state: &SharedDaemonState,
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    source: RunSource,
    config: &AgentConfig,
    input: &mut Content,
    extractor: Arc<dyn DocumentExtractor>,
    cancel: CancelSignal,
    deadline: Option<tokio::time::Instant>,
) {
    admit_with_reader(
        state,
        agent_id,
        session_id,
        run_id,
        source,
        config,
        input,
        extractor,
        cancel,
        deadline,
        Arc::new(read_attachment_bytes),
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn admit_with_reader(
    state: &SharedDaemonState,
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    source: RunSource,
    config: &AgentConfig,
    input: &mut Content,
    extractor: Arc<dyn DocumentExtractor>,
    cancel: CancelSignal,
    deadline: Option<tokio::time::Instant>,
    reader: SourceReader,
) {
    if let Some(metadata) = &mut input.metadata {
        metadata.remove(DOCUMENT_EXTRACTS_METADATA_KEY);
    }
    let (store, root, max_bytes) = {
        let guard = state.read().await;
        let Some(session) = guard.sessions.get(agent_id, session_id) else {
            return;
        };
        if source != RunSource::Web
            || session.kind != SessionKind::Chat
            || session.origin != SessionOrigin::Web
            || crate::agent_runs::is_helper_config(config)
            || !guard.runs.get(run_id).is_some_and(|run| {
                run.agent_id == agent_id
                    && run.session_id == session_id
                    && run.status == RunStatus::Running
            })
        {
            return;
        }
        let Some(root) = guard.workspace.as_ref().map(|w| w.root_path.clone()) else {
            return;
        };
        let factor = session
            .context_calibration_permille
            .map_or(1.0, |p| f64::from(p) / 1000.0);
        let estimator = TokenEstimator::new(factor.max(1.0));
        let available = crate::sessions::context::ContextBudget::for_config(config)
            .history_tokens(estimator.content_tokens(input))
            .saturating_sub(
                session
                    .summary
                    .as_ref()
                    .map_or(0, |s| estimator.text_tokens(&s.text)),
            );
        // Documents reserve one token per UTF-8 byte. Flooring leaves
        // ceiling/calibration headroom and includes every locator and quote.
        let max_bytes = ((available as f64 / estimator.calibration().max(1.0)).floor() as usize)
            .min(MAX_DOCUMENT_RENDERED_BYTES);
        (guard.history.store(), root, max_bytes)
    };
    let selected = input
        .attachments
        .iter()
        .flatten()
        .filter_map(|a| matching_metadata(input, a))
        .filter(|a| a.kind == AttachmentKind::Document)
        .take(10)
        .collect::<Vec<_>>();
    let mut snapshots = Vec::new();
    let mut document_count = 0;
    for selected in selected {
        // One absolute document deadline includes its pre-read, worker and
        // post-read; it is never renewed after parsing completes.
        let own_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        let document_deadline = deadline.map_or(own_deadline, |d| d.min(own_deadline));
        let Ok(Some(record)) = store.get_attachment(agent_id, &selected.id).await else {
            continue;
        };
        if record.agent_id != agent_id
            || record.session_id != session_id
            || record.metadata() != selected
        {
            continue;
        }
        let recognized_format = document_format(&record);
        let format = recognized_format.unwrap_or(DocumentFormat::Docx);
        // Supported selections consume the shared attempt allowance even when
        // cancellation, input size or parsing prevents a usable excerpt.
        if recognized_format.is_some() {
            document_count += 1;
        }
        let mut extraction = if cancel.is_cancelled() {
            ExtractedDocumentText::empty_for(format, Status::Cancelled)
        } else if recognized_format.is_none() {
            ExtractedDocumentText::empty_for(format, Status::Unsupported)
        } else if document_count > 2 {
            let mut omitted = ExtractedDocumentText::empty_for(format, Status::ContextOmitted);
            omitted.omissions.push(DocumentOmission::SelectionLimit);
            omitted
        } else if record.size > MAX_DOCUMENT_INPUT_BYTES {
            ExtractedDocumentText::empty_for(format, Status::TooLarge)
        } else {
            let bytes = read_source(
                root.clone(),
                record.clone(),
                Arc::clone(&reader),
                &cancel,
                document_deadline,
            )
            .await;
            match bytes {
                Ok(bytes) => {
                    let extracted = tokio::select! { biased;
                        () = cancel.cancelled() => Err(WorkerFailure::Cancelled),
                        result = tokio::time::timeout_at(document_deadline, extractor.extract(format, bytes, cancel.clone(), Some(document_deadline))) =>
                            result.unwrap_or(Err(WorkerFailure::TimedOut)),
                    };
                    match extracted {
                        Ok(value)
                            if value.format == format
                                && validate_document_extraction(&value).is_ok() =>
                        {
                            value
                        }
                        Err(WorkerFailure::Cancelled) => {
                            ExtractedDocumentText::empty_for(format, Status::Cancelled)
                        }
                        _ => ExtractedDocumentText::empty_for(format, Status::Unavailable),
                    }
                }
                _ => ExtractedDocumentText::empty_for(format, Status::Unavailable),
            }
        };
        if cancel.is_cancelled() {
            extraction = ExtractedDocumentText::empty_for(format, Status::Cancelled);
        }
        // Recheck durable ownership and source identity after the await. A
        // modified/deleted/replaced file cannot publish even a successful parse.
        let current = store.get_attachment(agent_id, &record.id).await;
        if !matches!(current, Ok(Some(ref row)) if row == &record) {
            continue;
        }
        if !extraction.blocks.is_empty() {
            if read_source(
                root.clone(),
                record.clone(),
                Arc::clone(&reader),
                &cancel,
                document_deadline,
            )
            .await
            .is_err()
            {
                extraction = ExtractedDocumentText::empty_for(format, Status::Unavailable);
            }
        }
        snapshots.push(DocumentSnapshot {
            attachment_id: record.id,
            source_sha256: record.sha256,
            extraction,
        });
    }
    let guard = state.read().await;
    if guard.workspace.as_ref().map(|w| &w.root_path) != Some(&root)
        || !guard.agents.contains_key(agent_id)
        || guard.sessions.get(agent_id, session_id).is_none()
        || !guard.runs.get(run_id).is_some_and(|run| {
            run.agent_id == agent_id
                && run.session_id == session_id
                && run.status == RunStatus::Running
        })
    {
        return;
    }
    if cancel.is_cancelled() {
        for snapshot in &mut snapshots {
            snapshot.extraction =
                ExtractedDocumentText::empty_for(snapshot.extraction.format, Status::Cancelled);
        }
    }
    if let Ok(snapshots) = fit_document_snapshots(&snapshots, max_bytes) {
        if !snapshots.is_empty() {
            if let Ok(value) = document_snapshots_metadata(&snapshots) {
                input
                    .metadata
                    .get_or_insert_with(Default::default)
                    .insert(DOCUMENT_EXTRACTS_METADATA_KEY.into(), value);
            }
        }
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
