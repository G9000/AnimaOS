use std::io;

use anima_daemon::{run_from_env, ServeOptions};

fn main() -> io::Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .iter()
        .any(|a| a.to_string_lossy().starts_with("--execution-worker-"))
    {
        if arguments.len() != 1 || arguments[0] != "--execution-worker-v1" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution worker requires exactly --execution-worker-v1",
            ));
        }
        return anima_daemon::execution_worker_entry();
    }
    if arguments
        .iter()
        .any(|argument| argument.to_string_lossy().starts_with("--document-worker-"))
    {
        if arguments.len() != 1 || arguments[0] != "--document-worker-v2" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "document worker requires exactly --document-worker-v2",
            ));
        }
        return anima_daemon::document_worker_entry();
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_from_env(ServeOptions::default()))
}
