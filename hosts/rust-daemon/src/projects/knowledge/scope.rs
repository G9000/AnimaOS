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
    pub(super) grant: KnowledgeFolderGrant,
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
                        grant: KnowledgeFolderGrant {
                            root_path: root_path.clone(),
                            root_identity: root_identity.clone(),
                        },
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
        folder::ensure_root_allowed(
            std::path::Path::new(&source.grant.root_path),
            &self.protected,
        )
        .map_err(|_| ProjectError::AccessUnavailable)?;
        folder::open_grant(&source.grant).map_err(|_| ProjectError::AccessUnavailable)?;
        Ok(())
    }

    pub(super) async fn check(&self, state: &SharedDaemonState) -> Result<(), ProjectError> {
        self.check_physical()?;
        let state = state.read().await;
        self.check_state(&state)
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
        let documents = self.documents.clone();
        let scope = self.scope.clone();
        let signal = self.cancel.clone();
        let deadline = self.deadline;
        let result = read_retained_with_slots(
            move || {
                let mut bytes = 0usize;
                for document in documents {
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
                                &document.source.grant,
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
