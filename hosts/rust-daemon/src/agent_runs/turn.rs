//! Awaited publication of turn progress. The save worker owns data, never dispatch.
use crate::{
    app::SharedDaemonState,
    runs::{RunLink, RunStatus},
    state::DaemonState,
};
use anima_core::{
    turn_recovery::{TurnCheckpoint, TurnCheckpointSink, TurnPhase},
    CancelSignal,
};
use async_trait::async_trait;
use std::sync::{Arc, Weak};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub(super) struct HostTurnCheckpointSink {
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    run: RunLink,
    cancel: CancelSignal,
}

impl HostTurnCheckpointSink {
    pub(super) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        run: RunLink,
        cancel: CancelSignal,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            transactions,
            run,
            cancel,
        }
    }

    fn check<'a>(
        &self,
        state: &'a DaemonState,
        phase: TurnPhase,
    ) -> Result<&'a crate::runs::RunRecord, String> {
        state.ensure_profile_owned().map_err(|e| e.to_string())?;
        let record = state
            .runs
            .get(&self.run.run_id)
            .filter(|r| r.agent_id == self.run.agent_id && r.session_id == self.run.session_id)
            .ok_or("Turn owner no longer exists")?;
        if record.stop.is_some() {
            self.cancel.cancel();
        }
        if self.cancel.is_cancelled() {
            return Err("Turn stopped before checkpoint admission".into());
        }
        let settling = phase != TurnPhase::BeforeModel;
        if record.status != RunStatus::Running
            && !(settling && record.status == RunStatus::AwaitingApproval)
        {
            return Err("Turn is no longer running".into());
        }
        crate::runs::turn::active(state, record)?;
        Ok(record)
    }

    async fn persist(&self, checkpoint: TurnCheckpoint) -> Result<(), String> {
        let state = self.state.upgrade().ok_or("Turn owner is unavailable")?;
        super::ownership::verify(&state).await?;
        let _transaction =
            tokio::time::timeout(std::time::Duration::from_secs(15), self.transactions.lock())
                .await
                .map_err(|_| "Turn checkpoint admission timed out")?;
        for _ in 0..8 {
            let (candidate, persist) = {
                let mut guard = state.write().await;
                let record = self.check(&guard, checkpoint.phase)?;
                let run_id = record.id.clone();
                let mut candidate = record.turn.clone().ok_or("Turn binding unavailable")?;
                checkpoint.validate_after(&candidate.binding, candidate.checkpoint.as_ref())?;
                candidate.checkpoint = Some(checkpoint.clone());
                let persist = guard
                    .control_plane_persist_request()
                    .with_run_turn(&run_id, candidate.clone())
                    .ok_or("Turn missing from persistence snapshot")?;
                (candidate, persist)
            };
            if let Err(error) = persist.save_action_candidate().await {
                if crate::state::is_superseded_control_plane_save(&error) {
                    continue;
                }
                return Err(format!(
                    "Turn checkpoint could not be saved; further dispatch stopped: {error}"
                ));
            }
            let mut guard = state.write().await;
            // Never publish a stale candidate over a deleted/settled/stopped run.
            self.check(&guard, checkpoint.phase)?;
            let record = guard
                .runs
                .get_mut(&self.run.run_id)
                .ok_or("Turn was deleted")?;
            record.turn = Some(candidate);
            record.mirrored = false;
            return Ok(());
        }
        Err("Turn checkpoint could not acquire current persistence ordering".into())
    }
}

#[async_trait]
impl TurnCheckpointSink for HostTurnCheckpointSink {
    async fn save(&self, checkpoint: TurnCheckpoint) -> Result<(), String> {
        let sink = self.clone();
        // Cancellation of a caller must not abandon a partially acknowledged write.
        tokio::spawn(async move { sink.persist(checkpoint).await })
            .await
            .map_err(|_| "Turn checkpoint save worker stopped".to_string())?
    }
}
