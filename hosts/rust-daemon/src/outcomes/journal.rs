use crate::{app::SharedDaemonState, runs::RunLink};
use anima_core::outcomes::{OutcomeStatus, RunOutcome};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) async fn publish(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    run: &RunLink,
    expected: u64,
    next: RunOutcome,
) -> Result<(), String> {
    next.validate().map_err(|error| error.to_string())?;
    let state = state.clone();
    let run = run.clone();
    tokio::spawn(async move {
        let _transaction = transactions.lock_owned().await;
        let persist = {
            let mut guard = state.write().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| error.to_string())?;
            let record = guard
                .runs
                .get(&run.run_id)
                .filter(|record| {
                    record.agent_id == run.agent_id && record.session_id == run.session_id
                })
                .ok_or("Outcome run unavailable")?;
            if record.outcome.as_ref().map(|outcome| outcome.revision) != Some(expected) {
                return Err("Outcome generation changed; stale result refused".into());
            }
            if guard.control_plane_store.is_none() {
                return Err("Outcome checks require durable storage".into());
            }
            if !matches!(
                next.status,
                OutcomeStatus::NeedsReview
                    | OutcomeStatus::NeedsInput
                    | OutcomeStatus::Exhausted
                    | OutcomeStatus::Stopped
            ) {
                super::validate_publication(&guard, &run, &next)?;
            }
            guard
                .control_plane_persist_request()
                .with_run_outcome(&run.run_id, next.clone())
                .ok_or("Outcome run absent from snapshot")?
        };
        let saved = persist.save_action_candidate().await;
        let mut guard = state.write().await;
        guard
            .ensure_profile_owned()
            .map_err(|error| error.to_string())?;
        let authority = if matches!(
            next.status,
            OutcomeStatus::NeedsReview
                | OutcomeStatus::NeedsInput
                | OutcomeStatus::Exhausted
                | OutcomeStatus::Stopped
        ) {
            Ok(())
        } else {
            super::validate_publication(&guard, &run, &next)
        };
        let record = guard
            .runs
            .get_mut(&run.run_id)
            .filter(|record| record.agent_id == run.agent_id && record.session_id == run.session_id)
            .ok_or("Outcome run was removed")?;
        if record.outcome.as_ref().map(|outcome| outcome.revision) != Some(expected) {
            return Err("Outcome publication generation changed".into());
        }
        let mut published = next;
        if saved.is_err() || authority.is_err() {
            let _ = published.hold(OutcomeStatus::NeedsReview);
        }
        record.outcome = Some(published);
        record.mirrored = false;
        saved.map_err(|error| {
            format!("Outcome evidence save failed; further effects held: {error}")
        })?;
        authority
    })
    .await
    .map_err(|_| "Outcome persistence worker stopped; inspection required".to_string())?
}
