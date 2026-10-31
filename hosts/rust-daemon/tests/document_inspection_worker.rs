//! Parent-only actual-engine acceptance. No private documents or providers.
//!
//! Run explicitly with --ignored --exact --test-threads=1 after configuring the
//! pinned operator engine and ANIMA_DOCUMENT_INSPECTION_ACCEPTANCE_OUTPUT.
//! The PDF observer reads only this test's unique worker directory. It may miss
//! an extremely short-lived export; that is an explicit capture failure, never
//! evidence that PDF bytes were verified. No production retention seam is used.
#![cfg(windows)]

use anima_document_content::{render_document, OfficeFormat};
use anima_document_visual::inspection::{
    admit_office, build_report, ArtifactInspectionReport, CalculationEvidence, ExportedView,
    FormulaValue, InspectionLimitation, RenderCoverage,
};
use anima_document_visual::{PdfPageEvidence, RecognizedText};
use rawzip::{CompressionMethod, ZipArchive, ZipArchiveWriter};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use win_custody::{anonymous_pipe, Job, JobLimits, Launch, WaitOutcome};

const READY: &[u8] = b"ANIMA-DOCUMENT-INSPECTION-READY-v1\n";
const MAX_SOURCE: usize = 4 * 1024 * 1024;
const MAX_HEADER: usize = 1024 * 1024;
const MAX_PDF: usize = 2 * 1024 * 1024;
const MAX_PNG: usize = 2 * 1024 * 1024;
const MAX_OUTPUT: usize = 20 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 8;

