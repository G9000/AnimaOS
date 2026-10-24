use crate::*;
use anima_village::{
    reviewed_turn, GameInput, GameState, Language, NpcEngine, PlayedTurn, Preference, RuntimeInfo,
};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

fn input(id: &str, choice: Option<&str>) -> Interaction {
    Interaction {
        request_id: id.into(),
        message: String::new(),
        choice_id: choice.map(str::to_owned),
        recovery_of: None,
        npc_id: None,
    }
}
fn service(path: &std::path::Path) -> Service {
    Service::new(Store::open(path).unwrap(), None, Duration::from_millis(50))
}

#[tokio::test]
async fn exact_retry_returns_original_receipt_and_changed_payload_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let app = service(&dir.path().join("saves.sqlite"));
    let save = app.create(Language::Ja).unwrap();
    let request = input("order-1", Some("collect_order"));
    let first = app
        .interact(&save.save_id, request.clone(), false)
        .await
        .unwrap();
    app.interact(&save.save_id, input("pref-1", Some("honey")), false)
        .await
        .unwrap();
    let replay = app.interact(&save.save_id, request, false).await.unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert_eq!(
        app.interact(&save.save_id, input("order-1", Some("sesame")), false)
            .await
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(app.get(&save.save_id).unwrap().inventory[0].quantity, 2);
}

#[tokio::test]
async fn reopen_preserves_preference_and_quest_without_cross_save_leaks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saves.sqlite");
    let app = service(&path);
    let a = app.create(Language::Zh).unwrap();
    let b = app.create(Language::Ja).unwrap();
    app.interact(&a.save_id, input("visit-1", None), true)
        .await
        .unwrap();
    for (id, choice) in [
        ("order", "collect_order"),
        ("pref", "sesame"),
        ("help", "help"),
    ] {
        app.interact(&a.save_id, input(id, Some(choice)), false)
            .await
            .unwrap();
    }
    drop(app);
    let reopened = service(&path);
    let next = reopened
        .interact(&a.save_id, input("visit-2", None), true)
        .await
        .unwrap();
    assert_eq!(next.visits, 2);
    assert_eq!(next.coins, 20);
    assert_eq!(next.bread_preference, Some(Preference::Sesame));
    assert!(next.dialogue.translation.contains("sesame"));
    assert!(next.dialogue.translation.contains("encouragement"));
    let other = reopened.get(&b.save_id).unwrap();
    assert!(other.inventory.is_empty());
    assert_eq!(other.coins, 0);
    assert!(other.memories.is_empty());
}

struct Gate {
    entered: Notify,
    release: Notify,
}
#[async_trait]
impl NpcEngine for Gate {
    fn runtime(&self) -> RuntimeInfo {
        RuntimeInfo {
            mode: "live".into(),
            engine: "anima-harness".into(),
            provider: "test".into(),
            model: "gate".into(),
        }
    }
    async fn play(
        &self,
        state: GameState,
        input: GameInput,
    ) -> std::result::Result<PlayedTurn, String> {
        self.entered.notify_one();
        self.release.notified().await;
        reviewed_turn(state, &input, &self.runtime())
    }
}
#[tokio::test]
async fn recovery_wins_and_late_generation_cannot_overwrite_it() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let app = Service::new(
        Store::open(&dir.path().join("saves.sqlite")).unwrap(),
        Some(gate.clone()),
        Duration::from_secs(3),
    );
    let save = app.create(Language::Ja).unwrap();
    let id = save.save_id;
    let running = tokio::spawn({
        let app = app.clone();
        let id = id.clone();
        async move { app.interact(&id, input("slow", Some("honey")), false).await }
    });
    gate.entered.notified().await;
    let mut recovery = input("recover", Some("sesame"));
    recovery.recovery_of = Some("slow".into());
    let winner = app.interact(&id, recovery.clone(), false).await.unwrap();
    assert_eq!(winner.bread_preference, Some(Preference::Sesame));
    gate.release.notify_one();
    let late = running.await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(late).unwrap(),
        serde_json::to_value(&winner).unwrap()
    );
    assert_eq!(
        app.get(&id).unwrap().bread_preference,
        Some(Preference::Sesame)
    );
    assert_eq!(
        app.interact(&id, input("slow", Some("help")), false)
            .await
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        app.interact(&id, recovery, false)
            .await
            .unwrap()
            .bread_preference,
        Some(Preference::Sesame)
    );
}

