//! Durable owner mutations and narrow, membership-checked project projections.

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::context::{eligible_session, validate_resource_access};
use super::records::{
    clean_title, digest, next_revision, ProjectDonorBinding, ProjectError, ProjectInvalidation,
    ProjectMutationReplay, ProjectRecord, ProjectResourceRecord, ProjectResourceRole,
    ProjectResourceTarget, ProjectSessionBindingPatch, ProjectStatus,
};
use super::registry::{ProjectCreateRequest, ProjectPatchRequest, ProjectRegistry};
use crate::app::SharedDaemonState;
use crate::live::{LiveEvent, LiveEventBody};
use crate::state::DaemonState;

#[derive(Clone)]
pub(crate) struct ProjectService {
    state: SharedDaemonState,
    transactions: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AddResourceRequest {
    pub(crate) expected_revision: u64,
    pub(crate) request_key: String,
    pub(crate) as_agent_id: String,
    pub(crate) target: ProjectResourceTarget,
    pub(crate) role: ProjectResourceRole,
    pub(crate) label: Option<String>,
    pub(crate) use_in_context: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MoveChatRequest {
    pub(crate) project_id: Option<String>,
    pub(crate) expected_binding_revision: u64,
    pub(crate) expected_project_revision: Option<u64>,
}

#[derive(Clone)]
pub(crate) enum ProjectMutation {
    Create(ProjectCreateRequest),
    Patch {
        id: String,
        input: ProjectPatchRequest,
    },
    Members {
        id: String,
        expected_revision: u64,
        agents: Vec<String>,
    },
    Delete {
        id: String,
        expected_revision: u64,
    },
    AddResource {
        id: String,
        input: AddResourceRequest,
    },
    PatchResource {
        id: String,
        resource_id: String,
        as_agent: String,
        expected_revision: u64,
        label: Option<String>,
        role: Option<ProjectResourceRole>,
        use_in_context: Option<bool>,
    },
    RemoveResource {
        id: String,
        resource_id: String,
        as_agent: String,
        expected_revision: u64,
    },
    OrderResources {
        id: String,
        as_agent: String,
        expected_revision: u64,
        resource_ids: Vec<String>,
    },
    MoveChat {
        agent: String,
        session: String,
        input: MoveChatRequest,
    },
}

struct PreparedMutation {
    registry: ProjectRegistry,
    bindings: Vec<ProjectSessionBindingPatch>,
    changes: Vec<ProjectInvalidation>,
    result: Value,
}

impl ProjectService {
    pub(crate) fn new(state: SharedDaemonState, transactions: Arc<Mutex<()>>) -> Self {
        Self {
            state,
            transactions,
        }
    }

    pub(crate) async fn list(
        &self,
        member: Option<&str>,
        archived: bool,
    ) -> Result<Value, ProjectError> {
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        let mut records: Vec<_> = state
            .projects
            .records()
            .filter(|record| {
                (member.is_none()
                    || record.status
                        == (if archived {
                            ProjectStatus::Archived
                        } else {
                            ProjectStatus::Active
                        }))
                    && member.is_none_or(|agent| record.member(agent).is_some())
            })
            .collect();
        if let Some(agent) = member {
            validate_agents(&state, &[agent.to_string()])?;
        }
        records.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut projects = Vec::new();
        for record in records {
            if let Some(agent) = member {
                state.project_access.acquire_read(record, agent)?.check()?;
            }
            projects.push(summary(&state, record));
        }
        Ok(json!({"projects":projects}))
    }

    pub(crate) async fn get(&self, id: &str, agent: &str) -> Result<Value, ProjectError> {
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        let record = member_project(&state, id, agent)?;
        let lease = state.project_access.acquire_read(record, agent)?;
        let project = project_view(&state, record);
        lease.check()?;
        Ok(json!({"project":project}))
    }

    pub(crate) async fn chats(
        &self,
        id: &str,
        agent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value, ProjectError> {
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        let project = member_project(&state, id, agent)?;
        let lease = state.project_access.acquire_read(project, agent)?;
        let mut sessions: Vec<_> = state
            .sessions
            .records()
            .filter(|session| {
                session.project_id.as_deref() == Some(id)
                    && project.member(&session.agent_id).is_some()
                    && eligible_session(&state, session)
            })
            .collect();
        sessions
            .sort_by(|left, right| (&left.agent_id, &left.id).cmp(&(&right.agent_id, &right.id)));
        let start = match cursor {
            Some(cursor) => sessions
                .iter()
                .position(|row| {
                    chat_cursor(
                        project,
                        agent,
                        &row.agent_id,
                        &row.id,
                        row.project_binding_revision,
                    ) == cursor
                })
                .map(|index| index + 1)
                .ok_or(ProjectError::ScopeChanged)?,
            None => 0,
        };
        let end = (start + limit.clamp(1, 100)).min(sessions.len());
        let rows: Vec<_> = sessions[start..end].iter().map(|session| json!({"ownerAgentId":session.agent_id,"sessionId":session.id,
            "title":session.title,"archived":session.archived,"lastActivityAtMs":session.last_activity_at_ms,
            "activeRuns":state.runs.active_count_for_session(&session.agent_id,&session.id),"projectBindingRevision":session.project_binding_revision})).collect();
        let next = (end < sessions.len()).then(|| {
            chat_cursor(
                project,
                agent,
                &sessions[end - 1].agent_id,
                &sessions[end - 1].id,
                sessions[end - 1].project_binding_revision,
            )
        });
        lease.check()?;
        Ok(
            json!({"chats":rows,"nextCursor":next,"projectRevision":project.revision,"accessEpoch":project.access_epoch,
            "memberGeneration":project.member(agent).ok_or(ProjectError::NotMember)?.generation}),
        )
    }

    pub(crate) async fn mutate(&self, mutation: ProjectMutation) -> Result<Value, ProjectError> {
        let service = self.clone();
        tokio::spawn(async move { service.mutate_owned(mutation).await })
            .await
            .map_err(|_| ProjectError::AccessUnavailable)?
    }

    async fn mutate_owned(&self, mutation: ProjectMutation) -> Result<Value, ProjectError> {
        let preflight = if let ProjectMutation::AddResource { id, input } = &mutation {
            let (resource, binding) = {
                let state = self.state.read().await;
                let candidate = prepare(&state, mutation.clone())?;
                let resource = candidate
                    .registry
                    .get(id)
                    .and_then(|project| {
                        project
                            .resources
                            .iter()
                            .find(|resource| resource.add_replay.request_key == input.request_key)
                    })
                    .ok_or(ProjectError::NotFound)?
                    .clone();
                let binding = digest(&(
                    &resource.target,
                    &resource.donor,
                    resource.source_project_binding_revision,
                    &resource.workspace_key,
                ))?;
                (resource, binding)
            };
            super::context::validate_new_resource(&self.state, id, &input.as_agent_id, &resource)
                .await?;
            Some((id.clone(), input.request_key.clone(), binding))
        } else {
            None
        };
        let _transaction = self.transactions.clone().lock_owned().await;
        let owner = {
            let state = self.state.read().await;
            state
                .ensure_profile_owned()
                .map_err(|_| ProjectError::AccessUnavailable)?;
            state.profile_owner()
        };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| ProjectError::AccessUnavailable)?;
        }
        let (mut prepared, mut stops) = {
            let mut state = self.state.write().await;
            let prepared = prepare(&state, mutation)?;
            if let Some((id, key, binding)) = &preflight {
                let resource = prepared
                    .registry
                    .get(id)
                    .and_then(|project| {
                        project
                            .resources
                            .iter()
                            .find(|resource| &resource.add_replay.request_key == key)
                    })
                    .ok_or(ProjectError::ScopeChanged)?;
                if digest(&(
                    &resource.target,
                    &resource.donor,
                    resource.source_project_binding_revision,
                    &resource.workspace_key,
                ))? != *binding
                {
                    return Err(ProjectError::ScopeChanged);
                }
            }
            let affected: BTreeSet<_> = prepared
                .changes
                .iter()
                .filter(|notice| {
                    notice.deleted
                        || state
                            .projects
                            .get(&notice.project_id)
                            .is_some_and(|old| old.access_epoch != notice.access_epoch)
                })
                .map(|notice| notice.project_id.clone())
                .collect();
            let roots: Vec<_> = state
                .runs
                .active_records()
                .into_iter()
                .filter(|run| {
                    run.project_witness
                        .as_ref()
                        .and_then(|witness| witness.project.as_ref())
                        .is_some_and(|binding| affected.contains(&binding.project_id))
                })
                .map(|run| (run.agent_id.clone(), run.id.clone()))
                .collect();
            let mut stops = Vec::new();
            for (agent, run) in roots {
                if let Some(plan) =
                    state.request_run_stop(&agent, &run, anima_core::primitives::now_millis())
                {
                    stops.push(plan);
                }
            }
            (prepared, stops)
        };
        prepared.result["stopRequestedRunIds"] = json!(stops
            .iter()
            .flat_map(|plan| plan.signal.iter().cloned())
            .collect::<BTreeSet<_>>());
        let mut saved = false;
        for _ in 0..3 {
            let request = {
                let mut state = self.state.write().await;
                state
                    .control_plane_persist_request()
                    .with_project_candidate(
                        prepared.registry.snapshot(),
                        prepared.registry.creation_receipts_snapshot(),
                        prepared.bindings.clone(),
                    )
            };
            let Some(request) = request else {
                let mut state = self.state.write().await;
                for plan in stops.drain(..).rev() {
                    state.revert_run_stop(plan.undo);
                }
                return Err(ProjectError::ScopeChanged);
            };
            match request.save_project_candidate().await {
                Ok(()) => {
                    saved = true;
                    break;
                }
                Err(error) if crate::state::is_superseded_control_plane_save(&error) => continue,
                Err(error) => {
                    let mut state = self.state.write().await;
                    if crate::control_plane_store::is_durability_uncertain(&error) {
                        for notice in &prepared.changes {
                            state.project_access.fence_uncertain(&notice.project_id);
                        }
                        for plan in &stops {
                            for run in &plan.signal {
                                if let Some(control) = state.live.runs().control(run) {
                                    control.cancel.cancel();
                                }
                            }
                        }
                    } else {
                        for plan in stops.drain(..).rev() {
                            state.revert_run_stop(plan.undo);
                        }
                    }
                    return Err(ProjectError::Persistence(error.to_string()));
                }
            }
        }
        if !saved {
            let mut state = self.state.write().await;
            for plan in stops.drain(..).rev() {
                state.revert_run_stop(plan.undo);
            }
            return Err(ProjectError::ScopeChanged);
        }
        let mut state = self.state.write().await;
        state.projects = prepared.registry;
        for patch in &prepared.bindings {
            let session = state
                .sessions
                .get_mut(&patch.agent_id, &patch.session_id)
                .ok_or(ProjectError::AccessUnavailable)?;
            if session.created_at_ms != patch.created_at_ms
                || session.project_binding_revision != patch.expected_binding_revision
            {
                for notice in &prepared.changes {
                    state.project_access.fence_uncertain(&notice.project_id);
                }
                return Err(ProjectError::AccessUnavailable);
            }
            session.project_id = patch.project_id.clone();
            session.project_binding_revision = patch.project_binding_revision;
        }
        state.project_access.publish_records(&state.projects)?;
        for plan in &stops {
            for run in &plan.signal {
                if let Some(control) = state.live.runs().control(run) {
                    control.cancel.cancel();
                }
            }
            for run in &plan.cancelled {
                state.live.publish(crate::live::run_status_event(run), None);
            }
        }
        for patch in &prepared.bindings {
            state.publish_session_event(
                &patch.agent_id,
                &patch.session_id,
                LiveEventBody::SessionUpdated,
            );
        }
        for notice in &prepared.changes {
            publish_project_invalidation(&state, notice);
        }
        Ok(prepared.result)
    }
}

fn chat_cursor(
    project: &ProjectRecord,
    requesting_agent: &str,
    owner: &str,
    session: &str,
    binding_revision: u64,
) -> String {
    digest(&(
        &project.id,
        project.revision,
        project.access_epoch,
        requesting_agent,
        project
            .member(requesting_agent)
            .map(|member| member.generation),
        owner,
        session,
        binding_revision,
    ))
    .expect("project chat cursor contains only serializable identity fields")
}

fn validate_agents(state: &DaemonState, agents: &[String]) -> Result<(), ProjectError> {
    if agents.iter().any(|id| {
        !state
            .agents
            .get(id)
            .is_some_and(|agent| !crate::agent_runs::is_helper_config(agent.config()))
    }) {
        return Err(ProjectError::NotMember);
    }
    Ok(())
}

pub(super) fn member_project<'a>(
    state: &'a DaemonState,
    id: &str,
    agent: &str,
) -> Result<&'a ProjectRecord, ProjectError> {
    validate_agents(state, &[agent.to_string()])?;
    state
        .projects
        .get(id)
        .filter(|project| project.member(agent).is_some())
        .ok_or(ProjectError::NotMember)
}

