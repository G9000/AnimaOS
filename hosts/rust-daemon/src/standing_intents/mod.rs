//! Host authority and durable mutation ownership for conversational intentions.
//! No scheduler or model participates in matching.
use crate::app::SharedDaemonState;
use crate::routes::ApiError;
pub(crate) use anima_core::standing_intents::StandingIntentRegistry;
use anima_core::standing_intents::{IntentSpec, OwnerSender, StandingIntent};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;
mod admission;
pub(crate) use admission::admit;
#[cfg(test)]
mod tests;
#[derive(Clone)]
pub(crate) struct StandingIntentService {
    state: SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    admission: Arc<dyn Fn() -> bool + Send + Sync>,
}
#[derive(Clone)]
pub(crate) struct StandingAuthority {
    pub(crate) link: crate::runs::RunLink,
    pub(crate) call: anima_core::ToolCall,
    pub(crate) approved: bool,
    pub(crate) owner_context: bool,
    pub(crate) cancel: Option<anima_core::CancelSignal>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntentMutation {
    pub(crate) intent: Option<StandingIntent>,
    pub(crate) change_id: String,
}
fn conflict() -> ApiError {
    ApiError::conflict("Standing intention or owner authority changed; refresh current state")
}
fn unavailable() -> ApiError {
    ApiError::service_unavailable("Standing intention persistence is unavailable or uncertain; inspect current state before retrying")
}
/// Reuse the shared owner-turn classifier without assuming a future model result.
/// Provenance is checked independently: user metadata never establishes identity.
fn trusted_connector_message(
    state: &crate::state::DaemonState,
    record: &crate::runs::RunRecord,
) -> bool {
    if !crate::learning::owner_only_connector_chat(state, record) {
        return false;
    }
    let Some((connector_id, message_id)) = record
        .source_ref
        .as_deref()
        .and_then(|s| s.rsplit_once(':'))
    else {
        return false;
    };
    match record.source {
        crate::runs::RunSource::Telegram => {
            let Ok(update) = message_id.parse::<i64>() else {
                return false;
            };
            state
                .inbound
                .get(&(connector_id.into(), update))
                .is_some_and(|m| {
                    m.agent_id == record.agent_id
                        && crate::sessions::session_id_for_room(&m.room_id) == record.session_id
                        && m.sender.id == m.chat.id
                        && state.connectors.get(connector_id).is_some_and(|c| {
                            c.enabled && c.deleted_at_ms.is_none() && c.agent_id == record.agent_id
                        })
                })
        }
        crate::runs::RunSource::Discord => state
            .discord_inbound
            .get(&(connector_id.into(), message_id.into()))
            .is_some_and(|m| {
                m.agent_id == record.agent_id
                    && crate::sessions::session_id_for_room(&m.room_id) == record.session_id
                    && state.discord_connectors.get(connector_id).is_some_and(|c| {
                        c.is_active()
                            && c.agent_id == record.agent_id
                            && c.channel.id == m.channel_id
                            && c.approved_user_ids.as_slice() == [m.sender.id.clone()]
                    })
            }),
        _ => false,
    }
}
pub(crate) fn owner_sender(
    state: &crate::state::DaemonState,
    record: &crate::runs::RunRecord,
    agent: &anima_core::AgentConfig,
    content: &anima_core::Content,
) -> Option<OwnerSender> {
    use crate::runs::{RunSource, RunStatus};
    if record.stop.is_some() || record.error.is_some() {
        return None;
    }
    let sender = match record.source {
        RunSource::Web | RunSource::Api => OwnerSender::Local,
        RunSource::Telegram if trusted_connector_message(state, record) => OwnerSender::Telegram,
        RunSource::Discord if trusted_connector_message(state, record) => OwnerSender::Discord,
        _ => return None,
    };
    // Check-in and nonconversational sessions stay out even for an API-created run.
    if state
        .sessions
        .get(&record.agent_id, &record.session_id)
        .is_some_and(|s| {
            !matches!(
                s.kind,
                crate::sessions::SessionKind::Chat
                    | crate::sessions::SessionKind::Telegram
                    | crate::sessions::SessionKind::Discord
            ) || s.parent_run_id.is_some()
                || s.parent_agent_id.is_some()
        })
    {
        return None;
    }
    let mut completed = record.clone();
    completed.status = RunStatus::Completed;
    let message = anima_core::Message {
        id: "standing-owner-classification".into(),
        agent_id: record.agent_id.clone(),
        room_id: record.session_id.clone(),
        content: content.clone(),
        role: anima_core::MessageRole::User,
        created_at_ms: record.started_at_ms.unwrap_or_default(),
    };
    crate::learning::counts_as_owner_turn(
        &completed,
        agent,
        Some(&message),
        Some("owner admission"),
        sender != OwnerSender::Local,
    )
    .then_some(sender)
}
pub(crate) fn conversation_started(
    state: &crate::state::DaemonState,
    record: &crate::runs::RunRecord,
) -> bool {
    let no_prior_run = !state
        .runs
        .for_session(&record.agent_id, &record.session_id)
        .iter()
        .any(|r| {
            r.id != record.id && r.session_id == record.session_id && r.started_at_ms.is_some()
        });
    let no_archive = state
        .sessions
        .get(&record.agent_id, &record.session_id)
        .is_none_or(|s| {
            s.summary.is_none() && s.pruned_through.is_none() && s.imported_from.is_none()
        });
    let no_message = state.agents.get(&record.agent_id).is_none_or(|a| {
        !a.messages()
            .iter()
            .any(|m| crate::sessions::session_id_for_room(&m.room_id) == record.session_id)
    });
    no_prior_run && no_archive && no_message
}
impl StandingAuthority {
    fn validate(&self, state: &crate::state::DaemonState, agent: &str) -> Result<(), ApiError> {
        if !self.owner_context
            || self.cancel.as_ref().is_some_and(|c| c.is_cancelled())
            || self.link.agent_id != agent
        {
            return Err(conflict());
        }
        let run = state
            .runs
            .get(&self.link.run_id)
            .filter(|r| {
                r.agent_id == agent
                    && r.session_id == self.link.session_id
                    && r.status == crate::runs::RunStatus::Running
                    && r.stop.is_none()
            })
            .ok_or_else(conflict)?;
        let owner = state.get_agent(agent).ok_or_else(conflict)?;
        let content = anima_core::Content {
            text: run.effective_input().text.clone(),
            ..Default::default()
        };
        if owner_sender(state, run, &owner.state.config, &content) != Some(OwnerSender::Local)
            || !owner.state.config.allows_tool(&self.call.name)
            || !matches!(
                self.call.name.as_str(),
                "create_standing_intent"
                    | "update_standing_intent"
                    | "cancel_standing_intent"
                    | "undo_standing_intent"
            )
        {
            return Err(conflict());
        }
        if !matches!(
            (
                state.approval_verdict(&owner.state, &self.link.session_id, &self.call),
                self.approved
            ),
            (crate::approvals::Verdict::Allow, _) | (crate::approvals::Verdict::Ask, true)
        ) {
            return Err(conflict());
        }
        Ok(())
    }
}
enum Mutation {
    Create(IntentSpec),
    Update {
        id: String,
        revision: u64,
        spec: IntentSpec,
    },
    Cancel {
        id: String,
        revision: u64,
    },
    Undo {
        change_id: String,
    },
}
impl StandingIntentService {
    pub(crate) fn new(state: SharedDaemonState, transactions: Arc<Mutex<()>>) -> Self {
        Self {
            state,
            transactions,
            admission: Arc::new(|| true),
        }
    }
    pub(crate) fn with_admission(mut self, admission: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.admission = admission;
        self
    }
    pub(crate) async fn request_shutdown(&self) {
        self.state.read().await.standing_intent_slots.close();
    }
    pub(crate) async fn shutdown(&self) {
        let slots = self.state.read().await.standing_intent_slots.clone();
        slots.close();
        while slots.available_permits() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    pub(crate) async fn list(&self, agent: &str) -> Result<serde_json::Value, ApiError> {
        let state = self.state.read().await;
        if state.get_agent(agent).is_none() {
            return Err(ApiError::not_found());
        }
        let intents: Vec<_> = state
            .standing_intents
            .intents
            .values()
            .filter(|r| r.agent_id == agent)
            .collect();
        let changes:Vec<_>=state.standing_intents.changes.values().filter(|c|c.after.agent_id==agent).map(|c|serde_json::json!({"id":c.id,"intentId":c.after.id,"undone":c.undone,"undoSupported":!c.undone&&state.standing_intents.intents.get(&c.after.id)==Some(&c.after)})).collect();
        Ok(serde_json::json!({"intents":intents,"changes":changes,"maxMatchesPerTurn":3}))
    }
    pub(crate) async fn create(
        &self,
        agent: String,
        spec: IntentSpec,
        authority: Option<StandingAuthority>,
    ) -> Result<IntentMutation, ApiError> {
        self.mutate(agent, Mutation::Create(spec), authority).await
    }
    pub(crate) async fn update(
        &self,
        agent: String,
        id: String,
        revision: u64,
        spec: IntentSpec,
        authority: Option<StandingAuthority>,
    ) -> Result<IntentMutation, ApiError> {
        self.mutate(agent, Mutation::Update { id, revision, spec }, authority)
            .await
    }
    pub(crate) async fn cancel(
        &self,
        agent: String,
        id: String,
        revision: u64,
        authority: Option<StandingAuthority>,
    ) -> Result<IntentMutation, ApiError> {
        self.mutate(agent, Mutation::Cancel { id, revision }, authority)
            .await
    }
    pub(crate) async fn undo(
        &self,
        agent: String,
        change_id: String,
        authority: Option<StandingAuthority>,
    ) -> Result<IntentMutation, ApiError> {
        self.mutate(agent, Mutation::Undo { change_id }, authority)
            .await
    }
    async fn mutate(
        &self,
        agent: String,
        mutation: Mutation,
        authority: Option<StandingAuthority>,
    ) -> Result<IntentMutation, ApiError> {
        if !(self.admission)() {
            return Err(conflict());
        }
        let permit = self
            .state
            .read()
            .await
            .standing_intent_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::conflict("Standing intention mutation is busy or closed"))?;
        let this = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _tx = this.transactions.lock().await;
            if !(this.admission)() {
                return Err(conflict());
            }
            let mut g = this.state.write().await;
            let owner = g.get_agent(&agent).ok_or_else(ApiError::not_found)?;
            if crate::agent_runs::is_helper_config(&owner.state.config) {
                return Err(conflict());
            }
            if g.control_plane_store.is_none() {
                return Err(unavailable());
            }
            if let Some(a) = authority {
                a.validate(&g, &agent)?;
            }
            let before = g.standing_intents.clone();
            let now = anima_core::primitives::now_millis();
            let output = match mutation {
                Mutation::Create(spec) => {
                    let (row, change) = g
                        .standing_intents
                        .create(&uuid::Uuid::new_v4().to_string(), &agent, spec, now)
                        .map_err(|_| {
                            ApiError::bad_request_static(
                                "Invalid standing intention or registry limit reached",
                            )
                        })?;
                    IntentMutation {
                        intent: Some(row),
                        change_id: change.id,
                    }
                }
                Mutation::Update { id, revision, spec } => {
                    if !g
                        .standing_intents
                        .intents
                        .get(&id)
                        .is_some_and(|r| r.agent_id == agent)
                    {
                        return Err(ApiError::not_found());
                    }
                    let (row, change) = g
                        .standing_intents
                        .update(&id, revision, spec, now)
                        .map_err(|_| conflict())?;
                    IntentMutation {
                        intent: Some(row),
                        change_id: change.id,
                    }
                }
                Mutation::Cancel { id, revision } => {
                    if !g
                        .standing_intents
                        .intents
                        .get(&id)
                        .is_some_and(|r| r.agent_id == agent)
                    {
                        return Err(ApiError::not_found());
                    }
                    let (row, change) = g
                        .standing_intents
                        .cancel(&id, revision, now)
                        .map_err(|_| conflict())?;
                    IntentMutation {
                        intent: Some(row),
                        change_id: change.id,
                    }
                }
                Mutation::Undo { change_id } => {
                    if !g
                        .standing_intents
                        .changes
                        .get(&change_id)
                        .is_some_and(|c| c.after.agent_id == agent)
                    {
                        return Err(ApiError::not_found());
                    }
                    IntentMutation {
                        intent: g
                            .standing_intents
                            .undo(&change_id, now)
                            .map_err(|_| conflict())?,
                        change_id,
                    }
                }
            };
            let save = g.control_plane_persist_request();
            drop(g);
            if let Err(error) = save.save().await {
                if !crate::control_plane_store::is_durability_uncertain(&error) {
                    this.state.write().await.standing_intents = before;
                } else {
                    // Retain the uncertain candidate but dispatch no further matches
                    // or mutations until a fresh restore establishes durable state.
                    this.state.read().await.standing_intent_slots.close();
                }
                return Err(unavailable());
            }
            Ok(output)
        })
        .await
        .unwrap_or_else(|_| Err(unavailable()))
    }
}
