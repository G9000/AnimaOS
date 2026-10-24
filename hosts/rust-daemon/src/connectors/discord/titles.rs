//! Hermes-style placeholder → semantic title lifecycle; see HERMES-NOTICE.
use super::*;

impl DiscordManager {
    pub(super) fn schedule_thread_title(
        &self,
        c: &DiscordConnectorRecord,
        run: &str,
        first: String,
        reply: String,
        initial_name: String,
        epoch: u64,
    ) -> Result<(), DiscordManagerError> {
        // Register ownership synchronously so shutdown cannot miss the background task.
        self.ensure_open()?;
        self.owned_count.fetch_add(1, Ordering::SeqCst);
        let guard = OwnedBody {
            count: self.owned_count.clone(),
            notify: self.owned_notify.clone(),
        };
        let manager = self.clone();
        let c = c.clone();
        let run = run.to_owned();
        tokio::spawn(async move {
            let _guard = guard;
            let mut close = manager.close.clone();
            tokio::select! {
                biased;
                _ = close.changed() => {},
                result = manager.title_thread(&c, &run, &first, &reply, &initial_name, epoch) => {
                    if let Err(error) = result {
                        tracing::warn!(connector_id = %c.id, error = %error, "could not title Discord thread");
                    }
                }
            }
        });
        Ok(())
    }

    async fn title_thread(
        &self,
        c: &DiscordConnectorRecord,
        run: &str,
        first: &str,
        reply: &str,
        initial_name: &str,
        epoch: u64,
    ) -> Result<(), DiscordManagerError> {
        if !self.authority_current(&c.id, epoch) {
            return Ok(());
        }
        let Some(title) = self
            .runs
            .generate_conversation_title(&c.agent_id, &c.room_id, run, first, reply)
            .await
        else {
            return Ok(());
        };
        let title = text::thread_title(&title);
        let title_lock = self.title_lock(c);
        let _title = title_lock.lock().await;
        // A separate lifecycle claim lets reply delivery proceed during title I/O,
        // while credential changes still drain all in-flight native effects.
        let Some(_claim) = self.claim_title(&c.id, epoch) else {
            return Ok(());
        };
        let current = self.get(&c.agent_id, &c.id).await?;
        if !current.matches_destination(c)
            || !current.conversations.iter().any(|v| {
                v.room_id == c.room_id
                    && v.auto_title
                        .as_ref()
                        .is_some_and(|v| v.initial_name == initial_name)
            })
        {
            return Ok(());
        }
        let token = self.load_token(c).await?;
        let remote = self
            .native()?
            .get_channel(&token, &c.channel.id)
            .await
            .map_err(transport_error)?;
        // Discord has no conditional PATCH; use Hermes's best-effort current-name guard.
        if !remote.routing_eq(&c.channel)
            || remote.kind != DiscordChannelKind::GuildThread
            || remote.name.as_deref() != Some(initial_name)
            || !self.authority_current(&c.id, epoch)
        {
            return Ok(());
        }
        let renamed = self
            .native()?
            .rename_thread(&token, &c.channel.id, &title)
            .await
            .map_err(transport_error)?;
        if !renamed.routing_eq(&c.channel) || renamed.name.as_deref() != Some(&title) {
            return Err(DiscordManagerError::Conflict);
        }
        // Refresh only the destination metadata, preserving intake/configuration changes
        // made while the network request was in flight.
        let _txn = self.transaction.lock().await;
        let (previous, persist) = {
            let mut state = self.state.write().await;
            if !self.authority_current(&c.id, epoch) {
                return Ok(());
            }
            let Some(current) = state
                .discord_connectors
                .get_mut(&c.id)
                .filter(|v| v.matches_destination(c))
            else {
                return Ok(());
            };
            let previous = current.clone();
            if let Some(conversation) = current
                .conversations
                .iter_mut()
                .find(|v| v.room_id == c.room_id)
            {
                conversation.channel.name = renamed.name;
            }
            (previous, state.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            restore_metadata(self, previous).await;
            return Err(DiscordManagerError::Persistence);
        }
        Ok(())
    }
}

// Both callers hold the manager's shared transaction lock across save/rollback.
async fn restore_metadata(manager: &DiscordManager, previous: DiscordConnectorRecord) {
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(previous.id.clone(), previous);
}
