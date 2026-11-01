//! Original public source data for owner review. Nothing in this module executes
//! retrieved instructions, activates a draft, or treats an inspection ID as a grant.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use anima_core::primitives::now_millis;
use anima_core::skill_packages::{
    skill_package_hash, validate_skill_package, validate_supporting_path, MAX_SKILL_PACKAGE_BYTES,
    MAX_SKILL_SUPPORTING_FILES, MAX_SKILL_SUPPORTING_FILE_BYTES,
};
use futures::future::BoxFuture;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::github::{self, GitHubInspection, GitHubSkillRequest, OwnerImportScope};
use super::{
    parse_skill_file, skill_hash, SkillDraft, SkillError, SkillService, SkillSupportingFile,
    MAX_SKILL_FILE_BYTES,
};

const CAPTURE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_CAPTURES: usize = 4;
const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PAGE_CHARS: usize = 16_000;
const DEFAULT_PAGE_CHARS: usize = 8_000;
const INDEX_PAGE_ENTRIES: usize = 100;
const SOURCE_DEADLINE: Duration = Duration::from_secs(120);
const MAX_URL_REQUESTS: usize = 300;
const MAX_URL_WIRE_BYTES: usize = 48 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceInspectionRequest {
    pub(crate) source: Option<String>,
    pub(crate) revision: Option<String>,
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) supporting_paths: Vec<String>,
    pub(crate) inspection_id: Option<String>,
    pub(crate) file_path: Option<String>,
    #[serde(default)]
    pub(crate) offset: usize,
    pub(crate) max_chars: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceImportRequest {
    pub(crate) source: Option<String>,
    pub(crate) revision: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) slug: Option<String>,
    #[serde(default)]
    pub(crate) supporting_paths: Vec<String>,
    pub(crate) inspection_id: Option<String>,
}

fn invalid(message: &str) -> SkillError {
    SkillError::invalid(message)
}
fn unavailable(message: &str) -> SkillError {
    SkillError::Unavailable(message.into())
}

fn validate_selection(
    source: Option<&str>,
    revision: Option<&str>,
    path: Option<&str>,
    files: &[String],
    id: Option<&str>,
) -> Result<(), SkillError> {
    if source.is_none() && id.is_none() {
        return Err(invalid("Provide a public source or a live inspectionId"));
    }
    if id.is_some_and(|id| !id.starts_with("skill_inspection_") || id.len() > 100)
        || source.is_some_and(|source| {
            source.is_empty() || source.len() > 2048 || source.chars().any(char::is_control)
        })
        || revision.is_some_and(|revision| revision.is_empty() || revision.len() > 200)
        || path.is_some_and(|path| path.len() > 512)
        || files.len() > MAX_SKILL_SUPPORTING_FILES
    {
        return Err(invalid("Invalid or oversized source selection"));
    }
    let mut seen = BTreeSet::new();
    for file in files {
        validate_supporting_path(file).map_err(SkillError::invalid)?;
        if !seen.insert(file.to_ascii_lowercase()) || file.contains('%') {
            return Err(invalid(
                "Supporting paths must be unique safe relative package files",
            ));
        }
    }
    if let Some(source) = source {
        selection(source, revision, path)?;
    }
    Ok(())
}

impl SourceInspectionRequest {
    pub(crate) fn validate(&self) -> Result<(), SkillError> {
        validate_selection(
            self.source.as_deref(),
            self.revision.as_deref(),
            self.path.as_deref(),
            &self.supporting_paths,
            self.inspection_id.as_deref(),
        )?;
        if self
            .max_chars
            .is_some_and(|limit| limit == 0 || limit > MAX_PAGE_CHARS)
            || self.offset > MAX_SKILL_PACKAGE_BYTES
            || self
                .file_path
                .as_deref()
                .is_some_and(|path| !safe_inspection_path(path))
        {
            return Err(invalid(
                "Use a safe filePath, bounded offset and maxChars from 1 to 16000",
            ));
        }
        Ok(())
    }
}
impl SourceImportRequest {
    pub(crate) fn validate(&self) -> Result<(), SkillError> {
        validate_selection(
            self.source.as_deref(),
            self.revision.as_deref(),
            self.path.as_deref(),
            &self.supporting_paths,
            self.inspection_id.as_deref(),
        )?;
        if self
            .slug
            .as_deref()
            .is_some_and(|slug| !super::is_valid_slug(slug))
        {
            return Err(invalid(super::SKILL_SLUG_INVALID));
        }
        Ok(())
    }
}

