//! Bind approval to executable resolution, bytes and identifiable entry scripts.
//!
//! This covers entrypoints, not their imported modules, libraries or subprocesses.
//! Resolution is read-only; no shell is invoked to discover a command. Unsupported
//! shell state changes, dynamic words and loader forms fail closed. Revalidation
//! belongs after awaited approval/receipt checks AND inside the final spawn closure.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::approvals::shell_lex;

const CHANGED: &str = "Execution identity changed while approval was pending; request approval again; the command did not run";
const UNSUPPORTED: &str = "Cannot bind this command safely to approval; use a static command with an explicit executable and entry script";
const MAX_ENTRYPOINT_DEPTH: usize = 8;
const MAX_BOUND_FILES: usize = 64;
const MAX_BOUND_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Shell {
        executable: String,
        flags: Vec<String>,
        command: String,
    },
    Program {
        executable: String,
        args: Vec<String>,
    },
    Image {
        executable: String,
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    path: PathBuf,
    digest: [u8; 32],
    bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    cwd: PathBuf,
    request_digest: [u8; 32],
    files: Vec<FileIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandBinding {
    request: Request,
    cwd: PathBuf,
    // Only tests supply an override, avoiding process-global PATH races.
    path_override: Option<OsString>,
    snapshot: Snapshot,
}

impl CommandBinding {
    pub(crate) fn capture_image(
        executable: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<Self, String> {
        if !Path::new(executable).is_absolute() {
            return Err(format!(
                "{UNSUPPORTED}: native transport executable must be an absolute path"
            ));
        }
        Self::capture(
            Request::Image {
                executable: executable.into(),
                args: args.to_vec(),
            },
            cwd,
            None,
        )
    }
    pub(crate) fn capture_shell(
        executable: &str,
        flags: &[String],
        command: &str,
        cwd: &Path,
    ) -> Result<Self, String> {
        Self::capture(
            Request::Shell {
                executable: executable.into(),
                flags: flags.to_vec(),
                command: command.into(),
            },
            cwd,
            None,
        )
    }

    pub(crate) fn capture_program(
        executable: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<Self, String> {
        if !Path::new(executable).is_absolute() {
            return Err(format!(
                "{UNSUPPORTED}: native process executable must be an absolute path"
            ));
        }
        Self::capture(
            Request::Program {
                executable: executable.into(),
                args: args.to_vec(),
            },
            cwd,
            None,
        )
    }

    fn capture(
        request: Request,
        cwd: &Path,
        path_override: Option<OsString>,
    ) -> Result<Self, String> {
        if !cwd.is_absolute() {
            return Err(format!("{UNSUPPORTED}: working directory must be absolute"));
        }
        let mut snapshot = Snapshot {
            cwd: std::fs::canonicalize(cwd)
                .map_err(|_| format!("{UNSUPPORTED}: working directory is unavailable"))?,
            request_digest: request_digest(&request),
            files: Vec::new(),
        };
        if !snapshot.cwd.is_dir() {
            return Err(format!("{UNSUPPORTED}: working directory is unavailable"));
        }
        let resolver = Resolver {
            cwd: &snapshot.cwd,
            path: path_override.clone().or_else(|| std::env::var_os("PATH")),
            posix_root: None,
        };
        let mut files = Vec::new();
        match &request {
            Request::Shell {
                executable,
                flags,
                command,
            } => {
                let shell = resolver.executable(executable)?;
                if !supports_static_shell(&shell) || flags != &["-c"] {
                    return Err(format!("{UNSUPPORTED}: use bash, dash or sh with -c; other shell startup and command grammars are unsupported"));
                }
                let shell_resolver = capture_shell_image(&resolver, &shell, &mut files, 0)?;
                capture_shell_commands(&shell_resolver, command, &mut files, 0)?;
            }
            Request::Program { executable, args } => {
                let executable = resolver.executable(executable)?;
                capture_entrypoint(&resolver, &executable, args, &mut files, 0)?;
            }
            Request::Image { executable, .. } => {
                let executable = resolver.executable(executable)?;
                capture_image(&resolver, &executable, &mut files, 0)?;
            }
        }
        snapshot.files = files;
        Ok(Self {
            request,
            cwd: cwd.to_path_buf(),
            path_override,
            snapshot,
        })
    }

    /// Compare a fresh capture when mutable host configuration selected a launcher.
    pub(crate) fn ensure_unchanged(&self, current: &Self) -> Result<(), String> {
        if self.request == current.request && self.snapshot == current.snapshot {
            Ok(())
        } else {
            Err(CHANGED.into())
        }
    }

    /// Repeat actual path lookup and hashing, including earlier PATH candidates.
    pub(crate) fn revalidate(&self) -> Result<(), String> {
        let current = Self::capture(self.request.clone(), &self.cwd, self.path_override.clone())
            .map_err(|_| CHANGED.to_string())?;
        self.ensure_unchanged(&current)
    }
}

#[cfg(test)]
fn capture_program_with_path(
    executable: &str,
    args: &[String],
    cwd: &Path,
    path: Option<OsString>,
) -> Result<CommandBinding, String> {
    CommandBinding::capture(
        Request::Program {
            executable: executable.into(),
            args: args.to_vec(),
        },
        cwd,
        path,
    )
}

fn request_digest(request: &Request) -> [u8; 32] {
    let mut hash = Sha256::new();
    let mut part = |value: &str| {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    };
    match request {
        Request::Shell {
            executable,
            flags,
            command,
        } => {
            part("shell");
            part(executable);
            for flag in flags {
                part(flag);
            }
            part(command);
        }
        Request::Program { executable, args } => {
            part("program");
            part(executable);
            for arg in args {
                part(arg);
            }
        }
        Request::Image { executable, args } => {
            part("image");
            part(executable);
            for arg in args {
                part(arg);
            }
        }
    }
    hash.finalize().into()
}

struct Resolver<'a> {
    cwd: &'a Path,
    path: Option<OsString>,
    posix_root: Option<PathBuf>,
}

impl Resolver<'_> {
    fn path(&self, value: &str) -> Result<PathBuf, String> {
        #[cfg(windows)]
        if value.starts_with('/') {
            if value.starts_with("//")
                || value.eq_ignore_ascii_case("/tmp")
                || value
                    .get(..5)
                    .is_some_and(|part| part.eq_ignore_ascii_case("/tmp/"))
            {
                return Err(format!("{UNSUPPORTED}: use a Windows absolute path instead of a virtual POSIX /tmp or network path"));
            }
            let bytes = value.as_bytes();
            if bytes.len() >= 3 && bytes[1].is_ascii_alphabetic() && bytes[2] == b'/' {
                return Ok(PathBuf::from(format!(
                    "{}:{}",
                    char::from(bytes[1]),
                    &value[2..]
                )));
            }
            if let Some(root) = &self.posix_root {
                let relative = value.trim_start_matches('/');
                let relative = relative
                    .strip_prefix("bin/")
                    .map(|s| format!("usr/bin/{s}"))
                    .unwrap_or_else(|| relative.into());
                return Ok(root.join(relative));
            }
            return Err(format!(
                "{UNSUPPORTED}: POSIX path cannot be mapped to the selected shell"
            ));
        }
        let path = Path::new(value);
        Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        })
    }

    fn executable(&self, value: &str) -> Result<PathBuf, String> {
        self.executables(value)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("{UNSUPPORTED}: executable {value:?} cannot be resolved"))
    }

    fn executables(&self, value: &str) -> Result<Vec<PathBuf>, String> {
        if value.is_empty() || value.contains('\0') {
            return Err(UNSUPPORTED.into());
        }
        let explicit = value.contains(['/', '\\']) || Path::new(value).is_absolute();
        let mut candidates: Vec<PathBuf> = if explicit {
            vec![self.path(value)?]
        } else {
            self.path
                .as_ref()
                .map(|path| {
                    std::env::split_paths(path)
                        .map(|dir| {
                            if dir.is_absolute() {
                                dir.join(value)
                            } else {
                                self.cwd.join(dir).join(value)
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        if !explicit {
            if let Some(root) = &self.posix_root {
                // Git's compatibility launcher prepends runtime directories.
                // Bind the entire possible candidate set as well as PATH, so
                // search-order differences and a newly earlier image are fenced.
                for relative in [
                    "mingw64/bin",
                    "mingw32/bin",
                    "clangarm64/bin",
                    "usr/bin",
                    "bin",
                ] {
                    candidates.push(root.join(relative).join(value));
                }
            }
        }
        let mut resolved = Vec::new();
        for candidate in candidates {
            let mut alternatives = vec![candidate.clone()];
            if cfg!(windows) && candidate.extension().is_none() {
                alternatives.push(candidate.with_extension("exe"));
                alternatives.push(candidate.with_extension("com"));
            }
            for candidate in alternatives {
                if executable_file(&candidate) {
                    let path =
                        std::fs::canonicalize(candidate).map_err(|_| UNSUPPORTED.to_string())?;
                    let extension = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if matches!(name(&path).as_str(), "cmd" | "powershell" | "pwsh")
                        || matches!(extension.as_str(), "cmd" | "bat" | "ps1")
                    {
                        return Err(format!(
                            "{UNSUPPORTED}: this launcher needs a different command grammar"
                        ));
                    }
                    if !resolved.contains(&path) {
                        resolved.push(path);
                    }
                }
            }
        }
        if resolved.is_empty() {
            Err(format!(
                "{UNSUPPORTED}: executable {value:?} cannot be resolved"
            ))
        } else {
            Ok(resolved)
        }
    }
}

fn executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}

fn name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

fn is_posix_shell(path: &Path) -> bool {
    matches!(name(path).as_str(), "sh" | "bash" | "dash" | "zsh" | "ksh")
}

fn supports_static_shell(path: &Path) -> bool {
    // zsh reads .zshenv even for non-interactive -c, which can install aliases
    // or replace lookup before our command. Do not claim static resolution there.
    matches!(name(path).as_str(), "sh" | "bash" | "dash")
}

fn git_posix_root(shell: &Path) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let bin = shell.parent()?;
    if bin.file_name()?.to_str()?.eq_ignore_ascii_case("bin") {
        let parent = bin.parent()?;
        Some(
            if parent.file_name()?.to_str()?.eq_ignore_ascii_case("usr") {
                parent.parent()?.to_path_buf()
            } else {
                parent.to_path_buf()
            },
        )
    } else {
        None
    }
}

fn capture_shell_image<'a>(
    resolver: &Resolver<'a>,
    shell: &Path,
    files: &mut Vec<FileIdentity>,
    depth: usize,
) -> Result<Resolver<'a>, String> {
    if !supports_static_shell(shell) {
        return Err(format!(
            "{UNSUPPORTED}: this shell can change command resolution during startup"
        ));
    }
    let child = Resolver {
        cwd: resolver.cwd,
        path: resolver.path.clone(),
        posix_root: git_posix_root(shell),
    };
    capture_image(&child, shell, files, depth)?;
    if let Some(root) = &child.posix_root {
        capture_git_mounts(root, files)?;
        if root.join("etc/git-bash.config").exists() {
            return Err(format!(
                "{UNSUPPORTED}: Git Bash custom launcher configuration is unsupported"
            ));
        }
        // Git's bin/bash.exe is a compatibility launcher for this second image.
        // Hash both, including an image appearing between capture and revalidation.
        let actual = root
            .join("usr/bin")
            .join(shell.file_name().ok_or_else(|| UNSUPPORTED.to_string())?);
        if actual.is_file() && std::fs::canonicalize(&actual).ok().as_deref() != Some(shell) {
            capture_image(&child, &actual, files, depth)?;
        }
    }
    Ok(child)
}

fn capture_git_mounts(root: &Path, files: &mut Vec<FileIdentity>) -> Result<(), String> {
    let global = root.join("etc/fstab");
    if global.exists() {
        capture_mount_file(&global, true, files)?;
    }
    let per_user = root.join("etc/fstab.d");
    if per_user.exists() {
        let entries = std::fs::read_dir(&per_user)
            .map_err(|_| format!("{UNSUPPORTED}: Git mount configuration is unreadable"))?;
        let mut paths = Vec::new();
        for entry in entries.take(MAX_BOUND_FILES + 1) {
            paths.push(entry.map_err(|_| UNSUPPORTED.to_string())?.path());
        }
        if paths.len() > MAX_BOUND_FILES {
            return Err(format!(
                "{UNSUPPORTED}: too many Git mount configuration files"
            ));
        }
        paths.sort();
        for path in paths {
            capture_mount_file(&path, false, files)?;
        }
    }
    // The native Windows temp mount is intentionally not guessed from daemon
    // state: explicit /tmp operands are refused by Resolver::path.
    Ok(())
}

fn capture_mount_file(
    path: &Path,
    defaults: bool,
    files: &mut Vec<FileIdentity>,
) -> Result<(), String> {
    let prefix = capture_file(path, files)?;
    if files.last().is_some_and(|file| file.bytes > 4096) {
        return Err(format!(
            "{UNSUPPORTED}: Git mount configuration exceeds 4 KiB"
        ));
    }
    let text = std::str::from_utf8(&prefix)
        .map_err(|_| format!("{UNSUPPORTED}: Git mount configuration is not text"))?;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        let standard = defaults
            && (fields
                == [
                    "none",
                    "/",
                    "cygdrive",
                    "binary,posix=0,noacl,user",
                    "0",
                    "0",
                ]
                || fields == ["none", "/tmp", "usertemp", "binary,posix=0,noacl", "0", "0"]);
        if !standard {
            return Err(format!("{UNSUPPORTED}: custom Git Bash fstab/fstab.d mounts are unsupported; use the default mount configuration"));
        }
    }
    Ok(())
}

