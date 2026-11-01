//! Exact approved setup commands in sealed mutable copies of reviewed skill packages.
//! Private storage separates state; commands retain the daemon user's OS access.

use super::{package::SkillPackage, python_runtime as runtime};
use anima_core::CancelSignal;
use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::{Dir, Metadata};
pub(crate) use runtime::RuntimeAction;
use runtime::{private_handle, RuntimeAuthority, RuntimeOutput};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};

const MAX_COMMAND_BYTES: usize = 16 * 1024;
const MAX_ENTRIES: usize = 50_000;
const MAX_TREE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
type FileIdentity = (u64, u64);
type DirectoryEdges = HashMap<FileIdentity, Vec<FileIdentity>>;

#[derive(Clone, Copy, Debug)]
struct StorageLimits {
    entries: usize,
    bytes: u64,
    depth: usize,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            entries: MAX_ENTRIES,
            bytes: MAX_TREE_BYTES,
            depth: MAX_DEPTH,
        }
    }
}

#[derive(Clone, Debug)]
enum StorageViolation {
    Limit(&'static str),
    Unavailable,
    Interrupted,
}

impl StorageViolation {
    fn stop(&self) -> &'static str {
        match self {
            Self::Limit(_) => "storage_limit",
            _ => "storage_unverified",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::Limit(reason) => reason,
            Self::Unavailable => "The private command workspace could not be measured safely",
            Self::Interrupted => "Command storage observation was interrupted",
        }
    }
}

struct StorageObservation {
    next: tokio::time::Instant,
    violation: Option<StorageViolation>,
}

/// Samples held private storage without following links or reading file bytes.
/// Sampling may transiently overshoot; this is not an OS quota, and commands
/// retain the ability to write elsewhere or cause external effects.
struct StorageObserver {
    root: Dir,
    identity: FileIdentity,
    limits: StorageLimits,
    deadline: tokio::time::Instant,
    cancel: CancelSignal,
    call: CancelSignal,
    state: Mutex<StorageObservation>,
}

impl StorageObserver {
    fn new(
        environment: &Path,
        limits: StorageLimits,
        deadline: tokio::time::Instant,
        cancel: CancelSignal,
        call: CancelSignal,
    ) -> Result<Self, String> {
        let root = private_handle(environment)?
            .held_directory()
            .map_err(|_| "Command storage is unavailable")?;
        let metadata = root
            .dir_metadata()
            .map_err(|_| "Command storage is unavailable")?;
        Ok(Self {
            root,
            identity: (metadata.dev(), metadata.ino()),
            limits,
            deadline,
            cancel,
            call,
            state: Mutex::new(StorageObservation {
                next: tokio::time::Instant::now(),
                violation: None,
            }),
        })
    }

    fn sample(&self, force: bool) -> Result<(), String> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Command storage observation is unavailable")?;
            if let Some(violation) = &state.violation {
                return Err(violation.message().into());
            }
            if !force && tokio::time::Instant::now() < state.next {
                return Ok(());
            }
            state.next = tokio::time::Instant::now() + Duration::from_secs(1);
        }
        if let Err(violation) = self.measure(&self.root, &mut 0, &mut 0, 0, self.deadline, true) {
            if !matches!(violation, StorageViolation::Interrupted) {
                self.state
                    .lock()
                    .map_err(|_| "Command storage observation is unavailable")?
                    .violation = Some(violation.clone());
            }
            return Err(violation.message().into());
        }
        Ok(())
    }

    fn sample_retired(&self) {
        if self.violation().is_some() {
            return;
        }
        // Retirement cleanup remains necessary after Stop/deadline. This bounded
        // metadata pass grants no new command authority and cannot publish success.
        let result = self.measure(
            &self.root,
            &mut 0,
            &mut 0,
            0,
            tokio::time::Instant::now() + Duration::from_secs(2),
            false,
        );
        if let Err(violation) = result {
            let violation = if matches!(violation, StorageViolation::Interrupted) {
                StorageViolation::Unavailable
            } else {
                violation
            };
            if let Ok(mut state) = self.state.lock() {
                state.violation = Some(violation);
            }
        }
    }

    fn violation(&self) -> Option<StorageViolation> {
        self.state
            .lock()
            .map_or(Some(StorageViolation::Unavailable), |state| {
                state.violation.clone()
            })
    }

    fn measure(
        &self,
        directory: &Dir,
        entries: &mut usize,
        bytes: &mut u64,
        depth: usize,
        deadline: tokio::time::Instant,
        observe_cancel: bool,
    ) -> Result<(), StorageViolation> {
        if (observe_cancel && (self.cancel.is_cancelled() || self.call.is_cancelled()))
            || tokio::time::Instant::now() >= deadline
        {
            return Err(StorageViolation::Interrupted);
        }
        if depth > self.limits.depth {
            return Err(StorageViolation::Limit(
                "Command storage nesting exceeded its configured limit",
            ));
        }
        for entry in directory
            .entries()
            .map_err(|_| StorageViolation::Unavailable)?
        {
            if (observe_cancel && (self.cancel.is_cancelled() || self.call.is_cancelled()))
                || tokio::time::Instant::now() >= deadline
            {
                return Err(StorageViolation::Interrupted);
            }
            let entry = entry.map_err(|_| StorageViolation::Unavailable)?;
            *entries += 1;
            if *entries > self.limits.entries {
                return Err(StorageViolation::Limit(
                    "Command storage exceeded its configured filesystem-entry limit",
                ));
            }
            let name = entry.file_name();
            let metadata = match directory.symlink_metadata(&name) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(StorageViolation::Unavailable),
            };
            if crate::tools::workspace_bytes::linked(&metadata) {
                continue;
            }
            if metadata.is_file() {
                *bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or(StorageViolation::Limit(
                        "Command storage exceeded its configured byte limit",
                    ))?;
                if *bytes > self.limits.bytes {
                    return Err(StorageViolation::Limit(
                        "Command storage exceeded its configured byte limit",
                    ));
                }
            } else if metadata.is_dir() {
                let child = match directory.open_dir_nofollow(&name) {
                    Ok(child) => child,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => return Err(StorageViolation::Unavailable),
                };
                let opened = child
                    .dir_metadata()
                    .map_err(|_| StorageViolation::Unavailable)?;
                if crate::tools::workspace_bytes::linked(&opened) {
                    continue;
                }
                if (metadata.dev(), metadata.ino()) != (opened.dev(), opened.ino()) {
                    return Err(StorageViolation::Unavailable);
                }
                self.measure(&child, entries, bytes, depth + 1, deadline, observe_cancel)?;
            } else {
                return Err(StorageViolation::Unavailable);
            }
        }
        Ok(())
    }
}

