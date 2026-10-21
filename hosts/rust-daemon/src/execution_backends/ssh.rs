use super::{
    config::SshProfile,
    process,
    worker::{self, WorkerRequest, WorkerResponse},
    ExecutionReceipt, ExecutionRequest,
};
use anima_core::CancelSignal;
use anima_process::Custody;
use process_wrap::tokio::CommandWrap;
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) fn arguments(profile: &SshProfile) -> Vec<String> {
    vec![
        "-F".into(),
        if cfg!(windows) {
            "NUL".into()
        } else {
            "/dev/null".into()
        },
        "-T".into(),
        "-oBatchMode=yes".into(),
        "-oIdentitiesOnly=yes".into(),
        "-oIdentityAgent=none".into(),
        "-oForwardAgent=no".into(),
        "-oClearAllForwardings=yes".into(),
        "-oPermitLocalCommand=no".into(),
        "-oStrictHostKeyChecking=yes".into(),
        "-oUpdateHostKeys=no".into(),
        "-oGlobalKnownHostsFile=none".into(),
        format!(
            "-oUserKnownHostsFile={}",
            profile.known_hosts_file.display()
        ),
        "-oConnectTimeout=5".into(),
        "-oConnectionAttempts=1".into(),
        "-oServerAliveInterval=2".into(),
        "-oServerAliveCountMax=2".into(),
        "-i".into(),
        profile.identity_file.to_string_lossy().into_owned(),
        "-p".into(),
        profile.port.to_string(),
        "-l".into(),
        profile.user.clone(),
        profile.host.clone(),
        format!("{} --execution-worker-v1", profile.worker_entrypoint),
    ]
}

