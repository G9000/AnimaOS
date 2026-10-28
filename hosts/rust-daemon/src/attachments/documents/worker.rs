//! Fixed, one-shot document worker. The physical owner outlives the calling future.
//! Jobs constrain resources/lifetime; they are not a filesystem/network sandbox.
use std::io::{self, Read, Write};

use anima_core::CancelSignal;
use anima_service_adapters::document_content::{
    validate_document_extraction, DocumentFormat, ExtractedDocumentText,
};
use async_trait::async_trait;
use tokio::time::Instant;

const READY: &[u8] = b"ANIMA-DOCUMENT-READY-v2\n";
const MAX_INPUT: usize = 2 * 1024 * 1024;
const MAX_STDOUT: usize = 64 * 1024;
const MAX_FRAME: usize = MAX_STDOUT - READY.len() - 4;
const MAX_STDERR: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerFailure {
    Busy,
    Unavailable,
    Cancelled,
    TimedOut,
    InvalidOutput,
}

#[async_trait]
pub(crate) trait DocumentExtractor: Send + Sync {
    async fn extract(
        &self,
        format: DocumentFormat,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure>;
}

#[derive(Default)]
pub(crate) struct DocumentWorker;

/// Editing and extraction share this one actual process-custody admission slot.
pub(crate) fn shared_physical_slot() -> std::sync::Arc<tokio::sync::Semaphore> {
    #[cfg(windows)]
    {
        physical::shared_physical_slot()
    }
    #[cfg(not(windows))]
    {
        static SLOT: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
            std::sync::OnceLock::new();
        SLOT.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }
}

#[async_trait]
impl DocumentExtractor for DocumentWorker {
    async fn extract(
        &self,
        format: DocumentFormat,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        if cancel.is_cancelled() {
            return Err(WorkerFailure::Cancelled);
        }
        if deadline.is_some_and(|value| Instant::now() >= value) {
            return Err(WorkerFailure::TimedOut);
        }
        if bytes.len() > MAX_INPUT {
            return Err(WorkerFailure::InvalidOutput);
        }
        #[cfg(not(windows))]
        {
            let _ = (format, bytes);
            Err(WorkerFailure::Unavailable)
        }
        #[cfg(windows)]
        {
            physical::extract(format, bytes, cancel, deadline).await
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid document worker frame")
}

fn read_request(reader: &mut impl Read) -> io::Result<(DocumentFormat, Vec<u8>)> {
    let mut format = [0; 1];
    reader.read_exact(&mut format)?;
    let format = match format[0] {
        0 => DocumentFormat::Docx,
        1 => DocumentFormat::Pdf,
        2 => DocumentFormat::Xlsx,
        _ => return Err(invalid()),
    };
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_INPUT {
        return Err(invalid());
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    let mut trailing = [0; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err(invalid());
    }
    Ok((format, bytes))
}

fn write_result(writer: &mut impl Write, value: &ExtractedDocumentText) -> io::Result<()> {
    validate_document_extraction(value).map_err(|_| invalid())?;
    let json = serde_json::to_vec(value).map_err(|_| invalid())?;
    if json.len() > MAX_FRAME {
        return Err(invalid());
    }
    writer.write_all(&(json.len() as u32).to_le_bytes())?;
    writer.write_all(&json)?;
    writer.flush()
}

/// Called by the binary before tracing, Tokio, configuration, or host services.
pub fn document_worker_entry() -> io::Result<()> {
    #[cfg(not(windows))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "document worker requires Windows Job limits",
    ));
    #[cfg(windows)]
    {
        install_resource_limits()?;
        let mut output = io::stdout().lock();
        output.write_all(READY)?;
        output.flush()?;
        let (format, bytes) = read_request(&mut io::stdin().lock())?;
        // Synchronous Rust parser: no runtime, threads, converter, or subprocess.
        let extracted = anima_document_content::extract_document(format, &bytes);
        write_result(&mut output, &extracted)
    }
}

#[cfg(windows)]
fn install_resource_limits() -> io::Result<()> {
    use win_custody::{Job, JobLimits};
    let expected = JobLimits::new()
        .kill_on_close(true)
        .active_processes(1)
        .process_memory(256 * 1024 * 1024)
        .job_memory(256 * 1024 * 1024);
    let resource_job = Job::create(&expected)?;
    if resource_job.limits()? != expected {
        return Err(io::Error::other("document worker resource readback failed"));
    }
    // This safe API retains the inner Job handle until this worker exits. The
    // parent's separate process-wrap Job still owns termination and observation.
    resource_job.assign_current_process()
}

#[cfg(any(windows, test))]
async fn exchange(
    format: DocumentFormat,
    mut stdin: impl tokio::io::AsyncWrite + Unpin,
    mut stdout: impl tokio::io::AsyncRead + Unpin,
    bytes: Vec<u8>,
) -> Result<Option<ExtractedDocumentText>, WorkerFailure> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut ready = [0; READY.len()];
    stdout
        .read_exact(&mut ready)
        .await
        .map_err(|_| WorkerFailure::InvalidOutput)?;
    if ready != READY {
        return Err(WorkerFailure::InvalidOutput);
    }
    // The READY check is the only gate to transmitting untrusted document bytes.
    let format_code = match format {
        DocumentFormat::Docx => 0,
        DocumentFormat::Pdf => 1,
        DocumentFormat::Xlsx => 2,
    };
    stdin
        .write_all(&[format_code])
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    stdin
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .await
        .map_err(|_| WorkerFailure::Unavailable)?;
    stdin
        .write_all(&bytes)
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
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(WorkerFailure::InvalidOutput);
    }
    let mut json = vec![0; length];
    stdout
        .read_exact(&mut json)
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
    let result: ExtractedDocumentText =
        serde_json::from_slice(&json).map_err(|_| WorkerFailure::InvalidOutput)?;
    validate_document_extraction(&result).map_err(|_| WorkerFailure::InvalidOutput)?;
    if result.format != format {
        return Err(WorkerFailure::InvalidOutput);
    }
    Ok(Some(result))
}

