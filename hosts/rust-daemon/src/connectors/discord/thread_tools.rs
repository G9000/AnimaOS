//! Host-created current-thread capability. Model arguments never select a destination.
use super::*;
use crate::runs::{RunLink, RunSource};
use anima_core::CancelSignal;

#[path = "thread_history.rs"]
mod history;

const REFUSED: &str = "Discord thread authority changed or this run is no longer active";

#[derive(Clone)]
pub(crate) struct DiscordThreadScope {
    manager: DiscordManager,
    destination: DiscordConnectorRecord,
    epoch: u64,
}
impl std::fmt::Debug for DiscordThreadScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordThreadScope")
            .field("room_id", &self.destination.room_id)
            .finish_non_exhaustive()
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordRenameReceipt {
    renamed: bool,
    channel_id: String,
    title: String,
}

/// Catalog revalidation, independent of caller-supplied message metadata.
pub(crate) fn valid_thread_run(state: &DaemonState, link: &RunLink) -> bool {
    let Some(runtime) = state.agents.get(&link.agent_id) else {
        return false;
    };
    let Some(run) = state.runs.get(&link.run_id) else {
        return false;
    };
    !crate::agent_runs::is_helper_config(runtime.config())
        && run.agent_id == link.agent_id
        && run.session_id == link.session_id
        && run.source == RunSource::Discord
        && run.status.is_in_flight()
        && run.stop.is_none()
        && run.parent_run_id.is_none()
        && !run.system_followup.unwrap_or(false)
        && state
            .live
            .runs()
            .control(&link.run_id)
            .is_some_and(|control| !control.cancel.is_cancelled())
        && state
            .sessions
            .get(&link.agent_id, &link.session_id)
            .is_some_and(|session| !session.archived)
        && state.discord_connectors.values().any(|c| {
            c.agent_id == link.agent_id
                && c.is_active()
                && c.destination_for_room(&link.session_id)
                    .is_some_and(|destination| {
                        destination.channel.kind == DiscordChannelKind::GuildThread
                            && c.channel_allowed(&destination.channel)
                    })
        })
}

fn checked_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().any(char::is_control) || title.encode_utf16().count() > 80
    {
        return Err("title must be non-empty plain text without control characters, at most 80 UTF-16 units".into());
    }
    Ok(title.into())
}

impl DiscordThreadScope {
    pub(crate) async fn send_file(
        &self,
        run: &RunLink,
        path: &str,
        caption: &str,
        call_id: &str,
        cancel: Option<CancelSignal>,
    ) -> Result<DiscordFileReceipt, String> {
        let scope = self.clone();
        let run = run.clone();
        let path = path.to_string();
        let caption = caption.to_string();
        let call_id = call_id.to_string();
        self.manager
            .owned(async move {
                Ok(scope
                    .send_file_owned(&run, path, caption, call_id, cancel)
                    .await)
            })
            .await
            .map_err(|e| e.to_string())?
    }

