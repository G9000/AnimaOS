//! The coding apps this computer has, for the Coder template: which are on
//! PATH, whether they start, and the know-how bundle that teaches an agent
//! to drive each one. Installing a bundle still goes through the owner's
//! community-bundle review.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

const VERSION_TIMEOUT: Duration = Duration::from_secs(8);

struct App {
    id: &'static str,
    name: &'static str,
    program: &'static str,
    bundle_id: &'static str,
    manifest: &'static str,
}

const APPS: &[App] = &[
    App {
        id: "claude-code",
        name: "Claude Code",
        program: "claude",
        bundle_id: "anima.coding.claude-code",
        manifest: include_str!("../../../bundles/coding-agents/claude-code/manifest.json"),
    },
    App {
        id: "codex",
        name: "Codex",
        program: "codex",
        bundle_id: "anima.coding.codex",
        manifest: include_str!("../../../bundles/coding-agents/codex/manifest.json"),
    },
    App {
        id: "opencode",
        name: "OpenCode",
        program: "opencode",
        bundle_id: "anima.coding.opencode",
        manifest: include_str!("../../../bundles/coding-agents/opencode/manifest.json"),
    },
    App {
        id: "gemini-cli",
        name: "Gemini CLI",
        program: "gemini",
        bundle_id: "anima.coding.gemini-cli",
        manifest: include_str!("../../../bundles/coding-agents/gemini-cli/manifest.json"),
    },
];

const HANDOFF_BUNDLE_ID: &str = "anima.coding.delegate";
const HANDOFF_MANIFEST: &str =
    include_str!("../../../bundles/coding-agents/delegate/manifest.json");

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingAppView {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) found: bool,
    pub(crate) version: Option<String>,
    pub(crate) problem: Option<String>,
    pub(crate) bundle_id: &'static str,
    pub(crate) manifest_text: &'static str,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffBundleView {
    pub(crate) bundle_id: &'static str,
    pub(crate) manifest_text: &'static str,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingAppsEnvelope {
    pub(crate) apps: Vec<CodingAppView>,
    pub(crate) handoff: HandoffBundleView,
}

/// Looks for every app; blocking, so callers run it off the async runtime.
pub(crate) fn detect() -> CodingAppsEnvelope {
    let path = std::env::var_os("PATH");
    CodingAppsEnvelope {
        apps: APPS
            .iter()
            .map(|app| {
                let (found, version, problem) = match find_on_path(app.program, path.as_deref()) {
                    None => (false, None, None),
                    Some(program) => match version_of(&program) {
                        Ok(version) => (true, version, None),
                        Err(problem) => (true, None, Some(problem)),
                    },
                };
                CodingAppView {
                    id: app.id,
                    name: app.name,
                    found,
                    version,
                    problem,
                    bundle_id: app.bundle_id,
                    manifest_text: app.manifest,
                }
            })
            .collect(),
        handoff: HandoffBundleView {
            bundle_id: HANDOFF_BUNDLE_ID,
            manifest_text: HANDOFF_MANIFEST,
        },
    }
}

fn find_on_path(program: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(path?).find_map(|dir| {
        extensions.iter().find_map(|extension| {
            let candidate = dir.join(format!("{program}{extension}"));
            candidate.is_file().then_some(candidate)
        })
    })
}

/// The app's version, or why it would not start.
fn version_of(program: &std::path::Path) -> Result<Option<String>, String> {
    let mut child = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "installed but won't start".to_string())?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < VERSION_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("didn't answer --version in time".to_string());
            }
        }
    };
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        let _ = stdout.by_ref().take(4096).read_to_string(&mut output);
    }
    if !status.success() {
        return Err("installed but won't start".to_string());
    }
    Ok(parse_version(&output))
}

/// The first dotted number, e.g. "2.1.286 (Claude Code)" → "2.1.286".
fn parse_version(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|word| word.trim_matches('.'))
        .find(|word| word.contains('.') && word.split('.').all(|part| !part.is_empty()))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_the_first_dotted_number() {
        assert_eq!(
            parse_version("2.1.286 (Claude Code)"),
            Some("2.1.286".into())
        );
        assert_eq!(parse_version("codex-cli 0.50.0\n"), Some("0.50.0".into()));
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn every_bundle_manifest_names_its_bundle() {
        for app in APPS {
            assert!(app.manifest.contains(app.bundle_id), "{}", app.id);
        }
        assert!(HANDOFF_MANIFEST.contains(HANDOFF_BUNDLE_ID));
    }

    #[cfg(unix)]
    #[test]
    fn an_app_that_fails_to_start_is_found_with_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("codex");
        std::fs::write(&broken, "#!/bin/sh\nexit 1\n").unwrap();
        let mut permissions = std::fs::metadata(&broken).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&broken, permissions).unwrap();
        let found = find_on_path("codex", Some(dir.path().as_os_str())).unwrap();
        assert_eq!(
            version_of(&found),
            Err("installed but won't start".to_string())
        );
        assert!(find_on_path("claude", Some(dir.path().as_os_str())).is_none());
    }
}
