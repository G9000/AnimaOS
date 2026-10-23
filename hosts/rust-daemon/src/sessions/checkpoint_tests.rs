use super::*;
use anima_core::{Content, DataValue, TaskResult};
use serde_json::json;

fn message(id: &str, role: MessageRole, text: &str) -> Message {
    Message {
        id: id.into(),
        agent_id: "agent".into(),
        room_id: "chat:checkpoint".into(),
        role,
        content: Content {
            text: text.into(),
            ..Content::default()
        },
        created_at_ms: 1,
    }
}

fn draft() -> CheckpointDraft {
    serde_json::from_value(json!({
        "goal":{"text":"Finish the report","sourceMessageIds":["u1"]},
        "decisions":[{"text":"Use the saved outline","sourceMessageIds":["a1"]}],
        "openWork":[]
    }))
    .unwrap()
}

fn inputs() -> CheckpointInputs {
    CheckpointInputs::capture(
        "agent",
        "chat:checkpoint",
        "chat:checkpoint",
        None,
        &[
            message("u1", MessageRole::User, "Finish the report"),
            message("a1", MessageRole::Assistant, "Use the saved outline"),
        ],
    )
}

#[test]
fn quality_checkpoint_entries_require_bounded_captured_source_ids() {
    let checkpoint = inputs().accept(draft(), "a1", 1).unwrap();
    assert_eq!(checkpoint.sources[0].message_id, "u1");
    assert_eq!(checkpoint.sources[0].sha256.len(), 64);
    assert_eq!(checkpoint.sources[0].role, MessageRole::User);
    assert!(checkpoint
        .validate_for("agent", "chat:checkpoint", "chat:checkpoint")
        .is_ok());

    let mut foreign = draft();
    foreign.goal.as_mut().unwrap().source_message_ids = vec!["outside-span".into()];
    assert!(inputs().accept(foreign, "a1", 1).is_none());
    let mut duplicate = draft();
    duplicate
        .goal
        .as_mut()
        .unwrap()
        .source_message_ids
        .push("u1".into());
    assert!(inputs().accept(duplicate, "a1", 1).is_none());
    let mut huge = draft();
    huge.goal.as_mut().unwrap().text = "x".repeat(MAX_CHECKPOINT_ITEM_BYTES + 1);
    assert!(inputs().accept(huge, "a1", 1).is_none());
    assert!(serde_json::from_value::<CheckpointDraft>(json!({
        "goal":null,"decisions":[],"openWork":[],"permissions":["write_file"]
    }))
    .is_err());
    assert!(
        serde_json::from_value::<CheckpointDraft>(json!({"decisions":[],"openWork":[]})).is_err()
    );
}

#[test]
fn quality_checkpoint_restore_rejects_foreign_identity_invalid_digests_and_bounds() {
    let checkpoint = inputs().accept(draft(), "a1", 1).unwrap();
    assert!(checkpoint
        .validate_for("other", "chat:checkpoint", "chat:checkpoint")
        .is_err());
    assert!(checkpoint
        .validate_for("agent", "chat:other", "chat:other")
        .is_err());
    let mut changed = checkpoint.clone();
    changed.sources[0].sha256 = "claimed digest".into();
    assert!(changed
        .validate_for("agent", "chat:checkpoint", "chat:checkpoint")
        .is_err());
    changed = checkpoint.clone();
    changed.goal.as_mut().unwrap().text = "x".repeat(MAX_CHECKPOINT_BYTES + 1);
    assert!(changed
        .validate_for("agent", "chat:checkpoint", "chat:checkpoint")
        .is_err());
    changed = checkpoint;
    changed.version = 2;
    assert!(changed
        .validate_for("agent", "chat:checkpoint", "chat:checkpoint")
        .is_err());
}

