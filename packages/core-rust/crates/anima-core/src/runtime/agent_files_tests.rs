use super::*;
use async_trait::async_trait;
use futures::executor::block_on;
use std::sync::Mutex;

struct PromptSpy(Arc<Mutex<Vec<String>>>);
#[async_trait]
impl ModelAdapter for PromptSpy {
    fn provider(&self) -> &str {
        "offline-spy"
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<crate::ModelGenerateResponse, String> {
        self.0.lock().unwrap().push(request.system.clone());
        Ok(crate::ModelGenerateResponse {
            content: Content {
                text: "reply".into(),
                attachments: None,
                metadata: None,
            },
            tool_calls: None,
            usage: TokenUsage::default(),
            stop_reason: ModelStopReason::End,
        })
    }
}
fn config() -> AgentConfig {
    AgentConfig {
        name: "test".into(),
        model: "offline".into(),
        provider: None,
        bio: Some("legacy-bio".into()),
        lore: Some("legacy-lore".into()),
        adjectives: Some(vec!["legacy-adjective".into()]),
        topics: Some(vec!["legacy-topic".into()]),
        style: Some("legacy-style".into()),
        system: Some("legacy-system".into()),
        knowledge: Some(vec!["retained-knowledge".into()]),
        tools: None,
        plugins: None,
        settings: None,
        fallbacks: Vec::new(),
    }
}
#[test]
fn accepted_identity_replaces_persona_without_durable_text() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(PromptSpy(captured.clone()));
    let mut runtime = AgentRuntime::new(config(), adapter.clone());
    let original = runtime.state.config.clone();
    runtime.set_agent_files(crate::RenderedAgentFiles {
        identity: Some(crate::RuntimePromptIdentity {
            text: "accepted-soul".into(),
        }),
        project_context: "accepted-project".into(),
        estimated_tokens: 8,
    });
    runtime.set_host_guidance("immutable-host-role".into());
    for _ in 0..2 {
        block_on(runtime.run(Content {
            text: "hello".into(),
            attachments: None,
            metadata: None,
        }));
    }
    let prompts = captured.lock().unwrap();
    assert_eq!(prompts.len(), 2);
    for prompt in prompts.iter() {
        for absent in [
            "legacy-bio",
            "legacy-lore",
            "legacy-adjective",
            "legacy-topic",
            "legacy-style",
            "legacy-system",
        ] {
            assert!(!prompt.contains(absent), "{absent}");
        }
        for retained in [
            "accepted-soul",
            "accepted-project",
            "immutable-host-role",
            "retained-knowledge",
        ] {
            assert_eq!(prompt.matches(retained).count(), 1);
        }
    }
    assert_eq!(runtime.state.config, original);
    let snapshot = runtime.snapshot();
    let json = serde_json::to_string(&snapshot).unwrap();
    for transient in ["accepted-soul", "accepted-project", "immutable-host-role"] {
        assert!(!json.contains(transient));
    }
    let restored = AgentRuntime::from_snapshot(snapshot, adapter);
    let prompt = restored.build_system_prompt(&["provider-context".into()]);
    for legacy in [
        "legacy-bio",
        "legacy-lore",
        "legacy-adjective",
        "legacy-topic",
        "legacy-style",
        "legacy-system",
        "retained-knowledge",
        "provider-context",
    ] {
        assert!(prompt.contains(legacy));
    }
}
#[test]
fn absent_identity_preserves_complete_legacy_and_can_replace_snapshot() {
    let mut runtime = AgentRuntime::new(
        config(),
        Arc::new(PromptSpy(Arc::new(Mutex::new(Vec::new())))),
    );
    let baseline = runtime.build_system_prompt(&[]);
    runtime.set_agent_files(crate::RenderedAgentFiles {
        project_context: "project-only".into(),
        ..Default::default()
    });
    assert!(runtime.build_system_prompt(&[]).starts_with(&baseline));
    runtime.set_agent_files(crate::RenderedAgentFiles::default());
    assert_eq!(runtime.build_system_prompt(&[]), baseline);
}
#[test]
fn empty_identity_keeps_persona_and_runtime_instances_are_isolated() {
    let adapter = Arc::new(PromptSpy(Arc::new(Mutex::new(Vec::new()))));
    let mut first = AgentRuntime::new(config(), adapter.clone());
    let mut second = AgentRuntime::new(config(), adapter);
    first.set_agent_files(crate::RenderedAgentFiles {
        identity: Some(crate::RuntimePromptIdentity {
            text: "first-only".into(),
        }),
        ..Default::default()
    });
    second.set_agent_files(crate::RenderedAgentFiles {
        identity: Some(crate::RuntimePromptIdentity { text: " \n".into() }),
        ..Default::default()
    });
    assert!(first.build_system_prompt(&[]).contains("first-only"));
    let prompt = second.build_system_prompt(&[]);
    assert!(!prompt.contains("first-only"));
    assert!(prompt.contains("legacy-system"));
}
