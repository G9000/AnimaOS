//! Host custody, current owner authority, and delivery checks around isolated parser work.
use super::{store::DocumentInspectionStore, worker::DocumentInspectionWorker};
use crate::{
    app::SharedDaemonState,
    attachments::{AttachmentKind, AttachmentRecord},
    document_edits::{
        readonly_scope_in,
        service::{digest, format, safe_path, scope_in, workspace_snapshot},
        DocumentEditRecord, DocumentEditSource, DocumentEditStatus, OwnerInspectionScope,
    },
    runs::{RunLink, RunSource, RunStatus},
};
use anima_core::{execution_limits::ExecutionContext, CancelSignal};
use anima_document_content::OfficeFormat;
use anima_document_visual::inspection::ArtifactInspectionReport;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Mutex, time::Duration};

const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const LIFETIME_MS: u64 = 30 * 60 * 1000;
const MAX_LIVE_WITNESSES: usize = 8;

/// An exact current source identity, never an expected visual result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InspectionSubject {
    Workspace {
        file_path: String,
        expected_sha256: String,
    },
    Attachment {
        attachment_id: String,
        expected_sha256: String,
    },
    Candidate {
        proposal_id: String,
        expected_sha256: String,
    },
}
impl InspectionSubject {
    pub(super) fn expected_sha256(&self) -> &str {
        match self {
            Self::Workspace {
                expected_sha256, ..
            }
            | Self::Attachment {
                expected_sha256, ..
            }
            | Self::Candidate {
                expected_sha256, ..
            } => expected_sha256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionRequest {
    pub(crate) subject: InspectionSubject,
}

/// A digest-bound private page locator, reauthorized for every delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionPageRef {
    pub(crate) page: u32,
    pub(crate) png_sha256: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionEnvelope {
    pub(crate) version: u32,
    pub(crate) report_sha256: String,
    pub(crate) report: ArtifactInspectionReport,
    pub(crate) pages: Vec<InspectionPageRef>,
}
impl InspectionEnvelope {
    pub(super) fn validate(&self) -> Result<(), String> {
        self.report
            .validate()
            .map_err(|_| "Document inspection report is invalid")?;
        let rendered = &self.report.rendering().pages;
        if self.version != 1
            || self.report_sha256 != self.report.sha256()
            || self.pages.len() != rendered.len()
            || self.pages.iter().zip(rendered).any(|(page, rendered)| {
                page.page != rendered.ordinal
                    || page.png_sha256 != rendered.png_sha256
                    || page.width != rendered.width
                    || page.height != rendered.height
            })
        {
            return Err("Document inspection report or pages changed".into());
        }
        Ok(())
    }
}

/// Only memory holds live grants. Persisted reports cannot resurrect access after restart.
pub(crate) struct InspectionRuntime {
    pub(crate) worker: DocumentInspectionWorker,
    pub(crate) authority: crate::attachments::documents::authority::WorkspaceDocumentAuthority,
    live: Mutex<BTreeMap<String, LiveWitness>>,
}
impl Default for InspectionRuntime {
    fn default() -> Self {
        Self {
            worker: DocumentInspectionWorker::new(),
            authority: Default::default(),
            live: Mutex::new(BTreeMap::new()),
        }
    }
}
impl InspectionRuntime {
    pub(crate) fn revoke_all(&self) {
        self.authority.revoke_all();
        self.live
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
    }
    pub(crate) fn revoke(&self, agent: &str) {
        self.authority.revoke(agent);
        self.live
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .retain(|_, witness| witness.scope.agent_id() != agent);
    }
    fn remember(&self, key: String, witness: LiveWitness) -> Result<(), String> {
        let mut live = self
            .live
            .lock()
            .map_err(|_| "Document inspection authority is unavailable")?;
        witness.check()?;
        let now = anima_core::primitives::now_millis();
        live.retain(|_, value| value.expires_at_ms > now && value.check().is_ok());
        if !live.contains_key(&key) && live.len() >= MAX_LIVE_WITNESSES {
            return Err("Document inspection capacity is full".into());
        }
        live.insert(key, witness);
        Ok(())
    }
    fn witness(&self, key: &str) -> Result<LiveWitness, String> {
        let witness = self
            .live
            .lock()
            .map_err(|_| "Document inspection authority is unavailable")?
            .get(key)
            .cloned()
            .ok_or("Document inspection is unavailable; inspect the source again")?;
        witness.check()?;
        Ok(witness)
    }
}

#[derive(Clone)]
enum SourceCustody {
    Workspace { file_path: String, sha256: String },
    Attachment { record: AttachmentRecord },
    Candidate { record: Box<DocumentEditRecord> },
}
#[derive(Clone)]
struct LiveWitness {
    scope: OwnerInspectionScope,
    run: Option<RunLink>,
    request: InspectionRequest,
    custody: SourceCustody,
    grant: CancelSignal,
    workspace: CancelSignal,
    source_version: String,
    expires_at_ms: u64,
}
impl LiveWitness {
    fn check(&self) -> Result<(), String> {
        if self.grant.is_cancelled()
            || self.workspace.is_cancelled()
            || self.expires_at_ms <= anima_core::primitives::now_millis()
        {
            Err("Document inspection authority changed; inspect the source again".into())
        } else {
            Ok(())
        }
    }
}
struct Capture {
    witness: LiveWitness,
    bytes: Vec<u8>,
    format: OfficeFormat,
}

struct CancelWorkerOnDrop(CancelSignal);
impl Drop for CancelWorkerOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(Clone, Copy)]
enum AuthorityPhase {
    ActiveRun,
    OwnerDelivery,
}

#[derive(Clone)]
pub(crate) struct DocumentInspectionService {
    state: SharedDaemonState,
}
impl DocumentInspectionService {
    pub(crate) fn new(state: SharedDaemonState) -> Self {
        Self { state }
    }

    async fn owner_scope(
        &self,
        agent: &str,
        session: &str,
        run: Option<&RunLink>,
    ) -> Result<OwnerInspectionScope, String> {
        let owner = self.state.read().await.profile_owner();
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| "Document profile ownership was lost")?;
        }
        let state = self.state.read().await;
        let scope = readonly_scope_in(&state, agent, session)?;
        let allowed = state
            .agents
            .get(agent)
            .is_some_and(|agent| agent.config().allows_tool("inspect_office_document"));
        state.document_inspections.authority.update(agent, allowed);
        if !allowed {
            return Err("Document inspection requires its current owner grant".into());
        }
        if let Some(run) = run {
            if run.agent_id != agent || run.session_id != session {
                return Err("Document inspection owner run changed".into());
            }
            scope_in(&state, run, "inspect_office_document")?;
        }
        Ok(scope)
    }