    async fn send_file_owned(
        &self,
        run: &RunLink,
        path: String,
        caption: String,
        call_id: String,
        cancel: Option<CancelSignal>,
    ) -> Result<DiscordFileReceipt, String> {
        let c = &self.destination;
        let root = {
            let state = self.manager.state.read().await;
            if !self.valid_for(&state, run) || cancel.as_ref().is_some_and(|v| v.is_cancelled()) {
                return Err(REFUSED.into());
            }
            state
                .workspace
                .as_ref()
                .ok_or("Configure a workspace before sending files")?
                .root_path
                .clone()
        };
        if caption.encode_utf16().count() > 2000 {
            return Err("caption exceeds Discord's 2000 UTF-16 unit limit".into());
        }
        let source_root = root.clone();
        let file = tokio::task::spawn_blocking(move || -> Result<DiscordFileUpload,String> {
            let workspace = crate::tools::workspace_bytes::WorkspaceBytes::open(&source_root).map_err(|_| "Workspace is unavailable")?;
            let bytes = workspace.read_file_snapshot(&path, 10 * 1024 * 1024).map_err(|_| "File must be a readable regular file within the workspace, at most 10 MiB; links and traversal are refused")?;
            let name = path.rsplit('/').next().ok_or("Invalid file path")?;
            let validated = crate::attachments::validation::validate_upload(name, &bytes).map_err(|e| e.to_string())?;
            if validated.kind == crate::attachments::AttachmentKind::Audio { return Err("Voice/audio delivery is not available".into()); }
            Ok(DiscordFileUpload {filename:validated.name,mime:validated.mime,bytes})
        }).await.map_err(|_| "Could not capture workspace file")??;
        let _effect = self.manager.claim_title(&c.id, self.epoch).ok_or(REFUSED)?;
        let token = self
            .manager
            .load_token(c)
            .await
            .map_err(|e| e.to_string())?;
        {
            let state = self.manager.state.read().await;
            if !self.valid_for(&state, run)
                || cancel.as_ref().is_some_and(|v| v.is_cancelled())
                || state.workspace.as_ref().is_none_or(|w| w.root_path != root)
            {
                return Err(REFUSED.into());
            }
        }
        // Native response validation proves channel/name/size and actual attachment.
        // The existing run tool journal owns the durable acknowledgement.
        self.manager.native().map_err(|e| e.to_string())?
            .send_file(&token, &c.channel.id, &file, &caption, &format!("discord-file:{}:{call_id}",run.run_id))
            .await.map_err(|error| match error {
                DiscordTransportError::PermissionDenied => "Discord denied the upload; grant the bot Attach Files permission in this channel/thread. No file send was confirmed.".into(),
                DiscordTransportError::Uncertain | DiscordTransportError::Transport | DiscordTransportError::InvalidResponse | DiscordTransportError::ResponseTooLarge => "Discord file delivery is uncertain; inspect this thread before retrying. Do not claim the file was sent or automatically retry.".into(),
                _ => format!("Discord did not confirm file delivery: {error}"),
            })
    }

    pub(crate) fn valid_for(&self, state: &DaemonState, link: &RunLink) -> bool {
        let c = &self.destination;
        c.agent_id == link.agent_id
            && c.room_id == link.session_id
            && self.manager.authority_current(&c.id, self.epoch)
            && valid_thread_run(state, link)
            && state
                .discord_connectors
                .get(&c.id)
                .is_some_and(|current| current.matches_destination(c))
    }

    pub(crate) async fn rename(
        &self,
        run: &RunLink,
        title: &str,
        cancel: Option<CancelSignal>,
    ) -> Result<DiscordRenameReceipt, String> {
        let title = checked_title(title)?;
        let scope = self.clone();
        let run = run.clone();
        let manager = self.manager.clone();
        // Once admitted, the effect/save belongs to the manager even if a caller
        // stops awaiting. The ordinary tool journal still owns its acknowledgement.
        self.manager
            .owned(async move {
                Ok(manager
                    .rename_thread_owned(&scope, &run, title, cancel)
                    .await)
            })
            .await
            .map_err(|e| e.to_string())?
    }
}

impl DiscordManager {
    pub(super) fn thread_scope(
        &self,
        c: &DiscordConnectorRecord,
        epoch: u64,
    ) -> DiscordThreadScope {
        DiscordThreadScope {
            manager: self.worker_context(),
            destination: c.clone(),
            epoch,
        }
    }

    pub(super) fn title_lock(&self, c: &DiscordConnectorRecord) -> Arc<Mutex<()>> {
        let key = format!("{}:{}", c.id, c.channel.id);
        let mut locks = self.title_locks.lock().unwrap_or_else(|e| e.into_inner());
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(&key).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }

