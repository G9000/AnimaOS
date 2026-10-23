//! Exact run-bound controls for a direct owner conversation.
use super::AgentRunCoordinator;
use crate::runs::{RunLink, RunSource};
use crate::sessions::{clean_owner_title, SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;

const OWNER_CHAT_REQUIRED: &str = "Chat controls require a live direct owner web/API chat run";

pub(crate) fn validate_owner_chat<'a>(
    state: &'a DaemonState,
    link: &RunLink,
) -> Result<&'a SessionRecord, String> {
    let runtime = state
        .agents
        .get(&link.agent_id)
        .ok_or(OWNER_CHAT_REQUIRED)?;
    let run = state.runs.get(&link.run_id).ok_or(OWNER_CHAT_REQUIRED)?;
    let session = state
        .sessions
        .get(&link.agent_id, &link.session_id)
        .ok_or(OWNER_CHAT_REQUIRED)?;
    if crate::agent_runs::is_helper_config(runtime.config())
        || run.agent_id != link.agent_id
        || run.session_id != link.session_id
        || !matches!(run.source, RunSource::Web | RunSource::Api)
        || !run.status.is_in_flight()
        || run.stop.is_some()
        || state
            .live
            .runs()
            .control(&link.run_id)
            .is_none_or(|control| control.cancel.is_cancelled())
        || run.system_followup.unwrap_or(false)
        || run.parent_run_id.is_some()
        || session.kind != SessionKind::Chat
        || !matches!(session.origin, SessionOrigin::Web | SessionOrigin::Api)
        || session.parent_run_id.is_some()
        || session.parent_agent_id.is_some()
        || session.archived
        || session.pending_clear_run_id.is_some()
        || state.pending_history_deletions.iter().any(|deletion| {
            deletion.agent_id == link.agent_id
                && deletion
                    .session_id
                    .as_deref()
                    .is_none_or(|id| id == link.session_id)
        })
    {
        return Err(OWNER_CHAT_REQUIRED.into());
    }
    Ok(session)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatExport {
    pub(crate) url: String,
    pub(crate) filename: String,
    pub(crate) requires_owner_authentication: bool,
}

fn path_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

impl AgentRunCoordinator {
    pub(crate) async fn rename_owner_chat(
        &self,
        link: &RunLink,
        title: &str,
    ) -> Result<String, String> {
        let title = clean_owner_title(title).map_err(str::to_string)?;
        let coordinator = self.clone();
        let link = link.clone();
        // Once mutation starts, cancellation of a tool dispatch cannot interrupt
        // its durable save or rollback and release an inconsistent transaction.
        tokio::spawn(async move { coordinator.rename_owner_chat_saved(&link, title).await })
            .await
            .map_err(|_| "The chat rename worker stopped".to_string())?
    }

