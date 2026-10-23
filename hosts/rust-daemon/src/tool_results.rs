//! Private, immutable retained tool output. This store never executes saved data.
use anima_core::{Content, TaskResult};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const INLINE_LIMIT: usize = 16 * 1024;
pub(crate) const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_PAGE_BYTES: usize = 8 * 1024;
const MAX_FILE_BYTES: usize = 2 * MAX_ARTIFACT_BYTES + 16 * 1024;
const UNAVAILABLE: &str = "Saved tool result unavailable or integrity check failed";
static WRITES: Mutex<()> = Mutex::new(());

#[derive(Clone)]
pub(crate) struct ToolResultStore {
    root: PathBuf,
    max_count: usize,
    max_bytes: u64,
    owner: Option<crate::profile_owner::ProfileOwner>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolResultArtifact {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) run_id: String,
    pub(crate) tool_call_id: String,
    pub(crate) tool_name: String,
    pub(crate) bytes: usize,
    pub(crate) sha256: String,
    pub(crate) capture_complete: bool,
    pub(crate) created_at_ms: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolResultPage {
    pub(crate) artifact: ToolResultArtifact,
    pub(crate) text: String,
    pub(crate) offset: usize,
    pub(crate) next_offset: Option<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    workspace: String,
    artifact: ToolResultArtifact,
    /// Serialized once so paging and download return exactly the hashed bytes.
    payload: String,
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn workspace_key(path: &Path) -> Result<String, String> {
    let root =
        crate::tools::workspace_bytes::WorkspaceBytes::open(path).map_err(|_| UNAVAILABLE)?;
    Ok(hash(root.canonical_root.to_string_lossy().as_bytes()))
}
fn valid_id(id: &str) -> bool {
    id.len() == 67
        && id.starts_with("tr_")
        && id[3..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identity(agent: &str, workspace: &str, run: &str, call: &str, name: &str, sha: &str) -> String {
    format!(
        "tr_{}",
        hash(
            &serde_json::to_vec(&(agent, workspace, run, call, name, sha))
                .expect("string tuple serializes")
        )
    )
}
fn incomplete(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            (matches!(
                key.as_str(),
                "truncated" | "incomplete" | "outputTruncated" | "output_truncated" | "shortened"
            ) && value == &serde_json::Value::Bool(true))
                || (key == "omittedBlocks" && value.as_u64().is_some_and(|n| n > 0))
                || incomplete(value)
        }),
        serde_json::Value::Array(items) => items.iter().any(incomplete),
        _ => false,
    }
}
fn text_capture_complete(text: &str) -> bool {
    !text.contains("[output truncated]")
        && !text.contains("[Truncated:")
        && !text.contains("[truncated]")
        && !text.contains("[output omitted")
        && !text.contains(" lines omitted]")
        && !text.contains("[response exceeded byte limit; truncated]")
        && !text.contains("[Output incomplete:")
        && !serde_json::from_str::<serde_json::Value>(text).is_ok_and(|v| incomplete(&v))
}
fn capture_complete(result: &TaskResult<Content>) -> bool {
    result.error.as_deref().is_none_or(text_capture_complete)
        && result
            .data
            .as_ref()
            .is_none_or(|content| text_capture_complete(&content.text))
        && !result
            .data
            .as_ref()
            .and_then(|c| c.metadata.as_ref())
            .is_some_and(|metadata| {
                metadata.iter().any(|(k, v)| {
                    matches!(
                        k.as_str(),
                        "truncated"
                            | "outputTruncated"
                            | "outputIncomplete"
                            | "incomplete"
                            | "shortened"
                    ) && v == &anima_core::DataValue::Bool(true)
                        || k == "omittedBlocks"
                            && matches!(v,anima_core::DataValue::Number(n) if *n > 0.0)
                })
            })
}

/// Walk from the volume anchor through held nofollow handles, including all
/// ancestors of a configured store. No host paths are accepted from a reader.
fn open_directory(root: &Path, create: bool) -> Result<Dir, String> {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir().map_err(|_| UNAVAILABLE)?.join(root)
    };
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for part in absolute.components() {
        match part {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                anchor.push(part.as_os_str())
            }
            std::path::Component::Normal(name) => names.push(name.to_os_string()),
            _ => return Err(UNAVAILABLE.into()),
        }
    }
    if !anchor.is_absolute() {
        return Err(UNAVAILABLE.into());
    }
    let mut dir =
        Dir::open_ambient_dir(anchor, cap_std::ambient_authority()).map_err(|_| UNAVAILABLE)?;
    for name in names {
        if create {
            match dir.create_dir(&name) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(UNAVAILABLE.into()),
            }
        }
        let meta = dir.symlink_metadata(&name).map_err(|_| UNAVAILABLE)?;
        if !meta.is_dir() || crate::tools::workspace_bytes::linked(&meta) {
            return Err(UNAVAILABLE.into());
        }
        dir = dir.open_dir_nofollow(name).map_err(|_| UNAVAILABLE)?;
        if crate::tools::workspace_bytes::linked(&dir.dir_metadata().map_err(|_| UNAVAILABLE)?) {
            return Err(UNAVAILABLE.into());
        }
    }
    #[cfg(unix)]
    if create {
        use cap_std::fs::PermissionsExt;
        dir.set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))
            .map_err(|_| UNAVAILABLE)?;
    }
    Ok(dir)
}

