//! Manual check against the real Claude Code ACP adapter: it fixes a one-line
//! bug, its edit request reaches the host, and stop leaves nothing running.
//! Uses the owner's Claude Code login and downloads the adapter with npx.
//! Run with: cargo test -p anima-acp --test live_claude_code -- --ignored --nocapture
use anima_acp::{
    AcpClientHost, AcpEvent, AcpLaunch, AcpSession, BoxFuture, PermissionOptionKind,
    PermissionRequest, TurnStatus,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct AllowOnce {
    asked: Mutex<Vec<String>>,
    writes: Mutex<Vec<PathBuf>>,
}

impl AcpClientHost for AllowOnce {
    fn request_permission(&self, request: PermissionRequest) -> BoxFuture<'static, Option<String>> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("{:?}: {}", request.kind, request.title));
        let pick = request
            .options
            .iter()
            .find(|option| option.kind == PermissionOptionKind::AllowOnce)
            .map(|option| option.id.clone());
        Box::pin(async move { pick })
    }

    fn approve_write(&self, path: &Path) -> BoxFuture<'static, Result<(), String>> {
        self.writes.lock().unwrap().push(path.to_path_buf());
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives the real Claude Code ACP adapter with the owner's login"]
async fn claude_code_over_acp_fixes_a_bug_with_permission_requests() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    std::fs::write(
        repo.join("sum.js"),
        "function sum(numbers) {\n  let total = 0;\n  for (let i = 1; i < numbers.length; i++) total += numbers[i];\n  return total;\n}\nmodule.exports = { sum };\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("sum.test.js"),
        "const assert = require('node:assert');\nconst { sum } = require('./sum');\nassert.strictEqual(sum([1, 2, 3]), 6);\nconsole.log('ok');\n",
    )
    .unwrap();
    let host = Arc::new(AllowOnce::default());
    let env = ["PATH", "HOME", "USER", "TMPDIR", "LANG"]
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_string(), value))
        })
        .collect();
    let session = AcpSession::start(
        AcpLaunch {
            program: PathBuf::from("npx"),
            args: vec![
                "-y".into(),
                "@agentclientprotocol/claude-agent-acp@0.93.0".into(),
            ],
            cwd: repo.clone(),
            env,
            env_clear: true,
            resume_session_id: None,
            ask_modes: vec!["default".into()],
        },
        host.clone(),
    )
    .await
    .expect("adapter starts");
    eprintln!("agent: {:?}", session.agent_info());
    eprintln!("mode: {:?}", session.mode());
    session
        .prompt("sum.js has an off-by-one bug. Fix it so `node sum.test.js` prints ok. Edit only sum.js. Don't run commands.".into())
        .await
        .unwrap();
    let status = session.wait(Duration::from_secs(240)).await;
    let (events, _) = session.events_since(0);
    for event in &events {
        match event {
            AcpEvent::AgentMessage { text } => eprintln!("message: {}", text.trim()),
            AcpEvent::ToolCall { title, kind, .. } => eprintln!("tool call: {kind:?} {title}"),
            AcpEvent::PermissionAsked { title, .. } => eprintln!("asked: {title}"),
            AcpEvent::PermissionAnswered { option_id, .. } => eprintln!("answered: {option_id:?}"),
            AcpEvent::FileWritten { path } => eprintln!("wrote: {}", path.display()),
            AcpEvent::TurnEnded { stop_reason } => eprintln!("turn ended: {stop_reason}"),
            _ => {}
        }
    }
    eprintln!("status: {status:?}");
    eprintln!("asked: {:?}", host.asked.lock().unwrap());
    eprintln!("writes via fs: {:?}", host.writes.lock().unwrap());
    assert!(session.stop().await, "stop proves the tree gone");
    let fixed = std::fs::read_to_string(repo.join("sum.js")).unwrap();
    eprintln!("sum.js now:\n{fixed}");
    let test = std::process::Command::new("node")
        .arg("sum.test.js")
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(matches!(status, TurnStatus::Finished { .. }), "{status:?}");
    assert!(test.status.success(), "the bug is fixed");
}
