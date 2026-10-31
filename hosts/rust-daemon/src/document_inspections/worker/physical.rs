//! The owner retains the shared permit until the complete Job and pipes retire.
use super::{
    exchange, inspection_artifact_bytes, retire_never_sent, Output, Request, WorkerFailure,
    MAX_ARTIFACT_BYTES,
};
use anima_core::execution_limits::{ExecutionContext, ExecutionPermit, SettlementOutcome};
use anima_core::CancelSignal;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use win_custody::{Job, JobLimits, Launch, Process, WaitOutcome};

#[path = "physical/members.rs"]
mod members;
use members::Members;
#[path = "physical/directory.rs"]
mod directory;
use directory::PrivateDirectory;

type Task = JoinHandle<Result<Option<Output>, WorkerFailure>>;
struct Resources {
    _permit: OwnedSemaphorePermit,
    directory: PrivateDirectory,
    executable: PathBuf,
    execution: Option<ExecutionContext>,
    budget: Option<ExecutionPermit>,
    process_creation_attempted: bool,
    job: Option<Job>,
    child: Option<Process>,
    members: Members,
    io: Option<Task>,
    stderr: Option<Task>,
}
struct Lease(Option<Resources>);
impl Drop for Lease {
    fn drop(&mut self) {
        // Uncertain retirement must withhold admission even if a task unwinds.
        static QUARANTINE: OnceLock<Mutex<Vec<Resources>>> = OnceLock::new();
        if let Some(mut resources) = self.0.take() {
            // Retain the handles and admission after an uncertain exit, but always
            // request termination even when the owner future is dropped or unwinds.
            if let Some(job) = resources.job.as_ref() {
                let _ = job.terminate(1);
            }
            if let Some(child) = resources.child.as_ref() {
                let _ = child.terminate(1);
            }
            // Abandonment submits Unknown(false) to the admitted authority; it
            // retains the reserved artifact ceiling and never invents retirement.
            drop(resources.budget.take());
            QUARANTINE
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .expect("document inspection quarantine lock poisoned")
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
    execution: Option<ExecutionContext>,
    mut budget: Option<ExecutionPermit>,
    executable: Option<PathBuf>,
) -> Result<Output, WorkerFailure> {
    let deadline = deadline.min(Instant::now() + Duration::from_secs(60));
    let preflight = (|| {
        if cancel.is_cancelled() {
            return Err(WorkerFailure::Cancelled);
        }
        if let Some(failure) = execution_failure(execution.as_ref()) {
            return Err(failure);
        }
        if Instant::now() >= deadline {
            return Err(WorkerFailure::TimedOut);
        }
        let executable = executable
            .map_or_else(std::env::current_exe, Ok)
            .map_err(|_| WorkerFailure::Unavailable)?;
        let permit = slots.try_acquire_owned().map_err(|_| WorkerFailure::Busy)?;
        let directory = PrivateDirectory::plan().map_err(|_| WorkerFailure::Unavailable)?;
        Ok((permit, directory, executable))
    })();
    let (permit, directory, executable) = match preflight {
        Ok(resources) => resources,
        Err(failure) => {
            retire_never_sent(&mut budget)
                .await
                .map_err(|_| WorkerFailure::Unavailable)?;
            return Err(failure);
        }
    };
    let lease = Lease(Some(Resources {
        _permit: permit,
        directory,
        executable,
        execution,
        budget,
        process_creation_attempted: false,
        job: None,
        child: None,
        members: Members::default(),
        io: None,
        stderr: None,
    }));
    let stop = CancellationToken::new();
    let _caller_drop = CallerDrop(stop.clone());
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(owner(lease, request, bytes, cancel, stop, deadline, sender));
    receiver.await.map_err(|_| WorkerFailure::Unavailable)?
}

fn execution_failure(execution: Option<&ExecutionContext>) -> Option<WorkerFailure> {
    let execution = execution?;
    execution.check_live().err().map(|_| {
        if execution.clock.now_ms() >= execution.deadline_at_ms {
            WorkerFailure::TimedOut
        } else if execution.cancel.is_cancelled()
            || execution
                .secondary_cancel
                .as_ref()
                .is_some_and(CancelSignal::is_cancelled)
        {
            WorkerFailure::Cancelled
        } else {
            WorkerFailure::Unavailable
        }
    })
}

async fn execution_stopped(execution: Option<&ExecutionContext>) -> WorkerFailure {
    let Some(execution) = execution else {
        return std::future::pending().await;
    };
    let secondary = execution.secondary_cancel.clone().unwrap_or_default();
    tokio::select! { biased;
        () = execution.cancel.cancelled() => WorkerFailure::Cancelled,
        () = secondary.cancelled() => WorkerFailure::Cancelled,
        () = execution.clock.wait_until_ms(execution.deadline_at_ms) => WorkerFailure::TimedOut,
    }
}

async fn retire_artifact(
    resources: &mut Resources,
    artifact_bytes: u64,
) -> Result<(), WorkerFailure> {
    if let Some(permit) = resources.budget.as_mut() {
        permit
            .finish_owned(SettlementOutcome::Artifact { artifact_bytes }, true)
            .await
            .map_err(|_| WorkerFailure::Unavailable)?;
    }
    resources.budget.take();
    Ok(())
}

fn close_retired_resources(resources: &mut Resources) -> io::Result<()> {
    if resources.io.is_some() || resources.stderr.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "document inspection pipe tasks have not retired",
        ));
    }
    if resources.process_creation_attempted && resources.job.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "document inspection Job retirement is unproven",
        ));
    }
    if let Some(job) = resources.job.as_ref() {
        resources.members.verify_retired(job)?;
    }
    if let Some(child) = resources.child.as_ref() {
        if child.wait(Some(Duration::ZERO))? != WaitOutcome::Signaled {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "document inspection leader has not retired",
            ));
        }
    }
    // Kernel accounting and pipe retirement must precede handle closure. Close
    // every owned process/Job reference before checked Windows directory removal.
    drop(resources.child.take());
    resources.members.clear();
    drop(resources.job.take());
    resources.directory.close()
}

