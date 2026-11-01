mod support;

use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn general_skill_setup_capabilities_offer_inspection_import_and_documented_commands() {
    let app = support::test_app();
    let (status, body) = support::send_empty_request(&app, "GET", "/api/capabilities").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    let tools = body["tools"].as_array().unwrap();
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "import_github_skill"));
    for name in ["inspect_skill_source", "import_skill", "manage_skill_setup"] {
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "the runtime must offer {name} for source-driven setup beyond the legacy GitHub/Python path"
        );
    }
}
