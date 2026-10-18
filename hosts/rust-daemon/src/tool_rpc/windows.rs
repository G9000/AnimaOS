//! Windows Job custody only. Arbitrary Python retains the host user's rights.
use super::{
    service::CellResources,
    worker::{LaunchSpec, OwnedWorker, WorkerFailure, WorkerLauncher, WorkerPipes},
};
use async_trait::async_trait;
use std::sync::Mutex;

pub(super) struct CellLauncher {
    pending: Mutex<Option<CellResources>>,
}
impl CellLauncher {
    pub(super) fn new(cell: CellResources) -> Self {
        Self {
            pending: Mutex::new(Some(cell)),
        }
    }
    pub(super) fn retire_pending(&self) -> Option<CellResources> {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }
    #[cfg(windows)]
    fn prepare_windows(&self, spec: LaunchSpec) -> Result<physical::WindowsWorker, WorkerFailure> {
        let cell = self.retire_pending().ok_or(WorkerFailure::Unavailable)?;
        Ok(physical::WindowsWorker::new(spec, cell))
    }
}
impl Drop for CellLauncher {
    fn drop(&mut self) {
        if let Some(cell) = self
            .pending
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            cell.retire_later();
        }
    }
}
#[async_trait]
impl WorkerLauncher for CellLauncher {
    async fn prepare(&self, spec: LaunchSpec) -> Result<Box<dyn OwnedWorker>, WorkerFailure> {
        #[cfg(windows)]
        {
            Ok(Box::new(self.prepare_windows(spec)?))
        }
        #[cfg(not(windows))]
        {
            let _ = spec;
            Err(WorkerFailure::Unavailable)
        }
    }
}

#[cfg(windows)]
mod physical {
    use super::*;
    use process_wrap::tokio::{ChildWrapper, CommandWrap, CreationFlags, JobObject, KillOnDrop};
    use std::process::Stdio;
    use tokio::process::Command;

