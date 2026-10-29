//! Revocable project reads layered on existing model and native authority.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::{Arc, RwLock, Weak};

use anima_core::execution_limits::ModelCallContext;
use anima_core::{
    AgentConfig, CancelSignal, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
    ModelStreamFrame, ModelStreamSink, TokenUsage,
};
use async_trait::async_trait;

use super::records::{ProjectError, ProjectRecord, ProjectStatus, RunProjectBinding};
use super::registry::ProjectRegistry;

#[derive(Clone, Default)]
pub(crate) struct ProjectAccessRegistry {
    inner: Arc<ProjectAccessInner>,
}

#[derive(Default)]
struct ProjectAccessInner {
    published: RwLock<BTreeMap<String, PublishedProjectAccess>>,
}

struct PublishedProjectAccess {
    revision: u64,
    access_epoch: u64,
    status: ProjectStatus,
    members: BTreeMap<String, u64>,
    epoch_cancel: CancelSignal,
    closed_uncertain: bool,
    revoked_sources: BTreeSet<String>,
}

#[derive(Clone, Copy)]
enum ProjectAccessPurpose {
    MemberRead,
    Run,
}

#[derive(Clone)]
pub(crate) struct ProjectAccessLease {
    registry: Weak<ProjectAccessInner>,
    project_id: String,
    requesting_agent_id: String,
    member_generation: u64,
    access_epoch: u64,
    purpose: ProjectAccessPurpose,
    epoch_cancel: CancelSignal,
}

impl ProjectAccessRegistry {
    pub(crate) fn publish_records(&self, records: &ProjectRegistry) -> Result<(), ProjectError> {
        let mut published = self
            .inner
            .published
            .write()
            .expect("project access publication lock poisoned");
        for record in records.records() {
            record.validate()?;
            if let Some(old) = published.get_mut(&record.id) {
                let members = member_map(record);
                if record.revision < old.revision
                    || record.access_epoch < old.access_epoch
                    || (record.access_epoch == old.access_epoch
                        && (members != old.members || record.status != old.status))
                {
                    old.closed_uncertain = true;
                    old.epoch_cancel.cancel();
                    return Err(ProjectError::AccessUnavailable);
                }
            }
        }
        published.retain(|id, value| {
            if records.get(id).is_some() {
                true
            } else {
                value.epoch_cancel.cancel();
                false
            }
        });
        for record in records.records() {
            if let Some(old) = published.get_mut(&record.id) {
                if record.access_epoch == old.access_epoch {
                    if old.closed_uncertain {
                        return Err(ProjectError::AccessUnavailable);
                    }
                    old.revision = record.revision;
                    continue;
                }
                old.epoch_cancel.cancel();
            }
            published.insert(
                record.id.clone(),
                PublishedProjectAccess {
                    revision: record.revision,
                    access_epoch: record.access_epoch,
                    status: record.status,
                    members: member_map(record),
                    epoch_cancel: CancelSignal::new(),
                    closed_uncertain: false,
                    revoked_sources: BTreeSet::new(),
                },
            );
        }
        Ok(())
    }

    pub(crate) fn fence_uncertain(&self, project_id: &str) {
        if let Some(record) = self
            .inner
            .published
            .write()
            .expect("project access publication lock poisoned")
            .get_mut(project_id)
        {
            record.closed_uncertain = true;
            record.epoch_cancel.cancel();
        }
    }

    /// Cancel captured content once per revoked source while allowing fresh healthy-source turns.
    pub(super) fn publish_source_revocations(&self, project_id: &str, revoked: BTreeSet<String>) {
        let mut published = self
            .inner
            .published
            .write()
            .expect("project access publication lock poisoned");
        if let Some(current) = published.get_mut(project_id) {
            if revoked
                .iter()
                .any(|id| !current.revoked_sources.contains(id))
                && !current.closed_uncertain
            {
                current.epoch_cancel.cancel();
                current.epoch_cancel = CancelSignal::new();
            }
            current.revoked_sources = revoked;
        }
    }

