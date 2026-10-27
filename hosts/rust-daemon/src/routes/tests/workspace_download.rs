use super::*;

fn owner_get(path: &str) -> Request<Body> {
    crate::routes::owner_test_support::authenticate(
        Request::builder()
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200"),
    )
    .body(Body::empty())
    .unwrap()
}

#[tokio::test]
async fn m68_owner_download_returns_exact_report_bytes_and_safe_headers() {
    let fixture = WorkspaceAvatarTemp::new("m68-report-download");
    let reports = fixture.root.join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    let file = reports.join("ledger.csv");
    let expected = "\u{feff}label,amount\r\n账本,137.42\r\n\"comma, quoted\",2\r\n".as_bytes();
    std::fs::write(&file, expected).unwrap();
    let app = router(fixture.state(), DaemonConfig::default());

    // Establish the existing owner/read path independently of the new route.
    let listed = app
        .clone()
        .oneshot(owner_get("/api/workspace/files"))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);

    let response = app
        .oneshot(owner_get(
            "/api/workspace/file/download?path=reports/ledger.csv",
        ))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&file).unwrap(),
        expected,
        "download never rewrites the source"
    );
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the authenticated route must return report bytes, not the missing-route 404"
    );
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.headers()["content-length"].to_str().unwrap(),
        expected.len().to_string()
    );
    let disposition = response.headers()["content-disposition"].to_str().unwrap();
    assert!(disposition.starts_with("attachment;"));
    assert!(disposition.contains("ledger.csv"));
    assert!(!disposition.contains(['/', '\\', '\r', '\n']));
    let bytes = to_bytes(response.into_body(), 1024 * 1024 + 1)
        .await
        .unwrap();
    assert_eq!(
        bytes.as_ref(),
        expected,
        "BOM, UTF-8, CRLF and CSV quoting must survive exactly"
    );
}

