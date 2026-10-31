//! The only converter path: preflight original bytes, pinned Python/UNO, native PDF.
use super::{
    configuration_digest, digest, invalid, EngineConfig, InspectionOutput, Request, CONTROLLER,
    MAX_OUTPUT, MAX_PAGES, MAX_PDF, MAX_PNG, MAX_RESPONSE_HEADER,
};
use anima_document_content::OfficeFormat;
use anima_document_visual::inspection::{
    admit_office, build_report, CalculationEvidence, RendererIdentity,
};
use anima_document_visual::{render_pdf_pages, PdfPageSelection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant as NativeInstant};

const UNAVAILABLE: &str =
    "Document inspection engine is unavailable or its trusted identity changed";
const REJECTED: &str =
    "Document inspection rejects unsupported, active, or externally linked Office content";
const FAILED: &str = "Document inspection engine did not produce complete bounded evidence";
const MAX_TREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TREE_FILES: usize = 20_000;
const MAX_PRIVATE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PRIVATE_ENTRIES: usize = 512;

#[derive(Serialize)]
struct ControllerRequest<'a> {
    version: u8,
    format: OfficeFormat,
    source: &'a Path,
    source_sha256: &'a str,
    pdf: &'a Path,
    result: &'a Path,
    profile: &'a Path,
    soffice: &'a Path,
    pipe_name: String,
    formulas: &'a [anima_document_visual::inspection::FormulaIdentity],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerResult {
    version: u8,
    source_sha256: String,
    engine_version: String,
    calculation: CalculationEvidence,
}

