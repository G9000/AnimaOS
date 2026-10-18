//! Scripted ACP agent for anima-acp's integration tests. Never a runtime agent.
//! `FAKE_ACP_SCENARIO` picks the behaviour; see `turn` below.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

struct Peer {
    lines: io::Lines<io::StdinLock<'static>>,
    next_id: u64,
    cancelled: bool,
    record: Option<PathBuf>,
}

impl Peer {
    fn send(&self, value: &Value) {
        let mut stdout = io::stdout().lock();
        serde_json::to_writer(&mut stdout, value).unwrap();
        stdout.write_all(b"\n").unwrap();
        stdout.flush().unwrap();
    }

    fn raw(&self, line: &str) {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{line}").unwrap();
        stdout.flush().unwrap();
    }

    fn recv(&mut self) -> Option<Value> {
        let line = self.lines.next()?.ok()?;
        let value: Value = serde_json::from_str(&line).ok()?;
        if let Some(path) = &self.record {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(file, "{value}").unwrap();
        }
        if value["method"] == "session/cancel" {
            self.cancelled = true;
        }
        Some(value)
    }

    fn update(&self, update: Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": "sess-1", "update": update},
        }));
    }

    fn say(&self, text: &str) {
        self.update(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text},
        }));
    }

    /// Sends a request to the client and waits for its reply.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, i64> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.recv().expect("client reply");
            if message["id"] == id && message.get("method").is_none() {
                return match message.get("error") {
                    Some(error) => Err(error["code"].as_i64().unwrap_or(0)),
                    None => Ok(message["result"].clone()),
                };
            }
        }
    }
}

fn main() {
    let scenario = std::env::var("FAKE_ACP_SCENARIO").unwrap_or_else(|_| "basic".into());
    if scenario == "heartbeat" {
        let path = PathBuf::from(std::env::var("FAKE_ACP_HEARTBEAT").unwrap());
        let mut ticks = 0u64;
        loop {
            ticks += 1;
            let _ = std::fs::write(&path, ticks.to_string());
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    eprintln!("fake agent starting: {scenario}");
    let mut peer = Peer {
        lines: io::stdin().lock().lines(),
        next_id: 1000,
        cancelled: false,
        record: std::env::var_os("FAKE_ACP_RECORD").map(PathBuf::from),
    };
    while let Some(message) = peer.recv() {
        let id = message["id"].clone();
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        if id.is_null() {
            continue;
        }
        let reply = match method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": scenario == "load",
                    "promptCapabilities": {"image": false, "audio": false, "embeddedContext": true},
                },
                "agentInfo": {"name": "fake-agent", "title": "Fake Agent", "version": "0.0.1"},
                "authMethods": if scenario == "auth" {
                    json!([{"id": "oauth", "name": "Log in", "description": "Browser login"}])
                } else {
                    json!([])
                },
            })),
            "session/new" if scenario == "auth" => Err(json!({
                "code": -32000,
                "message": "Authentication required",
            })),
            "session/new" if scenario == "modes" => Ok(json!({
                "sessionId": "sess-1",
                "modes": {
                    "currentModeId": "acceptEdits",
                    "availableModes": [
                        {"id": "default", "name": "Default"},
                        {"id": "acceptEdits", "name": "Accept edits"},
                    ],
                },
            })),
            "session/new" => Ok(json!({"sessionId": "sess-1"})),
            // The mode the client picked, said back so a test can see it.
            "session/set_mode" => {
                peer.say(&format!(
                    "mode {}",
                    message["params"]["modeId"].as_str().unwrap_or("")
                ));
                Ok(json!({}))
            }
            "session/load" => {
                peer.update(json!({
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": "earlier question"},
                }));
                peer.say("earlier answer");
                Ok(json!({}))
            }
            "session/prompt" => {
                peer.cancelled = false;
                let stop = turn(&mut peer, &scenario);
                Ok(json!({"stopReason": stop}))
            }
            _ => Err(json!({"code": -32601, "message": "method not found"})),
        };
        let response = match reply {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        peer.send(&response);
    }
}