    async fn capture(
        &self,
        agent: &str,
        session: &str,
        run: Option<&RunLink>,
        request: InspectionRequest,
        cancel: &CancelSignal,
    ) -> Result<Capture, String> {
        check_cancel(cancel)?;
        if !valid_hash(request.subject.expected_sha256()) {
            return Err("Document inspection requires an exact source SHA-256".into());
        }
        let scope = self.owner_scope(agent, session, run).await?;
        let (grant, workspace) = {
            let state = self.state.read().await;
            (
                state.document_inspections.authority.lease(agent, true),
                state.document_inspections.authority.scope_lease(),
            )
        };
        let (custody, format) = match &request.subject {
            InspectionSubject::Workspace {
                file_path,
                expected_sha256,
            } => {
                safe_path(file_path)?;
                (
                    SourceCustody::Workspace {
                        file_path: file_path.clone(),
                        sha256: expected_sha256.clone(),
                    },
                    office_format(file_path)?,
                )
            }
            InspectionSubject::Attachment {
                attachment_id,
                expected_sha256,
            } => {
                let run = run.ok_or("Uploaded document inspection requires a current owner run")?;
                let record = self
                    .selected_attachment(run, attachment_id, AuthorityPhase::ActiveRun)
                    .await?;
                if &record.sha256 != expected_sha256 {
                    return Err("Document source changed; inspect it again".into());
                }
                let format = office_format(&record.name)?;
                (SourceCustody::Attachment { record }, format)
            }
            InspectionSubject::Candidate {
                proposal_id,
                expected_sha256,
            } => {
                let record = {
                    let state = self.state.read().await;
                    let record = state
                        .document_edits
                        .get(proposal_id)
                        .cloned()
                        .ok_or("Document inspection candidate is unavailable")?;
                    validate_candidate(&state, &record, &scope, expected_sha256)?;
                    record
                };
                if let DocumentEditSource::Attachment { attachment_id, .. } = &record.draft.source {
                    let run =
                        run.ok_or("Uploaded candidate inspection requires a current owner run")?;
                    if Some(
                        self.selected_attachment(run, attachment_id, AuthorityPhase::ActiveRun)
                            .await?,
                    ) != record.draft.attachment_record
                    {
                        return Err("Document candidate source authority changed".into());
                    }
                }
                let format = office_format(&record.draft.file_path)?;
                (
                    SourceCustody::Candidate {
                        record: Box::new(record),
                    },
                    format,
                )
            }
        };
        let source_version = digest(
            &serde_json::to_vec(&(1_u8, &scope, &request))
                .map_err(|_| "Document inspection scope is invalid")?,
        );
        let witness = LiveWitness {
            scope,
            run: run.cloned(),
            request,
            custody,
            grant,
            workspace,
            source_version,
            expires_at_ms: anima_core::primitives::now_millis().saturating_add(LIFETIME_MS),
        };
        let bytes = self
            .read_source(&witness, cancel, AuthorityPhase::ActiveRun)
            .await?;
        let capture = Capture {
            witness,
            bytes,
            format,
        };
        self.validate_capture(&capture, cancel).await?;
        Ok(capture)
    }