fn capture_file(path: &Path, files: &mut Vec<FileIdentity>) -> Result<Vec<u8>, String> {
    if files.len() >= MAX_BOUND_FILES {
        return Err(format!(
            "{UNSUPPORTED}: at most 64 executable/script images can be bound"
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| format!("{UNSUPPORTED}: entrypoint is unavailable"))?;
    let mut file = std::fs::File::open(&canonical)
        .map_err(|_| format!("{UNSUPPORTED}: entrypoint cannot be read"))?;
    let metadata = file.metadata().map_err(|_| UNSUPPORTED.to_string())?;
    if !metadata.is_file() {
        return Err(format!("{UNSUPPORTED}: entrypoint must be a regular file"));
    }
    let previous_bytes: u64 = files.iter().map(|file| file.bytes).sum();
    let remaining_bytes = MAX_BOUND_BYTES.saturating_sub(previous_bytes);
    if metadata.len() > remaining_bytes {
        return Err(format!(
            "{UNSUPPORTED}: executable/script images exceed 256 MiB total"
        ));
    }
    let mut hash = Sha256::new();
    let mut prefix = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| UNSUPPORTED.to_string())?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > remaining_bytes {
            return Err(format!(
                "{UNSUPPORTED}: executable/script image grew beyond the 256 MiB binding limit"
            ));
        }
        hash.update(&buffer[..count]);
        if prefix.len() < 4096 {
            prefix.extend_from_slice(&buffer[..count.min(4096 - prefix.len())]);
        }
    }
    if prefix.starts_with(b"!<symlink>") {
        return Err(format!("{UNSUPPORTED}: legacy MSYS symlink files are unsupported; use a native filesystem symlink or explicit target path"));
    }
    files.push(FileIdentity {
        path: canonical,
        digest: hash.finalize().into(),
        bytes,
    });
    Ok(prefix)
}

