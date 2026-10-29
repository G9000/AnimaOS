//! Copy-on-write conversation revisions preserve the original action evidence.
use super::AgentRunCoordinator;
use crate::routes::ApiError;
use crate::sessions::{views, SessionKind, SessionOrigin, SessionRecord, TitleSource};
use anima_core::{DataValue, Message, MessageRole};

pub(crate) struct ChatRevision {
    pub(crate) session_id: String,
    pub(crate) retry_prompt: Option<String>,
    pub(crate) retry_document_edit_context:
        Option<crate::document_edits::preview_context::PreviewEditContext>,
    pub(crate) attachments_omitted: bool,
}

fn revision_prefix(
    mut messages: Vec<Message>,
    mode: &str,
    through: Option<&str>,
) -> Result<(Vec<Message>, Option<String>, bool), ApiError> {
    if !matches!(mode, "branch" | "undo" | "retry") {
        return Err(ApiError::bad_request_static(
            "mode must be branch, undo, or retry",
        ));
    }
    if mode != "branch" && through.is_some() {
        return Err(ApiError::bad_request_static(
            "Only branch accepts throughMessageId",
        ));
    }
    if let Some(anchor) = through {
        let index = messages
            .iter()
            .position(|message| message.id == anchor)
            .ok_or_else(|| ApiError::conflict("The branch message is not in this conversation"))?;
        // Never copy half a tool exchange: the boundary is the end of its user turn.
        let end = messages
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, message)| {
                message.role == MessageRole::User
                    && !message
                        .content
                        .metadata
                        .as_ref()
                        .is_some_and(|m| m.contains_key("steer"))
            })
            .map_or(messages.len(), |(index, _)| index);
        messages.truncate(end);
    }
    let mut retry = None;
    let mut attachments_omitted = false;
    if mode != "branch" {
        let index = messages
            .iter()
            .rposition(|message| {
                message.role == MessageRole::User
                    && !message
                        .content
                        .metadata
                        .as_ref()
                        .is_some_and(|m| m.contains_key("steer"))
            })
            .ok_or_else(|| ApiError::conflict("There is no exchange to undo"))?;
        if mode == "retry" {
            retry = Some(original_input_text(&messages[index]));
            attachments_omitted = messages[index]
                .content
                .attachments
                .as_ref()
                .is_some_and(|a| !a.is_empty());
        }
        messages.truncate(index);
    }
    Ok((messages, retry, attachments_omitted))
}