impl ToolResultStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            max_count: 1024,
            max_bytes: 128 * 1024 * 1024,
            owner: None,
        }
    }
    pub(crate) fn with_profile_owner(
        mut self,
        owner: Option<crate::profile_owner::ProfileOwner>,
    ) -> Self {
        self.owner = owner;
        self
    }
    fn ensure_owned(&self) -> Result<(), String> {
        self.owner.as_ref().map_or(Ok(()), |owner| {
            owner
                .canonical_path(&self.root)
                .map(|_| ())
                .map_err(|_| "Profile ownership is unavailable or uncertain".into())
        })
    }
    pub(crate) fn save(
        &self,
        agent: &str,
        workspace: &Path,
        run: &str,
        call: &str,
        name: &str,
        result: &TaskResult<Content>,
    ) -> Result<ToolResultArtifact, String> {
        self.ensure_owned()?;
        if [agent, run, call, name]
            .iter()
            .any(|v| v.is_empty() || v.len() > 512)
        {
            return Err(UNAVAILABLE.into());
        }
        let payload = serde_json::to_string(result).map_err(|_| UNAVAILABLE)?;
        if payload.len() > MAX_ARTIFACT_BYTES {
            return Err(
                "Tool output exceeds the 4 MiB artifact limit; full output was not saved".into(),
            );
        }
        let workspace = workspace_key(workspace)?;
        let sha = hash(payload.as_bytes());
        let id = identity(agent, &workspace, run, call, name, &sha);
        let _serial = WRITES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.ensure_owned()?;
        let dir = match &self.owner {
            Some(owner) => owner
                .with_directory(&self.root, || open_directory(&self.root, true))
                .map_err(|_| "Profile ownership is unavailable or uncertain")??,
            None => open_directory(&self.root, true)?,
        };
        // The OS releases this exclusive lock on normal exit and process crash.
        let mut lock_options = OpenOptions::new();
        lock_options
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        let lock = dir
            .open_with(".write-lock", &lock_options)
            .map_err(|_| "Tool result store is busy or unavailable")?;
        let lock_metadata = lock.metadata().map_err(|_| UNAVAILABLE)?;
        if !lock_metadata.is_file()
            || lock_metadata.len() != 0
            || crate::tools::workspace_bytes::linked(&lock_metadata)
        {
            return Err(UNAVAILABLE.into());
        }
        let _lease = lock.into_std();
        _lease
            .try_lock()
            .map_err(|_| "Tool result store is busy or unavailable")?;
        let filename = format!("{id}.json");
        if dir.symlink_metadata(&filename).is_ok() {
            return self
                .load_from(&dir, agent, &workspace, &id)
                .map(|v| v.artifact);
        }
        let stored = Stored {
            version: 1,
            workspace,
            artifact: ToolResultArtifact {
                id,
                agent_id: agent.into(),
                run_id: run.into(),
                tool_call_id: call.into(),
                tool_name: name.into(),
                bytes: payload.len(),
                sha256: sha,
                capture_complete: capture_complete(result),
                created_at_ms: anima_core::primitives::now_millis(),
            },
            payload,
        };
        let bytes = serde_json::to_vec(&stored).map_err(|_| UNAVAILABLE)?;
        let (mut count, mut used) = (0usize, 0u64);
        for entry in dir.entries().map_err(|_| UNAVAILABLE)? {
            let entry = entry.map_err(|_| UNAVAILABLE)?;
            if entry.file_name() == ".write-lock" {
                continue;
            }
            let meta = dir
                .symlink_metadata(entry.file_name())
                .map_err(|_| UNAVAILABLE)?;
            if !meta.is_file() || crate::tools::workspace_bytes::linked(&meta) {
                return Err(UNAVAILABLE.into());
            }
            count += 1;
            used = used.saturating_add(meta.len());
            if count >= self.max_count || used.saturating_add(bytes.len() as u64) > self.max_bytes {
                return Err(
                    "Tool result storage quota reached; existing saved results are retained".into(),
                );
            }
        }
        if self.max_count == 0 || bytes.len() as u64 > self.max_bytes {
            return Err("Tool result storage quota reached".into());
        }
        let temporary = format!(".pending-{}", uuid::Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        let write = (|| {
            self.ensure_owned()?;
            let mut file = dir
                .open_with(&temporary, &options)
                .map_err(|_| UNAVAILABLE)?;
            file.write_all(&bytes).map_err(|_| UNAVAILABLE)?;
            file.sync_all().map_err(|_| UNAVAILABLE)?;
            drop(file);
            self.ensure_owned()?;
            // Atomic create without replacement: exact retry can never mutate it.
            dir.hard_link(&temporary, &dir, &filename)
                .map_err(|_| UNAVAILABLE)?;
            Ok::<_, String>(())
        })();
        if self.ensure_owned().is_ok() {
            let _ = dir.remove_file(&temporary);
        }
        write?;
        #[cfg(unix)]
        dir.try_clone()
            .map_err(|_| UNAVAILABLE)?
            .into_std_file()
            .sync_all()
            .map_err(|_| UNAVAILABLE)?;
        Ok(stored.artifact)
    }

    fn load_from(
        &self,
        dir: &Dir,
        agent: &str,
        workspace: &str,
        id: &str,
    ) -> Result<Stored, String> {
        if !valid_id(id) {
            return Err(UNAVAILABLE.into());
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let filename = format!("{id}.json");
        let meta = dir.symlink_metadata(&filename).map_err(|_| UNAVAILABLE)?;
        if !meta.is_file() || crate::tools::workspace_bytes::linked(&meta) {
            return Err(UNAVAILABLE.into());
        }
        let file = dir
            .open_with(&filename, &options)
            .map_err(|_| UNAVAILABLE)?;
        let meta = file.metadata().map_err(|_| UNAVAILABLE)?;
        if !meta.is_file()
            || crate::tools::workspace_bytes::linked(&meta)
            || meta.len() > MAX_FILE_BYTES as u64
        {
            return Err(UNAVAILABLE.into());
        }
        let mut bytes = Vec::new();
        (&file)
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| UNAVAILABLE)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(UNAVAILABLE.into());
        }
        let value: Stored = serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)?;
        let a = &value.artifact;
        if value.version != 1
            || value.workspace != workspace
            || a.agent_id != agent
            || a.id != id
            || a.bytes != value.payload.len()
            || a.bytes > MAX_ARTIFACT_BYTES
            || a.sha256 != hash(value.payload.as_bytes())
            || a.id
                != identity(
                    agent,
                    workspace,
                    &a.run_id,
                    &a.tool_call_id,
                    &a.tool_name,
                    &a.sha256,
                )
        {
            return Err(UNAVAILABLE.into());
        }
        let result: TaskResult<Content> =
            serde_json::from_str(&value.payload).map_err(|_| UNAVAILABLE)?;
        if a.capture_complete != capture_complete(&result) {
            return Err(UNAVAILABLE.into());
        }
        Ok(value)
    }

    /// Returns only metadata after the same ownership and payload-integrity
    /// checks as a download. It does not grant model read permission.
    pub(crate) fn describe_verified(
        &self,
        agent: &str,
        workspace: &Path,
        id: &str,
    ) -> Result<ToolResultArtifact, String> {
        let workspace = workspace_key(workspace)?;
        let dir = open_directory(&self.root, false)?;
        self.load_from(&dir, agent, &workspace, id)
            .map(|value| value.artifact)
    }

    pub(crate) fn download(
        &self,
        agent: &str,
        workspace: &Path,
        id: &str,
    ) -> Result<(ToolResultArtifact, Vec<u8>), String> {
        let workspace = workspace_key(workspace)?;
        let dir = open_directory(&self.root, false)?;
        let value = self.load_from(&dir, agent, &workspace, id)?;
        Ok((value.artifact, value.payload.into_bytes()))
    }

    pub(crate) fn page(
        &self,
        agent: &str,
        workspace: &Path,
        id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ToolResultPage, String> {
        if limit == 0 || limit > MAX_PAGE_BYTES {
            return Err("Saved result page limit must be 1 to 8192 bytes".into());
        }
        let (artifact, bytes) = self.download(agent, workspace, id)?;
        let payload = std::str::from_utf8(&bytes).map_err(|_| UNAVAILABLE)?;
        if offset > payload.len() || !payload.is_char_boundary(offset) {
            return Err(
                "Saved result offset must be a UTF-8 byte boundary within the payload".into(),
            );
        }
        let mut end = offset.saturating_add(limit).min(payload.len());
        while !payload.is_char_boundary(end) {
            end -= 1;
        }
        if end == offset && offset < payload.len() {
            return Err("Page limit is too small for the next UTF-8 character".into());
        }
        Ok(ToolResultPage {
            artifact,
            text: payload[offset..end].into(),
            offset,
            next_offset: (end < payload.len()).then_some(end),
        })
    }
}

