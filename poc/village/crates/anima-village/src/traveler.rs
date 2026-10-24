use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinTone {
    Fair,
    Warm,
    Deep,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HairStyle {
    Short,
    Bob,
    Braid,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HairColor {
    Chestnut,
    Ink,
    Gold,
    Auburn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outfit {
    Cloak,
    Tunic,
    Overalls,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutfitColor {
    Moss,
    Ochre,
    Berry,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Traveler {
    pub name: String,
    pub skin_tone: SkinTone,
    pub hair_style: HairStyle,
    pub hair_color: HairColor,
    pub outfit: Outfit,
    pub outfit_color: OutfitColor,
}
impl Traveler {
    pub fn validated(mut self) -> Result<Self, String> {
        if self.name.chars().any(char::is_control) {
            return Err("Traveler name cannot contain control characters".into());
        }
        self.name = self.name.trim().to_owned();
        if !(1..=24).contains(&self.name.chars().count()) {
            return Err("Traveler name must contain 1–24 trimmed characters".into());
        }
        Ok(self)
    }
}
