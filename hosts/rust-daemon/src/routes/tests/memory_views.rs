//! Real owner-router coverage using Anima's canonical memory and persistence seams.
use super::*;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::memory_embeddings::MemoryEmbeddingRuntime;
use crate::memory_store::{load_memory_snapshot, MemoryStoreConfig};
use crate::memory_views::source_hash;
use anima_memory::{Memory, MemoryScope, MemorySearchOptions, MemoryType};
use serde_json::{json, Value};

const OWNER: &str = "http://localhost:4200";

fn request(method: &str, uri: &str, origin: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080");
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    builder
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .unwrap()
}
async fn answer(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    status: StatusCode,
) -> Value {
    let response = app
        .clone()
        .oneshot(request(method, uri, Some(OWNER), body))
        .await
        .unwrap();
    assert_eq!(response.status(), status, "{method} {uri}");
    if uri.contains("/memory-views") {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
struct Fixture {
    app: axum::Router,
    state: Arc<RwLock<DaemonState>>,
    agent: String,
    temp: WorkspaceAvatarTemp,
}
impl Fixture {
    fn new() -> Self {
        let temp = WorkspaceAvatarTemp::new("memory-views");
        let mut state = DaemonState::new();
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            temp.root.join("control-plane.json"),
        )));
        state.set_memory_store(Some(MemoryStoreConfig::Json(temp.root.join("memory.json"))));
        state.memory_embeddings = Arc::new(RwLock::new(MemoryEmbeddingRuntime::disabled()));
        let agent = state
            .create_agent(test_config("Profile companion"))
            .unwrap()
            .state
            .id;
        let state = Arc::new(RwLock::new(state));
        Self {
            app: router(state.clone(), DaemonConfig::default()),
            state,
            agent,
            temp,
        }
    }
    fn uri(&self) -> String {
        format!("/api/agents/{}/memory-views", self.agent)
    }
    fn source_uri(&self, id: &str) -> String {
        format!("{}/sources/{id}", self.uri())
    }
    async fn seed(&self, content: &str) -> Memory {
        let value = answer(&self.app, "POST", "/api/memories", Some(json!({
            "agentId":self.agent,"agentName":"Profile companion","type":"fact","content":content,
            "scope":"private","importance":0.7,"tags":["preserve"]
        })), StatusCode::CREATED).await;
        self.stored(value["id"].as_str().unwrap()).await.unwrap()
    }
    async fn stored(&self, id: &str) -> Option<Memory> {
        let memory = self.state.read().await.memory_handle();
        let value = memory.read().await.get(id);
        value
    }
    async fn view(&self) -> Value {
        answer(&self.app, "GET", &self.uri(), None, StatusCode::OK).await
    }
    async fn select(&self, version: u64, profile: &[&Memory], notes: &[&Memory]) -> Value {
        answer(
            &self.app,
            "PATCH",
            &self.uri(),
            Some(selection(version, profile, notes)),
            StatusCode::OK,
        )
        .await
    }
}
fn selection(version: u64, profile: &[&Memory], notes: &[&Memory]) -> Value {
    let picks = |records: &[&Memory]| {
        records
            .iter()
            .map(|record| json!({"id":record.id,"sourceHash":source_hash(record)}))
            .collect::<Vec<_>>()
    };
    json!({"expectedVersion":version,"userProfile":picks(profile),"agentNotes":picks(notes)})
}
fn correction(version: u64, source: &Memory, content: &str) -> Value {
    json!({"expectedVersion":version,"expectedContent":source.content,"expectedSourceHash":source_hash(source),"content":content})
}

