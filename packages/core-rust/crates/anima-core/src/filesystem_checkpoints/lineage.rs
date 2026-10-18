//! Ordered checkpoint evidence only; authentic hosts own receipts, observations,
//! sealed index authority and all physical effects. Digests grant no authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    CheckpointBlob, CheckpointError, CheckpointScope, FileState, FilesystemCheckpoint,
    MutationDisposition, RestoreConflict, RestoreConflictReason, RestoreOperation, RestoreProposal,
};

pub const CHECKPOINT_LINEAGE_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_LINEAGE_ACTIONS: usize = 50;
pub const MAX_CHECKPOINT_LINEAGE_MEMBERS: usize = 50_000;
pub const MAX_CHECKPOINT_LINEAGE_STEPS_PER_PATH: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationActionOutcome {
    Acknowledged,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationChange {
    pub path: String,
    pub disposition: MutationDisposition,
    pub observed_preimage: FileState,
    pub acknowledged_postimage: Option<FileState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationAction {
    pub sequence: u64,
    pub action_id: String,
    pub native_run_id: String,
    pub native_tool_call_id: String,
    pub request_digest: String,
    pub outcome: MutationActionOutcome,
    pub changes: Vec<MutationChange>,
}

/// Field order is the frozen digest encoding, after the head domain prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineageHeadDigestInput {
    #[serde(deserialize_with = "deserialize_lineage_version")]
    pub version: u32,
    pub checkpoint_id: String,
    pub checkpoint_sequence: u64,
    pub manifest_digest: String,
    pub scope_digest: String,
    pub action_count: u32,
    pub last_sequence: u64,
    pub action_digests: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationLineage {
    #[serde(deserialize_with = "deserialize_lineage_version")]
    pub version: u32,
    pub checkpoint_id: String,
    pub checkpoint_sequence: u64,
    pub manifest_digest: String,
    pub scope_digest: String,
    pub action_count: u32,
    pub last_sequence: u64,
    pub actions: Vec<MutationAction>,
    pub head_digest: String,
}

/// Supplied independently by the authenticated host's sealed private index.
/// No decoded lineage or model-chosen run identity can construct its authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExpectedActionBinding {
    pub sequence: u64,
    pub action_id: String,
    pub native_run_id: String,
    pub native_tool_call_id: String,
    pub request_digest: String,
    pub action_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExpectedLineageHead {
    pub checkpoint_id: String,
    pub checkpoint_sequence: u64,
    pub manifest_digest: String,
    pub scope_digest: String,
    pub action_count: u32,
    pub last_sequence: u64,
    pub head_digest: String,
    pub action_bindings: Vec<ExpectedActionBinding>,
}

fn deserialize_lineage_version<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != CHECKPOINT_LINEAGE_VERSION {
        return Err(serde::de::Error::custom(
            "unsupported checkpoint lineage version",
        ));
    }
    Ok(version)
}

fn domain_digest(domain: &[u8], value: &impl Serialize) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    // Closed structs contain no fallible custom serializers. Planner callers
    // first stream-bound the full input before constructing these encodings.
    hash.update(serde_json::to_vec(value).expect("checkpoint lineage serialization"));
    format!("{:x}", hash.finalize())
}

pub fn checkpoint_manifest_digest(checkpoint: &FilesystemCheckpoint) -> String {
    domain_digest(b"anima-filesystem-checkpoint-manifest-v1\0", checkpoint)
}

pub fn checkpoint_action_digest(action: &MutationAction) -> String {
    domain_digest(b"anima-filesystem-checkpoint-action-v1\0", action)
}

pub fn checkpoint_lineage_head_digest(head: &LineageHeadDigestInput) -> String {
    domain_digest(b"anima-filesystem-checkpoint-lineage-head-v1\0", head)
}

pub fn prepare_safe_restore_with_lineage(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
    current: &[super::CheckpointEntry],
    lineage: &MutationLineage,
    expected_head: &ExpectedLineageHead,
) -> Result<RestoreProposal, CheckpointError> {
    prepare_safe_restore_with_lineage_profile(
        checkpoint,
        expected_scope,
        objects,
        current,
        lineage,
        expected_head,
        super::ValidationProfile::Strict,
    )
}

