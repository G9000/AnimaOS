//! Bounded real-byte observations around ordinary, already-authorized tools.
use anima_core::completion::{
    CompletionAttempt, CompletionCheckOutcome, CompletionCheckpoint, CompletionFile,
};
use anima_core::{
    AgentConfig, AgentRuntime, CompletionReview, Content, DataValue, Evaluator, EvaluatorResult,
    Message, TaskResult, TaskStatus, ToolCall,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::workspace::{
    normalized_relative_path, resolve_workspace_search_root, workspace_root_path,
};
use super::super::workspace_bytes::WorkspaceBytes;
use super::contracts::{self, Contract};

const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct HostCompletionReview {
    core: CompletionReview,
    pub(super) contracts: Arc<Vec<Contract>>,
    root: Option<PathBuf>,
    identity: Option<String>,
}

pub(crate) struct PreparedCompletion {
    checkpoint: CompletionCheckpoint,
    workspace: Option<WorkspaceBytes>,
    input_path: Option<String>,
    read: Option<Snapshot>,
    checks: Vec<CompletionAttempt>,
    cwd: Option<PathBuf>,
}

struct Snapshot {
    file: CompletionFile,
    digest: Option<String>,
}

impl From<CompletionReview> for HostCompletionReview {
    fn from(core: CompletionReview) -> Self {
        Self {
            core,
            contracts: Arc::new(Vec::new()),
            root: None,
            identity: None,
        }
    }
}

impl HostCompletionReview {
    pub(crate) fn new(config: &AgentConfig, configured_root: Option<&Path>) -> Self {
        let root = workspace_root_path("completion review", configured_root).ok();
        let identity = root
            .as_deref()
            .and_then(|root| WorkspaceBytes::open(root).ok())
            .map(|workspace| identity(&workspace.canonical_root));
        Self {
            core: CompletionReview::default(),
            contracts: Arc::new(contracts::from_config(config)),
            root,
            identity,
        }
    }

    pub(crate) fn prepare(
        &self,
        configured_root: Option<&Path>,
        call: &ToolCall,
    ) -> PreparedCompletion {
        let workspace = workspace_root_path("completion review", configured_root)
            .ok()
            .and_then(|root| WorkspaceBytes::open(&root).ok())
            .filter(|workspace| {
                self.root.is_none()
                    || self.identity.as_deref()
                        == Some(identity(&workspace.canonical_root).as_str())
            });
        let input_path = match call.args.get("file_path") {
            Some(DataValue::String(value)) if !value.is_empty() && value.len() <= 4096 => {
                Some(value.clone())
            }
            _ => None,
        };
        let mut read = None;
        let mut checks = Vec::new();
        let mut resolved_cwd = None;
        if call.name == "bash" {
            self.refresh(workspace.as_ref());
            if let Some(workspace) = &workspace {
                if let (Some(DataValue::String(command)), Some(cwd)) =
                    (call.args.get("command"), cwd(call))
                {
                    let actual = resolve_workspace_search_root(
                        &workspace.canonical_root,
                        &cwd,
                        "completion review",
                    )
                    .ok();
                    resolved_cwd = actual.clone();
                    for contract in self.contracts.iter() {
                        let expected = resolve_workspace_search_root(
                            &workspace.canonical_root,
                            &contract.cwd,
                            "completion review",
                        )
                        .ok();
                        if command.trim() == contract.command
                            && actual.is_some()
                            && actual == expected
                        {
                            let files = self
                                .core
                                .tracked_files()
                                .into_iter()
                                .filter(|file| {
                                    file.workspace == identity(&workspace.canonical_root)
                                        && contract.covers(&file.path)
                                })
                                .collect();
                            checks.push(self.core.begin_check(&contract.id, files));
                        }
                    }
                }
            }
        } else if matches!(call.name.as_str(), "read_file" | "read_document_source") {
            let mut budget = MAX_SNAPSHOT_BYTES;
            read = workspace
                .as_ref()
                .zip(input_path.as_deref())
                .and_then(|(workspace, path)| snapshot_input(workspace, path, &mut budget));
            if let Some(snapshot) = &read {
                self.core
                    .refresh_file(&snapshot.file, snapshot.digest.clone());
            }
        }
        PreparedCompletion {
            checkpoint: self.core.checkpoint(),
            workspace,
            input_path,
            read,
            checks,
            cwd: resolved_cwd,
        }
    }

    pub(crate) fn settle(
        &self,
        prepared: PreparedCompletion,
        call: &ToolCall,
        result: &TaskResult<Content>,
    ) {
        let success = result.status == TaskStatus::Success;
        match call.name.as_str() {
            "write_file" | "write_document" | "edit_file" | "multi_edit" if success => {
                let mut budget = MAX_SNAPSHOT_BYTES;
                let snapshot = prepared
                    .workspace
                    .as_ref()
                    .zip(prepared.input_path.as_deref())
                    .and_then(|(workspace, path)| snapshot_input(workspace, path, &mut budget));
                if let Some(snapshot) = snapshot {
                    let required: Vec<_> = self
                        .contracts
                        .iter()
                        .filter(|contract| contract.covers(&snapshot.file.path))
                        .map(|contract| contract.id.clone())
                        .collect();
                    let requires_check =
                        !required.is_empty() || super::requires_check(&snapshot.file.path);
                    self.core
                        .record_edit(snapshot.file, snapshot.digest, requires_check, required);
                } else {
                    self.core.note_gap();
                }
            }
            "read_file" | "read_document_source"
                if success && call.args.get("limit") != Some(&DataValue::Number(0.0)) =>
            {
                let mut budget = MAX_SNAPSHOT_BYTES;
                let after = prepared
                    .workspace
                    .as_ref()
                    .zip(prepared.input_path.as_deref())
                    .and_then(|(workspace, path)| snapshot_input(workspace, path, &mut budget));
                if let Some(after) = after {
                    self.core.refresh_file(&after.file, after.digest.clone());
                    if let Some(before) = prepared.read {
                        if before.file == after.file && before.digest == after.digest {
                            self.core.read_back(
                                prepared.checkpoint,
                                &after.file,
                                after.digest.as_deref(),
                            );
                        }
                    }
                }
            }
            "bash" => {
                self.refresh(prepared.workspace.as_ref());
                if prepared.checks.is_empty() {
                    super::observe(&self.core, prepared.checkpoint, call, result);
                } else {
                    let cwd_unchanged = prepared
                        .workspace
                        .as_ref()
                        .zip(cwd(call))
                        .and_then(|(workspace, cwd)| {
                            resolve_workspace_search_root(
                                &workspace.canonical_root,
                                &cwd,
                                "completion review",
                            )
                            .ok()
                        })
                        .is_some_and(|cwd| prepared.cwd.as_ref() == Some(&cwd));
                    let outcome = if success && cwd_unchanged {
                        CompletionCheckOutcome::Passed
                    } else {
                        CompletionCheckOutcome::Unknown
                    };
                    for attempt in prepared.checks {
                        self.core.finish_check(attempt, outcome);
                    }
                }
            }
            "write_file"
            | "write_document"
            | "edit_file"
            | "multi_edit"
            | "read_file"
            | "read_document_source" => {}
            _ => super::observe(&self.core, prepared.checkpoint, call, result),
        }
    }

    fn refresh(&self, workspace: Option<&WorkspaceBytes>) {
        let mut budget = MAX_SNAPSHOT_BYTES;
        for file in self.core.tracked_files() {
            let digest = workspace
                .filter(|workspace| file.workspace == identity(&workspace.canonical_root))
                .and_then(|workspace| snapshot_digest(workspace, &file.path, &mut budget));
            self.core.refresh_file(&file, digest);
        }
    }

    fn refresh_bound_workspace(&self) {
        let workspace = self
            .root
            .as_deref()
            .and_then(|root| WorkspaceBytes::open(root).ok())
            .filter(|workspace| {
                self.identity.as_deref() == Some(identity(&workspace.canonical_root).as_str())
            });
        self.refresh(workspace.as_ref());
    }
}

fn cwd(call: &ToolCall) -> Option<String> {
    match call.args.get("cwd") {
        Some(DataValue::String(value)) if !value.trim().is_empty() => {
            Some(value.trim().to_string())
        }
        Some(DataValue::String(_)) | None => Some(".".into()),
        _ => None,
    }
}

fn identity(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let value = value.to_ascii_lowercase();
    value
}

fn snapshot_input(workspace: &WorkspaceBytes, input: &str, budget: &mut usize) -> Option<Snapshot> {
    let canonical = workspace.canonical_root.join(input).canonicalize().ok()?;
    let relative = normalized_relative_path(&workspace.canonical_root, &canonical).ok()?;
    #[cfg(windows)]
    let relative = relative.to_ascii_lowercase();
    if relative.is_empty() || relative.len() > 4096 {
        return None;
    }
    let digest = snapshot_digest(workspace, &relative, budget);
    Some(Snapshot {
        file: CompletionFile {
            workspace: identity(&workspace.canonical_root),
            path: relative,
        },
        digest,
    })
}

fn snapshot_digest(
    workspace: &WorkspaceBytes,
    relative: &str,
    budget: &mut usize,
) -> Option<String> {
    if *budget == 0 {
        return None;
    }
    let file_limit = if super::super::generated_documents::format_from_path(relative).is_some() {
        anima_document_content::MAX_GENERATED_DOCUMENT_BYTES
    } else {
        MAX_FILE_BYTES
    };
    let allowance = file_limit.min(*budget);
    *budget -= allowance;
    let bytes = workspace.read_file_snapshot(relative, allowance).ok()?;
    *budget += allowance.saturating_sub(bytes.len().max(1));
    Some(hex::encode(Sha256::digest(&bytes)))
}

#[async_trait]
impl Evaluator for HostCompletionReview {
    fn name(&self) -> &str {
        "completion_review"
    }
    fn description(&self) -> &str {
        "One bounded review of changed-file versions and owner-declared verification scope"
    }
    fn requires_tool_budget(&self) -> bool {
        true
    }
    async fn validate(&self, runtime: &AgentRuntime, message: &Message) -> Result<bool, String> {
        self.core.validate(runtime, message).await
    }
    async fn evaluate(
        &self,
        runtime: &AgentRuntime,
        message: &Message,
        response: &Content,
    ) -> Result<EvaluatorResult, String> {
        self.refresh_bound_workspace();
        self.core.evaluate(runtime, message, response).await
    }
}
