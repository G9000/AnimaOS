//! The workspace steward's rule-based fallback.
//!
//! The steward is the one system agent every workspace has. It uses a
//! language model when one is connected; without one it still answers here,
//! the way assistants did before language models: a fixed set of phrasings is
//! matched to a reply or to one host tool call, and anything else gets an
//! honest generic answer. Nothing in this crate performs an action itself:
//! the host runs the tool calls through its normal permission path.

use serde_json::{Map, Value};

/// Roster tool the host exposes to the steward.
pub const LIST_AGENTS_TOOL: &str = "list_workspace_agents";
/// Owner control that turns an agent's talk-with-agents grant on or off.
pub const SET_TALK_TOOL: &str = "set_agent_talk";

/// What the steward understood from one owner message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Greeting,
    Help,
    Thanks,
    Yes,
    No,
    ListAgents,
    SetTalk {
        agent: String,
        enabled: bool,
    },
    /// A question it cannot answer without a model.
    Question,
    Unknown,
}

/// The steward's next step for one owner message.
#[derive(Clone, Debug, PartialEq)]
pub enum StewardTurn {
    Reply(String),
    Tool {
        name: &'static str,
        args: Map<String, Value>,
    },
}

const HELP: &str = "I'm the workspace steward. Without a language model I can only do a few fixed things:\n- \"list agents\" shows who is in this workspace\n- \"let Ada talk with other agents\" or \"make Ada work alone\" changes who may talk\n- \"help\" shows this list\nConnect a model in Settings and I can do much more.";

/// Classifies one owner message.
pub fn classify(input: &str) -> Intent {
    let text = normalize(input);
    if text.is_empty() {
        return Intent::Unknown;
    }
    if let Some(intent) = talk_intent(input, &text) {
        return intent;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let only = |options: &[&str]| words.len() <= 3 && options.iter().any(|o| text == *o);
    if only(&[
        "yes",
        "y",
        "yeah",
        "yep",
        "sure",
        "ok",
        "okay",
        "do it",
        "go ahead",
        "please do",
    ]) {
        return Intent::Yes;
    }
    if only(&[
        "no",
        "n",
        "nope",
        "nah",
        "cancel",
        "never mind",
        "dont",
        "do not",
    ]) {
        return Intent::No;
    }
    if words
        .iter()
        .any(|w| matches!(*w, "thanks" | "thank" | "thx" | "ty"))
        && words.len() <= 4
    {
        return Intent::Thanks;
    }
    if words.len() <= 4
        && words.first().is_some_and(|w| {
            matches!(
                *w,
                "hi" | "hello" | "hey" | "hiya" | "morning" | "evening" | "good"
            )
        })
    {
        return Intent::Greeting;
    }
    if text == "help"
        || text == "?"
        || text.contains("what can you do")
        || text.contains("how do i use you")
        || text.starts_with("help ")
    {
        return Intent::Help;
    }
    let mentions_agents = words.iter().any(|w| {
        matches!(
            *w,
            "agents" | "agent" | "roster" | "team" | "everyone" | "who"
        )
    });
    let asks_to_show = text.starts_with("list")
        || text.starts_with("show")
        || text.starts_with("who")
        || text.starts_with("which agents")
        || text == "agents"
        || text == "roster";
    if mentions_agents && asks_to_show {
        return Intent::ListAgents;
    }
    if input.trim_end().ends_with('?')
        || words.first().is_some_and(|w| {
            matches!(
                *w,
                "what"
                    | "why"
                    | "how"
                    | "when"
                    | "where"
                    | "can"
                    | "could"
                    | "should"
                    | "is"
                    | "are"
                    | "do"
                    | "does"
                    | "will"
            )
        })
    {
        return Intent::Question;
    }
    Intent::Unknown
}

/// The steward's answer to one owner message, with no tool results yet.
pub fn respond(input: &str) -> StewardTurn {
    match classify(input) {
        Intent::Greeting => StewardTurn::Reply(
            "Hello. I'm the workspace steward. Say \"help\" to see what I can do right now.".into(),
        ),
        Intent::Help | Intent::Unknown => StewardTurn::Reply(HELP.into()),
        Intent::Thanks => StewardTurn::Reply("You're welcome.".into()),
        Intent::Yes => StewardTurn::Reply("Okay.".into()),
        Intent::No => StewardTurn::Reply("Alright, nothing changed.".into()),
        Intent::Question => StewardTurn::Reply(format!(
            "I can't answer that without a language model, so I won't guess. {}",
            "Connect a model in Settings, or say \"help\" for what I can do without one."
        )),
        Intent::ListAgents => StewardTurn::Tool {
            name: LIST_AGENTS_TOOL,
            args: Map::new(),
        },
        Intent::SetTalk { agent, enabled } => {
            let mut args = Map::new();
            args.insert("agent".into(), Value::String(agent));
            args.insert("enabled".into(), Value::Bool(enabled));
            StewardTurn::Tool {
                name: SET_TALK_TOOL,
                args,
            }
        }
    }
}

/// Plain-language summary of a tool result the host returned.
///
/// `result` is the tool message text: either the tool's own JSON output or a
/// task-result envelope (`{"status": ..., "data": ..., "error": ...}`).
pub fn summarize(tool: &str, result: &str) -> String {
    let parsed: Value = serde_json::from_str(result).unwrap_or(Value::Null);
    if parsed.get("status").and_then(Value::as_str) == Some("error") {
        let error = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("it was refused");
        return format!("I couldn't do that: {error}");
    }
    let data = match parsed.get("data") {
        Some(Value::String(inner)) => serde_json::from_str(inner).unwrap_or(Value::Null),
        Some(other) if parsed.get("status").is_some() => other.clone(),
        _ => parsed,
    };
    match tool {
        LIST_AGENTS_TOOL => summarize_roster(&data),
        SET_TALK_TOOL => summarize_talk(&data),
        _ => "Done.".into(),
    }
}

fn summarize_roster(data: &Value) -> String {
    let agents: Vec<String> = data
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|agent| agent.get("role").and_then(Value::as_str) != Some("helper"))
        .filter_map(|agent| {
            let name = agent.get("name")?.as_str()?;
            let status = agent
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            Some(format!("{name} ({})", status.to_lowercase()))
        })
        .collect();
    match agents.len() {
        0 => "There are no agents in this workspace yet.".into(),
        1 => format!("There is one agent: {}.", agents[0]),
        n => format!("There are {n} agents: {}.", agents.join(", ")),
    }
}

