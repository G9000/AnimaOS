//! Profile ownership fences execution independently of tool approval.
use crate::app::SharedDaemonState;
use anima_core::{CancelCause, CancelSignal};

impl super::AgentRunCoordinator {
    pub(crate) async fn profile_loss_signal(&self) -> CancelSignal {
        self.state
            .read()
            .await
            .profile_owner()
            .map(|owner| owner.lost_signal())
            .unwrap_or_default()
    }
}

pub(super) async fn verify(state: &SharedDaemonState) -> Result<(), String> {
    let owner = {
        let state = state.read().await;
        state.ensure_profile_owned().map_err(|e| e.to_string())?;
        state.profile_owner()
    };
    if let Some(owner) = owner {
        owner.verify_owned().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) struct RunProfileWatch(Option<tokio::task::JoinHandle<()>>);

impl Drop for RunProfileWatch {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

pub(super) async fn watch(state: &SharedDaemonState, cancel: CancelSignal) -> RunProfileWatch {
    let signal = state
        .read()
        .await
        .profile_owner()
        .map(|owner| owner.lost_signal());
    RunProfileWatch(signal.map(|signal| {
        tokio::spawn(async move {
            signal.cancelled().await;
            cancel.cancel_with(CancelCause::ProfileOwnershipLost);
        })
    }))
}
