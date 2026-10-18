//! Granted workspace document reads return captured bytes as ordinary untrusted
//! tool data. They never upload or manufacture attachment/history authority.
use std::{path::PathBuf, sync::Arc, time::Duration};

use anima_core::{AgentState, CancelSignal, Content, DataValue, Message, TaskResult, ToolCall};
use anima_service_adapters::document_content::{
    fit_document_snapshots, validate_document_extraction, DocumentFormat, DocumentSnapshot,
    ExtractedDocumentText, MAX_DOCUMENT_RENDERED_BYTES,
};
use futures::future::BoxFuture;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::{sync::Semaphore, time::Instant};

use super::{workspace_bytes::WorkspaceBytes, ToolExecutionContext};
use crate::attachments::documents::{
    source_read::{read_retained_with_slots, shared_read_slots},
    worker::{DocumentExtractor, WorkerFailure},
    MAX_DOCUMENT_INPUT_BYTES,
};

const UNAVAILABLE: &str = "Workspace document is unavailable";
const INVALID_PATH: &str = "read_document requires one supported workspace-relative file_path";
const CANCELLED: &str = "Workspace document read cancelled";
const TIMED_OUT: &str = "Workspace document read timed out";

pub(super) fn read_document(
    context: ToolExecutionContext,
    _agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let deadline = Instant::now() + Duration::from_secs(10);
        let result = async {
            let (path, format) = arguments(&call)?;
            let root = context.workspace_root.ok_or(UNAVAILABLE)?;
            let cancel = context.cancel.ok_or(UNAVAILABLE)?;
            execute(
                ReadRequest {
                    root,
                    path,
                    format,
                    extractor: context.document_extractor,
                    cancel,
                    deadline,
                },
                shared_read_slots(),
            )
            .await
        }
        .await;
        match result {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

fn arguments(call: &ToolCall) -> Result<(String, DocumentFormat), &'static str> {
    if call.args.len() != 1 {
        return Err(INVALID_PATH);
    }
    let Some(DataValue::String(path)) = call.args.get("file_path") else {
        return Err(INVALID_PATH);
    };
    if path.is_empty()
        || path.len() > 1024
        || path.chars().any(char::is_control)
        || path.contains(['\\', ':'])
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(INVALID_PATH);
    }
    let (_, extension) = path.rsplit_once('.').ok_or(INVALID_PATH)?;
    let format = match extension.to_ascii_lowercase().as_str() {
        "docx" => DocumentFormat::Docx,
        "pdf" => DocumentFormat::Pdf,
        "xlsx" => DocumentFormat::Xlsx,
        _ => return Err(INVALID_PATH),
    };
    Ok((path.clone(), format))
}

struct ReadRequest {
    root: PathBuf,
    path: String,
    format: DocumentFormat,
    extractor: Arc<dyn DocumentExtractor>,
    cancel: CancelSignal,
    deadline: Instant,
}

fn active(cancel: &CancelSignal, deadline: Instant) -> Result<(), &'static str> {
    if cancel.is_cancelled() {
        return Err(CANCELLED);
    }
    if Instant::now() >= deadline {
        return Err(TIMED_OUT);
    }
    Ok(())
}

fn failure(error: WorkerFailure) -> &'static str {
    match error {
        WorkerFailure::Cancelled => CANCELLED,
        WorkerFailure::TimedOut => TIMED_OUT,
        _ => UNAVAILABLE,
    }
}

async fn execute(request: ReadRequest, slots: Arc<Semaphore>) -> Result<String, &'static str> {
    let ReadRequest {
        root,
        path,
        format,
        extractor,
        cancel,
        deadline,
    } = request;
    active(&cancel, deadline)?;
    let relative = path.clone();
    let (bytes, sha256) = read_retained_with_slots(
        move || {
            let workspace = WorkspaceBytes::open(&root).map_err(|_| UNAVAILABLE)?;
            let bytes = workspace
                .read_file_snapshot(&relative, MAX_DOCUMENT_INPUT_BYTES as usize)
                .map_err(|_| UNAVAILABLE)?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            Ok::<_, &'static str>((bytes, hash))
        },
        &cancel,
        deadline,
        slots,
    )
    .await
    .map_err(failure)??;
    active(&cancel, deadline)?;
    // Apply the same outer absolute deadline to injected extractors; cooperative
    // cancellation is not sufficient. The real worker owns retirement on drop.
    let extracted = tokio::select! { biased;
        () = cancel.cancelled() => return Err(CANCELLED),
        () = tokio::time::sleep_until(deadline) => return Err(TIMED_OUT),
        result = extractor.extract(format,bytes,cancel.clone(),Some(deadline)) => result.map_err(failure)?,
    };
    active(&cancel, deadline)?;
    if extracted.format != format {
        return Err(UNAVAILABLE);
    }
    let text = fit_envelope(&path, &sha256, extracted, &cancel, deadline)?;
    active(&cancel, deadline)?;
    Ok(text)
}

#[derive(Serialize)]
struct Source<'a> {
    path: &'a str,
    sha256: &'a str,
}

#[derive(Serialize)]
struct Envelope<'a> {
    kind: &'static str,
    trust: &'static str,
    source: Source<'a>,
    extraction: &'a ExtractedDocumentText,
}

fn serialize(
    path: &str,
    sha256: &str,
    extraction: &ExtractedDocumentText,
) -> Result<String, &'static str> {
    validate_document_extraction(extraction).map_err(|_| UNAVAILABLE)?;
    serde_json::to_string(&Envelope {
        kind: "workspace_document",
        trust: "untrusted_data",
        source: Source { path, sha256 },
        extraction,
    })
    .map_err(|_| UNAVAILABLE)
}

fn fit_envelope(
    path: &str,
    sha256: &str,
    extraction: ExtractedDocumentText,
    cancel: &CancelSignal,
    deadline: Instant,
) -> Result<String, &'static str> {
    active(cancel, deadline)?;
    let original = serialize(path, sha256, &extraction)?;
    active(cancel, deadline)?;
    if original.len() <= MAX_DOCUMENT_RENDERED_BYTES {
        return Ok(original);
    }
    let snapshots = [DocumentSnapshot {
        attachment_id: "workspace-document".into(),
        source_sha256: sha256.to_owned(),
        extraction,
    }];
    let probe = |budget| -> Result<String, &'static str> {
        active(cancel, deadline)?;
        let fitted = fit_document_snapshots(&snapshots, budget).map_err(|_| UNAVAILABLE)?;
        let text = serialize(path, sha256, &fitted[0].extraction)?;
        active(cancel, deadline)?;
        Ok(text)
    };
    // One zero-budget probe plus at most fourteen search probes. Sacrificing
    // the single largest internal fit budget bounds the search to fifteen;
    // exact envelope bytes, not the synthetic source row, decide admission.
    let mut best = probe(0)?;
    if best.len() > MAX_DOCUMENT_RENDERED_BYTES {
        return Err(UNAVAILABLE);
    }
    let mut low = 1usize;
    let mut high = MAX_DOCUMENT_RENDERED_BYTES - 1;
    while low <= high {
        let middle = low + (high - low) / 2;
        let candidate = probe(middle)?;
        if candidate.len() <= MAX_DOCUMENT_RENDERED_BYTES {
            best = candidate;
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    active(cancel, deadline)?;
    if best.len() > MAX_DOCUMENT_RENDERED_BYTES {
        return Err(UNAVAILABLE);
    }
    Ok(best)
}

#[cfg(test)]
#[path = "workspace_document_tests.rs"]
mod tests;
