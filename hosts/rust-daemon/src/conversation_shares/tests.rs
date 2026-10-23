use super::*;
use crate::sessions::{SessionOrigin, TitleSource};

fn session() -> SessionRecord {
    SessionRecord::new(
        "agent",
        "chat:one",
        SessionKind::Chat,
        SessionOrigin::Web,
        "One".into(),
        TitleSource::Owner,
        10,
    )
}

#[test]
fn credentials_are_independent_of_link_and_only_digests_are_persisted() {
    let mut record = session();
    let (grant, secret) = issue(&record, "Colleague", 20).unwrap();
    assert_eq!(secret.len(), 64);
    assert!(secret.bytes().all(|b| b.is_ascii_hexdigit()));
    let encoded = serde_json::to_string(&grant).unwrap();
    assert!(!encoded.contains(&secret));
    assert!(!serde_json::to_string(&grant.view(&record))
        .unwrap()
        .contains("sha256"));
    record.conversation_shares.push(grant.clone());
    assert!(grant.authorizes(&record, &grant.viewer_id, &secret, 21));
    assert!(!grant.authorizes(&record, &grant.viewer_id, &grant.id, 21));
    assert!(!grant.authorizes(&record, "another-viewer", &secret, 21));
    assert!(!grant.authorizes(&record, &grant.viewer_id, &"0".repeat(64), 21));
    let restored: SessionRecord =
        serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
    assert!(restored.conversation_shares[0].authorizes(&restored, &grant.viewer_id, &secret, 21));
}

#[test]
fn revoke_clear_and_replacement_end_access() {
    let record = session();
    let (mut grant, secret) = issue(&record, "Viewer", 20).unwrap();
    let mut cleared = record.clone();
    cleared.clear_generation += 1;
    assert!(!grant.authorizes(&cleared, &grant.viewer_id, &secret, 21));
    let mut replacement = record.clone();
    replacement.created_at_ms += 1;
    assert!(!grant.authorizes(&replacement, &grant.viewer_id, &secret, 21));
    let mut other = record.clone();
    other.id = "chat:another".into();
    assert!(!grant.authorizes(&other, &grant.viewer_id, &secret, 21));
    let mut other_agent = record.clone();
    other_agent.agent_id = "another-agent".into();
    assert!(!grant.authorizes(&other_agent, &grant.viewer_id, &secret, 21));
    grant.revoked_at_ms = Some(21);
    assert!(!grant.authorizes(&record, &grant.viewer_id, &secret, 21));
}

#[test]
fn old_unbound_grants_restore_compatibly_and_fail_closed() {
    let record = session();
    let (grant, secret) = issue(&record, "Viewer", 20).unwrap();
    let mut saved = serde_json::to_value(&grant).unwrap();
    saved.as_object_mut().unwrap().remove("agentId");
    saved.as_object_mut().unwrap().remove("sessionId");
    let old: ConversationShareGrant = serde_json::from_value(saved).unwrap();
    assert!(!old.authorizes(&record, &old.viewer_id, &secret, 21));
}

#[test]
fn projection_omits_tools_system_attachments_and_all_metadata() {
    use crate::sessions::test_support::message;
    let mut user = message("agent", "one", "chat:one", MessageRole::User, "Question", 1);
    user.content.metadata = Some(std::collections::BTreeMap::from([(
        "toolCalls".into(),
        anima_core::DataValue::String("secret arguments".into()),
    )]));
    let projected = project_message(&PageMessage {
        message: user,
        hidden: false,
    })
    .unwrap();
    let value = serde_json::to_value(projected).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 4);
    assert_eq!(value["text"], "Question");
    for role in [MessageRole::Tool, MessageRole::System] {
        assert!(project_message(&PageMessage {
            message: message("agent", "hidden", "chat:one", role, "private effects", 2),
            hidden: false
        })
        .is_none());
    }
    assert!(project_message(&PageMessage {
        message: message(
            "agent",
            "hidden",
            "chat:one",
            MessageRole::Assistant,
            "silent",
            2
        ),
        hidden: true
    })
    .is_none());
}

#[test]
fn projection_omits_revised_drafts_and_preserves_the_final_answer() {
    use crate::sessions::test_support::message;
    let mut draft = message(
        "agent",
        "draft",
        "chat:one",
        MessageRole::Assistant,
        "Rejected private draft",
        1,
    );
    draft.content.metadata = Some(std::collections::BTreeMap::from([(
        anima_core::REVISED_METADATA_KEY.into(),
        anima_core::DataValue::Bool(true),
    )]));
    assert!(project_message(&PageMessage {
        message: draft,
        hidden: false
    })
    .is_none());
    let final_answer = project_message(&PageMessage {
        message: message(
            "agent",
            "final",
            "chat:one",
            MessageRole::Assistant,
            "Final answer",
            2,
        ),
        hidden: false,
    })
    .unwrap();
    assert_eq!(
        serde_json::to_value(final_answer).unwrap()["text"],
        "Final answer"
    );
}