pub(super) fn summary(state: &DaemonState, project: &ProjectRecord) -> Value {
    json!({"id":project.id,"title":project.title,"status":project.status,"revision":project.revision,"accessEpoch":project.access_epoch,
        "members":project.members,"createdAtMs":project.created_at_ms,"updatedAtMs":project.updated_at_ms,
        "chatCount":state.sessions.records().filter(|session| session.project_id.as_deref()==Some(&project.id)&&project.member(&session.agent_id).is_some()).count(),
        "resourceCount":project.resources.len()})
}

fn project_view(state: &DaemonState, project: &ProjectRecord) -> Value {
    let mut value = summary(state, project);
    value["instructions"] = json!(project.instructions);
    value["resources"] = json!(project.resources.iter().map(|resource| {
        let availability = validate_resource_access(state, project, resource);
        let revoked = availability == Err(super::ProjectSourceState::AccessRemoved);
        json!({"id":resource.id,"target":if revoked {Value::Null}else{json!(resource.target)},"state":availability.err().unwrap_or(super::ProjectSourceState::Ready),
            "role":resource.role,"label":if revoked {Value::Null}else{json!(resource.label)},"useInContext":resource.use_in_context,"createdAtMs":resource.created_at_ms})
    }).collect::<Vec<_>>());
    value
}

