//! Self-delegation through native helper tools and recorded model requests.
use super::test_support::{calculate_call, chat_request, lead_config, Gate, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::skills::test_support::{temp_workspace, with_workspace};
use crate::state::DaemonState;
use anima_core::{DataValue, MessageRole, ToolCall};
use std::collections::BTreeMap;
use std::sync::Arc;

fn assignment(id: &str, playbook: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "spawn_helper".into(),
        args: BTreeMap::from([
            ("name".into(), DataValue::String("Copy for research".into())),
            (
                "task".into(),
                DataValue::String("Check the project facts".into()),
            ),
            ("playbook".into(), DataValue::String(playbook.into())),
        ]),
    }
}

#[test]
fn self_clone_keeps_the_model_but_omits_legacy_personality_and_owner_knowledge() {
    let mut parent = lead_config("Companion");
    parent.bio = Some("Identity: calm, curious companion".into());
    parent.lore = Some("Backstory: built for this owner".into());
    parent.style = Some("Speak plainly".into());
    parent.adjectives = Some(vec!["thoughtful".into()]);
    parent.topics = Some(vec!["design".into()]);
    parent.system = Some("Keep the same core values".into());
    parent.knowledge = Some(vec!["Private owner context".into()]);
    let parent = anima_core::AgentState {
        id: "parent".into(),
        name: parent.name.clone(),
        config: parent,
        status: anima_core::AgentStatus::Idle,
        created_at_ms: 1,
        token_usage: Default::default(),
    };
    let helper = super::helper_config(&parent, "Research assignment".into());
    assert!(!helper
        .legacy_personality()
        .contains("Keep the same core values"));
    assert!(!helper
        .legacy_personality()
        .contains("Identity: calm, curious companion"));
    assert!(helper.legacy_personality().contains("bounded task helper"));
    assert_eq!(helper.model, parent.config.model);
    assert_eq!(helper.provider, parent.config.provider);
    assert!(helper.knowledge.is_none());
    assert!(super::is_helper_config(&helper));
    assert_eq!(
        super::helper_parent(&anima_core::AgentState {
            config: helper,
            ..parent
        }),
        Some("parent")
    );
}

#[tokio::test]
async fn self_clone_omits_main_soul_and_uses_its_own_playbook_through_the_native_tool() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![assignment(
            "clone-1",
            "Research playbook sentinel: cite evidence",
        )]),
        Step::Text(vec!["Helper evidence"]),
        Step::Text(vec!["Companion result"]),
    ]);
    let root = temp_workspace("self-clone-soul");
    std::fs::write(
        root.join("SOUL.md"),
        "Active inherited personality sentinel: warm and precise",
    )
    .unwrap();
    let mut config = lead_config("Companion");
    config.bio = Some("Obsolete biography sentinel".into());
    config.system = Some("Obsolete legacy sentinel".into());
    let mut state = with_workspace(DaemonState::with_model_adapter(model.clone()), &root);
    let parent = state.create_agent(config.clone()).unwrap().state;
    let coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(tokio::sync::Semaphore::new(4)),
    );
    coordinator
        .run(chat_request(
            &parent.id,
            "chat:self-clone",
            "Ask your copy to research",
        ))
        .await
        .unwrap();
    let requests = model.requests();
    assert_eq!(requests.len(), 3, "the native helper must actually run");
    assert!(requests[0]
        .system
        .contains("Active inherited personality sentinel"));
    let child = &requests[1];
    assert!(!child
        .system
        .contains("Active inherited personality sentinel"));
    assert!(!child.system.contains("Obsolete legacy sentinel"));
    assert!(!child.system.contains("Obsolete biography sentinel"));
    assert!(child.system.contains("bounded task helper"));
    assert!(child
        .messages
        .iter()
        .any(|message| message.role == MessageRole::User
            && message.content.text.contains("Research playbook sentinel")));
    assert!(!requests[2].system.contains("Research playbook sentinel"));
    let guard = coordinator.state.read().await;
    assert_eq!(guard.get_agent(&parent.id).unwrap().state.config, config);
    let snapshot = serde_json::to_string(&guard.control_plane_snapshot()).unwrap();
    assert!(
        !snapshot.contains("Active inherited personality sentinel"),
        "raw SOUL stays in the source file, not copied into durable helper configuration"
    );
}

