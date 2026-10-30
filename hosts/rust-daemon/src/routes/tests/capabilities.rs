use super::*;

#[tokio::test]
async fn m83_capabilities_explain_anonymous_search_without_granting_it() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let before = serde_json::to_value(state.read().await.control_plane_snapshot()).unwrap();
    let app = router(state.clone(), DaemonConfig::default());
    let response = app
        .oneshot(
            crate::routes::owner_test_support::authenticate(
                Request::builder()
                    .uri("/api/capabilities")
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let search = value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "web_search")
        .expect("native anonymous search inventory");
    let requirements = search["requirements"].to_string().to_lowercase();
    for word in ["anonymous", "exa", "rate", "grant"] {
        assert!(requirements.contains(word), "{requirements}");
    }
    assert_eq!(
        serde_json::to_value(state.read().await.control_plane_snapshot()).unwrap(),
        before
    );
}

#[tokio::test]
async fn capability_inventory_matches_registered_tools_without_granting_access() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let expected = state.read().await.tool_registry.tool_names();
    let app = router(state.clone(), DaemonConfig::default());
    let response = app
        .oneshot(
            crate::routes::owner_test_support::authenticate(
                Request::builder()
                    .uri("/api/capabilities")
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let actual: Vec<_> = value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["runtime"]["images"]["ledgerVersion"], 1);
    assert_eq!(value["runtime"]["pdfPages"]["version"], 1);
    assert!(value["runtime"]["observedAtMs"].as_u64().unwrap() > 0);
    assert_eq!(value["persistence"]["controlPlane"], "ephemeral");
    assert_eq!(value["persistence"]["memory"], "ephemeral");
    assert_eq!(value["persistence"]["executionJournal"], false);
    for id in ["camera", "smart-home", "coding-agents"] {
        let extension = value["extensions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .unwrap();
        assert_eq!(extension["status"], "planned", "{id}");
    }
    assert!(state.read().await.list_agents().is_empty());
    let search = value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "exa_search")
        .unwrap();
    assert!(!search["requirements"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn m75_capability_inventory_reports_supported_partial_slices_without_activation() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state.clone(), DaemonConfig::default());
    let before = {
        let state = state.read().await;
        assert!(state.list_agents().is_empty());
        serde_json::to_value(state.control_plane_snapshot()).unwrap()
    };
    assert_eq!(before["runs"], serde_json::json!([]));
    let response = app
        .oneshot(
            crate::routes::owner_test_support::authenticate(
                Request::builder()
                    .uri("/api/capabilities")
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["runtime"]["images"]["ledgerVersion"], 1);
    assert_eq!(value["runtime"]["pdfPages"]["version"], 1);
    assert!(value["runtime"]["observedAtMs"].as_u64().unwrap() > 0);
    let extensions = value["extensions"].as_array().unwrap();
    for (id, expected_status) in [
        ("voice", "partial"),
        ("document-engine", "partial"),
        ("community-bundles", "partial"),
        ("camera", "planned"),
        ("smart-home", "planned"),
        ("coding-agents", "planned"),
    ] {
        let matching: Vec<_> = extensions.iter().filter(|item| item["id"] == id).collect();
        assert_eq!(matching.len(), 1, "{id}");
        assert_eq!(matching[0]["status"], expected_status, "{id}");
        let description = matching[0]["description"].as_str().unwrap();
        assert!(!description.is_empty() && description.len() <= 512, "{id}");
    }
    assert_eq!(extensions.len(), 6);
    let tools = value["tools"].as_array().unwrap();
    let document = tools
        .iter()
        .find(|tool| tool["name"] == "read_document")
        .unwrap();
    assert_eq!(document["category"], "documents");
    let requirements = document["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    for prerequisite in ["workspace", "document", "opt-in", "grant"] {
        assert!(
            requirements.contains(prerequisite),
            "missing {prerequisite}: {requirements}"
        );
    }
    for name in [
        "browser_navigate",
        "browser_observe",
        "browser_act",
        "browser_transfer",
    ] {
        let browser = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(browser["category"], "browser", "{name}");
        assert!(!browser["requirements"].as_array().unwrap().is_empty());
    }
    let after = {
        let state = state.read().await;
        assert!(state.list_agents().is_empty());
        serde_json::to_value(state.control_plane_snapshot()).unwrap()
    };
    assert_eq!(after["runs"], serde_json::json!([]));
    assert_eq!(
        after, before,
        "Discovery must not mutate grants or persisted runtime state"
    );
}

#[tokio::test]
async fn capability_inventory_rejects_untrusted_origins() {
    let app = router(
        Arc::new(RwLock::new(DaemonState::new())),
        DaemonConfig::default(),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/capabilities")
                .header("host", "127.0.0.1:8080")
                .header("origin", "https://untrusted.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