fn capture_image(
    resolver: &Resolver<'_>,
    executable: &Path,
    files: &mut Vec<FileIdentity>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_ENTRYPOINT_DEPTH {
        return Err(format!("{UNSUPPORTED}: entrypoint nesting is too deep"));
    }
    let prefix = capture_file(executable, files)?;
    if prefix.starts_with(b"#!") {
        let line = prefix.split(|b| *b == b'\n').next().unwrap_or_default();
        let line = std::str::from_utf8(&line[2..])
            .map_err(|_| UNSUPPORTED.to_string())?
            .trim();
        let words: Vec<_> = line.split_whitespace().collect();
        if words.is_empty()
            || words.len() > 2
            || !words[0].contains('/')
            || name(Path::new(words[0])) == "env"
        {
            return Err(format!(
                "{UNSUPPORTED}: shebang interpreter must be an explicit static path"
            ));
        }
        let interpreter = resolver.executable(words[0])?;
        if name(&interpreter) == "env" {
            return Err(format!(
                "{UNSUPPORTED}: shebang environment loaders are unsupported"
            ));
        }
        if let Some(option) = words.get(1) {
            let supported = match name(&interpreter).as_str() {
                "sh" | "bash" | "dash" => matches!(*option, "-e" | "-u" | "-eu" | "-ue"),
                python if python.starts_with("python") => matches!(
                    *option,
                    "-I" | "-u" | "-S" | "-s" | "-E" | "-B" | "-O" | "-OO"
                ),
                _ => false,
            };
            if !supported {
                return Err(format!(
                    "{UNSUPPORTED}: shebang loader option is unsupported"
                ));
            }
        }
        // The entry script itself is already bound. Bind the shebang interpreter
        // as an executable without treating this script's user argv as its flags.
        capture_image(resolver, &interpreter, files, depth + 1)?;
    }
    Ok(())
}

