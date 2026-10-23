//! Deterministic candidate paths; no cwd, home, recursive glob, or attachment lookup.
use super::{AgentFileScope, MAX_DIRECTORY_DEPTH};

pub(super) fn chain(relative: &str) -> Option<Vec<String>> {
    if relative.is_empty() {
        return Some(vec![String::new()]);
    }
    let parts: Vec<_> = relative.split('/').collect();
    if parts.len() > MAX_DIRECTORY_DEPTH
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || part.contains(['\\', ':', '`', '[', ']'])
                || part.chars().any(char::is_control)
        })
        || parts[0].eq_ignore_ascii_case("uploads")
    {
        return None;
    }
    let mut chain = vec![String::new()];
    for end in 1..=parts.len() {
        chain.push(parts[..end].join("/"));
    }
    Some(chain)
}
pub(super) fn path(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}
pub(crate) fn profile_relative(agent_id: &str) -> String {
    let hex: String = agent_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(".animaos-swarm/agent-profiles/{hex}")
}
pub(crate) fn soul(scope: &AgentFileScope) -> Option<String> {
    if scope.profile_enabled {
        Some(path(&profile_relative(&scope.agent_id), "SOUL.md"))
    } else if scope.companion {
        Some("SOUL.md".into())
    } else {
        None
    }
}
pub(super) fn strip_frontmatter(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix("---\n") {
        let mut consumed = 0;
        for line in rest.split_inclusive('\n') {
            consumed += line.len();
            if line.trim_end_matches('\n') == "---" {
                return &rest[consumed..];
            }
        }
    }
    text
}