fn observed_authority(
    authority: RuntimeAuthority,
    observer: Arc<StorageObserver>,
) -> RuntimeAuthority {
    Arc::new(move || {
        let authority = authority.clone();
        let observer = observer.clone();
        Box::pin(async move {
            observer.sample(false)?;
            authority().await
        })
    })
}

/// An owner-approved operation bound to the exact package and current mutable state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandRuntimeRequest {
    pub(crate) action: RuntimeAction,
    pub(crate) slug: String,
    pub(crate) hash: String,
    #[serde(default)]
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) state_hash: Option<String>,
    #[serde(default = "default_timeout")]
    pub(crate) timeout_ms: u64,
}

fn default_timeout() -> u64 {
    120_000
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_cwd(value: &str) -> Result<(), String> {
    if value == "." {
        return Ok(());
    }
    anima_core::skill_packages::validate_supporting_path(value).map_err(str::to_string)
}

impl CommandRuntimeRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !super::is_valid_slug(&self.slug) || !valid_hash(&self.hash) {
            return Err("Select an exact approved skill slug and package hash".into());
        }
        if !(1_000..=300_000).contains(&self.timeout_ms) {
            return Err("Skill setup timeout must be between 1000 and 300000 ms".into());
        }
        match self.action {
            RuntimeAction::Run => {
                let command = self
                    .command
                    .as_deref()
                    .ok_or("Run needs an exact command")?;
                if command.trim().is_empty()
                    || command.len() > MAX_COMMAND_BYTES
                    || command.contains('\0')
                {
                    return Err("The exact command must be nonblank and at most 16384 bytes".into());
                }
                if !self.state_hash.as_deref().is_some_and(valid_hash) {
                    return Err(
                        "Run needs the latest exact stateHash from prepare or status".into(),
                    );
                }
                validate_cwd(self.cwd.as_deref().unwrap_or("."))?;
            }
            _ if self.command.is_some() || self.cwd.is_some() || self.state_hash.is_some() => {
                return Err("Only run accepts command, cwd or stateHash".into());
            }
            _ => {}
        }
        Ok(())
    }
}

/// A successful receipt proves only this command and working directory at one state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandCheck {
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) input_state_hash: String,
    pub(crate) exit_code: i32,
    pub(crate) checked_at_ms: u64,
}

/// State and bounded output for a prepared workspace or exact command operation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandRuntimeResult {
    pub(crate) status: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) state_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) working_directory: Option<String>,
    pub(crate) last_check: Option<CommandCheck>,
    pub(crate) output: Option<RuntimeOutput>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Shell {
    path: PathBuf,
    mode: String,
    identity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: u32,
    slug: String,
    hash: String,
    directory: String,
    previous_directory: Option<String>,
    shell: Shell,
    /// None is durably published before spawn and survives unproven retirement.
    state_hash: Option<String>,
    last_check: Option<CommandCheck>,
}

struct OwnedEnvironment {
    root: PathBuf,
    directory: String,
    physical: Arc<runtime::PhysicalLease>,
    keep: bool,
}

impl Drop for OwnedEnvironment {
    fn drop(&mut self) {
        if !self.keep && self.physical.settled.load(Ordering::Acquire) {
            let _ = remove_environment(&self.root, &self.directory);
        }
    }
}

fn normalized_root(root: &Path, workspace: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("Runtime storage must be an absolute private directory".into());
    }
    let parent = root
        .parent()
        .ok_or("Runtime storage has no parent")?
        .canonicalize()
        .map_err(|_| "Runtime storage parent is unavailable")?;
    let root = parent.join(root.file_name().ok_or("Runtime storage name is invalid")?);
    if runtime::directory_is_within(&root, workspace)
        || runtime::directory_is_within(workspace, &root)
    {
        return Err("Runtime storage must be outside the agent workspace".into());
    }
    Ok(root)
}

/// Retains native custody until the process tree retires even when the caller drops.
pub(crate) async fn execute(
    root: PathBuf,
    workspace: PathBuf,
    package: SkillPackage,
    request: CommandRuntimeRequest,
    cancel: CancelSignal,
    authority: RuntimeAuthority,
) -> Result<CommandRuntimeResult, String> {
    execute_with_limits(
        root,
        workspace,
        package,
        request,
        cancel,
        authority,
        StorageLimits::default(),
    )
    .await
}

