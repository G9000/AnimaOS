//! Fixed Office inspection subprocess with bounded framing and whole-tree custody.
//! Jobs and UNO settings constrain resources and lifetime, not filesystem or network access.

use anima_core::execution_limits::{ExecutionContext, ExecutionPermit, SettlementOutcome};
use anima_core::CancelSignal;
use anima_document_content::OfficeFormat;
use anima_document_visual::inspection::ArtifactInspectionReport;
use anima_document_visual::{PdfPageEvidence, RecognizedText};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{sync::Semaphore, time::Instant};

use crate::attachments::documents::worker::WorkerFailure;

const READY: &[u8] = b"ANIMA-DOCUMENT-INSPECTION-READY-v1\n";
const MAX_HEADER: usize = 64 * 1024;
const MAX_INPUT: usize = 4 * 1024 * 1024;
const MAX_RESPONSE_HEADER: usize = 1024 * 1024;
const MAX_PDF: usize = 2 * 1024 * 1024;
const MAX_PNG: usize = 2 * 1024 * 1024;
const MAX_PAGES: usize = 10;
const MAX_OUTPUT: usize = 20 * 1024 * 1024;
// Framed report/pages are bounded at 20 MiB; the exported PDF adds at most 2 MiB.
const MAX_ARTIFACT_BYTES: u64 = (MAX_OUTPUT + MAX_PDF) as u64;
const CONTROLLER: &str = include_str!("worker/controller.py");

/// Trusted operator configuration, captured once at daemon startup.
///
/// The entire installation is pinned, including Python, DLLs and configuration.
/// Model/tool input cannot select this path, hash, command, or controller.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineConfig {
    program_directory: PathBuf,
    tree_sha256: String,
}

impl EngineConfig {
    /// Captures an explicit absolute LibreOffice program directory and tree pin.
    ///
    /// Both environment values must be supplied together. An absent installation
    /// is an unavailable backend; the daemon never searches PATH or user apps.
    pub(crate) fn from_environment() -> Result<Option<Self>, String> {
        let directory = std::env::var_os("ANIMA_DOCUMENT_INSPECTION_LIBREOFFICE_PROGRAM");
        let hash = std::env::var("ANIMA_DOCUMENT_INSPECTION_LIBREOFFICE_TREE_SHA256").ok();
        match (directory, hash) {
            (None, None) => Ok(None),
            (Some(directory), Some(hash)) => Self::validate_parts(directory.into(), hash).map(Some),
            _ => Err(
                "Document inspection engine is unavailable: incomplete operator configuration"
                    .into(),
            ),
        }
    }