#[cfg(any(windows, test))]
async fn drain_stderr(
    mut reader: impl tokio::io::AsyncRead + Unpin,
) -> Result<Option<ExtractedDocumentText>, WorkerFailure> {
    use tokio::io::AsyncReadExt;
    let mut used = 0usize;
    let mut buffer = [0; 512];
    loop {
        let length = reader
            .read(&mut buffer)
            .await
            .map_err(|_| WorkerFailure::Unavailable)?;
        if length == 0 {
            return Ok(None);
        }
        used += length;
        if used > MAX_STDERR {
            return Err(WorkerFailure::InvalidOutput);
        }
    }
}

#[cfg(windows)]
mod physical {
    use super::*;
    use process_wrap::tokio::{ChildWrapper, CommandWrap, CreationFlags, JobObject, KillOnDrop};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::process::Command;
    use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
    use tokio::task::JoinHandle;
    use tokio_util::sync::CancellationToken;

    type Task = JoinHandle<Result<Option<ExtractedDocumentText>, WorkerFailure>>;

    struct Runtime {
        permit: Arc<Semaphore>,
        quarantine: Mutex<Vec<Resources>>,
    }
    fn runtime() -> Arc<Runtime> {
        static RUNTIME: OnceLock<Arc<Runtime>> = OnceLock::new();
        RUNTIME
            .get_or_init(|| {
                Arc::new(Runtime {
                    permit: Arc::new(Semaphore::new(1)),
                    quarantine: Mutex::new(Vec::new()),
                })
            })
            .clone()
    }
    pub(super) fn shared_physical_slot() -> Arc<Semaphore> {
        runtime().permit.clone()
    }
    struct Resources {
        _permit: OwnedSemaphorePermit,
        directory: TempDir,
        child: Option<Box<dyn ChildWrapper>>,
        io: Option<Task>,
        stderr: Option<Task>,
    }
    struct Lease {
        runtime: Arc<Runtime>,
        resources: Option<Resources>,
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            if let Some(resources) = self.resources.take() {
                self.runtime
                    .quarantine
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
    enum Program {
        Daemon,
        #[cfg(test)]
        Peer(std::path::PathBuf, &'static str),
    }

    pub(super) async fn extract(
        format: DocumentFormat,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        extract_with_program(format, bytes, cancel, deadline, Program::Daemon).await
    }

    async fn extract_with_program(
        format: DocumentFormat,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        deadline: Option<Instant>,
        program: Program,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        let runtime = runtime();
        let permit = runtime
            .permit
            .clone()
            .try_acquire_owned()
            .map_err(|_| WorkerFailure::Busy)?;
        // Covers spawn and every handshake/write/read phase, not just parsing.
        let deadline = deadline.map_or_else(
            || Instant::now() + Duration::from_secs(10),
            |d| d.min(Instant::now() + Duration::from_secs(10)),
        );
        let stop = CancellationToken::new();
        let _caller_drop = CallerDrop(stop.clone());
        let directory = tempfile::Builder::new()
            .prefix("anima-document-")
            .tempdir()
            .map_err(|_| WorkerFailure::Unavailable)?;
        let lease = Lease {
            runtime,
            resources: Some(Resources {
                _permit: permit,
                directory,
                child: None,
                io: None,
                stderr: None,
            }),
        };
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(owner(
            lease,
            (format, bytes),
            cancel,
            stop,
            deadline,
            sender,
            program,
        ));
        receiver.await.map_err(|_| WorkerFailure::Unavailable)?
    }

    fn launch(
        resources: &mut Resources,
        format: DocumentFormat,
        bytes: Vec<u8>,
        program: Program,
    ) -> Result<(), WorkerFailure> {
        let mut native = match program {
            Program::Daemon => {
                let executable = std::env::current_exe().map_err(|_| WorkerFailure::Unavailable)?;
                let mut command = Command::new(executable);
                command.arg("--document-worker-v2");
                command
            }
            #[cfg(test)]
            Program::Peer(executable, mode) => {
                let mut command = Command::new(executable);
                command.arg(mode);
                command
            }
        };
        native
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
        flags.0 .0 = 0x08000000; // CREATE_NO_WINDOW, maintained typed wrapper.
        command.wrap(flags).wrap(JobObject);
        // A failed spawn can conceal a partially created child. Keep its permit
        // quarantined rather than admitting a replacement without proof.
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
        resources.io = Some(tokio::spawn(exchange(format, stdin, stdout, bytes)));
        resources.stderr = Some(tokio::spawn(drain_stderr(stderr)));
        Ok(())
    }

    async fn owner(
        mut lease: Lease,
        request: (DocumentFormat, Vec<u8>),
        cancel: CancelSignal,
        stop: CancellationToken,
        deadline: Instant,
        sender: oneshot::Sender<Result<ExtractedDocumentText, WorkerFailure>>,
        program: Program,
    ) {
        let (format, bytes) = request;
        let mut sender = Some(sender);
        if cancel.is_cancelled() || stop.is_cancelled() || Instant::now() >= deadline {
            let failure = if Instant::now() >= deadline {
                WorkerFailure::TimedOut
            } else {
                WorkerFailure::Cancelled
            };
            let _ = sender.take().expect("response exists").send(Err(failure));
            lease.resources.take(); // No spawn attempted, so retirement is proven.
            return;
        }
        let resources = lease.resources.as_mut().expect("owner resources");
        if let Err(failure) = launch(resources, format, bytes, program) {
            let _ = sender.take().expect("response exists").send(Err(failure));
            // Failed containment/spawn is permanently quarantined, including
            // any owned child and pipe handles, even if cleanup later succeeds.
            if let Some(child) = resources.child.as_mut() {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            return;
        }
        let mut extraction = None;
        let failure = loop {
            if resources.io.is_none() && resources.stderr.is_none() {
                break None;
            }
            tokio::select! { biased;
                _ = cancel.cancelled() => break Some(WorkerFailure::Cancelled),
                _ = stop.cancelled() => break Some(WorkerFailure::Cancelled),
                _ = tokio::time::sleep_until(deadline) => break Some(WorkerFailure::TimedOut),
                result = async { resources.io.as_mut().expect("guarded io").await }, if resources.io.is_some() => {
                    resources.io.take();
                    match result { Ok(Ok(Some(value))) => extraction = Some(value), Ok(Err(e)) => break Some(e), _ => break Some(WorkerFailure::Unavailable) }
                }
                result = async { resources.stderr.as_mut().expect("guarded stderr").await }, if resources.stderr.is_some() => {
                    resources.stderr.take();
                    match result { Ok(Ok(None)) => {}, Ok(Err(e)) => break Some(e), _ => break Some(WorkerFailure::Unavailable) }
                }
            }
        };
        if let Some(failure) = failure {
            let _ = sender.take().expect("response exists").send(Err(failure));
        }
        let child = resources.child.as_mut().expect("owned child");
        // Wait for the native leader only as an exit trigger. Its wait is
        // cancellation-safe and does not consume the outer Job completion port.
        // Killing before this on a successful exchange would race normal exit
        // and turn a valid result into an artificial failure exit status.
        if sender.is_some() {
            let leader = tokio::select! { biased;
                _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
                _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
                _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
                status = child.inner_mut().wait() => status.map_err(|_| WorkerFailure::Unavailable).and_then(|status| if status.success() { Ok(()) } else { Err(WorkerFailure::Unavailable) }),
            };
            if let Err(failure) = leader {
                let _ = sender.take().expect("response exists").send(Err(failure));
            }
        }
        // Terminate the entire outer Job even after a valid result: a leader
        // closing stdout is not retirement. Never cancel/recreate this Job wait.
        let killed = child.start_kill();
        let waited = child.wait().await;
        if killed.is_err() || waited.is_err() {
            return;
        }
        for task in [&mut resources.io, &mut resources.stderr] {
            if let Some(handle) = task.as_mut() {
                // A pipe task is retained on timeout, never aborted or detached
                // while a replacement could acquire this physical slot.
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
                extraction.ok_or(WorkerFailure::InvalidOutput)
            };
            let _ = sender.send(result);
        }
        // Only this complete whole-Job + both-pipes proof releases the permit.
        lease.resources.take();
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn dropped_physical_owner_quarantines_the_slot_and_owned_directory() {
            let runtime = Arc::new(Runtime {
                permit: Arc::new(Semaphore::new(1)),
                quarantine: Mutex::new(Vec::new()),
            });
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().to_owned();
            let lease = Lease {
                runtime: runtime.clone(),
                resources: Some(Resources {
                    _permit: runtime.permit.clone().try_acquire_owned().unwrap(),
                    directory,
                    child: None,
                    io: None,
                    stderr: None,
                }),
            };
            drop(lease);
            assert!(runtime.permit.clone().try_acquire_owned().is_err());
            assert!(path.is_dir());
            assert_eq!(runtime.quarantine.lock().unwrap().len(), 1);
            // Synthetic lease has no process: explicit test cleanup is safe.
            runtime.quarantine.lock().unwrap().clear();
        }

        #[test]
        fn caller_drop_signals_only_the_private_stop() {
            let primary = CancelSignal::new();
            let stop = CancellationToken::new();
            drop(CallerDrop(stop.clone()));
            assert!(stop.is_cancelled());
            assert!(!primary.is_cancelled());
        }

        fn peer(mode: &'static str) -> Program {
            let executable = std::env::current_exe().unwrap();
            let target = executable.parent().unwrap().parent().unwrap();
            let path = target.join("examples").join("document_worker_peer.exe");
            assert!(
                path.is_file(),
                "root must build the document_worker_peer example first"
            );
            Program::Peer(path, mode)
        }

        async fn available(
            format: DocumentFormat,
            mode: &'static str,
            bytes: Vec<u8>,
        ) -> Result<ExtractedDocumentText, WorkerFailure> {
            // Finite wait for the previous physical owner to observe retirement.
            for _ in 0..100 {
                let result = extract_with_program(
                    format,
                    bytes.clone(),
                    CancelSignal::new(),
                    None,
                    peer(mode),
                )
                .await;
                if result != Err(WorkerFailure::Busy) {
                    return result;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("physical slot was not retired within two seconds");
        }

        #[tokio::test]
        #[ignore = "physical Windows Job/pipe acceptance: root builds the finite peer and runs explicitly"]
        async fn document_physical_framing_stop_timeout_drop_and_resources() {
            for format in [DocumentFormat::Docx, DocumentFormat::Pdf] {
                for mode in [
                    "bad-ready",
                    "old-ready",
                    "oversize",
                    "malformed",
                    "old-version",
                    "unknown-format",
                    "wrong-format",
                    "trailing",
                    "stderr-flood",
                    "crash",
                ] {
                    assert_eq!(
                        available(format, mode, Vec::new()).await,
                        Err(WorkerFailure::InvalidOutput),
                        "{mode}"
                    );
                }
                // An error may return while the independent owner is retiring the
                // Job. Prove the slot is reusable before starting a deadline case;
                // never equate a logical failure response with physical retirement.
                available(format, "valid", Vec::new()).await.unwrap();
                for mode in [
                    "before-ready",
                    "during-input",
                    "during-output",
                    "late",
                    "cpu-burn",
                ] {
                    // Empty handshake mode or a full input that fills a closed peer's pipe.
                    let bytes = if mode == "during-input" {
                        vec![0; MAX_INPUT]
                    } else {
                        Vec::new()
                    };
                    let result = extract_with_program(
                        format,
                        bytes,
                        CancelSignal::new(),
                        Some(Instant::now() + Duration::from_millis(300)),
                        peer(mode),
                    )
                    .await;
                    assert_eq!(result, Err(WorkerFailure::TimedOut), "{mode}");
                    assert_eq!(
                        available(format, "valid", Vec::new()).await.unwrap().status,
                        anima_service_adapters::document_content::DocumentExtractionStatus::NoText
                    );
                }
                for dropped in [false, true] {
                    for mode in ["before-ready", "during-input", "during-output"] {
                        let primary = CancelSignal::new();
                        let bytes = if mode == "during-input" {
                            vec![0; MAX_INPUT]
                        } else {
                            Vec::new()
                        };
                        let task = tokio::spawn(extract_with_program(
                            format,
                            bytes,
                            primary.clone(),
                            None,
                            peer(mode),
                        ));
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        assert_eq!(
                            DocumentWorker
                                .extract(format, Vec::new(), CancelSignal::new(), None)
                                .await,
                            Err(WorkerFailure::Busy),
                            "all extractors share the occupied physical slot"
                        );
                        if dropped {
                            task.abort();
                            assert!(task.await.unwrap_err().is_cancelled());
                            assert!(!primary.is_cancelled());
                        } else {
                            primary.cancel();
                            assert_eq!(
                                tokio::time::timeout(Duration::from_secs(1), task)
                                    .await
                                    .unwrap()
                                    .unwrap(),
                                Err(WorkerFailure::Cancelled)
                            );
                        }
                        available(format, "valid", Vec::new()).await.unwrap();
                    }
                }
                let resources = available(format, "resources", Vec::new()).await.unwrap();
                assert_eq!(resources.blocks[0].text, "memory-refused child-refused");
            }

            // An unrelated, finite source-controlled process is outside every
            // document Job. Stopping a worker must leave this exact handle live.
            let Program::Peer(path, _) = peer("neighbor") else {
                unreachable!()
            };
            let mut native = Command::new(path);
            native
                .arg("neighbor")
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            for name in ["SystemRoot", "WINDIR"] {
                if let Some(value) = std::env::var_os(name) {
                    native.env(name, value);
                }
            }
            let mut wrapped = CommandWrap::from(native);
            wrapped.wrap(KillOnDrop).wrap(JobObject);
            let mut neighbor = wrapped.spawn().unwrap();
            let primary = CancelSignal::new();
            let task = tokio::spawn(extract_with_program(
                DocumentFormat::Docx,
                Vec::new(),
                primary.clone(),
                None,
                peer("before-ready"),
            ));
            tokio::time::sleep(Duration::from_millis(100)).await;
            primary.cancel();
            assert_eq!(task.await.unwrap(), Err(WorkerFailure::Cancelled));
            assert!(matches!(neighbor.inner_mut().try_wait(), Ok(None)));
            neighbor.start_kill().unwrap();
            neighbor.wait().await.unwrap();
            available(DocumentFormat::Docx, "valid", Vec::new())
                .await
                .unwrap();

            let parsed = available(
                DocumentFormat::Docx,
                "real",
                include_bytes!("../../../tests/fixtures/documents/m54-selected.docx").to_vec(),
            )
            .await
            .unwrap();
            let text = parsed
                .blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("M54 selected record: cobalt harbour 7319."));
            assert!(!text.contains("HEADER_OMITTED_4096"));
            assert!(!text.contains("PHONETIC_GUIDE_NOT_BODY"));
            let parsed = available(
                DocumentFormat::Pdf,
                "real",
                include_bytes!("../../../tests/fixtures/documents/m55-selected.pdf").to_vec(),
            )
            .await
            .unwrap();
            assert_eq!(parsed.format, DocumentFormat::Pdf);
            assert!(parsed.blocks.iter().any(|block| block
                .text
                .contains("M55 selected invoice total: USD 137.42.")));
            let coverage = parsed.pdf_coverage.unwrap();
            assert_eq!(coverage.page_count, 2);
            assert_eq!(coverage.text_page_count, 1);
            assert_eq!(coverage.included_page_count, 1);
        }
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
