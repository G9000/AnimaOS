//! Public GitHub skill acquisition. Retrieved bytes remain inert import data.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anima_core::skill_packages::{
    skill_package_hash, validate_skill_package, validate_supporting_path, MAX_SKILL_PACKAGE_BYTES,
    MAX_SKILL_SUPPORTING_FILES, MAX_SKILL_SUPPORTING_FILE_BYTES,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use super::{
    parse_skill_file, SkillError, SkillService, SkillSupportingFile, MAX_SKILL_FILE_BYTES,
};

const MAX_METADATA_BYTES: usize = 1024 * 1024;
const MAX_WIRE_BYTES: usize = 48 * 1024 * 1024;
const MAX_REQUESTS: usize = 300;
const MAX_TREE_ENTRIES: usize = 1024;
const ACQUISITION_TIME: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitHubSkillSource {
    pub(crate) repository: String,
    pub(crate) commit: String,
    pub(crate) path: String,
    pub(crate) source_url: String,
    pub(crate) package_hash: String,
    pub(crate) ignored_paths: Vec<String>,
    /// Exact upstream file, including unknown metadata. Kept as inert review
    /// data so approving a package does not rewrite its manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) original_skill_text: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GitHubSkillRequest {
    pub(crate) repository: String,
    pub(crate) revision: Option<String>,
    #[serde(default)]
    pub(crate) path: String,
    pub(crate) slug: Option<String>,
}

impl GitHubSkillRequest {
    pub(crate) fn validate(&self) -> Result<(), GitHubSkillError> {
        let repository = self.repository.split('/').collect::<Vec<_>>();
        if repository.len() != 2
            || repository[0].len() > 39
            || repository[1].len() > 100
            || repository.iter().any(|part| !source_segment(part))
            || self.path.len() > 512
            || (!self.path.is_empty()
                && (self.path.split('/').count() > 12
                    || self.path.split('/').any(|part| !source_segment(part))))
            || self.revision.as_ref().is_some_and(|revision| {
                revision.is_empty()
                    || revision.len() > 200
                    || revision.split('/').any(|part| !source_segment(part))
                    || revision.contains("..")
            })
            || self
                .slug
                .as_deref()
                .is_some_and(|slug| !super::is_valid_slug(slug))
        {
            return Err(GitHubSkillError::Invalid);
        }
        Ok(())
    }
    fn api(&self, tail: &[&str], raw: bool, recursive: bool, limit: usize) -> ApiRequest {
        ApiRequest {
            segments: std::iter::once("repos")
                .chain(self.repository.split('/'))
                .chain(tail.iter().copied())
                .map(str::to_string)
                .collect(),
            raw,
            recursive,
            limit,
        }
    }
    fn raw(&self, commit: &str, path: &str, limit: usize) -> ApiRequest {
        ApiRequest {
            segments: self
                .repository
                .split('/')
                .chain(std::iter::once(commit))
                .chain(self.path.split('/').filter(|segment| !segment.is_empty()))
                .chain(path.split('/'))
                .map(str::to_string)
                .collect(),
            raw: true,
            recursive: false,
            limit,
        }
    }
}

fn source_segment(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}
fn object_id(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitHubSkillError {
    Invalid,
    Network,
    NotPublic,
    RateLimit,
    Protocol,
    Limits,
    Package,
    Timeout,
}
impl std::fmt::Display for GitHubSkillError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "Use a public GitHub owner/repository, an optional branch/tag/commit, a relative skill directory, and an optional valid slug. Do not include URLs, credentials or installer commands.",
            Self::Network => "GitHub could not be reached securely; no skill draft was saved",
            Self::NotPublic => "The GitHub repository, revision or skill directory was not found or is not public",
            Self::RateLimit => "GitHub refused public access or its unauthenticated rate limit was reached; no skill draft was saved",
            Self::Protocol => "GitHub returned incomplete or unsupported source metadata; no skill draft was saved",
            Self::Limits => "The GitHub skill exceeds source request, file, byte or package limits; no skill draft was saved",
            Self::Package => "The selected directory must contain SKILL.md and a complete portable package of supported text and binary assets. Symlinks, submodules, executable binaries, Git LFS pointers and unsafe paths cannot be imported. No skill draft was saved.",
            Self::Timeout => "GitHub skill acquisition exceeded 120 seconds; no skill draft was saved",
        })
    }
}

