use super::store::{child, create, digest_valid, encode, inventory, Session};
use super::workspace::*;
use super::*;
use anima_core::filesystem_checkpoints::{
    self as core, checkpoint_digest, checkpoint_scope_digest, CheckpointBlob, CheckpointEntry,
    FileState, FilesystemCheckpoint, MetadataAttribute, MetadataKind, MetadataSnapshot,
    RestoreProposal,
};
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, OpenOptions};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Seek, SeekFrom, Write};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    plan_id: String,
    scope_digest: String,
    plan_digest: String,
    safety_checkpoint_id: String,
    selected_paths: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Finish {
    version: u32,
    plan_id: String,
    scope_digest: String,
    plan_digest: String,
    safety_checkpoint_id: String,
    committed: bool,
}

pub(super) fn pure_state(state: &CaptureState) -> Result<FileState, CaptureError> {
    Ok(match state {
        CaptureState::Absent {} => FileState::Absent,
        CaptureState::File {
            content,
            basic_mode,
        } => FileState::File {
            content: content.clone(),
            metadata: MetadataSnapshot {
                required: vec![MetadataKind::BasicPermissions],
                attributes: vec![MetadataAttribute {
                    kind: MetadataKind::BasicPermissions,
                    encoding_version: 1,
                    bytes: encode(basic_mode, 1024)?,
                }],
            },
        },
    })
}

fn state_from_read(bytes: &[u8], mode: BasicMode) -> Result<FileState, CaptureError> {
    pure_state(&CaptureState::File {
        content: BlobReference {
            sha256: checkpoint_digest(bytes),
            byte_count: bytes.len() as u64,
        },
        basic_mode: mode,
    })
}

fn core_error(error: core::CheckpointError) -> CaptureError {
    match error {
        core::CheckpointError::Budget => CaptureError::Budget,
        core::CheckpointError::InvalidScope => CaptureError::InvalidScope,
        core::CheckpointError::InvalidPath => CaptureError::UnsafePath,
        core::CheckpointError::InvalidObject => CaptureError::Corrupt,
        _ => CaptureError::InvalidWitness,
    }
}

fn plan_digest(plan: &BasicRestorePlan) -> Result<String, CaptureError> {
    encode(plan, core::MAX_CHECKPOINT_MANIFEST_BYTES)?;
    let mut body = plan.clone();
    body.plan_digest.clear();
    digest_value(
        b"anima-filesystem-checkpoint-basic-restore-plan-v1\0",
        &body,
    )
}

fn missing(directory: &Dir, name: &str) -> Result<bool, CaptureError> {
    match directory.symlink_metadata(name) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(io_error(error)),
    }
}

impl CheckpointStore {
    fn reserve_restore(
        &self,
        session: &Session,
        bytes: u64,
        entries: usize,
    ) -> Result<(), CaptureError> {
        let mut remaining = 200_000;
        let mut occupied = 0u64;
        inventory(
            &session.store.directory,
            0,
            &mut remaining,
            &mut occupied,
            self.config.policy.limits.store_bytes,
        )?;
        if entries > remaining
            || occupied.checked_add(bytes).ok_or(CaptureError::Budget)?
                > self.config.policy.limits.store_bytes
        {
            return Err(CaptureError::Budget);
        }
        Ok(())
    }

    fn load_plan(&self, session: &Session, id: &str) -> Result<BasicRestorePlan, CaptureError> {
        if !digest_valid(id) {
            return Err(CaptureError::Unavailable);
        }
        let plans = open_child(&session.partition, "restore-plans")?;
        let (bytes, _) = read_file(
            &plans,
            &format!("{id}.json"),
            self.config.policy.limits.manifest_bytes as u64,
        )?;
        let plan: BasicRestorePlan =
            serde_json::from_slice(&bytes).map_err(|_| CaptureError::Corrupt)?;
        if plan.plan_id != id
            || plan.scope_digest != checkpoint_scope_digest(&self.config.scope)
            || plan.plan_digest != plan_digest(&plan)?
        {
            return Err(CaptureError::Corrupt);
        }
        Ok(plan)
    }

