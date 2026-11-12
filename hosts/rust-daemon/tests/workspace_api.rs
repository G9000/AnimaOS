#[allow(dead_code)]
mod support;

use anima_daemon::DaemonConfig;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use support::{owner_request, send_empty_request, send_json_request, send_request, test_app};

#[tokio::test]
async fn get_workspace_reports_unconfigured_with_default_root() {
    let app = test_app();
    let (status, body) = send_json_request(&app, "GET", "/api/workspace", "").await;
    assert_eq!(status, 200);

    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    assert_eq!(body["configured"], false);
    assert!(
        body["defaultRoot"]
            .as_str()
            .expect("defaultRoot is a string")
            .len()
            > 0
    );
    assert!(body["workspace"].is_null());
    assert!(body.get("rootPathExists").is_none());
}

#[tokio::test]
async fn openapi_spec_lists_workspace_endpoint() {
    let app = test_app();
    let (status, body) = send_json_request(&app, "GET", "/openapi.json", "").await;
    assert_eq!(status, 200);
    assert!(body.contains("\"/api/workspace\""), "{body}");
    assert!(body.contains("WorkspaceResponse"), "{body}");
}

#[tokio::test]
async fn put_workspace_rejects_relative_path() {
    let app = test_app();
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": "relative/folder",
            "companyName": "Northwind",
            "mission": "Research"
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("absolute"), "{body}");
}

#[tokio::test]
async fn put_workspace_validate_only_does_not_persist() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-validate-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": root,
            "companyName": "Northwind",
            "mission": "Research",
            "validateOnly": true
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (_, body) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["configured"], false, "{body}");
    assert!(!root.exists(), "validate-only must not create the folder");
}

#[tokio::test]
async fn put_workspace_persists_and_creates_folder() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-persist-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": root,
            "companyName": "Northwind",
            "mission": "Research",
            "values": ["cite sources"]
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(root.is_dir());
    let (_, body) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["configured"], true, "{body}");
    assert_eq!(body["workspace"]["companyName"], "Northwind", "{body}");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn put_workspace_validate_only_reports_root_path_exists() {
    let app = test_app();

    let missing = std::env::temp_dir().join(format!("anima-missing-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": missing,
            "companyName": "Northwind",
            "mission": "Research",
            "validateOnly": true
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["rootPathExists"], false, "{body}");
    assert!(
        !missing.exists(),
        "validate-only must not create the folder"
    );

    let existing = std::env::temp_dir();
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": existing,
            "companyName": "Northwind",
            "mission": "Research",
            "validateOnly": true
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["rootPathExists"], true, "{body}");
}

#[tokio::test]
async fn put_workspace_validate_only_reflects_current_configuration() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-configured-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": root,
            "companyName": "Northwind",
            "mission": "Research"
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let probe = std::env::temp_dir().join(format!("anima-probe-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "PUT",
        "/api/workspace",
        &serde_json::json!({
            "rootPath": probe,
            "companyName": "Contoso",
            "mission": "Sales",
            "validateOnly": true
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        body["configured"], true,
        "validate-only must reflect the already-configured daemon: {body}"
    );
    assert!(!probe.exists(), "validate-only must not create the folder");

    std::fs::remove_dir_all(&root).ok();
}

fn bootstrap_body(root: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "workspace": {
            "rootPath": root,
            "companyName": "Northwind Research",
            "mission": "Continuous equity research",
            "values": ["cite sources"]
        },
        "agent": {
            "name": "Anima",
            "presetId": "chief-of-staff",
            "bio": "A vigilant chief of staff.",
            "adjectives": ["vigilant", "concise", "proactive"],
            "style": "brief, numbered",
            "system": "You are Anima, chief of staff at Northwind Research...",
            "provider": "deterministic",
            "model": "deterministic",
            "tools": ["memory_search", "memory_add", "recent_memories", "get_current_time", "calculate", "read_file"]
        }
    })
}

#[tokio::test]
async fn bootstrap_creates_workspace_file_agent_and_steward() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-boot-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(&root).to_string(),
    )
    .await;
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(status, 201, "body: {body}");
    assert_eq!(body["workspace"]["companyName"], "Northwind Research");
    assert!(body.get("workers").is_none(), "{body}");
    assert_eq!(
        body["agent"]["state"]["config"]["bio"],
        "A vigilant chief of staff."
    );
    assert_eq!(
        body["agent"]["state"]["config"]["adjectives"],
        serde_json::json!(["vigilant", "concise", "proactive"])
    );
    // No agent ranks above another: the first agent carries no role.
    assert!(body["agent"]["state"]["config"]["settings"]["additional"]["workspaceRole"].is_null());
    // The steward comes with the workspace. A mock model is no model, so it
    // runs on the rules.
    let steward = &body["steward"]["state"];
    assert_eq!(steward["name"], "Steward", "{body}");
    assert_eq!(
        steward["config"]["settings"]["additional"]["agentKind"],
        "steward"
    );
    assert_eq!(steward["config"]["provider"], "steward");

    // anima.yaml lists the agent as an equal; the steward is not listed.
    let yaml_path = root.join("anima.yaml");
    assert!(yaml_path.is_file());
    assert!(
        !root.join("anima.yaml.tmp").exists(),
        "tmp file must be renamed away"
    );
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
    assert_eq!(yaml["name"], "Northwind Research");
    assert_eq!(yaml["agents"][0]["name"], "Anima");
    assert_eq!(yaml["agents"][0]["bio"], "A vigilant chief of staff.");
    assert_eq!(yaml["agents"].as_sequence().unwrap().len(), 1);
    assert!(yaml["orchestrator"].is_null());
    assert!(yaml["strategy"].is_null());
    assert_eq!(yaml["provider"], "deterministic");

    // Workspace config is live.
    let (_, workspace) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], true);

    // The agent's tools were resolved to canonical descriptors.
    let tools = body["agent"]["state"]["config"]["tools"]
        .as_array()
        .unwrap();
    let read_file = tools.iter().find(|t| t["name"] == "read_file").unwrap();
    assert!(read_file["description"].as_str().unwrap().len() > 0);

    // The steward cannot be deleted.
    let steward_id = steward["id"].as_str().unwrap();
    let (status, body) = delete_agent(&app, steward_id).await;
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("steward cannot be deleted"), "{body}");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn bootstrap_rejects_unknown_tools_without_side_effects() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-boot-bad-{}", std::process::id()));
    let mut body = bootstrap_body(&root);
    body["agent"]["tools"] = serde_json::json!(["definitely_not_a_tool"]);
    let (status, _) =
        send_json_request(&app, "POST", "/api/workspace/bootstrap", &body.to_string()).await;
    assert_eq!(status, 400);
    assert!(
        !root.join("anima.yaml").exists(),
        "failed bootstrap must not write anima.yaml"
    );
    assert!(
        !root.join("anima.yaml.tmp").exists(),
        "failed bootstrap must not leave a tmp file"
    );
    let (_, workspace) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], false);
    let (_, agents) = send_json_request(&app, "GET", "/api/agents", "").await;
    let agents: serde_json::Value = serde_json::from_str(&agents).unwrap();
    assert_eq!(agents["agents"].as_array().unwrap().len(), 0);

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn bootstrap_rejects_empty_system() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-boot-sys-{}", std::process::id()));
    let mut body = bootstrap_body(&root);
    body["agent"]["system"] = serde_json::json!("  ");
    let (status, _) =
        send_json_request(&app, "POST", "/api/workspace/bootstrap", &body.to_string()).await;
    assert_eq!(status, 400);
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn bootstrap_rejects_blank_bio() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-boot-bio-{}", std::process::id()));
    let mut body = bootstrap_body(&root);
    body["agent"]["bio"] = serde_json::json!("  ");
    let (status, _) =
        send_json_request(&app, "POST", "/api/workspace/bootstrap", &body.to_string()).await;
    assert_eq!(status, 400);
    assert!(
        !root.join("anima.yaml").exists(),
        "blank bio must not produce anima.yaml"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn bootstrap_conflicts_when_already_bootstrapped() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-boot-twice-{}", std::process::id()));
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(&root).to_string(),
    )
    .await;
    assert_eq!(status, 201, "body: {body}");

    // Second bootstrap (even against a different root) is rejected.
    let other_root =
        std::env::temp_dir().join(format!("anima-boot-twice-other-{}", std::process::id()));
    let (status, _) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(&other_root).to_string(),
    )
    .await;
    assert_eq!(status, 409);

    let (_, agents) = send_json_request(&app, "GET", "/api/agents", "").await;
    let agents: serde_json::Value = serde_json::from_str(&agents).unwrap();
    assert_eq!(
        agents["agents"].as_array().unwrap().len(),
        2,
        "the first agent and the steward: {agents}"
    );

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&other_root).ok();
}

