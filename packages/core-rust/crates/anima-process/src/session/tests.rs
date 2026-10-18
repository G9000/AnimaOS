use super::*;

fn sh(script: &str) -> AppSessionConfig {
    let mut config = AppSessionConfig::new(
        "/bin/sh",
        vec!["-c".into(), script.into()],
        std::env::temp_dir(),
    );
    config.env = vec![("PATH".into(), "/usr/bin:/bin".into())];
    config.cols = 80;
    config.rows = 24;
    config
}

fn until(text: &str, max_ms: u64) -> WaitFor {
    WaitFor {
        until: Some(Until::Text(text.into())),
        settle: None,
        max: Duration::from_millis(max_ms),
    }
}

#[cfg(unix)]
fn running(marker: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", marker])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn typed_text_reaches_the_app_and_its_reply_is_on_screen() {
    let session =
        AppSession::start(sh("printf 'name? '; read name; echo \"hello $name\"")).unwrap();
    assert_eq!(
        session.wait(0, &until("name?", 5_000)).await,
        WaitOutcome::Matched
    );
    session.send(Some("anima"), &[Key::Enter]).unwrap();
    assert_eq!(
        session.wait(0, &until("hello anima", 5_000)).await,
        WaitOutcome::Matched
    );
    assert!(session.screen().text().contains("hello anima"));
    let exited = WaitFor {
        until: None,
        settle: None,
        max: Duration::from_secs(5),
    };
    assert_eq!(session.wait(0, &exited).await, WaitOutcome::Exited);
    assert_eq!(session.exit().map(|exit| exit.code), Some(0));
    let output = session.output_since(0, 1024);
    assert!(
        output.text.contains("name? anima\nhello anima"),
        "{:?}",
        output.text
    );
    assert!(session.stop().await);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ctrl_c_interrupts_the_foreground_app() {
    let session = AppSession::start(sh(
        "trap 'echo caught; exit 3' INT; echo ready; while :; do sleep 0.05; done",
    ))
    .unwrap();
    assert_eq!(
        session.wait(0, &until("ready", 5_000)).await,
        WaitOutcome::Matched
    );
    session.send(None, &[Key::CtrlC]).unwrap();
    assert_eq!(
        session.wait(0, &until("caught", 5_000)).await,
        WaitOutcome::Matched
    );
    let exited = WaitFor {
        until: None,
        settle: None,
        max: Duration::from_secs(5),
    };
    assert_eq!(session.wait(0, &exited).await, WaitOutcome::Exited);
    assert_eq!(session.exit().map(|exit| exit.code), Some(3));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cursor_query_gets_the_real_cursor() {
    // The app prints three characters, asks where the cursor is and prints
    // the answer it read back from its own input.
    let session = AppSession::start(sh("stty -icanon -echo; printf 'abc'; printf '\\033[6n'; \
         reply=$(dd bs=1 count=6 2>/dev/null); printf '\\nreply:%s\\n' \"${reply#??}\""))
    .unwrap();
    assert_eq!(
        session.wait(0, &until("reply:1;4R", 5_000)).await,
        WaitOutcome::Matched
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_full_screen_app_reads_as_its_visible_rows() {
    let session = AppSession::start(sh(
        "printf '\\033[?1049h\\033[2J\\033[H top\\033[5;3Hfifth'; read _",
    ))
    .unwrap();
    assert_eq!(
        session.wait(0, &until("fifth", 5_000)).await,
        WaitOutcome::Matched
    );
    let screen = session.screen();
    assert!(screen.alternate);
    assert_eq!(screen.rows[0], " top");
    assert_eq!(screen.rows[4], "  fifth");
    assert_eq!(screen.cursor, (4, 7));
    assert!(session.stop().await);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn waiting_settles_once_output_goes_quiet() {
    let session = AppSession::start(sh("echo one; sleep 0.2; echo two; sleep 30")).unwrap();
    let settle = WaitFor {
        until: None,
        settle: Some(Duration::from_millis(600)),
        max: Duration::from_secs(5),
    };
    let started = Instant::now();
    assert_eq!(session.wait(0, &settle).await, WaitOutcome::Settled);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(session.output_since(0, 1024).text.contains("two"));
    let timed_out = until("never printed", 200);
    assert_eq!(session.wait(0, &timed_out).await, WaitOutcome::TimedOut);
    assert!(session.stop().await);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn stop_kills_the_apps_descendants() {
    let marker = "31.7253";
    let session = AppSession::start(sh(&format!("sleep {marker} & echo started; wait"))).unwrap();
    assert_eq!(
        session.wait(0, &until("started", 5_000)).await,
        WaitOutcome::Matched
    );
    assert!(running(marker));
    assert!(session.stop().await);
    assert!(!running(marker));
    assert_eq!(session.containment(), Containment::ProcessGroup);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn dropping_a_session_kills_its_descendants() {
    let marker = "32.6481";
    let session = AppSession::start(sh(&format!("sleep {marker} & echo started; wait"))).unwrap();
    assert_eq!(
        session.wait(0, &until("started", 5_000)).await,
        WaitOutcome::Matched
    );
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(marker) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!running(marker));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_environment_is_only_what_the_caller_passed() {
    std::env::set_var("ANIMA_SESSION_TEST_SECRET", "leak");
    let mut config = sh("echo \"secret=[$ANIMA_SESSION_TEST_SECRET] term=$TERM pager=$PAGER\"");
    config.env.push(("PAGER".into(), "cat".into()));
    let session = AppSession::start(config).unwrap();
    assert_eq!(
        session
            .wait(0, &until("secret=[] term=xterm-256color pager=cat", 5_000))
            .await,
        WaitOutcome::Matched
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_missing_program_fails_to_start() {
    let config = AppSessionConfig::new("/nonexistent/anima-app", Vec::new(), std::env::temp_dir());
    assert!(AppSession::start(config).is_err());
}

/// A real line-editing REPL: skipped when Node is not installed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_node_repl_evaluates_what_is_typed() {
    let Some(node) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("node"))
            .find(|candidate| candidate.is_file())
    }) else {
        return;
    };
    let mut config = AppSessionConfig::new(node, vec!["-i".into()], std::env::temp_dir());
    config.env = vec![("PATH".into(), "/usr/bin:/bin".into())];
    let session = AppSession::start(config).unwrap();
    assert_eq!(
        session.wait(0, &until("> ", 10_000)).await,
        WaitOutcome::Matched
    );
    let from = session.output_end();
    session.send(Some("6*7"), &[Key::Enter]).unwrap();
    assert_eq!(
        session.wait(from, &until("42", 10_000)).await,
        WaitOutcome::Matched
    );
    session.send(None, &[Key::CtrlD]).unwrap();
    let exited = WaitFor {
        until: None,
        settle: None,
        max: Duration::from_secs(10),
    };
    assert_eq!(session.wait(0, &exited).await, WaitOutcome::Exited);
}

/// Manual check against the real Claude Code TUI: it must reach its first
/// screen (the folder-trust prompt) on our terminal. Sends no prompt.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Claude Code installed; run with --ignored"]
async fn claude_code_reaches_its_first_screen() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    let mut config = AppSessionConfig::new("claude", Vec::new(), dir.path());
    config.env = vec![
        ("PATH".into(), path),
        ("HOME".into(), std::env::var("HOME").unwrap_or_default()),
    ];
    let session = AppSession::start(config).unwrap();
    let trust = WaitFor {
        until: Some(Until::Pattern(regex::Regex::new("(?i)trust").unwrap())),
        settle: None,
        max: Duration::from_secs(30),
    };
    let outcome = session.wait(0, &trust).await;
    eprintln!("{}", session.screen().text());
    assert_eq!(outcome, WaitOutcome::Matched);
    assert!(session.stop().await);
}
