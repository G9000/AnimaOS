use super::*;
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use serde_json::json;

fn session() -> SessionRecord {
    SessionRecord::new(
        "agent",
        "chat:results",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Chat".into(),
        TitleSource::Owner,
        1,
    )
}
fn input(key: &str) -> SaveResultInput {
    serde_json::from_value(json!({"requestKey":key,"expectedRevision":0,"title":"Report","payload":{"kind":"document","format":"text","text":"Safe <script> text"}})).unwrap()
}

#[test]
fn stable_results_retry_update_and_pin_with_revision_conflicts() {
    let mut room = session();
    let first = upsert(&mut room, input("create"), "run", "call", 2).unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(
        first,
        upsert(&mut room, input("create"), "run", "replayed-call", 3).unwrap()
    );
    assert_eq!(room.chat_results.len(), 1);
    let pinned = pin(&mut room, &first.id, 1, true, 4).unwrap();
    let mut change = input("edit");
    change.id = Some(first.id.clone());
    change.expected_revision = 2;
    change.title = "Updated report".into();
    let updated = upsert(&mut room, change.clone(), "next-run", "next-call", 5).unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.revision, 3);
    assert!(updated.pinned);
    assert_eq!(updated.created_at_ms, 2);
    assert_eq!(
        updated,
        upsert(&mut room, change, "next-run", "retry", 6).unwrap()
    );
    assert!(upsert(&mut room, input("create"), "run", "late-create-retry", 6).is_err());
    assert_eq!(room.chat_results.len(), 1);
    assert!(pin(&mut room, &first.id, pinned.revision, false, 6).is_err());
    assert!(validate_session(&room).is_ok());
    let restored: SessionRecord =
        serde_json::from_value(serde_json::to_value(room).unwrap()).unwrap();
    assert_eq!(restored.chat_results, vec![updated]);
}

#[test]
fn result_payloads_are_closed_bounded_and_bound_to_the_conversation_generation() {
    let mut room = session();
    let result = upsert(&mut room, input("create"), "run", "call", 2).unwrap();
    room.clear_generation = 1;
    assert!(validate_session(&room).is_err());
    assert!(pin(&mut room, &result.id, 1, true, 3).is_err());
    for payload in [
        json!({"kind":"html","html":"<script>fetch('/api')</script>"}),
        json!({"kind":"document","format":"html","text":"<b>no</b>"}),
        json!({"kind":"document","format":"text","text":"ok","script":"bad"}),
        json!({"kind":"calculator","inputs":[{"key":"x","label":"X","value":1}],"outputLabel":"Total","expression":{"op":"call","name":"eval"}}),
    ] {
        assert!(serde_json::from_value::<ResultPayload>(payload).is_err());
    }
    let invalid: ResultPayload = serde_json::from_value(json!({"kind":"chart","chartType":"bar","labels":["A"],"series":[{"label":"Series","values":[1,2]}]})).unwrap();
    assert!(invalid.validate().is_err());
    let mut oversized = input("large");
    oversized.payload = ResultPayload::Document {
        format: DocumentFormat::Text,
        text: "a".repeat(MAX_RESULT_BYTES + 1),
    };
    assert!(upsert(&mut session(), oversized, "run", "call", 3).is_err());
}

#[test]
fn calculator_references_and_depth_are_validated_without_executable_code() {
    let valid = json!({"kind":"calculator","inputs":[{"key":"price","label":"Price","value":10,"min":0},{"key":"count","label":"Count","value":2}],"outputLabel":"Total","expression":{"op":"multiply","left":{"op":"input","key":"price"},"right":{"op":"input","key":"count"}}});
    let payload: ResultPayload = serde_json::from_value(valid.clone()).unwrap();
    assert!(payload.validate().is_ok());
    let mut missing = valid.clone();
    missing["expression"]["right"]["key"] = json!("missing");
    assert!(serde_json::from_value::<ResultPayload>(missing)
        .unwrap()
        .validate()
        .is_err());
    let mut invalid_range = valid;
    invalid_range["inputs"][0]["max"] = json!(5);
    assert!(serde_json::from_value::<ResultPayload>(invalid_range)
        .unwrap()
        .validate()
        .is_err());
}