#[tokio::test]
async fn bootstrap_rolls_back_when_workspace_file_write_fails() {
    let app = test_app();
    let root = std::env::temp_dir().join(format!("anima-boot-iofail-{}", std::process::id()));
    // Force the anima.yaml write to fail: pre-create the target path as a
    // directory so the atomic rename cannot replace it with a file.
    std::fs::create_dir_all(root.join("anima.yaml")).expect("anima.yaml dir placeholder");

    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(&root).to_string(),
    )
    .await;
    assert_eq!(status, 503, "body: {body}");

    // Rollback also cleans up the tmp file from the atomic write.
    assert!(
        !root.join("anima.yaml.tmp").exists(),
        "tmp file must be removed on rollback"
    );

    // Full rollback: no agent, no steward, no live workspace configuration.
    let (_, agents) = send_json_request(&app, "GET", "/api/agents", "").await;
    let agents: serde_json::Value = serde_json::from_str(&agents).unwrap();
    assert_eq!(
        agents["agents"].as_array().unwrap().len(),
        0,
        "agent and steward must be rolled back"
    );
    let (_, workspace) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(
        workspace["configured"], false,
        "workspace must be rolled back"
    );

    std::fs::remove_dir_all(&root).ok();
}

fn inspect_uri(root: &std::path::Path) -> String {
    support::query_uri(
        "/api/workspace/inspect",
        "rootPath",
        root.to_str().expect("temp root is utf-8"),
    )
}

const VALID_WORKSPACE_YAML: &str = r#"name: Northwind Research
description: Continuous equity research
mission: Continuous equity research
values: [cite sources]
model: kimi-k2
provider: moonshot
agents:
  - name: Anima
    bio: A vigilant chief of staff.
    system: You are Anima.
    model: kimi-k2
    tools: [read_file]
  - name: Scout
    bio: A scout.
    system: You are Scout.
"#;

#[tokio::test]
async fn inspect_returns_found_false_without_yaml() {
    let root = support::use_temp_workspace_root("inspect-empty");
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"found\":false"), "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    assert!(body.get("companyName").is_none(), "{body}");
    assert!(body.get("agents").is_none(), "{body}");
}

#[tokio::test]
async fn inspect_returns_preview_for_valid_yaml() {
    let root = support::use_temp_workspace_root("inspect-valid");
    std::fs::write(root.path().join("anima.yaml"), VALID_WORKSPACE_YAML).expect("yaml writes");
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 200, "{body}");

    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    assert_eq!(body["found"], true, "{body}");
    assert_eq!(body["companyName"], "Northwind Research", "{body}");
    assert_eq!(body["mission"], "Continuous equity research", "{body}");
    assert_eq!(
        body["values"],
        serde_json::json!(["cite sources"]),
        "{body}"
    );
    assert!(body.get("orchestrator").is_none(), "{body}");

    // Every agent is previewed the same way.
    let agents = body["agents"].as_array().expect("agents is an array");
    assert_eq!(agents.len(), 2, "{body}");
    assert_eq!(agents[0]["name"], "Anima", "{body}");
    assert_eq!(agents[0]["bio"], "A vigilant chief of staff.", "{body}");
    assert_eq!(agents[0]["provider"], "moonshot", "{body}");
    assert_eq!(agents[0]["model"], "kimi-k2", "{body}");
    assert_eq!(agents[1]["name"], "Scout", "{body}");
    assert_eq!(agents[1]["bio"], "A scout.", "{body}");
    assert_eq!(agents[1]["model"], "kimi-k2", "{body}");
    assert!(
        agents[1].get("system").is_none(),
        "previews omit system: {body}"
    );

    assert!(
        body["providerAvailable"].is_boolean(),
        "providerAvailable present: {body}"
    );
}

#[tokio::test]
async fn inspect_rejects_malformed_yaml() {
    let root = support::use_temp_workspace_root("inspect-malformed");
    std::fs::write(root.path().join("anima.yaml"), "{{ not yaml").expect("yaml writes");
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn inspect_rejects_a_file_without_agents_such_as_an_orchestrator_file() {
    let root = support::use_temp_workspace_root("inspect-no-agents");
    let yaml = "name: Old\nmission: Old\norchestrator:\n  name: Boss\n  bio: b\n  system: s\n";
    std::fs::write(root.path().join("anima.yaml"), yaml).expect("yaml writes");
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.contains("agents must list at least one agent"),
        "{body}"
    );
}

#[tokio::test]
async fn inspect_rejects_yaml_without_mission_or_description() {
    let root = support::use_temp_workspace_root("inspect-no-mission");
    let yaml = VALID_WORKSPACE_YAML
        .replace("description: Continuous equity research\n", "")
        .replace("mission: Continuous equity research\n", "");
    std::fs::write(root.path().join("anima.yaml"), yaml).expect("yaml writes");
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.contains("mission or description is required"),
        "{body}"
    );
}