#[tokio::test]
async fn m40_owner_routes_require_strict_write_authority_and_strict_payloads() {
    let f = Fixture::new();
    let fact = f.seed("owner preference").await;
    let before = f.stored(&fact.id).await;
    for (method, uri, body) in [
        ("GET", f.uri(), None),
        ("GET", format!("{}/candidates", f.uri()), None),
        ("PATCH", f.uri(), Some(json!({"malformed":true}))),
        (
            "PATCH",
            f.source_uri(&fact.id),
            Some(json!({"malformed":true})),
        ),
    ] {
        let response = f
            .app
            .clone()
            .oneshot(request(method, &uri, Some("http://evil.example"), body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    for uri in [f.uri(), f.source_uri(&fact.id)] {
        let mut req = request("PATCH", &uri, None, Some(json!({"malformed":true})));
        req.headers_mut().insert("referer", OWNER.parse().unwrap());
        req.headers_mut()
            .insert("sec-fetch-site", "same-origin".parse().unwrap());
        let response = f.app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Referer alone cannot authorize a write"
        );
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    for body in [
        json!({"expectedVersion":0,"userProfile":[],"agentNotes":[],"unknown":true}),
        json!({"expectedVersion":null,"userProfile":[],"agentNotes":[]}),
        json!({"expectedVersion":0,"userProfile":[{"id":fact.id,"sourceHash":source_hash(&fact),"content":"invented"}],"agentNotes":[]}),
        selection(0, &[&fact], &[&fact]),
    ] {
        answer(
            &f.app,
            "PATCH",
            &f.uri(),
            Some(body),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    answer(
        &f.app,
        "GET",
        "/api/agents/foreign-agent/memory-views",
        None,
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(f.view().await["version"], 0);
    assert_eq!(f.stored(&fact.id).await, before);
}

#[tokio::test]
async fn m40_selected_sources_are_own_clean_private_facts_and_refusals_do_not_leak_content() {
    let f = Fixture::new();
    let good = f.seed("valid selected preference").await;
    let mut unsafe_records = Vec::new();
    for index in 0..8 {
        let record = f.seed(&format!("PRIVATE_SECRET_{index}")).await;
        let memory = f.state.read().await.memory_handle();
        let mut manager = memory.write().await;
        let mut snapshot = manager.snapshot();
        let current = snapshot
            .memories
            .iter_mut()
            .find(|m| m.id == record.id)
            .unwrap();
        match index {
            0 => current.agent_id = "foreign-agent".into(),
            1 => current.scope = MemoryScope::Shared,
            2 => {
                current.scope = MemoryScope::Room;
                current.room_id = Some("room-current".into());
            }
            3 => current.world_id = Some("world".into()),
            4 => current.session_id = Some("session".into()),
            5 => current.memory_type = MemoryType::Reflection,
            6 => current.content.push('\u{202E}'),
            _ => current.tags = Some(vec!["hidden\u{E0041}".into()]),
        }
        unsafe_records.push(current.clone());
        manager.replace_snapshot(snapshot);
    }
    for record in &unsafe_records {
        let refusal = answer(
            &f.app,
            "PATCH",
            &f.uri(),
            Some(selection(0, &[record], &[])),
            StatusCode::NOT_FOUND,
        )
        .await;
        assert!(!refusal.to_string().contains("PRIVATE_SECRET"));
    }
    let candidates = answer(
        &f.app,
        "GET",
        &format!("{}/candidates", f.uri()),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(candidates["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(candidates["candidates"][0]["id"], good.id);
    assert!(!candidates.to_string().contains("PRIVATE_SECRET"));
    assert_eq!(f.view().await["version"], 0);
}

#[tokio::test]
async fn m40_selection_persists_ordered_references_and_reads_current_hashes_without_canonical_writes(
) {
    let f = Fixture::new();
    let a = f.seed("Sensitive profile first").await;
    let b = f.seed("Sensitive profile second").await;
    let note = f.seed("Sensitive private agent note").await;
    let memory = f.state.read().await.memory_handle();
    let before = memory.read().await.snapshot();
    let bytes = std::fs::read(f.temp.root.join("memory.json")).unwrap();
    let selected = f.select(0, &[&b, &a], &[&note]).await;
    assert_eq!(selected["version"], 1);
    assert_eq!(selected["userProfile"][0]["id"], b.id);
    assert_eq!(selected["userProfile"][1]["id"], a.id);
    assert_eq!(selected["agentNotes"][0]["id"], note.id);
    assert_eq!(selected["userProfile"][0]["sourceHash"], source_hash(&b));
    assert_eq!(memory.read().await.snapshot(), before);
    assert_eq!(
        std::fs::read(f.temp.root.join("memory.json")).unwrap(),
        bytes
    );
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(
        f.temp.root.join("control-plane.json"),
    ))
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        saved.memory_views[0].user_profile_ids,
        vec![b.id.clone(), a.id.clone()]
    );
    assert!(
        !std::fs::read_to_string(f.temp.root.join("control-plane.json"))
            .unwrap()
            .contains("Sensitive")
    );
    assert_eq!(f.select(1, &[&b, &a], &[&note]).await, selected);
    answer(
        &f.app,
        "PATCH",
        &f.uri(),
        Some(selection(0, &[&b, &a], &[&note])),
        StatusCode::CONFLICT,
    )
    .await;
    answer(
        &f.app,
        "PATCH",
        &format!("/api/memories/{}", b.id),
        Some(json!({"tags":["metadata correction"]})),
        StatusCode::OK,
    )
    .await;
    let current = f.stored(&b.id).await.unwrap();
    let view = f.view().await;
    assert_eq!(view["version"], 1);
    assert_ne!(view["viewDigest"], selected["viewDigest"]);
    assert_eq!(view["userProfile"][0]["sourceHash"], source_hash(&current));
    answer(
        &f.app,
        "PATCH",
        &f.uri(),
        Some(selection(1, &[&b, &a], &[&note])),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(f.select(1, &[&current, &a], &[&note]).await["version"], 1);
    answer(
        &f.app,
        "DELETE",
        &format!("/api/memories/{}", a.id),
        None,
        StatusCode::OK,
    )
    .await;
    let missing = f.view().await;
    assert_eq!(missing["version"], 1);
    assert_eq!(
        missing["unavailable"],
        json!([{"id":a.id,"reason":"unavailable"}])
    );
    assert!(!missing.to_string().contains(&a.content));
}

#[tokio::test]
async fn m40_correction_requires_selected_current_full_source_and_preserves_neighbors_metadata_and_index(
) {
    let f = Fixture::new();
    let source = f
        .seed(&format!("oldseraphic {} FULL_SOURCE_END", "茶".repeat(801)))
        .await;
    let neighbor = f.seed("neighbor kestrel remains untouched").await;
    f.select(0, &[&source], &[]).await;
    let uri = f.source_uri(&source.id);
    let new_text = "newjade migration checklist";
    let mut wrong_hash = correction(1, &source, new_text);
    wrong_hash["expectedSourceHash"] = json!("0".repeat(64));
    let mut excerpt = correction(1, &source, new_text);
    excerpt["expectedContent"] = json!(source.content.chars().take(800).collect::<String>());
    let mut unknown = correction(1, &source, new_text);
    unknown["importance"] = json!(1.0);
    for (body, status) in [
        (correction(0, &source, new_text), StatusCode::CONFLICT),
        (wrong_hash, StatusCode::CONFLICT),
        (excerpt, StatusCode::CONFLICT),
        (unknown, StatusCode::BAD_REQUEST),
        (
            json!({"expectedVersion":1,"expectedSourceHash":source_hash(&source),"content":new_text}),
            StatusCode::BAD_REQUEST,
        ),
        (
            correction(1, &source, &"a".repeat(8001)),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        answer(&f.app, "PATCH", &uri, Some(body), status).await;
        assert_eq!(f.stored(&source.id).await, Some(source.clone()));
    }
    answer(
        &f.app,
        "PATCH",
        &f.source_uri(&neighbor.id),
        Some(correction(1, &neighbor, new_text)),
        StatusCode::NOT_FOUND,
    )
    .await;
    // Full canonical metadata participates in CAS even when the source text is unchanged.
    answer(
        &f.app,
        "PATCH",
        &format!("/api/memories/{}", source.id),
        Some(json!({"importance":0.8})),
        StatusCode::OK,
    )
    .await;
    answer(
        &f.app,
        "PATCH",
        &uri,
        Some(correction(1, &source, new_text)),
        StatusCode::CONFLICT,
    )
    .await;
    let current = f.stored(&source.id).await.unwrap();
    let result = answer(
        &f.app,
        "PATCH",
        &uri,
        Some(correction(1, &current, new_text)),
        StatusCode::OK,
    )
    .await;
    let mut expected = current.clone();
    expected.content = new_text.into();
    assert_eq!(f.stored(&source.id).await, Some(expected.clone()));
    assert_eq!(f.stored(&neighbor.id).await, Some(neighbor));
    assert_eq!(result["version"], 1);
    assert_eq!(result["userProfile"][0]["content"], new_text);
    assert_eq!(
        result["userProfile"][0]["sourceHash"],
        source_hash(&expected)
    );
    let memory = f.state.read().await.memory_handle();
    let manager = memory.read().await;
    assert!(!manager
        .search("oldseraphic", MemorySearchOptions::default())
        .iter()
        .any(|m| m.id == source.id));
    assert!(manager
        .search("newjade", MemorySearchOptions::default())
        .iter()
        .any(|m| m.id == source.id));
    drop(manager);
    let saved = load_memory_snapshot(&MemoryStoreConfig::Json(f.temp.root.join("memory.json")))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.memories.iter().find(|m| m.id == source.id),
        Some(&expected)
    );
}

#[tokio::test]
async fn m40_candidates_use_bounded_visible_substring_search_and_durable_source_availability() {
    let f = Fixture::new();
    for index in 0..51 {
        f.seed(&format!("citrus candidate {index:02}")).await;
    }
    let all = answer(
        &f.app,
        "GET",
        &format!("{}/candidates?query=CITRUS", f.uri()),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(all["candidates"].as_array().unwrap().len(), 50);
    assert_eq!(all["hasMore"], true);
    let one = answer(
        &f.app,
        "GET",
        &format!("{}/candidates?query=candidate%2003", f.uri()),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(one["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(one["hasMore"], false);
    assert_eq!(one["candidates"][0]["content"], "citrus candidate 03");
    for query in [
        "limit=999".into(),
        format!("query={}", "x".repeat(513)),
        "query=%E2%80%AE".into(),
    ] {
        answer(
            &f.app,
            "GET",
            &format!("{}/candidates?{query}", f.uri()),
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    f.state.write().await.set_memory_store(None);
    let absent = answer(
        &f.app,
        "GET",
        &format!("{}/candidates", f.uri()),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(absent, json!({"candidates":[],"hasMore":false}));
    answer(
        &f.app,
        "PATCH",
        &f.uri(),
        Some(selection(0, &[], &[])),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
}

async fn provider_text(f: &Fixture, providers: &[Arc<dyn anima_core::Provider>]) -> String {
    let guard = f.state.read().await;
    let runtime = guard.agents.get(&f.agent).unwrap();
    providers[0]
        .get(
            runtime,
            &anima_core::Message {
                id: "profile-context".into(),
                agent_id: f.agent.clone(),
                room_id: "room-current".into(),
                content: Content {
                    text: "Hello".into(),
                    attachments: None,
                    metadata: None,
                },
                role: anima_core::MessageRole::User,
                created_at_ms: 1,
            },
        )
        .await
        .unwrap()
        .text
}

#[tokio::test]
async fn m40_selection_publication_waits_for_save_and_settles_after_the_http_caller_is_dropped() {
    for fail in [true, false] {
        let f = Fixture::new();
        let old = f.seed("Old published preference").await;
        let next = f.seed("Next published preference").await;
        f.select(0, &[&old], &[]).await;
        let (published, providers) = {
            let guard = f.state.read().await;
            let published = guard.memory_view_selections.clone();
            let providers = crate::components::default_providers_with_memory_views(
                guard.memory_handle(),
                guard.memory_embeddings_handle(),
                published.clone(),
                Default::default(),
            );
            (published, providers)
        };
        let gate = f
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(fail);
        let app = f.app.clone();
        let req = request(
            "PATCH",
            &f.uri(),
            Some(OWNER),
            Some(selection(1, &[&next], &[])),
        );
        let caller = tokio::spawn(async move { app.oneshot(req).await.unwrap() });
        tokio::time::timeout(Duration::from_secs(2), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        assert_eq!(
            published
                .read()
                .unwrap()
                .get(&f.agent)
                .unwrap()
                .user_profile_ids,
            vec![old.id.clone()]
        );
        let paused = provider_text(&f, &providers).await;
        assert!(paused.contains(&old.content));
        assert!(!paused.contains(&next.content));
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        gate.release.add_permits(1);
        let expected_version = if fail { 1 } else { 2 };
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let guard = f.state.read().await;
                let ready = guard.memory_views.selection(&f.agent).version == expected_version
                    && published.read().unwrap().get(&f.agent).unwrap().version == expected_version;
                drop(guard);
                if ready {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the owned save must settle after the request is dropped");
        let final_text = provider_text(&f, &providers).await;
        assert_eq!(final_text.contains(&next.content), !fail);
        assert_eq!(final_text.contains(&old.content), fail);
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(
            f.temp.root.join("control-plane.json"),
        ))
        .await
        .unwrap()
        .unwrap();
        assert_eq!(saved.memory_views[0].version, expected_version);
        assert!(Arc::ptr_eq(
            &published,
            &f.state.read().await.memory_view_selections
        ));
    }
}

#[tokio::test]
async fn m40_restore_defaults_and_missing_sources_keep_the_existing_published_handle() {
    let f = Fixture::new();
    let fact = f.seed("private restored preference").await;
    f.select(0, &[&fact], &[]).await;
    let alias = f.state.read().await.memory_view_selections.clone();
    let mut snapshot = serde_json::to_value(f.state.read().await.control_plane_snapshot()).unwrap();
    snapshot["memoryViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"agentId":"removed-agent","version":1,
        "userProfileIds":[fact.id],"agentNoteIds":[]}));
    let memory = f.state.read().await.memory_handle();
    memory.write().await.delete_memory(&fact.id).unwrap();
    f.state
        .write()
        .await
        .restore_control_plane_snapshot(serde_json::from_value(snapshot.clone()).unwrap())
        .unwrap();
    assert!(Arc::ptr_eq(
        &alias,
        &f.state.read().await.memory_view_selections
    ));
    assert!(!alias.read().unwrap().contains_key("removed-agent"));
    let unavailable = f.view().await;
    assert_eq!(unavailable["userProfile"], json!([]));
    assert_eq!(
        unavailable["unavailable"],
        json!([{"id":fact.id,"reason":"unavailable"}])
    );
    assert!(!unavailable.to_string().contains(&fact.content));
    f.state.write().await.set_memory_store(None);
    assert!(alias.read().unwrap().is_empty());
    let nondurable = f.view().await;
    assert_eq!(nondurable["version"], 1);
    assert_eq!(
        nondurable["unavailable"],
        json!([{"id":fact.id,"reason":"unavailable"}])
    );
    snapshot["version"] = json!(15);
    snapshot.as_object_mut().unwrap().remove("memoryViews");
    let mut legacy = DaemonState::new();
    legacy
        .restore_control_plane_snapshot(serde_json::from_value(snapshot).unwrap())
        .unwrap();
    let legacy = Arc::new(RwLock::new(legacy));
    let app = router(legacy, DaemonConfig::default());
    let empty = answer(&app, "GET", &f.uri(), None, StatusCode::OK).await;
    assert_eq!(empty["version"], 0);
    assert_eq!(empty["userProfile"], json!([]));
    assert_eq!(empty["agentNotes"], json!([]));
}

#[tokio::test]
async fn m40_failed_correction_rolls_back_canonical_text_metadata_and_search_index() {
    let f = Fixture::new();
    let source = f.seed("oldrollback source").await;
    f.select(0, &[&source], &[]).await;
    let before = f.view().await;
    let bytes = std::fs::read(f.temp.root.join("memory.json")).unwrap();
    let blocked = f.temp.root.join("blocked-memory-file");
    std::fs::create_dir_all(&blocked).unwrap();
    f.state
        .write()
        .await
        .set_memory_store(Some(MemoryStoreConfig::Json(blocked)));
    let error = answer(
        &f.app,
        "PATCH",
        &f.source_uri(&source.id),
        Some(correction(1, &source, "newrollback source")),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert!(!error.to_string().contains("unchanged"));
    assert!(!error.to_string().contains("undone"));
    assert_eq!(f.stored(&source.id).await, Some(source.clone()));
    assert_eq!(f.view().await, before);
    assert_eq!(
        std::fs::read(f.temp.root.join("memory.json")).unwrap(),
        bytes
    );
    let memory = f.state.read().await.memory_handle();
    let manager = memory.read().await;
    assert!(manager
        .search("oldrollback", MemorySearchOptions::default())
        .iter()
        .any(|m| m.id == source.id));
    assert!(!manager
        .search("newrollback", MemorySearchOptions::default())
        .iter()
        .any(|m| m.id == source.id));
}

#[tokio::test]
async fn m40_poisoned_publication_refuses_owner_reads_and_selection_and_omits_context() {
    let f = Fixture::new();
    let source = f.seed("Selected poisoned preference").await;
    f.select(0, &[&source], &[]).await;
    let (published, providers) = {
        let guard = f.state.read().await;
        let published = guard.memory_view_selections.clone();
        let providers = crate::components::default_providers_with_memory_views(
            guard.memory_handle(),
            guard.memory_embeddings_handle(),
            published.clone(),
            Default::default(),
        );
        (published, providers)
    };
    let bytes = std::fs::read(f.temp.root.join("control-plane.json")).unwrap();
    assert!(provider_text(&f, &providers)
        .await
        .contains(&source.content));
    let poison = published.clone();
    assert!(std::thread::spawn(move || {
        let _guard = poison.write().unwrap();
        panic!("poison the published selection leaf");
    })
    .join()
    .is_err());
    for (method, body) in [
        ("GET", None),
        ("PATCH", Some(selection(1, &[&source], &[]))),
    ] {
        let refusal = answer(
            &f.app,
            method,
            &f.uri(),
            body,
            StatusCode::SERVICE_UNAVAILABLE,
        )
        .await;
        assert!(!refusal.to_string().contains(&source.content));
    }
    assert_eq!(provider_text(&f, &providers).await, "no relevant memories");
    assert_eq!(f.stored(&source.id).await, Some(source));
    assert_eq!(
        std::fs::read(f.temp.root.join("control-plane.json")).unwrap(),
        bytes
    );
}

#[tokio::test]
async fn m40_deleted_selection_rows_do_not_consume_live_capacity_and_failed_save_restores_them() {
    let f = Fixture::new();
    let source = f.seed("Live selected preference").await;
    let old = crate::memory_views::MemoryViewRegistry {
        rows: (0..1000)
            .map(|index| {
                let agent = format!("removed-agent-{index}");
                (
                    agent.clone(),
                    crate::memory_views::MemoryViewSelection {
                        agent_id: agent,
                        version: 1,
                        user_profile_ids: vec![format!("removed-source-{index}")],
                        agent_note_ids: Vec::new(),
                    },
                )
            })
            .collect(),
    };
    crate::memory_views::MemoryViewRegistry::validate(&old.snapshot()).unwrap();
    let (published, providers) = {
        let mut guard = f.state.write().await;
        guard.memory_views = old.clone();
        let published = guard.memory_view_selections.clone();
        *published.write().unwrap() = old.rows.clone();
        let providers = crate::components::default_providers_with_memory_views(
            guard.memory_handle(),
            guard.memory_embeddings_handle(),
            published.clone(),
            Default::default(),
        );
        (published, providers)
    };
    // Router startup's mail reconciliation also saves the control plane and can
    // consume a shared one-shot gate. Fail this selection's captured destination
    // directly so the rollback assertion does not depend on writer scheduling.
    let blocked = f.temp.root.join("blocked-control-plane-file");
    std::fs::create_dir_all(&blocked).unwrap();
    let memory_bytes = std::fs::read(f.temp.root.join("memory.json")).unwrap();
    f.state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(blocked)));
    answer(
        &f.app,
        "PATCH",
        &f.uri(),
        Some(selection(0, &[&source], &[])),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(f.state.read().await.memory_views.rows, old.rows);
    assert_eq!(*published.read().unwrap(), old.rows);
    assert_eq!(f.view().await["version"], 0);
    assert_eq!(provider_text(&f, &providers).await, "no relevant memories");
    assert_eq!(f.stored(&source.id).await, Some(source.clone()));
    assert_eq!(
        std::fs::read(f.temp.root.join("memory.json")).unwrap(),
        memory_bytes
    );
    f.state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            f.temp.root.join("control-plane.json"),
        )));
    let selected = f.select(0, &[&source], &[]).await;
    assert_eq!(selected["version"], 1);
    assert_eq!(f.state.read().await.memory_views.rows.len(), 1);
    assert_eq!(published.read().unwrap().len(), 1);
    assert_eq!(
        published
            .read()
            .unwrap()
            .get(&f.agent)
            .unwrap()
            .user_profile_ids,
        vec![source.id.clone()]
    );
    assert!(provider_text(&f, &providers)
        .await
        .contains(&source.content));
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(
        f.temp.root.join("control-plane.json"),
    ))
    .await
    .unwrap()
    .unwrap();
    assert_eq!(saved.memory_views.len(), 1);
    assert_eq!(saved.memory_views[0].agent_id, f.agent);
    assert_eq!(
        saved.memory_views[0].user_profile_ids,
        vec![source.id.clone()]
    );
    assert_eq!(f.stored(&source.id).await, Some(source));
    assert!(Arc::ptr_eq(
        &published,
        &f.state.read().await.memory_view_selections
    ));
}
