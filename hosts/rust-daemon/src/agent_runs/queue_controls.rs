//! Owner edits of accepted inputs that the session drainer has not claimed.

use super::{is_helper_config, AgentRunCoordinator};
use crate::live::run_status_event;
use crate::routes::ApiError;
use crate::runs::{RunRecord, RunSource, RunStatus, MAX_RUN_INPUT_TEXT_BYTES};
use crate::sessions::{SessionKind, SessionOrigin};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub(crate) struct QueuedRunPatch {
    pub(crate) text: Option<String>,
    /// Absent preserves position, null moves to the end, an ID moves before it.
    pub(crate) before_run_id: Option<Option<String>>,
}

const QUEUE_EDIT_REFUSED: &str = "This input is no longer an editable waiting owner message";

fn editable(run: &RunRecord) -> bool {
    run.status == RunStatus::Queued
        && run.started_at_ms.is_none()
        && run.stop.is_none()
        && matches!(run.source, RunSource::Web | RunSource::Api)
        && run.source_ref.is_none()
        && run.system_followup == Some(false)
        && run.parent_run_id.is_none()
        && run.recovery_of.is_none()
        && run.async_helper.is_none()
        && run.helper_completion.is_none()
        && run.pending_steers.is_empty()
        && run.steered_keys.is_empty()
        && run
            .turn
            .as_ref()
            .is_some_and(|turn| turn.binding.automatic_eligible && turn.checkpoint.is_none())
}

impl AgentRunCoordinator {
    pub(crate) async fn update_queued_run(
        &self,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
        patch: QueuedRunPatch,
    ) -> Result<RunRecord, ApiError> {
        // A dropped HTTP request must not interrupt save/rollback halfway.
        let coordinator = self.clone();
        let (agent_id, session_id, run_id) = (
            agent_id.to_owned(),
            session_id.to_owned(),
            run_id.to_owned(),
        );
        tokio::spawn(async move {
            coordinator
                .update_queued_run_owned(&agent_id, &session_id, &run_id, patch)
                .await
        })
        .await
        .map_err(|_| ApiError::service_unavailable("Queue edit worker stopped"))?
    }

