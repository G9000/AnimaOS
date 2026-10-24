use crate::{GameState, Language, NpcId, Phase, Phrase, Preference, Traveler};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Expression {
    Warm,
    Thoughtful,
    Happy,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DialogueSource {
    Model,
    Reviewed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub text: String,
    pub translation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialogue {
    pub text: String,
    pub translation: String,
    pub reading: String,
    pub expression: Expression,
    pub source: DialogueSource,
    pub suggestions: Vec<Suggestion>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub mode: String,
    pub engine: String,
    pub provider: String,
    pub model: String,
}
impl RuntimeInfo {
    pub fn reviewed(&self) -> Self {
        Self {
            mode: "reviewed".into(),
            ..self.clone()
        }
    }
    pub fn unavailable() -> Self {
        Self {
            mode: "reviewed".into(),
            engine: "anima-harness".into(),
            provider: "unavailable".into(),
            model: "unavailable".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub name: String,
    pub quantity: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    #[serde(default)]
    pub traveler: Option<Traveler>,
    #[serde(default)]
    pub npc_id: NpcId,
    #[serde(default)]
    pub phase: Phase,
    pub save_id: String,
    pub language: Language,
    pub quest_status: String,
    pub inventory: Vec<InventoryItem>,
    pub coins: u32,
    pub bread_preference: Option<Preference>,
    pub visits: u32,
    pub memories: Vec<String>,
    pub journal: Vec<Phrase>,
    pub dialogue: Dialogue,
    pub runtime: RuntimeInfo,
}
impl GameView {
    pub fn new(
        save_id: String,
        state: &GameState,
        dialogue: Dialogue,
        runtime: RuntimeInfo,
    ) -> Self {
        Self {
            traveler: state.traveler.clone(),
            npc_id: state.npc_id,
            phase: state.phase,
            save_id,
            language: state.language,
            quest_status: if state.phase != Phase::Day {
                "delivered"
            } else if state.has_collected() {
                "collected"
            } else {
                "pending"
            }
            .into(),
            inventory: if state.loaves == 0 {
                vec![]
            } else {
                vec![InventoryItem {
                    id: "mira_loaves".into(),
                    name: match state.language {
                        Language::Ja => "ミラのパン",
                        Language::Zh => "米拉的面包",
                    }
                    .into(),
                    quantity: state.loaves,
                }]
            },
            coins: state.coins,
            bread_preference: state.bread_preference,
            visits: state.visits,
            memories: state.memories(),
            journal: state.journal.clone(),
            dialogue,
            runtime,
        }
    }
}