pub(super) fn prepare_safe_restore_with_lineage_profile(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
    current: &[super::CheckpointEntry],
    lineage: &MutationLineage,
    expected_head: &ExpectedLineageHead,
    profile: super::ValidationProfile,
) -> Result<RestoreProposal, CheckpointError> {
    let semantics = checkpoint.scope.path_semantics;
    // Preflight complete count/encoding bounds before any state composition,
    // metadata cloning, digest encoding allocation or raw object resolution.
    if lineage.actions.len() > MAX_CHECKPOINT_LINEAGE_ACTIONS
        || expected_head.action_bindings.len() > MAX_CHECKPOINT_LINEAGE_ACTIONS
    {
        return Err(CheckpointError::Budget);
    }
    let mut members = 0usize;
    for action in &lineage.actions {
        members = members
            .checked_add(action.changes.len())
            .ok_or(CheckpointError::Budget)?;
        if members > MAX_CHECKPOINT_LINEAGE_MEMBERS {
            return Err(CheckpointError::Budget);
        }
    }
    super::bounded_json(lineage)?;
    super::bounded_json(expected_head)?;
    super::validate_checkpoint_shape_with_profile(checkpoint, expected_scope, profile)?;
    super::validate_entries_with_profile(current, semantics, false, profile)?;
    if lineage.version != CHECKPOINT_LINEAGE_VERSION
        || lineage.checkpoint_id != checkpoint.checkpoint_id
        || lineage.checkpoint_sequence != checkpoint.sequence
        || lineage.scope_digest != checkpoint.scope_digest
        || lineage.manifest_digest != checkpoint_manifest_digest(checkpoint)
        || lineage.action_count as usize != lineage.actions.len()
        || lineage.last_sequence != lineage.actions.len() as u64
        || !super::valid_digest(&lineage.head_digest)
        || expected_head.checkpoint_id != lineage.checkpoint_id
        || expected_head.checkpoint_sequence != lineage.checkpoint_sequence
        || expected_head.manifest_digest != lineage.manifest_digest
        || expected_head.scope_digest != lineage.scope_digest
        || expected_head.action_count != lineage.action_count
        || expected_head.last_sequence != lineage.last_sequence
        || expected_head.head_digest != lineage.head_digest
        || expected_head.action_bindings.len() != lineage.actions.len()
    {
        return Err(CheckpointError::InvalidWitness);
    }
    let targets: HashMap<_, _> = checkpoint
        .entries
        .iter()
        .map(|entry| (super::path_key(&entry.path, semantics), &entry.state))
        .collect();
    let mut action_ids = HashSet::new();
    let mut native_ids = HashSet::new();
    let mut path_steps = HashMap::<String, usize>::new();
    let mut observed_bytes = 0;
    let mut action_digests = Vec::with_capacity(lineage.actions.len());
    for (index, (action, binding)) in lineage
        .actions
        .iter()
        .zip(&expected_head.action_bindings)
        .enumerate()
    {
        if action.sequence != index as u64 + 1
            || action.changes.is_empty()
            || !super::valid_id(&action.action_id)
            || !super::valid_id(&action.native_run_id)
            || !super::valid_id(&action.native_tool_call_id)
            || !super::valid_digest(&action.request_digest)
            || !action_ids.insert(action.action_id.as_str())
            || !native_ids.insert((
                action.native_run_id.as_str(),
                action.native_tool_call_id.as_str(),
            ))
            || binding.sequence != action.sequence
            || binding.action_id != action.action_id
            || binding.native_run_id != action.native_run_id
            || binding.native_tool_call_id != action.native_tool_call_id
            || binding.request_digest != action.request_digest
            || !super::valid_digest(&binding.action_digest)
        {
            return Err(CheckpointError::InvalidWitness);
        }
        // Metadata/content budgets include both sides of every observation;
        // repeated references count here even when physical blobs are shared.
        for change in &action.changes {
            super::validate_path(&change.path, semantics)?;
            let key = super::path_key(&change.path, semantics);
            if !targets.contains_key(&key) {
                return Err(CheckpointError::InvalidWitness);
            }
            let steps = path_steps.entry(key).or_default();
            *steps += 1;
            if *steps > MAX_CHECKPOINT_LINEAGE_STEPS_PER_PATH {
                return Err(CheckpointError::Budget);
            }
            super::validate_state_with_profile(
                &change.observed_preimage,
                &mut observed_bytes,
                profile,
            )?;
            if let Some(postimage) = &change.acknowledged_postimage {
                super::validate_state_with_profile(postimage, &mut observed_bytes, profile)?;
            }
            validate_transition(action.outcome, change)?;
        }
        // Reuse the parent's canonical path/alias/topology validators on both
        // complete preimages and the known postimages of this single action.
        // Full input has already been encoded-byte bounded before these clones.
        let preimages: Vec<_> = action
            .changes
            .iter()
            .map(|change| super::CheckpointEntry {
                path: change.path.clone(),
                state: change.observed_preimage.clone(),
            })
            .collect();
        let postimages: Vec<_> = action
            .changes
            .iter()
            .filter_map(|change| {
                change
                    .acknowledged_postimage
                    .as_ref()
                    .map(|state| super::CheckpointEntry {
                        path: change.path.clone(),
                        state: state.clone(),
                    })
            })
            .collect();
        super::validate_entries_with_profile(&preimages, semantics, true, profile)?;
        super::validate_entries_with_profile(&postimages, semantics, true, profile)?;
        let digest = checkpoint_action_digest(action);
        if digest != binding.action_digest {
            return Err(CheckpointError::InvalidWitness);
        }
        action_digests.push(digest);
    }
    let head = LineageHeadDigestInput {
        version: lineage.version,
        checkpoint_id: lineage.checkpoint_id.clone(),
        checkpoint_sequence: lineage.checkpoint_sequence,
        manifest_digest: lineage.manifest_digest.clone(),
        scope_digest: lineage.scope_digest.clone(),
        action_count: lineage.action_count,
        last_sequence: lineage.last_sequence,
        action_digests,
    };
    if checkpoint_lineage_head_digest(&head) != lineage.head_digest {
        return Err(CheckpointError::InvalidWitness);
    }
    // Resolve all original, observed and acknowledged state objects only after
    // every declared-byte and complete-input structural check has succeeded.
    let inventory = super::validate_objects(objects)?;
    for entry in checkpoint.entries.iter().chain(current) {
        super::resolve_state(&entry.state, &inventory)?;
    }
    for action in &lineage.actions {
        for change in &action.changes {
            super::resolve_state(&change.observed_preimage, &inventory)?;
            if let Some(postimage) = &change.acknowledged_postimage {
                super::resolve_state(postimage, &inventory)?;
            }
        }
    }
    struct Chain<'a> {
        last_acknowledgement: Option<&'a FileState>,
        discontinuous: bool,
        uncertain: bool,
        proven_creation: bool,
    }
    let mut chains = HashMap::<String, Chain<'_>>::new();
    for action in &lineage.actions {
        for change in &action.changes {
            let key = super::path_key(&change.path, semantics);
            let chain = chains.entry(key.clone()).or_insert_with(|| Chain {
                last_acknowledgement: Some(targets[&key]),
                discontinuous: false,
                uncertain: false,
                proven_creation: matches!(targets[&key], FileState::Absent)
                    && matches!(change.observed_preimage, FileState::Absent)
                    && change.disposition == MutationDisposition::Created
                    && action.outcome == MutationActionOutcome::Acknowledged,
            });
            if chain
                .last_acknowledgement
                .is_some_and(|previous| previous != &change.observed_preimage)
            {
                chain.discontinuous = true;
            }
            chain.uncertain |= action.outcome == MutationActionOutcome::Uncertain
                || change.disposition == MutationDisposition::Uncertain
                || change.acknowledged_postimage.is_none();
            chain.last_acknowledgement = change.acknowledged_postimage.as_ref();
        }
    }
    let observations: BTreeMap<_, _> = current
        .iter()
        .map(|entry| (super::path_key(&entry.path, semantics), &entry.state))
        .collect();
    let mut proposal = RestoreProposal {
        checkpoint_id: checkpoint.checkpoint_id.clone(),
        scope_digest: checkpoint.scope_digest.clone(),
        operations: Vec::new(),
        conflicts: Vec::new(),
    };
    for entry in &checkpoint.entries {
        let key = super::path_key(&entry.path, semantics);
        let chain = chains.get(&key);
        let observation = observations.get(&key).copied();
        let reason = match (chain, observation) {
            (Some(chain), _) if chain.uncertain => Some(RestoreConflictReason::Uncertain),
            (Some(chain), _) if chain.discontinuous => {
                Some(RestoreConflictReason::LineageDiscontinuity)
            }
            (_, None) => Some(RestoreConflictReason::IncompleteObservation),
            (None, Some(state)) if state == &entry.state => None,
            (None, Some(_)) => Some(RestoreConflictReason::MissingEvidence),
            (Some(chain), Some(state)) if chain.last_acknowledgement != Some(state) => {
                Some(RestoreConflictReason::Dirty)
            }
            (Some(_), Some(state)) if state == &entry.state => None,
            (Some(chain), Some(_))
                if matches!(entry.state, FileState::Absent) && !chain.proven_creation =>
            {
                Some(RestoreConflictReason::UnprovenCreation)
            }
            (Some(_), Some(state)) => {
                proposal.operations.push(RestoreOperation {
                    path: entry.path.clone(),
                    expected_current: state.clone(),
                    desired: entry.state.clone(),
                });
                None
            }
        };
        if let Some(reason) = reason {
            proposal.conflicts.push(RestoreConflict {
                path: entry.path.clone(),
                reason,
            });
        }
    }
    super::bounded_json(&proposal)?;
    Ok(proposal)
}

