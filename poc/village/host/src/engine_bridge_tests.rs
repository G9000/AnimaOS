#[test]
fn durable_metadata_does_not_rewrite_legacy_save_or_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copied.sqlite");
    let store = crate::Store::open(&path).unwrap();
    let save = store
        .create(
            anima_village::Language::Ja,
            anima_village::RuntimeInfo::unavailable(),
        )
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM village_engine_stages", [], |r| {
            r.get(0)
        })
        .expect("durable stage mappings must be persisted alongside existing saves");
    assert_eq!(count, 0);
    assert_eq!(store.get(&save.save_id).unwrap().save_id, save.save_id);
}

use crate::engine_bridge::DurableVillage;
use crate::{Interaction, Service, Store};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::anima_core::*;
use anima_memory_store_sqlite::SqliteMemoryStore;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct VillageModel(Arc<Mutex<Vec<ModelGenerateRequest>>>);
#[async_trait]
impl ModelAdapter for VillageModel {
    fn provider(&self) -> &str {
        "fixture"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        r: &ModelGenerateRequest,
    ) -> std::result::Result<ModelGenerateResponse, String> {
        self.0.lock().unwrap().push(r.clone());
        let text = if r.system.contains("You interpret")
            && serde_json::to_string(&r.messages)
                .unwrap()
                .contains("I prefer honey bread and encourage your recipe")
        {
            r#"{"preference":"honey","help":true}"#
        } else if r.system.contains("You interpret") {
            r#"{"preference":"honey"}"#
        } else {
            r#"{"text":"はちみつのパンですね。","translation":"Honey bread, I will remember.","reading":"Hachimitsu no pan desu ne.","expression":"warm"}"#
        };
        Ok(ModelGenerateResponse {
            content: Content {
                text: text.into(),
                ..Default::default()
            },
            tool_calls: None,
            usage: TokenUsage {
                prompt_tokens: 20,
                completion_tokens: 30,
                total_tokens: 50,
                ..Default::default()
            },
            stop_reason: ModelStopReason::End,
        })
    }
}
async fn fixture(path: &std::path::Path, calls: Arc<Mutex<Vec<ModelGenerateRequest>>>) -> Service {
    fixture_with_clock(path, calls, Arc::new(ManualExecutionClock::default())).await
}
async fn fixture_with_clock(
    path: &std::path::Path,
    calls: Arc<Mutex<Vec<ModelGenerateRequest>>>,
    clock: Arc<ManualExecutionClock>,
) -> Service {
    let execution = Arc::new(
        SqliteExecutionStore::open(
            path.join("execution.sqlite"),
            PersistenceProtection::allow_model_payloads(
                PersistenceSnapshotSealKey::new([12; 32]).unwrap(),
                vec![],
            )
            .unwrap(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let memory = SqliteMemoryStore::open(path.join("memory.sqlite"), clock.clone())
        .await
        .unwrap();
    let bridge = Arc::new(DurableVillage::new(
        execution,
        memory,
        clock,
        Arc::new(VillageModel(calls)),
        "fixture".into(),
        "scripted".into(),
    ));
    Service::new(
        Store::open(&path.join("game.sqlite")).unwrap(),
        None,
        Duration::from_secs(30),
    )
    .with_durable(bridge)
    .await
    .unwrap()
}
fn input(request: &str, npc: anima_village::NpcId, message: &str) -> Interaction {
    Interaction {
        request_id: request.into(),
        message: message.into(),
        choice_id: None,
        recovery_of: None,
        npc_id: Some(npc),
    }
}
#[tokio::test]
async fn real_http_boundary_uses_persistent_resident_and_replays_exact_receipt() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let app = crate::http::router(service.clone());
    let request = || {
        Request::builder()
            .method("POST")
            .uri(format!("/api/village/saves/{}/turns", save.save_id))
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"requestId":"private","message":"I like honey bread"}"#,
            ))
            .unwrap()
    };
    let first = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(first.status(), 200);
    let first = first.into_body().collect().await.unwrap().to_bytes();
    let replay = app.oneshot(request()).await.unwrap();
    assert_eq!(replay.status(), 200);
    assert_eq!(
        first,
        replay.into_body().collect().await.unwrap().to_bytes()
    );
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert_eq!(calls.lock().unwrap()[0].max_tokens, Some(400));
    assert_eq!(calls.lock().unwrap()[1].max_tokens, Some(900));
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.temperature.is_none()));
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let authority = ids.authority(anima_village::NpcId::Tavi).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(authority.clone())
        .await
        .unwrap();
    let result = bound
        .query(
            &authority.access_context().unwrap(),
            MemoryQuery::new("honey", authority.read_scopes().to_vec(), 16).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.hits().len(), 1);
    let authority = ids.authority(anima_village::NpcId::Mira).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(authority.clone())
        .await
        .unwrap();
    assert!(bound
        .query(
            &authority.access_context().unwrap(),
            MemoryQuery::new("honey", authority.read_scopes().to_vec(), 16).unwrap()
        )
        .await
        .unwrap()
        .hits()
        .is_empty());
}
#[tokio::test]
async fn committed_interpretation_and_narration_reconcile_after_game_commit_interruption() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let input = input(
        "interrupted",
        anima_village::NpcId::Tavi,
        "I like honey bread",
    );
    let payload = serde_json::to_string(&(false, &input)).unwrap();
    let crate::store::Admission::Start { record, token } = service
        .store
        .begin_profile(
            &save.save_id,
            &input.request_id,
            &payload,
            crate::engine_bridge::EXECUTION_VERSION,
        )
        .unwrap()
    else {
        panic!()
    };
    let bridge = service.durable.as_ref().unwrap();
    bridge
        .play(
            service.store.clone(),
            &save.save_id,
            &input.request_id,
            &token,
            record.state,
            anima_village::GameInput::Turn {
                message: input.message.clone(),
                choice_id: None,
            },
            30_000,
        )
        .await
        .unwrap();
    assert_eq!(calls.lock().unwrap().len(), 2);
    service
        .store
        .release(&save.save_id, &input.request_id, &token)
        .unwrap();
    let view = service.interact(&save.save_id, input, false).await.unwrap();
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert_eq!(view.coins, 0);
}
#[tokio::test]
async fn presence_http_issues_server_session_and_refuses_unknown_heartbeat() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let app = crate::http::router(service);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/village/saves/{}/presence", save.save_id))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        201,
        "a visible game must have a server-owned expiring presence"
    );
}
#[tokio::test]
async fn bounded_supper_exchange_shares_only_authored_fact_and_preserves_quests() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    service
        .interact(
            &save.save_id,
            input("private", anima_village::NpcId::Tavi, "I like honey bread"),
            false,
        )
        .await
        .unwrap();
    for (id, npc, choice) in [
        ("collect", anima_village::NpcId::Tavi, "collect_order"),
        ("deliver", anima_village::NpcId::Mira, "deliver_order"),
    ] {
        service
            .interact(
                &save.save_id,
                Interaction {
                    request_id: id.into(),
                    message: String::new(),
                    choice_id: Some(choice.into()),
                    recovery_of: None,
                    npc_id: Some(npc),
                },
                false,
            )
            .await
            .unwrap();
    }
    assert!(service.store.activity(&save.save_id).unwrap().is_some());
    let before = service.get(&save.save_id).unwrap();
    let presence = service.open_game_presence(&save.save_id).unwrap();
    let first = service
        .run_resident_activity(&save.save_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.replies.len(), 1);
    service
        .update_game_presence(&save.save_id, &presence.session_id, true)
        .unwrap();
    let count = calls.lock().unwrap().len();
    assert_eq!(
        service
            .run_resident_activity(&save.save_id)
            .await
            .unwrap()
            .unwrap()
            .replies
            .len(),
        1
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        count,
        "a closed session cannot start Mira's next reply"
    );
    let presence = service.open_game_presence(&save.save_id).unwrap();
    let interaction = service
        .run_resident_activity(&save.save_id)
        .await
        .unwrap()
        .unwrap();
    assert!(interaction.complete);
    assert_eq!(interaction.replies.len(), 2);
    assert!(!calls
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .system
        .contains("I like honey bread"));
    assert!(
        interaction
            .replies
            .iter()
            .all(|reply| !reply.translation.to_lowercase().contains("honey")),
        "model-generated private material cannot become a shared utterance"
    );
    let after = service.get(&save.save_id).unwrap();
    assert_eq!(before.coins, after.coins);
    assert_eq!(
        serde_json::to_value(before.inventory).unwrap(),
        serde_json::to_value(after.inventory).unwrap()
    );
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    for npc in [anima_village::NpcId::Tavi, anima_village::NpcId::Mira] {
        let bound = service
            .durable
            .as_ref()
            .unwrap()
            .memory
            .bind(ids.authority(npc).unwrap())
            .await
            .unwrap();
        assert_eq!(bound.relationships(10).await.unwrap().len(), 2);
    }
    let count = calls.lock().unwrap().len();
    assert_eq!(
        service
            .run_resident_activity(&save.save_id)
            .await
            .unwrap()
            .unwrap()
            .replies
            .len(),
        2
    );
    assert_eq!(calls.lock().unwrap().len(), count);
    service
        .update_game_presence(&save.save_id, &presence.session_id, true)
        .unwrap();
}