async fn execute_with_limits(
    root: PathBuf,
    workspace: PathBuf,
    package: SkillPackage,
    request: CommandRuntimeRequest,
    cancel: CancelSignal,
    authority: RuntimeAuthority,
    limits: StorageLimits,
) -> Result<CommandRuntimeResult, String> {
    request.validate()?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(request.timeout_ms);
    anima_core::skill_packages::validate_skill_package(&package.bytes, &package.files)
        .map_err(str::to_string)?;
    if package.hash() != request.hash {
        return Err("Skill package changed; review it again".into());
    }
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "Skill workspace is unavailable")?;
    let root = normalized_root(&root, &workspace)?;
    let call = CancelSignal::new();
    let guard = runtime::CancelOnDrop(call.clone());
    let joined = tokio::spawn(async move {
        let _active = runtime::Active::enter();
        let locking = runtime::lease(&root)?;
        let _lease = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err("Skill runtime operation cancelled".into()),
            _ = call.cancelled() => return Err("Skill runtime operation cancelled".into()),
            _ = tokio::time::sleep_until(deadline) => return Err("Skill runtime operation timed out".into()),
            lease = locking.lock() => lease,
        };
        runtime::require_authority(&authority, deadline, &cancel, &call).await?;
        execute_owned(root, workspace, package, request, cancel, call, authority, deadline, limits).await
    })
    .await
    .map_err(|_| "Skill setup worker failed with unresolved physical custody; reconcile before retrying")?;
    drop(guard);
    joined
}

async fn execute_owned(
    root: PathBuf,
    workspace: PathBuf,
    package: SkillPackage,
    request: CommandRuntimeRequest,
    cancel: CancelSignal,
    call: CancelSignal,
    authority: RuntimeAuthority,
    deadline: tokio::time::Instant,
    limits: StorageLimits,
) -> Result<CommandRuntimeResult, String> {
    runtime::ensure_operation_live(deadline, &cancel, &call)?;
    let name = manifest_name(&request);
    // Status never initializes storage, repairs permissions, probes programs or writes a manifest.
    if request.action == RuntimeAction::Status {
        if !root
            .try_exists()
            .map_err(|_| "Runtime storage is unavailable")?
        {
            return Ok(unprepared("installed", "The original package is installed. Prepare a private command workspace before running documented setup commands.", None));
        }
        let held = private_handle(&root)?;
        if runtime::persisted_marker_count(&root) != 0 {
            return Ok(unprepared("needs_prepare", "An earlier operation has unresolved physical custody. Reconcile it before another command.", None));
        }
        return match read_manifest(&held, &name)? {
            None => Ok(unprepared("installed", "The original package is installed. No command workspace or successful command has been verified.", None)),
            Some(manifest) => {
                match validate_manifest(&root, &workspace, &package, &request, &manifest, deadline, &cancel, &call) {
                    Ok(()) => {
                        runtime::require_authority(&authority, deadline, &cancel, &call).await?;
                        Ok(result_from_manifest(&root, &manifest, None))
                    }
                    Err(error) => {
                        runtime::ensure_operation_live(deadline, &cancel, &call)?;
                        Ok(unprepared("needs_prepare", &format!("Prepare again before running: {error}"), None))
                    }
                }
            }
        };
    }
    crate::document_edits::store::initialize_owned_directory(&root)?;
    let held = private_handle(&root)?;
    if runtime::persisted_marker_count(&root) != 0 {
        return Err("A previous skill runtime operation has unresolved physical custody. Reconcile it before another operation".into());
    }
    let prior = read_manifest(&held, &name)?;
    let physical = runtime::PhysicalLease::admit(&root, runtime::OPERATION_MARKER.into())?;
    match request.action {
        RuntimeAction::Prepare => {
            runtime::check_environment_capacity(&held)?;
            let shell = discover_shell(&workspace, &root)?;
            let directory = format!("r-{}", uuid::Uuid::new_v4().simple());
            let environment = root.join(&directory);
            crate::document_edits::store::initialize_owned_directory(&environment)?;
            let mut owned = OwnedEnvironment {
                root: root.clone(),
                directory: directory.clone(),
                physical,
                keep: false,
            };
            for part in [
                "source",
                "work",
                "home",
                "home/config",
                "home/cache",
                "home/data",
            ] {
                runtime::ensure_operation_live(deadline, &cancel, &call)?;
                crate::document_edits::store::initialize_owned_directory(&environment.join(part))?;
            }
            super::package::write_package(&environment.join("source"), &request.slug, &package)?;
            super::package::write_package(&environment.join("work"), &request.slug, &package)?;
            let mut manifest = Manifest {
                version: 1,
                slug: request.slug.clone(),
                hash: request.hash.clone(),
                directory,
                previous_directory: prior.as_ref().map(|value| value.directory.clone()),
                shell,
                state_hash: None,
                last_check: None,
            };
            manifest.state_hash = Some(state_hash(
                &environment,
                &manifest.shell,
                deadline,
                &cancel,
                &call,
            )?);
            validate_manifest(
                &root, &workspace, &package, &request, &manifest, deadline, &cancel, &call,
            )?;
            runtime::require_authority(&authority, deadline, &cancel, &call).await?;
            write_manifest(&held, &name, prior.as_ref(), &manifest)?;
            owned.keep = true;
            if let Some(old) = prior.and_then(|value| value.previous_directory) {
                // A changed retained tree is left in place for explicit reconciliation.
                let _ = remove_environment(&root, &old);
            }
            Ok(result_from_manifest(&root, &manifest, None))
        }
        RuntimeAction::Run => {
            let mut manifest = prior.ok_or("Prepare this exact package before running it")?;
            validate_manifest(
                &root, &workspace, &package, &request, &manifest, deadline, &cancel, &call,
            )?;
            if manifest.state_hash != request.state_hash {
                return Err("The command workspace state changed; use the latest exact stateHash from status".into());
            }
            let environment = root.join(&manifest.directory);
            let relative_cwd = request.cwd.as_deref().unwrap_or(".");
            let cwd = selected_cwd(&environment, &request.slug, relative_cwd)?;
            let command = request
                .command
                .clone()
                .ok_or("Run needs an exact command")?;
            let previous = manifest.clone();
            manifest.state_hash = None;
            manifest.last_check = None;
            runtime::require_authority(&authority, deadline, &cancel, &call).await?;
            // Invalidation is durable before any child can execute, including failed launches.
            write_manifest(&held, &name, Some(&previous), &manifest)?;
            let helpers = runtime::sanitized_helper_directories(
                runtime::OPERATOR_TOOL_PATH
                    .get_or_init(|| std::env::var_os("PATH").unwrap_or_default()),
                &workspace,
                &root,
            );
            let observer = Arc::new(StorageObserver::new(
                &environment,
                limits,
                deadline,
                cancel.clone(),
                call.clone(),
            )?);
            let monitored_authority = observed_authority(authority.clone(), observer.clone());
            let launched = runtime::launch_invocation(
                &manifest.shell.path,
                runtime::ProcessInvocation::ShellCommand(command.clone()),
                &cwd,
                &environment.join("home"),
                deadline,
                &cancel,
                &call,
                &monitored_authority,
                &physical,
                &helpers,
            )
            .await;
            // Also measure after leader exit: a fast writer can finish between polls.
            observer.sample_retired();
            let violation = observer.violation();
            let environment_identity = observer.identity;
            drop(monitored_authority);
            drop(observer);
            let mut output = match launched {
                Ok(output) => output,
                Err(error) => {
                    if violation.is_none() {
                        return Err(error);
                    }
                    RuntimeOutput {
                        exit_code: None,
                        stdout: String::new(),
                        stderr: error,
                        stop: None,
                        cleanup_confirmed: physical.settled.load(Ordering::Acquire),
                    }
                }
            };
            if let Some(violation) = violation {
                output.stop = Some(violation.stop().into());
                return Ok(storage_failure(
                    &held,
                    &manifest.directory,
                    environment_identity,
                    &physical,
                    violation,
                    output,
                ));
            }
            if !output.cleanup_confirmed {
                return Ok(unprepared("command_failed", "The command's physical retirement is unproven. Stop for unproven effects and reconcile custody before continuing. External and remote effects are not rolled back.", Some(output)));
            }
            if output.stop.is_some() {
                return Ok(unprepared("command_failed", "The command stopped. Its successful receipt was invalidated; prepare again before another command. External and remote effects are not rolled back.", Some(output)));
            }
            if let Err(error) =
                runtime::require_authority(&authority, deadline, &cancel, &call).await
            {
                return Ok(unprepared("command_failed", &format!("The command retired but its authority is no longer current: {error}. Prepare again before continuing."), Some(output)));
            }
            if let Err(error) = validate_source(&environment, &package, &request.slug) {
                return Ok(unprepared("command_failed", &error, Some(output)));
            }
            let sealed = match state_hash(&environment, &manifest.shell, deadline, &cancel, &call) {
                Ok(sealed) => sealed,
                Err(error) => return Ok(unprepared("command_failed", &format!("Command state cannot be resealed: {error}. Prepare again before continuing."), Some(output))),
            };
            runtime::require_authority(&authority, deadline, &cancel, &call).await?;
            let mut checked = manifest.clone();
            checked.state_hash = Some(sealed);
            if output.succeeded() {
                checked.last_check = Some(CommandCheck {
                    command,
                    cwd: relative_cwd.into(),
                    input_state_hash: previous
                        .state_hash
                        .ok_or("Command input state is unavailable")?,
                    exit_code: 0,
                    checked_at_ms: anima_core::primitives::now_millis(),
                });
            }
            write_manifest(&held, &name, Some(&manifest), &checked)?;
            let succeeded = checked.last_check.is_some();
            let mut result = result_from_manifest(&root, &checked, Some(output));
            if !succeeded {
                result.status = "command_failed".into();
                result.message = "The command returned a failure. Its changed workspace is sealed for recovery; no successful command receipt remains.".into();
            }
            Ok(result)
        }
        RuntimeAction::Status => unreachable!(),
    }
}