fn failure(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageHeader {
    page: u32,
    width: u32,
    height: u32,
    sha256: String,
    byte_length: usize,
    recognized: RecognizedText,
}
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum ResultKind {
    Inspected { report: ArtifactInspectionReport },
    Rejected { message: String },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    request_sha256: String,
    source_sha256: String,
    result: ResultKind,
    pages: Vec<PageHeader>,
}
struct Exchange {
    response: Response,
    pages: Vec<PdfPageEvidence>,
}
struct Snapshot {
    sha256: String,
    bytes: Vec<u8>,
}
struct Actual {
    report: ArtifactInspectionReport,
    pages: Vec<PdfPageEvidence>,
    pdf: Vec<u8>,
}

fn read_frame(reader: &mut impl Read, maximum: usize) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > maximum {
        return Err(failure("worker frame exceeds acceptance bound"));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn write_frame(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(
        &u32::try_from(bytes.len())
            .map_err(|_| failure("oversized frame"))?
            .to_le_bytes(),
    )?;
    writer.write_all(bytes)
}
fn exchange(
    mut stdin: File,
    mut stdout: File,
    header: Vec<u8>,
    source: Vec<u8>,
) -> io::Result<Exchange> {
    let mut ready = vec![0; READY.len()];
    stdout.read_exact(&mut ready)?;
    if ready != READY {
        return Err(failure("worker did not report checked inner Job readiness"));
    }
    // No source bytes are sent before the exact fixed readiness frame.
    write_frame(&mut stdin, &header)?;
    write_frame(&mut stdin, &source)?;
    stdin.flush()?;
    drop(stdin);
    let response_bytes = read_frame(&mut stdout, MAX_HEADER)?;
    let response: Response =
        serde_json::from_slice(&response_bytes).map_err(|error| failure(error.to_string()))?;
    if response.version != 1
        || response.request_sha256 != hash(&header)
        || response.source_sha256 != hash(&source)
        || response.pages.len() > 10
    {
        return Err(failure("worker response binding is invalid"));
    }
    let mut total = response_bytes.len();
    let mut pages = Vec::with_capacity(response.pages.len());
    for metadata in &response.pages {
        let limit = MAX_PNG.min(
            MAX_OUTPUT
                .checked_sub(total)
                .ok_or_else(|| failure("aggregate output exceeded"))?,
        );
        let png = read_frame(&mut stdout, limit)?;
        total += png.len();
        if png.len() != metadata.byte_length || hash(&png) != metadata.sha256 {
            return Err(failure(
                "actual PNG frame differs from its declared identity",
            ));
        }
        pages.push(PdfPageEvidence {
            page: metadata.page,
            width: metadata.width,
            height: metadata.height,
            png,
            recognized: metadata.recognized.clone(),
        });
    }
    let mut trailing = [0; 1];
    if stdout.read(&mut trailing)? != 0 {
        return Err(failure("trailing worker output"));
    }
    Ok(Exchange { response, pages })
}

fn observe_pdf(directory: &Path, stop: &AtomicBool, snapshots: &Mutex<Vec<Snapshot>>) {
    while !stop.load(Ordering::Acquire) {
        if let Ok(entries) = fs::read_dir(directory) {
            for entry in entries.take(16).flatten() {
                let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                    continue;
                };
                if !metadata.is_file()
                    || metadata.file_attributes() & 0x400 != 0
                    || entry.file_name() != "export.pdf"
                {
                    continue;
                }
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if !metadata.is_file()
                    || metadata.file_attributes() & 0x400 != 0
                    || metadata.len() == 0
                    || metadata.len() > MAX_PDF as u64
                {
                    continue;
                }
                let Ok(file) = File::open(path) else {
                    continue;
                };
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                if file
                    .take(MAX_PDF as u64 + 1)
                    .read_to_end(&mut bytes)
                    .is_err()
                    || bytes.is_empty()
                    || bytes.len() > MAX_PDF
                {
                    continue;
                }
                let sha256 = hash(&bytes);
                let mut snapshots = snapshots
                    .lock()
                    .expect("acceptance PDF observer lock poisoned");
                if snapshots.iter().any(|snapshot| snapshot.sha256 == sha256) {
                    continue;
                }
                if snapshots.len() == MAX_SNAPSHOTS {
                    snapshots.remove(0);
                }
                snapshots.push(Snapshot { sha256, bytes });
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run_worker(
    source: &[u8],
    format: OfficeFormat,
    name: &str,
    program: &Path,
    tree: &str,
) -> io::Result<Actual> {
    if source.is_empty() || source.len() > MAX_SOURCE {
        return Err(failure("synthetic source exceeds input bound"));
    }
    let directory = tempfile::Builder::new().prefix("aia-").tempdir_in(
        std::env::var_os("ANIMAOS_TASK_TEMP_ROOT").map_or_else(std::env::temp_dir, PathBuf::from),
    )?;
    let user = win_custody::current_user_sid_string()?;
    win_custody::SecurityDescriptor::from_sddl(&format!(
        "O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"
    ))?
    .apply_to_tree(directory.path(), true)?;
    let source_version = format!("synthetic-acceptance:{name}");
    let header = serde_json::to_vec(
        &json!({"version":1,"source_sha256":hash(source),"source_byte_length":source.len(),
        "source_version":&source_version,"format":format,"engine":{"program_directory":program,"tree_sha256":tree}}),
    )?;
    if header.len() > 64 * 1024 {
        return Err(failure("acceptance request header exceeds bound"));
    }
    let limits = JobLimits::new()
        .kill_on_close(true)
        .active_processes(8)
        .process_memory(512 * 1024 * 1024)
        .job_memory(1024 * 1024 * 1024);
    let job = Job::create(&limits)?;
    if job.limits()? != limits || job.active_process_count()? != 0 {
        return Err(failure("outer Job readback/accounting failed"));
    }
    let (child_stdin, parent_stdin) = anonymous_pipe()?;
    let (parent_stdout, child_stdout) = anonymous_pipe()?;
    let (parent_stderr, child_stderr) = anonymous_pipe()?;
    let mut launch = Launch::new(env!("CARGO_BIN_EXE_anima-daemon"));
    launch
        .arg("--document-inspection-worker-v1")
        .env_clear()
        .create_no_window(true)
        .current_dir(directory.path())
        .env("TEMP", directory.path())
        .env("TMP", directory.path())
        .env("USERPROFILE", directory.path())
        .env("APPDATA", directory.path())
        .env("LOCALAPPDATA", directory.path())
        // A normal daemon would reject this before startup; fixed mode must run first.
        .env("ANIMAOS_RS_MAX_REQUEST_BYTES", "invalid")
        .stdin(&child_stdin)
        .stdout(&child_stdout)
        .stderr(&child_stderr);
    for variable in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(variable) {
            launch.env(variable, value);
        }
    }
    if let Some(value) = std::env::var_os("LOCALAPPDATA") {
        launch.env(
            "ANIMA_INSPECTION_USER_FONT_DIRECTORY",
            PathBuf::from(value).join("Microsoft/Windows/Fonts"),
        );
    }
    let suspended = launch.spawn_suspended()?;
    job.assign(suspended.process())?;
    if job.limits()? != limits || job.active_process_count()? != 1 {
        return Err(failure("suspended worker assignment/accounting failed"));
    }
    let process = suspended.resume()?;
    drop(launch);
    drop((child_stdin, child_stdout, child_stderr));
    let (sender, receiver) = mpsc::sync_channel(1);
    let source_bytes = source.to_vec();
    let io_thread = std::thread::spawn(move || {
        let _ = sender.send(exchange(
            File::from(parent_stdin),
            File::from(parent_stdout),
            header,
            source_bytes,
        ));
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        File::from(parent_stderr)
            .take(4097)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let stop = Arc::new(AtomicBool::new(false));
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let watch_stop = stop.clone();
    let watch_snapshots = snapshots.clone();
    let watch_directory = directory.path().to_path_buf();
    let watcher =
        std::thread::spawn(move || observe_pdf(&watch_directory, &watch_stop, &watch_snapshots));
    let response = receiver.recv_timeout(Duration::from_secs(60));
    let leader_exited = matches!(
        process.wait(Some(Duration::from_secs(2))),
        Ok(WaitOutcome::Signaled)
    );
    let leader_code = process.exit_code();
    // Retire even when framing, output, native exit, or the deadline failed.
    let terminated = job.terminate(1);
    let retirement_deadline = Instant::now() + Duration::from_secs(5);
    let mut zero = false;
    while Instant::now() < retirement_deadline {
        match job.active_process_count() {
            Ok(0) => {
                zero = true;
                break;
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    stop.store(true, Ordering::Release);
    watcher
        .join()
        .map_err(|_| failure("PDF observer panicked"))?;
    if terminated.is_err() || !zero {
        // The test must preserve its owned directory and pipe owners when
        // retirement cannot be proved, just as the production owner does.
        // Handles are reclaimed on harness exit; the Job still kills on close.
        // Deliberate quarantine is preferable to deleting a live worker's files.
        std::mem::forget((job, process, directory, io_thread, stderr_thread));
        return Err(failure(
            "worker tree did not reach verified zero-process retirement",
        ));
    }
    io_thread
        .join()
        .map_err(|_| failure("acceptance framing thread panicked"))?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| failure("acceptance stderr thread panicked"))??;
    if !leader_exited || !matches!(leader_code, Ok(Some(0))) || !stderr.is_empty() {
        return Err(failure(format!(
            "{name}: worker retirement failed: leader_signaled={leader_exited}, exit={leader_code:?}, exchange={}, stderr={}",
            match &response {
                Ok(Ok(_)) => "received".to_owned(),
                Ok(Err(error)) => format!("failed: {error}"),
                Err(error) => format!("timed out: {error}"),
            },
            String::from_utf8_lossy(&stderr),
        )));
    }
    let exchange =
        response.map_err(|_| failure("actual worker exchange exceeded sixty seconds"))??;
    let report = match exchange.response.result {
        ResultKind::Inspected { report } => report,
        ResultKind::Rejected { message } => {
            return Err(failure(format!(
                "{name}: actual engine rejected fixture: {message}"
            )))
        }
    };
    report
        .validate()
        .map_err(|error| failure(error.to_string()))?;
    if report.source().sha256 != hash(source)
        || report.source().byte_length != source.len() as u64
        || report.source().format != format
        || report.source().source_version != source_version
    {
        return Err(failure(
            "report does not bind the exact original synthetic bytes",
        ));
    }
    let mut captured = snapshots
        .lock()
        .expect("acceptance PDF observer lock poisoned");
    let index = captured.iter().position(|snapshot| snapshot.sha256 == report.rendering().pdf_sha256
        && snapshot.bytes.len() as u64 == report.rendering().pdf_byte_length)
        .ok_or_else(|| failure("actual PDF observer missed the exact report-bound export; PDF bytes are unverified"))?;
    let pdf = captured.swap_remove(index).bytes;
    let admission = admit_office(format, source).map_err(|error| failure(error.to_string()))?;
    let rebuilt = build_report(
        &source_version,
        &admission,
        report.renderer().clone(),
        &pdf,
        &exchange.pages,
        report.calculation().evidence.clone(),
    )
    .map_err(|error| failure(error.to_string()))?;
    if rebuilt.sha256() != report.sha256() {
        return Err(failure(
            "actual PDF/PNG bytes do not rebuild the received report",
        ));
    }
    Ok(Actual {
        report,
        pages: exchange.pages,
        pdf,
    })
}

fn generated(spec: Value) -> io::Result<Vec<u8>> {
    let spec = serde_json::from_value(spec)?;
    render_document(&spec)
        .map(|output| output.bytes)
        .map_err(|error| failure(error.to_string()))
}

// The local generator uses DEFLATE ZIP entries. Keep the rewrite bounded and
// drop generation comments so metadata cannot stand in for the mutated bytes.
fn parts(bytes: &[u8]) -> io::Result<BTreeMap<String, Vec<u8>>> {
    if bytes.len() > MAX_SOURCE {
        return Err(failure("fixture ZIP is oversized"));
    }
    let archive = ZipArchive::from_slice(bytes).map_err(|error| failure(error.to_string()))?;
    let mut entries = archive.entries();
    let mut output = BTreeMap::new();
    let mut total = 0usize;
    while let Some(entry) = entries
        .next_entry()
        .map_err(|error| failure(error.to_string()))?
    {
        if output.len() >= 256 || entry.compression_method() != CompressionMethod::DEFLATE {
            return Err(failure(
                "synthetic fixture must use bounded DEFLATE entries",
            ));
        }
        let name = std::str::from_utf8(entry.file_path().as_bytes())
            .map_err(|error| failure(error.to_string()))?
            .to_owned();
        let local = archive
            .get_entry(entry.wayfinder())
            .map_err(|error| failure(error.to_string()))?;
        let mut body = Vec::new();
        local
            .verifying_reader(flate2::read::DeflateDecoder::new(local.data()))
            .take(MAX_PDF as u64 + 1)
            .read_to_end(&mut body)?;
        total = total
            .checked_add(body.len())
            .ok_or_else(|| failure("fixture ZIP overflow"))?;
        if body.len() > MAX_PDF
            || total > MAX_SOURCE
            || name.len() > 256
            || output.insert(name, body).is_some()
        {
            return Err(failure("fixture ZIP body bound or duplicate entry"));
        }
    }
    Ok(output)
}
fn zip(parts: &BTreeMap<String, Vec<u8>>) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut archive = ZipArchiveWriter::new(&mut output);
    for (name, body) in parts {
        let (mut entry, config) = archive
            .new_file(name)
            .compression_method(CompressionMethod::DEFLATE)
            .start()
            .map_err(|error| failure(error.to_string()))?;
        let encoder =
            flate2::write::DeflateEncoder::new(&mut entry, flate2::Compression::default());
        let mut writer = config.wrap(encoder);
        writer.write_all(body)?;
        let (encoder, descriptor) = writer
            .finish()
            .map_err(|error| failure(error.to_string()))?;
        encoder.finish()?;
        entry
            .finish(descriptor)
            .map_err(|error| failure(error.to_string()))?;
    }
    archive
        .finish()
        .map_err(|error| failure(error.to_string()))?;
    if output.len() > MAX_SOURCE {
        return Err(failure("rewritten synthetic source exceeds bound"));
    }
    Ok(output)
}
fn edit(
    parts: &mut BTreeMap<String, Vec<u8>>,
    name: &str,
    transform: impl FnOnce(&str) -> io::Result<String>,
) -> io::Result<()> {
    let body = parts
        .get_mut(name)
        .ok_or_else(|| failure(format!("missing fixture part {name}")))?;
    let xml = std::str::from_utf8(body).map_err(|error| failure(error.to_string()))?;
    *body = transform(xml)?.into_bytes();
    Ok(())
}
fn replace_once(xml: &str, before: &str, after: &str) -> io::Result<String> {
    if xml.matches(before).count() != 1 {
        return Err(failure(format!(
            "synthetic fixture anchor changed: {before}"
        )));
    }
    Ok(xml.replacen(before, after, 1))
}

struct Case {
    name: &'static str,
    format: OfficeFormat,
    bytes: Vec<u8>,
}
fn cases() -> io::Result<Vec<Case>> {
    let word = generated(
        json!({"format":"docx","title":"Actual Office acceptance","blocks":[{"type":"paragraph","text":"A visible first page from the original Office bytes."}]}),
    )?;
    let mut blank = parts(&word)?;
    edit(&mut blank, "word/document.xml", |xml| {
        replace_once(
            xml,
            "<w:sectPr>",
            "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p><w:p/><w:sectPr>",
        )
    })?;
    let presentation = generated(
        json!({"format":"pptx","title":"Actual slide acceptance","slides":[{"title":"Original slide","blocks":[{"type":"paragraph","text":"PPTX_CLIP_MARKER"}]}]}),
    )?;
    let mut clipped = parts(&presentation)?;
    edit(&mut clipped, "ppt/slides/slide1.xml", |xml| {
        let start = xml
            .find("name=\"Content 3\"")
            .ok_or_else(|| failure("clipping fixture shape missing"))?;
        let end = start
            + xml[start..]
                .find("</p:sp>")
                .ok_or_else(|| failure("clipping fixture shape end"))?;
        let shape = &xml[start..end];
        let ext = shape
            .find("<a:ext ")
            .ok_or_else(|| failure("clipping fixture geometry"))?;
        let ext_end = ext
            + shape[ext..]
                .find("/>")
                .ok_or_else(|| failure("clipping fixture extent end"))?
            + 2;
        let shape = shape
            .replacen(
                &shape[ext..ext_end],
                "<a:ext cx=\"914400\" cy=\"127000\"/>",
                1,
            )
            .replace("<a:normAutofit/>", "<a:noAutofit/>")
            .replace(
                "<a:bodyPr wrap=\"square\"",
                "<a:bodyPr wrap=\"square\" vertOverflow=\"clip\" horzOverflow=\"clip\"",
            )
            .replace(
                "PPTX_CLIP_MARKER",
                &"Intentionally oversized synthetic text for visual review. ".repeat(12),
            );
        Ok(format!("{}{}{}", &xml[..start], shape, &xml[end..]))
    })?;
    let workbook = generated(
        json!({"format":"xlsx","title":"Actual calculation acceptance","sheets":[{"name":"Visible","columns":["Value"],"rows":[[7]]}]}),
    )?;
    let mut stale = parts(&workbook)?;
    edit(&mut stale, "xl/worksheets/sheet1.xml", |xml| {
        replace_once(
            xml,
            "<c r=\"A2\"><v>7</v></c>",
            "<c r=\"A2\"><f>1+2</f><v>999</v></c>",
        )
    })?;
    edit(&mut stale, "xl/workbook.xml", |xml| {
        replace_once(
            xml,
            "</workbook>",
            "<calcPr calcMode=\"manual\" fullCalcOnLoad=\"0\" forceFullCalc=\"0\"/></workbook>",
        )
    })?;
    let hidden_base = generated(
        json!({"format":"xlsx","title":"Hidden calculation acceptance","sheets":[{"name":"Visible","columns":["Value"],"rows":[[7]]},{"name":"Hidden","columns":["Value"],"rows":[[7]]}]}),
    )?;
    let mut hidden = parts(&hidden_base)?;
    edit(&mut hidden, "xl/workbook.xml", |xml| {
        replace_once(
            xml,
            "name=\"Hidden\" sheetId=\"2\"",
            "name=\"Hidden\" sheetId=\"2\" state=\"hidden\"",
        )
    })?;
    edit(&mut hidden, "xl/worksheets/sheet2.xml", |xml| {
        let xml = replace_once(
            xml,
            "</sheetData>",
            "<row r=\"40\" hidden=\"1\"><c r=\"C40\"><f>1/0</f><v>999</v></c></row></sheetData>",
        )?;
        replace_once(&xml, "dimension ref=\"A1:A2\"", "dimension ref=\"A1:C40\"")
    })?;
    let mut unprinted = parts(&workbook)?;
    edit(&mut unprinted, "xl/worksheets/sheet1.xml", |xml| {
        let xml = replace_once(
            xml,
            "</sheetData>",
            "<row r=\"99\"><c r=\"Z99\"><f>1/0</f><v>999</v></c></row></sheetData>",
        )?;
        replace_once(&xml, "dimension ref=\"A1:A2\"", "dimension ref=\"A1:Z99\"")
    })?;
    edit(&mut unprinted, "xl/workbook.xml", |xml| {
        replace_once(xml, "</sheets>", "</sheets><definedNames><definedName name=\"_xlnm.Print_Area\" localSheetId=\"0\">'Visible'!$A$1:$A$2</definedName></definedNames>")
    })?;
    Ok(vec![
        Case {
            name: "docx_normal",
            format: OfficeFormat::Docx,
            bytes: word,
        },
        Case {
            name: "docx_extra_blank_page",
            format: OfficeFormat::Docx,
            bytes: zip(&blank)?,
        },
        Case {
            name: "pptx_normal",
            format: OfficeFormat::Pptx,
            bytes: presentation,
        },
        Case {
            name: "pptx_clipped_text_review",
            format: OfficeFormat::Pptx,
            bytes: zip(&clipped)?,
        },
        Case {
            name: "xlsx_stale_cached_numeric",
            format: OfficeFormat::Xlsx,
            bytes: zip(&stale)?,
        },
        Case {
            name: "xlsx_hidden_sheet_error",
            format: OfficeFormat::Xlsx,
            bytes: zip(&hidden)?,
        },
        Case {
            name: "xlsx_unprinted_error",
            format: OfficeFormat::Xlsx,
            bytes: zip(&unprinted)?,
        },
    ])
}

#[test]
#[ignore = "parent-only real pinned LibreOffice/Windows PDF acceptance; requires explicit engine and output environment"]
fn document_inspection_real_engine_matrix() -> io::Result<()> {
    let program = PathBuf::from(
        std::env::var_os("ANIMA_DOCUMENT_INSPECTION_LIBREOFFICE_PROGRAM")
            .ok_or_else(|| failure("operator PROGRAM is required"))?,
    );
    let tree = std::env::var("ANIMA_DOCUMENT_INSPECTION_LIBREOFFICE_TREE_SHA256")
        .map_err(|_| failure("operator TREE_SHA256 is required"))?;
    if !program.is_absolute()
        || program.file_name().and_then(|part| part.to_str()) != Some("program")
        || tree.len() != 64
        || !tree
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(failure(
            "exact absolute operator engine and lowercase tree pin are required",
        ));
    }
    let output = PathBuf::from(
        std::env::var_os("ANIMA_DOCUMENT_INSPECTION_ACCEPTANCE_OUTPUT")
            .ok_or_else(|| failure("explicit acceptance output directory is required"))?,
    );
    if !output.is_absolute()
        || !output
            .components()
            .any(|component| matches!(component, Component::Normal(name) if name == "test-output"))
    {
        return Err(failure(
            "acceptance output must be an explicit absolute test-output directory",
        ));
    }
    for ancestor in output.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_attributes() & 0x400 != 0 => {
                return Err(failure("acceptance output cannot traverse a reparse point"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    fs::create_dir_all(&output)?;
    let output = output.join(format!("actual-office-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&output)?;
    eprintln!(
        "Synthetic actual Office evidence directory: {}",
        output.display()
    );
    let mut summary = Vec::new();
    for case in cases()? {
        // Fail before launching if a synthetic XML tweak exceeds the closed subset.
        let admission = admit_office(case.format, &case.bytes)
            .map_err(|error| failure(format!("{} admission: {error}", case.name)))?;
        let case_output = output.join(case.name);
        fs::create_dir(&case_output)?;
        let extension = match case.format {
            OfficeFormat::Docx => "docx",
            OfficeFormat::Xlsx => "xlsx",
            OfficeFormat::Pptx => "pptx",
            _ => unreachable!("matrix formats are fixed Office types"),
        };
        fs::write(
            case_output.join(format!("original.{extension}")),
            &case.bytes,
        )?;
        let actual = run_worker(&case.bytes, case.format, case.name, &program, &tree)?;
        fs::write(
            case_output.join("report.json"),
            serde_json::to_vec_pretty(&actual.report)?,
        )?;
        fs::write(case_output.join("actual-export.pdf"), &actual.pdf)?;
        for page in &actual.pages {
            fs::write(
                case_output.join(format!("page-{:02}.png", page.page)),
                &page.png,
            )?;
        }
        assert_eq!(
            actual.report.formula_inventory().formulas.as_slice(),
            admission.formulas(),
            "{} full original formula inventory",
            case.name
        );
        assert_eq!(
            actual.report.rendering().coverage,
            RenderCoverage::CompleteExportedView,
            "small synthetic exported view"
        );
        assert!(actual
            .report
            .limitations()
            .contains(&InspectionLimitation::NoGenericLayoutOrOutcomePass));
        assert!(actual
            .report
            .limitations()
            .contains(&InspectionLimitation::RasterInkDoesNotDecideIntentionalBlanknessOrClipping));
        match case.name {
            "docx_normal" => assert_eq!(actual.report.rendering().exported_page_count, 1),
            "docx_extra_blank_page" => {
                assert_eq!(
                    actual.report.rendering().exported_page_count,
                    2,
                    "explicit blank page must survive export"
                );
                assert_eq!(
                    actual.report.rendering().pages[1].ink.non_background_pixels,
                    0,
                    "actual final page must be white"
                );
            }
            "pptx_normal" | "pptx_clipped_text_review" => {
                assert_eq!(actual.report.rendering().exported_page_count, 1)
            }
            "xlsx_stale_cached_numeric" => {
                let CalculationEvidence::Recalculated { results } =
                    &actual.report.calculation().evidence
                else {
                    return Err(failure(
                        "stale cache did not receive independent recalculation",
                    ));
                };
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].identity.address, "A2");
                assert_eq!(
                    results[0].value,
                    FormulaValue::Number { value: 3.0 },
                    "cache999 is not calculation proof"
                );
                assert!(actual.report.calculation().errors.is_empty());
            }
            "xlsx_hidden_sheet_error" | "xlsx_unprinted_error" => {
                assert_eq!(
                    actual.report.rendering().view,
                    ExportedView::CalcPrintedSheetsAndPrintAreas
                );
                assert_eq!(
                    actual.report.rendering().exported_page_count,
                    1,
                    "hidden/unprinted formula must not expand exported view"
                );
                assert_eq!(actual.report.calculation().total_formulas, 1);
                assert_eq!(actual.report.calculation().inspected_formulas, 1);
                assert_eq!(
                    actual.report.calculation().errors.len(),
                    1,
                    "formula errors are found independently of rendered visibility"
                );
                let identity = &actual.report.calculation().errors[0].identity;
                assert_eq!(
                    identity.address,
                    if case.name == "xlsx_hidden_sheet_error" {
                        "C40"
                    } else {
                        "Z99"
                    }
                );
                assert_eq!(
                    identity.sheet_ordinal,
                    if case.name == "xlsx_hidden_sheet_error" {
                        1
                    } else {
                        0
                    }
                );
                assert!(actual.report.calculation().errors[0].code > 0);
            }
            _ => unreachable!("matrix names are fixed"),
        }
        summary.push(json!({"case":case.name,"source_sha256":hash(&case.bytes),"report_sha256":actual.report.sha256(),
            "actual_pdf_sha256":hash(&actual.pdf),"pages":actual.pages.len(),"manual_layout_review_required":true,
            "generic_clipping_or_quality_pass":false,"owned_tree_retired_to_zero":true}));
    }
    fs::write(
        output.join("matrix.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    eprintln!("Synthetic actual Office evidence: {}", output.display());
    Ok(())
}