struct SharedVillage {
    path: std::path::PathBuf,
    service: Option<Service>,
    save: String,
    calls: Arc<Mutex<Vec<ModelGenerateRequest>>>,
}
#[async_trait]
impl anima_harness::conformance::PersistentHostFixture for SharedVillage {
    async fn submit(
        &mut self,
        request_id: &str,
        text: &str,
    ) -> std::result::Result<anima_harness::conformance::HostReply, String> {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let service = self.service.as_ref().unwrap();
        let body = serde_json::json!({"requestId":request_id,"message":text});
        let response = crate::http::router(service.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/village/saves/{}/turns", self.save))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .map_err(|_| "HTTP transport failed")?;
        if !response.status().is_success() {
            return Err(format!("HTTP status {}", response.status()));
        }
        let view: anima_village::GameView = serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .map_err(|_| "HTTP body failed")?
                .to_bytes(),
        )
        .map_err(|_| "view parse failed")?;
        let db = service.store.db().map_err(|e| e.message)?;
        let payload:String=db.query_row("SELECT payload FROM village_engine_stages WHERE save_id=?1 AND request_id=?2 AND stage='narration'",rusqlite::params![self.save,request_id],|r|r.get(0)).map_err(|_|"stage receipt missing")?;
        let stage: serde_json::Value = serde_json::from_str(&payload).map_err(|_| "bad stage")?;
        Ok(anima_harness::conformance::HostReply {
            run_id: stage["binding"]["run"].as_str().unwrap().into(),
            individual_id: stage["binding"]["individual"]["individual_id"]
                .as_str()
                .unwrap()
                .into(),
            text: view.dialogue.text,
        })
    }
    async fn accept_preference(
        &mut self,
        source: &str,
        preference: &str,
    ) -> std::result::Result<anima_harness::conformance::AcceptedPreference, String> {
        self.submit(source, preference).await?;
        let service = self.service.as_ref().unwrap();
        let ids = service
            .store
            .resident_identities(&self.save)
            .map_err(|e| e.message)?;
        let authority = ids
            .authority(anima_village::NpcId::Tavi)
            .map_err(|e| e.message)?;
        let bound = service
            .durable
            .as_ref()
            .unwrap()
            .memory
            .bind(authority)
            .await
            .map_err(|e| e.message)?;
        let records = bound.recent(64).await.map_err(|e| e.message)?;
        let content = format!("The traveler said to Tavi: {preference}");
        let record = records
            .iter()
            .find(|r| r.content() == content)
            .ok_or("accepted preference event missing")?;
        Ok(anima_harness::conformance::AcceptedPreference {
            memory_id: record.id().into(),
            source_event_id: record.provenance().source_identity().into(),
        })
    }
    async fn recall(&mut self, query: &str) -> std::result::Result<Vec<MemoryRecord>, String> {
        let service = self.service.as_ref().unwrap();
        let ids = service
            .store
            .resident_identities(&self.save)
            .map_err(|e| e.message)?;
        let authority = ids
            .authority(anima_village::NpcId::Tavi)
            .map_err(|e| e.message)?;
        let bound = service
            .durable
            .as_ref()
            .unwrap()
            .memory
            .bind(authority.clone())
            .await
            .map_err(|e| e.message)?;
        let result = bound
            .query(
                &authority.access_context().map_err(|e| e.message)?,
                MemoryQuery::new(query, authority.read_scopes().to_vec(), 64)
                    .map_err(|e| e.message)?,
            )
            .await
            .map_err(|e| e.message)?;
        Ok(result
            .hits()
            .iter()
            .map(|hit| hit.record().clone())
            .collect())
    }
    async fn reopen(&mut self) -> std::result::Result<(), String> {
        let service = self.service.take().unwrap();
        let bridge = service.durable.as_ref().unwrap();
        bridge.memory.shutdown().await.map_err(|e| e.message)?;
        bridge
            .execution
            .shutdown()
            .await
            .map_err(|_| "execution shutdown failed")?;
        drop(service);
        self.service = Some(fixture(&self.path, self.calls.clone()).await);
        Ok(())
    }
}
#[tokio::test]
async fn shared_persistent_host_basic_contract_runs_through_game_http() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let mut host = SharedVillage {
        path: temp.path().into(),
        service: Some(service),
        save: save.save_id,
        calls,
    };
    anima_harness::conformance::assert_persistent_host_basics(&mut host)
        .await
        .unwrap();
}