fn manifest_name(request: &CommandRuntimeRequest) -> String {
    format!("command-{}-{}.json", request.slug, request.hash)
}

fn work_directory(environment: &Path, slug: &str) -> PathBuf {
    environment.join("work/skills").join(slug)
}

fn unprepared(status: &str, message: &str, output: Option<RuntimeOutput>) -> CommandRuntimeResult {
    CommandRuntimeResult {
        status: status.into(),
        message: message.into(),
        state_hash: None,
        working_directory: None,
        last_check: None,
        output,
    }
}

fn result_from_manifest(
    root: &Path,
    value: &Manifest,
    output: Option<RuntimeOutput>,
) -> CommandRuntimeResult {
    CommandRuntimeResult {
        status: if value.last_check.is_some() { "command_verified" } else { "environment_prepared" }.into(),
        message: if value.last_check.is_some() { "The recorded exact command completed successfully in this workspace state. This does not verify the whole skill, account access or other commands." } else { "The private command workspace is prepared. Follow the original setup instructions with exact approved commands, then run an actual preflight." }.into(),
        state_hash: value.state_hash.clone(),
        working_directory: Some(work_directory(&root.join(&value.directory), &value.slug).display().to_string()),
        last_check: value.last_check.clone(),
        output,
    }
}

fn read_manifest(
    held: &crate::tools::workspace_bytes::WorkspaceBytes,
    name: &str,
) -> Result<Option<Manifest>, String> {
    use crate::tools::workspace_bytes::InstructionReadError;
    match held.read_instruction_file(name, 64 * 1024) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Command runtime manifest is invalid".into()),
        Err(InstructionReadError::Missing) => Ok(None),
        Err(_) => Err("Command runtime manifest is unavailable".into()),
    }
}