pub(super) fn inspect(
    request: &Request,
    source: &[u8],
    started: NativeInstant,
) -> Result<InspectionOutput, &'static str> {
    // ZIP/XML/formula/PDF/PNG parsing occurs only in this physically owned process.
    let admission = admit_office(request.format, source).map_err(|_| REJECTED)?;
    let allowed =
        anima_document_content::check_office_document_text(request.format, source, |text| {
            !crate::tools::generated_documents::credential_fields(text)
                && crate::foreign_sessions::redact_secrets(text) == text
        })
        .map_err(|_| REJECTED)?;
    if !allowed {
        return Err("Document inspection retained content failed privacy screening");
    }
    let (program, mut engine_files) = trusted_program(&request.engine, started)?;
    let executable_sha256 =
        hash_file(&program.join("soffice.bin"), started).map_err(|_| UNAVAILABLE)?;
    let font_manifest_sha256 =
        font_manifest(&request.engine.tree_sha256, &mut engine_files, started)?;
    // The physical owner supplies one private, short working directory and is
    // its sole cleanup owner after all processes and pipes have retired.
    let root = std::env::current_dir().map_err(|_| FAILED)?;
    let extension = match request.format {
        OfficeFormat::Docx => "docx",
        OfficeFormat::Xlsx => "xlsx",
        OfficeFormat::Pptx => "pptx",
        _ => return Err(REJECTED),
    };
    let input = root.join(format!("original.{extension}"));
    let pdf_path = root.join("export.pdf");
    let result_path = root.join("result.json");
    let profile = root.join("profile");
    let controller_path = root.join("controller.py");
    let descriptor_path = root.join("request.json");
    fs::create_dir(&profile).map_err(|_| FAILED)?;
    write_new(&input, source)?;
    let _source_lock = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&input)
        .map_err(|_| FAILED)?;
    write_new(&controller_path, CONTROLLER.as_bytes())?;
    let soffice = program.join("soffice.com");
    let descriptor = ControllerRequest {
        version: 1,
        format: request.format,
        source: &input,
        source_sha256: admission.source_sha256(),
        pdf: &pdf_path,
        result: &result_path,
        profile: &profile,
        soffice: &soffice,
        pipe_name: format!("anima_inspection_{}", uuid::Uuid::new_v4().simple()),
        formulas: admission.formulas(),
    };
    let descriptor_bytes = serde_json::to_vec(&descriptor).map_err(|_| FAILED)?;
    if descriptor_bytes.len() > MAX_RESPONSE_HEADER {
        return Err(FAILED);
    }
    write_new(&descriptor_path, &descriptor_bytes)?;
    let configuration_sha256 = configuration_digest(&request.engine).map_err(|_| FAILED)?;
    run_controller(&program, &controller_path, &descriptor_path, &root, started)?;
    check_deadline(started)?;
    // Existing engine files stay read-locked, and the complete tree must still
    // match after conversion. Newly created executable caches are not pinned.
    let installation = program.parent().ok_or(UNAVAILABLE)?;
    let (after, _postflight_files) =
        tree_manifest(installation, started).map_err(|_| UNAVAILABLE)?;
    if after != request.engine.tree_sha256 {
        return Err(UNAVAILABLE);
    }
    if hash_file(&input, started).map_err(|_| FAILED)? != admission.source_sha256() {
        return Err(FAILED);
    }
    let result: ControllerResult =
        serde_json::from_slice(&read_bounded(&result_path, MAX_RESPONSE_HEADER)?)
            .map_err(|_| FAILED)?;
    if result.version != 1
        || result.source_sha256 != admission.source_sha256()
        || result.engine_version.is_empty()
        || result.engine_version.len() > 64
        || result.engine_version.chars().any(char::is_control)
    {
        return Err(FAILED);
    }
    let pdf = read_bounded(&pdf_path, MAX_PDF)?;
    let extracted = anima_document_content::extract_pdf(&pdf);
    let count = extracted.pdf_coverage.ok_or(FAILED)?.page_count;
    if count == 0 || count > anima_document_visual::MAX_PDF_PAGES {
        return Err(FAILED);
    }
    let selected: Vec<u32> = (1..=count.min(MAX_PAGES as u32)).collect();
    let mut pages = Vec::with_capacity(selected.len());
    let mut total = 0usize;
    for pair in selected.chunks(2) {
        check_deadline(started)?;
        let selection = PdfPageSelection::new(pair.to_vec()).map_err(|_| FAILED)?;
        let rendered = render_pdf_pages(&pdf, &selection).map_err(|_| FAILED)?;
        if rendered.source_sha256 != digest(&pdf) || rendered.page_count != count {
            return Err(FAILED);
        }
        for page in rendered.pages {
            total = total.checked_add(page.png.len()).ok_or(FAILED)?;
            if page.png.len() > MAX_PNG || total > MAX_OUTPUT {
                return Err(FAILED);
            }
            pages.push(page);
        }
    }
    check_deadline(started)?;
    let renderer = RendererIdentity {
        backend: "libreoffice_uno".into(),
        version: result.engine_version,
        executable_sha256,
        configuration_sha256,
        font_manifest_sha256,
        rasterizer: rasterizer_identity()?,
    };
    let report = build_report(
        &request.source_version,
        &admission,
        renderer,
        &pdf,
        &pages,
        result.calculation,
    )
    .map_err(|_| FAILED)?;
    let report_bytes = serde_json::to_vec(&report).map_err(|_| FAILED)?;
    if report_bytes.len() > MAX_RESPONSE_HEADER
        || total.checked_add(report_bytes.len()).ok_or(FAILED)? > MAX_OUTPUT
    {
        return Err(FAILED);
    }
    Ok(InspectionOutput { report, pages })
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| FAILED)?;
    file.write_all(bytes).map_err(|_| FAILED)?;
    file.flush().map_err(|_| FAILED)
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FAILED)?;
    if !metadata.is_file()
        || is_reparse(&metadata)
        || metadata.len() == 0
        || metadata.len() > limit as u64
    {
        return Err(FAILED);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|_| FAILED)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FAILED)?;
    if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
        return Err(FAILED);
    }
    Ok(bytes)
}
fn check_deadline(started: NativeInstant) -> Result<(), &'static str> {
    if started.elapsed() >= Duration::from_secs(60) {
        Err(FAILED)
    } else {
        Ok(())
    }
}
fn rasterizer_identity() -> Result<String, &'static str> {
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        .map_err(|_| UNAVAILABLE)?;
    let build: String = key
        .get_value("CurrentBuildNumber")
        .map_err(|_| UNAVAILABLE)?;
    let revision: u32 = key.get_value("UBR").map_err(|_| UNAVAILABLE)?;
    if build.is_empty() || build.len() > 16 || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(UNAVAILABLE);
    }
    Ok(format!("windows_pdf:os-build-{build}.{revision}:1600px:v1"))
}
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & 0x400 != 0
}

