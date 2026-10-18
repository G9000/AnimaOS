//! Keys the owner hands to the coding apps an agent starts (spec: coding
//! agents P5). Off by default; a fixed set of names; the values come from
//! the daemon's own environment, the same place model providers read them,
//! and never pass through the model.
//!
//! The owner's choices are saved with the approval policies in the control
//! plane, which agents cannot write. This module mirrors them for the tools,
//! which only read.
use std::collections::{BTreeSet, HashMap};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// The names an owner may pass through, in display order.
pub(crate) const CODING_KEYS: &[(&str, &str)] = &[
    ("ANTHROPIC_API_KEY", "Anthropic API key"),
    ("OPENAI_API_KEY", "OpenAI API key"),
    ("GEMINI_API_KEY", "Gemini API key"),
    ("CLAUDE_CODE_OAUTH_TOKEN", "Claude Code sign-in token"),
    ("OPENROUTER_API_KEY", "OpenRouter API key"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentCodingKeys {
    pub(crate) agent_id: String,
    pub(crate) enabled: Vec<String>,
}

fn mirror() -> &'static RwLock<HashMap<String, BTreeSet<String>>> {
    static MIRROR: std::sync::OnceLock<RwLock<HashMap<String, BTreeSet<String>>>> =
        std::sync::OnceLock::new();
    MIRROR.get_or_init(Default::default)
}

pub(crate) fn is_known(name: &str) -> bool {
    CODING_KEYS.iter().any(|(known, _)| *known == name)
}

/// Whether the daemon has a value for `name` to hand over.
pub(crate) fn is_saved(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

/// Replaces what the tools see, after a load or a saved change.
pub(crate) fn sync(records: &[AgentCodingKeys]) {
    let mut mirror = mirror().write().unwrap_or_else(|p| p.into_inner());
    mirror.clear();
    for record in records {
        let enabled: BTreeSet<String> = record
            .enabled
            .iter()
            .filter(|name| is_known(name))
            .cloned()
            .collect();
        if !enabled.is_empty() {
            mirror.insert(record.agent_id.clone(), enabled);
        }
    }
}

/// What the control plane saves, for agents that still exist.
pub(crate) fn records(live_agents: &std::collections::HashSet<String>) -> Vec<AgentCodingKeys> {
    let mirror = mirror().read().unwrap_or_else(|p| p.into_inner());
    let mut records: Vec<AgentCodingKeys> = mirror
        .iter()
        .filter(|(agent_id, _)| live_agents.contains(*agent_id))
        .map(|(agent_id, enabled)| AgentCodingKeys {
            agent_id: agent_id.clone(),
            enabled: enabled.iter().cloned().collect(),
        })
        .collect();
    records.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    records
}

/// Replaces one agent's choice; returns the one it replaced, for a revert.
pub(crate) fn set(agent_id: &str, enabled: BTreeSet<String>) -> BTreeSet<String> {
    let mut mirror = mirror().write().unwrap_or_else(|p| p.into_inner());
    let previous = if enabled.is_empty() {
        mirror.remove(agent_id)
    } else {
        mirror.insert(agent_id.to_string(), enabled)
    };
    previous.unwrap_or_default()
}

pub(crate) fn enabled(agent_id: &str) -> BTreeSet<String> {
    mirror()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .get(agent_id)
        .cloned()
        .unwrap_or_default()
}

/// The variables a coding app started by `agent_id` receives on top of the
/// shell allowlist: enabled names the daemon has a value for.
pub(crate) fn child_env(agent_id: &str) -> Vec<(String, String)> {
    enabled(agent_id)
        .into_iter()
        .filter_map(|name| {
            let value = std::env::var(&name).ok()?;
            (!value.trim().is_empty()).then_some((name, value))
        })
        .collect()
}

/// Checks a requested set: every name known and saved.
pub(crate) fn validate(names: &[String]) -> Result<BTreeSet<String>, String> {
    let mut set = BTreeSet::new();
    for name in names {
        if !is_known(name) {
            return Err(format!("{name} is not a key coding apps can be given"));
        }
        if !is_saved(name) {
            return Err(format!(
                "{name} is not saved on this computer, so it can't be turned on"
            ));
        }
        set.insert(name.clone());
    }
    Ok(set)
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingKeyView {
    pub(crate) name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) saved: bool,
    pub(crate) enabled: bool,
}

pub(crate) fn view(agent_id: &str) -> Vec<CodingKeyView> {
    let enabled = enabled(agent_id);
    CODING_KEYS
        .iter()
        .map(|(name, label)| CodingKeyView {
            name,
            label,
            saved: is_saved(name),
            enabled: enabled.contains(*name),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_saved_known_keys_reach_a_child() {
        std::env::set_var("OPENROUTER_API_KEY", "sk-or-test");
        std::env::remove_var("GEMINI_API_KEY");
        sync(&[AgentCodingKeys {
            agent_id: "agent-coding-keys-test".into(),
            enabled: vec![
                "OPENROUTER_API_KEY".into(),
                "GEMINI_API_KEY".into(),
                "DATABASE_URL".into(),
            ],
        }]);
        assert_eq!(
            child_env("agent-coding-keys-test"),
            vec![("OPENROUTER_API_KEY".to_string(), "sk-or-test".to_string())]
        );
        assert!(child_env("another-agent").is_empty());
        assert!(validate(&["DATABASE_URL".into()]).is_err());
        assert!(validate(&["GEMINI_API_KEY".into()]).is_err());
        assert!(validate(&["OPENROUTER_API_KEY".into()]).is_ok());
    }
}