fn write_manifest(
    held: &crate::tools::workspace_bytes::WorkspaceBytes,
    name: &str,
    expected: Option<&Manifest>,
    value: &Manifest,
) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| "Command runtime manifest cannot be encoded")?;
    if let Some(expected) = expected {
        let prior = held
            .read_instruction_file(name, 64 * 1024)
            .map_err(|_| "Command runtime manifest changed")?;
        let current: Manifest =
            serde_json::from_slice(&prior).map_err(|_| "Command runtime manifest changed")?;
        if serde_json::to_vec(&current).ok() != serde_json::to_vec(expected).ok() {
            return Err("Command runtime manifest changed".into());
        }
        held.compare_replace_instruction_file(name, &prior, &bytes, || Ok(()))
            .map_err(|_| "Command runtime manifest could not be saved".into())
    } else {
        held.create_new_bytes_atomic(name, &bytes, || Ok(()))
            .map_err(|_| "Command runtime manifest could not be saved".into())
    }
}

fn valid_directory(name: &str) -> bool {
    name.len() == 34
        && name.starts_with("r-")
        && name[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_source(environment: &Path, package: &SkillPackage, slug: &str) -> Result<(), String> {
    verify_layout(environment, slug)?;
    let snapshot = super::package::read_package(&environment.join("source"), slug)?
        .ok_or("Original approved source snapshot is unavailable")?;
    if snapshot.hash() != package.hash() {
        return Err("The original approved source snapshot changed; prepare again".into());
    }
    Ok(())
}

fn validate_manifest(
    root: &Path,
    workspace: &Path,
    package: &SkillPackage,
    request: &CommandRuntimeRequest,
    manifest: &Manifest,
    deadline: tokio::time::Instant,
    cancel: &CancelSignal,
    call: &CancelSignal,
) -> Result<(), String> {
    if manifest.version != 1
        || manifest.slug != request.slug
        || manifest.hash != request.hash
        || !valid_directory(&manifest.directory)
        || manifest.state_hash.is_none()
    {
        return Err("The command workspace is not sealed for this approved package".into());
    }
    let environment = root.join(&manifest.directory);
    let _held = private_handle(&environment)?;
    let _source = private_handle(&environment.join("source"))?;
    let _home = private_handle(&environment.join("home"))?;
    let _work = private_handle(&environment.join("work"))?;
    let current_shell = discover_shell(workspace, root)?;
    if current_shell.path != manifest.shell.path
        || current_shell.mode != manifest.shell.mode
        || current_shell.identity != manifest.shell.identity
    {
        return Err("The selected trusted shell changed".into());
    }
    validate_source(&environment, package, &request.slug)?;
    let actual_state = state_hash(&environment, &manifest.shell, deadline, cancel, call)?;
    if manifest.state_hash.as_deref() != Some(actual_state.as_str()) {
        return Err("The source, mutable work, private home or shell state changed".into());
    }
    runtime::ensure_operation_live(deadline, cancel, call)
}

fn nofollow_child(parent: &Dir, name: &Path) -> Result<Dir, String> {
    let before = parent
        .symlink_metadata(name)
        .map_err(|_| "Command environment directory is unavailable")?;
    if !before.is_dir() || crate::tools::workspace_bytes::linked(&before) {
        return Err("Command environment layout was redirected".into());
    }
    let child = parent
        .open_dir_nofollow(name)
        .map_err(|_| "Command environment layout was redirected")?;
    let after = child
        .dir_metadata()
        .map_err(|_| "Command environment directory is unavailable")?;
    if crate::tools::workspace_bytes::linked(&after)
        || metadata_fingerprint(&before) != metadata_fingerprint(&after)
    {
        return Err("Command environment directory changed while opening".into());
    }
    Ok(child)
}

fn verify_layout(environment: &Path, slug: &str) -> Result<(), String> {
    let parent = crate::tools::workspace_bytes::WorkspaceBytes::open(
        environment
            .parent()
            .ok_or("Command environment has no parent")?,
    )
    .map_err(|_| "Command environment parent is unavailable")?;
    let parent = parent
        .held_directory()
        .map_err(|_| "Command environment parent is unavailable")?;
    let environment = nofollow_child(
        &parent,
        Path::new(
            environment
                .file_name()
                .ok_or("Command environment name is invalid")?,
        ),
    )?;
    let mut identities = HashSet::new();
    for name in ["source", "work", "home"] {
        let child = nofollow_child(&environment, Path::new(name))?;
        let metadata = child
            .dir_metadata()
            .map_err(|_| "Command environment layout is unavailable")?;
        if crate::tools::workspace_bytes::linked(&metadata)
            || !identities.insert((metadata.dev(), metadata.ino()))
        {
            return Err(
                "Original source, mutable work and private home must remain separate directories"
                    .into(),
            );
        }
        if name != "home" {
            let skills = nofollow_child(&child, Path::new("skills"))?;
            let _package = nofollow_child(&skills, Path::new(slug))?;
        }
    }
    Ok(())
}

fn selected_cwd(environment: &Path, slug: &str, relative: &str) -> Result<PathBuf, String> {
    validate_cwd(relative)?;
    let held = private_handle(environment)?;
    let directory = held
        .held_directory()
        .map_err(|_| "Command workspace is unavailable")?;
    let work = nofollow_child(&directory, Path::new("work"))?;
    let skills = nofollow_child(&work, Path::new("skills"))?;
    let directory = nofollow_child(&skills, Path::new(slug))?;
    let resolved = directory
        .canonicalize(relative)
        .map_err(|_| "Command cwd must resolve inside the mutable workspace")?;
    if resolved.is_absolute()
        || resolved.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Command cwd must remain inside the mutable workspace".into());
    }
    let _cwd = directory
        .open_dir(&resolved)
        .map_err(|_| "Command cwd is not a directory")?;
    Ok(work_directory(environment, slug).join(resolved))
}

fn discover_shell(workspace: &Path, root: &Path) -> Result<Shell, String> {
    #[cfg(windows)]
    let (candidate, mode) = (
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32/cmd.exe"),
        "cmd-d-eon-voff-s-c-v2",
    );
    #[cfg(not(windows))]
    let (candidate, mode) = (PathBuf::from("/bin/sh"), "posix-sh-c-v1");
    let path = candidate
        .canonicalize()
        .map_err(|_| "The installed system command shell is unavailable")?;
    if runtime::directory_is_within(&path, workspace) || runtime::directory_is_within(&path, root) {
        return Err(
            "The trusted command shell must be outside workspace and runtime storage".into(),
        );
    }
    runtime::reject_linked_file(&path)?;
    let identity = executable_identity(&path)?;
    Ok(Shell {
        path,
        mode: mode.into(),
        identity,
    })
}

fn executable_identity(path: &Path) -> Result<String, String> {
    let held = crate::tools::workspace_bytes::WorkspaceBytes::open(
        path.parent().ok_or("Shell has no parent")?,
    )
    .map_err(|_| "Shell parent is unavailable")?;
    let directory = held
        .held_directory()
        .map_err(|_| "Shell parent is unavailable")?;
    let name = path.file_name().ok_or("Shell has no filename")?;
    let mut file = crate::tools::workspace_bytes::open_regular_file(
        &directory,
        name.to_str().ok_or("Shell filename is not portable")?,
    )
    .map_err(|_| "Shell is unavailable")?;
    let before = file
        .metadata()
        .map_err(|_| "Shell metadata is unavailable")?;
    let mut hash = Sha256::new();
    hash_metadata(&mut hash, &before);
    hash_permissions(
        &mut hash,
        &file
            .try_clone()
            .map_err(|_| "Shell is unavailable")?
            .into_std(),
    )?;
    if before.len() > 64 * 1024 * 1024 {
        return Err("The trusted shell exceeds 64 MiB".into());
    }
    hash.update(before.len().to_be_bytes());
    let mut read = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|_| "The trusted shell cannot be read")?;
        if size == 0 {
            break;
        }
        read += size as u64;
        if read > before.len() {
            return Err("The trusted shell changed while reading".into());
        }
        hash.update(&buffer[..size]);
    }
    let after = file
        .metadata()
        .map_err(|_| "Shell metadata is unavailable")?;
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| "Shell metadata is unavailable")?;
    if read != before.len()
        || metadata_fingerprint(&before) != metadata_fingerprint(&named)
        || metadata_fingerprint(&before) != metadata_fingerprint(&after)
        || before.modified().ok() != after.modified().ok()
    {
        return Err("The trusted shell changed while reading".into());
    }
    Ok(hex::encode(hash.finalize()))
}