fn trusted_program(
    config: &EngineConfig,
    started: NativeInstant,
) -> Result<(PathBuf, Vec<File>), &'static str> {
    use std::path::{Component, Prefix};
    if !matches!(config.program_directory.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return Err(UNAVAILABLE);
    }
    for path in config.program_directory.ancestors() {
        if is_reparse(&fs::symlink_metadata(path).map_err(|_| UNAVAILABLE)?) {
            return Err(UNAVAILABLE);
        }
    }
    let program = fs::canonicalize(&config.program_directory).map_err(|_| UNAVAILABLE)?;
    let installation = program.parent().ok_or(UNAVAILABLE)?;
    let (actual, files) = tree_manifest(installation, started).map_err(|_| UNAVAILABLE)?;
    if actual != config.tree_sha256 {
        return Err(UNAVAILABLE);
    }
    for required in [
        "python.exe",
        "soffice.com",
        "soffice.bin",
        "uno.py",
        "pyuno.pyd",
        "fundamental.ini",
    ] {
        let metadata = fs::symlink_metadata(program.join(required)).map_err(|_| UNAVAILABLE)?;
        if !metadata.is_file() || is_reparse(&metadata) {
            return Err(UNAVAILABLE);
        }
    }
    Ok((program, files))
}

fn hash_file(path: &Path, started: NativeInstant) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut bytes = [0; 64 * 1024];
    loop {
        check_deadline(started).map_err(io::Error::other)?;
        let count = file.read(&mut bytes)?;
        if count == 0 {
            return Ok(hex::encode(hash.finalize()));
        }
        total += count as u64;
        if total > MAX_TREE_BYTES {
            return Err(invalid());
        }
        hash.update(&bytes[..count]);
    }
}

fn tree_manifest(root: &Path, started: NativeInstant) -> io::Result<(String, Vec<File>)> {
    let mut entries = Vec::new();
    let mut total = 0u64;
    collect_files(root, root, 0, &mut entries, &mut total, started)?;
    entries.sort();
    let mut manifest = Sha256::new();
    let mut files = Vec::with_capacity(entries.len());
    manifest.update(b"anima-office-engine-tree-v1\0");
    for relative in entries {
        check_deadline(started).map_err(io::Error::other)?;
        let path = root.join(&relative);
        let name = relative.to_str().ok_or_else(invalid)?.replace('\\', "/");
        // Withhold write/delete sharing until conversion and rasterization finish.
        let mut file = OpenOptions::new().read(true).share_mode(1).open(&path)?;
        let metadata = file.metadata()?;
        if is_reparse(&metadata) || !metadata.is_file() {
            return Err(invalid());
        }
        manifest.update((name.len() as u64).to_le_bytes());
        manifest.update(name.as_bytes());
        manifest.update(metadata.len().to_le_bytes());
        let mut hash = Sha256::new();
        let mut actual = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            check_deadline(started).map_err(io::Error::other)?;
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            actual += count as u64;
            if actual > MAX_TREE_BYTES {
                return Err(invalid());
            }
            hash.update(&buffer[..count]);
        }
        if actual != metadata.len() {
            return Err(invalid());
        }
        manifest.update(hex::encode(hash.finalize()).as_bytes());
        files.push(file);
    }
    Ok((hex::encode(manifest.finalize()), files))
}
fn collect_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    entries: &mut Vec<PathBuf>,
    total: &mut u64,
    started: NativeInstant,
) -> io::Result<()> {
    check_deadline(started).map_err(io::Error::other)?;
    if depth > 32 || is_reparse(&fs::symlink_metadata(directory)?) {
        return Err(invalid());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if is_reparse(&metadata) {
            return Err(invalid());
        }
        if metadata.is_dir() {
            collect_files(root, &entry.path(), depth + 1, entries, total, started)?;
        } else if metadata.is_file() {
            *total = total.checked_add(metadata.len()).ok_or_else(invalid)?;
            if *total > MAX_TREE_BYTES || entries.len() >= MAX_TREE_FILES {
                return Err(invalid());
            }
            entries.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| invalid())?
                    .to_path_buf(),
            );
        } else {
            return Err(invalid());
        }
    }
    Ok(())
}

