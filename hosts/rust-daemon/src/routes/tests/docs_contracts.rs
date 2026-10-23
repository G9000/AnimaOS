use super::*;
use std::collections::BTreeSet;
use utoipa::OpenApi;

const README: &str = include_str!("../../../README.md");
const START: &str = "<!-- openapi-route-inventory:start -->";
const END: &str = "<!-- openapi-route-inventory:end -->";

fn inventory() -> BTreeSet<(String, String)> {
    let document = serde_json::to_value(crate::routes::ApiDoc::openapi()).unwrap();
    let mut entries = BTreeSet::new();
    for (path, item) in document["paths"].as_object().unwrap() {
        for method in ["get", "post", "put", "patch", "delete", "head", "options"] {
            if item.get(method).is_some() {
                entries.insert((method.to_uppercase(), path.clone()));
            }
        }
    }
    entries
}

#[test]
fn docs_contracts_readme_inventory_matches_actual_openapi() {
    let table = README
        .split_once(START)
        .expect("inventory start")
        .1
        .split_once(END)
        .expect("inventory end")
        .0;
    let documented: BTreeSet<_> = table
        .lines()
        .filter_map(|line| {
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            if cells.len() < 4 || !cells[1].starts_with('`') {
                return None;
            }
            Some((
                cells[1].trim_matches('`').to_string(),
                cells[2].trim_matches('`').to_string(),
            ))
        })
        .collect();
    let actual = inventory();
    assert_eq!(actual.len(), 290, "complete registered operation count");
    let runtime_capability_additions = [
        ("GET", "/api/doctor"),
        ("GET", "/api/doctor/memory-index"),
        ("POST", "/api/doctor/memory-index"),
        ("GET", "/api/agents/{agent_id}/doctor/repairs"),
        ("POST", "/api/agents/{agent_id}/doctor/repairs"),
        ("POST", "/api/agents/{agent_id}/doctor/undo"),
        ("GET", "/api/agents/{agent_id}/standing-intents"),
        ("POST", "/api/agents/{agent_id}/standing-intents"),
        ("PUT", "/api/agents/{agent_id}/standing-intents/{intent_id}"),
        (
            "POST",
            "/api/agents/{agent_id}/standing-intents/{intent_id}/cancel",
        ),
        ("POST", "/api/agents/{agent_id}/standing-intents/undo"),
        ("GET", "/api/agents/{agent_id}/paired-devices"),
        ("POST", "/api/agents/{agent_id}/paired-devices"),
        (
            "POST",
            "/api/agents/{agent_id}/paired-devices/{device_id}/revoke",
        ),
    ];
    for (method, path) in runtime_capability_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "runtime capability operation must be registered"
        );
    }
    let browser_profile_additions = [
        ("GET", "/api/agents/{agent_id}/browser-profiles"),
        ("POST", "/api/agents/{agent_id}/browser-profiles"),
        (
            "POST",
            "/api/agents/{agent_id}/browser-profiles/{profile_id}/sign-in/start",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/browser-profiles/{profile_id}/sign-in/finish",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/browser-profiles/{profile_id}/revoke",
        ),
    ];
    for (method, path) in browser_profile_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "browser profile operation must be registered"
        );
    }
    let maintenance_additions = [
        ("GET", "/api/agents/{agent_id}/memory-maintenance/policy"),
        ("PATCH", "/api/agents/{agent_id}/memory-maintenance/policy"),
        (
            "GET",
            "/api/agents/{agent_id}/memory-maintenance/operations",
        ),
        ("POST", "/api/agents/{agent_id}/memory-maintenance/sources"),
        ("POST", "/api/agents/{agent_id}/memory-maintenance/review"),
        (
            "PATCH",
            "/api/agents/{agent_id}/memory-maintenance/operations/{operation_id}",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/memory-maintenance/operations/{operation_id}/undo",
        ),
    ];
    for (method, path) in maintenance_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "memory maintenance operation must be registered"
        );
    }
    let skill_learning_additions = [
        ("GET", "/api/skills/{slug}/evaluation-suite"),
        ("PUT", "/api/skills/{slug}/evaluation-suite"),
        ("GET", "/api/skill-drafts/{draft_id}/evaluation"),
        ("POST", "/api/skill-drafts/{draft_id}/evaluation"),
        ("POST", "/api/skill-drafts/{draft_id}/evaluation/cancel"),
        ("GET", "/api/skills/{slug}/learning"),
        ("POST", "/api/skills/{slug}/feedback"),
    ];
    for (method, path) in skill_learning_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "skill learning operations must be registered"
        );
    }
    let reusable_additions = [
        ("GET", "/api/agents/{agent_id}/workflow-templates"),
        ("POST", "/api/agents/{agent_id}/workflow-templates"),
        ("GET", "/api/agents/{agent_id}/workflow-templates/{slug}"),
        ("DELETE", "/api/agents/{agent_id}/workflow-templates/{slug}"),
        (
            "POST",
            "/api/agents/{agent_id}/workflow-templates/{slug}/run",
        ),
        ("GET", "/api/agents/{agent_id}/tool-results/{artifact_id}"),
        (
            "GET",
            "/api/agents/{agent_id}/tool-results/{artifact_id}/download",
        ),
    ];
    for (method, path) in reusable_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "reusable work operations must be registered"
        );
    }
    let workflow_additions = [
        ("GET", "/api/agents/{agent_id}/workflows"),
        ("POST", "/api/agents/{agent_id}/workflows"),
        ("GET", "/api/agents/{agent_id}/workflows/{workflow_id}"),
        (
            "POST",
            "/api/agents/{agent_id}/workflows/{workflow_id}/resume",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/workflows/{workflow_id}/cancel",
        ),
    ];
    for (method, path) in workflow_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "saved workflow operations must be registered"
        );
    }
    let durable_work_additions = [
        ("GET", "/api/agents/{agent_id}/runs/{run_id}/recovery"),
        ("POST", "/api/agents/{agent_id}/runs/{run_id}/recovery"),
        (
            "GET",
            "/api/agents/{agent_id}/sessions/{session_id}/clarifications",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/sessions/{session_id}/clarifications/{request_id}/answer",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/sessions/{session_id}/side-question",
        ),
        ("POST", "/api/agents/{agent_id}/automation-events"),
        (
            "GET",
            "/api/agents/{agent_id}/schedules/{schedule_id}/status",
        ),
    ];
    for (method, path) in durable_work_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "recovery, clarification, side question and automation event operations must be registered"
        );
    }
    let discord_interaction_additions = [
        ("POST", "/api/agents/{agent_id}/connectors/discord/check"),
        (
            "PUT",
            "/api/agents/{agent_id}/connectors/discord/{connector_id}/settings",
        ),
    ];
    for (method, path) in discord_interaction_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "Discord bot check and interaction settings must be registered"
        );
    }
    let message_search_additions = [("GET", "/api/agents/{agent_id}/message-search")];
    for (method, path) in message_search_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "individual message search must be registered"
        );
    }
    let main_chat_additions = [("POST", "/api/agents/{agent_id}/main-chat")];
    for (method, path) in main_chat_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "main chat resolution must be registered"
        );
    }
    let report_additions = [("GET", "/api/workspace/file/download")];
    for (method, path) in report_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "explicit workspace report download operation must be registered"
        );
    }
    let credential_additions = [
        (
            "PUT",
            "/api/agents/{agent_id}/mcp-connections/{connection_id}/credential",
        ),
        (
            "DELETE",
            "/api/agents/{agent_id}/mcp-connections/{connection_id}/credential",
        ),
    ];
    for (method, path) in credential_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "explicit MCP credential operation must be registered"
        );
    }
    let speech_additions = [
        (
            "GET",
            "/api/agents/{agent_id}/sessions/{session_id}/voice/speech",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/sessions/{session_id}/voice/speech",
        ),
        (
            "DELETE",
            "/api/agents/{agent_id}/sessions/{session_id}/voice/speech/{operation_id}",
        ),
    ];
    for (method, path) in speech_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "explicit speech operation must be registered"
        );
    }
    let side_model_additions = [
        ("GET", "/api/settings/side-model"),
        ("PUT", "/api/settings/side-model"),
    ];
    for (method, path) in side_model_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "side-task model operation must be registered"
        );
    }
    let provider_key_additions = [
        ("GET", "/api/provider-keys"),
        ("POST", "/api/provider-keys/{provider}"),
        ("DELETE", "/api/provider-keys/{provider}/{key_id}"),
        ("PUT", "/api/provider-keys/{provider}/strategy"),
        ("POST", "/api/provider-keys/{provider}/{key_id}/wake"),
    ];
    for (method, path) in provider_key_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "provider key pool operation must be registered"
        );
    }
    let system_additions = [
        ("GET", "/api/system/drain"),
        ("POST", "/api/system/drain"),
        ("DELETE", "/api/system/drain"),
        ("POST", "/api/system/shutdown"),
        ("GET", "/api/system"),
        ("POST", "/api/system/actions/{action}"),
        ("POST", "/api/system/session"),
    ];
    for (method, path) in system_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "system operation must be registered"
        );
    }
    let autonomy_additions = [
        ("GET", "/api/agents/{agent_id}/autonomy"),
        ("PUT", "/api/agents/{agent_id}/autonomy"),
    ];
    let learning_undo_additions = [(
        "POST",
        "/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}/undo",
    )];
    for (method, path) in learning_undo_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "Free-mode lesson undo must be registered"
        );
    }
    let skill_ledger_additions = [
        ("GET", "/api/skill-ledger"),
        ("POST", "/api/skills/{slug}/revert-last"),
    ];
    for (method, path) in skill_ledger_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "skill ledger operation must be registered"
        );
    }
    let coding_additions = [
        ("GET", "/api/agents/{agent_id}/coding-keys"),
        ("PUT", "/api/agents/{agent_id}/coding-keys"),
        (
            "POST",
            "/api/agents/{agent_id}/app-sessions/{session_id}/stop",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/acp-sessions/{session_id}/stop",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/acp-sessions/{session_id}/cancel",
        ),
        ("GET", "/api/coding-apps"),
    ];
    for (method, path) in coding_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "coding app operation must be registered"
        );
    }
    // These replace the four memory-view operations, removed when pins folded
    // into core memory; the pre-M58 and pre-M49 counts below drop by four.
    let core_memory_additions = [
        ("GET", "/api/agents/{agent_id}/core-memory"),
        ("POST", "/api/agents/{agent_id}/core-memory/{block}"),
        (
            "PATCH",
            "/api/agents/{agent_id}/core-memory/{block}/{entry_id}",
        ),
        (
            "DELETE",
            "/api/agents/{agent_id}/core-memory/{block}/{entry_id}",
        ),
    ];
    for (method, path) in core_memory_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "core memory operation must be registered"
        );
    }
    let reminder_additions = [
        (
            "POST",
            "/api/agents/{agent_id}/schedules/{schedule_id}/done",
        ),
        (
            "POST",
            "/api/agents/{agent_id}/schedules/{schedule_id}/snooze",
        ),
    ];
    for (method, path) in reminder_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "reminder done and snooze operations must be registered"
        );
    }
    let memory_export_additions = [("GET", "/api/memories/export")];
    for (method, path) in memory_export_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "memory export must be registered"
        );
    }
    let import_additions = [
        ("GET", "/api/foreign-sessions"),
        ("GET", "/api/foreign-sessions/{handle}/preview"),
        (
            "POST",
            "/api/agents/{agent_id}/foreign-sessions/{handle}/import",
        ),
    ];
    for (method, path) in import_additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "foreign session import operation must be registered"
        );
    }
    assert_eq!(
        actual
            .iter()
            .filter(|(method, path)| {
                !skill_learning_additions.contains(&(method.as_str(), path.as_str()))
                    && !maintenance_additions.contains(&(method.as_str(), path.as_str()))
                    && !browser_profile_additions.contains(&(method.as_str(), path.as_str()))
                    && !runtime_capability_additions.contains(&(method.as_str(), path.as_str()))
                    && !reusable_additions.contains(&(method.as_str(), path.as_str()))
                    && !workflow_additions.contains(&(method.as_str(), path.as_str()))
                    && !coding_additions.contains(&(method.as_str(), path.as_str()))
                    && !autonomy_additions.contains(&(method.as_str(), path.as_str()))
                    && !side_model_additions.contains(&(method.as_str(), path.as_str()))
                    && !provider_key_additions.contains(&(method.as_str(), path.as_str()))
                    && !system_additions.contains(&(method.as_str(), path.as_str()))
                    && !skill_ledger_additions.contains(&(method.as_str(), path.as_str()))
                    && !learning_undo_additions.contains(&(method.as_str(), path.as_str()))
                    && !import_additions.contains(&(method.as_str(), path.as_str()))
                    && !speech_additions.contains(&(method.as_str(), path.as_str()))
                    && !message_search_additions.contains(&(method.as_str(), path.as_str()))
                    && !credential_additions.contains(&(method.as_str(), path.as_str()))
                    && !report_additions.contains(&(method.as_str(), path.as_str()))
                    && !main_chat_additions.contains(&(method.as_str(), path.as_str()))
                    && !core_memory_additions.contains(&(method.as_str(), path.as_str()))
                    && !memory_export_additions.contains(&(method.as_str(), path.as_str()))
                    && !discord_interaction_additions.contains(&(method.as_str(), path.as_str()))
                    && !durable_work_additions.contains(&(method.as_str(), path.as_str()))
                    && !reminder_additions.contains(&(method.as_str(), path.as_str()))
            })
            .count(),
        193,
        "all pre-M58 operations remain present"
    );
    let additions = [
        ("GET", "/api/providers/chatgpt/models"),
        ("POST", "/api/providers/chatgpt/models/refresh"),
        ("GET", "/api/agents/{agent_id}/sessions/{session_id}/voice"),
        (
            "POST",
            "/api/agents/{agent_id}/sessions/{session_id}/voice/transcriptions",
        ),
        (
            "DELETE",
            "/api/agents/{agent_id}/sessions/{session_id}/voice/transcriptions/{operation_id}",
        ),
    ];
    for (method, path) in additions {
        assert!(
            actual.contains(&(method.to_string(), path.to_string())),
            "model metadata and explicit voice operations must be registered"
        );
    }
    assert_eq!(
        actual
            .iter()
            .filter(|(method, path)| !skill_learning_additions
                .contains(&(method.as_str(), path.as_str()))
                && !reusable_additions.contains(&(method.as_str(), path.as_str()))
                && !workflow_additions.contains(&(method.as_str(), path.as_str()))
                && !additions.contains(&(method.as_str(), path.as_str()))
                && !coding_additions.contains(&(method.as_str(), path.as_str()))
                && !autonomy_additions.contains(&(method.as_str(), path.as_str()))
                && !side_model_additions.contains(&(method.as_str(), path.as_str()))
                && !provider_key_additions.contains(&(method.as_str(), path.as_str()))
                && !system_additions.contains(&(method.as_str(), path.as_str()))
                && !skill_ledger_additions.contains(&(method.as_str(), path.as_str()))
                && !learning_undo_additions.contains(&(method.as_str(), path.as_str()))
                && !import_additions.contains(&(method.as_str(), path.as_str()))
                && !message_search_additions.contains(&(method.as_str(), path.as_str()))
                && !speech_additions.contains(&(method.as_str(), path.as_str()))
                && !credential_additions.contains(&(method.as_str(), path.as_str()))
                && !report_additions.contains(&(method.as_str(), path.as_str()))
                && !main_chat_additions.contains(&(method.as_str(), path.as_str()))
                && !core_memory_additions.contains(&(method.as_str(), path.as_str()))
                && !memory_export_additions.contains(&(method.as_str(), path.as_str()))
                && !discord_interaction_additions.contains(&(method.as_str(), path.as_str()))
                && !durable_work_additions.contains(&(method.as_str(), path.as_str()))
                && !reminder_additions.contains(&(method.as_str(), path.as_str()))
                && !maintenance_additions.contains(&(method.as_str(), path.as_str()))
                && !browser_profile_additions.contains(&(method.as_str(), path.as_str()))
                && !runtime_capability_additions.contains(&(method.as_str(), path.as_str())))
            .count(),
        188,
        "pre-M49 registered operations remain present"
    );
    assert_eq!(
        documented, actual,
        "Regenerate the marked README table from ApiDoc::openapi(); aliases belong outside it."
    );
}