#[tokio::test]
async fn original_commit_wins_and_recovery_returns_it_without_another_action() {
    let dir = tempfile::tempdir().unwrap();
    let app = service(&dir.path().join("saves.sqlite"));
    let save = app.create(Language::Ja).unwrap();
    let first = app
        .interact(&save.save_id, input("original", Some("honey")), false)
        .await
        .unwrap();
    let mut recovery = input("recover", Some("collect_order"));
    recovery.recovery_of = Some("original".into());
    let view = app.interact(&save.save_id, recovery, false).await.unwrap();
    assert_eq!(
        serde_json::to_value(view).unwrap(),
        serde_json::to_value(first).unwrap()
    );
    assert!(app.get(&save.save_id).unwrap().inventory.is_empty());
}

#[tokio::test]
async fn timeout_leaves_live_choice_pending_until_explicit_reviewed_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let app = Service::new(
        Store::open(&dir.path().join("saves.sqlite")).unwrap(),
        Some(gate),
        Duration::from_millis(30),
    );
    let a = app.create(Language::Ja).unwrap();
    let b = app.create(Language::Zh).unwrap();
    let first = tokio::spawn({
        let app = app.clone();
        let id = a.save_id.clone();
        async move {
            app.interact(&id, input("wait", Some("collect_order")), false)
                .await
        }
    });
    assert_eq!(
        app.interact(&b.save_id, input("help", Some("help")), false)
            .await
            .unwrap_err()
            .status,
        503
    );
    assert_eq!(first.await.unwrap().unwrap_err().status, 503);
    assert!(app.get(&a.save_id).unwrap().inventory.is_empty());
    let mut recovery = input("reviewed", Some("collect_order"));
    recovery.recovery_of = Some("wait".into());
    let recovered = app.interact(&a.save_id, recovery, false).await.unwrap();
    assert_eq!(recovered.coins, 20);
    assert_eq!(recovered.runtime.mode, "reviewed");
}

#[tokio::test]
async fn invalid_choice_and_recovery_are_rejected_before_any_effect() {
    let dir = tempfile::tempdir().unwrap();
    let app = service(&dir.path().join("saves.sqlite"));
    let save = app.create(Language::Ja).unwrap();
    assert_eq!(
        app.interact(&save.save_id, input("bad", Some("admin")), false)
            .await
            .unwrap_err()
            .status,
        400
    );
    let mut r = input("recovery", Some("honey"));
    r.recovery_of = Some("missing".into());
    assert_eq!(
        app.interact(&save.save_id, r, false)
            .await
            .unwrap_err()
            .status,
        404
    );
    assert_eq!(app.get(&save.save_id).unwrap().coins, 0);
}

#[tokio::test]
async fn pending_identity_survives_restart_and_recovery_is_atomic_with_its_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.sqlite");
    let app = service(&path);
    let save = app.create(Language::Ja).unwrap();
    let original = input("lost", Some("collect_order"));
    let payload = serde_json::to_string(&(false, &original)).unwrap();
    let token = match app.store.begin(&save.save_id, "lost", &payload).unwrap() {
        store::Admission::Start { token, .. } => token,
        _ => panic!("first request must own generation"),
    };
    assert_eq!(
        app.store
            .begin(&save.save_id, "lost", "changed")
            .err()
            .unwrap()
            .status,
        409
    );
    drop(app);
    let reopened = service(&path);
    assert!(reopened.get(&save.save_id).unwrap().inventory.is_empty());
    let mut recovery = input("reopen-recovery", Some("collect_order"));
    recovery.recovery_of = Some("lost".into());
    let recovered = reopened
        .interact(&save.save_id, recovery, false)
        .await
        .unwrap();
    assert_eq!(recovered.coins, 20);
    let late = reviewed_turn(
        GameState::new(Language::Ja),
        &GameInput::Turn {
            message: String::new(),
            choice_id: Some("honey".into()),
        },
        &RuntimeInfo::unavailable(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .store
            .commit(&save.save_id, "lost", &token, late)
            .unwrap()
            .coins,
        20
    );
    drop(reopened);
    let final_app = service(&path);
    let replay = final_app
        .interact(&save.save_id, original, false)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(recovered).unwrap()
    );
}

