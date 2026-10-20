//! A hardened, bounded `git` CLI runner.
//!
//! Every call runs unattended against a repository the host does not control,
//! so a hostile `.git/config` must not execute anything: hooks, fsmonitor,
//! pagers, editors, credential helpers, external diff drivers and
//! repository-named clean/smudge/process filters are all pinned off with
//! command-line `-c` values, which outrank every configuration file. Global
//! and system configuration are not read at all, stdin is closed, and each
//! call is killed when it outlives its timeout.
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::WorktreeError;

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

/// Bytes kept from one stream of one git call; the rest is drained unread.
pub(crate) const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

/// Windows canonical paths use the extended namespace, which Git for Windows
/// cannot persist in a linked worktree's metadata. Convert only complete path
/// arguments at the CLI boundary; internal canonical paths remain unchanged.
fn cli_argument(argument: &OsStr) -> OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use std::path::{Component, Prefix};

        match Path::new(argument).components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => {
                    return OsString::from_wide(
                        &argument.encode_wide().skip(4).collect::<Vec<_>>(),
                    );
                }
                Prefix::VerbatimUNC(_, _) => {
                    let mut native = OsString::from(r"\\");
                    native.push(OsString::from_wide(
                        &argument.encode_wide().skip(8).collect::<Vec<_>>(),
                    ));
                    return native;
                }
                _ => {}
            },
            _ => {}
        }
    }
    argument.to_os_string()
}

/// Fixed configuration pins applied to every call.
fn pinned_config() -> Vec<String> {
    [
        ("core.hooksPath", NULL_DEVICE),
        ("core.fsmonitor", "false"),
        ("core.untrackedCache", "false"),
        ("core.pager", "cat"),
        ("core.editor", "true"),
        ("core.askPass", ""),
        ("core.sshCommand", "ssh -o BatchMode=yes"),
        ("sequence.editor", "true"),
        ("credential.helper", ""),
        ("diff.external", ""),
        ("protocol.allow", "never"),
        ("protocol.file.allow", "never"),
        ("submodule.recurse", "false"),
        ("gc.auto", "0"),
        ("maintenance.auto", "false"),
        ("advice.detachedHead", "false"),
    ]
    .iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect()
}

/// Environment variables that could redirect git at another repository,
/// another config, another executable, or inject configuration.
const REMOVED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_EXEC_PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_TEMPLATE_DIR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_PROXY_COMMAND",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
];

pub(crate) struct GitOutput {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: String,
    pub(crate) truncated: bool,
}

impl GitOutput {
    pub(crate) fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Git {
    pub(crate) timeout: Duration,
}

impl Git {
    fn command(&self, cwd: &Path, extra_config: &[String], args: &[&OsStr]) -> Command {
        let mut command = Command::new("git");
        command.current_dir(cwd);
        for (key, _) in std::env::vars_os() {
            let removable = key.to_str().is_some_and(|key| {
                key.starts_with("GIT_CONFIG_KEY_") || key.starts_with("GIT_CONFIG_VALUE_")
            });
            if removable {
                command.env_remove(&key);
            }
        }
        for key in REMOVED_ENV {
            command.env_remove(key);
        }
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "Never")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
            .env("GIT_CONFIG_SYSTEM", NULL_DEVICE)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("GIT_EDITOR", "true")
            .env("GIT_ADVICE", "0")
            .env("LC_ALL", "C")
            .env("LANG", "C");
        command.arg("--no-pager");
        for pair in pinned_config().iter().chain(extra_config) {
            command.arg("-c").arg(pair);
        }
        command.args(args.iter().map(|argument| cli_argument(argument)));
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Runs one git call; a non-zero exit is reported in `success`, never as
    /// an error. Spawn failures and timeouts are errors.
    pub(crate) fn run(
        &self,
        cwd: &Path,
        extra_config: &[String],
        args: &[&OsStr],
    ) -> Result<GitOutput, WorktreeError> {
        let label = describe(args);
        let mut child = self
            .command(cwd, extra_config, args)
            .spawn()
            .map_err(|error| WorktreeError::GitUnavailable(error.to_string()))?;
        let deadline = Instant::now() + self.timeout;
        let stdout = spawn_reader(child.stdout.take());
        let stderr = spawn_reader(child.stderr.take());
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    kill(&mut child);
                    return Err(WorktreeError::Timeout(label));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(error) => {
                    kill(&mut child);
                    return Err(WorktreeError::Io(error.to_string()));
                }
            }
        };
        // A grandchild holding a pipe open must not hang the caller.
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(250));
        let (stdout, out_truncated) = stdout
            .recv_timeout(remaining)
            .map_err(|_| WorktreeError::Timeout(label.clone()))?;
        let (stderr, _) = stderr
            .recv_timeout(Duration::from_millis(250))
            .unwrap_or_default();
        Ok(GitOutput {
            success: status.success(),
            code: status.code(),
            stdout,
            stderr: bounded(String::from_utf8_lossy(&stderr).trim(), 1024),
            truncated: out_truncated,
        })
    }

    /// Runs a call that must succeed, returning its trimmed stdout.
    pub(crate) fn text(
        &self,
        cwd: &Path,
        extra_config: &[String],
        args: &[&str],
    ) -> Result<String, WorktreeError> {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let output = self.run(cwd, extra_config, &args)?;
        if !output.success {
            return Err(WorktreeError::Git {
                command: describe(&args),
                message: output.stderr,
            });
        }
        Ok(output.stdout_text())
    }

    /// `-c` overrides that empty every clean/smudge/process filter the
    /// repository names. Filter driver names are chosen by the repository
    /// (via `.gitattributes`), so they are discovered rather than pinned.
    /// Discovery fails closed.
    pub(crate) fn filter_overrides(&self, cwd: &Path) -> Result<Vec<String>, WorktreeError> {
        let args = [
            "config",
            "--includes",
            "-z",
            "--name-only",
            "--get-regexp",
            r"^filter\.",
        ];
        let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let output = self.run(cwd, &[], &os_args)?;
        if output.truncated {
            return Err(WorktreeError::FilterDiscovery);
        }
        // `git config --get-regexp` exits 1 when nothing matches.
        if !matches!(output.code, Some(0 | 1)) {
            return Err(WorktreeError::FilterDiscovery);
        }
        let mut names: Vec<String> = Vec::new();
        for key in String::from_utf8_lossy(&output.stdout)
            .split(['\0', '\n'])
            .filter(|key| !key.is_empty())
        {
            let Some(rest) = key.strip_prefix("filter.") else {
                return Err(WorktreeError::FilterDiscovery);
            };
            let Some((name, _field)) = rest.rsplit_once('.') else {
                continue;
            };
            if name.is_empty() || name.contains('=') || name.contains('\0') {
                return Err(WorktreeError::FilterDiscovery);
            }
            if !names.iter().any(|known| known == name) {
                names.push(name.to_string());
            }
        }
        Ok(names
            .iter()
            .flat_map(|name| {
                [
                    format!("filter.{name}.clean="),
                    format!("filter.{name}.smudge="),
                    format!("filter.{name}.process="),
                    format!("filter.{name}.required=false"),
                ]
            })
            .collect())
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_reader<R: Read + Send + 'static>(source: Option<R>) -> mpsc::Receiver<(Vec<u8>, bool)> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        if let Some(mut source) = source {
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match source.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let room = MAX_CAPTURE_BYTES.saturating_sub(kept.len());
                        if read > room {
                            truncated = true;
                        }
                        kept.extend_from_slice(&buffer[..read.min(room)]);
                    }
                }
            }
        }
        let _ = sender.send((kept, truncated));
    });
    receiver
}

