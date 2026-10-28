//! Boot validation restores durable membership, never in-process access leases.

use std::collections::BTreeSet;

use super::records::{
    valid_hash, valid_id, valid_message_id, valid_prefixed_uuid, ProjectContextManifest,
    ProjectCreationReceipt, ProjectError, ProjectRecord, ProjectSourceState, ProjectSourceWitness,
    MAX_CONTEXT_RESOURCES, MAX_SAFE_INTEGER,
};
use super::{ProjectAccessRegistry, ProjectRegistry};
use crate::state::DaemonState;

pub(crate) struct RestoredProjects {
    pub(crate) registry: ProjectRegistry,
    pub(crate) access: ProjectAccessRegistry,
}

pub(crate) fn restore_projects(
    records: Vec<ProjectRecord>,
    creation_receipts: Vec<ProjectCreationReceipt>,
    state_with_agents_and_sessions: &DaemonState,
) -> Result<RestoredProjects, ProjectError> {
    let registry = ProjectRegistry::from_snapshot(records, creation_receipts)?;
    for project in registry.records() {
        for member in &project.members {
            if !state_with_agents_and_sessions
                .agents
                .get(&member.agent_id)
                .is_some_and(|agent| !crate::agent_runs::is_helper_config(agent.config()))
            {
                return Err(ProjectError::InvalidRecord(
                    "saved project member is not an existing independent agent",
                ));
            }
        }
    }
    for session in state_with_agents_and_sessions.sessions.records() {
        validate_session_project_fields(session)?;
        // Missing project/member stays unresolved. Admission, never restore, decides access.
    }
    let access = ProjectAccessRegistry::default();
    access.publish_records(&registry)?;
    Ok(RestoredProjects { registry, access })
}

/// Structural validation shared by snapshot preflight and project restoration.
/// It does not resolve membership or grant access to a saved project binding.
pub(crate) fn validate_session_project_fields(
    session: &crate::sessions::SessionRecord,
) -> Result<(), ProjectError> {
    if session.project_binding_revision > MAX_SAFE_INTEGER
        || session
            .project_id
            .as_deref()
            .is_some_and(|id| !valid_prefixed_uuid(id, "project_"))
        || (session.project_id.is_some() && session.project_binding_revision == 0)
    {
        return Err(ProjectError::InvalidRecord(
            "saved chat project binding is invalid",
        ));
    }
    Ok(())
}

pub(crate) fn validate_run_project_fields(
    run: &crate::runs::RunRecord,
) -> Result<(), ProjectError> {
    if let Some(witness) = &run.project_witness {
        if witness.session_binding_revision > MAX_SAFE_INTEGER {
            return Err(ProjectError::InvalidRecord(
                "saved project turn binding is invalid",
            ));
        }
        if let Some(binding) = &witness.project {
            if !valid_prefixed_uuid(&binding.project_id, "project_")
                || binding.project_revision == 0
                || binding.project_revision > MAX_SAFE_INTEGER
                || binding.access_epoch == 0
                || binding.access_epoch > binding.project_revision
                || binding.member_generation == 0
                || binding.member_generation > binding.access_epoch
                || witness.session_binding_revision == 0
            {
                return Err(ProjectError::InvalidRecord(
                    "saved project turn membership is invalid",
                ));
            }
        }
    }
    if let Some(manifest) = &run.project_context {
        let witness = run
            .project_witness
            .as_ref()
            .ok_or(ProjectError::InvalidRecord(
                "saved project context has no turn witness",
            ))?;
        if manifest.version != 1
            || manifest.run_id != run.id
            || manifest.agent_id != run.agent_id
            || manifest.session_id != run.session_id
            || manifest.session_binding_revision != witness.session_binding_revision
            || witness.project.as_ref() != Some(&manifest.project)
            || !valid_hash(&manifest.instructions_sha256)
            || !valid_hash(&manifest.rendered_sha256)
            || manifest.estimated_tokens > 3_000
            || manifest.sources.len() > MAX_CONTEXT_RESOURCES
            || manifest.captured_at_ms > MAX_SAFE_INTEGER
        {
            return Err(ProjectError::InvalidRecord(
                "saved project context does not match its turn",
            ));
        }
        validate_sources(manifest)?;
    }
    Ok(())
}

fn validate_sources(manifest: &ProjectContextManifest) -> Result<(), ProjectError> {
    let mut seen = BTreeSet::new();
    let mut included_chars = 0u32;
    for source in &manifest.sources {
        if !valid_prefixed_uuid(&source.resource_id, "resource_")
            || !seen.insert(&source.resource_id)
            || source.included_chars > 2_000
            || source
                .included_sha256
                .as_deref()
                .is_some_and(|hash| !valid_hash(hash))
            || (source.state != ProjectSourceState::Ready && source.included_chars != 0)
            || (source.state != ProjectSourceState::Ready && source.included_sha256.is_some())
            || (source.state == ProjectSourceState::Ready
                && (source.included_sha256.is_none() || source.witness.is_none()))
        {
            return Err(ProjectError::InvalidRecord(
                "saved project source coverage is invalid",
            ));
        }
        included_chars = included_chars.saturating_add(source.included_chars);
        if included_chars > 12_000 {
            return Err(ProjectError::InvalidRecord(
                "saved project source coverage exceeds its budget",
            ));
        }
        if let Some(witness) = &source.witness {
            let valid = match witness {
                ProjectSourceWitness::WorkspaceFile {
                    workspace_key,
                    path,
                    capture_sha256,
                    ..
                } => {
                    valid_hash(workspace_key)
                        && valid_hash(capture_sha256)
                        && super::records::valid_relative_path(path)
                }
                ProjectSourceWitness::ChatExcerpt {
                    donor,
                    session_id,
                    clear_generation,
                    project_binding_revision,
                    source_state_sha256,
                    message_ids,
                } => {
                    valid_id(&donor.agent_id)
                        && donor.member_generation > 0
                        && donor.member_generation <= MAX_SAFE_INTEGER
                        && valid_id(session_id)
                        && *clear_generation <= MAX_SAFE_INTEGER
                        && *project_binding_revision > 0
                        && *project_binding_revision <= MAX_SAFE_INTEGER
                        && valid_hash(source_state_sha256)
                        && !message_ids.is_empty()
                        && message_ids.len() <= 4
                        && message_ids.iter().all(|id| valid_message_id(id))
                        && message_ids.iter().collect::<BTreeSet<_>>().len() == message_ids.len()
                }
                ProjectSourceWitness::ChatResult {
                    donor,
                    session_id,
                    clear_generation,
                    project_binding_revision,
                    result_id,
                    result_revision,
                } => {
                    valid_id(&donor.agent_id)
                        && donor.member_generation > 0
                        && donor.member_generation <= MAX_SAFE_INTEGER
                        && valid_id(session_id)
                        && *clear_generation <= MAX_SAFE_INTEGER
                        && *project_binding_revision > 0
                        && *project_binding_revision <= MAX_SAFE_INTEGER
                        && valid_message_id(result_id)
                        && *result_revision > 0
                        && *result_revision <= MAX_SAFE_INTEGER
                }
            };
            if !valid {
                return Err(ProjectError::InvalidRecord(
                    "saved project source witness is invalid",
                ));
            }
        }
    }
    Ok(())
}
