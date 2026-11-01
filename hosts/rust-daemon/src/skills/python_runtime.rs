//! Explicitly approved local Python package execution. A venv separates dependencies;
//! it does not restrict the approved program's operating-system access.
use super::package::SkillPackage;
use anima_core::CancelSignal;
use anima_process::Custody;
use futures::future::BoxFuture;
use process_wrap::tokio::CommandWrap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};

pub(crate) type RuntimeAuthority =
    Arc<dyn Fn() -> BoxFuture<'static, Result<(), String>> + Send + Sync>;
const MAX_OUTPUT: usize = 8192;
const MAX_REQUIREMENTS: usize = 64 * 1024;
const OPERATION_MARKER: &str = "operation.active";
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static QUARANTINED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
// Captured from the operator's daemon environment, never from a model request.
static OPERATOR_TOOL_PATH: OnceLock<std::ffi::OsString> = OnceLock::new();

pub(crate) fn active_count() -> usize {
    ACTIVE.load(Ordering::Acquire)
}

/// Read-only startup/drain recovery. An unresolved marker or an unreadable
/// existing runtime root cannot be reported as physically idle.
pub(crate) fn persisted_marker_count(root: &Path) -> usize {
    // In-memory quarantine also belongs to the exact profile. A failed marker
    // removal must remain visible even if the marker was removed externally.
    let key = root
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .and_then(|parent| {
            root.file_name()
                .map(|name| parent.join(name).join(OPERATION_MARKER))
        });
    if QUARANTINED
        .get_or_init(Default::default)
        .lock()
        .map_or(true, |set| {
            key.as_ref().is_some_and(|key| set.contains(key))
        })
    {
        return 1;
    }
    match root.try_exists() {
        Ok(false) => 0,
        Ok(true) => match private_handle(root) {
            Ok(held) => usize::from(held.instruction_marker_exists(OPERATION_MARKER)),
            Err(_) => 1,
        },
        Err(_) => 1,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeAction {
    Status,
    Prepare,
    Run,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PythonRuntimeRequest {
    pub(crate) action: RuntimeAction,
    pub(crate) slug: String,
    pub(crate) hash: String,
    #[serde(default)]
    pub(crate) entrypoint: Option<String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    /// None discovers one requirements.txt; empty explicitly selects no dependencies.
    #[serde(default)]
    pub(crate) requirements: Option<String>,
    #[serde(default = "default_timeout")]
    pub(crate) timeout_ms: u64,
}
fn default_timeout() -> u64 {
    120_000
}
impl PythonRuntimeRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !super::is_valid_slug(&self.slug)
            || self.hash.len() != 64
            || !self
                .hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Select an exact approved skill slug and package hash".into());
        }
        if !(1_000..=300_000).contains(&self.timeout_ms) {
            return Err("Python runtime timeout must be between 1000 and 300000 ms".into());
        }
        if self.args.len() > 64
            || self.args.iter().map(String::len).sum::<usize>() > 8192
            || self
                .args
                .iter()
                .any(|arg| arg.contains('\0') || arg.len() > 2048)
        {
            return Err("Python runtime arguments exceed the allowed bounds".into());
        }
        if let Some(path) = &self.requirements {
            if !path.is_empty() {
                anima_core::skill_packages::validate_supporting_path(path)
                    .map_err(str::to_string)?;
                if !path.ends_with(".txt") {
                    return Err("Select a reviewed requirements .txt file".into());
                }
            }
        }
        match (&self.action, &self.entrypoint) {
            (RuntimeAction::Run, Some(path)) if path.ends_with(".py") => {
                anima_core::skill_packages::validate_supporting_path(path)
                    .map_err(str::to_string)?;
            }
            (RuntimeAction::Run, _) => {
                return Err("Run needs a reviewed relative Python .py entrypoint".into())
            }
            (_, Some(_)) => return Err("Only run accepts an entrypoint".into()),
            _ => {}
        }
        if self.action != RuntimeAction::Run && !self.args.is_empty() {
            return Err("Only run accepts script arguments".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeCheck {
    pub(crate) entrypoint: String,
    pub(crate) args: Vec<String>,
    pub(crate) exit_code: i32,
    pub(crate) checked_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PythonRuntimeResult {
    pub(crate) status: String,
    pub(crate) message: String,
    pub(crate) python: Option<String>,
    pub(crate) requirements: Option<String>,
    pub(crate) last_check: Option<RuntimeCheck>,
    pub(crate) output: Option<RuntimeOutput>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeOutput {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stop: Option<String>,
    pub(crate) cleanup_confirmed: bool,
}
impl RuntimeOutput {
    fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && self.stop.is_none() && self.cleanup_confirmed
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: u32,
    slug: String,
    hash: String,
    directory: String,
    interpreter: PathBuf,
    interpreter_hash: String,
    environment_python_hash: String,
    environment_hash: String,
    #[serde(default)]
    previous_directory: Option<String>,
    requirements: Option<String>,
    requirements_hash: Option<String>,
    last_check: Option<RuntimeCheck>,
}

struct Active;
impl Active {
    fn enter() -> Self {
        ACTIVE.fetch_add(1, Ordering::AcqRel);
        Self
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}
struct CancelOnDrop(CancelSignal);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// The durable marker is removed only after physical retirement is proven. A
/// crash or unknown cleanup blocks reuse and remains visible to drain/Doctor.
struct PhysicalLease {
    root: crate::tools::workspace_bytes::WorkspaceBytes,
    marker: String,
    key: PathBuf,
    settled: AtomicBool,
}
impl PhysicalLease {
    fn admit(root: &Path, marker: String) -> Result<Arc<Self>, String> {
        let held = private_handle(root)?;
        let key = root
            .canonicalize()
            .map_err(|_| "Runtime storage is unavailable")?
            .join(&marker);
        if held
            .create_new_bytes_atomic(
                &marker,
                b"owned Python runtime operation; reconcile exact process custody before removing",
                || Ok(()),
            )
            .is_err()
        {
            quarantine(&key);
            return Err("A previous Python runtime operation has unresolved physical custody. Reconcile it before another operation".into());
        }
        Ok(Arc::new(Self {
            root: held,
            marker,
            key,
            settled: AtomicBool::new(true),
        }))
    }
}
impl Drop for PhysicalLease {
    fn drop(&mut self) {
        let removed = self.settled.load(Ordering::Acquire)
            && self
                .root
                .held_directory()
                .and_then(|directory| directory.remove_file(&self.marker))
                .is_ok();
        if !removed {
            quarantine(&self.key);
        }
    }
}
fn quarantine(key: &Path) {
    if let Ok(mut set) = QUARANTINED.get_or_init(Default::default).lock() {
        set.insert(key.to_path_buf());
    }
}

struct OwnedEnvironment {
    root: PathBuf,
    directory: String,
    physical: Arc<PhysicalLease>,
    keep: bool,
}
impl Drop for OwnedEnvironment {
    fn drop(&mut self) {
        if !self.keep && self.physical.settled.load(Ordering::Acquire) {
            let _ = remove_owned_environment(&self.root, &self.directory);
        }
    }
}

fn lease(path: &Path) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    type Locks = HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>;
    static LOCKS: OnceLock<Mutex<Locks>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Runtime admission is unavailable")?;
    locks.retain(|_, value| value.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    Ok(lock)
}

/// Owns physical work until the exact process tree is retired even when a caller drops.
/// The authority callback must revalidate the live owner run, approval and package hash.
pub(crate) async fn execute(
    runtime_root: PathBuf,
    workspace_root: PathBuf,
    package: SkillPackage,
    request: PythonRuntimeRequest,
    cancel: CancelSignal,
    authority: RuntimeAuthority,
) -> Result<PythonRuntimeResult, String> {
    request.validate()?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(request.timeout_ms);
    anima_core::skill_packages::validate_skill_package(&package.bytes, &package.files)
        .map_err(str::to_string)?;
    if package.hash() != request.hash {
        return Err("Skill package changed; review it again".into());
    }
    let call_cancel = CancelSignal::new();
    let guard = CancelOnDrop(call_cancel.clone());
    let joined = tokio::spawn(async move {
        let _active = Active::enter();
        let locking = lease(&runtime_root)?;
        let _lease = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err("Skill runtime operation cancelled".into()),
            _ = call_cancel.cancelled() => return Err("Skill runtime operation cancelled".into()),
            _ = tokio::time::sleep_until(deadline) => return Err("Skill runtime operation timed out".into()),
            lease = locking.lock() => lease,
        };
        require_authority(&authority, deadline, &cancel, &call_cancel).await?;
        execute_owned(
            runtime_root,
            workspace_root,
            package,
            request,
            cancel,
            call_cancel,
            authority,
            deadline,
        )
        .await
    })
    .await
    .map_err(|_| {
        "Skill runtime worker failed with unresolved physical custody; reconcile before retrying"
    })?;
    drop(guard);
    joined
}

async fn execute_owned(
    root: PathBuf,
    workspace: PathBuf,
    package: SkillPackage,
    request: PythonRuntimeRequest,
    cancel: CancelSignal,
    call_cancel: CancelSignal,
    authority: RuntimeAuthority,
    deadline: tokio::time::Instant,
) -> Result<PythonRuntimeResult, String> {
    ensure_operation_live(deadline, &cancel, &call_cancel)?;
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "Skill workspace is unavailable")?;
    if !root.is_absolute() {
        return Err("Runtime storage must be an absolute private directory".into());
    }
    let parent = root
        .parent()
        .ok_or("Runtime storage has no parent")?
        .canonicalize()
        .map_err(|_| "Runtime storage parent is unavailable")?;
    let root = parent.join(root.file_name().ok_or("Runtime storage name is invalid")?);
    if root.starts_with(&workspace) || workspace.starts_with(&root) {
        return Err("Runtime storage must be outside the agent workspace".into());
    }
    let helper_dirs = sanitized_helper_directories(
        OPERATOR_TOOL_PATH.get_or_init(|| std::env::var_os("PATH").unwrap_or_default()),
        &workspace,
        &root,
    );
    let requirements = select_requirements(&package, request.requirements.as_deref())?;
    if let Some((_, bytes)) = &requirements {
        validate_requirements(bytes)?;
    }
    let manifest_name = format!("{}-{}.json", request.slug, request.hash);
    if request.action == RuntimeAction::Status && !root.exists() {
        return Ok(PythonRuntimeResult { status: "installed".into(), message: "Original package is installed. No Python environment or successful command has been verified. Use prepare, then run an actual preflight entrypoint.".into(), python: discover_interpreter(&workspace).ok().map(|path| path.display().to_string()), requirements: requirements.map(|(path, _)| path), last_check: None, output: None });
    }
    crate::document_edits::store::initialize_owned_directory(&root)?;
    let root_handle = private_handle(&root)?;
    let marker = OPERATION_MARKER.to_string();
    if persisted_marker_count(&root) != 0 {
        quarantine(&root.join(&marker));
        return Err("A previous Python runtime operation has unresolved physical custody. Reconcile it before another operation".into());
    }
    let prior = read_manifest(&root_handle, &manifest_name)?;
    if request.action == RuntimeAction::Status {
        return match prior {
            Some(manifest) => match validate_manifest(&root, &workspace, &package, &request, &manifest, &requirements) {
                Ok(()) => Ok(result_from_manifest(&manifest, None)),
                Err(_) => Ok(PythonRuntimeResult { status: "needs_prepare".into(), message: "The prepared environment, interpreter or source snapshot changed. Prepare again before running.".into(), python: None, requirements: requirements.map(|(path, _)| path), last_check: None, output: None }),
            },
            None => Ok(PythonRuntimeResult { status: "installed".into(), message: "Original package is installed; Python dependencies and a real command have not been verified.".into(), python: discover_interpreter(&workspace).ok().map(|path| path.display().to_string()), requirements: requirements.map(|(path, _)| path), last_check: None, output: None }),
        };
    }
    let physical = PhysicalLease::admit(&root, marker)?;
    match request.action {
        RuntimeAction::Prepare => {
            let interpreter = discover_interpreter(&workspace)?;
            let interpreter_hash = file_hash(&interpreter)?;
            let probe = launch(
                &interpreter,
                vec![
                    "-I".into(),
                    "-c".into(),
                    "import sys,json; print(json.dumps(list(sys.version_info[:3])))".into(),
                ],
                &root,
                &root,
                deadline,
                &cancel,
                &call_cancel,
                &authority,
                &physical,
                &helper_dirs,
            )
            .await?;
            if !probe.succeeded() {
                return Ok(failed_result(
                    "interpreter_failed",
                    "The selected Python interpreter could not be verified",
                    requirements.as_ref().map(|(path, _)| path.clone()),
                    probe,
                ));
            }
            let version: Vec<u32> = serde_json::from_str(probe.stdout.trim())
                .map_err(|_| "Python version probe returned an invalid response")?;
            if version.len() != 3 || version[0] != 3 || version[1] < 12 {
                return Err("Managed skill execution needs Python 3.12 or newer; configure ANIMAOS_SKILL_PYTHON to a trusted installed interpreter".into());
            }
            // A new environment prevents an earlier skill run from supplying mutated dependencies.
            let directory = format!("r-{}", uuid::Uuid::new_v4().simple());
            check_environment_capacity(&root_handle)?;
            let environment = root.join(&directory);
            crate::document_edits::store::initialize_owned_directory(&environment)?;
            let mut owned = OwnedEnvironment {
                root: root.clone(),
                directory: directory.clone(),
                physical: physical.clone(),
                keep: false,
            };
            let _held_environment = private_handle(&environment)?;
            super::package::write_package(&environment, &request.slug, &package)?;
            let home = environment.join("home");
            crate::document_edits::store::initialize_owned_directory(&home)?;
            let venv = environment.join("venv");
            let created = launch(
                &interpreter,
                vec![
                    "-I".into(),
                    "-m".into(),
                    "venv".into(),
                    "--copies".into(),
                    venv.to_string_lossy().into_owned(),
                ],
                &environment,
                &home,
                deadline,
                &cancel,
                &call_cancel,
                &authority,
                &physical,
                &helper_dirs,
            )
            .await?;
            if !created.succeeded() {
                return Ok(failed_result(
                    "environment_failed",
                    "Python environment creation failed; the previous environment remains selected",
                    requirements.as_ref().map(|(path, _)| path.clone()),
                    created,
                ));
            }
            let python = environment_python(&environment);
            reject_linked_file(&python)?;
            if let Some((path, _)) = &requirements {
                let requirement_file = environment.join("skills").join(&request.slug).join(path);
                let installed = launch(
                    &python,
                    vec![
                        "-I".into(),
                        "-m".into(),
                        "pip".into(),
                        "--isolated".into(),
                        "--disable-pip-version-check".into(),
                        "--no-input".into(),
                        "--no-cache-dir".into(),
                        "install".into(),
                        "--index-url".into(),
                        "https://pypi.org/simple".into(),
                        "--only-binary=:all:".into(),
                        "-r".into(),
                        requirement_file.to_string_lossy().into_owned(),
                    ],
                    &environment,
                    &home,
                    deadline,
                    &cancel,
                    &call_cancel,
                    &authority,
                    &physical,
                    &helper_dirs,
                )
                .await?;
                if !installed.succeeded() {
                    return Ok(failed_result("dependencies_failed", "Reviewed Python dependencies could not be installed; the previous environment remains selected", Some(path.clone()), installed));
                }
            }
            let manifest = Manifest {
                version: 1,
                slug: request.slug.clone(),
                hash: request.hash.clone(),
                directory,
                interpreter,
                interpreter_hash,
                environment_python_hash: file_hash(&python)?,
                environment_hash: environment_hash(&environment)?,
                previous_directory: prior.as_ref().map(|old| old.directory.clone()),
                requirements: requirements.as_ref().map(|(path, _)| path.clone()),
                requirements_hash: requirements.as_ref().map(|(_, bytes)| digest(bytes)),
                last_check: None,
            };
            validate_manifest(
                &root,
                &workspace,
                &package,
                &request,
                &manifest,
                &requirements,
            )?;
            require_authority(&authority, deadline, &cancel, &call_cancel).await?;
            ensure_operation_live(deadline, &cancel, &call_cancel)?;
            write_manifest(&root_handle, &manifest_name, prior.as_ref(), &manifest)?;
            owned.keep = true;
            if let Some(old) = prior.and_then(|old| old.previous_directory) {
                remove_owned_environment(&root, &old)?;
            }
            Ok(result_from_manifest(&manifest, None))
        }
        RuntimeAction::Run => {
            let mut manifest = prior.ok_or("Prepare this exact package before running it")?;
            validate_manifest(
                &root,
                &workspace,
                &package,
                &request,
                &manifest,
                &requirements,
            )?;
            let entrypoint = request
                .entrypoint
                .clone()
                .ok_or("Python entrypoint is missing")?;
            let entry = package
                .files
                .iter()
                .find(|file| file.path == entrypoint)
                .ok_or("Entrypoint is not part of the approved package")?;
            let _ = entry.bytes().map_err(str::to_string)?;
            let previous = manifest.clone();
            manifest.last_check = None;
            require_authority(&authority, deadline, &cancel, &call_cancel).await?;
            ensure_operation_live(deadline, &cancel, &call_cancel)?;
            write_manifest(&root_handle, &manifest_name, Some(&previous), &manifest)?;
            let environment = root.join(&manifest.directory);
            let cwd = environment.join("skills").join(&request.slug);
            let mut args = vec![
                "-E".into(),
                "-s".into(),
                "-B".into(),
                cwd.join(&entrypoint).to_string_lossy().into_owned(),
            ];
            args.extend(request.args.clone());
            let output = launch(
                &environment_python(&environment),
                args,
                &cwd,
                &environment.join("home"),
                deadline,
                &cancel,
                &call_cancel,
                &authority,
                &physical,
                &helper_dirs,
            )
            .await?;
            if output.succeeded() {
                // A successful leader that mutated the approved source is not a valid preflight.
                validate_manifest(
                    &root,
                    &workspace,
                    &package,
                    &request,
                    &manifest,
                    &requirements,
                )?;
                let mut checked = manifest.clone();
                checked.last_check = Some(RuntimeCheck {
                    entrypoint,
                    args: request.args,
                    exit_code: 0,
                    checked_at: anima_core::primitives::now_millis(),
                });
                require_authority(&authority, deadline, &cancel, &call_cancel).await?;
                ensure_operation_live(deadline, &cancel, &call_cancel)?;
                write_manifest(&root_handle, &manifest_name, Some(&manifest), &checked)?;
                Ok(result_from_manifest(&checked, Some(output)))
            } else {
                Ok(failed_result(
                    "command_failed",
                    "The command did not pass. No successful preflight is recorded",
                    manifest.requirements,
                    output,
                ))
            }
        }
        RuntimeAction::Status => unreachable!(),
    }
}

fn result_from_manifest(manifest: &Manifest, output: Option<RuntimeOutput>) -> PythonRuntimeResult {
    PythonRuntimeResult { status: if manifest.last_check.is_some() { "command_verified" } else { "environment_prepared" }.into(), message: if manifest.last_check.is_some() { "The recorded exact command completed successfully. This does not verify account access or other commands." } else { "The Python environment is prepared. Run the package's actual preflight before claiming it works." }.into(), python: Some(manifest.interpreter.display().to_string()), requirements: manifest.requirements.clone(), last_check: manifest.last_check.clone(), output }
}
fn failed_result(
    status: &str,
    message: &str,
    requirements: Option<String>,
    output: RuntimeOutput,
) -> PythonRuntimeResult {
    PythonRuntimeResult {
        status: status.into(),
        message: message.into(),
        python: None,
        requirements,
        last_check: None,
        output: Some(output),
    }
}
fn private_handle(path: &Path) -> Result<crate::tools::workspace_bytes::WorkspaceBytes, String> {
    let held = crate::tools::workspace_bytes::WorkspaceBytes::open(path)
        .map_err(|_| "Runtime storage is unavailable")?;
    crate::document_edits::store::verify_private_directory(path, &held, false)?;
    Ok(held)
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn file_hash(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let parent = path.parent().ok_or("Runtime executable has no parent")?;
    let held = crate::tools::workspace_bytes::WorkspaceBytes::open(parent)
        .map_err(|_| "Runtime executable parent is unavailable")?;
    let directory = held
        .held_directory()
        .map_err(|_| "Runtime executable parent is unavailable")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Runtime executable has no filename")?;
    let mut file = crate::tools::workspace_bytes::open_regular_file(&directory, name)
        .map_err(|_| "Runtime executable is unavailable")?;
    if file
        .metadata()
        .map_err(|_| "Runtime executable metadata is unavailable")?
        .len()
        > 64 * 1024 * 1024
    {
        return Err("Runtime executable exceeds 64 MiB".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Runtime executable could not be read")?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Runtime executable exceeds 64 MiB".into());
    }
    Ok(digest(&bytes))
}
fn reject_linked_file(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "Runtime executable is unavailable")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Runtime executable must be a regular file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Runtime executable must not be redirected".into());
        }
    }
    Ok(())
}
fn trusted_interpreter(path: &Path, workspace: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "Python interpreter is unavailable")?;
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "Skill workspace is unavailable")?;
    if canonical.starts_with(&workspace) || !canonical.is_absolute() {
        return Err("Python interpreter must be installed outside the agent workspace".into());
    }
    reject_linked_file(&canonical)?;
    Ok(canonical)
}
fn discover_interpreter(workspace: &Path) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ANIMAOS_SKILL_PYTHON") {
        return trusted_interpreter(Path::new(&path), workspace);
    }
    let paths = std::env::var_os("PATH")
        .ok_or("Python is not installed; configure ANIMAOS_SKILL_PYTHON")?;
    for directory in std::env::split_paths(&paths).filter(|path| path.is_absolute()) {
        for name in if cfg!(windows) {
            ["python.exe", "python3.exe"]
        } else {
            ["python3", "python"]
        } {
            if let Ok(path) = trusted_interpreter(&directory.join(name), workspace) {
                return Ok(path);
            }
        }
    }
    Err("Python 3.12 or newer is not installed; configure ANIMAOS_SKILL_PYTHON to a trusted installed interpreter".into())
}
fn environment_python(environment: &Path) -> PathBuf {
    if cfg!(windows) {
        environment.join("venv/Scripts/python.exe")
    } else {
        environment.join("venv/bin/python")
    }
}