fn capture_entrypoint(
    resolver: &Resolver<'_>,
    executable: &Path,
    args: &[String],
    files: &mut Vec<FileIdentity>,
    depth: usize,
) -> Result<(), String> {
    if !is_posix_shell(executable) {
        capture_image(resolver, executable, files, depth)?;
    }
    let executable_name = name(executable);
    if matches!(
        executable_name.as_str(),
        "env"
            | "command"
            | "exec"
            | "sudo"
            | "doas"
            | "su"
            | "nice"
            | "nohup"
            | "timeout"
            | "stdbuf"
            | "setsid"
            | "xargs"
            | "npm"
            | "npx"
            | "pnpm"
            | "yarn"
            | "bun"
            | "deno"
            | "busybox"
            | "toybox"
            | "perl"
            | "ruby"
            | "php"
            | "lua"
            | "luajit"
            | "rscript"
            | "awk"
            | "gawk"
            | "mawk"
            | "sed"
            | "tsx"
            | "ts-node"
    ) {
        return Err(format!(
            "{UNSUPPORTED}: wrapper or package-loader execution is not bound"
        ));
    }
    if is_posix_shell(executable) {
        let shell_resolver = capture_shell_image(resolver, executable, files, depth)?;
        if args.first().is_some_and(|arg| arg == "-c") && args.len() >= 2 {
            return capture_shell_commands(&shell_resolver, &args[1], files, depth + 1);
        }
        let mut index = 0;
        while args
            .get(index)
            .is_some_and(|arg| matches!(arg.as_str(), "--" | "-e" | "-u" | "-eu" | "-ue"))
        {
            index += 1;
        }
        let script = args
            .get(index)
            .filter(|arg| !arg.starts_with('-'))
            .ok_or_else(|| {
                format!("{UNSUPPORTED}: shell input or options are not statically bindable")
            })?;
        capture_file(&shell_resolver.path(script)?, files)?;
        return Ok(());
    }
    if executable_name.starts_with("python") || matches!(executable_name.as_str(), "pypy" | "pypy3")
    {
        return capture_interpreter_script(resolver, args, files, true);
    }
    if matches!(executable_name.as_str(), "node" | "nodejs") {
        return capture_interpreter_script(resolver, args, files, false);
    }
    // Explicit secondary command syntax is not a single resolved entrypoint.
    if executable_name == "find"
        && args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
    {
        return Err(format!(
            "{UNSUPPORTED}: secondary command execution is unsupported"
        ));
    }
    // An interpreter can be renamed. A plainly named script operand is still
    // identifiable even when the executable's basename is unfamiliar.
    for arg in args {
        let extension = Path::new(arg)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !arg.starts_with('-')
            && matches!(
                extension.as_str(),
                "py" | "pyc" | "js" | "mjs" | "cjs" | "sh" | "bash" | "rb" | "pl" | "php" | "lua"
            )
        {
            capture_file(&resolver.path(arg)?, files)?;
        }
    }
    Ok(())
}