    fn build_proposal(
        &self,
        session: &Session,
        checkpoint_id: &str,
        selected_paths: &[String],
        authority: &BasicRestoreAuthority,
    ) -> Result<RestoreProposal, CaptureError> {
        encode(
            &(
                &authority.expected_scope,
                &authority.lineage,
                &authority.expected_head,
            ),
            self.config.policy.limits.manifest_bytes,
        )?;
        encode(&selected_paths, self.config.policy.limits.manifest_bytes)?;
        if selected_paths.len() > self.config.policy.limits.entries {
            return Err(CaptureError::Budget);
        }
        if authority.expected_scope != self.config.scope || !digest_valid(checkpoint_id) {
            return Err(CaptureError::InvalidScope);
        }
        if selected_paths.is_empty() {
            return Err(CaptureError::UnsafePath);
        }
        let (selected, _) = self.selections(
            &session.workspace.directory,
            &CaptureSelection::Paths {
                paths: selected_paths.to_vec(),
            },
        )?;
        let semantics = self.config.scope.path_semantics;
        let selected_keys = selected
            .iter()
            .map(|path| path_key(path, semantics))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let record = self
            .history(session)?
            .into_iter()
            .find(|record| record.checkpoint_id == checkpoint_id)
            .ok_or(CaptureError::Unavailable)?;
        let mut entries = Vec::new();
        for entry in &record.entries {
            entries.push(CheckpointEntry {
                path: entry.path.clone(),
                state: pure_state(&entry.state)?,
            });
        }
        let checkpoint = FilesystemCheckpoint {
            version: 1,
            checkpoint_id: record.checkpoint_id,
            sequence: record.sequence,
            scope: self.config.scope.clone(),
            scope_digest: record.scope_digest,
            entries,
        };
        let target_keys = checkpoint
            .entries
            .iter()
            .map(|entry| path_key(&entry.path, semantics))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if !selected_keys.is_subset(&target_keys) {
            return Err(CaptureError::UnsafePath);
        }
        let observed_paths = checkpoint
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let (observed, current_blobs, _) =
            self.observe(&session.workspace.directory, &observed_paths)?;
        let mut current = Vec::new();
        for entry in &observed {
            current.push(CheckpointEntry {
                path: entry.path.clone(),
                state: pure_state(&entry.state)?,
            });
        }
        // Run full count/encoded/metadata/lineage/head validation before raw
        // reference resolution. InvalidObject is the sole expected empty-store result.
        match core::prepare_content_basic_v1_safe_restore_with_lineage(
            &checkpoint,
            &self.config.scope,
            &[],
            &current,
            &authority.lineage,
            &authority.expected_head,
        ) {
            Ok(_) | Err(core::CheckpointError::InvalidObject) => {}
            Err(error) => return Err(core_error(error)),
        }
        let mut references = BTreeMap::new();
        let mut collect = |state: &FileState| -> Result<(), CaptureError> {
            if let FileState::File { content, .. } = state {
                if let Some(previous) = references.insert(content.sha256.clone(), content.clone()) {
                    if previous != *content {
                        return Err(CaptureError::Corrupt);
                    }
                }
            }
            Ok(())
        };
        for entry in checkpoint.entries.iter().chain(&current) {
            collect(&entry.state)?;
        }
        for action in &authority.lineage.actions {
            for change in &action.changes {
                collect(&change.observed_preimage)?;
                if let Some(state) = &change.acknowledged_postimage {
                    collect(state)?;
                }
            }
        }
        let mut objects = Vec::new();
        for (hash, reference) in references {
            let bytes = match current_blobs.get(&hash) {
                Some(bytes) => bytes.clone(),
                None => self.read_blob(session, &reference)?,
            };
            objects.push(CheckpointBlob { reference, bytes });
        }
        let mut proposal = core::prepare_content_basic_v1_safe_restore_with_lineage(
            &checkpoint,
            &self.config.scope,
            &objects,
            &current,
            &authority.lineage,
            &authority.expected_head,
        )
        .map_err(core_error)?;
        proposal.operations.retain(|operation| {
            selected_keys.contains(&path_key(&operation.path, semantics).unwrap_or_default())
        });
        proposal.conflicts.retain(|conflict| {
            selected_keys.contains(&path_key(&conflict.path, semantics).unwrap_or_default())
        });
        if !proposal.conflicts.is_empty() {
            return Err(
                if proposal
                    .conflicts
                    .iter()
                    .all(|conflict| conflict.reason == core::RestoreConflictReason::Dirty)
                {
                    CaptureError::Dirty
                } else {
                    CaptureError::InvalidWitness
                },
            );
        }
        Ok(proposal)
    }