    pub(super) fn acquire_read(
        &self,
        record: &ProjectRecord,
        agent: &str,
    ) -> Result<ProjectAccessLease, ProjectError> {
        self.acquire(record, agent, ProjectAccessPurpose::MemberRead)
    }

    pub(super) fn acquire_run(
        &self,
        record: &ProjectRecord,
        agent: &str,
    ) -> Result<ProjectAccessLease, ProjectError> {
        self.acquire(record, agent, ProjectAccessPurpose::Run)
    }

    fn acquire(
        &self,
        record: &ProjectRecord,
        agent: &str,
        purpose: ProjectAccessPurpose,
    ) -> Result<ProjectAccessLease, ProjectError> {
        let member = record.member(agent).ok_or(ProjectError::NotMember)?;
        let published = self
            .inner
            .published
            .read()
            .expect("project access publication lock poisoned");
        let current = published
            .get(&record.id)
            .ok_or(ProjectError::AccessUnavailable)?;
        if current.closed_uncertain || current.epoch_cancel.is_cancelled() {
            return Err(ProjectError::AccessUnavailable);
        }
        if current.access_epoch != record.access_epoch
            || current.members.get(agent) != Some(&member.generation)
        {
            return Err(ProjectError::AccessRevoked);
        }
        if matches!(purpose, ProjectAccessPurpose::Run) && current.status != ProjectStatus::Active {
            return Err(ProjectError::Archived);
        }
        Ok(ProjectAccessLease {
            registry: Arc::downgrade(&self.inner),
            project_id: record.id.clone(),
            requesting_agent_id: agent.into(),
            member_generation: member.generation,
            access_epoch: record.access_epoch,
            purpose,
            epoch_cancel: current.epoch_cancel.clone(),
        })
    }

    pub(super) fn check_binding(
        &self,
        binding: &RunProjectBinding,
        agent: &str,
    ) -> Result<(), ProjectError> {
        let published = self
            .inner
            .published
            .read()
            .expect("project access publication lock poisoned");
        let current = published
            .get(&binding.project_id)
            .ok_or(ProjectError::AccessRevoked)?;
        if current.closed_uncertain {
            return Err(ProjectError::AccessUnavailable);
        }
        if current.epoch_cancel.is_cancelled()
            || current.access_epoch != binding.access_epoch
            || current.members.get(agent) != Some(&binding.member_generation)
        {
            return Err(ProjectError::AccessRevoked);
        }
        if current.status != ProjectStatus::Active {
            return Err(ProjectError::Archived);
        }
        Ok(())
    }
}

fn member_map(record: &ProjectRecord) -> BTreeMap<String, u64> {
    record
        .members
        .iter()
        .map(|member| (member.agent_id.clone(), member.generation))
        .collect()
}

impl ProjectAccessLease {
    pub(crate) fn check(&self) -> Result<(), ProjectError> {
        if self.epoch_cancel.is_cancelled() {
            return Err(ProjectError::AccessRevoked);
        }
        let registry = self
            .registry
            .upgrade()
            .ok_or(ProjectError::AccessUnavailable)?;
        let published = registry
            .published
            .read()
            .expect("project access publication lock poisoned");
        let current = published
            .get(&self.project_id)
            .ok_or(ProjectError::AccessRevoked)?;
        if current.closed_uncertain {
            return Err(ProjectError::AccessUnavailable);
        }
        if current.access_epoch != self.access_epoch
            || current.members.get(&self.requesting_agent_id) != Some(&self.member_generation)
            || current.epoch_cancel.is_cancelled()
        {
            return Err(ProjectError::AccessRevoked);
        }
        if matches!(self.purpose, ProjectAccessPurpose::Run)
            && current.status != ProjectStatus::Active
        {
            return Err(ProjectError::Archived);
        }
        Ok(())
    }

    pub(crate) async fn cancelled(&self) {
        self.epoch_cancel.cancelled().await;
    }

