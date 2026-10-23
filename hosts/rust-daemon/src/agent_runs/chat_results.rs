//! The coordinator's transaction lane makes result content and pins durable.
use super::AgentRunCoordinator;
use crate::chat_results::{ChatResult, SaveResultInput};
use crate::routes::ApiError;
use crate::runs::RunLink;

#[derive(Clone)]
enum Change {
    Save {
        input: SaveResultInput,
        run: String,
        call: String,
        link: Option<RunLink>,
    },
    Pin {
        id: String,
        revision: u64,
        pinned: bool,
    },
}

#[cfg(test)]
#[path = "chat_results_tests.rs"]
mod tests;

impl AgentRunCoordinator {
    pub(crate) async fn list_chat_results_from_run(
        &self,
        link: &RunLink,
    ) -> Result<Vec<ChatResult>, ApiError> {
        let _transaction = self.control_plane_transaction().await;
        let state = self.state.read().await;
        state
            .ensure_profile_owned()
            .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
        let session =
            super::chat_controls::validate_owner_chat(&state, link).map_err(ApiError::conflict)?;
        crate::chat_results::validate_session(session).map_err(ApiError::service_unavailable)?;
        Ok(session.chat_results.clone())
    }

    pub(crate) async fn save_chat_result_from_run(
        &self,
        link: &RunLink,
        call: &str,
        input: SaveResultInput,
    ) -> Result<ChatResult, ApiError> {
        let generation = {
            let state = self.state.read().await;
            super::chat_controls::validate_owner_chat(&state, link)
                .map_err(ApiError::conflict)?
                .clear_generation
        };
        self.change_chat_result(
            &link.agent_id,
            &link.session_id,
            generation,
            Change::Save {
                input,
                run: link.run_id.clone(),
                call: call.into(),
                link: Some(link.clone()),
            },
        )
        .await
    }

    pub(crate) async fn save_chat_result_owner(
        &self,
        agent: &str,
        session: &str,
        generation: u64,
        input: SaveResultInput,
    ) -> Result<ChatResult, ApiError> {
        let call = input.request_key.clone();
        self.change_chat_result(
            agent,
            session,
            generation,
            Change::Save {
                input,
                run: "owner".into(),
                call,
                link: None,
            },
        )
        .await
    }

    pub(crate) async fn pin_chat_result_owner(
        &self,
        agent: &str,
        session: &str,
        generation: u64,
        id: &str,
        revision: u64,
        pinned: bool,
    ) -> Result<ChatResult, ApiError> {
        self.change_chat_result(
            agent,
            session,
            generation,
            Change::Pin {
                id: id.into(),
                revision,
                pinned,
            },
        )
        .await
    }

    async fn change_chat_result(
        &self,
        agent: &str,
        session: &str,
        generation: u64,
        change: Change,
    ) -> Result<ChatResult, ApiError> {
        let coordinator = self.clone();
        let (agent, session) = (agent.to_string(), session.to_string());
        // Complete persistence or rollback even if the HTTP/tool caller drops.
        tokio::spawn(async move {
            coordinator
                .change_chat_result_saved(&agent, &session, generation, change)
                .await
        })
        .await
        .map_err(|_| {
            ApiError::service_unavailable("Result save worker stopped; outcome is unknown")
        })?
    }

    async fn change_chat_result_saved(
        &self,
        agent: &str,
        session_id: &str,
        generation: u64,
        change: Change,
    ) -> Result<ChatResult, ApiError> {
        super::ownership::verify(&self.state)
            .await
            .map_err(ApiError::service_unavailable)?;
        let _transaction = self.control_plane_transaction().await;
        let (previous, result, persist) = {
            let mut state = self.state.write().await;
            state
                .ensure_profile_owned()
                .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
            if state.control_plane_store.is_none() {
                return Err(ApiError::service_unavailable(
                    "Configure durable storage before saving results",
                ));
            }
            if let Change::Save {
                link: Some(link), ..
            } = &change
            {
                super::chat_controls::validate_owner_chat(&state, link)
                    .map_err(ApiError::conflict)?;
            }
            if !state.agents.contains_key(agent) {
                return Err(ApiError::not_found());
            }
            let session = state
                .sessions
                .get_mut(agent, session_id)
                .ok_or_else(ApiError::not_found)?;
            if session.clear_generation != generation
                || session.pending_clear_run_id.is_some()
                || session.archived
                || session.kind != crate::sessions::SessionKind::Chat
                || !matches!(
                    session.origin,
                    crate::sessions::SessionOrigin::Web | crate::sessions::SessionOrigin::Api
                )
            {
                return Err(ApiError::conflict(
                    "The conversation changed or cannot save results",
                ));
            }
            let previous = session.chat_results.clone();
            let now = anima_core::primitives::now_millis();
            let result = match change {
                Change::Save {
                    input, run, call, ..
                } => {
                    input.payload.validate().map_err(ApiError::bad_request)?;
                    crate::chat_results::upsert(session, input, &run, &call, now)
                }
                Change::Pin {
                    id,
                    revision,
                    pinned,
                } => crate::chat_results::pin(session, &id, revision, pinned, now),
            }
            .map_err(ApiError::conflict)?;
            (previous, result, state.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            if let Some(session) = self.state.write().await.sessions.get_mut(agent, session_id) {
                session.chat_results = previous;
            }
            return Err(ApiError::service_unavailable(format!(
                "Result save was not acknowledged: {error}"
            )));
        }
        self.state.read().await.publish_session_event(
            agent,
            session_id,
            crate::live::LiveEventBody::SessionUpdated,
        );
        Ok(result)
    }
}
