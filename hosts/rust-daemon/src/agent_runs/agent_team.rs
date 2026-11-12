//! Who talks with whom, and dissolving an agent's temporary helper team.
use super::{
    config_helper_parent, is_standalone_config, talks_with_agents, AgentRunCoordinator, PEER_TOOLS,
};

impl AgentRunCoordinator {
    /// Turns the owner's talk-with-agents grant on or off for one standalone
    /// agent, through the same validated and persisted path as Settings.
    pub(crate) async fn set_agent_talk(
        &self,
        target: &str,
        enabled: bool,
    ) -> Result<serde_json::Value, String> {
        let (agent_id, name, tools, before) = {
            let guard = self.state.read().await;
            let mut matches = guard.agent_states().into_iter().filter(|agent| {
                is_standalone_config(&agent.config)
                    && (agent.id == target || agent.name.eq_ignore_ascii_case(target))
            });
            let agent = match (matches.next(), matches.next()) {
                (Some(agent), None) => agent,
                (Some(_), Some(_)) => {
                    return Err("More than one agent has that name; use its ID".into())
                }
                _ => return Err("No agent has that name or ID".into()),
            };
            let before = talks_with_agents(&agent.config);
            let mut tools: Vec<String> = agent
                .config
                .tools
                .iter()
                .flatten()
                .map(|tool| tool.name.clone())
                .filter(|name| !PEER_TOOLS.contains(&name.as_str()))
                .collect();
            if enabled {
                tools.extend(PEER_TOOLS.iter().map(|name| name.to_string()));
            }
            (agent.id, agent.name, tools, before)
        };
        if before != enabled {
            let _transaction = self.control_plane_transaction().await;
            let body = serde_json::to_vec(&serde_json::json!({ "tools": tools }))
                .map_err(|error| error.to_string())?;
            crate::routes::agents::handle_update_agent(&agent_id, body, &self.state)
                .await
                .map_err(|error| error.message().to_string())?;
        }
        Ok(serde_json::json!({
            "agentId": agent_id,
            "name": name,
            "talksWithAgents": enabled,
            "changed": before != enabled,
        }))
    }

    /// Removes `owner`'s helpers that are not working now. Busy ones stay.
    pub(crate) async fn dissolve_helpers(&self, owner: &str) -> Result<serde_json::Value, String> {
        let _transaction = self.control_plane_transaction().await;
        let (removed, kept, persist) = {
            let mut guard = self.state.write().await;
            let helpers: Vec<_> = guard
                .agent_states()
                .into_iter()
                .filter(|agent| config_helper_parent(&agent.config) == Some(owner))
                .collect();
            let mut removed = Vec::new();
            let mut kept = 0usize;
            for helper in helpers {
                let idle = guard.in_flight_runs(&helper.id) == 0
                    && !guard.runs.has_active_assignment(&helper.id)
                    && !self.is_agent_busy(&helper.id);
                if !idle {
                    kept += 1;
                    continue;
                }
                if let Some(snapshot) = guard.get_agent(&helper.id) {
                    guard.remove_agent(&helper.id);
                    removed.push(snapshot);
                }
            }
            (removed, kept, guard.control_plane_persist_request())
        };
        if removed.is_empty() {
            return Ok(serde_json::json!({ "dissolved": 0, "stillWorking": kept }));
        }
        if let Err(error) = persist.save().await {
            let mut guard = self.state.write().await;
            for snapshot in removed {
                let _ = guard.restore_removed_agent(snapshot);
            }
            return Err(format!("Could not save the dissolved team: {error}"));
        }
        let guard = self.state.read().await;
        for snapshot in &removed {
            guard.live.close_agent(&snapshot.state.id);
        }
        Ok(serde_json::json!({
            "dissolved": removed.len(),
            "names": removed.iter().map(|snapshot| snapshot.state.name.clone()).collect::<Vec<_>>(),
            "stillWorking": kept,
        }))
    }
}
