//! Owner-controlled project membership and explicitly selected shared context.

mod access;
mod agent_removal;
mod context;
mod records;
mod registry;
mod restore;
mod service;
mod source_changes;

#[cfg(test)]
mod tests;

pub(crate) use access::{bind_model_adapter, ProjectAccessLease, ProjectAccessRegistry};
pub(crate) use agent_removal::ProjectAgentRemovalCandidate;
pub(crate) use context::{
    capture_context_scope, capture_run_witness, context_change_summary, current_authorized_witness,
    load_context, member_read_lease, preview_context, preview_source, recheck_context,
    validate_active_lease, validate_project_chat_selection, validate_queued_witness,
    AdmittedProjectContext, CapturedProjectScope, LoadedProjectContext, ProjectChangeReason,
    ProjectContextChange, ProjectContextReport, ProjectRenderBudget,
};
pub(crate) use records::valid_relative_path;
pub(crate) use records::{
    ProjectContextManifest, ProjectCreationReceipt, ProjectDonorBinding, ProjectError,
    ProjectInvalidation, ProjectMemberGrant, ProjectRecord, ProjectResourceRecord,
    ProjectResourceRole, ProjectResourceTarget, ProjectSessionBindingPatch, ProjectSourceAdmission,
    ProjectSourceState, ProjectSourceWitness, ProjectStatus, QueuedProjectWitness,
    RunProjectBinding,
};
pub(crate) use registry::ProjectRegistry;
pub(crate) use registry::{ProjectCreateRequest, ProjectPatchRequest};
pub(crate) use restore::{
    restore_projects, validate_run_project_fields, validate_session_project_fields,
    RestoredProjects,
};
pub(crate) use service::publish_project_invalidation;
pub(crate) use service::{AddResourceRequest, MoveChatRequest, ProjectMutation, ProjectService};
pub(crate) use source_changes::ProjectSourceChange;