#[test]
#[ignore = "Print sorted OpenAPI README rows for explicit documentation regeneration"]
fn docs_contracts_print_inventory() {
    for (method, path) in inventory() {
        println!("| `{method}` | `{path}` |");
    }
}

fn owner_console_path(path: &str) -> bool {
    path == "/api/status"
        || path.starts_with("/api/logs")
        || path.starts_with("/api/usage")
        || path.starts_with("/api/pricing")
        || path.starts_with("/api/settings")
        || path.starts_with("/api/skills")
        || (path.starts_with("/api/agents/")
            && [
                "/sessions",
                "/message-search",
                "/events",
                "/automation-events",
                "/runs",
                "/approvals",
                "/approval-policy",
                "/approval-rules",
                "/agent-files",
                "/learning",
            ]
            .iter()
            .any(|part| path.contains(part)))
}

#[tokio::test]
async fn docs_contracts_console_operations_are_mounted_behind_owner_boundary() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state, DaemonConfig::default());
    let mut checked = 0;
    for (method, path) in inventory()
        .into_iter()
        .filter(|(_, path)| owner_console_path(path))
    {
        let mut concrete = String::new();
        let mut placeholder = false;
        for character in path.chars() {
            match character {
                '{' => {
                    placeholder = true;
                    concrete.push_str("missing");
                }
                '}' => placeholder = false,
                _ if !placeholder => concrete.push(character),
                _ => {}
            }
        }
        let request = Request::builder()
            .method(method.as_str())
            .uri(&concrete)
            .header("host", "127.0.0.1:8080")
            .header("origin", "https://foreign.example.invalid")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {path} must reach a mounted owner guard before reading/mutating"
        );
        checked += 1;
    }
    assert!(checked >= 25, "console inventory unexpectedly shrank");
}