#[tokio::test]
async fn inspect_rejects_missing_root_path_param_with_json_error() {
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", "/api/workspace/inspect").await;
    assert_eq!(status, 400, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("error body is json");
    assert_eq!(body["error"], "rootPath is required", "{body}");
}

#[tokio::test]
async fn inspect_rejects_empty_root_path_param() {
    let app = test_app();
    let uri = support::query_uri("/api/workspace/inspect", "rootPath", "");
    let (status, body) = send_empty_request(&app, "GET", &uri).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("rootPath is required"), "{body}");
}

#[tokio::test]
async fn inspect_defaults_provider_to_openai_when_yaml_omits_provider() {
    let root = support::use_temp_workspace_root("inspect-default-provider");
    let yaml = VALID_WORKSPACE_YAML.replace("provider: moonshot\n", "");
    std::fs::write(root.path().join("anima.yaml"), yaml).expect("yaml writes");
    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 200, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    assert_eq!(body["found"], true, "{body}");
    assert_eq!(body["agents"][0]["provider"], "openai", "{body}");
    assert_eq!(body["agents"][1]["provider"], "openai", "{body}");
    assert!(
        body["providerAvailable"].is_boolean(),
        "providerAvailable present: {body}"
    );
}

const RESUME_WORKSPACE_YAML: &str = r#"name: Northwind Research
description: Continuous equity research
mission: Continuous equity research
values: [cite sources]
model: kimi-k2
provider: moonshot
agents:
  - name: Anima
    bio: A vigilant chief of staff.
    system: You are Anima.
    model: kimi-k2
    tools: [read_file]
  - name: Scout
    bio: A scout.
    system: You are Scout.
  - name: Scribe
    bio: A scribe.
    system: You are Scribe.
"#;

#[tokio::test]
async fn resume_adopts_every_agent_as_an_equal_and_adds_the_steward() {
    let root = support::use_temp_workspace_root("resume-adopt");
    let yaml_path = root.path().join("anima.yaml");
    std::fs::write(&yaml_path, RESUME_WORKSPACE_YAML).expect("yaml writes");
    let yaml_before = std::fs::read(&yaml_path).expect("yaml reads");
    let app = test_app();

    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/resume",
        &serde_json::json!({ "rootPath": root.path() }).to_string(),
    )
    .await;
    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    assert_eq!(status, 201, "body: {body}");
    assert_eq!(
        body["workspace"]["companyName"], "Northwind Research",
        "{body}"
    );
    assert_eq!(
        body["skipped"],
        serde_json::json!([]),
        "fresh adopt without collisions skips nothing: {body}"
    );
    let agents = body["agents"].as_array().expect("agents is an array");
    assert_eq!(
        agents
            .iter()
            .map(|agent| agent["state"]["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Anima", "Scout", "Scribe"],
        "{body}"
    );
    for agent in agents {
        assert!(agent["state"]["config"]["settings"]["additional"]["workspaceRole"].is_null());
    }
    assert_eq!(body["steward"]["state"]["name"], "Steward", "{body}");

    // Workspace config is live and the whole roster exists, plus the steward.
    let (_, workspace) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], true, "{workspace}");
    let (_, agents) = send_json_request(&app, "GET", "/api/agents", "").await;
    let agents: serde_json::Value = serde_json::from_str(&agents).unwrap();
    assert_eq!(agents["agents"].as_array().unwrap().len(), 4, "{agents}");

    // Resume never owns the yaml: it must be byte-identical afterwards.
    let yaml_after = std::fs::read(&yaml_path).expect("yaml reads");
    assert_eq!(yaml_before, yaml_after, "resume must not modify anima.yaml");
}

#[tokio::test]
async fn resume_rejects_unknown_tool_without_side_effects() {
    let root = support::use_temp_workspace_root("resume-badtool");
    // Scout carries the unknown tool, so Anima is created first and the
    // mid-batch failure must roll it back.
    let yaml = RESUME_WORKSPACE_YAML.replace(
        "  - name: Scout\n    bio: A scout.\n    system: You are Scout.\n",
        "  - name: Scout\n    bio: A scout.\n    system: You are Scout.\n    tools: [not_a_real_tool]\n",
    );
    assert!(yaml.contains("not_a_real_tool"), "fixture edit applied");
    std::fs::write(root.path().join("anima.yaml"), yaml).expect("yaml writes");
    let app = test_app();

    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/resume",
        &serde_json::json!({ "rootPath": root.path() }).to_string(),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.contains("Scout"),
        "error names the failing agent: {body}"
    );

    // Anima was created before Scout failed, so an empty roster proves the
    // in-guard batch rollback ran.
    let (_, agents) = send_json_request(&app, "GET", "/api/agents", "").await;
    let agents: serde_json::Value = serde_json::from_str(&agents).unwrap();
    assert_eq!(
        agents["agents"].as_array().unwrap().len(),
        0,
        "no agents may be created on failure: {agents}"
    );
    let (_, workspace) = send_json_request(&app, "GET", "/api/workspace", "").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], false, "{workspace}");
}

#[tokio::test]
async fn resume_without_yaml_returns_400() {
    let root = support::use_temp_workspace_root("resume-noyaml");
    let app = test_app();
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/resume",
        &serde_json::json!({ "rootPath": root.path() }).to_string(),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("anima.yaml"), "{body}");
}

