//! Packaged Windows acceptance: real owner routes, isolated edit worker and
//! private publication, with only a loopback scripted model and inert data.
#![cfg(windows)]

mod common;

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    future::IntoFuture,
    io::{Cursor, Read, Write},
    net::TcpListener,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const TOKEN: &str = "document-acceptance-local-owner";
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

fn package(parts: Vec<(&str, String)>) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, content) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn types(overrides: &[(&str, &str)]) -> String {
    format!(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>{}</Types>"#, overrides.iter().map(|(path, kind)| format!(r#"<Override PartName="/{path}" ContentType="application/vnd.openxmlformats-officedocument.{kind}+xml"/>"#)).collect::<String>())
}
fn relationships(main: &str) -> String {
    format!(
        r#"<Relationships xmlns="{REL}"><Relationship Id="r1" Type="{R}/officeDocument" Target="{main}"/></Relationships>"#
    )
}
fn fixture(format: &str) -> Vec<u8> {
    match format {
        "docx" => package(vec![
            (
                "[Content_Types].xml",
                types(&[
                    ("word/document.xml", "wordprocessingml.document.main"),
                    ("word/styles.xml", "wordprocessingml.styles"),
                ]),
            ),
            ("_rels/.rels", relationships("word/document.xml")),
            (
                "word/document.xml",
                format!(
                    r#"<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body><w:p><w:r><w:t>Original</w:t></w:r></w:p></w:body></w:document>"#
                ),
            ),
            (
                "word/styles.xml",
                format!(r#"<w:styles xmlns:w="{W}"><w:style w:styleId="Keep"/></w:styles>"#),
            ),
        ]),
        "xlsx" => package(vec![
            (
                "[Content_Types].xml",
                types(&[
                    ("xl/workbook.xml", "spreadsheetml.sheet.main"),
                    ("xl/worksheets/sheet1.xml", "spreadsheetml.worksheet"),
                    ("xl/styles.xml", "spreadsheetml.styles"),
                ]),
            ),
            ("_rels/.rels", relationships("xl/workbook.xml")),
            (
                "xl/workbook.xml",
                format!(
                    r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="Budget" sheetId="1" r:id="s1"/></sheets><calcPr calcMode="manual"/></workbook>"#
                ),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                format!(
                    r#"<Relationships xmlns="{REL}"><Relationship Id="s1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
                ),
            ),
            (
                "xl/worksheets/sheet1.xml",
                format!(
                    r#"<worksheet xmlns="{S}"><dimension ref="A1:C3"/><sheetData><row r="1"><c r="A1" s="0"><v>2</v></c><c r="B1"><v>3</v></c></row></sheetData></worksheet>"#
                ),
            ),
            (
                "xl/styles.xml",
                format!(
                    r#"<styleSheet xmlns="{S}"><cellXfs count="1"><xf numFmtId="0"/></cellXfs></styleSheet>"#
                ),
            ),
        ]),
        "pptx" => package(vec![
            (
                "[Content_Types].xml",
                types(&[
                    ("ppt/presentation.xml", "presentationml.presentation.main"),
                    ("ppt/slides/slide1.xml", "presentationml.slide"),
                    ("ppt/slides/slide2.xml", "presentationml.slide"),
                ]),
            ),
            ("_rels/.rels", relationships("ppt/presentation.xml")),
            (
                "ppt/presentation.xml",
                format!(
                    r#"<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="s1"/><p:sldId id="257" r:id="s2"/></p:sldIdLst></p:presentation>"#
                ),
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                format!(
                    r#"<Relationships xmlns="{REL}"><Relationship Id="s1" Type="{R}/slide" Target="slides/slide1.xml"/><Relationship Id="s2" Type="{R}/slide" Target="slides/slide2.xml"/></Relationships>"#
                ),
            ),
            (
                "ppt/slides/slide1.xml",
                format!(
                    r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/></p:nvSpPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>First</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
                ),
            ),
            (
                "ppt/slides/slide2.xml",
                format!(
                    r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/></p:nvSpPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Second</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
                ),
            ),
        ]),
        _ => unreachable!(),
    }
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut value = Vec::new();
        entry.read_to_end(&mut value).unwrap();
        parts.insert(name, value);
    }
    parts
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// Diagnostics read only this synthetic fixture after its owned process exits.
// Keep file reads and traversal finite, and do not traverse observed links.
fn diagnostic_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.file_attributes() & 0x400 != 0
        || metadata.len() > limit
    {
        return Err(format!(
            "not an ordinary bounded file (bytes={})",
            metadata.len()
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("file exceeded diagnostic read bound".into());
    }
    Ok(bytes)
}

fn diagnostic_json(path: &Path, limit: u64) -> Value {
    match diagnostic_bytes(path, limit) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| json!({"diagnostic_error":error.to_string()})),
        Err(error) => json!({"diagnostic_error":error}),
    }
}

fn diagnostic_file(path: &Path, limit: u64) -> Value {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => json!({
            "exists":true,
            "is_file":metadata.is_file(),
            "is_symlink":metadata.file_type().is_symlink(),
            "bytes":metadata.len(),
            "read":match diagnostic_bytes(path, limit) {
                Ok(bytes) => json!({"sha256":hash(&bytes)}),
                Err(error) => json!({"error":error}),
            },
        }),
        Err(error) => json!({"exists":false,"error":error.to_string()}),
    }
}