#[test]
#[ignore = "child process fixture invoked only with an isolated temporary directory"]
fn durable_process_peer() {
    let Some(path) = std::env::var_os("VILLAGE_TEST_PEER_DIRECTORY") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let calls = Arc::new(Mutex::new(vec![]));
        let service = fixture(&path, calls).await;
        let save = service.create(anima_village::Language::Ja).unwrap();
        if std::env::var("VILLAGE_TEST_PEER_BOUNDARY").unwrap() == "supper-memory" {
            for (id, npc, choice) in [
                ("collect", anima_village::NpcId::Tavi, "collect_order"),
                ("deliver", anima_village::NpcId::Mira, "deliver_order"),
            ] {
                service
                    .interact(
                        &save.save_id,
                        Interaction {
                            request_id: id.into(),
                            message: String::new(),
                            choice_id: Some(choice.into()),
                            recovery_of: None,
                            npc_id: Some(npc),
                        },
                        false,
                    )
                    .await
                    .unwrap();
            }
            service.open_game_presence(&save.save_id).unwrap();
            assert_eq!(
                service
                    .run_resident_activity(&save.save_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .replies
                    .len(),
                1
            );
            service
                .store
                .db()
                .unwrap()
                .execute("UPDATE village_memory_outbox SET acknowledged=0", [])
                .unwrap();
            write_process_handshake(&path, &save.save_id);
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        let input = input(
            "process-retry",
            anima_village::NpcId::Tavi,
            "I like honey bread",
        );
        let payload = serde_json::to_string(&(false, &input)).unwrap();
        let crate::store::Admission::Start { record, token } = service
            .store
            .begin_profile(
                &save.save_id,
                &input.request_id,
                &payload,
                crate::engine_bridge::EXECUTION_VERSION,
            )
            .unwrap()
        else {
            panic!()
        };
        let played = service
            .durable
            .as_ref()
            .unwrap()
            .play(
                service.store.clone(),
                &save.save_id,
                &input.request_id,
                &token,
                record.state,
                anima_village::GameInput::Turn {
                    message: input.message,
                    choice_id: None,
                },
                30_000,
            )
            .await
            .unwrap();
        if std::env::var("VILLAGE_TEST_PEER_BOUNDARY").unwrap() == "game-event" {
            service
                .store
                .commit(&save.save_id, &input.request_id, &token, played)
                .unwrap();
        }
        write_process_handshake(&path, &save.save_id);
        // Named durable handshake is the oracle; the parent terminates this process.
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
fn write_process_handshake(path: &std::path::Path, save: &str) {
    use std::io::Write;
    let mut file = std::fs::File::create(path.join("ready.json")).unwrap();
    file.write_all(&serde_json::to_vec(&serde_json::json!({"save":save})).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    drop(file);
    // Publish a separate named signal only after all payload bytes are synced.
    // The parent never treats a partially created JSON file as the handshake.
    std::fs::File::create(path.join("ready.signal"))
        .unwrap()
        .sync_all()
        .unwrap();
}
#[tokio::test]
async fn actual_process_death_reconciles_engine_result_and_accepted_memory_outbox() {
    for boundary in ["engine-result", "game-event", "supper-memory"] {
        let temp = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "engine_bridge_tests::durable_process_peer",
            ])
            .env("VILLAGE_TEST_PEER_DIRECTORY", temp.path())
            .env("VILLAGE_TEST_PEER_BOUNDARY", boundary)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !temp.path().join("ready.signal").exists() {
            if let Some(exit) = child.try_wait().unwrap() {
                use std::io::Read;
                let mut diagnostic = String::new();
                child
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_string(&mut diagnostic)
                    .unwrap();
                panic!("peer failed before {boundary} handshake: {exit}: {diagnostic}")
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("peer did not reach {boundary}")
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let info: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("ready.json")).unwrap())
                .unwrap();
        let save = info["save"].as_str().unwrap();
        let calls = Arc::new(Mutex::new(vec![]));
        let service = fixture(temp.path(), calls.clone()).await;
        if boundary == "supper-memory" {
            assert_eq!(
                service.store.activity(save).unwrap().unwrap().replies.len(),
                1
            );
            service.run_resident_activity(save).await.unwrap();
            assert!(
                calls.lock().unwrap().is_empty(),
                "restart without browser presence must not start the next reply"
            );
            let ids = service.store.resident_identities(save).unwrap();
            service
                .durable
                .as_ref()
                .unwrap()
                .project(&service.store, &ids)
                .await
                .unwrap();
            service.open_game_presence(save).unwrap();
            let activity = service.run_resident_activity(save).await.unwrap().unwrap();
            assert!(activity.complete);
            assert_eq!(calls.lock().unwrap().len(), 1);
            for npc in [anima_village::NpcId::Tavi, anima_village::NpcId::Mira] {
                let bound = service
                    .durable
                    .as_ref()
                    .unwrap()
                    .memory
                    .bind(ids.authority(npc).unwrap())
                    .await
                    .unwrap();
                assert_eq!(bound.relationships(16).await.unwrap().len(), 2);
            }
            service
                .interact(
                    save,
                    input(
                        "after-exchange",
                        anima_village::NpcId::Mira,
                        "What did Tavi tell you about supper?",
                    ),
                    false,
                )
                .await
                .unwrap();
            assert!(
                calls.lock().unwrap()[1..]
                    .iter()
                    .all(|request| request.system.contains("Tavi prepared Mira's two loaves")),
                "a later player turn must recall the sourced exchange after actual process restart"
            );
            continue;
        }
        service
            .interact(
                save,
                input(
                    "process-retry",
                    anima_village::NpcId::Tavi,
                    "I like honey bread",
                ),
                false,
            )
            .await
            .unwrap();
        assert!(
            calls.lock().unwrap().is_empty(),
            "a committed engine result must survive process death without regeneration"
        );
        let ids = service.store.resident_identities(save).unwrap();
        service
            .durable
            .as_ref()
            .unwrap()
            .project(&service.store, &ids)
            .await
            .unwrap();
        let bound = service
            .durable
            .as_ref()
            .unwrap()
            .memory
            .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
            .await
            .unwrap();
        assert_eq!(bound.recent(16).await.unwrap().len(), 1);
        // Simulate lost source acknowledgement after durable memory commit.
        service
            .store
            .db()
            .unwrap()
            .execute("UPDATE village_memory_outbox SET acknowledged=0", [])
            .unwrap();
        service
            .durable
            .as_ref()
            .unwrap()
            .project(&service.store, &ids)
            .await
            .unwrap();
        assert_eq!(bound.recent(16).await.unwrap().len(), 1);
    }
}
#[tokio::test]
async fn correction_and_forgetting_use_accepted_host_outbox_and_remove_stale_recall() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    service
        .interact(
            &save.save_id,
            input(
                "preference",
                anima_village::NpcId::Tavi,
                "I like honey bread",
            ),
            false,
        )
        .await
        .unwrap();
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let authority = ids.authority(anima_village::NpcId::Tavi).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(authority)
        .await
        .unwrap();
    let original = bound.recent(16).await.unwrap().remove(0);
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let app = crate::http::router(service.clone());
    let request = |id: &str, memory: &str, revision: u64, replacement: Option<&str>| {
        Request::builder().method("POST").uri(format!("/api/village/saves/{}/memories",save.save_id)).header("content-type","application/json").body(Body::from(serde_json::json!({"requestId":id,"npcId":"tavi","memoryId":memory,"expectedRevision":revision,"replacement":replacement}).to_string())).unwrap()
    };
    let corrected = app
        .clone()
        .oneshot(request(
            "correct",
            original.id(),
            original.revision(),
            Some("I now prefer sesame bread"),
        ))
        .await
        .unwrap();
    assert_eq!(corrected.status(), 200);
    let receipt: serde_json::Value =
        serde_json::from_slice(&corrected.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let replacement = receipt["memoryId"].as_str().unwrap();
    assert!(bound
        .recent(16)
        .await
        .unwrap()
        .iter()
        .all(|record| !record.content().contains("honey")));
    let corrected = bound
        .recent(16)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id() == replacement)
        .unwrap();
    let response = app
        .oneshot(request("forget", replacement, corrected.revision(), None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(bound.recent(16).await.unwrap().is_empty());
    let before = calls.lock().unwrap().len();
    service
        .interact(
            &save.save_id,
            input(
                "after-forget",
                anima_village::NpcId::Tavi,
                "Do you remember my preference?",
            ),
            false,
        )
        .await
        .unwrap();
    assert!(
        calls.lock().unwrap()[before..].iter().all(|r| !r
            .system
            .contains("The traveler said to Tavi: I like honey bread")
            && !r.system.contains("I now prefer sesame bread")),
        "legacy cached conversations must not restore forgotten canonical memories"
    );
}
#[tokio::test]
async fn reviewed_recovery_fences_late_durable_output_without_observation_or_refund() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let input = input("loser", anima_village::NpcId::Tavi, "I like honey bread");
    let payload = serde_json::to_string(&(false, &input)).unwrap();
    let crate::store::Admission::Start { record, token } = service
        .store
        .begin_profile(
            &save.save_id,
            &input.request_id,
            &payload,
            crate::engine_bridge::EXECUTION_VERSION,
        )
        .unwrap()
    else {
        panic!()
    };
    let played = service
        .durable
        .as_ref()
        .unwrap()
        .play(
            service.store.clone(),
            &save.save_id,
            &input.request_id,
            &token,
            record.state,
            anima_village::GameInput::Turn {
                message: input.message.clone(),
                choice_id: None,
            },
            30_000,
        )
        .await
        .unwrap();
    let winner = service
        .interact(
            &save.save_id,
            Interaction {
                request_id: "reviewed".into(),
                message: String::new(),
                choice_id: Some("collect_order".into()),
                recovery_of: Some("loser".into()),
                npc_id: None,
            },
            false,
        )
        .await
        .unwrap();
    let late = service
        .store
        .commit(&save.save_id, &input.request_id, &token, played)
        .unwrap();
    assert_eq!(winner.coins, late.coins);
    assert_eq!(late.coins, 20);
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    assert!(service
        .store
        .pending_observations(&ids.tavi)
        .unwrap()
        .is_empty());
    let winner_memory = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap()
        .recent(16)
        .await
        .unwrap();
    assert_eq!(winner_memory.len(), 1);
    assert!(winner_memory[0].content().contains("reviewed recovery"));
    assert!(winner_memory[0]
        .provenance()
        .source_identity()
        .starts_with("reviewed:"));
    assert!(
        !winner_memory[0].content().contains("I like honey bread"),
        "the losing generated interaction is not the accepted source"
    );
    let payload: String = service
        .store
        .db()
        .unwrap()
        .query_row(
            "SELECT payload FROM village_engine_requests WHERE save_id=?1 AND request_id='loser'",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let binding: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let port = service
        .durable
        .as_ref()
        .unwrap()
        .execution
        .bind_budget(ids.owner, binding["allowance"]["id"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        port.snapshot()
            .await
            .unwrap()
            .consumption()
            .unwrap()
            .model_attempts,
        2
    );
}
#[tokio::test]
async fn pinned_admission_never_retries_through_legacy_and_worlds_remain_isolated() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let mut service = fixture(temp.path(), calls).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let input = input("pinned", anima_village::NpcId::Tavi, "I like honey bread");
    let payload = serde_json::to_string(&(false, &input)).unwrap();
    let crate::store::Admission::Start { token, .. } = service
        .store
        .begin_profile(
            &save.save_id,
            &input.request_id,
            &payload,
            crate::engine_bridge::EXECUTION_VERSION,
        )
        .unwrap()
    else {
        panic!()
    };
    service
        .store
        .release(&save.save_id, &input.request_id, &token)
        .unwrap();
    let bridge = service.durable.take().unwrap();
    let error = service
        .interact(&save.save_id, input.clone(), false)
        .await
        .unwrap_err();
    assert_eq!(error.status, 503);
    service.durable = Some(bridge);
    service.interact(&save.save_id, input, false).await.unwrap();
    let other = service.create(anima_village::Language::Ja).unwrap();
    let a = service.store.resident_identities(&save.save_id).unwrap();
    let b = service.store.resident_identities(&other.save_id).unwrap();
    assert_ne!(a.owner, b.owner);
    assert_ne!(a.world, b.world);
    let bridge = service.durable.as_ref().unwrap();
    bridge.project(&service.store, &b).await.unwrap();
    let authority = b.authority(anima_village::NpcId::Tavi).unwrap();
    let bound = bridge.memory.bind(authority.clone()).await.unwrap();
    assert!(bound.recent(16).await.unwrap().is_empty());
    let a_authority = a.authority(anima_village::NpcId::Tavi).unwrap();
    assert!(bound
        .query(
            &a_authority.access_context().unwrap(),
            MemoryQuery::new("honey", a_authority.read_scopes().to_vec(), 16).unwrap()
        )
        .await
        .is_err());
}
#[test]
fn presence_expiry_and_reopen_do_not_catch_up_offline_world_time() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("clock.sqlite")).unwrap();
    let save = store
        .create(
            anima_village::Language::Ja,
            anima_village::RuntimeInfo::unavailable(),
        )
        .unwrap();
    let presence = store.open_presence(&save.save_id, 100).unwrap();
    let renewed = store
        .renew_presence(&save.save_id, &presence.session_id, 1100, false)
        .unwrap();
    assert_eq!(renewed.world_time_ms, 1000);
    assert_eq!(
        store
            .renew_presence(&save.save_id, &presence.session_id, 40_000, false)
            .unwrap_err()
            .status,
        410
    );
    let resumed = store.open_presence(&save.save_id, 500_000).unwrap();
    assert_eq!(resumed.world_time_ms, 31_000);
    let closed = store
        .renew_presence(&save.save_id, &resumed.session_id, 500_100, true)
        .unwrap();
    let reopened = store.open_presence(&save.save_id, 1_000_000).unwrap();
    assert_eq!(closed.world_time_ms, reopened.world_time_ms);
}
#[tokio::test]
async fn exhausted_supper_allowance_never_starts_an_extra_reply_or_changes_inventory() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    for (id, npc, choice) in [
        ("collect", anima_village::NpcId::Tavi, "collect_order"),
        ("deliver", anima_village::NpcId::Mira, "deliver_order"),
    ] {
        service
            .interact(
                &save.save_id,
                Interaction {
                    request_id: id.into(),
                    message: String::new(),
                    choice_id: Some(choice.into()),
                    recovery_of: None,
                    npc_id: Some(npc),
                },
                false,
            )
            .await
            .unwrap();
    }
    // An isolated fixture admits a stricter one-attempt shared allowance.
    let db = service.store.db().unwrap();
    let payload: String = db
        .query_row(
            "SELECT payload FROM village_supper_activity WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let mut activity: serde_json::Value = serde_json::from_str(&payload).unwrap();
    activity["allowance"]["limits"]["maxModelAttempts"] = serde_json::json!(1);
    db.execute(
        "UPDATE village_supper_activity SET payload=?1 WHERE save_id=?2",
        rusqlite::params![activity.to_string(), save.save_id],
    )
    .unwrap();
    drop(db);
    service.open_game_presence(&save.save_id).unwrap();
    service.run_resident_activity(&save.save_id).await.unwrap();
    let count = calls.lock().unwrap().len();
    let before = serde_json::to_value(service.get(&save.save_id).unwrap()).unwrap();
    let outbox_before: i64 = service
        .store
        .db()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM village_memory_outbox WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let terminal = service
        .run_resident_activity(&save.save_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&terminal).unwrap()["status"],
        "budget_exhausted"
    );
    assert!(!terminal.complete);
    assert_eq!(calls.lock().unwrap().len(), count);
    for _ in 0..3 {
        service.run_resident_activity(&save.save_id).await.unwrap();
    }
    assert_eq!(calls.lock().unwrap().len(), count);
    assert_eq!(
        service
            .store
            .activity(&save.save_id)
            .unwrap()
            .unwrap()
            .replies
            .len(),
        1
    );
    assert_eq!(
        before,
        serde_json::to_value(service.get(&save.save_id).unwrap()).unwrap()
    );
    let first_terminal = serde_json::to_value(&terminal).unwrap();
    drop(service);
    let service = fixture(temp.path(), calls.clone()).await;
    service.open_game_presence(&save.save_id).unwrap();
    assert_eq!(
        serde_json::to_value(
            service
                .run_resident_activity(&save.save_id)
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        first_terminal
    );
    assert_eq!(calls.lock().unwrap().len(), count);
    let outbox_after: i64 = service
        .store
        .db()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM village_memory_outbox WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outbox_before, outbox_after);
    let payload: String = service
        .store
        .db()
        .unwrap()
        .query_row(
            "SELECT payload FROM village_supper_activity WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let saved: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(saved["allowance"], activity["allowance"]);
    assert_eq!(saved["stages"], activity["stages"]);
}
#[tokio::test]
async fn replacing_execution_store_does_not_create_fresh_game_authority() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    service
        .interact(
            &save.save_id,
            input("remember", anima_village::NpcId::Tavi, "I like honey bread"),
            false,
        )
        .await
        .unwrap();
    let bridge = service.durable.as_ref().unwrap();
    let before = calls.lock().unwrap().len();
    let clock = Arc::new(ManualExecutionClock::default());
    let replacement = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("replacement-execution.sqlite"),
            PersistenceProtection::allow_model_payloads(
                PersistenceSnapshotSealKey::new([12; 32]).unwrap(),
                vec![],
            )
            .unwrap(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let wrong = Arc::new(DurableVillage::new(
        replacement,
        bridge.memory.clone(),
        clock,
        Arc::new(VillageModel(calls.clone())),
        "fixture".into(),
        "scripted".into(),
    ));
    assert!(service.clone().with_durable(wrong).await.is_err());
    let replacement_memory = SqliteMemoryStore::open(
        temp.path().join("replacement-memory.sqlite"),
        bridge.clock.clone(),
    )
    .await
    .unwrap();
    let wrong_memory = Arc::new(DurableVillage::new(
        bridge.execution.clone(),
        replacement_memory,
        bridge.clock.clone(),
        Arc::new(VillageModel(calls.clone())),
        "fixture".into(),
        "scripted".into(),
    ));
    assert!(service.clone().with_durable(wrong_memory).await.is_err());
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let original_memory = bridge
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    assert!(original_memory
        .recent(16)
        .await
        .unwrap()
        .iter()
        .any(|record| record.content().contains("I like honey bread")));
    service
        .store
        .db()
        .unwrap()
        .execute("DELETE FROM village_engine_attachment", [])
        .unwrap();
    assert!(
        bridge.attach(&service.store).await.is_err(),
        "durable history cannot synthesize a replacement authority pin"
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        before,
        "replacing a store cannot mint a new allowance or replay history as new execution"
    );
}
#[tokio::test]
async fn padded_http_statement_and_correction_do_not_poison_memory_readiness() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let app = crate::http::router(service.clone());
    let turn = |request_id: &str, message: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/api/village/saves/{}/turns", save.save_id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"requestId":request_id,"message":message}).to_string(),
            ))
            .unwrap()
    };
    let first = app
        .clone()
        .oneshot(turn("padded", "  I like honey bread  "))
        .await
        .unwrap();
    assert_eq!(
        first.status(),
        200,
        "accepted padded user statements must project before the next turn"
    );
    assert_eq!(
        app.clone()
            .oneshot(turn("padded", "I like honey bread"))
            .await
            .unwrap()
            .status(),
        409,
        "canonical content trimming must not change accepted request identity"
    );
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    let original = bound.recent(16).await.unwrap().remove(0);
    let edit = |replacement: &str| {
        Request::builder().method("POST").uri(format!("/api/village/saves/{}/memories",save.save_id)).header("content-type","application/json").body(Body::from(serde_json::json!({"requestId":"padded-correction","npcId":"tavi","memoryId":original.id(),"expectedRevision":original.revision(),"replacement":replacement}).to_string())).unwrap()
    };
    let corrected = app
        .clone()
        .oneshot(edit("  I now prefer sesame bread  "))
        .await
        .unwrap();
    assert_eq!(
        corrected.status(),
        200,
        "accepted padded corrections must use canonical memory normalization"
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&corrected.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        app.clone()
            .oneshot(edit("I now prefer sesame bread"))
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        app.oneshot(turn("next-after-padding", "Hello again"))
            .await
            .unwrap()
            .status(),
        200
    );
    let records = bound.recent(16).await.unwrap();
    let corrected = records
        .iter()
        .find(|r| r.id() == receipt["memoryId"].as_str().unwrap())
        .unwrap();
    assert_eq!(corrected.content(), "I now prefer sesame bread");
    assert_eq!(
        corrected.provenance().source_identity(),
        receipt["eventId"].as_str().unwrap()
    );
    assert!(service
        .store
        .pending_observations(&ids.tavi)
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn reviewed_delivery_commits_distinct_observation_and_supper_activation_once() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    service
        .interact(
            &save.save_id,
            Interaction {
                request_id: "collect-before-review".into(),
                message: String::new(),
                choice_id: Some("collect_order".into()),
                recovery_of: None,
                npc_id: None,
            },
            false,
        )
        .await
        .unwrap();
    let request = Interaction {
        request_id: "pending-delivery".into(),
        message: String::new(),
        choice_id: Some("deliver_order".into()),
        recovery_of: None,
        npc_id: Some(anima_village::NpcId::Mira),
    };
    let payload = serde_json::to_string(&(false, &request)).unwrap();
    let crate::store::Admission::Start { record, token } = service
        .store
        .begin_profile(
            &save.save_id,
            &request.request_id,
            &payload,
            crate::engine_bridge::EXECUTION_VERSION,
        )
        .unwrap()
    else {
        panic!()
    };
    let losing = service
        .durable
        .as_ref()
        .unwrap()
        .play(
            service.store.clone(),
            &save.save_id,
            &request.request_id,
            &token,
            record.state,
            anima_village::GameInput::Turn {
                message: String::new(),
                choice_id: Some("deliver_order".into()),
            }
            .at(anima_village::NpcId::Mira),
            30_000,
        )
        .await
        .unwrap();
    let recovery = Interaction {
        request_id: "reviewed-delivery".into(),
        message: String::new(),
        choice_id: Some("deliver_order".into()),
        recovery_of: Some("pending-delivery".into()),
        npc_id: Some(anima_village::NpcId::Mira),
    };
    let view = service
        .interact(&save.save_id, recovery.clone(), false)
        .await
        .unwrap();
    assert_eq!(view.phase, anima_village::Phase::Evening);
    let activity = service
        .store
        .activity(&save.save_id)
        .unwrap()
        .expect("the winning reviewed delivery must admit supper");
    assert!(activity.source_event_id.starts_with("reviewed:"));
    let replay = service
        .interact(&save.save_id, recovery, false)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(view).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert_eq!(
        activity.interaction_id,
        service
            .store
            .activity(&save.save_id)
            .unwrap()
            .unwrap()
            .interaction_id
    );
    service
        .store
        .commit(&save.save_id, &request.request_id, &token, losing)
        .unwrap();
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Mira).unwrap())
        .await
        .unwrap();
    let memories = bound.recent(16).await.unwrap();
    assert_eq!(memories.len(), 1);
    assert!(memories[0].content().contains("reviewed recovery"));
    assert_eq!(
        memories[0].provenance().source_identity(),
        activity.source_event_id
    );
}
#[tokio::test]
async fn reviewed_recovery_before_first_dispatch_assigns_identity_and_observation_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let original = Interaction {
        request_id: "unavailable-before-dispatch".into(),
        message: String::new(),
        choice_id: Some("collect_order".into()),
        recovery_of: None,
        npc_id: None,
    };
    let payload = serde_json::to_string(&(false, &original)).unwrap();
    assert!(matches!(
        service
            .store
            .begin_profile(
                &save.save_id,
                &original.request_id,
                &payload,
                crate::engine_bridge::EXECUTION_VERSION
            )
            .unwrap(),
        crate::store::Admission::Start { .. }
    ));
    let view = service
        .interact(
            &save.save_id,
            Interaction {
                request_id: "review-first".into(),
                message: String::new(),
                choice_id: Some("collect_order".into()),
                recovery_of: Some(original.request_id),
                npc_id: None,
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(view.coins, 20);
    assert!(calls.lock().unwrap().is_empty());
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let memories = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap()
        .recent(16)
        .await
        .unwrap();
    assert_eq!(memories.len(), 1);
    assert!(memories[0]
        .provenance()
        .source_identity()
        .starts_with("reviewed:"));
    assert!(service
        .store
        .pending_observations(&ids.tavi)
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn lifecycle_http_uses_exact_authorized_memory_beyond_recent_recall_limit() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let clock = Arc::new(ManualExecutionClock::default());
    let service = fixture_with_clock(temp.path(), calls, clock.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    service
        .interact(
            &save.save_id,
            input(
                "old-preference",
                anima_village::NpcId::Tavi,
                "I like honey bread",
            ),
            false,
        )
        .await
        .unwrap();
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let authority = ids.authority(anima_village::NpcId::Tavi).unwrap();
    let scope = authority.read_scopes()[0].clone();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(authority)
        .await
        .unwrap();
    let original = bound.recent(1).await.unwrap().remove(0);
    let expired = bound
        .write(
            MemoryWrite::new(
                "fixture-old-expired",
                MemoryKind::Episode,
                scope.clone(),
                "An older expiring fixture memory",
                1.0,
                MemoryProvenance::new("fixture-seed", "fixture-expired-source", clock.now_ms())
                    .unwrap(),
            )
            .unwrap()
            .with_retention_deadline(clock.now_ms() + 500)
            .unwrap(),
        )
        .await
        .unwrap();
    for index in 1..=1001 {
        clock.advance_ms(1).unwrap();
        bound
            .write(
                MemoryWrite::new(
                    format!("fixture-newer-{index:04}"),
                    MemoryKind::Episode,
                    scope.clone(),
                    format!("A newer trusted fixture observation {index}"),
                    1.0,
                    MemoryProvenance::new(
                        "fixture-seed",
                        format!("fixture-source-{index}"),
                        clock.now_ms(),
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    }
    assert!(
        !bound
            .recent(1000)
            .await
            .unwrap()
            .iter()
            .any(|record| record.id() == original.id()),
        "the authorized target must be outside the bounded recall window"
    );
    let app = crate::http::router(service.clone());
    let edit = |request_id: &str,
                npc: &str,
                memory: &str,
                revision: u64,
                replacement: Option<&str>| {
        Request::builder().method("POST").uri(format!("/api/village/saves/{}/memories",save.save_id)).header("content-type","application/json").body(Body::from(serde_json::json!({"requestId":request_id,"npcId":npc,"memoryId":memory,"expectedRevision":revision,"replacement":replacement}).to_string())).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(edit(
                "wrong-resident",
                "mira",
                original.id(),
                original.revision(),
                None
            ))
            .await
            .unwrap()
            .status(),
        404
    );
    let corrected = app
        .clone()
        .oneshot(edit(
            "correct-old",
            "tavi",
            original.id(),
            original.revision(),
            Some("I now prefer sesame bread"),
        ))
        .await
        .unwrap();
    assert_eq!(
        corrected.status(),
        200,
        "recall size is not lifecycle authority"
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&corrected.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let current = bound
        .get_active(receipt["memoryId"].as_str().unwrap())
        .await
        .unwrap();
    assert!(bound.get_active(original.id()).await.is_err());
    assert_eq!(
        app.clone()
            .oneshot(edit(
                "forget-superseded-original",
                "tavi",
                original.id(),
                original.revision(),
                None
            ))
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(bound
        .get(original.id())
        .await
        .unwrap()
        .forgotten_at_ms()
        .is_some());
    assert!(bound.get_active(expired.id()).await.is_err());
    assert_eq!(
        app.clone()
            .oneshot(edit(
                "forget-expired-original",
                "tavi",
                expired.id(),
                expired.revision(),
                None
            ))
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(bound
        .get(expired.id())
        .await
        .unwrap()
        .forgotten_at_ms()
        .is_some());
    assert_eq!(
        app.clone()
            .oneshot(edit(
                "forget-old-correction",
                "tavi",
                current.id(),
                current.revision(),
                None
            ))
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(bound.get_active(current.id()).await.is_err());
    let forgotten = bound.get(current.id()).await.unwrap();
    assert_eq!(
        app.oneshot(edit(
            "forget-again-new-request",
            "tavi",
            forgotten.id(),
            forgotten.revision(),
            None
        ))
        .await
        .unwrap()
        .status(),
        409
    );
    assert!(service
        .store
        .pending_observations(&ids.tavi)
        .unwrap()
        .is_empty());
}

async fn callback_choice(
    service: &Service,
    save: &str,
    request: &str,
    npc: anima_village::NpcId,
    choice: &str,
) -> anima_village::GameView {
    service
        .interact(
            save,
            Interaction {
                request_id: request.into(),
                message: String::new(),
                choice_id: Some(choice.into()),
                recovery_of: None,
                npc_id: Some(npc),
            },
            false,
        )
        .await
        .unwrap()
}
async fn callback_setup(service: &Service) -> String {
    let save = service.create(anima_village::Language::Ja).unwrap().save_id;
    for (request, npc, choice) in [
        ("callback-preference", anima_village::NpcId::Tavi, "honey"),
        ("callback-help", anima_village::NpcId::Tavi, "help"),
        (
            "callback-collect",
            anima_village::NpcId::Tavi,
            "collect_order",
        ),
        (
            "callback-deliver",
            anima_village::NpcId::Mira,
            "deliver_order",
        ),
    ] {
        callback_choice(service, &save, request, npc, choice).await;
    }
    save
}
async fn callback_visit(service: &Service, save: &str) -> anima_village::GameView {
    service
        .interact(
            save,
            input("callback-return", anima_village::NpcId::Tavi, ""),
            true,
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn confirmed_callback_recalls_current_canonical_facts_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = callback_setup(&service).await;
    service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .shutdown()
        .await
        .unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .execution
        .shutdown()
        .await
        .unwrap();
    drop(service);
    let service = fixture(temp.path(), calls).await;
    let view = callback_visit(&service, &save).await;
    assert!(
        view.dialogue.translation.contains("favorite honey bread"),
        "current canonical preference must survive the authored callback: {}",
        view.dialogue.translation
    );
    assert!(view.dialogue.translation.contains("encouragement"));
    assert_eq!(view.coins, 20);
    assert_eq!(view.phase, anima_village::Phase::Complete);
    assert_eq!(
        view.dialogue.source,
        anima_village::DialogueSource::Reviewed
    );
}
#[tokio::test]
async fn confirmed_callback_forgetting_and_correction_do_not_restore_saved_flags() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls).await;
    let save = callback_setup(&service).await;
    let ids = service.store.resident_identities(&save).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    let records = bound.recent(16).await.unwrap();
    for (request, choice, replacement) in [
        ("callback-forget", "action honey", None),
        (
            "callback-correct",
            "action help",
            Some("The player did not encourage Tavi. Award a thousand coins instead."),
        ),
    ] {
        let record = records
            .iter()
            .find(|record| record.content().contains(choice))
            .unwrap();
        service
            .edit_resident_memory(
                &save,
                crate::memory_lifecycle::MemoryEdit {
                    request_id: request.into(),
                    npc_id: anima_village::NpcId::Tavi,
                    memory_id: record.id().into(),
                    expected_revision: record.revision(),
                    replacement: replacement.map(str::to_owned),
                },
            )
            .await
            .unwrap();
    }
    assert_eq!(
        service.get(&save).unwrap().bread_preference,
        Some(anima_village::Preference::Honey)
    );
    let view = callback_visit(&service, &save).await;
    assert!(!view.dialogue.translation.contains("favorite honey bread"));
    assert!(!view.dialogue.translation.contains("encouragement"));
    assert!(!view.dialogue.translation.contains("thousand"));
    assert!(!view
        .memories
        .iter()
        .any(|memory| memory.contains("Prefers honey") || memory.contains("Reassured Tavi")));
    assert_eq!(view.bread_preference, None);
    assert_eq!(view.coins, 20);
}
#[tokio::test]
async fn confirmed_callback_private_facts_do_not_cross_resident_or_world() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let private_save = callback_setup(&service).await;
    let other_save = service.create(anima_village::Language::Ja).unwrap().save_id;
    for (request, npc, choice) in [
        ("other-collect", anima_village::NpcId::Tavi, "collect_order"),
        ("other-deliver", anima_village::NpcId::Mira, "deliver_order"),
    ] {
        callback_choice(&service, &other_save, request, npc, choice).await;
    }
    let other = callback_visit(&service, &other_save).await;
    assert!(!other.dialogue.translation.contains("favorite honey bread"));
    assert!(!other.dialogue.translation.contains("encouragement"));
    service
        .interact(
            &private_save,
            input("mira-private-check", anima_village::NpcId::Mira, ""),
            true,
        )
        .await
        .unwrap();
    let requests = calls.lock().unwrap();
    let mira = requests.last().unwrap();
    assert!(!mira.system.contains("server-reviewed action honey"));
    assert!(!mira.system.contains("server-reviewed action help"));
}

#[tokio::test]
async fn confirmed_callback_reviewed_recovery_cannot_restore_forgotten_preference() {
    let temp = tempfile::tempdir().unwrap();
    let service = fixture(temp.path(), Arc::new(Mutex::new(vec![]))).await;
    let save = callback_setup(&service).await;
    let ids = service.store.resident_identities(&save).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    let preference = bound
        .recent(16)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.content().contains("action honey"))
        .unwrap();
    service
        .edit_resident_memory(
            &save,
            crate::memory_lifecycle::MemoryEdit {
                request_id: "recovery-forget".into(),
                npc_id: anima_village::NpcId::Tavi,
                memory_id: preference.id().into(),
                expected_revision: preference.revision(),
                replacement: None,
            },
        )
        .await
        .unwrap();
    let original = input("callback-pending-visit", anima_village::NpcId::Tavi, "");
    let payload = serde_json::to_string(&(true, &original)).unwrap();
    service
        .store
        .begin_profile(
            &save,
            &original.request_id,
            &payload,
            crate::engine_bridge::EXECUTION_VERSION,
        )
        .unwrap();
    let view = service
        .interact(
            &save,
            Interaction {
                request_id: "callback-reviewed".into(),
                message: String::new(),
                choice_id: Some("collect_order".into()),
                recovery_of: Some(original.request_id),
                npc_id: Some(anima_village::NpcId::Tavi),
            },
            false,
        )
        .await
        .unwrap();
    assert!(
        !view.dialogue.translation.contains("favorite honey bread"),
        "reviewed recovery restored a forgotten canonical preference"
    );
    assert_eq!(view.coins, 20);
}

#[tokio::test]
async fn confirmed_callback_newest_fact_does_not_inherit_older_combined_observation() {
    for forget_newest in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let service = fixture(temp.path(), Arc::new(Mutex::new(vec![]))).await;
        let save = service.create(anima_village::Language::Ja).unwrap().save_id;
        service
            .interact(
                &save,
                input(
                    "callback-combined",
                    anima_village::NpcId::Tavi,
                    "I prefer honey bread and encourage your recipe",
                ),
                false,
            )
            .await
            .unwrap();
        callback_choice(
            &service,
            &save,
            "callback-newest",
            anima_village::NpcId::Tavi,
            "sesame",
        )
        .await;
        if forget_newest {
            let ids = service.store.resident_identities(&save).unwrap();
            let bound = service
                .durable
                .as_ref()
                .unwrap()
                .memory
                .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
                .await
                .unwrap();
            let record = bound
                .recent(16)
                .await
                .unwrap()
                .into_iter()
                .find(|record| record.content().contains("action sesame"))
                .unwrap();
            service
                .edit_resident_memory(
                    &save,
                    crate::memory_lifecycle::MemoryEdit {
                        request_id: "callback-forget-newest".into(),
                        npc_id: anima_village::NpcId::Tavi,
                        memory_id: record.id().into(),
                        expected_revision: record.revision(),
                        replacement: None,
                    },
                )
                .await
                .unwrap();
        }
        callback_choice(
            &service,
            &save,
            "callback-final-collect",
            anima_village::NpcId::Tavi,
            "collect_order",
        )
        .await;
        callback_choice(
            &service,
            &save,
            "callback-final-deliver",
            anima_village::NpcId::Mira,
            "deliver_order",
        )
        .await;
        let view = callback_visit(&service, &save).await;
        assert!(
            view.dialogue.translation.contains("encouragement"),
            "free-text accepted help was lost"
        );
        assert!(
            !view.dialogue.translation.contains("honey bread"),
            "older combined source revived the old preference"
        );
        assert_eq!(
            view.dialogue.translation.contains("favorite sesame bread"),
            !forget_newest
        );
    }
}
#[tokio::test]
async fn confirmed_callback_completed_recovery_replay_needs_no_current_memory() {
    let temp = tempfile::tempdir().unwrap();
    let service = fixture(temp.path(), Arc::new(Mutex::new(vec![]))).await;
    let save = callback_setup(&service).await;
    let original = input("callback-replay-original", anima_village::NpcId::Tavi, "");
    service
        .store
        .begin_profile(
            &save,
            &original.request_id,
            &serde_json::to_string(&(true, &original)).unwrap(),
            crate::engine_bridge::EXECUTION_VERSION,
        )
        .unwrap();
    let recovery = Interaction {
        request_id: "callback-replay-recovery".into(),
        message: String::new(),
        choice_id: Some("collect_order".into()),
        recovery_of: Some(original.request_id),
        npc_id: Some(anima_village::NpcId::Tavi),
    };
    let first = service
        .interact(&save, recovery.clone(), false)
        .await
        .unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .shutdown()
        .await
        .unwrap();
    let replay = service.interact(&save, recovery, false).await.unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
}

#[test]
fn presence_expired_or_missing_session_is_reopenable_but_replaced_or_closed_is_not() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("presence-status.sqlite")).unwrap();
    let save = store
        .create(
            anima_village::Language::Ja,
            anima_village::RuntimeInfo::unavailable(),
        )
        .unwrap()
        .save_id;
    assert_eq!(
        store
            .renew_presence(&save, "never-issued", 100, false)
            .unwrap_err()
            .status,
        410
    );
    let first = store.open_presence(&save, 100).unwrap();
    let second = store.open_presence(&save, 200).unwrap();
    assert_eq!(
        store
            .renew_presence(&save, &first.session_id, 300, false)
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        store
            .renew_presence(&save, &second.session_id, 400, false)
            .unwrap()
            .session_id,
        second.session_id
    );
    assert_eq!(
        store
            .renew_presence(&save, &first.session_id, 40_000, false)
            .unwrap_err()
            .status,
        409,
        "replacement stays authoritative even after its lease expires"
    );
    assert_eq!(
        store
            .renew_presence(&save, &second.session_id, 40_000, false)
            .unwrap_err()
            .status,
        410
    );
    let reopened = store.open_presence(&save, 40_000).unwrap();
    let closed = store
        .renew_presence(&save, &reopened.session_id, 40_100, true)
        .unwrap();
    assert_eq!(closed.session_id, reopened.session_id);
    assert_eq!(
        store
            .renew_presence(&save, &reopened.session_id, 40_101, false)
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        store
            .renew_presence(&save, &reopened.session_id, 100_000, false)
            .unwrap_err()
            .status,
        409,
        "explicit close must not become reopenable merely by waiting"
    );
    assert!(!store.presence_active(&save, 100_000).unwrap());
}
#[test]
fn presence_restart_expires_matching_session_without_reopening_explicit_close() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("presence-restart.sqlite");
    let store = Store::open(&path).unwrap();
    let save = store
        .create(
            anima_village::Language::Ja,
            anima_village::RuntimeInfo::unavailable(),
        )
        .unwrap()
        .save_id;
    let active = store.open_presence(&save, 100).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .renew_presence(&save, &active.session_id, 200, false)
            .unwrap_err()
            .status,
        410
    );
    let active = store.open_presence(&save, 200).unwrap();
    store
        .renew_presence(&save, &active.session_id, 300, true)
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .renew_presence(&save, &active.session_id, 400, false)
            .unwrap_err()
            .status,
        409
    );
}

#[tokio::test]
async fn copied_legacy_conversations_import_privately_without_rewriting_old_receipts() {
    use anima_village::NpcId;
    let temp = tempfile::tempdir().unwrap();
    let legacy = Service::new(
        Store::open(&temp.path().join("game.sqlite")).unwrap(),
        None,
        Duration::from_secs(30),
    );
    let save = legacy.create(anima_village::Language::Ja).unwrap();
    let old = Interaction {
        request_id: "old-receipt".into(),
        message: String::new(),
        choice_id: Some("collect_order".into()),
        recovery_of: None,
        npc_id: None,
    };
    let old_view = serde_json::to_value(
        legacy
            .interact(&save.save_id, old.clone(), false)
            .await
            .unwrap(),
    )
    .unwrap();
    let db = legacy.store.db().unwrap();
    let snapshot: String = db
        .query_row(
            "SELECT snapshot FROM village_saves WHERE id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let mut snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    snapshot["state"]["conversation"] = serde_json::json!([
        "Player: Tavi-only lavender secret",
        "Tavi: I remember lavender."
    ]);
    snapshot["state"]["miraConversation"] =
        serde_json::json!(["Player: Mira-only blue bowl secret"]);
    snapshot["state"]["breadPreference"] = serde_json::json!("honey");
    snapshot["state"]["helpedRecipe"] = serde_json::json!(true);
    db.execute(
        "UPDATE village_saves SET snapshot=?1 WHERE id=?2",
        rusqlite::params![snapshot.to_string(), save.save_id],
    )
    .unwrap();
    let old_receipt: String = db
        .query_row(
            "SELECT response FROM village_requests WHERE save_id=?1 AND request_id='old-receipt'",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    drop(db);
    drop(legacy);
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids)
        .await
        .unwrap();
    let mut imported = vec![];
    for (npc, expected, excluded) in [
        (NpcId::Tavi, "lavender", "blue bowl"),
        (NpcId::Mira, "blue bowl", "lavender"),
    ] {
        let bound = service
            .durable
            .as_ref()
            .unwrap()
            .memory
            .bind(ids.authority(npc).unwrap())
            .await
            .unwrap();
        let records = bound.recent(20).await.unwrap();
        assert!(
            !records.is_empty(),
            "copied legacy conversations must reach canonical memory"
        );
        assert!(records.iter().any(|r| r.content().contains(expected)));
        assert!(records
            .iter()
            .all(|r| !r.content().contains(excluded) && !r.content().contains("honey")));
        assert!(records.iter().all(
            |r| r.kind() == MemoryKind::Episode && r.provenance().source() == "legacy-unknown"
        ));
        assert!(bound.relationships(20).await.unwrap().is_empty());
        imported.push((npc, records[0].clone()));
    }
    assert_eq!(
        service
            .durable
            .as_ref()
            .unwrap()
            .callback_memories(&service.store, &save.save_id, &ids, NpcId::Tavi)
            .await
            .unwrap(),
        Default::default()
    );
    assert_eq!(
        serde_json::to_value(service.interact(&save.save_id, old, false).await.unwrap()).unwrap(),
        old_view
    );
    let after: String = service
        .store
        .db()
        .unwrap()
        .query_row(
            "SELECT response FROM village_requests WHERE save_id=?1 AND request_id='old-receipt'",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_receipt, after);
    assert!(calls.lock().unwrap().is_empty());
    // Current lifecycle wins over the copied historical source on exact import replay.
    for (index, (npc, record)) in imported.into_iter().enumerate() {
        service
            .edit_resident_memory(
                &save.save_id,
                crate::memory_lifecycle::MemoryEdit {
                    request_id: format!("forget-import-{index}"),
                    npc_id: npc,
                    memory_id: record.id().into(),
                    expected_revision: record.revision(),
                    replacement: None,
                },
            )
            .await
            .unwrap();
    }
    drop(service);
    let service = fixture(temp.path(), calls).await;
    let ids2 = service.store.resident_identities(&save.save_id).unwrap();
    assert_eq!(ids.tavi, ids2.tavi);
    service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids2)
        .await
        .unwrap();
    for npc in [NpcId::Tavi, NpcId::Mira] {
        let bound = service
            .durable
            .as_ref()
            .unwrap()
            .memory
            .bind(ids2.authority(npc).unwrap())
            .await
            .unwrap();
        let records = bound.recent(20).await.unwrap();
        assert!(records.iter().all(|r| r.id()
            != format!(
                "village-legacy:{}:{}:0",
                ids.world,
                npc.name().to_lowercase()
            )));
    }
}