#[tokio::test]
async fn inspect_reports_provider_unavailable_without_moonshot_keys() {
    let root = support::use_temp_workspace_root("inspect-provider-keys");
    std::fs::write(root.path().join("anima.yaml"), VALID_WORKSPACE_YAML).expect("yaml writes");
    // Pin the environment: moonshot reports configured only when one of its
    // API-key env vars is set, so clear them all for a deterministic result.
    // Serialized with the other inspect tests via the workspace-root lock
    // held by `root`.
    let saved: Vec<(&'static str, Option<std::ffi::OsString>)> = [
        "MOONSHOT_API_KEY",
        "MOONSHOT_KEY",
        "MOONSHOT_TOKEN",
        "KIMI_API_KEY",
    ]
    .into_iter()
    .map(|name| {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
        (name, previous)
    })
    .collect();

    let app = test_app();
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;

    for (name, previous) in saved {
        if let Some(previous) = previous {
            std::env::set_var(name, previous);
        }
    }

    assert_eq!(status, 200, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    assert_eq!(body["found"], true, "{body}");
    assert_eq!(body["providerAvailable"], false, "{body}");
}

// --- resume conflict, idempotency, and rollback semantics -----------------

/// Two-agent fixture (Anima and Scout) for the idempotency tests.
const RESUME_RESTORE_YAML: &str = r#"name: Northwind Research
description: Continuous equity research
mission: Continuous equity research
values: [cite sources]
model: kimi-k2
provider: moonshot
agents:
  - name: Anima
    bio: A vigilant chief of staff.
    system: You are Anima.
    model: kimi-k2
    tools: [read_file]
  - name: Scout
    bio: A scout.
    system: You are Scout.
"#;

/// A roster whose names deliberately differ from the bootstrap agent, so a
/// different-root 409 cannot be confused with a name collision.
const OTHER_ROOT_YAML: &str = r#"name: Contoso
description: Sales
mission: Sales
model: kimi-k2
agents:
  - name: Atlas
    bio: A tireless agent.
    system: You are Atlas.
  - name: Scout
    system: You are Scout.
"#;

async fn resume_workspace(app: &axum::Router, root: &std::path::Path) -> (StatusCode, String) {
    send_json_request(
        app,
        "POST",
        "/api/workspace/resume",
        &serde_json::json!({ "rootPath": root }).to_string(),
    )
    .await
}

async fn delete_agent(app: &axum::Router, agent_id: &str) -> (StatusCode, String) {
    send_request(
        app,
        owner_request(Request::builder())
            .method("DELETE")
            .uri(format!("/api/agents/{agent_id}"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await
}

/// The live roster as (id, name) pairs, sorted by name for stable assertions.
fn roster(body: &str) -> Vec<(String, String)> {
    let body: serde_json::Value = serde_json::from_str(body).expect("body is json");
    let mut agents = body["agents"]
        .as_array()
        .expect("agents is an array")
        .iter()
        .map(|agent| {
            (
                agent["state"]["id"].as_str().expect("id").to_string(),
                agent["state"]["name"].as_str().expect("name").to_string(),
            )
        })
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.1.cmp(&right.1));
    agents
}

fn names(body: &str) -> Vec<String> {
    roster(body).into_iter().map(|(_, name)| name).collect()
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[tokio::test]
async fn resume_conflicts_when_configured_for_different_root() {
    let guard = support::use_temp_workspace_root("resume-diff-root");
    let root_a = guard.path().join("a");
    let root_b = guard.path().join("b");
    std::fs::create_dir_all(&root_b).expect("root b created");
    std::fs::write(root_b.join("anima.yaml"), OTHER_ROOT_YAML).expect("yaml writes");
    let app = test_app();

    // Bootstrap configures the daemon for root A.
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(&root_a).to_string(),
    )
    .await;
    assert_eq!(status, 201, "body: {body}");

    // Resuming root B is a conflict that names root A.
    let (status, body) = resume_workspace(&app, &root_b).await;
    assert_eq!(status, 409, "{body}");
    let canonical_a = root_a.canonicalize().expect("root a canonicalizes");
    let error: serde_json::Value = serde_json::from_str(&body).expect("error body is json");
    let message = error["error"].as_str().expect("error message");
    assert!(
        message.contains(&canonical_a.display().to_string()),
        "conflict names the configured root: {body}"
    );

    // Nothing from root B's yaml was created.
    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    assert_eq!(names(&agents), vec!["Anima", "Steward"], "{agents}");
}

#[tokio::test]
async fn resume_same_root_restores_missing_agents_only() {
    let root = support::use_temp_workspace_root("resume-restore");
    std::fs::write(root.path().join("anima.yaml"), RESUME_RESTORE_YAML).expect("yaml writes");
    let app = test_app();

    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 201, "{body}");
    let first: serde_json::Value = serde_json::from_str(&body).expect("body is json");
    let anima_id = first["agents"][0]["state"]["id"]
        .as_str()
        .expect("anima id")
        .to_string();
    let scout_id = first["agents"][1]["state"]["id"]
        .as_str()
        .expect("scout id")
        .to_string();
    let steward_id = first["steward"]["state"]["id"]
        .as_str()
        .expect("steward id")
        .to_string();

    // Scout disappears (crash, owner cleanup, ...); a re-resume of the same
    // root must restore exactly it.
    let (status, body) = delete_agent(&app, &scout_id).await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 201, "{body}");
    let second: serde_json::Value = serde_json::from_str(&body).expect("body is json");

    // Every listed agent is reported: the kept Anima and a fresh Scout.
    assert_eq!(
        second["agents"][0]["state"]["id"].as_str(),
        Some(anima_id.as_str()),
        "{second}"
    );
    assert_eq!(
        second["skipped"],
        serde_json::json!(["Anima"]),
        "the kept agent is reported as skipped: {second}"
    );
    assert_eq!(second["agents"][1]["state"]["name"], "Scout", "{second}");
    assert_ne!(
        second["agents"][1]["state"]["id"].as_str(),
        Some(scout_id.as_str()),
        "the restored agent is a fresh agent: {second}"
    );
    // The existing steward is kept, not duplicated.
    assert_eq!(
        second["steward"]["state"]["id"].as_str(),
        Some(steward_id.as_str()),
        "{second}"
    );

    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    assert_eq!(
        names(&agents),
        vec!["Anima", "Scout", "Steward"],
        "{agents}"
    );

    let (_, workspace) = send_empty_request(&app, "GET", "/api/workspace").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], true, "{workspace}");
}

#[tokio::test]
async fn resume_fresh_adopt_skips_persisted_agent_name_collisions() {
    let root = support::use_temp_workspace_root("resume-adopt-collision");
    std::fs::write(root.path().join("anima.yaml"), RESUME_RESTORE_YAML).expect("yaml writes");
    let app = test_app();

    // A standalone agent that happens to share a listed agent's name.
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/agents",
        r#"{"name":"Anima","model":"gpt-5.4"}"#,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let existing_id = support::extract_json_string_field(&body, "id");

    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 201, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("body is json");

    // The persisted agent is kept and reported in its place; only the
    // missing agent is created.
    assert_eq!(
        body["agents"][0]["state"]["id"].as_str(),
        Some(existing_id.as_str()),
        "{body}"
    );
    assert_eq!(
        body["skipped"],
        serde_json::json!(["Anima"]),
        "the name collision is reported as skipped: {body}"
    );
    assert_eq!(body["agents"][1]["state"]["name"], "Scout", "{body}");

    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    let live = roster(&agents);
    assert_eq!(
        names(&agents),
        vec!["Anima", "Scout", "Steward"],
        "{agents}"
    );
    assert!(
        live.iter()
            .any(|(id, name)| id == &existing_id && name == "Anima"),
        "pre-existing agent kept with its id: {agents}"
    );

    let (_, workspace) = send_empty_request(&app, "GET", "/api/workspace").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], true, "{workspace}");
}

