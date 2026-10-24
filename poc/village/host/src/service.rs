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
        }
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
            return self.store.recover(
                id,
                &input.request_id,
                &payload,
                original,
                &game_input,
                &self.runtime(),
            );
        }
        match self.store.begin(id, &input.request_id, &payload)? {
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
                    let state = record.state;
                    let played = if let Some(engine) = &app.engine {
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
                    app.store.commit(&id, &input.request_id, &token, played)
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