fn state_hash(
    environment: &Path,
    shell: &Shell,
    deadline: tokio::time::Instant,
    cancel: &CancelSignal,
    call: &CancelSignal,
) -> Result<String, String> {
    runtime::ensure_operation_live(deadline, cancel, call)?;
    if executable_identity(&shell.path)? != shell.identity {
        return Err("The selected trusted shell changed".into());
    }
    let mut hash = Sha256::new();
    hash.update(b"anima-command-state-v1\0");
    field(
        &mut hash,
        environment.as_os_str().to_string_lossy().as_bytes(),
    );
    field(
        &mut hash,
        shell.path.as_os_str().to_string_lossy().as_bytes(),
    );
    field(&mut hash, shell.mode.as_bytes());
    field(&mut hash, shell.identity.as_bytes());
    field(
        &mut hash,
        tree_seal(environment, deadline, cancel, call)?.as_bytes(),
    );
    runtime::ensure_operation_live(deadline, cancel, call)?;
    Ok(hex::encode(hash.finalize()))
}

fn field(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

fn hash_metadata(hash: &mut Sha256, metadata: &Metadata) {
    hash.update(metadata.dev().to_be_bytes());
    hash.update(metadata.ino().to_be_bytes());
    hash.update(metadata.nlink().to_be_bytes());
    hash.update([u8::from(metadata.permissions().readonly())]);
    #[cfg(unix)]
    {
        use cap_std::fs::{MetadataExt as _, PermissionsExt};
        hash.update(metadata.permissions().mode().to_be_bytes());
        hash.update(metadata.uid().to_be_bytes());
        hash.update(metadata.gid().to_be_bytes());
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt as _;
        hash.update(metadata.file_attributes().to_be_bytes());
    }
}

fn metadata_fingerprint(metadata: &Metadata) -> String {
    let mut hash = Sha256::new();
    hash_metadata(&mut hash, metadata);
    hash.update(metadata.len().to_be_bytes());
    hex::encode(hash.finalize())
}

#[cfg(windows)]
fn hash_permissions(hash: &mut Sha256, file: &std::fs::File) -> Result<(), String> {
    let descriptor = win_custody::ObjectSecurityDescriptor::query_file_handle(file)
        .map_err(|_| "Command state permissions are unavailable")?;
    hash.update(
        descriptor
            .control()
            .map_err(|_| "Command state permissions are unavailable")?
            .to_be_bytes(),
    );
    // The validated values own every owner SID and ACE field. Their structural
    // encoding also detects permission changes when file bytes stay identical.
    field(
        hash,
        format!(
            "{:?}",
            descriptor
                .owner()
                .map_err(|_| "Command state owner is unavailable")?
        )
        .as_bytes(),
    );
    field(
        hash,
        format!(
            "{:?}",
            descriptor
                .dacl()
                .map_err(|_| "Command state ACL is unsupported")?
        )
        .as_bytes(),
    );
    Ok(())
}

#[cfg(not(windows))]
fn hash_permissions(_hash: &mut Sha256, _file: &std::fs::File) -> Result<(), String> {
    Ok(())
}

struct TreeSeal<'a> {
    root: &'a Dir,
    hash: Sha256,
    entries: usize,
    bytes: u64,
    edges: DirectoryEdges,
    deadline: tokio::time::Instant,
    cancel: &'a CancelSignal,
    call: &'a CancelSignal,
}

