//! Portable filesystem checkpoint proposals only. Hosts verify actual file I/O,
//! identity, metadata fidelity, authority and durability. These data grant none.
//! No filesystem, OS, process or database operation belongs in this module.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod content_basic_v1;
pub use content_basic_v1::{
    prepare_content_basic_v1_safe_restore_with_lineage,
    validate_content_basic_v1_filesystem_checkpoint, ContentBasicPermissionsV1,
};

mod lineage;
pub use lineage::{
    checkpoint_action_digest, checkpoint_lineage_head_digest, checkpoint_manifest_digest,
    prepare_safe_restore_with_lineage, ExpectedActionBinding, ExpectedLineageHead,
    LineageHeadDigestInput, MutationAction, MutationActionOutcome, MutationChange, MutationLineage,
    CHECKPOINT_LINEAGE_VERSION, MAX_CHECKPOINT_LINEAGE_ACTIONS, MAX_CHECKPOINT_LINEAGE_MEMBERS,
    MAX_CHECKPOINT_LINEAGE_STEPS_PER_PATH,
};

pub const FILESYSTEM_CHECKPOINT_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_ENTRIES: usize = 50_000;
pub const MAX_CHECKPOINT_PATH_BYTES: usize = 1_024;
pub const MAX_CHECKPOINT_ID_BYTES: usize = 128;
pub const MAX_CHECKPOINT_DEPTH: usize = 64;
pub const MAX_CHECKPOINT_STREAM_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_CHECKPOINT_CONTENT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_CHECKPOINT_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CHECKPOINT_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_CHECKPOINT_STORE_BYTES: u64 = 500 * 1024 * 1024;
pub const MAX_CHECKPOINT_HISTORY_RECORDS: usize = 1_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointScope {
    pub owner_id: String,
    pub workspace_id: String,
    pub environment_id: String,
    pub path_semantics: PathSemantics,
    pub root_identity_digest: String,
    pub policy_digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSemantics {
    WindowsInsensitive,
    PosixSensitive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobReference {
    pub sha256: String,
    pub byte_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointBlob {
    pub reference: BlobReference,
    pub bytes: Vec<u8>,
}

/// Exact metadata bytes are host-encoded, not parsed as platform metadata here.
/// The closed attribute tags preserve fidelity requirements without asserting
/// that any physical adapter has captured or can restore those requirements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataKind {
    BasicPermissions,
    ModificationTime,
    CreationTime,
    AccessTime,
    WindowsSecurity,
    ExtendedAttributes,
    NamedStreamIndex,
    HardLinkTopology,
    PlatformFlags,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataAttribute {
    pub kind: MetadataKind,
    pub encoding_version: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataSnapshot {
    pub required: Vec<MetadataKind>,
    pub attributes: Vec<MetadataAttribute>,
}

/// Initial portable regular-file/absence contract. Additional entry kinds need
/// a separately reviewed typed contract; this is not full platform acceptance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileState {
    Absent,
    File {
        content: BlobReference,
        metadata: MetadataSnapshot,
    },
}

impl<'de> Deserialize<'de> for FileState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Serde's internally tagged unit variant discards unknown content even
        // with deny_unknown_fields. An empty struct variant closes that gap.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum ClosedState {
            Absent {},
            File {
                content: BlobReference,
                metadata: MetadataSnapshot,
            },
        }
        match ClosedState::deserialize(deserializer)? {
            ClosedState::Absent {} => Ok(Self::Absent),
            ClosedState::File { content, metadata } => Ok(Self::File { content, metadata }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointEntry {
    pub path: String,
    pub state: FileState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemCheckpoint {
    #[serde(deserialize_with = "deserialize_checkpoint_version")]
    pub version: u32,
    pub checkpoint_id: String,
    pub sequence: u64,
    pub scope: CheckpointScope,
    pub scope_digest: String,
    pub entries: Vec<CheckpointEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationDisposition {
    Created,
    Replaced,
    Deleted,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationWitness {
    pub action_id: String,
    pub checkpoint_id: String,
    pub scope_digest: String,
    pub path: String,
    pub disposition: MutationDisposition,
    pub acknowledged_postimage: FileState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreOperation {
    pub path: String,
    pub expected_current: FileState,
    pub desired: FileState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreConflictReason {
    MissingEvidence,
    IncompleteObservation,
    Dirty,
    Uncertain,
    UnprovenCreation,
    LineageDiscontinuity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreConflict {
    pub path: String,
    pub reason: RestoreConflictReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreProposal {
    pub checkpoint_id: String,
    pub scope_digest: String,
    pub operations: Vec<RestoreOperation>,
    pub conflicts: Vec<RestoreConflict>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPin {
    ActiveTurn,
    InFlightEffect,
    RestoreSelected,
    Safety,
    RecoveryRequired,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionRecord {
    pub checkpoint_id: String,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub manifest_bytes: u64,
    pub object_digests: Vec<String>,
    pub pins: Vec<RetentionPin>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_groups: usize,
    pub max_bytes: u64,
    pub retention_ms: u64,
    pub observed_now_ms: u64,
    pub last_observed_now_ms: u64,
    pub required_reserve_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionProposal {
    pub retained_checkpoint_ids: Vec<String>,
    pub prune_checkpoint_ids: Vec<String>,
    pub prune_object_digests: Vec<String>,
    pub used_bytes_after: u64,
    pub can_reserve: bool,
    pub observed_now_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    InvalidManifest,
    InvalidScope,
    InvalidPath,
    InvalidMetadata,
    InvalidObject,
    InvalidWitness,
    InvalidRetention,
    Budget,
}

fn deserialize_checkpoint_version<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != FILESYSTEM_CHECKPOINT_VERSION {
        return Err(serde::de::Error::custom(
            "unsupported filesystem checkpoint version",
        ));
    }
    Ok(version)
}

pub fn checkpoint_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Domain: `anima-filesystem-checkpoint-scope-v1\0`, then exact serde JSON of
/// this closed struct. The caller separately supplies the authentic expected
/// scope; a matching digest alone can never establish authority.
pub fn checkpoint_scope_digest(scope: &CheckpointScope) -> String {
    let mut hash = Sha256::new();
    hash.update(b"anima-filesystem-checkpoint-scope-v1\0");
    // This closed struct contains only strings and a closed enum, which serde
    // JSON serializes infallibly. Invalid/unbounded scopes are rejected below.
    hash.update(serde_json::to_vec(scope).expect("checkpoint scope serialization"));
    hex::encode(hash.finalize())
}

pub fn validate_filesystem_checkpoint(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
) -> Result<(), CheckpointError> {
    validate_filesystem_checkpoint_with_profile(
        checkpoint,
        expected_scope,
        objects,
        ValidationProfile::Strict,
    )
}

#[derive(Clone, Copy)]
enum ValidationProfile {
    Strict,
    ContentBasicV1(PathSemantics),
}

fn validate_filesystem_checkpoint_with_profile(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
    profile: ValidationProfile,
) -> Result<(), CheckpointError> {
    validate_checkpoint_shape_with_profile(checkpoint, expected_scope, profile)?;
    let inventory = validate_objects(objects)?;
    for entry in &checkpoint.entries {
        resolve_state(&entry.state, &inventory)?;
    }
    Ok(())
}

/// Current observations and witnesses must come from authenticated host-owned
/// reads/receipts. This pure function does not verify actual filesystem state.
pub fn prepare_safe_restore(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
    current: &[CheckpointEntry],
    witnesses: &[MutationWitness],
) -> Result<RestoreProposal, CheckpointError> {
    validate_checkpoint_shape(checkpoint, expected_scope)?;
    let semantics = checkpoint.scope.path_semantics;
    validate_entries(current, semantics, false)?;
    if witnesses.len() > MAX_CHECKPOINT_ENTRIES {
        return Err(CheckpointError::Budget);
    }
    // Bound the complete encoded evidence before hashing referenced objects,
    // examining receipts or cloning any output. A no-op proposal is no excuse
    // for an oversized input containing otherwise valid metadata attributes.
    bounded_json(witnesses)?;
    let inventory = validate_objects(objects)?;
    let mut targets = BTreeMap::new();
    for entry in &checkpoint.entries {
        resolve_state(&entry.state, &inventory)?;
        targets.insert(path_key(&entry.path, semantics), entry);
    }
    let mut observations = BTreeMap::new();
    for entry in current {
        resolve_state(&entry.state, &inventory)?;
        observations.insert(path_key(&entry.path, semantics), &entry.state);
    }
    let mut evidence = BTreeMap::new();
    let mut witnessed_bytes = 0;
    for witness in witnesses {
        validate_path(&witness.path, semantics).map_err(|_| CheckpointError::InvalidWitness)?;
        if !valid_id(&witness.action_id)
            || witness.checkpoint_id != checkpoint.checkpoint_id
            || witness.scope_digest != checkpoint.scope_digest
        {
            return Err(CheckpointError::InvalidWitness);
        }
        validate_state(&witness.acknowledged_postimage, &mut witnessed_bytes)?;
        resolve_state(&witness.acknowledged_postimage, &inventory)?;
        let key = path_key(&witness.path, semantics);
        let target = targets.get(&key).ok_or(CheckpointError::InvalidWitness)?;
        if evidence.insert(key, witness).is_some() {
            return Err(CheckpointError::InvalidWitness);
        }
        // A false replacement claim for an absent preimage is reported as an
        // unproven creation below. Other impossible receipt shapes are invalid.
        let valid_transition = match witness.disposition {
            MutationDisposition::Created => {
                matches!(target.state, FileState::Absent)
                    && matches!(witness.acknowledged_postimage, FileState::File { .. })
            }
            MutationDisposition::Replaced => {
                matches!(witness.acknowledged_postimage, FileState::File { .. })
            }
            MutationDisposition::Deleted => {
                matches!(target.state, FileState::File { .. })
                    && matches!(witness.acknowledged_postimage, FileState::Absent)
            }
            MutationDisposition::Uncertain => true,
        };
        if !valid_transition {
            return Err(CheckpointError::InvalidWitness);
        }
    }
    let mut proposal = RestoreProposal {
        checkpoint_id: checkpoint.checkpoint_id.clone(),
        scope_digest: checkpoint.scope_digest.clone(),
        operations: Vec::new(),
        conflicts: Vec::new(),
    };
    for entry in &checkpoint.entries {
        let key = path_key(&entry.path, semantics);
        let reason = match observations.get(&key) {
            None => Some(RestoreConflictReason::IncompleteObservation),
            Some(state) if **state == entry.state => None,
            Some(state) => match evidence.get(&key) {
                None => Some(RestoreConflictReason::MissingEvidence),
                Some(witness) if witness.disposition == MutationDisposition::Uncertain => {
                    Some(RestoreConflictReason::Uncertain)
                }
                Some(witness) if **state != witness.acknowledged_postimage => {
                    Some(RestoreConflictReason::Dirty)
                }
                Some(witness)
                    if matches!(entry.state, FileState::Absent)
                        && witness.disposition != MutationDisposition::Created =>
                {
                    Some(RestoreConflictReason::UnprovenCreation)
                }
                Some(_) => {
                    proposal.operations.push(RestoreOperation {
                        path: entry.path.clone(),
                        expected_current: (*state).clone(),
                        desired: entry.state.clone(),
                    });
                    None
                }
            },
        };
        if let Some(reason) = reason {
            proposal.conflicts.push(RestoreConflict {
                path: entry.path.clone(),
                reason,
            });
        }
    }
    // The returned JSON is bounded independently from the input manifest;
    // expected postimages can double the metadata represented in an operation.
    bounded_json(&proposal)?;
    Ok(proposal)
}

/// Object inventory is unique by digest. Pins and protected object references
/// retain recovery data even when count/age/space limits cannot be satisfied.
pub fn plan_checkpoint_retention(
    records: &[RetentionRecord],
    objects: &[BlobReference],
    protected_objects: &[String],
    policy: &RetentionPolicy,
) -> Result<RetentionProposal, CheckpointError> {
    if records.len() > MAX_CHECKPOINT_HISTORY_RECORDS
        || objects.len() > MAX_CHECKPOINT_ENTRIES
        || protected_objects.len() > MAX_CHECKPOINT_ENTRIES
    {
        return Err(CheckpointError::Budget);
    }
    if policy.max_groups == 0
        || policy.max_groups > MAX_CHECKPOINT_HISTORY_RECORDS
        || policy.max_bytes == 0
        || policy.max_bytes > MAX_CHECKPOINT_STORE_BYTES
        || policy.retention_ms == 0
    {
        return Err(CheckpointError::InvalidRetention);
    }
    let now = policy.observed_now_ms.max(policy.last_observed_now_ms);
    let mut inventory = BTreeMap::new();
    for object in objects {
        validate_reference(object)?;
        if inventory
            .insert(object.sha256.as_str(), object.byte_count)
            .is_some()
        {
            return Err(CheckpointError::InvalidRetention);
        }
    }
    let mut protected = BTreeSet::new();
    for digest in protected_objects {
        if !inventory.contains_key(digest.as_str()) || !protected.insert(digest.as_str()) {
            return Err(CheckpointError::InvalidRetention);
        }
    }
    let mut ids = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    let mut references = BTreeMap::<&str, usize>::new();
    let mut manifest_bytes = 0u64;
    for record in records {
        if !valid_id(&record.checkpoint_id)
            || record.sequence == 0
            || !ids.insert(record.checkpoint_id.as_str())
            || !sequences.insert(record.sequence)
            || record.manifest_bytes == 0
        {
            return Err(CheckpointError::InvalidRetention);
        }
        if record.manifest_bytes > MAX_CHECKPOINT_MANIFEST_BYTES as u64
            || record.object_digests.len() > MAX_CHECKPOINT_ENTRIES
            || record.pins.len() > 6
        {
            return Err(CheckpointError::Budget);
        }
        bounded_json(record)?;
        for (index, pin) in record.pins.iter().enumerate() {
            if record.pins[..index].contains(pin) {
                return Err(CheckpointError::InvalidRetention);
            }
        }
        let mut seen = BTreeSet::new();
        for digest in &record.object_digests {
            if !inventory.contains_key(digest.as_str()) || !seen.insert(digest.as_str()) {
                return Err(CheckpointError::InvalidRetention);
            }
            *references.entry(digest).or_default() += 1;
        }
        manifest_bytes = manifest_bytes
            .checked_add(record.manifest_bytes)
            .ok_or(CheckpointError::Budget)?;
    }
    let mut used = manifest_bytes;
    for (digest, bytes) in &inventory {
        if references.contains_key(digest) || protected.contains(digest) {
            used = used.checked_add(*bytes).ok_or(CheckpointError::Budget)?;
        }
    }
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|record| record.sequence);
    let mut retained = vec![true; ordered.len()];
    let mut count = ordered.len();
    // Keep the newest generation even if age/quota limits cannot be met. Pins
    // and the newest floor never become prune candidates to claim success.
    for (index, record) in ordered.iter().enumerate() {
        if index + 1 == ordered.len() || !record.pins.is_empty() {
            continue;
        }
        let expired = now.saturating_sub(record.recorded_at_ms) >= policy.retention_ms;
        let fits = used
            .checked_add(policy.required_reserve_bytes)
            .is_some_and(|bytes| bytes <= policy.max_bytes);
        if !expired && count <= policy.max_groups && fits {
            continue;
        }
        retained[index] = false;
        count -= 1;
        used -= record.manifest_bytes;
        for digest in &record.object_digests {
            let remaining = references
                .get_mut(digest.as_str())
                .expect("validated reference");
            *remaining -= 1;
            if *remaining == 0 && !protected.contains(digest.as_str()) {
                used -= inventory[digest.as_str()];
            }
        }
    }
    let mut proposal = RetentionProposal {
        retained_checkpoint_ids: Vec::new(),
        prune_checkpoint_ids: Vec::new(),
        prune_object_digests: Vec::new(),
        used_bytes_after: used,
        can_reserve: count <= policy.max_groups
            && used
                .checked_add(policy.required_reserve_bytes)
                .is_some_and(|bytes| bytes <= policy.max_bytes),
        observed_now_ms: now,
    };
    for (record, keep) in ordered.into_iter().zip(retained) {
        if keep {
            proposal
                .retained_checkpoint_ids
                .push(record.checkpoint_id.clone());
        } else {
            proposal
                .prune_checkpoint_ids
                .push(record.checkpoint_id.clone());
        }
    }
    for digest in inventory.keys() {
        if references.get(digest).copied().unwrap_or(0) == 0 && !protected.contains(digest) {
            proposal.prune_object_digests.push((*digest).to_owned());
        }
    }
    Ok(proposal)
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty()
        && id.len() <= MAX_CHECKPOINT_ID_BYTES
        && !id.chars().any(char::is_control)
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_scope(scope: &CheckpointScope) -> Result<(), CheckpointError> {
    if !valid_id(&scope.owner_id)
        || !valid_id(&scope.workspace_id)
        || !valid_id(&scope.environment_id)
        || !valid_digest(&scope.root_identity_digest)
        || !valid_digest(&scope.policy_digest)
    {
        return Err(CheckpointError::InvalidScope);
    }
    Ok(())
}

fn path_key(path: &str, semantics: PathSemantics) -> String {
    match semantics {
        // validate_path restricts this initial profile to ASCII. Unicode
        // lowercase cannot establish actual Windows filesystem identity.
        PathSemantics::WindowsInsensitive => path.to_ascii_lowercase(),
        PathSemantics::PosixSensitive => path.to_owned(),
    }
}

fn validate_path(path: &str, semantics: PathSemantics) -> Result<(), CheckpointError> {
    if path.is_empty()
        || path.len() > MAX_CHECKPOINT_PATH_BYTES
        || path.chars().any(char::is_control)
    {
        return Err(CheckpointError::InvalidPath);
    }
    if semantics == PathSemantics::WindowsInsensitive && !path.is_ascii() {
        // Full Windows Unicode support requires host-attested comparison and
        // opened-object identity evidence; it remains adapter acceptance work.
        return Err(CheckpointError::InvalidPath);
    }
    let mut depth = 0;
    for component in path.split('/') {
        depth += 1;
        if depth > MAX_CHECKPOINT_DEPTH
            || component.is_empty()
            || component == "."
            || component == ".."
        {
            return Err(CheckpointError::InvalidPath);
        }
        if semantics == PathSemantics::WindowsInsensitive {
            if component.ends_with(['.', ' '])
                || component.chars().any(|ch| "\\:<>\"|?*".contains(ch))
            {
                return Err(CheckpointError::InvalidPath);
            }
            let stem = component
                .split('.')
                .next()
                .unwrap_or_default()
                .trim_end_matches(' ')
                .to_uppercase();
            let device = matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                });
            if device {
                return Err(CheckpointError::InvalidPath);
            }
        }
    }
    Ok(())
}

fn validate_reference(reference: &BlobReference) -> Result<(), CheckpointError> {
    if !valid_digest(&reference.sha256) {
        return Err(CheckpointError::InvalidObject);
    }
    if reference.byte_count > MAX_CHECKPOINT_STREAM_BYTES {
        return Err(CheckpointError::Budget);
    }
    Ok(())
}

fn validate_state(state: &FileState, content_bytes: &mut u64) -> Result<(), CheckpointError> {
    validate_state_with_profile(state, content_bytes, ValidationProfile::Strict)
}

fn validate_state_with_profile(
    state: &FileState,
    content_bytes: &mut u64,
    profile: ValidationProfile,
) -> Result<(), CheckpointError> {
    if let FileState::File { content, metadata } = state {
        validate_reference(content)?;
        *content_bytes = content_bytes
            .checked_add(content.byte_count)
            .ok_or(CheckpointError::Budget)?;
        if *content_bytes > MAX_CHECKPOINT_CONTENT_BYTES {
            return Err(CheckpointError::Budget);
        }
        if metadata.required.len() > 9
            || metadata.attributes.len() > 9
            || metadata.required.windows(2).any(|pair| pair[0] >= pair[1])
            || metadata
                .attributes
                .windows(2)
                .any(|pair| pair[0].kind >= pair[1].kind)
        {
            return Err(CheckpointError::InvalidMetadata);
        }
        let mut bytes = 0usize;
        for attribute in &metadata.attributes {
            bytes = bytes
                .checked_add(attribute.bytes.len())
                .ok_or(CheckpointError::Budget)?;
            if bytes > MAX_CHECKPOINT_METADATA_BYTES {
                return Err(CheckpointError::Budget);
            }
            if attribute.encoding_version != 1 {
                return Err(CheckpointError::InvalidMetadata);
            }
        }
        if metadata.required.iter().any(|required| {
            !metadata
                .attributes
                .iter()
                .any(|attr| attr.kind == *required)
        }) {
            return Err(CheckpointError::InvalidMetadata);
        }
        match profile {
            ValidationProfile::Strict => {
                if !metadata.required.contains(&MetadataKind::BasicPermissions)
                    || !metadata.required.contains(&MetadataKind::ModificationTime)
                {
                    return Err(CheckpointError::InvalidMetadata);
                }
            }
            ValidationProfile::ContentBasicV1(semantics) => {
                content_basic_v1::validate_metadata(metadata, semantics)?
            }
        }
    }
    Ok(())
}

fn validate_entries(
    entries: &[CheckpointEntry],
    semantics: PathSemantics,
    ordered: bool,
) -> Result<(), CheckpointError> {
    validate_entries_with_profile(entries, semantics, ordered, ValidationProfile::Strict)
}

fn validate_entries_with_profile(
    entries: &[CheckpointEntry],
    semantics: PathSemantics,
    ordered: bool,
    profile: ValidationProfile,
) -> Result<(), CheckpointError> {
    if entries.len() > MAX_CHECKPOINT_ENTRIES {
        return Err(CheckpointError::Budget);
    }
    let mut seen = HashMap::new();
    let mut content_bytes = 0;
    let mut previous: Option<&str> = None;
    for entry in entries {
        validate_path(&entry.path, semantics)?;
        if seen
            .insert(
                path_key(&entry.path, semantics),
                matches!(entry.state, FileState::File { .. }),
            )
            .is_some()
            || (ordered && previous.is_some_and(|path| path >= entry.path.as_str()))
        {
            return Err(CheckpointError::InvalidPath);
        }
        previous = Some(&entry.path);
        validate_state_with_profile(&entry.state, &mut content_bytes, profile)?;
    }
    // Check component prefixes after collecting all entries, so unordered
    // current observations and case aliases receive exactly the same check.
    // At most depth-1 hash lookups per entry; never compare all entry pairs.
    for (path, present) in &seen {
        for (separator, _) in path.match_indices('/') {
            if let Some(ancestor_present) = seen.get(&path[..separator]) {
                // A regular-file ancestor cannot contain a child of any kind.
                // An absent ancestor also cannot contain a present child; two
                // absent observations may simply describe a missing subtree.
                if *ancestor_present || *present {
                    return Err(CheckpointError::InvalidPath);
                }
            }
        }
    }
    bounded_json(entries)
}

fn validate_checkpoint_shape(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
) -> Result<(), CheckpointError> {
    validate_checkpoint_shape_with_profile(checkpoint, expected_scope, ValidationProfile::Strict)
}

fn validate_checkpoint_shape_with_profile(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    profile: ValidationProfile,
) -> Result<(), CheckpointError> {
    if checkpoint.version != FILESYSTEM_CHECKPOINT_VERSION
        || !valid_id(&checkpoint.checkpoint_id)
        || checkpoint.sequence == 0
    {
        return Err(CheckpointError::InvalidManifest);
    }
    validate_scope(expected_scope)?;
    if checkpoint.scope != *expected_scope
        || checkpoint.scope_digest != checkpoint_scope_digest(expected_scope)
    {
        return Err(CheckpointError::InvalidScope);
    }
    validate_entries_with_profile(
        &checkpoint.entries,
        checkpoint.scope.path_semantics,
        true,
        profile,
    )?;
    bounded_json(checkpoint)
}

fn validate_objects(objects: &[CheckpointBlob]) -> Result<BTreeMap<&str, u64>, CheckpointError> {
    if objects.len() > MAX_CHECKPOINT_ENTRIES {
        return Err(CheckpointError::Budget);
    }
    let mut inventory = BTreeMap::new();
    let mut total = 0u64;
    for object in objects {
        validate_reference(&object.reference)?;
        total = total
            .checked_add(object.bytes.len() as u64)
            .ok_or(CheckpointError::Budget)?;
        if total > MAX_CHECKPOINT_STORE_BYTES {
            return Err(CheckpointError::Budget);
        }
        if object.bytes.len() as u64 != object.reference.byte_count
            || inventory
                .insert(
                    object.reference.sha256.as_str(),
                    object.reference.byte_count,
                )
                .is_some()
            || checkpoint_digest(&object.bytes) != object.reference.sha256
        {
            return Err(CheckpointError::InvalidObject);
        }
    }
    Ok(inventory)
}

fn resolve_state(
    state: &FileState,
    inventory: &BTreeMap<&str, u64>,
) -> Result<(), CheckpointError> {
    if let FileState::File { content, .. } = state {
        if inventory.get(content.sha256.as_str()) != Some(&content.byte_count) {
            return Err(CheckpointError::InvalidObject);
        }
    }
    Ok(())
}

// Counts the actual persisted JSON encoding, including numeric byte arrays,
// without allocating an unbounded encoded buffer. This writer has no I/O.
fn bounded_json(value: &(impl Serialize + ?Sized)) -> Result<(), CheckpointError> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|bytes| *bytes <= MAX_CHECKPOINT_MANIFEST_BYTES)
                .ok_or_else(|| std::io::Error::other("checkpoint JSON budget"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(0), value).map_err(|_| CheckpointError::Budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn digest(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }
    fn scope() -> CheckpointScope {
        CheckpointScope {
            owner_id: "owner-1".into(),
            workspace_id: "workspace-1".into(),
            environment_id: "host-1".into(),
            root_identity_digest: "1".repeat(64),
            path_semantics: PathSemantics::WindowsInsensitive,
            policy_digest: "2".repeat(64),
        }
    }
    fn scope_hash(scope: &CheckpointScope) -> String {
        let mut bytes = b"anima-filesystem-checkpoint-scope-v1\0".to_vec();
        bytes.extend(serde_json::to_vec(scope).unwrap());
        digest(&bytes)
    }
    fn metadata(byte: u8) -> MetadataSnapshot {
        MetadataSnapshot {
            required: vec![
                MetadataKind::BasicPermissions,
                MetadataKind::ModificationTime,
            ],
            attributes: vec![
                MetadataAttribute {
                    kind: MetadataKind::BasicPermissions,
                    encoding_version: 1,
                    bytes: vec![byte, 0, 255],
                },
                MetadataAttribute {
                    kind: MetadataKind::ModificationTime,
                    encoding_version: 1,
                    bytes: vec![1, 2, 3, 4],
                },
            ],
        }
    }
    fn object(bytes: &[u8]) -> CheckpointBlob {
        CheckpointBlob {
            reference: BlobReference {
                sha256: digest(bytes),
                byte_count: bytes.len() as u64,
            },
            bytes: bytes.to_vec(),
        }
    }
    fn file(object: &CheckpointBlob, metadata_byte: u8) -> FileState {
        FileState::File {
            content: object.reference.clone(),
            metadata: metadata(metadata_byte),
        }
    }
    fn checkpoint(mut entries: Vec<CheckpointEntry>) -> FilesystemCheckpoint {
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let scope = scope();
        FilesystemCheckpoint {
            version: 1,
            checkpoint_id: "checkpoint-1".into(),
            sequence: 1,
            scope_digest: scope_hash(&scope),
            scope,
            entries,
        }
    }
    fn entry(path: &str, state: FileState) -> CheckpointEntry {
        CheckpointEntry {
            path: path.into(),
            state,
        }
    }
    fn witness(path: &str, disposition: MutationDisposition, state: FileState) -> MutationWitness {
        MutationWitness {
            action_id: "action-1".into(),
            checkpoint_id: "checkpoint-1".into(),
            scope_digest: scope_hash(&scope()),
            path: path.into(),
            disposition,
            acknowledged_postimage: state,
        }
    }

    #[test]
    fn fp08_exact_bytes_metadata_and_absence_survive_manifest_reload() {
        let bytes = b"\xff\0original\r\nbytes\r\n";
        let original = object(bytes);
        assert_eq!(checkpoint_digest(bytes), original.reference.sha256);
        assert_ne!(
            original.reference.sha256,
            digest(b"\xff\0original\nbytes\n")
        );
        assert_eq!(checkpoint_scope_digest(&scope()), scope_hash(&scope()));
        let value = checkpoint(vec![
            entry("raw.bin", file(&original, 128)),
            entry("missing.bin", FileState::Absent),
        ]);
        let json = serde_json::to_vec(&value).unwrap();
        let decoded: FilesystemCheckpoint = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(
            serde_json::to_value(&decoded.entries[0].state).unwrap(),
            serde_json::json!({"kind":"absent"})
        );
        validate_filesystem_checkpoint(&decoded, &scope(), &[original.clone()]).unwrap();
        assert_eq!(original.bytes.as_slice(), bytes.as_slice());
        let empty = object(b"");
        assert_ne!(file(&empty, 128), FileState::Absent);
    }

    #[test]
    fn fp08_safe_restore_requires_proven_postimage_and_preserves_exact_preimage() {
        let before = object(b"original\r\n");
        let after = object(b"agent edit\n");
        let original_state = file(&before, 128);
        let post_state = file(&after, 0);
        let cp = checkpoint(vec![entry("src/file.txt", original_state.clone())]);
        let current = vec![
            entry("src/file.txt", post_state.clone()),
            entry("unrelated.txt", file(&after, 0)),
        ];
        let acknowledged = witness(
            "src/file.txt",
            MutationDisposition::Replaced,
            post_state.clone(),
        );
        let input = (cp.clone(), current.clone(), acknowledged.clone());
        let plan = prepare_safe_restore(
            &cp,
            &scope(),
            &[before, after],
            &current,
            &[acknowledged.clone()],
        )
        .unwrap();
        assert_eq!(
            plan.operations,
            vec![RestoreOperation {
                path: "src/file.txt".into(),
                expected_current: post_state,
                desired: original_state
            }]
        );
        assert!(plan.conflicts.is_empty());
        assert_eq!(plan.checkpoint_id, cp.checkpoint_id);
        assert_eq!(plan.scope_digest, cp.scope_digest);
        assert_eq!((cp, current, acknowledged), input);
    }

    #[test]
    fn fp08_created_disposition_is_required_before_any_delete() {
        let created = object(b"agent created");
        let current_state = file(&created, 0);
        let cp = checkpoint(vec![entry("new.txt", FileState::Absent)]);
        let current = vec![entry("new.txt", current_state.clone())];
        let creation = witness(
            "new.txt",
            MutationDisposition::Created,
            current_state.clone(),
        );
        let good =
            prepare_safe_restore(&cp, &scope(), &[created.clone()], &current, &[creation]).unwrap();
        assert_eq!(
            good.operations,
            vec![RestoreOperation {
                path: "new.txt".into(),
                expected_current: current_state.clone(),
                desired: FileState::Absent
            }]
        );
        let lie = witness("new.txt", MutationDisposition::Replaced, current_state);
        let bad = prepare_safe_restore(&cp, &scope(), &[created], &current, &[lie]).unwrap();
        assert!(bad.operations.is_empty());
        assert_eq!(
            bad.conflicts,
            vec![RestoreConflict {
                path: "new.txt".into(),
                reason: RestoreConflictReason::UnprovenCreation
            }]
        );
    }

    #[test]
    fn fp08_dirty_unrelated_uncertain_and_missing_ledger_never_fall_back_to_full_restore() {
        let before = object(b"before");
        let agent = object(b"agent");
        let user = object(b"user");
        let cp = checkpoint(vec![entry("owned.txt", file(&before, 0))]);
        let witness = witness("owned.txt", MutationDisposition::Replaced, file(&agent, 0));
        let objects = [before, agent.clone(), user.clone()];
        for (current_state, witnesses, reason) in [
            (
                file(&user, 0),
                vec![witness.clone()],
                RestoreConflictReason::Dirty,
            ),
            (
                file(&agent, 1),
                vec![witness.clone()],
                RestoreConflictReason::Dirty,
            ),
            (
                FileState::Absent,
                vec![witness.clone()],
                RestoreConflictReason::Dirty,
            ),
            (
                file(&agent, 0),
                vec![],
                RestoreConflictReason::MissingEvidence,
            ),
            (
                file(&agent, 0),
                vec![MutationWitness {
                    disposition: MutationDisposition::Uncertain,
                    ..witness.clone()
                }],
                RestoreConflictReason::Uncertain,
            ),
        ] {
            let current = vec![
                entry("owned.txt", current_state),
                entry("unrelated.txt", file(&user, 0)),
            ];
            let plan = prepare_safe_restore(&cp, &scope(), &objects, &current, &witnesses).unwrap();
            assert!(plan.operations.is_empty());
            assert_eq!(
                plan.conflicts,
                vec![RestoreConflict {
                    path: "owned.txt".into(),
                    reason
                }]
            );
        }
        let missing = prepare_safe_restore(&cp, &scope(), &objects, &[], &[witness]).unwrap();
        assert_eq!(
            missing.conflicts[0].reason,
            RestoreConflictReason::IncompleteObservation
        );
    }

    #[test]
    fn fp08_manifest_refuses_paths_aliases_scope_lies_and_invalid_objects_without_mutation() {
        let blob = object(b"exact");
        let good = checkpoint(vec![entry("src/file.bin", file(&blob, 0))]);
        let pristine = good.clone();
        for path in [
            "",
            ".",
            "../file",
            "a/../file",
            "/absolute",
            "C:/file",
            "a\\file",
            "a//file",
            "a/./file",
            "a:stream",
            "a/line\n",
            "a/file.",
            "a/file ",
            "a/CON",
            "a/NUL.txt",
        ] {
            let mut invalid = good.clone();
            invalid.entries[0].path = path.into();
            assert!(
                validate_filesystem_checkpoint(&invalid, &scope(), &[blob.clone()]).is_err(),
                "accepted {path:?}"
            );
        }
        let mut aliases = good.clone();
        aliases.entries.push(entry("SRC/FILE.BIN", file(&blob, 0)));
        assert!(validate_filesystem_checkpoint(&aliases, &scope(), &[blob.clone()]).is_err());
        let mut bad_scope = good.clone();
        bad_scope.scope.owner_id = "other-owner".into();
        bad_scope.scope_digest = scope_hash(&bad_scope.scope);
        assert!(validate_filesystem_checkpoint(&bad_scope, &scope(), &[blob.clone()]).is_err());
        let mut bad_hash = good.clone();
        bad_hash.scope_digest = "0".repeat(64);
        assert!(validate_filesystem_checkpoint(&bad_hash, &scope(), &[blob.clone()]).is_err());
        let mut corrupt = blob.clone();
        corrupt.bytes[0] ^= 1;
        assert!(validate_filesystem_checkpoint(&good, &scope(), &[corrupt]).is_err());
        assert!(validate_filesystem_checkpoint(&good, &scope(), &[]).is_err());
        let literal_percent = checkpoint(vec![entry("dir/100%literal%66ile.bin", file(&blob, 0))]);
        validate_filesystem_checkpoint(&literal_percent, &scope(), &[blob.clone()]).unwrap();
        let mut posix_scope = scope();
        posix_scope.path_semantics = PathSemantics::PosixSensitive;
        assert_ne!(
            scope_hash(&posix_scope),
            scope_hash(&scope()),
            "path interpretation is part of the scope pin"
        );
        let mut posix = checkpoint(vec![
            entry("SRC/FILE.BIN", file(&blob, 0)),
            entry("src/file.bin", file(&blob, 0)),
        ]);
        posix.scope = posix_scope.clone();
        posix.scope_digest = scope_hash(&posix_scope);
        validate_filesystem_checkpoint(&posix, &posix_scope, &[blob.clone()]).unwrap();
        assert!(
            validate_filesystem_checkpoint(&posix, &scope(), &[blob.clone()]).is_err(),
            "a case-sensitive scope cannot impersonate the Windows scope"
        );
        let mut missing_metadata = good.clone();
        if let FileState::File { metadata, .. } = &mut missing_metadata.entries[0].state {
            metadata.attributes.pop();
        }
        assert!(validate_filesystem_checkpoint(&missing_metadata, &scope(), &[blob]).is_err());
        assert_eq!(good, pristine);
        let mut unknown = serde_json::to_value(&good).unwrap();
        unknown["force"] = serde_json::json!(true);
        assert!(serde_json::from_value::<FilesystemCheckpoint>(unknown).is_err());
        let mut future = serde_json::to_value(&good).unwrap();
        future["version"] = serde_json::json!(2);
        assert!(
            serde_json::from_value::<FilesystemCheckpoint>(future).is_err(),
            "persisted future versions must fail closed"
        );
        assert!(serde_json::from_value::<FileState>(
            serde_json::json!({"kind":"absent","bytes":[]})
        )
        .is_err());
    }

    #[test]
    fn fp08_manifest_bounds_paths_depth_metadata_and_capture_bytes() {
        let empty = object(b"");
        let exact_path = format!("{}.bin", "x".repeat(MAX_CHECKPOINT_PATH_BYTES - 4));
        let cp = checkpoint(vec![entry(&exact_path, file(&empty, 0))]);
        validate_filesystem_checkpoint(&cp, &scope(), &[empty.clone()]).unwrap();
        let mut too_long = cp.clone();
        too_long.entries[0].path.push('x');
        assert!(validate_filesystem_checkpoint(&too_long, &scope(), &[empty.clone()]).is_err());
        let deep = checkpoint(vec![entry(
            &format!("{}x", "d/".repeat(MAX_CHECKPOINT_DEPTH)),
            file(&empty, 0),
        )]);
        assert!(validate_filesystem_checkpoint(&deep, &scope(), &[empty.clone()]).is_err());
        let mut metadata_big = checkpoint(vec![entry("file", file(&empty, 0))]);
        if let FileState::File { metadata, .. } = &mut metadata_big.entries[0].state {
            metadata.attributes[0].bytes = vec![0; MAX_CHECKPOINT_METADATA_BYTES + 1];
        }
        assert!(validate_filesystem_checkpoint(&metadata_big, &scope(), &[empty.clone()]).is_err());
        let too_many = checkpoint(
            (0..=MAX_CHECKPOINT_ENTRIES)
                .map(|i| entry(&format!("f{i:06}"), FileState::Absent))
                .collect(),
        );
        assert!(validate_filesystem_checkpoint(&too_many, &scope(), &[]).is_err());
        let oversized = BlobReference {
            sha256: "a".repeat(64),
            byte_count: MAX_CHECKPOINT_STREAM_BYTES + 1,
        };
        let too_large = checkpoint(vec![entry(
            "file",
            FileState::File {
                content: oversized,
                metadata: metadata(0),
            },
        )]);
        assert!(validate_filesystem_checkpoint(&too_large, &scope(), &[]).is_err());
        let capture_overflow = checkpoint(
            (1..=7)
                .map(|index| {
                    entry(
                        &format!("file-{index}"),
                        FileState::File {
                            content: BlobReference {
                                sha256: format!("{index:064x}"),
                                byte_count: MAX_CHECKPOINT_STREAM_BYTES,
                            },
                            metadata: metadata(0),
                        },
                    )
                })
                .collect(),
        );
        assert_eq!(
            validate_filesystem_checkpoint(&capture_overflow, &scope(), &[]),
            Err(CheckpointError::Budget),
            "aggregate declared bytes must be bounded before reading object payloads"
        );
        let mut manifest_overflow = checkpoint(
            (0..60)
                .map(|index| entry(&format!("file-{index:03}"), file(&empty, 0)))
                .collect(),
        );
        for value in &mut manifest_overflow.entries {
            if let FileState::File { metadata, .. } = &mut value.state {
                metadata.attributes[0].bytes = vec![255; 40_000];
            }
        }
        assert_eq!(
            validate_filesystem_checkpoint(&manifest_overflow, &scope(), &[empty]),
            Err(CheckpointError::Budget)
        );
    }

    #[test]
    fn fp08_restore_witnesses_remain_scope_and_checkpoint_bound() {
        let before = object(b"before");
        let after = object(b"after");
        let cp = checkpoint(vec![entry("file", file(&before, 0))]);
        let current = vec![entry("file", file(&after, 0))];
        let good = witness("file", MutationDisposition::Replaced, file(&after, 0));
        let valid = prepare_safe_restore(
            &cp,
            &scope(),
            &[before.clone(), after.clone()],
            &current,
            &[good.clone()],
        )
        .unwrap();
        assert_eq!(
            valid.operations,
            vec![RestoreOperation {
                path: "file".into(),
                expected_current: file(&after, 0),
                desired: file(&before, 0),
            }]
        );
        assert!(valid.conflicts.is_empty());
        for mutation in 0..4 {
            let mut bad = good.clone();
            match mutation {
                0 => bad.scope_digest = "0".repeat(64),
                1 => bad.checkpoint_id = "foreign-checkpoint".into(),
                2 => bad.path = "../file".into(),
                _ => bad.action_id.clear(),
            }
            assert!(prepare_safe_restore(
                &cp,
                &scope(),
                &[before.clone(), after.clone()],
                &current,
                &[bad]
            )
            .is_err());
        }
        assert!(prepare_safe_restore(
            &cp,
            &scope(),
            &[before, after],
            &current,
            &[good.clone(), good]
        )
        .is_err());
    }

    #[test]
    fn fp08_manifest_rejects_file_ancestor_topology_including_case_aliases() {
        let blob = object(b"file");
        let ordinary = checkpoint(vec![
            entry("a", file(&blob, 0)),
            entry("a-sibling/file", file(&blob, 0)),
        ]);
        validate_filesystem_checkpoint(&ordinary, &scope(), &[blob.clone()]).unwrap();
        for (ancestor, descendant) in [("a", "a/b"), ("A", "a/b"), ("a/b", "A/B/c")] {
            let impossible = checkpoint(vec![
                entry(ancestor, file(&blob, 0)),
                entry(descendant, file(&blob, 0)),
            ]);
            assert_eq!(
                validate_filesystem_checkpoint(&impossible, &scope(), &[blob.clone()]),
                Err(CheckpointError::InvalidPath),
                "a regular file cannot also be an ancestor: {ancestor}, {descendant}"
            );
            let absent_ancestor = checkpoint(vec![
                entry(ancestor, FileState::Absent),
                entry(descendant, file(&blob, 0)),
            ]);
            assert_eq!(
                validate_filesystem_checkpoint(&absent_ancestor, &scope(), &[blob.clone()]),
                Err(CheckpointError::InvalidPath),
                "an absent ancestor cannot contain a present child"
            );
            let both_absent = checkpoint(vec![
                entry(ancestor, FileState::Absent),
                entry(descendant, FileState::Absent),
            ]);
            validate_filesystem_checkpoint(&both_absent, &scope(), &[]).unwrap();
        }
    }

    #[test]
    fn fp08_restore_rejects_impossible_current_observation_topology() {
        let blob = object(b"file");
        let cp = checkpoint(vec![entry("safe", FileState::Absent)]);
        let ordinary = vec![
            entry("a", file(&blob, 0)),
            entry("a-sibling/file", file(&blob, 0)),
            entry("safe", FileState::Absent),
        ];
        let valid = prepare_safe_restore(&cp, &scope(), &[blob.clone()], &ordinary, &[]).unwrap();
        assert!(valid.operations.is_empty() && valid.conflicts.is_empty());
        for (ancestor, descendant) in [("a", "a/b"), ("A", "a/b"), ("a/b", "A/B/c")] {
            let current = vec![
                entry(descendant, file(&blob, 0)),
                entry("safe", FileState::Absent),
                entry(ancestor, file(&blob, 0)),
            ];
            assert_eq!(
                prepare_safe_restore(&cp, &scope(), &[blob.clone()], &current, &[]),
                Err(CheckpointError::InvalidPath),
                "unrelated impossible observations must be rejected too"
            );
            let absent_ancestor = vec![
                entry(descendant, file(&blob, 0)),
                entry("safe", FileState::Absent),
                entry(ancestor, FileState::Absent),
            ];
            assert_eq!(
                prepare_safe_restore(&cp, &scope(), &[blob.clone()], &absent_ancestor, &[]),
                Err(CheckpointError::InvalidPath)
            );
            let both_absent = vec![
                entry(descendant, FileState::Absent),
                entry("safe", FileState::Absent),
                entry(ancestor, FileState::Absent),
            ];
            prepare_safe_restore(&cp, &scope(), &[], &both_absent, &[]).unwrap();
        }
    }

    #[test]
    fn fp08_windows_profile_refuses_unattested_unicode_comparison() {
        let blob = object(b"file");
        let ascii = checkpoint(vec![entry("Case/File.txt", file(&blob, 0))]);
        validate_filesystem_checkpoint(&ascii, &scope(), &[blob.clone()]).unwrap();
        // Unicode lowercase is not Windows filesystem identity: sigma/final
        // sigma can alias under Windows comparison while Rust keeps them apart.
        // This initial portable Windows profile must refuse non-ASCII paths.
        // Full Unicode support still requires a verified host comparison port.
        for paths in [vec!["σ.txt", "ς.txt"], vec!["café.txt"]] {
            let cp = checkpoint(
                paths
                    .iter()
                    .map(|path| entry(path, file(&blob, 0)))
                    .collect(),
            );
            assert_eq!(
                validate_filesystem_checkpoint(&cp, &scope(), &[blob.clone()]),
                Err(CheckpointError::InvalidPath)
            );
        }
        let mut posix_scope = scope();
        posix_scope.path_semantics = PathSemantics::PosixSensitive;
        let mut posix = checkpoint(vec![
            entry("σ.txt", file(&blob, 0)),
            entry("ς.txt", file(&blob, 0)),
            entry("café.txt", file(&blob, 0)),
        ]);
        posix.scope = posix_scope.clone();
        posix.scope_digest = scope_hash(&posix_scope);
        validate_filesystem_checkpoint(&posix, &posix_scope, &[blob.clone()]).unwrap();
        let current = [
            entry("σ.txt", file(&blob, 0)),
            entry("Case/File.txt", file(&blob, 0)),
        ];
        assert_eq!(
            prepare_safe_restore(&ascii, &scope(), &[blob], &current, &[]),
            Err(CheckpointError::InvalidPath)
        );
    }

    #[test]
    fn fp08_restore_bounds_aggregate_encoded_witness_input_even_without_operations() {
        let blob = object(b"");
        let entries: Vec<_> = (0..60)
            .map(|index| entry(&format!("file-{index:03}"), FileState::Absent))
            .collect();
        let cp = checkpoint(entries.clone());
        let small = witness(
            &entries[0].path,
            MutationDisposition::Uncertain,
            file(&blob, 0),
        );
        let valid =
            prepare_safe_restore(&cp, &scope(), &[blob.clone()], &entries, &[small]).unwrap();
        assert!(valid.operations.is_empty() && valid.conflicts.is_empty());
        let witnesses: Vec<_> = entries
            .iter()
            .map(|entry| {
                let mut postimage = file(&blob, 0);
                if let FileState::File { metadata, .. } = &mut postimage {
                    metadata.attributes[0].bytes = vec![255; 40_000];
                }
                witness(&entry.path, MutationDisposition::Uncertain, postimage)
            })
            .collect();
        assert_eq!(
            bounded_json(&witnesses),
            Err(CheckpointError::Budget),
            "the fixture exceeds the encoded JSON budget while each metadata object fits"
        );
        let before = witnesses.clone();
        assert_eq!(
            prepare_safe_restore(&cp, &scope(), &[blob], &entries, &witnesses),
            Err(CheckpointError::Budget),
            "a small no-op proposal cannot excuse oversized witness input"
        );
        assert_eq!(witnesses, before);
    }

    fn retention(
        id: &str,
        sequence: u64,
        digest: &str,
        pins: Vec<RetentionPin>,
    ) -> RetentionRecord {
        RetentionRecord {
            checkpoint_id: id.into(),
            sequence,
            recorded_at_ms: sequence * 100,
            manifest_bytes: 10,
            object_digests: vec![digest.into()],
            pins,
        }
    }
    fn policy() -> RetentionPolicy {
        RetentionPolicy {
            max_groups: 2,
            max_bytes: 500,
            retention_ms: 10_000,
            observed_now_ms: 1_000,
            last_observed_now_ms: 1_000,
            required_reserve_bytes: 0,
        }
    }

    #[test]
    fn fp08_retention_pins_incomplete_safety_and_uncertain_generations_and_shared_objects() {
        let shared = "a".repeat(64);
        let old_only = "b".repeat(64);
        let protected = "c".repeat(64);
        let objects = vec![
            BlobReference {
                sha256: shared.clone(),
                byte_count: 100,
            },
            BlobReference {
                sha256: old_only.clone(),
                byte_count: 50,
            },
            BlobReference {
                sha256: protected.clone(),
                byte_count: 25,
            },
        ];
        for pin in [
            RetentionPin::ActiveTurn,
            RetentionPin::InFlightEffect,
            RetentionPin::RestoreSelected,
            RetentionPin::Safety,
            RetentionPin::RecoveryRequired,
            RetentionPin::Uncertain,
        ] {
            let records = vec![
                retention("old-unpinned", 1, &old_only, vec![]),
                retention("pinned", 2, &shared, vec![pin]),
                retention("latest", 3, &shared, vec![]),
            ];
            let before = records.clone();
            let plan =
                plan_checkpoint_retention(&records, &objects, &[protected.clone()], &policy())
                    .unwrap();
            assert_eq!(plan.prune_checkpoint_ids, ["old-unpinned"]);
            assert_eq!(plan.retained_checkpoint_ids, ["pinned", "latest"]);
            assert_eq!(plan.prune_object_digests, [old_only.clone()]);
            assert_eq!(
                plan.used_bytes_after, 145,
                "shared blob counted once; protected recovery object retained"
            );
            assert!(plan.can_reserve);
            assert_eq!(records, before);
        }
    }

    #[test]
    fn fp08_retention_reports_pinned_quota_failure_and_clamps_clock_rollback() {
        let hash = "a".repeat(64);
        let objects = [BlobReference {
            sha256: hash.clone(),
            byte_count: 100,
        }];
        let records = [
            retention("safety", 1, &hash, vec![RetentionPin::Safety]),
            retention("uncertain", 2, &hash, vec![RetentionPin::Uncertain]),
        ];
        let mut limited = policy();
        limited.max_groups = 1;
        limited.max_bytes = 110;
        limited.required_reserve_bytes = 1;
        limited.observed_now_ms = 1;
        let plan = plan_checkpoint_retention(&records, &objects, &[], &limited).unwrap();
        assert!(plan.prune_checkpoint_ids.is_empty());
        assert!(plan.prune_object_digests.is_empty());
        assert!(
            !plan.can_reserve,
            "pins cannot be deleted to pretend the store fits"
        );
        assert_eq!(plan.used_bytes_after, 120);
        assert_eq!(plan.observed_now_ms, 1_000);
        let mut invalid = records.to_vec();
        invalid[1].checkpoint_id = invalid[0].checkpoint_id.clone();
        assert!(plan_checkpoint_retention(&invalid, &objects, &[], &limited).is_err());
    }
}