    async fn rename_thread_owned(
        &self,
        scope: &DiscordThreadScope,
        run: &RunLink,
        title: String,
        cancel: Option<CancelSignal>,
    ) -> Result<DiscordRenameReceipt, String> {
        let c = &scope.destination;
        let title_lock = self.title_lock(c);
        let _title = title_lock.lock().await;
        if cancel.as_ref().is_some_and(|v| v.is_cancelled())
            || !scope.valid_for(&*self.state.read().await, run)
        {
            return Err(REFUSED.into());
        }
        let _effect = self.claim_title(&c.id, scope.epoch).ok_or(REFUSED)?;
        // Save suppression before the network effect. Never restore eligibility if
        // Discord's outcome is uncertain or the later metadata save fails.
        {
            let _txn = self.transaction.lock().await;
            let (previous, persist) = {
                let mut state = self.state.write().await;
                if !scope.valid_for(&state, run) {
                    return Err(REFUSED.into());
                }
                let current = state.discord_connectors.get_mut(&c.id).ok_or(REFUSED)?;
                let previous = current.clone();
                if let Some(conversation) = current
                    .conversations
                    .iter_mut()
                    .find(|v| v.room_id == c.room_id)
                {
                    conversation.auto_title = None;
                }
                (previous, state.control_plane_persist_request())
            };
            if let Err(error) = persist.save().await {
                self.state
                    .write()
                    .await
                    .discord_connectors
                    .insert(previous.id.clone(), previous);
                return Err(format!(
                    "Could not save rename intent; Discord was not changed: {error}"
                ));
            }
        }
        let token = self.load_token(c).await.map_err(|e| e.to_string())?;
        let remote = self
            .native()
            .map_err(|e| e.to_string())?
            .get_channel(&token, &c.channel.id)
            .await
            .map_err(|e| format!("Could not inspect the current Discord thread: {e}"))?;
        if !remote.routing_eq(&c.channel) || remote.kind != DiscordChannelKind::GuildThread {
            return Err("Discord returned a different thread; no rename was sent".into());
        }
        if cancel.as_ref().is_some_and(|v| v.is_cancelled())
            || !scope.valid_for(&*self.state.read().await, run)
        {
            return Err(REFUSED.into());
        }
        let renamed = self
            .native()
            .map_err(|e| e.to_string())?
            .rename_thread(&token, &c.channel.id, &title)
            .await
            .map_err(rename_error)?;
        if !renamed.routing_eq(&c.channel) || renamed.name.as_deref() != Some(title.as_str()) {
            return Err("Discord rename could not be confirmed for the requested thread/title; it may have changed. Inspect Discord before retrying.".into());
        }
        // Persist the response in both primary-thread and conversation metadata.
        // A failed save rolls back metadata only, never the already-suppressed auto title.
        let _txn = self.transaction.lock().await;
        let (previous, persist) = {
            let mut state = self.state.write().await;
            let current = state.discord_connectors.get_mut(&c.id).filter(|v| v.matches_destination(c)).ok_or("Discord accepted the rename, but its local connection changed; inspect Discord before retrying")?;
            let previous = current.clone();
            if current.channel.id == c.channel.id {
                current.channel.name = renamed.name.clone();
            }
            if let Some(conversation) = current
                .conversations
                .iter_mut()
                .find(|v| v.room_id == c.room_id)
            {
                conversation.channel.name = renamed.name;
            }
            (previous, state.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            self.state
                .write()
                .await
                .discord_connectors
                .insert(previous.id.clone(), previous);
            return Err(format!("Discord accepted the rename, but local metadata could not be saved. Inspect Discord before retrying: {error}"));
        }
        Ok(DiscordRenameReceipt {
            renamed: true,
            channel_id: c.channel.id.clone(),
            title,
        })
    }
}

fn rename_error(error: DiscordTransportError) -> String {
    match error {
        DiscordTransportError::HttpStatus { status } if status != 400 && status != 404 => "Discord rename outcome is uncertain; the title may have changed. Inspect Discord before retrying; do not confirm success.".into(),
        DiscordTransportError::Uncertain | DiscordTransportError::Transport | DiscordTransportError::InvalidResponse | DiscordTransportError::ResponseTooLarge => "Discord rename outcome is uncertain; the title may have changed. Inspect Discord before retrying; do not confirm success.".into(),
        DiscordTransportError::PermissionDenied => "Discord denied the rename. The bot needs permission to manage this thread or to be its creator; no rename was confirmed.".into(),
        DiscordTransportError::RateLimited { retry_after_ms, .. } => format!("Discord rate-limited the rename (retry after {retry_after_ms} ms); no rename was confirmed."),
        _ => format!("Discord did not accept the rename: {error}"),
    }
}