fn font_manifest(
    engine_tree_sha256: &str,
    files: &mut Vec<File>,
    started: NativeInstant,
) -> Result<String, &'static str> {
    let windows = std::env::var_os("SystemRoot").ok_or(UNAVAILABLE)?;
    let mut manifest = Sha256::new();
    manifest.update(b"anima-office-font-files-v1\0");
    let (hash, mut fonts) =
        tree_manifest(&PathBuf::from(windows).join("Fonts"), started).map_err(|_| UNAVAILABLE)?;
    manifest.update(hash.as_bytes());
    files.append(&mut fonts);
    if let Some(user) = std::env::var_os("ANIMA_INSPECTION_USER_FONT_DIRECTORY") {
        let user = PathBuf::from(user);
        match fs::symlink_metadata(&user) {
            Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {
                let (hash, mut fonts) = tree_manifest(&user, started).map_err(|_| UNAVAILABLE)?;
                manifest.update(hash.as_bytes());
                files.append(&mut fonts);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                manifest.update(b"no-user-font-directory")
            }
            _ => return Err(UNAVAILABLE),
        }
    }
    manifest.update(engine_tree_sha256.as_bytes());
    Ok(hex::encode(manifest.finalize()))
}

fn private_size(
    directory: &Path,
    depth: usize,
    entries: &mut usize,
    bytes: &mut u64,
) -> Result<(), &'static str> {
    if depth > 16 {
        return Err(FAILED);
    }
    let directory_entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(FAILED),
    };
    for entry in directory_entries {
        let entry = entry.map_err(|_| FAILED)?;
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(FAILED),
        };
        *entries += 1;
        *bytes = bytes.checked_add(metadata.len()).ok_or(FAILED)?;
        if *entries > MAX_PRIVATE_ENTRIES || *bytes > MAX_PRIVATE_BYTES || is_reparse(&metadata) {
            return Err(FAILED);
        }
        if (entry.file_name() == "export.pdf" && metadata.len() > MAX_PDF as u64)
            || (entry.file_name() == "result.json" && metadata.len() > MAX_RESPONSE_HEADER as u64)
        {
            return Err(FAILED);
        }
        if metadata.is_dir() {
            private_size(&entry.path(), depth + 1, entries, bytes)?;
        }
    }
    Ok(())
}

fn bounded_drain(mut pipe: impl Read, overflow: &AtomicBool) {
    let mut bytes = [0; 512];
    let mut total = 0usize;
    loop {
        match pipe.read(&mut bytes) {
            Ok(0) => return,
            Ok(count) => {
                total += count;
                if total > 4096 {
                    overflow.store(true, Ordering::Release);
                    return;
                }
            }
            Err(_) => {
                overflow.store(true, Ordering::Release);
                return;
            }
        }
    }
}

