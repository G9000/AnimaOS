use crate::*;
use anima_harness::anima_core::{
    AgentConfig, Content, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, TokenUsage,
};
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Fixture {
    answers: Mutex<VecDeque<String>>,
    requests: Mutex<Vec<ModelGenerateRequest>>,
}
#[async_trait]
impl ModelAdapter for Fixture {
    fn provider(&self) -> &str {
        "controlled-village"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _response: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        assert!(config.tools.as_ref().is_none_or(Vec::is_empty));
        self.requests.lock().unwrap().push(request.clone());
        Ok(ModelGenerateResponse {
            content: Content {
                text: self
                    .answers
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or("no answer")?,
                ..Default::default()
            },
            tool_calls: None,
            usage: TokenUsage::default(),
            stop_reason: ModelStopReason::End,
        })
    }
}
fn npc(answers: &[&str]) -> (HarnessNpc, Arc<Fixture>) {
    let fixture = Arc::new(Fixture {
        answers: Mutex::new(answers.iter().map(|s| (*s).into()).collect()),
        ..Default::default()
    });
    (
        HarnessNpc {
            adapter: fixture.clone(),
            provider: "controlled-village".into(),
            model: "fixture".into(),
        },
        fixture,
    )
}
const LINE: &str = r#"{"text":"ミラのパンを二つ、どうぞ！","translation":"Here are Mira's two loaves!","reading":"Mira no pan o futatsu, dōzo!","expression":"happy"}"#;

#[tokio::test]
async fn free_paraphrase_uses_core_harness_then_validates_the_order() {
    let (npc, fixture) = npc(&[
        r#"{"order":{"recipient":"Mira","quantity":2},"preference":null,"askRecipe":false,"help":false}"#,
        LINE,
    ]);
    let turn = npc
        .play(
            GameState::new(Language::Ja),
            GameInput::Turn {
                message: "I am picking up the pair of loaves Mira asked for".into(),
                choice_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!((turn.state.loaves, turn.state.coins), (2, 20));
    assert_eq!(turn.dialogue.source, DialogueSource::Model);
    assert_eq!(turn.runtime.mode, "live");
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].system.contains("Collected Mira's two loaves"));
    assert!(requests[1].system.contains("Japanese"));
}