#[tokio::test]
async fn self_clone_omits_main_profile_and_isolates_playbooks_when_reused() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![assignment("copy-first", "First playbook sentinel")]),
            Step::Tools(vec![calculate_call("calc-copy", "6*7")]),
            Step::Text(vec!["First child result"]),
            Step::Text(vec!["First parent result"]),
            Step::Tools(vec![assignment("copy-second", "Second playbook sentinel")]),
            Step::Text(vec!["Second child result"]),
            Step::Text(vec!["Second parent result"]),
        ],
        gate.clone(),
    );
    let root = temp_workspace("self-clone-profile");
    std::fs::write(
        root.join("SOUL.md"),
        "Workspace identity that must not replace the profile",
    )
    .unwrap();
    let mut config = lead_config("Companion");
    config.bio = Some("Obsolete family biography sentinel".into());
    config.settings.as_mut().unwrap().additional.insert(
        crate::agent_files::runtime::PROFILE_SETTING.into(),
        DataValue::Bool(true),
    );
    let mut state = with_workspace(DaemonState::with_model_adapter(model.clone()), &root);
    let parent = state.create_agent(config.clone()).unwrap().state;
    let profile = root.join(crate::agent_files::profile_relative(&parent.id));
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(
        profile.join("SOUL.md"),
        "Profile personality alpha sentinel",
    )
    .unwrap();
    let coordinator = AgentRunCoordinator::new(
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(tokio::sync::Semaphore::new(4)),
    );
    let first = {
        let coordinator = coordinator.clone();
        let id = parent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &id,
                    "chat:clone-profile",
                    "Private owner history sentinel",
                ))
                .await
        })
    };
    gate.entered().await;
    gate.release();
    gate.entered().await;
    assert!(!model.requests()[1]
        .system
        .contains("Profile personality alpha sentinel"));
    assert!(!model.requests()[1]
        .system
        .contains("Workspace identity that must not replace the profile"));
    std::fs::write(profile.join("SOUL.md"), "Profile personality beta sentinel").unwrap();
    gate.release();
    gate.entered().await;
    assert!(!model.requests()[2]
        .system
        .contains("Profile personality alpha sentinel"));
    assert!(!model.requests()[2]
        .system
        .contains("Profile personality beta sentinel"));
    gate.release();
    gate.entered().await;
    gate.release();
    first.await.unwrap().unwrap();
    let second = {
        let coordinator = coordinator.clone();
        let id = parent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &id,
                    "chat:clone-profile",
                    "Run another assignment",
                ))
                .await
        })
    };
    gate.entered().await;
    gate.release();
    gate.entered().await;
    let requests = model.requests();
    assert!(!requests[4]
        .system
        .contains("Obsolete family biography sentinel"));
    let child = &requests[5];
    assert!(requests[4]
        .system
        .contains("Profile personality beta sentinel"));
    assert!(!child.system.contains("Profile personality beta sentinel"));
    let input = child
        .messages
        .iter()
        .map(|m| m.content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(input.contains("Second playbook sentinel"));
    assert!(!input.contains("First playbook sentinel"));
    assert!(!input.contains("Private owner history sentinel"));
    gate.release();
    gate.entered().await;
    gate.release();
    second.await.unwrap().unwrap();
    let guard = coordinator.state.read().await;
    let helpers: Vec<_> = guard
        .agent_states()
        .into_iter()
        .filter(|a| super::helper_parent(a) == Some(parent.id.as_str()))
        .collect();
    assert_eq!(
        helpers.len(),
        1,
        "an idle copy is reused with a fresh assignment session"
    );
    assert_eq!(guard.runs.for_agent(&helpers[0].id).len(), 2);
    assert_eq!(guard.get_agent(&parent.id).unwrap().state.config, config);
    assert!(!serde_json::to_string(&guard.control_plane_snapshot())
        .unwrap()
        .contains("Profile personality beta sentinel"));
}
