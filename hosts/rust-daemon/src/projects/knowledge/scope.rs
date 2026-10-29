//! Captured project authority; cached rows and file paths never grant access.

use std::path::PathBuf;
use std::sync::Arc;

use anima_core::CancelSignal;
use tokio::sync::Semaphore;
use tokio::time::Instant;

use super::folder::{self, KnowledgeFolderGrant};
use crate::app::SharedDaemonState;
use crate::attachments::documents::source_read::read_retained_with_slots;
use crate::profile_owner::ProfileOwner;
use crate::projects::{ProjectAccessLease, ProjectError, ProjectRecord, ProjectResourceTarget};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    Manual,
    Automatic,
}

#[derive(Clone)]
pub(super) struct Source {
    pub(super) id: String,
    pub(super) grant: SourceGrant,
    pub(super) owner_name: Option<String>,
    pub(super) account_label: Option<String>,
}

#[derive(Clone)]
pub(super) enum SourceGrant {
    Folder(KnowledgeFolderGrant),
    Connected(anima_document_store_sqlite::connected::ConnectedSourceGrant),
}

#[derive(Clone)]
pub(super) struct Scope {
    pub(super) project: ProjectRecord,
    pub(super) agent: String,
    pub(super) member_generation: u64,
    pub(super) lease: ProjectAccessLease,
    pub(super) sources: Vec<Source>,
    owner: Option<ProfileOwner>,
    protected: Vec<PathBuf>,
}

impl Scope {
    pub(super) async fn capture(
        state: &SharedDaemonState,
        id: &str,
        agent: &str,
        purpose: Purpose,
    ) -> Result<Self, ProjectError> {
        let state = state.read().await;
        state
            .ensure_profile_owned()
            .map_err(|_| ProjectError::AccessUnavailable)?;
        let project = state
            .projects
            .get(id)
            .ok_or(ProjectError::NotFound)?
            .clone();
        let member_generation = project
            .member(agent)
            .ok_or(ProjectError::NotMember)?
            .generation;
        let lease = crate::projects::member_read_lease(&state, id, agent)?;
        lease.check()?;
        let owner = state.profile_owner();
        let protected = owner
            .as_ref()
            .map(ProfileOwner::protected_paths)
            .unwrap_or_default();
        let sources = project
            .resources
            .iter()
            .filter_map(|resource| {
                if purpose == Purpose::Automatic && !resource.use_in_context {
                    return None;
                }
                match &resource.target {
                    ProjectResourceTarget::KnowledgeFolder {
                        root_path,
                        root_identity,
                    } => Some(Source {
                        id: resource.id.clone(),
                        owner_name: None,
                        account_label: None,
                        grant: SourceGrant::Folder(KnowledgeFolderGrant {
                            root_path: root_path.clone(),
                            root_identity: root_identity.clone(),
                        }),
                    }),
                    ProjectResourceTarget::ConnectedSource { grant } => Some(Source {
                        id: resource.id.clone(),
                        grant: SourceGrant::Connected(grant.clone()),
                        owner_name: state
                            .agents
                            .get(&grant.owner_agent_id)
                            .map(|agent| agent.config().name.clone()),
                        account_label: state
                            .drive_records
                            .get(&grant.connector_id)
                            .and_then(|r| r.account_label.clone())
                            .or_else(|| {
                                state
                                    .microsoft_file_records
                                    .get(&grant.connector_id)
                                    .and_then(|r| r.account_label.clone())
                            })
                            .or_else(|| {
                                state
                                    .mail_records
                                    .get(&grant.connector_id)
                                    .and_then(|r| r.connector.account_label.clone())
                            }),
                    }),
                    _ => None,
                }
            })
            .collect();
        let scope = Self {
            project,
            agent: agent.to_owned(),
            member_generation,
            lease,
            sources,
            owner,
            protected,
        };
        scope.check_physical()?;
        Ok(scope)
    }