#[tokio::test]
async fn simultaneous_exact_retries_share_one_generation() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let app = Service::new(
        Store::open(&dir.path().join("retry.sqlite")).unwrap(),
        Some(gate.clone()),
        Duration::from_secs(2),
    );
    let save = app.create(Language::Ja).unwrap();
    let id = save.save_id;
    let first = tokio::spawn({
        let app = app.clone();
        let id = id.clone();
        async move {
            app.interact(&id, input("same", Some("collect_order")), false)
                .await
        }
    });
    gate.entered.notified().await;
    let retry = tokio::spawn({
        let app = app.clone();
        let id = id.clone();
        async move {
            app.interact(&id, input("same", Some("collect_order")), false)
                .await
        }
    });
    tokio::task::yield_now().await;
    gate.release.notify_one();
    let first = first.await.unwrap().unwrap();
    let second = retry.await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap()
    );
    assert_eq!(second.inventory[0].quantity, 2);
}

struct SelectiveGate {
    entered: Notify,
    release: Notify,
}
#[async_trait]
impl NpcEngine for SelectiveGate {
    fn runtime(&self) -> RuntimeInfo {
        RuntimeInfo::unavailable()
    }
    async fn play(
        &self,
        state: GameState,
        input: GameInput,
    ) -> std::result::Result<PlayedTurn, String> {
        if matches!(&input,GameInput::Turn {message,..}if message=="slow") {
            self.entered.notify_one();
            self.release.notified().await;
        }
        reviewed_turn(state, &input, &self.runtime())
    }
}
#[tokio::test]
async fn another_save_progresses_while_generation_waits_and_disconnect_keeps_worker_alive() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(SelectiveGate {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let app = Service::new(
        Store::open(&dir.path().join("parallel.sqlite")).unwrap(),
        Some(gate.clone()),
        Duration::from_secs(3),
    );
    let a = app.create(Language::Ja).unwrap();
    let b = app.create(Language::Zh).unwrap();
    let mut slow = input("slow", Some("honey"));
    slow.message = "slow".into();
    let caller = tokio::spawn({
        let app = app.clone();
        let id = a.save_id.clone();
        async move { app.interact(&id, slow, false).await }
    });
    gate.entered.notified().await;
    let other = tokio::time::timeout(
        Duration::from_millis(500),
        app.interact(&b.save_id, input("collect", Some("collect_order")), false),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(other.coins, 20);
    caller.abort();
    gate.release.notify_one();
    for _ in 0..50 {
        if app.get(&a.save_id).unwrap().bread_preference.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        app.get(&a.save_id).unwrap().bread_preference,
        Some(Preference::Honey)
    );
}

#[tokio::test]
async fn recovery_of_a_pending_visit_counts_entry_once_before_the_reviewed_choice() {
    let dir = tempfile::tempdir().unwrap();
    let app = service(&dir.path().join("visit-recovery.sqlite"));
    let save = app.create(Language::Ja).unwrap();
    let original = input("entry", None);
    let payload = serde_json::to_string(&(true, &original)).unwrap();
    app.store.begin(&save.save_id, "entry", &payload).unwrap();
    let mut recovery = input("entry-recovery", Some("honey"));
    recovery.recovery_of = Some("entry".into());
    let recovered = app.interact(&save.save_id, recovery, false).await.unwrap();
    assert_eq!(recovered.visits, 1);
    assert_eq!(recovered.bread_preference, Some(Preference::Honey));
    let returned = app
        .interact(&save.save_id, input("entry-again", None), true)
        .await
        .unwrap();
    assert_eq!(returned.visits, 2);
    assert!(returned.dialogue.translation.contains("honey"));
}

struct RetryEngine {
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl NpcEngine for RetryEngine {
    fn runtime(&self) -> RuntimeInfo {
        RuntimeInfo {
            mode: "live".into(),
            engine: "anima-harness".into(),
            provider: "fixture".into(),
            model: "retry".into(),
        }
    }
    async fn play(
        &self,
        state: GameState,
        _input: GameInput,
    ) -> std::result::Result<PlayedTurn, String> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        let mut turn = reviewed_turn(
            state,
            &GameInput::Turn {
                message: String::new(),
                choice_id: Some("collect_order".into()),
            },
            &self.runtime(),
        )?;
        turn.runtime = self.runtime();
        turn.dialogue.source = anima_village::DialogueSource::Model;
        Ok(turn)
    }
}
#[tokio::test]
async fn timed_out_free_text_can_retry_same_identity_and_then_replay_its_one_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(RetryEngine {
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let app = Service::new(
        Store::open(&dir.path().join("retry-timeout.sqlite")).unwrap(),
        Some(engine.clone()),
        Duration::from_millis(20),
    );
    let save = app.create(Language::Ja).unwrap();
    let mut request = input("typed-order", None);
    request.message = "Mira asked me to collect her two loaves".into();
    assert_eq!(
        app.interact(&save.save_id, request.clone(), false)
            .await
            .unwrap_err()
            .status,
        503
    );
    assert!(app.get(&save.save_id).unwrap().inventory.is_empty());
    let retried = app
        .interact(&save.save_id, request.clone(), false)
        .await
        .unwrap();
    assert_eq!(retried.coins, 20);
    assert_eq!(retried.runtime.mode, "live");
    let replay = app.interact(&save.save_id, request, false).await.unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(retried).unwrap()
    );
    assert_eq!(engine.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn restart_releases_stale_pending_ownership_for_exact_retry_and_late_commit_stays_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restart-retry.sqlite");
    let app = service(&path);
    let save = app.create(Language::Ja).unwrap();
    let request = input("lost-generation", Some("collect_order"));
    let payload = serde_json::to_string(&(false, &request)).unwrap();
    let old_token = match app
        .store
        .begin(&save.save_id, "lost-generation", &payload)
        .unwrap()
    {
        store::Admission::Start { token, .. } => token,
        _ => panic!("must admit"),
    };
    assert!(
        Store::open(&path).is_err(),
        "two active hosts must not share one save database"
    );
    drop(app);
    let reopened = service(&path);
    let retried = tokio::time::timeout(
        Duration::from_millis(500),
        reopened.interact(&save.save_id, request, false),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(retried.coins, 20);
    let late = reviewed_turn(
        GameState::new(Language::Ja),
        &GameInput::Turn {
            message: String::new(),
            choice_id: Some("sesame".into()),
        },
        &RuntimeInfo::unavailable(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .store
            .commit(&save.save_id, "lost-generation", &old_token, late)
            .unwrap()
            .coins,
        20
    );
    assert_eq!(reopened.get(&save.save_id).unwrap().bread_preference, None);
}

#[tokio::test]
async fn timed_out_free_text_recovery_grants_once_and_a_late_token_cannot_overwrite_it() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let app = Service::new(
        Store::open(&dir.path().join("typed-recovery.sqlite")).unwrap(),
        Some(gate.clone()),
        Duration::from_millis(30),
    );
    let save = app.create(Language::Ja).unwrap();
    let mut original = input("typed", None);
    original.message = "Two loaves for Mira, please".into();
    let running = tokio::spawn({
        let app = app.clone();
        let id = save.save_id.clone();
        let original = original.clone();
        async move { app.interact(&id, original, false).await }
    });
    gate.entered.notified().await;
    let old_token: String = rusqlite::Connection::open(&app.store.path)
        .unwrap()
        .query_row(
            "SELECT token FROM village_requests WHERE save_id=?1 AND request_id='typed'",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(running.await.unwrap().unwrap_err().status, 503);
    let mut recovery = input("typed-recovery", Some("collect_order"));
    recovery.recovery_of = Some("typed".into());
    let winner = app
        .interact(&save.save_id, recovery.clone(), false)
        .await
        .unwrap();
    assert_eq!(winner.coins, 20);
    let late = reviewed_turn(
        GameState::new(Language::Ja),
        &GameInput::Turn {
            message: String::new(),
            choice_id: Some("sesame".into()),
        },
        &RuntimeInfo::unavailable(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(
            app.store
                .commit(&save.save_id, "typed", &old_token, late)
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&winner).unwrap()
    );
    assert_eq!(
        app.interact(&save.save_id, original, false)
            .await
            .unwrap()
            .coins,
        20
    );
    assert_eq!(
        app.interact(&save.save_id, recovery, false)
            .await
            .unwrap()
            .coins,
        20
    );
    assert_eq!(app.get(&save.save_id).unwrap().inventory[0].quantity, 2);
    assert_eq!(app.get(&save.save_id).unwrap().bread_preference, None);
}

#[tokio::test]
async fn speaker_identity_changes_conflict_and_recovery_cannot_switch_speaker() {
    let dir = tempfile::tempdir().unwrap();
    let app = service(&dir.path().join("speaker.sqlite"));
    let save = app.create(Language::Ja).unwrap();
    let mira:Interaction=serde_json::from_value(serde_json::json!({"requestId":"mira","message":"","choiceId":"deliver_order","npcId":"mira"})).unwrap();
    let payload = serde_json::to_string(&(false, &mira)).unwrap();
    app.store.begin(&save.save_id, "mira", &payload).unwrap();
    let mut wrong = input("wrong", Some("collect_order"));
    wrong.recovery_of = Some("mira".into());
    assert_eq!(
        app.interact(&save.save_id, wrong, false)
            .await
            .unwrap_err()
            .status,
        409
    );
    let recovery:Interaction=serde_json::from_value(serde_json::json!({"requestId":"mira-recovery","choiceId":"deliver_order","recoveryOf":"mira","npcId":"mira"})).unwrap();
    let view = app.interact(&save.save_id, recovery, false).await.unwrap();
    assert!(view.inventory.is_empty());
    let changed:Interaction=serde_json::from_value(serde_json::json!({"requestId":"mira","message":"","choiceId":"deliver_order","npcId":"tavi"})).unwrap();
    assert_eq!(
        app.interact(&save.save_id, changed, false)
            .await
            .unwrap_err()
            .status,
        409
    );
}
#[test]
fn old_receipt_defaults_are_readable_without_rewriting_latest_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let app = service(&dir.path().join("legacy.sqlite"));
    let save = app.create(Language::Ja).unwrap();
    let mut old = serde_json::to_value(&save).unwrap();
    for field in ["traveler", "npcId", "phase"] {
        old.as_object_mut().unwrap().remove(field);
    }
    let legacy: anima_village::GameView = serde_json::from_value(old).unwrap();
    let value = serde_json::to_value(legacy).unwrap();
    assert_eq!(value["traveler"], serde_json::Value::Null);
    assert_eq!(value["npcId"], "tavi");
    assert_eq!(value["phase"], "day");
}
#[tokio::test]
async fn legacy_pending_payload_without_npc_id_retries_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old-pending.sqlite");
    let app = service(&path);
    let save = app.create(Language::Ja).unwrap();
    let old = r#"[false,{"requestId":"legacy","message":"","choiceId":"collect_order","recoveryOf":null}]"#;
    app.store.begin(&save.save_id, "legacy", old).unwrap();
    drop(app);
    let app = service(&path);
    let view = app
        .interact(&save.save_id, input("legacy", Some("collect_order")), false)
        .await
        .unwrap();
    assert_eq!(view.coins, 20);
}
