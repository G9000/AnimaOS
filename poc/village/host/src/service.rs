use crate::store::Admission;
use crate::{Error, Result, Store};
use anima_village::{
    choice_intent, reviewed_turn, GameInput, GameView, Language, NpcEngine, NpcId, RuntimeInfo,
    Traveler,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Interaction {
    pub request_id: String,
    #[serde(default)]
    pub message: String,
    pub choice_id: Option<String>,
    pub recovery_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc_id: Option<NpcId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Customization {
    pub request_id: String,
    pub traveler: Traveler,
}
#[derive(Clone)]
pub struct Service {
    pub store: Arc<Store>,
    pub engine: Option<Arc<dyn NpcEngine>>,
    pub deadline: Duration,
    pub unavailable_reason: Option<String>,
    pub runtime_info: RuntimeInfo,
    pub durable: Option<Arc<crate::engine_bridge::DurableVillage>>,
    pub(crate) workers: anima_harness::durable::hosted::HostWorkers,
    pub(crate) startup_lease: Option<Arc<anima_harness::durable::hosted::StorageLease>>,
    pub(crate) memory_edit_gate: Arc<tokio::sync::Mutex<()>>,
}
impl Service {
    pub fn new(store: Store, engine: Option<Arc<dyn NpcEngine>>, deadline: Duration) -> Self {
        let runtime_info = engine
            .as_ref()
            .map(|e| e.runtime())
            .unwrap_or_else(RuntimeInfo::unavailable);
        Self {
            store: Arc::new(store),
            engine,
            deadline,
            unavailable_reason: None,
            runtime_info,
            durable: None,
            workers: Default::default(),
            startup_lease: None,
            memory_edit_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
    /// Opt in only for newly admitted resident dialogue. Existing request
    /// mappings keep their execution version across retries and restarts.
    pub async fn with_durable(
        mut self,
        bridge: Arc<crate::engine_bridge::DurableVillage>,
    ) -> Result<Self> {
        bridge.attach(&self.store).await?;
        self.runtime_info = bridge.runtime();
        self.durable = Some(bridge);
        Ok(self)
    }
    pub fn with_admission(self, active: bool) -> Result<Self> {
        self.store.set_admission(active)?;
        Ok(self)
    }
    pub(crate) fn track(&self) -> Result<anima_harness::durable::hosted::HostWork> {
        self.workers
            .enter()
            .ok_or_else(|| Error::new(503, "The village host is shutting down"))
    }
    pub(crate) fn spawn_activity(&self, save: String) -> Result<()> {
        let work = self.track()?;
        let worker = self.clone();
        tokio::spawn(async move {
            let _work = work;
            let _ = worker.run_resident_activity_reserved(&save).await;
        });
        Ok(())
    }
    pub fn close_workers(&self) {
        self.workers.close();
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.close_workers();
        while !self.workers.is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if self.startup_lease.is_some() {
            if let Some(bridge) = &self.durable {
                bridge
                    .memory
                    .shutdown()
                    .await
                    .map_err(|_| Error::storage())?;
                bridge
                    .execution
                    .shutdown()
                    .await
                    .map_err(|_| Error::storage())?;
            }
        }
        Ok(())
    }
    pub fn runtime(&self) -> RuntimeInfo {
        self.runtime_info.clone()
    }
    pub fn create(&self, language: Language) -> Result<GameView> {
        self.store.create(language, self.runtime().reviewed())
    }
    pub fn get(&self, id: &str) -> Result<GameView> {
        self.store.get(id)
    }
    pub fn customize(&self, id: &str, mut input: Customization) -> Result<GameView> {
        if !valid_id(&input.request_id) {
            return Err(Error::new(
                400,
                "Use a requestId of 1–128 letters, numbers, hyphens or underscores",
            ));
        }
        input.traveler = input.traveler.validated().map_err(|e| Error::new(400, e))?;
        let payload = serde_json::to_string(&("traveler", &input))
            .map_err(|_| Error::new(400, "Invalid traveler"))?;
        self.store
            .customize(id, &input.request_id, &payload, input.traveler)
    }
    pub async fn interact(&self, id: &str, input: Interaction, visit: bool) -> Result<GameView> {
        let work = self.track()?;
        validate(&input, visit)?;
        let game_input = if visit {
            GameInput::Visit
        } else {
            GameInput::Turn {
                message: input.message.clone(),
                choice_id: input.choice_id.clone(),
            }
        };
        let game_input = match input.npc_id {
            Some(npc) => game_input.at(npc),
            None => game_input,
        };
        let payload = serde_json::to_string(&(visit, &input))
            .map_err(|_| Error::new(400, "Invalid interaction"))?;
        if let Some(original) = &input.recovery_of {
            let persistent = self.store.persistent_request(id, original)?;
            let callback_memories = if persistent {
                let facts = if self.store.needs_callback_recovery(
                    id,
                    &input.request_id,
                    original,
                    game_input.speaker(),
                )? {
                    if let Some(bridge) = &self.durable {
                        let ids = self.store.resident_identities(id)?;
                        bridge
                            .callback_memories(&self.store, id, &ids, game_input.speaker())
                            .await?
                    } else {
                        Default::default()
                    }
                } else {
                    Default::default()
                };
                // Preflight only avoids unnecessary recall. Every persistent
                // recovery carries an explicit view, including an empty one.
                Some(facts)
            } else {
                None
            };
            use anima_harness::anima_core::{execution_limits::SystemBudgetClock, ExecutionClock};
            let at_ms = self
                .durable
                .as_ref()
                .map(|bridge| bridge.clock.now_ms())
                .unwrap_or_else(|| SystemBudgetClock.now_ms());
            let (view, newly_committed) = self.store.recover(
                id,
                &input.request_id,
                &payload,
                original,
                &game_input,
                &self.runtime(),
                at_ms,
                callback_memories,
            )?;
            if !newly_committed {
                return Ok(view);
            }
            if let Some(bridge) = self.durable.as_ref().filter(|_| persistent) {
                let ids = self.store.resident_identities(id)?;
                bridge.project(&self.store, &ids).await?;
                if self.store.presence_active(id, bridge.clock.now_ms())? {
                    let _ = self.spawn_activity(id.to_owned());
                }
            }
            return Ok(view);
        }
        let profile = if self.durable.is_some() {
            crate::engine_bridge::EXECUTION_VERSION
        } else {
            "legacy"
        };
        match self
            .store
            .begin_profile(id, &input.request_id, &payload, profile)?
        {
            Admission::Replay(view) => Ok(*view),
            Admission::Pending => {
                let expires = tokio::time::Instant::now() + self.deadline + Duration::from_secs(2);
                loop {
                    if let Some(view) = self.store.committed(id, &input.request_id)? {
                        return Ok(view);
                    }
                    if self.store.retryable(id, &input.request_id)? {
                        return Err(retry_error());
                    }
                    if tokio::time::Instant::now() >= expires {
                        return Err(Error::new(409,"This request is still pending; use reviewed recovery with a new requestId"));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            Admission::Start { record, token } => {
                let app = self.clone();
                let id = id.to_owned();
                // The worker outlives a disconnected HTTP caller. Only its durable
                // generation token can commit; recovery revokes that token.
                tokio::spawn(async move {
                    let _work=work;
                    let state = record.state;
                    let persistent = app.store.persistent_request(&id, &input.request_id)?;
                    let played = if persistent && app.durable.is_some() {
                        let bridge = app.durable.as_ref().ok_or_else(Error::storage)?;
                        match bridge.play(app.store.clone(), &id, &input.request_id, &token, state.clone(), game_input.clone(), app.deadline.as_millis() as u64).await {
                            Ok(played) => played,
                            Err(error) => {
                                if let Some(winner) = app.store.release(&id, &input.request_id, &token)? { return Ok(winner); }
                                return Err(error);
                            }
                        }
                    } else if persistent {
                        app.store.release(&id, &input.request_id, &token)?;
                        return Err(Error::new(503, "This request requires its persistent resident engine; retry with that profile available or use reviewed recovery"));
                    } else if let Some(engine) = &app.engine {
                        match tokio::time::timeout(
                            app.deadline,
                            engine.play(state.clone(), game_input.clone()),
                        )
                        .await
                        {
                            Ok(Ok(played)) => played,
                            _ => {
                                if let Some(winner) =
                                    app.store.release(&id, &input.request_id, &token)?
                                {
                                    return Ok(winner);
                                }
                                return Err(retry_error());
                            }
                        }
                    } else {
                        match reviewed_turn(state, &game_input, &app.runtime()) {
                            Ok(played) => played,
                            Err(_) => {
                                if let Some(winner) =
                                    app.store.release(&id, &input.request_id, &token)?
                                {
                                    return Ok(winner);
                                }
                                return Err(retry_error());
                            }
                        }
                    };
                    let view = app.store.commit(&id, &input.request_id, &token, played)?;
                    if let Some(bridge) = &app.durable {
                        let ids = app.store.resident_identities(&id)?;
                        // The game result is already durable. Projection failure
                        // remains pending and blocks the next dependent turn.
                        bridge.project(&app.store, &ids).await?;
                        if app.store.presence_active(&id, bridge.clock.now_ms())? {
                            let _=app.spawn_activity(id.clone());
                        }
                    }
                    Ok(view)
                })
                .await
                .map_err(|_| {
                    Error::new(503, "The interaction worker stopped; use reviewed recovery")
                })?
            }
        }
    }
}
fn retry_error() -> Error {
    Error::new(503,"The model request could not complete. Retry this exact request or choose reviewed recovery; no game outcome was committed.")
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn validate(input: &Interaction, visit: bool) -> Result<()> {
    if !valid_id(&input.request_id)
        || input
            .recovery_of
            .as_deref()
            .is_some_and(|id| !valid_id(id) || id == input.request_id)
    {
        return Err(Error::new(
            400,
            "Use distinct request ids of 1–128 letters, numbers, hyphens or underscores",
        ));
    }
    if input.message.chars().count() > 1000 || input.message.contains('\0') {
        return Err(Error::new(
            400,
            "Message must contain at most 1000 characters",
        ));
    }
    if let Some(choice) = &input.choice_id {
        choice_intent(choice).map_err(|e| Error::new(400, e))?;
    }
    if visit {
        if !input.message.is_empty() || input.choice_id.is_some() || input.recovery_of.is_some() {
            return Err(Error::new(
                400,
                "A visit accepts only a requestId and optional npcId",
            ));
        }
    } else if input.choice_id.is_none() && input.message.trim().is_empty() {
        return Err(Error::new(
            400,
            "Type a message or select a reviewed choice",
        ));
    }
    if input.recovery_of.is_some() && input.choice_id.is_none() {
        return Err(Error::new(400, "Recovery requires a reviewed choice"));
    }
    Ok(())
}