enum PipeDirection {
    Input,
    Output,
}

async fn private_pipe(direction: PipeDirection) -> Result<(File, NamedPipeServer), WorkerFailure> {
    let name = format!(
        r"\\.\pipe\anima-document-inspection-{}",
        uuid::Uuid::new_v4()
    );
    let input = matches!(direction, PipeDirection::Input);
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .reject_remote_clients(true)
        .access_inbound(!input)
        .access_outbound(input)
        .create(&name)
        .map_err(|_| WorkerFailure::Unavailable)?;
    // The owner opens both ends before launch; the child only inherits its
    // explicit synchronous standard handle, never a pipe name to connect to.
    let child = OpenOptions::new()
        .read(input)
        .write(!input)
        .open(&name)
        .map_err(|_| WorkerFailure::Unavailable)?;
    tokio::time::timeout(Duration::from_secs(2), server.connect())
        .await
        .map_err(|_| WorkerFailure::Unavailable)?
        .map_err(|_| WorkerFailure::Unavailable)?;
    Ok((child, server))
}

fn spawn_in_checked_job(
    resources: &mut Resources,
    command: &mut Launch<'_>,
    deadline: Instant,
) -> Result<(), WorkerFailure> {
    let expected = JobLimits::new()
        .kill_on_close(true)
        .active_processes(8)
        .process_memory(512 * 1024 * 1024)
        .job_memory(1024 * 1024 * 1024);
    resources.job = Some(Job::create(&expected).map_err(|_| WorkerFailure::Unavailable)?);
    let job = resources.job.as_ref().ok_or(WorkerFailure::Unavailable)?;
    if job.limits().map_err(|_| WorkerFailure::Unavailable)? != expected {
        return Err(WorkerFailure::Unavailable);
    }
    resources.process_creation_attempted = true;
    let child = command
        .spawn_suspended()
        .map_err(|_| WorkerFailure::Unavailable)?;
    job.assign(child.process())
        .map_err(|_| WorkerFailure::Unavailable)?;
    if job
        .active_process_count()
        .map_err(|_| WorkerFailure::Unavailable)?
        != 1
    {
        return Err(WorkerFailure::Unavailable);
    }
    // Capture the initial exact witness while it is still suspended. It cannot
    // execute or create a descendant before its identity enters our ledger.
    resources
        .members
        .capture(job)
        .map_err(|_| WorkerFailure::Unavailable)?;
    if Instant::now() >= deadline {
        return Err(WorkerFailure::TimedOut);
    }
    // The checked, non-breakaway, kill-on-close Job owns the initial runner
    // before its first instruction and all descendants it can ever create.
    resources.child = Some(child.resume().map_err(|_| WorkerFailure::Unavailable)?);
    Ok(())
}