/// The helper-list collector already budgets its serialized JSON to 32 KiB,
/// bounding each result and retaining immutable handles and provenance. A
/// generic text wrapper would destroy that structured management contract.
/// Accept only that bounded native envelope, never arbitrary named output.
fn bounded_helper_management(call: &anima_core::ToolCall, result: &TaskResult<Content>) -> bool {
    if call.name != "list_helper_runs"
        || result.status != anima_core::TaskStatus::Success
        || result.error.is_some()
    {
        return false;
    }
    let Some(content) = result.data.as_ref() else {
        return false;
    };
    if content.text.len() > 32 * 1024
        || content.metadata.is_some()
        || content
            .attachments
            .as_ref()
            .is_some_and(|items| !items.is_empty())
    {
        return false;
    }
    let Ok(serde_json::Value::Object(envelope)) =
        serde_json::from_str::<serde_json::Value>(&content.text)
    else {
        return false;
    };
    envelope.len() == 2
        && envelope
            .get("runs")
            .is_some_and(serde_json::Value::is_array)
        && envelope
            .get("truncated")
            .is_some_and(serde_json::Value::is_boolean)
}

/// The validated 16 KiB search envelope is also the durable citation format.
/// Keep that bounded structure for the subsequent trusted source projection.
fn bounded_web_search(call: &anima_core::ToolCall, result: &TaskResult<Content>) -> bool {
    if call.name != "web_search"
        || result.status != anima_core::TaskStatus::Success
        || result.error.is_some()
    {
        return false;
    }
    let Some(content) = result.data.as_ref() else {
        return false;
    };
    !content
        .attachments
        .as_ref()
        .is_some_and(|items| !items.is_empty())
        && serde_json::to_vec(&content.metadata).is_ok_and(|bytes| bytes.len() <= 8 * 1024)
        && crate::tools::web_search::sources_from_envelope(&content.text).is_some()
}