    async fn update_queued_run_owned(
        &self,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
        patch: QueuedRunPatch,
    ) -> Result<RunRecord, ApiError> {
        if patch.text.is_none() && patch.before_run_id.is_none() {
            return Err(ApiError::bad_request_static(
                "text or beforeRunId is required",
            ));
        }
        if patch
            .text
            .as_ref()
            .is_some_and(|text| text.len() > MAX_RUN_INPUT_TEXT_BYTES)
        {
            return Err(ApiError::bad_request_static("text must be at most 32 KiB"));
        }
        let _transaction = self.control_plane_transaction().await;
        let key = (agent_id.to_owned(), session_id.to_owned());
        let (before, changed, persist) = {
            let mut state = self.state.write().await;
            let runtime = state.agents.get(agent_id).ok_or_else(ApiError::not_found)?;
            let session = state
                .sessions
                .get(agent_id, session_id)
                .ok_or_else(ApiError::not_found)?;
            if self.is_being_deleted(agent_id)
                || is_helper_config(runtime.config())
                || session.kind != SessionKind::Chat
                || !matches!(session.origin, SessionOrigin::Web | SessionOrigin::Api)
                || session.archived
                || session.parent_agent_id.is_some()
                || session.parent_run_id.is_some()
                || super::main_chat_clear::clear_pending(&state, agent_id, session_id)
            {
                return Err(ApiError::conflict(QUEUE_EDIT_REFUSED));
            }
            let original = state
                .runs
                .get(run_id)
                .filter(|run| run.agent_id == agent_id && run.session_id == session_id)
                .cloned()
                .ok_or_else(ApiError::not_found)?;
            let waiting: Vec<String> = {
                let queues = self
                    .session_queues
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let queue = queues
                    .get(&key)
                    .ok_or_else(|| ApiError::conflict(QUEUE_EDIT_REFUSED))?;
                if !queue
                    .iter()
                    .any(|entry| entry.run_id == run_id && entry.start.reserved_ticket.is_none())
                {
                    return Err(ApiError::conflict(QUEUE_EDIT_REFUSED));
                }
                queue
                    .iter()
                    .filter(|entry| {
                        state
                            .runs
                            .get(&entry.run_id)
                            .is_some_and(|run| run.status == RunStatus::Queued)
                    })
                    .map(|entry| entry.run_id.clone())
                    .collect()
            };
            if !editable(&original) {
                return Err(ApiError::conflict(QUEUE_EDIT_REFUSED));
            }
            crate::runs::turn::current(&state, &original)
                .map_err(|_| ApiError::conflict(QUEUE_EDIT_REFUSED))?;
            let mut next = original.clone();
            if let Some(text) = patch.text {
                if text.trim().is_empty() && original.input.attachment_ids.is_empty() {
                    return Err(ApiError::bad_request_static(
                        "text or attachments are required",
                    ));
                }
                let mut input = original.effective_input().clone();
                input.text = text;
                next.edited_input = Some(input.clone());
                let turn = next.turn.as_mut().expect("editable checked turn");
                turn.binding.attempt_input_digest = crate::runs::turn::digest(&input);
                turn.binding.original_input_digest = turn.binding.attempt_input_digest.clone();
                turn.original_input = input;
                next.mirrored = false;
            }
            let mut changed = HashMap::from([(run_id.to_owned(), next)]);
            if let Some(target) = patch.before_run_id {
                if waiting
                    .iter()
                    .any(|id| state.runs.get(id).is_none_or(|run| !editable(run)))
                {
                    return Err(ApiError::conflict(
                        "Protected inputs prevent this queue from being reordered",
                    ));
                }
                if let Some(target) = &target {
                    if target != run_id
                        && (!waiting.contains(target)
                            || state.runs.get(target).is_none_or(|run| !editable(run)))
                    {
                        return Err(ApiError::conflict(
                            "The queue position is no longer available",
                        ));
                    }
                }
                if target.as_deref() != Some(run_id) {
                    // Reuse the accepted positions so later accepts and steers
                    // keep their original chronological relation to this queue.
                    let mut positions: Vec<u64> = waiting
                        .iter()
                        .filter_map(|id| state.runs.get(id).map(RunRecord::queue_order))
                        .collect();
                    positions.sort_unstable();
                    let mut ordered = waiting.clone();
                    ordered.retain(|id| id != run_id);
                    let index = target
                        .as_ref()
                        .and_then(|target| ordered.iter().position(|id| id == target))
                        .unwrap_or(ordered.len());
                    ordered.insert(index, run_id.to_owned());
                    for (index, id) in ordered.into_iter().enumerate() {
                        let Some(record) = state.runs.get(&id) else {
                            continue;
                        };
                        let record = changed.entry(id).or_insert_with(|| record.clone());
                        record.queue_position = Some(positions[index]);
                        record.mirrored = false;
                    }
                }
            }
            let before: Vec<RunRecord> = changed
                .keys()
                .filter_map(|id| state.runs.get(id).cloned())
                .collect();
            let changed: Vec<RunRecord> = changed.into_values().collect();
            for run in &changed {
                state.runs.insert(run.clone());
            }
            (before, changed, state.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            let mut state = self.state.write().await;
            for run in before {
                state.runs.insert(run);
            }
            return Err(ApiError::service_unavailable(error.to_string()));
        }
        let order: HashMap<_, _> = changed
            .iter()
            .map(|run| (run.id.clone(), run.queue_order()))
            .collect();
        {
            let mut queues = self
                .session_queues
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(queue) = queues.get_mut(&key) {
                for entry in queue.iter_mut() {
                    if let Some(position) = order.get(&entry.run_id) {
                        entry.accepted_at_ms = *position;
                    }
                }
                queue
                    .make_contiguous()
                    .sort_by_key(|entry| entry.accepted_at_ms);
            }
        }
        let state = self.state.read().await;
        for run in &changed {
            let parent = state.live_parent_agent(agent_id, session_id);
            state.live.publish(run_status_event(run), parent.as_deref());
        }
        Ok(changed
            .into_iter()
            .find(|run| run.id == run_id)
            .expect("edited run retained"))
    }

    #[cfg(test)]
    fn queued_entries(&self, agent_id: &str, session_id: &str) -> Vec<(String, u64)> {
        let queues = self
            .session_queues
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        queues
            .get(&(agent_id.to_owned(), session_id.to_owned()))
            .into_iter()
            .flatten()
            .map(|entry| (entry.run_id.clone(), entry.accepted_at_ms))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::test_support::{
        accept, accept_web, add_chat, chat_request, coordinator_with, wait_for, Gate,
        ScriptedModel, Step,
    };
    use crate::agent_runs::AcceptedRun;
    use crate::runs::{RunSource, RunStatus};

    const SESSION: &str = "chat:queue-controls";

    #[tokio::test]
    async fn queue_controls_edit_keeps_original_replay_and_executes_the_edited_input() {
        let model = ScriptedModel::new(vec![Step::Text(vec!["done"])]);
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let id = accept_web(&coordinator, &agent, SESSION, "original-key").await;
        let edited = coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("The revised instruction".into()),
                    before_run_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(edited.input.text, "original-key");
        assert_eq!(edited.effective_input().text, "The revised instruction");
        crate::runs::turn::validate(&edited).unwrap();
        let replay = coordinator
            .accept_run(
                accept(&agent, SESSION, "original-key"),
                coordinator.web_start(
                    agent.clone(),
                    SESSION.into(),
                    "original-key".into(),
                    "original-key".into(),
                ),
            )
            .await
            .unwrap();
        assert!(
            matches!(replay, AcceptedRun::Replayed(run) if run.id == id && run.effective_input().text == "The revised instruction")
        );
        let mut wrong = accept(&agent, SESSION, "original-key");
        wrong.text = "The revised instruction".into();
        assert!(coordinator
            .accept_run(
                wrong,
                coordinator.web_start(
                    agent.clone(),
                    SESSION.into(),
                    "unused".into(),
                    "original-key".into()
                ),
            )
            .await
            .is_err());
        drop(room);
        wait_for(&coordinator, &id, RunStatus::Completed).await;
        assert_eq!(
            model.requests()[0].messages.last().unwrap().content.text,
            "The revised instruction"
        );
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .runs
                .for_session(&agent, SESSION)
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn queue_controls_reorder_is_durable_and_drains_in_the_requested_order() {
        let model = ScriptedModel::new(vec![]);
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let first = accept_web(&coordinator, &agent, SESSION, "first").await;
        let second = accept_web(&coordinator, &agent, SESSION, "second").await;
        let third = accept_web(&coordinator, &agent, SESSION, "third").await;
        let created = coordinator
            .state
            .read()
            .await
            .runs
            .get(&third)
            .unwrap()
            .created_at_ms;
        coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &third,
                QueuedRunPatch {
                    text: None,
                    before_run_id: Some(Some(first.clone())),
                },
            )
            .await
            .unwrap();
        let snapshot = coordinator.state.read().await.control_plane_snapshot();
        let mut restored = crate::state::DaemonState::new();
        restored.restore_control_plane_snapshot(snapshot).unwrap();
        let mut recovered = restored.runs.for_session(&agent, SESSION);
        recovered.sort_by_key(|run| run.queue_order());
        assert_eq!(
            recovered.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            [third.as_str(), first.as_str(), second.as_str()]
        );
        assert_eq!(restored.runs.get(&third).unwrap().created_at_ms, created);
        drop(room);
        wait_for(&coordinator, &second, RunStatus::Completed).await;
        assert_eq!(
            model
                .requests()
                .iter()
                .map(|r| r.messages.last().unwrap().content.text.as_str())
                .collect::<Vec<_>>(),
            ["third", "first", "second"]
        );
    }

    #[tokio::test]
    async fn queue_controls_failed_save_rolls_back_input_binding_and_live_order() {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let first = accept_web(&coordinator, &agent, SESSION, "first").await;
        let second = accept_web(&coordinator, &agent, SESSION, "second").await;
        let before = coordinator
            .state
            .read()
            .await
            .runs
            .for_session(&agent, SESSION)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        gate.release.add_permits(1);
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &second,
                QueuedRunPatch {
                    text: Some("changed".into()),
                    before_run_id: Some(Some(first.clone())),
                }
            )
            .await
            .is_err());
        let after = coordinator
            .state
            .read()
            .await
            .runs
            .for_session(&agent, SESSION)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(after, before);
        assert_eq!(
            coordinator
                .queued_entries(&agent, SESSION)
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            [first.as_str(), second.as_str()]
        );
    }

