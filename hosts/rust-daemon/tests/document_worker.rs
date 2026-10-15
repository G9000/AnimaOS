//! Physical CLI entry acceptance, without starting the daemon or any services.
#![cfg(windows)]

use anima_core::document_content::{validate_document_extraction, ExtractedDocumentText};
use process_wrap::tokio::{CommandWrap, CreationFlags, JobObject, KillOnDrop};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "root runs physical Windows DOCX CLI acceptance explicitly"]
async fn document_worker_cli_reads_selected_literal_before_any_host_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_anima-daemon"));
    command
        .arg("--document-worker-v1")
        .env_clear()
        .current_dir(directory.path())
        // A normal daemon would reject this configuration before binding.
        .env("ANIMAOS_RS_MAX_REQUEST_BYTES", "invalid")
        .env("TEMP", directory.path())
        .env("TMP", directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut wrapped = CommandWrap::from(command);
    wrapped.wrap(KillOnDrop);
    let mut flags = CreationFlags(Default::default());
    flags.0 .0 = 0x08000000;
    wrapped.wrap(flags).wrap(JobObject);
    let mut child = wrapped.spawn().unwrap();
    let mut stdin = child.stdin().take().unwrap();
    let mut stdout = child.stdout().take().unwrap();
    let mut stderr = child.stderr().take().unwrap();
    let mut pipes = tokio::spawn(async move {
        let mut ready = [0; 20];
        // The exact READY string has 20 bytes including the newline.
        stdout.read_exact(&mut ready).await.unwrap();
        assert_eq!(&ready, b"ANIMA-DOCX-READY-v1\n");
        let docx = include_bytes!("fixtures/documents/m54-selected.docx");
        stdin
            .write_all(&(docx.len() as u32).to_le_bytes())
            .await
            .unwrap();
        stdin.write_all(docx).await.unwrap();
        stdin.shutdown().await.unwrap();
        drop(stdin);
        let mut length = [0; 4];
        stdout.read_exact(&mut length).await.unwrap();
        let length = u32::from_le_bytes(length) as usize;
        assert!(length <= 64 * 1024 - ready.len() - 4);
        let mut json = vec![0; length];
        stdout.read_exact(&mut json).await.unwrap();
        let mut extra = [0; 1];
        assert_eq!(stdout.read(&mut extra).await.unwrap(), 0);
        let extraction: ExtractedDocumentText = serde_json::from_slice(&json).unwrap();
        validate_document_extraction(&extraction).unwrap();
        let text = extraction
            .blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("M54 selected record: cobalt harbour 7319."));
        assert!(!text.contains("HEADER_OMITTED_4096"));
        assert!(!text.contains("PHONETIC_GUIDE_NOT_BODY"));
        let mut errors = [0; 1];
        assert_eq!(
            stderr.read(&mut errors).await.unwrap(),
            0,
            "worker tracing must not emit document contents"
        );
    });
    let result = tokio::time::timeout(Duration::from_secs(5), &mut pipes).await;
    // Even failed assertions/timeouts cannot abandon an owned worker tree.
    child.start_kill().unwrap();
    child.wait().await.unwrap();
    if result.is_err() {
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut pipes).await;
    }
    result
        .expect("bounded CLI exchange")
        .expect("CLI framing and selected text");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}
