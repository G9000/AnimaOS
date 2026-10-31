//! Host-owned source custody and acknowledged proposal transitions; no parser runs in the host.
use super::{
    approval::{self, DocumentEditOwnerApproval, DocumentEditWitness},
    projection::{self, DocumentEditReview, OfficeInventoryPage, SourceLabel},
    DocumentEditDraft, DocumentEditRegistry, DocumentEditScope, DocumentEditSource,
    DocumentEditStatus, DocumentEditStore, DocumentPublicationPhase, DocumentPublicationReceipt,
    ExpectedDestination,
};
use crate::{
    app::SharedDaemonState,
    attachments::AttachmentRecord,
    runs::{RunLink, RunSource, RunStatus},
    state::DaemonState,
    tools::workspace_bytes::{InstructionReadError, WorkspaceBytes},
};
use anima_core::{CancelSignal, ToolCall};
use anima_document_content::{OfficeEditSpec, OfficeFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_HTML_BYTES: usize = 1024 * 1024;
const LIFETIME_MS: u64 = 30 * 60 * 1000;
pub(crate) const UNCERTAIN: &str =
    "Document publication needs recovery; effects may have occurred. Do not repeat this write.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SourceReference {
    Attachment { attachment_id: String },
    Workspace { file_path: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposeRequest {
    pub(crate) source: SourceReference,
    pub(crate) expected_sha256: String,
    pub(crate) file_path: String,
    pub(crate) expected_destination: ExpectedDestination,
    pub(crate) operations: Vec<anima_document_content::OfficeEditOperation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayArguments {
    proposal_id: String,
    review_sha256: String,
    file_path: String,
    expected_destination: ExpectedDestination,
}
#[derive(Clone)]
pub(crate) struct DocumentEditService {
    pub(super) state: SharedDaemonState,
    pub(super) transactions: Arc<Mutex<()>>,
}
#[derive(Clone)]
pub(crate) struct WeakDocumentEditService {
    state: std::sync::Weak<tokio::sync::RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
}
impl WeakDocumentEditService {
    pub(crate) fn upgrade(&self) -> Option<DocumentEditService> {
        Some(DocumentEditService {
            state: self.state.upgrade()?,
            transactions: self.transactions.clone(),
        })
    }
}
pub(crate) struct ApplyingDocument {
    pub(crate) draft: DocumentEditDraft,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn format(path: &str) -> Result<OfficeFormat, String> {
    match path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("docx") => Ok(OfficeFormat::Docx),
        Some("xlsx") => Ok(OfficeFormat::Xlsx),
        Some("pptx") => Ok(OfficeFormat::Pptx),
        Some("html" | "htm") => Ok(OfficeFormat::Html),
        _ => Err("Document editing supports DOCX, XLSX, PPTX and UTF-8 HTML".into()),
    }
}
pub(crate) fn safe_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > 1024
        || path.trim() != path
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| {
            matches!(part, "" | "." | "..")
                || crate::tools::generated_documents::excluded_component(part)
        })
        || path
            .split('/')
            .next()
            .is_some_and(|part| part.eq_ignore_ascii_case("uploads"))
        || crate::foreign_sessions::redact_secrets(path) != path
    {
        return Err("Document path is unavailable or excluded".into());
    }
    format(path).map(drop)
}
fn label(source: &DocumentEditSource, attachment: Option<&AttachmentRecord>) -> SourceLabel {
    match source {
        DocumentEditSource::Attachment { .. } => SourceLabel {
            kind: "attachment".into(),
            display_name: attachment
                .map(|row| row.name.clone())
                .unwrap_or_else(|| "Uploaded document".into()),
        },
        DocumentEditSource::Workspace { file_path, .. } => SourceLabel {
            kind: "workspace".into(),
            display_name: file_path.clone(),
        },
    }
}
pub(super) fn screened(value: &impl Serialize) -> Result<(), String> {
    fn sensitive(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(text) => {
                crate::tools::generated_documents::credential_fields(text)
                    || crate::foreign_sessions::redact_secrets(text) != *text
            }
            serde_json::Value::Array(values) => values.iter().any(sensitive),
            serde_json::Value::Object(values) => values.values().any(sensitive),
            _ => false,
        }
    }
    if serde_json::to_value(value).map_or(true, |value| sensitive(&value)) {
        Err("Documents containing credential fields cannot be edited or previewed".into())
    } else {
        Ok(())
    }
}

