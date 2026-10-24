use crate::*;
use anima_village::{Language, NpcId, Phase};
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::time::Duration;
use tower::ServiceExt;

fn traveler(name: &str) -> Value {
    json!({"name":name,"skinTone":"warm","hairStyle":"braid","hairColor":"ink","outfit":"cloak","outfitColor":"moss"})
}
async fn post(app: &Router, uri: &str, body: Value) -> (u16, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let app = Service::new(
        Store::open(&dir.path().join("day.sqlite")).unwrap(),
        None,
        Duration::from_millis(50),
    );
    (dir, app)
}
#[tokio::test]
async fn traveler_is_atomic_model_free_persistent_and_exactly_replayed_after_other_progress() {
    let (_dir, service) = setup();
    let save = service.create(Language::Ja).unwrap();
    let app = http::router(service.clone());
    let uri = format!("/api/village/saves/{}/traveler", save.save_id);
    let request = json!({"requestId":"appearance","traveler":traveler("  栞  ")});
    let (status, first) = post(&app, &uri, request.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(first["traveler"]["name"], "栞");
    let turns = format!("/api/village/saves/{}/turns", save.save_id);
    assert_eq!(
        post(
            &app,
            &turns,
            json!({"requestId":"bread","choiceId":"collect_order"})
        )
        .await
        .0,
        200
    );
    let (status, replay) = post(&app, &uri, request).await;
    assert_eq!(status, 200);
    assert_eq!(replay, first);
    assert_eq!(service.get(&save.save_id).unwrap().coins, 20);
    let (status, _) = post(
        &app,
        &uri,
        json!({"requestId":"appearance","traveler":traveler("Other")}),
    )
    .await;
    assert_eq!(status, 409);
    let (status, edited) = post(
        &app,
        &uri,
        json!({"requestId":"appearance-2","traveler":traveler("Ren")}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(edited["coins"], 20);
    assert_eq!(edited["traveler"]["name"], "Ren");
}
#[tokio::test]
async fn traveler_validation_happens_before_admission_and_never_overwrites_pending_conversation() {
    let (_dir, service) = setup();
    let save = service.create(Language::Ja).unwrap();
    let app = http::router(service.clone());
    let uri = format!("/api/village/saves/{}/traveler", save.save_id);
    for name in ["", "   ", "\nAra", "1234567890123456789012345"] {
        assert_eq!(
            post(
                &app,
                &uri,
                json!({"requestId":"invalid","traveler":traveler(name)})
            )
            .await
            .0,
            400
        );
    }
    let mut invalid = traveler("Ara");
    invalid["hairStyle"] = json!("unknown");
    assert_eq!(
        post(
            &app,
            &uri,
            json!({"requestId":"invalid","traveler":invalid})
        )
        .await
        .0,
        400
    );
    let original: Interaction =
        serde_json::from_value(json!({"requestId":"pending","message":"hi"})).unwrap();
    let payload = serde_json::to_string(&(false, &original)).unwrap();
    service
        .store
        .begin(&save.save_id, "pending", &payload)
        .unwrap();
    assert_eq!(
        post(
            &app,
            &uri,
            json!({"requestId":"invalid","traveler":traveler("Ara")})
        )
        .await
        .0,
        409
    );
    let turns = format!("/api/village/saves/{}/turns", save.save_id);
    assert_eq!(
        post(
            &app,
            &turns,
            json!({"requestId":"recovery","choiceId":"honey","recoveryOf":"pending"})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        post(
            &app,
            &uri,
            json!({"requestId":"invalid","traveler":traveler("Ara")})
        )
        .await
        .0,
        200
    );
}
#[tokio::test]
async fn complete_route_persists_delivered_inventory_and_speaker() {
    let (_dir, service) = setup();
    let save = service.create(Language::Zh).unwrap();
    let app = http::router(service.clone());
    let turns = format!("/api/village/saves/{}/turns", save.save_id);
    post(
        &app,
        &turns,
        json!({"requestId":"collect","choiceId":"collect_order"}),
    )
    .await;
    let (_, delivered) = post(
        &app,
        &turns,
        json!({"requestId":"deliver","choiceId":"deliver_order","npcId":"mira"}),
    )
    .await;
    assert_eq!(delivered["phase"], "evening");
    assert_eq!(delivered["questStatus"], "delivered");
    assert_eq!(delivered["inventory"], json!([]));
    let visit = format!("/api/village/saves/{}/visit", save.save_id);
    let (_, complete) = post(&app, &visit, json!({"requestId":"return","npcId":"tavi"})).await;
    assert_eq!(complete["phase"], "complete");
    let restored = service.get(&save.save_id).unwrap();
    assert_eq!(restored.phase, Phase::Complete);
    assert_eq!(restored.npc_id, NpcId::Tavi);
    assert_eq!(restored.coins, 20);
}

#[tokio::test]
async fn old_committed_receipt_replay_supplies_defaults_without_overwriting_latest_progress() {
    let (_dir, service) = setup();
    let save = service.create(Language::Ja).unwrap();
    let request: Interaction =
        serde_json::from_value(json!({"requestId":"old","choiceId":"collect_order"})).unwrap();
    let first = service
        .interact(&save.save_id, request.clone(), false)
        .await
        .unwrap();
    let mut old = serde_json::to_value(&first).unwrap();
    for field in ["traveler", "npcId", "phase"] {
        old.as_object_mut().unwrap().remove(field);
    }
    rusqlite::Connection::open(&service.store.path)
        .unwrap()
        .execute(
            "UPDATE village_requests SET response=?1 WHERE save_id=?2 AND request_id='old'",
            rusqlite::params![old.to_string(), save.save_id],
        )
        .unwrap();
    let app = http::router(service.clone());
    let turns = format!("/api/village/saves/{}/turns", save.save_id);
    post(
        &app,
        &turns,
        json!({"requestId":"deliver","choiceId":"deliver_order","npcId":"mira"}),
    )
    .await;
    let uri = format!("/api/village/saves/{}/traveler", save.save_id);
    post(
        &app,
        &uri,
        json!({"requestId":"create","traveler":traveler("Ara")}),
    )
    .await;
    let replay = serde_json::to_value(
        service
            .interact(&save.save_id, request, false)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(replay.get("traveler").unwrap().is_null());
    assert_eq!(replay["phase"], "day");
    assert_eq!(replay["npcId"], "tavi");
    assert_eq!(replay["inventory"][0]["quantity"], 2);
    let current = service.get(&save.save_id).unwrap();
    assert_eq!(current.phase, Phase::Evening);
    assert_eq!(current.traveler.unwrap().name, "Ara");
    assert!(current.inventory.is_empty());
}
#[tokio::test]
async fn failed_evening_visit_preserves_phase_and_reviewed_recovery_commits_callback_once() {
    use anima_village::{GameInput, GameState, NpcEngine, PlayedTurn, RuntimeInfo};
    use std::sync::Arc;
    struct Fail;
    #[async_trait::async_trait]
    impl NpcEngine for Fail {
        fn runtime(&self) -> RuntimeInfo {
            RuntimeInfo::unavailable()
        }
        async fn play(
            &self,
            _state: GameState,
            _input: GameInput,
        ) -> std::result::Result<PlayedTurn, String> {
            Err("unusable generation".into())
        }
    }
    let (dir, service) = setup();
    let path = service.store.path.clone();
    let save = service.create(Language::Ja).unwrap();
    let app = http::router(service.clone());
    let turns = format!("/api/village/saves/{}/turns", save.save_id);
    post(
        &app,
        &turns,
        json!({"requestId":"collect","choiceId":"collect_order"}),
    )
    .await;
    post(
        &app,
        &turns,
        json!({"requestId":"delivery","choiceId":"deliver_order","npcId":"mira"}),
    )
    .await;
    drop(app);
    drop(service);
    let service = Service::new(
        Store::open(&path).unwrap(),
        Some(Arc::new(Fail)),
        Duration::from_millis(50),
    );
    let visit: Interaction =
        serde_json::from_value(json!({"requestId":"evening","npcId":"tavi"})).unwrap();
    assert_eq!(
        service
            .interact(&save.save_id, visit.clone(), true)
            .await
            .unwrap_err()
            .status,
        503
    );
    assert_eq!(service.get(&save.save_id).unwrap().phase, Phase::Evening);
    let recovery:Interaction=serde_json::from_value(json!({"requestId":"evening-review","choiceId":"honey","recoveryOf":"evening","npcId":"tavi"})).unwrap();
    let complete = service
        .interact(&save.save_id, recovery, false)
        .await
        .unwrap();
    assert_eq!(complete.phase, Phase::Complete);
    assert!(complete.dialogue.translation.contains("supper"));
    assert_eq!(complete.visits, 1);
    assert_eq!(complete.bread_preference, None);
    let replay = service.interact(&save.save_id, visit, true).await.unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(complete).unwrap()
    );
    drop(service);
    drop(dir);
}
#[tokio::test]
async fn all_traveler_allowlists_and_unicode_name_boundaries_are_enforced_without_model_calls() {
    let (_dir, service) = setup();
    let save = service.create(Language::Ja).unwrap();
    let app = http::router(service.clone());
    let uri = format!("/api/village/saves/{}/traveler", save.save_id);
    for (field, values) in [
        ("skinTone", vec!["fair", "warm", "deep"]),
        ("hairStyle", vec!["short", "bob", "braid"]),
        ("hairColor", vec!["chestnut", "ink", "gold", "auburn"]),
        ("outfit", vec!["cloak", "tunic", "overalls"]),
        ("outfitColor", vec!["moss", "ochre", "berry"]),
    ] {
        for value in values {
            let mut person = traveler(&"栞".repeat(24));
            person[field] = json!(value);
            assert_eq!(
                post(
                    &app,
                    &uri,
                    json!({"requestId":format!("{field}-{value}"),"traveler":person})
                )
                .await
                .0,
                200
            );
        }
        let mut person = traveler("Ara");
        person[field] = json!("invalid");
        assert_eq!(
            post(
                &app,
                &uri,
                json!({"requestId":format!("{field}-invalid"),"traveler":person})
            )
            .await
            .0,
            400
        );
    }
}
