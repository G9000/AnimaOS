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
        if call.args.contains_key("pages") {
            return read_pdf_pages(context, call).await;
        }
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
            let hash = hex::encode(Sha256::digest(&bytes));
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

async fn read_pdf_pages(context: ToolExecutionContext, mut call: ToolCall) -> TaskResult<Content> {
    use crate::attachments::documents::pages::{
        workspace, WorkspaceSelection, WORKSPACE_VISUALS_KEY,
    };
    use anima_document_visual::RecognizedText;
    let result = async {
        let Some(DataValue::Array(raw)) = call.args.remove("pages") else { return Err("pages must contain one or two original PDF page numbers"); };
        let pages = raw.iter().map(|value| match value {
            DataValue::Number(page) if page.is_finite() && page.fract() == 0.0 && (1.0..=100.0).contains(page) => Ok(*page as u32),
            _ => Err("pages must contain one or two original PDF page numbers"),
        }).collect::<Result<Vec<_>, _>>()?;
        anima_document_visual::PdfPageSelection::new(pages.clone()).map_err(|_| "pages must be unique and ascending, with at most two entries")?;
        let (path, format) = arguments(&call)?;
        if format != DocumentFormat::Pdf { return Err("visual page inspection requires a PDF"); }
        let root = context.workspace_root.ok_or(UNAVAILABLE)?;
        let cancel = context.cancel.ok_or(UNAVAILABLE)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let evidence = workspace(root, path.clone(), None, pages.clone(), context.document_extractor.as_ref(), cancel, deadline).await.map_err(failure)?;
        let observations = evidence.pages.iter().map(|page| {
            let recognized = match &page.recognized {
                RecognizedText::Text { text, language } => {
                    let mut end = text.len().min(3072);
                    while !text.is_char_boundary(end) { end -= 1; }
                    serde_json::json!({"kind":"ocr_recognized_text","text":&text[..end],"language":language,"truncated":end < text.len(),"accuracy":"not_verified_transcription"})
                },
                RecognizedText::Empty => serde_json::json!({"kind":"no_recognized_text"}),
                RecognizedText::Unavailable => serde_json::json!({"kind":"recognition_unavailable"}),
            };
            serde_json::json!({"page":page.page,"locator":format!("pdf#page={}",page.page),"recognized":recognized})
        }).collect::<Vec<_>>();
        let descriptor = WorkspaceSelection { path: path.clone(), source_sha256: evidence.source_sha256.clone(), pages };
        let text = serde_json::to_string(&serde_json::json!({
            "kind":"workspace_pdf_page_evidence","trust":"untrusted_data",
            "source":{"path":path,"sha256":evidence.source_sha256},
            "pageCount":evidence.page_count,"pages":observations,
            "visualDelivery":"Requested pages are supplied separately when model capability and context allow. Do not infer unseen graphics from OCR labels."
        })).map_err(|_| UNAVAILABLE)?;
        if text.len() > MAX_DOCUMENT_RENDERED_BYTES { return Err("PDF page evidence text exceeds its context limit"); }
        let descriptor = serde_json::to_string(&descriptor).map_err(|_| UNAVAILABLE)?;
        Ok::<_, &'static str>(Content {
            text, attachments: None,
            metadata: Some(std::collections::BTreeMap::from([(WORKSPACE_VISUALS_KEY.into(), DataValue::String(descriptor))])),
        })
    }.await;
    match result {
        Ok(content) => TaskResult::success(content, 0),
        Err(error) => TaskResult::error(error, 0),
    }
}