    async fn selected_attachment(
        &self,
        run: &RunLink,
        id: &str,
        phase: AuthorityPhase,
    ) -> Result<AttachmentRecord, String> {
        let history = {
            let state = self.state.read().await;
            validate_producer(&state, run, phase)?;
            if !state.runs.get(&run.run_id).is_some_and(|row| {
                row.effective_input()
                    .attachment_ids
                    .iter()
                    .any(|selected| selected == id)
            }) {
                return Err("Select this uploaded document in the current owner message".into());
            }
            state.history.store()
        };
        let row = history
            .get_attachment(&run.agent_id, id)
            .await
            .map_err(|_| "Document upload history is unavailable")?
            .filter(|row| row.agent_id == run.agent_id && row.session_id == run.session_id)
            .ok_or("Document upload is unavailable")?;
        if row.kind != AttachmentKind::Document
            || row.size == 0
            || row.size > MAX_SOURCE_BYTES as u64
        {
            return Err("Document upload is unsupported or exceeds 4 MiB".into());
        }
        Ok(row)
    }

    async fn validate_authority(
        &self,
        witness: &LiveWitness,
        cancel: &CancelSignal,
        phase: AuthorityPhase,
    ) -> Result<(), String> {
        check_cancel(cancel)?;
        witness.check()?;
        let active_run = match phase {
            AuthorityPhase::ActiveRun => witness.run.as_ref(),
            AuthorityPhase::OwnerDelivery => None,
        };
        if self
            .owner_scope(
                witness.scope.agent_id(),
                witness.scope.session_id(),
                active_run,
            )
            .await?
            != witness.scope
        {
            return Err("Document inspection owner scope changed".into());
        }
        if let Some(run) = &witness.run {
            validate_producer(&*self.state.read().await, run, phase)?;
        }
        match &witness.custody {
            SourceCustody::Attachment { record } => {
                let run = witness
                    .run
                    .as_ref()
                    .ok_or("Document owner run is unavailable")?;
                if self.selected_attachment(run, &record.id, phase).await? != *record {
                    return Err("Document upload authority changed".into());
                }
            }
            SourceCustody::Candidate { record } => {
                let state = self.state.read().await;
                let current = state
                    .document_edits
                    .get(&record.draft.id)
                    .ok_or("Document inspection candidate is unavailable")?;
                if current != record.as_ref() {
                    return Err("Document inspection candidate changed".into());
                }
                validate_candidate(
                    &state,
                    current,
                    &witness.scope,
                    witness.request.subject.expected_sha256(),
                )?;
                drop(state);
                if let DocumentEditSource::Attachment { attachment_id, .. } = &record.draft.source {
                    let run = witness
                        .run
                        .as_ref()
                        .ok_or("Document owner run is unavailable")?;
                    if Some(self.selected_attachment(run, attachment_id, phase).await?)
                        != record.draft.attachment_record
                    {
                        return Err("Document candidate upload authority changed".into());
                    }
                }
            }
            SourceCustody::Workspace { .. } => {}
        }
        witness.check()?;
        check_cancel(cancel)
    }

