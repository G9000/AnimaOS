//! Derive the worker's exact Windows tree pin from a previously verified manifest.
//! This helper does not launch an engine or assert current installed-file custody.
#[cfg(not(windows))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::other(
        "The engine manifest uses Windows path ordering",
    ))
}

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeSet,
        io::{self, Read},
        path::{Component, Path},
    };
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Entry {
        path: String,
        bytes: u64,
        sha256: String,
    }
    fn fail() -> io::Error {
        io::Error::other("Invalid bounded engine manifest")
    }
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(io::Error::other(
            "Expected manifest path and its exact SHA-256",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let manifest_sha256 = hex(&Sha256::digest(&bytes));
    if bytes.len() > 8 * 1024 * 1024 || args[1].to_str() != Some(&manifest_sha256) {
        return Err(fail());
    }
    let mut entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|_| fail())?;
    if entries.is_empty() || entries.len() > 20_000 {
        return Err(fail());
    }
    entries.sort_by(|a, b| Path::new(&a.path).cmp(Path::new(&b.path)));
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    let mut pin = Sha256::new();
    pin.update(b"anima-office-engine-tree-v1\0");
    for entry in &entries {
        if entry.path.is_empty()
            || entry.path.contains(['\\', ':'])
            || Path::new(&entry.path)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !seen.insert(&entry.path)
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(fail());
        }
        total = total.checked_add(entry.bytes).ok_or_else(fail)?;
        if total > 2 * 1024 * 1024 * 1024 {
            return Err(fail());
        }
        pin.update((entry.path.len() as u64).to_le_bytes());
        pin.update(entry.path.as_bytes());
        pin.update(entry.bytes.to_le_bytes());
        pin.update(entry.sha256.as_bytes());
    }
    println!(
        "{}",
        serde_json::json!({"kind":"engine_tree_pin_derived","manifest_sha256":manifest_sha256,
        "engine_tree_sha256":hex(&pin.finalize()),"file_count":entries.len(),"tree_bytes":total,
        "current_engine_files_checked":false,"engine_started":false})
    );
    Ok(())
}