#[tokio::test]
async fn resume_all_agents_exist_returns_409() {
    let root = support::use_temp_workspace_root("resume-nothing-to-do");
    std::fs::write(root.path().join("anima.yaml"), RESUME_RESTORE_YAML).expect("yaml writes");
    let app = test_app();

    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 201, "{body}");

    // A second resume with the full roster already live is a meaningful
    // "nothing to do" conflict rather than a silent 200.
    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body.contains("all agents from anima.yaml already exist"),
        "{body}"
    );

    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    assert_eq!(roster(&agents).len(), 3, "{agents}");
}

#[tokio::test]
async fn resume_rolls_back_when_persist_fails() {
    let workspace = support::use_temp_workspace_root("resume-rollback");
    let control_plane_path = workspace.control_plane_path();
    // Serialized against the other workspace tests by the root lock held in
    // `workspace`.
    let _guard = EnvVarGuard::set("ANIMAOS_RS_CONTROL_PLANE_FILE", &control_plane_path);
    let adopt_root = workspace.path().join("adopt");
    std::fs::create_dir_all(&adopt_root).expect("adopt root created");
    std::fs::write(adopt_root.join("anima.yaml"), RESUME_RESTORE_YAML).expect("yaml writes");
    support::run_in_joined_runtime(move || async move {
        let app = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .expect("app configures persistence");

        // Force the next control-plane save to fail: replace the snapshot file
        // with a directory so the atomic rename cannot replace it.
        std::fs::remove_file(&control_plane_path).expect("startup snapshot exists");
        std::fs::create_dir(&control_plane_path).expect("sabotage directory created");

        let (status, body) = resume_workspace(&app, &adopt_root).await;
        assert_eq!(status, 503, "{body}");

        // Full rollback: no agents, no steward, no live workspace.
        let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
        assert_eq!(
            roster(&agents).len(),
            0,
            "agents must be rolled back: {agents}"
        );
        let (_, workspace_body) = send_empty_request(&app, "GET", "/api/workspace").await;
        let workspace_body: serde_json::Value = serde_json::from_str(&workspace_body).unwrap();
        assert_eq!(
            workspace_body["configured"], false,
            "workspace must be rolled back: {workspace_body}"
        );
    })
    .await;
}

#[tokio::test]
async fn resume_rejects_yaml_internal_duplicate_names() {
    let root = support::use_temp_workspace_root("resume-dup-names");
    let yaml = RESUME_RESTORE_YAML.replace("  - name: Scout\n", "  - name: Anima\n");
    assert!(
        yaml.matches("  - name: Anima\n").count() == 2,
        "fixture edit applied"
    );
    std::fs::write(root.path().join("anima.yaml"), yaml).expect("yaml writes");
    let app = test_app();

    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.contains("duplicate agent name 'Anima'"),
        "error names the duplicate: {body}"
    );

    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    assert_eq!(roster(&agents).len(), 0, "{agents}");
    let (_, workspace) = send_empty_request(&app, "GET", "/api/workspace").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], false, "{workspace}");
}

#[tokio::test]
async fn resume_rejects_whitespace_agent_system() {
    let root = support::use_temp_workspace_root("resume-blank-system");
    let yaml = RESUME_RESTORE_YAML.replace("    system: You are Scout.\n", "    system: \" \"\n");
    assert!(yaml.contains("system: \" \""), "fixture edit applied");
    std::fs::write(root.path().join("anima.yaml"), yaml).expect("yaml writes");
    let app = test_app();

    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("Scout"), "error names the agent: {body}");

    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    assert_eq!(roster(&agents).len(), 0, "{agents}");
    let (_, workspace) = send_empty_request(&app, "GET", "/api/workspace").await;
    let workspace: serde_json::Value = serde_json::from_str(&workspace).unwrap();
    assert_eq!(workspace["configured"], false, "{workspace}");
}

#[tokio::test]
async fn resumed_agents_survive_restart() {
    let workspace = support::use_temp_workspace_root("resume-restart");
    let control_plane_path = workspace.control_plane_path();
    let _guard = EnvVarGuard::set("ANIMAOS_RS_CONTROL_PLANE_FILE", &control_plane_path);
    std::fs::write(workspace.path().join("anima.yaml"), RESUME_RESTORE_YAML).expect("yaml writes");

    let first_root = workspace.path().to_path_buf();
    support::run_in_joined_runtime(move || async move {
        let first_app = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .expect("first app should configure persistence");
        let (status, body) = resume_workspace(&first_app, &first_root).await;
        assert_eq!(status, 201, "{body}");
        drop(first_app);
    })
    .await;

    let second_root = workspace.path().to_path_buf();
    support::run_in_joined_runtime(move || async move {
        let second_app = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .expect("second app should configure persistence");
        let (_, agents) = send_empty_request(&second_app, "GET", "/api/agents").await;
        assert_eq!(
            names(&agents),
            vec!["Anima", "Scout", "Steward"],
            "resumed roster restored after restart: {agents}"
        );
        let (_, workspace_body) = send_empty_request(&second_app, "GET", "/api/workspace").await;
        let workspace_body: serde_json::Value = serde_json::from_str(&workspace_body).unwrap();
        assert_eq!(workspace_body["configured"], true, "{workspace_body}");
        let canonical_root = second_root.canonicalize().expect("root canonicalizes");
        assert_eq!(
            workspace_body["workspace"]["rootPath"].as_str(),
            Some(canonical_root.display().to_string().as_str()),
            "{workspace_body}"
        );
    })
    .await;
}