#[tokio::test]
async fn m68_download_query_and_path_refusals_are_not_cacheable() {
    let fixture = WorkspaceAvatarTemp::new("m68-query");
    std::fs::write(fixture.root.join("report.txt"), "ordinary report").unwrap();
    for name in ["report.pdf", "report.xlsx"] {
        std::fs::write(fixture.root.join(name), b"unsupported binary document").unwrap();
    }
    let app = router(fixture.state(), DaemonConfig::default());
    for query in [
        "",
        "?path=",
        "?path=report.txt&path=report.txt",
        "?path=report.txt&extra=1",
        "?path=%",
        "?path=%GG",
        "?path=%FF",
        "?path=/report.txt",
        "?path=../report.txt",
        "?path=./report.txt",
        "?path=reports//report.txt",
        "?path=reports%5Creport.txt",
        "?path=C%3Areport.txt",
        "?path=report.txt%3Astream",
        "?path=report%00.txt",
        "?path=.hidden/report.txt",
        "?path=credentials/report.txt",
        "?path=report.pdf",
        "?path=report.xlsx",
    ] {
        let response = app
            .clone()
            .oneshot(owner_get(&format!("/api/workspace/file/download{query}")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(response.headers()["cache-control"], "no-store", "{query}");
        assert!(!response.headers().contains_key("content-disposition"));
    }
    let mut unauthorized = owner_get("/api/workspace/file/download?path=report.txt");
    unauthorized
        .headers_mut()
        .insert("origin", "https://untrusted.example".parse().unwrap());
    let response = app.clone().oneshot(unauthorized).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let response = app
        .oneshot(owner_get("/api/workspace/file/download?path=missing.txt"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let state = fixture.state();
    state.write().await.workspace = None;
    let response = router(state, DaemonConfig::default())
        .oneshot(owner_get("/api/workspace/file/download?path=report.txt"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["cache-control"], "no-store");
}

fn finished_document_spec(format: &str, text: &str) -> serde_json::Value {
    match format {
        "xlsx" => serde_json::json!({"format":format,"title":"Report","sheets":[{
            "name":"Summary","columns":["Note"],"rows":[[text]]
        }]}),
        "pptx" => serde_json::json!({"format":format,"title":"Report","slides":[{
            "title":"Summary","blocks":[{"type":"paragraph","text":text}]
        }]}),
        _ => serde_json::json!({"format":format,"title":"Report","blocks":[{
            "type":"paragraph","text":text
        }]}),
    }
}

fn render_finished(value: serde_json::Value) -> Vec<u8> {
    anima_document_content::render_document(&serde_json::from_value(value).unwrap())
        .unwrap()
        .bytes
}

#[tokio::test]
async fn finished_documents_owner_preview_and_download_verify_current_bytes() {
    use sha2::{Digest, Sha256};
    let fixture = WorkspaceAvatarTemp::new("finished-document-download");
    let app = router(fixture.state(), DaemonConfig::default());
    for format in ["docx", "xlsx", "pptx", "pdf"] {
        let path = format!("report.{format}");
        let expected = render_finished(finished_document_spec(format, "Original body"));
        std::fs::write(fixture.root.join(&path), &expected).unwrap();
        let preview = app
            .clone()
            .oneshot(owner_get(&format!("/api/workspace/file?path={path}")))
            .await
            .unwrap();
        assert_eq!(preview.status(), StatusCode::OK, "{format}");
        let preview: serde_json::Value =
            serde_json::from_slice(&to_bytes(preview.into_body(), 128 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(preview["path"], path);
        assert_eq!(preview["previewKind"], "document");
        assert_eq!(preview["truncated"], false);
        assert_eq!(preview["sha256"], hex::encode(Sha256::digest(&expected)));
        assert!(preview["content"]
            .as_str()
            .unwrap()
            .contains("Original body"));
        let response = app
            .clone()
            .oneshot(owner_get(&format!(
                "/api/workspace/file/download?path={path}"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{format}");
        assert_eq!(
            response.headers()["content-type"],
            "application/octet-stream"
        );
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            response.headers()["content-length"].to_str().unwrap(),
            expected.len().to_string()
        );
        assert!(response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains(&path));
        assert_eq!(
            to_bytes(response.into_body(), 4 * 1024 * 1024 + 1)
                .await
                .unwrap()
                .as_ref(),
            expected
        );
        let mut tampered = expected;
        tampered[0] ^= 1;
        std::fs::write(fixture.root.join(&path), tampered).unwrap();
        for endpoint in ["/api/workspace/file", "/api/workspace/file/download"] {
            let response = app
                .clone()
                .oneshot(owner_get(&format!("{endpoint}?path={path}")))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{format} {endpoint}"
            );
            assert!(!response.headers().contains_key("content-disposition"));
        }
    }
}

#[tokio::test]
async fn finished_documents_scan_logical_source_before_markdown_escaping() {
    let fixture = WorkspaceAvatarTemp::new("finished-document-secret");
    let app = router(fixture.state(), DaemonConfig::default());
    let mut specs = ["docx", "xlsx", "pptx", "pdf"]
        .map(|format| finished_document_spec(format, "api_key = synthetic-fixture"))
        .to_vec();
    specs.push(
        serde_json::json!({"format":"docx","title":"Table","blocks":[{
            "type":"table","headers":["Value"],"rows":[["api_key: synthetic-fixture"]]
        }]}),
    );
    for (index, spec) in specs.into_iter().enumerate() {
        let path = format!("report-{index}.{}", spec["format"].as_str().unwrap());
        std::fs::write(fixture.root.join(&path), render_finished(spec)).unwrap();
        for endpoint in ["/api/workspace/file", "/api/workspace/file/download"] {
            let response = app
                .clone()
                .oneshot(owner_get(&format!("{endpoint}?path={path}")))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{path} {endpoint}"
            );
            let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("synthetic-fixture"));
        }
    }
}

#[tokio::test]
async fn finished_documents_refuse_untrusted_oversized_and_excluded_files() {
    let fixture = WorkspaceAvatarTemp::new("finished-document-boundaries");
    let bytes = render_finished(finished_document_spec("xlsx", "Visible body"));
    std::fs::write(fixture.root.join("report.xlsx"), &bytes).unwrap();
    std::fs::create_dir(fixture.root.join(".hidden")).unwrap();
    std::fs::write(fixture.root.join(".hidden/report.xlsx"), &bytes).unwrap();
    std::fs::write(fixture.root.join("api_key.xlsx"), &bytes).unwrap();
    std::fs::write(
        fixture.root.join("oversized.xlsx"),
        vec![b'a'; 4 * 1024 * 1024 + 1],
    )
    .unwrap();
    let app = router(fixture.state(), DaemonConfig::default());
    for endpoint in ["/api/workspace/file", "/api/workspace/file/download"] {
        let mut request = owner_get(&format!("{endpoint}?path=report.xlsx"));
        request
            .headers_mut()
            .insert("origin", "https://untrusted.example".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        for path in [".hidden/report.xlsx", "api_key.xlsx", "oversized.xlsx"] {
            let response = app
                .clone()
                .oneshot(owner_get(&format!("{endpoint}?path={path}")))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{path} {endpoint}"
            );
            assert!(!response.headers().contains_key("content-disposition"));
        }
    }
}

#[cfg(windows)]
#[tokio::test]
async fn finished_documents_refuse_windows_junction_ancestors() {
    let fixture = WorkspaceAvatarTemp::new("finished-document-junction");
    let outside = tempfile::tempdir().unwrap();
    let bytes = render_finished(finished_document_spec("xlsx", "Outside document"));
    std::fs::write(outside.path().join("report.xlsx"), &bytes).unwrap();
    let alias = fixture.root.join("reports");
    let junction = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&alias)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "{}",
        String::from_utf8_lossy(&junction.stderr)
    );
    let app = router(fixture.state(), DaemonConfig::default());
    for endpoint in ["/api/workspace/file", "/api/workspace/file/download"] {
        let response = app
            .clone()
            .oneshot(owner_get(&format!("{endpoint}?path=reports/report.xlsx")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    std::fs::remove_dir(alias).unwrap();
    assert_eq!(
        std::fs::read(outside.path().join("report.xlsx")).unwrap(),
        bytes
    );
}

#[tokio::test]
async fn m68_download_decodes_once_preserves_empty_unicode_and_current_bytes() {
    let fixture = WorkspaceAvatarTemp::new("m68-current");
    std::fs::create_dir_all(fixture.root.join("reports")).unwrap();
    let app = router(fixture.state(), DaemonConfig::default());
    for (name, encoded, content) in [
        ("empty.TXT", "empty.TXT", ""),
        ("账本 report.MD", "%E8%B4%A6%E6%9C%AC%20report.MD", "旧内容"),
        (
            "%2e%2e.csv",
            "%252e%252e.csv",
            "literal percent filename\r\n",
        ),
    ] {
        std::fs::write(fixture.root.join("reports").join(name), content).unwrap();
        let response = app
            .clone()
            .oneshot(owner_get(&format!(
                "/api/workspace/file/download?path=reports/{encoded}"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert_eq!(
            response.headers()["content-length"].to_str().unwrap(),
            content.len().to_string()
        );
        let disposition = response.headers()["content-disposition"].to_str().unwrap();
        assert!(disposition.is_ascii());
        assert!(!disposition.contains(['/', '\\', '\r', '\n']));
        assert!(disposition.contains("filename*=UTF-8''"));
        assert_eq!(
            to_bytes(response.into_body(), 1024 * 1024 + 1)
                .await
                .unwrap()
                .as_ref(),
            content.as_bytes()
        );
    }
    let path = fixture.root.join("reports").join("账本 report.MD");
    let current = "\u{feff}updated at click\r\n";
    std::fs::write(&path, current).unwrap();
    let response = app
        .oneshot(owner_get(
            "/api/workspace/file/download?path=reports/%E8%B4%A6%E6%9C%AC%20report.MD",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024 * 1024 + 1)
            .await
            .unwrap()
            .as_ref(),
        current.as_bytes()
    );
    assert_eq!(std::fs::read(path).unwrap(), current.as_bytes());
}

#[tokio::test]
async fn m68_download_scans_the_full_snapshot_and_enforces_the_byte_boundary() {
    let fixture = WorkspaceAvatarTemp::new("m68-full-scan");
    let app = router(fixture.state(), DaemonConfig::default());
    let file = fixture.root.join("report.csv");
    let limit = crate::attachments::MAX_TEXT_BYTES;
    std::fs::write(&file, vec![b'a'; limit]).unwrap();
    let response = app
        .clone()
        .oneshot(owner_get("/api/workspace/file/download?path=report.csv"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), limit + 1)
            .await
            .unwrap()
            .len(),
        limit
    );
    let mut late_credential = vec![b'a'; 128 * 1024 + 16];
    late_credential.extend_from_slice(b"\npassword = m68-private-value");
    let mut late_control = vec![b'a'; 128 * 1024 + 16];
    late_control.push(0);
    for bytes in [
        vec![b'a'; limit + 1],
        late_credential,
        late_control,
        vec![0xff],
    ] {
        std::fs::write(&file, &bytes).unwrap();
        let response = app
            .clone()
            .oneshot(owner_get("/api/workspace/file/download?path=report.csv"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(!response.headers().contains_key("content-disposition"));
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("m68-private-value"));
        assert_eq!(
            std::fs::read(&file).unwrap(),
            bytes,
            "refusal must not alter bytes"
        );
    }
    std::fs::create_dir(fixture.root.join("directory.txt")).unwrap();
    let response = app
        .oneshot(owner_get("/api/workspace/file/download?path=directory.txt"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn m68_download_drop_while_waiting_for_state_admits_no_read() {
    let fixture = WorkspaceAvatarTemp::new("m68-drop");
    let state = fixture.state();
    std::fs::write(fixture.root.join("report.txt"), "unchanged").unwrap();
    let held = state.write().await;
    // The first await precedes the read-pool admission. Dropping this request
    // releases it without queuing work; M67 separately tests physical leases
    // surviving cancellation/drop once a blocking read has actually started.
    let pending = super::super::workspace_files::download(&state, "report.txt".into());
    assert!(tokio::time::timeout(Duration::from_millis(20), pending)
        .await
        .is_err());
    drop(held);
    let response = router(state, DaemonConfig::default())
        .oneshot(owner_get("/api/workspace/file/download?path=report.txt"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap().as_ref(),
        b"unchanged"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn m68_download_refuses_final_and_intermediate_symlinks() {
    let fixture = WorkspaceAvatarTemp::new("m68-links");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("report.txt"), "outside marker").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("report.txt"),
        fixture.root.join("report.txt"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), fixture.root.join("reports")).unwrap();
    let app = router(fixture.state(), DaemonConfig::default());
    for path in ["report.txt", "reports/report.txt"] {
        let response = app
            .clone()
            .oneshot(owner_get(&format!(
                "/api/workspace/file/download?path={path}"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    assert_eq!(
        std::fs::read(outside.path().join("report.txt")).unwrap(),
        b"outside marker"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn m68_download_refuses_windows_hidden_file_metadata() {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = WorkspaceAvatarTemp::new("m68-hidden");
    let file = fixture.root.join("report.txt");
    let mut hidden = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(0x2)
        .open(&file)
        .unwrap();
    hidden.write_all(b"hidden marker").unwrap();
    drop(hidden);
    let response = router(fixture.state(), DaemonConfig::default())
        .oneshot(owner_get("/api/workspace/file/download?path=report.txt"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(std::fs::read(file).unwrap(), b"hidden marker");
}

#[tokio::test]
async fn m68_normal_write_file_receipt_then_owner_files_and_download_are_exact() {
    use crate::agent_runs::test_support::{chat_request, lead_config, ScriptedModel, Step};
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    use crate::runs::{ActionStatus, RunStatus};
    use anima_core::{DataValue, MessageRole, ToolCall};
    use std::collections::BTreeMap;

    let private = tempfile::tempdir().unwrap();
    let root = private.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let report = "\u{feff}label,amount\r\n账本,137.42\r\n";
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![ToolCall {
            id: "m68-write-report".into(),
            name: "write_file".into(),
            args: BTreeMap::from([
                (
                    "file_path".into(),
                    DataValue::String("reports/ledger.csv".into()),
                ),
                ("content".into(), DataValue::String(report.into())),
            ]),
        }]),
        Step::Text(vec!["The report is saved in reports/ledger.csv."]),
    ]);
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.workspace = Some(WorkspaceConfig {
        root_path: root.clone(),
        company_name: "M68 synthetic".into(),
        mission: "Create and retrieve an ordinary report".into(),
        values: vec![],
    });
    let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
    daemon.set_control_plane_store(Some(store.clone()));
    let mut config = lead_config("M68 report companion");
    config.tools = Some(
        daemon
            .tool_registry
            .resolve_descriptors(["write_file"])
            .unwrap(),
    );
    config.settings.as_mut().unwrap().max_retries = Some(0);
    let agent = daemon.create_agent(config).unwrap().state.id;
    let state = Arc::new(RwLock::new(daemon));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    // Uses ordinary creation grants and the normal default allow-write policy;
    // no direct writer invocation or manually constructed success receipt.
    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        runs.run(chat_request(
            &agent,
            "chat:m68-report",
            "Write the ledger report.",
        )),
    )
    .await;
    runs.shutdown().await;
    outcome.unwrap().unwrap();
    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    let tool = requests[1]
        .messages
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap();
    assert!(tool.content.attachments.as_ref().is_none_or(Vec::is_empty));
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    let completed = {
        let state = state.read().await;
        let records = state.runs.for_session(&agent, "chat:m68-report");
        assert_eq!(records.len(), 1);
        (*records[0]).clone()
    };
    assert_eq!(completed.status, RunStatus::Completed);
    let persisted = saved
        .runs
        .iter()
        .find(|run| run.id == completed.id)
        .unwrap();
    assert_eq!(persisted.action_receipts.len(), 1);
    assert_eq!(persisted.action_receipts[0].name, "write_file");
    assert_eq!(persisted.action_receipts[0].status, ActionStatus::Succeeded);
    let before = std::fs::read(root.join("reports/ledger.csv")).unwrap();
    assert_eq!(before, report.as_bytes());
    let secondary_calls = model.secondary_requests().len();
    let app = router(state, DaemonConfig::default());
    let listed = app
        .clone()
        .oneshot(owner_get("/api/workspace/files"))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: serde_json::Value =
        serde_json::from_slice(&to_bytes(listed.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert!(listed["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["path"] == "reports/ledger.csv"));
    let response = app
        .oneshot(owner_get(
            "/api/workspace/file/download?path=reports/ledger.csv",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let downloaded = to_bytes(response.into_body(), 1024 * 1024 + 1)
        .await
        .unwrap();
    assert_eq!(downloaded.as_ref(), report.as_bytes());
    assert_eq!(
        std::fs::read(root.join("reports/ledger.csv")).unwrap(),
        before
    );
    assert_eq!(
        model.requests().len(),
        2,
        "listing and downloading make no model calls"
    );
    assert_eq!(model.secondary_requests().len(), secondary_calls);
}
