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

fn tracker() -> serde_json::Value {
    json!({
        "kind":"tracker",
        "columns":[
            {"key":"task","label":"Task","kind":"text"},
            {"key":"hours","label":"Hours","kind":"number"},
            {"key":"done","label":"Done","kind":"checkbox"}
        ],
        "rows":[{"id":"task_1","cells":{"task":"Draft <script> is text","hours":2,"done":false}}]
    })
}

fn valid_payload(value: serde_json::Value) -> bool {
    serde_json::from_value::<ResultPayload>(value).is_ok_and(|payload| payload.validate().is_ok())
}

#[test]
fn dashboard_tracker_payloads_compose_and_round_trip_as_closed_data() {
    let tracker_data = tracker();
    assert!(valid_payload(tracker_data.clone()));
    let dashboard = json!({"kind":"dashboard","widgets":[
        {"id":"tasks","title":"Project tasks","payload":tracker_data},
        {"id":"summary","title":"Summary","payload":{"kind":"document","format":"markdown","text":"# Project"}},
        {"id":"cost","title":"Estimate","payload":{"kind":"calculator","inputs":[{"key":"hours","label":"Hours","value":2}],"expression":{"op":"multiply","left":{"op":"input","key":"hours"},"right":{"op":"constant","value":10}},"outputLabel":"Cost"}}
    ]});
    assert!(valid_payload(dashboard.clone()));
    let parsed: ResultPayload = serde_json::from_value(dashboard.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), dashboard);
    let mut empty_tracker = tracker();
    empty_tracker["rows"] = json!([]);
    assert!(valid_payload(empty_tracker));
}

#[test]
fn tracker_payloads_validate_stable_ids_columns_cell_types_and_limits() {
    let mut invalid = Vec::new();
    for value in [
        json!("wrong"),
        json!(true),
        json!(null),
        json!(9_007_199_254_740_992_u64),
    ] {
        let mut item = tracker();
        item["rows"][0]["cells"]["hours"] = value;
        invalid.push(item);
    }
    let mut item = tracker();
    item["rows"][0]["cells"]
        .as_object_mut()
        .unwrap()
        .remove("done");
    invalid.push(item);
    let mut item = tracker();
    item["rows"][0]["cells"]["extra"] = json!(false);
    invalid.push(item);
    let mut item = tracker();
    item["rows"][0]["cells"]["done"] = json!("false");
    invalid.push(item);
    let mut item = tracker();
    item["rows"][0]["cells"]["task"] = json!(2048);
    invalid.push(item);
    let mut item = tracker();
    item["rows"][0]["cells"]["task"] = json!("a".repeat(2049));
    invalid.push(item);
    let mut item = tracker();
    item["columns"][1] = item["columns"][0].clone();
    invalid.push(item);
    let mut item = tracker();
    item["rows"] = json!([item["rows"][0].clone(), item["rows"][0].clone()]);
    invalid.push(item);
    for id in ["", "row-1", "../row", "row 1", "é"] {
        let mut item = tracker();
        item["rows"][0]["id"] = json!(id);
        invalid.push(item);
    }
    let mut item = tracker();
    item["columns"] = json!([]);
    invalid.push(item);
    let mut item = tracker();
    item["columns"][0]["key"] = json!("bad-key");
    invalid.push(item);
    let mut item = tracker();
    item["columns"][0]["label"] = json!("a".repeat(161));
    invalid.push(item);
    let mut item = tracker();
    item["columns"] = serde_json::Value::Array(
        (0..13)
            .map(|index| json!({"key":format!("col_{index}"),"label":"Column","kind":"text"}))
            .collect(),
    );
    item["rows"] = json!([]);
    invalid.push(item);
    let mut item = tracker();
    item["rows"] = serde_json::Value::Array((0..101).map(|index| json!({"id":format!("row_{index}"),"cells":{"task":"Task","hours":2,"done":false}})).collect());
    invalid.push(item);
    let mut item = tracker();
    item["columns"][0]["script"] = json!("alert(1)");
    invalid.push(item);
    let mut item = tracker();
    item["rows"][0]["script"] = json!("alert(1)");
    invalid.push(item);
    assert!(invalid.into_iter().all(|value| !valid_payload(value)));
}

#[test]
fn dashboards_refuse_nesting_duplicate_widgets_and_unbounded_composition() {
    let widget = json!({"id":"tasks","title":"Tasks","payload":tracker()});
    for widgets in [
        json!([]),
        json!([widget.clone(),widget.clone()]),
        json!([{"id":"nested","title":"Nested","payload":{"kind":"dashboard","widgets":[widget.clone()]}}]),
        serde_json::Value::Array((0..9).map(|index| json!({"id":format!("widget_{index}"),"title":"Tasks","payload":tracker()})).collect()),
        json!([{"id":"bad-id","title":"Bad","payload":tracker()}]),
        json!([{"id":"bad","title":"a".repeat(161),"payload":tracker()}]),
        json!([{"id":"bad","title":"Bad","payload":{"kind":"html","html":"<script>bad</script>"}}]),
    ] {
        assert!(!valid_payload(json!({"kind":"dashboard","widgets":widgets})));
    }
    let widgets = (0..8).map(|index| json!({"id":format!("widget_{index}"),"title":"Document","payload":{"kind":"document","format":"text","text":"a".repeat(5000)}})).collect::<Vec<_>>();
    assert!(!valid_payload(
        json!({"kind":"dashboard","widgets":widgets})
    ));
}

#[test]
fn tracker_owner_edits_require_the_latest_revision_for_future_model_replacements() {
    let mut room = session();
    let mut create = input("create_tracker");
    create.payload = serde_json::from_value(tracker()).unwrap();
    let first = upsert(&mut room, create, "run", "call", 2).unwrap();
    let mut edited = tracker();
    edited["rows"][0]["cells"]["done"] = json!(true);
    let mut save = input("owner_edit");
    save.id = Some(first.id.clone());
    save.expected_revision = first.revision;
    save.payload = serde_json::from_value(edited.clone()).unwrap();
    let acknowledged = upsert(&mut room, save, "owner", "owner_edit", 3).unwrap();
    let mut stale = input("stale_model_update");
    stale.id = Some(first.id);
    stale.expected_revision = 1;
    assert!(upsert(&mut room, stale, "next-run", "stale-call", 4).is_err());
    assert_eq!(room.chat_results[0], acknowledged);
    let reopened: SessionRecord =
        serde_json::from_slice(&serde_json::to_vec(&room).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.chat_results[0].payload).unwrap(),
        edited
    );
}
