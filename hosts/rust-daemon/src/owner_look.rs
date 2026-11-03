//! How the owner's console looks: theme, accent, text, the chat background,
//! and how much of an agent's work it shows. The daemon keeps it so the owner
//! can change it by asking in chat, and every open console follows.
use std::collections::BTreeMap;

use anima_core::DataValue;
use serde::{Deserialize, Serialize};

/// Each choice the console offers, as it names them. A field missing from a
/// saved record or a request takes its default, so an older record loads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct OwnerLook {
    /// `dark`, `light`, or `system`.
    pub(crate) theme: String,
    /// `pink`, `lavender`, `amber`, `sage`, `coral`, `paper`, or `custom`
    /// for the owner's own colour in `customAccent`.
    pub(crate) accent: String,
    /// The owner's own accent as `#rrggbb`, kept while a preset is chosen.
    pub(crate) custom_accent: String,
    /// `small`, `default`, or `large`.
    pub(crate) text_size: String,
    /// `comfortable` or `compact`.
    pub(crate) density: String,
    pub(crate) reduce_motion: bool,
    /// Do/Plan and the model picker on the chat input.
    pub(crate) advanced_input: bool,
    /// Each tool step while an agent works; off shows one plain line.
    pub(crate) show_work: bool,
    /// What sits behind the chat: `full`, `quiet`, or `off`.
    pub(crate) chat_background: String,
}

const THEMES: &[&str] = &["dark", "light", "system"];
/// The preset accents; `custom` uses the owner's own colour.
pub(crate) const ACCENTS: &[&str] = &["pink", "lavender", "amber", "sage", "coral", "paper"];
const TEXT_SIZES: &[&str] = &["small", "default", "large"];
const DENSITIES: &[&str] = &["comfortable", "compact"];
const CHAT_BACKGROUNDS: &[&str] = &["full", "quiet", "off"];
const VIEWS: &[&str] = &["simple", "advanced"];

/// The choices `change_look` takes, with what each may be; `view` sets the
/// two power-user switches together. `accent` also takes any `#rrggbb`.
pub(crate) const LOOK_CHOICES: &[(&str, &[&str])] = &[
    ("view", VIEWS),
    ("theme", THEMES),
    ("textSize", TEXT_SIZES),
    ("density", DENSITIES),
    ("chatBackground", CHAT_BACKGROUNDS),
];
pub(crate) const LOOK_SWITCHES: &[&str] = &["reduceMotion", "advancedInput", "showWork"];

impl Default for OwnerLook {
    /// What a new console shows: simple, dark, pink, the quiet background.
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            accent: "pink".into(),
            custom_accent: "#e94dab".into(),
            text_size: "default".into(),
            density: "comfortable".into(),
            reduce_motion: false,
            advanced_input: false,
            show_work: false,
            chat_background: "quiet".into(),
        }
    }
}

fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("{field} must be one of: {}", allowed.join(", ")))
    }
}