fn diagnostic_fields(value: &Value, fields: &[&str]) -> Value {
    Value::Object(
        fields
            .iter()
            .filter_map(|field| {
                value
                    .get(*field)
                    .map(|value| ((*field).into(), value.clone()))
            })
            .collect(),
    )
}

fn checkpoint_diagnostics(root: &Path) -> Value {
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut entries = Vec::new();
    let mut remaining_bytes = 128 * 1024_u64;
    while let Some((path, depth)) = pending.pop() {
        if entries.len() >= 128 {
            entries.push(json!({"diagnostic_limit":"128 checkpoint entries"}));
            break;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                entries.push(json!({"path":relative,"error":error.to_string()}));
                continue;
            }
        };
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            entries.push(json!({"path":relative,"diagnostic_error":"link not followed"}));
        } else if metadata.is_dir() {
            entries.push(json!({"path":relative,"directory":true}));
            if depth >= 6 {
                continue;
            }
            match std::fs::read_dir(&path) {
                Ok(children) => {
                    let mut children: Vec<_> = children.take(128).collect();
                    children
                        .sort_by_key(|entry| entry.as_ref().ok().map(|entry| entry.file_name()));
                    for entry in children.into_iter().rev() {
                        match entry {
                            Ok(entry) => pending.push((entry.path(), depth + 1)),
                            Err(error) => {
                                entries.push(json!({"path":relative,"error":error.to_string()}))
                            }
                        }
                    }
                }
                Err(error) => entries.push(json!({"path":relative,"error":error.to_string()})),
            }
        } else {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let include = path
                .extension()
                .is_some_and(|extension| extension == "json")
                || matches!(
                    name,
                    "prepared-ack" | "refused" | "index-ack" | "capture.lock"
                )
                || path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "acks");
            let mut evidence = json!({"path":relative,"bytes":metadata.len()});
            if include && metadata.len() <= remaining_bytes && metadata.len() <= 16 * 1024 {
                match diagnostic_bytes(&path, remaining_bytes.min(16 * 1024)) {
                    Ok(bytes) => {
                        remaining_bytes = remaining_bytes.saturating_sub(bytes.len() as u64);
                        evidence["sha256"] = json!(hash(&bytes));
                        evidence["content"] = serde_json::from_slice(&bytes)
                            .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
                    }
                    Err(error) => evidence["read_error"] = json!(error),
                }
            }
            entries.push(evidence);
        }
    }
    json!({"entries":entries,"remaining_read_budget":remaining_bytes})
}

struct PublicationDiagnostic {
    run: String,
    proposal: String,
    destination: String,
    candidate_sha256: String,
}