fn tree_seal(
    environment: &Path,
    deadline: tokio::time::Instant,
    cancel: &CancelSignal,
    call: &CancelSignal,
) -> Result<String, String> {
    let held = private_handle(environment)?;
    let directory = held
        .held_directory()
        .map_err(|_| "Command state is unavailable")?;
    let mut seal = TreeSeal {
        root: &directory,
        hash: Sha256::new(),
        entries: 0,
        bytes: 0,
        edges: HashMap::new(),
        deadline,
        cancel,
        call,
    };
    seal.hash.update(b"anima-command-tree-v1\0");
    seal.directory(&directory, Path::new(""), 0)?;
    let root_metadata = directory
        .dir_metadata()
        .map_err(|_| "Command state directory is unavailable")?;
    reject_directory_cycles((root_metadata.dev(), root_metadata.ino()), &seal.edges)?;
    runtime::ensure_operation_live(deadline, cancel, call)?;
    Ok(hex::encode(seal.hash.finalize()))
}

impl TreeSeal<'_> {
    fn check(&self) -> Result<(), String> {
        runtime::ensure_operation_live(self.deadline, self.cancel, self.call)
    }

    fn directory(&mut self, directory: &Dir, relative: &Path, depth: usize) -> Result<(), String> {
        self.check()?;
        if depth > MAX_DEPTH {
            return Err("Command state nesting exceeds 64 levels".into());
        }
        let before = directory
            .dir_metadata()
            .map_err(|_| "Command state directory is unavailable")?;
        field(&mut self.hash, b"directory");
        field(
            &mut self.hash,
            relative.as_os_str().to_string_lossy().as_bytes(),
        );
        hash_metadata(&mut self.hash, &before);
        hash_permissions(
            &mut self.hash,
            &directory
                .try_clone()
                .map_err(|_| "Command state directory is unavailable")?
                .into_std_file(),
        )?;
        let mut names = Vec::new();
        for entry in directory
            .entries()
            .map_err(|_| "Command state directory is unavailable")?
        {
            self.check()?;
            if names.len() >= MAX_ENTRIES.saturating_sub(self.entries) {
                return Err("Command state exceeds 50000 filesystem entries".into());
            }
            names.push(
                entry
                    .map_err(|_| "Command state directory is unavailable")?
                    .file_name(),
            );
        }
        names.sort();
        let identity = (before.dev(), before.ino());
        self.edges.entry(identity).or_default();
        for name in names {
            self.check()?;
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err("Command state exceeds 50000 filesystem entries".into());
            }
            let name = name
                .to_str()
                .ok_or("Command state has a nonportable filename")?;
            let path = relative.join(name);
            let metadata = directory
                .symlink_metadata(name)
                .map_err(|_| "Command state changed while sealing")?;
            if metadata.file_type().is_symlink() {
                self.link(directory, name, &path, identity, &metadata)?;
            } else if crate::tools::workspace_bytes::linked(&metadata) {
                return Err("Command state contains an unsupported reparse path".into());
            } else if metadata.is_dir() {
                let child = directory
                    .open_dir_nofollow(name)
                    .map_err(|_| "Command state directory changed")?;
                let opened = child
                    .dir_metadata()
                    .map_err(|_| "Command state directory changed")?;
                if metadata_fingerprint(&metadata) != metadata_fingerprint(&opened) {
                    return Err("Command state directory changed while opening".into());
                }
                self.edges
                    .entry(identity)
                    .or_default()
                    .push((opened.dev(), opened.ino()));
                self.directory(&child, &path, depth + 1)?;
            } else if metadata.is_file() {
                self.file(directory, name, &path)?;
            } else {
                return Err("Command state contains a nonregular file".into());
            }
        }
        let after = directory
            .dir_metadata()
            .map_err(|_| "Command state directory is unavailable")?;
        if metadata_fingerprint(&before) != metadata_fingerprint(&after)
            || before.modified().ok() != after.modified().ok()
        {
            return Err("Command state directory changed while sealing".into());
        }
        Ok(())
    }

    fn file(&mut self, directory: &Dir, name: &str, relative: &Path) -> Result<(), String> {
        let mut file = crate::tools::workspace_bytes::open_regular_file(directory, name)
            .map_err(|_| "Command state file changed")?;
        let before = file
            .metadata()
            .map_err(|_| "Command state file is unavailable")?;
        if before.nlink() != 1 {
            return Err("Command state contains an externally aliased file".into());
        }
        self.bytes = self
            .bytes
            .checked_add(before.len())
            .ok_or("Command state exceeds its byte limit")?;
        if self.bytes > MAX_TREE_BYTES {
            return Err("Command state exceeds 512 MiB".into());
        }
        field(&mut self.hash, b"file");
        field(
            &mut self.hash,
            relative.as_os_str().to_string_lossy().as_bytes(),
        );
        hash_metadata(&mut self.hash, &before);
        self.hash.update(before.len().to_be_bytes());
        hash_permissions(
            &mut self.hash,
            &file
                .try_clone()
                .map_err(|_| "Command state file is unavailable")?
                .into_std(),
        )?;
        let mut buffer = [0; 64 * 1024];
        let mut read = 0;
        loop {
            self.check()?;
            let size = file
                .read(&mut buffer)
                .map_err(|_| "Command state file cannot be read")?;
            if size == 0 {
                break;
            }
            read += size as u64;
            if read > before.len() {
                return Err("Command state file changed while sealing".into());
            }
            self.hash.update(&buffer[..size]);
        }
        let after = file
            .metadata()
            .map_err(|_| "Command state file is unavailable")?;
        let named = directory
            .symlink_metadata(name)
            .map_err(|_| "Command state file changed")?;
        if read != before.len()
            || metadata_fingerprint(&before) != metadata_fingerprint(&after)
            || metadata_fingerprint(&before) != metadata_fingerprint(&named)
            || before.modified().ok() != after.modified().ok()
        {
            return Err("Command state file changed while sealing".into());
        }
        Ok(())
    }

    fn link(
        &mut self,
        directory: &Dir,
        name: &str,
        relative: &Path,
        parent: FileIdentity,
        metadata: &Metadata,
    ) -> Result<(), String> {
        let target = directory
            .read_link_contents(name)
            .map_err(|_| "Command state link is unavailable")?;
        if target.is_absolute()
            || target
                .components()
                .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
        {
            return Err("Command state link escapes the private environment".into());
        }
        // Resolution is relative to the held root capability. Parent traversals
        // and intermediate symlinks cannot escape it, including during races.
        let resolved = self
            .root
            .canonicalize(relative)
            .map_err(|_| "Command state link escapes or contains a cycle")?;
        let target_metadata = self
            .root
            .symlink_metadata(&resolved)
            .map_err(|_| "Command state link target is unavailable")?;
        if crate::tools::workspace_bytes::linked(&target_metadata)
            || (!target_metadata.is_file() && !target_metadata.is_dir())
        {
            return Err("Command state link target is unsupported".into());
        }
        if target_metadata.is_dir() {
            self.edges
                .entry(parent)
                .or_default()
                .push((target_metadata.dev(), target_metadata.ino()));
        }
        field(&mut self.hash, b"link");
        field(
            &mut self.hash,
            relative.as_os_str().to_string_lossy().as_bytes(),
        );
        field(
            &mut self.hash,
            target.as_os_str().to_string_lossy().as_bytes(),
        );
        field(
            &mut self.hash,
            resolved.as_os_str().to_string_lossy().as_bytes(),
        );
        hash_metadata(&mut self.hash, metadata);
        hash_metadata(&mut self.hash, &target_metadata);
        let after = directory
            .symlink_metadata(name)
            .map_err(|_| "Command state link changed")?;
        if metadata_fingerprint(metadata) != metadata_fingerprint(&after)
            || directory.read_link_contents(name).ok().as_ref() != Some(&target)
        {
            return Err("Command state link changed while sealing".into());
        }
        Ok(())
    }
}