fn summarize_talk(data: &Value) -> String {
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("That agent");
    let talks = data.get("talksWithAgents").and_then(Value::as_bool);
    let changed = data.get("changed").and_then(Value::as_bool).unwrap_or(true);
    match (talks, changed) {
        (Some(true), true) => {
            format!("Done. {name} can now talk with other agents that also allow it.")
        }
        (Some(true), false) => format!("{name} could already talk with other agents."),
        (Some(false), true) => format!("Done. {name} now works alone."),
        (Some(false), false) => format!("{name} already works alone."),
        (None, _) => "Done.".into(),
    }
}

/// Lowercase words with punctuation stripped, single-spaced.
fn normalize(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '?' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// "let Ada talk with other agents", "make Ada work alone", "turn off
/// talking for Ada", "allow me to talk to agents" (the steward itself).
fn talk_intent(input: &str, text: &str) -> Option<Intent> {
    let talks = text.contains("talk") || text.contains("message") || text.contains("chat with");
    let alone = text.contains("alone")
        || text.contains("solo")
        || text.contains("by itself")
        || text.contains("by themselves");
    if !talks && !alone {
        return None;
    }
    let disable = alone
        || [
            "stop ",
            "disable",
            "turn off",
            "switch off",
            "dont let",
            "do not let",
            "block ",
            "no longer",
            "cannot",
            "cant ",
            "not talk",
        ]
        .iter()
        .any(|phrase| text.contains(phrase));
    let enable = !disable
        && [
            "let ",
            "allow",
            "enable",
            "turn on",
            "switch on",
            "can talk",
            "may talk",
            "able to talk",
            "open up",
        ]
        .iter()
        .any(|phrase| text.contains(phrase));
    if !enable && !disable {
        return None;
    }
    let agent = agent_name(input)?;
    Some(Intent::SetTalk {
        agent,
        enabled: enable,
    })
}

/// The agent a talk command names, keeping its original spelling.
fn agent_name(input: &str) -> Option<String> {
    const LEADS: &[&str] = &[
        "talking for ",
        "talk for ",
        "messaging for ",
        "for ",
        "don't let ",
        "dont let ",
        "do not let ",
        "let ",
        "allow ",
        "enable ",
        "disable ",
        "stop ",
        "block ",
        "make ",
        "keep ",
        "set ",
    ];
    const ENDS: &[&str] = &[
        " talk", " to talk", " message", " chat", " work", " be ", " stay", " from", " can",
        " may", " is ", " so ", " and", " with", " to ", ",", ".", "!", "?",
    ];
    let lower = input.to_lowercase();
    let start = LEADS
        .iter()
        .filter_map(|lead| lower.find(lead).map(|at| at + lead.len()))
        .min()?;
    let rest = &input[start..];
    let rest_lower = &lower[start..];
    let end = ENDS
        .iter()
        .filter_map(|end| rest_lower.find(end))
        .min()
        .unwrap_or(rest.len());
    let name = rest[..end]
        .trim()
        .trim_start_matches("the ")
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != ' ')
        .trim();
    let name = match name.to_lowercase().as_str() {
        "" => return None,
        "me" | "yourself" | "you" | "steward" => "self".to_string(),
        "agents" | "all agents" | "everyone" | "talking" | "talk" => return None,
        _ => name.to_string(),
    };
    Some(name)
}