async fn launch(
    resources: &mut Resources,
    request: Request,
    bytes: Vec<u8>,
    deadline: Instant,
) -> Result<(), WorkerFailure> {
    resources
        .directory
        .create_and_verify()
        .map_err(|_| WorkerFailure::Unavailable)?;
    if Instant::now() >= deadline {
        return Err(WorkerFailure::TimedOut);
    }
    if let Some(permit) = resources.budget.as_ref() {
        permit
            .revalidate()
            .await
            .map_err(|_| WorkerFailure::Unavailable)?;
    }
    let (child_stdin, stdin) = private_pipe(PipeDirection::Input).await?;
    let (child_stdout, stdout) = private_pipe(PipeDirection::Output).await?;
    let (child_stderr, stderr) = private_pipe(PipeDirection::Output).await?;
    let mut native = Launch::new(&resources.executable);
    native
        .arg("--document-inspection-worker-v1")
        .env_clear()
        .current_dir(resources.directory.path())
        .env("TEMP", resources.directory.path())
        .env("TMP", resources.directory.path())
        .env("USERPROFILE", resources.directory.path())
        .env("APPDATA", resources.directory.path())
        .env("LOCALAPPDATA", resources.directory.path())
        .stdin(&child_stdin)
        .stdout(&child_stdout)
        .stderr(&child_stderr);
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            native.env(name, value);
        }
    }
    // Font inventory has an explicit read-only purpose and is never an engine profile.
    if let Some(value) = std::env::var_os("LOCALAPPDATA") {
        native.env(
            "ANIMA_INSPECTION_USER_FONT_DIRECTORY",
            PathBuf::from(value).join("Microsoft/Windows/Fonts"),
        );
    }
    spawn_in_checked_job(resources, &mut native, deadline)?;
    drop(native);
    drop(child_stdin);
    drop(child_stdout);
    drop(child_stderr);
    resources.io = Some(tokio::spawn(exchange(request, bytes, stdin, stdout)));
    resources.stderr = Some(tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = stderr;
        let mut total = 0usize;
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

async fn wait_for_leader(
    resources: &mut Resources,
    deadline: Instant,
) -> Result<u32, WorkerFailure> {
    loop {
        let job = resources.job.as_ref().ok_or(WorkerFailure::Unavailable)?;
        resources
            .members
            .capture(job)
            .map_err(|_| WorkerFailure::Unavailable)?;
        let child = resources.child.as_ref().ok_or(WorkerFailure::Unavailable)?;
        if child
            .wait(Some(Duration::ZERO))
            .map_err(|_| WorkerFailure::Unavailable)?
            == WaitOutcome::Signaled
        {
            return child
                .exit_code()
                .map_err(|_| WorkerFailure::Unavailable)?
                .ok_or(WorkerFailure::Unavailable);
        }
        if Instant::now() >= deadline {
            return Err(WorkerFailure::TimedOut);
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(10))).await;
    }
}