    async fn guard<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
        context: Option<&ModelCallContext>,
    ) -> Result<T, String> {
        self.check().map_err(|error| error.to_string())?;
        futures::pin_mut!(work);
        let result = tokio::select! { biased;
            () = self.cancelled() => {
                if let Some(context) = context { context.request_cancel.cancel(); }
                // Controlled transport retains custody and settles real attempts.
                let _ = work.await;
                return Err(ProjectError::AccessRevoked.to_string());
            }
            result = &mut work => result,
        };
        self.check().map_err(|error| error.to_string())?;
        result
    }
}

pub(crate) fn bind_model_adapter(
    inner: Arc<dyn ModelAdapter>,
    lease: Option<ProjectAccessLease>,
) -> Arc<dyn ModelAdapter> {
    match lease {
        Some(lease) => Arc::new(ProjectModelAdapter { inner, lease }),
        None => inner,
    }
}

struct ProjectModelAdapter {
    inner: Arc<dyn ModelAdapter>,
    lease: ProjectAccessLease,
}

struct ProjectSink<'a> {
    lease: &'a ProjectAccessLease,
    downstream: &'a dyn ModelStreamSink,
}
#[async_trait]
impl ModelStreamSink for ProjectSink<'_> {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        if let Err(error) = self.lease.check() {
            if let ModelStreamFrame::Final(response) = frame {
                self.downstream.complete_usage(response.usage).await?;
            }
            return Err(error.to_string());
        }
        self.downstream.emit(frame).await
    }
    async fn complete_usage(&self, usage: TokenUsage) -> Result<(), String> {
        self.downstream.complete_usage(usage).await
    }
}

