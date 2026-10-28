//! Opt-in host capture/store and native mutation boundary. ContentBasicV1 makes
//! no ACL, stream, timestamp or snapshot claim about captured workspace files.
use anima_core::filesystem_checkpoints::ContentBasicPermissionsV1 as BasicMode;
use anima_core::filesystem_checkpoints::{BlobReference, CheckpointScope};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod provision;
mod restore;
mod service;
mod store;
mod workspace;
pub(crate) use service::{
    NativeCheckpointService, NativeCheckpointSummary, NativeMutationReceipt,
    NativePublicationEvidence, NATIVE_RECOVERY_REQUIRED,
};
use store::CheckpointStore;
#[cfg(all(test, windows))]
mod service_tests;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaptureProfile {
    ContentBasicV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CaptureState {
    Absent {},
    File {
        content: BlobReference,
        basic_mode: BasicMode,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedEntry {
    path: String,
    state: CaptureState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CaptureSelection {
    Workspace {},
    Paths { paths: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureLimits {
    file_bytes: u64,
    content_bytes: u64,
    entries: usize,
    manifest_bytes: usize,
    store_bytes: u64,
    history_records: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            file_bytes: 10 * 1024 * 1024,
            content_bytes: 64 * 1024 * 1024,
            entries: 50_000,
            manifest_bytes: 8 * 1024 * 1024,
            store_bytes: 500 * 1024 * 1024,
            history_records: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturePolicy {
    profile: CaptureProfile,
    excluded_prefixes: Vec<String>,
    limits: CaptureLimits,
}

struct CaptureConfig {
    workspace_root: PathBuf,
    store_root: PathBuf,
    /// Supplied by the authentic host workspace binding, never model arguments.
    scope: CheckpointScope,
    policy: CapturePolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureRecord {
    version: u32,
    checkpoint_id: String,
    sequence: u64,
    scope_digest: String,
    profile: CaptureProfile,
    selection: CaptureSelection,
    entries: Vec<CapturedEntry>,
    /// Relative excluded roots actually encountered; no contents are inspected.
    excluded: Vec<String>,
    content_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CaptureSummary {
    checkpoint_id: String,
    sequence: u64,
    entry_count: usize,
    content_bytes: u64,
    profile: CaptureProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffKind {
    Added,
    Modified,
    Deleted,
    ModeChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CaptureDiff {
    path: String,
    kind: DiffKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureError {
    Unavailable,
    InvalidScope,
    UnsafePath,
    Busy,
    Linked,
    NonRegular,
    Unsupported,
    Budget,
    Corrupt,
    Interrupted,
    RecoveryRequired,
    InsecureStore,
    Dirty,
    InvalidWitness,
}

/// Internal authentic native journal/index context, never a deserialized tool
/// argument. Tests supply synthetic native identities with real TempDir I/O.
struct BasicRestoreAuthority {
    expected_scope: CheckpointScope,
    lineage: anima_core::filesystem_checkpoints::MutationLineage,
    expected_head: anima_core::filesystem_checkpoints::ExpectedLineageHead,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasicRestorePlan {
    plan_id: String,
    checkpoint_id: String,
    scope_digest: String,
    lineage_head_digest: String,
    selected_paths: Vec<String>,
    proposal: anima_core::filesystem_checkpoints::RestoreProposal,
    plan_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BasicRestoreResult {
    restored_paths: Vec<String>,
    safety_checkpoint_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BasicRecoveryStatus {
    Clean,
    RecoveryRequired {
        plan_id: String,
        safety_checkpoint_id: Option<String>,
    },
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestRestoreFault {
    BeforeSafetySync,
    AfterPreparedBeforeAck,
    AfterFirstTargetWrite,
    ModeMetadataReadFails,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestRestorePhase {
    BeforeTargetLease,
    BeforeAbsentCreate,
    BeforeCreatedMove,
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum TestPublishFault {
    BeforeIndexCommit,
    AfterIndexCommitBeforeAck,
}