    pub(super) fn prepare_basic_restore(
        &mut self,
        checkpoint_id: &str,
        selected_paths: &[String],
        authority: &BasicRestoreAuthority,
    ) -> Result<BasicRestorePlan, CaptureError> {
        let session = self.session()?;
        if !matches!(
            self.status_in_session(&session)?,
            BasicRecoveryStatus::Clean
        ) {
            return Err(CaptureError::RecoveryRequired);
        }
        let proposal = self.build_proposal(&session, checkpoint_id, selected_paths, authority)?;
        let mut plan = BasicRestorePlan {
            plan_id: checkpoint_digest(uuid::Uuid::new_v4().as_bytes()),
            checkpoint_id: checkpoint_id.into(),
            scope_digest: checkpoint_scope_digest(&self.config.scope),
            lineage_head_digest: authority.expected_head.head_digest.clone(),
            selected_paths: selected_paths.to_vec(),
            proposal,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan_digest(&plan)?;
        let bytes = encode(&plan, self.config.policy.limits.manifest_bytes)?;
        self.reserve_restore(&session, bytes.len() as u64, 2)?;
        create(
            &child(&session.partition, "restore-plans")?,
            &format!("{}.json", plan.plan_id),
            &bytes,
        )?;
        Ok(plan)
    }

    fn load_intent(
        &self,
        session: &Session,
        directory: &Dir,
        id: &str,
    ) -> Result<(Intent, Vec<u8>), CaptureError> {
        let (bytes, _) = read_file(
            directory,
            "prepared.json",
            self.config.policy.limits.manifest_bytes as u64,
        )?;
        let intent: Intent =
            serde_json::from_slice(&bytes).map_err(|_| CaptureError::RecoveryRequired)?;
        let plan = self.load_plan(session, id)?;
        if intent.version != 1
            || intent.plan_id != id
            || intent.scope_digest != plan.scope_digest
            || intent.plan_digest != plan.plan_digest
            || intent.selected_paths != plan.selected_paths
            || !digest_valid(&intent.safety_checkpoint_id)
        {
            return Err(CaptureError::Corrupt);
        }
        Ok((intent, bytes))
    }

    pub(super) fn status_in_session(
        &self,
        session: &Session,
    ) -> Result<BasicRecoveryStatus, CaptureError> {
        if missing(&session.partition, "restores")? {
            return Ok(BasicRecoveryStatus::Clean);
        }
        let restores = open_child(&session.partition, "restores")?;
        let mut remaining = 1000;
        for id in names(&restores, &mut remaining)? {
            if !digest_valid(&id) {
                return Err(CaptureError::Corrupt);
            }
            let directory = open_child(&restores, &id)?;
            let (intent, prepared) = match self.load_intent(session, &directory, &id) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(BasicRecoveryStatus::RecoveryRequired {
                        plan_id: id,
                        safety_checkpoint_id: None,
                    })
                }
            };
            let pending = || BasicRecoveryStatus::RecoveryRequired {
                plan_id: id.clone(),
                safety_checkpoint_id: Some(intent.safety_checkpoint_id.clone()),
            };
            if missing(&directory, "prepared-ack")? || missing(&directory, "finish.json")? {
                return Ok(pending());
            }
            let (ack, _) = read_file(&directory, "prepared-ack", 64)?;
            if ack != checkpoint_digest(&prepared).as_bytes() {
                return Ok(pending());
            }
            let (bytes, _) = read_file(&directory, "finish.json", 4096)?;
            let finish: Finish =
                serde_json::from_slice(&bytes).map_err(|_| CaptureError::RecoveryRequired)?;
            if finish.version != 1
                || finish.plan_id != intent.plan_id
                || finish.scope_digest != intent.scope_digest
                || finish.plan_digest != intent.plan_digest
                || finish.safety_checkpoint_id != intent.safety_checkpoint_id
            {
                return Ok(pending());
            }
            if finish.committed {
                let plan = self.load_plan(session, &id)?;
                for (index, operation) in plan.proposal.operations.iter().enumerate() {
                    let (done, _) = read_file(&directory, &format!("{index:05}-done"), 64)
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    if done
                        != digest_value(
                            b"anima-filesystem-checkpoint-restored-state-v1\0",
                            operation,
                        )?
                        .as_bytes()
                    {
                        return Ok(pending());
                    }
                }
            }
        }
        Ok(BasicRecoveryStatus::Clean)
    }

    pub(super) fn restore_status(&self) -> Result<BasicRecoveryStatus, CaptureError> {
        let session = self.session()?;
        self.history(&session)?;
        self.status_in_session(&session)
    }

    fn finish_intent(
        &self,
        directory: &Dir,
        intent: &Intent,
        committed: bool,
    ) -> Result<(), CaptureError> {
        create(
            directory,
            "finish.json",
            &encode(
                &Finish {
                    version: 1,
                    plan_id: intent.plan_id.clone(),
                    scope_digest: intent.scope_digest.clone(),
                    plan_digest: intent.plan_digest.clone(),
                    safety_checkpoint_id: intent.safety_checkpoint_id.clone(),
                    committed,
                },
                4096,
            )?,
        )
    }

    pub(super) fn execute_basic_restore(
        &mut self,
        plan: &BasicRestorePlan,
        authority: &BasicRestoreAuthority,
    ) -> Result<BasicRestoreResult, CaptureError> {
        self.execute_basic_restore_checked(plan, authority, &mut || Ok(()))
    }

    pub(super) fn execute_basic_restore_checked<C>(
        &mut self,
        plan: &BasicRestorePlan,
        authority: &BasicRestoreAuthority,
        validate: &mut C,
    ) -> Result<BasicRestoreResult, CaptureError>
    where
        C: FnMut() -> Result<(), CaptureError>,
    {
        let session = self.session()?;
        if !matches!(
            self.status_in_session(&session)?,
            BasicRecoveryStatus::Clean
        ) {
            return Err(CaptureError::RecoveryRequired);
        }
        #[cfg(not(windows))]
        {
            let _ = (plan, authority, validate);
            return Err(CaptureError::Unsupported);
        }
        #[cfg(windows)]
        self.execute_windows_basic_restore(&session, plan, authority, validate)
    }

    #[cfg(windows)]
    fn execute_windows_basic_restore<C>(
        &mut self,
        session: &Session,
        plan: &BasicRestorePlan,
        authority: &BasicRestoreAuthority,
        validate: &mut C,
    ) -> Result<BasicRestoreResult, CaptureError>
    where
        C: FnMut() -> Result<(), CaptureError>,
    {
        if self.load_plan(session, &plan.plan_id)? != *plan
            || plan.lineage_head_digest != authority.expected_head.head_digest
        {
            return Err(CaptureError::InvalidWitness);
        }
        if self.build_proposal(
            session,
            &plan.checkpoint_id,
            &plan.selected_paths,
            authority,
        )? != plan.proposal
        {
            return Err(CaptureError::Dirty);
        }
        // Refuse replay of even a previously completed restore plan.
        let restores = child(&session.partition, "restores")?;
        if !missing(&restores, &plan.plan_id)? {
            return Err(CaptureError::RecoveryRequired);
        }
        #[cfg(test)]
        let fault = self.restore_fault.take();
        #[cfg(test)]
        if matches!(fault, Some(TestRestoreFault::BeforeSafetySync)) {
            return Err(CaptureError::Interrupted);
        }
        let safety = self.capture_in_session(
            session,
            CaptureSelection::Paths {
                paths: plan.selected_paths.clone(),
            },
        )?;
        for operation in &plan.proposal.operations {
            let state = safety
                .entries
                .iter()
                .find(|entry| {
                    path_key(&entry.path, self.config.scope.path_semantics).ok()
                        == path_key(&operation.path, self.config.scope.path_semantics).ok()
                })
                .ok_or(CaptureError::Dirty)?;
            if pure_state(&state.state)? != operation.expected_current {
                return Err(CaptureError::Dirty);
            }
        }
        let intent = Intent {
            version: 1,
            plan_id: plan.plan_id.clone(),
            scope_digest: plan.scope_digest.clone(),
            plan_digest: plan.plan_digest.clone(),
            safety_checkpoint_id: safety.checkpoint_id.clone(),
            selected_paths: plan.selected_paths.clone(),
        };
        let prepared = encode(&intent, self.config.policy.limits.manifest_bytes)?;
        let displaced_bytes =
            plan.proposal
                .operations
                .iter()
                .try_fold(0u64, |sum, operation| {
                    let bytes = if let FileState::File { content, .. } = &operation.expected_current
                    {
                        content.byte_count
                    } else {
                        0
                    };
                    sum.checked_add(bytes).ok_or(CaptureError::Budget)
                })?;
        let receipt_bytes = (plan.proposal.operations.len() as u64)
            .checked_mul(4096 + 64)
            .ok_or(CaptureError::Budget)?;
        self.reserve_restore(
            session,
            prepared.len() as u64 + receipt_bytes + displaced_bytes + 8192,
            plan.proposal
                .operations
                .len()
                .checked_mul(3)
                .and_then(|n| n.checked_add(6))
                .ok_or(CaptureError::Budget)?,
        )?;
        let directory = child(&restores, &plan.plan_id)?;
        create(&directory, "prepared.json", &prepared)
            .map_err(|_| CaptureError::RecoveryRequired)?;
        #[cfg(test)]
        if matches!(fault, Some(TestRestoreFault::AfterPreparedBeforeAck)) {
            return Err(CaptureError::RecoveryRequired);
        }
        create(
            &directory,
            "prepared-ack",
            checkpoint_digest(&prepared).as_bytes(),
        )
        .map_err(|_| CaptureError::RecoveryRequired)?;
        let mut changed = false;
        let mut restored = Vec::new();
        for (index, operation) in plan.proposal.operations.iter().enumerate() {
            #[cfg(test)]
            if let Some(hook) = self.restore_hook.as_mut() {
                hook(TestRestorePhase::BeforeTargetLease);
            }
            let parent = held_parent(&session.workspace.directory, &operation.path)
                .map_err(|_| CaptureError::RecoveryRequired)?;
            let desired = match &operation.desired {
                FileState::File { content, .. } => Some(
                    self.read_blob(session, content)
                        .map_err(|_| CaptureError::RecoveryRequired)?,
                ),
                _ => None,
            };
            // Same held target is checked and written; no separate weak pathname
            // clear-readonly operation or overwrite rename is substituted.
            let mut target = match &operation.expected_current {
                FileState::File { .. } if matches!(&operation.desired, FileState::File { .. }) => {
                    Some(open_read_write(&parent).map_err(|_| CaptureError::RecoveryRequired)?)
                }
                FileState::File { .. } => Some(
                    open_file(&parent.directory, &parent.name)
                        .map_err(|_| CaptureError::RecoveryRequired)?,
                ),
                FileState::Absent => None,
            };
            if let Some(file) = target.as_mut() {
                let (bytes, mode) = read_open_file(file, self.config.policy.limits.file_bytes)
                    .map_err(|_| CaptureError::RecoveryRequired)?;
                if state_from_read(&bytes, mode).map_err(|_| CaptureError::RecoveryRequired)?
                    != operation.expected_current
                {
                    if !changed {
                        self.finish_intent(&directory, &intent, false)
                            .map_err(|_| CaptureError::RecoveryRequired)?;
                        return Err(CaptureError::Dirty);
                    }
                    return Err(CaptureError::RecoveryRequired);
                }
            } else if !missing(&parent.directory, &parent.name)
                .map_err(|_| CaptureError::RecoveryRequired)?
            {
                if !changed {
                    self.finish_intent(&directory, &intent, false)
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    return Err(CaptureError::Dirty);
                }
                return Err(CaptureError::RecoveryRequired);
            }
            create(
                &directory,
                &format!("{index:05}-applying.json"),
                &encode(operation, 4096).map_err(|_| CaptureError::RecoveryRequired)?,
            )
            .map_err(|_| CaptureError::RecoveryRequired)?;
            let effect: Result<(), CaptureError> = match desired {
                Some(bytes) => {
                    if let Err(error) = validate() {
                        if !changed {
                            self.finish_intent(&directory, &intent, false)
                                .map_err(|_| CaptureError::RecoveryRequired)?;
                            return Err(error);
                        }
                        return Err(CaptureError::RecoveryRequired);
                    }
                    if target.is_none() {
                        #[cfg(test)]
                        if let Some(hook) = self.restore_hook.as_mut() {
                            hook(TestRestorePhase::BeforeAbsentCreate);
                        }
                        let mut options = OpenOptions::new();
                        options
                            .read(true)
                            .write(true)
                            .create_new(true)
                            .follow(FollowSymlinks::No)
                            .nonblock(true);
                        {
                            use cap_std::fs::OpenOptionsExt;
                            options.share_mode(1);
                        }
                        match parent.directory.open_with(&parent.name, &options) {
                            Ok(file) => {
                                target = Some(file);
                            }
                            Err(error)
                                if error.kind() == std::io::ErrorKind::AlreadyExists
                                    && !changed =>
                            {
                                self.finish_intent(&directory, &intent, false)
                                    .map_err(|_| CaptureError::RecoveryRequired)?;
                                return Err(CaptureError::Dirty);
                            }
                            Err(_) => return Err(CaptureError::RecoveryRequired),
                        }
                    }
                    let file = target.as_mut().ok_or(CaptureError::RecoveryRequired)?;
                    let metadata = file
                        .metadata()
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    if linked(&metadata) || !metadata.is_file() || metadata.nlink() != 1 {
                        return Err(CaptureError::RecoveryRequired);
                    }
                    changed = true;
                    file.seek(SeekFrom::Start(0))
                        .and_then(|_| file.set_len(0))
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    let first = bytes.len().min(1);
                    file.write_all(&bytes[..first])
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    #[cfg(test)]
                    if matches!(fault, Some(TestRestoreFault::AfterFirstTargetWrite)) {
                        file.sync_all()
                            .map_err(|_| CaptureError::RecoveryRequired)?;
                        return Err(CaptureError::RecoveryRequired);
                    }
                    file.write_all(&bytes[first..])
                        .and_then(|_| file.sync_all())
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    let mode_result = {
                        #[cfg(test)]
                        {
                            if matches!(fault, Some(TestRestoreFault::ModeMetadataReadFails)) {
                                Err(CaptureError::Unavailable)
                            } else {
                                apply_mode(file, &operation.desired)
                            }
                        }
                        #[cfg(not(test))]
                        {
                            apply_mode(file, &operation.desired)
                        }
                    };
                    mode_result.map_err(|_| CaptureError::RecoveryRequired)?;
                    let (actual, mode) = read_open_file(file, self.config.policy.limits.file_bytes)
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    if state_from_read(&actual, mode).map_err(|_| CaptureError::RecoveryRequired)?
                        != operation.desired
                    {
                        return Err(CaptureError::RecoveryRequired);
                    }
                    Ok(())
                }
                None => {
                    let metadata = target
                        .as_ref()
                        .ok_or(CaptureError::RecoveryRequired)?
                        .metadata()
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    let expected_identity = (metadata.dev(), metadata.ino());
                    drop(target.take()); // no-delete-share target must close before preserve-first move
                    #[cfg(test)]
                    if let Some(hook) = self.restore_hook.as_mut() {
                        hook(TestRestorePhase::BeforeCreatedMove);
                    }
                    let displaced = child(&directory, "displaced")
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    let name = format!("{}.blob", checkpoint_digest(operation.path.as_bytes()));
                    // canonicalize existing held roots to Windows verbatim paths
                    // before appending digest members. atomicwrites forwards
                    // UTF-16 directly to MoveFileExW without adding a long-path
                    // prefix, while this owned destination can exceed MAX_PATH.
                    let destination = self
                        .config
                        .store_root
                        .canonicalize()
                        .map_err(|_| CaptureError::RecoveryRequired)?
                        .join("scopes")
                        .join(checkpoint_scope_digest(&self.config.scope))
                        .join("restores")
                        .join(&plan.plan_id)
                        .join("displaced")
                        .join(&name);
                    let source = self
                        .config
                        .workspace_root
                        .canonicalize()
                        .map_err(|_| CaptureError::RecoveryRequired)?
                        .join(&operation.path);
                    // move_atomic has no overwrite/copy-across-volume fallback.
                    // All source/destination ancestors remain held; preserve the
                    // actual moved object before deciding whether it was ours.
                    // Storage/path preparation precedes the last authority read:
                    // no persistence wait can carry a stale Stop/grant decision
                    // into removal of a file created by the native action.
                    if let Err(error) = validate() {
                        if !changed {
                            self.finish_intent(&directory, &intent, false)
                                .map_err(|_| CaptureError::RecoveryRequired)?;
                            return Err(error);
                        }
                        return Err(CaptureError::RecoveryRequired);
                    }
                    atomicwrites::move_atomic(&source, &destination)
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    changed = true;
                    let mut moved =
                        open_file(&displaced, &name).map_err(|_| CaptureError::RecoveryRequired)?;
                    let metadata = moved
                        .metadata()
                        .map_err(|_| CaptureError::RecoveryRequired)?;
                    let (bytes, mode) =
                        read_open_file(&mut moved, self.config.policy.limits.file_bytes)
                            .map_err(|_| CaptureError::RecoveryRequired)?;
                    if (metadata.dev(), metadata.ino()) != expected_identity
                        || state_from_read(&bytes, mode)
                            .map_err(|_| CaptureError::RecoveryRequired)?
                            != operation.expected_current
                        || !missing(&parent.directory, &parent.name)
                            .map_err(|_| CaptureError::RecoveryRequired)?
                    {
                        return Err(CaptureError::RecoveryRequired);
                    }
                    Ok(())
                }
            };
            effect?;
            create(
                &directory,
                &format!("{index:05}-done"),
                digest_value(
                    b"anima-filesystem-checkpoint-restored-state-v1\0",
                    operation,
                )
                .map_err(|_| CaptureError::RecoveryRequired)?
                .as_bytes(),
            )
            .map_err(|_| CaptureError::RecoveryRequired)?;
            restored.push(operation.path.clone());
        }
        self.finish_intent(&directory, &intent, true)
            .map_err(|_| CaptureError::RecoveryRequired)?;
        Ok(BasicRestoreResult {
            restored_paths: restored,
            safety_checkpoint_id: safety.checkpoint_id,
        })
    }

    pub(super) fn read_safety_content(
        &self,
        plan_id: &str,
        path: &str,
    ) -> Result<Vec<u8>, CaptureError> {
        let session = self.session()?;
        if !digest_valid(plan_id) {
            return Err(CaptureError::Unavailable);
        }
        let restores = open_child(&session.partition, "restores")?;
        let directory = open_child(&restores, plan_id)?;
        let (intent, _) = self.load_intent(&session, &directory, plan_id)?;
        let record = self
            .history(&session)?
            .into_iter()
            .find(|record| record.checkpoint_id == intent.safety_checkpoint_id)
            .ok_or(CaptureError::Corrupt)?;
        let entry = record
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .ok_or(CaptureError::Unavailable)?;
        match &entry.state {
            CaptureState::File { content, .. } => self.read_blob(&session, content),
            _ => Err(CaptureError::Unavailable),
        }
    }

    pub(super) fn preserved_restore_content(
        &self,
        plan_id: &str,
        path: &str,
    ) -> Result<Vec<u8>, CaptureError> {
        let session = self.session()?;
        let plan = self.load_plan(&session, plan_id)?;
        let key = path_key(path, self.config.scope.path_semantics)?;
        if !plan.selected_paths.iter().any(|selected| {
            path_key(selected, self.config.scope.path_semantics)
                .ok()
                .as_ref()
                == Some(&key)
        }) {
            return Err(CaptureError::Unavailable);
        }
        let operation = plan
            .proposal
            .operations
            .iter()
            .find(|operation| {
                path_key(&operation.path, self.config.scope.path_semantics)
                    .ok()
                    .as_ref()
                    == Some(&key)
            })
            .ok_or(CaptureError::Unavailable)?;
        let restores = open_child(&session.partition, "restores")?;
        let directory = open_child(&restores, plan_id)?;
        self.load_intent(&session, &directory, plan_id)?;
        let displaced = open_child(&directory, "displaced")?;
        Ok(read_file(
            &displaced,
            &format!("{}.blob", checkpoint_digest(operation.path.as_bytes())),
            self.config.policy.limits.file_bytes,
        )?
        .0)
    }

    #[cfg(test)]
    pub(super) fn set_restore_fault(&mut self, fault: TestRestoreFault) {
        self.restore_fault = Some(fault);
    }
    #[cfg(test)]
    pub(super) fn set_restore_hook(&mut self, hook: Box<dyn FnMut(TestRestorePhase) + Send>) {
        self.restore_hook = Some(hook);
    }
}

#[cfg(windows)]
fn apply_mode(file: &File, state: &FileState) -> Result<(), CaptureError> {
    let metadata = match state {
        FileState::File { metadata, .. } => metadata,
        _ => return Err(CaptureError::InvalidWitness),
    };
    let mode: BasicMode = serde_json::from_slice(&metadata.attributes[0].bytes)
        .map_err(|_| CaptureError::InvalidWitness)?;
    let mut permissions = file.metadata().map_err(io_error)?.permissions();
    match mode {
        BasicMode::Windows { readonly } => permissions.set_readonly(readonly),
        _ => return Err(CaptureError::Unsupported),
    }
    file.set_permissions(permissions)
        .and_then(|_| file.sync_all())
        .map_err(|_| CaptureError::RecoveryRequired)
}