fn ensure_idle(state: &DaemonState, project_id: &str) -> Result<(), ProjectError> {
    if state.sessions.records().any(|session| {
        session.project_id.as_deref() == Some(project_id)
            && state
                .runs
                .active_count_for_session(&session.agent_id, &session.id)
                > 0
    }) {
        return Err(ProjectError::Busy);
    }
    Ok(())
}

fn patch_for(
    session: &crate::sessions::SessionRecord,
    project_id: Option<String>,
) -> Result<ProjectSessionBindingPatch, ProjectError> {
    Ok(ProjectSessionBindingPatch {
        agent_id: session.agent_id.clone(),
        session_id: session.id.clone(),
        created_at_ms: session.created_at_ms,
        expected_binding_revision: session.project_binding_revision,
        project_id,
        project_binding_revision: next_revision(session.project_binding_revision)?,
    })
}

fn invalidation(
    old: Option<&ProjectRecord>,
    new: Option<&ProjectRecord>,
) -> Result<ProjectInvalidation, ProjectError> {
    let record = new.or(old).ok_or(ProjectError::NotFound)?;
    Ok(ProjectInvalidation {
        project_id: record.id.clone(),
        revision: if new.is_some() {
            record.revision
        } else {
            next_revision(record.revision)?
        },
        access_epoch: if new.is_some() {
            record.access_epoch
        } else {
            next_revision(record.access_epoch)?
        },
        deleted: new.is_none(),
        membership_changed: old.map(|row| &row.members) != new.map(|row| &row.members),
        destinations: old
            .into_iter()
            .chain(new)
            .flat_map(|row| row.members.iter().map(|member| member.agent_id.clone()))
            .collect(),
    })
}

