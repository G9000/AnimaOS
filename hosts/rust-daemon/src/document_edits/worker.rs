//! One-shot, resource-limited imported-document worker with bounded binary framing.
//! Physical custody is Windows-only and its injected slot must be shared with extraction.
use crate::attachments::documents::worker::WorkerFailure;
use anima_core::CancelSignal;
use anima_document_content::{OfficeEditReview, OfficeEditSpec, OfficeFormat, OfficeInventory};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::sync::Arc;
use tokio::{sync::Semaphore, time::Instant};

const READY: &[u8] = b"ANIMA-DOCUMENT-EDIT-READY-v1\n";
const MAX_HEADER: usize = 64 * 1024;
const MAX_INPUT: usize = 4 * 1024 * 1024;
const MAX_RESPONSE_HEADER: usize = 512 * 1024;
const MAX_CANDIDATE: usize = 4 * 1024 * 1024;
const MAX_HTML_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Inspect {
        format: OfficeFormat,
    },
    Prepare {
        format: OfficeFormat,
        edits: OfficeEditSpec,
    },
}
impl Command {
    fn byte_limit(&self) -> usize {
        match self {
            Self::Inspect {
                format: OfficeFormat::Html,
            }
            | Self::Prepare {
                format: OfficeFormat::Html,
                ..
            } => MAX_HTML_BYTES,
            _ => MAX_INPUT,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    source_sha256: String,
    command: Command,
}
impl Request {
    fn inspect(format: OfficeFormat, source: &[u8]) -> Result<Self, WorkerFailure> {
        Self::new(Command::Inspect { format }, source)
    }
    fn prepare(
        format: OfficeFormat,
        edits: OfficeEditSpec,
        source: &[u8],
    ) -> Result<Self, WorkerFailure> {
        Self::new(Command::Prepare { format, edits }, source)
    }
    fn new(command: Command, source: &[u8]) -> Result<Self, WorkerFailure> {
        if source.is_empty() || source.len() > command.byte_limit() {
            return Err(WorkerFailure::InvalidOutput);
        }
        let request = Self {
            version: 1,
            source_sha256: digest(source),
            command,
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
    Inventory { inventory: OfficeInventory },
    Prepared { review: OfficeEditReview },
    Rejected { message: String },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    request_sha256: String,
    source_sha256: String,
    result: ResponseKind,
}

/// The bytes and review are accepted only from the owned fixed worker protocol.
pub(crate) struct PreparedEdit {
    pub(crate) bytes: Vec<u8>,
    pub(crate) review: OfficeEditReview,
}
enum Output {
    Inventory(OfficeInventory),
    Prepared(PreparedEdit),
    Rejected(String),
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid document edit worker frame",
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
fn read_request(reader: &mut impl Read) -> io::Result<(Request, Vec<u8>, Vec<u8>)> {
    let header = read_frame(reader, MAX_HEADER)?;
    let request: Request = serde_json::from_slice(&header).map_err(|_| invalid())?;
    let source = read_frame(reader, request.command.byte_limit())?;
    let mut trailing = [0; 1];
    if reader.read(&mut trailing)? != 0
        || request.version != 1
        || source.is_empty()
        || digest(&source) != request.source_sha256
    {
        return Err(invalid());
    }
    Ok((request, header, source))
}
fn write_response(
    writer: &mut impl Write,
    response: &Response,
    candidate: &[u8],
) -> io::Result<()> {
    let header = serde_json::to_vec(response).map_err(|_| invalid())?;
    if header.len() > MAX_RESPONSE_HEADER || candidate.len() > MAX_CANDIDATE {
        return Err(invalid());
    }
    writer.write_all(&(header.len() as u32).to_le_bytes())?;
    writer.write_all(&header)?;
    writer.write_all(&(candidate.len() as u32).to_le_bytes())?;
    writer.write_all(candidate)?;
    writer.flush()
}
fn process_request(request: Request, header: &[u8], source: &[u8]) -> (Response, Vec<u8>) {
    let format = match &request.command {
        Command::Inspect { format } | Command::Prepare { format, .. } => *format,
    };
    let text_allowed = |text: &str| {
        !crate::tools::generated_documents::credential_fields(text)
            && crate::foreign_sessions::redact_secrets(text) == text
    };
    let result: Result<(ResponseKind, Vec<u8>), String> = (|| {
        if !anima_document_content::check_office_document_text(format, source, text_allowed)
            .map_err(|error| error.to_string())?
        {
            return Err("Document retained content failed privacy screening".into());
        }
        match &request.command {
            Command::Inspect { format } => {
                anima_document_content::inspect_office_document(*format, source)
                    .map(|inventory| (ResponseKind::Inventory { inventory }, Vec::new()))
                    .map_err(|error| error.to_string())
            }
            Command::Prepare { format, edits } => {
                let prepared = anima_document_content::prepare_office_edit(*format, source, edits)
                    .map_err(|error| error.to_string())?;
                if !anima_document_content::check_office_document_text(
                    *format,
                    prepared.bytes(),
                    text_allowed,
                )
                .map_err(|error| error.to_string())?
                {
                    return Err("Document retained content failed privacy screening".into());
                }
                Ok((
                    ResponseKind::Prepared {
                        review: prepared.review().clone(),
                    },
                    prepared.bytes().to_vec(),
                ))
            }
        }
    })();
    let (result, bytes) = match result {
        Ok(value) => value,
        Err(error) => (
            ResponseKind::Rejected {
                message: error.to_string(),
            },
            Vec::new(),
        ),
    };
    (
        Response {
            version: 1,
            request_sha256: digest(header),
            source_sha256: request.source_sha256,
            result,
        },
        bytes,
    )
}

fn validate_response(
    request: &Request,
    header: &[u8],
    response: &Response,
    candidate: &[u8],
) -> Result<(), WorkerFailure> {
    if response.version != 1
        || response.request_sha256 != digest(header)
        || response.source_sha256 != request.source_sha256
        || candidate.len() > MAX_CANDIDATE
        || candidate.len() > request.command.byte_limit()
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    match (&request.command, &response.result) {
        (Command::Inspect { format }, ResponseKind::Inventory { inventory })
            if candidate.is_empty() && inventory.version == 1 && inventory.format == *format =>
        {
            Ok(())
        }
        (Command::Prepare { format, edits }, ResponseKind::Prepared { review })
            if !candidate.is_empty()
                && review.version == 1
                && review.before.format == *format
                && review.after.format == *format
                && review.preservation.unchanged_bytes_verified =>
        {
            let mut direct = vec![0u8; edits.operations.len()];
            for change in &review.changes {
                if !change.derived {
                    let index = change.operation_index.ok_or(WorkerFailure::InvalidOutput)?;
                    let count = direct.get_mut(index).ok_or(WorkerFailure::InvalidOutput)?;
                    *count = count.saturating_add(1);
                }
            }
            if direct.iter().any(|n| *n != 1)
                || serde_json::to_vec(review)
                    .map_err(|_| WorkerFailure::InvalidOutput)?
                    .len()
                    > 64 * 1024
            {
                return Err(WorkerFailure::InvalidOutput);
            }
            Ok(())
        }
        (_, ResponseKind::Rejected { message })
            if candidate.is_empty()
                && !message.is_empty()
                && message.len() <= 512
                && !message.chars().any(char::is_control) =>
        {
            Ok(())
        }
        _ => Err(WorkerFailure::InvalidOutput),
    }
}

/// Called before tracing, Tokio, config or host services by the fixed daemon mode.
pub fn document_edit_worker_entry() -> io::Result<()> {
    #[cfg(not(windows))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "document edit worker requires Windows Job limits",
    ));
    #[cfg(windows)]
    {
        use win_custody::{Job, JobLimits};
        let expected = JobLimits::new()
            .kill_on_close(true)
            .active_processes(1)
            .process_memory(256 * 1024 * 1024)
            .job_memory(256 * 1024 * 1024);
        let job = Job::create(&expected)?;
        if job.limits()? != expected {
            return Err(io::Error::other("document edit resource readback failed"));
        }
        job.assign_current_process()?;
        let mut output = io::stdout().lock();
        output.write_all(READY)?;
        output.flush()?;
        let (request, header, source) = read_request(&mut io::stdin().lock())?;
        let (response, candidate) = process_request(request, &header, &source);
        write_response(&mut output, &response, &candidate)
    }
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
        || source.len() > request.command.byte_limit()
        || digest(&source) != request.source_sha256
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    stdin
        .write_all(&(header.len() as u32).to_le_bytes())
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    stdin
        .write_all(&header)
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    stdin
        .write_all(&(source.len() as u32).to_le_bytes())
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    stdin
        .write_all(&source)
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    stdin
        .shutdown()
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    drop(stdin);
    let mut length = [0; 4];
    stdout
        .read_exact(&mut length)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    let size = u32::from_le_bytes(length) as usize;
    if size > MAX_RESPONSE_HEADER {
        return Err(WorkerFailure::InvalidOutput);
    }
    let mut response_bytes = vec![0; size];
    stdout
        .read_exact(&mut response_bytes)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    let response: Response =
        serde_json::from_slice(&response_bytes).map_err(|_| WorkerFailure::InvalidOutput)?;
    stdout
        .read_exact(&mut length)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    let size = u32::from_le_bytes(length) as usize;
    if size > MAX_CANDIDATE || size > request.command.byte_limit() {
        return Err(WorkerFailure::InvalidOutput);
    }
    let mut candidate = vec![0; size];
    stdout
        .read_exact(&mut candidate)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    let mut trailing = [0; 1];
    if stdout
        .read(&mut trailing)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?
        != 0
    {
        return Err(WorkerFailure::InvalidOutput);
    }
    validate_response(&request, &header, &response, &candidate)?;
    Ok(Some(match response.result {
        ResponseKind::Inventory { inventory } => Output::Inventory(inventory),
        ResponseKind::Prepared { review } => Output::Prepared(PreparedEdit {
            bytes: candidate,
            review,
        }),
        ResponseKind::Rejected { message } => Output::Rejected(message),
    }))
}

/// The constructor requires the extraction worker's actual shared physical slot.
#[derive(Clone)]
pub(crate) struct DocumentEditWorker {
    slots: Arc<Semaphore>,
}
impl DocumentEditWorker {
    pub(crate) fn new(shared_document_slot: Arc<Semaphore>) -> Self {
        Self {
            slots: shared_document_slot,
        }
    }
    pub(crate) async fn inspect(
        &self,
        format: OfficeFormat,
        source: Vec<u8>,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<OfficeInventory, String> {
        let request = Request::inspect(format, &source).map_err(worker_error)?;
        match self.run(request, source, cancel, deadline).await? {
            Output::Inventory(value) => Ok(value),
            Output::Rejected(error) => Err(error),
            _ => Err("Document worker returned the wrong result".into()),
        }
    }
    pub(crate) async fn prepare(
        &self,
        format: OfficeFormat,
        source: Vec<u8>,
        edits: OfficeEditSpec,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<PreparedEdit, String> {
        let request = Request::prepare(format, edits, &source).map_err(worker_error)?;
        match self.run(request, source, cancel, deadline).await? {
            Output::Prepared(value) => Ok(value),
            Output::Rejected(error) => Err(error),
            _ => Err("Document worker returned the wrong result".into()),
        }
    }
    async fn run(
        &self,
        request: Request,
        source: Vec<u8>,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<Output, String> {
        #[cfg(not(windows))]
        {
            let _ = (&self.slots, request, source, cancel, deadline);
            Err("Document editing is unavailable on this platform".into())
        }
        #[cfg(windows)]
        {
            physical::run(self.slots.clone(), request, source, cancel, deadline)
                .await
                .map_err(worker_error)
        }
    }
}
fn worker_error(error: WorkerFailure) -> String {
    format!("Document worker is unavailable: {error:?}")
}

#[cfg(windows)]
mod physical {
    use super::*;
    use process_wrap::tokio::{ChildWrapper, CommandWrap, CreationFlags, JobObject, KillOnDrop};
    use std::{
        process::Stdio,
        sync::{Mutex, OnceLock},
        time::Duration,
    };
    use tempfile::TempDir;
    use tokio::{
        process::Command,
        sync::{oneshot, OwnedSemaphorePermit},
        task::JoinHandle,
    };
    use tokio_util::sync::CancellationToken;

    type Task = JoinHandle<Result<Option<Output>, WorkerFailure>>;
    struct Resources {
        _permit: OwnedSemaphorePermit,
        directory: TempDir,
        child: Option<Box<dyn ChildWrapper>>,
        io: Option<Task>,
        stderr: Option<Task>,
    }
    struct Lease(Option<Resources>);
    impl Drop for Lease {
        fn drop(&mut self) {
            static QUARANTINE: OnceLock<Mutex<Vec<Resources>>> = OnceLock::new();
            if let Some(resources) = self.0.take() {
                QUARANTINE
                    .get_or_init(|| Mutex::new(Vec::new()))
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(resources);
            }
        }
    }
    struct CallerDrop(CancellationToken);
    impl Drop for CallerDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    pub(super) async fn run(
        slots: Arc<Semaphore>,
        request: Request,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<Output, WorkerFailure> {
        if cancel.is_cancelled() {
            return Err(WorkerFailure::Cancelled);
        }
        let deadline = deadline.min(Instant::now() + Duration::from_secs(10));
        if Instant::now() >= deadline {
            return Err(WorkerFailure::TimedOut);
        }
        let permit = slots.try_acquire_owned().map_err(|_| WorkerFailure::Busy)?;
        let directory = tempfile::Builder::new()
            .prefix("anima-document-edit-")
            .tempdir()
            .map_err(|_| WorkerFailure::Unavailable)?;
        let lease = Lease(Some(Resources {
            _permit: permit,
            directory,
            child: None,
            io: None,
            stderr: None,
        }));
        let stop = CancellationToken::new();
        let _caller_drop = CallerDrop(stop.clone());
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(owner(lease, request, bytes, cancel, stop, deadline, sender));
        receiver.await.map_err(|_| WorkerFailure::Unavailable)?
    }
    fn launch(
        resources: &mut Resources,
        request: Request,
        bytes: Vec<u8>,
    ) -> Result<(), WorkerFailure> {
        let executable = std::env::current_exe().map_err(|_| WorkerFailure::Unavailable)?;
        let mut native = Command::new(executable);
        native
            .arg("--document-edit-worker-v1")
            .env_clear()
            .current_dir(resources.directory.path())
            .env("TEMP", resources.directory.path())
            .env("TMP", resources.directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                native.env(name, value);
            }
        }
        let mut command = CommandWrap::from(native);
        command.wrap(KillOnDrop);
        let mut flags = CreationFlags(Default::default());
        flags.0 .0 = 0x08000000;
        command.wrap(flags).wrap(JobObject);
        resources.child = Some(command.spawn().map_err(|_| WorkerFailure::Unavailable)?);
        let child = resources.child.as_mut().ok_or(WorkerFailure::Unavailable)?;
        if child.as_ref().type_id() != std::any::TypeId::of::<process_wrap::tokio::JobObjectChild>()
            || child.inner().type_id() != std::any::TypeId::of::<tokio::process::Child>()
        {
            return Err(WorkerFailure::Unavailable);
        }
        let stdin = child.stdin().take().ok_or(WorkerFailure::Unavailable)?;
        let stdout = child.stdout().take().ok_or(WorkerFailure::Unavailable)?;
        let stderr = child.stderr().take().ok_or(WorkerFailure::Unavailable)?;
        resources.io = Some(tokio::spawn(exchange(request, bytes, stdin, stdout)));
        resources.stderr = Some(tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut reader = stderr;
            let mut total = 0;
            let mut buffer = [0; 512];
            loop {
                let count = reader
                    .read(&mut buffer)
                    .await
                    .map_err(|_| WorkerFailure::Unavailable)?;
                if count == 0 {
                    return Ok(None);
                }
                total += count;
                if total > 4096 {
                    return Err(WorkerFailure::InvalidOutput);
                }
            }
        }));
        Ok(())
    }
    async fn owner(
        mut lease: Lease,
        request: Request,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        stop: CancellationToken,
        deadline: Instant,
        sender: oneshot::Sender<Result<Output, WorkerFailure>>,
    ) {
        let mut sender = Some(sender);
        if cancel.is_cancelled() || stop.is_cancelled() || Instant::now() >= deadline {
            let failure = if Instant::now() >= deadline {
                WorkerFailure::TimedOut
            } else {
                WorkerFailure::Cancelled
            };
            if let Some(sender) = sender.take() {
                let _ = sender.send(Err(failure));
            }
            lease.0.take();
            return;
        }
        let Some(resources) = lease.0.as_mut() else {
            return;
        };
        if let Err(failure) = launch(resources, request, bytes) {
            if let Some(sender) = sender.take() {
                let _ = sender.send(Err(failure));
            }
            if let Some(child) = resources.child.as_mut() {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            return; // Uncertain spawn or containment remains quarantined.
        }
        let mut output = None;
        let failure = loop {
            if resources.io.is_none() && resources.stderr.is_none() {
                break None;
            }
            tokio::select! { biased;
                _ = cancel.cancelled() => break Some(WorkerFailure::Cancelled),
                _ = stop.cancelled() => break Some(WorkerFailure::Cancelled),
                _ = tokio::time::sleep_until(deadline) => break Some(WorkerFailure::TimedOut),
                result = async { resources.io.as_mut().expect("guarded io").await }, if resources.io.is_some() => { resources.io.take(); match result { Ok(Ok(Some(value))) => output = Some(value), Ok(Err(e)) => break Some(e), _ => break Some(WorkerFailure::Unavailable) } },
                result = async { resources.stderr.as_mut().expect("guarded stderr").await }, if resources.stderr.is_some() => { resources.stderr.take(); match result { Ok(Ok(None)) => {}, Ok(Err(e)) => break Some(e), _ => break Some(WorkerFailure::Unavailable) } },
            }
        };
        if let Some(failure) = failure {
            if let Some(sender) = sender.take() {
                let _ = sender.send(Err(failure));
            }
        }
        let Some(child) = resources.child.as_mut() else {
            return;
        };
        if sender.is_some() {
            let leader = tokio::select! { biased;
                _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
                _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
                _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
                status = child.inner_mut().wait() => status.map_err(|_| WorkerFailure::Unavailable).and_then(|status| if status.success() { Ok(()) } else { Err(WorkerFailure::Unavailable) }),
            };
            if let Err(failure) = leader {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(Err(failure));
                }
            }
        }
        let killed = child.start_kill();
        let waited = child.wait().await;
        if killed.is_err() || waited.is_err() {
            return;
        }
        for task in [&mut resources.io, &mut resources.stderr] {
            if let Some(handle) = task.as_mut() {
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(2), handle).await,
                    Ok(Ok(_))
                ) {
                    return;
                }
                task.take();
            }
        }
        if let Some(sender) = sender.take() {
            let result = if cancel.is_cancelled() || stop.is_cancelled() {
                Err(WorkerFailure::Cancelled)
            } else if Instant::now() >= deadline {
                Err(WorkerFailure::TimedOut)
            } else if !waited.is_ok_and(|status| status.success()) {
                Err(WorkerFailure::Unavailable)
            } else {
                output.ok_or(WorkerFailure::InvalidOutput)
            };
            let _ = sender.send(result);
        }
        lease.0.take();
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