pub(crate) fn scope_in(
    state: &DaemonState,
    run: &RunLink,
    tool: &str,
) -> Result<DocumentEditScope, String> {
    state
        .ensure_profile_owned()
        .map_err(|_| "Document profile ownership was lost")?;
    let store = state
        .document_edit_store
        .as_ref()
        .ok_or("Private document editing storage is unavailable")?;
    super::workspace_boundary::ensure_state_separate(
        state,
        state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path.as_path()),
    )?;
    let session = state
        .sessions
        .get(&run.agent_id, &run.session_id)
        .ok_or("Document owner session is unavailable")?;
    let record = state
        .runs
        .get(&run.run_id)
        .filter(|row| row.agent_id == run.agent_id && row.session_id == run.session_id)
        .ok_or("Document owner run is unavailable")?;
    let agent = state
        .agents
        .get(&run.agent_id)
        .ok_or("Document owner is unavailable")?;
    if record.source != RunSource::Web
        || record.parent_run_id.is_some()
        || record.stop.is_some()
        || !matches!(
            record.status,
            RunStatus::Running | RunStatus::AwaitingApproval
        )
        || crate::agent_runs::is_helper_config(agent.config())
        || session.kind != crate::sessions::SessionKind::Chat
        || session.origin != crate::sessions::SessionOrigin::Web
        || session.pending_clear_run_id.is_some()
        || !agent.config().allows_tool(tool)
        || state.pending_history_deletions.iter().any(|row| {
            row.agent_id == run.agent_id
                && row
                    .session_id
                    .as_ref()
                    .is_none_or(|id| id == &run.session_id)
        })
    {
        return Err("Document editing requires a currently granted ordinary owner chat".into());
    }
    crate::projects::validate_active_lease(state, record)
        .map_err(|_| "Document project authority changed")?;
    let root = &state
        .workspace
        .as_ref()
        .ok_or("Configure a workspace before editing documents")?
        .root_path;
    let workspace = WorkspaceBytes::open(root).map_err(|_| "Document workspace is unavailable")?;
    Ok(DocumentEditScope {
        profile_key: store.profile_key(),
        workspace_key: crate::workspace_file_preview::workspace_key(&workspace.canonical_root),
        agent_id: run.agent_id.clone(),
        session_id: run.session_id.clone(),
        clear_generation: session.clear_generation,
        run_id: run.run_id.clone(),
    })
}
fn source_now(
    root: &std::path::Path,
    source: &DocumentEditSource,
    attachment: Option<&AttachmentRecord>,
) -> Result<Vec<u8>, String> {
    let bytes = match source {
        DocumentEditSource::Attachment {
            attachment_id,
            sha256,
        } => {
            let row = attachment
                .filter(|row| row.id == *attachment_id && row.sha256 == *sha256)
                .ok_or("Document upload custody changed")?;
            crate::attachments::files::read_attachment_bytes(root, row)
                .map_err(|_| "Document upload changed or is unavailable")?
        }
        DocumentEditSource::Workspace { file_path, .. } => {
            safe_path(file_path)?;
            workspace_snapshot(root, file_path)?.ok_or("Document source disappeared")?
        }
    };
    if bytes.is_empty() || bytes.len() > MAX_BYTES || digest(&bytes) != source.sha256() {
        return Err("Document source changed; inspect it again".into());
    }
    Ok(bytes)
}
pub(crate) fn workspace_snapshot(
    root: &std::path::Path,
    path: &str,
) -> Result<Option<Vec<u8>>, String> {
    safe_path(path)?;
    let workspace = WorkspaceBytes::open(root).map_err(|_| "Document workspace is unavailable")?;
    let limit = if format(path)? == OfficeFormat::Html {
        MAX_HTML_BYTES
    } else {
        MAX_BYTES
    };
    match workspace.read_file_snapshot_with_policy(path, limit, |name, metadata| {
        !crate::tools::generated_documents::excluded_component(name)
            && !crate::tools::generated_documents::hidden(metadata)
    }) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(InstructionReadError::Missing) => Ok(None),
        Err(_) => Err("Document file is unavailable or excluded".into()),
    }
}
pub(crate) fn destination_matches(
    bytes: Option<&[u8]>,
    expected: &ExpectedDestination,
) -> Result<(), String> {
    match (bytes, expected) {
        (None, ExpectedDestination::Absent) => Ok(()),
        (Some(bytes), ExpectedDestination::Present { sha256 }) if digest(bytes) == *sha256 => {
            Ok(())
        }
        _ => Err("Document destination changed; propose and review again".into()),
    }
}

impl DocumentEditService {
    pub(crate) async fn inspect_artifact(
        &self,
        run: &RunLink,
        execution: &anima_core::execution_limits::ExecutionContext,
        request: crate::document_inspections::InspectionRequest,
        cancel: CancelSignal,
    ) -> Result<crate::document_inspections::InspectionEnvelope, String> {
        if let Some(context) = self.preview_context(run).await? {
            use crate::document_inspections::InspectionSubject;
            let (source, hash) = match &request.subject {
                InspectionSubject::Workspace {
                    file_path,
                    expected_sha256,
                } => (
                    SourceReference::Workspace {
                        file_path: file_path.clone(),
                    },
                    expected_sha256.clone(),
                ),
                InspectionSubject::Attachment {
                    attachment_id,
                    expected_sha256,
                } => (
                    SourceReference::Attachment {
                        attachment_id: attachment_id.clone(),
                    },
                    expected_sha256.clone(),
                ),
                InspectionSubject::Candidate { proposal_id, .. } => {
                    let state = self.state.read().await;
                    let record = state
                        .document_edits
                        .get(proposal_id)
                        .ok_or("Document inspection candidate is unavailable")?;
                    let source = match &record.draft.source {
                        DocumentEditSource::Workspace { file_path, .. } => {
                            SourceReference::Workspace {
                                file_path: file_path.clone(),
                            }
                        }
                        DocumentEditSource::Attachment { attachment_id, .. } => {
                            SourceReference::Attachment {
                                attachment_id: attachment_id.clone(),
                            }
                        }
                    };
                    (source, record.draft.source.sha256().to_owned())
                }
            };
            context.bind_source(&source, &hash)?;
        }
        crate::document_inspections::DocumentInspectionService::new(self.state.clone())
            .inspect_for_run(run, execution, request, cancel)
            .await
    }

