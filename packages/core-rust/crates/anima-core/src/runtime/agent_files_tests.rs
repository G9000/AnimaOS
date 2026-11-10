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
fn default_personality_yields_to_custom_system_and_soul_without_changing_config() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(PromptSpy(captured.clone()));
    let mut default_config = config();
    default_config.system = None;
    let personality = default_config.legacy_personality();
    assert!(personality.contains("## Default conversation behavior"));
    assert!(personality.contains("configured identity, personality, communication style"));
    let mut runtime = AgentRuntime::new(default_config.clone(), adapter.clone());
    runtime.set_host_guidance("host-permissions-retained".into());
    let run = |runtime: &mut AgentRuntime| {
        block_on(runtime.run(Content {
            text: "Please be brief and ask fewer questions.".into(),
            attachments: None,
            metadata: None,
        }));
    };
    run(&mut runtime);
    runtime.set_agent_files(crate::RenderedAgentFiles {
        identity: Some(crate::RuntimePromptIdentity {
            text: "Custom SOUL: be formal and reserved.".into(),
        }),
        project_context: "project-instructions-retained".into(),
        estimated_tokens: 20,
    });
    run(&mut runtime);
    runtime.set_agent_files(crate::RenderedAgentFiles::default());
    run(&mut runtime);
    assert_eq!(runtime.state.config, default_config);

    let prompts = captured.lock().unwrap();
    assert!(prompts[0].contains("## Default conversation behavior"));
    assert!(prompts[0].contains("legacy-style"));
    assert!(prompts[0].contains("legacy-adjective"));
    assert!(prompts[1].contains("Custom SOUL: be formal and reserved."));
    assert!(prompts[1].contains("project-instructions-retained"));
    assert!(!prompts[1].contains("## Default conversation behavior"));
    assert!(!prompts[1].contains("legacy-style"));
    assert_eq!(prompts[0], prompts[2]);
    for prompt in prompts.iter() {
        assert!(prompt.contains("retained-knowledge"));
        assert!(prompt.contains("host-permissions-retained"));
    }
    drop(prompts);

    for custom in ["Custom system: be terse.", ""] {
        let mut custom_config = default_config.clone();
        custom_config.system = Some(custom.into());
        let runtime = AgentRuntime::new(custom_config.clone(), adapter.clone());
        let prompt = runtime.build_system_prompt(&[]);
        assert!(!prompt.contains("## Default conversation behavior"));
        assert!(prompt.contains(custom));
        assert!(!custom_config
            .legacy_personality()
            .contains("## Default conversation behavior"));
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

struct LiveContext(std::sync::atomic::AtomicBool);
#[async_trait]
impl super::prompt_context::PromptContextSource for LiveContext {
    async fn current(&self, _: &AgentState) -> super::prompt_context::PromptContext {
        if self.0.load(std::sync::atomic::Ordering::SeqCst) {
            super::prompt_context::PromptContext {
                identity: Some("remote-identity".into()),
                reference: "remote-reference".into(),
                ..Default::default()
            }
        } else {
            super::prompt_context::PromptContext {
                reference: "context-unavailable".into(),
                ..Default::default()
            }
        }
    }
}

#[test]
fn workspace_context_transient_identity_online_offline_recovery_preserves_local_personality() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(PromptSpy(captured.clone()));
    let mut runtime = AgentRuntime::new(config(), adapter.clone());
    let source = Arc::new(LiveContext(std::sync::atomic::AtomicBool::new(true)));
    runtime.set_prompt_context(source.clone());
    let original = runtime.config().clone();
    for online in [true, false, true] {
        source.0.store(online, std::sync::atomic::Ordering::SeqCst);
        block_on(runtime.run(Content {
            text: "same-chat".into(),
            ..Default::default()
        }));
    }
    let prompts = captured.lock().unwrap();
    for index in [0, 2] {
        assert!(prompts[index].contains("remote-identity"));
        assert!(prompts[index].contains("remote-reference"));
        for local in [
            "legacy-bio",
            "legacy-lore",
            "legacy-adjective",
            "legacy-topic",
            "legacy-style",
            "legacy-system",
        ] {
            assert!(!prompts[index].contains(local), "{local}");
        }
    }
    assert!(prompts[1].contains("context-unavailable"));
    for local in [
        "legacy-bio",
        "legacy-lore",
        "legacy-adjective",
        "legacy-topic",
        "legacy-style",
        "legacy-system",
    ] {
        assert!(prompts[1].contains(local), "{local}");
    }
    assert!(!prompts[1].contains("remote-identity"));
    assert_eq!(runtime.config(), &original);
    let snapshot = runtime.snapshot();
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains("remote-identity"));
    let restored = AgentRuntime::from_snapshot(snapshot, adapter);
    assert!(!restored
        .build_system_prompt(&[])
        .contains("remote-identity"));
    assert!(restored.build_system_prompt(&[]).contains("legacy-system"));
}