    #[tokio::test]
    async fn queue_controls_refuses_a_popped_run_even_before_its_running_save() {
        let gate = Gate::new();
        let model = ScriptedModel::gated(vec![], gate.clone());
        let (coordinator, agent) = coordinator_with(model).await;
        let coordinator = coordinator.with_max_runs_per_agent(1);
        add_chat(&coordinator, &agent, SESSION).await;
        let running = {
            let coordinator = coordinator.clone();
            let request = chat_request(&agent, "other-room", "hold the slot");
            tokio::spawn(async move { coordinator.run(request).await })
        };
        gate.entered().await;
        let id = accept_web(&coordinator, &agent, SESSION, "waiting-for-slot").await;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while coordinator
                .queued_entries(&agent, SESSION)
                .iter()
                .any(|(queued, _)| queued == &id)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            coordinator.state.read().await.runs.get(&id).unwrap().status,
            RunStatus::Queued
        );
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("too late".into()),
                    before_run_id: None
                }
            )
            .await
            .is_err());
        gate.release();
        running.await.unwrap().unwrap();
        gate.entered().await;
        gate.release();
        wait_for(&coordinator, &id, RunStatus::Completed).await;
    }

    #[tokio::test]
    async fn queue_controls_refuses_foreign_targets_and_source_protected_rows_without_partial_edits(
    ) {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let id = accept_web(&coordinator, &agent, SESSION, "original").await;
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("must roll back".into()),
                    before_run_id: Some(Some("foreign-run".into()))
                }
            )
            .await
            .is_err());
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .runs
                .get(&id)
                .unwrap()
                .effective_input()
                .text,
            "original"
        );
        coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(&id)
            .unwrap()
            .source = RunSource::Delegation;
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("must refuse".into()),
                    before_run_id: None
                }
            )
            .await
            .is_err());
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .runs
                .get(&id)
                .unwrap()
                .effective_input()
                .text,
            "original"
        );
    }

    #[tokio::test]
    async fn queue_controls_edited_never_started_turn_keeps_valid_recovery_after_restore() {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let id = accept_web(&coordinator, &agent, SESSION, "original").await;
        coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("latest owner instruction".into()),
                    before_run_id: None,
                },
            )
            .await
            .unwrap();
        let snapshot = coordinator.state.read().await.control_plane_snapshot();
        let mut restored = crate::state::DaemonState::new();
        restored.restore_control_plane_snapshot(snapshot).unwrap();
        let run = restored.runs.get(&id).unwrap();
        assert_eq!(run.effective_input().text, "latest owner instruction");
        assert_eq!(
            run.turn.as_ref().unwrap().original_input.text,
            "latest owner instruction"
        );
        crate::runs::turn::current(&restored, run).unwrap();
    }

    #[tokio::test]
    async fn queue_controls_text_only_keeps_position_and_null_moves_to_end() {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let first = accept_web(&coordinator, &agent, SESSION, "first").await;
        let second = accept_web(&coordinator, &agent, SESSION, "second").await;
        coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &first,
                QueuedRunPatch {
                    text: Some("edited first".into()),
                    before_run_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            coordinator
                .queued_entries(&agent, SESSION)
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            [first.as_str(), second.as_str()]
        );
        coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &first,
                QueuedRunPatch {
                    text: None,
                    before_run_id: Some(None),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            coordinator
                .queued_entries(&agent, SESSION)
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            [second.as_str(), first.as_str()]
        );
        let before = coordinator
            .state
            .read()
            .await
            .runs
            .get(&first)
            .unwrap()
            .clone();
        for patch in [
            QueuedRunPatch::default(),
            QueuedRunPatch {
                text: Some(" ".into()),
                before_run_id: Some(Some(second.clone())),
            },
            QueuedRunPatch {
                text: Some("x".repeat(MAX_RUN_INPUT_TEXT_BYTES + 1)),
                before_run_id: None,
            },
        ] {
            assert!(coordinator
                .update_queued_run(&agent, SESSION, &first, patch)
                .await
                .is_err());
            assert_eq!(
                coordinator.state.read().await.runs.get(&first).unwrap(),
                &before
            );
        }
    }

    #[tokio::test]
    async fn queue_controls_protected_sources_and_existing_binding_cannot_be_reauthorized_by_editing(
    ) {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let id = accept_web(&coordinator, &agent, SESSION, "original").await;
        let original = coordinator
            .state
            .read()
            .await
            .runs
            .get(&id)
            .unwrap()
            .clone();
        for source in [
            RunSource::Telegram,
            RunSource::Discord,
            RunSource::Email,
            RunSource::Schedule,
            RunSource::Job,
            RunSource::Delegation,
            RunSource::Peer,
        ] {
            let mut protected = original.clone();
            protected.source = source;
            coordinator
                .state
                .write()
                .await
                .runs
                .insert(protected.clone());
            assert!(coordinator
                .update_queued_run(
                    &agent,
                    SESSION,
                    &id,
                    QueuedRunPatch {
                        text: Some("changed".into()),
                        before_run_id: None
                    }
                )
                .await
                .is_err());
            assert_eq!(
                coordinator.state.read().await.runs.get(&id).unwrap(),
                &protected
            );
        }
        let mut protected = original.clone();
        protected.system_followup = Some(true);
        coordinator
            .state
            .write()
            .await
            .runs
            .insert(protected.clone());
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("changed".into()),
                    before_run_id: None
                }
            )
            .await
            .is_err());
        assert_eq!(
            coordinator.state.read().await.runs.get(&id).unwrap(),
            &protected
        );
        let mut stale = original;
        stale.turn.as_mut().unwrap().binding.config_digest = "f".repeat(64);
        coordinator.state.write().await.runs.insert(stale.clone());
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("changed".into()),
                    before_run_id: None
                }
            )
            .await
            .is_err());
        assert_eq!(
            coordinator.state.read().await.runs.get(&id).unwrap(),
            &stale
        );
    }

    #[tokio::test]
    async fn queue_controls_restart_recovers_effective_prompts_in_saved_order() {
        let model = ScriptedModel::new(vec![]);
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let dir = tempfile::tempdir().unwrap();
        coordinator
            .state
            .write()
            .await
            .set_control_plane_store(Some(
                crate::control_plane_store::ControlPlaneStoreConfig::Json(
                    dir.path().join("state.json"),
                ),
            ));
        let first = accept_web(&coordinator, &agent, SESSION, "first").await;
        let second = accept_web(&coordinator, &agent, SESSION, "second").await;
        coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &second,
                QueuedRunPatch {
                    text: Some("revised second".into()),
                    before_run_id: Some(Some(first.clone())),
                },
            )
            .await
            .unwrap();
        let snapshot = coordinator.state.read().await.control_plane_snapshot();
        let mut restored = crate::state::DaemonState::new();
        restored.restore_control_plane_snapshot(snapshot).unwrap();
        // Restore durable evidence while retaining the fixture's test adapter.
        // Pending originals are now interrupted and cannot execute.
        coordinator.state.write().await.runs = restored.runs;
        coordinator.auto_recover_unstarted().await;
        let (first_child, second_child) = {
            let state = coordinator.state.read().await;
            let first_child = state
                .runs
                .get(&first)
                .unwrap()
                .recovery
                .as_ref()
                .unwrap()
                .continuation_run_id
                .clone();
            let source = state.runs.get(&second).unwrap();
            assert_eq!(source.input.text, "second");
            let second_child = source
                .recovery
                .as_ref()
                .unwrap()
                .continuation_run_id
                .clone();
            assert_eq!(
                state.runs.get(&second_child).unwrap().input.text,
                "revised second"
            );
            (first_child, second_child)
        };
        drop(room);
        wait_for(&coordinator, &first_child, RunStatus::Completed).await;
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .runs
                .get(&second_child)
                .unwrap()
                .status,
            RunStatus::Completed
        );
        assert_eq!(
            model
                .requests()
                .iter()
                .map(|r| r.messages.last().unwrap().content.text.clone())
                .collect::<Vec<_>>(),
            ["revised second", "first"]
        );
    }

    #[tokio::test]
    async fn queue_controls_protected_waiting_row_refuses_reorder_without_partial_text_or_order() {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let first = accept_web(&coordinator, &agent, SESSION, "first").await;
        let second = accept_web(&coordinator, &agent, SESSION, "second").await;
        coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(&second)
            .unwrap()
            .system_followup = Some(true);
        let before = coordinator
            .state
            .read()
            .await
            .runs
            .for_session(&agent, SESSION)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let before_queue = coordinator.queued_entries(&agent, SESSION);
        assert!(coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &first,
                QueuedRunPatch {
                    text: Some("must not save".into()),
                    before_run_id: Some(None),
                }
            )
            .await
            .is_err());
        let after = coordinator
            .state
            .read()
            .await
            .runs
            .for_session(&agent, SESSION)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(before, after);
        assert_eq!(before_queue, coordinator.queued_entries(&agent, SESSION));
    }

    #[tokio::test]
    async fn queue_controls_requires_live_ordinary_owner_session_but_allows_revisions() {
        let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let _room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let id = accept_web(&coordinator, &agent, SESSION, "original").await;
        let original = coordinator
            .state
            .read()
            .await
            .sessions
            .get(&agent, SESSION)
            .unwrap()
            .clone();
        let mut protected = Vec::new();
        let mut archived = original.clone();
        archived.archived = true;
        protected.push(archived);
        let mut helper = original.clone();
        helper.parent_agent_id = Some("parent".into());
        protected.push(helper);
        let mut job = original.clone();
        job.parent_run_id = Some("parent-run".into());
        protected.push(job);
        let mut channel = original.clone();
        channel.origin = SessionOrigin::Telegram;
        protected.push(channel);
        for session in protected {
            *coordinator
                .state
                .write()
                .await
                .sessions
                .get_mut(&agent, SESSION)
                .unwrap() = session;
            assert!(coordinator
                .update_queued_run(
                    &agent,
                    SESSION,
                    &id,
                    QueuedRunPatch {
                        text: Some("must not save".into()),
                        before_run_id: None,
                    }
                )
                .await
                .is_err());
            assert_eq!(
                coordinator
                    .state
                    .read()
                    .await
                    .runs
                    .get(&id)
                    .unwrap()
                    .effective_input()
                    .text,
                "original"
            );
        }
        let mut revision = original;
        revision.parent_session_id = Some("chat:source".into());
        *coordinator
            .state
            .write()
            .await
            .sessions
            .get_mut(&agent, SESSION)
            .unwrap() = revision;
        let edited = coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &id,
                QueuedRunPatch {
                    text: Some("revision prompt".into()),
                    before_run_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(edited.effective_input().text, "revision prompt");
    }

    #[tokio::test]
    async fn queue_controls_cancelled_sibling_keeps_its_record_and_does_not_block_remaining_order()
    {
        let model = ScriptedModel::new(vec![]);
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, SESSION).await;
        let room = coordinator.try_reserve_room(&agent, SESSION).unwrap();
        let first = accept_web(&coordinator, &agent, SESSION, "first").await;
        let cancelled = accept_web(&coordinator, &agent, SESSION, "cancelled").await;
        let last = accept_web(&coordinator, &agent, SESSION, "last").await;
        coordinator.stop_run(&agent, &cancelled).await.unwrap();
        let cancelled_before = coordinator
            .state
            .read()
            .await
            .runs
            .get(&cancelled)
            .unwrap()
            .clone();
        assert_eq!(cancelled_before.status, RunStatus::Cancelled);
        assert!(coordinator
            .queued_entries(&agent, SESSION)
            .iter()
            .any(|(id, _)| id == &cancelled));
        coordinator
            .update_queued_run(
                &agent,
                SESSION,
                &last,
                QueuedRunPatch {
                    text: None,
                    before_run_id: Some(Some(first.clone())),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            coordinator.state.read().await.runs.get(&cancelled).unwrap(),
            &cancelled_before
        );
        drop(room);
        wait_for(&coordinator, &first, RunStatus::Completed).await;
        assert_eq!(
            model
                .requests()
                .iter()
                .map(|request| request.messages.last().unwrap().content.text.clone())
                .collect::<Vec<_>>(),
            ["last", "first"]
        );
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .runs
                .get(&last)
                .unwrap()
                .status,
            RunStatus::Completed
        );
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .runs
                .get(&cancelled)
                .unwrap()
                .status,
            RunStatus::Cancelled
        );
    }
}
