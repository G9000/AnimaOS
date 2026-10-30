//! One selection contract for execution and owner-visible capability inspection.
use anima_core::{AgentConfig, DataValue, ToolDescriptor};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatToolCapability {
    name: String,
    description: String,
    origin: &'static str,
    risk: &'static str,
    approval: &'static str,
    argument_dependent: bool,
    available: bool,
    reason: Option<&'static str>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatConnectionCapability {
    id: String,
    label: String,
    state: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatCapabilities {
    agent_id: String,
    session_id: String,
    basis: &'static str,
    run_id: Option<String>,
    preparing: bool,
    tools: Vec<ChatToolCapability>,
    connections: Vec<ChatConnectionCapability>,
    notice: &'static str,
    runtime: crate::routes::capabilities::RuntimeCapabilityObservation,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Scope {
    pub can_delegate: bool,
    pub helper_completion: bool,
    pub workflow_job: bool,
    pub reminder_verification: bool,
    pub delegated: bool,
    pub peers: bool,
    pub owner_chat: bool,
    pub owner_main: bool,
    pub discord_thread: bool,
    pub mcp_available: bool,
    pub automatic_mcp: bool,
}

pub(crate) fn discovery_configured(config: &AgentConfig) -> bool {
    config
        .settings
        .as_ref()
        .and_then(|s| s.additional.get("toolDiscovery"))
        == Some(&DataValue::Bool(true))
        || (config.allows_tool("search_tools") && config.allows_tool("describe_tool"))
}

pub(crate) fn discovery(config: &AgentConfig, scope: Scope) -> bool {
    !scope.reminder_verification && discovery_configured(config)
}

pub(crate) fn select(
    config: &AgentConfig,
    ready_mcp: &[ToolDescriptor],
    scope: Scope,
) -> Vec<ToolDescriptor> {
    let registry = crate::tools::ToolRegistry::new();
    let mut tools = config.tools.clone().unwrap_or_default();
    // Existing agents may retain older serialized history schemas. Refresh
    // definitions only for names they already hold; this adds no grant.
    for tool in &mut tools {
        if matches!(
            tool.name.as_str(),
            "search_conversations" | "read_conversation_window"
        ) {
            if let Some(current) = registry.descriptor(&tool.name) {
                *tool = current;
            }
        }
    }
    if scope.automatic_mcp && !super::is_helper_config(config) && !scope.delegated && !scope.peers {
        for tool in ready_mcp {
            if !tools.contains(tool) {
                tools.retain(|old| old.name != tool.name);
                tools.push(tool.clone());
            }
        }
    }
    tools.retain(|tool| {
        !matches!(tool.name.as_str(), "delegate_to_agent" | "spawn_helper")
            && (scope.can_delegate
                || !matches!(
                    tool.name.as_str(),
                    "spawn_helper_async"
                        | "spawn_helpers"
                        | "wait_helper_runs"
                        | "list_helper_runs"
                        | "steer_helper"
                        | "stop_helper"
                ))
    });
    if scope.workflow_job {
        tools.retain(|tool| {
            !super::is_coordination_tool(&tool.name)
                && !matches!(
                    tool.name.as_str(),
                    "create_workflow" | "save_workflow_template" | "run_workflow_template"
                )
        });
    }
    if scope.helper_completion {
        tools.retain(|tool| {
            !super::is_coordination_tool(&tool.name)
                && !crate::tools::is_automation_tool(&tool.name)
                && tool.name != "bg_start"
        });
    }
    if scope.delegated {
        tools.retain(|tool| !matches!(tool.name.as_str(), "send_message" | "broadcast_message"));
    }
    tools.retain(|tool| !tool.name.starts_with("mcp_") || ready_mcp.contains(tool));
    let mut add = |names: &[&str]| {
        for name in names {
            if !tools.iter().any(|tool| tool.name == *name) {
                tools.push(registry.descriptor(name).expect("registered native tool"));
            }
        }
    };
    if !super::is_helper_config(config)
        && !scope.delegated
        && !scope.peers
        && config.allows_tool("memory_add")
    {
        add(&["core_memory"]);
    }
    if !scope.delegated && !scope.helper_completion && !scope.workflow_job {
        add(&["list_workspace_agents", "send_message", "broadcast_message"]);
    }
    if scope.can_delegate {
        add(&["list_workspace_agents", "delegate_to_agent", "spawn_helper"]);
    }
    if discovery(config, scope) {
        add(&["search_tools", "describe_tool"]);
    }
    if scope.reminder_verification {
        tools.clear();
    }
    tools.retain(|tool| {
        !matches!(
            tool.name.as_str(),
            "clear_main_chat"
                | "rename_discord_thread"
                | "send_discord_file"
                | "read_discord_thread"
                | "rename_chat"
                | "compact_chat"
                | "export_chat"
                | "manage_mcp"
                | "show_result"
                | "list_results"
        )
    });
    if scope.owner_chat {
        for name in [
            "rename_chat",
            "compact_chat",
            "export_chat",
            "show_result",
            "list_results",
        ] {
            tools.push(registry.descriptor(name).expect("registered owner control"));
        }
        if scope.mcp_available {
            tools.push(registry.descriptor("manage_mcp").unwrap());
        }
    }
    if scope.owner_main {
        tools.push(registry.descriptor("clear_main_chat").unwrap());
    }
    if scope.discord_thread
        && !scope.reminder_verification
        && !scope.helper_completion
        && !scope.workflow_job
        && !scope.delegated
        && !scope.peers
        && !super::is_helper_config(config)
    {
        tools.push(registry.descriptor("rename_discord_thread").unwrap());
        tools.push(registry.descriptor("send_discord_file").unwrap());
        tools.push(registry.descriptor("read_discord_thread").unwrap());
    }
    tools
}

impl super::AgentRunCoordinator {
    pub(crate) async fn chat_capabilities(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<ChatCapabilities, crate::routes::ApiError> {
        use crate::sessions::{SessionKind, SessionOrigin};
        let connection_views = match &self.mcp {
            Some(manager) => manager.list(agent_id).await.unwrap_or_default(),
            None => vec![],
        };
        let state = self.state.read().await;
        let runtime = state
            .agents
            .get(agent_id)
            .ok_or_else(crate::routes::ApiError::not_found)?;
        let session = state
            .sessions
            .get(agent_id, session_id)
            .ok_or_else(crate::routes::ApiError::not_found)?;
        if session.kind != SessionKind::Chat
            || !matches!(session.origin, SessionOrigin::Web | SessionOrigin::Api)
            || session.parent_run_id.is_some()
            || session.parent_agent_id.is_some()
            || super::is_helper_config(runtime.config())
        {
            return Err(crate::routes::ApiError::conflict(
                "Capabilities require a direct owner web/API chat",
            ));
        }
        let owner_chat = !session.archived
            && session.pending_clear_run_id.is_none()
            && !state.pending_history_deletions.iter().any(|d| {
                d.agent_id == agent_id && d.session_id.as_deref().is_none_or(|id| id == session_id)
            });
        let scope = Scope {
            can_delegate: super::is_workspace_manager(&runtime.state()),
            owner_chat,
            owner_main: owner_chat && session.main_chat && session.has_main_chat_identity(),
            mcp_available: self.mcp.is_some(),
            automatic_mcp: crate::mcp::McpToolAccess::for_agent(&state, agent_id)
                == crate::mcp::McpToolAccess::Automatic,
            ..Scope::default()
        };
        let ready = self
            .mcp
            .as_ref()
            .map(|m| m.ready_descriptors(agent_id))
            .unwrap_or_default();
        let mut current = select(runtime.config(), &ready, scope);
        if !owner_chat {
            current.clear();
        }
        let active = state
            .runs
            .for_session(agent_id, session_id)
            .into_iter()
            .find(|r| r.status.is_in_flight() && r.started_at_ms.is_some());
        let (basis, run_id, preparing, ceiling) = match active {
            Some(run) => {
                let captured = state.live.runs().tool_scope(&run.id);
                (
                    "active_turn",
                    Some(run.id.clone()),
                    captured.is_none(),
                    captured.unwrap_or_default(),
                )
            }
            None => ("next_owner_turn", None, false, current.clone()),
        };
        let agent = runtime.state();
        let tools = ceiling.iter().map(|tool| {
            let available = current.contains(tool) && active.is_none_or(|r| r.stop.is_none()
                && state.live.runs().control(&r.id).is_some_and(|c| !c.cancel.is_cancelled()));
            let risk = crate::approvals::policy::risk_class(&tool.name);
            let verdict = state.approval_policy_verdict(&agent, session_id, &anima_core::ToolCall {
                id: "capabilities".into(), name: tool.name.clone(), args: Default::default(),
            });
            let approval = match verdict {
                crate::approvals::Verdict::Allow => "allow",
                crate::approvals::Verdict::Ask => "ask",
                crate::approvals::Verdict::Deny => "deny",
            };
            ChatToolCapability {
                name: tool.name.clone(),
                description: tool.description.chars().take(384).collect(),
                origin: if tool.name.starts_with("mcp_") { "mcp" } else { "native" },
                risk: risk.as_str(), approval,
                argument_dependent: risk != crate::approvals::RiskClass::Read,
                available: available && approval != "deny",
                reason: if !available { Some("Current grants, connection readiness, or chat state no longer permit this tool") }
                    else if approval == "deny" { Some("Current owner policy denies this tool class") } else { None },
            }
        }).collect();
        Ok(ChatCapabilities {
            runtime: crate::routes::capabilities::runtime_observation(&state),
            agent_id: agent_id.into(),
            session_id: session_id.into(),
            basis,
            run_id,
            preparing,
            tools,
            connections: connection_views
                .into_iter()
                .map(|v| ChatConnectionCapability {
                    id: v.record.id,
                    label: v.record.label,
                    state: v.state.as_str().into(),
                })
                .collect(),
            notice: "Scope is rechecked for every call. Approval rules, argument-specific safeguards, provider credentials and service availability can still prevent execution. This view grants no permissions.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_schema_refresh_keeps_the_original_grants_and_updates_existing_agent_definitions() {
        let mut config = crate::agent_runs::test_support::companion_config("Mom");
        let mut stale = crate::tools::ToolRegistry::new()
            .descriptor("search_conversations")
            .unwrap();
        stale.description = "old search definition".into();
        stale.parameters_schema.clear();
        config.tools = Some(vec![stale]);
        let selected = select(
            &config,
            &[],
            Scope {
                owner_chat: true,
                ..Scope::default()
            },
        );
        assert_eq!(
            selected
                .iter()
                .find(|tool| tool.name == "search_conversations")
                .unwrap(),
            &crate::tools::ToolRegistry::new()
                .descriptor("search_conversations")
                .unwrap()
        );
        assert!(
            !selected
                .iter()
                .any(|tool| tool.name == "read_conversation_window"),
            "schema refresh cannot add a grant"
        );
    }

    async fn fixture() -> (super::super::AgentRunCoordinator, String) {
        let (coordinator, agent) = crate::agent_runs::test_support::coordinator_with(
            crate::agent_runs::test_support::ScriptedModel::new(vec![]),
        )
        .await;
        crate::agent_runs::test_support::add_chat(&coordinator, &agent, "chat:capabilities").await;
        (coordinator, agent)
    }

    #[tokio::test]
    async fn idle_capabilities_use_the_execution_selector_for_the_same_direct_owner_chat() {
        let (coordinator, agent) = fixture().await;
        let expected = {
            let state = coordinator.state.read().await;
            let runtime = state.agents.get(&agent).unwrap();
            select(
                runtime.config(),
                &[],
                Scope {
                    can_delegate: super::super::is_workspace_manager(&runtime.state()),
                    owner_chat: true,
                    ..Scope::default()
                },
            )
        };
        let view = coordinator
            .chat_capabilities(&agent, "chat:capabilities")
            .await
            .unwrap();
        assert_eq!(view.basis, "next_owner_turn");
        assert!(view.run_id.is_none());
        assert!(!view.preparing);
        assert_eq!(
            view.tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(view.tools.iter().all(|tool| tool.available));
    }

    #[tokio::test]
    async fn active_capability_ceiling_cannot_gain_grants_and_rechecks_revocation_policy_and_cancel(
    ) {
        let (coordinator, agent) = fixture().await;
        let registry = crate::tools::ToolRegistry::new();
        let run_id = {
            let mut state = coordinator.state.write().await;
            let mut config = state.agents.get(&agent).unwrap().config().clone();
            config.tools = Some(
                registry
                    .resolve_descriptors(["calculate", "write_file"])
                    .unwrap(),
            );
            state.restore_agent_config(&agent, config.clone());
            let record = crate::runs::RunRecord::running(
                crate::runs::RunStart {
                    agent_id: agent.clone(),
                    session_id: "chat:capabilities".into(),
                    source: crate::runs::RunSource::Api,
                    source_ref: None,
                    idempotency_key: None,
                    text: "Held owner turn".into(),
                    model: config.model.clone(),
                    provider: config.provider.clone(),
                    parent_run_id: None,
                },
                10,
            );
            let id = record.id.clone();
            state.runs.insert(record);
            state.live.runs().register(&id);
            state.live.runs().set_tool_scope(
                &id,
                registry
                    .resolve_descriptors(["calculate", "write_file"])
                    .unwrap(),
            );
            // Even a later caller cannot widen this live run's captured ceiling.
            state.live.runs().set_tool_scope(
                &id,
                registry
                    .resolve_descriptors(["calculate", "write_file", "read_file"])
                    .unwrap(),
            );
            id
        };
        let first = coordinator
            .chat_capabilities(&agent, "chat:capabilities")
            .await
            .unwrap();
        assert_eq!(first.basis, "active_turn");
        assert_eq!(first.run_id.as_deref(), Some(run_id.as_str()));
        assert!(!first.preparing);
        assert!(first.tools.iter().all(|tool| tool.available));
        {
            let mut state = coordinator.state.write().await;
            let mut config = state.agents.get(&agent).unwrap().config().clone();
            config.tools = Some(
                registry
                    .resolve_descriptors(["read_file", "write_file"])
                    .unwrap(),
            );
            state.restore_agent_config(&agent, config);
            state.approvals.set_policy(
                &agent,
                crate::approvals::ApprovalPolicy::default().with(
                    crate::approvals::RiskClass::Write,
                    crate::approvals::PolicyAction::Deny,
                ),
            );
        }
        let changed = coordinator
            .chat_capabilities(&agent, "chat:capabilities")
            .await
            .unwrap();
        assert!(!changed.tools.iter().any(|tool| tool.name == "read_file"));
        let revoked = changed
            .tools
            .iter()
            .find(|tool| tool.name == "calculate")
            .unwrap();
        assert!(!revoked.available);
        assert!(revoked.reason.is_some());
        let denied = changed
            .tools
            .iter()
            .find(|tool| tool.name == "write_file")
            .unwrap();
        assert!(!denied.available);
        assert_eq!(denied.approval, "deny");
        {
            let state = coordinator.state.read().await;
            state.live.runs().control(&run_id).unwrap().cancel.cancel();
        }
        let cancelled = coordinator
            .chat_capabilities(&agent, "chat:capabilities")
            .await
            .unwrap();
        assert_eq!(cancelled.basis, "active_turn");
        assert!(cancelled.tools.iter().all(|tool| !tool.available));
    }

    #[tokio::test]
    async fn capability_http_authorizes_before_looking_up_a_missing_conversation() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let (coordinator, _) = fixture().await;
        let app = crate::routes::router(
            coordinator.state.clone(),
            crate::app::DaemonConfig::default(),
        );
        for (host, origin) in [
            ("127.0.0.1:8080", "https://untrusted.example"),
            ("192.0.2.10:8080", "http://localhost:4200"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/agents/missing/sessions/chat:missing/capabilities")
                        .header("host", host)
                        .header("origin", origin)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    }

    #[test]
    fn selection_drops_disconnected_mcp_and_cannot_smuggle_owner_controls() {
        let registry = crate::tools::ToolRegistry::new();
        let mut config = crate::agent_runs::test_support::companion_config("scope");
        let mut external = registry.descriptor("read_file").unwrap();
        external.name = "mcp_disconnected".into();
        config.tools = Some(vec![
            external.clone(),
            registry.descriptor("clear_main_chat").unwrap(),
            registry.descriptor("rename_chat").unwrap(),
        ]);
        let scope = Scope::default();
        assert!(!select(&config, &[], scope)
            .iter()
            .any(|t| t.name.starts_with("mcp_")
                || t.name == "clear_main_chat"
                || t.name == "rename_chat"));
        assert!(select(&config, &[external], scope)
            .iter()
            .any(|t| t.name == "mcp_disconnected"));
        assert!(select(
            &config,
            &[],
            Scope {
                owner_chat: true,
                ..scope
            }
        )
        .iter()
        .any(|t| t.name == "rename_chat"));
    }

    #[test]
    fn reminders_have_no_tools_even_when_owner_controls_are_configured() {
        let config = crate::agent_runs::test_support::companion_config("scope");
        assert!(select(
            &config,
            &[],
            Scope {
                reminder_verification: true,
                ..Scope::default()
            }
        )
        .is_empty());
    }
}