    pub(crate) fn weak(&self) -> WeakDocumentEditService {
        WeakDocumentEditService {
            state: Arc::downgrade(&self.state),
            transactions: self.transactions.clone(),
        }
    }
    pub(crate) fn new(state: SharedDaemonState, transactions: Arc<Mutex<()>>) -> Self {
        Self {
            state,
            transactions,
        }
    }
    /// Retires only host-named unreferenced staging after restored metadata is installed.
    pub(crate) async fn cleanup_staging(&self) -> Result<(), String> {
        let store = self
            .state
            .read()
            .await
            .document_edit_store
            .clone()
            .ok_or("Document storage is unavailable")?;
        for hash in store.candidate_hashes()? {
            self.retire(&hash);
        }
        for id in store.readback_proof_ids()? {
            self.retire_proof(&id);
        }
        Ok(())
    }
    /// Authenticated owner downloads require current server provenance and exact actual bytes.
    pub(crate) async fn published_download(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        if format(path).is_err() {
            return Ok(None);
        }
        safe_path(path)?;
        let (root, profile, workspace_key, owner) = {
            let state = self.state.read().await;
            state
                .ensure_profile_owned()
                .map_err(|_| "Document profile ownership was lost")?;
            let Some(store) = state.document_edit_store.as_ref() else {
                return Ok(None);
            };
            let root = state
                .workspace
                .as_ref()
                .ok_or("Document workspace is unavailable")?
                .root_path
                .clone();
            let workspace =
                WorkspaceBytes::open(&root).map_err(|_| "Document workspace is unavailable")?;
            (
                root,
                store.profile_key(),
                crate::workspace_file_preview::workspace_key(&workspace.canonical_root),
                state.profile_owner(),
            )
        };
        if let Some(owner) = &owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| "Document profile ownership was lost")?;
        }
        let source_root = root.clone();
        let source_path = path.to_string();
        let cancel = CancelSignal::new();
        let bytes = crate::attachments::documents::source_read::read_retained(
            move || workspace_snapshot(&source_root, &source_path),
            &cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document download read is unavailable")??
        .ok_or("Document download is unavailable")?;
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| "Document profile ownership was lost")?;
        }
        let source_root = root.clone();
        let source_path = path.to_string();
        let current = crate::attachments::documents::source_read::read_retained(
            move || workspace_snapshot(&source_root, &source_path),
            &cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document download read is unavailable")??
        .ok_or("Document download disappeared")?;
        if current != bytes {
            return Err("Published document changed during download".into());
        }
        let state = self.state.read().await;
        state
            .ensure_profile_owned()
            .map_err(|_| "Document profile ownership was lost")?;
        let current_root = &state
            .workspace
            .as_ref()
            .ok_or("Document workspace is unavailable")?
            .root_path;
        let current_workspace =
            WorkspaceBytes::open(current_root).map_err(|_| "Document workspace is unavailable")?;
        if current_root != &root
            || state
                .document_edit_store
                .as_ref()
                .map(DocumentEditStore::profile_key)
                .as_deref()
                != Some(profile.as_str())
            || crate::workspace_file_preview::workspace_key(&current_workspace.canonical_root)
                != workspace_key
        {
            return Err("Document download authority changed".into());
        }
        let Some(receipt) =
            state
                .document_edits
                .publication(&profile, &workspace_key, path, &digest(&bytes))
        else {
            return Ok(None);
        };
        if receipt.size_bytes != bytes.len() as u64 {
            return Err("Published document changed".into());
        }
        Ok(Some(bytes))
    }
    /// Owner routes call this only after authentication; arbitrary imported files yield no preview.
    pub(crate) async fn published_preview(
        &self,
        path: &str,
        cursor: Option<&str>,
    ) -> Result<Option<OfficeInventoryPage>, String> {
        safe_path(path)?;
        let (root, profile, workspace_key, worker, owner) = {
            let state = self.state.read().await;
            state
                .ensure_profile_owned()
                .map_err(|_| "Document profile ownership was lost")?;
            let Some(store) = state.document_edit_store.as_ref() else {
                return Ok(None);
            };
            let root = state
                .workspace
                .as_ref()
                .ok_or("Document workspace is unavailable")?
                .root_path
                .clone();
            let workspace =
                WorkspaceBytes::open(&root).map_err(|_| "Document workspace is unavailable")?;
            (
                root,
                store.profile_key(),
                crate::workspace_file_preview::workspace_key(&workspace.canonical_root),
                state.document_edit_worker.clone(),
                state.profile_owner(),
            )
        };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| "Document profile ownership was lost")?;
        }
        let source_root = root.clone();
        let source_path = path.to_string();
        let cancel = CancelSignal::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let bytes = crate::attachments::documents::source_read::read_retained(
            move || workspace_snapshot(&source_root, &source_path),
            &cancel,
            deadline,
        )
        .await
        .map_err(|_| "Document preview read is unavailable")??
        .ok_or("Document preview is unavailable")?;
        let hash = digest(&bytes);
        let receipt = {
            let state = self.state.read().await;
            state
                .document_edits
                .publication(&profile, &workspace_key, path, &hash)
                .cloned()
        };
        let Some(receipt) = receipt else {
            return Ok(None);
        };
        if receipt.size_bytes != bytes.len() as u64 {
            return Err("Published document changed".into());
        }
        let inventory = worker
            .inspect(format(path)?, bytes, cancel.clone(), deadline)
            .await?;
        screened(&inventory)?;
        let source_root = root.clone();
        let source_path = path.to_string();
        let observed = crate::attachments::documents::source_read::read_retained(
            move || workspace_snapshot(&source_root, &source_path),
            &cancel,
            deadline,
        )
        .await
        .map_err(|_| "Document preview read is unavailable")??
        .ok_or("Document preview disappeared")?;
        if digest(&observed) != hash {
            return Err("Published document changed during preview".into());
        }
        let state = self.state.read().await;
        state
            .ensure_profile_owned()
            .map_err(|_| "Document profile ownership was lost")?;
        let current_root = &state
            .workspace
            .as_ref()
            .ok_or("Document workspace is unavailable")?
            .root_path;
        let current_workspace =
            WorkspaceBytes::open(current_root).map_err(|_| "Document workspace is unavailable")?;
        if current_root != &root
            || state
                .document_edit_store
                .as_ref()
                .map(DocumentEditStore::profile_key)
                .as_deref()
                != Some(profile.as_str())
            || crate::workspace_file_preview::workspace_key(&current_workspace.canonical_root)
                != workspace_key
            || state
                .document_edits
                .publication(&profile, &workspace_key, path, &hash)
                != Some(&receipt)
        {
            return Err("Document preview authority changed".into());
        }
        projection::inventory_page(
            &inventory,
            &hash,
            SourceLabel {
                kind: "workspace".into(),
                display_name: path.into(),
            },
            &format!("{profile}:{workspace_key}:{path}"),
            cursor,
        )
        .map(Some)
    }
    async fn checked_scope(
        &self,
        run: &RunLink,
        tool: &str,
        cancel: &CancelSignal,
    ) -> Result<DocumentEditScope, String> {
        if cancel.is_cancelled() {
            return Err(cancel.tool_result().into());
        }
        let owner = { self.state.read().await.profile_owner() };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| "Document profile ownership was lost")?;
        }
        if cancel.is_cancelled() {
            return Err(cancel.tool_result().into());
        }
        scope_in(&*self.state.read().await, run, tool)
    }
    async fn resolve(
        &self,
        run: &RunLink,
        tool: &str,
        source: SourceReference,
        cancel: &CancelSignal,
    ) -> Result<
        (
            DocumentEditScope,
            PathBuf,
            DocumentEditSource,
            Option<AttachmentRecord>,
            Vec<u8>,
            OfficeFormat,
        ),
        String,
    > {
        let scope = self.checked_scope(run, tool, cancel).await?;
        let (root, history, selected) = {
            let state = self.state.read().await;
            (
                state
                    .workspace
                    .as_ref()
                    .ok_or("Document workspace is unavailable")?
                    .root_path
                    .clone(),
                state.history.store(),
                state
                    .runs
                    .get(&run.run_id)
                    .ok_or("Document run is unavailable")?
                    .effective_input()
                    .attachment_ids
                    .clone(),
            )
        };
        let (source, attachment, format) = match source {
            SourceReference::Workspace { file_path } => {
                safe_path(&file_path)?;
                let format = format(&file_path)?;
                let root_copy = root.clone();
                let path = file_path.clone();
                let bytes = crate::attachments::documents::source_read::read_retained(
                    move || workspace_snapshot(&root_copy, &path),
                    cancel,
                    tokio::time::Instant::now() + Duration::from_secs(10),
                )
                .await
                .map_err(|_| "Document read is unavailable")??
                .ok_or("Document source is unavailable")?;
                (
                    DocumentEditSource::Workspace {
                        file_path,
                        sha256: digest(&bytes),
                    },
                    None,
                    format,
                )
            }
            SourceReference::Attachment { attachment_id } => {
                if !selected.contains(&attachment_id) {
                    return Err("Select this uploaded document in the current owner message".into());
                }
                let row = history
                    .get_attachment(&run.agent_id, &attachment_id)
                    .await
                    .map_err(|_| "Document upload history is unavailable")?
                    .filter(|row| row.agent_id == run.agent_id && row.session_id == run.session_id)
                    .ok_or("Document upload is unavailable")?;
                if row.kind != crate::attachments::AttachmentKind::Document
                    || row.size > MAX_BYTES as u64
                {
                    return Err("Document upload is unsupported or exceeds 4 MiB".into());
                }
                let format = format(&row.name)?;
                (
                    DocumentEditSource::Attachment {
                        attachment_id,
                        sha256: row.sha256.clone(),
                    },
                    Some(row),
                    format,
                )
            }
        };
        let root_copy = root.clone();
        let source_copy = source.clone();
        let row = attachment.clone();
        let bytes = crate::attachments::documents::source_read::read_retained(
            move || source_now(&root_copy, &source_copy, row.as_ref()),
            cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document read is unavailable")??;
        if self.checked_scope(run, tool, cancel).await? != scope {
            return Err("Document scope changed".into());
        }
        Ok((scope, root, source, attachment, bytes, format))
    }
    pub(crate) async fn inspect(
        &self,
        run: &RunLink,
        source: SourceReference,
        cursor: Option<&str>,
        cancel: CancelSignal,
    ) -> Result<OfficeInventoryPage, String> {
        let preview_context = self.preview_context(run).await?;
        if let Some(context) = &preview_context {
            context.bind_source(&source, context.sha256())?;
        }
        let (scope, _, source, row, bytes, format) = self
            .resolve(run, "inspect_office_document", source, &cancel)
            .await?;
        if preview_context
            .as_ref()
            .is_some_and(|context| context.sha256() != source.sha256())
        {
            return Err("Selected document source changed; select it again".into());
        }
        let worker = self.state.read().await.document_edit_worker.clone();
        let inventory = worker
            .inspect(
                format,
                bytes,
                cancel.clone(),
                tokio::time::Instant::now() + Duration::from_secs(10),
            )
            .await?;
        screened(&inventory)?;
        self.validate_source(
            run,
            "inspect_office_document",
            &scope,
            &source,
            row.as_ref(),
            &cancel,
        )
        .await?;
        projection::inventory_page(
            &inventory,
            source.sha256(),
            label(&source, row.as_ref()),
            &digest(&serde_json::to_vec(&scope).map_err(|_| "Document scope is invalid")?),
            cursor,
        )
    }
    pub(crate) async fn propose(
        &self,
        run: RunLink,
        input: ProposeRequest,
        cancel: CancelSignal,
    ) -> Result<serde_json::Value, String> {
        if let Some(context) = self.preview_context(&run).await? {
            context.bind_source(&input.source, &input.expected_sha256)?;
            context.bind_operations(&input.operations)?;
        }
        safe_path(&input.file_path)?;
        screened(&input.operations)?;
        if !super::registry::valid_hash(&input.expected_sha256) {
            return Err("Document expected_sha256 is invalid".into());
        }
        let (scope, root, source, row, bytes, source_format) = self
            .resolve(&run, "propose_document_edit", input.source, &cancel)
            .await?;
        if source.sha256() != input.expected_sha256 || format(&input.file_path)? != source_format {
            return Err("Document source hash or destination format changed".into());
        }
        destination_matches(
            workspace_snapshot(&root, &input.file_path)?.as_deref(),
            &input.expected_destination,
        )?;
        let worker = self.state.read().await.document_edit_worker.clone();
        let edits = OfficeEditSpec {
            operations: input.operations,
        };
        let operations_sha256 =
            digest(&serde_json::to_vec(&edits).map_err(|_| "Document operations are invalid")?);
        let prepared = worker
            .prepare(
                source_format,
                bytes,
                edits,
                cancel.clone(),
                tokio::time::Instant::now() + Duration::from_secs(10),
            )
            .await?;
        screened(&prepared.review)?;
        self.validate_source(
            &run,
            "propose_document_edit",
            &scope,
            &source,
            row.as_ref(),
            &cancel,
        )
        .await?;
        let now = anima_core::primitives::now_millis();
        let store = self
            .state
            .read()
            .await
            .document_edit_store
            .clone()
            .ok_or("Document storage is unavailable")?;
        let staged = store.stage(&prepared.bytes)?;
        let draft = DocumentEditDraft {
            id: format!("edit_{}", uuid::Uuid::new_v4()),
            scope,
            source,
            attachment_record: row,
            file_path: input.file_path,
            expected_destination: input.expected_destination,
            candidate_sha256: staged.hash().into(),
            candidate_bytes: staged.size(),
            operations_sha256,
            content_review: prepared.review,
            created_at_ms: now,
            expires_at_ms: now.saturating_add(LIFETIME_MS),
        };
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction=service.transactions.clone().lock_owned().await;
            let outcome=async {
                service.validate_source(&run,"propose_document_edit",&draft.scope,&draft.source,draft.attachment_record.as_ref(),&cancel).await?;
                destination_matches(workspace_snapshot(&root,&draft.file_path)?.as_deref(),&draft.expected_destination)?;
                let old=service.state.read().await.document_edits.clone();
                let mut next=old.clone();
                next.expire_prepared(anima_core::primitives::now_millis());
                let review=next.insert(draft.clone(),anima_core::primitives::now_millis())?;
                let record=next.get(&draft.id).ok_or("Document proposal is unavailable")?;
                if serde_json::to_vec(&DocumentEditReview::from_record(record,label(&draft.source,draft.attachment_record.as_ref()))).map_err(|_|"Document review cannot be encoded")?.len()>64*1024 {
                    return Err("Complete document review exceeds 64 KiB; split the edits".into());
                }
                service.persist(&old,next).await?;
                service.validate_source(&run,"propose_document_edit",&draft.scope,&draft.source,draft.attachment_record.as_ref(),&cancel).await?;
                destination_matches(workspace_snapshot(&root,&draft.file_path)?.as_deref(),&draft.expected_destination)?;
                Ok(serde_json::json!({"version":1,"proposal_id":draft.id,"review_sha256":review,"file_path":draft.file_path,"change_count":draft.content_review.changes.len(),"review_url":format!("/api/document-edits/{}?agent_id={}&session_id={}",draft.id,encode(&run.agent_id),encode(&run.session_id)),"preview_quality":"structural_content"}))
            }.await;
            let hash=staged.hash().to_string();
            drop(staged);
            if outcome.is_err() { service.retire(&hash); }
            outcome
        }).await.map_err(|_|"Document proposal save did not settle")?
    }
    async fn validate_source(
        &self,
        run: &RunLink,
        tool: &str,
        scope: &DocumentEditScope,
        source: &DocumentEditSource,
        row: Option<&AttachmentRecord>,
        cancel: &CancelSignal,
    ) -> Result<(), String> {
        if &self.checked_scope(run, tool, cancel).await? != scope {
            return Err("Document owner scope changed".into());
        }
        let (root, history) = {
            let state = self.state.read().await;
            (
                state
                    .workspace
                    .as_ref()
                    .ok_or("Document workspace is unavailable")?
                    .root_path
                    .clone(),
                state.history.store(),
            )
        };
        if let Some(row) = row {
            if history
                .get_attachment(&run.agent_id, &row.id)
                .await
                .map_err(|_| "Document upload history is unavailable")?
                .as_ref()
                != Some(row)
            {
                return Err("Document upload custody changed".into());
            }
        }
        let source = source.clone();
        let row = row.cloned();
        crate::attachments::documents::source_read::read_retained(
            move || source_now(&root, &source, row.as_ref()).map(drop),
            cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document source read is unavailable")??;
        if &self.checked_scope(run, tool, cancel).await? != scope {
            return Err("Document owner scope changed".into());
        }
        Ok(())
    }
    pub(super) async fn persist(
        &self,
        old: &DocumentEditRegistry,
        next: DocumentEditRegistry,
    ) -> Result<(), String> {
        next.validate()?;
        let retired = old
            .records()
            .filter(|record| !next.references(&record.draft.candidate_sha256))
            .map(|record| record.draft.candidate_sha256.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for _ in 0..3 {
            let request = {
                let mut state = self.state.write().await;
                if state.control_plane_store.is_none() {
                    return Err("Document editing requires durable profile metadata".into());
                }
                if &state.document_edits != old {
                    return Err("Document proposal registry changed".into());
                }
                state
                    .control_plane_persist_request()
                    .with_document_edit_candidate(next.clone())
                    .ok_or("Document proposal persistence is unavailable")?
            };
            match request.save_document_edit_transition().await {
                Ok(crate::state::DocumentEditSaveOutcome::Acknowledged) => {
                    let mut state = self.state.write().await;
                    if &state.document_edits != old {
                        return Err(UNCERTAIN.into());
                    }
                    state.document_edits = next;
                    drop(state);
                    for hash in &retired {
                        self.retire(hash);
                    }
                    let _ = self.cleanup_staging().await;
                    return Ok(());
                }
                Ok(crate::state::DocumentEditSaveOutcome::ObservedUncertain(observed)) => {
                    let mut state = self.state.write().await;
                    if &state.document_edits != old {
                        return Err(UNCERTAIN.into());
                    }
                    // Preserve the exact observed comparison base; no file effect follows.
                    state.document_edits = observed;
                    return Err(UNCERTAIN.into());
                }
                Err(error) if crate::state::is_superseded_control_plane_save(&error) => continue,
                Err(_) => return Err("Document proposal could not be saved".into()),
            }
        }
        Err("Document proposal save was superseded".into())
    }
    fn retire(&self, hash: &str) {
        let Ok(state) = self.state.clone().try_read_owned() else {
            return;
        };
        let Some(store) = state.document_edit_store.clone() else {
            return;
        };
        drop(state);
        let _ = store.retire_if_unreferenced(hash, || {
            let state = self
                .state
                .clone()
                .try_read_owned()
                .map_err(|_| "Document registry is busy")?;
            state
                .ensure_profile_owned()
                .map_err(|_| "Document profile ownership was lost")?;
            if state.document_edits.references(hash) {
                return Err("Document candidate is still referenced".into());
            }
            Ok(state)
        });
    }
    fn retire_proof(&self, id: &str) {
        let Ok(state) = self.state.clone().try_read_owned() else {
            return;
        };
        let Some(store) = state.document_edit_store.clone() else {
            return;
        };
        drop(state);
        let _ = store.retire_readback_proof(id, || {
            let state = self
                .state
                .clone()
                .try_read_owned()
                .map_err(|_| "Document registry is busy")?;
            state
                .ensure_profile_owned()
                .map_err(|_| "Document profile ownership was lost")?;
            if state.document_edits.get(id).is_some() {
                return Err("Document proof is still referenced".into());
            }
            Ok(state)
        });
    }
    pub(crate) async fn capture_apply(
        &self,
        run: &RunLink,
        call: &ToolCall,
    ) -> Result<Option<DocumentEditWitness>, String> {
        if call.name != approval::APPLY_TOOL {
            return Ok(None);
        }
        if self.replay(run, call).await?.is_some() {
            return Ok(None);
        }
        let cancel = CancelSignal::new();
        let scope = self.checked_scope(run, &call.name, &cancel).await?;
        let (witness, draft, store, root) = {
            let state = self.state.read().await;
            let witness = approval::capture(
                &state.document_edits,
                &scope,
                call,
                anima_core::primitives::now_millis(),
            )?
            .ok_or("Document proposal is unavailable")?;
            let id = call
                .args
                .get("proposal_id")
                .and_then(|value| {
                    if let anima_core::DataValue::String(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                })
                .ok_or("Document proposal is invalid")?;
            (
                witness,
                state
                    .document_edits
                    .get(id)
                    .ok_or("Document proposal is unavailable")?
                    .draft
                    .clone(),
                state
                    .document_edit_store
                    .clone()
                    .ok_or("Document storage is unavailable")?,
                state
                    .workspace
                    .as_ref()
                    .ok_or("Document workspace is unavailable")?
                    .root_path
                    .clone(),
            )
        };
        store.load(&draft.candidate_sha256, draft.candidate_bytes)?;
        self.validate_source(
            run,
            &call.name,
            &scope,
            &draft.source,
            draft.attachment_record.as_ref(),
            &cancel,
        )
        .await?;
        let state = self.state.read().await;
        if scope_in(&state, run, &call.name)? != scope
            || !witness.revalidate(
                &state.document_edits,
                &scope,
                call,
                anima_core::primitives::now_millis(),
            )
        {
            return Err("Document review changed during source verification".into());
        }
        store.load(&draft.candidate_sha256, draft.candidate_bytes)?;
        source_now(&root, &draft.source, draft.attachment_record.as_ref())?;
        destination_matches(
            workspace_snapshot(&root, &draft.file_path)?.as_deref(),
            &draft.expected_destination,
        )?;
        Ok(Some(witness))
    }
    pub(crate) async fn revalidate_apply(
        &self,
        run: &RunLink,
        call: &ToolCall,
        witness: &DocumentEditWitness,
    ) -> Result<(), String> {
        if self.capture_apply(run, call).await?.as_ref() != Some(witness) {
            return Err("Document review changed during approval".into());
        }
        Ok(())
    }
    /// A completed proposal is read back from its actual destination; it never repeats a write.
    pub(crate) async fn replay(
        &self,
        run: &RunLink,
        call: &ToolCall,
    ) -> Result<Option<DocumentPublicationReceipt>, String> {
        let input: ReplayArguments = serde_json::from_value(crate::routes::data_value_to_json(
            &anima_core::DataValue::Object(call.args.clone()),
        ))
        .map_err(|_| "Document apply arguments are invalid")?;
        let scope = self
            .checked_scope(run, approval::APPLY_TOOL, &CancelSignal::new())
            .await?;
        let state = self.state.read().await;
        if scope_in(&state, run, approval::APPLY_TOOL)? != scope {
            return Err("Document completed review authority changed".into());
        }
        let Some(receipt) = state
            .document_edits
            .receipt_for_proposal(&input.proposal_id)
        else {
            return Ok(None);
        };
        if receipt.profile_key != scope.profile_key
            || receipt.workspace_key != scope.workspace_key
            || receipt.agent_id != scope.agent_id
            || receipt.session_id != scope.session_id
            || receipt.clear_generation != scope.clear_generation
            || receipt.review_sha256 != input.review_sha256
            || receipt.file_path != input.file_path
            || receipt.expected_destination != input.expected_destination
        {
            return Err("Document completed review scope changed".into());
        }
        let root = &state
            .workspace
            .as_ref()
            .ok_or("Document workspace is unavailable")?
            .root_path;
        let bytes = workspace_snapshot(root, &receipt.file_path)?
            .ok_or("Published document is unavailable")?;
        if bytes.len() as u64 != receipt.size_bytes || digest(&bytes) != receipt.sha256 {
            return Err("Published document changed; propose and review again".into());
        }
        Ok(Some(receipt.clone()))
    }
    pub(crate) async fn reserve(
        &self,
        run: RunLink,
        call: ToolCall,
        witness: DocumentEditWitness,
        approval: DocumentEditOwnerApproval,
        cancel: CancelSignal,
    ) -> Result<ApplyingDocument, String> {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction = service.transactions.clone().lock_owned().await;
            service.revalidate_apply(&run, &call, &witness).await?;
            let scope = service.checked_scope(&run, &call.name, &cancel).await?;
            let id = call
                .args
                .get("proposal_id")
                .and_then(|value| {
                    if let anima_core::DataValue::String(value) = value {
                        Some(value.clone())
                    } else {
                        None
                    }
                })
                .ok_or("Document proposal is invalid")?;
            let (old, store) = {
                let state = service.state.read().await;
                (
                    state.document_edits.clone(),
                    state
                        .document_edit_store
                        .clone()
                        .ok_or("Document storage is unavailable")?,
                )
            };
            let draft = old
                .get(&id)
                .ok_or("Document proposal is unavailable")?
                .draft
                .clone();
            let review = old
                .get(&id)
                .ok_or("Document proposal is unavailable")?
                .review_sha256
                .clone();
            let bytes = store.load(&draft.candidate_sha256, draft.candidate_bytes)?;
            let mut next = old.clone();
            next.begin_apply(
                &id,
                &review,
                &scope,
                &approval,
                &call.id,
                anima_core::primitives::now_millis(),
            )?;
            service.persist(&old, next).await?;
            let validation = service
                .validate_source(
                    &run,
                    &call.name,
                    &scope,
                    &draft.source,
                    draft.attachment_record.as_ref(),
                    &cancel,
                )
                .await;
            if let Err(error) = validation {
                let current = service.state.read().await.document_edits.clone();
                let mut cancelled = current.clone();
                cancelled.cancel(&id, true)?;
                service.persist(&current, cancelled).await?;
                return Err(error);
            }
            Ok(ApplyingDocument { draft, bytes })
        })
        .await
        .map_err(|_| "Document applying intent did not settle")?
    }
    /// Used only by the native handler before it enters the publication closure.
    pub(crate) async fn begin_publication(
        &self,
        run: RunLink,
        call: ToolCall,
        cancel: CancelSignal,
    ) -> Result<(), String> {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction = service.transactions.clone().lock_owned().await;
            let scope = service.checked_scope(&run, &call.name, &cancel).await?;
            let old = service.state.read().await.document_edits.clone();
            let id = call
                .args
                .get("proposal_id")
                .and_then(|value| {
                    if let anima_core::DataValue::String(value) = value {
                        Some(value.clone())
                    } else {
                        None
                    }
                })
                .ok_or("Document proposal is invalid")?;
            let record = old
                .get(&id)
                .ok_or("Document applying intent is unavailable")?;
            if !approval::matches_record(&call, record)
                || !old.reserved_apply(
                    &id,
                    &record.review_sha256,
                    &scope,
                    &call.id,
                    anima_core::primitives::now_millis(),
                )
            {
                return Err("Document publication authority changed".into());
            }
            service
                .validate_source(
                    &run,
                    &call.name,
                    &scope,
                    &record.draft.source,
                    record.draft.attachment_record.as_ref(),
                    &cancel,
                )
                .await?;
            let store = service
                .state
                .read()
                .await
                .document_edit_store
                .clone()
                .ok_or("Document storage is unavailable")?;
            store.ensure_readback_capacity()?;
            let mut next = old.clone();
            next.begin_publication(&id, &call.id)?;
            service.persist(&old, next).await?;
            if service.checked_scope(&run, &call.name, &cancel).await? != scope {
                return Err("Document publication authority changed".into());
            }
            Ok(())
        })
        .await
        .map_err(|_| "Document publication admission did not settle")?
    }
    /// Used only by the native handler before it enters the publication closure.
    pub(crate) async fn abandon_before_publication(
        &self,
        id: String,
        call_id: String,
    ) -> Result<(), String> {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction=service.transactions.clone().lock_owned().await;
            let old=service.state.read().await.document_edits.clone();
            let record=old.get(&id).ok_or("Document applying intent is unavailable")?;
            if !matches!(&record.status,DocumentEditStatus::Applying{tool_call_id,..} if tool_call_id==&call_id) {return Err("Document applying intent changed".into())}
            let mut next=old.clone();next.cancel(&id,true)?;service.persist(&old,next).await
        }).await.map_err(|_|"Document cancellation did not settle")?
    }
    pub(crate) async fn confirm(
        &self,
        id: String,
        call_id: String,
        observed: String,
        checkpoint: Option<String>,
    ) -> Result<DocumentPublicationReceipt, String> {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction = service.transactions.clone().lock_owned().await;
            let old = service.state.read().await.document_edits.clone();
            let record = old
                .get(&id)
                .ok_or("Document applying intent is unavailable")?;
            let (store, run) = {
                let state = service.state.read().await;
                (
                    state
                        .document_edit_store
                        .clone()
                        .ok_or("Document storage is unavailable")?,
                    state
                        .runs
                        .get(&record.draft.scope.run_id)
                        .cloned()
                        .ok_or("Document native intent is unavailable")?,
                )
            };
            if !store.readback_proof(&id)?.is_some_and(|proof| {
                proof.matches(record, &run, &call_id) && proof.checkpoint_id == checkpoint
            }) {
                return Err(UNCERTAIN.into());
            }
            let mut next = old.clone();
            let receipt = next.confirm_publication(
                &id,
                &call_id,
                &observed,
                checkpoint,
                anima_core::primitives::now_millis(),
            )?;
            service
                .persist(&old, next)
                .await
                .map_err(|_| UNCERTAIN.to_string())?;
            Ok(receipt)
        })
        .await
        .map_err(|_| UNCERTAIN.to_string())?
    }
    pub(crate) async fn record_readback(
        &self,
        id: String,
        call_id: String,
        observed: String,
        bytes: u64,
        checkpoint_id: Option<String>,
    ) -> Result<(), String> {
        let service = self.clone();
        tokio::spawn(async move {
            let state = service.state.read().await;
            state
                .ensure_profile_owned()
                .map_err(|_| "Document profile ownership was lost")?;
            let record = state
                .document_edits
                .get(&id)
                .ok_or("Document applying intent is unavailable")?;
            let run = state
                .runs
                .get(&record.draft.scope.run_id)
                .ok_or("Document native intent is unavailable")?;
            let proof = super::recovery::PublicationReadbackProof::from_native_readback(
                record,
                run,
                &call_id,
                &observed,
                bytes,
                checkpoint_id,
                anima_core::primitives::now_millis(),
            )?;
            let store = state
                .document_edit_store
                .clone()
                .ok_or("Document storage is unavailable")?;
            drop(state);
            store.write_readback_proof(&proof)
        })
        .await
        .map_err(|_| UNCERTAIN.to_string())?
    }
    pub(crate) async fn review(
        &self,
        id: &str,
        agent: &str,
        session: &str,
    ) -> Result<DocumentEditReview, String> {
        let state = self.state.read().await;
        let record = state
            .document_edits
            .get(id)
            .ok_or("Document review is unavailable")?;
        if record.draft.scope.agent_id == agent
            && record.draft.scope.session_id == session
            && matches!(record.status, DocumentEditStatus::RecoveryRequired { .. })
        {
            return Err(UNCERTAIN.into());
        }
        if record.draft.scope.agent_id != agent
            || record.draft.scope.session_id != session
            || record.status != DocumentEditStatus::Prepared
            || record.draft.scope
                != scope_in(
                    &state,
                    &RunLink {
                        agent_id: agent.into(),
                        session_id: session.into(),
                        run_id: record.draft.scope.run_id.clone(),
                    },
                    approval::APPLY_TOOL,
                )?
            || anima_core::primitives::now_millis() >= record.draft.expires_at_ms
        {
            return Err("Document review changed or expired".into());
        }
        state
            .document_edit_store
            .as_ref()
            .ok_or("Document storage is unavailable")?
            .load(&record.draft.candidate_sha256, record.draft.candidate_bytes)?;
        Ok(DocumentEditReview::from_record(
            record,
            label(
                &record.draft.source,
                record.draft.attachment_record.as_ref(),
            ),
        ))
    }
    pub(crate) async fn cancel(
        &self,
        id: String,
        agent: String,
        session: String,
    ) -> Result<serde_json::Value, String> {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction = service.transactions.clone().lock_owned().await;
            let old = service.state.read().await.document_edits.clone();
            let record = old.get(&id).ok_or("Document proposal is unavailable")?;
            if record.draft.scope.agent_id != agent || record.draft.scope.session_id != session {
                return Err("Document proposal is unavailable".into());
            }
            {
                let state = service.state.read().await;
                let store = state
                    .document_edit_store
                    .as_ref()
                    .ok_or("Document storage is unavailable")?;
                let root = &state
                    .workspace
                    .as_ref()
                    .ok_or("Document workspace is unavailable")?
                    .root_path;
                let workspace =
                    WorkspaceBytes::open(root).map_err(|_| "Document workspace is unavailable")?;
                if store.profile_key() != record.draft.scope.profile_key
                    || crate::workspace_file_preview::workspace_key(&workspace.canonical_root)
                        != record.draft.scope.workspace_key
                {
                    return Err("Document proposal scope changed".into());
                }
            }
            let mut next = old.clone();
            next.cancel(&id, false)?;
            service.persist(&old, next).await?;
            Ok(serde_json::json!({"version":1,"proposal_id":id,"cancelled":true}))
        })
        .await
        .map_err(|_| "Document cancellation did not settle")?
    }
}

