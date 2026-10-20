//! Save-before-display human input and versioned answers. No tool authority is granted.
use super::AgentRunCoordinator;
use crate::{
    clarifications::{ClarificationError, ClarificationRequest, ClarificationStatus},
    live::LiveEventBody,
    routes::ApiError,
    runs::{RunLink, RunSource, RunStatus},
    sessions::SessionKind,
};
use anima_core::{
    interaction::{normalize_questions, ClarificationAnswer, ClarificationQuestionInput},
    primitives::now_millis,
    CancelCause, CancelSignal,
};

/// Never discard an unresolved interruption to make room. Old settled
/// questions can retire once their source finished, stopped, or recovered.
fn retirement(state: &crate::state::DaemonState) -> Vec<String> {
    let mut records = state.clarifications.snapshot();
    records.sort_by_key(|request| (request.created_at_ms, request.id.clone()));
    let mut remove = Vec::new();
    let mut eligible = Vec::new();
    for request in &records {
        let source = state.runs.get(&request.run_id).filter(|source| {
            source.agent_id == request.agent_id && source.session_id == request.session_id
        });
        if !state
            .sessions
            .contains(&request.agent_id, &request.session_id)
            || source.is_none()
        {
            remove.push(request.id.clone());
            continue;
        }
        let source = source.unwrap();
        if request.status != ClarificationStatus::Pending
            && (source.stop.is_some()
                || source.recovery.is_some()
                || matches!(source.status, RunStatus::Completed | RunStatus::Cancelled))
        {
            eligible.push(request.id.clone());
        }
    }
    if records.len().saturating_sub(remove.len())
        >= crate::clarifications::MAX_CLARIFICATION_RECORDS
    {
        let count = records
            .len()
            .saturating_sub(remove.len())
            .saturating_sub(150);
        remove.extend(eligible.into_iter().take(count));
    }
    remove
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::test_support::{add_chat, coordinator_with, ScriptedModel};
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    use crate::runs::{RunRecord, RunStart, RunStopRequest};
    use anima_core::AgentConfigUpdate;
    use std::{sync::Arc, time::Duration};

    async fn fixture() -> (AgentRunCoordinator, RunLink, tempfile::TempDir) {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, "chat:questions").await;
        let dir = tempfile::tempdir().unwrap();
        let run = {
            let mut state = coordinator.state.write().await;
            let tools = crate::tools::ToolRegistry::new()
                .resolve_descriptors(["clarify"])
                .unwrap();
            state
                .update_agent(
                    &agent,
                    AgentConfigUpdate {
                        tools: Some(tools),
                        ..Default::default()
                    },
                )
                .unwrap();
            state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
                dir.path().join("state.json"),
            )));
            let run = RunRecord::running(
                RunStart {
                    agent_id: agent.clone(),
                    session_id: "chat:questions".into(),
                    source: RunSource::Web,
                    source_ref: None,
                    idempotency_key: None,
                    text: "Prepare a report".into(),
                    model: "test".into(),
                    provider: None,
                    parent_run_id: None,
                },
                now_millis(),
            );
            state.runs.insert(run.clone());
            run
        };
        (
            coordinator,
            RunLink {
                run_id: run.id,
                agent_id: agent,
                session_id: run.session_id,
            },
            dir,
        )
    }
    fn questions() -> Vec<ClarificationQuestionInput> {
        vec![ClarificationQuestionInput {
            question: "Format?".into(),
            choices: vec!["Markdown".into(), "PDF".into()],
            multi_select: false,
        }]
    }
    async fn pending(coordinator: &AgentRunCoordinator, run: &RunLink) -> ClarificationRequest {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(request) = coordinator
                    .state
                    .read()
                    .await
                    .clarifications
                    .for_session(&run.agent_id, &run.session_id)
                    .into_iter()
                    .next()
                {
                    return request;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    fn ask(
        coordinator: &AgentRunCoordinator,
        run: &RunLink,
        cancel: CancelSignal,
    ) -> tokio::task::JoinHandle<Result<ClarificationRequest, String>> {
        let (coordinator, run) = (coordinator.clone(), run.clone());
        tokio::spawn(async move {
            coordinator
                .request_clarification(run, "tc".into(), questions(), cancel)
                .await
        })
    }
    fn answer() -> Vec<ClarificationAnswer> {
        vec![ClarificationAnswer {
            question_id: "q0".into(),
            choice_ids: vec!["c1".into()],
            text: None,
        }]
    }

    #[tokio::test]
    async fn interaction_ephemeral_storage_never_claims_a_question_was_saved() {
        let (coordinator, run, _dir) = fixture().await;
        coordinator
            .state
            .write()
            .await
            .set_control_plane_store(None);
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            ask(&coordinator, &run, CancelSignal::new()),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.unwrap_err().contains("persistent storage"));
        assert!(coordinator
            .state
            .read()
            .await
            .clarifications
            .snapshot()
            .is_empty());
    }

    #[tokio::test]
    async fn interaction_reviewed_recovery_child_can_still_request_a_saved_question() {
        let (coordinator, run, _dir) = fixture().await;
        {
            let mut state = coordinator.state.write().await;
            let mut parent = state.runs.get(&run.run_id).unwrap().clone();
            parent.id = format!("original-{}", run.run_id);
            parent.finish(RunStatus::Interrupted, None, now_millis());
            parent.recovery = Some(crate::runs::recovery::RecoveryRecord {
                fingerprint: crate::runs::recovery::summary(&parent).fingerprint,
                continuation_run_id: run.run_id.clone(),
                reviewed_at_ms: now_millis(),
            });
            let child = state.runs.get_mut(&run.run_id).unwrap();
            child.system_followup = Some(true);
            child.recovery_of = Some(parent.id.clone());
            child.recovery_attempt = 1;
            state.runs.insert(parent);
        }
        let cancel = CancelSignal::new();
        let task = ask(&coordinator, &run, cancel.clone());
        assert_eq!(pending(&coordinator, &run).await.run_id, run.run_id);
        cancel.cancel();
        assert!(task.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn interaction_saved_question_survives_shutdown_and_answer_does_not_replay_the_run() {
        let (coordinator, run, dir) = fixture().await;
        let cancel = CancelSignal::new();
        let task = ask(&coordinator, &run, cancel.clone());
        let request = pending(&coordinator, &run).await;
        let shown = coordinator
            .list_clarifications(&run.agent_id, &run.session_id)
            .await
            .unwrap();
        assert_eq!(shown[0].id, request.id);
        let store = ControlPlaneStoreConfig::Json(dir.path().join("state.json"));
        let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(saved.clarifications[0].id, request.id);
        cancel.cancel_with(CancelCause::DaemonShutdown);
        assert!(task.await.unwrap().is_err());
        let mut restored = crate::state::DaemonState::new();
        restored.set_control_plane_store(Some(store));
        restored.restore_control_plane_snapshot(saved).unwrap();
        let restarted = AgentRunCoordinator::new(
            Arc::new(tokio::sync::RwLock::new(restored)),
            Arc::new(tokio::sync::Semaphore::new(4)),
        );
        assert_eq!(
            restarted
                .list_clarifications(&run.agent_id, &run.session_id)
                .await
                .unwrap()[0]
                .status,
            ClarificationStatus::Pending
        );
        assert!(restarted
            .answer_clarification(
                "other".into(),
                run.session_id.clone(),
                request.id.clone(),
                1,
                answer()
            )
            .await
            .is_err());
        assert!(restarted
            .answer_clarification(
                run.agent_id.clone(),
                run.session_id.clone(),
                request.id.clone(),
                8,
                answer()
            )
            .await
            .is_err());
        let answered = restarted
            .answer_clarification(
                run.agent_id.clone(),
                run.session_id.clone(),
                request.id,
                1,
                answer(),
            )
            .await
            .unwrap();
        assert_eq!(answered.status, ClarificationStatus::Answered);
        let state = restarted.state.read().await;
        assert_eq!(
            state.runs.get(&run.run_id).unwrap().status,
            RunStatus::Interrupted
        );
        assert_eq!(
            state.runs.for_session(&run.agent_id, &run.session_id).len(),
            1
        );
        assert_eq!(
            state.clarifications.answered_for_run(&run.run_id),
            [answered]
        );
    }

    #[tokio::test]
    async fn interaction_failed_answer_save_does_not_wake_the_tool() {
        let (coordinator, run, _dir) = fixture().await;
        let cancel = CancelSignal::new();
        let task = ask(&coordinator, &run, cancel.clone());
        let request = pending(&coordinator, &run).await;
        coordinator
            .list_clarifications(&run.agent_id, &run.session_id)
            .await
            .unwrap();
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let writer = tokio::spawn({
            let coordinator = coordinator.clone();
            let run = run.clone();
            let id = request.id.clone();
            async move {
                coordinator
                    .answer_clarification(run.agent_id, run.session_id, id, 1, answer())
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        let reader = tokio::spawn({
            let coordinator = coordinator.clone();
            let run = run.clone();
            async move {
                coordinator
                    .list_clarifications(&run.agent_id, &run.session_id)
                    .await
            }
        });
        tokio::task::yield_now().await;
        assert!(!reader.is_finished());
        gate.release.add_permits(1);
        assert!(writer.await.unwrap().is_err());
        assert_eq!(
            reader.await.unwrap().unwrap()[0].status,
            ClarificationStatus::Pending
        );
        assert!(!task.is_finished());
        {
            let mut state = coordinator.state.write().await;
            let source = state.runs.get_mut(&run.run_id).unwrap();
            source.stop = Some(RunStopRequest {
                requested_at_ms: now_millis(),
            });
            source.finish(RunStatus::Cancelled, None, now_millis());
        }
        cancel.cancel();
        assert!(tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .clarifications
                .get(&request.id)
                .unwrap()
                .status,
            ClarificationStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn interaction_question_save_failure_is_not_shown_and_connector_calls_fail_fast() {
        let (coordinator, run, _dir) = fixture().await;
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let task = ask(&coordinator, &run, CancelSignal::new());
        tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        let reader = tokio::spawn({
            let coordinator = coordinator.clone();
            let run = run.clone();
            async move {
                coordinator
                    .list_clarifications(&run.agent_id, &run.session_id)
                    .await
            }
        });
        tokio::task::yield_now().await;
        assert!(!reader.is_finished());
        gate.release.add_permits(1);
        assert!(task.await.unwrap().is_err());
        assert!(reader.await.unwrap().unwrap().is_empty());
        coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(&run.run_id)
            .unwrap()
            .source = RunSource::Discord;
        assert!(tokio::time::timeout(
            Duration::from_secs(1),
            ask(&coordinator, &run, CancelSignal::new())
        )
        .await
        .unwrap()
        .unwrap()
        .is_err());
        assert!(coordinator
            .state
            .read()
            .await
            .clarifications
            .snapshot()
            .is_empty());
    }
}

pub(crate) fn clarification_error(error: ClarificationError) -> ApiError {
    match error {
        ClarificationError::NotFound => ApiError::not_found(),
        ClarificationError::Conflict(message) => ApiError::conflict(message),
        ClarificationError::Invalid(message) => ApiError::bad_request(message),
    }
}

impl AgentRunCoordinator {
    pub(crate) async fn request_clarification(
        &self,
        run: RunLink,
        call: String,
        questions: Vec<ClarificationQuestionInput>,
        cancel: CancelSignal,
    ) -> Result<ClarificationRequest, String> {
        let questions = normalize_questions(questions)?;
        let coordinator = self.clone();
        let run_for_open = run.clone();
        // Let an admitted save settle even when the executing task is dropped.
        let request = tokio::spawn(async move {
            let _transaction = coordinator.control_plane_transaction().await;
            let (record, changed, before, persist) = {
                let mut state = coordinator.state.write().await;
                if state.control_plane_store.is_none() {
                    return Err("Configure persistent storage before requesting a saved question".to_string());
                }
                let source = state.runs.get(&run_for_open.run_id).filter(|source| source.agent_id == run_for_open.agent_id && source.session_id == run_for_open.session_id && source.status.is_in_flight() && source.stop.is_none() && source.parent_run_id.is_none() && (source.system_followup != Some(true) || source.recovery_of.is_some()) && matches!(source.source, RunSource::Web | RunSource::Api)).ok_or_else(|| "Clarify requires an ordinary active web chat run".to_string())?;
                let session = state.sessions.get(&source.agent_id, &source.session_id).filter(|session| session.kind == SessionKind::Chat && session.parent_agent_id.is_none()).ok_or_else(|| "Clarify is available in web chats; this channel cannot deliver structured answers".to_string())?;
                let agent = state.agents.get(&source.agent_id).filter(|runtime| !super::is_helper_config(runtime.config()) && runtime.config().allows_tool("clarify")).ok_or_else(|| "Clarify is unavailable for this agent".to_string())?;
                let (agent_id, session_id) = (agent.state().id, session.id.clone());
                let before = state.clarifications.clone();
                let retired = retirement(&state);
                for id in &retired { state.clarifications.remove(id); }
                let (record, created) = match state.clarifications.open(&agent_id, &session_id, &run_for_open.run_id, &call, questions, now_millis()) { Ok(opened) => opened, Err(error) => { state.clarifications = before; return Err(format!("Question could not be opened: {error:?}")); } };
                let persist = state.control_plane_persist_request();
                (record, created || !retired.is_empty(), before, persist)
            };
            if changed {
                if persist.save().await.is_err() {
                    coordinator.state.write().await.clarifications = before;
                    return Err("The question could not be saved; no answer was requested".to_string());
                }
                coordinator.state.read().await.publish_session_event(&record.agent_id, &record.session_id, LiveEventBody::SessionUpdated);
            }
            Ok(record)
        }).await.map_err(|_| "The question save worker stopped".to_string())??;
        loop {
            let current = {
                // Answer mutation and storage acknowledgement share this
                // transaction. A polling tool must never consume a tentative
                // answer that an unsuccessful save will roll back.
                let _transaction = self.control_plane_transaction().await;
                self.state
                    .read()
                    .await
                    .clarifications
                    .get(&request.id)
                    .cloned()
                    .ok_or_else(|| "The saved question is no longer available".to_string())?
            };
            match current.status {
                ClarificationStatus::Answered => return Ok(current),
                ClarificationStatus::Cancelled => {
                    return Err("Question cancelled; no answer was provided".into())
                }
                ClarificationStatus::Pending => {}
            }
            let stopped = {
                let state = self.state.read().await;
                state.runs.get(&run.run_id).is_none_or(|source| {
                    source.stop.is_some() || source.status == RunStatus::Cancelled
                })
            };
            if stopped {
                self.cancel_clarifications(&run.agent_id, &run.session_id)
                    .await
                    .map_err(|_| "The cancelled question could not be saved".to_string())?;
                return Err("Question cancelled; the run was stopped".into());
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    if cancel.cause() != Some(CancelCause::DaemonShutdown) { let _ = self.cancel_clarifications(&run.agent_id, &run.session_id).await; }
                    return Err(cancel.tool_result().into());
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {},
            }
        }
    }

    /// Reconcile stopped runs without expiring questions whose run was interrupted.
    pub(crate) async fn cancel_clarifications(
        &self,
        agent: &str,
        session: &str,
    ) -> Result<(), ApiError> {
        let _transaction = self.control_plane_transaction().await;
        let (before, changed, persist) = {
            let mut state = self.state.write().await;
            let before = state.clarifications.clone();
            let cancelled: Vec<_> = state
                .clarifications
                .for_session(agent, session)
                .into_iter()
                .filter(|request| {
                    request.status == ClarificationStatus::Pending
                        && state.runs.get(&request.run_id).is_some_and(|run| {
                            run.stop.is_some()
                                || matches!(run.status, RunStatus::Cancelled | RunStatus::Completed)
                        })
                })
                .map(|request| request.run_id)
                .collect();
            let mut changed = !cancelled.is_empty();
            for run in cancelled {
                state.clarifications.cancel_run(&run, now_millis());
            }
            let retired = retirement(&state);
            changed |= !retired.is_empty();
            for id in retired {
                state.clarifications.remove(&id);
            }
            (before, changed, state.control_plane_persist_request())
        };
        if changed {
            if persist.save().await.is_err() {
                self.state.write().await.clarifications = before;
                return Err(ApiError::service_unavailable(
                    "The question cancellation could not be saved",
                ));
            }
            self.state.read().await.publish_session_event(
                agent,
                session,
                LiveEventBody::SessionUpdated,
            );
        }
        Ok(())
    }

    pub(crate) async fn list_clarifications(
        &self,
        agent: &str,
        session: &str,
    ) -> Result<Vec<ClarificationRequest>, ApiError> {
        if !self.state.read().await.sessions.contains(agent, session) {
            return Err(ApiError::not_found());
        }
        self.cancel_clarifications(agent, session).await?;
        let _transaction = self.control_plane_transaction().await;
        Ok(self
            .state
            .read()
            .await
            .clarifications
            .for_session(agent, session))
    }

    pub(crate) async fn answer_clarification(
        &self,
        agent: String,
        session: String,
        id: String,
        revision: u64,
        answers: Vec<ClarificationAnswer>,
    ) -> Result<ClarificationRequest, ApiError> {
        self.cancel_clarifications(&agent, &session).await?;
        let coordinator = self.clone();
        tokio::spawn(async move {
            let _transaction = coordinator.control_plane_transaction().await;
            let (before, answered, persist) = {
                let mut state = coordinator.state.write().await;
                let before = state
                    .clarifications
                    .get(&id)
                    .filter(|request| {
                        request.agent_id == agent
                            && request.session_id == session
                            && state.sessions.contains(&agent, &session)
                    })
                    .cloned()
                    .ok_or_else(ApiError::not_found)?;
                let source = state
                    .runs
                    .get(&before.run_id)
                    .filter(|source| source.agent_id == agent && source.session_id == session)
                    .ok_or_else(ApiError::not_found)?;
                if before.status != ClarificationStatus::Answered
                    && (source.recovery.is_some()
                        || source.stop.is_some()
                        || matches!(source.status, RunStatus::Cancelled | RunStatus::Completed))
                {
                    return Err(ApiError::conflict("This run no longer accepts answers"));
                }
                let answered = state
                    .clarifications
                    .answer(&id, revision, answers, now_millis())
                    .map_err(clarification_error)?;
                let persist = state.control_plane_persist_request();
                (before, answered, persist)
            };
            if before == answered {
                return Ok(answered);
            }
            if persist.save().await.is_err() {
                coordinator.state.write().await.clarifications.put(before);
                return Err(ApiError::service_unavailable(
                    "The answer could not be saved; the run is still waiting",
                ));
            }
            coordinator.state.read().await.publish_session_event(
                &agent,
                &session,
                LiveEventBody::SessionUpdated,
            );
            Ok(answered)
        })
        .await
        .map_err(|_| ApiError::service_unavailable("The answer save worker stopped"))?
    }
}