/// The steward's whole turn, given its conversation so far: the last owner
/// message and any tool results that came after it.
pub fn next_turn(last_owner_message: &str, tool_results: &[(String, String)]) -> StewardTurn {
    match tool_results.last() {
        Some((tool, result)) => StewardTurn::Reply(summarize(tool, result)),
        None => respond(last_owner_message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn talk(agent: &str, enabled: bool) -> Intent {
        Intent::SetTalk {
            agent: agent.into(),
            enabled,
        }
    }

    #[test]
    fn understands_talk_commands_and_keeps_the_name() {
        assert_eq!(
            classify("Let Ada talk with other agents"),
            talk("Ada", true)
        );
        assert_eq!(
            classify("allow Big Bo to talk to agents"),
            talk("Big Bo", true)
        );
        assert_eq!(classify("turn on talking for Nova."), talk("Nova", true));
        assert_eq!(classify("make Ada work alone"), talk("Ada", false));
        assert_eq!(
            classify("stop Ada talking to the others"),
            talk("Ada", false)
        );
        assert_eq!(classify("turn off talking for Nova"), talk("Nova", false));
        assert_eq!(
            classify("let yourself talk with agents"),
            talk("self", true)
        );
    }

    #[test]
    fn talk_without_a_name_falls_through() {
        assert!(!matches!(
            classify("let agents talk"),
            Intent::SetTalk { .. }
        ));
        assert!(!matches!(
            classify("can agents talk?"),
            Intent::SetTalk { .. }
        ));
    }

    #[test]
    fn simple_intents() {
        assert_eq!(classify("yes"), Intent::Yes);
        assert_eq!(classify("Nope."), Intent::No);
        assert_eq!(classify("thanks!"), Intent::Thanks);
        assert_eq!(classify("hello there"), Intent::Greeting);
        assert_eq!(classify("help"), Intent::Help);
        assert_eq!(classify("list agents"), Intent::ListAgents);
        assert_eq!(classify("who is on the team"), Intent::ListAgents);
        assert_eq!(classify("what's the capital of France?"), Intent::Question);
        assert_eq!(classify("banana"), Intent::Unknown);
    }

    #[test]
    fn commands_become_tool_calls_and_never_guess_answers() {
        match respond("list agents") {
            StewardTurn::Tool { name, args } => {
                assert_eq!(name, LIST_AGENTS_TOOL);
                assert!(args.is_empty());
            }
            other => panic!("{other:?}"),
        }
        match respond("let Ada talk with agents") {
            StewardTurn::Tool { name, args } => {
                assert_eq!(name, SET_TALK_TOOL);
                assert_eq!(args["agent"], "Ada");
                assert_eq!(args["enabled"], true);
            }
            other => panic!("{other:?}"),
        }
        let StewardTurn::Reply(text) = respond("why is the sky blue?") else {
            panic!("a question is answered in words")
        };
        assert!(text.contains("without a language model"));
    }

    #[test]
    fn summarizes_results_and_errors() {
        let roster = r#"{"totalAgents":3,"agents":[{"name":"Ada","role":"agent","status":"Idle"},{"name":"h","role":"helper","status":"Idle"},{"name":"Bo","role":"agent","status":"Running"}]}"#;
        assert_eq!(
            summarize(LIST_AGENTS_TOOL, roster),
            "There are 2 agents: Ada (idle), Bo (running)."
        );
        let wrapped = json!({"status": "success", "data": roster}).to_string();
        assert_eq!(
            summarize(LIST_AGENTS_TOOL, &wrapped),
            summarize(LIST_AGENTS_TOOL, roster)
        );
        assert_eq!(
            summarize(
                SET_TALK_TOOL,
                r#"{"name":"Ada","talksWithAgents":false,"changed":true}"#
            ),
            "Done. Ada now works alone."
        );
        assert_eq!(
            summarize(
                SET_TALK_TOOL,
                r#"{"status":"error","error":"No agent has that name or ID"}"#
            ),
            "I couldn't do that: No agent has that name or ID"
        );
        assert_eq!(
            next_turn(
                "list agents",
                &[(
                    SET_TALK_TOOL.into(),
                    r#"{"name":"Bo","talksWithAgents":true,"changed":false}"#.into()
                )]
            ),
            StewardTurn::Reply("Bo could already talk with other agents.".into())
        );
    }
}
