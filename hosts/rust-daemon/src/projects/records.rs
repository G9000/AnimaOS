//! Bounded project records and content-free admission evidence.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const MAX_PROJECTS: usize = 100;
pub(crate) const MAX_CREATION_RECEIPTS: usize = 10_000;
pub(crate) const MAX_MEMBERS: usize = 32;
pub(crate) const MAX_RESOURCES: usize = 24;
pub(crate) const MAX_CONTEXT_RESOURCES: usize = 12;
pub(crate) const MAX_EXCERPTS: usize = 6;
pub(crate) const MAX_INSTRUCTION_CHARS: usize = 4_000;
pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProjectError {
    InvalidRecord(&'static str),
    NotFound,
    NotMember,
    UnresolvedBinding,
    Archived,
    StaleRevision,
    ScopeChanged,
    AccessRevoked,
    AccessUnavailable,
    InstructionBudget,
    Cancelled,
    Deadline,
    ReceiptCapacity,
    DeletedCreation,
    Busy,
    Persistence(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRecord(message) => message,
            Self::NotFound | Self::NotMember => "project is unavailable for this agent",
            Self::UnresolvedBinding => "this chat's project needs repair before it can continue",
            Self::Archived => "restore the project or remove this chat before continuing",
            Self::StaleRevision => "project changed; refresh before saving",
            Self::ScopeChanged => {
                "project context changed; review the saved message before continuing"
            }
            Self::AccessRevoked => "project access changed; this reply has stopped",
            Self::AccessUnavailable => "project access is unavailable; reconnect before continuing",
            Self::InstructionBudget => {
                "shorten the project instructions or current message before continuing"
            }
            Self::Cancelled => "project context selection stopped",
            Self::Deadline => "project sources took too long to read; try again",
            Self::ReceiptCapacity => "project creation history is full; no new project was created",
            Self::DeletedCreation => {
                "the project created by this request was deleted; it will not be recreated"
            }
            Self::Busy => {
                "finish or stop the project's active and queued replies before this change"
            }
            Self::Persistence(message) => {
                return write!(f, "project could not be saved: {message}")
            }
        };
        f.write_str(message)
    }
}
impl std::error::Error for ProjectError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectStatus {
    Active,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectMemberGrant {
    pub(crate) agent_id: String,
    pub(crate) generation: u64,
    pub(crate) joined_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectMutationReplay {
    pub(crate) request_key: String,
    pub(crate) request_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectCreationReceipt {
    pub(crate) request_key: String,
    pub(crate) request_sha256: String,
    pub(crate) project_id: String,
    pub(crate) created_at_ms: u64,
    pub(crate) deleted_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectDonorBinding {
    pub(crate) agent_id: String,
    pub(crate) member_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectResourceRole {
    Reference,
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ProjectResourceTarget {
    WorkspaceFile {
        path: String,
    },
    ChatExcerpt {
        owner_agent_id: String,
        session_id: String,
        clear_generation: u64,
        message_ids: Vec<String>,
    },
    ChatResult {
        owner_agent_id: String,
        session_id: String,
        clear_generation: u64,
        result_id: String,
    },
    Workflow {
        owner_agent_id: String,
        workflow_id: String,
    },
    WorkflowTemplate {
        owner_agent_id: String,
        slug: String,
    },
}

impl ProjectResourceTarget {
    pub(crate) fn owner_agent_id(&self) -> Option<&str> {
        match self {
            Self::WorkspaceFile { .. } => None,
            Self::ChatExcerpt { owner_agent_id, .. }
            | Self::ChatResult { owner_agent_id, .. }
            | Self::Workflow { owner_agent_id, .. }
            | Self::WorkflowTemplate { owner_agent_id, .. } => Some(owner_agent_id),
        }
    }

    pub(crate) fn session_id(&self) -> Option<&str> {
        match self {
            Self::ChatExcerpt { session_id, .. } | Self::ChatResult { session_id, .. } => {
                Some(session_id)
            }
            _ => None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ProjectError> {
        if self.owner_agent_id().is_some_and(|id| !valid_id(id)) {
            return Err(ProjectError::InvalidRecord(
                "project source agent is invalid",
            ));
        }
        match self {
            Self::WorkspaceFile { path } if !valid_relative_path(path) => Err(
                ProjectError::InvalidRecord("select a regular workspace file without traversal"),
            ),
            Self::ChatExcerpt {
                session_id,
                clear_generation,
                message_ids,
                ..
            } => {
                if !valid_id(session_id)
                    || *clear_generation > MAX_SAFE_INTEGER
                    || message_ids.is_empty()
                    || message_ids.len() > 4
                    || message_ids.iter().any(|id| !valid_message_id(id))
                    || message_ids.iter().collect::<BTreeSet<_>>().len() != message_ids.len()
                {
                    Err(ProjectError::InvalidRecord(
                        "select one to four visible messages from one project chat",
                    ))
                } else {
                    Ok(())
                }
            }
            Self::ChatResult {
                session_id,
                clear_generation,
                result_id,
                ..
            } if !valid_id(session_id)
                || *clear_generation > MAX_SAFE_INTEGER
                || !valid_message_id(result_id) =>
            {
                Err(ProjectError::InvalidRecord(
                    "project output identity is invalid",
                ))
            }
            Self::Workflow { workflow_id, .. } if !valid_message_id(workflow_id) => Err(
                ProjectError::InvalidRecord("project workflow identity is invalid"),
            ),
            Self::WorkflowTemplate { slug, .. } if !valid_id(slug) => Err(
                ProjectError::InvalidRecord("project workflow template identity is invalid"),
            ),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectResourceRecord {
    pub(crate) id: String,
    pub(crate) target: ProjectResourceTarget,
    pub(crate) role: ProjectResourceRole,
    pub(crate) label: Option<String>,
    pub(crate) use_in_context: bool,
    pub(crate) created_at_ms: u64,
    pub(crate) add_replay: ProjectMutationReplay,
    pub(crate) donor: Option<ProjectDonorBinding>,
    pub(crate) source_project_binding_revision: Option<u64>,
    pub(crate) workspace_key: Option<String>,
}

impl ProjectResourceRecord {
    pub(crate) fn validate(&self) -> Result<(), ProjectError> {
        self.target.validate()?;
        if !valid_prefixed_uuid(&self.id, "resource_")
            || !valid_request_key(&self.add_replay.request_key)
            || !valid_hash(&self.add_replay.request_sha256)
            || self
                .label
                .as_ref()
                .is_some_and(|label| clean_title(label).as_ref() != Ok(label))
        {
            return Err(ProjectError::InvalidRecord(
                "saved project source is invalid",
            ));
        }
        let valid_shape = match &self.target {
            ProjectResourceTarget::WorkspaceFile { .. } => {
                self.donor.is_none()
                    && self.source_project_binding_revision.is_none()
                    && self.workspace_key.as_deref().is_some_and(valid_hash)
            }
            ProjectResourceTarget::ChatExcerpt { .. }
            | ProjectResourceTarget::ChatResult { .. } => {
                self.workspace_key.is_none()
                    && self
                        .source_project_binding_revision
                        .is_some_and(|v| v > 0 && v <= MAX_SAFE_INTEGER)
            }
            ProjectResourceTarget::Workflow { .. }
            | ProjectResourceTarget::WorkflowTemplate { .. } => {
                !self.use_in_context
                    && self.workspace_key.is_none()
                    && self.source_project_binding_revision.is_none()
            }
        };
        if !valid_shape
            || self.target.owner_agent_id().is_some_and(|owner| {
                !self.donor.as_ref().is_some_and(|donor| {
                    donor.agent_id == owner
                        && donor.member_generation > 0
                        && donor.member_generation <= MAX_SAFE_INTEGER
                })
            })
        {
            return Err(ProjectError::InvalidRecord(
                "saved project source binding is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectRecord {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) status: ProjectStatus,
    pub(crate) revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) members: Vec<ProjectMemberGrant>,
    pub(crate) instructions: String,
    pub(crate) resources: Vec<ProjectResourceRecord>,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
}

impl ProjectRecord {
    pub(crate) fn member(&self, agent: &str) -> Option<&ProjectMemberGrant> {
        self.members.iter().find(|member| member.agent_id == agent)
    }

    pub(crate) fn validate(&self) -> Result<(), ProjectError> {
        if !valid_prefixed_uuid(&self.id, "project_")
            || clean_title(&self.title).as_ref() != Ok(&self.title)
            || self.revision == 0
            || self.revision > MAX_SAFE_INTEGER
            || self.access_epoch == 0
            || self.access_epoch > self.revision
            || self.members.len() > MAX_MEMBERS
            || self.resources.len() > MAX_RESOURCES
            || !valid_instructions(&self.instructions)
            || self.updated_at_ms < self.created_at_ms
        {
            return Err(ProjectError::InvalidRecord("saved project is invalid"));
        }
        let mut previous: Option<&str> = None;
        for member in &self.members {
            if !valid_id(&member.agent_id)
                || member.generation == 0
                || member.generation > self.access_epoch
                || previous.is_some_and(|old| old >= member.agent_id.as_str())
            {
                return Err(ProjectError::InvalidRecord(
                    "saved project membership is invalid",
                ));
            }
            previous = Some(&member.agent_id);
        }
        let mut ids = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut request_keys = BTreeSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if !ids.insert(&resource.id)
                || !targets.insert(digest(&resource.target)?)
                || !request_keys.insert(&resource.add_replay.request_key)
            {
                return Err(ProjectError::InvalidRecord(
                    "saved project sources contain duplicate identities",
                ));
            }
        }
        if self
            .resources
            .iter()
            .filter(|row| row.use_in_context)
            .count()
            > MAX_CONTEXT_RESOURCES
            || self
                .resources
                .iter()
                .filter(|row| matches!(row.target, ProjectResourceTarget::ChatExcerpt { .. }))
                .count()
                > MAX_EXCERPTS
        {
            return Err(ProjectError::InvalidRecord(
                "project has too many shared context sources",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueuedProjectWitness {
    pub(crate) session_binding_revision: u64,
    pub(crate) project: Option<RunProjectBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunProjectBinding {
    pub(crate) project_id: String,
    pub(crate) project_revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) member_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ProjectSourceWitness {
    WorkspaceFile {
        workspace_key: String,
        path: String,
        capture_sha256: String,
        complete_source: bool,
    },
    ChatExcerpt {
        donor: ProjectDonorBinding,
        session_id: String,
        clear_generation: u64,
        project_binding_revision: u64,
        source_state_sha256: String,
        message_ids: Vec<String>,
    },
    ChatResult {
        donor: ProjectDonorBinding,
        session_id: String,
        clear_generation: u64,
        project_binding_revision: u64,
        result_id: String,
        result_revision: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectSourceState {
    Ready,
    Missing,
    NotInProject,
    AccessRemoved,
    SourceChanged,
    WorkspaceChanged,
    Excluded,
    UnsafeContent,
    Unsupported,
    Unavailable,
    BudgetOmitted,
    AlreadyInCurrentChat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSourceAdmission {
    pub(crate) resource_id: String,
    pub(crate) state: ProjectSourceState,
    pub(crate) included_chars: u32,
    pub(crate) shortened: bool,
    pub(crate) included_sha256: Option<String>,
    pub(crate) witness: Option<ProjectSourceWitness>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectContextManifest {
    pub(crate) version: u32,
    pub(crate) run_id: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) session_binding_revision: u64,
    pub(crate) project: RunProjectBinding,
    pub(crate) captured_at_ms: u64,
    pub(crate) instructions_sha256: String,
    pub(crate) rendered_sha256: String,
    pub(crate) estimated_tokens: u64,
    pub(crate) sources: Vec<ProjectSourceAdmission>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectInvalidation {
    pub(crate) project_id: String,
    pub(crate) revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) deleted: bool,
    pub(crate) membership_changed: bool,
    pub(crate) destinations: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSessionBindingPatch {
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) created_at_ms: u64,
    pub(crate) expected_binding_revision: u64,
    pub(crate) project_id: Option<String>,
    pub(crate) project_binding_revision: u64,
}

pub(crate) fn valid_id(value: &str) -> bool {
    crate::sessions::is_valid_session_id(value)
}
pub(crate) fn valid_message_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
pub(crate) fn valid_prefixed_uuid(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id))
}
pub(crate) fn valid_request_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
pub(crate) fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.contains(['\\', ':'])
        && !value.chars().any(char::is_control)
        && value.split('/').all(|part| {
            !part.is_empty()
                && !matches!(part, "." | "..")
                && !crate::tools::generated_documents::excluded_component(part)
        })
}
pub(crate) fn clean_title(value: &str) -> Result<String, ProjectError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 120 || value.chars().any(char::is_control) {
        return Err(ProjectError::InvalidRecord(
            "use a project name between 1 and 120 characters",
        ));
    }
    Ok(value.to_owned())
}
pub(crate) fn valid_instructions(value: &str) -> bool {
    value.len() <= 16 * 1024
        && value.chars().count() <= MAX_INSTRUCTION_CHARS
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
pub(crate) fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String, ProjectError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| ProjectError::InvalidRecord("project request cannot be encoded"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}
pub(crate) fn text_digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
pub(crate) fn next_revision(value: u64) -> Result<u64, ProjectError> {
    value
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .ok_or(ProjectError::InvalidRecord(
            "project revision limit reached",
        ))
}