fn reject_directory_cycles(root: FileIdentity, edges: &DirectoryEdges) -> Result<(), String> {
    fn visit(
        node: FileIdentity,
        edges: &DirectoryEdges,
        active: &mut HashSet<FileIdentity>,
        finished: &mut HashSet<FileIdentity>,
        depth: usize,
    ) -> Result<(), String> {
        if finished.contains(&node) {
            return Ok(());
        }
        if depth > MAX_DEPTH || !active.insert(node) {
            return Err("Command state contains cyclic directory links".into());
        }
        for target in edges.get(&node).into_iter().flatten() {
            visit(*target, edges, active, finished, depth + 1)?;
        }
        active.remove(&node);
        finished.insert(node);
        Ok(())
    }
    visit(root, edges, &mut HashSet::new(), &mut HashSet::new(), 0)
}

fn storage_failure(
    root: &crate::tools::workspace_bytes::WorkspaceBytes,
    name: &str,
    identity: FileIdentity,
    physical: &runtime::PhysicalLease,
    violation: StorageViolation,
    mut output: RuntimeOutput,
) -> CommandRuntimeResult {
    let detail = violation.message();
    if !output.cleanup_confirmed {
        return unprepared("command_failed", &format!("{detail}. Process retirement is unproven; the owned environment is retained and reuse remains blocked. External effects are not rolled back."), Some(output));
    }
    match remove_measured_environment(root, name, identity) {
        Ok(()) => unprepared("command_failed", &format!("{detail}. The process tree retired and its unusable owned environment was removed. Storage is sampled periodically and can transiently overshoot; this is not an OS quota and does not limit external writes or roll back external effects."), Some(output)),
        Err(error) => {
            // A physically retired child does not justify dropping the fence if
            // its over-budget owned storage cannot be proven removed.
            physical.settled.store(false, Ordering::Release);
            output.cleanup_confirmed = false;
            unprepared("command_failed", &format!("{detail}. The process tree retired, but owned storage cleanup could not be proven: {error}. Reuse remains blocked until reconciliation; external effects are not rolled back."), Some(output))
        }
    }
}

fn remove_measured_environment(
    root: &crate::tools::workspace_bytes::WorkspaceBytes,
    name: &str,
    identity: FileIdentity,
) -> Result<(), String> {
    if !valid_directory(name) {
        return Err("Runtime environment name is invalid".into());
    }
    let directory = root
        .held_directory()
        .map_err(|_| "Runtime storage is unavailable")?;
    let child = nofollow_child(&directory, Path::new(name))?;
    let metadata = child
        .dir_metadata()
        .map_err(|_| "Owned command environment is unavailable")?;
    if (metadata.dev(), metadata.ino()) != identity {
        return Err("Owned command environment identity changed".into());
    }
    drop(child);
    directory
        .remove_dir_all(name)
        .map_err(|_| "Owned command environment could not be removed")?;
    match directory.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("Owned command environment removal is unproven".into()),
    }
}

fn remove_environment(root: &Path, name: &str) -> Result<(), String> {
    if !valid_directory(name) {
        return Err("Runtime environment name is invalid".into());
    }
    let root = private_handle(root)?;
    let directory = root
        .held_directory()
        .map_err(|_| "Runtime storage is unavailable")?;
    let _child = directory
        .open_dir_nofollow(name)
        .map_err(|_| "Retained environment changed")?;
    // Capability-relative recursive deletion unlinks symlinks without following
    // them; an unproven operation never reaches this cleanup path.
    directory
        .remove_dir_all(name)
        .map_err(|_| "Retired command environment could not be removed".into())
}

#[cfg(test)]
#[path = "command_runtime_tests.rs"]
mod tests;