async fn frame(reader: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<WorkerResponse, String> {
    let size = reader
        .read_u32()
        .await
        .map_err(|_| "SSH worker connection ended without terminal evidence")?
        as usize;
    if size == 0 || size > worker::MAX_FRAME_BYTES {
        return Err("SSH worker frame is invalid".into());
    }
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| "SSH worker response was truncated")?;
    serde_json::from_slice(&bytes).map_err(|_| "SSH worker response is invalid".into())
}
async fn exchange(
    profile: &SshProfile,
    request: &WorkerRequest,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    timeout_ms: u64,
    scope: Option<&super::ExecutionScope>,
) -> Result<WorkerResponse, (String, bool, bool)> {
    let (mut custody, mut native) = Custody::prepare(&profile.executable, arguments(profile));
    native
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::clean_environment(&mut native);
    let mut wrapped = CommandWrap::from(native);
    custody.wrap(&mut wrapped);
    let mut child = wrapped.spawn().map_err(|_| {
        (
            "Configured SSH transport could not start".into(),
            false,
            false,
        )
    })?;
    custody.attach(child.as_ref());
    let mut stdin = child
        .stdin()
        .take()
        .ok_or(("SSH input unavailable".into(), false, false))?;
    let mut stdout =
        child
            .stdout()
            .take()
            .ok_or(("SSH output unavailable".into(), false, false))?;
    let errors = child
        .stderr()
        .take()
        .map(|stderr| tokio::spawn(process::capture(stderr)));
    let mut request_bytes = Vec::new();
    worker::write_frame(&mut request_bytes, request)
        .map_err(|_| ("SSH request is too large".into(), false, false))?;
    if let Some(scope) = scope {
        if let Err(error) = scope.revalidate().await {
            drop(stdin);
            let cleanup = custody.retire(&mut child).await;
            return Err((error, cleanup, false));
        }
    }
    if call.is_cancelled() || cancel.is_some_and(CancelSignal::is_cancelled) {
        drop(stdin);
        let cleanup = custody.retire(&mut child).await;
        return Err((
            "Execution stopped before SSH worker dispatch".into(),
            cleanup,
            false,
        ));
    }
    let sent = tokio::time::timeout(Duration::from_secs(10), stdin.write_all(&request_bytes)).await;
    if !matches!(sent, Ok(Ok(()))) {
        drop(stdin);
        let retired = custody.retire(&mut child).await;
        return Err((
            "SSH connection lost during dispatch; outcome is unknown".into(),
            retired,
            true,
        ));
    }
    let is_run = matches!(request, WorkerRequest::Run { .. });
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    let mut grace = None;
    let response = loop {
        let stopped = call.is_cancelled()
            || cancel.is_some_and(CancelSignal::is_cancelled)
            || tokio::time::Instant::now() >= deadline;
        if stopped && grace.is_none() {
            let _ = stdin.shutdown().await;
            grace = Some(tokio::time::Instant::now() + Duration::from_secs(65));
        }
        if grace.is_some_and(|at| tokio::time::Instant::now() >= at) {
            break Err("SSH worker terminal evidence did not arrive after cancellation".into());
        }
        // A partial frame must stay attached to its future across polling.
        let read = frame(&mut stdout);
        tokio::pin!(read);
        let next = loop {
            tokio::select! {result=&mut read=>break Some(result),_=tokio::time::sleep(Duration::from_millis(20))=>{if (call.is_cancelled()||cancel.is_some_and(CancelSignal::is_cancelled)||tokio::time::Instant::now()>=deadline)&&grace.is_none(){let _=stdin.shutdown().await;grace=Some(tokio::time::Instant::now()+Duration::from_secs(if is_run{65}else{0}));}if grace.is_some_and(|at|tokio::time::Instant::now()>=at){break None}}}
        };
        match next {
            Some(Ok(WorkerResponse::Accepted { .. })) if is_run => continue,
            Some(result) => break result,
            None => break Err("SSH worker terminal evidence unavailable".into()),
        }
    };
    drop(stdin);
    let retired = custody.retire(&mut child).await;
    if let Some(errors) = errors {
        let _ = tokio::time::timeout(Duration::from_secs(1), errors).await;
    }
    if !retired {
        return Err(("SSH transport cleanup is unproven".into(), false, true));
    }
    response.map_err(|message| (message, true, true))
}
pub(super) struct PendingSsh {
    profile: SshProfile,
    key: String,
    transport_cleanup_confirmed: bool,
    pub receipt: ExecutionReceipt,
}
impl PendingSsh {
    fn terminal(&self, response: WorkerResponse) -> Option<ExecutionReceipt> {
        let WorkerResponse::Terminal { receipt, proof } = response else {
            return None;
        };
        if receipt.execution_id == self.receipt.execution_id
            && receipt.target_id == self.receipt.target_id
            && receipt.backend == "ssh"
            && receipt.cleanup_confirmed
            && self.transport_cleanup_confirmed
            && worker::verify(&receipt, &self.key, &proof)
        {
            Some(receipt)
        } else {
            None
        }
    }
    pub(super) async fn reconcile(&mut self) -> Option<ExecutionReceipt> {
        let request = WorkerRequest::Status {
            version: 1,
            execution_id: self.receipt.execution_id.clone(),
        };
        let response = match exchange(
            &self.profile,
            &request,
            None,
            &CancelSignal::new(),
            10_000,
            None,
        )
        .await
        {
            Ok(response) => response,
            Err((_, cleanup, _)) => {
                self.transport_cleanup_confirmed &= cleanup;
                return None;
            }
        };
        self.terminal(response)
    }
}
pub(super) async fn run(
    profile: SshProfile,
    request: &ExecutionRequest,
    id: &str,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    scope: &super::ExecutionScope,
) -> PendingSsh {
    let key = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let wire = WorkerRequest::Run {
        version: 1,
        execution_id: id.into(),
        command: request.command.clone(),
        cwd: request.cwd.clone(),
        timeout_ms: request.timeout_ms,
        target_id: request.target_id.clone(),
        receipt_key: key.clone(),
    };
    let mut pending = PendingSsh {
        profile,
        key,
        transport_cleanup_confirmed: true,
        receipt: ExecutionReceipt::unknown(
            id,
            &request.target_id,
            "ssh",
            "SSH dispatch has no authoritative terminal worker receipt; outcomeUnknown",
        ),
    };
    match exchange(
        &pending.profile,
        &wire,
        cancel,
        call,
        request.timeout_ms.saturating_add(10_000),
        Some(scope),
    )
    .await
    {
        Ok(response) => {
            if let Some(receipt) = pending.terminal(response) {
                pending.receipt = receipt
            }
        }
        Err((message, cleanup, dispatched)) => {
            pending.transport_cleanup_confirmed &= cleanup;
            pending.receipt.message = message;
            if !dispatched && cleanup {
                pending.receipt.cleanup_confirmed = true;
                pending.receipt.status = super::ExecutionStatus::Failed;
            }
        }
    }
    pending
}
