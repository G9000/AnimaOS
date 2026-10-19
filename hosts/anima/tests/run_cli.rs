//! Spawns the real `anima run` binary against a temporary HOME.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_status(port: u16, path: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response.split_whitespace().nth(1)?.parse().ok()
}

fn forward_lines(stream: impl Read + Send + 'static, tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

#[test]
fn anima_run_serves_health_with_temp_home() {
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_anima"))
        .arg("run")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("ANIMAOS_RS_PORT", port.to_string())
        .env("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "test")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    forward_lines(child.stdout.take().unwrap(), tx.clone());
    forward_lines(child.stderr.take().unwrap(), tx);
    let _guard = KillOnDrop(child);

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut log = Vec::new();
    loop {
        if http_status(port, "/health") == Some(200) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "anima run never served /health; output so far:\n{}",
            log.join("\n")
        );
        while let Ok(line) = rx.try_recv() {
            log.push(line);
        }
        thread::sleep(Duration::from_millis(100));
    }
    while let Ok(line) = rx.recv_timeout(Duration::from_millis(300)) {
        log.push(line);
    }

    let startup = log
        .iter()
        .find(|line| line.contains("control_plane_durability"))
        .unwrap_or_else(|| panic!("no startup line in:\n{}", log.join("\n")));
    let store = startup
        .split("json:")
        .nth(1)
        .unwrap_or_else(|| panic!("no json: store in {startup}"))
        .split(['"', ' '])
        .next()
        .unwrap();
    let expected = std::path::Path::new(".animaos-swarm")
        .join("data")
        .join("control-plane.json");
    assert!(
        std::path::Path::new(store).ends_with(&expected),
        "store {store} should end with {expected:?}"
    );
    assert!(
        std::path::Path::new(store).starts_with(home.path()),
        "store {store} should live under the temp home"
    );
}

#[test]
fn unimplemented_commands_exit_with_status_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_anima"))
        .arg("setup")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not yet available"));
}
