use super::records::*;
use crate::agent_runs::AgentRunCoordinator;
use crate::app::SharedDaemonState;
use crate::learning::LearningMode;
use crate::memory_store::MemoryMutation;
use crate::routes::ApiError;
use anima_memory::MemoryConsolidationState;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{Mutex, Notify};

pub(super) fn now_ms() -> u64 {
    anima_core::primitives::now_millis()
}
fn unavailable() -> ApiError {
    ApiError::service_unavailable("Memory maintenance persistence is unavailable or uncertain; inspect the operation before retrying")
}
fn conflict() -> ApiError {
    ApiError::conflict("Memory maintenance authority, sources or operation revision changed; refresh before deciding")
}

#[derive(Clone)]
pub(crate) struct MaintenanceService {
    pub(super) state: SharedDaemonState,
    pub(super) transactions: Arc<Mutex<()>>,
    pub(super) coordinator: AgentRunCoordinator,
}
impl MaintenanceService {
    pub(crate) fn new(state: SharedDaemonState, coordinator: AgentRunCoordinator) -> Self {
        Self {
            state,
            transactions: coordinator.control_plane_transactions(),
            coordinator,
        }
    }
    pub(crate) async fn policy(&self, agent_id: &str) -> Result<MaintenancePolicy, ApiError> {
        let guard = self.state.read().await;
        if !guard.agents.contains_key(agent_id) {
            return Err(ApiError::not_found());
        }
        Ok(guard
            .memory_maintenance
            .policies
            .get(agent_id)
            .cloned()
            .unwrap_or_default())
    }
    pub(crate) async fn set_policy(
        &self,
        agent_id: &str,
        enabled: bool,
        expected_revision: u64,
    ) -> Result<MaintenancePolicy, ApiError> {
        let _transaction = self.transactions.lock().await;
        let mut guard = self.state.write().await;
        if !guard.agents.contains_key(agent_id) {
            return Err(ApiError::not_found());
        }
        if guard.control_plane_store.is_none()
            || guard.memory_store_config().is_none()
            || guard.learning_slots.is_closed()
        {
            return Err(unavailable());
        }
        let previous = guard.memory_maintenance.clone();
        let policy = guard
            .memory_maintenance
            .policies
            .entry(agent_id.into())
            .or_default();
        if policy.revision != expected_revision {
            return Err(conflict());
        }
        policy.revision = policy.revision.checked_add(1).ok_or_else(conflict)?;
        policy.enabled = enabled;
        // Enabling does not reset an existing cadence or refund daily reservations.
        let result = policy.clone();
        if !enabled {
            for op in guard
                .memory_maintenance
                .operations
                .values_mut()
                .filter(|o| {
                    o.agent_id == agent_id
                        && matches!(
                            o.state,
                            MaintenanceState::Reviewing | MaintenanceState::Pending
                        )
                })
            {
                op.finish(MaintenanceState::Dismissed, "Maintenance disabled by owner");
            }
        }
        let persist = guard.control_plane_persist_request();
        drop(guard);
        if persist.save().await.is_err() {
            // A failed disable stays disabled in memory. An uncertain enable never admits work.
            let mut guard = self.state.write().await;
            if enabled {
                guard.memory_maintenance = previous;
            }
            if let Some(policy) = guard.memory_maintenance.policies.get_mut(agent_id) {
                policy.enabled = false;
            }
            return Err(unavailable());
        }
        Ok(result)
    }
    pub(crate) async fn list(&self, agent_id: &str) -> Result<Vec<MaintenanceOperation>, ApiError> {
        let guard = self.state.read().await;
        if !guard.agents.contains_key(agent_id) {
            return Err(ApiError::not_found());
        }
        let mut rows: Vec<_> = guard
            .memory_maintenance
            .operations
            .values()
            .filter(|o| o.agent_id == agent_id)
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            b.reserved_at_ms
                .cmp(&a.reserved_at_ms)
                .then(a.id.cmp(&b.id))
        });
        Ok(rows)
    }
    /// Only owner routes call this; tags and tool-written memories never call it.
    pub(crate) async fn approve_sources(
        &self,
        agent_id: &str,
        ids: &[String],
    ) -> Result<(), ApiError> {
        if ids.is_empty() || ids.len() > 8 {
            return Err(ApiError::bad_request_static(
                "Choose one to eight memory sources",
            ));
        }
        let _transaction = self.transactions.lock().await;
        let memory = {
            let guard = self.state.read().await;
            if !guard.agents.contains_key(agent_id) {
                return Err(ApiError::not_found());
            }
            if guard.control_plane_store.is_none() || guard.memory_store_config().is_none() {
                return Err(unavailable());
            }
            guard.memory_handle()
        };
        let sources: Vec<_> = {
            let memory = memory.read().await;
            ids.iter()
                .map(|id| {
                    memory
                        .get(id)
                        .filter(|m| m.agent_id == agent_id && eligible(m))
                        .ok_or_else(conflict)
                })
                .collect::<Result<_, _>>()?
        };
        let mut guard = self.state.write().await;
        let previous = guard.memory_maintenance.clone();
        for source in &sources {
            guard
                .memory_maintenance
                .trust(source)
                .map_err(|_| conflict())?;
        }
        let persist = guard.control_plane_persist_request();
        drop(guard);
        if persist.save().await.is_err() {
            self.state.write().await.memory_maintenance = previous;
            return Err(unavailable());
        }
        Ok(())
    }
    /// Reserve before dispatch. The returned row is a durable attempt, not a success claim.
    pub(crate) async fn request_review(
        &self,
        agent_id: &str,
        due_only: bool,
    ) -> Result<MaintenanceOperation, ApiError> {
        self.request_sources_review(agent_id, due_only, &[]).await
    }
    pub(crate) async fn request_sources_review(
        &self,
        agent_id: &str,
        due_only: bool,
        source_ids: &[String],
    ) -> Result<MaintenanceOperation, ApiError> {
        if source_ids.len() > 8 || (due_only && !source_ids.is_empty()) {
            return Err(ApiError::bad_request_static(
                "Manual source reviews accept at most eight sources",
            ));
        }
        let _transaction = self.transactions.lock().await;
        if self.coordinator.is_draining() || self.coordinator.is_shutting_down() {
            return Err(unavailable());
        }
        let (memory, registry, policy, config, adapter, slots, free) = {
            let guard = self.state.read().await;
            let runtime = guard.agents.get(agent_id).ok_or_else(ApiError::not_found)?;
            if crate::agent_runs::is_helper_config(runtime.config()) {
                return Err(conflict());
            }
            let policy = guard
                .memory_maintenance
                .policies
                .get(agent_id)
                .cloned()
                .unwrap_or_default();
            if !policy.enabled
                || (due_only && now_ms() < policy.next_due_ms)
                || !guard.memory_maintenance.can_reserve(agent_id, now_ms())
            {
                return Err(conflict());
            }
            if guard.control_plane_store.is_none() || guard.memory_store_config().is_none() {
                return Err(unavailable());
            }
            (
                guard.memory_handle(),
                MaintenanceRegistry {
                    sources: guard.memory_maintenance.sources.clone(),
                    ..Default::default()
                },
                policy,
                runtime.config().clone(),
                guard.profile_model_adapter(),
                Arc::clone(&guard.learning_slots),
                guard.learning_mode(agent_id) == LearningMode::Free,
            )
        };
        let permit = slots.try_acquire_owned().map_err(|_| {
            ApiError::conflict("A physical memory or learning review is still active")
        })?;
        let cohort = {
            let memory = memory.read().await;
            let sources: Vec<_> = if source_ids.is_empty() {
                registry
                    .sources
                    .keys()
                    .filter_map(|id| memory.get(id))
                    .filter(|m| m.agent_id == agent_id && eligible(m) && registry.trusted(m))
                    .collect()
            } else {
                if source_ids
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != source_ids.len()
                {
                    return Err(conflict());
                }
                source_ids
                    .iter()
                    .map(|id| {
                        memory
                            .get(id)
                            .filter(|m| m.agent_id == agent_id && eligible(m))
                            .ok_or_else(conflict)
                    })
                    .collect::<Result<_, _>>()?
            };
            let cohort = select_cohort_by(sources, |m| {
                registry
                    .sources
                    .get(&m.id)
                    .map_or(0, |s| s.last_reviewed_ms)
            });
            if cohort.len() < 2 {
                return Err(conflict());
            }
            if !source_ids.is_empty() && cohort.len() != source_ids.len() {
                return Err(conflict());
            }
            cohort
        };
        let trusted = cohort.iter().all(|m| registry.trusted(m));
        let row = MaintenanceOperation {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            revision: 1,
            policy_revision: policy.revision,
            reserved_at_ms: now_ms(),
            state: MaintenanceState::Reviewing,
            sources_before: cohort.iter().map(MemoryImage::from).collect(),
            sources_after: vec![],
            summary: None,
            reason: "Bounded source review reserved".into(),
            automatic_apply: free && trusted,
        };
        let mut guard = self.state.write().await;
        guard
            .memory_maintenance
            .policies
            .get_mut(agent_id)
            .ok_or_else(conflict)?
            .next_due_ms = now_ms().saturating_add(CADENCE_MS);
        for source in &cohort {
            if let Some(approval) = guard.memory_maintenance.sources.get_mut(&source.id) {
                approval.last_reviewed_ms = row.reserved_at_ms;
            }
        }
        guard
            .memory_maintenance
            .operations
            .insert(row.id.clone(), row.clone());
        let persist = guard.control_plane_persist_request();
        drop(guard);
        if persist.save().await.is_err() {
            // Do not dispatch after a failed reservation. Preserve the consumed attempt
            // in memory because the store may already contain it.
            let mut guard = self.state.write().await;
            if let Some(op) = guard.memory_maintenance.operations.get_mut(&row.id) {
                op.finish(
                    MaintenanceState::Interrupted,
                    "Reservation durability uncertain; no model dispatched",
                );
            }
            return Err(unavailable());
        }
        let service = self.clone();
        let work = row.clone();
        tokio::spawn(async move {
            service.review(work, config, adapter, permit).await;
        });
        Ok(row)
    }
    pub(crate) async fn decide(
        &self,
        agent_id: &str,
        id: &str,
        revision: u64,
        decision: MaintenanceDecision,
    ) -> Result<MaintenanceOperation, ApiError> {
        let _transaction = self.transactions.lock().await;
        if matches!(decision, MaintenanceDecision::Accept) {
            return self.apply_locked(agent_id, id, revision, false).await;
        }
        let mut guard = self.state.write().await;
        let row = guard
            .memory_maintenance
            .operations
            .get_mut(id)
            .filter(|o| o.agent_id == agent_id)
            .ok_or_else(ApiError::not_found)?;
        if row.revision != revision
            || !matches!(
                row.state,
                MaintenanceState::Pending
                    | MaintenanceState::Reviewing
                    | MaintenanceState::NeedsReview
            )
        {
            return Err(conflict());
        }
        row.finish(MaintenanceState::Dismissed, "Dismissed by owner");
        let result = row.clone();
        let persist = guard.control_plane_persist_request();
        drop(guard);
        persist.save().await.map_err(|_| unavailable())?;
        Ok(result)
    }
    pub(super) async fn apply_locked(
        &self,
        agent_id: &str,
        id: &str,
        revision: u64,
        automatic: bool,
    ) -> Result<MaintenanceOperation, ApiError> {
        if self.coordinator.is_draining() || self.coordinator.is_shutting_down() {
            return Err(unavailable());
        }
        let (row, memory, embeddings, store) = {
            let mut guard = self.state.write().await;
            if !guard.agents.contains_key(agent_id) {
                return Err(ApiError::not_found());
            }
            let row = guard
                .memory_maintenance
                .operations
                .get(id)
                .filter(|o| o.agent_id == agent_id)
                .cloned()
                .ok_or_else(ApiError::not_found)?;
            if row.revision != revision
                || row.state != MaintenanceState::Pending
                || !guard
                    .memory_maintenance
                    .policies
                    .get(agent_id)
                    .is_some_and(|p| p.enabled && p.revision == row.policy_revision)
                || (automatic
                    && (!row.automatic_apply
                        || guard.learning_mode(agent_id) != LearningMode::Free))
                || (automatic
                    && row.sources_before.iter().any(|m| {
                        m.memory().is_err()
                            || !guard.memory_maintenance.trusted(&m.memory().unwrap())
                    }))
            {
                return Err(conflict());
            }
            row.prepared().map_err(|_| conflict())?;
            if guard.memory_store_config().is_none() || guard.control_plane_store.is_none() {
                return Err(unavailable());
            }
            guard
                .memory_maintenance
                .operations
                .get_mut(id)
                .unwrap()
                .finish(
                    MaintenanceState::PreparedApply,
                    "Apply intent saved before memory mutation",
                );
            let persist = guard.control_plane_persist_request();
            let stores = (
                row,
                guard.memory_handle(),
                guard.memory_embeddings_handle(),
                guard.memory_store_config(),
            );
            drop(guard);
            persist.save().await.map_err(|_| unavailable())?;
            stores
        };
        let prepared = row.prepared().map_err(|_| conflict())?;
        {
            let mut memory = memory.write().await;
            let mut mutation = MemoryMutation::new(&mut memory);
            if mutation
                .apply_fact_consolidation(&prepared, eligible)
                .is_err()
            {
                drop(mutation);
                drop(memory);
                self.finish(
                    id,
                    MaintenanceState::NeedsReview,
                    "Source changed before memory commit; no mutation applied",
                )
                .await?;
                return Err(conflict());
            }
            mutation
                .persist(store.as_ref())
                .await
                .map_err(|_| unavailable())?;
            drop(mutation);
            let mut embeddings = embeddings.write().await;
            for image in row.sources_after.iter().chain(row.summary.iter()) {
                if let Some(current) = memory.get(&image.id) {
                    let _ = embeddings.upsert_memory(&current);
                }
            }
        }
        self.finish(
            id,
            MaintenanceState::Applied,
            "Consolidated source facts; originals retained as reflections",
        )
        .await
    }
    pub(crate) async fn undo(
        &self,
        agent_id: &str,
        id: &str,
        revision: u64,
    ) -> Result<MaintenanceOperation, ApiError> {
        let _transaction = self.transactions.lock().await;
        let (row, memory, embeddings, store) = {
            let mut guard = self.state.write().await;
            if !guard.agents.contains_key(agent_id) {
                return Err(ApiError::not_found());
            }
            let row = guard
                .memory_maintenance
                .operations
                .get(id)
                .filter(|o| o.agent_id == agent_id)
                .cloned()
                .ok_or_else(ApiError::not_found)?;
            if row.revision != revision || row.state != MaintenanceState::Applied {
                return Err(conflict());
            }
            if guard.memory_store_config().is_none() || guard.control_plane_store.is_none() {
                return Err(unavailable());
            }
            guard
                .memory_maintenance
                .operations
                .get_mut(id)
                .unwrap()
                .finish(
                    MaintenanceState::PreparedUndo,
                    "Undo intent saved before memory mutation",
                );
            let persist = guard.control_plane_persist_request();
            let stores = (
                row,
                guard.memory_handle(),
                guard.memory_embeddings_handle(),
                guard.memory_store_config(),
            );
            drop(guard);
            persist.save().await.map_err(|_| unavailable())?;
            stores
        };
        let prepared = row.prepared().map_err(|_| conflict())?;
        {
            let mut memory = memory.write().await;
            let mut mutation = MemoryMutation::new(&mut memory);
            // Archived sources are Reflections, so this protection predicate must
            // not require Fact; the core checks the full journal images itself.
            if mutation
                .undo_fact_consolidation(&prepared, |m| !core_tagged(m))
                .is_err()
            {
                drop(mutation);
                drop(memory);
                self.finish(
                    id,
                    MaintenanceState::Applied,
                    "Undo refused because a source, summary or citation changed",
                )
                .await?;
                return Err(conflict());
            }
            mutation
                .persist(store.as_ref())
                .await
                .map_err(|_| unavailable())?;
            drop(mutation);
            let mut embeddings = embeddings.write().await;
            for image in &row.sources_before {
                if let Some(current) = memory.get(&image.id) {
                    let _ = embeddings.upsert_memory(&current);
                }
            }
            let _ = embeddings.remove_memories(&[prepared.summary().id.clone()]);
        }
        self.finish(
            id,
            MaintenanceState::Undone,
            "Only this consolidation was reversed",
        )
        .await
    }
    pub(super) async fn finish(
        &self,
        id: &str,
        state: MaintenanceState,
        reason: &str,
    ) -> Result<MaintenanceOperation, ApiError> {
        let mut guard = self.state.write().await;
        let row = guard
            .memory_maintenance
            .operations
            .get_mut(id)
            .ok_or_else(ApiError::not_found)?;
        row.finish(state, reason);
        let result = row.clone();
        let persist = guard.control_plane_persist_request();
        drop(guard);
        persist.save().await.map_err(|_| unavailable())?;
        Ok(result)
    }
    /// No replay: compare only this cohort after BOTH stores have loaded.
    pub(crate) async fn recover(&self) -> Result<(), ApiError> {
        let _transaction = self.transactions.lock().await;
        let (memory, rows) = {
            let guard = self.state.read().await;
            (
                guard.memory_handle(),
                guard
                    .memory_maintenance
                    .operations
                    .values()
                    .filter(|o| {
                        matches!(
                            o.state,
                            MaintenanceState::Reviewing
                                | MaintenanceState::PreparedApply
                                | MaintenanceState::PreparedUndo
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        for row in rows {
            let state = if row.state == MaintenanceState::Reviewing {
                MaintenanceState::Interrupted
            } else {
                let plan = row.prepared().map_err(|_| conflict())?;
                match (
                    row.state,
                    memory.read().await.fact_consolidation_state(&plan),
                ) {
                    (MaintenanceState::PreparedApply, MemoryConsolidationState::Before) => {
                        MaintenanceState::Interrupted
                    }
                    (MaintenanceState::PreparedApply, MemoryConsolidationState::After) => {
                        MaintenanceState::Applied
                    }
                    (MaintenanceState::PreparedUndo, MemoryConsolidationState::Before) => {
                        MaintenanceState::Undone
                    }
                    (MaintenanceState::PreparedUndo, MemoryConsolidationState::After) => {
                        MaintenanceState::Applied
                    }
                    _ => MaintenanceState::NeedsReview,
                }
            };
            self.finish(
                &row.id,
                state,
                "Restart reconciled exact journal images without replay",
            )
            .await?;
        }
        Ok(())
    }
    pub(crate) fn start(&self) -> MaintenanceWorker {
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Notify::new());
        let worker_stop = stop.clone();
        let worker_wake = wake.clone();
        let service = self.clone();
        let task = tokio::spawn(async move {
            loop {
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                let agents: Vec<_> = service
                    .state
                    .read()
                    .await
                    .memory_maintenance
                    .policies
                    .iter()
                    .filter(|(_, p)| p.enabled && now_ms() >= p.next_due_ms)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in agents {
                    if worker_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let _ = service.request_review(&id, true).await;
                }
                tokio::select! { _ = worker_wake.notified() => {}, _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {} }
            }
        });
        MaintenanceWorker {
            stop,
            wake,
            task: Mutex::new(Some(task)),
            service: self.clone(),
        }
    }
}

pub(crate) struct MaintenanceWorker {
    stop: Arc<AtomicBool>,
    wake: Arc<Notify>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    service: MaintenanceService,
}
impl MaintenanceWorker {
    pub(crate) fn request_shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
    pub(crate) async fn shutdown(&self) {
        self.request_shutdown();
        if let Some(task) = self.task.lock().await.take() {
            let _ = task.await;
        }
        // Request futures keep physical ownership after facade timeout/cancel.
        // The server's single shutdown deadline bounds this drain.
        while self
            .service
            .state
            .read()
            .await
            .learning_slots
            .available_permits()
            == 0
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}
impl Drop for MaintenanceWorker {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}