    fn validate_parts(program_directory: PathBuf, tree_sha256: String) -> Result<Self, String> {
        if !program_directory.is_absolute()
            || program_directory.file_name().and_then(|name| name.to_str()) != Some("program")
            || !valid_digest(&tree_sha256)
            || program_directory.to_str().is_none()
        {
            return Err(
                "Document inspection engine is unavailable: invalid operator configuration".into(),
            );
        }
        Ok(Self {
            program_directory,
            tree_sha256,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    source_sha256: String,
    source_byte_length: u64,
    source_version: String,
    format: OfficeFormat,
    engine: EngineConfig,
}

impl Request {
    fn new(
        format: OfficeFormat,
        source_version: String,
        source: &[u8],
        engine: EngineConfig,
    ) -> Result<Self, WorkerFailure> {
        if source.is_empty()
            || source.len() > MAX_INPUT
            || !matches!(
                format,
                OfficeFormat::Docx | OfficeFormat::Xlsx | OfficeFormat::Pptx
            )
            || source_version.is_empty()
            || source_version.len() > 256
            || source_version.chars().any(char::is_control)
        {
            return Err(WorkerFailure::InvalidOutput);
        }
        let request = Self {
            version: 1,
            source_sha256: digest(source),
            source_byte_length: source.len() as u64,
            source_version,
            format,
            engine,
        };
        if serde_json::to_vec(&request)
            .map_err(|_| WorkerFailure::InvalidOutput)?
            .len()
            > MAX_HEADER
        {
            return Err(WorkerFailure::InvalidOutput);
        }
        Ok(request)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum ResponseKind {
    Inspected { report: ArtifactInspectionReport },
    Rejected { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageHeader {
    page: u32,
    width: u32,
    height: u32,
    sha256: String,
    byte_length: usize,
    recognized: RecognizedText,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    request_sha256: String,
    source_sha256: String,
    result: ResponseKind,
    pages: Vec<PageHeader>,
}

/// Immutable evidence from the fixed worker after all its descendants retire.
#[derive(Debug)]
pub(crate) struct InspectionOutput {
    pub(crate) report: ArtifactInspectionReport,
    pub(crate) pages: Vec<PdfPageEvidence>,
}

enum Output {
    Inspected(InspectionOutput),
    Rejected(String),
}

fn inspection_artifact_bytes(output: &InspectionOutput) -> Result<u64, WorkerFailure> {
    let report_bytes = serde_json::to_vec(&output.report)
        .map_err(|_| WorkerFailure::InvalidOutput)?
        .len() as u64;
    output.pages.iter().try_fold(
        report_bytes
            .checked_add(output.report.rendering().pdf_byte_length)
            .ok_or(WorkerFailure::InvalidOutput)?,
        |total, page| {
            total
                .checked_add(page.png.len() as u64)
                .ok_or(WorkerFailure::InvalidOutput)
        },
    )
}

async fn retire_never_sent(permit: &mut Option<ExecutionPermit>) -> Result<(), String> {
    if let Some(permit) = permit.as_mut() {
        permit
            .finish_owned(SettlementOutcome::ProvenNeverSent, true)
            .await?;
    }
    permit.take();
    Ok(())
}

fn inspection_deadline(
    deadline: Instant,
    execution: Option<&ExecutionContext>,
) -> Result<Instant, String> {
    let now = Instant::now();
    let mut deadline = deadline.min(now + std::time::Duration::from_secs(60));
    if let Some(execution) = execution {
        deadline = deadline.min(
            now + std::time::Duration::from_millis(execution.remaining_wall_time_ms()?.min(60_000)),
        );
    }
    if deadline <= now {
        return Err("Document inspection deadline reached".into());
    }
    Ok(deadline)
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn configuration_digest(engine: &EngineConfig) -> Result<String, WorkerFailure> {
    let mut configuration = Sha256::new();
    configuration.update(b"anima-office-inspection-policy-v3\0original-readonly;macros=0;updates=0;abort-interaction;calculate-all;exported-view;blank-pages;job=8/512MiB/1GiB;60s;core-python=-s,-S,-P,-B;trusted-home-and-path-only;embedded-python-no-bytecode;no-breakpad;short-private-root;engine-postflight-pin\0");
    configuration.update(serde_json::to_vec(engine).map_err(|_| WorkerFailure::InvalidOutput)?);
    configuration.update(digest(CONTROLLER.as_bytes()).as_bytes());
    Ok(hex::encode(configuration.finalize()))
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid document inspection worker frame",
    )
}
fn read_frame(reader: &mut impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > limit {
        return Err(invalid());
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn write_frame(writer: &mut impl Write, bytes: &[u8], limit: usize) -> io::Result<()> {
    if bytes.len() > limit {
        return Err(invalid());
    }
    let length = u32::try_from(bytes.len()).map_err(|_| invalid())?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(bytes)
}
fn read_request(reader: &mut impl Read) -> io::Result<(Request, Vec<u8>, Vec<u8>)> {
    let header = read_frame(reader, MAX_HEADER)?;
    let request: Request = serde_json::from_slice(&header).map_err(|_| invalid())?;
    let source = read_frame(reader, MAX_INPUT)?;
    let mut trailing = [0; 1];
    if reader.read(&mut trailing)? != 0
        || request.version != 1
        || source.is_empty()
        || source.len() as u64 != request.source_byte_length
        || digest(&source) != request.source_sha256
        || Request::new(
            request.format,
            request.source_version.clone(),
            &source,
            request.engine.clone(),
        )
        .is_err()
        || EngineConfig::validate_parts(
            request.engine.program_directory.clone(),
            request.engine.tree_sha256.clone(),
        )
        .is_err()
    {
        return Err(invalid());
    }
    Ok((request, header, source))
}

fn validate_response(
    request: &Request,
    header: &[u8],
    response: &Response,
    pages: &[PdfPageEvidence],
) -> Result<(), WorkerFailure> {
    if response.version != 1
        || response.request_sha256 != digest(header)
        || response.source_sha256 != request.source_sha256
        || pages.len() > MAX_PAGES
        || pages.len() != response.pages.len()
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    let mut total = serde_json::to_vec(response)
        .map_err(|_| WorkerFailure::InvalidOutput)?
        .len();
    for (metadata, page) in response.pages.iter().zip(pages) {
        total = total
            .checked_add(page.png.len())
            .ok_or(WorkerFailure::InvalidOutput)?;
        if total > MAX_OUTPUT
            || page.png.len() > MAX_PNG
            || page.png.len() != metadata.byte_length
            || page.page != metadata.page
            || page.width != metadata.width
            || page.height != metadata.height
            || digest(&page.png) != metadata.sha256
            || page.recognized != metadata.recognized
        {
            return Err(WorkerFailure::InvalidOutput);
        }
    }
    match &response.result {
        ResponseKind::Rejected { message }
            if pages.is_empty()
                && !message.is_empty()
                && message.len() <= 256
                && !message.chars().any(char::is_control) =>
        {
            Ok(())
        }
        ResponseKind::Inspected { report } => {
            report
                .validate()
                .map_err(|_| WorkerFailure::InvalidOutput)?;
            let source = report.source();
            if source.sha256 != request.source_sha256
                || source.source_version != request.source_version
                || source.format != request.format
                || source.byte_length != request.source_byte_length
                || report.rendering().pages.len() != pages.len()
                || pages.is_empty()
                || report.renderer().configuration_sha256 != configuration_digest(&request.engine)?
            {
                return Err(WorkerFailure::InvalidOutput);
            }
            for (evidence, page) in report.rendering().pages.iter().zip(pages) {
                if evidence.ordinal != page.page
                    || evidence.width != page.width
                    || evidence.height != page.height
                    || evidence.byte_length != page.png.len() as u64
                    || evidence.png_sha256 != digest(&page.png)
                {
                    return Err(WorkerFailure::InvalidOutput);
                }
            }
            Ok(())
        }
        _ => Err(WorkerFailure::InvalidOutput),
    }
}

fn write_response(
    writer: &mut impl Write,
    response: &Response,
    pages: &[PdfPageEvidence],
) -> io::Result<()> {
    let header = serde_json::to_vec(response).map_err(|_| invalid())?;
    let total = pages
        .iter()
        .try_fold(header.len(), |sum, page| sum.checked_add(page.png.len()))
        .ok_or_else(invalid)?;
    if total > MAX_OUTPUT || pages.len() > MAX_PAGES || pages.len() != response.pages.len() {
        return Err(invalid());
    }
    write_frame(writer, &header, MAX_RESPONSE_HEADER)?;
    for page in pages {
        write_frame(writer, &page.png, MAX_PNG)?;
    }
    writer.flush()
}

async fn exchange(
    request: Request,
    source: Vec<u8>,
    mut stdin: impl tokio::io::AsyncWrite + Unpin,
    mut stdout: impl tokio::io::AsyncRead + Unpin,
) -> Result<Option<Output>, WorkerFailure> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut ready = vec![0; READY.len()];
    stdout
        .read_exact(&mut ready)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    if ready != READY {
        return Err(WorkerFailure::InvalidOutput);
    }
    let header = serde_json::to_vec(&request).map_err(|_| WorkerFailure::InvalidOutput)?;
    if header.len() > MAX_HEADER
        || source.len() > MAX_INPUT
        || digest(&source) != request.source_sha256
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    for bytes in [&header[..], &source[..]] {
        stdin
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .await
            .map_err(|_| WorkerFailure::Unavailable)?;
        stdin
            .write_all(bytes)
            .await
            .map_err(|_| WorkerFailure::Unavailable)?;
    }
    stdin
        .shutdown()
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    drop(stdin);
    let response_header = async_read_frame(&mut stdout, MAX_RESPONSE_HEADER).await?;
    let response: Response =
        serde_json::from_slice(&response_header).map_err(|_| WorkerFailure::InvalidOutput)?;
    if response.pages.len() > MAX_PAGES {
        return Err(WorkerFailure::InvalidOutput);
    }
    let mut total = response_header.len();
    let mut pages = Vec::with_capacity(response.pages.len());
    for metadata in &response.pages {
        if metadata.byte_length > MAX_PNG {
            return Err(WorkerFailure::InvalidOutput);
        }
        let limit = MAX_PNG.min(
            MAX_OUTPUT
                .checked_sub(total)
                .ok_or(WorkerFailure::InvalidOutput)?,
        );
        let png = async_read_frame(&mut stdout, limit).await?;
        total += png.len();
        pages.push(PdfPageEvidence {
            page: metadata.page,
            width: metadata.width,
            height: metadata.height,
            png,
            recognized: metadata.recognized.clone(),
        });
    }
    let mut trailing = [0; 1];
    if stdout
        .read(&mut trailing)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?
        != 0
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    validate_response(&request, &header, &response, &pages)?;
    Ok(Some(match response.result {
        ResponseKind::Inspected { report } => Output::Inspected(InspectionOutput { report, pages }),
        ResponseKind::Rejected { message } => Output::Rejected(message),
    }))
}

async fn async_read_frame(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    limit: usize,
) -> Result<Vec<u8>, WorkerFailure> {
    use tokio::io::AsyncReadExt;
    let mut length = [0; 4];
    reader
        .read_exact(&mut length)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > limit {
        return Err(WorkerFailure::InvalidOutput);
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    Ok(bytes)
}

/// Uses the extraction/edit workers' actual shared physical admission slot.
#[derive(Clone, Debug)]
pub(crate) struct DocumentInspectionWorker {
    slots: Arc<Semaphore>,
    engine: Option<EngineConfig>,
    #[cfg(all(windows, test))]
    test_executable: Option<PathBuf>,
}
impl Default for DocumentInspectionWorker {
    fn default() -> Self {
        Self::new()
    }
}
impl DocumentInspectionWorker {
    pub(crate) fn new() -> Self {
        Self::with_engine_config(None)
    }
    pub(crate) fn with_engine_config(engine: Option<EngineConfig>) -> Self {
        Self {
            slots: crate::attachments::documents::worker::shared_physical_slot(),
            engine,
            #[cfg(all(windows, test))]
            test_executable: None,
        }
    }
    #[cfg(all(windows, test))]
    pub(super) fn with_test_executable(mut self, executable: PathBuf) -> Self {
        self.test_executable = Some(executable);
        self
    }
    #[cfg(all(windows, test))]
    pub(super) fn test_physical_capacity_available(&self) -> bool {
        self.slots.available_permits() > 0
    }
    pub(crate) async fn inspect(
        &self,
        original: Vec<u8>,
        format: OfficeFormat,
        source_version: String,
        cancel: CancelSignal,
        deadline: Instant,
        execution: Option<ExecutionContext>,
    ) -> Result<InspectionOutput, String> {
        let bound = execution.clone();
        let operation = async move {
            if cancel.is_cancelled() {
                return Err(worker_error(WorkerFailure::Cancelled));
            }
            if let Some(context) = execution.as_ref() {
                context.check_authority().await?;
            }
            let deadline = inspection_deadline(deadline, execution.as_ref())?;
            let mut budget = match execution.as_ref() {
                Some(context) => Some(
                    context
                        .reserve_native_artifact(
                            &format!("document-inspection:{}", uuid::Uuid::new_v4()),
                            MAX_ARTIFACT_BYTES,
                        )
                        .await?,
                ),
                None => None,
            };
            let request = (|| {
                let engine = self.engine.clone().ok_or_else(|| "Document inspection engine is unavailable: configure the trusted operator installation".to_string())?;
                Request::new(format, source_version, &original, engine).map_err(worker_error)
            })();
            let request = match request {
                Ok(request) => request,
                Err(error) => {
                    retire_never_sent(&mut budget).await?;
                    return Err(error);
                }
            };
            #[cfg(not(windows))]
            {
                let _ = (&self.slots, request, cancel, deadline);
                retire_never_sent(&mut budget).await?;
                Err("Document inspection is unavailable on this platform".into())
            }
            #[cfg(windows)]
            {
                #[cfg(test)]
                let executable = self.test_executable.clone();
                #[cfg(not(test))]
                let executable = None;
                match physical::run(
                    self.slots.clone(),
                    request,
                    original,
                    cancel,
                    deadline,
                    execution,
                    budget,
                    executable,
                )
                .await
                .map_err(worker_error)?
                {
                    Output::Inspected(output) => Ok(output),
                    Output::Rejected(message) => Err(message),
                }
            }
        };
        match bound {
            Some(context) => context.bounded(operation).await,
            None => operation.await,
        }
    }
}
fn worker_error(error: WorkerFailure) -> String {
    format!("Document inspection worker is unavailable: {error:?}")
}

/// Runs before tracing, Tokio, normal configuration, and host service startup.
pub fn document_inspection_worker_entry() -> io::Result<()> {
    #[cfg(not(windows))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "document inspection requires Windows Job limits",
    ));
    #[cfg(windows)]
    {
        use win_custody::{Job, JobLimits};
        let expected = JobLimits::new()
            .kill_on_close(true)
            .active_processes(8)
            .process_memory(512 * 1024 * 1024)
            .job_memory(1024 * 1024 * 1024);
        let job = Job::create(&expected)?;
        if job.limits()? != expected {
            return Err(io::Error::other(
                "document inspection resource readback failed",
            ));
        }
        job.assign_current_process()?;
        let started = std::time::Instant::now();
        let mut output = io::stdout().lock();
        output.write_all(READY)?;
        output.flush()?;
        let (request, header, source) = read_request(&mut io::stdin().lock())?;
        let result = engine::inspect(&request, &source, started);
        let (result, pages) = match result {
            Ok(output) => (
                ResponseKind::Inspected {
                    report: output.report,
                },
                output.pages,
            ),
            Err(message) => (
                ResponseKind::Rejected {
                    message: message.into(),
                },
                Vec::new(),
            ),
        };
        let response = Response {
            version: 1,
            request_sha256: digest(&header),
            source_sha256: request.source_sha256,
            result,
            pages: pages
                .iter()
                .map(|page| PageHeader {
                    page: page.page,
                    width: page.width,
                    height: page.height,
                    sha256: digest(&page.png),
                    byte_length: page.png.len(),
                    recognized: page.recognized.clone(),
                })
                .collect(),
        };
        if started.elapsed() >= std::time::Duration::from_secs(60) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "document inspection deadline elapsed",
            ));
        }
        write_response(&mut output, &response, &pages)
    }
}

#[cfg(windows)]
#[path = "worker/engine.rs"]
mod engine;
#[cfg(windows)]
#[path = "worker/physical.rs"]
mod physical;
#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