pub(crate) fn validate_final(
    state: &DaemonState,
    run: &RunLink,
    call: &ToolCall,
    now: u64,
) -> Result<(), String> {
    if call.name != approval::APPLY_TOOL {
        return Ok(());
    }
    let scope = scope_in(state, run, &call.name)?;
    let id = call
        .args
        .get("proposal_id")
        .and_then(|value| {
            if let anima_core::DataValue::String(value) = value {
                Some(value)
            } else {
                None
            }
        })
        .ok_or("Document proposal is invalid")?;
    let record = state
        .document_edits
        .get(id)
        .ok_or("Document applying intent is unavailable")?;
    if record.publication_phase != DocumentPublicationPhase::Publishing
        || !approval::matches_record(call, record)
        || !state
            .document_edits
            .reserved_apply(id, &record.review_sha256, &scope, &call.id, now)
        || !record
            .owner_approval
            .as_ref()
            .is_some_and(|proof| proof.matches(&record.draft, &record.review_sha256, &call.id))
    {
        return Err("Document exact owner authority is unavailable".into());
    }
    // This is synchronous under the native authority guard, directly before publication.
    let root = &state
        .workspace
        .as_ref()
        .ok_or("Document workspace is unavailable")?
        .root_path;
    source_now(
        root,
        &record.draft.source,
        record.draft.attachment_record.as_ref(),
    )?;
    Ok(())
}
pub(crate) fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