/// `#` and six hex digits.
fn is_hex_colour(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

const ACCENT_EXPECTED: &str =
    "accent must be pink, lavender, amber, sage, coral, paper, custom, or a #rrggbb colour";

impl OwnerLook {
    /// Every choice is one the console knows.
    pub(crate) fn validate(&self) -> Result<(), String> {
        one_of("theme", &self.theme, THEMES)?;
        if self.accent != "custom" && !ACCENTS.contains(&self.accent.as_str()) {
            return Err(ACCENT_EXPECTED.into());
        }
        if !is_hex_colour(&self.custom_accent) {
            return Err("customAccent must be a #rrggbb colour".into());
        }
        one_of("textSize", &self.text_size, TEXT_SIZES)?;
        one_of("density", &self.density, DENSITIES)?;
        one_of("chatBackground", &self.chat_background, CHAT_BACKGROUNDS)
    }

    /// This look with a tool call's changes, and the names of the choices
    /// that actually changed. Every argument is checked before any applies.
    pub(crate) fn changed_by(
        &self,
        args: &BTreeMap<String, DataValue>,
    ) -> Result<(OwnerLook, Vec<&'static str>), String> {
        let mut next = self.clone();
        for (key, value) in args {
            if key == "accent" {
                let DataValue::String(choice) = value else {
                    return Err("accent must be a string".into());
                };
                let choice = choice.trim().to_ascii_lowercase();
                if is_hex_colour(&choice) {
                    next.accent = "custom".into();
                    next.custom_accent = choice;
                } else if choice == "custom" || ACCENTS.contains(&choice.as_str()) {
                    next.accent = choice;
                } else {
                    return Err(ACCENT_EXPECTED.into());
                }
            } else if let Some((name, allowed)) = LOOK_CHOICES.iter().find(|(name, _)| name == key)
            {
                let DataValue::String(choice) = value else {
                    return Err(format!("{name} must be a string"));
                };
                let choice = choice.trim().to_ascii_lowercase();
                one_of(name, &choice, allowed)?;
                match *name {
                    "view" => {
                        let advanced = choice == "advanced";
                        next.show_work = advanced;
                        next.advanced_input = advanced;
                    }
                    "theme" => next.theme = choice,
                    "textSize" => next.text_size = choice,
                    "density" => next.density = choice,
                    _ => next.chat_background = choice,
                }
            } else if let Some(name) = LOOK_SWITCHES.iter().find(|name| **name == key) {
                let DataValue::Bool(on) = value else {
                    return Err(format!("{name} must be true or false"));
                };
                match *name {
                    "reduceMotion" => next.reduce_motion = *on,
                    "advancedInput" => next.advanced_input = *on,
                    _ => next.show_work = *on,
                }
            } else {
                return Err(format!("Unknown change_look argument: {key}"));
            }
        }
        let changed = [
            ("theme", self.theme != next.theme),
            ("accent", self.accent != next.accent),
            ("customAccent", self.custom_accent != next.custom_accent),
            ("textSize", self.text_size != next.text_size),
            ("density", self.density != next.density),
            ("reduceMotion", self.reduce_motion != next.reduce_motion),
            ("advancedInput", self.advanced_input != next.advanced_input),
            ("showWork", self.show_work != next.show_work),
            (
                "chatBackground",
                self.chat_background != next.chat_background,
            ),
        ]
        .into_iter()
        .filter_map(|(name, differs)| differs.then_some(name))
        .collect();
        Ok((next, changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DaemonState;

    fn args(pairs: &[(&str, DataValue)]) -> BTreeMap<String, DataValue> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn advanced_view_turns_on_both_power_user_switches_and_simple_turns_them_off() {
        let (advanced, changed) = OwnerLook::default()
            .changed_by(&args(&[("view", DataValue::String("Advanced".into()))]))
            .unwrap();
        assert!(advanced.show_work && advanced.advanced_input);
        assert_eq!(changed, ["advancedInput", "showWork"]);
        let (simple, _) = advanced
            .changed_by(&args(&[("view", DataValue::String("simple".into()))]))
            .unwrap();
        assert_eq!(simple, OwnerLook::default());
    }

    #[test]
    fn a_choice_the_console_does_not_offer_changes_nothing() {
        let error = OwnerLook::default()
            .changed_by(&args(&[
                ("theme", DataValue::String("light".into())),
                ("accent", DataValue::String("teal".into())),
            ]))
            .unwrap_err();
        assert!(error.contains("pink, lavender"), "{error}");
        assert!(OwnerLook::default()
            .changed_by(&args(&[("showWork", DataValue::String("yes".into()))]))
            .is_err());
        assert!(OwnerLook::default()
            .changed_by(&args(&[("font", DataValue::String("serif".into()))]))
            .is_err());
    }

    #[test]
    fn any_colour_becomes_the_custom_accent_and_presets_keep_it() {
        let (custom, changed) = OwnerLook::default()
            .changed_by(&args(&[("accent", DataValue::String(" #14B8A6 ".into()))]))
            .unwrap();
        assert_eq!(
            (custom.accent.as_str(), custom.custom_accent.as_str()),
            ("custom", "#14b8a6")
        );
        assert_eq!(changed, ["accent", "customAccent"]);
        let (preset, changed) = custom
            .changed_by(&args(&[("accent", DataValue::String("sage".into()))]))
            .unwrap();
        assert_eq!(
            preset.custom_accent, "#14b8a6",
            "the owner's colour is kept"
        );
        assert_eq!(changed, ["accent"]);
        for bad in ["#14b8a", "teal", "#zzzzzz"] {
            assert!(
                OwnerLook::default()
                    .changed_by(&args(&[("accent", DataValue::String(bad.into()))]))
                    .is_err(),
                "{bad}"
            );
        }
        let stored = OwnerLook {
            custom_accent: "blue".into(),
            ..OwnerLook::default()
        };
        assert!(stored.validate().is_err());
    }

    #[test]
    fn only_choices_that_differ_count_as_changed() {
        let (look, changed) = OwnerLook::default()
            .changed_by(&args(&[
                ("theme", DataValue::String("dark".into())),
                ("chatBackground", DataValue::String("off".into())),
            ]))
            .unwrap();
        assert_eq!(look.chat_background, "off");
        assert_eq!(changed, ["chatBackground"]);
    }

    #[test]
    fn a_partial_saved_record_fills_in_defaults() {
        let look: OwnerLook =
            serde_json::from_value(serde_json::json!({"theme": "light"})).unwrap();
        assert_eq!(look.theme, "light");
        assert_eq!(look.chat_background, "quiet");
        assert!(look.validate().is_ok());
    }

    #[test]
    fn the_look_round_trips_through_the_control_plane() {
        let mut source = DaemonState::new();
        let look = OwnerLook {
            theme: "light".into(),
            show_work: true,
            ..OwnerLook::default()
        };
        source.set_owner_look(Some(look.clone()));
        let payload = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        assert_eq!(payload["ownerLook"]["theme"], "light");
        let mut restored = DaemonState::new();
        restored
            .restore_control_plane_snapshot(serde_json::from_value(payload).unwrap())
            .unwrap();
        assert_eq!(restored.owner_look.as_deref(), Some(&look));

        let mut payload =
            serde_json::to_value(DaemonState::new().control_plane_snapshot()).unwrap();
        payload.as_object_mut().unwrap().remove("ownerLook");
        let mut older = DaemonState::new();
        older
            .restore_control_plane_snapshot(serde_json::from_value(payload).unwrap())
            .expect("a snapshot from before the look loads");
        assert_eq!(older.owner_look, None);
    }
}