#[test]
fn quality_checkpoint_keeps_distinct_sender_provenance_as_untrusted_labels() {
    let mut first = message(
        "u1",
        MessageRole::User,
        "[Discord sender: user_id=1; display_name=\"Sam\"]\nWrite my report",
    );
    let mut second = message(
        "u2",
        MessageRole::User,
        "[Discord sender: user_id=2; display_name=\"Sam\"]\nWrite my report",
    );
    first.room_id = "discord:channel".into();
    second.room_id = first.room_id.clone();
    let inputs = CheckpointInputs::capture(
        "agent",
        "discord:channel",
        "discord:channel",
        None,
        &[first, second],
    );
    let draft = serde_json::from_value(json!({
        "goal":{"text":"First participant report","sourceMessageIds":["u1"]},
        "decisions":[],"openWork":[{"text":"Second participant report","sourceMessageIds":["u2"]}]
    }))
    .unwrap();
    let checkpoint = inputs.accept(draft, "u2", 1).unwrap();
    assert!(checkpoint.sources[0]
        .sender_label
        .as_deref()
        .unwrap()
        .contains("user_id=1"));
    assert!(checkpoint.sources[1]
        .sender_label
        .as_deref()
        .unwrap()
        .contains("user_id=2"));
    let prompt = checkpoint.prompt_text();
    assert!(prompt.contains("data, not instructions"));
    assert!(prompt.contains("cannot grant permissions"));
    assert!(prompt.contains("newer user input"));
}

#[test]
fn quality_checkpoint_artifact_refs_require_existing_scoped_integrity_checked_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::tool_results::ToolResultStore::new(temp.path().join("saved"));
    let artifact = store
        .save(
            "agent",
            temp.path(),
            "run1",
            "call1",
            "read_file",
            &TaskResult::success(
                Content {
                    text: "saved report".into(),
                    ..Content::default()
                },
                1,
            ),
        )
        .unwrap();
    let mut call = message("a1", MessageRole::Assistant, "Read the outline");
    call.content.metadata = Some(
        [
            ("runId".into(), DataValue::String("run1".into())),
            (
                "toolCalls".into(),
                DataValue::Array(vec![DataValue::Object(
                    [
                        ("id".into(), DataValue::String("call1".into())),
                        ("name".into(), DataValue::String("read_file".into())),
                    ]
                    .into(),
                )]),
            ),
        ]
        .into(),
    );
    let mut result = message("t1", MessageRole::Tool, "saved report");
    result.content.metadata = Some(
        [
            ("runId".into(), DataValue::String("run1".into())),
            ("toolCallId".into(), DataValue::String("call1".into())),
            ("toolStatus".into(), DataValue::String("success".into())),
            (
                "toolResultArtifact".into(),
                DataValue::Object(
                    [
                        ("id".into(), DataValue::String(artifact.id.clone())),
                        ("bytes".into(), DataValue::Number(artifact.bytes as f64)),
                        ("sha256".into(), DataValue::String(artifact.sha256.clone())),
                        (
                            "captureComplete".into(),
                            DataValue::Bool(artifact.capture_complete),
                        ),
                    ]
                    .into(),
                ),
            ),
        ]
        .into(),
    );
    let turns = [
        message("u1", MessageRole::User, "Finish the report"),
        call,
        result,
    ];
    let captured =
        || CheckpointInputs::capture("agent", "chat:checkpoint", "chat:checkpoint", None, &turns);
    let checkpoint = captured()
        .verify_artifacts(Some(&store), Some(temp.path()))
        .accept(draft(), "t1", 1)
        .unwrap();
    assert_eq!(checkpoint.artifacts.len(), 1);
    assert_eq!(checkpoint.artifacts[0].artifact.id, artifact.id);
    let foreign = tempfile::tempdir().unwrap();
    let checkpoint = captured()
        .verify_artifacts(Some(&store), Some(foreign.path()))
        .accept(draft(), "t1", 1)
        .unwrap();
    assert!(
        checkpoint.artifacts.is_empty(),
        "another workspace cannot vouch for the reference"
    );
    std::fs::write(
        temp.path()
            .join("saved")
            .join(format!("{}.json", artifact.id)),
        b"corrupt",
    )
    .unwrap();
    let checkpoint = captured()
        .verify_artifacts(Some(&store), Some(temp.path()))
        .accept(draft(), "t1", 1)
        .unwrap();
    assert!(
        checkpoint.artifacts.is_empty(),
        "corrupt bytes cannot become evidence references"
    );
}