fn describe(args: &[&OsStr]) -> String {
    let mut words: Vec<OsString> = vec!["git".into()];
    words.extend(args.iter().take(3).map(|arg| arg.to_os_string()));
    words
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cuts `text` to at most `max` bytes on a character boundary.
pub(crate) fn bounded(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(all(test, windows))]
mod windows_path_tests {
    use super::*;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    fn arguments(args: &[&OsStr]) -> Vec<OsString> {
        Git {
            timeout: Duration::from_secs(1),
        }
        .command(Path::new("C:\\synthetic-repo"), &[], args)
        .get_args()
        .map(OsStr::to_os_string)
        .collect()
    }

    #[test]
    fn windows_git_argv_converts_verbatim_drive_paths_without_changing_internal_paths() {
        let internal = Path::new(r"\\?\C:\synthetic repo\資料\.worktrees\helper");
        let argv = arguments(&[
            OsStr::new("worktree"),
            OsStr::new("add"),
            internal.as_os_str(),
        ]);
        assert_eq!(
            argv.last().unwrap(),
            OsStr::new(r"C:\synthetic repo\資料\.worktrees\helper")
        );
        assert_eq!(
            internal,
            Path::new(r"\\?\C:\synthetic repo\資料\.worktrees\helper")
        );
    }

    #[test]
    fn windows_git_argv_preserves_unc_authority_and_non_unicode_code_units() {
        for (raw, expected) in [
            (
                r"\\?\UNC\server\share\synthetic repo\",
                r"\\server\share\synthetic repo\",
            ),
            (r"\\?\D:\synthetic repo\", r"D:\synthetic repo\"),
        ] {
            let mut raw: Vec<u16> = OsStr::new(raw).encode_wide().collect();
            raw.extend([0xD800, u16::from(b'x')]);
            let mut expected: Vec<u16> = OsStr::new(expected).encode_wide().collect();
            expected.extend([0xD800, u16::from(b'x')]);
            let raw = OsString::from_wide(&raw);
            let argv = arguments(&[raw.as_os_str()]);
            assert_eq!(
                argv.last().unwrap().encode_wide().collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn windows_git_argv_keeps_options_normal_paths_and_other_namespaces_unchanged() {
        let args: Vec<&OsStr> = [
            "--quiet",
            "--",
            r"relative\file",
            r"C:\ordinary repo\file",
            r"\\server\share\file",
            r"--output=\\?\C:\file",
            r"\\.\pipe\synthetic",
            r"\\?\Volume{synthetic}\file",
        ]
        .iter()
        .map(OsStr::new)
        .collect();
        let argv = arguments(&args);
        assert_eq!(&argv[argv.len() - args.len()..], args.as_slice());
        assert!(argv
            .windows(2)
            .any(|pair| pair == [OsStr::new("-c"), OsStr::new("core.hooksPath=NUL")]));
    }
}
