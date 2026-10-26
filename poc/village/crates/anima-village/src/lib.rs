//! Reusable village rules and NPC execution. HTTP and storage belong to hosts.
use serde::{Deserialize, Serialize};
mod content;
mod day_content;
mod npc;
mod traveler;
mod view;
pub use content::{choice_intent, reviewed_dialogue, suggestions};
pub use npc::{
    parse_generated_dialogue, play_with, reviewed_turn, reviewed_turn_with_memories,
    CallbackMemories, GameInput, HarnessNpc, NpcEngine, NpcStage, NpcStageExecutor, PlayedTurn,
};
pub use traveler::*;
pub use view::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Ja,
    Zh,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preference {
    Honey,
    Sesame,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NpcId {
    #[default]
    Tavi,
    Mira,
}
impl NpcId {
    pub fn name(self) -> &'static str {
        match self {
            Self::Tavi => "Tavi",
            Self::Mira => "Mira",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Day,
    Evening,
    Complete,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrderClaim {
    pub recipient: Option<String>,
    pub quantity: Option<u32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Intent {
    pub order: Option<OrderClaim>,
    pub delivery: Option<OrderClaim>,
    #[serde(default)]
    pub ask_supper: bool,
    pub preference: Option<Preference>,
    #[serde(default)]
    pub ask_recipe: bool,
    #[serde(default)]
    pub help: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Welcome,
    Delivered,
    AlreadyDelivered,
    NeedBread,
    ClarifyDelivery,
    WrongSpeaker,
    Supper,
    EveningCallback,
    Collected,
    AlreadyCollected,
    ClarifyOrder,
    Preference,
    Recipe,
    Help,
    Talk,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phrase {
    pub text: String,
    pub translation: String,
    pub reading: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    #[serde(default)]
    pub traveler: Option<Traveler>,
    #[serde(default)]
    pub npc_id: NpcId,
    #[serde(default)]
    pub phase: Phase,
    #[serde(default)]
    pub collected: bool,
    #[serde(default)]
    pub mira_conversation: Vec<String>,
    #[serde(default)]
    pub mira_visits: u32,
    pub language: Language,
    pub loaves: u32,
    pub coins: u32,
    pub bread_preference: Option<Preference>,
    pub visits: u32,
    pub recipe_discussed: bool,
    pub helped_recipe: bool,
    pub pending_order: Option<OrderClaim>,
    pub journal: Vec<Phrase>,
    pub conversation: Vec<String>,
}
impl GameState {
    pub fn new(language: Language) -> Self {
        Self {
            traveler: None,
            npc_id: NpcId::Tavi,
            phase: Phase::Day,
            collected: false,
            mira_conversation: vec![],
            mira_visits: 0,
            language,
            loaves: 0,
            coins: 0,
            bread_preference: None,
            visits: 0,
            recipe_discussed: false,
            helped_recipe: false,
            pending_order: None,
            journal: vec![content::intro_note(language)],
            conversation: vec![],
        }
    }
    pub fn has_collected(&self) -> bool {
        self.collected || self.loaves >= 2 || self.phase != Phase::Day
    }
    pub fn conversation(&self) -> &[String] {
        match self.npc_id {
            NpcId::Tavi => &self.conversation,
            NpcId::Mira => &self.mira_conversation,
        }
    }
    pub fn conversation_mut(&mut self) -> &mut Vec<String> {
        match self.npc_id {
            NpcId::Tavi => &mut self.conversation,
            NpcId::Mira => &mut self.mira_conversation,
        }
    }
    pub fn apply(&mut self, intent: Intent) -> Outcome {
        // Upgrade legacy collection evidence before inventory can be consumed.
        self.collected = self.has_collected();
        if let Some(claim) = intent.delivery {
            let outcome = if self.npc_id != NpcId::Mira {
                Outcome::WrongSpeaker
            } else if self.phase != Phase::Day {
                Outcome::AlreadyDelivered
            } else if !claim.recipient.as_deref().is_some_and(is_mira) || claim.quantity != Some(2)
            {
                Outcome::ClarifyDelivery
            } else if self.loaves != 2 {
                Outcome::NeedBread
            } else {
                self.loaves = 0;
                self.collected = true;
                self.phase = Phase::Evening;
                Outcome::Delivered
            };
            self.record(outcome);
            return outcome;
        }
        if self.npc_id == NpcId::Mira {
            let outcome = if intent.order.is_some() {
                Outcome::WrongSpeaker
            } else if intent.ask_supper {
                Outcome::Supper
            } else {
                Outcome::Talk
            };
            self.record(outcome);
            return outcome;
        }
        let mut outcome = Outcome::Talk;
        if let Some(preference) = intent.preference {
            self.bread_preference = Some(preference);
            outcome = Outcome::Preference;
        }
        if intent.ask_recipe {
            self.recipe_discussed = true;
            outcome = Outcome::Recipe;
        }
        if intent.help {
            self.recipe_discussed = true;
            self.helped_recipe = true;
            outcome = Outcome::Help;
        }
        if let Some(claim) = intent.order {
            if self.has_collected() {
                outcome = Outcome::AlreadyCollected;
            } else {
                let pending = self.pending_order.get_or_insert_with(Default::default);
                // A newly supplied field replaces an earlier wrong answer.
                if claim.recipient.is_some() {
                    pending.recipient = claim.recipient;
                }
                if claim.quantity.is_some() {
                    pending.quantity = claim.quantity;
                }
                let mira = pending.recipient.as_deref().is_some_and(|name| {
                    matches!(
                        name.trim().to_lowercase().as_str(),
                        "mira" | "ミラ" | "米拉"
                    )
                });
                if mira && pending.quantity == Some(2) {
                    self.collected = true;
                    self.loaves = 2;
                    self.coins = 20;
                    self.pending_order = None;
                    self.recipe_discussed = true;
                    outcome = Outcome::Collected;
                } else {
                    outcome = Outcome::ClarifyOrder;
                }
            }
        }
        if outcome == Outcome::Talk && !self.recipe_discussed {
            self.recipe_discussed = true;
            outcome = Outcome::Recipe;
        }
        self.record(outcome);
        outcome
    }
    pub fn visit(&mut self) -> Outcome {
        let outcome = match self.npc_id {
            NpcId::Mira => {
                self.mira_visits = self.mira_visits.saturating_add(1);
                Outcome::Welcome
            }
            NpcId::Tavi => {
                self.visits = self.visits.saturating_add(1);
                if self.phase == Phase::Evening {
                    self.phase = Phase::Complete;
                    Outcome::EveningCallback
                } else {
                    Outcome::Welcome
                }
            }
        };
        self.record(outcome);
        outcome
    }
    pub fn memories(&self) -> Vec<String> {
        let mut facts = Vec::new();
        if self.npc_id == NpcId::Mira {
            if self.phase != Phase::Day {
                facts.push("Received both loaves from the traveler for the village supper.".into());
            }
            return facts;
        }
        if self.has_collected() {
            facts.push("Collected Mira's two loaves; received 20 coins once.".into());
        }
        if self.phase == Phase::Complete {
            facts.push("The traveler reported delivering both loaves to Mira for supper.".into());
        }
        if let Some(preference) = self.bread_preference {
            facts.push(format!(
                "Prefers {} bread; Tavi sets it on the counter plate on return.",
                match preference {
                    Preference::Honey => "honey",
                    Preference::Sesame => "sesame",
                }
            ));
        }
        if self.helped_recipe {
            facts.push("Reassured Tavi about the new recipe; Tavi remembers the kindness.".into());
        }
        facts
    }
    fn record(&mut self, outcome: Outcome) {
        let dialogue = reviewed_dialogue(self, outcome);
        if !self.journal.iter().any(|p| p.text == dialogue.text) {
            self.journal.push(Phrase {
                text: dialogue.text,
                translation: dialogue.translation,
                reading: dialogue.reading,
            });
        }
        if self.journal.len() > 32 {
            self.journal.remove(1);
        }
    }
}

#[cfg(test)]
mod npc_tests;
#[cfg(test)]
mod rules_tests;

fn is_mira(name: &str) -> bool {
    matches!(
        name.trim().to_lowercase().as_str(),
        "mira" | "ミラ" | "米拉"
    )
}