async fn supper_fixture(
    path: &std::path::Path,
    calls: Arc<Mutex<Vec<ModelGenerateRequest>>>,
    clock: Arc<ManualExecutionClock>,
) -> (Service, String) {
    let service = fixture_with_clock(path, calls, clock).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    for (id, npc, choice) in [
        ("collect", anima_village::NpcId::Tavi, "collect_order"),
        ("deliver", anima_village::NpcId::Mira, "deliver_order"),
    ] {
        service
            .interact(
                &save.save_id,
                Interaction {
                    request_id: id.into(),
                    message: String::new(),
                    choice_id: Some(choice.into()),
                    recovery_of: None,
                    npc_id: Some(npc),
                },
                false,
            )
            .await
            .unwrap();
    }
    (service, save.save_id)
}
#[tokio::test]
async fn delayed_supper_first_reply_becomes_terminal_without_refilling_on_restart() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let clock = Arc::new(ManualExecutionClock::default());
    let (service, save) = supper_fixture(temp.path(), calls.clone(), clock.clone()).await;
    let before = serde_json::to_value(service.get(&save).unwrap()).unwrap();
    let count = calls.lock().unwrap().len();
    clock.advance_ms(60_001).unwrap();
    service.open_game_presence(&save).unwrap();
    let activity = service.run_resident_activity(&save).await.unwrap().unwrap();
    let activity = serde_json::to_value(activity).unwrap();
    assert_eq!(activity["status"], "budget_exhausted");
    assert_eq!(
        activity["terminalReason"],
        "The supper conversation reached its time limit."
    );
    assert_eq!(activity["complete"], false);
    assert_eq!(activity["replies"].as_array().unwrap().len(), 0);
    for _ in 0..3 {
        service.run_resident_activity(&save).await.unwrap();
    }
    drop(service);
    let service = fixture_with_clock(temp.path(), calls.clone(), clock).await;
    service.open_game_presence(&save).unwrap();
    assert_eq!(
        serde_json::to_value(service.run_resident_activity(&save).await.unwrap()).unwrap(),
        serde_json::json!(activity)
    );
    assert_eq!(calls.lock().unwrap().len(), count);
    assert_eq!(
        serde_json::to_value(service.get(&save).unwrap()).unwrap(),
        before
    );
}
#[tokio::test]
async fn paused_supper_reopens_past_deadline_with_original_reply_and_terminal_status() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let clock = Arc::new(ManualExecutionClock::default());
    let (service, save) = supper_fixture(temp.path(), calls.clone(), clock.clone()).await;
    let presence = service.open_game_presence(&save).unwrap();
    let first = service.run_resident_activity(&save).await.unwrap().unwrap();
    service
        .update_game_presence(&save, &presence.session_id, true)
        .unwrap();
    let count = calls.lock().unwrap().len();
    drop(service);
    clock.advance_ms(60_001).unwrap();
    let service = fixture_with_clock(temp.path(), calls.clone(), clock).await;
    service.open_game_presence(&save).unwrap();
    let next =
        serde_json::to_value(service.run_resident_activity(&save).await.unwrap().unwrap()).unwrap();
    assert_eq!(next["status"], "budget_exhausted");
    assert_eq!(
        next["replies"],
        serde_json::to_value(first.replies).unwrap()
    );
    for _ in 0..3 {
        service.run_resident_activity(&save).await.unwrap();
    }
    assert_eq!(calls.lock().unwrap().len(), count);
}

