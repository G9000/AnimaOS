//! Detached text-only snapshot answers; no run admission or transcript commit.
use super::AgentRunCoordinator;
use crate::routes::ApiError;
use crate::sessions::{hidden_message_ids, SessionKind};
use crate::usage::{
    metered::{record_secondary, Metered, SecondaryCall},
    UsageSource,
};
use anima_core::{
    interaction::side_question_request, primitives::now_millis, Content, Message, MessageRole,
    ModelAdapter,
};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use utoipa::ToSchema;

static SIDE_QUESTION_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SideQuestionAnswer {
    pub(crate) answer: String,
    pub(crate) snapshot_at_ms: u64,
    pub(crate) snapshot_through_message_id: Option<String>,
}
impl AgentRunCoordinator {
    pub(crate) async fn side_question(
        &self,
        agent: &str,
        session: &str,
        question: &str,
    ) -> Result<SideQuestionAnswer, ApiError> {
        let _slot = SIDE_QUESTION_SLOTS.try_acquire().map_err(|_| {
            ApiError::too_many_requests("Two side questions are already in progress")
        })?;
        let (config, adapter, request, snapshot_through_message_id, snapshot_at_ms) = {
            let state = self.state.read().await;
            let record = state
                .sessions
                .get(agent, session)
                .filter(|record| {
                    record.kind == SessionKind::Chat
                        && record.parent_run_id.is_none()
                        && record.parent_agent_id.is_none()
                })
                .ok_or_else(ApiError::not_found)?;
            let runtime = state
                .agents
                .get(agent)
                .filter(|runtime| !super::is_helper_config(runtime.config()))
                .ok_or_else(ApiError::not_found)?;
            let room = record.room_id();
            let room_messages: Vec<_> = runtime
                .messages()
                .iter()
                .filter(|message| message.room_id == room && message.agent_id == agent)
                .collect();
            let hidden = hidden_message_ids(room_messages.iter().copied());
            let mut messages: Vec<_> = room_messages
                .into_iter()
                .rev()
                .filter(|message| !hidden.contains(&message.id))
                .take(100)
                .cloned()
                .collect();
            messages.reverse();
            let through = messages.last().map(|message| message.id.clone());
            // The canonical transcript commits when a run finishes. Capture
            // its accepted owner input separately so a first-turn side
            // question can still explain the task currently in progress.
            if let Some(active) = state
                .runs
                .for_session(agent, session)
                .into_iter()
                .find(|run| {
                    run.status.is_in_flight()
                        && run.parent_run_id.is_none()
                        && (run.system_followup != Some(true) || run.recovery_of.is_some())
                        && matches!(
                            run.source,
                            crate::runs::RunSource::Web | crate::runs::RunSource::Api
                        )
                })
            {
                let owner_input: String = active.input.text.chars().take(1200).collect();
                let evidence: Vec<_> = active.action_receipts.iter().rev().take(3).map(|receipt| serde_json::json!({"tool":receipt.name,"status":receipt.status,"result":receipt.result_preview.as_deref().map(|text|text.chars().take(150).collect::<String>())})).collect();
                let progress = state
                    .live
                    .runs()
                    .view(&active.id)
                    .map(|view| view.tools)
                    .unwrap_or_default()
                    .into_iter()
                    .rev()
                    .take(3)
                    .map(|tool| format!("{}: {}", tool.name, tool.status))
                    .collect::<Vec<_>>()
                    .join(", ");
                let evidence: String = serde_json::to_string(&evidence)
                    .unwrap_or_default()
                    .chars()
                    .take(600)
                    .collect();
                let progress: String = progress.chars().take(150).collect();
                messages.push(Message { id: format!("side-snapshot:{}", active.id), agent_id: agent.into(), room_id: room.into(), role: MessageRole::User, created_at_ms: now_millis(), content: Content { text: format!("Active owner request (snapshot data): {owner_input}\nRecorded action evidence (effects not verified): {evidence}\nRecorded tool progress: {progress}"), ..Content::default() } });
            }
            let request = side_question_request(
                agent,
                room,
                &messages,
                record.summary.as_ref().map(|summary| summary.text.as_str()),
                question,
            )
            .map_err(ApiError::bad_request)?;
            (
                runtime.config().clone(),
                Arc::clone(&state.model_adapter),
                request,
                through,
                now_millis(),
            )
        };
        let meter = Metered::new(adapter);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
        let response = tokio::time::timeout_at(
            deadline,
            crate::side_model::side_call(&self.state, &config, deadline, |mut config| {
                config.tools = None;
                let (meter, request) = (&meter, &request);
                async move { meter.generate(&config, request).await }
            }),
        )
        .await;
        // Account for completed calls even when their response is unusable.
        record_secondary(
            &self.state,
            self.control_plane_transactions(),
            &meter,
            SecondaryCall::for_config(agent, Some(session), UsageSource::SideQuestion, &config),
        )
        .await
        .map_err(|_| {
            ApiError::service_unavailable("The side question's usage could not be saved")
        })?;
        let response = response
            .map_err(|_| ApiError::service_unavailable("The side question timed out"))?
            .map_err(|_| {
                ApiError::service_unavailable("The side question could not be answered")
            })?;
        if response
            .tool_calls
            .as_ref()
            .is_some_and(|calls| !calls.is_empty())
            || response.content.text.trim().is_empty()
            || response.content.text.len() > 32 * 1024
        {
            return Err(ApiError::service_unavailable(
                "The side question did not return a bounded text answer",
            ));
        }
        // An answer for a deleted or replaced session never appears later.
        if !self.state.read().await.sessions.contains(agent, session) {
            return Err(ApiError::not_found());
        }
        Ok(SideQuestionAnswer {
            answer: response.content.text.trim().to_owned(),
            snapshot_at_ms,
            snapshot_through_message_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_runs::test_support::{
        add_chat, calculate_call, chat_request, coordinator_with, ScriptedModel, Step,
    };

    #[tokio::test]
    async fn interaction_side_question_finishes_while_the_main_run_continues() {
        for recovery_child in [false, true] {
            let model = ScriptedModel::with_secondary(
                vec![Step::Hold(vec!["Working"])],
                vec![Step::Text(vec!["Snapshot answer"])],
            );
            let (coordinator, agent) = coordinator_with(model.clone()).await;
            add_chat(&coordinator, &agent, "chat:side").await;
            let task = tokio::spawn({
                let coordinator = coordinator.clone();
                let agent = agent.clone();
                async move {
                    coordinator
                        .run(chat_request(&agent, "chat:side", "Main work"))
                        .await
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while model.requests().is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            if recovery_child {
                let mut state = coordinator.state.write().await;
                let active_id = state
                    .runs
                    .for_session(&agent, "chat:side")
                    .into_iter()
                    .find(|run| run.status.is_in_flight())
                    .unwrap()
                    .id
                    .clone();
                let active = state.runs.get_mut(&active_id).unwrap();
                active.system_followup = Some(true);
                active.recovery_of = Some("reviewed-original".into());
                active.recovery_attempt = 1;
            }
            let before = coordinator
                .state
                .read()
                .await
                .agents
                .get(&agent)
                .unwrap()
                .messages()
                .to_vec();
            let answer = coordinator
                .side_question(&agent, "chat:side", "Why?")
                .await
                .unwrap();
            assert_eq!(answer.answer, "Snapshot answer");
            assert!(!task.is_finished());
            assert_eq!(
                coordinator
                    .state
                    .read()
                    .await
                    .agents
                    .get(&agent)
                    .unwrap()
                    .messages(),
                before
            );
            assert!(model.secondary_configs()[0].tools.is_none());
            assert!(model.secondary_requests()[0].messages[0]
                .content
                .text
                .contains("Main work"));
            task.abort();
            let _ = task.await;
        }
    }
    #[tokio::test]
    async fn interaction_side_question_rejects_tools_and_cross_agent_session() {
        let model = ScriptedModel::with_secondary(
            vec![],
            vec![Step::Tools(vec![calculate_call("call", "1+1")])],
        );
        let (coordinator, agent) = coordinator_with(model).await;
        add_chat(&coordinator, &agent, "chat:side").await;
        assert!(coordinator
            .side_question(&agent, "chat:side", "Why?")
            .await
            .is_err());
        assert!(coordinator
            .side_question("other", "chat:side", "Why?")
            .await
            .is_err());
    }
}
