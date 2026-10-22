//! Controlled subprocess peer. This fixture never invokes Docker, SSH, or a shell.
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--execution-worker-v1"] {
        anima_daemon::execution_worker_entry().expect("real Rust worker fixture entry");
        return;
    }
    let root = std::env::var_os("ANIMA_EXECUTION_FAKE_ROOT")
        .map(PathBuf::from)
        .expect("fixture root");
    fs::create_dir_all(&root).unwrap();
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("calls.jsonl"))
        .unwrap();
    writeln!(log, "{}", serde_json::to_string(&args).unwrap()).unwrap();
    let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    match args.first().map(String::as_str) {
        Some("image") => {
            if root.join("declared-volume").exists() {
                println!("{{\"/data\":{{}}}}")
            } else {
                println!("null")
            }
        }
        Some("create") => {
            let label = args
                .windows(2)
                .find(|w| w[0] == "--label")
                .map(|w| w[1].clone())
                .unwrap();
            fs::write(root.join("label"), label).unwrap();
            fs::write(root.join("exists"), id).unwrap();
            println!("{id}");
        }
        Some("inspect") => {
            if !root.join("exists").exists() {
                std::process::exit(1)
            }
            println!(
                "{}",
                fs::read_to_string(root.join("label"))
                    .unwrap()
                    .split_once('=')
                    .unwrap()
                    .1
            );
        }
        Some("start") => {
            println!("fixture-output");
            if root.join("hold").exists() {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
        Some("wait") => println!("0"),
        Some("rm") => {
            if root.join("refuse-removal").exists() {
                std::process::exit(1)
            }
            let _ = fs::remove_file(root.join("exists"));
            println!("{id}");
        }
        Some("ps") => {
            if root.join("exists").exists() {
                println!("{id}");
            }
        }
        _ => {
            // A fixed SSH worker command receives only a bounded frame on stdin.
            let mut length = [0; 4];
            io::stdin().read_exact(&mut length).unwrap();
            let mut data = vec![0; u32::from_be_bytes(length) as usize];
            io::stdin().read_exact(&mut data).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&data).unwrap();
            if request["operation"] == "status" && root.join("allow-status").exists() {
                let original: serde_json::Value =
                    serde_json::from_slice(&fs::read(root.join("worker-request.json")).unwrap())
                        .unwrap();
                #[derive(serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Receipt {
                    execution_id: String,
                    target_id: String,
                    backend: String,
                    status: String,
                    exit_code: Option<i32>,
                    stdout: String,
                    stderr: String,
                    cleanup_confirmed: bool,
                    message: String,
                }
                let receipt = Receipt {
                    execution_id: original["executionId"].as_str().unwrap().into(),
                    target_id: original["targetId"].as_str().unwrap().into(),
                    backend: "ssh".into(),
                    status: "cancelled".into(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    cleanup_confirmed: true,
                    message: "Controlled terminal reconciliation".into(),
                };
                use hmac::{Hmac, KeyInit, Mac};
                let mut mac = Hmac::<sha2::Sha256>::new_from_slice(
                    original["receiptKey"].as_str().unwrap().as_bytes(),
                )
                .unwrap();
                mac.update(&serde_json::to_vec(&receipt).unwrap());
                let proof: String = mac
                    .finalize()
                    .into_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let response = serde_json::to_vec(
                    &serde_json::json!({"type":"terminal","receipt":receipt,"proof":proof}),
                )
                .unwrap();
                io::stdout()
                    .write_all(&(response.len() as u32).to_be_bytes())
                    .unwrap();
                io::stdout().write_all(&response).unwrap();
                return;
            }
            if request["operation"] == "run" {
                fs::write(root.join("worker-request.json"), data).unwrap();
            }
            // Simulate a dropped connection after dispatch; no terminal evidence.
            std::process::exit(255);
        }
    }
}