fn capture_interpreter_script(
    resolver: &Resolver<'_>,
    args: &[String],
    files: &mut Vec<FileIdentity>,
    python: bool,
) -> Result<(), String> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if (python && arg == "-c")
            || (!python && matches!(arg.as_str(), "-e" | "--eval" | "-p" | "--print"))
        {
            let source = args.get(index + 1).ok_or_else(|| UNSUPPORTED.to_string())?;
            if !python {
                // Node keeps parsing runtime flags after eval/print source. A
                // missing print expression can also leave the next flag active;
                // never mistake that flag for the inline program being bound.
                if source.starts_with('-') {
                    return Err(format!(
                        "{UNSUPPORTED}: Node inline source must be an explicit non-option argument"
                    ));
                }
                for trailing in &args[index + 2..] {
                    if trailing == "--" || trailing == "-" || !trailing.starts_with('-') {
                        break;
                    }
                    if !supported_node_runtime_option(trailing) {
                        return Err(format!(
                            "{UNSUPPORTED}: Node runtime option after inline source is unsupported"
                        ));
                    }
                }
            }
            return Ok(());
        }
        if arg == "--" {
            index += 1;
            break;
        }
        if !arg.starts_with('-') {
            break;
        }
        let allowed = if python {
            matches!(
                arg.as_str(),
                "-I" | "-u"
                    | "-S"
                    | "-s"
                    | "-E"
                    | "-B"
                    | "-O"
                    | "-OO"
                    | "-q"
                    | "-v"
                    | "-V"
                    | "--version"
                    | "--help"
            )
        } else {
            supported_node_runtime_option(arg)
        };
        if !allowed {
            return Err(format!(
                "{UNSUPPORTED}: interpreter loader/input option is unsupported"
            ));
        }
        index += 1;
    }
    let Some(script) = args.get(index) else {
        return if args
            .iter()
            .any(|a| matches!(a.as_str(), "-V" | "-v" | "--version" | "--help"))
        {
            Ok(())
        } else {
            Err(format!(
                "{UNSUPPORTED}: interpreter input is not an entry script or inline program"
            ))
        };
    };
    if script == "-" {
        return Err(format!(
            "{UNSUPPORTED}: interpreter standard input is not bound"
        ));
    }
    let path = resolver.path(script)?;
    if !python && path.extension().is_none() {
        return Err(format!(
            "{UNSUPPORTED}: Node entrypoint must name a file including its extension"
        ));
    }
    capture_file(&path, files)?;
    Ok(())
}

fn supported_node_runtime_option(arg: &str) -> bool {
    matches!(arg, "--no-warnings" | "--version" | "-v" | "--help")
}