struct ApiRequest {
    segments: Vec<String>,
    recursive: bool,
    raw: bool,
    limit: usize,
}
impl ApiRequest {
    fn url(&self) -> Result<reqwest::Url, GitHubSkillError> {
        let mut url = reqwest::Url::parse(if self.raw {
            "https://raw.githubusercontent.com/"
        } else {
            "https://api.github.com/"
        })
        .map_err(|_| GitHubSkillError::Protocol)?;
        url.path_segments_mut()
            .map_err(|_| GitHubSkillError::Protocol)?
            .clear()
            .extend(&self.segments);
        if self.recursive {
            url.query_pairs_mut().append_pair("recursive", "1");
        }
        Ok(url)
    }
}
trait GitHubApi: Sync {
    fn read(&self, request: ApiRequest) -> BoxFuture<'_, Result<Vec<u8>, GitHubSkillError>>;
}
struct PublicGitHub {
    client: reqwest::Client,
    requests: AtomicUsize,
    bytes: AtomicUsize,
}
impl PublicGitHub {
    fn new() -> Result<Self, GitHubSkillError> {
        Ok(Self {
            client: crate::tools::web::ssrf::client_builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(5))
                .user_agent("AnimaOS GitHub skill import")
                .build()
                .map_err(|_| GitHubSkillError::Network)?,
            requests: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
        })
    }
}
impl GitHubApi for PublicGitHub {
    fn read(&self, request: ApiRequest) -> BoxFuture<'_, Result<Vec<u8>, GitHubSkillError>> {
        Box::pin(async move {
            if self.requests.fetch_add(1, Ordering::Relaxed) >= MAX_REQUESTS {
                return Err(GitHubSkillError::Limits);
            }
            let mut response = self
                .client
                .get(request.url()?)
                .header(
                    "accept",
                    if request.raw {
                        "application/octet-stream"
                    } else {
                        "application/vnd.github+json"
                    },
                )
                .send()
                .await
                .map_err(|_| GitHubSkillError::Network)?;
            match response.status().as_u16() {
                200 => {}
                404 => return Err(GitHubSkillError::NotPublic),
                401 | 403 | 429 => return Err(GitHubSkillError::RateLimit),
                _ => return Err(GitHubSkillError::Protocol),
            }
            if response
                .content_length()
                .is_some_and(|length| length > request.limit as u64)
            {
                return Err(GitHubSkillError::Limits);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| GitHubSkillError::Network)?
            {
                if bytes.len().saturating_add(chunk.len()) > request.limit
                    || self
                        .bytes
                        .fetch_add(chunk.len(), Ordering::Relaxed)
                        .saturating_add(chunk.len())
                        > MAX_WIRE_BYTES
                {
                    return Err(GitHubSkillError::Limits);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
    }
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
    commit: CommitBody,
}
#[derive(Deserialize)]
struct CommitBody {
    tree: TreeId,
}
#[derive(Deserialize)]
struct TreeId {
    sha: String,
}
#[derive(Deserialize)]
struct Tree {
    truncated: bool,
    tree: Vec<TreeEntry>,
}
#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    size: Option<usize>,
}
pub(super) struct RetrievedPackage {
    pub(super) bytes: Vec<u8>,
    pub(super) files: Vec<SkillSupportingFile>,
    pub(super) source: GitHubSkillSource,
}

pub(super) async fn acquire_public(
    request: &GitHubSkillRequest,
) -> Result<RetrievedPackage, GitHubSkillError> {
    acquire(request, &PublicGitHub::new()?).await
}

/// A bounded directory index pinned to the resolved commit. Unlike an import,
/// inspection also works at a repository root containing only a README.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InspectionEntry {
    pub(super) path: String,
    pub(super) kind: String,
    pub(super) size_bytes: Option<usize>,
    pub(super) git_blob_hash: String,
}

#[derive(Clone)]
pub(super) struct GitHubInspection {
    pub(super) request: GitHubSkillRequest,
    pub(super) commit: String,
    pub(super) entries: Vec<InspectionEntry>,
    tree_sha: String,
}

pub(super) async fn inspect_public(
    candidates: &[GitHubSkillRequest],
) -> Result<Vec<GitHubInspection>, GitHubSkillError> {
    let api = PublicGitHub::new()?;
    tokio::time::timeout(ACQUISITION_TIME, async {
        let mut resolved = Vec::new();
        for request in candidates {
            match inspect_directory(request, &api).await {
                Ok(inspection) => {
                    resolved.push(inspection);
                    if resolved.len() == 2 {
                        break;
                    }
                }
                Err(GitHubSkillError::NotPublic) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(resolved)
    })
    .await
    .map_err(|_| GitHubSkillError::Timeout)?
}

async fn inspect_directory(
    request: &GitHubSkillRequest,
    api: &impl GitHubApi,
) -> Result<GitHubInspection, GitHubSkillError> {
    request.validate()?;
    let commit: Commit = serde_json::from_slice(
        &api.read(request.api(
            &["commits", request.revision.as_deref().unwrap_or("HEAD")],
            false,
            false,
            MAX_METADATA_BYTES,
        ))
        .await?,
    )
    .map_err(|_| GitHubSkillError::Protocol)?;
    if !object_id(&commit.sha) || !object_id(&commit.commit.tree.sha) {
        return Err(GitHubSkillError::Protocol);
    }
    let mut sha = commit.commit.tree.sha;
    for segment in request
        .path
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        let parent = tree(request, api, &sha, false).await?;
        let matches: Vec<_> = parent
            .tree
            .iter()
            .filter(|entry| entry.path == segment)
            .collect();
        if matches.len() != 1 || matches[0].mode != "040000" || matches[0].kind != "tree" {
            return Err(GitHubSkillError::NotPublic);
        }
        sha.clone_from(&matches[0].sha);
    }
    let entries = inspection_entries(tree(request, api, &sha, false).await?)?;
    let mut pinned = request.clone();
    pinned.revision = Some(commit.sha.clone());
    Ok(GitHubInspection {
        request: pinned,
        commit: commit.sha,
        entries,
        tree_sha: sha,
    })
}

fn inspection_entries(tree: Tree) -> Result<Vec<InspectionEntry>, GitHubSkillError> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in tree.tree {
        if entry.path.is_empty()
            || entry.path.len() > 512
            || entry.path.contains(['/', '\\'])
            || matches!(entry.path.as_str(), "." | "..")
            || entry.path.chars().any(char::is_control)
            || !seen.insert(entry.path.to_ascii_lowercase())
        {
            return Err(GitHubSkillError::Protocol);
        }
        entries.push(InspectionEntry {
            path: entry.path,
            kind: entry.kind,
            size_bytes: entry.size,
            git_blob_hash: entry.sha,
        });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

pub(super) async fn read_inspection_file(
    inspection: &GitHubInspection,
    path: &str,
) -> Result<Vec<u8>, GitHubSkillError> {
    if let Some((parent, leaf)) = path.rsplit_once('/') {
        let api = PublicGitHub::new()?;
        let mut sha = inspection.tree_sha.clone();
        for segment in parent.split('/') {
            if !source_segment(segment) {
                return Err(GitHubSkillError::Invalid);
            }
            let parent_tree = tree(&inspection.request, &api, &sha, false).await?;
            let matches: Vec<_> = parent_tree
                .tree
                .iter()
                .filter(|entry| entry.path == segment)
                .collect();
            if matches.len() != 1 || matches[0].mode != "040000" || matches[0].kind != "tree" {
                return Err(GitHubSkillError::NotPublic);
            }
            sha.clone_from(&matches[0].sha);
        }
        let mut nested = inspection.clone();
        nested.request.path = if nested.request.path.is_empty() {
            parent.to_string()
        } else {
            format!("{}/{parent}", nested.request.path)
        };
        nested.entries = inspection_entries(tree(&inspection.request, &api, &sha, false).await?)?;
        nested.tree_sha = sha;
        return read_indexed_file(&nested, leaf, &api).await;
    }
    read_indexed_file(inspection, path, &PublicGitHub::new()?).await
}

async fn read_indexed_file(
    inspection: &GitHubInspection,
    path: &str,
    api: &impl GitHubApi,
) -> Result<Vec<u8>, GitHubSkillError> {
    // Inspections use the same exact-commit raw URL and Git blob verification
    // as imports. A moving branch can never change a later page.
    let entry = inspection
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .ok_or(GitHubSkillError::NotPublic)?;
    if entry.kind != "blob"
        || entry
            .size_bytes
            .is_none_or(|size| size > MAX_SKILL_SUPPORTING_FILE_BYTES)
    {
        return Err(GitHubSkillError::Limits);
    }
    let content = api
        .read(
            inspection
                .request
                .raw(&inspection.commit, path, MAX_SKILL_SUPPORTING_FILE_BYTES),
        )
        .await?;
    if Some(content.len()) != entry.size_bytes
        || git_blob_hash(&content) != entry.git_blob_hash.to_ascii_lowercase()
    {
        return Err(GitHubSkillError::Protocol);
    }
    Ok(content)
}
async fn tree(
    request: &GitHubSkillRequest,
    api: &impl GitHubApi,
    sha: &str,
    recursive: bool,
) -> Result<Tree, GitHubSkillError> {
    let bytes = api
        .read(request.api(&["git", "trees", sha], false, recursive, MAX_METADATA_BYTES))
        .await?;
    let tree: Tree = serde_json::from_slice(&bytes).map_err(|_| GitHubSkillError::Protocol)?;
    if tree.truncated || tree.tree.len() > MAX_TREE_ENTRIES {
        return Err(GitHubSkillError::Limits);
    }
    if tree.tree.iter().any(|entry| !object_id(&entry.sha)) {
        return Err(GitHubSkillError::Protocol);
    }
    Ok(tree)
}
async fn acquire(
    request: &GitHubSkillRequest,
    api: &impl GitHubApi,
) -> Result<RetrievedPackage, GitHubSkillError> {
    tokio::time::timeout(ACQUISITION_TIME, acquire_inner(request, api))
        .await
        .map_err(|_| GitHubSkillError::Timeout)?
}
async fn acquire_inner(
    request: &GitHubSkillRequest,
    api: &impl GitHubApi,
) -> Result<RetrievedPackage, GitHubSkillError> {
    request.validate()?;
    let commit_bytes = api
        .read(request.api(
            &["commits", request.revision.as_deref().unwrap_or("HEAD")],
            false,
            false,
            MAX_METADATA_BYTES,
        ))
        .await?;
    let commit: Commit =
        serde_json::from_slice(&commit_bytes).map_err(|_| GitHubSkillError::Protocol)?;
    if !object_id(&commit.sha) || !object_id(&commit.commit.tree.sha) {
        return Err(GitHubSkillError::Protocol);
    }
    let mut sha = commit.commit.tree.sha;
    for segment in request
        .path
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        let parent = tree(request, api, &sha, false).await?;
        let mut matches = parent
            .tree
            .into_iter()
            .filter(|entry| entry.path == segment);
        let entry = matches.next().ok_or(GitHubSkillError::NotPublic)?;
        if matches.next().is_some() || entry.mode != "040000" || entry.kind != "tree" {
            return Err(GitHubSkillError::Package);
        }
        sha = entry.sha;
    }
    let entries = tree(request, api, &sha, true).await?.tree;
    let mut seen = BTreeSet::new();
    let mut ignored = Vec::new();
    let mut blobs = Vec::new();
    let mut total = 0usize;
    let mut main = false;
    for entry in entries {
        if !seen.insert(entry.path.to_ascii_lowercase()) {
            return Err(GitHubSkillError::Package);
        }
        if entry.kind == "tree" && entry.mode == "040000" {
            validate_supporting_path(&format!("{}/directory.md", entry.path))
                .map_err(|_| GitHubSkillError::Package)?;
            continue;
        }
        if entry.kind != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755") {
            return Err(GitHubSkillError::Package);
        }
        let is_main = entry.path == "SKILL.md";
        if [".gitignore", ".gitattributes", ".DS_Store"].contains(&entry.path.as_str()) {
            ignored.push(entry.path);
            continue;
        }
        if !is_main {
            validate_supporting_path(&entry.path).map_err(|_| GitHubSkillError::Package)?;
        }
        let size = entry.size.ok_or(GitHubSkillError::Protocol)?;
        if size
            > if is_main {
                MAX_SKILL_FILE_BYTES
            } else {
                MAX_SKILL_SUPPORTING_FILE_BYTES
            }
        {
            return Err(GitHubSkillError::Limits);
        }
        total = total.checked_add(size).ok_or(GitHubSkillError::Limits)?;
        if total > MAX_SKILL_PACKAGE_BYTES {
            return Err(GitHubSkillError::Limits);
        }
        main |= is_main;
        blobs.push(entry);
    }
    if !main || blobs.len() > MAX_SKILL_SUPPORTING_FILES + 1 {
        return Err(GitHubSkillError::Package);
    }
    // Reject file/directory aliases before requesting any package bytes.
    let paths = blobs
        .iter()
        .map(|entry| entry.path.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    for path in &paths {
        for (offset, _) in path.match_indices('/') {
            if paths.contains(&path[..offset]) {
                return Err(GitHubSkillError::Package);
            }
        }
    }
    blobs.sort_by(
        |left, right| match (left.path == "SKILL.md", right.path == "SKILL.md") {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.path.cmp(&right.path),
        },
    );
    let mut bytes = Vec::new();
    let mut files = Vec::new();
    for entry in blobs {
        let limit = if entry.path == "SKILL.md" {
            MAX_SKILL_FILE_BYTES
        } else {
            MAX_SKILL_SUPPORTING_FILE_BYTES
        };
        let content = api
            // The raw host avoids spending the public REST quota per asset.
            // Its path contains the resolved commit, never a moving branch.
            .read(request.raw(&commit.sha, &entry.path, limit))
            .await?;
        if Some(content.len()) != entry.size
            || git_blob_hash(&content) != entry.sha.to_ascii_lowercase()
        {
            return Err(GitHubSkillError::Protocol);
        }
        if content.starts_with(b"version https://git-lfs.github.com/spec/v1\n")
            || content.starts_with(b"version https://git-lfs.github.com/spec/v1\r\n")
        {
            return Err(GitHubSkillError::Package);
        }
        if entry.path == "SKILL.md" {
            parse_skill_file(&content).map_err(|_| GitHubSkillError::Package)?;
            bytes = content;
        } else {
            files.push(
                SkillSupportingFile::from_bytes(entry.path, content)
                    .map_err(|_| GitHubSkillError::Package)?,
            );
        }
    }
    validate_skill_package(&bytes, &files).map_err(|_| GitHubSkillError::Package)?;
    ignored.sort();
    let source_url = format!(
        "https://github.com/{}/tree/{}{}{}",
        request.repository,
        commit.sha,
        if request.path.is_empty() { "" } else { "/" },
        request.path
    );
    let source = GitHubSkillSource {
        repository: request.repository.clone(),
        commit: commit.sha,
        path: request.path.clone(),
        source_url,
        package_hash: skill_package_hash(&bytes, &files),
        ignored_paths: ignored,
        original_skill_text: Some(
            String::from_utf8(bytes.clone()).map_err(|_| GitHubSkillError::Package)?,
        ),
    };
    Ok(RetrievedPackage {
        bytes,
        files,
        source,
    })
}

fn git_blob_hash(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hash = Sha1::new();
    hash.update(format!("blob {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    hex::encode(hash.finalize())
}

/// Preserve the imported manifest at approval. Unknown upstream front matter
/// stays data: it neither grants tools nor changes the execution policy.
pub(super) fn approved_bytes(draft: &super::SkillDraft, body: &str) -> Result<Vec<u8>, SkillError> {
    if draft.github_source.is_some() && draft.url_source.is_some() {
        return Err(SkillError::invalid(
            "A skill draft cannot have two remote sources",
        ));
    }
    if let Some(source) = &draft.url_source {
        source.validate_draft(draft)?;
    }
    let original_and_hash = draft
        .github_source
        .as_ref()
        .map(|source| {
            (
                source.original_skill_text.as_deref(),
                source.package_hash.as_str(),
            )
        })
        .or_else(|| {
            draft.url_source.as_ref().map(|source| {
                (
                    source.original_skill_text.as_deref(),
                    source.package_hash.as_str(),
                )
            })
        });
    let Some((original, package_hash)) = original_and_hash else {
        return Ok(super::compose_skill_file(&draft.name, &draft.description, body).into_bytes());
    };
    let Some(original) = original else {
        // Old imported drafts did not retain the original file.
        return Ok(super::compose_skill_file(&draft.name, &draft.description, body).into_bytes());
    };
    let parsed = parse_skill_file(original.as_bytes()).map_err(SkillError::invalid)?;
    if parsed.name != draft.name
        || parsed.description != draft.description
        || parsed.body != draft.body
        || skill_package_hash(original.as_bytes(), &draft.files) != package_hash
    {
        return Err(SkillError::invalid(
            "The original GitHub skill package no longer matches its reviewed draft",
        ));
    }
    if body == draft.body {
        return Ok(original.as_bytes().to_vec());
    }
    // The parser returns the exact body slice. Subtracting its byte length
    // retains the complete BOM, YAML header and separator line endings.
    let prefix_len = original
        .len()
        .checked_sub(parsed.body.len())
        .ok_or_else(|| SkillError::invalid("Invalid original GitHub skill body"))?;
    let mut bytes = original.as_bytes()[..prefix_len].to_vec();
    bytes.extend_from_slice(body.as_bytes());
    parse_skill_file(&bytes).map_err(SkillError::invalid)?;
    if bytes.len() > MAX_SKILL_FILE_BYTES {
        return Err(SkillError::invalid(super::SKILL_FILE_TOO_LARGE));
    }
    Ok(bytes)
}

#[derive(Clone)]
pub(super) struct OwnerImportScope {
    run: crate::runs::RunLink,
    root: PathBuf,
    call: anima_core::ToolCall,
    approved: bool,
}
impl OwnerImportScope {
    pub(super) fn new(
        run: crate::runs::RunLink,
        root: PathBuf,
        call: anima_core::ToolCall,
        approved: bool,
    ) -> Self {
        Self {
            run,
            root,
            call,
            approved,
        }
    }
    pub(super) fn check(
        &self,
        state: &crate::state::DaemonState,
        root: &Path,
    ) -> Result<(), SkillError> {
        crate::agent_runs::chat_controls::validate_owner_chat(state, &self.run).map_err(|_| {
            SkillError::conflict("GitHub import requires a live direct owner web/API chat")
        })?;
        if root != self.root
            || state
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.as_path())
                != Some(root)
        {
            return Err(SkillError::conflict(
                "Workspace changed during GitHub import; no draft was saved",
            ));
        }
        let agent = state
            .agents
            .get(&self.run.agent_id)
            .ok_or(SkillError::NotFound)?
            .state();
        let verdict = state.approval_policy_verdict(&agent, &self.run.session_id, &self.call);
        if verdict == crate::approvals::Verdict::Deny
            || (verdict == crate::approvals::Verdict::Ask && !self.approved)
        {
            return Err(SkillError::conflict(crate::approvals::DENIED_BY_POLICY));
        }
        Ok(())
    }
}
impl SkillService {
    pub(crate) async fn import_github(
        &self,
        request: GitHubSkillRequest,
        run: crate::runs::RunLink,
        call: anima_core::ToolCall,
        approved: bool,
    ) -> Result<super::SkillDraft, SkillError> {
        request
            .validate()
            .map_err(|error| SkillError::Invalid(error.to_string()))?;
        let root = self.workspace().await?;
        let scope = OwnerImportScope {
            run,
            root,
            call,
            approved,
        };
        scope.check(&*self.state.read().await, &scope.root)?;
        let api =
            PublicGitHub::new().map_err(|error| SkillError::Unavailable(error.to_string()))?;
        let package = acquire(&request, &api)
            .await
            .map_err(|error| SkillError::Unavailable(error.to_string()))?;
        let mut scoped = self.clone();
        scoped.owner_import = Some(scope);
        scoped
            .import_package_from_github(package.bytes, request.slug, package.files, package.source)
            .await
    }
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
