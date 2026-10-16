//! Physical CLI entry acceptance, without starting the daemon or any services.
#![cfg(windows)]

use anima_core::document_content::{
    validate_document_extraction, DocumentFormat, ExtractedDocumentText, XlsxSheetVisibility,
};
use process_wrap::tokio::{CommandWrap, CreationFlags, JobObject, KillOnDrop};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "root runs physical Windows DOCX/PDF CLI acceptance explicitly"]
async fn document_worker_cli_reads_selected_literal_before_any_host_initialization() {
    for (format, bytes) in [
        (
            DocumentFormat::Docx,
            include_bytes!("fixtures/documents/m54-selected.docx").as_slice(),
        ),
        (
            DocumentFormat::Pdf,
            include_bytes!("fixtures/documents/m55-selected.pdf").as_slice(),
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_anima-daemon"));
        command
            .arg("--document-worker-v2")
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
            let mut ready = [0; b"ANIMA-DOCUMENT-READY-v2\n".len()];
            stdout.read_exact(&mut ready).await.unwrap();
            assert_eq!(&ready, b"ANIMA-DOCUMENT-READY-v2\n");
            stdin
                .write_u8(match format {
                    DocumentFormat::Docx => 0,
                    DocumentFormat::Pdf => 1,
                    DocumentFormat::Xlsx => 2,
                })
                .await
                .unwrap();
            stdin
                .write_all(&(bytes.len() as u32).to_le_bytes())
                .await
                .unwrap();
            stdin.write_all(bytes).await.unwrap();
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
            assert_eq!(extraction.version, 2);
            assert_eq!(extraction.format, format);
            let text = extraction
                .blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            match format {
                DocumentFormat::Docx => {
                    assert!(text.contains("M54 selected record: cobalt harbour 7319."));
                    assert!(!text.contains("HEADER_OMITTED_4096"));
                    assert!(!text.contains("PHONETIC_GUIDE_NOT_BODY"));
                }
                DocumentFormat::Pdf => {
                    assert!(text.contains("M55 selected invoice total: USD 137.42."));
                    let coverage = extraction.pdf_coverage.unwrap();
                    assert_eq!(coverage.page_count, 2);
                    assert_eq!(coverage.text_page_count, 1);
                    assert_eq!(coverage.included_page_count, 1);
                }
                DocumentFormat::Xlsx => unreachable!("legacy DOCX/PDF case list only"),
            }
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
}

#[tokio::test]
#[ignore = "root runs finite Windows M66 XLSX CLI acceptance explicitly"]
async fn m66_document_worker_cli_reads_selected_xlsx_before_host_initialization() {
    let bytes = include_bytes!("fixtures/documents/m66-selected.xlsx").as_slice();
    let directory = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_anima-daemon"));
    command
        .arg("--document-worker-v2")
        .env_clear()
        .current_dir(directory.path())
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
        let mut ready = [0; b"ANIMA-DOCUMENT-READY-v2\n".len()];
        stdout.read_exact(&mut ready).await.unwrap();
        assert_eq!(&ready, b"ANIMA-DOCUMENT-READY-v2\n");
        stdin.write_u8(2).await.unwrap();
        stdin
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .await
            .unwrap();
        stdin.write_all(bytes).await.unwrap();
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
        assert_eq!(extraction.version, 2);
        assert_eq!(extraction.format, DocumentFormat::Xlsx);
        let text = extraction
            .blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("M66 selected ledger"));
        assert!(text.contains("137.42"));
        assert!(text.contains("formula (not evaluated)=SUM(A2,2)"));
        assert!(text.contains("cached (possibly stale)=139.42"));
        assert!(text.contains("Hidden source marker: cobalt 7319"));
        let coverage = extraction.xlsx_coverage.unwrap();
        assert_eq!(coverage.sheet_count, 2);
        assert_eq!(coverage.observed_cell_count, 5);
        assert_eq!(coverage.included_cell_count, 5);
        assert_eq!(coverage.sheets[0].visibility, XlsxSheetVisibility::Visible);
        assert_eq!(coverage.sheets[1].visibility, XlsxSheetVisibility::Hidden);
        let mut errors = [0; 1];
        assert_eq!(
            stderr.read(&mut errors).await.unwrap(),
            0,
            "worker tracing must not emit document contents"
        );
    });
    let result = tokio::time::timeout(Duration::from_secs(5), &mut pipes).await;
    child.start_kill().unwrap();
    child.wait().await.unwrap();
    if result.is_err() {
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut pipes).await;
    }
    result
        .expect("bounded XLSX CLI exchange")
        .expect("M66 synthetic workbook extracted");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[tokio::test]
#[ignore = "root runs finite Windows private CLI rejection acceptance explicitly"]
async fn document_worker_cli_rejects_every_other_private_mode_before_host_initialization() {
    for arguments in [
        vec!["--document-worker-v1"],
        vec!["--document-worker-v3"],
        vec!["--document-worker-"],
        vec!["--document-worker-v2", "extra"],
        vec!["ignored", "--document-worker-v1"],
        vec!["ignored", "--document-worker-v2"],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_anima-daemon"));
        command
            .args(&arguments)
            .env_clear()
            .current_dir(directory.path())
            .env("ANIMAOS_RS_MAX_REQUEST_BYTES", "invalid")
            .stdin(Stdio::null())
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
        let stdout = child.stdout().take().unwrap();
        let stderr = child.stderr().take().unwrap();
        let mut pipes = tokio::spawn(async move {
            let mut output = Vec::new();
            stdout
                .take(64 * 1024 + 1)
                .read_to_end(&mut output)
                .await
                .unwrap();
            assert!(output.is_empty());
            let mut errors = Vec::new();
            stderr.take(4097).read_to_end(&mut errors).await.unwrap();
            assert!(errors.len() <= 4096);
            let errors = String::from_utf8(errors).unwrap();
            assert!(errors.contains("document worker requires exactly --document-worker-v2"));
            assert!(!errors.contains("ANIMAOS_RS_MAX_REQUEST_BYTES"));
        });
        let result = tokio::time::timeout(Duration::from_secs(5), &mut pipes).await;
        child.start_kill().unwrap();
        child.wait().await.unwrap();
        if result.is_err() {
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut pipes).await;
        }
        result
            .expect("bounded private CLI rejection")
            .expect("private mode rejected before host config");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
