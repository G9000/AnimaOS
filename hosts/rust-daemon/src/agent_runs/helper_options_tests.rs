use super::*;

fn report_schema() -> DataValue {
    DataValue::Object(BTreeMap::from([
        ("type".into(), DataValue::String("object".into())),
        (
            "properties".into(),
            DataValue::Object(BTreeMap::from([
                (
                    "summary".into(),
                    DataValue::Object(BTreeMap::from([(
                        "type".into(),
                        DataValue::String("string".into()),
                    )])),
                ),
                (
                    "count".into(),
                    DataValue::Object(BTreeMap::from([(
                        "type".into(),
                        DataValue::String("integer".into()),
                    )])),
                ),
            ])),
        ),
        (
            "required".into(),
            DataValue::Array(vec![
                DataValue::String("summary".into()),
                DataValue::String("count".into()),
            ]),
        ),
        ("additionalProperties".into(), DataValue::Bool(false)),
    ]))
}

fn bounded_report_call(id: &str) -> ToolCall {
    let mut call = launch(id, "Return the checked report");
    call.args.insert("output_schema".into(), report_schema());
    call.args.insert(
        "limits".into(),
        DataValue::Object(BTreeMap::from([
            ("max_tool_turns".into(), DataValue::Number(3.0)),
            ("timeout_ms".into(), DataValue::Number(20_000.0)),
            ("max_output_tokens".into(), DataValue::Number(900.0)),
        ])),
    );
    call
}

async fn grant_native_options_tools(h: &mut Harness) {
    let registry = ToolRegistry::new();
    h.parent
        .config
        .tools
        .as_mut()
        .unwrap()
        .extend(["spawn_helper", "wait_helper_runs"].map(|name| {
            registry
                .descriptor(name)
                .expect("native helper options tool")
        }));
    h.coordinator
        .state
        .write()
        .await
        .restore_agent_config(&h.parent.id, h.parent.config.clone());
}

async fn configure_research_route(h: &mut Harness) {
    h.parent.config.settings.as_mut().unwrap().delegation =
        Some(anima_core::delegation::DelegationPolicy {
            max_concurrent: 2,
            max_starts_per_run: 2,
            max_tool_iterations: 16,
            timeout_ms: 240_000,
            max_output_tokens: 8192,
            routes: vec![anima_core::delegation::DelegationRoute {
                id: "research".into(),
                provider: "openai".into(),
                model: "accepted-research-model".into(),
                fallbacks: vec![anima_core::ModelChoice {
                    provider: "openai".into(),
                    model: "accepted-fallback-model".into(),
                }],
            }],
            ..Default::default()
        });
    h.coordinator
        .state
        .write()
        .await
        .restore_agent_config(&h.parent.id, h.parent.config.clone());
}

fn collect_report(id: &str) -> ToolCall {
    ToolCall {
        id: "collect-report".into(),
        name: "wait_helper_runs".into(),
        args: BTreeMap::from([
            (
                "run_ids".into(),
                DataValue::Array(vec![DataValue::String(id.into())]),
            ),
            ("mode".into(), DataValue::String("all".into())),
            ("timeout_ms".into(), DataValue::Number(0.0)),
        ]),
    }
}

#[tokio::test]
async fn delegation_route_and_limits_are_durable_and_bound_to_retry() {
    let mut h = Harness::new(0).await;
    h.parent.config.settings.as_mut().unwrap().delegation =
        Some(anima_core::delegation::DelegationPolicy {
            max_concurrent: 2,
            max_starts_per_run: 2,
            max_tool_iterations: 16,
            timeout_ms: 240_000,
            max_output_tokens: 8192,
            routes: vec![anima_core::delegation::DelegationRoute {
                id: "research".into(),
                provider: "openai".into(),
                model: "research-model".into(),
                fallbacks: vec![anima_core::ModelChoice {
                    provider: "anthropic".into(),
                    model: "fallback-model".into(),
                }],
            }],
            ..Default::default()
        });
    h.coordinator
        .state
        .write()
        .await
        .restore_agent_config(&h.parent.id, h.parent.config.clone());
    let mut call = launch("routed", "Research the evidence");
    call.args
        .insert("route".into(), DataValue::String("research".into()));
    call.args.insert(
        "limits".into(),
        DataValue::Object(BTreeMap::from([
            ("max_tool_turns".into(), DataValue::Number(12.0)),
            ("timeout_ms".into(), DataValue::Number(200_000.0)),
            ("max_output_tokens".into(), DataValue::Number(6000.0)),
        ])),
    );
    let first = success_json(h.call(call.clone()).await);
    assert_eq!(first, success_json(h.call(call.clone()).await));
    let snapshot = h.coordinator.state.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(
            serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap(),
        )
        .unwrap();
    let run = restored.runs.get(first["runId"].as_str().unwrap()).unwrap();
    assert_eq!(run.model, "research-model");
    let config = &restored.get_agent(&run.agent_id).unwrap().state.config;
    assert_eq!(config.fallbacks[0].model, "fallback-model");
    let s = config.settings.as_ref().unwrap();
    assert_eq!(s.max_tool_iterations, Some(12));
    assert_eq!(s.timeout_ms, Some(200_000));
    assert_eq!(s.max_tokens, Some(6000));
    call.args
        .insert("route".into(), DataValue::String("other".into()));
    assert_eq!(h.call(call).await.status, anima_core::TaskStatus::Error);
    assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 1);
    h.coordinator.request_shutdown();
}

