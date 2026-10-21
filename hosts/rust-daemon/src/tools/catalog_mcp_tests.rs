//! Captured discovery stamps cannot be refreshed into new dispatch authority.
use super::*;
use crate::tools::catalog::RunToolCatalog;

fn catalog(f: &Fixture) -> Arc<RunToolCatalog> {
    Arc::new(RunToolCatalog::new(
        &f.state,
        f.run.clone(),
        f.cancel.clone(),
        &f.agent.config,
        &f.agent,
        Some(f.manager.clone()),
    ))
}
async fn refuse(f: &Fixture, catalog: Arc<RunToolCatalog>) {
    assert!(catalog.describe(&f.agent, &f.run, &f.alias).await.is_err());
    let result = f
        .context
        .clone()
        .with_tool_catalog(Some(catalog))
        .execute_tool(
            f.agent.clone(),
            tool_input(&f.agent.id, &f.run.session_id),
            f.call("catalog-stale"),
        )
        .await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(
        f.factory.calls.load(Ordering::SeqCst),
        0,
        "catalog staleness must refuse before transport"
    );
    f.manager.shutdown().await;
}
#[tokio::test]
async fn tool_discovery_mcp_revoke_and_regrant_cannot_refresh_catalog_epoch() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let catalog = catalog(&f);
    catalog.describe(&f.agent, &f.run, &f.alias).await.unwrap();
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    f.manager
        .set_grant(
            &f.agent.id,
            &f.connection_id,
            &f.alias,
            McpGrantInput {
                expected_version: current.record.version,
                descriptor_digest: f.descriptor_digest.clone(),
                granted: false,
            },
        )
        .await
        .unwrap();
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    f.manager
        .set_grant(
            &f.agent.id,
            &f.connection_id,
            &f.alias,
            McpGrantInput {
                expected_version: current.record.version,
                descriptor_digest: f.descriptor_digest.clone(),
                granted: true,
            },
        )
        .await
        .unwrap();
    refuse(&f, catalog).await;
}
#[tokio::test]
async fn tool_discovery_mcp_reconnect_cannot_refresh_catalog_generation() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let catalog = catalog(&f);
    catalog.describe(&f.agent, &f.run, &f.alias).await.unwrap();
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    f.manager
        .disconnect(&f.agent.id, &f.connection_id, current.record.version)
        .await
        .unwrap();
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    f.manager
        .connect(&f.agent.id, &f.connection_id, current.record.version)
        .await
        .unwrap();
    refuse(&f, catalog).await;
}
#[tokio::test]
async fn tool_discovery_mcp_replaced_review_version_cannot_refresh_catalog() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let catalog = catalog(&f);
    catalog.describe(&f.agent, &f.run, &f.alias).await.unwrap();
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    f.manager
        .update(
            &f.agent.id,
            &f.connection_id,
            UpdateMcpConnection {
                expected_version: current.record.version,
                label: Some("new reviewed connection generation".into()),
                endpoint: None,
                enabled: None,
            },
        )
        .await
        .unwrap();
    refuse(&f, catalog).await;
}

#[tokio::test]
async fn tool_discovery_mcp_replaced_schema_cannot_refresh_catalog_descriptor() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let catalog = catalog(&f);
    catalog.describe(&f.agent, &f.run, &f.alias).await.unwrap();
    let replacement=prepare_external_tool("echo_record","Replaced reviewed schema",json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false}),None).unwrap();
    *f.factory.discovery_descriptor.lock().unwrap() = Some(replacement);
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let discovered = f
        .manager
        .discover(&f.agent.id, &f.connection_id, current.record.version)
        .await
        .unwrap();
    f.manager
        .review(
            &f.agent.id,
            &f.connection_id,
            ReviewMcpTool {
                expected_version: discovered.record.version,
                discovery_id: discovered.discovery_id.unwrap(),
                descriptor_digest: discovered.candidates[0].descriptor_digest.clone(),
            },
        )
        .await
        .unwrap();
    refuse(&f, catalog).await;
}

#[tokio::test]
async fn tool_discovery_visibility_drops_mcp_schema_after_grant_epoch_changes() {
    use anima_core::ModelToolSelector;
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let catalog = catalog(&f);
    catalog.describe(&f.agent, &f.run, &f.alias).await.unwrap();
    assert!(catalog.visible_tool_names().contains(&f.alias));
    f.manager.note_agent_permissions_changed(&f.agent.id);
    assert!(!catalog.visible_tool_names().contains(&f.alias));
    f.manager.shutdown().await;
}