    pub(super) fn check_physical(&self) -> Result<(), ProjectError> {
        self.lease.check()?;
        if let Some(owner) = &self.owner {
            owner
                .ensure_owned()
                .map_err(|_| ProjectError::AccessUnavailable)?;
        }
        Ok(())
    }

    pub(super) fn check_source(&self, source: &Source) -> Result<(), ProjectError> {
        self.check_physical()?;
        let SourceGrant::Folder(grant) = &source.grant else {
            return Ok(());
        };
        folder::ensure_root_allowed(std::path::Path::new(&grant.root_path), &self.protected)
            .map_err(|_| ProjectError::AccessUnavailable)?;
        folder::open_grant(grant).map_err(|_| ProjectError::AccessUnavailable)?;
        Ok(())
    }

    pub(super) fn check_source_state(
        &self,
        state: &crate::state::DaemonState,
        source: &Source,
    ) -> Result<(), ProjectError> {
        self.check_state(state)?;
        let resource = self
            .project
            .resources
            .iter()
            .find(|resource| resource.id == source.id)
            .ok_or(ProjectError::AccessRevoked)?;
        crate::projects::context::validate_resource_access(state, &self.project, resource)
            .map_err(|_| ProjectError::AccessUnavailable)
    }

    pub(super) async fn check(&self, state: &SharedDaemonState) -> Result<(), ProjectError> {
        self.check_physical()?;
        let state = state.read().await;
        self.check_state(&state)
    }

    pub(super) async fn read_connected_retained(
        &self,
        state: &SharedDaemonState,
        source: &Source,
        cancel: &CancelSignal,
        deadline: Instant,
        slots: Arc<Semaphore>,
    ) -> Result<crate::projects::connected::ConnectedSnapshot, ProjectError> {
        check_read_budget(cancel, deadline)?;
        self.check_source_state(&*state.read().await, source)?;
        let SourceGrant::Connected(grant) = &source.grant else {
            return Err(ProjectError::AccessUnavailable);
        };
        let permit = slots.try_acquire_owned().map_err(|_| ProjectError::Busy)?;
        check_read_budget(cancel, deadline)?;
        let shared = state.clone();
        let scope = self.clone();
        let owned_source = source.clone();
        let grant = grant.clone();
        let signal = cancel.clone();
        let reading = tokio::spawn(async move {
            // The native manager may own nested jobs. Keep capacity until its complete await settles.
            let _permit = permit;
            check_read_budget(&signal, deadline)?;
            scope.check_source_state(&*shared.read().await, &owned_source)?;
            check_read_budget(&signal, deadline)?;
            scope.check_physical()?;
            let snapshot = crate::projects::connected::read(&shared, &grant).await?;
            scope.check_source_state(&*shared.read().await, &owned_source)?;
            check_read_budget(&signal, deadline)?;
            Ok(snapshot)
        });
        let result = tokio::select! { biased;
            () = cancel.cancelled() => return Err(ProjectError::Cancelled),
            () = self.lease.cancelled() => return Err(ProjectError::AccessRevoked),
            () = tokio::time::sleep_until(deadline) => return Err(ProjectError::Deadline),
            result = reading => result.map_err(|_| ProjectError::AccessUnavailable)?,
        };
        self.check_source_state(&*state.read().await, source)?;
        check_read_budget(cancel, deadline)?;
        result
    }