#[tokio::test]
async fn delegation_invalid_options_cannot_consume_starts_or_create_helpers() {
    let h = Harness::new(0).await;
    for (key, value) in [
        ("route", DataValue::String("missing".into())),
        (
            "limits",
            DataValue::Object(BTreeMap::from([(
                "max_tool_turns".into(),
                DataValue::Number(0.0),
            )])),
        ),
        (
            "output_schema",
            DataValue::Object(BTreeMap::from([(
                "$ref".into(),
                DataValue::String("https://example.invalid/schema".into()),
            )])),
        ),
    ] {
        let mut call = launch("invalid-options", "Inspect");
        call.args.insert(key.into(), value);
        assert_eq!(h.call(call).await.status, anima_core::TaskStatus::Error);
        assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 0);
        assert_eq!(h.coordinator.state.read().await.agent_states().len(), 1);
    }
    h.coordinator.request_shutdown();
}

#[tokio::test]
async fn delegation_schema_validation_reports_invalid_output_without_replay() {
    let h = Harness::new(1).await;
    let mut call = launch("schema-output", "Return a report");
    call.args.insert(
        "output_schema".into(),
        DataValue::Object(BTreeMap::from([
            ("type".into(), DataValue::String("object".into())),
            (
                "required".into(),
                DataValue::Array(vec![DataValue::String("summary".into())]),
            ),
        ])),
    );
    let handle = success_json(h.call(call).await);
    wait_for(
        &h.coordinator,
        handle["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    let record = h
        .coordinator
        .state
        .read()
        .await
        .runs
        .get(handle["runId"].as_str().unwrap())
        .unwrap()
        .clone();
    let raw = h.coordinator.committed_helper_text(&record).await.unwrap();
    let validation = crate::agent_runs::helper_options::result_validation(
        record.async_helper.as_ref().unwrap().options.as_ref(),
        &raw,
    )
    .unwrap();
    assert_eq!(validation["valid"], false);
    assert_eq!(h.model.requests().len(), 1);
    assert_eq!(raw, "recorded child result");
    assert!(h.model.requests()[0]
        .messages
        .iter()
        .any(|m| m.content.text.contains("JSON output contract")));
}

#[tokio::test]
async fn delegation_valid_json_is_annotated_by_native_list_and_collector_without_rerun() {
    const RAW: &str = r#"{"summary":"Checked evidence","count":2}"#;
    let mut h = Harness::with_model(1, ScriptedModel::new(vec![Step::Text(vec![RAW])])).await;
    grant_native_options_tools(&mut h).await;
    let handle = success_json(h.call(bounded_report_call("valid-native-report")).await);
    let id = handle["runId"].as_str().unwrap();
    wait_for(&h.coordinator, id, RunStatus::Completed).await;
    drained(&h, &handle).await;

    let listed = success_json(
        h.call(management(
            "list-valid-report",
            "list_helper_runs",
            Some(id),
            None,
        ))
        .await,
    );
    assert_eq!(listed["runs"][0]["runId"], id);
    assert_eq!(listed["runs"][0]["resultText"], RAW);
    assert_eq!(listed["runs"][0]["outputValidation"]["valid"], true);

    let collected = success_json(h.call(collect_report(id)).await);
    assert_eq!(collected["conditionMet"], true);
    assert_eq!(collected["readyRunIds"], serde_json::json!([id]));
    assert_eq!(collected["runs"][0]["resultText"], RAW);
    assert_eq!(collected["runs"][0]["outputValidation"]["valid"], true);
    assert_eq!(h.model.requests().len(), 1);
}

#[tokio::test]
async fn delegation_retry_rejects_changed_limits_and_output_contract_without_new_starts() {
    let h = Harness::new(0).await;
    let original = bounded_report_call("immutable-options");
    let handle = success_json(h.call(original.clone()).await);

    let mut changed_limits = original.clone();
    let Some(DataValue::Object(limits)) = changed_limits.args.get_mut("limits") else {
        unreachable!()
    };
    limits.insert("max_output_tokens".into(), DataValue::Number(901.0));
    let mut changed_schema = original.clone();
    let Some(DataValue::Object(schema)) = changed_schema.args.get_mut("output_schema") else {
        unreachable!()
    };
    schema.insert(
        "required".into(),
        DataValue::Array(vec![DataValue::String("summary".into())]),
    );
    for changed in [changed_limits, changed_schema] {
        let refused = h.call(changed).await;
        assert_eq!(refused.status, anima_core::TaskStatus::Error);
        assert!(refused
            .error
            .unwrap()
            .contains("different helper launch arguments"));
    }
    assert_eq!(success_json(h.call(original).await), handle);
    assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 1);
    assert_eq!(
        h.coordinator
            .state
            .read()
            .await
            .runs
            .async_helpers_for_owner(&h.parent.id)
            .len(),
        1,
    );
    assert!(h.model.requests().is_empty());
    h.coordinator.request_shutdown();
}

#[tokio::test]
async fn delegation_saved_route_edit_does_not_rewrite_an_accepted_assignment_or_replay() {
    let mut h = Harness::new(0).await;
    configure_research_route(&mut h).await;
    let mut call = launch("frozen-route", "Research with the accepted route");
    call.args
        .insert("route".into(), DataValue::String("research".into()));
    call.args.insert(
        "limits".into(),
        DataValue::Object(BTreeMap::from([
            ("max_tool_turns".into(), DataValue::Number(12.0)),
            ("timeout_ms".into(), DataValue::Number(200_000.0)),
            ("max_output_tokens".into(), DataValue::Number(6000.0)),
        ])),
    );
    let accepted = success_json(h.call(call.clone()).await);
    assert_eq!(accepted["bounds"]["maxToolTurns"], 12);
    assert_eq!(accepted["bounds"]["maxDurationMs"], 200_000);
    assert_eq!(accepted["bounds"]["maxOutputTokensPerResponse"], 6000);

    let route = &mut h
        .parent
        .config
        .settings
        .as_mut()
        .unwrap()
        .delegation
        .as_mut()
        .unwrap()
        .routes[0];
    route.model = "edited-research-model".into();
    route.fallbacks[0].model = "edited-fallback-model".into();
    h.coordinator
        .state
        .write()
        .await
        .restore_agent_config(&h.parent.id, h.parent.config.clone());
    assert_eq!(success_json(h.call(call.clone()).await), accepted);
    assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 1);

    h.permits.add_permits(1);
    wait_for(
        &h.coordinator,
        accepted["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    drained(&h, &accepted).await;
    let accepted_config = &h.model.configs()[0];
    assert_eq!(accepted_config.model, "accepted-research-model");
    assert_eq!(
        accepted_config.fallbacks[0].model,
        "accepted-fallback-model"
    );
    let settings = accepted_config.settings.as_ref().unwrap();
    assert_eq!(settings.max_tool_iterations, Some(12));
    assert_eq!(settings.timeout_ms, Some(200_000));
    assert_eq!(settings.max_tokens, Some(6000));

    call.id = "fresh-after-route-edit".into();
    let fresh = success_json(h.call(call).await);
    wait_for(
        &h.coordinator,
        fresh["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    drained(&h, &fresh).await;
    assert_eq!(h.model.configs()[1].model, "edited-research-model");
    assert_eq!(
        h.model.configs()[1].fallbacks[0].model,
        "edited-fallback-model"
    );
    assert_ne!(fresh["runId"], accepted["runId"]);
    assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 2);
}

#[tokio::test]
async fn delegation_sync_native_helper_applies_named_route_limits_and_validates_json() {
    const RAW: &str = r#"{"summary":"Sync checked evidence","count":3}"#;
    let mut h = Harness::with_model(1, ScriptedModel::new(vec![Step::Text(vec![RAW])])).await;
    grant_native_options_tools(&mut h).await;
    configure_research_route(&mut h).await;
    let mut call = bounded_report_call("sync-native-report");
    call.name = "spawn_helper".into();
    call.args.remove("completion");
    call.args
        .insert("route".into(), DataValue::String("research".into()));
    let result = success_json(h.call(call).await);
    assert_eq!(result["result"]["text"], RAW);
    assert_eq!(result["outputValidation"]["valid"], true);
    assert_eq!(h.model.requests().len(), 1);
    let config = &h.model.configs()[0];
    assert_eq!(config.model, "accepted-research-model");
    assert_eq!(config.provider.as_deref(), Some("openai"));
    assert_eq!(config.fallbacks[0].model, "accepted-fallback-model");
    let settings = config.settings.as_ref().unwrap();
    assert_eq!(settings.max_tool_iterations, Some(3));
    assert_eq!(settings.timeout_ms, Some(20_000));
    assert_eq!(settings.max_tokens, Some(900));
    assert!(h.model.requests()[0]
        .messages
        .iter()
        .any(|message| message.content.text.contains("JSON output contract")));
}

#[tokio::test]
async fn delegation_sync_native_invalid_json_is_annotated_without_rerunning() {
    const RAW: &str = "This is a report, but it is not JSON";
    let mut h = Harness::with_model(1, ScriptedModel::new(vec![Step::Text(vec![RAW])])).await;
    grant_native_options_tools(&mut h).await;
    let mut call = bounded_report_call("sync-invalid-report");
    call.name = "spawn_helper".into();
    call.args.remove("completion");
    let result = success_json(h.call(call).await);
    assert_eq!(result["result"]["text"], RAW);
    assert_eq!(result["outputValidation"]["valid"], false);
    assert_eq!(
        result["outputValidation"]["error"],
        "Result is not valid JSON"
    );
    assert_eq!(h.model.requests().len(), 1);
}

#[tokio::test]
async fn delegation_native_invalid_names_never_consume_starts_or_create_helpers() {
    let mut h = Harness::new(0).await;
    grant_native_options_tools(&mut h).await;
    for operation in ["spawn_helper", "spawn_helper_async"] {
        for (index, name) in [
            DataValue::String(String::new()),
            DataValue::String(" \t\n ".into()),
            DataValue::String("x".repeat(81)),
            DataValue::Bool(true),
        ]
        .into_iter()
        .enumerate()
        {
            let mut call = launch(&format!("invalid-name-{operation}-{index}"), "Inspect");
            call.name = operation.into();
            call.args.insert("name".into(), name);
            let result = h.call(call).await;
            assert_eq!(result.status, anima_core::TaskStatus::Error);
            assert!(result.error.unwrap().contains("name"));
            assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 0);
            assert_eq!(h.coordinator.state.read().await.agent_states().len(), 1);
        }
    }
    assert!(h.model.requests().is_empty());
    assert!(h
        .coordinator
        .state
        .read()
        .await
        .runs
        .async_helpers_for_owner(&h.parent.id)
        .is_empty());
    h.coordinator.request_shutdown();
}

#[tokio::test]
async fn delegation_disk_restore_keeps_route_policy_and_immutable_assignment() {
    let mut h = Harness::new(0).await;
    let directory = tempfile::tempdir().unwrap();
    let store = crate::control_plane_store::ControlPlaneStoreConfig::Json(
        directory.path().join("control-plane.json"),
    );
    let policy = anima_core::delegation::DelegationPolicy {
        max_concurrent: 2,
        routes: vec![anima_core::delegation::DelegationRoute {
            id: "research".into(),
            provider: "openai".into(),
            model: "disk-model".into(),
            fallbacks: vec![anima_core::ModelChoice {
                provider: "anthropic".into(),
                model: "disk-fallback".into(),
            }],
        }],
        ..Default::default()
    };
    h.parent.config.settings.as_mut().unwrap().delegation = Some(policy.clone());
    {
        let mut state = h.coordinator.state.write().await;
        state.restore_agent_config(&h.parent.id, h.parent.config.clone());
        state.set_control_plane_store(Some(store.clone()));
    }
    let mut call = launch("disk-route", "Inspect saved project evidence");
    call.args
        .insert("route".into(), DataValue::String("research".into()));
    let handle = success_json(h.call(call).await);
    h.coordinator.request_shutdown();
    let loaded = crate::control_plane_store::load_control_plane_snapshot(&store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded.version,
        crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
    );
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(loaded).unwrap();
    assert_eq!(
        restored
            .get_agent(&h.parent.id)
            .unwrap()
            .state
            .config
            .settings
            .unwrap()
            .delegation,
        Some(policy)
    );
    let child = restored
        .runs
        .get(handle["runId"].as_str().unwrap())
        .unwrap();
    assert_eq!(child.model, "disk-model");
    assert_eq!(
        child
            .async_helper
            .as_ref()
            .unwrap()
            .options
            .as_ref()
            .unwrap()
            .route
            .as_deref(),
        Some("research")
    );
    assert_eq!(
        restored
            .get_agent(&child.agent_id)
            .unwrap()
            .state
            .config
            .fallbacks[0]
            .model,
        "disk-fallback"
    );
    assert!(h.model.requests().is_empty());
}

#[tokio::test]
async fn delegation_v29_upgrade_backup_preserves_the_previous_store_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("control-plane.json");
    let original = b"{\n  \"version\":29,\n  \"agents\":[],\n  \"swarms\":[]\n}\n";
    std::fs::write(&path, original).unwrap();
    let store = crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone());
    let backup = crate::control_plane_store::write_pre_upgrade_backup(&store, 29)
        .await
        .unwrap();
    assert!(backup.ends_with(".pre-delegation-controls.bak"));
    assert_eq!(std::fs::read(backup).unwrap(), original);
    assert_eq!(std::fs::read(path).unwrap(), original);
}
