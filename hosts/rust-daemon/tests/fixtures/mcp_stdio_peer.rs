//! Deterministic native MCP peer for owned-stdio tests. Never a runtime server.
//! Root builds this example once; no nested compiler, shell, provider or account.
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

fn emit(value: &Value, split: bool) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    if split {
        for bytes in serde_json::to_vec(value)?.chunks(7) {
            stdout.write_all(bytes)?;
            stdout.flush()?;
            std::thread::sleep(Duration::from_millis(1));
        }
    } else {
        serde_json::to_writer(&mut stdout, value)?;
    }
    stdout.write_all(b"\n")?;
    stdout.flush()
}
fn hold(heartbeat: Option<&PathBuf>) -> ! {
    let mut ticks = 0u64;
    loop {
        if let Some(path) = heartbeat {
            ticks += 1;
            let _ = std::fs::write(path, ticks.to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn marker(
    path: Option<&PathBuf>,
    child: Option<u32>,
    counts: &BTreeMap<String, usize>,
) -> io::Result<()> {
    if let Some(path) = path {
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "parentPid":std::process::id(),"childPid":child,"counts":counts,
                "ambientEnvironmentPresent":(["PATH","HOME","USERPROFILE","CODEX_HOME","OPENAI_API_KEY","ANTHROPIC_API_KEY"]
                    .iter().any(|name|std::env::var_os(name).is_some())),
                "inheritedCwd":std::env::current_dir()?.join("AGENTS.md").exists()
            }))?,
        )?;
    }
    Ok(())
}
fn run() -> io::Result<()> {
    let mut mode = "normal".to_owned();
    let mut marker_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                mode = args
                    .next()
                    .ok_or_else(|| io::Error::other("fixture mode missing"))?
            }
            "--marker" => {
                marker_path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| io::Error::other("fixture marker missing"))?,
                ))
            }
            _ => return Err(io::Error::other("fixture argument invalid")),
        }
    }
    if mode == "held-child" {
        hold(marker_path.as_ref());
    }
    if ![
        "normal",
        "blocked-init",
        "blocked-call",
        "stderr-flood",
        "oversized-line",
        "no-newline",
        "malformed",
        "idle-flood",
        "aggregate-discovery",
        "split-frames",
        "exit-before-init",
        "descendant-held-pipes",
        "wrong-protocol",
    ]
    .contains(&mode.as_str())
    {
        return Err(io::Error::other("fixture mode invalid"));
    }
    let child = if mode == "descendant-held-pipes" {
        let heartbeat = marker_path
            .as_ref()
            .map(|path| path.with_extension("heartbeat"))
            .ok_or_else(|| io::Error::other("fixture heartbeat missing"))?;
        let child = Command::new(std::env::current_exe()?)
            .args(["--mode", "held-child", "--marker"])
            .arg(&heartbeat)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        // Establish actual descendant activity before the parent serves MCP.
        for _ in 0..100 {
            if heartbeat.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Some(child)
    } else {
        None
    };
    let child_pid = child.as_ref().map(std::process::Child::id);
    let mut counts = BTreeMap::new();
    marker(marker_path.as_ref(), child_pid, &counts)?;
    if mode == "exit-before-init" {
        return Ok(());
    }
    if mode == "blocked-init" {
        hold(None);
    }
    if mode == "stderr-flood" {
        let mut stderr = io::stderr().lock();
        for _ in 0..256 {
            stderr.write_all(&[b'e'; 8192])?;
        }
        stderr.flush()?;
    }
    if mode == "oversized-line" || mode == "no-newline" {
        let mut stdout = io::stdout().lock();
        stdout.write_all(&vec![b'x'; 65537])?;
        if mode == "oversized-line" {
            stdout.write_all(b"\n")?;
        }
        stdout.flush()?;
        drop(stdout);
        hold(None);
    }
    if mode == "malformed" {
        io::stdout().write_all(b"{\"jsonrpc\":broken,\"secret\":\"synthetic-sensitive\"}\n")?;
        io::stdout().flush()?;
        hold(None);
    }
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let method = request["method"]
            .as_str()
            .ok_or_else(|| io::Error::other("fixture method missing"))?;
        if ![
            "initialize",
            "notifications/initialized",
            "notifications/cancelled",
            "tools/list",
            "tools/call",
        ]
        .contains(&method)
        {
            if let Some(id) = request.get("id") {
                emit(
                    &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported synthetic method"}}),
                    mode == "split-frames",
                )?;
            }
            continue;
        }
        *counts.entry(method.to_owned()).or_insert(0) += 1;
        marker(marker_path.as_ref(), child_pid, &counts)?;
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":if mode=="wrong-protocol" {"2024-11-05"} else {"2025-11-25"},
                "capabilities":{"tools":{}},"serverInfo":{"name":"Synthetic stdio peer","version":"1"}})
            }
            "tools/list" => {
                json!({"tools":[{"name":"synthetic_stdio_record","description":"A synthetic local record",
                "inputSchema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}}]})
            }
            "tools/call" => {
                if mode == "blocked-call" {
                    hold(None);
                }
                json!({"content":[{"type":"text","text":"Synthetic stdio violet record"}],"isError":false})
            }
            _ => continue,
        };
        if method == "tools/list" && mode == "aggregate-discovery" {
            let notice = json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"});
            for _ in 0..6000 {
                emit(&notice, false)?;
            }
        }
        emit(
            &json!({"jsonrpc":"2.0","id":id,"result":result}),
            mode == "split-frames",
        )?;
        if method == "tools/list" && mode == "descendant-held-pipes" {
            // Leader exits while the descendant still owns both output pipes.
            // Dropping std::process::Child does not kill it: the host must own
            // group/job termination and reader completion to prove cleanup.
            return Ok(());
        }
        if method == "tools/list" && mode == "idle-flood" {
            let notice = json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"});
            for _ in 0..6000 {
                emit(&notice, false)?;
            }
            hold(None);
        }
    }
    Ok(())
}
fn main() {
    // Static exit only: raw errors/argv/paths never appear on protocol stdout.
    if run().is_err() {
        std::process::exit(2);
    }
}
