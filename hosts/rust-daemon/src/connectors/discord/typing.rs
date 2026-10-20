//! Ephemeral typing follows actual running work, never queued or already stopped runs.
use super::*;
use crate::runs::RunStatus;
use futures::future::BoxFuture;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) struct TypingWorker {
    run_id: String,
    cancel: CancellationToken,
    join: JoinHandle<()>,
}

pub(super) struct TypingGuard(CancellationToken);
impl Drop for TypingGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(PartialEq)]
enum Activity {
    Waiting,
    Running,
    Stopped,
}

impl DiscordManager {
    pub(super) async fn start_typing(
        &self,
        c: &DiscordConnectorRecord,
        run_id: &str,
        epoch: u64,
    ) -> Option<TypingGuard> {
        let token = self.load_token(c).await.ok()?;
        let mut workers = self.typing_workers.lock().await;
        if workers
            .get(&c.id)
            .is_some_and(|worker| !worker.join.is_finished())
        {
            return None;
        }
        if let Some(worker) = workers.remove(&c.id) {
            let _ = worker.join.await;
        }
        let cancel = CancellationToken::new();
        let manager = self.worker_context();
        let current = c.clone();
        let run = run_id.to_owned();
        let stopped = cancel.clone();
        // No await from this epoch check through registration: stop_worker revokes first,
        // then drains the same worker map before allowing credential mutation.
        let authorities = self.authorities.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_closing()
            || authorities
                .get(&c.id)
                .is_none_or(|authority| authority.epoch != epoch)
        {
            return None;
        }
        let join = tokio::spawn(async move {
            manager
                .typing_loop(current, run, epoch, token, stopped)
                .await;
        });
        workers.insert(
            c.id.clone(),
            TypingWorker {
                run_id: run_id.into(),
                cancel: cancel.clone(),
                join,
            },
        );
        Some(TypingGuard(cancel))
    }

    async fn typing_activity(
        &self,
        c: &DiscordConnectorRecord,
        run_id: &str,
        epoch: u64,
    ) -> Activity {
        let state = self.state.read().await;
        if self.is_closing()
            || !self.authority_current(&c.id, epoch)
            || !state
                .discord_connectors
                .get(&c.id)
                .is_some_and(|current| current.matches_destination(c))
        {
            return Activity::Stopped;
        }
        match state.runs.get(run_id) {
            Some(run)
                if run.agent_id == c.agent_id
                    && run.session_id == c.room_id
                    && run.status == RunStatus::Running
                    && run.stop.is_none() =>
            {
                Activity::Running
            }
            Some(run)
                if run.agent_id == c.agent_id
                    && run.session_id == c.room_id
                    && !run.status.is_terminal()
                    && run.stop.is_none() =>
            {
                Activity::Waiting
            }
            _ => Activity::Stopped,
        }
    }

    async fn typing_loop(
        &self,
        c: DiscordConnectorRecord,
        run: String,
        epoch: u64,
        token: DiscordBotToken,
        cancel: CancellationToken,
    ) {
        let Ok(transport) = self.native() else {
            return;
        };
        let mut close = self.close.clone();
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut due = tokio::time::Instant::now();
        let mut post: Option<BoxFuture<'_, Result<(), DiscordTransportError>>> = None;
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                _ = close.changed() => break,
                _ = tick.tick() => {
                    match self.typing_activity(&c, &run, epoch).await {
                        Activity::Stopped => break,
                        Activity::Waiting => {},
                        Activity::Running if post.is_none() && tokio::time::Instant::now() >= due => {
                            post = Some(Box::pin(transport.send_typing(&token, &c.channel.id)));
                        }
                        _ => {},
                    }
                }
                result = async { post.as_mut().expect("guarded request").await }, if post.is_some() => {
                    post = None;
                    let delay = match result {
                        Ok(()) => Duration::from_secs(8),
                        Err(DiscordTransportError::RateLimited { retry_after_ms, .. }) => Duration::from_millis(retry_after_ms.max(1000)),
                        // Typing is best effort and never changes outbox certainty or retries a message.
                        Err(_) => break,
                    };
                    let Some(next) = tokio::time::Instant::now().checked_add(delay) else {
                        break;
                    };
                    due = next;
                }
            }
        }
    }

    pub(super) async fn stop_typing_run(&self, id: &str, run_id: &str) {
        let worker = {
            let mut workers = self.typing_workers.lock().await;
            if workers.get(id).is_none_or(|worker| worker.run_id != run_id) {
                return;
            }
            workers.remove(id)
        };
        if let Some(worker) = worker {
            worker.cancel.cancel();
            let _ = worker.join.await;
        }
    }

    pub(super) async fn stop_typing_connector(&self, id: &str) {
        let worker = self.typing_workers.lock().await.remove(id);
        if let Some(worker) = worker {
            worker.cancel.cancel();
            let _ = worker.join.await;
        }
    }

    pub(super) async fn drain_typing(&self) {
        let workers = self
            .typing_workers
            .lock()
            .await
            .drain()
            .map(|(_, worker)| worker)
            .collect::<Vec<_>>();
        for worker in &workers {
            worker.cancel.cancel();
        }
        for worker in workers {
            let _ = worker.join.await;
        }
    }
}
