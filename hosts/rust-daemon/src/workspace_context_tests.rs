use super::*;
use serde_json::json;

fn binding() -> WorkspaceContextBinding {
    WorkspaceContextBinding {
        connection_id: "connection".into(),
        tool: "read_context".into(),
        mode: ContextMode::Shared,
        workspace_id: "workspace".into(),
        generation: "generation".into(),
        subject: "agent".into(),
    }
}
fn envelope(value: Value) -> String {
    json!({"isError":false,"shortened":false,"omittedBlocks":0,"structuredData":value}).to_string()
}
#[test]
fn workspace_context_shared_uses_reference_only_and_rejects_persona_and_oversize() {
    let binding = binding();
    let mut value = json!({"access":{"version":1,"mode":"shared","subject":"agent"},"workspace":{"id":"workspace","generation":"generation"},"records":[]});
    let parsed = binding.parse(&envelope(value.clone())).unwrap();
    assert!(parsed.identity.is_none());
    value["access"]["version"] = json!(1.0);
    assert!(
        binding.parse(&envelope(value.clone())).is_ok(),
        "MCP DataValue numeric bridge emits version1.0"
    );
    value["persona"] = json!({"content":"forbidden"});
    assert!(binding.parse(&envelope(value)).is_err());
    assert!(binding.parse(&"x".repeat(MAX_ENVELOPE + 1)).is_err());
}
#[test]
fn workspace_context_missing_version_and_unbounded_subject_cannot_activate() {
    let mut binding = binding();
    assert!(binding.validate("agent").is_ok());
    binding.subject = "foreign".into();
    assert!(binding.validate("agent").is_err());
    binding.subject = "agent".into();
    let value = json!({"access":{"mode":"shared","subject":"agent"},"workspace":{"id":"workspace","generation":"generation"}});
    assert!(binding.parse(&envelope(value)).is_err());
}