async fn send_workspace_files_request(
    app: &axum::Router,
    method: &str,
    uri: &str,
) -> (StatusCode, String) {
    send_request(
        app,
        owner_request(Request::builder())
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn workspace_files_are_confined_filtered_and_bounded() {
    let root = support::use_temp_workspace_root("workspace-files");
    let app = test_app();
    let (status, _) = send_workspace_files_request(&app, "GET", "/api/workspace/files").await;
    assert_eq!(status, 409);
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(root.path()).to_string(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs/readme.md"), "Hello workspace").unwrap();
    for secret in [
        ".env",
        "credentials.json",
        "private.key",
        "control-plane.json",
    ] {
        std::fs::write(root.path().join(secret), "SECRET_SENTINEL").unwrap();
    }
    std::fs::create_dir(root.path().join("node_modules")).unwrap();
    std::fs::write(root.path().join("node_modules/hidden.txt"), "excluded").unwrap();
    let (status, body) = send_workspace_files_request(&app, "GET", "/api/workspace/files").await;
    assert_eq!(status, 200, "{body}");
    let list: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(list["truncated"], false);
    assert!(list["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "docs/readme.md" && file["sizeBytes"] == 15));
    assert!(!body.contains("credentials.json"));
    assert!(!body.contains("node_modules"));
    for path in [
        "../escape",
        "/absolute",
        "C:\\outside.txt",
        "docs/../../escape",
        "docs\\..\\.env",
        ".env",
        "credentials.json",
        "private.key",
        "control-plane.json",
        "node_modules/hidden.txt",
    ] {
        let (status, body) = send_workspace_files_request(
            &app,
            "GET",
            &support::query_uri("/api/workspace/file", "path", path),
        )
        .await;
        assert_eq!(status, 400, "{path}: {body}");
        assert!(!body.contains("SECRET_SENTINEL"));
    }
    let (status, body) = send_workspace_files_request(
        &app,
        "GET",
        &support::query_uri("/api/workspace/file", "path", "docs/readme.md"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let file: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(file["content"], "Hello workspace");
    assert_eq!(file["path"], "docs/readme.md");
    assert_eq!(file["truncated"], false);
    for number in 0..205 {
        std::fs::write(root.path().join(format!("file-{number}.txt")), "ok").unwrap();
    }
    let (_, body) = send_workspace_files_request(&app, "GET", "/api/workspace/files").await;
    let list: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(list["files"].as_array().unwrap().len(), 200);
    assert_eq!(list["truncated"], true);
}

#[tokio::test]
async fn workspace_file_preview_rejects_binary_and_truncates_utf8_safely() {
    let root = support::use_temp_workspace_root("workspace-file-preview");
    let app = test_app();
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(root.path()).to_string(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    std::fs::write(
        root.path().join("large.txt"),
        format!("x{}", "🦊".repeat(40000)),
    )
    .unwrap();
    std::fs::write(root.path().join("binary.txt"), [0, 255, 0]).unwrap();
    std::fs::write(
        root.path().join("config.txt"),
        r#"{"apiKey" : "SECRET_SENTINEL"}"#,
    )
    .unwrap();
    let (status, body) = send_workspace_files_request(
        &app,
        "GET",
        &support::query_uri("/api/workspace/file", "path", "large.txt"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let file: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(file["truncated"], true);
    let content = file["content"].as_str().unwrap();
    assert!(content.len() <= 128 * 1024);
    assert!(!content.contains('�'));
    let (status, _) = send_workspace_files_request(
        &app,
        "GET",
        &support::query_uri("/api/workspace/file", "path", "binary.txt"),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = send_workspace_files_request(
        &app,
        "GET",
        &support::query_uri("/api/workspace/file", "path", "missing.txt"),
    )
    .await;
    assert_eq!(status, 404);
    let (status, body) = send_workspace_files_request(
        &app,
        "GET",
        &support::query_uri("/api/workspace/file", "path", "config.txt"),
    )
    .await;
    assert_eq!(status, 400);
    assert!(!body.contains("SECRET_SENTINEL"));
}

#[tokio::test]
async fn workspace_files_reject_directory_links_outside_the_configured_root() {
    let root = support::use_temp_workspace_root("workspace-file-links");
    let workspace = root.path().join("workspace");
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("outside.txt"), "OUTSIDE_SENTINEL").unwrap();
    let app = test_app();
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(&workspace).to_string(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let link = workspace.join("linked");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let (status, body) = send_workspace_files_request(&app, "GET", "/api/workspace/files").await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("linked"));
    let (status, body) = send_workspace_files_request(
        &app,
        "GET",
        &support::query_uri("/api/workspace/file", "path", "linked/outside.txt"),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(!body.contains("OUTSIDE_SENTINEL"));
}

#[tokio::test]
async fn model_overrides_survive_yaml_resume_and_restart() {
    let root = support::use_temp_workspace_root("team-model-overrides");
    let control_plane_path = root.control_plane_path();
    let _guard = EnvVarGuard::set("ANIMAOS_RS_CONTROL_PLANE_FILE", &control_plane_path);
    let mut request = bootstrap_body(root.path());
    request["agent"]["model"] = serde_json::json!("setup-model");
    let app = test_app();
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &request.to_string(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(root.path().join("anima.yaml")).unwrap())
            .unwrap();
    assert_eq!(yaml["agents"][0]["model"], "setup-model");
    assert_eq!(yaml["agents"][0]["provider"], "deterministic");
    let (status, body) = send_empty_request(&app, "GET", &inspect_uri(root.path())).await;
    assert_eq!(status, 200, "{body}");
    let inspected: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(inspected["agents"][0]["model"], "setup-model");
    let first_root = root.path().to_path_buf();
    support::run_in_joined_runtime(move || async move {
        let fresh = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        let (status, body) = resume_workspace(&fresh, &first_root).await;
        assert_eq!(status, 201, "{body}");
        assert_model_overrides(&fresh).await;
        drop(fresh);
    })
    .await;

    support::run_in_joined_runtime(|| async {
        let restarted = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        assert_model_overrides(&restarted).await;
    })
    .await;
}

async fn assert_model_overrides(app: &axum::Router) {
    let (_, body) = send_empty_request(app, "GET", "/api/agents").await;
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    let agent = body["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["state"]["name"] == "Anima")
        .unwrap();
    assert_eq!(agent["state"]["config"]["model"], "setup-model");
    assert_eq!(agent["state"]["config"]["provider"], "deterministic");
}

#[tokio::test]
async fn settings_model_updates_preserve_yaml_fields_and_survive_resume_and_restart() {
    let root = support::use_temp_workspace_root("settings-yaml-sync");
    let control_plane_path = root.control_plane_path();
    let _guard = EnvVarGuard::set("ANIMAOS_RS_CONTROL_PLANE_FILE", &control_plane_path);
    let yaml_path = root.path().join("anima.yaml");
    let mut original: serde_yaml::Value = serde_yaml::from_str(VALID_WORKSPACE_YAML).unwrap();
    original["custom"] = serde_yaml::from_str("retained: [one, two]").unwrap();
    original["agents"][1]["customAgent"] = serde_yaml::Value::String("retain me".into());
    std::fs::write(&yaml_path, serde_yaml::to_string(&original).unwrap()).unwrap();
    let first_root = root.path().to_path_buf();
    support::run_in_joined_runtime(move || async move {
        let app = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        let (status, body) = resume_workspace(&app, &first_root).await;
        assert_eq!(status, 201, "{body}");
        let (_, body) = send_empty_request(&app, "GET", "/api/agents").await;
        let roster = roster(&body);
        let anima = &roster.iter().find(|(_, name)| name == "Anima").unwrap().0;
        let scout = &roster.iter().find(|(_, name)| name == "Scout").unwrap().0;
        let (status, body) = send_json_request(
            &app,
            "PATCH",
            &format!("/api/agents/{anima}"),
            r#"{"model":"anima-new","provider":"ollama"}"#,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let after_anima: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
        assert_eq!(after_anima["agents"][0]["model"], "anima-new");
        assert_eq!(after_anima["agents"][0]["provider"], "ollama");
        assert_eq!(after_anima["agents"][1], original["agents"][1]);
        assert_eq!(after_anima["provider"], "moonshot");
        assert_eq!(after_anima["model"], "kimi-k2");
        let inherited = test_app();
        let (status, body) = resume_workspace(&inherited, &first_root).await;
        assert_eq!(status, 201, "{body}");
        let (_, body) = send_empty_request(&inherited, "GET", "/api/agents").await;
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        let inherited_scout = body["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["state"]["name"] == "Scout")
            .unwrap();
        assert_eq!(inherited_scout["state"]["config"]["provider"], "moonshot");
        assert_eq!(inherited_scout["state"]["config"]["model"], "kimi-k2");
        let (status, body) = send_json_request(
            &app,
            "PATCH",
            &format!("/api/agents/{scout}"),
            r#"{"name":"Scout renamed","model":"scout-new","provider":"openai"}"#,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let (status, body) = send_json_request(
            &app,
            "PATCH",
            &format!("/api/agents/{scout}"),
            r#"{"model":"scout-final"}"#,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let updated: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
        assert_eq!(updated["custom"], original["custom"]);
        assert_eq!(updated["agents"][1]["customAgent"], "retain me");
        assert_eq!(updated["agents"][1]["name"], "Scout renamed");
        assert_eq!(updated["agents"][1]["provider"], "openai");
        assert_eq!(updated["agents"][1]["model"], "scout-final");
        drop(app);
    })
    .await;

    let resumed_root = root.path().to_path_buf();
    support::run_in_joined_runtime(move || async move {
        let restarted = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        let fresh = test_app();
        let (status, body) = resume_workspace(&fresh, &resumed_root).await;
        assert_eq!(status, 201, "{body}");
        for instance in [&restarted, &fresh] {
            let (_, body) = send_empty_request(instance, "GET", "/api/agents").await;
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            for (name, model, provider) in [
                ("Anima", "anima-new", "ollama"),
                ("Scout renamed", "scout-final", "openai"),
            ] {
                let agent = body["agents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|agent| agent["state"]["name"] == name)
                    .unwrap();
                assert_eq!(agent["state"]["config"]["model"], model);
                assert_eq!(agent["state"]["config"]["provider"], provider);
            }
        }
    })
    .await;
}

#[tokio::test]
async fn settings_yaml_and_persistence_failures_leave_runtime_unchanged() {
    let root = support::use_temp_workspace_root("settings-yaml-failure");
    let control_plane_path = root.control_plane_path();
    let _guard = EnvVarGuard::set("ANIMAOS_RS_CONTROL_PLANE_FILE", &control_plane_path);
    let workspace_root = root.path().to_path_buf();
    support::run_in_joined_runtime(move || async move {
        let app = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        let (status, body) = send_json_request(
            &app,
            "POST",
            "/api/workspace/bootstrap",
            &bootstrap_body(&workspace_root).to_string(),
        )
        .await;
        assert_eq!(status, 201, "{body}");
        let (_, before) = send_empty_request(&app, "GET", "/api/agents").await;
        let id = roster(&before)
            .into_iter()
            .find(|(_, name)| name == "Anima")
            .unwrap()
            .0;
        let yaml_path = workspace_root.join("anima.yaml");
        let yaml = std::fs::read_to_string(&yaml_path).unwrap();
        for failure in ["malformed", "unwritable", "persistence"] {
            match failure {
                "malformed" => std::fs::write(&yaml_path, "invalid: [").unwrap(),
                "unwritable" => {
                    std::fs::remove_file(&yaml_path).unwrap();
                    std::fs::create_dir(&yaml_path).unwrap();
                }
                _ => {
                    std::fs::remove_file(&control_plane_path).unwrap();
                    std::fs::create_dir(&control_plane_path).unwrap();
                }
            }
            let (status, body) = send_json_request(
                &app,
                "PATCH",
                &format!("/api/agents/{id}"),
                r#"{"model":"must-not-stick","provider":"ollama"}"#,
            )
            .await;
            assert!(
                status.is_client_error() || status.is_server_error(),
                "{failure}: {status} {body}"
            );
            let (_, after) = send_empty_request(&app, "GET", "/api/agents").await;
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&before).unwrap(),
                serde_json::from_str::<serde_json::Value>(&after).unwrap(),
                "{failure}"
            );
            if failure == "unwritable" {
                std::fs::remove_dir(&yaml_path).unwrap();
            }
            if failure == "persistence" {
                assert_eq!(std::fs::read_to_string(&yaml_path).unwrap(), yaml);
            }
            std::fs::write(&yaml_path, &yaml).unwrap();
        }
    })
    .await;
}

#[tokio::test]
async fn settings_concurrent_updates_preserve_both_agents_and_reject_name_collisions() {
    let root = support::use_temp_workspace_root("settings-yaml-concurrency");
    std::fs::write(root.path().join("anima.yaml"), RESUME_RESTORE_YAML).expect("yaml writes");
    let app = test_app();
    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 201, "{body}");
    let (_, before) = send_empty_request(&app, "GET", "/api/agents").await;
    let agents = roster(&before);
    let anima = &agents.iter().find(|(_, name)| name == "Anima").unwrap().0;
    let scout = &agents.iter().find(|(_, name)| name == "Scout").unwrap().0;
    let anima_uri = format!("/api/agents/{anima}");
    let scout_uri = format!("/api/agents/{scout}");
    let (first, second) = tokio::join!(
        send_json_request(
            &app,
            "PATCH",
            &anima_uri,
            r#"{"model":"anima-concurrent","provider":"ollama"}"#
        ),
        send_json_request(
            &app,
            "PATCH",
            &scout_uri,
            r#"{"model":"scout-concurrent","provider":"openai"}"#
        ),
    );
    assert_eq!(first.0, 200, "{}", first.1);
    assert_eq!(second.0, 200, "{}", second.1);
    let yaml_path = root.path().join("anima.yaml");
    let before_collision = std::fs::read_to_string(&yaml_path).unwrap();
    let yaml: serde_yaml::Value = serde_yaml::from_str(&before_collision).unwrap();
    assert_eq!(yaml["agents"][0]["model"], "anima-concurrent");
    assert_eq!(yaml["agents"][1]["model"], "scout-concurrent");
    let (status, body) = send_json_request(
        &app,
        "PATCH",
        &scout_uri,
        r#"{"name":"Anima","model":"collision"}"#,
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(
        std::fs::read_to_string(&yaml_path).unwrap(),
        before_collision
    );
    let (status, body) = send_json_request(&app, "PATCH", &scout_uri, r#"{"provider":""}"#).await;
    assert_eq!(status, 200, "{body}");
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
    assert!(yaml["agents"][1]["provider"].is_null());
    assert_eq!(yaml["agents"][0]["provider"], "ollama");
}

#[tokio::test]
async fn agents_without_their_own_model_inherit_the_workspace_defaults() {
    let root = support::use_temp_workspace_root("legacy-team-provider");
    std::fs::write(root.path().join("anima.yaml"), VALID_WORKSPACE_YAML).unwrap();
    let app = test_app();
    let (status, body) = resume_workspace(&app, root.path()).await;
    assert_eq!(status, 201, "{body}");
    let (_, body) = send_empty_request(&app, "GET", "/api/agents").await;
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    // The steward follows the first agent's model, with the rules behind it.
    for agent in body["agents"].as_array().unwrap() {
        assert_eq!(agent["state"]["config"]["provider"], "moonshot", "{agent}");
        assert_eq!(agent["state"]["config"]["model"], "kimi-k2", "{agent}");
    }
}

#[tokio::test]
async fn inspect_checks_every_effective_provider() {
    let root = support::use_temp_workspace_root("inspect-team-providers");
    let _key = EnvVarGuard::set("MOONSHOT_API_KEY", std::path::Path::new("test-inspect-key"));
    for (first_provider, second_provider, expected) in [
        ("moonshot", "missing-provider", false),
        ("missing-provider", "moonshot", false),
        ("  ", "moonshot", true),
    ] {
        let mut yaml: serde_yaml::Value = serde_yaml::from_str(VALID_WORKSPACE_YAML).unwrap();
        yaml["agents"][0]["provider"] = serde_yaml::Value::String(first_provider.into());
        yaml["agents"][1]["provider"] = serde_yaml::Value::String(second_provider.into());
        std::fs::write(
            root.path().join("anima.yaml"),
            serde_yaml::to_string(&yaml).unwrap(),
        )
        .unwrap();
        let (status, body) =
            send_empty_request(&test_app(), "GET", &inspect_uri(root.path())).await;
        assert_eq!(status, 200, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["providerAvailable"], expected, "{body}");
        if first_provider.trim().is_empty() {
            assert_eq!(body["agents"][0]["provider"], "moonshot");
        }
    }
}

#[tokio::test]
async fn bootstrap_then_resume_on_a_fresh_daemon_keeps_one_steward() {
    let root = support::use_temp_workspace_root("bootstrap-resume-steward");
    let app = test_app();
    let (status, body) = send_json_request(
        &app,
        "POST",
        "/api/workspace/bootstrap",
        &bootstrap_body(root.path()).to_string(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let fresh = test_app();
    let (status, body) = resume_workspace(&fresh, root.path()).await;
    assert_eq!(status, 201, "{body}");
    let resumed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resumed["steward"]["state"]["name"], "Steward", "{resumed}");
    let (_, agents) = send_empty_request(&fresh, "GET", "/api/agents").await;
    assert_eq!(names(&agents), vec!["Anima", "Steward"], "{agents}");
}

#[tokio::test]
async fn bootstrap_validates_the_agent_before_mutation() {
    let root = support::use_temp_workspace_root("bootstrap-invalid");
    for (field, value) in [
        ("name", serde_json::json!(" ")),
        ("system", serde_json::json!(" ")),
        ("bio", serde_json::json!(" ")),
        ("model", serde_json::json!(" ")),
        ("tools", serde_json::json!([])),
        ("presetId", serde_json::json!("missing-preset")),
        ("tools", serde_json::json!(["missing_tool"])),
    ] {
        let app = test_app();
        let candidate = root.path().join(field);
        let mut request = bootstrap_body(&candidate);
        request["agent"][field] = value;
        let (status, body) = send_json_request(
            &app,
            "POST",
            "/api/workspace/bootstrap",
            &request.to_string(),
        )
        .await;
        assert_eq!(status, 400, "{field}: {body}");
        assert!(
            !candidate.exists(),
            "an invalid agent must not create the root"
        );
        let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
        assert_eq!(roster(&agents).len(), 0);
    }
}

#[tokio::test]
async fn bootstrap_survives_restart_and_persist_failure_rolls_back() {
    let workspace = support::use_temp_workspace_root("bootstrap-persist");
    let control_plane_path = workspace.control_plane_path();
    let _guard = EnvVarGuard::set("ANIMAOS_RS_CONTROL_PLANE_FILE", &control_plane_path);
    let root = workspace.path().join("team");
    let first_root = root.clone();
    let first_control_plane = control_plane_path.clone();
    support::run_in_joined_runtime(move || async move {
        let app = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        std::fs::remove_file(&first_control_plane).unwrap();
        std::fs::create_dir(&first_control_plane).unwrap();
        let (status, body) = send_json_request(
            &app,
            "POST",
            "/api/workspace/bootstrap",
            &bootstrap_body(&first_root).to_string(),
        )
        .await;
        assert_eq!(status, 503, "{body}");
        let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
        assert_eq!(roster(&agents).len(), 0);
        assert!(!first_root.join("anima.yaml").exists());
        let (_, body) = send_empty_request(&app, "GET", "/api/workspace").await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["configured"],
            false
        );
        drop(app);
    })
    .await;

    // A replaced payload revokes its owner permanently; recover with a new boot.
    std::fs::remove_dir(&control_plane_path).unwrap();
    let bootstrap_root = root.clone();
    support::run_in_joined_runtime(move || async move {
        let fresh = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        let (status, body) = send_json_request(
            &fresh,
            "POST",
            "/api/workspace/bootstrap",
            &bootstrap_body(&bootstrap_root).to_string(),
        )
        .await;
        assert_eq!(status, 201, "{body}");
        drop(fresh);
    })
    .await;

    support::run_in_joined_runtime(|| async {
        let restored = support::configured_loopback_test_app(DaemonConfig::default())
            .await
            .unwrap();
        let (_, agents) = send_empty_request(&restored, "GET", "/api/agents").await;
        assert_eq!(names(&agents), vec!["Anima", "Steward"], "{agents}");
    })
    .await;
}

#[tokio::test]
async fn concurrent_bootstraps_create_one_workspace() {
    let root = support::use_temp_workspace_root("bootstrap-race");
    let app = test_app();
    let request = bootstrap_body(root.path()).to_string();
    let (first, second) = tokio::join!(
        send_json_request(&app, "POST", "/api/workspace/bootstrap", &request),
        send_json_request(&app, "POST", "/api/workspace/bootstrap", &request)
    );
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [201, 409], "{first:?} {second:?}");
    let (_, agents) = send_empty_request(&app, "GET", "/api/agents").await;
    assert_eq!(names(&agents), vec!["Anima", "Steward"], "{agents}");
}