async fn wait_for_job_empty(job: &Job, deadline: Instant) -> Result<(), WorkerFailure> {
    loop {
        // Zero is only accounting evidence. Cleanup separately requires exact
        // process signals and a complete cumulative lifetime witness count.
        if job
            .active_process_count()
            .map_err(|_| WorkerFailure::Unavailable)?
            == 0
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(WorkerFailure::TimedOut);
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(10))).await;
    }
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
    let execution = lease
        .0
        .as_ref()
        .and_then(|resources| resources.execution.clone());
    if cancel.is_cancelled()
        || stop.is_cancelled()
        || Instant::now() >= deadline
        || execution_failure(execution.as_ref()).is_some()
    {
        let failure = execution_failure(execution.as_ref()).unwrap_or_else(|| {
            if Instant::now() >= deadline {
                WorkerFailure::TimedOut
            } else {
                WorkerFailure::Cancelled
            }
        });
        let _ = sender
            .take()
            .expect("sender present before launch")
            .send(Err(failure));
        if let Some(resources) = lease.0.as_mut() {
            if retire_never_sent(&mut resources.budget).await.is_err() {
                return;
            }
            if close_retired_resources(resources).is_err() {
                return;
            }
        }
        lease.0.take();
        return;
    }
    let Some(resources) = lease.0.as_mut() else {
        return;
    };
    let launched = tokio::select! { biased;
        _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
        _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
        failure = execution_stopped(execution.as_ref()) => Err(failure),
        _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
        result = launch(resources, request, bytes, deadline) => result,
    };
    if let Err(failure) = launched {
        let _ = sender
            .take()
            .expect("sender present after launch")
            .send(Err(failure));
        // Job setup alone cannot create a child. Once process creation has
        // been attempted, a launch error cannot certify physical retirement.
        if !resources.process_creation_attempted {
            if retire_never_sent(&mut resources.budget).await.is_err() {
                return;
            }
            if close_retired_resources(resources).is_err() {
                return;
            }
            lease.0.take();
            return;
        }
        if let Some(job) = resources.job.as_ref() {
            let _ = job.terminate(1);
            let _ = wait_for_job_empty(job, Instant::now() + Duration::from_secs(2)).await;
        }
        return; // Ambiguous spawn/assignment remains quarantined with its permit.
    }
    let mut output = None;
    let mut observations = tokio::time::interval(Duration::from_millis(10));
    observations.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let failure = loop {
        if resources.io.is_none() && resources.stderr.is_none() {
            break None;
        }
        tokio::select! { biased;
            _ = cancel.cancelled() => break Some(WorkerFailure::Cancelled),
            _ = stop.cancelled() => break Some(WorkerFailure::Cancelled),
            failure = execution_stopped(execution.as_ref()) => break Some(failure),
            _ = tokio::time::sleep_until(deadline) => break Some(WorkerFailure::TimedOut),
            _ = observations.tick() => {
                let Some(job) = resources.job.as_ref() else { break Some(WorkerFailure::Unavailable); };
                if resources.members.capture(job).is_err() { break Some(WorkerFailure::Unavailable); }
            },
            result = async { resources.io.as_mut().expect("io presence guarded by select").await }, if resources.io.is_some() => {
                resources.io.take();
                match result { Ok(Ok(Some(value))) => output = Some(value), Ok(Err(failure)) => break Some(failure), _ => break Some(WorkerFailure::Unavailable) }
            },
            result = async { resources.stderr.as_mut().expect("stderr presence guarded by select").await }, if resources.stderr.is_some() => {
                resources.stderr.take();
                match result { Ok(Ok(None)) => {}, Ok(Err(failure)) => break Some(failure), _ => break Some(WorkerFailure::Unavailable) }
            },
        }
    };
    if let Some(failure) = failure {
        let _ = sender
            .take()
            .expect("sender present before first failure")
            .send(Err(failure));
    }
    let mut leader_succeeded = false;
    if sender.is_some() {
        let leader = tokio::select! { biased;
            _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
            _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
            failure = execution_stopped(execution.as_ref()) => Err(failure),
            _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
            status = wait_for_leader(resources, deadline) => status.and_then(|code| if code == 0 { Ok(()) } else { Err(WorkerFailure::Unavailable) }),
        };
        match leader {
            Ok(()) => leader_succeeded = true,
            Err(failure) => {
                let _ = sender
                    .take()
                    .expect("sender present before leader failure")
                    .send(Err(failure));
            }
        }
    }
    let Some(job) = resources.job.as_ref() else {
        return;
    };
    // Capture before issuing asynchronous termination; a racing or already
    // missed member cannot be hidden by the final cumulative-count comparison.
    let captured = resources.members.capture(job);
    if job.terminate(1).is_err() {
        return;
    }
    if captured.is_err() {
        return;
    }
    let retirement_deadline = Instant::now() + Duration::from_secs(2);
    // Post-termination enumeration can still collect a late member; count
    // completeness remains mandatory if it has already left the PID snapshot.
    if resources.members.capture(job).is_err() {
        return;
    }
    if resources
        .members
        .wait_for_signals(retirement_deadline)
        .await
        .is_err()
    {
        return;
    }
    if wait_for_job_empty(job, retirement_deadline).await.is_err() {
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
    // Physical retirement precedes accounting and all admission/directory release.
    let mut result = None;
    if let Some(sender) = sender.take() {
        let outcome = if let Some(failure) = execution_failure(execution.as_ref()) {
            Err(failure)
        } else if cancel.is_cancelled() || stop.is_cancelled() {
            Err(WorkerFailure::Cancelled)
        } else if Instant::now() >= deadline {
            Err(WorkerFailure::TimedOut)
        } else if !leader_succeeded {
            Err(WorkerFailure::Unavailable)
        } else {
            output.ok_or(WorkerFailure::InvalidOutput)
        };
        result = Some((sender, outcome));
    }
    let artifact_bytes = match result.as_ref().map(|(_, outcome)| outcome) {
        Some(Ok(Output::Inspected(output))) => match inspection_artifact_bytes(output) {
            Ok(bytes) => bytes,
            Err(_) => MAX_ARTIFACT_BYTES,
        },
        _ => MAX_ARTIFACT_BYTES,
    };
    if close_retired_resources(resources).is_err() {
        if let Some((sender, _)) = result {
            let _ = sender.send(Err(WorkerFailure::Unavailable));
        }
        return;
    }
    if let Err(failure) = retire_artifact(resources, artifact_bytes).await {
        if let Some((sender, _)) = result {
            let _ = sender.send(Err(failure));
        }
        return;
    }
    lease.0.take();
    if let Some((sender, outcome)) = result {
        let _ = sender.send(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    const FIXTURE_ROLE: &str = "ANIMA_INSPECTION_CUSTODY_TEST_ROLE";
    const FIXTURE_DIRECTORY: &str = "ANIMA_INSPECTION_CUSTODY_TEST_DIRECTORY";

    fn fixture_name() -> &'static str {
        concat!(module_path!(), "::custody_fixture_process")
            .split_once("::")
            .expect("test module includes its crate")
            .1
    }

    #[test]
    fn custody_fixture_process() {
        let Some(role) = std::env::var_os(FIXTURE_ROLE) else {
            return;
        };
        let directory =
            PathBuf::from(std::env::var_os(FIXTURE_DIRECTORY).expect("fixture directory"));
        if role == "descendant" {
            let staging = directory.join("descendant-pid-staging");
            std::fs::write(&staging, std::process::id().to_string()).unwrap();
            std::fs::rename(staging, directory.join("descendant-ready")).unwrap();
            loop {
                std::thread::park_timeout(Duration::from_secs(60));
            }
        }
        assert_eq!(role, "leader");
        let mut descendant = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", fixture_name(), "--nocapture"])
            .env(FIXTURE_ROLE, "descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !directory.join("descendant-ready").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "descendant readiness timed out"
            );
            assert!(
                descendant.try_wait().unwrap().is_none(),
                "descendant exited before readiness"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // Dropping a std Child deliberately leaves the descendant alive after this leader exits.
    }

    #[tokio::test]
    async fn private_pipe_delivers_bytes_and_eof_after_async_writer_closes() {
        use std::io::Read;
        use tokio::io::AsyncWriteExt;

        let (mut reader, mut writer) = private_pipe(PipeDirection::Input).await.unwrap();
        let reader = tokio::task::spawn_blocking(move || {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).unwrap();
            bytes
        });
        writer.write_all(b"private worker frame").await.unwrap();
        writer.shutdown().await.unwrap();
        drop(writer);
        let received = tokio::time::timeout(Duration::from_secs(2), reader)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, b"private worker frame");
    }

    #[tokio::test]
    async fn leader_exit_keeps_permit_and_directory_until_the_owned_job_is_empty() {
        let (execution, budget_port) =
            super::super::tests::test_execution(super::super::MAX_ARTIFACT_BYTES + 1);
        let scope_id = execution.scope_id.clone();
        let budget = execution
            .reserve_native_artifact(
                "hung-descendant-regression",
                super::super::MAX_ARTIFACT_BYTES,
            )
            .await
            .unwrap();
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().try_acquire_owned().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let directory_path = directory.path().to_owned();
        let mut lease = Lease(Some(Resources {
            _permit: permit,
            directory: PrivateDirectory::new(directory),
            executable: std::env::current_exe().unwrap(),
            execution: Some(execution),
            budget: Some(budget),
            process_creation_attempted: false,
            job: None,
            child: None,
            members: Members::default(),
            io: None,
            stderr: None,
        }));
        let resources = lease.0.as_mut().unwrap();
        let null = win_custody::null_device().unwrap();
        let mut command = Launch::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", fixture_name(), "--nocapture"])
            .env(FIXTURE_ROLE, "leader")
            .env(FIXTURE_DIRECTORY, &directory_path)
            .current_dir(&directory_path)
            .stdin(&null)
            .stdout(&null)
            .stderr(&null);
        spawn_in_checked_job(
            resources,
            &mut command,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let leader_pid = resources.child.as_ref().unwrap().pid().unwrap();
        assert_eq!(
            wait_for_leader(resources, Instant::now() + Duration::from_secs(10))
                .await
                .unwrap(),
            0
        );
        let descendant_pid: u32 = std::fs::read_to_string(directory_path.join("descendant-ready"))
            .unwrap()
            .parse()
            .unwrap();
        resources
            .members
            .capture(resources.job.as_ref().unwrap())
            .unwrap();
        assert_ne!(leader_pid, descendant_pid);
        assert!(
            resources.members.contains_pid(leader_pid),
            "retain the exact leader witness"
        );
        assert!(
            resources.members.contains_pid(descendant_pid),
            "retain the exact descendant witness"
        );
        assert!(
            resources.members.len() >= 2,
            "retain every captured Job member"
        );
        let job = resources.job.as_ref().unwrap();
        assert!(
            job.active_process_count().unwrap() > 0,
            "leader exit must leave the hung descendant accounted"
        );
        assert!(
            wait_for_job_empty(job, Instant::now() + Duration::from_millis(50))
                .await
                .is_err()
        );
        close_retired_resources(resources)
            .expect_err("active descendants must prevent checked cleanup");
        assert!(
            slots.clone().try_acquire_owned().is_err(),
            "leader exit must not release physical admission"
        );
        assert!(
            directory_path.exists(),
            "active descendants must retain the private directory"
        );
        let scope = budget_port.snapshot(&scope_id).unwrap();
        assert!(
            scope
                .reservations
                .values()
                .next()
                .unwrap()
                .settlement
                .is_none(),
            "leader exit must not settle or refund the retained shared artifact reservation"
        );
        assert_eq!(
            scope.consumption().unwrap().charged_artifact_bytes,
            super::super::MAX_ARTIFACT_BYTES
        );
        assert_eq!(scope.consumption().unwrap().inflight_operations, 1);
        let job = resources.job.as_ref().unwrap();
        job.terminate(1).unwrap();
        resources
            .members
            .wait_for_signals(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        wait_for_job_empty(job, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(job.active_process_count().unwrap(), 0);
        assert_eq!(
            job.process_counts().unwrap().total as usize,
            resources.members.len(),
            "every lifetime Job member must have an exact retained witness"
        );
        resources.members.verify_retired(job).unwrap();
        let omitted = resources
            .members
            .remove_for_test(descendant_pid)
            .expect("captured exact descendant");
        close_retired_resources(resources)
            .expect_err("Job zero cannot certify a missed lifetime member");
        assert!(
            slots.clone().try_acquire_owned().is_err(),
            "incomplete lifetime coverage retains admission"
        );
        assert!(
            directory_path.exists(),
            "incomplete lifetime coverage must not attempt directory removal"
        );
        assert!(budget_port
            .snapshot(&scope_id)
            .unwrap()
            .reservations
            .values()
            .next()
            .unwrap()
            .settlement
            .is_none());
        resources.members.restore_for_test(omitted);
        close_retired_resources(resources).unwrap();
        retire_artifact(resources, super::super::MAX_ARTIFACT_BYTES)
            .await
            .unwrap();
        let scope = budget_port.snapshot(&scope_id).unwrap();
        let settlement = scope
            .reservations
            .values()
            .next()
            .unwrap()
            .settlement
            .as_ref()
            .unwrap();
        assert_eq!(
            settlement.outcome,
            SettlementOutcome::Artifact {
                artifact_bytes: super::super::MAX_ARTIFACT_BYTES
            }
        );
        assert!(settlement.physical_retired);
        assert_eq!(
            scope.consumption().unwrap().charged_artifact_bytes,
            super::super::MAX_ARTIFACT_BYTES
        );
        assert_eq!(scope.consumption().unwrap().inflight_operations, 0);
        lease.0.take();
        assert!(
            slots.try_acquire_owned().is_ok(),
            "complete process signals and lifetime coverage permit admission again"
        );
        assert!(
            !directory_path.exists(),
            "private directory retires only after complete process retirement"
        );
    }

    #[test]
    fn directory_cleanup_failure_retains_admission_until_checked_removal_succeeds() {
        use std::os::windows::fs::OpenOptionsExt;

        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().try_acquire_owned().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_owned();
        let locked = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(path.join("locked-output"))
            .unwrap();
        let mut lease = Lease(Some(Resources {
            _permit: permit,
            directory: PrivateDirectory::new(directory),
            executable: std::env::current_exe().unwrap(),
            execution: None,
            budget: None,
            process_creation_attempted: false,
            job: None,
            child: None,
            members: Members::default(),
            io: None,
            stderr: None,
        }));
        let resources = lease.0.as_mut().unwrap();
        close_retired_resources(resources)
            .expect_err("an exclusive Windows handle must block checked cleanup");
        assert!(
            slots.clone().try_acquire_owned().is_err(),
            "failed cleanup must retain physical admission"
        );
        assert!(
            path.exists(),
            "failed cleanup retains the private directory path"
        );
        drop(locked);
        // Remove only the two exact fixture paths after their known owner closes;
        // production cleanup never retries a path after TempDir::close failed.
        std::fs::remove_file(path.join("locked-output")).unwrap();
        std::fs::remove_dir(&path).unwrap();
        close_retired_resources(resources).unwrap();
        lease.0.take();
        assert!(slots.try_acquire_owned().is_ok());
        assert!(!path.exists());
    }
}