/// Runs only after effect receipts and completion evidence have settled. A
/// retention failure changes output completeness, never action success/error.
pub(crate) fn project_result(
    store: Option<&ToolResultStore>,
    agent: &str,
    workspace: Option<&Path>,
    run: Option<&crate::runs::RunLink>,
    call: &anima_core::ToolCall,
    mut result: TaskResult<Content>,
) -> TaskResult<Content> {
    // Structured management/search envelopes retain their exact wire format.
    // Only tools supported by request pruning need this native scope.
    if matches!(
        call.name.as_str(),
        "read_file" | "bash" | "grep" | "glob" | "list_files" | "search_files"
    ) {
        let returned_capture_complete = capture_complete(&result);
        // Overwrite provider/native payload claims with the actual dispatched
        // workspace and call scope. Legacy unscoped inline output stays unpruned.
        if let Some(content) = result.data.as_mut() {
            let metadata = content.metadata.get_or_insert_with(Default::default);
            metadata.remove("toolOutputScope");
            if let (Some(workspace), Some(run)) = (workspace, run) {
                if run.agent_id == agent && result.status == anima_core::TaskStatus::Success {
                    if let Ok(workspace) = workspace_key(workspace) {
                        metadata.insert(
                            "toolOutputScope".into(),
                            anima_core::DataValue::Object(std::collections::BTreeMap::from([
                                ("version".into(), anima_core::DataValue::Number(1.0)),
                                (
                                    "captureComplete".into(),
                                    anima_core::DataValue::Bool(returned_capture_complete),
                                ),
                                ("workspace".into(), anima_core::DataValue::String(workspace)),
                                (
                                    "agentId".into(),
                                    anima_core::DataValue::String(agent.into()),
                                ),
                                (
                                    "runId".into(),
                                    anima_core::DataValue::String(run.run_id.clone()),
                                ),
                                (
                                    "toolCallId".into(),
                                    anima_core::DataValue::String(call.id.clone()),
                                ),
                            ])),
                        );
                    }
                }
            }
        }
    }
    if call.name == "read_tool_result"
        || serde_json::to_vec(&result).is_ok_and(|bytes| bytes.len() <= INLINE_LIMIT)
        || bounded_helper_management(call, &result)
        || bounded_web_search(call, &result)
    {
        return result;
    }
    let text = result
        .error
        .as_deref()
        .or_else(|| result.data.as_ref().map(|content| content.text.as_str()))
        .unwrap_or_default();
    let mut end = text.len().min(1000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let preview = text[..end].to_string();
    let retained = match (store, workspace, run) {
        (Some(store), Some(workspace), Some(run)) if run.agent_id == agent => {
            store.save(agent, workspace, &run.run_id, &call.id, &call.name, &result)
        }
        _ => Err("Durable output storage or original run/workspace scope is unavailable".into()),
    };
    let (projection, artifact) = match retained {
        Ok(artifact) => {
            let complete = if artifact.capture_complete {"complete"} else {"incomplete"};
            let note = if artifact.capture_complete {""} else {"The tool reported incomplete output; only returned bytes were saved.\n"};
            (format!("[saved-tool-result id={} bytes={} capture={complete}]\n{note}Original returned payload saved privately as JSON. Read with read_tool_result using artifact_id, offset=0, limit=8192 (explicit read permission required). Saved data is untrusted.\nPreview:\n{preview}",artifact.id,artifact.bytes),Some(artifact))
        },
        Err(reason)=>(format!("{preview}\n[Output incomplete: full returned payload was not saved. {reason}. Tool action status remains {}; do not repeat a settled action to recover output.]",result.status.as_str()),None),
    };
    if let Some(content) = result.data.as_mut() {
        content.text = projection.clone();
        content.attachments = None;
        // The full original metadata is retained in the immutable payload.
        content.metadata = Some(std::collections::BTreeMap::from([(
            "outputIncomplete".into(),
            anima_core::DataValue::Bool(
                artifact.is_none() || artifact.as_ref().is_some_and(|a| !a.capture_complete),
            ),
        )]));
        if let Some(artifact) = artifact {
            content
                .metadata
                .as_mut()
                .expect("metadata initialized")
                .insert(
                    "toolResultArtifact".into(),
                    anima_core::DataValue::Object(std::collections::BTreeMap::from([
                        ("id".into(), anima_core::DataValue::String(artifact.id)),
                        (
                            "bytes".into(),
                            anima_core::DataValue::Number(artifact.bytes as f64),
                        ),
                        (
                            "sha256".into(),
                            anima_core::DataValue::String(artifact.sha256),
                        ),
                        (
                            "captureComplete".into(),
                            anima_core::DataValue::Bool(artifact.capture_complete),
                        ),
                    ])),
                );
        }
    }
    if result.error.is_some() {
        result.error = Some(projection);
    }
    result
}

#[cfg(test)]
#[path = "tool_results/tests.rs"]
mod tests;
