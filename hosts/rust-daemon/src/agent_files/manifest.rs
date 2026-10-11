//! Content-free owner visibility. Never add source bodies or absolute paths here.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

macro_rules! closed_enum {
    ($name:ident {$($variant:ident),* $(,)?}) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
        #[serde(rename_all = "snake_case")]
        pub(crate) enum $name { $($variant),* }
    };
}
closed_enum!(Availability {
    Available,
    UnavailableWorkspace,
    StaleScope
});
closed_enum!(IdentitySource {
    Legacy,
    WorkspaceSoul,
    ProfileSoul
});
closed_enum!(ProjectSourceKind {
    Anima,
    Agents,
    Claude,
    Cursor
});
closed_enum!(AgentFileKind {
    Soul,
    Anima,
    Agents,
    Claude,
    Cursor
});
closed_enum!(AgentFileState {
    Loaded,
    Truncated,
    Shadowed,
    Duplicate,
    Empty,
    Missing,
    Unreadable,
    InvalidUtf8,
    Linked,
    NonRegular,
    Oversized,
    Blocked,
    BudgetExcluded,
    TimedOut,
    Cancelled
});
closed_enum!(ReasonCode {
    ReadFailed,
    InvalidScope,
    FileTooLarge,
    ScanLimit,
    ThreatDetected,
    TotalReadLimit,
    CandidateLimit,
    RenderBudget,
    Deadline,
    Cancelled
});

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentFileManifestEntry {
    pub relative_path: String,
    pub kind: AgentFileKind,
    pub state: AgentFileState,
    pub raw_byte_count: u64,
    pub admitted_char_count: u64,
    pub digest: Option<String>,
    pub truncated: bool,
    pub threat_ids: Vec<String>,
    pub reason_code: Option<ReasonCode>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentFilesManifest {
    pub availability: Availability,
    pub captured_at_ms: u64,
    pub snapshot_digest: String,
    pub identity_source: IdentitySource,
    pub project_source_kind: Option<ProjectSourceKind>,
    pub estimated_tokens: u64,
    pub entries: Vec<AgentFileManifestEntry>,
}
impl AgentFileManifestEntry {
    pub(super) fn new(relative_path: String, kind: AgentFileKind) -> Self {
        Self {
            relative_path,
            kind,
            state: AgentFileState::Missing,
            raw_byte_count: 0,
            admitted_char_count: 0,
            digest: None,
            truncated: false,
            threat_ids: vec![],
            reason_code: None,
        }
    }
}
