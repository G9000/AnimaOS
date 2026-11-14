//! Native runtime regressions use injected transport and vault; never live Discord.
use super::*;
#[path = "channel_tests.rs"]
mod channel_tests;
#[test]
fn uncertainty_review_retains_receipts_and_audit_and_only_suppresses_unsent_tail() {
    let connector = fixture_connector();
    let mut record = outbound_for(&connector, "reply", "hello", false);
    record.chunks[0].state = DiscordDeliveryState::Delivered;
    record.chunks[0].provider_message_id = Some("9007199254740993".into());
    let mut unknown = record.chunks[0].clone();
    unknown.index = 1;
    unknown.state = DiscordDeliveryState::Unknown;
    unknown.provider_message_id = None;
    record.chunks.push(unknown);
    let mut tail = record.chunks[0].clone();
    tail.index = 2;
    tail.state = DiscordDeliveryState::Pending;
    tail.provider_message_id = None;
    record.chunks.push(tail);
    record.delivery_state = DiscordDeliveryState::Unknown;
    assert!(suppress_unknown_record(&mut record));
    assert_eq!(record.delivery_state, DiscordDeliveryState::Suppressed);
    assert_eq!(
        record.chunks[0].provider_message_id.as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(record.chunks[1].state, DiscordDeliveryState::Unknown);
    assert_eq!(record.chunks[2].state, DiscordDeliveryState::Suppressed);
}
#[test]
fn restore_sending_preserves_delivered_prefix_and_never_returns_pending() {
    let connector = fixture_connector();
    let mut record = outbound_for(&connector, "reply", "hello", false);
    record.chunks[0].state = DiscordDeliveryState::Sending;
    record.delivery_state = DiscordDeliveryState::Sending;
    recover_outbound(&mut record);
    assert_eq!(record.chunks[0].state, DiscordDeliveryState::Unknown);
    assert_eq!(record.delivery_state, DiscordDeliveryState::Unknown);
    assert!(!record.delivery_state.awaits_delivery());
}
fn fixture_connector() -> DiscordConnectorRecord {
    DiscordConnectorRecord {
        id: "discord-fixture".into(),
        agent_id: "agent".into(),
        room_id: "discord:discord-fixture".into(),
        bot: DiscordBotIdentity {
            id: "11".into(),
            username: None,
            display_name: None,
        },
        channel: DiscordChannelMetadata {
            id: "22".into(),
            guild_id: "33".into(),
            name: None,
            kind: DiscordChannelKind::GuildText,
            parent_channel_id: None,
        },
        bot_wide: false,
        guild_scope: None,
        application_id: None,
        settings: None,
        configuration_revision: 0,
        conversations: vec![],
        thread_starts: Default::default(),
        approved_user_ids: vec!["44".into()],
        credential_generation: 1,
        cursor: "0".into(),
        activated_at_ms: Some(1),
        enabled: true,
        deleted_at_ms: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use tokio::sync::{RwLock, Semaphore};
#[derive(Default)]
struct FakeTransport {
    admin_requests: StdMutex<Vec<channel_admin_model::DiscordChannelAdminRequest>>,
    admin_responses: StdMutex<VecDeque<Result<serde_json::Value, DiscordTransportError>>>,
    admin_write_responses: StdMutex<VecDeque<Result<serde_json::Value, DiscordTransportError>>>,
    admin_channels: StdMutex<HashMap<String, serde_json::Value>>,
    guild_channels: StdMutex<Vec<DiscordGuildChannel>>,
    guild_list_gate: AtomicBool,
    guild_list_entered: SemaphoreHolder,
    guild_list_release: SemaphoreHolder,
    guild_create_calls: AtomicUsize,
    guild_edit_calls: AtomicUsize,
    guild_error: StdMutex<Option<DiscordTransportError>>,
    guild_response: StdMutex<Option<DiscordGuildChannel>>,
    guild_gate: AtomicBool,
    guild_entered: SemaphoreHolder,
    guild_release: SemaphoreHolder,
    application_check: StdMutex<Option<Result<DiscordBotCheck, DiscordTransportError>>>,
    history_messages: StdMutex<Vec<DiscordMessage>>,
    history_calls: StdMutex<Vec<(String, Option<String>, usize)>>,
    history_gate: AtomicBool,
    history_entered: SemaphoreHolder,
    history_release: SemaphoreHolder,
    downloads: StdMutex<HashMap<String, Vec<u8>>>,
    download_calls: AtomicUsize,
    download_error: StdMutex<Option<DiscordTransportError>>,
    files: StdMutex<Vec<(String, String, Vec<u8>, String)>>,
    file_error: StdMutex<Option<DiscordTransportError>>,
    file_gate: AtomicBool,
    file_entered: SemaphoreHolder,
    file_release: SemaphoreHolder,
    channels: StdMutex<HashMap<String, DiscordChannelMetadata>>,
    title_lookup_gate: AtomicBool,
    title_lookup_entered: SemaphoreHolder,
    title_lookup_release: SemaphoreHolder,
    channel_errors: StdMutex<VecDeque<DiscordTransportError>>,
    member_roles: StdMutex<HashMap<String, Vec<String>>>,
    thread_calls: AtomicUsize,
    rename_calls: AtomicUsize,
    rename_error: StdMutex<Option<DiscordTransportError>>,
    rename_gate: AtomicBool,
    rename_entered: SemaphoreHolder,
    rename_release: SemaphoreHolder,
    rename_response: StdMutex<Option<DiscordChannelMetadata>>,
    thread_error: StdMutex<Option<DiscordTransportError>>,
    sent_chunks: StdMutex<Vec<(String, String)>>,
    typing_calls: AtomicUsize,
    typing_inflight: AtomicUsize,
    typing_gate: AtomicBool,
    typing_entered: SemaphoreHolder,
    typing_release: SemaphoreHolder,
    typing_results: StdMutex<VecDeque<Result<(), DiscordTransportError>>>,
    latest: AtomicU64,
    baseline: StdMutex<Option<String>>,
    sends: AtomicUsize,
    gate: AtomicBool,
    poll_calls: AtomicUsize,
    poll_error: StdMutex<Option<DiscordTransportError>>,
    entered: SemaphoreHolder,
    release: SemaphoreHolder,
    results: StdMutex<VecDeque<Result<DiscordSendReceipt, DiscordTransportError>>>,
}
struct SemaphoreHolder(Semaphore);
impl Default for SemaphoreHolder {
    fn default() -> Self {
        Self(Semaphore::new(0))
    }
}
#[async_trait::async_trait]
impl DiscordTransport for FakeTransport {
    async fn channel_admin(
        &self,
        token: &DiscordBotToken,
        request: &channel_admin_model::DiscordChannelAdminRequest,
    ) -> Result<serde_json::Value, DiscordTransportError> {
        use channel_admin_model::{
            DiscordChannelAdminMethod as Method, DiscordChannelAdminRoute as Route,
        };
        use serde_json::{json, Value};
        self.admin_requests.lock().unwrap().push(request.clone());
        if let Some(response) = self.admin_responses.lock().unwrap().pop_front() {
            return response;
        }
        if request.method != Method::Get {
            if let Some(response) = self.admin_write_responses.lock().unwrap().pop_front() {
                return response;
            }
        }
        let current_channel = |id: &str| -> Result<Value, DiscordTransportError> {
            if let Some(channel) = self.admin_channels.lock().unwrap().get(id).cloned() {
                return Ok(channel);
            }
            let inventory_channel = self
                .guild_channels
                .lock()
                .unwrap()
                .iter()
                .find(|channel| channel.id == id)
                .map(|channel| {
                    let mut value = serde_json::to_value(channel).unwrap();
                    value["permission_overwrites"] = json!([]);
                    value
                });
            if let Some(channel) = inventory_channel {
                return Ok(channel);
            }
            self.channels.lock().unwrap().get(id).map(|channel| {
                let kind = if channel.kind == DiscordChannelKind::GuildThread { 11 } else { 0 };
                json!({"id":channel.id,"guild_id":channel.guild_id,"name":channel.name,
                    "type":kind,"parent_id":channel.parent_channel_id,"permission_overwrites":[],
                    "thread_metadata":{"archived":false,"locked":false,"auto_archive_duration":1440}})
            }).ok_or(DiscordTransportError::HttpStatus { status:404 })
        };
        match &request.route {
            Route::Channel { channel_id } => {
                let mut channel = current_channel(channel_id)?;
                match request.method {
                    Method::Get => Ok(channel),
                    Method::Delete => {
                        self.admin_channels.lock().unwrap().remove(channel_id);
                        self.guild_channels
                            .lock()
                            .unwrap()
                            .retain(|value| value.id != *channel_id);
                        Ok(channel)
                    }
                    Method::Patch => {
                        let legacy_rename = request
                            .body
                            .as_ref()
                            .and_then(Value::as_object)
                            .filter(|body| body.len() == 1)
                            .and_then(|body| body.get("name"))
                            .and_then(Value::as_str)
                            .filter(|_| {
                                self.channels.lock().unwrap().get(channel_id).is_some_and(
                                    |channel| channel.kind == DiscordChannelKind::GuildThread,
                                )
                            });
                        if let Some(name) = legacy_rename {
                            let renamed = self.rename_thread(token, channel_id, name).await?;
                            channel["id"] = json!(renamed.id);
                            channel["guild_id"] = json!(renamed.guild_id);
                            channel["name"] = json!(renamed.name);
                            channel["parent_id"] = json!(renamed.parent_channel_id);
                            self.admin_channels
                                .lock()
                                .unwrap()
                                .insert(channel_id.clone(), channel.clone());
                            return Ok(channel);
                        }
                        for (key, value) in request
                            .body
                            .as_ref()
                            .and_then(Value::as_object)
                            .ok_or(DiscordTransportError::InvalidInput)?
                        {
                            if matches!(
                                key.as_str(),
                                "archived" | "locked" | "auto_archive_duration" | "invitable"
                            ) {
                                channel["thread_metadata"][key] = value.clone();
                            } else {
                                channel[key] = value.clone();
                            }
                        }
                        self.admin_channels
                            .lock()
                            .unwrap()
                            .insert(channel_id.clone(), channel.clone());
                        Ok(channel)
                    }
                    _ => Err(DiscordTransportError::InvalidInput),
                }
            }
            Route::GuildChannels { guild_id } => {
                let mut channel = request
                    .body
                    .clone()
                    .ok_or(DiscordTransportError::InvalidInput)?;
                channel["id"] = json!("990");
                channel["guild_id"] = json!(guild_id);
                channel["position"] = json!(0);
                self.admin_channels
                    .lock()
                    .unwrap()
                    .insert("990".into(), channel.clone());
                Ok(channel)
            }
            Route::GuildPositions { guild_id } => {
                let positions = request
                    .body
                    .as_ref()
                    .and_then(Value::as_array)
                    .ok_or(DiscordTransportError::InvalidInput)?;
                for position in positions {
                    let id = position["id"]
                        .as_str()
                        .ok_or(DiscordTransportError::InvalidInput)?;
                    let mut channel = current_channel(id)?;
                    if channel["guild_id"] != *guild_id {
                        return Err(DiscordTransportError::InvalidInput);
                    }
                    for field in ["position", "parent_id"] {
                        if let Some(value) = position.get(field) {
                            channel[field] = value.clone();
                        }
                    }
                    self.admin_channels
                        .lock()
                        .unwrap()
                        .insert(id.into(), channel.clone());
                }
                Ok(Value::Null)
            }
            Route::Permission {
                channel_id,
                overwrite_id,
            } => {
                let mut channel = current_channel(channel_id)?;
                if channel.get("permission_overwrites").is_none() {
                    channel["permission_overwrites"] = json!([]);
                }
                let overwrites = channel["permission_overwrites"].as_array_mut().unwrap();
                overwrites.retain(|overwrite| overwrite["id"] != *overwrite_id);
                if request.method == Method::Put {
                    let mut overwrite = request
                        .body
                        .clone()
                        .ok_or(DiscordTransportError::InvalidInput)?;
                    overwrite["id"] = json!(overwrite_id);
                    overwrites.push(overwrite);
                }
                self.admin_channels
                    .lock()
                    .unwrap()
                    .insert(channel_id.clone(), channel);
                Ok(Value::Null)
            }
            Route::MemberPermissions {
                channel_id,
                user_id,
            } => {
                current_channel(channel_id)?;
                Ok(json!({"channel_id":channel_id,"user_id":user_id,"permissions":"1024"}))
            }
            Route::RolePermissions {
                channel_id,
                role_id,
            } => {
                current_channel(channel_id)?;
                Ok(json!({"channel_id":channel_id,"role_id":role_id,"permissions":"1024"}))
            }
            Route::ThreadStart {
                channel_id,
                message_id,
            } => {
                let parent = current_channel(channel_id)?;
                let body = request
                    .body
                    .as_ref()
                    .ok_or(DiscordTransportError::InvalidInput)?;
                let id = message_id.as_deref().unwrap_or("991");
                let kind = if parent["type"] == 5 {
                    10
                } else {
                    body.get("type").and_then(Value::as_u64).unwrap_or(11)
                };
                let mut thread = json!({"id":id,"guild_id":parent["guild_id"],"parent_id":channel_id,
                    "type":kind,"name":body["name"],"rate_limit_per_user":0,
                    "thread_metadata":{"archived":false,"locked":false,"auto_archive_duration":1440}});
                for field in ["rate_limit_per_user", "applied_tags"] {
                    if let Some(value) = body.get(field) {
                        thread[field] = value.clone();
                    }
                }
                for field in ["auto_archive_duration", "invitable"] {
                    if let Some(value) = body.get(field) {
                        thread["thread_metadata"][field] = value.clone();
                    }
                }
                if let Some(message) = body.get("message") {
                    thread["message"] = message.clone();
                    thread["message"]["id"] = json!("992");
                    thread["message"]["channel_id"] = json!(id);
                }
                self.admin_channels
                    .lock()
                    .unwrap()
                    .insert(id.into(), thread.clone());
                Ok(thread)
            }
            Route::GuildThreads { guild_id } => {
                let threads: Vec<_> = self
                    .admin_channels
                    .lock()
                    .unwrap()
                    .values()
                    .filter(|channel| {
                        channel["guild_id"] == *guild_id
                            && channel["parent_id"].is_string()
                            && matches!(channel["type"].as_u64(), Some(10..=12))
                    })
                    .cloned()
                    .collect();
                Ok(json!({"threads":threads,"members":[],"has_more":false}))
            }
            Route::ArchivedThreads { channel_id, .. } => {
                current_channel(channel_id)?;
                Ok(json!({"threads":[],"members":[],"has_more":false}))
            }
            Route::ThreadMembers {
                channel_id,
                user_id,
                ..
            } => {
                current_channel(channel_id)?;
                if request.method == Method::Get {
                    Ok(if let Some(user_id) = user_id {
                        json!({"id":channel_id,"user_id":user_id,"join_timestamp":"2026-11-14T00:00:00Z","flags":0})
                    } else {
                        json!([])
                    })
                } else {
                    Ok(Value::Null)
                }
            }
            Route::VoiceStatus { channel_id } => {
                current_channel(channel_id)?;
                Ok(Value::Null)
            }
            Route::Follow { channel_id } => Ok(json!({"channel_id":channel_id,"webhook_id":"998"})),
        }
    }
    async fn list_guild_channels(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<Vec<DiscordGuildChannel>, DiscordTransportError> {
        self.guild_list_entered.0.add_permits(1);
        if self.guild_list_gate.load(Ordering::SeqCst) {
            self.guild_list_release.0.acquire().await.unwrap().forget();
        }
        let inventory = self.guild_channels.lock().unwrap().clone();
        if !inventory.is_empty() {
            return Ok(inventory);
        }
        let channels = self.channels.lock().unwrap();
        let mut inventory = Vec::new();
        for channel in channels.values() {
            let (id, name) = if channel.kind == DiscordChannelKind::GuildThread {
                let Some(parent) = &channel.parent_channel_id else {
                    continue;
                };
                (parent.clone(), format!("channel-{parent}"))
            } else {
                (
                    channel.id.clone(),
                    channel
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("channel-{}", channel.id)),
                )
            };
            if inventory
                .iter()
                .any(|channel: &DiscordGuildChannel| channel.id == id)
            {
                continue;
            }
            inventory.push(DiscordGuildChannel {
                id,
                guild_id: channel.guild_id.clone(),
                name,
                kind: 0,
                topic: None,
                parent_id: None,
                position: 0,
            });
        }
        Ok(inventory)
    }
    async fn create_guild_channel(
        &self,
        _: &DiscordBotToken,
        guild: &str,
        request: &DiscordChannelCreate,
    ) -> Result<DiscordGuildChannel, DiscordTransportError> {
        let count = self.guild_create_calls.fetch_add(1, Ordering::SeqCst);
        self.guild_entered.0.add_permits(1);
        if self.guild_gate.load(Ordering::SeqCst) {
            self.guild_release.0.acquire().await.unwrap().forget();
        }
        if let Some(error) = *self.guild_error.lock().unwrap() {
            return Err(error);
        }
        let channel = DiscordGuildChannel {
            id: (900 + count).to_string(),
            guild_id: guild.into(),
            name: request.name.clone(),
            kind: request.kind,
            topic: request.topic.clone(),
            parent_id: request.parent_id.clone(),
            position: 0,
        };
        self.guild_channels.lock().unwrap().push(channel.clone());
        Ok(self
            .guild_response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(channel))
    }
    async fn edit_guild_channel(
        &self,
        _: &DiscordBotToken,
        _: &str,
        id: &str,
        request: &DiscordChannelEdit,
    ) -> Result<DiscordGuildChannel, DiscordTransportError> {
        self.guild_edit_calls.fetch_add(1, Ordering::SeqCst);
        self.guild_entered.0.add_permits(1);
        if self.guild_gate.load(Ordering::SeqCst) {
            self.guild_release.0.acquire().await.unwrap().forget();
        }
        if let Some(error) = *self.guild_error.lock().unwrap() {
            return Err(error);
        }
        let mut channels = self.guild_channels.lock().unwrap();
        let channel = channels
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(DiscordTransportError::InvalidInput)?;
        if let Some(name) = &request.name {
            channel.name = name.clone();
        }
        if let Some(topic) = &request.topic {
            channel.topic = Some(topic.clone());
        }
        if let Some(parent) = &request.parent_id {
            channel.parent_id = parent.clone();
        }
        Ok(self
            .guild_response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| channel.clone()))
    }
    async fn read_thread_history(
        &self,
        _: &DiscordBotToken,
        channel: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DiscordMessage>, DiscordTransportError> {
        self.history_calls
            .lock()
            .unwrap()
            .push((channel.into(), before.map(str::to_owned), limit));
        self.history_entered.0.add_permits(1);
        if self.history_gate.load(Ordering::SeqCst) {
            self.history_release.0.acquire().await.unwrap().forget();
        }
        let before = before.map(|id| id.parse::<u64>().unwrap());
        let mut messages = self
            .history_messages
            .lock()
            .unwrap()
            .iter()
            .filter(|message| {
                message.channel_id == channel
                    && before.is_none_or(|id| message.id.parse::<u64>().unwrap() < id)
            })
            .cloned()
            .collect::<Vec<_>>();
        messages.sort_by_key(|message| std::cmp::Reverse(message.id.parse::<u64>().unwrap()));
        messages.truncate(limit);
        Ok(messages)
    }
    async fn download_attachment(
        &self,
        _: &str,
        attachment: &DiscordAttachment,
    ) -> Result<Vec<u8>, DiscordTransportError> {
        self.download_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.download_error.lock().unwrap().take() {
            return Err(error);
        }
        self.downloads
            .lock()
            .unwrap()
            .get(&attachment.id)
            .cloned()
            .ok_or(DiscordTransportError::Transport)
    }
    async fn send_file(
        &self,
        _: &DiscordBotToken,
        channel: &str,
        file: &DiscordFileUpload,
        caption: &str,
        _: &str,
    ) -> Result<DiscordFileReceipt, DiscordTransportError> {
        self.file_entered.0.add_permits(1);
        if self.file_gate.load(Ordering::SeqCst) {
            self.file_release.0.acquire().await.unwrap().forget();
        }
        if let Some(error) = self.file_error.lock().unwrap().take() {
            return Err(error);
        }
        self.files.lock().unwrap().push((
            channel.into(),
            file.filename.clone(),
            file.bytes.clone(),
            caption.into(),
        ));
        Ok(DiscordFileReceipt {
            sent: true,
            message_id: "901".into(),
            channel_id: channel.into(),
            attachment_id: "902".into(),
            filename: file.filename.clone(),
            size: file.bytes.len() as u64,
        })
    }
    async fn check_application(
        &self,
        _: &DiscordBotToken,
    ) -> Result<DiscordBotCheck, DiscordTransportError> {
        if let Some(result) = self.application_check.lock().unwrap().clone() {
            return result;
        }
        Ok(DiscordBotCheck {
            application_id: "11".into(),
            bot: fixture_connector().bot,
            owner_user_ids: vec!["44".into()],
            management_owner_user_id: Some("44".into()),
            message_content_enabled: true,
            invite_url: client::invite_url("11"),
        })
    }
    async fn member_roles(
        &self,
        _: &DiscordBotToken,
        _: &str,
        user: &str,
    ) -> Result<Vec<String>, DiscordTransportError> {
        Ok(self
            .member_roles
            .lock()
            .unwrap()
            .get(user)
            .cloned()
            .unwrap_or_default())
    }
    async fn create_thread(
        &self,
        _: &DiscordBotToken,
        channel: &str,
        message: &str,
        name: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        self.thread_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.thread_error.lock().unwrap().take() {
            return Err(error);
        }
        let thread = DiscordChannelMetadata {
            id: message.into(),
            guild_id: "33".into(),
            name: Some(name.into()),
            kind: DiscordChannelKind::GuildThread,
            parent_channel_id: Some(channel.into()),
        };
        self.channels
            .lock()
            .unwrap()
            .insert(message.into(), thread.clone());
        Ok(thread)
    }
    async fn rename_thread(
        &self,
        _: &DiscordBotToken,
        channel: &str,
        name: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        self.rename_calls.fetch_add(1, Ordering::SeqCst);
        self.rename_entered.0.add_permits(1);
        if self.rename_gate.load(Ordering::SeqCst) {
            self.rename_release.0.acquire().await.unwrap().forget();
        }
        if let Some(error) = self.rename_error.lock().unwrap().take() {
            return Err(error);
        }
        if let Some(response) = self.rename_response.lock().unwrap().take() {
            return Ok(response);
        }
        let mut channels = self.channels.lock().unwrap();
        let thread = channels.get_mut(channel).expect("created thread");
        thread.name = Some(name.into());
        Ok(thread.clone())
    }
    async fn send_typing(&self, _: &DiscordBotToken, _: &str) -> Result<(), DiscordTransportError> {
        struct InFlight<'a>(&'a AtomicUsize);
        impl Drop for InFlight<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        self.typing_calls.fetch_add(1, Ordering::SeqCst);
        self.typing_inflight.fetch_add(1, Ordering::SeqCst);
        let _inflight = InFlight(&self.typing_inflight);
        self.typing_entered.0.add_permits(1);
        if self.typing_gate.load(Ordering::SeqCst) {
            self.typing_release.0.acquire().await.unwrap().forget();
        }
        self.typing_results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }
    async fn get_me(
        &self,
        _: &DiscordBotToken,
    ) -> Result<DiscordBotIdentity, DiscordTransportError> {
        Ok(fixture_connector().bot)
    }
    async fn get_channel(
        &self,
        _: &DiscordBotToken,
        channel: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        if channel == "101" && self.title_lookup_gate.load(Ordering::SeqCst) {
            self.title_lookup_entered.0.add_permits(1);
            self.title_lookup_release
                .0
                .acquire()
                .await
                .unwrap()
                .forget();
        }
        if let Some(error) = self.channel_errors.lock().unwrap().pop_front() {
            return Err(error);
        }
        if let Some(channel) = self.channels.lock().unwrap().get(channel).cloned() {
            return Ok(channel);
        }
        Ok(DiscordChannelMetadata {
            id: channel.into(),
            ..fixture_connector().channel
        })
    }
    async fn latest_cursor(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<String, DiscordTransportError> {
        Ok(self
            .baseline
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.latest.load(Ordering::SeqCst).to_string()))
    }
    async fn receive(
        &self,
        _: &DiscordBotToken,
        _: &str,
        cursor: &str,
    ) -> Result<DiscordMessageBatch, DiscordTransportError> {
        self.poll_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.poll_error.lock().unwrap().take() {
            return Err(error);
        }
        Ok(DiscordMessageBatch {
            messages: vec![],
            next_cursor: cursor.into(),
            boundary_proven: true,
        })
    }
    async fn send_chunk(
        &self,
        _: &DiscordBotToken,
        channel: &str,
        text: &str,
        _: &str,
    ) -> Result<DiscordSendReceipt, DiscordTransportError> {
        self.sent_chunks
            .lock()
            .unwrap()
            .push((channel.into(), text.into()));
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.entered.0.add_permits(1);
        if self.gate.load(Ordering::SeqCst) {
            self.release.0.acquire().await.unwrap().forget();
        }
        self.results.lock().unwrap().pop_front().unwrap_or_else(|| {
            Ok(DiscordSendReceipt {
                message_id: "9007199254740993".into(),
                channel_id: channel.into(),
            })
        })
    }
}
async fn setup() -> (DiscordManager, Arc<FakeTransport>, DiscordConnectorRecord) {
    let mut state = DaemonState::new();
    let agent = state
        .create_agent(anima_core::AgentConfig {
            name: "Anima".into(),
            model: "deterministic".into(),
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: None,
            system: None,
            tools: None,
            plugins: None,
            settings: Some(anima_core::AgentSettings::default()),
            fallbacks: Vec::new(),
        })
        .unwrap();
    let mut c = fixture_connector();
    c.agent_id = agent.state.id;
    state.discord_connectors.insert(c.id.clone(), c.clone());
    state.ensure_run_session(crate::state::RunSessionRequest {
        agent_id: &c.agent_id,
        room_id: &c.room_id,
        source: crate::runs::RunSource::Discord,
        system_followup_title: None,
        delegated_parent: None,
        peer_sender: None,
        parent: None,
        first_text: "",
        now_ms: 1,
    });
    let state = Arc::new(RwLock::new(state));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let transport = Arc::new(FakeTransport::default());
    let credentials = Arc::new(credentials::InMemoryDiscordCredentialStore::default());
    credentials
        .put(
            &c.id,
            1,
            &c.bot.id,
            &DiscordBotToken::parse("test.token").unwrap(),
        )
        .await
        .unwrap();
    let manager = DiscordManager::new(
        state,
        runs,
        credentials,
        Arc::new(ConnectorRegistry::with_discord(
            Arc::new(crate::app::DeterministicTelegramTransport),
            transport.clone(),
        )),
    );
    (manager, transport, c)
}

#[tokio::test]
async fn reminder_discord_requires_all_chunks_and_preserves_partial_send_uncertainty() {
    use crate::schedules::delivery::{
        DeliveryChannel, DeliveryEvent, DeliveryResult, ReminderStatus,
    };
    use crate::schedules::{test_automation, ScheduleLastFired};
    for error in [
        None,
        Some(DiscordTransportError::PermissionDenied),
        Some(DiscordTransportError::Uncertain),
    ] {
        let (manager, transport, c) = setup().await;
        let outbound = outbound_for(&c, "reminder-reply", &"x".repeat(2100), false);
        let outbound_id = outbound.id.clone();
        {
            let mut state = manager.state.write().await;
            let mut schedule = test_automation(&c.agent_id, "reminder");
            schedule.last_fired = Some(ScheduleLastFired {
                fired_at_ms: 71_000,
                scheduled_at_ms: Some(70_000),
                run_idempotency_key: "occurrence".into(),
                manual: false,
            });
            schedule.delivery_events.push(DeliveryEvent {
                occurrence_id: "occurrence".into(),
                outbound_id: Some(outbound_id.clone()),
                scheduled_at_ms: Some(70_000),
                channel: DeliveryChannel::Discord,
                attempted_at_ms: None,
                completed_at_ms: None,
                result: DeliveryResult::Unknown,
                provider_message_ids: Vec::new(),
            });
            state.schedules.insert(schedule.id.clone(), schedule);
            state.discord_outbound.insert(outbound_id.clone(), outbound);
        }
        transport
            .results
            .lock()
            .unwrap()
            .push_back(Ok(DiscordSendReceipt {
                message_id: "101".into(),
                channel_id: c.channel.id.clone(),
            }));
        assert!(manager.deliver_one(c.id.clone()).await.unwrap());
        assert_eq!(
            ReminderStatus::from_record(
                &manager.state.read().await.schedules["reminder"],
                now_ms()
            )
            .status,
            "delivery_unconfirmed"
        );
        if let Some(error) = error {
            transport.results.lock().unwrap().push_back(Err(error));
        } else {
            transport
                .results
                .lock()
                .unwrap()
                .push_back(Ok(DiscordSendReceipt {
                    message_id: "102".into(),
                    channel_id: c.channel.id.clone(),
                }));
        }
        assert!(manager.deliver_one(c.id.clone()).await.unwrap());
        let state = manager.state.read().await;
        let status = ReminderStatus::from_record(&state.schedules["reminder"], now_ms());
        assert_eq!(
            status.status,
            if error.is_none() {
                "sent"
            } else {
                "delivery_unconfirmed"
            }
        );
        assert!(!status.receipt_verified);
        assert!(status
            .delivery_events
            .last()
            .unwrap()
            .provider_message_ids
            .contains(&"101".into()));
    }
}
fn incoming(c: &DiscordConnectorRecord, id: &str) -> DiscordMessage {
    DiscordMessage {
        id: id.into(),
        channel_id: c.channel.id.clone(),
        guild_id: Some(c.channel.guild_id.clone()),
        sender: DiscordSenderMetadata {
            id: c.approved_user_ids[0].clone(),
            username: None,
            display_name: None,
        },
        text: "hello Anima".into(),
        attachments: vec![],
        is_bot: false,
        webhook_id: None,
        message_type: 0,
    }
}
async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(3), future)
        .await
        .expect("fixture completed within bound")
}
#[tokio::test]
async fn admission_save_failure_keeps_cursor_and_receipts_atomic_after_caller_cancellation() {
    let (manager, _, c) = setup().await;
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let job = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move {
            manager
                .accept_batch(
                    c.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![incoming(&c, "9007199254740993")],
                        next_cursor: "9007199254740993".into(),
                        boundary_proven: true,
                    },
                )
                .await
        }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    job.abort();
    gate.release.add_permits(1);
    within(async {
        loop {
            if manager.owned_count.load(Ordering::SeqCst) == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let state = manager.state.read().await;
    assert_eq!(state.discord_connectors[&c.id].cursor, "0");
    assert!(state.discord_inbound.is_empty());
    assert!(state.runs.active_records().is_empty());
}
#[tokio::test]
async fn admission_filters_native_authority_and_duplicate_replay_without_model_effects() {
    let (manager, _, c) = setup().await;
    let first = incoming(&c, "9007199254740993");
    let mut bot = incoming(&c, "9007199254740994");
    bot.is_bot = true;
    let mut webhook = incoming(&c, "9007199254740995");
    webhook.webhook_id = Some("555".into());
    let mut stranger = incoming(&c, "9007199254740996");
    stranger.sender.id = "77".into();
    let batch = DiscordMessageBatch {
        messages: vec![first.clone(), first, bot, webhook, stranger],
        next_cursor: "9007199254740996".into(),
        boundary_proven: true,
    };
    manager
        .accept_batch(c.id.clone(), batch.clone())
        .await
        .unwrap();
    manager.accept_batch(c.id.clone(), batch).await.unwrap();
    let state = manager.state.read().await;
    assert_eq!(state.discord_inbound.len(), 1);
    assert_eq!(state.discord_connectors[&c.id].cursor, "9007199254740996");
    assert!(state.runs.active_records().is_empty());
}
#[tokio::test]
async fn sending_is_durable_before_effect_and_revocation_drains_without_later_chunks() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let record = outbound_for(&c, "reply", &"x".repeat(2100), false);
    let output = record.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(output.clone(), record);
    let delivering = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    assert_eq!(
        manager.state.read().await.discord_outbound[&output].chunks[0].state,
        DiscordDeliveryState::Sending
    );
    let disabling = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move { manager.set_enabled(c.agent_id, c.id, false).await }
    });
    within(async {
        loop {
            if manager.epoch(&c.id) > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(!disabling.is_finished());
    transport.release.0.add_permits(1);
    within(delivering).await.unwrap().unwrap();
    within(disabling).await.unwrap().unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
    let state = manager.state.read().await;
    assert_eq!(
        state.discord_outbound[&output].delivery_state,
        DiscordDeliveryState::Unknown
    );
    assert_eq!(
        state.discord_outbound[&output].chunks[1].state,
        DiscordDeliveryState::Pending
    );
}
#[tokio::test]
async fn unknown_never_retries_and_explicit_review_preserves_prefix_and_unblocks_new_work() {
    let (manager, transport, c) = setup().await;
    let first = outbound_for(&c, "reply", &"x".repeat(2100), false);
    let first_id = first.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(first.id.clone(), first);
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Ok(DiscordSendReceipt {
            message_id: "100".into(),
            channel_id: c.channel.id.clone(),
        }));
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Err(DiscordTransportError::Uncertain));
    manager.deliver_one(c.id.clone()).await.unwrap();
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(
        manager
            .retry(c.agent_id.clone(), c.id.clone())
            .await
            .unwrap_err(),
        DiscordManagerError::UnknownDelivery
    );
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 2);
    manager
        .suppress_unknown(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    let state = manager.state.read().await;
    assert_eq!(
        state.discord_outbound[&first_id].chunks[0]
            .provider_message_id
            .as_deref(),
        Some("100")
    );
    assert_eq!(
        state.discord_outbound[&first_id].chunks[1].state,
        DiscordDeliveryState::Unknown
    );
    drop(state);
    let second = outbound_for(&c, "new-reply", "new work", false);
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(second.id.clone(), second);
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn rate_deadline_survives_retry_and_snapshot_restore() {
    let (manager, transport, c) = setup().await;
    let output = outbound_for(&c, "reply", "rate limited", false);
    let output_id = output.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(output.id.clone(), output);
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Err(DiscordTransportError::RateLimited {
            retry_after_ms: 60_000,
            global: true,
        }));
    manager.deliver_one(c.id.clone()).await.unwrap();
    let deadline = manager.state.read().await.discord_outbound[&output_id].chunks[0]
        .retry_at_ms
        .unwrap();
    manager
        .retry(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(
        manager.state.read().await.discord_outbound[&output_id].chunks[0].retry_at_ms,
        Some(deadline)
    );
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
    let snapshot = manager.state.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.discord_outbound[&output_id].chunks[0].retry_at_ms,
        Some(deadline)
    );
}
#[tokio::test]
async fn first_empty_activation_and_reactivation_keep_the_durable_boundary() {
    let (manager, transport, c) = setup().await;
    {
        let mut state = manager.state.write().await;
        let c = state.discord_connectors.get_mut(&c.id).unwrap();
        c.enabled = false;
        c.activated_at_ms = None;
    }
    let first = manager
        .activate(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(first.cursor, "0");
    assert!(first.activated_at_ms.is_some());
    manager
        .set_enabled(c.agent_id.clone(), c.id.clone(), false)
        .await
        .unwrap();
    transport.latest.store(999, Ordering::SeqCst);
    let again = manager
        .activate(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(again.cursor, "0");
    assert_eq!(again.activated_at_ms, first.activated_at_ms);
    manager.shutdown().await;
}
#[tokio::test]
async fn public_clone_drop_keeps_runtime_open_and_last_owner_closes_worker_context() {
    let (manager, _, _) = setup().await;
    let clone = manager.clone();
    let mut worker = manager.worker_context();
    drop(manager);
    assert!(!clone.is_closing());
    assert!(worker.close.has_changed().is_ok());
    drop(clone);
    assert!(worker.close.changed().await.is_err());
}

#[tokio::test]
async fn successful_post_then_receipt_save_failure_blocks_replay_even_after_restore() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let record = outbound_for(&c, "reply", "receipt failure", false);
    let id = record.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(id.clone(), record);
    let delivering = tokio::spawn({
        let manager = manager.clone();
        let connector = c.id.clone();
        async move { manager.deliver_one(connector).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    transport.release.0.add_permits(1);
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert_eq!(
        within(delivering).await.unwrap().unwrap_err(),
        DiscordManagerError::Persistence
    );
    assert_eq!(
        manager.state.read().await.discord_outbound[&id].delivery_state,
        DiscordDeliveryState::Unknown
    );
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
    let snapshot = manager.state.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.discord_outbound[&id].delivery_state,
        DiscordDeliveryState::Unknown
    );
}
#[tokio::test]
async fn received_turn_uses_native_coordinator_session_and_stable_run_link() {
    let (manager, _, c) = setup().await;
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![incoming(&c, "9007199254740993")],
                next_cursor: "9007199254740993".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    manager.process_one(c.id.clone()).await.unwrap();
    within(async {
        loop {
            if manager
                .state
                .read()
                .await
                .discord_inbound
                .values()
                .all(|r| r.processing_state.is_terminal())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let state = manager.state.read().await;
    let receipt = state.discord_inbound.values().next().unwrap();
    assert!(receipt.run_id.is_some());
    let run = state.runs.get(receipt.run_id.as_deref().unwrap()).unwrap();
    assert_eq!(run.source, crate::runs::RunSource::Discord);
    assert_eq!(run.session_id, c.room_id);
    assert_eq!(receipt.processing_state, DiscordProcessingState::Processed);
    assert_eq!(state.discord_outbound.len(), 1);
    let session = state.sessions.get(&c.agent_id, &c.room_id).unwrap();
    assert_eq!(session.kind, crate::sessions::SessionKind::Discord);
    drop(state);
    manager.runs.shutdown().await;
}
async fn received_one(manager: &DiscordManager, c: &DiscordConnectorRecord) {
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![incoming(c, "9007199254740993")],
                next_cursor: "9007199254740993".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
}

async fn receipt_state(manager: &DiscordManager) -> DiscordProcessingState {
    let state = manager.state.read().await;
    state
        .discord_inbound
        .values()
        .next()
        .unwrap()
        .processing_state
        .clone()
}

async fn processed_after_cancel(manager: &DiscordManager, c: &DiscordConnectorRecord) {
    manager.runs.cancel_drain();
    assert!(manager.process_one(c.id.clone()).await.unwrap());
    within(async {
        while receipt_state(manager).await != DiscordProcessingState::Processed {
            tokio::task::yield_now().await;
        }
    })
    .await;
    manager.runs.shutdown().await;
}

#[tokio::test]
async fn a_drain_leaves_a_received_turn_for_after_cancel() {
    let (manager, _, c) = setup().await;
    received_one(&manager, &c).await;
    manager.runs.begin_drain();

    assert!(!manager.process_one(c.id.clone()).await.unwrap());
    assert_eq!(
        receipt_state(&manager).await,
        DiscordProcessingState::Received
    );
    processed_after_cancel(&manager, &c).await;
}

#[tokio::test]
async fn a_drain_that_begins_mid_pickup_puts_the_receipt_back() {
    let (manager, _, c) = setup().await;
    received_one(&manager, &c).await;
    // Hold the pickup's save (the receipt is Processing), then drain.
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let picking = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.process_one(id).await }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    assert_eq!(
        receipt_state(&manager).await,
        DiscordProcessingState::Processing
    );
    manager.runs.begin_drain();
    gate.release.add_permits(1);

    assert!(!within(picking).await.unwrap().unwrap());
    assert_eq!(
        receipt_state(&manager).await,
        DiscordProcessingState::Received
    );
    processed_after_cancel(&manager, &c).await;
}

#[tokio::test]
async fn unproven_scan_and_capacity_pressure_admit_nothing_and_do_not_move_cursor() {
    let (manager, _, c) = setup().await;
    let message = incoming(&c, "9007199254740993");
    let unproven = DiscordMessageBatch {
        messages: vec![message],
        next_cursor: "9007199254740993".into(),
        boundary_proven: false,
    };
    assert_eq!(
        manager
            .accept_batch(c.id.clone(), unproven)
            .await
            .unwrap_err(),
        DiscordManagerError::Conflict
    );
    let messages = (1..=101).map(|id| incoming(&c, &id.to_string())).collect();
    assert_eq!(
        manager
            .accept_batch(
                c.id.clone(),
                DiscordMessageBatch {
                    messages,
                    next_cursor: "101".into(),
                    boundary_proven: true
                }
            )
            .await
            .unwrap_err(),
        DiscordManagerError::Backpressure
    );
    let state = manager.state.read().await;
    assert!(state.discord_inbound.is_empty());
    assert_eq!(state.discord_connectors[&c.id].cursor, "0");
}

struct GateDeleteStore {
    inner: credentials::InMemoryDiscordCredentialStore,
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
}
#[async_trait::async_trait]
impl DiscordCredentialStore for GateDeleteStore {
    async fn load(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
    ) -> Result<Option<DiscordBotToken>, credentials::DiscordCredentialError> {
        self.inner.load(id, generation, bot).await
    }
    async fn put(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
        token: &DiscordBotToken,
    ) -> Result<(), credentials::DiscordCredentialError> {
        self.inner.put(id, generation, bot, token).await
    }
    async fn delete(
        &self,
        id: &str,
        generation: u64,
    ) -> Result<(), credentials::DiscordCredentialError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        self.inner.delete(id, generation).await
    }
}
async fn delete_store(c: &DiscordConnectorRecord) -> Arc<GateDeleteStore> {
    let store = Arc::new(GateDeleteStore {
        inner: credentials::InMemoryDiscordCredentialStore::default(),
        calls: AtomicUsize::new(0),
        entered: Semaphore::new(0),
        release: Semaphore::new(0),
    });
    store
        .put(
            &c.id,
            c.credential_generation,
            &c.bot.id,
            &DiscordBotToken::parse("test.token").unwrap(),
        )
        .await
        .unwrap();
    store
}
#[tokio::test]
async fn delete_journals_exact_generation_cleanup_with_tombstone_before_vault_effect() {
    let (mut manager, _, c) = setup().await;
    let store = delete_store(&c).await;
    manager.credentials = store.clone();
    let path = std::env::temp_dir().join(format!(
        "anima-discord-delete-{}.json",
        uuid::Uuid::new_v4()
    ));
    let config = crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone());
    manager
        .state
        .write()
        .await
        .set_control_plane_store(Some(config.clone()));
    let deleting = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move { manager.delete(c.agent_id, c.id).await }
    });
    within(store.entered.acquire()).await.unwrap().forget();
    let snapshot = crate::control_plane_store::load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap();
    assert!(snapshot.discord_connectors[0].deleted_at_ms.is_some());
    assert!(!snapshot.discord_connectors[0].enabled);
    assert_eq!(snapshot.discord_credential_intents.len(), 1);
    assert_eq!(
        snapshot.discord_credential_intents[0].generation,
        c.credential_generation
    );
    assert_eq!(
        snapshot.discord_credential_intents[0].operation,
        DiscordCredentialOperation::Delete
    );
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert!(restored
        .discord_credential_intents
        .contains_key(&(c.id.clone(), c.credential_generation)));
    store.release.add_permits(1);
    within(deleting).await.unwrap().unwrap();
    assert_eq!(store.calls.load(Ordering::SeqCst), 1);
    assert!(manager
        .state
        .read()
        .await
        .discord_credential_intents
        .is_empty());
    std::fs::remove_file(path).unwrap();
}
#[tokio::test]
async fn deletion_save_failure_rolls_back_tombstone_and_intent_and_never_touches_vault() {
    let (mut manager, _, c) = setup().await;
    let store = delete_store(&c).await;
    manager.credentials = store.clone();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let deleting = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move { manager.delete(c.agent_id, c.id).await }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert_eq!(
        within(deleting).await.unwrap().unwrap_err(),
        DiscordManagerError::Persistence
    );
    assert_eq!(manager.state.read().await.discord_connectors[&c.id], c);
    assert!(manager
        .state
        .read()
        .await
        .discord_credential_intents
        .is_empty());
    assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    assert!(store
        .load(&c.id, c.credential_generation, &c.bot.id)
        .await
        .unwrap()
        .is_some());
}
#[tokio::test]
async fn malformed_fake_activation_baseline_never_enables_or_changes_cursor() {
    let (manager, transport, c) = setup().await;
    {
        let mut state = manager.state.write().await;
        let c = state.discord_connectors.get_mut(&c.id).unwrap();
        c.enabled = false;
        c.activated_at_ms = None;
    }
    let previous = manager.state.read().await.discord_connectors[&c.id].clone();
    *transport.baseline.lock().unwrap() = Some("not-a-snowflake".into());
    assert_eq!(
        manager
            .activate(c.agent_id, c.id.clone())
            .await
            .unwrap_err(),
        DiscordManagerError::Conflict
    );
    assert_eq!(
        manager.state.read().await.discord_connectors[&c.id],
        previous
    );
    assert!(manager.workers.lock().await.is_empty());
}
#[tokio::test]
async fn legitimate_long_input_is_admitted_but_over_bound_input_never_consumes_cursor() {
    let (manager, _, c) = setup().await;
    let mut message = incoming(&c, "100");
    message.text = "x".repeat(4000);
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![message],
                next_cursor: "100".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    let mut too_long = incoming(&c, "101");
    too_long.text = "x".repeat(65537);
    assert_eq!(
        manager
            .accept_batch(
                c.id.clone(),
                DiscordMessageBatch {
                    messages: vec![too_long],
                    next_cursor: "101".into(),
                    boundary_proven: true
                }
            )
            .await
            .unwrap_err(),
        DiscordManagerError::UnsupportedInput
    );
    assert_eq!(
        manager.state.read().await.discord_connectors[&c.id].cursor,
        "100"
    );
    assert_eq!(manager.state.read().await.discord_inbound.len(), 1);
}

async fn sending_run_fixture(
    manager: &DiscordManager,
    c: &DiscordConnectorRecord,
) -> (String, String) {
    let mut run = crate::runs::RunRecord::queued(
        crate::runs::RunStart {
            agent_id: c.agent_id.clone(),
            session_id: c.room_id.clone(),
            source: crate::runs::RunSource::Discord,
            source_ref: Some(c.id.clone()),
            idempotency_key: Some("owner-stop-fixture".into()),
            text: "hello".into(),
            model: "deterministic".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    run.reply_message_id = Some("reply".into());
    run.finish(crate::runs::RunStatus::Completed, None, 2);
    let run_id = run.id.clone();
    let output = outbound_for(c, "reply", &"x".repeat(2100), false);
    let output_id = output.id.clone();
    let mut state = manager.state.write().await;
    state.runs.insert(run);
    state.discord_outbound.insert(output_id.clone(), output);
    (run_id, output_id)
}
#[tokio::test]
async fn owner_stop_during_sending_preserves_receipt_and_suppresses_only_unsent_tail() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let (run, output) = sending_run_fixture(&manager, &c).await;
    let sending = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager.runs.stop_run(&c.agent_id, &run).await.unwrap();
    {
        let state = manager.state.read().await;
        assert!(state.discord_outbound[&output].stop_requested);
        assert_eq!(
            state.discord_outbound[&output].chunks[0].state,
            DiscordDeliveryState::Sending
        );
        assert_eq!(
            state.discord_outbound[&output].chunks[1].state,
            DiscordDeliveryState::Suppressed
        );
    }
    transport.release.0.add_permits(1);
    within(sending).await.unwrap().unwrap();
    assert!(!manager.deliver_one(c.id.clone()).await.unwrap());
    let state = manager.state.read().await;
    assert_eq!(
        state.discord_outbound[&output].delivery_state,
        DiscordDeliveryState::Suppressed
    );
    assert_eq!(
        state.discord_outbound[&output].chunks[0]
            .provider_message_id
            .as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn failed_owner_stop_save_restores_exact_inflight_attempt_and_pending_tail() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let (run, output) = sending_run_fixture(&manager, &c).await;
    let sending = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let stopping = tokio::spawn({
        let runs = manager.runs.clone();
        let agent = c.agent_id.clone();
        async move { runs.stop_run(&agent, &run).await }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert!(within(stopping).await.unwrap().is_err());
    {
        let state = manager.state.read().await;
        assert!(!state.discord_outbound[&output].stop_requested);
        assert_eq!(
            state.discord_outbound[&output].chunks[0].state,
            DiscordDeliveryState::Sending
        );
        assert_eq!(
            state.discord_outbound[&output].chunks[1].state,
            DiscordDeliveryState::Pending
        );
    }
    transport.release.0.add_permits(1);
    within(sending).await.unwrap().unwrap();
    transport.gate.store(false, Ordering::SeqCst);
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn terminal_poll_errors_pause_after_one_request_until_explicit_restart() {
    for error in [
        DiscordTransportError::Authentication,
        DiscordTransportError::PermissionDenied,
        DiscordTransportError::HttpStatus { status: 404 },
    ] {
        let (manager, transport, c) = setup().await;
        *transport.poll_error.lock().unwrap() = Some(error);
        manager.start_restored().await;
        within(async {
            loop {
                if manager
                    .workers
                    .lock()
                    .await
                    .get(&c.id)
                    .is_some_and(|worker| worker.join.is_finished())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(transport.poll_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            manager.status(&c.id).await,
            if error == DiscordTransportError::Authentication {
                DiscordRuntimeStatus::CredentialRequired
            } else {
                DiscordRuntimeStatus::Error
            }
        );
        assert_eq!(
            manager
                .accept_batch(
                    c.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![incoming(&c, "100")],
                        next_cursor: "100".into(),
                        boundary_proven: true
                    }
                )
                .await
                .unwrap_err(),
            DiscordManagerError::Conflict
        );
        assert_eq!(
            manager.state.read().await.discord_connectors[&c.id].cursor,
            "0"
        );
        manager
            .restart(c.agent_id.clone(), c.id.clone())
            .await
            .unwrap();
        within(async {
            loop {
                if transport.poll_calls.load(Ordering::SeqCst) > 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(manager.status(&c.id).await, DiscordRuntimeStatus::Ready);
        manager.shutdown().await;
    }
}
#[tokio::test]
async fn present_guild_or_channel_mismatch_rejects_entire_batch_without_receipts_or_cursor() {
    for guild in [true, false] {
        let (manager, _, c) = setup().await;
        let first = incoming(&c, "100");
        let mut mismatch = incoming(&c, "101");
        if guild {
            mismatch.guild_id = Some("88".into());
        } else {
            mismatch.channel_id = "88".into();
        }
        assert_eq!(
            manager
                .accept_batch(
                    c.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![first, mismatch],
                        next_cursor: "101".into(),
                        boundary_proven: true
                    }
                )
                .await
                .unwrap_err(),
            DiscordManagerError::Conflict
        );
        assert_eq!(
            manager.state.read().await.discord_connectors[&c.id].cursor,
            "0"
        );
        assert!(manager.state.read().await.discord_inbound.is_empty());
    }
}
#[tokio::test]
async fn contradictory_delivery_snapshot_is_rejected_before_any_restore_mutation() {
    let (manager, _, c) = setup().await;
    let mut output = outbound_for(&c, "reply", "hello", false);
    output.delivery_state = DiscordDeliveryState::Delivered;
    let mut snapshot = manager.state.read().await.control_plane_snapshot();
    snapshot.discord_outbound.push(output);
    let mut restored = DaemonState::new();
    assert!(restored.restore_control_plane_snapshot(snapshot).is_err());
    assert_eq!(restored.agent_count(), 0);
    assert!(restored.discord_connectors.is_empty());
    let mut output = outbound_for(&c, "review", "hello", false);
    output.chunks[0].state = DiscordDeliveryState::Unknown;
    output.chunks[0].attempts = 1;
    output.delivery_state = DiscordDeliveryState::Suppressed;
    let mut snapshot = manager.state.read().await.control_plane_snapshot();
    snapshot.discord_outbound.push(output);
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.discord_outbound.values().next().unwrap().chunks[0].state,
        DiscordDeliveryState::Unknown
    );
}
#[test]
fn setup_transport_errors_retain_safe_actionable_reasons() {
    use super::*;
    for (error, expected) in [
        (
            DiscordTransportError::PermissionDenied,
            "Discord denied access; check the bot's channel permissions",
        ),
        (
            DiscordTransportError::HttpStatus { status: 404 },
            "Discord could not find the selected channel",
        ),
        (
            DiscordTransportError::RateLimited {
                retry_after_ms: 1000,
                global: false,
            },
            "Discord is limiting requests; wait before retrying",
        ),
        (
            DiscordTransportError::InvalidResponse,
            "Discord returned an unexpected response",
        ),
        (
            DiscordTransportError::Transport,
            "The daemon could not reach Discord",
        ),
    ] {
        assert_eq!(transport_error(error).to_string(), expected);
    }
}

#[path = "gateway_tests.rs"]
mod gateway_tests;

#[path = "multi_channel_tests.rs"]
mod multi_channel_tests;
#[path = "speaker_tests.rs"]
mod speaker_tests;
#[path = "typing_tests.rs"]
mod typing_tests;

#[path = "interactions_tests.rs"]
mod interactions_tests;

#[tokio::test]
async fn configured_mentions_gate_execution_without_granting_sender_authority() {
    let (manager, _, mut c) = setup().await;
    let mut encoded = serde_json::to_value(&c).unwrap();
    encoded["settings"] = serde_json::json!({"requireMention":true,"autoThread":false});
    c = serde_json::from_value(encoded).unwrap();
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(c.id.clone(), c.clone());
    let mut plain = incoming(&c, "101");
    plain.text = "ordinary channel conversation".into();
    let mut outsider = incoming(&c, "102");
    outsider.sender.id = "99".into();
    outsider.text = "<@11> please reply".into();
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![plain, outsider],
                next_cursor: "102".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    assert!(
        manager.state.read().await.discord_inbound.is_empty(),
        "ordinary chatter and unauthorized mentions must not execute"
    );
    manager.shutdown().await;
}