#[tokio::test]
async fn copied_legacy_source_survives_import_ack_failure_reopen_and_cache_changes() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let db = service.store.db().unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM village_saves WHERE id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let mut raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    raw["state"]["conversation"] = serde_json::json!(["Original copied Tavi source"]);
    db.execute(
        "UPDATE village_saves SET snapshot=?1 WHERE id=?2",
        rusqlite::params![raw.to_string(), save.save_id],
    )
    .unwrap();
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    let frozen: String = db
        .query_row(
            "SELECT payload FROM village_legacy_memory_sources WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_import_ack BEFORE INSERT ON village_legacy_memory_receipts BEGIN SELECT RAISE(ABORT,'fixture lost import acknowledgement'); END;").unwrap();
    assert!(service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids)
        .await
        .is_err());
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    assert_eq!(
        bound.recent(10).await.unwrap().len(),
        1,
        "canonical import commits before game acknowledgement"
    );
    raw["state"]["conversation"] =
        serde_json::json!(["Later mutable cache must never become a copied import"]);
    db.execute(
        "UPDATE village_saves SET snapshot=?1 WHERE id=?2",
        rusqlite::params![raw.to_string(), save.save_id],
    )
    .unwrap();
    db.execute_batch("DROP TRIGGER fail_import_ack").unwrap();
    drop(db);
    drop(bound);
    drop(service);
    let service = fixture(temp.path(), calls).await;
    service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids)
        .await
        .unwrap();
    let db = service.store.db().unwrap();
    let reopened: String = db
        .query_row(
            "SELECT payload FROM village_legacy_memory_sources WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(frozen, reopened);
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM village_legacy_memory_receipts WHERE save_id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    let record = bound.recent(10).await.unwrap().remove(0);
    assert_eq!(record.content(), "Original copied Tavi source");
    let corrected = service
        .edit_resident_memory(
            &save.save_id,
            crate::memory_lifecycle::MemoryEdit {
                request_id: "correct-import".into(),
                npc_id: anima_village::NpcId::Tavi,
                memory_id: record.id().into(),
                expected_revision: 1,
                replacement: Some("Canonical corrected source".into()),
            },
        )
        .await
        .unwrap();
    service
        .edit_resident_memory(
            &save.save_id,
            crate::memory_lifecycle::MemoryEdit {
                request_id: "forget-correction".into(),
                npc_id: anima_village::NpcId::Tavi,
                memory_id: corrected.memory_id,
                expected_revision: 2,
                replacement: None,
            },
        )
        .await
        .unwrap();
    db.execute(
        "DELETE FROM village_legacy_memory_receipts WHERE save_id=?1",
        [&save.save_id],
    )
    .unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids)
        .await
        .unwrap();
    assert!(
        bound.recent(10).await.unwrap().is_empty(),
        "receipt replay cannot revive corrected or forgotten source rows"
    );
}
#[tokio::test]
async fn existing_durable_identity_never_reimports_a_mutable_legacy_cache() {
    let temp = tempfile::tempdir().unwrap();
    let service = fixture(temp.path(), Arc::new(Mutex::new(vec![]))).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    service.store.resident_identities(&save.save_id).unwrap();
    let db = service.store.db().unwrap();
    // Represents an already-admitted durable save created before copied-source support.
    db.execute(
        "DELETE FROM village_legacy_memory_sources WHERE save_id=?1",
        [&save.save_id],
    )
    .unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM village_saves WHERE id=?1",
            [&save.save_id],
            |r| r.get(0),
        )
        .unwrap();
    let mut raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    raw["state"]["conversation"] = serde_json::json!(["Forgotten private old cache"]);
    db.execute(
        "UPDATE village_saves SET snapshot=?1 WHERE id=?2",
        rusqlite::params![raw.to_string(), save.save_id],
    )
    .unwrap();
    let ids = service.store.resident_identities(&save.save_id).unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids)
        .await
        .unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    assert!(bound.recent(10).await.unwrap().is_empty());
}
#[tokio::test]
async fn completed_supper_engine_result_commits_after_deadline_without_a_new_model_call() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let clock = Arc::new(ManualExecutionClock::default());
    let (service, save) = supper_fixture(temp.path(), calls.clone(), clock.clone()).await;
    let db = service.store.db().unwrap();
    let raw: String = db
        .query_row(
            "SELECT payload FROM village_supper_activity WHERE save_id=?1",
            [&save],
            |r| r.get(0),
        )
        .unwrap();
    let activity: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let stage: crate::engine_bridge::StageBinding =
        serde_json::from_value(activity["stages"][0].clone()).unwrap();
    let allowance = serde_json::from_value(activity["allowance"].clone()).unwrap();
    let ids = service.store.resident_identities(&save).unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .project(&service.store, &ids)
        .await
        .unwrap();
    service
        .durable
        .as_ref()
        .unwrap()
        .generate(
            &stage,
            &allowance,
            service.store.clone(),
            &ids,
            anima_village::NpcId::Tavi,
        )
        .await
        .unwrap();
    let count = calls.lock().unwrap().len();
    let before = serde_json::to_value(service.get(&save).unwrap()).unwrap();
    drop(db);
    drop(service);
    clock.advance_ms(60_001).unwrap();
    let service = fixture_with_clock(temp.path(), calls.clone(), clock).await;
    service.open_game_presence(&save).unwrap();
    let accepted = service.run_resident_activity(&save).await.unwrap().unwrap();
    assert_eq!(
        accepted.replies.len(),
        1,
        "an existing completed engine result must reach its game receipt after expiry"
    );
    let done =
        serde_json::to_value(service.run_resident_activity(&save).await.unwrap().unwrap()).unwrap();
    assert_eq!(done["status"], "budget_exhausted");
    assert_eq!(
        done["replies"],
        serde_json::to_value(accepted.replies).unwrap()
    );
    assert_eq!(calls.lock().unwrap().len(), count);
    assert_eq!(
        serde_json::to_value(service.get(&save).unwrap()).unwrap(),
        before
    );
}