    async fn read_source(
        &self,
        witness: &LiveWitness,
        cancel: &CancelSignal,
        phase: AuthorityPhase,
    ) -> Result<Vec<u8>, String> {
        self.validate_authority(witness, cancel, phase).await?;
        let root = witness.scope.root().to_path_buf();
        let custody = witness.custody.clone();
        let store = self
            .state
            .read()
            .await
            .document_edit_store
            .clone()
            .ok_or("Private document storage is unavailable")?;
        let bytes = crate::attachments::documents::source_read::read_retained(
            move || read_custody(&store, &root, &custody),
            cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document inspection source read is unavailable")??;
        let bytes = checked_bytes(bytes, witness.request.subject.expected_sha256())?;
        self.validate_authority(witness, cancel, phase).await?;
        Ok(bytes)
    }

    async fn validate_capture(
        &self,
        capture: &Capture,
        cancel: &CancelSignal,
    ) -> Result<(), String> {
        if self
            .read_source(&capture.witness, cancel, AuthorityPhase::ActiveRun)
            .await?
            != capture.bytes
        {
            return Err("Document inspection source changed".into());
        }
        Ok(())
    }

    pub(crate) async fn inspect_owner(
        &self,
        agent: &str,
        session: &str,
        request: InspectionRequest,
        cancel: CancelSignal,
    ) -> Result<InspectionEnvelope, String> {
        self.inspect(agent, session, None, None, request, cancel)
            .await
    }

    pub(crate) async fn inspect_for_run(
        &self,
        run: &RunLink,
        execution: &ExecutionContext,
        request: InspectionRequest,
        cancel: CancelSignal,
    ) -> Result<InspectionEnvelope, String> {
        self.inspect(
            &run.agent_id,
            &run.session_id,
            Some(run),
            Some(execution),
            request,
            cancel,
        )
        .await
    }

    async fn inspect(
        &self,
        agent: &str,
        session: &str,
        run: Option<&RunLink>,
        execution: Option<&ExecutionContext>,
        request: InspectionRequest,
        cancel: CancelSignal,
    ) -> Result<InspectionEnvelope, String> {
        if let Some(execution) = execution {
            let run = run.ok_or("Document inspection execution binding is unavailable")?;
            validate_execution_binding(&*self.state.read().await, run, execution)?;
            execution.check_authority().await?;
            validate_execution_binding(&*self.state.read().await, run, execution)?;
        } else if run.is_some() {
            return Err("Document inspection requires its admitted execution binding".into());
        }
        let capture = match execution {
            Some(execution) => {
                execution
                    .bounded(self.capture(agent, session, run, request, &cancel))
                    .await?
            }
            None => self.capture(agent, session, run, request, &cancel).await?,
        };
        let worker = self.state.read().await.document_inspections.worker.clone();
        let worker_cancel = CancelSignal::new();
        let _worker_lifetime = CancelWorkerOnDrop(worker_cancel.clone());
        let operation = worker.inspect(
            capture.bytes.clone(),
            capture.format,
            capture.witness.source_version.clone(),
            worker_cancel.clone(),
            tokio::time::Instant::now() + Duration::from_secs(60),
            execution.cloned(),
        );
        tokio::pin!(operation);
        let output = tokio::select! { biased;
            () = cancel.cancelled() => { worker_cancel.cancel(); return Err("Document inspection was cancelled".into()); }
            () = capture.witness.grant.cancelled() => { worker_cancel.cancel(); return Err("Document inspection authority changed".into()); }
            () = capture.witness.workspace.cancelled() => { worker_cancel.cancel(); return Err("Document inspection workspace changed".into()); }
            output = &mut operation => output?,
        };
        match execution {
            Some(execution) => {
                execution
                    .bounded(self.validate_capture(&capture, &cancel))
                    .await?
            }
            None => self.validate_capture(&capture, &cancel).await?,
        }
        if output.report.source().sha256 != capture.witness.request.subject.expected_sha256()
            || output.report.source().source_version != capture.witness.source_version
            || output.report.source().format != capture.format
            || output.report.source().byte_length != capture.bytes.len() as u64
        {
            return Err("Document inspection worker source identity changed".into());
        }
        let pages = output
            .pages
            .iter()
            .map(|page| InspectionPageRef {
                page: page.page,
                png_sha256: digest(&page.png),
                width: page.width,
                height: page.height,
            })
            .collect();
        let envelope = InspectionEnvelope {
            version: 1,
            report_sha256: output.report.sha256().to_string(),
            report: output.report,
            pages,
        };
        envelope.validate()?;
        let key = record_key(&capture.witness.scope, &envelope.report_sha256)?;
        let store = self.store().await?;
        let scope = capture.witness.scope.clone();
        let request = capture.witness.request.clone();
        let stored = envelope.clone();
        let expires = capture.witness.expires_at_ms;
        let publication_state = self.state.clone();
        let publication_witness = capture.witness.clone();
        let publication_cancel = cancel.clone();
        let publication_execution = execution.cloned();
        crate::attachments::documents::source_read::read_retained(
            move || {
                store.publish_authorized(&scope, &request, &stored, &output.pages, expires, || {
                    let current = publication_state
                        .clone()
                        .try_read_owned()
                        .map_err(|_| "Document inspection authority is unavailable")?;
                    validate_publication_capture(
                        &current,
                        &publication_witness,
                        &publication_cancel,
                        publication_execution.as_ref(),
                    )?;
                    // The read guard protects state authority through the atomic link publication.
                    Ok(current)
                })
            },
            &cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document inspection storage is unavailable")??;
        match execution {
            Some(execution) => {
                execution
                    .bounded(self.validate_capture(&capture, &cancel))
                    .await?
            }
            None => self.validate_capture(&capture, &cancel).await?,
        }
        let current = self.state.read().await;
        if let Some(execution) = execution {
            validate_execution_binding(
                &current,
                run.ok_or("Document inspection owner run is unavailable")?,
                execution,
            )?;
        }
        remember_current(&current, key, &capture.witness, &cancel)?;
        Ok(envelope)
    }

    async fn store(&self) -> Result<DocumentInspectionStore, String> {
        let store = self
            .state
            .read()
            .await
            .document_edit_store
            .clone()
            .ok_or("Private document storage is unavailable")?;
        DocumentInspectionStore::open(store)
    }

    async fn delivery(
        &self,
        agent: &str,
        session: &str,
        hash: &str,
    ) -> Result<(LiveWitness, InspectionEnvelope), String> {
        if !valid_hash(hash) {
            return Err("Document inspection report identity is invalid".into());
        }
        let scope = self.owner_scope(agent, session, None).await?;
        let key = record_key(&scope, hash)?;
        let witness = self.state.read().await.document_inspections.witness(&key)?;
        if witness.scope != scope {
            return Err("Document inspection owner scope changed".into());
        }
        let cancel = CancelSignal::new();
        self.read_source(&witness, &cancel, AuthorityPhase::OwnerDelivery)
            .await?;
        let store = self.store().await?;
        let source_request = witness.request.clone();
        let report_hash = hash.to_owned();
        let envelope = crate::attachments::documents::source_read::read_retained(
            move || store.report(&scope, &source_request, &report_hash),
            &cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document inspection storage is unavailable")??;
        self.read_source(&witness, &cancel, AuthorityPhase::OwnerDelivery)
            .await?;
        Ok((witness, envelope))
    }

    pub(crate) async fn get(
        &self,
        agent: &str,
        session: &str,
        report_sha256: &str,
    ) -> Result<InspectionEnvelope, String> {
        Ok(self.delivery(agent, session, report_sha256).await?.1)
    }

    pub(crate) async fn page(
        &self,
        agent: &str,
        session: &str,
        report_sha256: &str,
        page: u32,
    ) -> Result<Vec<u8>, String> {
        let (witness, envelope) = self.delivery(agent, session, report_sha256).await?;
        let expected = envelope
            .pages
            .iter()
            .find(|reference| reference.page == page)
            .ok_or("Document inspection page is unavailable")?
            .clone();
        let store = self.store().await?;
        let scope = witness.scope.clone();
        let request = witness.request.clone();
        let hash = report_sha256.to_owned();
        let cancel = CancelSignal::new();
        let bytes = crate::attachments::documents::source_read::read_retained(
            move || store.page(&scope, &request, &hash, &expected),
            &cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|_| "Document inspection page is unavailable")??;
        self.read_source(&witness, &cancel, AuthorityPhase::OwnerDelivery)
            .await?;
        Ok(bytes)
    }
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_publication_capture(
    state: &crate::state::DaemonState,
    witness: &LiveWitness,
    cancel: &CancelSignal,
    execution: Option<&ExecutionContext>,
) -> Result<(), String> {
    check_cancel(cancel)?;
    witness.check()?;
    let scope = readonly_scope_in(state, witness.scope.agent_id(), witness.scope.session_id())?;
    if scope != witness.scope
        || !state
            .agents
            .get(witness.scope.agent_id())
            .is_some_and(|agent| agent.config().allows_tool("inspect_office_document"))
    {
        return Err("Document inspection owner authority changed".into());
    }
    if let Some(run) = &witness.run {
        scope_in(state, run, "inspect_office_document")?;
        validate_execution_binding(
            state,
            run,
            execution.ok_or("Document inspection execution binding is unavailable")?,
        )?;
    }
    if let SourceCustody::Candidate { record } = &witness.custody {
        if state.document_edits.get(&record.draft.id) != Some(record.as_ref()) {
            return Err("Document inspection candidate changed".into());
        }
        validate_candidate(
            state,
            record,
            &witness.scope,
            witness.request.subject.expected_sha256(),
        )?;
    }
    let parent = state
        .document_edit_store
        .as_ref()
        .ok_or("Private document storage is unavailable")?;
    checked_bytes(
        read_custody(parent, witness.scope.root(), &witness.custody)?,
        witness.request.subject.expected_sha256(),
    )?;
    check_cancel(cancel)?;
    witness.check()?;
    if let Some(run) = &witness.run {
        // Source I/O can consume the last admitted milliseconds. This check is
        // immediately before the caller returns its state publication guard.
        validate_execution_binding(
            state,
            run,
            execution.ok_or("Document inspection execution binding is unavailable")?,
        )?;
    }
    Ok(())
}

fn validate_execution_binding(
    state: &crate::state::DaemonState,
    run: &RunLink,
    execution: &ExecutionContext,
) -> Result<(), String> {
    execution.check_live()?;
    if execution.run_id != run.run_id
        || state.execution_budgets.run_scopes.get(&run.run_id) != Some(&execution.scope_id)
        || !state
            .execution_budgets
            .scopes
            .get(&execution.scope_id)
            .is_some_and(|scope| scope.deadline_at_ms == execution.deadline_at_ms)
        || !state
            .execution_budget_failures
            .get(&execution.scope_id)
            .is_some_and(|failures| std::sync::Arc::ptr_eq(failures, &execution.failures))
    {
        return Err("Document inspection execution binding changed or is unavailable".into());
    }
    Ok(())
}
fn validate_producer(
    state: &crate::state::DaemonState,
    run: &RunLink,
    phase: AuthorityPhase,
) -> Result<(), String> {
    if matches!(phase, AuthorityPhase::ActiveRun) {
        return scope_in(state, run, "inspect_office_document").map(drop);
    }
    let record = state
        .runs
        .get(&run.run_id)
        .filter(|record| record.agent_id == run.agent_id && record.session_id == run.session_id)
        .ok_or("Document inspection producer is unavailable")?;
    if record.source != RunSource::Web
        || record.parent_run_id.is_some()
        || record.stop.is_some()
        || !(matches!(
            record.status,
            RunStatus::Running | RunStatus::AwaitingApproval | RunStatus::Completed
        ) || record.allows_outcome_review_delivery())
    {
        return Err("Document inspection producer authority changed".into());
    }
    crate::projects::validate_active_lease(state, record)
        .map_err(|_| "Document inspection project authority changed".into())
}
fn check_cancel(cancel: &CancelSignal) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("Document inspection was cancelled".into())
    } else {
        Ok(())
    }
}
fn office_format(path: &str) -> Result<OfficeFormat, String> {
    match format(path)? {
        value @ (OfficeFormat::Docx | OfficeFormat::Xlsx | OfficeFormat::Pptx) => Ok(value),
        _ => Err("Document layout inspection supports DOCX, XLSX and PPTX".into()),
    }
}
fn checked_bytes(bytes: Vec<u8>, hash: &str) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES || digest(&bytes) != hash {
        Err("Document source changed; inspect it again".into())
    } else {
        Ok(bytes)
    }
}
fn read_workspace(root: &std::path::Path, path: &str, hash: &str) -> Result<Vec<u8>, String> {
    checked_bytes(
        workspace_snapshot(root, path)?.ok_or("Document inspection source is unavailable")?,
        hash,
    )
}
fn read_original(root: &std::path::Path, record: &DocumentEditRecord) -> Result<(), String> {
    match &record.draft.source {
        DocumentEditSource::Workspace { file_path, sha256 } => {
            read_workspace(root, file_path, sha256).map(drop)
        }
        DocumentEditSource::Attachment { sha256, .. } => {
            let row = record
                .draft
                .attachment_record
                .as_ref()
                .ok_or("Document candidate source is unavailable")?;
            let bytes = crate::attachments::files::read_attachment_bytes(root, row)
                .map_err(|_| "Document candidate source changed or is unavailable")?;
            checked_bytes(bytes, sha256).map(drop)
        }
    }
}
fn read_custody(
    store: &crate::document_edits::DocumentEditStore,
    root: &std::path::Path,
    custody: &SourceCustody,
) -> Result<Vec<u8>, String> {
    match custody {
        SourceCustody::Workspace { file_path, sha256 } => read_workspace(root, file_path, sha256),
        SourceCustody::Attachment { record } => {
            let bytes = crate::attachments::files::read_attachment_bytes(root, record)
                .map_err(|_| "Document upload changed or is unavailable")?;
            checked_bytes(bytes, &record.sha256)
        }
        SourceCustody::Candidate { record } => {
            read_original(root, record)?;
            store.load(&record.draft.candidate_sha256, record.draft.candidate_bytes)
        }
    }
}
fn validate_candidate(
    state: &crate::state::DaemonState,
    record: &DocumentEditRecord,
    scope: &OwnerInspectionScope,
    hash: &str,
) -> Result<(), String> {
    record.draft.validate()?;
    if record.status != DocumentEditStatus::Prepared
        || !scope.matches_draft(&record.draft.scope)
        || record.draft.expires_at_ms <= anima_core::primitives::now_millis()
        || record.draft.candidate_sha256 != hash
    {
        return Err("Document inspection candidate changed or is unavailable".into());
    }
    validate_producer(
        state,
        &RunLink {
            run_id: record.draft.scope.run_id.clone(),
            agent_id: record.draft.scope.agent_id.clone(),
            session_id: record.draft.scope.session_id.clone(),
        },
        AuthorityPhase::OwnerDelivery,
    )?;
    Ok(())
}

fn remember_current(
    state: &crate::state::DaemonState,
    key: String,
    witness: &LiveWitness,
    cancel: &CancelSignal,
) -> Result<(), String> {
    check_cancel(cancel)?;
    witness.check()?;
    if readonly_scope_in(state, witness.scope.agent_id(), witness.scope.session_id())?
        != witness.scope
        || !state
            .agents
            .get(witness.scope.agent_id())
            .is_some_and(|agent| agent.config().allows_tool("inspect_office_document"))
    {
        return Err("Document inspection owner authority changed".into());
    }
    if let Some(run) = &witness.run {
        validate_producer(state, run, AuthorityPhase::ActiveRun)?;
    }
    if let SourceCustody::Candidate { record } = &witness.custody {
        if state.document_edits.get(&record.draft.id) != Some(record.as_ref()) {
            return Err("Document inspection candidate changed".into());
        }
        validate_candidate(
            state,
            record,
            &witness.scope,
            witness.request.subject.expected_sha256(),
        )?;
    }
    state.document_inspections.remember(key, witness.clone())
}
pub(super) fn record_key(
    scope: &OwnerInspectionScope,
    report_sha256: &str,
) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_vec(&(1_u8, scope, report_sha256))
            .map_err(|_| "Document inspection scope is invalid")?,
    ))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