fn original_input_text(message: &Message) -> String {
    message
        .content
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(crate::attachments::ATTACHMENT_INPUT_TEXT_KEY))
        .and_then(|value| match value {
            DataValue::String(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| message.content.text.clone())
}

impl AgentRunCoordinator {
    fn revision_source<'a>(
        &self,
        state: &'a crate::state::DaemonState,
        agent: &str,
        session: &str,
    ) -> Result<&'a SessionRecord, ApiError> {
        let record = state
            .sessions
            .get(agent, session)
            .ok_or_else(ApiError::not_found)?;
        if record.kind != SessionKind::Chat
            || !matches!(record.origin, SessionOrigin::Web | SessionOrigin::Api)
            || record.parent_agent_id.is_some()
            || record.parent_run_id.is_some()
            || self.is_being_deleted(agent)
            || state
                .agents
                .get(agent)
                .is_none_or(|runtime| super::is_helper_config(runtime.config()))
        {
            return Err(ApiError::conflict(
                "Revisions require an ordinary owner chat",
            ));
        }
        Ok(record)
    }
    pub(crate) async fn revise_chat(
        &self,
        agent: &str,
        session: &str,
        mode: &str,
        through: Option<&str>,
    ) -> Result<ChatRevision, ApiError> {
        let coordinator = self.clone();
        let (agent, session, mode, through) = (
            agent.to_owned(),
            session.to_owned(),
            mode.to_owned(),
            through.map(str::to_owned),
        );
        tokio::spawn(async move {
            coordinator
                .revise_chat_owned(&agent, &session, &mode, through.as_deref())
                .await
        })
        .await
        .map_err(|_| ApiError::service_unavailable("The revision worker stopped"))?
    }

    async fn revise_chat_owned(
        &self,
        agent: &str,
        session: &str,
        mode: &str,
        through: Option<&str>,
    ) -> Result<ChatRevision, ApiError> {
        let room = {
            let guard = self.state.read().await;
            self.revision_source(&guard, agent, session)?
                .room_id()
                .to_string()
        };
        let _room = self
            .try_reserve_room(agent, &room)
            .ok_or_else(|| ApiError::conflict("Wait for the reply before revising this chat"))?;
        let _transaction = self.control_plane_transaction().await;
        {
            let guard = self.state.read().await;
            if self.revision_source(&guard, agent, session)?.room_id() != room {
                return Err(ApiError::conflict("The source conversation changed"));
            }
            if super::main_chat_clear::clear_pending(&guard, agent, session)
                || guard.runs.active_count_for_session(agent, session) > 0
            {
                return Err(ApiError::conflict(
                    "Wait for queued or active work before revising this chat",
                ));
            }
        }
        let (source, _, messages) = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            views::full_transcript(&self.state, agent, session),
        )
        .await
        .map_err(|_| ApiError::service_unavailable("Reading the conversation timed out"))?
        .map_err(|_| {
            ApiError::service_unavailable("The complete conversation could not be read")
        })?;
        if messages.len() > 5_000
            || serde_json::to_vec(&messages).map_or(true, |bytes| bytes.len() > 8 * 1024 * 1024)
        {
            return Err(ApiError::conflict(
                "This conversation is too large to branch; export it instead",
            ));
        }
        let retry_document_edit_context = if mode == "retry" {
            let value = messages
                .iter()
                .rfind(|message| {
                    message.role == MessageRole::User
                        && !message
                            .content
                            .metadata
                            .as_ref()
                            .is_some_and(|metadata| metadata.contains_key("steer"))
                })
                .and_then(|message| message.content.metadata.as_ref())
                .and_then(|metadata| metadata.get("documentEditContext"));
            match value {
                None => None,
                Some(DataValue::String(text)) => {
                    let context: crate::document_edits::preview_context::PreviewEditContext = serde_json::from_str(text).map_err(|_| ApiError::conflict("Selected document context is unavailable; select the content again"))?;
                    context.validate().map_err(ApiError::conflict)?;
                    Some(context)
                }
                Some(_) => {
                    return Err(ApiError::conflict(
                        "Selected document context is invalid; select the content again",
                    ))
                }
            }
        } else {
            None
        };
        let (mut messages, retry_prompt, mut attachments_omitted) =
            revision_prefix(messages, mode, through)?;
        let now = anima_core::primitives::now_millis();
        let id = crate::sessions::new_chat_session_id();
        let mut record = SessionRecord::new(
            agent,
            &id,
            SessionKind::Chat,
            SessionOrigin::Web,
            crate::sessions::clean_owner_title(&format!(
                "{} · revision",
                source.title.chars().take(100).collect::<String>()
            ))
            .map_err(ApiError::bad_request_static)?,
            TitleSource::Owner,
            now,
        );
        record.parent_session_id = Some(source.id.clone());
        record.model_preferences = source.model_preferences.clone();
        record.last_read_at_ms = Some(now);
        let message_namespace = uuid::Uuid::new_v4();
        for (index, message) in messages.iter_mut().enumerate() {
            if message
                .content
                .attachments
                .as_ref()
                .is_some_and(|files| !files.is_empty())
            {
                attachments_omitted = true;
                message.content.text = format!("{}\n\n[Attachments remain in the original chat and are unavailable in this revision.]", original_input_text(message));
                message.content.attachments = None;
            }
            message.id = format!("msg:revision:{message_namespace}:{index:06}");
            message.agent_id = agent.into();
            message.room_id = id.clone();
            // These are historical context, not live request identities or authority.
            let metadata = message
                .content
                .metadata
                .get_or_insert_with(Default::default);
            metadata.retain(|key, _| {
                !matches!(
                    key.as_str(),
                    "runId"
                        | "clientRequestId"
                        | "acceptedAtMs"
                        | "stepId"
                        | "taskResult"
                        | "attachmentRecords"
                        | "attachmentInputText"
                        | "documentExtracts"
                        | "communication"
                )
            });
            metadata.insert(
                "revisionSourceSession".into(),
                DataValue::String(source.id.clone()),
            );
        }
        let (undo, persist, history) = {
            let mut guard = self.state.write().await;
            if self.revision_source(&guard, agent, session)?.room_id() != room {
                return Err(ApiError::conflict("The source conversation changed"));
            }
            if !guard.session_limiter.try_acquire(agent, now) {
                return Err(ApiError::conflict("Too many new chats; try again shortly"));
            }
            let undo = guard
                .commit_imported_session(record, messages.clone(), None)
                .ok_or_else(ApiError::not_found)?;
            (
                undo,
                guard.control_plane_persist_request(),
                guard.history.clone(),
            )
        };
        if let Err(error) = persist.save().await {
            self.state.write().await.revert_imported_session(undo);
            return Err(ApiError::service_unavailable(error.to_string()));
        }
        history.enqueue_committed(agent, &id, &messages);
        self.state.read().await.publish_session_event(
            agent,
            &id,
            crate::live::LiveEventBody::SessionCreated,
        );
        Ok(ChatRevision {
            session_id: id,
            retry_prompt,
            retry_document_edit_context,
            attachments_omitted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        add_chat, chat_request, coordinator_with, ScriptedModel, Step,
    };
    use super::*;

    #[test]
    fn chat_revision_retry_restores_raw_input_and_marks_files_for_reattachment() {
        let mut user = crate::sessions::test_support::message(
            "agent",
            "user",
            "chat:test",
            MessageRole::User,
            "Look at this\nAttached image: scan.png",
            1,
        );
        user.content.metadata = Some(std::collections::BTreeMap::from([(
            crate::attachments::ATTACHMENT_INPUT_TEXT_KEY.into(),
            DataValue::String("Look at this".into()),
        )]));
        user.content.attachments = Some(vec![anima_core::Attachment {
            attachment_type: anima_core::AttachmentType::Image,
            name: "scan.png".into(),
            data: "upload-id".into(),
            mime_type: Some("image/png".into()),
        }]);
        let (prefix, retry, omitted) = revision_prefix(vec![user], "retry", None).unwrap();
        assert!(prefix.is_empty());
        assert_eq!(retry.as_deref(), Some("Look at this"));
        assert!(omitted);
    }

    #[tokio::test]
    async fn chat_revision_rechecks_source_scope_after_waiting_for_transaction() {
        let (runs, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&runs, &agent, "chat:source").await;
        let transaction = runs.control_plane_transaction().await;
        let revision = {
            let runs = runs.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                runs.revise_chat(&agent, "chat:source", "branch", None)
                    .await
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if runs.try_reserve_room(&agent, "chat:source").is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        runs.state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:source")
            .unwrap()
            .origin = SessionOrigin::Telegram;
        drop(transaction);
        assert!(revision.await.unwrap().is_err());
        assert_eq!(
            runs.state
                .read()
                .await
                .sessions
                .records()
                .filter(|s| s.agent_id == agent)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn chat_revision_undo_preserves_source_and_main_identity() {
        let (runs, agent) = coordinator_with(ScriptedModel::new(vec![
            Step::Text(vec!["first"]),
            Step::Text(vec!["second"]),
        ]))
        .await;
        add_chat(&runs, &agent, "chat:source").await;
        runs.state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:source")
            .unwrap()
            .main_chat = true;
        runs.run(chat_request(&agent, "chat:source", "one"))
            .await
            .unwrap();
        runs.run(chat_request(&agent, "chat:source", "two"))
            .await
            .unwrap();
        let before = views::full_transcript(&runs.state, &agent, "chat:source")
            .await
            .unwrap()
            .2;
        let revised = runs
            .revise_chat(&agent, "chat:source", "retry", None)
            .await
            .unwrap();
        assert_eq!(revised.retry_prompt.as_deref(), Some("two"));
        let (record, _, copied) = views::full_transcript(&runs.state, &agent, &revised.session_id)
            .await
            .unwrap();
        assert!(!record.main_chat);
        assert_eq!(record.parent_session_id.as_deref(), Some("chat:source"));
        assert_eq!(copied.len(), 2);
        assert!(copied.iter().all(|m| !before.iter().any(|b| b.id == m.id)));
        assert!(copied.iter().all(|m| !m
            .content
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.contains_key("runId"))));
        assert_eq!(
            views::full_transcript(&runs.state, &agent, "chat:source")
                .await
                .unwrap()
                .2,
            before
        );
        assert_eq!(
            runs.state
                .read()
                .await
                .runs
                .for_session(&agent, "chat:source")
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn chat_revision_refuses_busy_room_and_invalid_boundary_without_creating_chat() {
        let (runs, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&runs, &agent, "chat:source").await;
        let lease = runs.try_reserve_room(&agent, "chat:source").unwrap();
        assert!(runs
            .revise_chat(&agent, "chat:source", "branch", None)
            .await
            .is_err());
        drop(lease);
        assert!(runs
            .revise_chat(&agent, "chat:source", "branch", Some("missing"))
            .await
            .is_err());
        assert!(runs
            .revise_chat(&agent, "chat:source", "undo", None)
            .await
            .is_err());
        assert_eq!(
            runs.state
                .read()
                .await
                .sessions
                .records()
                .filter(|s| s.agent_id == agent)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn chat_revision_copies_cold_history_and_keeps_tool_exchange_complete() {
        use crate::history::HistoryMessage;
        use crate::sessions::test_support::message;
        let (runs, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&runs, &agent, "chat:source").await;
        let original = vec![
            message(&agent, "m1", "chat:source", MessageRole::User, "one", 1),
            message(
                &agent,
                "m2",
                "chat:source",
                MessageRole::Assistant,
                "calling tool",
                1,
            ),
            message(
                &agent,
                "m3",
                "chat:source",
                MessageRole::Tool,
                "completed action",
                1,
            ),
            message(
                &agent,
                "m4",
                "chat:source",
                MessageRole::Assistant,
                "first answer",
                1,
            ),
            message(&agent, "m5", "chat:source", MessageRole::User, "two", 2),
            message(
                &agent,
                "m6",
                "chat:source",
                MessageRole::Assistant,
                "second answer",
                3,
            ),
        ];
        let store = runs.state.read().await.history.store();
        store
            .upsert_messages(
                &original
                    .iter()
                    .map(|message| HistoryMessage {
                        agent_id: agent.clone(),
                        session_id: "chat:source".into(),
                        hidden: false,
                        message: message.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
            .await
            .unwrap();
        let revision = runs
            .revise_chat(&agent, "chat:source", "branch", Some("m2"))
            .await
            .unwrap();
        let (_, _, copied) = views::full_transcript(&runs.state, &agent, &revision.session_id)
            .await
            .unwrap();
        assert_eq!(
            copied
                .iter()
                .map(|message| message.content.text.as_str())
                .collect::<Vec<_>>(),
            ["one", "calling tool", "completed action", "first answer"]
        );
        assert_eq!(
            views::full_transcript(&runs.state, &agent, "chat:source")
                .await
                .unwrap()
                .2,
            original
        );
    }

    #[tokio::test]
    async fn chat_revision_failed_save_rolls_back_new_session_without_mutating_original() {
        let (runs, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&runs, &agent, "chat:source").await;
        runs.run(chat_request(&agent, "chat:source", "one"))
            .await
            .unwrap();
        let before = views::full_transcript(&runs.state, &agent, "chat:source")
            .await
            .unwrap()
            .2;
        let gate = runs
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let operation = {
            let runs = runs.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                runs.revise_chat(&agent, "chat:source", "branch", None)
                    .await
            })
        };
        gate.entered.acquire().await.unwrap().forget();
        gate.release.add_permits(1);
        assert!(operation.await.unwrap().is_err());
        assert_eq!(
            views::full_transcript(&runs.state, &agent, "chat:source")
                .await
                .unwrap()
                .2,
            before
        );
        assert_eq!(
            runs.state
                .read()
                .await
                .sessions
                .records()
                .filter(|s| s.agent_id == agent)
                .count(),
            1
        );
    }
}