    pub(super) struct WindowsWorker {
        command: Option<CommandWrap>,
        child: Option<Box<dyn ChildWrapper>>,
        cell: CellResources,
        tree_wait_started: bool,
    }
    impl WindowsWorker {
        #[cfg(test)]
        pub(super) fn prepared_unstarted(&self) -> bool {
            self.command.is_some() && self.child.is_none()
        }
        #[cfg(test)]
        pub(super) fn listener_joined(&self) -> bool {
            self.cell.transport.retirement_joined()
        }
        pub(super) fn new(spec: LaunchSpec, cell: CellResources) -> Self {
            let mut native = Command::new(&spec.interpreter);
            native
                .args(spec.args)
                .env_clear()
                .envs(spec.environment)
                .current_dir(spec.workspace)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut command = CommandWrap::from(native);
            command.wrap(KillOnDrop);
            let mut flags = CreationFlags(Default::default());
            flags.0 .0 = 0x08000000; // CREATE_NO_WINDOW; JobObject adds suspended assignment.
            command.wrap(flags).wrap(JobObject);
            Self {
                command: Some(command),
                child: None,
                cell,
                tree_wait_started: false,
            }
        }
    }
    #[async_trait]
    impl OwnedWorker for WindowsWorker {
        async fn start(&mut self) -> Result<(), WorkerFailure> {
            let mut command = self.command.take().ok_or(WorkerFailure::Launch)?;
            self.child = Some(command.spawn().map_err(|_| WorkerFailure::Launch)?);
            let child = self.child.as_ref().unwrap();
            if child.as_ref().type_id()
                != std::any::TypeId::of::<process_wrap::tokio::JobObjectChild>()
                || child.inner().type_id() != std::any::TypeId::of::<tokio::process::Child>()
            {
                return Err(WorkerFailure::Launch);
            }
            Ok(())
        }
        fn take_pipes(&mut self) -> Result<WorkerPipes, WorkerFailure> {
            let child = self.child.as_mut().ok_or(WorkerFailure::Launch)?;
            Ok(WorkerPipes {
                stdin: Box::new(child.stdin().take().ok_or(WorkerFailure::Stdin)?),
                stdout: Box::new(child.stdout().take().ok_or(WorkerFailure::InvalidFrame)?),
                stderr: Box::new(child.stderr().take().ok_or(WorkerFailure::InvalidFrame)?),
            })
        }
        fn close_admission(&mut self) {
            self.cell.transport.close();
        }
        async fn wait_leader(&mut self) -> Result<i32, WorkerFailure> {
            // The raw leader wait is cancellation safe; never poll the outer
            // JobObject wait until the uncancelled retirement phase.
            let child = self.child.as_mut().ok_or(WorkerFailure::Launch)?;
            child
                .inner_mut()
                .wait()
                .await
                .map(|status| status.code().unwrap_or(-1))
                .map_err(|_| WorkerFailure::Launch)
        }
        async fn terminate_tree(&mut self) -> Result<(), WorkerFailure> {
            match self.child.as_mut() {
                Some(child) => child
                    .start_kill()
                    .map_err(|_| WorkerFailure::CleanupUnknown),
                // A retained command proves start was never polled. Once start
                // takes it, missing child alone never proves no partial spawn.
                None if self.command.is_some() => Ok(()),
                None => Err(WorkerFailure::CleanupUnknown),
            }
        }
        async fn wait_tree(&mut self) -> Result<(), WorkerFailure> {
            if self.tree_wait_started {
                return Err(WorkerFailure::CleanupUnknown);
            }
            self.tree_wait_started = true;
            let result = match self.child.as_mut() {
                Some(child) => child
                    .wait()
                    .await
                    .map(|_| ())
                    .map_err(|_| WorkerFailure::CleanupUnknown),
                None if self.command.is_some() => Ok(()),
                None => Err(WorkerFailure::CleanupUnknown),
            };
            // Even after a native wait error, retain settlement of an admitted
            // broker future. No timeout/abort may discard durable native saves.
            let transport = self.cell.transport.retire().await;
            result?;
            transport.map_err(|_| WorkerFailure::CleanupUnknown)
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::tool_rpc::{
        service::CellLeases,
        transport::{Dispatch, Transport},
        worker::{WorkerBootstrap, WorkerConfig, WorkerRuntime},
        BrokerReply,
    };
    use anima_core::{tool_rpc::ProgramRequest, CancelSignal};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::time::Instant;

    struct NeverDispatch;
    #[async_trait]
    impl Dispatch for NeverDispatch {
        async fn dispatch(&mut self, _bytes: &[u8]) -> BrokerReply {
            panic!("an unstarted interpreter cannot make an RPC request")
        }
    }
    #[derive(Default)]
    struct Trace {
        prepared: AtomicBool,
        starts: AtomicUsize,
        waits: AtomicUsize,
        joined: AtomicBool,
    }
    struct CancelPrepared {
        launcher: CellLauncher,
        cancel: CancelSignal,
        trace: Arc<Trace>,
    }
    struct Observed {
        inner: physical::WindowsWorker,
        trace: Arc<Trace>,
    }
    #[async_trait]
    impl WorkerLauncher for CancelPrepared {
        async fn prepare(&self, spec: LaunchSpec) -> Result<Box<dyn OwnedWorker>, WorkerFailure> {
            let inner = self.launcher.prepare_windows(spec)?;
            self.trace
                .prepared
                .store(inner.prepared_unstarted(), Ordering::SeqCst);
            self.cancel.cancel();
            Ok(Box::new(Observed {
                inner,
                trace: self.trace.clone(),
            }))
        }
    }
    #[async_trait]
    impl OwnedWorker for Observed {
        async fn start(&mut self) -> Result<(), WorkerFailure> {
            self.trace.starts.fetch_add(1, Ordering::SeqCst);
            self.inner.start().await
        }
        fn take_pipes(&mut self) -> Result<WorkerPipes, WorkerFailure> {
            self.inner.take_pipes()
        }
        fn close_admission(&mut self) {
            self.inner.close_admission();
        }
        async fn wait_leader(&mut self) -> Result<i32, WorkerFailure> {
            self.inner.wait_leader().await
        }
        async fn terminate_tree(&mut self) -> Result<(), WorkerFailure> {
            self.inner.terminate_tree().await
        }
        async fn wait_tree(&mut self) -> Result<(), WorkerFailure> {
            self.trace.waits.fetch_add(1, Ordering::SeqCst);
            let result = self.inner.wait_tree().await;
            self.trace
                .joined
                .store(self.inner.listener_joined(), Ordering::SeqCst);
            result
        }
    }

    #[tokio::test]
    async fn fp06_windows_prepared_cancellation_retires_listener_and_reuses_exact_capacity() {
        let directory = tempfile::tempdir().unwrap();
        let interpreter = directory.path().join("nonexecuted-python.exe");
        std::fs::write(&interpreter, b"never launched").unwrap();
        let cells = CellLeases::new();
        let lease = cells.acquire("prepared-agent", "prepared-session").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let transport = Transport::bind(Box::new(NeverDispatch), deadline)
            .await
            .unwrap();
        let endpoint = transport.endpoint;
        let cancel = CancelSignal::new();
        let trace = Arc::new(Trace::default());
        let launcher = Arc::new(CancelPrepared {
            launcher: CellLauncher::new(CellResources { lease, transport }),
            cancel: cancel.clone(),
            trace: trace.clone(),
        });
        let runtime = WorkerRuntime::new();
        let result = runtime
            .run(
                WorkerConfig {
                    interpreter: Some(interpreter),
                    workspace: directory.path().to_owned(),
                },
                ProgramRequest {
                    code: "print(1)".into(),
                    reset: false,
                },
                WorkerBootstrap {
                    endpoint,
                    token: "unissued-private-token".into(),
                    cell_epoch: "prepared-epoch".into(),
                },
                cancel,
                deadline,
                launcher,
            )
            .await;
        let reusable = cells
            .acquire("prepared-agent", "prepared-session")
            .is_some();
        assert!(
            trace.prepared.load(Ordering::SeqCst),
            "actual command blueprint retained; spawn never attempted"
        );
        assert_eq!(trace.starts.load(Ordering::SeqCst), 0);
        assert_eq!(result, Err(WorkerFailure::Cancelled));
        assert_eq!(trace.waits.load(Ordering::SeqCst), 1);
        assert!(trace.joined.load(Ordering::SeqCst));
        assert_eq!(runtime.available_slots(), 1);
        assert!(
            reusable,
            "exact agent/session lease must be reusable after proven no-spawn retirement"
        );
    }
}
