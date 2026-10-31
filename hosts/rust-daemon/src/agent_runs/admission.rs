//! Room locks, agent slots, and bounded waiting for daemon runs.
//!
//! Registries share ownership across coordinator clones. Leases release both
//! capacity and registry entries on completion, cancellation, and failed admission.

use crate::routes::ApiError;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};

/// Shared resource registries; global permits remain with the coordinator.
#[derive(Clone, Default)]
pub(super) struct RunAdmission {
    session_locks: SessionLockMap,
    agent_slots: AgentSlotMap,
    waiting_budget: WaitingBudgetMap,
}

/// Runs of one agent that fail-fast callers (the legacy run route and
/// Telegram owner sends) may have waiting at once for a room lock, an agent
/// slot, or a global permit (spec §16).
pub(crate) const MAX_QUEUED_RUNS_PER_AGENT: usize = 8;

const SPECIALIST_BUSY: &str = "Specialist is busy";

type SessionLockMap = Arc<StdMutex<HashMap<(String, String), Arc<Mutex<()>>>>>;
type AgentSlotMap = Arc<StdMutex<HashMap<String, Arc<Semaphore>>>>;
/// Units of each agent's waiting budget in use; an agent with none has no entry.
type WaitingBudgetMap = Arc<StdMutex<HashMap<String, usize>>>;

/// One unit of an agent's waiting budget (`MAX_QUEUED_RUNS_PER_AGENT`).
///
/// A fail-fast caller takes it without waiting (`try_take_waiting_unit`)
/// before its run waits for a room lock and an agent slot. The run drops it
/// as soon as it holds its global permit; any other exit (an error, a panic,
/// or the run's task being dropped) drops it too. Nothing ever waits for a
/// unit, so holding one while waiting for a room lock, slot, or permit cannot
/// deadlock, and a permit holder never needs one.
pub(crate) struct WaitingBudgetUnit {
    agent_id: String,
    budget: WaitingBudgetMap,
}

impl Drop for WaitingBudgetUnit {
    fn drop(&mut self) {
        let mut budget = self
            .budget
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(waiting) = budget.get_mut(&self.agent_id) {
            *waiting = waiting.saturating_sub(1);
            if *waiting == 0 {
                budget.remove(&self.agent_id);
            }
        }
    }
}

/// How a run waits for its room lock and agent slot (spec §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmitMode {
    /// Wait in acceptance order (top-level runs).
    Wait,
    /// Fail fast with "Specialist is busy" (nested delegated and peer runs).
    TryNow,
}

/// One room lock of an agent. tokio's mutex serves waiters in FIFO order,
/// which keeps same-room runs in acceptance order.
pub(super) struct SessionLease {
    key: (String, String),
    lock: Option<Arc<Mutex<()>>>,
    guard: Option<OwnedMutexGuard<()>>,
    locks: SessionLockMap,
}

impl SessionLease {
    pub(super) fn room_id(&self) -> &str {
        &self.key.1
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        self.guard.take();
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Take this lease's own reference out of the `Option` (still alive,
        // just moved into `lock`) and let it drop at the end of this `if
        // let` block, still *inside* the critical section, before deciding
        // whether to remove the registry entry. `== 2` means only the
        // registry's copy and this local `lock` remain, i.e. no other lease
        // is alive. Leaving this lease's reference to drop only implicitly,
        // after `drop()` returns, would race another lease's own check here:
        // both could see the pre-decrement count and neither would remove
        // the entry, leaking it (spec: room/slot lease cleanup).
        if let Some(lock) = self.lock.take() {
            if Arc::strong_count(&lock) == 2
                && locks
                    .get(&self.key)
                    .is_some_and(|candidate| Arc::ptr_eq(candidate, &lock))
            {
                locks.remove(&self.key);
            }
        }
    }
}

/// One of an agent's run slots.
pub(super) struct SlotLease {
    agent_id: String,
    slots: Option<Arc<Semaphore>>,
    permit: Option<OwnedSemaphorePermit>,
    registry: AgentSlotMap,
}

impl Drop for SlotLease {
    fn drop(&mut self) {
        self.permit.take();
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // See `SessionLease::drop`: this lease's own reference must drop
        // inside the critical section (at the end of this `if let` block),
        // not be left to the automatic field-drop after this function
        // returns.
        if let Some(slots) = self.slots.take() {
            if Arc::strong_count(&slots) == 2
                && registry
                    .get(&self.agent_id)
                    .is_some_and(|candidate| Arc::ptr_eq(candidate, &slots))
            {
                registry.remove(&self.agent_id);
            }
        }
    }
}

/// A run's room lock and agent slot, held until the run finishes.
pub(crate) struct RunReservation {
    _session: SessionLease,
    _slot: SlotLease,
}

/// A room held by a non-run operation (session deletion); runs for the room
/// wait until it is dropped.
pub(crate) struct RoomReservation {
    _lease: SessionLease,
}

impl WaitingBudgetUnit {
    pub(super) fn belongs_to(&self, agent_id: &str) -> bool {
        self.agent_id == agent_id
    }
}

impl RunReservation {
    pub(super) fn new(session: SessionLease, slot: SlotLease) -> Self {
        Self {
            _session: session,
            _slot: slot,
        }
    }
}

impl RunAdmission {
    /// Advisory: the agent has no free run slot (spec §4.4 item 7).
    pub(super) fn is_agent_busy(&self, agent_id: &str) -> bool {
        self.agent_slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(agent_id)
            .is_some_and(|slots| slots.available_permits() == 0)
    }