fn validate_transition(
    outcome: MutationActionOutcome,
    change: &MutationChange,
) -> Result<(), CheckpointError> {
    let acknowledged = outcome == MutationActionOutcome::Acknowledged;
    if acknowledged && change.acknowledged_postimage.is_none() {
        return Err(CheckpointError::InvalidWitness);
    }
    let valid = match change.disposition {
        MutationDisposition::Created => {
            matches!(change.observed_preimage, FileState::Absent)
                && change
                    .acknowledged_postimage
                    .as_ref()
                    .is_none_or(|state| matches!(state, FileState::File { .. }))
        }
        MutationDisposition::Replaced => {
            matches!(change.observed_preimage, FileState::File { .. })
                && change
                    .acknowledged_postimage
                    .as_ref()
                    .is_none_or(|state| matches!(state, FileState::File { .. }))
        }
        MutationDisposition::Deleted => {
            matches!(change.observed_preimage, FileState::File { .. })
                && change
                    .acknowledged_postimage
                    .as_ref()
                    .is_none_or(|state| matches!(state, FileState::Absent))
        }
        MutationDisposition::Uncertain => !acknowledged,
    };
    if valid {
        Ok(())
    } else {
        Err(CheckpointError::InvalidWitness)
    }
}

#[cfg(test)]
mod tests;