fn capture_shell_commands(
    resolver: &Resolver<'_>,
    command: &str,
    files: &mut Vec<FileIdentity>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_ENTRYPOINT_DEPTH || has_compound_syntax(command) {
        return Err(format!(
            "{UNSUPPORTED}: compound shell syntax is unsupported"
        ));
    }
    let lexed = shell_lex::lex(command);
    if lexed.unclosed || lexed.pipelines.is_empty() {
        return Err(UNSUPPORTED.into());
    }
    for pipeline in lexed.pipelines {
        for segment in pipeline {
            if !segment.redirects.is_empty()
                || !segment.heredocs.is_empty()
                || !segment.here_strings.is_empty()
            {
                return Err(format!(
                    "{UNSUPPORTED}: redirects and shell input loaders are unsupported"
                ));
            }
            if segment.words.iter().any(|word| {
                word.dynamic || !word.substitutions.is_empty() || has_unquoted_pattern(&word.raw)
            }) {
                return Err(format!("{UNSUPPORTED}: shell expansions are unsupported"));
            }
            let words: Vec<_> = segment
                .words
                .iter()
                .map(|word| word.value.clone())
                .collect();
            let Some(executable) = words.first() else {
                return Err(UNSUPPORTED.into());
            };
            if executable.contains('=')
                || matches!(
                    executable.as_str(),
                    "cd" | "pushd"
                        | "popd"
                        | "source"
                        | "."
                        | "eval"
                        | "exec"
                        | "command"
                        | "builtin"
                        | "enable"
                        | "alias"
                        | "unalias"
                        | "export"
                        | "unset"
                        | "set"
                        | "readonly"
                        | "read"
                        | "trap"
                        | "hash"
                        | "function"
                        | "if"
                        | "then"
                        | "else"
                        | "elif"
                        | "fi"
                        | "for"
                        | "while"
                        | "until"
                        | "do"
                        | "done"
                        | "case"
                        | "esac"
                        | "select"
                        | "coproc"
                        | "time"
                        | "!"
                        | "return"
                        | "break"
                        | "continue"
                        | "local"
                        | "declare"
                        | "typeset"
                        | "let"
                        | "getopts"
                        | "shopt"
                        | "umask"
                        | "ulimit"
                )
            {
                return Err(format!(
                    "{UNSUPPORTED}: shell context changes and dynamic dispatch are unsupported"
                ));
            }
            if matches!(
                executable.as_str(),
                "echo" | "printf" | "pwd" | "true" | "false" | "test" | "[" | ":" | "exit"
            ) {
                if executable == "printf" && words.get(1).is_some_and(|arg| arg == "-v") {
                    return Err(format!(
                        "{UNSUPPORTED}: printf variable assignment is unsupported"
                    ));
                }
                continue;
            }
            for path in resolver.executables(executable)? {
                capture_entrypoint(resolver, &path, &words[1..], files, depth + 1)?;
            }
        }
    }
    Ok(())
}

// The lexer intentionally discards grouping separators; reject them before lexing.
fn has_compound_syntax(text: &str) -> bool {
    has_unquoted_syntax(text, &['(', ')', '{', '}'], true)
}

fn has_unquoted_pattern(text: &str) -> bool {
    has_unquoted_syntax(text, &['*', '?', '[', '~', '{', '}'], false)
}

