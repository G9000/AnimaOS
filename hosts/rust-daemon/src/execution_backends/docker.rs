use super::{config::DockerProfile, process, ExecutionReceipt, ExecutionRequest, ExecutionStatus};
use crate::tools::command_binding::CommandBinding;
use anima_core::CancelSignal;
use std::{sync::Arc, time::Duration};

const OWNER_LABEL: &str = "anima.execution.owner";
pub(super) fn create_arguments(
    profile: &DockerProfile,
    request: &ExecutionRequest,
    name: &str,
    token: &str,
) -> Result<Vec<String>, String> {
    let cwd = workspace_cwd(&request.cwd)?;
    Ok(vec![
        "create".into(),
        "--pull=never".into(),
        "--name".into(),
        name.into(),
        "--label".into(),
        format!("{OWNER_LABEL}={token}"),
        "--label".into(),
        format!("anima.execution.target={}", request.target_id),
        "--network=none".into(),
        "--read-only".into(),
        "--user=65534:65534".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        format!("--memory={}m", profile.memory_mb),
        format!("--memory-swap={}m", profile.memory_mb),
        format!("--cpus={}", profile.cpus),
        format!("--pids-limit={}", profile.pids),
        "--tmpfs=/workspace:rw,noexec,nosuid,nodev,size=67108864,mode=1777".into(),
        "--tmpfs=/tmp:rw,noexec,nosuid,nodev,size=16777216,mode=1777".into(),
        "--workdir".into(),
        cwd,
        "--entrypoint=/bin/sh".into(),
        profile.image.clone(),
        "-c".into(),
        request.command.clone(),
    ])
}
fn workspace_cwd(cwd: &str) -> Result<String, String> {
    if cwd.contains('\\')
        || cwd.contains('\0')
        || cwd.split('/').any(|p| p == "..")
        || cwd.starts_with('/')
    {
        return Err("Docker cwd must be relative to /workspace".into());
    }
    let components: Vec<_> = cwd
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    Ok(if components.is_empty() {
        "/workspace".into()
    } else {
        format!("/workspace/{}", components.join("/"))
    })
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(super) async fn cli(
    profile: &DockerProfile,
    args: Vec<String>,
    timeout: Duration,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    binding: &CommandBinding,
    scope: Option<&super::ExecutionScope>,
) -> Result<process::ProcessOutput, String> {
    if let Some(scope) = scope {
        scope.revalidate().await?;
    }
    process::run_bound(
        &profile.executable,
        args,
        None,
        None,
        timeout,
        cancel,
        call,
        false,
        binding,
    )
    .await
}
pub(super) struct DockerOwnership {
    pub profile: DockerProfile,
    binding: Arc<CommandBinding>,
    pub name: String,
    pub token: String,
    pub id: Option<String>,
    pub receipt: ExecutionReceipt,
    pub desired_status: ExecutionStatus,
    transport_cleanup_confirmed: bool,
}
impl DockerOwnership {
    /// A successful engine listing, exact immutable ID and matching random label
    /// are required. CLI disappearance or an inspect error alone prove nothing.
    pub(super) async fn retire(&mut self) -> bool {
        let call = CancelSignal::new();
        if self.id.is_none() {
            let found = cli(
                &self.profile,
                vec![
                    "ps".into(),
                    "--all".into(),
                    "--no-trunc".into(),
                    "--filter".into(),
                    format!("name=^/{}$", self.name),
                    "--filter".into(),
                    format!("label={OWNER_LABEL}={}", self.token),
                    "--format={{.ID}}".into(),
                ],
                Duration::from_secs(10),
                None,
                &call,
                &self.binding,
                None,
            )
            .await;
            let Ok(found) = found else { return false };
            self.transport_cleanup_confirmed &= found.cleanup;
            if found.exit_code != Some(0) || !found.cleanup {
                return false;
            }
            let ids: Vec<_> = found.stdout.split_whitespace().collect();
            if ids.is_empty() {
                return self.transport_cleanup_confirmed;
            }
            if ids.len() != 1 || !valid_id(ids[0]) {
                return false;
            }
            self.id = Some(ids[0].into());
        }
        let id = self.id.as_ref().unwrap().clone();
        let inspect = cli(
            &self.profile,
            vec![
                "inspect".into(),
                "--type=container".into(),
                format!("--format={{{{index .Config.Labels \"{OWNER_LABEL}\"}}}}"),
                id.clone(),
            ],
            Duration::from_secs(10),
            None,
            &call,
            &self.binding,
            None,
        )
        .await;
        let Ok(inspection) = inspect else {
            return false;
        };
        self.transport_cleanup_confirmed &= inspection.cleanup;
        if inspection.exit_code == Some(0) && inspection.cleanup {
            if inspection.stdout.trim() != self.token {
                return false;
            }
            let removal = cli(
                &self.profile,
                vec!["rm".into(), "--force".into(), id.clone()],
                Duration::from_secs(10),
                None,
                &call,
                &self.binding,
                None,
            )
            .await;
            let Ok(removal) = removal else { return false };
            self.transport_cleanup_confirmed &= removal.cleanup;
        }
        let listed = cli(
            &self.profile,
            vec![
                "ps".into(),
                "--all".into(),
                "--no-trunc".into(),
                "--filter".into(),
                format!("id={id}"),
                "--format={{.ID}}".into(),
            ],
            Duration::from_secs(10),
            None,
            &call,
            &self.binding,
            None,
        )
        .await;
        let Ok(listed) = listed else { return false };
        self.transport_cleanup_confirmed &= listed.cleanup;
        self.transport_cleanup_confirmed
            && listed.exit_code == Some(0)
            && listed.cleanup
            && listed.stdout.trim().is_empty()
    }
}
pub(super) async fn run(
    profile: DockerProfile,
    request: &ExecutionRequest,
    id: &str,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    scope: &super::ExecutionScope,
    binding: Arc<CommandBinding>,
) -> Result<DockerOwnership, String> {
    let token = uuid::Uuid::new_v4().to_string();
    let name = format!("anima-execution-{id}");
    let args = create_arguments(&profile, request, &name, &token)?;
    // Restart recovery fails closed if any execution for this profile still exists.
    let existing = cli(
        &profile,
        vec![
            "ps".into(),
            "--all".into(),
            "--no-trunc".into(),
            "--filter".into(),
            format!("label=anima.execution.target={}", request.target_id),
            "--format={{.ID}}".into(),
        ],
        Duration::from_secs(10),
        None,
        call,
        &binding,
        Some(scope),
    )
    .await?;
    if !existing.cleanup {
        return Ok(DockerOwnership {
            profile,
            binding: binding.clone(),
            name,
            token,
            id: None,
            receipt: ExecutionReceipt::unknown(
                id,
                &request.target_id,
                "docker",
                "Docker CLI physical cleanup is unproven",
            ),
            desired_status: ExecutionStatus::Failed,
            transport_cleanup_confirmed: false,
        });
    }
    if existing.exit_code != Some(0) || !existing.stdout.trim().is_empty() {
        return Err("Docker target is unavailable or has an unreconciled prior container; no command was dispatched".into());
    }
    if call.is_cancelled() || cancel.is_some_and(CancelSignal::is_cancelled) {
        return Err("Execution cancelled before Docker dispatch".into());
    }
    let image = cli(
        &profile,
        vec![
            "image".into(),
            "inspect".into(),
            "--format={{json .Config.Volumes}}".into(),
            profile.image.clone(),
        ],
        Duration::from_secs(10),
        None,
        call,
        &binding,
        Some(scope),
    )
    .await?;
    if !image.cleanup {
        return Ok(DockerOwnership {
            profile,
            binding: binding.clone(),
            name,
            token,
            id: None,
            receipt: ExecutionReceipt::unknown(
                id,
                &request.target_id,
                "docker",
                "Docker image inspection cleanup is unproven",
            ),
            desired_status: ExecutionStatus::Failed,
            transport_cleanup_confirmed: false,
        });
    }
    let volumes = serde_json::from_str::<serde_json::Value>(image.stdout.trim()).ok();
    if image.exit_code != Some(0)
        || !matches!(volumes, Some(serde_json::Value::Null))
            && !matches!(volumes,Some(serde_json::Value::Object(ref value))if value.is_empty())
    {
        return Err("Docker target requires an installed image without declared volumes; no command was dispatched".into());
    }
    let created = cli(
        &profile,
        args,
        Duration::from_secs(20),
        cancel,
        call,
        &binding,
        Some(scope),
    )
    .await;
    let mut owner = DockerOwnership {
        profile,
        binding: binding.clone(),
        name,
        token,
        id: None,
        receipt: ExecutionReceipt::unknown(
            id,
            &request.target_id,
            "docker",
            "Docker cleanup awaits authoritative engine confirmation",
        ),
        desired_status: ExecutionStatus::Failed,
        transport_cleanup_confirmed: true,
    };
    match created {
        Ok(result)
            if result.exit_code == Some(0) && result.cleanup && valid_id(result.stdout.trim()) =>
        {
            owner.id = Some(result.stdout.trim().into())
        }
        Ok(result) => {
            owner.transport_cleanup_confirmed &= result.cleanup;
            owner.receipt.stderr = result.stderr;
            owner.desired_status = result.stop.unwrap_or(ExecutionStatus::Failed);
            let _ = owner.retire().await;
            return Ok(owner);
        }
        Err(error) => {
            // The common launch boundary returns Err only before spawning.
            // No container was created, so there is no unknown effect to adopt.
            return Err(error);
        }
    }
    // Inspect label custody before starting exactly the ID returned by create.
    let container_id = owner.id.as_ref().unwrap().clone();
    let inspected = cli(
        &owner.profile,
        vec![
            "inspect".into(),
            "--type=container".into(),
            format!("--format={{{{index .Config.Labels \"{OWNER_LABEL}\"}}}}"),
            container_id.clone(),
        ],
        Duration::from_secs(10),
        None,
        call,
        &binding,
        Some(scope),
    )
    .await;
    let inspected = match inspected {
        Ok(inspected) => inspected,
        Err(error) => {
            // A refused launch created no additional process custody. Keep
            // the earlier proof so independent cleanup can still reconcile.
            owner.receipt.message = error;
            return Ok(owner);
        }
    };
    owner.transport_cleanup_confirmed &= inspected.cleanup;
    if inspected.exit_code != Some(0)
        || !inspected.cleanup
        || inspected.stdout.trim() != owner.token
    {
        return Ok(owner);
    }
    let started = cli(
        &owner.profile,
        vec!["start".into(), "--attach".into(), container_id.clone()],
        Duration::from_millis(request.timeout_ms),
        cancel,
        call,
        &binding,
        Some(scope),
    )
    .await;
    match started {
        Ok(output) => {
            owner.transport_cleanup_confirmed &= output.cleanup;
            owner.receipt.stdout = output.stdout;
            owner.receipt.stderr = output.stderr;
            owner.desired_status = output.stop.unwrap_or(ExecutionStatus::Failed);
            if owner.desired_status == ExecutionStatus::Failed
                && output.exit_code == Some(0)
                && output.cleanup
            {
                let waited = cli(
                    &owner.profile,
                    vec!["wait".into(), container_id],
                    Duration::from_secs(10),
                    None,
                    &CancelSignal::new(),
                    &owner.binding,
                    None,
                )
                .await;
                if let Ok(waited) = waited {
                    owner.transport_cleanup_confirmed &= waited.cleanup;
                    if waited.exit_code == Some(0) && waited.cleanup {
                        owner.receipt.exit_code = waited.stdout.trim().parse().ok();
                        owner.desired_status = if owner.receipt.exit_code == Some(0) {
                            ExecutionStatus::Succeeded
                        } else {
                            ExecutionStatus::Failed
                        };
                    }
                }
            }
        }
        Err(error) => owner.receipt.message = error,
    }
    Ok(owner)
}