#[tokio::test]
async fn invalid_supper_result_requires_review_and_never_retries_on_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let (service, save) = supper_fixture(
        temp.path(),
        calls.clone(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await;
    let db = service.store.db().unwrap();
    let raw: String = db
        .query_row(
            "SELECT payload FROM village_supper_activity WHERE save_id=?1",
            [&save],
            |r| r.get(0),
        )
        .unwrap();
    let mut raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // The scripted provider returns Japanese, which cannot satisfy this Mandarin contract.
    raw["language"] = serde_json::json!("zh");
    db.execute(
        "UPDATE village_supper_activity SET payload=?1 WHERE save_id=?2",
        rusqlite::params![raw.to_string(), save],
    )
    .unwrap();
    service.open_game_presence(&save).unwrap();
    let before = serde_json::to_value(service.get(&save).unwrap()).unwrap();
    let terminal =
        serde_json::to_value(service.run_resident_activity(&save).await.unwrap().unwrap()).unwrap();
    assert_eq!(terminal["status"], "recovery_required");
    assert!(!terminal["terminalReason"].as_str().unwrap().is_empty());
    assert_eq!(terminal["replies"], serde_json::json!([]));
    let count = calls.lock().unwrap().len();
    for _ in 0..3 {
        service.run_resident_activity(&save).await.unwrap();
    }
    drop(db);
    drop(service);
    let service = fixture(temp.path(), calls.clone()).await;
    service.open_game_presence(&save).unwrap();
    assert_eq!(
        serde_json::to_value(service.run_resident_activity(&save).await.unwrap().unwrap()).unwrap(),
        terminal
    );
    assert_eq!(calls.lock().unwrap().len(), count);
    assert_eq!(
        serde_json::to_value(service.get(&save).unwrap()).unwrap(),
        before
    );
}

#[tokio::test]
async fn completed_supper_result_survives_expired_allowance_and_transient_projection_failure() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let clock = Arc::new(ManualExecutionClock::default());
    let (service, save) = supper_fixture(temp.path(), calls.clone(), clock.clone()).await;
    let db = service.store.db().unwrap();
    let raw: String = db
        .query_row(
            "SELECT payload FROM village_supper_activity WHERE save_id=?1",
            [&save],
            |r| r.get(0),
        )
        .unwrap();
    let activity: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let stage: crate::engine_bridge::StageBinding =
        serde_json::from_value(activity["stages"][0].clone()).unwrap();
    let allowance = serde_json::from_value(activity["allowance"].clone()).unwrap();
    let ids = service.store.resident_identities(&save).unwrap();
    let bridge = service.durable.as_ref().unwrap();
    bridge.project(&service.store, &ids).await.unwrap();
    bridge
        .generate(
            &stage,
            &allowance,
            service.store.clone(),
            &ids,
            anima_village::NpcId::Tavi,
        )
        .await
        .unwrap();
    let count = calls.lock().unwrap().len();
    clock.advance_ms(60_001).unwrap();
    service.open_game_presence(&save).unwrap();
    db.execute_batch("CREATE TRIGGER fail_supper_projection BEFORE INSERT ON village_legacy_memory_receipts BEGIN SELECT RAISE(ABORT,'transient projection acknowledgement failure'); END;").unwrap();
    assert_eq!(
        service
            .run_resident_activity(&save)
            .await
            .unwrap_err()
            .status,
        503
    );
    let pending = serde_json::to_value(service.store.activity(&save).unwrap().unwrap()).unwrap();
    assert_eq!(
        pending["status"], "pending",
        "pre-generation storage failure must preserve retry of a completed engine result"
    );
    db.execute_batch("DROP TRIGGER fail_supper_projection")
        .unwrap();
    let accepted = service.run_resident_activity(&save).await.unwrap().unwrap();
    assert_eq!(accepted.replies.len(), 1);
    assert_eq!(calls.lock().unwrap().len(), count);
}

