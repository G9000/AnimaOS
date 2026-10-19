use super::*;

use anima_memory::{NewTemporalFact, RelationshipEndpointKind};
use serde_json::{json, Value};

const OWNER_ORIGIN: &str = "http://localhost:4200";
const FOREIGN_ORIGIN: &str = "http://evil.example";

struct Fixture {
    state: Arc<RwLock<DaemonState>>,
    app: axum::Router,
    first: String,
    second: String,
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    text: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap()
    }

    fn header(&self, name: &str) -> &str {
        self.headers.get(name).unwrap().to_str().unwrap()
    }
}

impl Fixture {
    fn new() -> Self {
        let mut state = DaemonState::new();
        let first = state.create_agent(test_config("First")).unwrap().state.id;
        let second = state.create_agent(test_config("Second")).unwrap().state.id;
        let state = Arc::new(RwLock::new(state));
        let app = router(Arc::clone(&state), DaemonConfig::default());
        Self {
            state,
            app,
            first,
            second,
        }
    }

    async fn call(&self, origin: &str, method: &str, uri: &str, body: Option<Value>) -> Reply {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:8080")
            .header("origin", origin);
        let request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Reply {
            status,
            headers,
            text: String::from_utf8(bytes.to_vec()).unwrap(),
        }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.call(OWNER_ORIGIN, "GET", uri, None).await
    }