#[derive(Clone)]
struct Case {
    format: String,
    attachment: String,
    source_hash: String,
    destination: String,
    stage: usize,
}
type ModelState = Arc<Mutex<Option<Case>>>;
async fn model(State(state): State<ModelState>, Json(input): Json<Value>) -> Json<Value> {
    let mut state = state.lock().unwrap();
    let case = state
        .as_mut()
        .expect("unexpected model call without active fixture");
    let messages = input["messages"].as_array().expect("model messages");
    let previous = || -> Value {
        let message = messages
            .iter()
            .rev()
            .find(|m| m["role"] == "tool")
            .expect("previous tool response");
        serde_json::from_str(message["content"].as_str().expect("tool response text"))
            .expect("successful tool JSON")
    };
    let source = json!({"kind":"attachment","attachment_id":case.attachment});
    let (tool, args) = match case.stage {
        0 => {
            let tools = input["tools"]
                .as_array()
                .expect("document tools are enabled");
            for name in [
                "inspect_office_document",
                "propose_document_edit",
                "apply_document_edit",
            ] {
                assert!(
                    tools.iter().any(|tool| tool["function"]["name"] == name),
                    "missing granted tool {name}"
                );
            }
            ("inspect_office_document", json!({"source":source}))
        }
        1 => {
            let inventory = previous();
            assert_eq!(inventory["source_sha256"], case.source_hash);
            assert_eq!(inventory["coverage"]["complete"], true);
            assert_eq!(inventory["source"]["kind"], "attachment");
            let operation = match case.format.as_str() {
                "docx" => {
                    json!({"type":"word_replace_text","target_id":"word:0","start":0,"end":8,"expected_text":"Original","new_text":"Revised"})
                }
                "xlsx" => {
                    json!({"type":"set_cell_value","sheet_id":"sheet:s1","cell":"A1","value":{"type":"number","value":7}})
                }
                "pptx" => json!({"type":"reorder_slides","slide_ids":["slide:s2","slide:s1"]}),
                _ => unreachable!(),
            };
            (
                "propose_document_edit",
                json!({"source":source,"expected_sha256":case.source_hash,"file_path":case.destination,"expected_destination":{"state":"absent"},"operations":[operation]}),
            )
        }
        2 => {
            let proposal = previous();
            assert!(proposal["proposal_id"].as_str().is_some());
            assert_eq!(proposal["review_sha256"].as_str().map(str::len), Some(64));
            (
                "apply_document_edit",
                json!({"proposal_id":proposal["proposal_id"],"review_sha256":proposal["review_sha256"],"file_path":case.destination,"expected_destination":{"state":"absent"}}),
            )
        }
        _ => {
            case.stage += 1;
            return Json(
                json!({"choices":[{"message":{"role":"assistant","content":"Fixture completed."},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
            );
        }
    };
    case.stage += 1;
    Json(
        json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":format!("fixture-{}",case.stage),"type":"function","function":{"name":tool,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
    )
}

struct Daemon {
    child: Child,
    log: PathBuf,
    base: String,
    client: reqwest::Client,
    home: PathBuf,
    workspace: PathBuf,
    publication: Option<PublicationDiagnostic>,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let exit = self.child.wait();
        if std::thread::panicking() {
            eprintln!("packaged owned process exit: {exit:?}");
            if let Some(publication) = &self.publication {
                let data = self.home.join(".animaos-swarm/data");
                let control = diagnostic_json(&data.join("control-plane.json"), 4 * 1024 * 1024);
                let registry = &control["documentEdits"];
                let record = &registry["drafts"][&publication.proposal];
                let receipt = registry["publications"].as_object().and_then(|items| {
                    items
                        .values()
                        .find(|item| item["proposal_id"] == publication.proposal)
                });
                let run = control["runs"]
                    .as_array()
                    .and_then(|runs| runs.iter().find(|run| run["id"] == publication.run));
                eprintln!(
                    "packaged publication diagnostics (observations only; never replay): {}",
                    json!({
                        "run_id":publication.run,
                        "proposal_id":publication.proposal,
                        "expected_candidate_sha256":publication.candidate_sha256,
                        "destination":diagnostic_file(&self.workspace.join(&publication.destination), 4 * 1024 * 1024),
                        "candidate":diagnostic_file(&data.join("document-edits").join(format!("{}.blob", publication.candidate_sha256)), 4 * 1024 * 1024),
                        "control_plane_error":control.get("diagnostic_error"),
                        "registry_record_present":!record.is_null(),
                        "registry":diagnostic_fields(record, &["status","publication_phase","review_sha256","owner_approval"]),
                        "publication_receipt":receipt,
                        "run":run.map(|run| diagnostic_fields(run, &["status","error","actionReceipts","turn","checkpoint"])),
                        "readback_proof":diagnostic_json(&data.join("document-edits").join(format!("proof_{}.json", publication.proposal)), 64 * 1024),
                        "checkpoints":checkpoint_diagnostics(&data.join("checkpoints")),
                    })
                );
            }
            eprintln!(
                "packaged daemon log:\n{}",
                std::fs::read_to_string(&self.log).unwrap_or_default()
            );
        }
    }
}
impl Daemon {
    async fn api(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        key: Option<&str>,
    ) -> Value {
        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.base))
            .bearer_auth(TOKEN);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        let response = request
            .send()
            .await
            .unwrap_or_else(|e| panic!("{method} {path}: {e}"));
        let status = response.status();
        let text = response.text().await.unwrap();
        assert!(status.is_success(), "{method} {path}: {status}: {text}");
        serde_json::from_str(&text).unwrap()
    }
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn packaged_selected_office_edits_require_owner_and_round_trip() {
    let home = common::PrivateHome::new().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_address = listener.local_addr().unwrap();
    let model_state: ModelState = Arc::new(Mutex::new(None));
    let model_task = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(model))
                .with_state(model_state.clone()),
        )
        .into_future(),
    );
    let port = free_port();
    let log_path = home.path().join("daemon.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_anima"));
    command.arg("run").env_clear();
    for key in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    for key in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ANIMA_HOME",
    ] {
        command.env(key, home.path());
    }
    command
        .env("ANIMA_LOCAL_ADMIN_TOKEN", TOKEN)
        .env("ANIMAOS_RS_PORT", port.to_string())
        .env("ANIMAOS_RS_MEMORY_EMBEDDINGS", "disabled")
        .env(
            "ANIMAOS_RS_KEYCHAIN_NAMESPACE",
            format!("document-acceptance-{port}"),
        )
        .env("OPENAI_API_KEY", "local-fixture-only")
        .env("OPENAI_BASE_URL", format!("http://{model_address}/v1"))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
    let child = command.spawn().unwrap();
    let mut daemon = Daemon {
        child,
        log: log_path,
        base: format!("http://127.0.0.1:{port}"),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .unwrap(),
        home: home.path().to_path_buf(),
        workspace: workspace.clone(),
        publication: None,
    };
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if daemon
            .client
            .get(format!("{}/health", daemon.base))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            break;
        }
        assert!(
            daemon.child.try_wait().unwrap().is_none(),
            "packaged daemon exited"
        );
        assert!(
            Instant::now() < deadline,
            "packaged daemon readiness timed out"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    daemon.api(reqwest::Method::PUT, "/api/workspace", Some(json!({"rootPath":workspace,"companyName":"Office fixture","mission":"Local synthetic acceptance","values":[]})), None).await;
    let agent = daemon.api(reqwest::Method::POST, "/api/agents", Some(json!({"name":"office-fixture","model":"fixture","provider":"openai","tools":["inspect_office_document","propose_document_edit","apply_document_edit"],"settings":{"agentFilesProfileEnabled":false,"maxToolIterations":6},"fallbacks":[]})), None).await["agent"]["state"]["id"].as_str().unwrap().to_owned();
    daemon
        .api(
            reqwest::Method::PUT,
            &format!("/api/agents/{agent}/autonomy"),
            Some(json!({"mode":"strict","smartCheck":false})),
            None,
        )
        .await;
    daemon
        .api(
            reqwest::Method::PATCH,
            &format!("/api/agents/{agent}/learning-policy"),
            Some(json!({"autoEnabled":false})),
            None,
        )
        .await;

    for format in ["docx", "xlsx", "pptx"] {
        let session = daemon
            .api(
                reqwest::Method::POST,
                &format!("/api/agents/{agent}/sessions"),
                Some(json!({"title":"Office fixture"})),
                None,
            )
            .await["session"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let session_path = encode(&session);
        let bytes = fixture(format);
        let source_hash = hash(&bytes);
        let filename = format!("selected.{format}");
        let boundary = "anima-document-acceptance-boundary";
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
        body.extend_from_slice(&bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let response = daemon
            .client
            .post(format!(
                "{}/api/agents/{agent}/sessions/{session_path}/attachments",
                daemon.base
            ))
            .bearer_auth(TOKEN)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert!(status.is_success(), "upload {format}: {status}: {text}");
        let upload: Value = serde_json::from_str(&text).unwrap();
        let attachment = upload["attachment"]["id"].as_str().unwrap().to_owned();
        let source_path = upload["attachment"]["path"].as_str().unwrap();
        let destination = format!("edited.{format}");
        let mut denied_proposal: Option<String> = None;
        let attempts = if format == "docx" { 2 } else { 1 };
        for attempt in 0..attempts {
            let deny = format == "docx" && attempt == 0;
            *model_state.lock().unwrap() = Some(Case {
                format: format.into(),
                attachment: attachment.clone(),
                source_hash: source_hash.clone(),
                destination: destination.clone(),
                stage: 0,
            });
            let run = daemon.api(reqwest::Method::POST, &format!("/api/agents/{agent}/sessions/{session_path}/runs"), Some(json!({"text":"Edit the selected inert fixture.","attachmentIds":[attachment.clone()],"mode":"queue"})), Some(&format!("office-{format}-{attempt}"))).await["run"]["id"].as_str().unwrap().to_owned();
            let deadline = Instant::now() + Duration::from_secs(90);
            let pending = loop {
                let pending = daemon
                    .api(
                        reqwest::Method::GET,
                        &format!("/api/approvals?agentId={agent}"),
                        None,
                        None,
                    )
                    .await;
                if let Some(record) = pending["approvals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| {
                        item["runId"] == run
                            && item["tool"] == "apply_document_edit"
                            && item["status"] == "pending"
                    })
                {
                    break record.clone();
                }
                let state = daemon
                    .api(
                        reqwest::Method::GET,
                        &format!("/api/agents/{agent}/runs/{run}"),
                        None,
                        None,
                    )
                    .await;
                assert!(
                    !matches!(
                        state["run"]["status"].as_str(),
                        Some("failed" | "cancelled" | "completed")
                    ),
                    "run ended before approval: {state}"
                );
                assert!(Instant::now() < deadline, "approval timed out: {state}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            assert!(!workspace.join(&destination).exists());
            let approval_args: Value =
                serde_json::from_str(pending["arguments"].as_str().unwrap()).unwrap();
            assert_eq!(pending["argumentsTruncated"], false);
            assert_eq!(approval_args["_anima_exact_document_edit"], true);
            assert_eq!(approval_args["file_path"], destination);
            assert_eq!(
                approval_args["expected_destination"],
                json!({"state":"absent"})
            );
            assert_eq!(approval_args["source_sha256"], source_hash);
            let proposal = approval_args["proposal_id"].as_str().unwrap();
            daemon.publication = Some(PublicationDiagnostic {
                run: run.clone(),
                proposal: proposal.to_owned(),
                destination: destination.clone(),
                candidate_sha256: approval_args["candidate_sha256"].as_str().unwrap().into(),
            });
            if let Some(denied) = &denied_proposal {
                assert_ne!(proposal, denied, "retry must prepare a fresh proposal");
            }
            let review = daemon
                .api(
                    reqwest::Method::GET,
                    &format!(
                        "/api/document-edits/{proposal}?agent_id={}&session_id={}",
                        encode(&agent),
                        session_path
                    ),
                    None,
                    None,
                )
                .await;
            let unauthenticated = daemon
                .client
                .get(format!(
                    "{}/api/document-edits/{proposal}?agent_id={}&session_id={}",
                    daemon.base,
                    encode(&agent),
                    session_path
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(unauthenticated.status(), reqwest::StatusCode::FORBIDDEN);
            assert_eq!(review["review_sha256"], approval_args["review_sha256"]);
            assert_eq!(review["source_sha256"], source_hash);
            assert_eq!(review["file_path"], destination);
            assert_eq!(
                review["expected_destination"],
                approval_args["expected_destination"]
            );
            assert_eq!(review["source"]["kind"], "attachment");
            assert_eq!(review["before"]["format"], format);
            assert_eq!(review["after"]["format"], format);
            assert!(review["before"]["targets"]
                .as_array()
                .is_some_and(|items| !items.is_empty()));
            assert!(review["after"]["targets"]
                .as_array()
                .is_some_and(|items| !items.is_empty()));
            assert!(review["changes"]
                .as_array()
                .is_some_and(|changes| !changes.is_empty()));
            assert_eq!(review["preservation"]["unchanged_bytes_verified"], true);
            daemon
            .api(
                reqwest::Method::POST,
                &format!(
                    "/api/approvals/{}/decision",
                    pending["id"].as_str().unwrap()
                ),
                Some(json!({"decision":if deny { "deny" } else { "allow_once" },"revision":pending["revision"]})),
                None,
            )
            .await;
            loop {
                let state = daemon
                    .api(
                        reqwest::Method::GET,
                        &format!("/api/agents/{agent}/runs/{run}"),
                        None,
                        None,
                    )
                    .await;
                if state["run"]["status"] == "completed"
                    || (deny && state["run"]["status"] == "failed")
                {
                    break;
                }
                assert!(
                    !matches!(
                        state["run"]["status"].as_str(),
                        Some("failed" | "cancelled")
                    ),
                    "run failed: {state}"
                );
                assert!(Instant::now() < deadline, "publication timed out: {state}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if deny {
                assert!(
                    !workspace.join(&destination).exists(),
                    "denial must not publish a file"
                );
                assert_eq!(std::fs::read(workspace.join(source_path)).unwrap(), bytes);
                denied_proposal = Some(proposal.to_owned());
                continue;
            }
            let response = daemon
                .client
                .get(format!(
                    "{}/api/workspace/file/download?path={}",
                    daemon.base,
                    encode(&destination)
                ))
                .bearer_auth(TOKEN)
                .send()
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "download {format}: {}",
                response.status()
            );
            let downloaded = response.bytes().await.unwrap();
            assert_eq!(
                hash(&downloaded),
                approval_args["candidate_sha256"].as_str().unwrap()
            );
            assert_eq!(std::fs::read(workspace.join(source_path)).unwrap(), bytes);
            assert_eq!(
                std::fs::read(workspace.join(&destination)).unwrap(),
                downloaded.as_ref()
            );
            let before = parts(&bytes);
            let after = parts(&downloaded);
            match format {
                "docx" => {
                    assert!(
                        String::from_utf8_lossy(&after["word/document.xml"]).contains("Revised")
                    );
                    assert_eq!(before["word/styles.xml"], after["word/styles.xml"]);
                }
                "xlsx" => {
                    assert!(String::from_utf8_lossy(&after["xl/worksheets/sheet1.xml"])
                        .contains("<v>7</v>"));
                    assert_eq!(before["xl/styles.xml"], after["xl/styles.xml"]);
                }
                "pptx" => {
                    let xml = String::from_utf8_lossy(&after["ppt/presentation.xml"]);
                    assert!(xml.find("r:id=\"s2\"").unwrap() < xml.find("r:id=\"s1\"").unwrap());
                    assert_eq!(
                        before["ppt/slides/slide1.xml"],
                        after["ppt/slides/slide1.xml"]
                    );
                    assert_eq!(
                        before["ppt/slides/slide2.xml"],
                        after["ppt/slides/slide2.xml"]
                    );
                }
                _ => unreachable!(),
            }
        }
    }
    drop(daemon);
    model_task.abort();
}