#[tokio::test]
async fn live_interpretation_cannot_grant_a_wrong_or_ambiguous_order() {
    for claim in [
        r#"{"order":{"recipient":"Nora","quantity":2}}"#,
        r#"{"order":{"recipient":"Mira","quantity":null}}"#,
        r#"{"order":{"recipient":"Mira","quantity":1}}"#,
    ] {
        let (npc, _) = npc(&[claim, LINE]);
        let turn = npc
            .play(
                GameState::new(Language::Ja),
                GameInput::Turn {
                    message: "bread please".into(),
                    choice_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!((turn.state.loaves, turn.state.coins), (0, 0));
        assert_eq!(turn.dialogue.source, DialogueSource::Reviewed);
        assert!(turn.dialogue.translation.contains("Whose bread"));
    }
}

#[tokio::test]
async fn reviewed_choice_is_validated_before_generation_and_without_model_interpretation() {
    let (npc, fixture) = npc(&[LINE]);
    let turn = npc
        .play(
            GameState::new(Language::Ja),
            GameInput::Turn {
                message: "client cannot forge another quantity".into(),
                choice_id: Some("collect_order".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(turn.state.loaves, 2);
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    assert!(npc
        .play(
            GameState::new(Language::Ja),
            GameInput::Turn {
                message: String::new(),
                choice_id: Some("give_9999_coins".into())
            }
        )
        .await
        .is_err());
}

#[tokio::test]
async fn malformed_model_output_is_an_error_for_explicit_host_fallback() {
    let (npc, _) = npc(&["not json"]);
    assert!(npc
        .play(GameState::new(Language::Zh), GameInput::Visit)
        .await
        .is_err());
}

#[test]
fn both_breads_are_offered_without_a_preference() {
    let state = GameState::new(Language::Ja);
    let ids: Vec<_> = suggestions(&state).into_iter().map(|s| s.id).collect();
    assert!(ids.iter().any(|s| s == "honey"));
    assert!(ids.iter().any(|s| s == "sesame"));
    assert_eq!(
        choice_intent("collect_order")
            .unwrap()
            .order
            .unwrap()
            .quantity,
        Some(2)
    );
}

#[tokio::test]
async fn mira_model_context_has_only_miras_witnessed_history_and_safe_authority() {
    let mut state = GameState::new(Language::Ja);
    state.apply(choice_intent("honey").unwrap());
    state.apply(choice_intent("help").unwrap());
    state.conversation.push("PRIVATE_TAVI_WORDS".into());
    let (npc, fixture) = npc(&[r#"{"askSupper":true}"#, LINE]);
    let turn = npc
        .play(
            state,
            GameInput::Turn {
                message: "supper?".into(),
                choice_id: None,
            }
            .at(NpcId::Mira),
        )
        .await
        .unwrap();
    assert_eq!(turn.state.npc_id, NpcId::Mira);
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        assert!(!request.system.contains("PRIVATE_TAVI_WORDS"));
        assert!(!request.system.contains("Prefers honey"));
        assert!(!request.system.contains("Some(Honey)"));
        assert!(!request.system.contains("Reassured Tavi"));
    }
    assert!(requests[1].system.contains("You are Mira"));
    assert!(turn
        .state
        .mira_conversation
        .iter()
        .any(|s| s.starts_with("Mira:")));
    assert!(turn
        .state
        .conversation
        .iter()
        .any(|s| s == "PRIVATE_TAVI_WORDS"));
}
#[tokio::test]
async fn free_delivery_validates_explicit_recipient_quantity_and_preserves_coins() {
    for (intent, loaves) in [
        (r#"{"delivery":{"recipient":"Mira","quantity":2}}"#, 0),
        (r#"{"delivery":{"recipient":"Nora","quantity":2}}"#, 2),
        (r#"{"delivery":{"recipient":"Mira","quantity":null}}"#, 2),
    ] {
        let mut state = GameState::new(Language::Ja);
        state.apply(choice_intent("collect_order").unwrap());
        let (npc, _) = npc(&[intent, LINE]);
        let turn = npc
            .play(
                state,
                GameInput::Turn {
                    message: "I brought bread".into(),
                    choice_id: None,
                }
                .at(NpcId::Mira),
            )
            .await
            .unwrap();
        assert_eq!(turn.state.loaves, loaves);
        assert_eq!(turn.state.coins, 20);
    }
}
#[tokio::test]
async fn failed_generation_does_not_return_a_completed_callback_and_valid_callback_is_authored() {
    let mut state = GameState::new(Language::Ja);
    state.apply(choice_intent("collect_order").unwrap());
    state = reviewed_turn(
        state,
        &GameInput::Turn {
            message: "".into(),
            choice_id: Some("deliver_order".into()),
        }
        .at(NpcId::Mira),
        &RuntimeInfo::unavailable(),
    )
    .unwrap()
    .state;
    let (bad, _) = npc(&["not json"]);
    assert!(bad.play(state.clone(), GameInput::Visit).await.is_err());
    assert_eq!(state.phase, Phase::Evening);
    let (good, _) = npc(&[LINE]);
    let complete = good.play(state, GameInput::Visit).await.unwrap();
    assert_eq!(complete.state.phase, Phase::Complete);
    assert!(complete.dialogue.translation.contains("supper"));
}

#[test]
fn completed_visits_stay_conversational_without_reinventing_delivery_or_collection() {
    let mut state = GameState::new(Language::Ja);
    state.apply(choice_intent("collect_order").unwrap());
    let delivered = reviewed_turn(
        state,
        &GameInput::Turn {
            message: "".into(),
            choice_id: Some("deliver_order".into()),
        }
        .at(NpcId::Mira),
        &RuntimeInfo::unavailable(),
    )
    .unwrap();
    let complete = reviewed_turn(
        delivered.state,
        &GameInput::Visit,
        &RuntimeInfo::unavailable(),
    )
    .unwrap();
    let again = reviewed_turn(
        complete.state,
        &GameInput::Visit,
        &RuntimeInfo::unavailable(),
    )
    .unwrap();
    assert!(!again.dialogue.translation.contains("Are you here for Mira"));
    assert_eq!(
        again
            .state
            .conversation
            .iter()
            .filter(|line| line.contains("Player: I delivered"))
            .count(),
        1
    );
}