#[async_trait]
impl ModelAdapter for ProjectModelAdapter {
    fn provider(&self) -> &str {
        self.inner.provider()
    }
    fn single_attempt_contract(&self) -> bool {
        self.inner.single_attempt_contract()
    }
    fn generated_usage_is_complete(&self, response: &ModelGenerateResponse) -> bool {
        self.inner.generated_usage_is_complete(response)
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.lease
            .guard(self.inner.generate(config, request), None)
            .await
    }
    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let sink = ProjectSink {
            lease: &self.lease,
            downstream: sink,
        };
        self.lease
            .guard(self.inner.stream(config, request, &sink), None)
            .await
    }
    async fn generate_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        self.lease
            .guard(
                self.inner.generate_controlled(config, request, context),
                Some(context),
            )
            .await
    }
    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &ModelCallContext,
    ) -> Result<(), String> {
        let sink = ProjectSink {
            lease: &self.lease,
            downstream: sink,
        };
        self.lease
            .guard(
                self.inner
                    .stream_controlled(config, request, &sink, context),
                Some(context),
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::registry::ProjectCreateRequest;

    fn fixture() -> (ProjectRegistry, ProjectAccessRegistry, ProjectRecord) {
        let mut registry = ProjectRegistry::default();
        let (record, _) = registry
            .create(
                ProjectCreateRequest {
                    request_key: "access".into(),
                    title: "Shared".into(),
                    member_agent_ids: vec!["a".into(), "b".into()],
                    instructions: String::new(),
                },
                1,
            )
            .unwrap();
        let access = ProjectAccessRegistry::default();
        access.publish_records(&registry).unwrap();
        (registry, access, record)
    }

    #[tokio::test]
    async fn selected_account_revocation_stops_old_runs_once_and_keeps_future_healthy_turns_available(
    ) {
        let (registry, access, record) = fixture();
        let old = access.acquire_run(&record, "a").unwrap();
        access.publish_source_revocations(
            &record.id,
            BTreeSet::from(["selected-account-source".into()]),
        );
        assert_eq!(old.check(), Err(ProjectError::AccessRevoked));
        tokio::time::timeout(std::time::Duration::from_millis(100), old.cancelled())
            .await
            .unwrap();
        let healthy = access.acquire_run(&record, "a").unwrap();
        access.publish_source_revocations(
            &record.id,
            BTreeSet::from(["selected-account-source".into()]),
        );
        access.publish_records(&registry).unwrap();
        assert!(
            healthy.check().is_ok(),
            "reconciliation of an already revoked source cannot starve healthy new turns"
        );
        access.publish_source_revocations(&record.id, BTreeSet::new());
        assert_eq!(
            old.check(),
            Err(ProjectError::AccessRevoked),
            "rotation never revives a captured lease"
        );
        access.publish_source_revocations(
            &record.id,
            BTreeSet::from(["another-selected-source".into()]),
        );
        assert_eq!(healthy.check(), Err(ProjectError::AccessRevoked));
    }

    #[tokio::test]
    async fn member_removal_fences_hydration_and_readd_cannot_revive_a_lease() {
        let (mut registry, access, record) = fixture();
        let a = access.acquire_run(&record, "a").unwrap();
        let b = access.acquire_run(&record, "b").unwrap();
        assert!(matches!(
            access.acquire_read(&record, "c"),
            Err(ProjectError::NotMember)
        ));
        let removed = registry
            .replace_members(&record.id, 1, vec!["b".into()], 2)
            .unwrap();
        access.publish_records(&registry).unwrap();
        assert_eq!(a.check(), Err(ProjectError::AccessRevoked));
        assert_eq!(
            b.check(),
            Err(ProjectError::AccessRevoked),
            "remaining member must discard old shared context too"
        );
        tokio::time::timeout(std::time::Duration::from_millis(100), a.cancelled())
            .await
            .unwrap();
        let readded = registry
            .replace_members(
                &record.id,
                removed.revision,
                vec!["a".into(), "b".into()],
                3,
            )
            .unwrap();
        access.publish_records(&registry).unwrap();
        assert!(access.acquire_run(&readded, "a").unwrap().check().is_ok());
        assert_eq!(a.check(), Err(ProjectError::AccessRevoked));
    }

    #[test]
    fn uncertain_publication_stays_closed_and_wrong_registry_cannot_supply_access() {
        let (_, access, record) = fixture();
        let lease = access.acquire_read(&record, "a").unwrap();
        access.fence_uncertain(&record.id);
        assert!(lease.check().is_err());
        assert!(matches!(
            access.acquire_read(&record, "a"),
            Err(ProjectError::AccessUnavailable)
        ));
        assert!(matches!(
            ProjectAccessRegistry::default().acquire_read(&record, "a"),
            Err(ProjectError::AccessUnavailable)
        ));
    }

    #[tokio::test]
    async fn revoked_lease_blocks_a_later_model_attempt_before_transport_dispatch() {
        let (mut registry, access, record) = fixture();
        let lease = access.acquire_run(&record, "b").unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let first = lease
            .guard(
                async {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                },
                None,
            )
            .await;
        assert!(first.is_ok());
        registry
            .replace_members(&record.id, 1, vec!["b".into()], 2)
            .unwrap();
        access.publish_records(&registry).unwrap();
        let retry = lease
            .guard(
                async {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                },
                None,
            )
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn revocation_retains_inflight_transport_custody_until_real_settlement() {
        let (mut registry, access, record) = fixture();
        let lease = access.acquire_run(&record, "b").unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let settle = Arc::new(tokio::sync::Notify::new());
        let settled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = tokio::spawn({
            let started = started.clone();
            let settle = settle.clone();
            let settled = settled.clone();
            async move {
                lease
                    .guard(
                        async {
                            started.notify_one();
                            settle.notified().await;
                            settled.store(true, std::sync::atomic::Ordering::SeqCst);
                            Ok(())
                        },
                        None,
                    )
                    .await
            }
        });
        crate::sessions::test_support::within("transport start", started.notified()).await;
        registry
            .replace_members(&record.id, 1, vec!["b".into()], 2)
            .unwrap();
        access.publish_records(&registry).unwrap();
        tokio::task::yield_now().await;
        assert!(
            !worker.is_finished(),
            "revocation cannot drop custody before the in-flight attempt settles"
        );
        settle.notify_one();
        let result = crate::sessions::test_support::within("transport settlement", worker)
            .await
            .unwrap();
        assert!(result.is_err());
        assert!(settled.load(std::sync::atomic::Ordering::SeqCst));
    }
}
