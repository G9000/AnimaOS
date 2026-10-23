//! Frozen owner relevance declarations, never model-supplied coverage or grants.
use anima_core::{AgentConfig, DataValue};
use std::collections::BTreeSet;

pub(super) struct Contract {
    pub(super) id: String,
    pub(super) command: String,
    pub(super) cwd: String,
    files: Vec<String>,
    directories: Vec<String>,
}

impl Contract {
    pub(super) fn covers(&self, path: &str) -> bool {
        self.files.iter().any(|file| file == path)
            || self.directories.iter().any(|directory| {
                directory == "."
                    || path
                        .strip_prefix(directory)
                        .is_some_and(|tail| tail.starts_with('/'))
            })
    }
}

pub(super) fn from_config(config: &AgentConfig) -> Vec<Contract> {
    parse(config).unwrap_or_default()
}

fn parse(config: &AgentConfig) -> Option<Vec<Contract>> {
    let value = config
        .settings
        .as_ref()?
        .additional
        .get("completionVerificationContracts")?;
    let DataValue::Array(items) = value else {
        return None;
    };
    if items.len() > 32 {
        return None;
    }
    let mut ids = BTreeSet::new();
    let mut coverage_count = 0;
    let mut bytes = 0;
    let mut contracts = Vec::new();
    for item in items {
        let DataValue::Object(fields) = item else {
            return None;
        };
        if fields.len() != 5
            || fields.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "id" | "command" | "cwd" | "files" | "directories"
                )
            })
        {
            return None;
        }
        let text = |key: &str| match fields.get(key) {
            Some(DataValue::String(value)) => Some(value.as_str()),
            _ => None,
        };
        let id = text("id")?;
        let command = text("command")?;
        if id.is_empty()
            || id.len() > 80
            || id.chars().any(char::is_control)
            || !ids.insert(id.to_string())
            || command.is_empty()
            || command.len() > 4096
            || command.trim() != command
            || command.chars().any(char::is_control)
        {
            return None;
        }
        let cwd = relative(text("cwd")?, true)?;
        let mut paths = |key: &str, root_allowed| -> Option<Vec<String>> {
            let Some(DataValue::Array(values)) = fields.get(key) else {
                return None;
            };
            if values.len() > 64 {
                return None;
            }
            let mut result = Vec::new();
            for value in values {
                let DataValue::String(value) = value else {
                    return None;
                };
                let path = relative(value, root_allowed)?;
                coverage_count += 1;
                bytes += path.len();
                if coverage_count > 256 || bytes > 128 * 1024 {
                    return None;
                }
                result.push(path);
            }
            Some(result)
        };
        let files = paths("files", false)?;
        let directories = paths("directories", true)?;
        if files.is_empty() && directories.is_empty() {
            return None;
        }
        bytes += id.len() + command.len() + cwd.len();
        if bytes > 128 * 1024 {
            return None;
        }
        contracts.push(Contract {
            id: id.into(),
            command: command.into(),
            cwd,
            files,
            directories,
        });
    }
    Some(contracts)
}

fn relative(path: &str, root_allowed: bool) -> Option<String> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return None;
    }
    let path = path.replace('\\', "/");
    if root_allowed && path == "." {
        return Some(path);
    }
    if path.split('/').any(|part| {
        part.is_empty()
            || part == "."
            || part == ".."
            || part.contains([':', '*', '?', '[', ']', '{', '}'])
    }) {
        return None;
    }
    #[cfg(windows)]
    let path = path.to_ascii_lowercase();
    Some(path)
}