fn run_controller(
    program: &Path,
    script: &Path,
    descriptor: &Path,
    directory: &Path,
    started: NativeInstant,
) -> Result<(), &'static str> {
    check_deadline(started)?;
    let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or(UNAVAILABLE)?);
    let search_path = std::env::join_paths([program, system.join("System32").as_path()])
        .map_err(|_| UNAVAILABLE)?;
    let core = bundled_python(program)?;
    let python_path =
        std::env::join_paths([core.join("lib"), program.to_path_buf()]).map_err(|_| UNAVAILABLE)?;
    // This nonstandard bundled layout requires PYTHONHOME. -I would ignore it.
    // Clear all caller settings, then disable user/site hooks and unsafe paths.
    // The wrapper also discards inherited stdio, so launch its pinned core.
    let mut command = Command::new(core.join("bin/python.exe"));
    command
        .args(["-s", "-S", "-P", "-B"])
        .arg(script)
        .arg(descriptor)
        .env_clear()
        .current_dir(directory)
        .env("PATH", search_path)
        .env("SystemRoot", &system)
        .env("WINDIR", &system)
        .env("PYTHONHOME", &core)
        .env("PYTHONPATH", python_path)
        // LibreOffice embeds another Python interpreter. The controller's -B
        // flag is not inherited by that interpreter, but this fixed setting is.
        .env("PYTHONDONTWRITEBYTECODE", "1")
        // A crash cannot create unpinned Breakpad output in the installation.
        // env_clear also excludes CRASH_DUMP_ENABLE, whose nonempty values
        // override this bootstrap setting even when the value is "0".
        .env("CrashDumpEnable", "false")
        .env("TEMP", directory)
        .env("TMP", directory)
        .env("USERPROFILE", directory)
        .env("APPDATA", directory)
        .env("LOCALAPPDATA", directory)
        .env("UNO_PATH", program)
        .env(
            "URE_BOOTSTRAP",
            format!(
                "vnd.sun.star.pathname:{}",
                program.join("fundamental.ini").display()
            ),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|_| UNAVAILABLE)?;
    let stdout = child.stdout.take().ok_or(FAILED)?;
    let stderr = child.stderr.take().ok_or(FAILED)?;
    let overflow = AtomicBool::new(false);
    // The fixed controller's office process has null stdio, so only Python owns these pipes.
    std::thread::scope(|scope| {
        let out = scope.spawn(|| bounded_drain(stdout, &overflow));
        let err = scope.spawn(|| bounded_drain(stderr, &overflow));
        let result = loop {
            if check_deadline(started).is_err()
                || overflow.load(Ordering::Acquire)
                || private_size(directory, 0, &mut 0, &mut 0).is_err()
            {
                let _ = child.kill();
                let _ = child.wait();
                break Err(FAILED);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err(FAILED)
                    }
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(FAILED);
                }
            }
        };
        if out.join().is_err() || err.join().is_err() || overflow.load(Ordering::Acquire) {
            return Err(FAILED);
        }
        result
    })
}

fn bundled_python(program: &Path) -> Result<PathBuf, &'static str> {
    let mut selected = None;
    for entry in fs::read_dir(program)
        .map_err(|_| UNAVAILABLE)?
        .take(MAX_TREE_FILES + 1)
    {
        let entry = entry.map_err(|_| UNAVAILABLE)?;
        let name = entry.file_name();
        let Some(version) = name
            .to_str()
            .and_then(|name| name.strip_prefix("python-core-"))
        else {
            continue;
        };
        if version.is_empty()
            || !version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            || selected.is_some()
        {
            return Err(UNAVAILABLE);
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| UNAVAILABLE)?;
        if !metadata.is_dir() || is_reparse(&metadata) {
            return Err(UNAVAILABLE);
        }
        selected = Some(entry.path());
    }
    let core = selected.ok_or(UNAVAILABLE)?;
    for path in [core.join("bin"), core.join("lib")] {
        let metadata = fs::symlink_metadata(path).map_err(|_| UNAVAILABLE)?;
        if !metadata.is_dir() || is_reparse(&metadata) {
            return Err(UNAVAILABLE);
        }
    }
    let metadata = fs::symlink_metadata(core.join("bin/python.exe")).map_err(|_| UNAVAILABLE)?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(UNAVAILABLE);
    }
    Ok(core)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_python_requires_one_pinned_version_directory_and_regular_executable() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path();
        assert!(bundled_python(program).is_err());
        let core = program.join("python-core-3.13.17");
        fs::create_dir_all(core.join("bin")).unwrap();
        fs::create_dir(core.join("lib")).unwrap();
        fs::write(core.join("bin/python.exe"), b"fixture only; never launched").unwrap();
        assert_eq!(bundled_python(program).unwrap(), core);
        fs::create_dir(program.join("python-core-3.13.18")).unwrap();
        assert!(
            bundled_python(program).is_err(),
            "ambiguous core must not select an engine"
        );
    }
}
