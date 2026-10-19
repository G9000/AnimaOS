//! A run falls back to the agent's next model and stays there for the run.
use super::test_support::{calculate_call, chat_request, coordinator_with, ScriptedModel, Step};
use anima_core::{AgentConfigUpdate, ModelChoice};

#[tokio::test]
async fn a_run_falls_back_and_sticks_and_the_next_run_starts_at_the_primary() {
    let model = ScriptedModel::new(vec![
        Step::Fail("OpenAI API error (429): slow down"),
        Step::Tools(vec![calculate_call("call-1", "2 + 2")]),
        Step::Text(vec!["four"]),
        Step::Text(vec!["back on the primary"]),
    ]);
    let (coordinator, agent_id) = coordinator_with(model.clone()).await;
    coordinator
        .state
        .write()
        .await
        .update_agent(
            &agent_id,
            AgentConfigUpdate {
                fallbacks: Some(vec![ModelChoice {
                    provider: "anthropic".into(),
                    model: "claude-fallback".into(),
                }]),
                ..AgentConfigUpdate::default()
            },
        )
        .unwrap();

    let first = coordinator
        .run(chat_request(&agent_id, "chat:fallback", "add"))
        .await
        .unwrap();
    let second = coordinator
        .run(chat_request(&agent_id, "chat:fallback", "again"))
        .await
        .unwrap();

    assert_eq!(first.result.status, "success", "{:?}", first.result.error);
    assert_eq!(first.result.data.unwrap().text, "four");
    assert_eq!(second.result.data.unwrap().text, "back on the primary");
    let models: Vec<_> = model
        .configs()
        .iter()
        .map(|config| format!("{}/{}", config.provider.as_deref().unwrap(), config.model))
        .collect();
    assert_eq!(
        models,
        vec![
            "openai/gpt-5.4",
            "anthropic/claude-fallback",
            "anthropic/claude-fallback",
            "openai/gpt-5.4",
        ]
    );
}

#[test]
fn a_helper_inherits_its_parents_fallbacks_with_its_model() {
    let mut parent = super::test_support::companion_config("companion");
    parent.fallbacks = vec![ModelChoice {
        provider: "anthropic".into(),
        model: "claude-fallback".into(),
    }];
    let mut state = crate::state::DaemonState::new();
    let parent = state.create_agent(parent).unwrap().state;

    let helper = super::helper_config(&parent, "helper".into());

    assert_eq!(helper.model, parent.config.model);
    assert_eq!(helper.fallbacks, parent.config.fallbacks);
}