fn turn(peer: &mut Peer, scenario: &str) -> &'static str {
    match scenario {
        "basic" | "load" => {
            for chunk in ["Hel", "lo ", "world"] {
                peer.say(chunk);
            }
            peer.update(
                json!({"sessionUpdate": "available_commands_update", "availableCommands": []}),
            );
            peer.update(json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "t1",
                "title": "Read notes",
                "kind": "read",
                "status": "pending",
                "locations": [{"path": "/tmp/notes.txt", "line": 3}],
            }));
            peer.update(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "t1",
                "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": "3 lines"}}],
            }));
            peer.update(json!({
                "sessionUpdate": "plan",
                "entries": [
                    {"content": "Read notes", "priority": "high", "status": "completed"},
                    {"content": "Summarise", "priority": "medium", "status": "pending"},
                ],
            }));
            peer.raw("this is not json");
            peer.update(json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": {"type": "text", "text": "done"},
            }));
            "end_turn"
        }
        "permission" => {
            let result = peer.call(
                "session/request_permission",
                json!({
                    "sessionId": "sess-1",
                    "toolCall": {
                        "toolCallId": "t9",
                        "title": "Edit main.rs",
                        "kind": "edit",
                        "locations": [{"path": "/work/main.rs"}],
                        "rawInput": {"path": "/work/main.rs"},
                    },
                    "options": [
                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "always", "name": "Always allow", "kind": "allow_always"},
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                    ],
                }),
            );
            let outcome = result.expect("permission reply")["outcome"].clone();
            let chose = if outcome["outcome"] == "selected" {
                outcome["optionId"].as_str().unwrap_or("?").to_owned()
            } else {
                outcome["outcome"].as_str().unwrap_or("?").to_owned()
            };
            peer.say(&format!("chose:{chose}"));
            if peer.cancelled {
                "cancelled"
            } else {
                "end_turn"
            }
        }
        "fs" => {
            let ops: Vec<Value> =
                serde_json::from_str(&std::env::var("FAKE_ACP_FS_OPS").unwrap()).unwrap();
            for op in ops {
                let result = if let Some(path) = op.get("write") {
                    peer.call(
                        "fs/write_text_file",
                        json!({"sessionId": "sess-1", "path": path, "content": op["content"]}),
                    )
                    .map(|_| String::new())
                } else if let Some(path) = op.get("read") {
                    peer.call(
                        "fs/read_text_file",
                        json!({"sessionId": "sess-1", "path": path, "line": op.get("line"), "limit": op.get("limit")}),
                    )
                    .map(|r| r["content"].as_str().unwrap_or_default().to_owned())
                } else {
                    peer.call(op["call"].as_str().unwrap(), json!({"sessionId": "sess-1"}))
                        .map(|_| String::new())
                };
                match result {
                    Ok(content) => peer.say(&format!("ok:{}\n", content.trim_end())),
                    Err(code) => peer.say(&format!("err:{code}\n")),
                }
            }
            "end_turn"
        }
        "cancel" => {
            peer.say("working");
            while !peer.cancelled {
                if peer.recv().is_none() {
                    std::process::exit(0);
                }
            }
            peer.update(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "t1",
                "status": "failed",
            }));
            "cancelled"
        }
        "exit" => {
            peer.say("bye");
            std::process::exit(3);
        }
        "grandchild" => {
            let heartbeat = std::env::var("FAKE_ACP_HEARTBEAT").unwrap();
            // Deliberately left running: the test proves Stop retires it.
            #[allow(clippy::zombie_processes)]
            std::process::Command::new(std::env::current_exe().unwrap())
                .env("FAKE_ACP_SCENARIO", "heartbeat")
                .env("FAKE_ACP_HEARTBEAT", heartbeat)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            peer.say("spawned");
            while !peer.cancelled {
                if peer.recv().is_none() {
                    std::process::exit(0);
                }
            }
            "cancelled"
        }
        other => panic!("unknown scenario {other}"),
    }
}