fn safe_inspection_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path.split('/').count() <= 16
        && path.split('/').all(|part| {
            !part.is_empty()
                && !matches!(part, "." | "..")
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UrlSourceFile {
    pub(crate) path: String,
    pub(crate) source_url: String,
    pub(crate) sha256: String,
    pub(crate) size_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UrlSkillSource {
    pub(crate) requested_url: String,
    pub(crate) source_url: String,
    pub(crate) observed_at_ms: u64,
    pub(crate) package_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) original_skill_text: Option<String>,
    pub(crate) coverage: String,
    pub(crate) files: Vec<UrlSourceFile>,
}

impl UrlSkillSource {
    pub(super) fn validate_package(
        &self,
        bytes: &[u8],
        files: &[SkillSupportingFile],
    ) -> Result<(), SkillError> {
        validate_skill_package(bytes, files).map_err(SkillError::invalid)?;
        checked_document_url(&self.requested_url)?;
        let final_url = checked_document_url(&self.source_url)?;
        let root = final_url
            .join(".")
            .map_err(|_| invalid("Invalid URL package root"))?;
        if self.coverage != "referenced_files"
            || self.observed_at_ms == 0
            || self.package_hash != skill_package_hash(bytes, files)
            || self
                .original_skill_text
                .as_ref()
                .is_none_or(|text| text.as_bytes() != bytes)
            || self.files.len() != files.len() + 1
        {
            return Err(invalid(
                "The original URL skill package no longer matches its reviewed draft",
            ));
        }
        let mut expected = vec![("SKILL.md", bytes.to_vec())];
        for file in files {
            expected.push((
                &file.path,
                file.bytes().map_err(SkillError::invalid)?.into_owned(),
            ));
        }
        let mut seen = BTreeSet::new();
        for provenance in &self.files {
            let Some((_, bytes)) = expected.iter().find(|(path, _)| *path == provenance.path)
            else {
                return Err(invalid("URL source contains an unexpected package file"));
            };
            let url = checked_https_url(&provenance.source_url)?;
            if !seen.insert(provenance.path.to_ascii_lowercase())
                || provenance.sha256 != skill_hash(bytes)
                || provenance.size_bytes != bytes.len()
                || (provenance.path == "SKILL.md" && url != final_url)
                || (provenance.path != "SKILL.md" && !within_root(&root, &url))
            {
                return Err(invalid(
                    "URL file provenance does not match captured package bytes",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_draft(&self, draft: &SkillDraft) -> Result<(), SkillError> {
        let original = self
            .original_skill_text
            .as_deref()
            .ok_or_else(|| invalid("URL import is missing its original document"))?;
        self.validate_package(original.as_bytes(), &draft.files)?;
        let parsed = parse_skill_file(original.as_bytes()).map_err(SkillError::invalid)?;
        if parsed.name != draft.name
            || parsed.description != draft.description
            || parsed.body != draft.body
        {
            return Err(invalid("URL original document no longer matches its draft"));
        }
        Ok(())
    }
}

fn checked_https_url(raw: &str) -> Result<Url, SkillError> {
    // Check before URL normalization, which would otherwise erase traversal.
    if raw.len() > 2048
        || raw.contains('%')
        || raw.chars().any(char::is_control)
        || raw.contains('\\')
        || raw.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid(
            "Source URLs cannot contain traversal or backslashes",
        ));
    }
    let url = Url::parse(raw).map_err(|_| invalid("Use a public HTTPS URL"))?;
    if url.scheme() != "https"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().contains('%')
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(invalid("Use a public HTTPS URL without credentials, escapes, query, fragment or nonstandard port"));
    }
    crate::tools::web::ssrf::reject_url(&url).map_err(|reason| invalid(&reason))?;
    Ok(url)
}
fn checked_document_url(raw: &str) -> Result<Url, SkillError> {
    let url = checked_https_url(raw)?;
    if url.path_segments().and_then(|mut parts| parts.next_back()) != Some("SKILL.md") {
        return Err(invalid(
            "A direct URL source must name the original SKILL.md document",
        ));
    }
    Ok(url)
}
fn within_root(root: &Url, url: &Url) -> bool {
    root.origin() == url.origin() && url.path().starts_with(root.path())
}

#[derive(Clone)]
enum Selection {
    GitHub(Vec<GitHubSkillRequest>),
    Url(Url),
}

fn selection(
    source: &str,
    revision: Option<&str>,
    path: Option<&str>,
) -> Result<Selection, SkillError> {
    let mut repository_parts;
    let mut url_tail = None;
    if source.starts_with("https://") {
        let url = checked_https_url(source)?;
        if url.host_str() != Some("github.com") {
            if revision.is_some() || path.is_some_and(|path| !path.is_empty()) {
                return Err(invalid(
                    "revision and path apply only to GitHub repositories",
                ));
            }
            return checked_document_url(source).map(Selection::Url);
        }
        repository_parts = url
            .path()
            .trim_matches('/')
            .split('/')
            .map(str::to_string)
            .collect::<Vec<_>>();
        if repository_parts.len() > 2 {
            if !matches!(repository_parts[2].as_str(), "tree" | "blob")
                || repository_parts.len() < 4
            {
                return Err(invalid("Use a GitHub repository, tree or blob URL"));
            }
            let blob = repository_parts[2] == "blob";
            let mut tail = repository_parts.split_off(3);
            if blob {
                if tail.len() < 2 {
                    return Err(invalid(
                        "A GitHub blob URL must include a revision and file",
                    ));
                }
                tail.pop();
            }
            url_tail = Some(tail);
            repository_parts.truncate(2);
        }
    } else {
        repository_parts = source.split('/').map(str::to_string).collect::<Vec<_>>();
    }
    if repository_parts.len() < 2 {
        return Err(invalid(
            "Use a public GitHub owner/repository or HTTPS SKILL.md URL",
        ));
    }
    let repository = repository_parts[..2]
        .join("/")
        .trim_end_matches(".git")
        .to_string();
    let mut candidates = Vec::new();
    let mut add = |revision: Option<String>, path: String| -> Result<(), SkillError> {
        let request = GitHubSkillRequest {
            repository: repository.clone(),
            revision,
            path: path.trim_end_matches("/SKILL.md").to_string(),
            slug: None,
        };
        let mut request = request;
        if request.path == "SKILL.md" {
            request.path.clear();
        }
        request
            .validate()
            .map_err(|error| invalid(&error.to_string()))?;
        candidates.push(request);
        Ok(())
    };
    match (url_tail, revision) {
        (Some(tail), Some(revision)) => {
            let full = tail.join("/");
            let inferred = if full == revision {
                ""
            } else {
                full.strip_prefix(&format!("{revision}/")).ok_or_else(|| {
                    invalid(
                        "The explicit revision must match the GitHub URL before its directory path",
                    )
                })?
            };
            if path.is_some_and(|path| {
                path.trim_end_matches("/SKILL.md") != inferred.trim_end_matches("/SKILL.md")
            }) {
                return Err(invalid("The explicit path does not match the GitHub URL"));
            }
            add(Some(revision.into()), path.unwrap_or(inferred).into())?;
        }
        (Some(tail), None) => {
            if tail.len() > 14 {
                return Err(invalid("GitHub URL exceeds revision/path depth limits"));
            }
            for split in 1..=tail.len() {
                let inferred = tail[split..].join("/");
                if path.is_some_and(|path| path != inferred.trim_end_matches("/SKILL.md")) {
                    continue;
                }
                add(Some(tail[..split].join("/")), inferred)?;
            }
            if candidates.is_empty() {
                return Err(invalid(
                    "The explicit path does not match any revision/path split in the GitHub URL",
                ));
            }
        }
        (None, _) => {
            let inferred = repository_parts[2..].join("/");
            if path.is_some_and(|path| !inferred.is_empty() && path != inferred) {
                return Err(invalid("Conflicting source and path directories"));
            }
            add(
                revision.map(str::to_string),
                path.unwrap_or(&inferred).into(),
            )?;
        }
    }
    Ok(Selection::GitHub(candidates))
}

async fn resolve_github(candidates: &[GitHubSkillRequest]) -> Result<GitHubInspection, SkillError> {
    let mut resolved = github::inspect_public(candidates)
        .await
        .map_err(|error| unavailable(&error.to_string()))?;
    if resolved.len() > 1 {
        return Err(invalid("This GitHub URL has multiple valid revision/path splits; provide its revision explicitly (slash-containing branch names are supported)"));
    }
    resolved.pop().ok_or_else(|| unavailable("The public GitHub revision or directory was not found; supply the exact revision and path when the URL is ambiguous"))
}

struct Fetched {
    final_url: Url,
    bytes: Vec<u8>,
}
trait PublicHttp: Sync {
    fn fetch(&self, url: Url, limit: usize) -> BoxFuture<'_, Result<Fetched, SkillError>>;
}
struct SecureHttp {
    client: reqwest::Client,
    requests: std::sync::atomic::AtomicUsize,
    wire_bytes: std::sync::atomic::AtomicUsize,
}
impl SecureHttp {
    fn new() -> Result<Self, SkillError> {
        let client = crate::tools::web::ssrf::client_builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .user_agent("AnimaOS public skill source review")
            .build()
            .map_err(|_| unavailable("Could not create a secure public source client"))?;
        Ok(Self {
            client,
            requests: Default::default(),
            wire_bytes: Default::default(),
        })
    }
}
impl PublicHttp for SecureHttp {
    fn fetch(&self, mut url: Url, limit: usize) -> BoxFuture<'_, Result<Fetched, SkillError>> {
        Box::pin(async move {
            use std::sync::atomic::Ordering;
            for redirects in 0..=MAX_REDIRECTS {
                checked_https_url(url.as_str())?;
                if self.requests.fetch_add(1, Ordering::Relaxed) >= MAX_URL_REQUESTS {
                    return Err(unavailable("Source request limit exceeded"));
                }
                let mut response = self
                    .client
                    .get(url.clone())
                    .header("accept", "application/octet-stream")
                    .send()
                    .await
                    .map_err(|_| unavailable("Public source could not be reached securely"))?;
                if response.status().is_redirection() {
                    if redirects == MAX_REDIRECTS {
                        return Err(unavailable("Source redirect limit exceeded"));
                    }
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|header| header.to_str().ok())
                        .ok_or_else(|| unavailable("Source redirect has no valid destination"))?;
                    // Validate the reference before join can erase traversal.
                    if location.contains('%')
                        || location.contains('\\')
                        || location.split('/').any(|part| matches!(part, "." | ".."))
                    {
                        return Err(invalid("Unsafe source redirect path"));
                    }
                    url = url
                        .join(location)
                        .map_err(|_| invalid("Invalid source redirect"))?;
                    checked_https_url(url.as_str())?;
                    continue;
                }
                if response.status() != reqwest::StatusCode::OK {
                    return Err(unavailable(
                        "A required source file is unavailable; no partial package was saved",
                    ));
                }
                if response
                    .content_length()
                    .is_some_and(|length| length > limit as u64)
                {
                    return Err(unavailable("Source file byte limit exceeded"));
                }
                let mut bytes = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| unavailable("Source download was interrupted"))?
                {
                    if bytes.len().saturating_add(chunk.len()) > limit
                        || self
                            .wire_bytes
                            .fetch_add(chunk.len(), Ordering::Relaxed)
                            .saturating_add(chunk.len())
                            > MAX_URL_WIRE_BYTES
                    {
                        return Err(unavailable("Source package wire byte limit exceeded"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                return Ok(Fetched {
                    final_url: url,
                    bytes,
                });
            }
            Err(unavailable("Source redirect limit exceeded"))
        })
    }
}

#[derive(Clone)]
struct UrlPackage {
    bytes: Vec<u8>,
    files: Vec<SkillSupportingFile>,
    source: UrlSkillSource,
}

enum PendingSourceImport {
    GitHub(github::RetrievedPackage),
    Url(UrlPackage),
}

/// Dropping the caller revokes queued work. Once a registry change has begun,
/// SkillService retains its existing completion/rollback responsibility.
struct SourceImportAdmission(Arc<std::sync::atomic::AtomicBool>);
impl Drop for SourceImportAdmission {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Resolve a document-relative reference before URL normalization can erase a
/// root escape. Navigation to an already captured document is not a package file.
fn package_reference(
    root: &Url,
    document: &Url,
    target: &str,
) -> Result<Option<String>, SkillError> {
    let target = target
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(['<', '>']);
    if target.is_empty()
        || target.starts_with('#')
        || target.contains("://")
        || target.starts_with("mailto:")
    {
        return Ok(None);
    }
    let target = target.split('#').next().unwrap_or(target);
    if target.len() > 2048 || target.starts_with('/') || target.contains(['%', '\\', '?', ':']) {
        return Err(invalid(
            "Supporting references must be safe relative package paths without encoded escapes",
        ));
    }
    if !within_root(root, document) {
        return Err(invalid(
            "Supporting document is outside the captured package root",
        ));
    }
    let current = document
        .path()
        .strip_prefix(root.path())
        .ok_or_else(|| invalid("Invalid supporting document root"))?;
    let mut segments: Vec<_> = current.split('/').collect();
    segments.pop();
    for segment in target.split('/') {
        match segment {
            "." => {}
            ".." => {
                segments.pop().ok_or_else(|| {
                    invalid("Supporting reference escapes the captured package root")
                })?;
            }
            "" => {
                return Err(invalid(
                    "Supporting references cannot contain empty path segments",
                ))
            }
            _ => segments.push(segment),
        }
    }
    let path = segments.join("/");
    if path == current || path == "SKILL.md" {
        return Ok(None);
    }
    validate_supporting_path(&path).map_err(SkillError::invalid)?;
    Ok(Some(path))
}

/// Ordinary inline/reference Markdown links are required package dependencies.
/// Each fetched Markdown document contributes links relative to its final URL.
fn referenced_paths(
    text: &str,
    root: &Url,
    document: &Url,
) -> Result<BTreeSet<String>, SkillError> {
    let mut paths = BTreeSet::new();
    let inline_targets = text
        .split("](")
        .skip(1)
        .filter_map(|suffix| suffix.split_once(')').map(|(target, _)| target));
    let reference_targets = text.lines().filter_map(|line| {
        let line = line.trim_start();
        line.starts_with('[')
            .then(|| line.split_once("]:").map(|(_, target)| target.trim_start()))
            .flatten()
    });
    for target in inline_targets.chain(reference_targets) {
        if let Some(path) = package_reference(root, document, target)? {
            paths.insert(path);
            if paths.len() > MAX_SKILL_SUPPORTING_FILES {
                return Err(invalid("Too many referenced or selected supporting files"));
            }
        }
    }
    Ok(paths)
}

fn enqueue_reference(
    pending: &mut BTreeSet<String>,
    discovered: &mut BTreeMap<String, String>,
    path: String,
) -> Result<(), SkillError> {
    validate_supporting_path(&path).map_err(SkillError::invalid)?;
    let folded = path.to_ascii_lowercase();
    if let Some(previous) = discovered.get(&folded) {
        if previous != &path {
            return Err(invalid(
                "Duplicate or conflicting supporting file paths are not allowed",
            ));
        }
        return Ok(());
    }
    if discovered.len() >= MAX_SKILL_SUPPORTING_FILES {
        return Err(invalid("Too many referenced or selected supporting files"));
    }
    discovered.insert(folded, path.clone());
    pending.insert(path);
    Ok(())
}

fn supporting_markdown(path: &str, final_url: &Url) -> bool {
    [path, final_url.path()].iter().any(|path| {
        path.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
    })
}

async fn acquire_url(
    url: Url,
    selected: &[String],
    http: &impl PublicHttp,
) -> Result<UrlPackage, SkillError> {
    let requested_url = url.to_string();
    let document = http.fetch(url, MAX_SKILL_FILE_BYTES).await?;
    checked_document_url(document.final_url.as_str())?;
    let text = String::from_utf8(document.bytes.clone())
        .map_err(|_| invalid("SKILL.md must be original UTF-8 text"))?;
    parse_skill_file(&document.bytes).map_err(SkillError::invalid)?;
    let root = document
        .final_url
        .join(".")
        .map_err(|_| invalid("Invalid URL package root"))?;
    let mut pending = BTreeSet::new();
    let mut discovered = BTreeMap::new();
    for path in referenced_paths(&text, &root, &document.final_url)?
        .into_iter()
        .chain(selected.iter().cloned())
    {
        enqueue_reference(&mut pending, &mut discovered, path)?;
    }
    let mut bytes_total = document.bytes.len();
    let mut files = Vec::new();
    let mut provenance = vec![UrlSourceFile {
        path: "SKILL.md".into(),
        source_url: document.final_url.to_string(),
        sha256: skill_hash(&document.bytes),
        size_bytes: document.bytes.len(),
    }];
    while let Some(path) = pending.pop_first() {
        validate_supporting_path(&path).map_err(SkillError::invalid)?;
        if path.contains('%') {
            return Err(invalid("Encoded supporting paths are not allowed"));
        }
        let target = root
            .join(&path)
            .map_err(|_| invalid("Invalid relative supporting URL"))?;
        checked_https_url(target.as_str())?;
        if !within_root(&root, &target) {
            return Err(invalid(
                "Supporting files must stay within the final document package root",
            ));
        }
        let fetched = http
            .fetch(
                target,
                MAX_SKILL_SUPPORTING_FILE_BYTES
                    .min(MAX_SKILL_PACKAGE_BYTES.saturating_sub(bytes_total)),
            )
            .await?;
        checked_https_url(fetched.final_url.as_str())?;
        if !within_root(&root, &fetched.final_url) {
            return Err(invalid(
                "A supporting file redirect left the document package root",
            ));
        }
        bytes_total = bytes_total
            .checked_add(fetched.bytes.len())
            .ok_or_else(|| invalid("URL package byte limit exceeded"))?;
        if bytes_total > MAX_SKILL_PACKAGE_BYTES {
            return Err(invalid("URL package byte limit exceeded"));
        }
        if supporting_markdown(&path, &fetched.final_url) {
            let markdown = std::str::from_utf8(&fetched.bytes)
                .map_err(|_| invalid("Supporting Markdown must be original UTF-8 text"))?;
            for referenced in referenced_paths(markdown, &root, &fetched.final_url)? {
                enqueue_reference(&mut pending, &mut discovered, referenced)?;
            }
        }
        provenance.push(UrlSourceFile {
            path: path.clone(),
            source_url: fetched.final_url.to_string(),
            sha256: skill_hash(&fetched.bytes),
            size_bytes: fetched.bytes.len(),
        });
        files.push(
            SkillSupportingFile::from_bytes(path, fetched.bytes).map_err(SkillError::invalid)?,
        );
    }
    validate_skill_package(&document.bytes, &files).map_err(SkillError::invalid)?;
    let source = UrlSkillSource {
        requested_url,
        source_url: document.final_url.to_string(),
        observed_at_ms: now_millis(),
        package_hash: skill_package_hash(&document.bytes, &files),
        original_skill_text: Some(text),
        coverage: "referenced_files".into(),
        files: provenance,
    };
    source.validate_package(&document.bytes, &files)?;
    Ok(UrlPackage {
        bytes: document.bytes,
        files,
        source,
    })
}

#[derive(Clone)]
enum CapturedData {
    GitHub {
        inspection: GitHubInspection,
        default_path: Option<String>,
        default_bytes: Vec<u8>,
    },
    Url(UrlPackage),
}
#[derive(Clone)]
struct CapturedSource {
    id: String,
    state: Weak<tokio::sync::RwLock<crate::state::DaemonState>>,
    root: PathBuf,
    profile_paths: Vec<PathBuf>,
    agent_id: String,
    session_id: String,
    requested_source: String,
    requested_revision: Option<String>,
    requested_path: Option<String>,
    supporting_paths: Vec<String>,
    captured_at_ms: u64,
    expires: Instant,
    data: CapturedData,
}
impl CapturedSource {
    fn storage_bytes(&self) -> usize {
        match &self.data {
            CapturedData::GitHub {
                inspection,
                default_bytes,
                ..
            } => {
                default_bytes.len()
                    + inspection
                        .entries
                        .iter()
                        .map(|entry| entry.path.len() + entry.git_blob_hash.len() + 128)
                        .sum::<usize>()
            }
            CapturedData::Url(package) => {
                package.bytes.len() * 2
                    + package
                        .files
                        .iter()
                        .map(|file| file.content.len() + file.path.len() + 128)
                        .sum::<usize>()
                    + package
                        .source
                        .files
                        .iter()
                        .map(|file| {
                            file.path.len() + file.source_url.len() + file.sha256.len() + 128
                        })
                        .sum::<usize>()
                    + 8192
            }
        }
    }
}

static CAPTURES: OnceLock<Mutex<VecDeque<Arc<CapturedSource>>>> = OnceLock::new();
fn captures() -> &'static Mutex<VecDeque<Arc<CapturedSource>>> {
    CAPTURES.get_or_init(|| Mutex::new(VecDeque::new()))
}
fn insert_capture(capture: CapturedSource) -> Result<Arc<CapturedSource>, SkillError> {
    let capture = Arc::new(capture);
    if capture.storage_bytes() > MAX_CAPTURE_BYTES {
        return Err(invalid(
            "Source capture exceeds the native review memory bound",
        ));
    }
    let mut cache = captures()
        .lock()
        .map_err(|_| unavailable("Source capture cache is unavailable"))?;
    cache.retain(|entry| {
        Arc::strong_count(entry) > 1
            || (entry.expires > Instant::now() && entry.state.strong_count() > 0)
    });
    while cache.len() >= MAX_CAPTURES
        || cache
            .iter()
            .map(|entry| entry.storage_bytes())
            .sum::<usize>()
            .saturating_add(capture.storage_bytes())
            > MAX_CAPTURE_BYTES
    {
        let Some(index) = cache.iter().position(|entry| Arc::strong_count(entry) == 1) else {
            return Err(SkillError::conflict("Source review cache is busy; finish a current review before capturing another source"));
        };
        cache.remove(index);
    }
    cache.push_back(Arc::clone(&capture));
    Ok(capture)
}

impl SkillService {
    async fn source_authority(
        &self,
        scope: &OwnerImportScope,
        root: &std::path::Path,
    ) -> Result<Vec<PathBuf>, SkillError> {
        let owner = self.profile_authority().await?;
        if self
            .evaluation_admission
            .as_ref()
            .is_some_and(|admit| !admit())
        {
            return Err(SkillError::conflict("Source call is no longer admitted"));
        }
        let guard = self.state.read().await;
        guard
            .ensure_profile_owned()
            .map_err(|error| unavailable(&error.to_string()))?;
        scope.check(&guard, root)?;
        Ok(owner
            .map(|owner| owner.protected_paths())
            .unwrap_or_default())
    }

    async fn source_capture(
        &self,
        request: &SourceInspectionRequest,
        run: &crate::runs::RunLink,
        scope: &OwnerImportScope,
        root: PathBuf,
    ) -> Result<Arc<CapturedSource>, SkillError> {
        let profile_paths = self.source_authority(scope, &root).await?;
        if let Some(id) = &request.inspection_id {
            let capture = {
                let mut cache = captures().lock().map_err(|_| unavailable("Source capture cache is unavailable"))?;
                cache.retain(|entry| Arc::strong_count(entry) > 1 || (entry.expires > Instant::now() && entry.state.strong_count() > 0));
                cache.iter().find(|entry| entry.id == *id && entry.expires > Instant::now() && entry.state.upgrade().is_some_and(|state| Arc::ptr_eq(&state, &self.state)) && entry.root == root && entry.profile_paths == profile_paths && entry.agent_id == run.agent_id && entry.session_id == run.session_id).cloned()
            }.ok_or_else(|| SkillError::conflict("Inspection expired or belongs to another owner/profile/workspace; inspect the source again"))?;
            if request.source.as_ref().is_some_and(|source| *source != capture.requested_source)
                || request.revision.as_ref().is_some_and(|revision| Some(revision) != capture.requested_revision.as_ref() && !matches!(&capture.data, CapturedData::GitHub { inspection, .. } if &inspection.commit == revision))
                || request.path.as_ref().is_some_and(|path| Some(path) != capture.requested_path.as_ref())
                || (!request.supporting_paths.is_empty() && request.supporting_paths != capture.supporting_paths)
            { return Err(SkillError::conflict("Inspection selection changed; use the captured selection or inspect again")); }
            return Ok(capture);
        }
        let requested_source = request
            .source
            .as_deref()
            .ok_or_else(|| invalid("Provide a source"))?;
        let data = match selection(
            requested_source,
            request.revision.as_deref(),
            request.path.as_deref(),
        )? {
            Selection::GitHub(candidates) => {
                if !request.supporting_paths.is_empty() {
                    return Err(invalid("GitHub imports capture the complete directory; supportingPaths applies to direct URLs"));
                }
                let inspection = resolve_github(&candidates).await?;
                let preferred_file = Url::parse(requested_source).ok().and_then(|url| {
                    let segments: Vec<_> = url.path_segments()?.collect();
                    (segments.get(2).copied() == Some("blob"))
                        .then(|| segments.last().map(|file| (*file).to_string()))
                        .flatten()
                });
                if preferred_file.as_ref().is_some_and(|file| {
                    !inspection
                        .entries
                        .iter()
                        .any(|entry| &entry.path == file && entry.kind == "blob")
                }) {
                    return Err(unavailable("The file named by the GitHub blob URL was not found at the resolved commit"));
                }
                let default_path = preferred_file.or_else(|| {
                    inspection
                        .entries
                        .iter()
                        .find(|entry| entry.path == "SKILL.md")
                        .or_else(|| {
                            inspection.entries.iter().find(|entry| {
                                entry.kind == "blob" && entry.path.eq_ignore_ascii_case("README.md")
                            })
                        })
                        .map(|entry| entry.path.clone())
                });
                let default_bytes = match &default_path {
                    Some(path) => github::read_inspection_file(&inspection, path)
                        .await
                        .map_err(|error| unavailable(&error.to_string()))?,
                    None => Vec::new(),
                };
                CapturedData::GitHub {
                    inspection,
                    default_path,
                    default_bytes,
                }
            }
            Selection::Url(url) => CapturedData::Url(
                acquire_url(url, &request.supporting_paths, &SecureHttp::new()?).await?,
            ),
        };
        if self.source_authority(scope, &root).await? != profile_paths {
            return Err(SkillError::conflict(
                "Profile changed during source inspection",
            ));
        }
        insert_capture(CapturedSource {
            id: format!("skill_inspection_{}", uuid::Uuid::new_v4()),
            state: Arc::downgrade(&self.state),
            root,
            profile_paths,
            agent_id: run.agent_id.clone(),
            session_id: run.session_id.clone(),
            requested_source: requested_source.into(),
            requested_revision: request.revision.clone(),
            requested_path: request.path.clone(),
            supporting_paths: request.supporting_paths.clone(),
            captured_at_ms: now_millis(),
            expires: Instant::now() + CAPTURE_TTL,
            data,
        })
    }

    pub(crate) async fn inspect_source(
        &self,
        request: SourceInspectionRequest,
        run: crate::runs::RunLink,
        call: anima_core::ToolCall,
        approved: bool,
    ) -> Result<Value, SkillError> {
        request.validate()?;
        tokio::time::timeout(SOURCE_DEADLINE, async {
            let root = self.workspace().await?;
            let scope = OwnerImportScope::new(run.clone(), root.clone(), call, approved);
            let capture = self
                .source_capture(&request, &run, &scope, root.clone())
                .await?;
            let response = inspection_page(&capture, &request).await?;
            if self.source_authority(&scope, &root).await? != capture.profile_paths {
                return Err(SkillError::conflict(
                    "Profile changed during source inspection",
                ));
            }
            Ok(response)
        })
        .await
        .map_err(|_| unavailable("Source inspection exceeded 120 seconds"))?
    }

    pub(crate) async fn import_source(
        &self,
        request: SourceImportRequest,
        run: crate::runs::RunLink,
        call: anima_core::ToolCall,
        approved: bool,
    ) -> Result<SkillDraft, SkillError> {
        self.import_source_with_budget(request, run, call, approved, SOURCE_DEADLINE)
            .await
    }

    async fn import_source_with_budget(
        &self,
        request: SourceImportRequest,
        run: crate::runs::RunLink,
        call: anima_core::ToolCall,
        approved: bool,
        budget: Duration,
    ) -> Result<SkillDraft, SkillError> {
        request.validate()?;
        let deadline = Instant::now() + budget;
        let live = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let _admission = SourceImportAdmission(Arc::clone(&live));
        let upstream_admission = self.evaluation_admission.clone();
        let mut scoped = self.clone();
        scoped.evaluation_admission = Some(Arc::new(move || {
            live.load(std::sync::atomic::Ordering::Acquire)
                && Instant::now() < deadline
                && upstream_admission.as_ref().is_none_or(|admit| admit())
        }));
        let root = scoped.workspace().await?;
        let scope = OwnerImportScope::new(run.clone(), root.clone(), call, approved);
        scoped.owner_import = Some(scope.clone());
        // Only acquisition is cancelled by this timeout. Await mutation to its
        // end; its queued admission is additionally revoked on Drop/deadline.
        let acquired = tokio::time::timeout(budget, async {
            // A repository README inspection may select a skill directory later,
            // still using the resolved commit captured by that inspection.
            let inspect = SourceInspectionRequest {
                source: request.source.clone(),
                revision: request.revision.clone(),
                path: if request.inspection_id.is_some() {
                    None
                } else {
                    request.path.clone()
                },
                supporting_paths: request.supporting_paths.clone(),
                inspection_id: request.inspection_id.clone(),
                ..Default::default()
            };
            let capture = scoped
                .source_capture(&inspect, &run, &scope, root.clone())
                .await?;
            let acquired = match &capture.data {
                CapturedData::GitHub { inspection, .. } => {
                    let mut pinned = inspection.request.clone();
                    if request.inspection_id.is_some() {
                        if let Some(path) = &request.path {
                            pinned.path = path.trim_end_matches("/SKILL.md").to_string();
                            if pinned.path == "SKILL.md" {
                                pinned.path.clear();
                            }
                        }
                    }
                    pinned
                        .validate()
                        .map_err(|error| invalid(&error.to_string()))?;
                    let package = github::acquire_public(&pinned)
                        .await
                        .map_err(|error| unavailable(&error.to_string()))?;
                    PendingSourceImport::GitHub(package)
                }
                CapturedData::Url(package) => {
                    if request.path.as_deref().is_some_and(|path| !path.is_empty()) {
                        return Err(invalid("URL captures cannot change their package path"));
                    }
                    PendingSourceImport::Url(package.clone())
                }
            };
            if scoped.source_authority(&scope, &root).await? != capture.profile_paths {
                return Err(SkillError::conflict("Profile changed during source import"));
            }
            Ok::<_, SkillError>(acquired)
        })
        .await
        .map_err(|_| {
            unavailable("Source acquisition exceeded its deadline; no skill draft was saved")
        })??;
        match acquired {
            PendingSourceImport::GitHub(package) => {
                scoped
                    .import_package_from_github(
                        package.bytes,
                        request.slug,
                        package.files,
                        package.source,
                    )
                    .await
            }
            PendingSourceImport::Url(package) => {
                scoped
                    .import_package_from_url(
                        package.bytes,
                        request.slug,
                        package.files,
                        package.source,
                    )
                    .await
            }
        }
    }
}

async fn inspection_page(
    capture: &CapturedSource,
    request: &SourceInspectionRequest,
) -> Result<Value, SkillError> {
    let (source, index, default_path, bytes) = match &capture.data {
        CapturedData::GitHub {
            inspection,
            default_path,
            default_bytes,
        } => {
            let selected = request.file_path.as_ref().or(default_path.as_ref());
            let bytes = if selected == default_path.as_ref() {
                default_bytes.clone()
            } else if let Some(path) = selected {
                github::read_inspection_file(inspection, path)
                    .await
                    .map_err(|error| unavailable(&error.to_string()))?
            } else {
                Vec::new()
            };
            (
                json!({"kind":"github", "repository":inspection.request.repository, "commit":inspection.commit, "path":inspection.request.path, "sourceUrl":format!("https://github.com/{}/tree/{}{}{}", inspection.request.repository, inspection.commit, if inspection.request.path.is_empty() { "" } else { "/" }, inspection.request.path), "coverage":"directory_index", "immutableCommit":true}),
                serde_json::to_value(&inspection.entries)
                    .map_err(|_| unavailable("Could not encode source index"))?,
                selected.cloned(),
                bytes,
            )
        }
        CapturedData::Url(package) => {
            let path = request.file_path.as_deref().unwrap_or("SKILL.md");
            let bytes = if path == "SKILL.md" {
                package.bytes.clone()
            } else {
                package
                    .files
                    .iter()
                    .find(|file| file.path == path)
                    .ok_or(SkillError::NotFound)?
                    .bytes()
                    .map_err(SkillError::invalid)?
                    .into_owned()
            };
            (
                json!({"kind":"url", "requestedUrl":package.source.requested_url,"sourceUrl":package.source.source_url,"observedAtMs":package.source.observed_at_ms,"packageHash":package.source.package_hash,"coverage":"referenced_files","immutableCommit":false}),
                serde_json::to_value(&package.source.files)
                    .map_err(|_| unavailable("Could not encode URL index"))?,
                Some(path.into()),
                bytes,
            )
        }
    };
    let index = index
        .as_array()
        .ok_or_else(|| unavailable("Invalid source index"))?;
    let index_offset = if request.file_path.is_none() {
        request.offset.min(index.len())
    } else {
        0
    };
    let end = (index_offset + INDEX_PAGE_ENTRIES).min(index.len());
    let limit = request.max_chars.unwrap_or(DEFAULT_PAGE_CHARS);
    let text = std::str::from_utf8(&bytes)
        .ok()
        .filter(|text| !text.contains('\0'));
    let text_offset = if request.file_path.is_some() {
        request.offset
    } else {
        0
    };
    let (page, total_chars, next_offset) = match text {
        Some(text) => {
            let total = text.chars().count();
            if text_offset > total {
                return Err(invalid(
                    "Text offset exceeds the captured file character count",
                ));
            }
            let page: String = text.chars().skip(text_offset).take(limit).collect();
            let next = text_offset + page.chars().count();
            (Some(page), total, (next < total).then_some(next))
        }
        None => (None, 0, None),
    };
    Ok(
        json!({"inspectionId":capture.id,"capturedAtMs":capture.captured_at_ms,"expiresAtMs":capture.captured_at_ms + CAPTURE_TTL.as_millis() as u64,"untrusted":true,"source":source,"files":&index[index_offset..end],"fileCount":index.len(),"filesOffset":index_offset,"filesNextOffset":(end < index.len()).then_some(end),"filePath":default_path,"sha256":skill_hash(&bytes),"sizeBytes":bytes.len(),"text":page,"binary":text.is_none(),"offset":text_offset,"totalChars":total_chars,"nextOffset":next_offset}),
    )
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