fn has_unquoted_syntax(text: &str, forbidden: &[char], comments: bool) -> bool {
    #[derive(Clone, Copy)]
    enum Quote {
        None,
        Single,
        Double,
        Ansi,
    }
    let chars: Vec<_> = text.chars().collect();
    let (mut quote, mut index, mut word_start) = (Quote::None, 0, true);
    while let Some(&c) = chars.get(index) {
        match quote {
            Quote::Single => {
                if c == '\'' {
                    quote = Quote::None;
                }
            }
            Quote::Double => match c {
                '\\' => index += 1,
                '"' => quote = Quote::None,
                _ => {}
            },
            Quote::Ansi => match c {
                '\\' => index += 1,
                '\'' => quote = Quote::None,
                _ => {}
            },
            Quote::None => match c {
                '\\' => {
                    index += 1;
                    word_start = false;
                }
                '$' if chars.get(index + 1) == Some(&'\'') => {
                    quote = Quote::Ansi;
                    index += 1;
                    word_start = false;
                }
                '\'' => {
                    quote = Quote::Single;
                    word_start = false;
                }
                '"' => {
                    quote = Quote::Double;
                    word_start = false;
                }
                '#' if comments && word_start => {
                    while chars.get(index).is_some_and(|c| *c != '\n') {
                        index += 1;
                    }
                    word_start = true;
                    continue;
                }
                _ => {
                    if forbidden.contains(&c) {
                        return true;
                    }
                    word_start = c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')');
                }
            },
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "anima-command-binding-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            path
        }

        fn executable(&self, name: &str) -> String {
            let name = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.into()
            };
            self.write(&name, b"test executable identity")
                .to_str()
                .unwrap()
                .into()
        }

        fn shell(&self, command: &str) -> Result<CommandBinding, String> {
            let shell = self.0.join(if cfg!(windows) { "bash.exe" } else { "bash" });
            if !shell.exists() {
                self.write(
                    shell.file_name().unwrap().to_str().unwrap(),
                    b"shell identity",
                );
            }
            CommandBinding::capture_shell(shell.to_str().unwrap(), &["-c".into()], command, &self.0)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn quote(path: &str) -> String {
        format!("'{}'", path.replace('\\', "/").replace('\'', "'\\''"))
    }

    #[test]
    fn unchanged_static_builtin_pipeline_passes() {
        let fixture = Fixture::new();
        fixture
            .shell("printf '%s\\n' 'unchanged' | echo okay; true")
            .unwrap()
            .revalidate()
            .unwrap();
    }

    #[test]
    fn python_script_edit_during_approval_is_refused() {
        let fixture = Fixture::new();
        let python = fixture.executable("python");
        fixture.write("script.py", b"print('approved')");
        let binding = fixture
            .shell(&format!("{} script.py", quote(&python)))
            .unwrap();
        fixture.write("script.py", b"print('changed!')");
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn node_script_edit_during_approval_is_refused() {
        let fixture = Fixture::new();
        let node = fixture.executable("node");
        fixture.write("script.js", b"console.log('approved')");
        let binding = fixture
            .shell(&format!("{} script.js", quote(&node)))
            .unwrap();
        fixture.write("script.js", b"console.log('changed!')");
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn direct_script_and_shell_binary_edits_are_refused() {
        let fixture = Fixture::new();
        fixture.shell("true").unwrap();
        let shell = fixture
            .0
            .join(if cfg!(windows) { "bash.exe" } else { "bash" });
        let script = fixture.write(
            "run.sh",
            format!(
                "#!{}\necho approved",
                shell.to_str().unwrap().replace('\\', "/")
            )
            .as_bytes(),
        );
        let binding = fixture.shell(&quote(script.to_str().unwrap())).unwrap();
        fixture.write("run.sh", b"#!/bin/sh\necho changed!");
        assert!(binding.revalidate().is_err());
        let binding = fixture.shell("echo okay").unwrap();
        fixture.write(
            if cfg!(windows) { "bash.exe" } else { "bash" },
            b"changed shell identity",
        );
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn explicit_program_binary_and_argument_drift_are_refused() {
        let fixture = Fixture::new();
        let interpreter = fixture.executable("python");
        let args = vec!["-I".into(), "-c".into(), "print('approved')".into()];
        let binding = CommandBinding::capture_program(&interpreter, &args, &fixture.0).unwrap();
        binding.revalidate().unwrap();
        let mut changed = args;
        changed[2] = "print('changed!')".into();
        assert!(binding
            .ensure_unchanged(
                &CommandBinding::capture_program(&interpreter, &changed, &fixture.0).unwrap()
            )
            .is_err());
        std::fs::write(interpreter, b"changed interpreter").unwrap();
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn executable_appearing_earlier_in_search_path_is_refused() {
        let fixture = Fixture::new();
        let early = fixture.0.join("early");
        let late = fixture.0.join("late");
        std::fs::create_dir(&early).unwrap();
        std::fs::create_dir(&late).unwrap();
        let name = if cfg!(windows) {
            "bound-command.exe"
        } else {
            "bound-command"
        };
        std::fs::write(late.join(name), b"approved binary").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(late.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        // An explicit search context avoids process-global PATH mutation in parallel tests.
        let binding = capture_program_with_path(
            "bound-command",
            &[],
            &fixture.0,
            Some(std::env::join_paths([&early, &late]).unwrap()),
        )
        .unwrap();
        std::fs::write(early.join(name), b"substitute binary").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(early.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn dynamic_contexts_loaders_and_unknown_shells_fail_closed() {
        let fixture = Fixture::new();
        for command in [
            "cd somewhere; echo hi",
            "PATH=/tmp echo hi",
            "eval 'echo hi'",
            "echo $(date)",
            "echo *.py",
            "env python x.py",
            "python -m package",
            "node --require hook.js x.js",
            "source ./run.sh",
            "exec echo hi",
            "for x in a; do echo hi; done",
        ] {
            assert!(fixture.shell(command).is_err(), "must reject {command}");
        }
        let shell = fixture.executable("powershell");
        assert!(
            CommandBinding::capture_shell(&shell, &["-Command".into()], "echo hi", &fixture.0)
                .is_err()
        );
    }

    #[test]
    fn direct_program_requires_an_absolute_image_and_unsupported_interpreters_refuse() {
        let fixture = Fixture::new();
        assert!(CommandBinding::capture_program(
            "python",
            &["-c".into(), "print(1)".into()],
            &fixture.0
        )
        .is_err());
        for program in ["perl", "ruby", "busybox"] {
            let program = fixture.executable(program);
            assert!(
                CommandBinding::capture_program(&program, &["script".into()], &fixture.0).is_err()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn git_shell_compatibility_launcher_also_binds_the_actual_shell() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.0.join("bin")).unwrap();
        std::fs::create_dir_all(fixture.0.join("usr/bin")).unwrap();
        let launcher = fixture.write("bin/bash.exe", b"Git compatibility launcher");
        fixture.write("usr/bin/bash.exe", b"actual shell");
        let binding = CommandBinding::capture_shell(
            launcher.to_str().unwrap(),
            &["-c".into()],
            "echo okay",
            &fixture.0,
        )
        .unwrap();
        fixture.write("usr/bin/bash.exe", b"substitute shell");
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn immutable_interpreter_entry_files_pass() {
        let fixture = Fixture::new();
        for (interpreter, script) in [("python", "entry.py"), ("node", "entry.js")] {
            let interpreter = fixture.executable(interpreter);
            fixture.write(script, b"approved script bytes");
            let binding = fixture
                .shell(&format!("{} {}", quote(&interpreter), script))
                .unwrap();
            binding.revalidate().unwrap();
        }
    }

    #[test]
    fn node_inline_programs_refuse_trailing_loader_options_before_argv_boundary() {
        let fixture = Fixture::new();
        let node = fixture.executable("node");
        for inline in ["-e", "--eval", "-p", "--print"] {
            for loader in [
                "--require",
                "--import",
                "--loader",
                "--experimental-loader",
                "-r",
                "--require=./hook.js",
                "--import=./hook.js",
                "--loader=./hook.js",
            ] {
                let args = vec![inline.into(), "0".into(), loader.into(), "./hook.js".into()];
                assert!(
                    CommandBinding::capture_program(&node, &args, &fixture.0).is_err(),
                    "must refuse {args:?}"
                );
                let absent_source = vec![inline.into(), loader.into(), "./hook.js".into()];
                assert!(
                    CommandBinding::capture_program(&node, &absent_source, &fixture.0).is_err(),
                    "must not mistake a runtime option for source: {absent_source:?}"
                );
            }
        }
    }

    #[test]
    fn node_inline_programs_preserve_literal_argv_after_option_termination() {
        let fixture = Fixture::new();
        let node = fixture.executable("node");
        for boundary in ["--", "argument", "", "-"] {
            let args = vec![
                "-e".into(),
                "0".into(),
                "--no-warnings".into(),
                boundary.into(),
                "--require".into(),
                "./hook.js".into(),
            ];
            CommandBinding::capture_program(&node, &args, &fixture.0)
                .unwrap()
                .revalidate()
                .unwrap();
        }
        // Python stops interpreter option parsing immediately after its -c source.
        let python = fixture.executable("python");
        CommandBinding::capture_program(
            &python,
            &["-c".into(), "print(0)".into(), "--require".into()],
            &fixture.0,
        )
        .unwrap()
        .revalidate()
        .unwrap();
    }

    #[test]
    fn escaped_ansi_quote_does_not_hide_unquoted_globs_or_grouping() {
        let fixture = Fixture::new();
        assert!(fixture.shell(r"echo $'\''*.py").is_err());
        assert!(fixture.shell(r"echo $'\''; (echo grouped)").is_err());
        fixture
            .shell(r"echo $'\'' 'literal*.py'")
            .unwrap()
            .revalidate()
            .unwrap();
    }

    #[test]
    fn transport_argv_is_bound_as_data_not_as_host_script_operands() {
        let fixture = Fixture::new();
        let docker = fixture.executable("docker");
        let binding =
            CommandBinding::capture_image(&docker, &["remote-script.py".into()], &fixture.0)
                .unwrap();
        binding.revalidate().unwrap();
        std::fs::write(&docker, b"changed transport").unwrap();
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn image_size_and_image_count_are_bounded_before_approval() {
        let fixture = Fixture::new();
        let image = fixture.executable("large");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&image)
            .unwrap()
            .set_len(256 * 1024 * 1024 + 1)
            .unwrap();
        assert!(CommandBinding::capture_image(&image, &[], &fixture.0).is_err());
        let image = fixture.executable("small");
        let command = vec![quote(&image); 65].join(";");
        assert!(fixture.shell(&command).is_err());
    }

    #[test]
    fn legacy_msys_symlink_stub_is_not_treated_as_the_script_image() {
        let fixture = Fixture::new();
        let script = fixture.write("entry.sh", b"!<symlink>unbound-entrypoint.sh");
        assert!(fixture.shell(&quote(script.to_str().unwrap())).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn git_mount_config_is_bound_and_custom_mounts_refuse() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.0.join("bin")).unwrap();
        std::fs::create_dir_all(fixture.0.join("etc/fstab.d")).unwrap();
        let launcher = fixture.write("bin/bash.exe", b"Git compatibility launcher");
        let capture = || {
            CommandBinding::capture_shell(
                launcher.to_str().unwrap(),
                &["-c".into()],
                "echo okay",
                &fixture.0,
            )
        };
        let absent = capture().unwrap();
        let defaults = b"# defaults\nnone / cygdrive binary,posix=0,noacl,user 0 0\nnone /tmp usertemp binary,posix=0,noacl 0 0\n";
        fixture.write("etc/fstab", defaults);
        assert!(absent.revalidate().is_err());
        let bound = capture().unwrap();
        fixture.write("etc/fstab", b"C:/elsewhere /workspace none binary 0 0\n");
        assert!(capture().is_err());
        assert!(bound.revalidate().is_err());
        fixture.write("etc/fstab", defaults);
        fixture.write(
            "etc/fstab.d/operator",
            b"C:/elsewhere /workspace none binary 0 0\n",
        );
        assert!(capture().is_err());
    }
}
