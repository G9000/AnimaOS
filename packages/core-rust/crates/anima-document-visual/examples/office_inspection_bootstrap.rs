#[cfg(not(windows))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows is required",
    ))
}

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeSet,
        fs,
        io::{self, Read},
        path::{Component, Path, PathBuf},
        sync::mpsc,
        time::{Duration, Instant},
    };
    use win_custody::{anonymous_pipe, read_to_end_bounded, Job, JobLimits, Launch, WaitOutcome};

    fn fail(message: &str) -> io::Error {
        io::Error::other(message)
    }
    fn hash(path: &Path) -> io::Result<String> {
        let mut file = fs::File::open(path)?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    fn regular(path: &Path) -> io::Result<fs::Metadata> {
        use std::os::windows::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_attributes() & 0x400 != 0 || !metadata.is_file() {
            return Err(fail("Manifest member is not a regular non-reparse file"));
        }
        Ok(metadata)
    }
    fn walk(root: &Path, path: &Path, files: &mut BTreeSet<String>) -> io::Result<()> {
        use std::os::windows::fs::MetadataExt;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(fail("Engine tree has a reparse point"));
            }
            if metadata.is_dir() {
                walk(root, &entry.path(), files)?;
            } else if metadata.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| fail("Engine path escaped"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative);
                if files.len() > 20000 {
                    return Err(fail("Engine file count exceeds limit"));
                }
            } else {
                return Err(fail("Engine tree has a nonregular entry"));
            }
        }
        Ok(())
    }
    fn file_uri(path: &Path) -> io::Result<String> {
        let path = path
            .to_str()
            .ok_or_else(|| fail("Profile path is not UTF-8"))?
            .replace('\\', "/");
        let path = path.strip_prefix("//?/").unwrap_or(&path);
        if path.starts_with('/') || path.as_bytes().get(1) != Some(&b':') {
            return Err(fail("Profile needs a local absolute drive path"));
        }
        let mut uri = String::from("file:///");
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/:._-~".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                uri.push_str(&format!("%{byte:02X}"));
            }
        }
        Ok(uri)
    }
    fn run(
        executable: &Path,
        args: &[String],
        program: &Path,
        scratch: &Path,
        python_home: Option<&Path>,
    ) -> io::Result<String> {
        let limits = JobLimits::new()
            .kill_on_close(true)
            .active_processes(8)
            .process_memory(512 * 1024 * 1024)
            .job_memory(1024 * 1024 * 1024);
        let job = Job::create(&limits)?;
        if job.limits()? != limits {
            return Err(fail("Job limit readback differs"));
        }
        let (read, write) = anonymous_pipe()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let result = read_to_end_bounded(&read, 4096);
            let _ = sender.send(result);
        });
        let system =
            std::env::var_os("SystemRoot").ok_or_else(|| fail("Windows runtime root missing"))?;
        let mut launch = Launch::new(executable);
        launch
            .args(args)
            .env_clear()
            .env("SystemRoot", &system)
            .env("WINDIR", &system)
            .env(
                "PATH",
                std::env::join_paths([
                    program.to_path_buf(),
                    PathBuf::from(&system).join("System32"),
                ])
                .map_err(|_| fail("Invalid runtime path"))?,
            )
            .env("TEMP", scratch)
            .env("TMP", scratch)
            .env("USERPROFILE", scratch)
            .env("APPDATA", scratch.join("appdata"))
            .env("LOCALAPPDATA", scratch.join("localappdata"))
            .current_dir(program)
            .stdout(&write)
            .stderr(&write);
        if let Some(core) = python_home {
            launch
                .env("PYTHONHOME", core)
                .env(
                    "PYTHONPATH",
                    std::env::join_paths([core.join("lib"), program.to_path_buf()])
                        .map_err(|_| fail("Invalid trusted Python path"))?,
                )
                .env("UNO_PATH", program)
                .env(
                    "URE_BOOTSTRAP",
                    format!(
                        "vnd.sun.star.pathname:{}",
                        program.join("fundamental.ini").display()
                    ),
                );
        }
        let suspended = launch.spawn_suspended()?;
        job.assign(suspended.process())?;
        if job.active_process_count()? != 1 {
            return Err(fail("Child is not the sole initial job member"));
        }
        let process = suspended.resume()?;
        drop(launch);
        drop(write);
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut timed_out = false;
        while process.wait(Some(Duration::from_millis(100)))? != WaitOutcome::Signaled {
            if Instant::now() >= deadline {
                timed_out = true;
                break;
            }
        }
        let exit_code = process.exit_code()?;
        job.terminate(1)?;
        let retire = Instant::now() + Duration::from_secs(5);
        while job.active_process_count()? != 0 {
            if Instant::now() >= retire {
                return Err(fail("Whole-job retirement could not be proved"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| fail("Pipe reader did not retire"))??;
        reader.join().map_err(|_| fail("Pipe reader panicked"))?;
        if timed_out || exit_code != Some(0) || output.len() > 4096 {
            return Err(io::Error::other(format!("Bootstrap command {:?} failed after retirement: timed_out={timed_out}, exit_code={exit_code:?}, output={:?}", executable.file_name(), String::from_utf8_lossy(&output))));
        }
        String::from_utf8(output).map_err(|_| fail("Bootstrap returned invalid UTF-8"))
    }

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err(fail(
            "Expected engine-root, manifest-file, expected-manifest-sha256, owned-output-root",
        ));
    }
    let engine = PathBuf::from(&args[0]).canonicalize()?;
    let manifest = PathBuf::from(&args[1]);
    let expected = args[2]
        .to_str()
        .ok_or_else(|| fail("Invalid manifest hash"))?;
    let mut manifest_bytes = Vec::new();
    fs::File::open(&manifest)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut manifest_bytes)?;
    let manifest_hash: String = Sha256::digest(&manifest_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if manifest_bytes.len() > 8 * 1024 * 1024 || manifest_hash != expected {
        return Err(fail("Manifest hash or bound differs"));
    }
    let entries: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).map_err(|_| fail("Manifest is not valid JSON"))?;
    let entries = entries
        .as_array()
        .filter(|entries| !entries.is_empty() && entries.len() <= 20000)
        .ok_or_else(|| fail("Invalid manifest entries"))?;
    let mut declared = BTreeSet::new();
    let mut total = 0_u64;
    for entry in entries {
        let path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| fail("Manifest path missing"))?;
        if path.contains(['\\', ':'])
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || !declared.insert(path.to_owned())
        {
            return Err(fail("Manifest path is unsafe or duplicated"));
        }
        let bytes = entry
            .get("bytes")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| fail("Manifest length missing"))?;
        let digest = entry
            .get("sha256")
            .and_then(|v| v.as_str())
            .ok_or_else(|| fail("Manifest hash missing"))?;
        total = total
            .checked_add(bytes)
            .ok_or_else(|| fail("Manifest bytes overflow"))?;
        if total > 2 * 1024 * 1024 * 1024
            || regular(&engine.join(path))?.len() != bytes
            || hash(&engine.join(path))? != digest
        {
            return Err(fail("Engine file identity or total byte limit differs"));
        }
    }
    let mut observed = BTreeSet::new();
    walk(&engine, &engine, &mut observed)?;
    if observed != declared {
        return Err(fail("Engine tree differs from full manifest"));
    }
    for required in [
        "program/soffice.com",
        "program/soffice.bin",
        "program/python.exe",
        "program/pyuno.pyd",
    ] {
        if !declared.contains(required) {
            return Err(fail("Required engine executable/bridge missing"));
        }
    }
    let core_members: Vec<_> = declared
        .iter()
        .filter(|path| {
            path.starts_with("program/python-core-") && path.ends_with("/bin/python.exe")
        })
        .collect();
    if core_members.len() != 1 {
        return Err(fail("Manifest needs one exact bundled core Python"));
    }
    let core_executable = engine.join(core_members[0]);
    let core = core_executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| fail("Invalid core Python path"))?;
    let output_root = PathBuf::from(&args[3]).canonicalize()?;
    let scratch = tempfile::Builder::new()
        .prefix("office-bootstrap-")
        .tempdir_in(output_root)?;
    for child in ["profile", "appdata", "localappdata"] {
        fs::create_dir(scratch.path().join(child))?;
    }
    let program = engine.join("program");
    let version = run(
        &program.join("soffice.com"),
        &[
            "--version".into(),
            format!(
                "-env:UserInstallation={}",
                file_uri(&scratch.path().join("profile"))?
            ),
        ],
        &program,
        scratch.path(),
        None,
    )?;
    if !version.contains("LibreOffice") {
        return Err(fail("Version output does not identify LibreOffice"));
    }
    let import = run(&core_executable, &["-s".into(), "-S".into(), "-P".into(), "-B".into(), "-c".into(), "import os,sys;assert sys.flags.safe_path and sys.flags.no_site and sys.flags.no_user_site;dll=os.add_dll_directory(sys.argv[1]);import uno;import pyuno;print('ANIMA_UNO_IMPORT_OK')".into(), program.to_string_lossy().into_owned()], &program, scratch.path(), Some(core))?;
    if import.trim() != "ANIMA_UNO_IMPORT_OK" {
        return Err(fail("UNO import did not confirm availability"));
    }
    println!(
        "{}",
        serde_json::json!({"kind":"bootstrap_observed","manifest_sha256":expected,"file_count":declared.len(),"tree_bytes":total,"version_output":version.trim(),"uno_import":"observed","source_documents_opened":false,"whole_tree_retired":true,"os_network_isolation":false})
    );
    Ok(())
}