async fn provider_capture_mcp(count: usize, discovery: bool) {
    use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};
    let f = if discovery {
        let mut properties = serde_json::Map::new();
        properties.insert("value".into(), json!({"type":"string"}));
        for index in 0..48 {
            properties.insert(
                format!("optional_{index}"),
                json!({"type":"string","description":"Exact reviewed schema detail ".repeat(8)}),
            );
        }
        let large = prepare_external_tool("echo_record","Read one synthetic record using the exact large reviewed schema",json!({"type":"object","properties":properties,"required":["value"],"additionalProperties":false}),None).unwrap();
        let factory = Arc::new(FakeFactory {
            discovery_descriptor: Arc::new(StdMutex::new(Some(large))),
            ..Default::default()
        });
        Fixture::with_factory(PolicyAction::Allow, factory, None, |_| {}).await
    } else {
        Fixture::new(PolicyAction::Allow, false, false).await
    };
    for index in 1..count {
        let created = f
            .manager
            .create(
                &f.agent.id,
                CreateMcpConnection {
                    label: format!("reviewed capture {index}"),
                    endpoint: "http://127.0.0.1:32123/mcp".into(),
                },
            )
            .await
            .unwrap();
        let enabled = f
            .manager
            .update(
                &f.agent.id,
                &created.record.id,
                UpdateMcpConnection {
                    expected_version: created.record.version,
                    label: None,
                    endpoint: None,
                    enabled: Some(true),
                },
            )
            .await
            .unwrap();
        let connected = f
            .manager
            .connect(&f.agent.id, &created.record.id, enabled.record.version)
            .await
            .unwrap();
        let reviewed = f
            .manager
            .review(
                &f.agent.id,
                &created.record.id,
                ReviewMcpTool {
                    expected_version: connected.record.version,
                    discovery_id: connected.discovery_id.unwrap(),
                    descriptor_digest: connected.candidates[0].descriptor_digest.clone(),
                },
            )
            .await
            .unwrap();
        let tool = &reviewed.record.reviewed_tools[0];
        f.manager
            .set_grant(
                &f.agent.id,
                &created.record.id,
                &tool.alias,
                McpGrantInput {
                    expected_version: reviewed.record.version,
                    descriptor_digest: tool.descriptor.descriptor_digest.clone(),
                    granted: true,
                },
            )
            .await
            .unwrap();
    }
    let mut steps = vec![];
    if discovery {
        steps.push(Step::Tools(vec![ToolCall {
            id: "discover".into(),
            name: "search_tools".into(),
            args: [
                ("query".into(), DataValue::String(f.alias.clone())),
                ("limit".into(), DataValue::Number(1.0)),
            ]
            .into(),
        }]));
    }
    steps.push(Step::Tools(vec![f.call("native-mcp")]));
    steps.push(Step::Text(vec!["settled MCP result"]));
    let model = ScriptedModel::new(steps);
    let original = {
        let mut guard = f.state.write().await;
        guard.model_adapter = model.clone();
        let mut config = guard.get_agent(&f.agent.id).unwrap().state.config;
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("toolDiscovery".into(), DataValue::Bool(discovery));
        guard.restore_agent_config(&f.agent.id, config.clone());
        config
    };
    let result = f
        .coordinator
        .run(chat_request(
            &f.agent.id,
            "chat:mcp-provider-capture",
            "read the reviewed record",
        ))
        .await
        .unwrap();
    assert_eq!(result.result.status, "success");
    let configs = model.configs();
    let exact = original
        .tools
        .as_ref()
        .unwrap()
        .iter()
        .find(|t| t.name == f.alias)
        .unwrap();
    let size = |config: &anima_core::AgentConfig| serde_json::to_vec(&config.tools).unwrap().len();
    if discovery {
        assert!(configs[0].allows_tool("search_tools"));
        assert!(!configs[0]
            .tools
            .iter()
            .flatten()
            .any(|t| t.name.starts_with("mcp_")));
        assert!(configs[1].tools.as_ref().unwrap().contains(exact));
        assert!(size(&configs[0]) * 3 < size(&original));
        eprintln!(
            "reviewed MCP schema bytes: full={}, compact={}, hydrated={}",
            size(&original),
            size(&configs[0]),
            size(&configs[1])
        );
    } else {
        assert!(!configs[0].allows_tool("search_tools"));
        assert!(configs[0].tools.as_ref().unwrap().contains(exact));
    }
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 1);
    let guard = f.state.read().await;
    assert_eq!(guard.get_agent(&f.agent.id).unwrap().state.config, original);
    let record = guard
        .runs
        .for_agent(&f.agent.id)
        .into_iter()
        .find(|r| r.session_id == "chat:mcp-provider-capture")
        .unwrap();
    assert_eq!(record.action_receipts.len(), if discovery { 2 } else { 1 });
    assert!(record
        .action_receipts
        .iter()
        .all(|r| r.status == ActionStatus::Succeeded
            && r.durability == crate::runs::ActionDurability::Persistent));
    eprintln!(
        "reviewed MCP {} capture: configured_tools_bytes={}, first_provider_tools_bytes={}, target_provider_tools_bytes={}, discovery_calls={}, transport_calls={}",
        if discovery { "large" } else { "small" },
        size(&original),
        size(&configs[0]),
        size(&configs[if discovery { 1 } else { 0 }]),
        record.action_receipts.iter().filter(|receipt| receipt.name == "search_tools").count(),
        f.factory.calls.load(Ordering::SeqCst),
    );
    let store = guard.control_plane_store.clone().unwrap();
    let run_id = record.id.clone();
    drop(guard);
    let snapshot = crate::control_plane_store::load_control_plane_snapshot(&store)
        .await
        .unwrap()
        .unwrap();
    assert!(snapshot.runs.iter().any(|r| r.id == run_id
        && r.action_receipts
            .iter()
            .any(|a| a.name == f.alias && a.status == ActionStatus::Succeeded)));
    f.manager.shutdown().await;
}
#[tokio::test]
async fn tool_discovery_small_reviewed_mcp_legacy_provider_request_and_durable_effect() {
    provider_capture_mcp(1, false).await;
}
#[tokio::test]
async fn tool_discovery_large_reviewed_mcp_provider_request_hydrates_exact_schema_and_durable_effect(
) {
    provider_capture_mcp(4, true).await;
}
