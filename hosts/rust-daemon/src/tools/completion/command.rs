//! Conservative invocation shapes, not a shell parser or a correctness oracle.
//!
//! Narrowly adapted from Hermes verification_evidence.py; see HERMES-NOTICE.
//! Unknown syntax/options fail open to the bounded missing-evidence reminder.

const MAX_COMMAND_BYTES: usize = 4096;
const MAX_COMMAND_TOKENS: usize = 128;

pub(super) fn recognizes(command: &str) -> bool {
    if command.len() > MAX_COMMAND_BYTES
        || !command
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" \t_./:-=+@,".contains(&byte))
    {
        return false;
    }
    let tokens: Vec<_> = command
        .split_ascii_whitespace()
        .take(MAX_COMMAND_TOKENS + 1)
        .collect();
    if tokens.len() > MAX_COMMAND_TOKENS {
        return false;
    }
    match tokens.as_slice() {
        ["cargo", action, args @ ..] => cargo(action, args),
        ["node", "--test", args @ ..] => arguments(
            args,
            &["--test-only"],
            &[
                "--test-reporter",
                "--test-name-pattern",
                "--test-skip-pattern",
            ],
            true,
        ),
        ["bun", "test", args @ ..] => {
            arguments(args, &[], &["--timeout", "-t", "--test-name-pattern"], true)
        }
        ["pytest", args @ ..] => pytest(args),
        ["python" | "python3" | "py", "-m", "pytest", args @ ..] => pytest(args),
        ["nx", args @ ..]
        | ["bun", "x", "nx", args @ ..]
        | ["pnpm", "exec", "nx", args @ ..]
        | ["npx", "nx", args @ ..] => nx(args),
        _ => false,
    }
}

fn cargo(action: &str, args: &[&str]) -> bool {
    let separator = args.iter().position(|arg| *arg == "--");
    let (options, trailing) = separator.map_or((args, &[][..]), |index| {
        (&args[..index], &args[index + 1..])
    });
    if action == "fmt" {
        return args.contains(&"--check")
            && arguments(
                options,
                &["--all", "--check", "--quiet", "-q"],
                &["--package", "-p", "--manifest-path"],
                false,
            )
            && arguments(trailing, &["--check"], &[], false);
    }
    if !matches!(action, "test" | "check" | "build" | "clippy") {
        return false;
    }
    if !arguments(
        options,
        &[
            "--workspace",
            "--all-targets",
            "--all-features",
            "--no-default-features",
            "--release",
            "--locked",
            "--offline",
            "--frozen",
            "--lib",
            "--bins",
            "--tests",
            "--examples",
            "--benches",
            "--doc",
            "--quiet",
            "-q",
            "--verbose",
            "-v",
            "--keep-going",
        ],
        &[
            "--package",
            "-p",
            "--manifest-path",
            "--target",
            "--features",
            "-F",
            "--profile",
            "--test",
            "--bin",
            "--example",
            "--bench",
            "--color",
            "--jobs",
            "-j",
            "--message-format",
            "--target-dir",
            "--exclude",
        ],
        action == "test",
    ) {
        return false;
    }
    match action {
        "test" => arguments(
            trailing,
            &[
                "--ignored",
                "--include-ignored",
                "--nocapture",
                "--show-output",
                "--exact",
                "--quiet",
                "-q",
            ],
            &["--test-threads", "--skip", "--format"],
            true,
        ),
        "clippy" => arguments(
            trailing,
            &[],
            &["-D", "--deny", "-W", "--warn", "-A", "--allow"],
            false,
        ),
        _ => separator.is_none(),
    }
}

fn pytest(args: &[&str]) -> bool {
    arguments(
        args,
        &[
            "-q",
            "-v",
            "-vv",
            "-x",
            "-s",
            "--exitfirst",
            "--disable-warnings",
            "--strict-markers",
            "--strict-config",
            "--no-header",
            "--no-summary",
        ],
        &["-k", "-m", "--maxfail", "--capture", "--tb"],
        true,
    )
}

fn nx(args: &[&str]) -> bool {
    let ["run", target, options @ ..] = args else {
        return false;
    };
    let parts: Vec<_> = target.split(':').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts.iter().all(|part| !part.is_empty())
        && matches!(parts[1], "test" | "build" | "lint" | "typecheck" | "check")
        && arguments(
            options,
            &[
                "--skipNxCache",
                "--skip-nx-cache",
                "--runInBand",
                "--verbose",
                "--nxBail",
            ],
            &["--configuration", "-c", "--parallel"],
            false,
        )
}

/// Accept only explicit switches/options; unrecognized modes stay unclassified.
fn arguments(args: &[&str], switches: &[&str], values: &[&str], positional: bool) -> bool {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        // Some runners expand @files before parsing options. Their contents
        // cannot be classified here; scoped Nx project names are parsed above.
        if arg.starts_with('@') {
            return false;
        }
        if !arg.starts_with('-') {
            if !positional {
                return false;
            }
        } else {
            let (flag, inline) = arg
                .split_once('=')
                .map_or((*arg, None), |(flag, value)| (flag, Some(value)));
            if values.contains(&flag) {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        index += 1;
                        let Some(value) = args.get(index) else {
                            return false;
                        };
                        value
                    }
                };
                if value.is_empty() || value.starts_with('-') || value.starts_with('@') {
                    return false;
                }
            } else if inline.is_some() || !switches.contains(&flag) {
                return false;
            }
        }
        index += 1;
    }
    true
}
