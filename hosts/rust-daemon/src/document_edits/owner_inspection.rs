//! Read-only owner inspection binds source bytes to the current chat and workspace.
use super::{
    projection::{inventory_page, OfficeInventoryPage, SourceLabel},
    service::{digest, format, safe_path, screened, workspace_snapshot},
    DocumentEditService,
};
use crate::{state::DaemonState, tools::workspace_bytes::WorkspaceBytes};
use anima_core::CancelSignal;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerInspectionScope {
    root: PathBuf,
    profile_key: String,
    workspace_key: String,
    agent_id: String,
    session_id: String,
    clear_generation: u64,
    project: crate::projects::QueuedProjectWitness,
}
impl OwnerInspectionScope {
    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }
    pub(crate) fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }
    pub(crate) fn matches_draft(&self, scope: &super::DocumentEditScope) -> bool {
        self.profile_key == scope.profile_key
            && self.workspace_key == scope.workspace_key
            && self.agent_id == scope.agent_id
            && self.session_id == scope.session_id
            && self.clear_generation == scope.clear_generation
    }
    pub(crate) fn source_scope(&self, path: &str, hash: &str) -> Result<String, String> {
        Ok(digest(
            &serde_json::to_vec(&(1_u8, &self.profile_key, &self.workspace_key, path, hash))
                .map_err(|_| "Document source scope is invalid")?,
        ))
    }
}

pub(super) fn validate_inspected_preview_context(
    context: &super::preview_context::PreviewEditContext,
    inventory: &anima_document_content::OfficeInventory,
    hash: &str,
    scope: &OwnerInspectionScope,
) -> Result<(), String> {
    if hash != context.sha256()
        || scope.source_scope(context.path(), hash)? != context.source_scope()
        || !context.matches_inventory(inventory)
    {
        return Err(
            "Selected document source, workspace or content changed; select it again".into(),
        );
    }
    Ok(())
}

pub(crate) fn readonly_scope_in(
    state: &DaemonState,
    agent_id: &str,
    session_id: &str,
) -> Result<OwnerInspectionScope, String> {
    state
        .ensure_profile_owned()
        .map_err(|_| "Document profile ownership was lost")?;
    let store = state
        .document_edit_store
        .as_ref()
        .ok_or("Private document storage is unavailable")?;
    let root = &state
        .workspace
        .as_ref()
        .ok_or("Document workspace is unavailable")?
        .root_path;
    super::workspace_boundary::ensure_state_separate(state, Some(root))?;
    let session = state
        .sessions
        .get(agent_id, session_id)
        .ok_or("Document owner session is unavailable")?;
    let agent = state
        .agents
        .get(agent_id)
        .ok_or("Document owner is unavailable")?;
    if session.kind != crate::sessions::SessionKind::Chat
        || session.origin != crate::sessions::SessionOrigin::Web
        || session.parent_agent_id.is_some()
        || session.parent_run_id.is_some()
        || session.pending_clear_run_id.is_some()
        || crate::agent_runs::is_helper_config(agent.config())
        || state.pending_history_deletions.iter().any(|row| {
            row.agent_id == agent_id && row.session_id.as_deref().is_none_or(|id| id == session_id)
        })
    {
        return Err("Document inspection requires an ordinary owner web chat".into());
    }
    let workspace = WorkspaceBytes::open(root).map_err(|_| "Document workspace is unavailable")?;
    let project = crate::projects::capture_run_witness(state, agent_id, session_id)
        .map_err(|_| "Document project authority changed")?;
    Ok(OwnerInspectionScope {
        root: root.clone(),
        profile_key: store.profile_key(),
        workspace_key: crate::workspace_file_preview::workspace_key(&workspace.canonical_root),
        agent_id: agent_id.into(),
        session_id: session_id.into(),
        clear_generation: session.clear_generation,
        project,
    })
}

impl DocumentEditService {
    async fn readonly_scope(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<OwnerInspectionScope, String> {
        let owner = self.state.read().await.profile_owner();
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| "Document profile ownership was lost")?;
        }
        readonly_scope_in(&*self.state.read().await, agent_id, session_id)
    }

    async fn inspect_owner_inventory(
        &self,
        agent_id: &str,
        session_id: &str,
        file_path: &str,
    ) -> Result<
        (
            anima_document_content::OfficeInventory,
            String,
            OwnerInspectionScope,
        ),
        String,
    > {
        safe_path(file_path)?;
        let format = format(file_path)?;
        let scope = self.readonly_scope(agent_id, session_id).await?;
        let cancel = CancelSignal::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let source_root = scope.root.clone();
        let source_path = file_path.to_owned();
        let bytes = crate::attachments::documents::source_read::read_retained(
            move || workspace_snapshot(&source_root, &source_path),
            &cancel,
            deadline,
        )
        .await
        .map_err(|_| "Document source read is unavailable")??
        .ok_or("Document source is unavailable")?;
        let hash = digest(&bytes);
        let worker = self.state.read().await.document_edit_worker.clone();
        let inventory = worker
            .inspect(format, bytes.clone(), cancel.clone(), deadline)
            .await?;
        screened(&inventory)?;
        let source_root = scope.root.clone();
        let source_path = file_path.to_owned();
        let observed = crate::attachments::documents::source_read::read_retained(
            move || workspace_snapshot(&source_root, &source_path),
            &cancel,
            deadline,
        )
        .await
        .map_err(|_| "Document source read is unavailable")??
        .ok_or("Document source disappeared")?;
        if observed != bytes || self.readonly_scope(agent_id, session_id).await? != scope {
            return Err("Document source or owner scope changed during inspection".into());
        }
        Ok((inventory, hash, scope))
    }

    pub(crate) async fn validate_preview_context(
        &self,
        agent_id: &str,
        session_id: &str,
        context: &super::preview_context::PreviewEditContext,
    ) -> Result<OwnerInspectionScope, String> {
        context.validate()?;
        let (inventory, hash, scope) = self
            .inspect_owner_inventory(agent_id, session_id, context.path())
            .await?;
        validate_inspected_preview_context(context, &inventory, &hash, &scope)?;
        Ok(scope)
    }

    pub(crate) async fn inspect_owner(
        &self,
        agent_id: &str,
        session_id: &str,
        file_path: &str,
        cursor: Option<&str>,
    ) -> Result<OfficeInventoryPage, String> {
        let (inventory, hash, scope) = self
            .inspect_owner_inventory(agent_id, session_id, file_path)
            .await?;
        let cursor_scope = digest(
            &serde_json::to_vec(&(&scope, file_path)).map_err(|_| "Document scope is invalid")?,
        );
        let mut page = inventory_page(
            &inventory,
            &hash,
            SourceLabel {
                kind: "workspace".into(),
                display_name: file_path.into(),
            },
            &cursor_scope,
            cursor,
        )?;
        page.source_scope = Some(scope.source_scope(file_path, &hash)?);
        Ok(page)
    }
}