    async fn core(&self, agent: &str, block: &str, text: &str) {
        let reply = self
            .call(
                OWNER_ORIGIN,
                "POST",
                &format!("/api/agents/{agent}/core-memory/{block}"),
                Some(json!({ "text": text })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    }

    async fn memory(&self, agent: &str, kind: &str, content: &str) -> String {
        let reply = self
            .call(
                OWNER_ORIGIN,
                "POST",
                "/api/memories",
                Some(json!({
                    "agentId": agent,
                    "agentName": "Agent",
                    "type": kind,
                    "content": content,
                    "importance": 0.5,
                })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
        reply.json()["id"].as_str().unwrap().to_string()
    }

    async fn fact_with(&self, evidence: &str, subject: (&str, &str), value: &str) {
        let memory = self.state.read().await.memory_handle();
        memory
            .write()
            .await
            .add_temporal_fact(NewTemporalFact {
                subject_kind: RelationshipEndpointKind::User,
                subject_id: subject.0.into(),
                subject_name: subject.1.into(),
                predicate: "likes".into(),
                object_kind: None,
                object_id: None,
                object_name: None,
                value: Some(value.into()),
                valid_from: None,
                valid_to: None,
                observed_at: Some(1_000),
                confidence: 0.6,
                evidence_memory_ids: vec![evidence.into()],
                supersedes_fact_ids: Vec::new(),
                status: None,
                tags: None,
                room_id: None,
                world_id: None,
                session_id: None,
            })
            .unwrap();
    }

    async fn fact(&self, evidence: &str, value: &str) {
        self.fact_with(evidence, ("user-1", "Leo"), value).await;
    }

    /// Core entries, one memory of each type, a relationship and a fact for
    /// the first agent; a single core entry and memory for the second.
    async fn seed(&self) {
        self.core(&self.first, "user", "prefers metric units").await;
        self.core(&self.first, "user", "lives in Lisbon").await;
        self.core(&self.first, "notes", "the build needs bun").await;
        self.memory(&self.first, "fact", "the sky is blue").await;
        self.memory(&self.first, "observation", "it rained today")
            .await;
        self.memory(&self.first, "task_result", "deployed the site")
            .await;
        let reflection = self
            .memory(&self.first, "reflection", "I rush replies")
            .await;
        let reply = self
            .call(
                OWNER_ORIGIN,
                "POST",
                "/api/memories/relationships",
                Some(json!({
                    "sourceAgentId": self.first,
                    "sourceAgentName": "First",
                    "targetKind": "user",
                    "targetAgentId": "user-1",
                    "targetAgentName": "Leo",
                    "relationshipType": "knows",
                })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
        self.fact(&reflection, "tea").await;
        self.core(&self.second, "notes", "second agent note").await;
        self.memory(&self.second, "fact", "second agent fact").await;
    }
}

fn agent<'a>(export: &'a Value, id: &str) -> &'a Value {
    export["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["id"] == id)
        .unwrap()
}

fn count(value: &Value) -> usize {
    value.as_array().unwrap().len()
}

fn keys(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                found.push(key.clone());
                keys(inner, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keys(item, found)),
        _ => {}
    }
}

#[tokio::test]
async fn the_json_export_round_trips_every_record_and_leaves_vectors_out() {
    let f = Fixture::new();
    f.seed().await;
    let reply = f
        .get(&format!("/api/memories/export?agentId={}", f.first))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.header("cache-control"), "no-store");
    assert!(reply.header("content-type").starts_with("application/json"));
    let disposition = reply.header("content-disposition");
    let prefix = format!("attachment; filename=\"animaos-memory-{}-", f.first);
    assert!(disposition.starts_with(&prefix), "{disposition}");
    let date = disposition
        .strip_prefix(&prefix)
        .unwrap()
        .strip_suffix(".json\"")
        .unwrap();
    assert_eq!(date.len(), 10);
    assert!(chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok());

    let export = reply.json();
    assert_eq!(export["version"], 1);
    assert!(export["exportedAt"].as_str().unwrap().ends_with('Z'));
    assert_eq!(count(&export["agents"]), 1);
    let first = &export["agents"][0];
    assert_eq!(first["id"], f.first.as_str());
    assert_eq!(first["name"], "First");
    assert_eq!(count(&first["core"]["user"]), 2);
    assert_eq!(first["core"]["user"][0]["content"], "prefers metric units");
    assert_eq!(first["core"]["user"][1]["content"], "lives in Lisbon");
    assert_eq!(count(&first["core"]["notes"]), 1);
    // Core entries are listed under `core` only; every other type is present.
    let mut types: Vec<&str> = first["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|memory| memory["type"].as_str().unwrap())
        .collect();
    types.sort();
    assert_eq!(types, ["fact", "observation", "reflection", "task_result"]);
    assert!(first["memories"]
        .as_array()
        .unwrap()
        .iter()
        .all(|memory| !memory["tags"].as_array().is_some_and(|tags| tags
            .iter()
            .any(|tag| tag.as_str().unwrap().starts_with("core:")))));
    assert_eq!(count(&first["relationships"]), 1);
    assert_eq!(count(&first["temporalFacts"]), 1);
    assert_eq!(first["temporalFacts"][0]["value"], "tea");
    let entity_names: Vec<&str> = first["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| entity["name"].as_str().unwrap())
        .collect();
    assert!(entity_names.contains(&"Leo"), "{entity_names:?}");

    let mut found = Vec::new();
    keys(&export, &mut found);
    assert!(
        found
            .iter()
            .all(|key| !key.to_lowercase().contains("embedding")
                && !key.to_lowercase().contains("vector")),
        "{found:?}"
    );
}

#[tokio::test]
async fn exporting_every_agent_covers_them_all_and_scopes_each_agents_records() {
    let f = Fixture::new();
    f.seed().await;
    let reply = f.get("/api/memories/export").await;
    assert_eq!(reply.status, StatusCode::OK);
    let disposition = reply.header("content-disposition");
    assert!(
        disposition.starts_with("attachment; filename=\"animaos-memory-all-"),
        "{disposition}"
    );
    let export = reply.json();
    let first = agent(&export, &f.first);
    let second = agent(&export, &f.second);
    assert_eq!(count(&first["memories"]), 4);
    assert_eq!(count(&second["memories"]), 1);
    assert_eq!(second["memories"][0]["content"], "second agent fact");
    assert_eq!(count(&second["core"]["notes"]), 1);
    assert_eq!(count(&second["core"]["user"]), 0);
    assert_eq!(count(&second["temporalFacts"]), 0);
    assert_eq!(count(&second["relationships"]), 0);
}

#[tokio::test]
async fn the_markdown_export_lists_the_core_blocks_first_then_facts_by_type() {
    let f = Fixture::new();
    f.seed().await;
    let reply = f
        .get(&format!(
            "/api/memories/export?format=md&agentId={}",
            f.first
        ))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert!(reply
        .header("content-type")
        .starts_with("text/markdown; charset=utf-8"));
    assert_eq!(reply.header("cache-control"), "no-store");
    let disposition = reply.header("content-disposition");
    assert!(disposition.ends_with(".md\""), "{disposition}");
    let text = &reply.text;
    let position = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in {text}"))
    };
    let about = position("### About you");
    let notes = position("### Agent notes");
    let facts = position("### Facts");
    assert!(about < notes && notes < facts, "{text}");
    assert!(position("- prefers metric units") > about);
    assert!(position("- prefers metric units") < notes);
    assert!(position("- the build needs bun") > notes);
    assert!(position("- the build needs bun") < facts);
    assert!(facts < position("### Observations"));
    assert!(position("### Observations") < position("### Task results"));
    assert!(position("### Task results") < position("### Reflections"));
    assert!(text.contains("- the sky is blue"));
    assert!(!text.contains("second agent"));
}

#[tokio::test]
async fn export_refuses_an_unknown_agent_a_bad_format_and_a_non_owner() {
    let f = Fixture::new();
    for uri in [
        "/api/memories/export?agentId=agent-missing",
        "/api/memories/export?format=md&agentId=agent-missing",
    ] {
        let reply = f.get(uri).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(reply.header("cache-control"), "no-store");
    }
    let reply = f.get("/api/memories/export?format=csv").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.json()["error"], "format must be json or md");
    let reply = f
        .call(FOREIGN_ORIGIN, "GET", "/api/memories/export", None)
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.header("cache-control"), "no-store");
}

#[test]
fn the_file_name_keeps_only_safe_characters() {
    use crate::routes::memory_export::filename_part;
    assert_eq!(filename_part("agent-1.a_b"), "agent-1.a_b");
    assert_eq!(filename_part("a/b\"c d\r\n"), "a_b_c_d__");
    assert_eq!(filename_part(""), "agent");
    assert_eq!(filename_part(&"x".repeat(200)).len(), 64);
}

fn ids(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            record["id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("{}:{}", record["kind"], record["id"]))
        })
        .collect()
}

#[tokio::test]
async fn the_all_agents_export_keeps_records_that_belong_to_no_agent_once() {
    let f = Fixture::new();
    f.seed().await;
    // A fact whose only evidence memory is then deleted: the delete keeps the
    // fact and empties its evidence.
    let evidence = f.memory(&f.second, "observation", "saw a heron").await;
    f.fact_with(&evidence, ("heron-watcher", "Watcher"), "birds")
        .await;
    let reply = f
        .call(
            OWNER_ORIGIN,
            "DELETE",
            &format!("/api/memories/{evidence}"),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    // An entity nothing names, and a relationship between two non-agents.
    let reply = f
        .call(
            OWNER_ORIGIN,
            "POST",
            "/api/memories/entities",
            Some(json!({ "kind": "external", "id": "mia", "name": "Mia" })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    let reply = f
        .call(
            OWNER_ORIGIN,
            "POST",
            "/api/memories/relationships",
            Some(json!({
                "sourceKind": "user",
                "sourceAgentId": "user-9",
                "sourceAgentName": "Ada",
                "targetKind": "external",
                "targetAgentId": "club",
                "targetAgentName": "Chess club",
                "relationshipType": "member_of",
            })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);

    let export = f.get("/api/memories/export").await.json();
    let rest = &export["unattributed"];
    let fact_values: Vec<&str> = rest["temporalFacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|fact| fact["value"].as_str().unwrap())
        .collect();
    assert_eq!(fact_values, ["birds"]);
    assert!(rest["entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entity| entity["name"] == "Mia"));
    let relationship_types: Vec<&str> = rest["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relationship| relationship["relationshipType"].as_str().unwrap())
        .collect();
    assert_eq!(relationship_types, ["member_of"]);

    // No record is under both an agent and `unattributed`, and together they
    // hold everything the store has.
    for key in ["entities", "relationships", "temporalFacts"] {
        let mut under_agents = Vec::new();
        for agent in export["agents"].as_array().unwrap() {
            under_agents.extend(ids(agent, key));
        }
        let unattributed = ids(rest, key);
        assert!(
            unattributed.iter().all(|id| !under_agents.contains(id)),
            "{key} appears twice: {unattributed:?} vs {under_agents:?}"
        );
    }
    let memory = f.state.read().await.memory_handle();
    let manager = memory.read().await;
    let facts_in_store = manager
        .list_temporal_facts(anima_memory::TemporalFactOptions {
            include_inactive: true,
            limit: Some(usize::MAX),
            ..Default::default()
        })
        .len();
    let relationships_in_store = manager
        .list_agent_relationships(anima_memory::AgentRelationshipOptions {
            limit: Some(usize::MAX),
            ..Default::default()
        })
        .len();
    let entities_in_store = manager
        .list_entities(anima_memory::MemoryEntityOptions {
            limit: Some(usize::MAX),
            ..Default::default()
        })
        .len();
    drop(manager);
    let total = |key: &str| {
        let mut all = std::collections::BTreeSet::new();
        for agent in export["agents"].as_array().unwrap() {
            all.extend(ids(agent, key));
        }
        all.extend(ids(rest, key));
        all.len()
    };
    assert_eq!(total("temporalFacts"), facts_in_store);
    assert_eq!(total("relationships"), relationships_in_store);
    assert_eq!(total("entities"), entities_in_store);

    // Markdown shows the section; a one-agent export has neither.
    let markdown = f.get("/api/memories/export?format=md").await;
    assert!(
        markdown.text.contains("\n## Unattributed\n"),
        "{}",
        markdown.text
    );
    assert!(markdown.text.contains("Mia (external)"));
    let one = f
        .get(&format!("/api/memories/export?agentId={}", f.first))
        .await;
    assert!(one.json().get("unattributed").is_none());
    let one_md = f
        .get(&format!(
            "/api/memories/export?format=md&agentId={}",
            f.first
        ))
        .await;
    assert!(!one_md.text.contains("Unattributed"));
}