    async fn rename_owner_chat_saved(
        &self,
        link: &RunLink,
        title: String,
    ) -> Result<String, String> {
        let _transaction = self.control_plane_transaction().await;
        let (previous, persist) = {
            let mut state = self.state.write().await;
            let session = validate_owner_chat(&state, link)?;
            let previous = (session.title.clone(), session.title_source);
            let session = state
                .sessions
                .get_mut(&link.agent_id, &link.session_id)
                .ok_or(OWNER_CHAT_REQUIRED)?;
            session.title = title.clone();
            session.title_source = TitleSource::Owner;
            (previous, state.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            if let Some(session) = self
                .state
                .write()
                .await
                .sessions
                .get_mut(&link.agent_id, &link.session_id)
            {
                (session.title, session.title_source) = previous;
            }
            return Err(format!("The chat title could not be saved: {error}"));
        }
        self.state.read().await.publish_session_event(
            &link.agent_id,
            &link.session_id,
            crate::live::LiveEventBody::SessionUpdated,
        );
        Ok(title)
    }

    pub(crate) async fn compact_owner_chat(
        &self,
        link: &RunLink,
        options: &crate::sessions::compaction::CompactionOptions,
        cancel: Option<anima_core::CancelSignal>,
    ) -> Result<(), String> {
        let coordinator = self.clone();
        let link = link.clone();
        let options = options.clone();
        tokio::spawn(async move {
            coordinator
                .compact_owner_chat_saved(&link, &options, cancel)
                .await
        })
        .await
        .map_err(|_| "The chat compaction worker stopped".to_string())?
    }

    async fn compact_owner_chat_saved(
        &self,
        link: &RunLink,
        options: &crate::sessions::compaction::CompactionOptions,
        cancel: Option<anima_core::CancelSignal>,
    ) -> Result<(), String> {
        let input = {
            let state = self.state.read().await;
            let session = validate_owner_chat(&state, link)?;
            let history = state.model_visible_history(&link.agent_id, session.room_id());
            let input = crate::sessions::compaction::manual_compaction_input_with_keep(
                &history,
                session.summary.as_ref(),
                options.keep_latest_exchanges,
            );
            if input.is_empty()
                && crate::sessions::context::uncovered_pruned_through(session, &history).is_none()
            {
                return Err("Nothing to compact yet".into());
            }
            input
        };
        // This owner turn already holds the room. Reserving it again would deadlock.
        self.compact_session_with_options(
            &link.agent_id,
            &link.session_id,
            &input,
            options,
            Some(link),
            cancel,
        )
        .await
    }

    pub(crate) async fn export_owner_chat(&self, link: &RunLink) -> Result<ChatExport, String> {
        {
            let state = self.state.read().await;
            validate_owner_chat(&state, link)?;
        }
        let (record, agent_name, messages) =
            crate::sessions::views::full_transcript(&self.state, &link.agent_id, &link.session_id)
                .await
                .map_err(|_| {
                    "The full transcript is unavailable; no export was prepared".to_string()
                })?;
        // Exercise the shared full renderer, but never return the transcript to the model.
        let _markdown =
            crate::sessions::views::transcript_markdown(&record, &agent_name, &messages);
        let state = self.state.read().await;
        let current = validate_owner_chat(&state, link)?;
        if current.created_at_ms != record.created_at_ms
            || current.clear_generation != record.clear_generation
        {
            return Err("The conversation changed while preparing its export".into());
        }
        Ok(ChatExport {
            url: format!(
                "/api/agents/{}/sessions/{}/export",
                path_segment(&link.agent_id),
                path_segment(&link.session_id)
            ),
            filename: format!(
                "{}.md",
                crate::sessions::views::export_file_stem(&record.title)
            ),
            requires_owner_authentication: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::test_support::{
        add_chat, chat_request, coordinator_with, ScriptedModel, Step,
    };

    async fn fixture() -> (AgentRunCoordinator, RunLink) {
        let (coordinator, agent) =
            coordinator_with(ScriptedModel::new(vec![Step::Text(vec!["saved reply"])])).await;
        add_chat(&coordinator, &agent, "chat:controls").await;
        coordinator
            .run(chat_request(
                &agent,
                "chat:controls",
                "full transcript entry",
            ))
            .await
            .unwrap();
        let link = {
            let mut state = coordinator.state.write().await;
            let run = state.runs.for_session(&agent, "chat:controls")[0]
                .id
                .clone();
            let record = state.runs.get_mut(&run).unwrap();
            record.status = crate::runs::RunStatus::Running;
            record.stop = None;
            state.live.runs().register(&run);
            RunLink {
                agent_id: agent,
                session_id: "chat:controls".into(),
                run_id: run,
            }
        };
        (coordinator, link)
    }

    #[tokio::test]
    async fn owner_chat_controls_rename_is_durable_and_rolls_back_failed_save() {
        let (coordinator, link) = fixture().await;
        coordinator
            .rename_owner_chat(&link, "  Renamed chat  ")
            .await
            .unwrap();
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .sessions
                .get(&link.agent_id, &link.session_id)
                .unwrap()
                .title,
            "Renamed chat"
        );
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let rename = {
            let coordinator = coordinator.clone();
            let link = link.clone();
            tokio::spawn(async move {
                coordinator
                    .rename_owner_chat(&link, "Should roll back")
                    .await
            })
        };
        gate.entered.acquire().await.unwrap().forget();
        gate.release.add_permits(1);
        assert!(rename.await.unwrap().is_err());
        assert_eq!(
            coordinator
                .state
                .read()
                .await
                .sessions
                .get(&link.agent_id, &link.session_id)
                .unwrap()
                .title,
            "Renamed chat"
        );
    }

    #[tokio::test]
    async fn owner_rename_finishes_rollback_if_dispatch_is_dropped_during_save() {
        let (coordinator, link) = fixture().await;
        let original = coordinator
            .state
            .read()
            .await
            .sessions
            .get(&link.agent_id, &link.session_id)
            .unwrap()
            .title
            .clone();
        let gate = coordinator
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let rename = {
            let coordinator = coordinator.clone();
            let link = link.clone();
            tokio::spawn(async move { coordinator.rename_owner_chat(&link, "Unsaved title").await })
        };
        gate.entered.acquire().await.unwrap().forget();
        rename.abort();
        gate.release.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if coordinator
                    .state
                    .read()
                    .await
                    .sessions
                    .get(&link.agent_id, &link.session_id)
                    .unwrap()
                    .title
                    == original
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("an interrupted tool dispatch cannot leave an unsaved title in memory");
    }

    #[tokio::test]
    async fn owner_chat_controls_revalidate_exact_run_origin_and_session() {
        let (coordinator, link) = fixture().await;
        for source in [
            crate::runs::RunSource::Telegram,
            crate::runs::RunSource::Delegation,
            crate::runs::RunSource::Schedule,
        ] {
            coordinator
                .state
                .write()
                .await
                .runs
                .get_mut(&link.run_id)
                .unwrap()
                .source = source;
            assert!(coordinator
                .rename_owner_chat(&link, "Forbidden")
                .await
                .is_err());
        }
        let mut state = coordinator.state.write().await;
        let run = state.runs.get_mut(&link.run_id).unwrap();
        run.source = crate::runs::RunSource::Web;
        run.system_followup = Some(true);
        assert!(validate_owner_chat(&state, &link).is_err());
        state.runs.get_mut(&link.run_id).unwrap().system_followup = None;
        state
            .sessions
            .get_mut(&link.agent_id, &link.session_id)
            .unwrap()
            .origin = crate::sessions::SessionOrigin::Peer;
        assert!(validate_owner_chat(&state, &link).is_err());
    }

    #[tokio::test]
    async fn owner_chat_controls_refuse_stopped_or_missing_live_run_and_forged_link() {
        let (coordinator, link) = fixture().await;
        let mut forged = link.clone();
        forged.session_id = "another-chat".into();
        assert!(coordinator
            .rename_owner_chat(&forged, "Forbidden")
            .await
            .is_err());
        let state = coordinator.state.read().await;
        state
            .live
            .runs()
            .control(&link.run_id)
            .unwrap()
            .cancel
            .cancel();
        assert!(validate_owner_chat(&state, &link).is_err());
        state.live.runs().remove(&link.run_id);
        assert!(validate_owner_chat(&state, &link).is_err());
    }

    #[tokio::test]
    async fn owner_chat_catalog_requires_exact_host_schema_and_rechecks_current_scope() {
        use crate::tools::catalog::RunToolCatalog;
        use anima_core::CancelSignal;
        let (coordinator, link) = fixture().await;
        let mut agent = coordinator
            .state
            .read()
            .await
            .get_agent(&link.agent_id)
            .unwrap()
            .state;
        let original = agent.config.clone();
        agent.config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["rename_chat", "compact_chat", "export_chat"])
                .unwrap(),
        );
        let catalog = RunToolCatalog::new(
            &coordinator.state,
            link.clone(),
            CancelSignal::default(),
            &original,
            &agent,
            None,
        );
        for name in ["rename_chat", "compact_chat", "export_chat"] {
            assert!(catalog.check(&agent, &link, name).await.is_ok());
        }
        let mut spoof = agent.clone();
        spoof.config.tools.as_mut().unwrap()[0]
            .description
            .push_str(" forged");
        let rejected = RunToolCatalog::new(
            &coordinator.state,
            link.clone(),
            CancelSignal::default(),
            &original,
            &spoof,
            None,
        );
        assert!(rejected.check(&spoof, &link, "rename_chat").await.is_err());
        let peer = RunToolCatalog::new(
            &coordinator.state,
            link.clone(),
            CancelSignal::default(),
            &original,
            &agent,
            None,
        )
        .with_origins(None, vec![link.agent_id.clone()]);
        assert!(peer.check(&agent, &link, "rename_chat").await.is_err());
        coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(&link.run_id)
            .unwrap()
            .system_followup = Some(true);
        assert!(catalog.check(&agent, &link, "rename_chat").await.is_err());
    }

    #[tokio::test]
    async fn owner_compaction_rejects_noop_without_a_secondary_call() {
        let (coordinator, link) = fixture().await;
        let options = crate::sessions::compaction::CompactionOptions::default();
        assert!(coordinator
            .compact_owner_chat(&link, &options, None)
            .await
            .unwrap_err()
            .contains("Nothing to compact"));
        assert!(coordinator
            .state
            .read()
            .await
            .sessions
            .get(&link.agent_id, &link.session_id)
            .unwrap()
            .summary
            .is_none());
    }

    #[tokio::test]
    async fn owner_compaction_inside_its_room_survives_terminal_save_and_reaches_next_turn() {
        use anima_core::{DataValue, ToolCall};
        let model = ScriptedModel::with_secondary(
            vec![
                Step::Text(vec!["first reply"]),
                Step::Text(vec!["second reply"]),
                Step::Tools(vec![ToolCall {
                    id: "compact".into(),
                    name: "compact_chat".into(),
                    args: [
                        ("keepLatestExchanges".into(), DataValue::Number(1.0)),
                        (
                            "focus".into(),
                            DataValue::String("deployment decisions".into()),
                        ),
                    ]
                    .into(),
                }]),
                Step::Text(vec!["Compacted and saved."]),
                Step::Text(vec!["Continuing"]),
            ],
            vec![Step::Text(vec![
                "Saved deployment decisions from the first exchange.",
            ])],
        );
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, "chat:compact").await;
        for text in [
            "first deployment discussion",
            "second deployment discussion",
            "compact our history",
        ] {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                coordinator.run(chat_request(&agent, "chat:compact", text)),
            )
            .await
            .expect("inline compaction must not reserve its own room")
            .unwrap();
        }
        let summary = coordinator
            .state
            .read()
            .await
            .sessions
            .get(&agent, "chat:compact")
            .unwrap()
            .summary
            .clone()
            .expect("the tool summary survives terminal persistence");
        assert!(summary.text.contains("Saved deployment decisions"));
        let secondary = model.secondary_requests();
        assert_eq!(secondary.len(), 1);
        assert!(secondary[0].messages[0]
            .content
            .text
            .contains("first deployment discussion"));
        assert!(!secondary[0].messages[0]
            .content
            .text
            .contains("second deployment discussion"));
        assert!(secondary[0].messages[0]
            .content
            .text
            .contains("deployment decisions"));
        coordinator
            .run(chat_request(&agent, "chat:compact", "continue"))
            .await
            .unwrap();
        assert!(model
            .requests()
            .last()
            .unwrap()
            .system
            .contains(&summary.text));
        let (_, _, messages) =
            crate::sessions::views::full_transcript(&coordinator.state, &agent, "chat:compact")
                .await
                .unwrap();
        assert!(messages
            .iter()
            .any(|message| message.content.text == "first deployment discussion"));
        assert!(messages
            .iter()
            .any(|message| message.content.text == "second deployment discussion"));
    }

    #[tokio::test]
    async fn owner_chat_export_verifies_full_history_and_returns_only_a_local_download_handle() {
        let (coordinator, link) = fixture().await;
        let export = coordinator.export_owner_chat(&link).await.unwrap();
        assert!(export.url.starts_with("/api/agents/"));
        assert!(export.url.ends_with("/export"));
        assert!(export.filename.ends_with(".md"));
        let encoded = serde_json::to_string(&export).unwrap();
        assert!(!encoded.contains("full transcript entry"));
        assert!(!encoded.contains("https://"));
    }
}