    pub(super) fn check_state(
        &self,
        state: &crate::state::DaemonState,
    ) -> Result<(), ProjectError> {
        self.check_physical()?;
        state
            .ensure_profile_owned()
            .map_err(|_| ProjectError::AccessUnavailable)?;
        let project = state
            .projects
            .get(&self.project.id)
            .ok_or(ProjectError::AccessRevoked)?;
        if project.revision != self.project.revision
            || project.access_epoch != self.project.access_epoch
            || project.member(&self.agent).map(|member| member.generation)
                != Some(self.member_generation)
            || project.resources != self.project.resources
        {
            return Err(ProjectError::ScopeChanged);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct DocumentWitness {
    pub(super) source: Source,
    pub(super) relative_path: String,
    pub(super) sha256: String,
}

/// Binds returned passages to the captured member, source grants and original bytes.
pub(crate) struct KnowledgeWitness {
    pub(super) scope: Scope,
    pub(super) documents: Vec<DocumentWitness>,
    pub(super) read_slots: Arc<Semaphore>,
    pub(super) cancel: CancelSignal,
    pub(super) deadline: Instant,
}

impl KnowledgeWitness {
    pub(crate) fn check_access(&self) -> Result<(), ProjectError> {
        self.scope.check_physical()
    }

    pub(crate) async fn check(&self, state: &SharedDaemonState) -> Result<(), ProjectError> {
        self.check_budget()?;
        self.scope.check(state).await?;
        if self.documents.is_empty() {
            return self.check_budget();
        }
        for document in &self.documents {
            if matches!(&document.source.grant, SourceGrant::Connected(_)) {
                let snapshot = self
                    .scope
                    .read_connected_retained(
                        state,
                        &document.source,
                        &self.cancel,
                        self.deadline,
                        self.read_slots.clone(),
                    )
                    .await?;
                if snapshot.name != document.relative_path
                    || snapshot.source_sha256 != document.sha256
                {
                    return Err(ProjectError::ScopeChanged);
                }
                self.check_budget()?;
                self.scope
                    .check_source_state(&*state.read().await, &document.source)?;
            }
        }
        let documents = self.documents.clone();
        let scope = self.scope.clone();
        let signal = self.cancel.clone();
        let deadline = self.deadline;
        let result = read_retained_with_slots(
            move || {
                let mut bytes = 0usize;
                for document in documents {
                    let SourceGrant::Folder(grant) = &document.source.grant else {
                        continue;
                    };
                    if signal.is_cancelled() {
                        return Err(ProjectError::Cancelled);
                    }
                    if Instant::now() >= deadline {
                        return Err(ProjectError::Deadline);
                    }
                    scope.check_physical()?;
                    scope.check_source(&document.source)?;
                    bytes = bytes
                        .checked_add(
                            folder::verify_document(
                                grant,
                                &document.relative_path,
                                &document.sha256,
                            )
                            .map_err(|_| ProjectError::ScopeChanged)?,
                        )
                        .ok_or(ProjectError::AccessUnavailable)?;
                    if bytes > folder::MAX_PASS_BYTES {
                        return Err(ProjectError::AccessUnavailable);
                    }
                }
                scope.check_physical()
            },
            &self.cancel,
            self.deadline,
            self.read_slots.clone(),
        )
        .await
        .map_err(super::service::worker_error)?;
        result?;
        self.scope.check(state).await?;
        self.check_budget()
    }

    fn check_budget(&self) -> Result<(), ProjectError> {
        if self.cancel.is_cancelled() {
            return Err(ProjectError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(ProjectError::Deadline);
        }
        Ok(())
    }
}

fn check_read_budget(cancel: &CancelSignal, deadline: Instant) -> Result<(), ProjectError> {
    if cancel.is_cancelled() {
        return Err(ProjectError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ProjectError::Deadline);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) async fn connected_read(
        state: &SharedDaemonState,
        project_id: &str,
        agent: &str,
        resource_id: &str,
        cancel: &CancelSignal,
        deadline: Instant,
        slots: Arc<Semaphore>,
    ) -> Result<crate::projects::connected::ConnectedSnapshot, ProjectError> {
        let scope = Scope::capture(state, project_id, agent, Purpose::Manual).await?;
        let source = scope
            .sources
            .iter()
            .find(|source| source.id == resource_id)
            .ok_or(ProjectError::NotFound)?;
        scope
            .read_connected_retained(state, source, cancel, deadline, slots)
            .await
    }
}
