//! Fixed Rust SSH worker. Untrusted command text is framed JSON, never a remote shell argument.
use super::{process, ExecutionReceipt, ExecutionStatus};
use anima_core::CancelSignal;
use hmac::{Hmac, KeyInit, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) const MAX_FRAME_BYTES: usize = 131_072;
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum WorkerRequest {
    Run {
        version: u32,
        #[serde(rename = "executionId")]
        execution_id: String,
        command: String,
        cwd: String,
        #[serde(rename = "timeoutMs")]
        timeout_ms: u64,
        #[serde(default, rename = "targetId")]
        target_id: String,
        #[serde(default, rename = "receiptKey")]
        receipt_key: String,
    },
    Status {
        version: u32,
        #[serde(rename = "executionId")]
        execution_id: String,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum WorkerResponse {
    Accepted {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
    Terminal {
        receipt: ExecutionReceipt,
        proof: String,
    },
    OutcomeUnknown {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
}
pub(crate) fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let size = u32::from_be_bytes(length) as usize;
    if size == 0 || size > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Execution frame length is invalid",
        ));
    }
    let mut data = vec![0; size];
    reader.read_exact(&mut data)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}
pub(crate) fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    if data.is_empty() || data.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Execution frame too large",
        ));
    }
    writer.write_all(&(data.len() as u32).to_be_bytes())?;
    writer.write_all(&data)?;
    writer.flush()
}
pub(super) fn proof(receipt: &ExecutionReceipt, key: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key");
    mac.update(&serde_json::to_vec(receipt).unwrap());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(super) fn verify(receipt: &ExecutionReceipt, key: &str, value: &str) -> bool {
    if value.len() != 64 {
        return false;
    }
    let mut bytes = Vec::with_capacity(32);
    for chunk in value.as_bytes().chunks_exact(2) {
        let Ok(s) = std::str::from_utf8(chunk) else {
            return false;
        };
        let Ok(byte) = u8::from_str_radix(s, 16) else {
            return false;
        };
        bytes.push(byte);
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key");
    mac.update(&serde_json::to_vec(receipt).unwrap());
    mac.verify_slice(&bytes).is_ok()
}
fn configured_directory(name: &str) -> io::Result<PathBuf> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other(format!("{name} must be operator-configured")))?;
    if !path.is_absolute() || !path.is_dir() {
        return Err(io::Error::other(format!(
            "{name} must be an existing absolute directory"
        )));
    }
    fs::canonicalize(path)
}
fn identity(id: &str) -> io::Result<()> {
    if uuid::Uuid::parse_str(id).is_err() {
        Err(io::Error::other("Invalid execution identity"))
    } else {
        Ok(())
    }
}
fn terminal_path(state: &Path, id: &str) -> PathBuf {
    state.join(format!("{id}.terminal.json"))
}
fn save_terminal(state: &Path, id: &str, response: &WorkerResponse) -> io::Result<()> {
    let path = terminal_path(state, id);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer(&mut file, response).map_err(io::Error::other)?;
    file.sync_all()
}

/// Called before daemon startup or clap. The worker accepts one framed operation.
pub fn execution_worker_entry() -> io::Result<()> {
    let request: WorkerRequest = read_frame(&mut io::stdin().lock())?;
    let root = configured_directory("ANIMAOS_EXECUTION_WORKER_ROOT")?;
    let state = configured_directory("ANIMAOS_EXECUTION_WORKER_STATE_DIR")?;
    if state.starts_with(&root) || root.starts_with(&state) {
        return Err(io::Error::other(
            "Execution worker state must be outside its workspace",
        ));
    }
    match request {
        WorkerRequest::Status {
            version,
            execution_id,
        } => {
            identity(&execution_id)?;
            if version != 1 {
                return Err(io::Error::other("Unsupported worker protocol"));
            }
            let response = fs::read(terminal_path(&state, &execution_id))
                .ok()
                .filter(|d| d.len() <= MAX_FRAME_BYTES)
                .and_then(|d| serde_json::from_slice::<WorkerResponse>(&d).ok())
                .unwrap_or(WorkerResponse::OutcomeUnknown { execution_id });
            write_frame(&mut io::stdout().lock(), &response)
        }
        WorkerRequest::Run {
            version,
            execution_id,
            command,
            cwd,
            timeout_ms,
            target_id,
            receipt_key,
        } => {
            identity(&execution_id)?;
            if version != 1
                || command.trim().is_empty()
                || command.len() > 16_384
                || command.contains('\0')
                || cwd.len() > 1024
                || !(1..=300_000).contains(&timeout_ms)
                || !super::config::safe_id(&target_id)
                || receipt_key.len() != 64
                || !receipt_key.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(io::Error::other("Invalid execution worker request"));
            }
            let cwd = process::local_cwd(&root, &cwd).map_err(io::Error::other)?;
            if terminal_path(&state, &execution_id).exists() {
                return Err(io::Error::other("Execution identity has already been used"));
            }
            // A durable active marker makes restart uncertainty fail closed. The
            // file is removed only after a terminal receipt is synced.
            let active = state.join("active.json");
            let mut marker = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&active)?;
            marker.write_all(execution_id.as_bytes())?;
            marker.sync_all()?;
            write_frame(
                &mut io::stdout().lock(),
                &WorkerResponse::Accepted {
                    execution_id: execution_id.clone(),
                },
            )?;
            let cancel = CancelSignal::new();
            let eof = cancel.clone();
            std::thread::spawn(move || {
                let mut byte = [0; 1];
                let _ = io::stdin().read(&mut byte);
                eof.cancel();
            });
            #[cfg(windows)]
            let (executable, args) = (
                PathBuf::from(
                    std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
                )
                .join("System32")
                .join("cmd.exe"),
                vec!["/d".into(), "/s".into(), "/c".into(), command],
            );
            #[cfg(not(windows))]
            let (executable, args) = (PathBuf::from("/bin/sh"), vec!["-c".into(), command]);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;
            let output = runtime.block_on(process::run(
                &executable,
                args,
                Some(&cwd),
                None,
                Duration::from_millis(timeout_ms),
                Some(&cancel),
                &CancelSignal::new(),
                true,
            ));
            let mut receipt = ExecutionReceipt::unknown(
                &execution_id,
                &target_id,
                "ssh",
                "Worker child cleanup is unproven",
            );
            match output {
                Ok(output) => {
                    receipt.stdout = output.stdout;
                    receipt.stderr = output.stderr;
                    receipt.exit_code = output.exit_code;
                    receipt.cleanup_confirmed = output.cleanup;
                    if output.cleanup {
                        receipt.status = output.stop.unwrap_or(if output.exit_code == Some(0) {
                            ExecutionStatus::Succeeded
                        } else {
                            ExecutionStatus::Failed
                        });
                        receipt.message = "Remote worker child custody retired".into();
                    }
                }
                Err(error) => {
                    receipt.status = ExecutionStatus::Failed;
                    receipt.cleanup_confirmed = true;
                    receipt.message = error;
                }
            }
            if !receipt.cleanup_confirmed {
                return write_frame(
                    &mut io::stdout().lock(),
                    &WorkerResponse::OutcomeUnknown { execution_id },
                );
            }
            let response = WorkerResponse::Terminal {
                proof: proof(&receipt, &receipt_key),
                receipt,
            };
            save_terminal(&state, &execution_id, &response)?;
            fs::remove_file(active)?;
            write_frame(&mut io::stdout().lock(), &response)
        }
    }
}