    /// Takes the room's run lock without waiting. `None` while a run holds or
    /// waits for it (spec §3.3: deleting a session with a run is 409).
    pub(super) fn try_reserve_room(
        &self,
        agent_id: &str,
        room_id: &str,
    ) -> Option<RoomReservation> {
        let in_use = self
            .session_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&(agent_id.to_string(), room_id.to_string()));
        if in_use {
            return None;
        }
        self.try_session_lease(agent_id, room_id)
            .map(|lease| RoomReservation { _lease: lease })
    }

    /// Takes one unit of the agent's waiting budget without waiting, or
    /// `None` when `MAX_QUEUED_RUNS_PER_AGENT` of its fail-fast runs are
    /// already waiting (spec §16). Only the legacy run route and Telegram
    /// owner sends take units; each answers `None` with its own saturation
    /// response.
    pub(super) fn try_take_waiting_unit(&self, agent_id: &str) -> Option<WaitingBudgetUnit> {
        let mut budget = self
            .waiting_budget
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let waiting = budget.entry(agent_id.to_string()).or_insert(0);
        if *waiting >= MAX_QUEUED_RUNS_PER_AGENT {
            return None;
        }
        *waiting += 1;
        Some(WaitingBudgetUnit {
            agent_id: agent_id.to_string(),
            budget: Arc::clone(&self.waiting_budget),
        })
    }

    /// `admit`, with `held`, the room's lease when the caller already took
    /// it (S2-B: a session queue's, handed to the run it picked).
    pub(super) async fn admit_holding(
        &self,
        agent_id: &str,
        room_key: &str,
        capacity: usize,
        mode: AdmitMode,
        held: Option<SessionLease>,
    ) -> Result<RunReservation, ApiError> {
        let key = (agent_id.to_string(), room_key.to_string());
        let session = match held {
            Some(lease) if lease.key == key => lease,
            held => {
                // Any other lease goes back before this waits for a room.
                drop(held);
                match mode {
                    AdmitMode::Wait => self.wait_session_lease(agent_id, room_key).await,
                    AdmitMode::TryNow => self
                        .try_session_lease(agent_id, room_key)
                        .ok_or_else(|| ApiError::service_unavailable(SPECIALIST_BUSY))?,
                }
            }
        };
        let slot = match mode {
            AdmitMode::Wait => self.wait_slot_lease(agent_id, capacity).await?,
            AdmitMode::TryNow => self
                .try_slot_lease(agent_id, capacity)
                .ok_or_else(|| ApiError::service_unavailable(SPECIALIST_BUSY))?,
        };
        Ok(RunReservation {
            _session: session,
            _slot: slot,
        })
    }

    fn session_lock(&self, agent_id: &str, room_id: &str) -> ((String, String), Arc<Mutex<()>>) {
        let key = (agent_id.to_string(), room_id.to_string());
        let lock = self
            .session_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        (key, lock)
    }

    pub(super) fn try_session_lease(&self, agent_id: &str, room_id: &str) -> Option<SessionLease> {
        let (key, lock) = self.session_lock(agent_id, room_id);
        let guard = Arc::clone(&lock).try_lock_owned().ok();
        let lease = SessionLease {
            key,
            lock: Some(lock),
            guard,
            locks: Arc::clone(&self.session_locks),
        };
        lease.guard.is_some().then_some(lease)
    }

    pub(super) async fn wait_session_lease(&self, agent_id: &str, room_id: &str) -> SessionLease {
        let (key, lock) = self.session_lock(agent_id, room_id);
        let mut lease = SessionLease {
            key,
            lock: Some(Arc::clone(&lock)),
            guard: None,
            locks: Arc::clone(&self.session_locks),
        };
        lease.guard = Some(lock.lock_owned().await);
        lease
    }

    fn agent_slot_semaphore(&self, agent_id: &str, capacity: usize) -> Arc<Semaphore> {
        self.agent_slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(agent_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(capacity)))
            .clone()
    }

    pub(super) fn try_slot_lease(&self, agent_id: &str, capacity: usize) -> Option<SlotLease> {
        let slots = self.agent_slot_semaphore(agent_id, capacity);
        let permit = Arc::clone(&slots).try_acquire_owned().ok();
        let lease = SlotLease {
            agent_id: agent_id.to_string(),
            slots: Some(slots),
            permit,
            registry: Arc::clone(&self.agent_slots),
        };
        lease.permit.is_some().then_some(lease)
    }

    async fn wait_slot_lease(
        &self,
        agent_id: &str,
        capacity: usize,
    ) -> Result<SlotLease, ApiError> {
        let slots = self.agent_slot_semaphore(agent_id, capacity);
        let mut lease = SlotLease {
            agent_id: agent_id.to_string(),
            slots: Some(Arc::clone(&slots)),
            permit: None,
            registry: Arc::clone(&self.agent_slots),
        };
        lease.permit =
            Some(slots.acquire_owned().await.map_err(|_| {
                ApiError::service_unavailable("agent run admission is unavailable")
            })?);
        Ok(lease)
    }

    /// Units of the agent's waiting budget in use.
    #[cfg(test)]
    pub(super) fn waiting_runs(&self, agent_id: &str) -> usize {
        self.waiting_budget
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(agent_id)
            .copied()
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub(super) fn lock_counts(&self) -> (usize, usize) {
        (
            self.session_locks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            self.agent_slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
        )
    }
}

#[cfg(test)]
mod tests;