fn prepare(
    state: &DaemonState,
    mutation: ProjectMutation,
) -> Result<PreparedMutation, ProjectError> {
    let mut registry = state.projects.clone();
    let mut bindings = Vec::new();
    let mut changes = Vec::new();
    let now = anima_core::primitives::now_millis();
    let result = match mutation {
        ProjectMutation::Create(input) => {
            // Replays are checked before live membership, so a deleted outcome cannot recreate.
            let normalized = ProjectRegistry::normalize_create(input)?;
            let (project, created) = registry.create(normalized.clone(), now)?;
            if created {
                validate_agents(state, &normalized.member_agent_ids)?;
                changes.push(invalidation(None, Some(&project))?);
            }
            json!({"project":project_view(state,&project),"created":created})
        }
        ProjectMutation::Patch { id, input } => {
            let old = registry.get(&id).ok_or(ProjectError::NotFound)?.clone();
            if input.status.is_some_and(|status| status != old.status) {
                ensure_idle(state, &id)?;
            }
            let changed = registry.patch(&id, input, now)?;
            changes.push(invalidation(Some(&old), Some(&changed))?);
            json!({"projectSummary":summary(state,&changed)})
        }
        ProjectMutation::Members {
            id,
            expected_revision,
            agents,
        } => {
            validate_agents(state, &agents)?;
            let old = registry.get(&id).ok_or(ProjectError::NotFound)?.clone();
            let changed = registry.replace_members(&id, expected_revision, agents, now)?;
            for session in state.sessions.records().filter(|session| {
                session.project_id.as_deref() == Some(&id)
                    && changed.member(&session.agent_id).is_none()
            }) {
                bindings.push(patch_for(session, None)?);
            }
            changes.push(invalidation(Some(&old), Some(&changed))?);
            json!({"projectSummary":summary(state,&changed),"accessEpoch":changed.access_epoch,"detachedChats":bindings.len()})
        }
        ProjectMutation::Delete {
            id,
            expected_revision,
        } => {
            ensure_idle(state, &id)?;
            let old = registry.delete(&id, expected_revision, now)?;
            for session in state
                .sessions
                .records()
                .filter(|session| session.project_id.as_deref() == Some(&id))
            {
                bindings.push(patch_for(session, None)?);
            }
            changes.push(invalidation(Some(&old), None)?);
            json!({"deleted":true,"detachedChatCount":bindings.len()})
        }
        ProjectMutation::AddResource { id, input } => {
            let old = member_project(state, &id, &input.as_agent_id)?.clone();
            input.target.validate()?;
            let donor = input
                .target
                .owner_agent_id()
                .map(|agent| {
                    old.member(agent)
                        .map(|member| ProjectDonorBinding {
                            agent_id: agent.into(),
                            member_generation: member.generation,
                        })
                        .ok_or(ProjectError::NotMember)
                })
                .transpose()?;
            let source_project_binding_revision = input
                .target
                .session_id()
                .map(|sid| {
                    let owner = input
                        .target
                        .owner_agent_id()
                        .ok_or(ProjectError::NotMember)?;
                    let session = state
                        .sessions
                        .get(owner, sid)
                        .ok_or(ProjectError::NotFound)?;
                    if session.project_id.as_deref() != Some(&id)
                        || !eligible_session(state, session)
                    {
                        return Err(ProjectError::NotMember);
                    }
                    Ok(session.project_binding_revision)
                })
                .transpose()?;
            match &input.target {
                ProjectResourceTarget::Workflow {
                    owner_agent_id,
                    workflow_id,
                } if !state
                    .workflows
                    .get(workflow_id)
                    .is_some_and(|row| row.agent_id == *owner_agent_id) =>
                {
                    return Err(ProjectError::NotFound)
                }
                ProjectResourceTarget::WorkflowTemplate {
                    owner_agent_id,
                    slug,
                } if !state.workflow_templates.values().any(|row| {
                    row.agent_id == *owner_agent_id && row.slug == *slug && !row.deleted
                }) =>
                {
                    return Err(ProjectError::NotFound)
                }
                _ => {}
            }
            let resource = ProjectResourceRecord {
                id: format!("resource_{}", uuid::Uuid::new_v4()),
                target: input.target.clone(),
                role: input.role,
                label: input
                    .label
                    .as_ref()
                    .map(|label| clean_title(label))
                    .transpose()?,
                use_in_context: input.use_in_context,
                created_at_ms: now,
                add_replay: ProjectMutationReplay {
                    request_key: input.request_key.clone(),
                    request_sha256: digest(&input)?,
                },
                donor,
                source_project_binding_revision,
                workspace_key: if matches!(
                    input.target,
                    ProjectResourceTarget::WorkspaceFile { .. }
                ) {
                    Some(
                        state
                            .workspace
                            .as_ref()
                            .map(|row| crate::workspace_file_preview::workspace_key(&row.root_path))
                            .ok_or(ProjectError::NotFound)?,
                    )
                } else {
                    None
                },
            };
            validate_resource_access(state, &old, &resource)
                .map_err(|_| ProjectError::NotMember)?;
            let changed = registry.add_resource(&id, input.expected_revision, resource, now)?;
            changes.push(invalidation(Some(&old), Some(&changed))?);
            json!({"project":project_view(state,&changed)})
        }
        ProjectMutation::PatchResource {
            id,
            resource_id,
            as_agent,
            expected_revision,
            label,
            role,
            use_in_context,
        } => {
            let old = member_project(state, &id, &as_agent)?.clone();
            let changed = registry.patch_resource(
                &id,
                expected_revision,
                &resource_id,
                label,
                role,
                use_in_context,
                now,
            )?;
            changes.push(invalidation(Some(&old), Some(&changed))?);
            json!({"project":project_view(state,&changed)})
        }
        ProjectMutation::RemoveResource {
            id,
            resource_id,
            as_agent,
            expected_revision,
        } => {
            let old = member_project(state, &id, &as_agent)?.clone();
            let changed = registry.remove_resource(&id, expected_revision, &resource_id, now)?;
            changes.push(invalidation(Some(&old), Some(&changed))?);
            json!({"project":project_view(state,&changed)})
        }
        ProjectMutation::OrderResources {
            id,
            as_agent,
            expected_revision,
            resource_ids,
        } => {
            let old = member_project(state, &id, &as_agent)?.clone();
            let changed = registry.reorder_resources(&id, expected_revision, &resource_ids, now)?;
            changes.push(invalidation(Some(&old), Some(&changed))?);
            json!({"project":project_view(state,&changed)})
        }
        ProjectMutation::MoveChat {
            agent,
            session,
            input,
        } => {
            let original = state
                .sessions
                .get(&agent, &session)
                .ok_or(ProjectError::NotFound)?;
            if !eligible_session(state, original)
                || original.project_binding_revision != input.expected_binding_revision
            {
                return Err(ProjectError::ScopeChanged);
            }
            if state.runs.active_count_for_session(&agent, &session) > 0 {
                return Err(ProjectError::Busy);
            }
            if let Some(target) = &input.project_id {
                let project = member_project(state, target, &agent)?;
                if project.status != ProjectStatus::Active {
                    return Err(ProjectError::Archived);
                }
                if input.expected_project_revision != Some(project.revision) {
                    return Err(ProjectError::StaleRevision);
                }
            } else if input.expected_project_revision.is_some() {
                return Err(ProjectError::InvalidRecord(
                    "a detached chat has no target project revision",
                ));
            }
            bindings.push(patch_for(original, input.project_id.clone())?);
            for changed in registry.invalidate_sources_for_session(&agent, &session, now)? {
                changes.push(invalidation(
                    state.projects.get(&changed.id),
                    Some(&changed),
                )?);
            }
            json!({"moved":true,"projectId":input.project_id,"projectBindingRevision":bindings[0].project_binding_revision})
        }
    };
    Ok(PreparedMutation {
        registry,
        bindings,
        changes,
        result,
    })
}

pub(crate) fn publish_project_invalidation(state: &DaemonState, change: &ProjectInvalidation) {
    for agent in &change.destinations {
        state.live.publish(
            LiveEvent::new(
                agent,
                LiveEventBody::ProjectUpdated {
                    project_id: change.project_id.clone(),
                    revision: change.revision,
                    access_epoch: change.access_epoch,
                    deleted: change.deleted,
                    membership_changed: change.membership_changed,
                },
            ),
            None,
        );
    }
}