fn sanitized_helper_directories(
    path: &std::ffi::OsStr,
    workspace: &Path,
    runtime_root: &Path,
) -> Vec<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let runtime_root = runtime_root
        .canonicalize()
        .unwrap_or_else(|_| runtime_root.to_path_buf());
    let mut safe = Vec::new();
    for directory in std::env::split_paths(path) {
        if !directory.is_absolute() || has_project_shim_component(&directory) {
            continue;
        }
        let Ok(canonical) = directory.canonicalize() else {
            continue;
        };
        if !canonical.is_dir()
            || directory_is_within(&canonical, &workspace)
            || directory_is_within(&canonical, &runtime_root)
            || has_project_shim_component(&canonical)
        {
            continue;
        }
        append_unique_directories(&mut safe, &[canonical]);
    }
    safe
}
fn has_project_shim_component(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("node_modules"))
    })
}
fn directory_key(path: &Path) -> std::ffi::OsString {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    {
        path.as_os_str().to_string_lossy().to_lowercase().into()
    }
    #[cfg(not(windows))]
    {
        path.into_os_string()
    }
}
fn directory_is_within(path: &Path, root: &Path) -> bool {
    Path::new(&directory_key(path)).starts_with(Path::new(&directory_key(root)))
}
fn append_unique_directories(path: &mut Vec<PathBuf>, directories: &[PathBuf]) {
    let mut seen = HashSet::new();
    path.retain(|directory| seen.insert(directory_key(directory)));
    for directory in directories {
        if seen.insert(directory_key(directory)) {
            path.push(directory.clone());
        }
    }
}
fn select_requirements(
    package: &SkillPackage,
    selected: Option<&str>,
) -> Result<Option<(String, Vec<u8>)>, String> {
    let candidates = package
        .files
        .iter()
        .filter(|file| file.path == "requirements.txt" || file.path.ends_with("/requirements.txt"))
        .collect::<Vec<_>>();
    let path = match selected {
        Some("") => return Ok(None),
        Some(path) => path,
        None if candidates.len() == 1 => candidates[0].path.as_str(),
        None if candidates.len() > 1 => return Err("This package has multiple requirements.txt files. Select the reviewed dependency file explicitly".into()),
        None if package.files.iter().any(|file| file.path == "pyproject.toml" || file.path.ends_with("/pyproject.toml") || file.path == "Pipfile") => return Err("This package declares Python dependencies outside requirements.txt. Inspect its setup instructions, select a reviewed requirements file, or explicitly choose an empty requirements string for no dependencies".into()),
        None => return Ok(None),
    };
    let file = package
        .files
        .iter()
        .find(|file| file.path == path)
        .ok_or("Requirements file is not part of the reviewed package")?;
    let bytes = file.bytes().map_err(str::to_string)?;
    Ok(Some((path.into(), bytes.into_owned())))
}
fn validate_requirements(bytes: &[u8]) -> Result<(), String> {
    let invalid = || {
        "Managed dependencies support at most 64 ordinary PyPI distribution requirements. URLs, editable/local installs, recursive includes and pip options are unsupported".to_string()
    };
    if bytes.len() > MAX_REQUIREMENTS {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut count = 0;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        count += 1;
        let line = line.split(" #").next().unwrap_or(line).trim();
        if count > 64
            || !line.as_bytes()[0].is_ascii_alphanumeric()
            || line.contains([':', '/', '\\', '@', '\0'])
            || line.contains("--")
            || line
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && !b"._-[]<>=!~,; '\"()*+".contains(&b))
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn read_manifest(
    held: &crate::tools::workspace_bytes::WorkspaceBytes,
    name: &str,
) -> Result<Option<Manifest>, String> {
    use crate::tools::workspace_bytes::InstructionReadError;
    match held.read_instruction_file(name, 64 * 1024) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Runtime manifest is invalid".into()),
        Err(InstructionReadError::Missing) => Ok(None),
        Err(_) => Err("Runtime manifest is unavailable".into()),
    }
}
fn write_manifest(
    held: &crate::tools::workspace_bytes::WorkspaceBytes,
    name: &str,
    expected: Option<&Manifest>,
    value: &Manifest,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Runtime manifest cannot be encoded")?;
    if expected.is_some() {
        let prior = held
            .read_instruction_file(name, 64 * 1024)
            .map_err(|_| "Runtime manifest changed")?;
        let current: Manifest =
            serde_json::from_slice(&prior).map_err(|_| "Runtime manifest changed")?;
        if serde_json::to_vec(&current).ok()
            != expected.and_then(|value| serde_json::to_vec(value).ok())
        {
            return Err("Runtime manifest changed".into());
        }
        held.compare_replace_instruction_file(name, &prior, &bytes, || Ok(()))
            .map_err(|_| "Runtime manifest could not be saved".to_string())
    } else {
        held.create_new_bytes_atomic(name, &bytes, || Ok(()))
            .map_err(|_| "Runtime manifest could not be saved".to_string())
    }
}
fn validate_manifest(
    root: &Path,
    workspace: &Path,
    package: &SkillPackage,
    request: &PythonRuntimeRequest,
    manifest: &Manifest,
    requirements: &Option<(String, Vec<u8>)>,
) -> Result<(), String> {
    if manifest.version != 1
        || manifest.slug != request.slug
        || manifest.hash != request.hash
        || !manifest.directory.starts_with("r-")
        || manifest.directory.len() != 34
        || !manifest.directory[2..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || manifest.requirements != requirements.as_ref().map(|(path, _)| path.clone())
        || manifest.requirements_hash != requirements.as_ref().map(|(_, bytes)| digest(bytes))
    {
        return Err(
            "The prepared environment does not match the approved package and dependency plan"
                .into(),
        );
    }
    let environment = root.join(&manifest.directory);
    let _held = private_handle(&environment)?;
    let _home = private_handle(&environment.join("home"))?;
    if trusted_interpreter(&manifest.interpreter, workspace)? != manifest.interpreter
        || file_hash(&manifest.interpreter)? != manifest.interpreter_hash
        || file_hash(&environment_python(&environment))? != manifest.environment_python_hash
    {
        return Err("The prepared Python interpreter changed; prepare again".into());
    }
    let snapshot = super::package::read_package(&environment, &manifest.slug)?
        .ok_or("Prepared source snapshot is unavailable")?;
    if snapshot.hash() != package.hash() {
        return Err("The prepared source snapshot changed; prepare again".into());
    }
    if environment_hash(&environment)? != manifest.environment_hash {
        return Err("Prepared Python dependencies or configuration changed; prepare again".into());
    }
    Ok(())
}

fn valid_environment_name(name: &str) -> bool {
    name.len() == 34
        && name.starts_with("r-")
        && name[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn check_environment_capacity(
    root: &crate::tools::workspace_bytes::WorkspaceBytes,
) -> Result<(), String> {
    let directory = root
        .held_directory()
        .map_err(|_| "Runtime storage is unavailable")?;
    let mut count = 0;
    for entry in directory
        .entries()
        .map_err(|_| "Runtime storage is unavailable")?
    {
        let entry = entry.map_err(|_| "Runtime storage is unavailable")?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(valid_environment_name)
        {
            count += 1;
        }
        if count >= 16 {
            return Err("Managed Python storage already has 16 environments. Reconcile retained environments before preparing another".into());
        }
    }
    Ok(())
}
fn remove_owned_environment(root: &Path, name: &str) -> Result<(), String> {
    if !valid_environment_name(name) {
        return Err("Runtime environment name is invalid".into());
    }
    let held = private_handle(root)?;
    let child = private_handle(&root.join(name))?;
    // Inspect without following links before recursive removal. Never delete an
    // unexpected redirected tree, including a tree changed by an approved script.
    inspect_tree(
        &child
            .held_directory()
            .map_err(|_| "Runtime storage is unavailable")?,
        "",
        &mut 0,
        &mut 0,
        None,
        0,
    )?;
    drop(child);
    held.held_directory()
        .map_err(|_| "Runtime storage is unavailable")?
        .remove_dir_all(name)
        .map_err(|_| "Retired Python environment could not be removed".into())
}
fn environment_hash(environment: &Path) -> Result<String, String> {
    let held = crate::tools::workspace_bytes::WorkspaceBytes::open(&environment.join("venv"))
        .map_err(|_| "Python environment is unavailable")?;
    let mut hash = Sha256::new();
    hash.update(b"anima-python-environment-v1\0");
    inspect_tree(
        &held
            .held_directory()
            .map_err(|_| "Python environment is unavailable")?,
        "",
        &mut 0,
        &mut 0,
        Some(&mut hash),
        0,
    )?;
    Ok(hex::encode(hash.finalize()))
}
fn inspect_tree(
    directory: &cap_std::fs::Dir,
    prefix: &str,
    count: &mut usize,
    total: &mut u64,
    mut hash: Option<&mut Sha256>,
    depth: usize,
) -> Result<(), String> {
    use cap_fs_ext::{DirExt, MetadataExt};
    use std::io::Read;
    if depth > 32 {
        return Err("Python environment nesting exceeds the limit".into());
    }
    let mut names = directory
        .entries()
        .map_err(|_| "Python environment is unavailable")?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Python environment is unavailable")?;
    names.sort();
    for name in names {
        *count += 1;
        if *count > 20_000 {
            return Err("Python environment exceeds 20000 filesystem entries".into());
        }
        let name = name
            .to_str()
            .ok_or("Python environment has a nonportable filename")?;
        let path = format!("{prefix}/{name}");
        let metadata = directory
            .symlink_metadata(name)
            .map_err(|_| "Python environment is unavailable")?;
        if crate::tools::workspace_bytes::linked(&metadata) {
            return Err("Python environment contains a redirected path".into());
        }
        if metadata.is_dir() {
            if let Some(hash) = hash.as_deref_mut() {
                hash.update(b"d");
                hash.update((path.len() as u64).to_be_bytes());
                hash.update(path.as_bytes());
            }
            let child = directory
                .open_dir_nofollow(name)
                .map_err(|_| "Python environment directory is unavailable")?;
            inspect_tree(&child, &path, count, total, hash.as_deref_mut(), depth + 1)?;
        } else if metadata.is_file() {
            let mut file = crate::tools::workspace_bytes::open_regular_file(directory, name)
                .map_err(|_| "Python environment file is unavailable")?;
            let before = file
                .metadata()
                .map_err(|_| "Python environment file is unavailable")?;
            if before.nlink() != 1 {
                return Err("Python environment contains an aliased file".into());
            }
            *total = total
                .checked_add(before.len())
                .ok_or("Python environment is too large")?;
            if *total > 512 * 1024 * 1024 {
                return Err("Python environment exceeds 512 MiB".into());
            }
            if let Some(hash) = hash.as_deref_mut() {
                hash.update(b"f");
                hash.update((path.len() as u64).to_be_bytes());
                hash.update(path.as_bytes());
                hash.update(before.len().to_be_bytes());
                let mut buffer = [0; 64 * 1024];
                let mut read = 0;
                loop {
                    let size = file
                        .read(&mut buffer)
                        .map_err(|_| "Python environment file cannot be read")?;
                    if size == 0 {
                        break;
                    }
                    read += size as u64;
                    if read > before.len() {
                        return Err("Python environment changed while reading".into());
                    }
                    hash.update(&buffer[..size]);
                }
                let after = file
                    .metadata()
                    .map_err(|_| "Python environment file is unavailable")?;
                if read != before.len()
                    || before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                {
                    return Err("Python environment changed while reading".into());
                }
            }
        } else {
            return Err("Python environment contains a nonregular file".into());
        }
    }
    Ok(())
}

async fn capture(mut reader: impl AsyncRead + Unpin) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(size) => {
                bytes.extend_from_slice(&buffer[..size.min(MAX_OUTPUT.saturating_sub(bytes.len()))])
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
async fn launch(
    executable: &Path,
    args: Vec<String>,
    cwd: &Path,
    home: &Path,
    deadline: tokio::time::Instant,
    cancel: &CancelSignal,
    call: &CancelSignal,
    authority: &RuntimeAuthority,
    physical: &PhysicalLease,
    helper_dirs: &[PathBuf],
) -> Result<RuntimeOutput, String> {
    ensure_operation_live(deadline, cancel, call)?;
    let (mut custody, mut native) = Custody::prepare(executable, args);
    custody
        .require_strong()
        .map_err(|error| error.to_string())?;
    native
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    // Keep OS discovery and sanitized installed helper directories only. Never
    // inherit provider keys, user homes, pip config or Python hooks.
    for name in ["SystemRoot", "WINDIR", "LANG", "LC_ALL"] {
        if let Some(value) = std::env::var_os(name) {
            native.env(name, value);
        }
    }
    native
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("APPDATA", home)
        .env("LOCALAPPDATA", home)
        .env("TEMP", home)
        .env("TMP", home)
        .env(
            "PIP_CONFIG_FILE",
            if cfg!(windows) { "nul" } else { "/dev/null" },
        )
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");
    let mut path = vec![executable
        .parent()
        .ok_or("Python executable parent is unavailable")?
        .to_path_buf()];
    #[cfg(windows)]
    if let Some(system) = std::env::var_os("SystemRoot") {
        path.push(PathBuf::from(system).join("System32"));
    }
    #[cfg(not(windows))]
    path.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    append_unique_directories(&mut path, helper_dirs);
    native.env(
        "PATH",
        std::env::join_paths(path).map_err(|_| "Python runtime path is invalid")?,
    );
    let mut wrapped = CommandWrap::from(native);
    custody.wrap(&mut wrapped);
    require_authority(authority, deadline, cancel, call).await?;
    ensure_operation_live(deadline, cancel, call)?;
    physical.settled.store(false, Ordering::Release);
    let mut child = match wrapped.spawn() {
        Ok(child) => child,
        Err(_) => {
            // A wrapper can fail after the native child starts. Preserve the
            // durable marker instead of assuming no physical process exists.
            return Err("Python runtime launch has unresolved physical custody; reconcile it before retrying".into());
        }
    };
    custody.attach(child.as_ref());
    let (Some(stdout), Some(stderr)) = (child.stdout().take(), child.stderr().take()) else {
        physical
            .settled
            .store(custody.retire(&mut child).await, Ordering::Release);
        return Err("Python runtime output capture is unavailable".into());
    };
    let mut stdout = tokio::spawn(capture(stdout));
    let mut stderr = tokio::spawn(capture(stderr));
    let mut stop = None;
    let mut authority_poll = tokio::time::Instant::now() + Duration::from_millis(250);
    let status = loop {
        if cancel.is_cancelled() || call.is_cancelled() {
            stop = Some("cancelled".into());
            break None;
        }
        if tokio::time::Instant::now() >= deadline {
            stop = Some("timed_out".into());
            break None;
        }
        if tokio::time::Instant::now() >= authority_poll {
            if require_authority(authority, deadline, cancel, call)
                .await
                .is_err()
            {
                stop = Some(
                    if cancel.is_cancelled() || call.is_cancelled() {
                        "cancelled"
                    } else if tokio::time::Instant::now() >= deadline {
                        "timed_out"
                    } else {
                        "authority_revoked"
                    }
                    .into(),
                );
                break None;
            }
            authority_poll = tokio::time::Instant::now() + Duration::from_millis(250);
        }
        match child.inner_mut().try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => {
                stop = Some("process_failed".into());
                break None;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let mut cleanup = custody.retire(&mut child).await;
    let stdout = match tokio::time::timeout(Duration::from_secs(2), &mut stdout).await {
        Ok(Ok(value)) => value,
        _ => {
            stdout.abort();
            let _ = stdout.await;
            cleanup = false;
            String::new()
        }
    };
    let stderr = match tokio::time::timeout(Duration::from_secs(2), &mut stderr).await {
        Ok(Ok(value)) => value,
        _ => {
            stderr.abort();
            let _ = stderr.await;
            cleanup = false;
            String::new()
        }
    };
    let exit_code = status.and_then(|value| value.code());
    cleanup &= !custody.join_failed(
        exit_code.unwrap_or(-1),
        !stdout.is_empty() || !stderr.is_empty(),
    );
    physical.settled.store(cleanup, Ordering::Release);
    Ok(RuntimeOutput {
        exit_code,
        stdout,
        stderr,
        stop,
        cleanup_confirmed: cleanup,
    })
}

fn ensure_operation_live(
    deadline: tokio::time::Instant,
    cancel: &CancelSignal,
    call: &CancelSignal,
) -> Result<(), String> {
    if cancel.is_cancelled() || call.is_cancelled() {
        return Err("Skill runtime operation cancelled".into());
    }
    if tokio::time::Instant::now() >= deadline {
        return Err("Skill runtime operation timed out".into());
    }
    Ok(())
}

async fn require_authority(
    authority: &RuntimeAuthority,
    deadline: tokio::time::Instant,
    cancel: &CancelSignal,
    call: &CancelSignal,
) -> Result<(), String> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("Skill runtime operation cancelled".into()),
        _ = call.cancelled() => Err("Skill runtime operation cancelled".into()),
        _ = tokio::time::sleep_until(deadline) => Err("Skill runtime operation timed out".into()),
        result = (authority)() => {
            result?;
            ensure_operation_live(deadline, cancel, call)
        },
    }
}

#[cfg(test)]
#[path = "python_runtime_tests.rs"]
mod tests;