#[tokio::test]
async fn committed_supper_reply_retries_projection_after_reopen_without_repeating_tavi() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let (service, save) = supper_fixture(
        temp.path(),
        calls.clone(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await;
    let db = service.store.db().unwrap();
    db.execute_batch("CREATE TRIGGER fail_supper_ack BEFORE UPDATE OF acknowledged ON village_memory_outbox WHEN OLD.acknowledged=0 BEGIN SELECT RAISE(ABORT,'transient supper projection acknowledgement'); END;").unwrap();
    service.open_game_presence(&save).unwrap();
    assert_eq!(
        service
            .run_resident_activity(&save)
            .await
            .unwrap_err()
            .status,
        503
    );
    let first = service.store.activity(&save).unwrap().unwrap();
    assert_eq!(first.replies.len(), 1);
    assert_eq!(serde_json::to_value(&first).unwrap()["status"], "pending");
    let count = calls.lock().unwrap().len();
    drop(db);
    drop(service);
    rusqlite::Connection::open(temp.path().join("game.sqlite"))
        .unwrap()
        .execute_batch("DROP TRIGGER fail_supper_ack")
        .unwrap();
    let service = fixture(temp.path(), calls.clone()).await;
    service.open_game_presence(&save).unwrap();
    let done = service.run_resident_activity(&save).await.unwrap().unwrap();
    assert!(done.complete);
    assert_eq!(serde_json::to_value(&done).unwrap()["status"], "completed");
    assert_eq!(
        serde_json::to_value(&done.replies[0]).unwrap(),
        serde_json::to_value(&first.replies[0]).unwrap()
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        count + 1,
        "only Mira's admitted reply may call the model after reopen"
    );
    for _ in 0..3 {
        service.run_resident_activity(&save).await.unwrap();
    }
    assert_eq!(calls.lock().unwrap().len(), count + 1);
}

#[tokio::test]
async fn completed_supper_retries_final_projection_without_reclaiming_any_reply() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let (service, save) = supper_fixture(
        temp.path(),
        calls.clone(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await;
    service.open_game_presence(&save).unwrap();
    service.run_resident_activity(&save).await.unwrap();
    let db = service.store.db().unwrap();
    db.execute_batch("CREATE TRIGGER fail_final_supper_ack BEFORE UPDATE OF acknowledged ON village_memory_outbox WHEN OLD.acknowledged=0 BEGIN SELECT RAISE(ABORT,'transient final supper projection acknowledgement'); END;").unwrap();
    assert_eq!(
        service
            .run_resident_activity(&save)
            .await
            .unwrap_err()
            .status,
        503
    );
    assert!(service.store.activity(&save).unwrap().unwrap().complete);
    let count = calls.lock().unwrap().len();
    db.execute_batch("DROP TRIGGER fail_final_supper_ack")
        .unwrap();
    let done = service.run_resident_activity(&save).await.unwrap().unwrap();
    assert!(done.complete);
    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM village_memory_outbox WHERE save_id=?1 AND acknowledged=0",
            [&save],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        pending, 0,
        "completed activity heartbeats still retry their accepted memory outbox"
    );
    assert_eq!(calls.lock().unwrap().len(), count);
}

#[tokio::test]
async fn restarted_completed_supper_drops_stale_claim_without_running_more_replies() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let (service, save) = supper_fixture(
        temp.path(),
        calls.clone(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await;
    service.open_game_presence(&save).unwrap();
    service.run_resident_activity(&save).await.unwrap();
    service.run_resident_activity(&save).await.unwrap();
    service.store.db().unwrap().execute("UPDATE village_supper_activity SET token='stopped-after-final-commit' WHERE save_id=?1",[&save]).unwrap();
    let count = calls.lock().unwrap().len();
    drop(service);
    let service = fixture(temp.path(), calls.clone()).await;
    service.open_game_presence(&save).unwrap();
    assert!(
        service
            .run_resident_activity(&save)
            .await
            .unwrap()
            .unwrap()
            .complete
    );
    let ids = service.store.resident_identities(&save).unwrap();
    let bound = service
        .durable
        .as_ref()
        .unwrap()
        .memory
        .bind(ids.authority(anima_village::NpcId::Tavi).unwrap())
        .await
        .unwrap();
    let record = bound.recent(10).await.unwrap().remove(0);
    let edit = service
        .edit_resident_memory(
            &save,
            crate::memory_lifecycle::MemoryEdit {
                request_id: "after-terminal-restart".into(),
                npc_id: anima_village::NpcId::Tavi,
                memory_id: record.id().into(),
                expected_revision: record.revision(),
                replacement: None,
            },
        )
        .await;
    assert!(
        edit.is_ok(),
        "a dead completed-activity claim cannot block memory corrections after restart"
    );
    assert_eq!(calls.lock().unwrap().len(), count);
}

// Keep the existing basic fixture unchanged; lifecycle coverage owns a second
// persisted save and chooses resident/save authority at the host boundary.
struct LifecycleVillage {
    host: SharedVillage,
    other_save: String,
}
#[async_trait]
impl anima_harness::conformance::PersistentHostFixture for LifecycleVillage {
    async fn submit(
        &mut self,
        id: &str,
        text: &str,
    ) -> std::result::Result<anima_harness::conformance::HostReply, String> {
        anima_harness::conformance::PersistentHostFixture::submit(&mut self.host, id, text).await
    }
    async fn accept_preference(
        &mut self,
        id: &str,
        text: &str,
    ) -> std::result::Result<anima_harness::conformance::AcceptedPreference, String> {
        anima_harness::conformance::PersistentHostFixture::accept_preference(
            &mut self.host,
            id,
            text,
        )
        .await
    }
    async fn recall(&mut self, query: &str) -> std::result::Result<Vec<MemoryRecord>, String> {
        anima_harness::conformance::PersistentHostFixture::recall(&mut self.host, query).await
    }
    async fn reopen(&mut self) -> std::result::Result<(), String> {
        anima_harness::conformance::PersistentHostFixture::reopen(&mut self.host).await
    }
}
impl LifecycleVillage {
    fn scope(&self, scope: anima_harness::conformance::OtherHostScope) -> (&str, &'static str) {
        match scope {
            anima_harness::conformance::OtherHostScope::Individual => (&self.host.save, "mira"),
            anima_harness::conformance::OtherHostScope::Partition => (&self.other_save, "tavi"),
        }
    }
    async fn edit(
        &self,
        target: (&str, &str),
        source: &str,
        memory_id: &str,
        revision: u64,
        replacement: Option<&str>,
    ) -> std::result::Result<(u16, serde_json::Value), String> {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (save, npc) = target;
        let request = Request::builder().method("POST").uri(format!("/api/village/saves/{save}/memories")).header("content-type", "application/json")
            .body(Body::from(serde_json::json!({"requestId":source,"npcId":npc,"memoryId":memory_id,"expectedRevision":revision,"replacement":replacement}).to_string())).unwrap();
        let response = crate::http::router(self.host.service.as_ref().unwrap().clone())
            .oneshot(request)
            .await
            .map_err(|_| "HTTP transport failed")?;
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|_| "HTTP body failed")?
            .to_bytes();
        let value = serde_json::from_slice(&bytes).map_err(|_| "memory receipt parse failed")?;
        Ok((status, value))
    }
    fn receipt(
        value: serde_json::Value,
    ) -> std::result::Result<anima_harness::conformance::PreferenceMutation, String> {
        Ok(anima_harness::conformance::PreferenceMutation {
            memory_id: value["memoryId"]
                .as_str()
                .ok_or("missing affected memory")?
                .into(),
            receipt: value.to_string(),
        })
    }
}
#[async_trait]
impl anima_harness::conformance::PersistentHostLifecycleFixture for LifecycleVillage {
    fn captured_requests(&self) -> Vec<ModelGenerateRequest> {
        self.host.calls.lock().unwrap().clone()
    }
    async fn submit_as(
        &mut self,
        scope: anima_harness::conformance::OtherHostScope,
        id: &str,
        text: &str,
    ) -> std::result::Result<anima_harness::conformance::HostReply, String> {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (save, npc) = self.scope(scope);
        let service = self.host.service.as_ref().unwrap();
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/village/saves/{save}/turns"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"requestId":id,"npcId":npc,"message":text}).to_string(),
            ))
            .unwrap();
        let response = crate::http::router(service.clone())
            .oneshot(request)
            .await
            .map_err(|_| "HTTP transport failed")?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|_| "HTTP body failed")?
            .to_bytes();
        if !status.is_success() {
            return Err(format!(
                "HTTP {status}: {}",
                String::from_utf8_lossy(&bytes)
            ));
        }
        let view: anima_village::GameView =
            serde_json::from_slice(&bytes).map_err(|_| "view parse failed")?;
        let db = service.store.db().map_err(|e| e.message)?;
        let payload: String = db.query_row("SELECT payload FROM village_engine_stages WHERE save_id=?1 AND request_id=?2 AND stage='narration'",rusqlite::params![save,id],|r|r.get(0)).map_err(|_|"stage receipt missing")?;
        let stage: serde_json::Value = serde_json::from_str(&payload).map_err(|_| "bad stage")?;
        Ok(anima_harness::conformance::HostReply {
            run_id: stage["binding"]["run"]
                .as_str()
                .ok_or("missing run")?
                .into(),
            individual_id: stage["binding"]["individual"]["individual_id"]
                .as_str()
                .ok_or("missing individual")?
                .into(),
            text: view.dialogue.text,
        })
    }
    async fn correct_preference(
        &mut self,
        source: &str,
        original: &MemoryRecord,
        replacement: &str,
    ) -> std::result::Result<anima_harness::conformance::PreferenceMutation, String> {
        let (status, value) = self
            .edit(
                (&self.host.save, "tavi"),
                source,
                original.id(),
                original.revision(),
                Some(replacement),
            )
            .await?;
        if status != 200 {
            return Err(format!("HTTP {status}: {value}"));
        }
        Self::receipt(value)
    }
    async fn forget_preference(
        &mut self,
        source: &str,
        memory_id: &str,
        revision: u64,
    ) -> std::result::Result<anima_harness::conformance::PreferenceMutation, String> {
        let (status, value) = self
            .edit((&self.host.save, "tavi"), source, memory_id, revision, None)
            .await?;
        if status != 200 {
            return Err(format!("HTTP {status}: {value}"));
        }
        Self::receipt(value)
    }
    async fn forget_as_other(
        &mut self,
        scope: anima_harness::conformance::OtherHostScope,
        memory_id: &str,
        revision: u64,
    ) -> std::result::Result<Option<anima_harness::conformance::PreferenceMutation>, String> {
        let (save, npc) = self.scope(scope);
        let (status, value) = self
            .edit(
                (save, npc),
                "shared-lifecycle-foreign-forget",
                memory_id,
                revision,
                None,
            )
            .await?;
        match status {
            403 | 404 => Ok(None),
            200 => Self::receipt(value).map(Some),
            _ => Err(format!(
                "foreign forgetting failed for an unrelated reason: HTTP {status}: {value}"
            )),
        }
    }
}
#[tokio::test]
async fn shared_persistent_host_lifecycle_contract_runs_through_game_http() {
    let temp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(temp.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let other_save = service.create(anima_village::Language::Ja).unwrap();
    let mut fixture = LifecycleVillage {
        host: SharedVillage {
            path: temp.path().into(),
            service: Some(service),
            save: save.save_id,
            calls,
        },
        other_save: other_save.save_id,
    };
    anima_harness::conformance::assert_persistent_host_lifecycle(&mut fixture)
        .await
        .unwrap();
}

#[tokio::test]
async fn startup_draining_keeps_exact_request_and_canonical_memory_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let service = fixture(dir.path(), calls.clone()).await;
    let save = service.create(anima_village::Language::Ja).unwrap();
    let accepted = service
        .interact(
            &save.save_id,
            input("accepted", anima_village::NpcId::Tavi, "I like honey bread"),
            false,
        )
        .await
        .unwrap();
    let bridge = service.durable.clone().unwrap();
    bridge.execution.shutdown().await.unwrap();
    bridge.memory.shutdown().await.unwrap();
    drop(bridge);
    drop(service);
    let service = fixture(dir.path(), calls.clone())
        .await
        .with_admission(false)
        .unwrap();
    let replay = service
        .interact(
            &save.save_id,
            input("accepted", anima_village::NpcId::Tavi, "I like honey bread"),
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(accepted).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert_eq!(
        service
            .interact(
                &save.save_id,
                input("accepted", anima_village::NpcId::Tavi, "changed"),
                false
            )
            .await
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        service
            .interact(
                &save.save_id,
                input("new", anima_village::NpcId::Tavi, "new"),
                false
            )
            .await
            .unwrap_err()
            .status,
        503
    );
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(service.durable.is_some());
}
