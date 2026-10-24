use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::util::ServiceExt;

const OWNER_ORIGIN: &str = "http://localhost:4200";

async fn owner_json(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, String, Value) {
    owner_json_from(app, OWNER_ORIGIN, method, uri, body).await
}

async fn owner_json_from(
    app: &Router,
    origin: &str,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, String, Value) {
    let mut request = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:8080")
            .header("origin", origin),
    );
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .expect("owner request builds"),
        )
        .await
        .expect("daemon responds");
    let status = response.status();
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body reads");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, cache_control, value)
}

async fn create_ordinary_agent(app: &Router, name: &str) -> String {
    let (status, _, response) = owner_json(
        app,
        "POST",
        "/api/agents",
        Some(json!({"name":name,"model":"gpt-5.4"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    response["agent"]["state"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

struct SyntheticSnapshot(std::path::PathBuf);
impl Drop for SyntheticSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn daemon_app() -> (Router, SyntheticSnapshot) {
    let path =
        std::env::temp_dir().join(format!("anima-mcp-owner-api-{}.json", uuid::Uuid::new_v4()));
    let mut daemon = crate::state::DaemonState::new();
    daemon.control_plane_store = Some(crate::control_plane_store::ControlPlaneStoreConfig::Json(
        path.clone(),
    ));
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(daemon));
    (
        crate::app::app_with_state_for_owner_test(state, crate::DaemonConfig::default()),
        SyntheticSnapshot(path),
    )
}

#[tokio::test]
async fn m45_owner_bundle_review_saves_no_connection_skill_or_authority() {
    let (app, snapshot) = daemon_app();
    let workspace = tempfile::tempdir().unwrap();
    let (workspace_status, _, configured) = owner_json(
        &app,
        "PUT",
        "/api/workspace",
        Some(json!({
            "rootPath": workspace.path().to_str().unwrap(),
            "companyName": "M45 synthetic workspace",
            "mission": "Review declarative content without authority",
            "values": []
        })),
    )
    .await;
    assert_eq!(workspace_status, StatusCode::OK);
    assert_eq!(configured["configured"], true);
    let agent_id = create_ordinary_agent(&app, "M45 ordinary bundle owner").await;
    let agent_path = format!("/api/agents/{agent_id}");
    let (before_status, _, before_agent) = owner_json(&app, "GET", &agent_path, None).await;
    assert_eq!(before_status, StatusCode::OK);
    let manifest = json!({
        "schemaVersion": 1,
        "id": "example.synthetic-violet",
        "version": "1.0.0",
        "name": "Synthetic violet procedure",
        "description": "A declarative inline skill review fixture",
        "compatibility": {
            "host": "rust-daemon",
            "minVersion": "0.1.0",
            "maxVersionExclusive": "0.2.0",
            "requiredCapabilities": ["skills.import-draft.v1"]
        },
        "permissions": {"nativeToolNames": [], "externalTools": []},
        "attribution": [{
            "component": "violet-procedure",
            "source": "repository-local synthetic fixture",
            "revision": "synthetic-m45",
            "license": "repository-license",
            "notice": "Synthetic declarative review only"
        }],
        "components": [{
            "key": "violet-procedure",
            "kind": "skill",
            "skillFile": "---\nname: Violet procedure\ndescription: Inspect synthetic reviewed evidence\n---\n\nUse only existing granted tools and quote the returned provenance.\n"
        }]
    });
    let (review_status, review_cache, _) = owner_json(
        &app,
        "POST",
        &format!("/api/agents/{agent_id}/community-bundles/reviews"),
        Some(json!({
            "manifestText": manifest.to_string(),
            "programBindings": {}
        })),
    )
    .await;
    assert_eq!(
        review_status,
        StatusCode::OK,
        "an owner can review inline skill content without installing or granting it"
    );
    assert_eq!(review_cache, "no-store");

    let (after_status, _, after_agent) = owner_json(&app, "GET", &agent_path, None).await;
    assert_eq!(after_status, StatusCode::OK);
    assert_eq!(
        after_agent["agent"]["state"]["config"]["tools"],
        before_agent["agent"]["state"]["config"]["tools"],
        "review cannot change any effective grant"
    );
    let (list_status, _, listed) = owner_json(
        &app,
        "GET",
        &format!("/api/agents/{agent_id}/mcp-connections"),
        None,
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(listed["connections"], json!([]));
    let (draft_status, draft_cache, drafts) =
        owner_json(&app, "GET", "/api/skill-drafts", None).await;
    assert_eq!(draft_status, StatusCode::OK);
    assert_eq!(draft_cache, "no-store");
    assert_eq!(drafts["drafts"], json!([]));
    let (skill_status, _, skills) = owner_json(&app, "GET", "/api/skills", None).await;
    assert_eq!(skill_status, StatusCode::OK);
    assert_eq!(skills["skills"], json!([]));
    let saved = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot.0.clone()),
    )
    .await
    .unwrap()
    .expect("ordinary agent creation persists the synthetic snapshot");
    assert!(saved.mcp_connections.is_empty());
    assert!(
        saved.skills.is_empty(),
        "review cannot approve or save a skill"
    );
    assert!(
        saved.skill_drafts.is_empty(),
        "review cannot stage a skill draft"
    );
}

#[tokio::test]
async fn m45_owner_routes_keep_canonical_permissions_and_refuse_managed_draft_edits() {
    let (app, snapshot) = daemon_app();
    let workspace = tempfile::tempdir().unwrap();
    assert_eq!(owner_json(&app,"PUT","/api/workspace",Some(json!({"rootPath":workspace.path().to_str().unwrap(),"companyName":"Synthetic M45 lifecycle","mission":"Bounded owner-route evidence","values":[]}))).await.0,StatusCode::OK);
    let agent = create_ordinary_agent(&app, "M45 route lifecycle owner").await;
    let path = format!("/api/agents/{agent}/community-bundles");
    let (_, _, before) = owner_json(&app, "GET", &format!("/api/agents/{agent}"), None).await;
    let tool_names = before["agent"]["state"]["config"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tool| tool["name"].clone())
        .collect::<Vec<_>>();
    let manifest = json!({"schemaVersion":1,"id":"synthetic.route-procedure","version":"1.0.0","name":"Synthetic route procedure","description":"Owner-route lifecycle fixture",
      "compatibility":{"host":"rust-daemon","minVersion":"0.1.0","maxVersionExclusive":"0.2.0","requiredCapabilities":["skills.import-draft.v1"]},"permissions":{"nativeToolNames":["shell"],"externalTools":[]},
      "attribution":[{"component":"procedure","source":"local synthetic test","revision":"m45","license":"MIT","notice":"Synthetic only"}],
      "components":[{"kind":"skill","key":"procedure","skillFile":"---\nname: Route procedure\ndescription: Controlled content\n---\n\nQuote controlled evidence.\n"}]});
    let (status, cache, review) = owner_json(
        &app,
        "POST",
        &format!("{path}/reviews"),
        Some(json!({"manifestText":manifest.to_string(),"programBindings":{}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache, "no-store");
    let receipt =
        json!({"reviewId":review["review"]["id"],"reviewDigest":review["review"]["reviewDigest"]});
    let (status, cache, installed) = owner_json(&app, "POST", &path, Some(receipt.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(cache, "no-store");
    assert_eq!(installed["bundle"]["toolNames"], json!(tool_names));
    let member = &installed["bundle"]["components"][0];
    let draft = member["draftId"].as_str().unwrap();
    let (status, _, edited) = owner_json(
        &app,
        "POST",
        &format!("/api/skill-drafts/{draft}/approve"),
        Some(json!({"body":"Changed through approval body"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(edited["error"].is_string());
    assert!(!workspace.path().join("skills").exists());
    assert_eq!(
        owner_json(&app, "POST", &path, Some(receipt)).await.0,
        StatusCode::CONFLICT
    );
    let bundle_id = installed["bundle"]["id"].as_str().unwrap();
    assert_eq!(
        owner_json(&app, "DELETE", &format!("/api/agents/{agent}"), None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let input = json!({"expectedVersion":installed["bundle"]["version"],"reviewedComponentStateDigest":installed["bundle"]["reviewedComponentStateDigest"],"enabled":false});
    let (status, cache, disabled) =
        owner_json(&app, "PATCH", &format!("{path}/{bundle_id}"), Some(input)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache, "no-store");
    assert_eq!(disabled["bundle"]["state"], "disabled");
    assert_eq!(disabled["bundle"]["toolNames"], json!(tool_names));
    assert_eq!(
        owner_json(
            &app,
            "POST",
            &format!("/api/skill-drafts/{draft}/approve"),
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let input = json!({"expectedVersion":disabled["bundle"]["version"],"reviewedComponentStateDigest":disabled["bundle"]["reviewedComponentStateDigest"]});
    let (status, cache, removed) =
        owner_json(&app, "DELETE", &format!("{path}/{bundle_id}"), Some(input)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache, "no-store");
    assert_eq!(removed["removed"], true);
    assert_eq!(removed["trashPaths"], json!([]));
    assert_eq!(removed["toolNames"], json!(tool_names));
    assert_eq!(
        owner_json(&app, "GET", &path, None).await.2["bundles"],
        json!([])
    );
    let saved = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot.0.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(saved.community_bundles.is_empty());
    assert!(saved.skill_drafts.is_empty());
    assert!(saved.skills.is_empty());
}

#[tokio::test]
async fn m45_owner_review_allows_bounded_json_escaping_and_static_strict_input_errors() {
    let (app, _snapshot) = daemon_app();
    let workspace = tempfile::tempdir().unwrap();
    let agent = create_ordinary_agent(&app, "M45 strict route owner").await;
    let path = format!("/api/agents/{agent}/community-bundles/reviews");
    let (status, _, error) = owner_json(
        &app,
        "GET",
        &format!("/api/agents/{agent}/community-bundles"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["error"].is_string());
    assert_eq!(owner_json(&app,"PUT","/api/workspace",Some(json!({"rootPath":workspace.path().to_str().unwrap(),"companyName":"M45 escaped review","mission":"Bounded decoded source","values":[]}))).await.0,StatusCode::OK);
    let source = format!(
        "---\nname: Escaped procedure\ndescription: Synthetic bounded text\n---\n\n{}",
        "\\".repeat(20 * 1024)
    );
    let manifest = json!({"schemaVersion":1,"id":"synthetic.escaped","version":"1.0.0","name":"Escaped synthetic source","description":"Bounded JSON escaping",
      "compatibility":{"host":"rust-daemon","minVersion":"0.1.0","maxVersionExclusive":"0.2.0","requiredCapabilities":["skills.import-draft.v1"]},"permissions":{"nativeToolNames":[],"externalTools":[]},
      "attribution":[{"component":"synthetic","source":"local synthetic test","revision":"m45","license":"MIT","notice":"Synthetic only"}],
      "components":[{"kind":"skill","key":"first","skillFile":source},{"kind":"skill","key":"second","skillFile":source}]});
    let valid = json!({"manifestText":manifest.to_string(),"programBindings":{}});
    assert!(valid.to_string().len() > 64 * 1024);
    assert_eq!(
        owner_json(&app, "POST", &path, Some(valid.clone())).await.0,
        StatusCode::OK
    );
    for input in [
        json!({"manifestText":manifest.to_string(),"programBindings":{},"expectedBundleVersion":1}),
        json!({"manifestText":manifest.to_string(),"programBindings":{},"bundleId":null}),
        json!({"manifestText":manifest.to_string(),"programBindings":{},"environment":{}}),
    ] {
        let (status, cache, error) = owner_json(&app, "POST", &path, Some(input)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(cache, "no-store");
        assert_eq!(error, json!({"error":"Community bundle input is invalid"}));
    }
    let (status, _, error) =
        owner_json_from(&app, "https://foreign.invalid", "POST", &path, Some(valid)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(error["error"].is_string());
    assert!(!workspace.path().join("skills").exists());
}

#[tokio::test]
async fn m42_save_only_owner_connection_stays_disabled_and_ungranted() {
    let (app, _snapshot) = daemon_app();
    let agent_id = create_ordinary_agent(&app, "M42 ordinary agent").await;
    let base = format!("/api/agents/{agent_id}/mcp-connections");
    let (_, _, before_agent) =
        owner_json(&app, "GET", &format!("/api/agents/{agent_id}"), None).await;

    let (created_status, created_cache, created) = owner_json(
        &app,
        "POST",
        &base,
        Some(json!({ "label": "M42 fixture", "endpoint": "http://127.0.0.1:32123/mcp" })),
    )
    .await;
    assert_eq!(created_status, StatusCode::CREATED);
    assert_eq!(created_cache, "no-store");
    let connection = &created["connection"];
    let connection_id = connection["id"].as_str().expect("created connection ID");
    assert_eq!(connection["agentId"], agent_id);
    assert_eq!(connection["label"], "M42 fixture");
    assert_eq!(connection["endpoint"], "http://127.0.0.1:32123/mcp");
    assert_eq!(connection["enabled"], false);
    assert_eq!(connection["version"], 1);
    assert_eq!(connection["state"], "disconnected");
    assert_eq!(connection["reviewedTools"], json!([]));

    let (list_status, list_cache, listed) = owner_json(&app, "GET", &base, None).await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_cache, "no-store");
    let connections = listed["connections"].as_array().expect("connection list");
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0]["id"], connection_id);
    assert_eq!(connections[0]["agentId"], agent_id);
    assert_eq!(connections[0]["version"], 1);
    assert_eq!(connections[0]["enabled"], false);
    assert_eq!(connections[0]["state"], "disconnected");
    assert_eq!(connections[0]["reviewedTools"], json!([]));

    let (_, _, after_agent) =
        owner_json(&app, "GET", &format!("/api/agents/{agent_id}"), None).await;
    assert_eq!(
        before_agent["agent"]["state"]["config"]["tools"],
        after_agent["agent"]["state"]["config"]["tools"],
        "saving an endpoint must not grant any MCP alias",
    );

    let (update_status, _, updated_agent) = owner_json(
        &app,
        "PATCH",
        &format!("/api/agents/{agent_id}"),
        Some(json!({"system":"Updated without changing tool grants"})),
    )
    .await;
    assert_eq!(update_status, StatusCode::OK);
    assert_eq!(
        before_agent["agent"]["state"]["config"]["tools"],
        updated_agent["agent"]["state"]["config"]["tools"],
        "an unrelated agent update must preserve every existing grant name and descriptor",
    );
}

#[tokio::test]
async fn m44_save_only_owner_stdio_connection_stays_disabled_and_unspawned() {
    let (app, _snapshot) = daemon_app();
    let agent_id = create_ordinary_agent(&app, "M44 ordinary stdio agent").await;
    let base = format!("/api/agents/{agent_id}/mcp-connections");

    let (created_status, created_cache, created) = owner_json(
        &app,
        "POST",
        &base,
        Some(json!({
            "label": "M44 synthetic stdio",
            "stdio": {
                "executable": "C:\\Synthetic\\m44-server.exe",
                "args": []
            }
        })),
    )
    .await;

    assert_eq!(created_status, StatusCode::CREATED);
    assert_eq!(created_cache, "no-store");
    let connection = &created["connection"];
    assert_eq!(connection["agentId"], agent_id);
    assert_eq!(connection["label"], "M44 synthetic stdio");
    assert_eq!(connection["transport"], "stdio");
    assert_eq!(connection["endpoint"], "");
    assert_eq!(
        connection["stdio"]["executable"],
        "C:\\Synthetic\\m44-server.exe"
    );
    assert_eq!(connection["stdio"]["args"], json!([]));
    assert_eq!(connection["enabled"], false);
    assert_eq!(connection["version"], 1);
    assert_eq!(connection["state"], "disconnected");
    assert_eq!(connection["reviewedTools"], json!([]));

    let connection_id = connection["id"].as_str().expect("created connection ID");
    let (list_status, list_cache, listed) = owner_json(&app, "GET", &base, None).await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_cache, "no-store");
    assert_eq!(listed["connections"].as_array().unwrap().len(), 1);
    assert_eq!(listed["connections"][0]["id"], connection_id);
    assert_eq!(listed["connections"][0]["transport"], "stdio");
    assert_eq!(listed["connections"][0]["enabled"], false);
    assert_eq!(listed["connections"][0]["state"], "disconnected");
    assert_eq!(listed["connections"][0]["reviewedTools"], json!([]));
}

#[tokio::test]
async fn m44_stdio_launch_is_strict_and_immutable_after_save() {
    let (app, _snapshot) = daemon_app();
    let agent_id = create_ordinary_agent(&app, "M44 stdio validation").await;
    let base = format!("/api/agents/{agent_id}/mcp-connections");
    let invalid_bodies = [
        json!({"label":"both","endpoint":"https://mcp.example.invalid","stdio":{"executable":"C:\\Synthetic\\server.exe","args":[]}}),
        json!({"label":"neither"}),
        json!({"label":"null","endpoint":null}),
        json!({"label":"unknown","stdio":{"executable":"C:\\Synthetic\\server.exe","args":[],"env":{}}}),
        json!({"label":"relative","stdio":{"executable":"server.exe","args":[]}}),
        json!({"label":"launcher","stdio":{"executable":"C:\\Windows\\System32\\cmd.exe","args":["/c","whoami"]}}),
        json!({"label":"nul","stdio":{"executable":"C:\\Synthetic\\server.exe\u{0000}","args":[]}}),
        json!({"label":"large arg","stdio":{"executable":"C:\\Synthetic\\server.exe","args":["x".repeat(2_049)]}}),
    ];
    for body in invalid_bodies {
        let (status, cache, _) = owner_json(&app, "POST", &base, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(cache, "no-store");
    }
    let (list_status, _, listed) = owner_json(&app, "GET", &base, None).await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(listed["connections"], json!([]));

    let (created_status, _, created) = owner_json(
        &app,
        "POST",
        &base,
        Some(json!({
            "label":"immutable launch",
            "stdio":{"executable":"C:\\Synthetic\\m44-server.exe","args":["--fixture"]}
        })),
    )
    .await;
    assert_eq!(created_status, StatusCode::CREATED);
    let connection = &created["connection"];
    let id = connection["id"].as_str().unwrap();
    let path = format!("{base}/{id}");

    let (launch_patch, cache, _) = owner_json(
        &app,
        "PATCH",
        &path,
        Some(json!({"expectedVersion":1,"stdio":{"executable":"C:\\Synthetic\\other.exe","args":[]}})),
    )
    .await;
    assert_eq!(launch_patch, StatusCode::BAD_REQUEST);
    assert_eq!(cache, "no-store");

    let (endpoint_patch, _, _) = owner_json(
        &app,
        "PATCH",
        &path,
        Some(json!({"expectedVersion":1,"endpoint":"https://mcp.example.invalid/mcp"})),
    )
    .await;
    assert_eq!(endpoint_patch, StatusCode::BAD_REQUEST);

    let (label_patch, _, renamed) = owner_json(
        &app,
        "PATCH",
        &path,
        Some(json!({"expectedVersion":1,"label":"renamed"})),
    )
    .await;
    assert_eq!(label_patch, StatusCode::OK);
    assert_eq!(renamed["connection"]["version"], 2);
    assert_eq!(renamed["connection"]["label"], "renamed");
    assert_eq!(
        renamed["connection"]["stdio"]["executable"],
        "C:\\Synthetic\\m44-server.exe"
    );
    assert_eq!(renamed["connection"]["stdio"]["args"], json!(["--fixture"]));
    assert_eq!(renamed["connection"]["state"], "disconnected");
}

#[tokio::test]
async fn mcp_owner_routes_reject_foreign_and_unknown_request_fields() {
    let (app, _snapshot) = daemon_app();
    let agent_id = create_ordinary_agent(&app, "M42 owner boundary").await;
    let base = format!("/api/agents/{agent_id}/mcp-connections");

    let (foreign_status, foreign_cache, _) = owner_json_from(
        &app,
        "https://foreign.example.invalid",
        "POST",
        &base,
        Some(json!({"label":"not saved","endpoint":"https://mcp.example.invalid/mcp"})),
    )
    .await;
    assert_eq!(foreign_status, StatusCode::FORBIDDEN);
    assert_eq!(foreign_cache, "no-store");

    let (invalid_status, invalid_cache, _) = owner_json(
        &app,
        "POST",
        &base,
        Some(json!({
            "label":"strict fields",
            "endpoint":"https://mcp.example.invalid/mcp",
            "headers":{"authorization":"must-not-be-accepted"}
        })),
    )
    .await;
    assert_eq!(invalid_status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_cache, "no-store");

    let (list_status, _, listed) = owner_json(&app, "GET", &base, None).await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(listed["connections"], json!([]));
}

#[tokio::test]
async fn mcp_updates_use_version_cas_and_preserve_config_on_stale_writes() {
    let (app, _snapshot) = daemon_app();
    let agent_id = create_ordinary_agent(&app, "M42 CAS agent").await;
    let base = format!("/api/agents/{agent_id}/mcp-connections");
    let (created_status, _, created) = owner_json(
        &app,
        "POST",
        &base,
        Some(json!({"label":"CAS fixture","endpoint":"https://mcp.example.invalid/mcp"})),
    )
    .await;
    assert_eq!(created_status, StatusCode::CREATED);
    let id = created["connection"]["id"].as_str().unwrap();
    let path = format!("{base}/{id}");

    let (updated_status, _, updated) = owner_json(
        &app,
        "PATCH",
        &path,
        Some(json!({"expectedVersion":1,"label":"renamed"})),
    )
    .await;
    assert_eq!(updated_status, StatusCode::OK);
    assert_eq!(updated["connection"]["version"], 2);
    assert_eq!(updated["connection"]["label"], "renamed");

    let (null_status, null_cache, _) = owner_json(
        &app,
        "PATCH",
        &path,
        Some(json!({"expectedVersion":2,"label":null,"enabled":true})),
    )
    .await;
    assert_eq!(null_status, StatusCode::BAD_REQUEST);
    assert_eq!(null_cache, "no-store");

    let (stale_status, stale_cache, _) = owner_json(
        &app,
        "PATCH",
        &path,
        Some(json!({"expectedVersion":1,"endpoint":"https://other.example.invalid/mcp"})),
    )
    .await;
    assert_eq!(stale_status, StatusCode::CONFLICT);
    assert_eq!(stale_cache, "no-store");
    let (get_status, _, current) = owner_json(&app, "GET", &path, None).await;
    assert_eq!(get_status, StatusCode::OK);
    assert_eq!(current["connection"]["version"], 2);
    assert_eq!(
        current["connection"]["endpoint"],
        "https://mcp.example.invalid/mcp"
    );
}
