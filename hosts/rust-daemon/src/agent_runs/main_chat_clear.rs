//! Owner-only conversation reset. A running model requests; the host settles.
use super::AgentRunCoordinator;
use crate::{history::HistoryDeletion, live::LiveEventBody, routes::ApiError, runs::RunLink};

pub(crate) const CLEAR_PENDING: &str =
    "The main chat is being cleared; try again after cleanup finishes";
pub(crate) const CLEAR_DONE: &str =
    "Conversation cleared. Your companion and saved memories are unchanged.";
const HELPERS_BUSY: &str =
    "Background tasks for this chat are still pending; wait for them to settle before clearing";

fn helpers_pending(state: &crate::state::DaemonState, agent: &str, session: &str) -> bool {
    state.runs.async_helpers_for_owner(agent).iter().any(|run| {
        run.async_helper.as_ref().is_some_and(|helper| {
            helper.owner_session_id == session
                && (!run.status.is_terminal() || helper.completion.is_unresolved())
        })
    })
}

pub(crate) fn clear_pending(state: &crate::state::DaemonState, agent: &str, session: &str) -> bool {
    state
        .sessions
        .get(agent, session)
        .is_some_and(|s| s.pending_clear_run_id.is_some())
        || state
            .pending_history_deletions
            .iter()
            .any(|d| d.agent_id == agent && d.session_id.as_deref().is_none_or(|id| id == session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::test_support::{
        add_chat, chat_request, coordinator_with, ScriptedModel, Step,
    };

    async fn fixture() -> (AgentRunCoordinator, String) {
        let (coordinator, agent) =
            coordinator_with(ScriptedModel::new(vec![Step::Text(vec!["old reply"])])).await;
        add_chat(&coordinator, &agent, "chat:main").await;
        add_chat(&coordinator, &agent, "chat:other").await;
        coordinator
            .state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:main")
            .unwrap()
            .main_chat = true;
        coordinator
            .run(chat_request(&agent, "chat:main", "private old history"))
            .await
            .unwrap();
        let memory = coordinator.state.read().await.memory_handle();
        crate::core_memory::apply_edit(
            &mut *memory.write().await,
            crate::core_memory::CoreAgent {
                id: &agent,
                name: "Companion",
            },
            crate::core_memory::CoreBlock::User,
            crate::core_memory::CoreEdit::Add {
                text: "The owner prefers jasmine tea.".into(),
            },
            crate::core_memory::ScanMode::Refuse,
        )
        .unwrap();
        {
            let mut state = coordinator.state.write().await;
            let run = state.runs.for_session(&agent, "chat:main")[0].id.clone();
            let questions = anima_core::interaction::normalize_questions(vec![
                anima_core::interaction::ClarificationQuestionInput {
                    question: "Old conversation question?".into(),
                    choices: vec!["Yes".into(), "No".into()],
                    multi_select: false,
                },
            ])
            .unwrap();
            state
                .clarifications
                .open(&agent, "chat:main", &run, "old-question", questions, 1)
                .unwrap();
            state.clarifications.cancel_run(&run, 2);
        }
        (coordinator, agent)
    }

    #[tokio::test]
    async fn main_chat_clear_invalidates_legacy_inputs_waiting_for_the_room() {
        let (coordinator, agent) = fixture().await;
        let reservation = coordinator.try_reserve_room(&agent, "chat:main").unwrap();
        let request = chat_request(&agent, "chat:main", "old waiting input");
        let mut waiting = Box::pin(coordinator.acquire_ticket(
            &request,
            super::super::PermitMode::TryNow,
            None,
            None,
        ));
        assert!(futures::poll!(&mut waiting).is_pending());
        // This test owns the lease, as the running self-reset turn does.
        coordinator
            .clear_main_chat_locked(&agent, "chat:main")
            .await
            .unwrap();
        drop(reservation);
        assert!(waiting.await.is_err());
        assert!(coordinator
            .state
            .read()
            .await
            .get_agent(&agent)
            .unwrap()
            .messages
            .is_empty());
    }

    #[tokio::test]
    async fn main_chat_clear_fences_a_helper_launch_waiting_on_the_clear_save() {
        use std::sync::{atomic::AtomicUsize, Arc};
        let (coordinator, agent) = fixture().await;
        let reservation = coordinator.try_reserve_room(&agent, "chat:main").unwrap();
        let parent = {
            let mut state = coordinator.state.write().await;
            let mut config = super::super::test_support::lead_config("Companion");
            config.tools.as_mut().unwrap().push(
                crate::tools::ToolRegistry::new()
                    .descriptor("spawn_helper_async")
                    .unwrap(),
            );
            state.restore_agent_config(&agent, config);
            let run = state.runs.for_session(&agent, "chat:main")[0].id.clone();
            state.runs.get_mut(&run).unwrap().status = crate::runs::RunStatus::Running;
            state.runs.get_mut(&run).unwrap().reply_sha256 = None;
            state.runs.get_mut(&run).unwrap().turn = None;
            state.live.runs().register(&run);
            RunLink {
                agent_id: agent.clone(),
                session_id: "chat:main".into(),
                run_id: run,
            }
        };
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(false);
        let clearing = {
            let coordinator = coordinator.clone();
            let parent = parent.clone();
            tokio::spawn(async move { coordinator.request_main_chat_clear(&parent).await })
        };
        gate.entered.acquire().await.unwrap().forget();
        let launch = coordinator.accept_async_helper(
            agent.clone(),
            parent.clone(),
            "racing-launch".into(),
            "Helper".into(),
            "Old task".into(),
            "chat".into(),
            Arc::new(AtomicUsize::new(0)),
            None,
        );
        let mut launch = Box::pin(launch);
        assert!(futures::poll!(&mut launch).is_pending());
        gate.release.add_permits(1);
        clearing.await.unwrap().unwrap();
        assert!(launch.await.unwrap_err().contains(CLEAR_PENDING));
        assert!(coordinator
            .state
            .read()
            .await
            .runs
            .async_helpers_for_owner(&agent)
            .is_empty());
        coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(&parent.run_id)
            .unwrap()
            .finish(
                crate::runs::RunStatus::Completed,
                None,
                anima_core::primitives::now_millis(),
            );
        coordinator
            .clear_main_chat_locked(&agent, "chat:main")
            .await
            .unwrap();
        drop(reservation);
    }

    #[tokio::test]
    async fn main_chat_clear_preserves_unsettled_helpers_and_restorable_lineage() {
        use crate::runs::{AsyncHelper, AsyncHelperBounds, HelperCompletionState, RunStatus};
        let (coordinator, agent) = fixture().await;
        let (parent, child_id) = {
            let mut state = coordinator.state.write().await;
            let parent = state.runs.for_session(&agent, "chat:main")[0].clone();
            let companion = state.get_agent(&agent).unwrap().state;
            let helper = state
                .create_agent(super::super::helper_config(
                    &companion,
                    "Reset fixture".into(),
                ))
                .unwrap()
                .state;
            let mut session = crate::sessions::SessionRecord::new(
                &helper.id,
                "helper:reset",
                crate::sessions::SessionKind::Helper,
                crate::sessions::SessionOrigin::Delegation,
                "Helper".into(),
                crate::sessions::TitleSource::System,
                1,
            );
            session.parent_agent_id = Some(agent.clone());
            session.parent_session_id = Some("chat:main".into());
            session.parent_run_id = Some(parent.id.clone());
            state.sessions.insert(session);
            let mut child = parent.clone();
            child.id = "run_reset_helper".into();
            child.agent_id = helper.id;
            child.session_id = "helper:reset".into();
            child.source = crate::runs::RunSource::Delegation;
            child.parent_run_id = Some(parent.id.clone());
            child.idempotency_key = None;
            child.reply_sha256 = None;
            child.turn = None;
            child.async_helper = Some(AsyncHelper {
                owner_agent_id: agent.clone(),
                owner_session_id: "chat:main".into(),
                parent_run_id: parent.id.clone(),
                launch_key: "reset-fixture".into(),
                launch_fingerprint: "a".repeat(64),
                options: None,
                effective_bounds: Some(AsyncHelperBounds::for_config(&companion.config)),
                origin: None,
                completion: HelperCompletionState::Manual,
                unconsumed_steers: vec![],
                worktree: None,
            });
            let id = child.id.clone();
            state.runs.insert(child);
            (parent, id)
        };
        for (status, completion) in [
            (RunStatus::Running, HelperCompletionState::Manual),
            (
                RunStatus::Completed,
                HelperCompletionState::AwaitingParent {
                    reserved_run_id: "run_reset_callback".into(),
                },
            ),
        ] {
            {
                let mut state = coordinator.state.write().await;
                let child = state.runs.get_mut(&child_id).unwrap();
                child.status = status;
                child.async_helper.as_mut().unwrap().completion = completion;
            }
            let before = coordinator.state.read().await.control_plane_snapshot();
            let mut restored =
                crate::state::DaemonState::with_model_adapter(ScriptedModel::new(vec![]));
            restored
                .restore_control_plane_snapshot(before.clone())
                .unwrap();
            let error = coordinator
                .clear_main_chat(&agent, "chat:main")
                .await
                .unwrap_err();
            assert!(error.message().contains("Background tasks"));
            assert_eq!(
                serde_json::to_value(coordinator.state.read().await.control_plane_snapshot())
                    .unwrap(),
                serde_json::to_value(before).unwrap()
            );
            // Refuse before saving a self-reset request, too: callbacks remain admissible.
            coordinator
                .state
                .write()
                .await
                .runs
                .get_mut(&parent.id)
                .unwrap()
                .status = RunStatus::Running;
            assert!(coordinator
                .request_main_chat_clear(&RunLink {
                    agent_id: agent.clone(),
                    session_id: "chat:main".into(),
                    run_id: parent.id.clone(),
                })
                .await
                .unwrap_err()
                .contains("Background tasks"));
            coordinator.state.write().await.runs.insert(parent.clone());
            assert!(!clear_pending(
                &*coordinator.state.read().await,
                &agent,
                "chat:main"
            ));
        }
        {
            let mut state = coordinator.state.write().await;
            let child = state.runs.get_mut(&child_id).unwrap();
            child.async_helper.as_mut().unwrap().completion = HelperCompletionState::Suppressed {
                reason: "The background task settled without a callback".into(),
                run_id: None,
            };
        }
        coordinator
            .clear_main_chat(&agent, "chat:main")
            .await
            .unwrap();
        let snapshot = coordinator.state.read().await.control_plane_snapshot();
        let mut restored =
            crate::state::DaemonState::with_model_adapter(ScriptedModel::new(vec![]));
        restored.restore_control_plane_snapshot(snapshot).unwrap();
    }

    #[tokio::test]
    async fn main_chat_clear_removes_hot_cold_and_summary_but_keeps_memory_and_other_rooms() {
        let (coordinator, agent) = fixture().await;
        coordinator
            .run(chat_request(&agent, "chat:other", "keep this conversation"))
            .await
            .unwrap();
        let (history, memory, other) = {
            let mut guard = coordinator.state.write().await;
            let mut other_question =
                guard.clarifications.for_session(&agent, "chat:main")[0].clone();
            other_question.id = "ask_other_chat".into();
            other_question.session_id = "chat:other".into();
            other_question.run_id = guard.runs.for_session(&agent, "chat:other")[0].id.clone();
            guard.clarifications.put(other_question);
            guard.sessions.get_mut(&agent, "chat:main").unwrap().summary =
                Some(crate::sessions::SessionSummary {
                    text: "old secret summary".into(),
                    through_message_id: "old".into(),
                    created_at_ms: 1,
                    source_message_count: 2,
                });
            (
                guard.history.clone(),
                guard.memory_handle(),
                guard
                    .agents
                    .get(&agent)
                    .unwrap()
                    .messages()
                    .iter()
                    .filter(|m| m.room_id == "chat:other")
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        history
            .flush_once(
                &coordinator.state,
                &coordinator.control_plane_transactions(),
                1,
            )
            .await
            .unwrap();
        let memories = memory.read().await.snapshot();
        coordinator
            .clear_main_chat(&agent, "chat:main")
            .await
            .unwrap();
        let guard = coordinator.state.read().await;
        let snapshot = guard.get_agent(&agent).unwrap();
        assert_eq!(snapshot.messages, other);
        assert!(snapshot.last_task.is_none());
        assert!(guard
            .sessions
            .get(&agent, "chat:main")
            .unwrap()
            .summary
            .is_none());
        assert!(guard.runs.for_session(&agent, "chat:main").is_empty());
        assert!(guard
            .clarifications
            .for_session(&agent, "chat:main")
            .is_empty());
        assert!(!clear_pending(&guard, &agent, "chat:main"));
        assert_eq!(
            guard.clarifications.for_session(&agent, "chat:other").len(),
            1
        );
        drop(guard);
        assert_eq!(memories, memory.read().await.snapshot());
        assert!(history
            .store()
            .page_messages(&crate::history::MessagePageQuery {
                agent_id: agent.clone(),
                session_id: "chat:main".into(),
                before: None,
                limit: 50,
                include_hidden: true,
            })
            .await
            .unwrap()
            .is_empty());
        // A second reset and the first fresh turn must also work on the same ID.
        coordinator
            .clear_main_chat(&agent, "chat:main")
            .await
            .unwrap();
        coordinator
            .run(chat_request(&agent, "chat:main", "fresh topic"))
            .await
            .unwrap();
        assert!(coordinator
            .state
            .read()
            .await
            .agents
            .get(&agent)
            .unwrap()
            .messages()
            .iter()
            .all(|m| !m.content.text.contains("private old history")));
    }

    #[tokio::test]
    async fn main_chat_clear_failed_save_restores_messages_session_and_runs() {
        let (coordinator, agent) = fixture().await;
        let questions = coordinator
            .state
            .read()
            .await
            .clarifications
            .for_session(&agent, "chat:main");
        let before = coordinator
            .state
            .read()
            .await
            .get_agent(&agent)
            .unwrap()
            .messages;
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let task = {
            let coordinator = coordinator.clone();
            let agent = agent.clone();
            tokio::spawn(async move { coordinator.clear_main_chat(&agent, "chat:main").await })
        };
        gate.entered.acquire().await.unwrap().forget();
        gate.release.add_permits(1);
        assert!(task.await.unwrap().is_err());
        let guard = coordinator.state.read().await;
        assert_eq!(
            guard.clarifications.for_session(&agent, "chat:main"),
            questions
        );
        assert_eq!(guard.get_agent(&agent).unwrap().messages, before);
        assert!(guard.sessions.get(&agent, "chat:main").unwrap().main_chat);
        assert!(!clear_pending(&guard, &agent, "chat:main"));
        assert_eq!(guard.runs.for_session(&agent, "chat:main").len(), 1);
    }

    #[tokio::test]
    async fn main_chat_clear_keeps_admission_closed_until_deletion_ack_is_durable() {
        let (coordinator, agent) = fixture().await;
        let first = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(false);
        let task = {
            let coordinator = coordinator.clone();
            let agent = agent.clone();
            tokio::spawn(async move { coordinator.clear_main_chat(&agent, "chat:main").await })
        };
        first.entered.acquire().await.unwrap().forget();
        let failure = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        first.release.add_permits(1);
        failure.entered.acquire().await.unwrap().forget();
        failure.release.add_permits(1);
        assert!(task.await.unwrap().is_err());
        assert!(clear_pending(
            &*coordinator.state.read().await,
            &agent,
            "chat:main"
        ));
        assert!(coordinator
            .run(chat_request(&agent, "chat:main", "must wait"))
            .await
            .is_err());
        let history = coordinator.state.read().await.history.clone();
        history
            .flush_once(
                &coordinator.state,
                &coordinator.control_plane_transactions(),
                2,
            )
            .await
            .unwrap();
        assert!(!clear_pending(
            &*coordinator.state.read().await,
            &agent,
            "chat:main"
        ));
        coordinator
            .run(chat_request(&agent, "chat:main", "fresh"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn main_chat_clear_recovers_a_saved_request_after_turn_settlement() {
        let (coordinator, agent) = fixture().await;
        coordinator
            .state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:main")
            .unwrap()
            .pending_clear_run_id = Some("settled-before-crash".into());
        coordinator.recover_main_chat_clears().await.unwrap();
        assert!(coordinator
            .state
            .read()
            .await
            .get_agent(&agent)
            .unwrap()
            .messages
            .is_empty());
    }

    #[tokio::test]
    async fn main_chat_clear_discards_old_queued_inputs_and_refuses_new_inputs_while_pending() {
        use crate::agent_runs::test_support::{accept_web, Gate};
        let gate = Gate::new();
        let model = ScriptedModel::gated(
            vec![
                Step::Tools(vec![anima_core::ToolCall {
                    id: "clear".into(),
                    name: "clear_main_chat".into(),
                    args: Default::default(),
                }]),
                Step::Text(vec!["Clearing after this reply."]),
            ],
            gate.clone(),
        );
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, "chat:main").await;
        coordinator
            .state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:main")
            .unwrap()
            .main_chat = true;
        let task = {
            let coordinator = coordinator.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                coordinator
                    .run(chat_request(&agent, "chat:main", "clear our chat"))
                    .await
            })
        };
        gate.entered().await;
        let queued = accept_web(&coordinator, &agent, "chat:main", "old queued input").await;
        gate.release();
        gate.entered().await;
        assert!(clear_pending(
            &*coordinator.state.read().await,
            &agent,
            "chat:main"
        ));
        let request =
            crate::agent_runs::test_support::accept(&agent, "chat:main", "new input must wait");
        let start = coordinator.web_start(
            agent.clone(),
            "chat:main".into(),
            request.text.clone(),
            request.idempotency_key.clone(),
        );
        assert!(coordinator.accept_run(request, start).await.is_err());
        gate.release();
        task.await.unwrap().unwrap();
        assert!(coordinator.state.read().await.runs.get(&queued).is_none());
        assert!(coordinator
            .state
            .read()
            .await
            .get_agent(&agent)
            .unwrap()
            .messages
            .is_empty());
        assert_eq!(model.requests().len(), 2);
    }

    #[tokio::test]
    async fn main_chat_clear_saved_request_recovers_after_final_turn_save_failure() {
        use crate::agent_runs::test_support::Gate;
        let gate = Gate::new();
        let model = ScriptedModel::gated(
            vec![
                Step::Tools(vec![anima_core::ToolCall {
                    id: "clear".into(),
                    name: "clear_main_chat".into(),
                    args: Default::default(),
                }]),
                Step::Text(vec!["Clear requested."]),
            ],
            gate.clone(),
        );
        let (coordinator, agent) = coordinator_with(model).await;
        add_chat(&coordinator, &agent, "chat:main").await;
        coordinator
            .state
            .write()
            .await
            .sessions
            .get_mut(&agent, "chat:main")
            .unwrap()
            .main_chat = true;
        let task = {
            let coordinator = coordinator.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                coordinator
                    .run(chat_request(&agent, "chat:main", "clear our chat"))
                    .await
            })
        };
        gate.entered().await;
        gate.release();
        gate.entered().await;
        let failure = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_non_turn_save_gate(true);
        gate.release();
        failure.entered.acquire().await.unwrap().forget();
        failure.release.add_permits(1);
        assert!(task.await.unwrap().is_err());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let state = coordinator.state.read().await;
                if !clear_pending(&state, &agent, "chat:main")
                    && state.get_agent(&agent).unwrap().messages.is_empty()
                {
                    break;
                }
                drop(state);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("saved reset is completed without restarting the daemon");
    }
}

impl AgentRunCoordinator {
    pub(crate) async fn request_main_chat_clear(&self, run: &RunLink) -> Result<(), String> {
        let _transaction = self.control_plane_transaction().await;
        let persist = {
            let mut guard = self.state.write().await;
            let record = guard
                .runs
                .get(&run.run_id)
                .filter(|r| {
                    r.agent_id == run.agent_id
                        && r.session_id == run.session_id
                        && matches!(
                            r.source,
                            crate::runs::RunSource::Web | crate::runs::RunSource::Api
                        )
                        && !r.system_followup.unwrap_or(false)
                        && r.status.is_in_flight()
                })
                .ok_or("Clearing requires a direct, live owner chat run")?;
            if record.parent_run_id.is_some() {
                return Err("Delegated runs cannot clear the main chat".into());
            }
            if helpers_pending(&guard, &run.agent_id, &run.session_id) {
                return Err(HELPERS_BUSY.into());
            }
            let session = guard
                .sessions
                .get_mut(&run.agent_id, &run.session_id)
                .filter(|s| s.main_chat && s.has_main_chat_identity())
                .ok_or("Only the current main web chat can be cleared")?;
            if let Some(id) = &session.pending_clear_run_id {
                return if id == &run.run_id {
                    Ok(())
                } else {
                    Err(CLEAR_PENDING.into())
                };
            }
            session.pending_clear_run_id = Some(run.run_id.clone());
            guard.control_plane_persist_request()
        };
        if let Err(error) = persist.save().await {
            if let Some(session) = self
                .state
                .write()
                .await
                .sessions
                .get_mut(&run.agent_id, &run.session_id)
            {
                session.pending_clear_run_id = None;
            }
            return Err(format!("The clear request could not be saved: {error}"));
        }
        // Also covers final-commit failure and a panicking run: the ordinary
        // success path clears inline, while this worker waits for its lease.
        let coordinator = self.clone();
        let run = run.clone();
        tokio::spawn(async move {
            let mut delay = 1;
            loop {
                if !coordinator
                    .main_chat_clear_requested(&run.agent_id, &run.session_id, &run.run_id)
                    .await
                {
                    break;
                }
                let room = {
                    let state = coordinator.state.read().await;
                    state
                        .sessions
                        .get(&run.agent_id, &run.session_id)
                        .map(|s| s.room_id().to_owned())
                };
                let Some(room) = room else {
                    break;
                };
                let lease = coordinator.wait_session_lease(&run.agent_id, &room).await;
                if !coordinator
                    .main_chat_clear_requested(&run.agent_id, &run.session_id, &run.run_id)
                    .await
                {
                    break;
                }
                let result = coordinator
                    .clear_main_chat_locked(&run.agent_id, &run.session_id)
                    .await;
                drop(lease);
                if result.is_ok() {
                    break;
                }
                tracing::warn!(run_id = %run.run_id, "Saved main-chat clear is pending; retrying after run settlement");
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                delay = (delay * 2).min(30);
            }
        });
        Ok(())
    }

    pub(crate) async fn main_chat_clear_requested(
        &self,
        agent: &str,
        session: &str,
        run: &str,
    ) -> bool {
        self.state
            .read()
            .await
            .sessions
            .get(agent, session)
            .is_some_and(|s| s.pending_clear_run_id.as_deref() == Some(run))
    }

    /// Idle owner endpoint. The self-reset settlement already owns this lease.
    pub(crate) async fn clear_main_chat(&self, agent: &str, session: &str) -> Result<(), ApiError> {
        let room = self
            .state
            .read()
            .await
            .sessions
            .get(agent, session)
            .ok_or_else(ApiError::not_found)?
            .room_id()
            .to_owned();
        let _reservation = self.try_reserve_room(agent, &room).ok_or_else(|| {
            ApiError::conflict("A reply is in progress; wait for it to finish before clearing")
        })?;
        self.clear_main_chat_locked(agent, session).await
    }

    pub(super) async fn clear_main_chat_locked(
        &self,
        agent: &str,
        session: &str,
    ) -> Result<(), ApiError> {
        let transaction = self.control_plane_transaction().await;
        let deletion = HistoryDeletion::session(agent, session);
        let (
            previous_agent,
            previous_session,
            removed_runs,
            removed_questions,
            removed_ids,
            history,
            persist,
        ) = {
            let mut guard = self.state.write().await;
            let previous_session = guard
                .sessions
                .get(agent, session)
                .filter(|s| s.main_chat && s.has_main_chat_identity())
                .cloned()
                .ok_or_else(|| ApiError::conflict("Only the main web chat can be cleared"))?;
            if helpers_pending(&guard, agent, session) {
                return Err(ApiError::conflict(HELPERS_BUSY));
            }
            let next_generation = previous_session
                .clear_generation
                .checked_add(1)
                .ok_or_else(|| ApiError::conflict("This conversation cannot be reset again"))?;
            if guard
                .runs
                .for_session(agent, session)
                .iter()
                .any(|r| r.status.is_in_flight())
            {
                return Err(ApiError::conflict("A reply is still in progress"));
            }
            if guard
                .pending_history_deletions
                .iter()
                .any(|d| d == &deletion)
            {
                return Err(ApiError::conflict(CLEAR_PENDING));
            }
            let runtime = guard
                .agents
                .get_mut(agent)
                .ok_or_else(ApiError::not_found)?;
            let previous_agent = runtime.snapshot();
            let removed_ids = runtime
                .retain_messages(|m| m.room_id != previous_session.room_id())
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>();
            let mut cleared_agent = runtime.snapshot();
            cleared_agent.last_task = None;
            guard
                .restore_removed_agent(cleared_agent)
                .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
            let mut fresh = crate::sessions::SessionRecord::new(
                agent,
                previous_session.room_id(),
                previous_session.kind,
                previous_session.origin,
                previous_session.title.clone(),
                previous_session.title_source,
                anima_core::primitives::now_millis(),
            );
            fresh.main_chat = true;
            fresh.clear_generation = next_generation;
            fresh.created_at_ms = previous_session.created_at_ms;
            fresh.model_preferences = previous_session.model_preferences.clone();
            guard.sessions.insert(fresh);
            // Accepted inputs from before the boundary are discarded as part of clearing.
            // Their queue workers require these exact records before starting a runtime.
            let ids = guard
                .runs
                .for_session(agent, session)
                .iter()
                .map(|r| r.id.clone())
                .collect::<Vec<_>>();
            let removed_runs = ids
                .iter()
                .filter_map(|id| guard.runs.remove(id))
                .collect::<Vec<_>>();
            let removed_questions = guard.clarifications.for_session(agent, session);
            guard
                .clarifications
                .retain(|question| question.agent_id != agent || question.session_id != session);
            guard.record_history_deletion(deletion.clone());
            (
                previous_agent,
                previous_session,
                removed_runs,
                removed_questions,
                removed_ids,
                guard.history.clone(),
                guard.control_plane_persist_request(),
            )
        };
        if let Err(error) = persist.save().await {
            let mut guard = self.state.write().await;
            guard
                .restore_removed_agent(previous_agent)
                .map_err(|e| ApiError::service_unavailable(e.to_string()))?;
            guard.sessions.insert(previous_session);
            for run in removed_runs {
                guard.runs.insert(run);
            }
            for question in removed_questions {
                guard.clarifications.put(question);
            }
            guard.clear_history_deletion(&deletion);
            return Err(ApiError::service_unavailable(error.to_string()));
        }
        // Ordered after every old transcript commit and before any fresh input.
        crate::chat_review::cancel_records(&previous_session.chat_reviews);
        history.enqueue_session_deletion(agent, session);
        history.forget_mirrored(removed_ids.iter().map(String::as_str));
        for run in &removed_runs {
            self.state.read().await.live.runs().remove(&run.id);
        }
        self.state.read().await.publish_session_event(
            agent,
            session,
            LiveEventBody::SessionCleared,
        );
        drop(transaction);
        history
            .flush_once(
                &self.state,
                &self.control_plane_transactions(),
                anima_core::primitives::now_millis(),
            )
            .await
            .map_err(|e| {
                ApiError::service_unavailable(format!(
                    "Conversation cleared; history cleanup is pending: {e}"
                ))
            })?;
        self.state.read().await.publish_session_event(
            agent,
            session,
            LiveEventBody::SessionUpdated,
        );
        Ok(())
    }

    /// A durable tool request survives a crash between turn settlement and reset.
    pub(crate) async fn recover_main_chat_clears(&self) -> Result<(), ApiError> {
        let pending = self
            .state
            .read()
            .await
            .control_plane_snapshot()
            .sessions
            .into_iter()
            .filter(|s| s.pending_clear_run_id.is_some())
            .collect::<Vec<_>>();
        for session in pending {
            self.clear_main_chat(&session.agent_id, &session.id).await?;
        }
        Ok(())
    }
}
