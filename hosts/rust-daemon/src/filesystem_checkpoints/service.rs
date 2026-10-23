//! Explicit host-bound native checkpoint admission. Physical effects are Windows-only.
use super::store::{child, create, digest_valid, encode, inventory, Session};
use super::workspace::*;
use super::*;
use crate::runs::RunLink;
use anima_core::filesystem_checkpoints as core;
use anima_core::{DataValue, ToolCall};
use cap_std::fs::Dir;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub(crate) const NATIVE_RECOVERY_REQUIRED: &str = "Filesystem checkpoint RecoveryRequired";

#[derive(Clone)]
pub(crate) struct NativeCheckpointService {
    store_root: PathBuf,
    owner: Option<crate::profile_owner::ProfileOwner>,
    #[cfg(test)]
    limits: Option<CaptureLimits>,
    #[cfg(test)]
    fault: std::sync::Arc<std::sync::Mutex<Option<NativeMutationFault>>>,
    #[cfg(test)]
    before_lease: std::sync::Arc<std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeMutationReceipt {
    pub(crate) checkpoint_id: String,
    pub(crate) path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct NativeCheckpointSummary {
    pub(crate) checkpoint_id: String,
    pub(crate) sequence: u64,
    pub(crate) path: String,
    pub(crate) native_run_id: String,
    pub(crate) native_tool_call_id: String,
    pub(crate) status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeRestoreReceipt {
    pub(crate) checkpoint_id: String,
    pub(crate) restored_paths: Vec<String>,
    pub(crate) safety_checkpoint_id: String,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeMutationFault {
    BeforePreimageAck,
    AfterFirstWrite,
    BeforePostimageAck,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeIntent {
    version: u32,
    checkpoint_id: String,
    checkpoint_sequence: u64,
    scope_digest: String,
    path: String,
    action_id: String,
    native_run_id: String,
    native_tool_call_id: String,
    request_digest: String,
}

/// Independently committed host binding, rather than authority decoded from
/// the portable lineage. This private index is never accepted as tool input.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeCommit {
    intent_digest: String,
    manifest_digest: String,
    action_digest: String,
    head_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreAttempt {
    version: u32,
    checkpoint_id: String,
    plan_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreOutcome {
    version: u32,
    checkpoint_id: String,
    plan_id: String,
    safety_checkpoint_id: String,
    restored: bool,
}

struct Group {
    intent: NativeIntent,
    authority: Option<BasicRestoreAuthority>,
    status: &'static str,
}

fn error(error: CaptureError) -> String {
    match error {
        CaptureError::RecoveryRequired => NATIVE_RECOVERY_REQUIRED.into(),
        other => format!("Filesystem checkpoint {other:?}"),
    }
}

fn recover<T>(result: Result<T, CaptureError>) -> Result<T, CaptureError> {
    result.map_err(|_| CaptureError::RecoveryRequired)
}

fn exists(directory: &Dir, name: &str) -> Result<bool, CaptureError> {
    match directory.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}

fn decode<T: serde::de::DeserializeOwned>(directory: &Dir, name: &str) -> Result<T, CaptureError> {
    let (bytes, _) = recover(read_file(directory, name, 64 * 1024))?;
    serde_json::from_slice(&bytes).map_err(|_| CaptureError::RecoveryRequired)
}

fn native_request_digest(call: &ToolCall, max: usize) -> Result<String, CaptureError> {
    fn string_bytes(value: &str) -> Result<usize, CaptureError> {
        value.bytes().try_fold(2usize, |sum, byte| {
            let width = match byte {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                0..=31 => 6,
                _ => 1,
            };
            sum.checked_add(width).ok_or(CaptureError::Budget)
        })
    }
    let mut used = string_bytes(&call.id)?
        .checked_add(string_bytes(&call.name)?)
        .and_then(|bytes| bytes.checked_add(8))
        .ok_or(CaptureError::Budget)?;
    let mut stack = Vec::new();
    for (key, value) in &call.args {
        used = used
            .checked_add(string_bytes(key)? + 2)
            .ok_or(CaptureError::Budget)?;
        if stack.len() >= 50_000 || used > max {
            return Err(CaptureError::Budget);
        }
        stack.push((value, 0usize));
    }
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > 50_000 || depth > 64 {
            return Err(CaptureError::Budget);
        }
        let added = match value {
            DataValue::Null | DataValue::Bool(_) => 6,
            DataValue::Number(value) if value.is_finite() => 32,
            DataValue::Number(_) => return Err(CaptureError::InvalidWitness),
            DataValue::String(value) => string_bytes(value)? + 1,
            DataValue::Array(values) => {
                if nodes + stack.len() + values.len() > 50_000 {
                    return Err(CaptureError::Budget);
                }
                stack.extend(values.iter().map(|value| (value, depth + 1)));
                values.len() + 2
            }
            DataValue::Object(values) => {
                if nodes + stack.len() + values.len() > 50_000 {
                    return Err(CaptureError::Budget);
                }
                for (key, value) in values {
                    used = used
                        .checked_add(string_bytes(key)? + 2)
                        .ok_or(CaptureError::Budget)?;
                    stack.push((value, depth + 1));
                }
                2
            }
        };
        used = used.checked_add(added).ok_or(CaptureError::Budget)?;
        if used > max {
            return Err(CaptureError::Budget);
        }
    }
    // Allocate/clone only after the complete borrowed arguments pass the count,
    // nesting and encoded-byte preflight. The tuple is the closed wire contract.
    let args = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
    let request = (&call.id, &call.name, args);
    encode(&request, max)?;
    digest_value(b"anima-native-checkpoint-request-v1\0", &request)
}

fn checkpoint(
    record: &CaptureRecord,
    scope: &CheckpointScope,
) -> Result<core::FilesystemCheckpoint, CaptureError> {
    Ok(core::FilesystemCheckpoint {
        version: 1,
        checkpoint_id: record.checkpoint_id.clone(),
        sequence: record.sequence,
        scope: scope.clone(),
        scope_digest: record.scope_digest.clone(),
        entries: record
            .entries
            .iter()
            .map(|entry| {
                Ok(core::CheckpointEntry {
                    path: entry.path.clone(),
                    state: super::restore::pure_state(&entry.state)?,
                })
            })
            .collect::<Result<_, CaptureError>>()?,
    })
}

fn lineage(
    record: &CaptureRecord,
    scope: &CheckpointScope,
    intent: &NativeIntent,
    action: core::MutationAction,
    commit: &NativeCommit,
) -> Result<BasicRestoreAuthority, CaptureError> {
    let manifest_digest = core::checkpoint_manifest_digest(&checkpoint(record, scope)?);
    let binding = core::ExpectedActionBinding {
        sequence: 1,
        action_id: intent.action_id.clone(),
        native_run_id: intent.native_run_id.clone(),
        native_tool_call_id: intent.native_tool_call_id.clone(),
        request_digest: intent.request_digest.clone(),
        action_digest: commit.action_digest.clone(),
    };
    let expected_head = core::ExpectedLineageHead {
        checkpoint_id: record.checkpoint_id.clone(),
        checkpoint_sequence: record.sequence,
        manifest_digest: manifest_digest.clone(),
        scope_digest: record.scope_digest.clone(),
        action_count: 1,
        last_sequence: 1,
        head_digest: commit.head_digest.clone(),
        action_bindings: vec![binding],
    };
    if commit.manifest_digest != manifest_digest
        || core::checkpoint_action_digest(&action) != commit.action_digest
    {
        return Err(CaptureError::RecoveryRequired);
    }
    Ok(BasicRestoreAuthority {
        expected_scope: scope.clone(),
        lineage: core::MutationLineage {
            version: 1,
            checkpoint_id: record.checkpoint_id.clone(),
            checkpoint_sequence: record.sequence,
            manifest_digest,
            scope_digest: record.scope_digest.clone(),
            action_count: 1,
            last_sequence: 1,
            actions: vec![action],
            head_digest: commit.head_digest.clone(),
        },
        expected_head,
    })
}

fn refuse(directory: &Dir, intent: &NativeIntent) -> Result<(), CaptureError> {
    recover(create(
        directory,
        "refused",
        core::checkpoint_digest(&encode(intent, 16 * 1024)?).as_bytes(),
    ))
}

impl NativeCheckpointService {
    pub(crate) fn new(store_root: PathBuf) -> Self {
        Self {
            store_root,
            owner: None,
            #[cfg(test)]
            limits: None,
            #[cfg(test)]
            fault: Default::default(),
            #[cfg(test)]
            before_lease: Default::default(),
        }
    }

    pub(crate) fn with_profile_owner(
        mut self,
        owner: Option<crate::profile_owner::ProfileOwner>,
    ) -> Self {
        self.owner = owner;
        self
    }

    /// Filesystem-only test entry; production dispatch must supply a current
    /// native authority callback through `mutate_checked`.
    #[cfg(test)]
    pub(crate) fn mutate<F>(
        &self,
        workspace: &Path,
        run: &RunLink,
        call: &ToolCall,
        path: &str,
        transform: F,
    ) -> Result<NativeMutationReceipt, String>
    where
        F: FnOnce(Option<&[u8]>) -> Result<Vec<u8>, String>,
    {
        self.mutate_checked(workspace, run, call, path, transform, || Ok(()))
    }

    pub(crate) fn mutate_checked<F, C>(
        &self,
        workspace: &Path,
        run: &RunLink,
        call: &ToolCall,
        path: &str,
        transform: F,
        mut validate: C,
    ) -> Result<NativeMutationReceipt, String>
    where
        F: FnOnce(Option<&[u8]>) -> Result<Vec<u8>, String>,
        C: FnMut() -> Result<(), String>,
    {
        #[cfg(not(windows))]
        {
            let _ = (workspace, run, call, path, transform, validate);
            return Err(error(CaptureError::Unsupported));
        }
        #[cfg(windows)]
        {
            let mut store = self.store(workspace, run).map_err(error)?;
            let session = store.session().map_err(error)?;
            let groups = self.groups(&store, &session).map_err(error)?;
            if !matches!(
                call.name.as_str(),
                "write_file" | "edit_file" | "multi_edit"
            ) || call.id.is_empty()
                || call.id.len() > 128
                || call.id.chars().any(char::is_control)
            {
                return Err(error(CaptureError::InvalidWitness));
            }
            if groups.iter().any(|group| {
                group.intent.native_run_id == run.run_id
                    && group.intent.native_tool_call_id == call.id
            }) {
                return Err(error(CaptureError::InvalidWitness));
            }
            let request_digest =
                native_request_digest(call, store.config.policy.limits.manifest_bytes)
                    .map_err(error)?;
            let (selected, _) = store
                .selections(
                    &session.workspace.directory,
                    &CaptureSelection::Paths {
                        paths: vec![path.into()],
                    },
                )
                .map_err(error)?;
            let path = &selected[0];
            let Some(DataValue::String(requested)) = call.args.get("file_path") else {
                return Err(error(CaptureError::InvalidWitness));
            };
            let canonical_root = workspace
                .canonicalize()
                .map_err(|_| error(CaptureError::InvalidScope))?;
            let requested_target =
                crate::tools::resolve_workspace_write_path(workspace, requested.trim(), &call.name)
                    .map_err(|_| error(CaptureError::UnsafePath))?;
            let requested_path =
                crate::tools::normalized_relative_path(&canonical_root, &requested_target)
                    .map_err(|_| error(CaptureError::UnsafePath))?;
            if path_key(&requested_path, store.config.scope.path_semantics).map_err(error)?
                != path_key(path, store.config.scope.path_semantics).map_err(error)?
            {
                return Err(error(CaptureError::InvalidWitness));
            }
            let parent = held_parent(&session.workspace.directory, path).map_err(error)?;
            let (initial, initial_blobs, _) = store
                .observe(&session.workspace.directory, &selected)
                .map_err(error)?;
            let source = match &initial[0].state {
                CaptureState::Absent {} => None,
                CaptureState::File { content, .. } => Some(
                    initial_blobs
                        .get(&content.sha256)
                        .ok_or_else(|| error(CaptureError::Corrupt))?
                        .as_slice(),
                ),
            };
            let desired = transform(source)?;
            let history = store.history(&session).map_err(error)?;
            if desired.len() as u64 > store.config.policy.limits.file_bytes
                || history.len() + 2 > store.config.policy.limits.history_records
            {
                return Err(error(CaptureError::Budget));
            }
            // Reserve preimage + postimage + worst-case one safety capture and
            // displaced creation, including a closed metadata envelope. No pruning.
            let reserve = (source.map_or(0, <[u8]>::len) as u64)
                .checked_add(desired.len() as u64)
                .and_then(|bytes| bytes.checked_add(store.config.policy.limits.file_bytes * 2))
                .and_then(|bytes| bytes.checked_add(512 * 1024))
                .ok_or_else(|| error(CaptureError::Budget))?;
            self.reserve(&store, &session, reserve).map_err(error)?;
            #[cfg(test)]
            let fault = self.fault.lock().unwrap().take();
            #[cfg(test)]
            if fault == Some(NativeMutationFault::BeforePreimageAck) {
                store.set_publish_fault(TestPublishFault::BeforeIndexCommit);
            }
            let intent = NativeIntent {
                version: 1,
                checkpoint_id: core::checkpoint_digest(uuid::Uuid::new_v4().as_bytes()),
                checkpoint_sequence: history.len() as u64 + 1,
                scope_digest: core::checkpoint_scope_digest(&store.config.scope),
                path: path.clone(),
                action_id: uuid::Uuid::new_v4().to_string(),
                native_run_id: run.run_id.clone(),
                native_tool_call_id: call.id.clone(),
                request_digest,
            };
            let operations = child(&session.partition, "native-operations").map_err(error)?;
            let directory = child(&operations, &intent.checkpoint_id).map_err(error)?;
            let prepared = encode(&intent, 16 * 1024).map_err(error)?;
            recover(create(&directory, "prepared.json", &prepared)).map_err(error)?;
            recover(create(
                &directory,
                "prepared-ack",
                core::checkpoint_digest(&prepared).as_bytes(),
            ))
            .map_err(error)?;
            let before = recover(store.capture_named_in_session(
                &session,
                CaptureSelection::Paths {
                    paths: selected.clone(),
                },
                intent.checkpoint_id.clone(),
            ))
            .map_err(error)?;
            if before.entries != initial {
                refuse(&directory, &intent).map_err(error)?;
                return Err(error(CaptureError::Dirty));
            }
            #[cfg(test)]
            if let Some(hook) = self.before_lease.lock().unwrap().take() {
                hook();
            }
            let mut target = match &before.entries[0].state {
                CaptureState::File { .. } => {
                    let mut file = match open_read_write(&parent) {
                        Ok(file) => file,
                        Err(reason) => {
                            refuse(&directory, &intent).map_err(error)?;
                            return Err(error(reason));
                        }
                    };
                    let (bytes, basic_mode) =
                        read_open_file(&mut file, store.config.policy.limits.file_bytes)
                            .map_err(error)?;
                    let actual = CaptureState::File {
                        content: BlobReference {
                            sha256: core::checkpoint_digest(&bytes),
                            byte_count: bytes.len() as u64,
                        },
                        basic_mode,
                    };
                    if actual != before.entries[0].state {
                        refuse(&directory, &intent).map_err(error)?;
                        return Err(error(CaptureError::Dirty));
                    }
                    Some(file)
                }
                CaptureState::Absent {} => {
                    if exists(&parent.directory, &parent.name).map_err(error)? {
                        refuse(&directory, &intent).map_err(error)?;
                        return Err(error(CaptureError::Dirty));
                    }
                    None
                }
            };
            // No persistence or await occurs between this current authority
            // check and the first target effect under the retained lease.
            if let Err(reason) = validate() {
                refuse(&directory, &intent).map_err(error)?;
                return Err(reason);
            }
            if target.is_none() {
                use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
                use cap_std::fs::{OpenOptions, OpenOptionsExt};
                let mut options = OpenOptions::new();
                options
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .follow(FollowSymlinks::No)
                    .nonblock(true)
                    .share_mode(1);
                target = match parent.directory.open_with(&parent.name, &options) {
                    Ok(file) => Some(file),
                    Err(reason) if reason.kind() == std::io::ErrorKind::AlreadyExists => {
                        refuse(&directory, &intent).map_err(error)?;
                        return Err(error(CaptureError::Dirty));
                    }
                    Err(_) => return Err(NATIVE_RECOVERY_REQUIRED.into()),
                };
            }
            let file = target
                .as_mut()
                .ok_or_else(|| NATIVE_RECOVERY_REQUIRED.to_string())?;
            let mut effect = || -> Result<(), CaptureError> {
                let metadata = file.metadata().map_err(io_error)?;
                use cap_fs_ext::MetadataExt;
                if linked(&metadata) || !metadata.is_file() || metadata.nlink() != 1 {
                    return Err(CaptureError::RecoveryRequired);
                }
                file.seek(SeekFrom::Start(0))
                    .and_then(|_| file.set_len(0))
                    .map_err(io_error)?;
                let first = desired.len().min(1);
                file.write_all(&desired[..first]).map_err(io_error)?;
                #[cfg(test)]
                if fault == Some(NativeMutationFault::AfterFirstWrite) {
                    file.sync_all().map_err(io_error)?;
                    return Err(CaptureError::RecoveryRequired);
                }
                file.write_all(&desired[first..])
                    .and_then(|_| file.sync_all())
                    .map_err(io_error)?;
                Ok(())
            };
            recover(effect()).map_err(error)?;
            let (bytes, mode) =
                recover(read_open_file(file, store.config.policy.limits.file_bytes))
                    .map_err(error)?;
            if bytes != desired {
                return Err(NATIVE_RECOVERY_REQUIRED.into());
            }
            let postimage = CaptureState::File {
                content: BlobReference {
                    sha256: core::checkpoint_digest(&bytes),
                    byte_count: bytes.len() as u64,
                },
                basic_mode: mode,
            };
            self.save_blob(&store, &session, &bytes)
                .map_err(|_| NATIVE_RECOVERY_REQUIRED.to_string())?;
            let action = core::MutationAction {
                sequence: 1,
                action_id: intent.action_id.clone(),
                native_run_id: intent.native_run_id.clone(),
                native_tool_call_id: intent.native_tool_call_id.clone(),
                request_digest: intent.request_digest.clone(),
                outcome: core::MutationActionOutcome::Acknowledged,
                changes: vec![core::MutationChange {
                    path: path.clone(),
                    disposition: if matches!(&before.entries[0].state, CaptureState::Absent {}) {
                        core::MutationDisposition::Created
                    } else {
                        core::MutationDisposition::Replaced
                    },
                    observed_preimage: recover(super::restore::pure_state(
                        &before.entries[0].state,
                    ))
                    .map_err(error)?,
                    acknowledged_postimage: Some(
                        recover(super::restore::pure_state(&postimage)).map_err(error)?,
                    ),
                }],
            };
            let manifest_digest = core::checkpoint_manifest_digest(
                &recover(checkpoint(&before, &store.config.scope)).map_err(error)?,
            );
            let action_digest = core::checkpoint_action_digest(&action);
            let commit = NativeCommit {
                intent_digest: core::checkpoint_digest(&prepared),
                manifest_digest: manifest_digest.clone(),
                action_digest: action_digest.clone(),
                head_digest: core::checkpoint_lineage_head_digest(&core::LineageHeadDigestInput {
                    version: 1,
                    checkpoint_id: before.checkpoint_id.clone(),
                    checkpoint_sequence: before.sequence,
                    manifest_digest,
                    scope_digest: before.scope_digest.clone(),
                    action_count: 1,
                    last_sequence: 1,
                    action_digests: vec![action_digest],
                }),
            };
            recover(create(
                &directory,
                "action.json",
                &recover(encode(&action, 64 * 1024)).map_err(error)?,
            ))
            .map_err(error)?;
            let committed = recover(encode(&commit, 16 * 1024)).map_err(error)?;
            recover(create(&directory, "index.json", &committed)).map_err(error)?;
            #[cfg(test)]
            if fault == Some(NativeMutationFault::BeforePostimageAck) {
                return Err(NATIVE_RECOVERY_REQUIRED.into());
            }
            recover(create(
                &directory,
                "index-ack",
                core::checkpoint_digest(&committed).as_bytes(),
            ))
            .map_err(error)?;
            Ok(NativeMutationReceipt {
                checkpoint_id: before.checkpoint_id,
                path: path.clone(),
            })
        }
    }

    pub(crate) fn list(
        &self,
        workspace: &Path,
        run: &RunLink,
    ) -> Result<Vec<NativeCheckpointSummary>, String> {
        let store = self.store(workspace, run).map_err(error)?;
        let session = store.session().map_err(error)?;
        Ok(self
            .groups(&store, &session)
            .map_err(error)?
            .into_iter()
            .rev()
            .map(|group| NativeCheckpointSummary {
                checkpoint_id: group.intent.checkpoint_id,
                sequence: group.intent.checkpoint_sequence,
                path: group.intent.path,
                native_run_id: group.intent.native_run_id,
                native_tool_call_id: group.intent.native_tool_call_id,
                status: group.status.into(),
            })
            .collect())
    }

    /// Filesystem-only test entry; production dispatch uses `restore_checked`.
    #[cfg(test)]
    pub(crate) fn restore(
        &self,
        workspace: &Path,
        run: &RunLink,
        checkpoint_id: &str,
    ) -> Result<NativeRestoreReceipt, String> {
        self.restore_checked(workspace, run, checkpoint_id, || Ok(()))
    }

    pub(crate) fn restore_checked<C>(
        &self,
        workspace: &Path,
        run: &RunLink,
        checkpoint_id: &str,
        mut validate: C,
    ) -> Result<NativeRestoreReceipt, String>
    where
        C: FnMut() -> Result<(), String>,
    {
        if !cfg!(windows) {
            return Err(error(CaptureError::Unsupported));
        }
        if !digest_valid(checkpoint_id) {
            return Err(error(CaptureError::Unavailable));
        }
        let mut store = self.store(workspace, run).map_err(error)?;
        let (intent, authority) = {
            let session = store.session().map_err(error)?;
            let group = self
                .groups(&store, &session)
                .map_err(error)?
                .into_iter()
                .find(|group| group.intent.checkpoint_id == checkpoint_id)
                .ok_or_else(|| error(CaptureError::Unavailable))?;
            if group.status != "acknowledged" {
                return Err(error(CaptureError::Unavailable));
            }
            let authority = group
                .authority
                .ok_or_else(|| error(CaptureError::Unavailable))?;
            let record = store
                .history(&session)
                .map_err(error)?
                .into_iter()
                .find(|record| record.checkpoint_id == checkpoint_id)
                .ok_or_else(|| error(CaptureError::Unavailable))?;
            let content = match &record.entries[0].state {
                CaptureState::File { content, .. } => {
                    store.read_blob(&session, content).map_err(error)?
                }
                CaptureState::Absent {} => Vec::new(),
            };
            let synthetic = ToolCall {
                id: "checkpoint-restore-classification".into(),
                name: "write_file".into(),
                args: BTreeMap::from([
                    (
                        "file_path".into(),
                        DataValue::String(group.intent.path.clone()),
                    ),
                    (
                        "content".into(),
                        DataValue::String(String::from_utf8_lossy(&content).into_owned()),
                    ),
                    (
                        "_anima_restore_content_sha256".into(),
                        DataValue::String(core::checkpoint_digest(&content)),
                    ),
                    (
                        "_anima_restore_absent".into(),
                        DataValue::Bool(matches!(
                            &record.entries[0].state,
                            CaptureState::Absent {}
                        )),
                    ),
                ]),
            };
            match crate::agent_files::write_policy::capture(Some(workspace), &synthetic) {
                Ok(None) => {}
                _ => return Err("Filesystem checkpoint protected restore refused".into()),
            }
            (group.intent, authority)
        };
        let plan = store
            .prepare_basic_restore(checkpoint_id, &[intent.path.clone()], &authority)
            .map_err(error)?;
        {
            let session = store.session().map_err(error)?;
            self.groups(&store, &session).map_err(error)?;
            let directory = open_child(
                &open_child(&session.partition, "native-operations").map_err(error)?,
                checkpoint_id,
            )
            .map_err(error)?;
            let attempt = RestoreAttempt {
                version: 1,
                checkpoint_id: checkpoint_id.into(),
                plan_id: plan.plan_id.clone(),
            };
            recover(create(
                &directory,
                "restore-attempt.json",
                &encode(&attempt, 4096).map_err(error)?,
            ))
            .map_err(error)?;
        }
        let mut denied = None;
        let result = store.execute_basic_restore_checked(&plan, &authority, &mut || {
            validate().map_err(|reason| {
                denied = Some(reason);
                CaptureError::Interrupted
            })
        });
        if matches!(&result, Err(CaptureError::RecoveryRequired)) {
            return Err(NATIVE_RECOVERY_REQUIRED.into());
        }
        let session = recover(store.session()).map_err(error)?;
        let safety_id = recover(self.safety_id(&session, &plan.plan_id)).map_err(error)?;
        let outcome = RestoreOutcome {
            version: 1,
            checkpoint_id: checkpoint_id.into(),
            plan_id: plan.plan_id,
            safety_checkpoint_id: safety_id.clone(),
            restored: result.is_ok(),
        };
        let directory = recover(open_child(
            &recover(open_child(&session.partition, "native-operations")).map_err(error)?,
            checkpoint_id,
        ))
        .map_err(error)?;
        recover(create(
            &directory,
            "restore-outcome.json",
            &recover(encode(&outcome, 4096)).map_err(error)?,
        ))
        .map_err(error)?;
        if let Some(reason) = denied {
            return Err(reason);
        }
        let result = result.map_err(error)?;
        Ok(NativeRestoreReceipt {
            checkpoint_id: checkpoint_id.into(),
            restored_paths: result.restored_paths,
            safety_checkpoint_id: safety_id,
        })
    }

    fn store(&self, workspace: &Path, run: &RunLink) -> Result<CheckpointStore, CaptureError> {
        for id in [&run.agent_id, &run.run_id, &run.session_id] {
            if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
                return Err(CaptureError::InvalidScope);
            }
        }
        let root = HeldRoot::open(workspace)?;
        let identity = root.identity()?;
        let policy = CapturePolicy {
            profile: CaptureProfile::ContentBasicV1,
            excluded_prefixes: vec![".git".into()],
            limits: {
                #[cfg(test)]
                {
                    self.limits.clone().unwrap_or_default()
                }
                #[cfg(not(test))]
                {
                    CaptureLimits::default()
                }
            },
        };
        let scope = CheckpointScope {
            owner_id: run.agent_id.clone(),
            workspace_id: identity.clone(),
            environment_id: "native-content-basic-v1".into(),
            path_semantics: if cfg!(windows) {
                core::PathSemantics::WindowsInsensitive
            } else {
                core::PathSemantics::PosixSensitive
            },
            root_identity_digest: identity,
            policy_digest: digest_value(
                b"anima-filesystem-checkpoint-capture-policy-v1\0",
                &policy,
            )?,
        };
        let config = CaptureConfig {
            workspace_root: workspace.into(),
            store_root: self.store_root.clone(),
            scope,
            policy,
        };
        match &self.owner {
            Some(owner) => owner
                .with_directory(&self.store_root, || {
                    CheckpointStore::provision_private_root(config)
                })
                .map_err(|_| CaptureError::InsecureStore)?,
            None => CheckpointStore::provision_private_root(config),
        }
    }

    fn reserve(
        &self,
        store: &CheckpointStore,
        session: &Session,
        reserve: u64,
    ) -> Result<(), CaptureError> {
        let mut count = 200_000;
        let mut bytes = 0;
        inventory(
            &session.store.directory,
            0,
            &mut count,
            &mut bytes,
            store.config.policy.limits.store_bytes,
        )?;
        if count < 512
            || bytes
                .checked_add(reserve)
                .is_none_or(|bytes| bytes > store.config.policy.limits.store_bytes)
        {
            return Err(CaptureError::Budget);
        }
        Ok(())
    }

    fn save_blob(
        &self,
        store: &CheckpointStore,
        session: &Session,
        bytes: &[u8],
    ) -> Result<(), CaptureError> {
        let hash = core::checkpoint_digest(bytes);
        let bucket = child(&open_child(&session.partition, "objects")?, &hash[..2])?;
        let name = format!("{hash}.blob");
        if exists(&bucket, &name)? {
            if store.read_blob(
                session,
                &BlobReference {
                    sha256: hash,
                    byte_count: bytes.len() as u64,
                },
            )? != bytes
            {
                return Err(CaptureError::Corrupt);
            }
            Ok(())
        } else {
            create(&bucket, &name, bytes)
        }
    }

    fn safety_id(&self, session: &Session, plan_id: &str) -> Result<String, CaptureError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SafetyIntent {
            version: u32,
            plan_id: String,
            scope_digest: String,
            plan_digest: String,
            safety_checkpoint_id: String,
            selected_paths: Vec<String>,
        }
        let directory = recover(open_child(
            &recover(open_child(&session.partition, "restores"))?,
            plan_id,
        ))?;
        let intent: SafetyIntent = decode(&directory, "prepared.json")?;
        if intent.version != 1
            || intent.plan_id != plan_id
            || !digest_valid(&intent.scope_digest)
            || !digest_valid(&intent.plan_digest)
            || !digest_valid(&intent.safety_checkpoint_id)
            || intent.selected_paths.len() != 1
        {
            return Err(CaptureError::RecoveryRequired);
        }
        Ok(intent.safety_checkpoint_id)
    }

    fn verified_objects(
        &self,
        store: &CheckpointStore,
        session: &Session,
        record: &CaptureRecord,
        authority: Option<&BasicRestoreAuthority>,
    ) -> Result<Vec<core::CheckpointBlob>, CaptureError> {
        let mut references = BTreeMap::new();
        let mut collect = |state: &core::FileState| -> Result<(), CaptureError> {
            if let core::FileState::File { content, .. } = state {
                if references
                    .insert(content.sha256.clone(), content.clone())
                    .is_some_and(|old| old != *content)
                {
                    return Err(CaptureError::RecoveryRequired);
                }
            }
            Ok(())
        };
        for entry in &checkpoint(record, &store.config.scope)?.entries {
            collect(&entry.state)?;
        }
        if let Some(authority) = authority {
            for action in &authority.lineage.actions {
                for change in &action.changes {
                    collect(&change.observed_preimage)?;
                    if let Some(postimage) = &change.acknowledged_postimage {
                        collect(postimage)?;
                    }
                }
            }
        }
        references
            .into_values()
            .map(|reference| {
                let bytes = recover(store.read_blob(session, &reference))?;
                Ok(core::CheckpointBlob { reference, bytes })
            })
            .collect()
    }

    fn groups(
        &self,
        store: &CheckpointStore,
        session: &Session,
    ) -> Result<Vec<Group>, CaptureError> {
        let history = store.history(session)?;
        if !matches!(
            recover(store.status_in_session(session))?,
            BasicRecoveryStatus::Clean
        ) {
            return Err(CaptureError::RecoveryRequired);
        }
        if !exists(&session.partition, "native-operations")? {
            return if history.is_empty() {
                Ok(Vec::new())
            } else {
                Err(CaptureError::RecoveryRequired)
            };
        }
        let operations = open_child(&session.partition, "native-operations")?;
        let mut remaining = store.config.policy.limits.history_records;
        let mut known = BTreeSet::new();
        let mut groups = Vec::new();
        for id in names(&operations, &mut remaining)? {
            if !digest_valid(&id) {
                return Err(CaptureError::RecoveryRequired);
            }
            let directory = recover(open_child(&operations, &id))?;
            let intent: NativeIntent = decode(&directory, "prepared.json")?;
            let prepared = encode(&intent, 16 * 1024)?;
            let (ack, _) = recover(read_file(&directory, "prepared-ack", 64))?;
            let record = history
                .iter()
                .find(|record| record.checkpoint_id == id)
                .ok_or(CaptureError::RecoveryRequired)?;
            if intent.version != 1
                || intent.checkpoint_id != id
                || intent.checkpoint_sequence != record.sequence
                || intent.scope_digest != record.scope_digest
                || record.entries.len() != 1
                || intent.path != record.entries[0].path
                || ack != core::checkpoint_digest(&prepared).as_bytes()
                || intent.action_id.is_empty()
                || intent.native_run_id.is_empty()
                || intent.native_tool_call_id.is_empty()
                || !digest_valid(&intent.request_digest)
            {
                return Err(CaptureError::RecoveryRequired);
            }
            known.insert(id.clone());
            let (authority, mut status) = if exists(&directory, "refused")? {
                let (refused, _) = recover(read_file(&directory, "refused", 64))?;
                if refused != core::checkpoint_digest(&prepared).as_bytes()
                    || exists(&directory, "action.json")?
                {
                    return Err(CaptureError::RecoveryRequired);
                }
                self.verified_objects(store, session, record, None)?;
                (None, "refused")
            } else {
                let action: core::MutationAction = decode(&directory, "action.json")?;
                let commit: NativeCommit = decode(&directory, "index.json")?;
                let (ack, _) = recover(read_file(&directory, "index-ack", 64))?;
                if ack != core::checkpoint_digest(&encode(&commit, 16 * 1024)?).as_bytes()
                    || commit.intent_digest != core::checkpoint_digest(&prepared)
                {
                    return Err(CaptureError::RecoveryRequired);
                }
                let authority = lineage(record, &store.config.scope, &intent, action, &commit)?;
                // Preflight closed shape/count/metadata/binding bounds before
                // resolving bytes. InvalidObject is allowed only at this stage.
                let checkpoint = checkpoint(record, &store.config.scope)?;
                let current = authority.lineage.actions[0]
                    .changes
                    .iter()
                    .map(|change| {
                        Ok(core::CheckpointEntry {
                            path: change.path.clone(),
                            state: change
                                .acknowledged_postimage
                                .clone()
                                .ok_or(CaptureError::RecoveryRequired)?,
                        })
                    })
                    .collect::<Result<Vec<_>, CaptureError>>()?;
                match core::prepare_content_basic_v1_safe_restore_with_lineage(
                    &checkpoint,
                    &store.config.scope,
                    &[],
                    &current,
                    &authority.lineage,
                    &authority.expected_head,
                ) {
                    Ok(_) | Err(core::CheckpointError::InvalidObject) => {}
                    _ => return Err(CaptureError::RecoveryRequired),
                }
                // Private receipts alone do not prove retained recovery bytes.
                // Re-read/hash every unique persisted preimage/postimage object
                // and require full planner validation before reporting success
                // or allowing the next mutation.
                let objects = self.verified_objects(store, session, record, Some(&authority))?;
                let proposal = core::prepare_content_basic_v1_safe_restore_with_lineage(
                    &checkpoint,
                    &store.config.scope,
                    &objects,
                    &current,
                    &authority.lineage,
                    &authority.expected_head,
                )
                .map_err(|_| CaptureError::RecoveryRequired)?;
                if !proposal.conflicts.is_empty() {
                    return Err(CaptureError::RecoveryRequired);
                }
                (Some(authority), "acknowledged")
            };
            if exists(&directory, "restore-attempt.json")? {
                let attempt: RestoreAttempt = decode(&directory, "restore-attempt.json")?;
                let outcome: RestoreOutcome = decode(&directory, "restore-outcome.json")?;
                if attempt.version != 1
                    || outcome.version != 1
                    || attempt.checkpoint_id != id
                    || outcome.checkpoint_id != id
                    || attempt.plan_id != outcome.plan_id
                    || !digest_valid(&attempt.plan_id)
                    || self.safety_id(session, &attempt.plan_id)? != outcome.safety_checkpoint_id
                {
                    return Err(CaptureError::RecoveryRequired);
                }
                let safety = history
                    .iter()
                    .find(|record| record.checkpoint_id == outcome.safety_checkpoint_id)
                    .ok_or(CaptureError::RecoveryRequired)?;
                self.verified_objects(store, session, safety, None)?;
                known.insert(outcome.safety_checkpoint_id);
                status = if outcome.restored {
                    "restored"
                } else {
                    "restore_refused"
                };
            }
            groups.push(Group {
                intent,
                authority,
                status,
            });
        }
        if history
            .iter()
            .any(|record| !known.contains(&record.checkpoint_id))
        {
            return Err(CaptureError::RecoveryRequired);
        }
        groups.sort_by_key(|group| group.intent.checkpoint_sequence);
        Ok(groups)
    }

    #[cfg(test)]
    pub(super) fn set_fault(&self, fault: NativeMutationFault) {
        *self.fault.lock().unwrap() = Some(fault);
    }

    #[cfg(test)]
    pub(super) fn set_before_lease(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_lease.lock().unwrap() = Some(Box::new(hook));
    }

    #[cfg(test)]
    pub(super) fn with_limits(mut self, limits: CaptureLimits) -> Self {
        self.limits = Some(limits);
        self
    }
}
