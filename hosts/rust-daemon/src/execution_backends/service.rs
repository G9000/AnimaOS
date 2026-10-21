use super::*;
use anima_core::CancelSignal;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::Semaphore;

struct Target {
    profile: ExecutionProfile,
    capacity: Arc<Semaphore>,
}
// Dropping or panicking the owner can never silently advertise free capacity.
struct PhysicalPermit {
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    settled: bool,
    service: ExecutionService,
    execution_id: String,
}
impl PhysicalPermit {
    fn settle(&mut self) {
        self.settled = true;
        drop(self.permit.take());
        self.service
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.execution_id);
        self.service.settled.notify_waiters();
    }
}
impl Drop for PhysicalPermit {
    fn drop(&mut self) {
        if !self.settled {
            if let Some(permit) = self.permit.take() {
                permit.forget();
            }
        }
    }
}
#[derive(Clone, Default)]
pub(crate) struct ExecutionService {
    targets: Arc<BTreeMap<String, Arc<Target>>>,
    receipts: Arc<Mutex<BTreeMap<String, OwnedReceipt>>>,
    active: Arc<Mutex<BTreeMap<String, CancelSignal>>>,
    shutting_down: Arc<AtomicBool>,
    settled: Arc<tokio::sync::Notify>,
}
#[derive(Clone)]
struct OwnedReceipt {
    agent_id: String,
    receipt: ExecutionReceipt,
}
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .thread_name("anima-execution-owner")
            .enable_all()
            .build()
            .expect("execution owner runtime")
    })
}
struct CallCancellation(CancelSignal);
impl Drop for CallCancellation {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl ExecutionService {
    pub(crate) fn in_flight_count(&self) -> usize {
        self.active.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
    pub(crate) fn request_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        for target in self.targets.values() {
            target.capacity.close();
        }
        for cancel in self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            cancel.cancel();
        }
    }
    pub(crate) async fn shutdown(&self) {
        self.request_shutdown();
        loop {
            let settled = self.settled.notified();
            tokio::pin!(settled);
            settled.as_mut().enable();
            if self.in_flight_count() == 0 {
                return;
            }
            settled.await;
        }
    }
    pub(crate) fn from_environment(workspace_root: Option<&Path>) -> Result<Self, String> {
        let Some(path) = std::env::var_os(config::CONFIG_ENV) else {
            return Ok(Self::default());
        };
        let path = std::path::PathBuf::from(path);
        if !config::absolute_file(&path) {
            return Err(
                "ANIMAOS_EXECUTION_PROFILES_FILE must be an existing absolute operator-owned file"
                    .into(),
            );
        }
        let canonical =
            std::fs::canonicalize(&path).map_err(|_| "Execution profiles file is unavailable")?;
        if workspace_root
            .and_then(|r| std::fs::canonicalize(r).ok())
            .is_some_and(|root| canonical.starts_with(root))
        {
            return Err("Execution profiles must be outside the model-writable workspace".into());
        }
        if std::fs::metadata(&canonical).map_or(true, |m| m.len() > 65_536) {
            return Err("Execution profiles exceed 64 KiB".into());
        }
        let service = Self::from_json(
            &std::fs::read_to_string(&canonical)
                .map_err(|_| "Execution profiles cannot be read")?,
        )?;
        if let Some(root) = workspace_root.and_then(|r| std::fs::canonicalize(r).ok()) {
            for target in service.targets.values() {
                let paths: Vec<&Path> = match &target.profile.backend {
                    Backend::Local(p) => vec![&p.executable],
                    Backend::Docker(p) => vec![&p.executable],
                    Backend::Ssh(p) => vec![&p.executable, &p.identity_file, &p.known_hosts_file],
                };
                for path in paths {
                    if std::fs::canonicalize(path).map_or(true, |path| path.starts_with(&root)) {
                        return Err("Execution transport executables, SSH keys and pins must be outside the model-writable workspace".into());
                    }
                }
            }
        }
        Ok(service)
    }
    pub(crate) fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > 65_536 {
            return Err("Execution profiles exceed 64 KiB".into());
        }
        let config: config::Configuration =
            serde_json::from_str(json).map_err(|e| format!("Invalid execution profiles: {e}"))?;
        if config.version != 1 || config.targets.len() > 16 {
            return Err("Unsupported execution profile version or target count".into());
        }
        let mut targets = BTreeMap::new();
        for mut profile in config.targets {
            profile.validate()?;
            let id = profile.id.clone();
            if targets
                .insert(
                    id,
                    Arc::new(Target {
                        profile,
                        capacity: Arc::new(Semaphore::new(1)),
                    }),
                )
                .is_some()
            {
                return Err("Duplicate execution target ID".into());
            }
        }
        Ok(Self {
            targets: Arc::new(targets),
            ..Self::default()
        })
    }
    pub(crate) fn targets(&self) -> Vec<TargetSummary> {
        self.targets.values().map(|t| t.profile.summary()).collect()
    }
    #[cfg(test)]
    pub(crate) fn target(&self, id: &str) -> Result<TargetSummary, String> {
        self.targets
            .get(id)
            .map(|t| t.profile.summary())
            .ok_or_else(|| "Execution target is not configured; no fallback is permitted".into())
    }
    pub(crate) fn receipt_for_agent(&self, id: &str, agent_id: &str) -> Option<ExecutionReceipt> {
        self.receipts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .filter(|owned| owned.agent_id == agent_id)
            .map(|owned| owned.receipt.clone())
    }
    #[cfg(test)]
    pub(crate) fn receipt(&self, id: &str) -> Option<ExecutionReceipt> {
        self.receipt_for_agent(id, "test-owner")
    }
    fn publish(&self, receipt: ExecutionReceipt, agent_id: &str) {
        self.receipts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                receipt.execution_id.clone(),
                OwnedReceipt {
                    agent_id: agent_id.into(),
                    receipt,
                },
            );
    }
    #[cfg(test)]
    pub(crate) async fn execute(
        &self,
        request: ExecutionRequest,
        cancel: Option<CancelSignal>,
    ) -> Result<ExecutionReceipt, String> {
        self.execute_scoped(
            request,
            cancel,
            ExecutionScope {
                agent_id: "test-owner".into(),
                authority: Arc::new(|| Box::pin(async { Ok(()) })),
            },
        )
        .await
    }
    pub(crate) async fn execute_scoped(
        &self,
        request: ExecutionRequest,
        cancel: Option<CancelSignal>,
        scope: ExecutionScope,
    ) -> Result<ExecutionReceipt, String> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("Execution service is shutting down".into());
        }
        let target = self
            .targets
            .get(&request.target_id)
            .cloned()
            .ok_or("Execution target is not configured; no fallback is permitted")?;
        if request.command.trim().is_empty()
            || request.command.len() > 16_384
            || request.command.contains('\0')
            || request.cwd.len() > 1024
            || request.cwd.contains('\0')
            || request.timeout_ms == 0
            || request.timeout_ms > target.profile.max_timeout_ms
        {
            return Err(
                "Invalid execution command, cwd, or timeout for the selected target".into(),
            );
        }
        if let Backend::Local(profile) = &target.profile.backend {
            process::local_cwd(&profile.workspace_root, &request.cwd)?;
        }
        let permit = target
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Execution target has active or unreconciled physical work")?;
        {
            let mut receipts = self.receipts.lock().unwrap_or_else(|e| e.into_inner());
            if receipts.len() >= 256 {
                if let Some(id) = receipts
                    .iter()
                    .find(|(_, r)| r.receipt.cleanup_confirmed)
                    .map(|(id, _)| id.clone())
                {
                    receipts.remove(&id);
                } else {
                    return Err("Execution witness capacity is exhausted".into());
                }
            }
        }
        let service = self.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let call = CallCancellation(CancelSignal::new());
        let owner_cancel = call.0.clone();
        {
            let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
            if self.shutting_down.load(Ordering::SeqCst) {
                return Err("Execution service is shutting down".into());
            }
            active.insert(id.clone(), owner_cancel.clone());
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        runtime().spawn(async move {
            let mut permit = PhysicalPermit {
                permit: Some(permit),
                settled: false,
                service: service.clone(),
                execution_id: id.clone(),
            };
            if let Err(error) = scope.revalidate().await {
                permit.settle();
                let _ = sender.send(Err(error));
                return;
            }
            match target.profile.backend.clone() {
                Backend::Local(profile) => {
                    let result = async {
                        let cwd = process::local_cwd(&profile.workspace_root, &request.cwd)?;
                        let mut args = profile.arguments;
                        args.push(request.command);
                        scope.revalidate().await?;
                        let result = process::run(
                            &profile.executable,
                            args,
                            Some(&cwd),
                            None,
                            Duration::from_millis(request.timeout_ms),
                            cancel.as_ref(),
                            &owner_cancel,
                            true,
                        )
                        .await?;
                        let mut receipt = ExecutionReceipt::unknown(
                            &id,
                            &request.target_id,
                            "local",
                            "Local process custody could not be proven retired",
                        );
                        receipt.stdout = result.stdout;
                        receipt.stderr = result.stderr;
                        receipt.exit_code = result.exit_code;
                        if result.cleanup {
                            receipt.cleanup_confirmed = true;
                            receipt.status =
                                result.stop.unwrap_or(if result.exit_code == Some(0) {
                                    ExecutionStatus::Succeeded
                                } else {
                                    ExecutionStatus::Failed
                                });
                            receipt.message = "Local process tree retired".into();
                        }
                        Ok::<_, String>(receipt)
                    }
                    .await;
                    if result
                        .as_ref()
                        .map_or(true, |receipt| receipt.cleanup_confirmed)
                    {
                        permit.settle();
                    }
                    if let Ok(receipt) = &result {
                        service.publish(receipt.clone(), &scope.agent_id);
                    }
                    let _ = sender.send(result);
                }
                Backend::Docker(profile) => {
                    match docker::run(
                        profile,
                        &request,
                        &id,
                        cancel.as_ref(),
                        &owner_cancel,
                        &scope,
                    )
                    .await
                    {
                        Err(error) => {
                            permit.settle();
                            let _ = sender.send(Err(error));
                        }
                        Ok(mut owner) => {
                            let retired = owner.retire().await;
                            if retired {
                                owner.receipt.cleanup_confirmed = true;
                                owner.receipt.status = owner.desired_status.clone();
                                owner.receipt.message =
                                    "Exact owned Docker container absence confirmed".into();
                                permit.settle();
                            }
                            service.publish(owner.receipt.clone(), &scope.agent_id);
                            let _ = sender.send(Ok(owner.receipt.clone()));
                            while !owner.receipt.cleanup_confirmed {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                if owner.retire().await {
                                    owner.receipt.cleanup_confirmed = true;
                                    owner.receipt.status = owner.desired_status.clone();
                                    owner.receipt.message =
                                        "Exact owned Docker container absence reconciled".into();
                                    permit.settle();
                                    service.publish(owner.receipt.clone(), &scope.agent_id);
                                }
                            }
                        }
                    }
                }
                Backend::Ssh(profile) => {
                    let mut pending = ssh::run(
                        profile,
                        &request,
                        &id,
                        cancel.as_ref(),
                        &owner_cancel,
                        &scope,
                    )
                    .await;
                    if pending.receipt.cleanup_confirmed {
                        permit.settle();
                    }
                    service.publish(pending.receipt.clone(), &scope.agent_id);
                    let _ = sender.send(Ok(pending.receipt.clone()));
                    while !pending.receipt.cleanup_confirmed {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        if let Some(receipt) = pending.reconcile().await {
                            pending.receipt = receipt;
                            if pending.receipt.cleanup_confirmed {
                                permit.settle();
                            }
                            service.publish(pending.receipt.clone(), &scope.agent_id);
                        }
                    }
                }
            }
            drop(permit);
        });
        let result = receiver
            .await
            .map_err(|_| "Execution owner failed; outcome and cleanup are unknown".to_string())?;
        drop(call);
        result
    }
}
