use super::*;
use crate::app::SharedDaemonState;
use async_trait::async_trait;
use std::sync::{Arc, Weak};
use tokio::sync::{Mutex, RwLock};
type ToolDelta = (
    String,
    Option<Vec<anima_core::ToolDescriptor>>,
    Option<Vec<anima_core::ToolDescriptor>>,
);
fn restore_tools(state: &mut DaemonState, changes: &[ToolDelta]) {
    for (id, old, applied) in changes {
        if state
            .agents
            .get(id)
            .is_some_and(|r| &r.state().config.tools == applied)
        {
            if let Some(runtime) = state.agents.get(id) {
                let mut config = runtime.config().clone();
                config.tools = old.clone();
                state.restore_agent_config(id, config);
            }
        }
    }
}

#[async_trait]
pub(crate) trait SourceReader: Send + Sync {
    async fn read(&self, source: &SourceAuthority) -> Result<SourceSnapshot, SearchError>;
    async fn read_history(
        &self,
        _source: &SourceAuthority,
        _request: &HistoryPageRequest,
    ) -> Result<HistoryPage, SearchError> {
        Err(SearchError::Unavailable)
    }
    async fn inbox_membership(
        &self,
        _source: &SourceAuthority,
        _ids: &[String],
    ) -> Result<Vec<bool>, SearchError> {
        Err(SearchError::Unavailable)
    }
    async fn attachments(
        &self,
        _source: &SourceAuthority,
        _parent: &IndexedDocument,
    ) -> Result<AttachmentBatch, SearchError> {
        Err(SearchError::Unavailable)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct SourceSnapshot {
    pub(crate) documents: Vec<IndexedDocument>,
    pub(crate) stream_epoch: Option<String>,
    pub(crate) selection_revision: Option<u64>,
    pub(crate) incomplete_count: usize,
}
impl From<Vec<IndexedDocument>> for SourceSnapshot {
    fn from(documents: Vec<IndexedDocument>) -> Self {
        Self {
            documents,
            stream_epoch: None,
            selection_revision: None,
            incomplete_count: 0,
        }
    }
}
struct Inner {
    #[cfg(test)]
    save_gate: std::sync::Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    reader: Arc<dyn SourceReader>,
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    history_round: Mutex<usize>,
    instance: String,
    closing: tokio_util::sync::CancellationToken,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.closing.cancel();
    }
}
#[derive(Clone)]
pub(crate) struct SearchService(Arc<Inner>);
#[derive(Clone)]
pub(crate) struct WeakSearchService(Weak<Inner>);
impl WeakSearchService {
    pub(crate) fn upgrade(&self) -> Option<SearchService> {
        self.0.upgrade().map(SearchService)
    }
}
impl SearchService {
    pub(crate) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        mail: crate::connectors::mail::MailManager,
        drive: crate::connectors::drive::DriveManager,
        calendar: crate::connectors::gcalendar::CalendarManager,
        microsoft_files: crate::connectors::microsoft_files::MicrosoftFileManager,
    ) -> Self {
        Self::with_reader(
            state,
            transactions,
            Arc::new(super::providers::NativeReader {
                state: Arc::downgrade(state),
                mail,
                drive,
                calendar,
                microsoft_files,
            }),
        )
    }
    pub(crate) fn with_reader(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        reader: Arc<dyn SourceReader>,
    ) -> Self {
        Self(Arc::new(Inner {
            #[cfg(test)]
            save_gate: std::sync::Mutex::new(None),
            state: Arc::downgrade(state),
            transactions,
            reader,
            gates: Mutex::new(HashMap::new()),
            history_round: Mutex::new(0),
            instance: uuid::Uuid::new_v4().to_string(),
            closing: tokio_util::sync::CancellationToken::new(),
        }))
    }
    pub(crate) fn weak(&self) -> WeakSearchService {
        WeakSearchService(Arc::downgrade(&self.0))
    }
    fn state(&self) -> Result<SharedDaemonState, SearchError> {
        self.0.state.upgrade().ok_or(SearchError::Unavailable)
    }
    pub(crate) fn start(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let weak = self.weak();
        let closing = self.0.closing.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {_=closing.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS))=>{}}
                let Some(service) = weak.upgrade() else {
                    break;
                };
                let Ok(state) = service.state() else {
                    break;
                };
                let agents = {
                    let s = state.read().await;
                    s.connected_search
                        .sources
                        .values()
                        .map(|s| s.agent_id.clone())
                        .collect::<HashSet<_>>()
                };
                for agent in agents {
                    let _ = service.refresh(&agent, None).await;
                }
            }
        });
    }
    pub(crate) async fn sources(&self, agent: &str) -> Result<SourcesResponse, SearchError> {
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        let mut views = source_views(&s, agent)?;
        mark_instance_coverage(&s, &mut views, &self.0.instance);
        Ok(SourcesResponse {
            sources: views,
            poll_interval_ms: POLL_INTERVAL_MS,
        })
    }
    /// All index and workflow changes share the durable control-plane transaction.
    async fn edit<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut DaemonState) -> Result<T, SearchError> + Send + 'static,
    ) -> Result<T, SearchError> {
        let service = self.clone();
        tokio::spawn(async move { service.edit_owned(f).await })
            .await
            .map_err(|_| SearchError::Persistence)?
    }
    async fn edit_owned<T>(
        &self,
        f: impl FnOnce(&mut DaemonState) -> Result<T, SearchError>,
    ) -> Result<T, SearchError> {
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let mut s = state.write().await;
        let old_index = s.connected_search.clone();
        let old_rules = s.account_workflows.clone();
        let old_workflows = s.workflows.clone();
        let old_tools = s
            .agents
            .iter()
            .map(|(id, r)| (id.clone(), r.state().config.tools.clone()))
            .collect::<Vec<_>>();
        let result = f(&mut s).and_then(|r| {
            validate_snapshot(&s.connected_search).map_err(|_| SearchError::Capacity)?;
            Ok(r)
        });
        let tool_changes = old_tools
            .into_iter()
            .filter_map(|(id, old)| {
                let applied = s.agents.get(&id)?.state().config.tools;
                (old != applied).then_some((id, old, applied))
            })
            .collect::<Vec<_>>();
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                s.connected_search = old_index;
                s.account_workflows = old_rules;
                s.workflows = old_workflows;
                restore_tools(&mut s, &tool_changes);
                return Err(e);
            }
        };
        let persist = s.control_plane_persist_request();
        drop(s);
        #[cfg(test)]
        {
            let gate = self.0.save_gate.lock().unwrap().take();
            if let Some((entered, release)) = gate {
                entered.notify_one();
                release.notified().await;
            }
        }
        if persist.save().await.is_err() {
            let mut s = state.write().await;
            s.connected_search = old_index;
            s.account_workflows = old_rules;
            s.workflows = old_workflows;
            restore_tools(&mut s, &tool_changes);
            return Err(SearchError::Persistence);
        }
        Ok(result)
    }
    pub(crate) async fn configure(
        &self,
        agent: &str,
        id: &str,
        input: SourceInput,
    ) -> Result<SourceView, SearchError> {
        if input.expected_generation.len() > 128 {
            return Err(SearchError::Invalid);
        }
        if input.settings.is_some() && input.expected_settings_revision.is_none() {
            return Err(SearchError::Invalid);
        }
        if !input.enabled && input.settings.is_some() {
            return Err(SearchError::Invalid);
        }
        let agent = agent.to_string();
        let id = id.to_string();
        self.edit(move |s| {
            let agent = agent.as_str();
            let id = id.as_str();
            let authority = source_authority(s, agent, id).ok_or(SearchError::NotFound)?;
            if authority.generation != input.expected_generation {
                return Err(SearchError::Conflict);
            }
            if input.enabled && !authority.read_granted {
                return Err(SearchError::Unavailable);
            }
            if input.enabled {
                if !s.connected_search.sources.contains_key(id)
                    && s.connected_search.sources.len() >= MAX_SOURCES
                {
                    return Err(SearchError::Capacity);
                }
                let already = s
                    .connected_search
                    .sources
                    .get(id)
                    .is_some_and(|i| i.enabled && i.generation == authority.generation);
                if !already {
                    s.connected_search
                        .sources
                        .insert(id.into(), SourceIndex::new(&authority));
                }
                if let Some(settings) = input.settings {
                    if !settings.valid_for(&authority.provider) {
                        return Err(SearchError::UnsupportedSetting);
                    }
                    let index = s
                        .connected_search
                        .sources
                        .get_mut(id)
                        .ok_or(SearchError::Conflict)?;
                    if index.settings != settings {
                        if input.expected_settings_revision != Some(index.settings_revision) {
                            return Err(SearchError::Conflict);
                        }
                        let now = crate::connectors::gcalendar::now_ms();
                        if index.settings.history_depth != settings.history_depth {
                            if let Some(days) = settings.history_depth.days() {
                                index.live_event_fence_at_ms.get_or_insert(now);
                                index.history = Some(HistoryProgress {
                                    coverage: HistoryCoverage {
                                        state: if settings.history_paused {
                                            HistoryStatus::Paused
                                        } else {
                                            HistoryStatus::Pending
                                        },
                                        requested_from_ms: Some(
                                            now.saturating_sub(days * 86_400_000),
                                        ),
                                        requested_before_ms: Some(now),
                                        ..Default::default()
                                    },
                                    cursor: None,
                                    pass_id: uuid::Uuid::new_v4().to_string(),
                                    verification_cursor: None,
                                    cursor_fingerprints: vec![],
                                });
                                let cutoff = now.saturating_sub(days * 86_400_000);
                                index.historical.retain(|_, document| {
                                    document
                                        .source_modified_at
                                        .as_deref()
                                        .and_then(|value| {
                                            chrono::DateTime::parse_from_rfc3339(value).ok()
                                        })
                                        .and_then(|value| {
                                            u64::try_from(value.timestamp_millis()).ok()
                                        })
                                        .is_some_and(|received| received >= cutoff)
                                });
                            } else {
                                index.history = None;
                                index.historical.clear();
                            }
                        } else if let Some(history) = index.history.as_mut() {
                            if settings.history_paused
                                && matches!(
                                    history.coverage.state,
                                    HistoryStatus::Pending | HistoryStatus::Running
                                )
                            {
                                history.coverage.state = HistoryStatus::Paused;
                            } else if !settings.history_paused
                                && history.coverage.state == HistoryStatus::Paused
                            {
                                history.coverage.state = HistoryStatus::Pending;
                            } else if history.coverage.state == HistoryStatus::Partial
                                && history.coverage.reason.as_deref() == Some("capacity")
                                && settings.retained_item_limit > index.settings.retained_item_limit
                            {
                                history.coverage.state = HistoryStatus::Pending;
                                history.coverage.reason = None;
                            }
                        }
                        if settings.retained_item_limit < index.settings.retained_item_limit {
                            let live_ids: HashSet<_> = index
                                .documents
                                .iter()
                                .map(|document| document.citation_id.clone())
                                .collect();
                            let keep = usize::from(settings.retained_item_limit)
                                .saturating_sub(live_ids.len());
                            let mut old = index
                                .historical
                                .iter()
                                .filter(|(id, _)| !live_ids.contains(*id))
                                .map(|(id, document)| {
                                    (document.source_modified_at.clone(), id.clone())
                                })
                                .collect::<Vec<_>>();
                            old.sort_by(|left, right| right.cmp(left));
                            for (_, id) in old.into_iter().skip(keep) {
                                index.historical.remove(&id);
                            }
                        }
                        if settings.attachment_text == AttachmentTextMode::MetadataOnly {
                            index.attachments.clear();
                            index.attachment_scanned.clear();
                            index.attachment_retries.clear();
                            index.attachment_omitted.clear();
                            index.attachment_error = None;
                        }
                        retain_enrichment(index);
                        index.settings_revision = index
                            .settings_revision
                            .checked_add(1)
                            .ok_or(SearchError::Capacity)?;
                        index.settings = settings;
                    }
                }
                let mut tools = s
                    .get_agent(agent)
                    .ok_or(SearchError::NotFound)?
                    .state
                    .config
                    .tools
                    .unwrap_or_default();
                if !tools.iter().any(|t| t.name == "connected_search") {
                    let descriptors = s
                        .tool_registry
                        .resolve_descriptors(["connected_search".to_string()])
                        .map_err(|_| SearchError::Unavailable)?;
                    tools.extend(descriptors);
                    s.update_agent(
                        agent,
                        anima_core::AgentConfigUpdate {
                            tools: Some(tools),
                            ..Default::default()
                        },
                    )
                    .map_err(|_| SearchError::NotFound)?;
                }
            } else {
                s.connected_search.sources.remove(id);
                crate::account_workflows::pause_for_source(s, agent, id, "Search source disabled");
            }
            source_views(s, agent)?
                .into_iter()
                .find(|v| v.connector_id == id)
                .ok_or(SearchError::NotFound)
        })
        .await
    }
    async fn reconcile(&self, agent: &str) -> Result<(), SearchError> {
        let agent = agent.to_string();
        self.edit(move |s| {
            let agent = agent.as_str();
            if s.get_agent(agent).is_none() {
                return Err(SearchError::NotFound);
            }
            let invalid = s
                .connected_search
                .sources
                .iter()
                .filter(|(_, i)| i.agent_id == agent)
                .filter(|(id, _)| source_authority(s, agent, id).is_none_or(|a| !a.enabled))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in invalid {
                s.connected_search.sources.remove(&id);
                crate::account_workflows::pause_for_source(
                    s,
                    agent,
                    &id,
                    "Account authorization or read permission changed",
                );
            }
            Ok(())
        })
        .await
    }
    pub(crate) async fn refresh(
        &self,
        agent: &str,
        connector: Option<&str>,
    ) -> Result<SourcesResponse, SearchError> {
        self.refresh_work(agent, connector, RefreshWork::All).await
    }
    async fn refresh_work(
        &self,
        agent: &str,
        connector: Option<&str>,
        work: RefreshWork,
    ) -> Result<SourcesResponse, SearchError> {
        self.reconcile(agent).await?;
        let views = self.sources(agent).await?;
        if connector.is_some_and(|id| !views.sources.iter().any(|v| v.connector_id == id)) {
            return Err(SearchError::NotFound);
        }
        let selected = views
            .sources
            .into_iter()
            .filter(|v| v.enabled && connector.is_none_or(|id| id == v.connector_id))
            .collect::<Vec<_>>();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
        if matches!(work, RefreshWork::Live | RefreshWork::All) {
            for view in &selected {
                if tokio::time::timeout_at(deadline, self.refresh_one(agent, &view.connector_id))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
        if matches!(work, RefreshWork::History | RefreshWork::All) {
            let history_deadline =
                deadline.min(tokio::time::Instant::now() + std::time::Duration::from_secs(20));
            let offset = if selected.is_empty() {
                0
            } else {
                let mut round = self.0.history_round.lock().await;
                let offset = *round % selected.len();
                *round = round.wrapping_add(1);
                offset
            };
            for view in selected.iter().cycle().skip(offset).take(selected.len()) {
                if tokio::time::timeout_at(
                    history_deadline,
                    self.refresh_history_one(agent, &view.connector_id),
                )
                .await
                .is_err()
                {
                    break;
                }
            }
        }
        self.sources(agent).await
    }
    /// Queue owned native reads so HTTP timeouts cannot cancel a refresh midway.
    pub(crate) async fn request_refresh(
        &self,
        agent: &str,
        connector: Option<&str>,
        work: RefreshWork,
    ) -> Result<SourcesResponse, SearchError> {
        let response = self.sources(agent).await?;
        if connector.is_some_and(|id| !response.sources.iter().any(|s| s.connector_id == id)) {
            return Err(SearchError::NotFound);
        }
        let service = self.clone();
        let agent = agent.to_string();
        let connector = connector.map(str::to_string);
        tokio::spawn(async move {
            let _ = service
                .refresh_work(&agent, connector.as_deref(), work)
                .await;
        });
        Ok(response)
    }
    async fn refresh_one(&self, agent: &str, id: &str) -> Result<(), SearchError> {
        let gate = self
            .0
            .gates
            .lock()
            .await
            .entry(id.into())
            .or_default()
            .clone();
        let Ok(_guard) = gate.try_lock() else {
            return Ok(());
        };
        let state = self.state()?;
        let (mut capture, grant_epoch, denial_fences) = {
            let _tx = self.0.transactions.lock().await;
            let s = state.read().await;
            let a = source_authority(&s, agent, id)
                .filter(|a| a.enabled)
                .ok_or(SearchError::Conflict)?;
            (
                a,
                s.connected_search
                    .sources
                    .get(id)
                    .ok_or(SearchError::Conflict)?
                    .grant_epoch
                    .clone(),
                s.connected_search
                    .sources
                    .get(id)
                    .ok_or(SearchError::Conflict)?
                    .microsoft_denial_fences
                    .clone(),
            )
        };
        capture.sync_started_at_ms = crate::connectors::gcalendar::now_ms();
        let fetched = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            self.0.reader.read(&capture),
        )
        .await
        .unwrap_or(Err(SearchError::Unavailable));
        let committed = capture.clone();
        let committed_epoch = grant_epoch.clone();
        let instance = self.0.instance.clone();
        let result = self
            .edit(move |s| {
                let capture = committed;
                let agent = capture.agent_id.as_str();
                let id = capture.connector_id.as_str();
                let grant_epoch = committed_epoch;
                let current = source_authority(s, agent, id)
                    .filter(|a| a.enabled && a.generation == capture.generation)
                    .ok_or(SearchError::Conflict)?;
                if current.generation != capture.generation {
                    return Err(SearchError::Conflict);
                }
                let index = s
                    .connected_search
                    .sources
                    .get_mut(id)
                    .ok_or(SearchError::Conflict)?;
                if index.grant_epoch != grant_epoch {
                    return Err(SearchError::Conflict);
                }
                let docs = match fetched {
                    Ok(d) => d,
                    Err(e) => {
                        index.last_attempt_at_ms = Some(capture.sync_started_at_ms);
                        index.error = Some(e.to_string());
                        return Ok(());
                    }
                };
                if matches!(capture.provider.as_str(), "onedrive" | "sharepoint")
                    && docs.documents.iter().any(|document| {
                        index.microsoft_denial_fences.get(&document.external_id)
                            != denial_fences.get(&document.external_id)
                    })
                {
                    return Err(SearchError::Conflict);
                }
                if matches!(capture.provider.as_str(), "onedrive" | "sharepoint")
                    && s.microsoft_file_records.get(id).is_none_or(|record| {
                        Some(record.selection_revision) != docs.selection_revision
                    })
                {
                    return Err(SearchError::Conflict);
                }
                if docs.stream_epoch != index.stream_epoch {
                    index.stream_epoch = docs.stream_epoch;
                    index.baseline_at_ms = None;
                    index.seen.clear();
                }
                let baseline = index.baseline_at_ms.is_none();
                let changes = apply_snapshot(
                    index,
                    &capture,
                    docs.documents,
                    crate::connectors::gcalendar::now_ms(),
                )?;
                if docs.incomplete_count > 0 {
                    index.error =
                        Some("Some selected files are unavailable; results are incomplete".into());
                }
                for document in &index.documents {
                    if matches!(capture.provider.as_str(), "onedrive" | "sharepoint")
                        || index.imported.contains_key(&document.citation_id)
                    {
                        index
                            .verified_membership
                            .insert(document.citation_id.clone(), instance.clone());
                    }
                }
                crate::account_workflows::admit_changes(s, &capture, &changes, baseline)
                    .map_err(|_| SearchError::Capacity)?;
                Ok(())
            })
            .await;
        if let Err(error) = result {
            if error != SearchError::Conflict {
                let message = error.to_string();
                let id = id.to_string();
                let _ = self
                    .edit(move |s| {
                        if let Some(index) = s.connected_search.sources.get_mut(&id).filter(|i| {
                            i.generation == capture.generation && i.grant_epoch == grant_epoch
                        }) {
                            index.error = Some(message);
                            index.last_attempt_at_ms = Some(capture.sync_started_at_ms);
                        }
                        Ok(())
                    })
                    .await;
            }
        }
        result
    }

    async fn refresh_history_one(&self, agent: &str, id: &str) -> Result<(), SearchError> {
        let gate = self
            .0
            .gates
            .lock()
            .await
            .entry(id.into())
            .or_default()
            .clone();
        let Ok(_guard) = gate.try_lock() else {
            return Ok(());
        };
        self.revalidate_history(agent, id).await?;
        let needs_history = {
            let state = self.state()?;
            let state = state.read().await;
            state
                .connected_search
                .sources
                .get(id)
                .is_some_and(|source| {
                    !source.settings.history_paused
                        && source.history.as_ref().is_some_and(|history| {
                            matches!(
                                history.coverage.state,
                                HistoryStatus::Pending | HistoryStatus::Error
                            )
                        })
                })
        };
        if needs_history {
            self.refresh_history(agent, id).await
        } else {
            self.refresh_attachments(agent, id).await
        }
    }

    async fn refresh_attachments(&self, agent: &str, id: &str) -> Result<(), SearchError> {
        let state = self.state()?;
        let (authority, grant_epoch, settings_revision, selection_revision, denial_fence, parent) = {
            let _tx = self.0.transactions.lock().await;
            let state = state.read().await;
            let authority = source_authority(&state, agent, id)
                .filter(|a| a.enabled)
                .ok_or(SearchError::Conflict)?;
            let index = state
                .connected_search
                .sources
                .get(id)
                .ok_or(SearchError::Conflict)?;
            if index.settings.attachment_text != AttachmentTextMode::SupportedText
                || !matches!(
                    authority.provider.as_str(),
                    "gmail" | "outlook" | "gdrive" | "onedrive" | "sharepoint"
                )
            {
                return Ok(());
            }
            let Some(parent) = index
                .documents
                .iter()
                .chain(index.historical.values())
                .find(|document| {
                    (document.kind == DocumentKind::Mail
                        || (matches!(
                            authority.provider.as_str(),
                            "gdrive" | "onedrive" | "sharepoint"
                        ) && document.kind == DocumentKind::File
                            && document.text.is_empty()))
                        && index.attachment_scanned.get(&document.citation_id)
                            != Some(&document.revision)
                        && attachments::retry_due(
                            index,
                            document,
                            crate::connectors::gcalendar::now_ms(),
                        )
                })
                .cloned()
            else {
                return Ok(());
            };
            let selection_revision = state
                .microsoft_file_records
                .get(id)
                .map(|record| record.selection_revision);
            let denial_fence = index
                .microsoft_denial_fences
                .get(&parent.external_id)
                .cloned();
            (
                authority,
                index.grant_epoch.clone(),
                index.settings_revision,
                selection_revision,
                denial_fence,
                parent,
            )
        };
        let ids = vec![parent.external_id.clone()];
        if parent.kind == DocumentKind::Mail
            && !self
                .check_attachment_parent(&authority, &grant_epoch, settings_revision, &parent, &ids)
                .await?
        {
            return Ok(());
        }
        let batch = match self.0.reader.attachments(&authority, &parent).await {
            Ok(batch) => batch,
            Err(error) => {
                if error == SearchError::Unavailable {
                    let captured = authority.clone();
                    let epoch = grant_epoch.clone();
                    let current_parent = parent.clone();
                    self.edit(move |state| {
                        source_authority(state, &captured.agent_id, &captured.connector_id)
                            .filter(|current| {
                                current.enabled && current.generation == captured.generation
                            })
                            .ok_or(SearchError::Conflict)?;
                        let index = state
                            .connected_search
                            .sources
                            .get_mut(&captured.connector_id)
                            .ok_or(SearchError::Conflict)?;
                        if index.grant_epoch != epoch
                            || index.settings_revision != settings_revision
                            || !index.documents.iter().chain(index.historical.values()).any(
                                |document| {
                                    document.citation_id == current_parent.citation_id
                                        && document.revision == current_parent.revision
                                },
                            )
                        {
                            return Err(SearchError::Conflict);
                        }
                        attachments::schedule_retry(
                            index,
                            &current_parent,
                            crate::connectors::gcalendar::now_ms(),
                        );
                        Ok(())
                    })
                    .await?;
                }
                return Err(error);
            }
        };
        if batch.items.len() > 10
            || batch.omitted > 100
            || batch
                .items
                .iter()
                .filter_map(|item| item.bytes.as_ref())
                .map(Vec::len)
                .sum::<usize>()
                > 4 * 1024 * 1024
        {
            return Err(SearchError::Capacity);
        }
        let mut entries = Vec::new();
        let mut ids = HashSet::new();
        for raw in batch.items {
            if !ids.insert(raw.id.clone()) {
                return Err(SearchError::Invalid);
            }
            entries.push(
                attachments::extract_attachment(
                    &authority,
                    &parent,
                    raw,
                    crate::connectors::gcalendar::now_ms(),
                )
                .await?,
            );
        }
        if parent.kind == DocumentKind::Mail
            && !self
                .check_attachment_parent(
                    &authority,
                    &grant_epoch,
                    settings_revision,
                    &parent,
                    &[parent.external_id.clone()],
                )
                .await?
        {
            return Ok(());
        }
        self.edit(move |state| {
            ensure_silent_origin(IngestionOrigin::ContentEnrichment)?;
            source_authority(state, &authority.agent_id, &authority.connector_id)
                .filter(|current| current.enabled && current.generation == authority.generation)
                .ok_or(SearchError::Conflict)?;
            if selection_revision.is_some_and(|revision| {
                state
                    .microsoft_file_records
                    .get(&authority.connector_id)
                    .is_none_or(|record| record.selection_revision != revision)
            }) {
                return Err(SearchError::Conflict);
            }
            let index = state
                .connected_search
                .sources
                .get_mut(&authority.connector_id)
                .ok_or(SearchError::Conflict)?;
            if index.grant_epoch != grant_epoch
                || index.settings_revision != settings_revision
                || index.settings.attachment_text != AttachmentTextMode::SupportedText
                || index.microsoft_denial_fences.get(&parent.external_id) != denial_fence.as_ref()
                || !index
                    .documents
                    .iter()
                    .chain(index.historical.values())
                    .any(|document| {
                        document.citation_id == parent.citation_id
                            && document.revision == parent.revision
                    })
            {
                return Err(SearchError::Conflict);
            }
            index
                .attachments
                .retain(|_, entry| entry.parent_citation_id != parent.citation_id);
            if index.attachments.len() + entries.len() > 256 {
                return Err(SearchError::Capacity);
            }
            let retry = entries.iter().any(|entry| entry.status == "unavailable");
            for entry in entries {
                index
                    .attachments
                    .insert(entry.document.citation_id.clone(), entry);
            }
            if retry {
                attachments::schedule_retry(index, &parent, crate::connectors::gcalendar::now_ms());
            } else {
                index.attachment_retries.remove(&parent.citation_id);
                index
                    .attachment_scanned
                    .insert(parent.citation_id.clone(), parent.revision.clone());
                index
                    .attachment_omitted
                    .insert(parent.citation_id.clone(), batch.omitted);
                if index.attachment_retries.is_empty() {
                    index.attachment_error = None;
                }
            }
            if serde_json::to_vec(index)
                .map_err(|_| SearchError::Invalid)?
                .len()
                > 2 * 1024 * 1024
            {
                return Err(SearchError::Capacity);
            }
            if serde_json::to_vec(&state.connected_search)
                .map_err(|_| SearchError::Invalid)?
                .len()
                > 12 * 1024 * 1024
            {
                return Err(SearchError::Capacity);
            }
            Ok(())
        })
        .await
    }

    async fn check_attachment_parent(
        &self,
        authority: &SourceAuthority,
        grant_epoch: &str,
        settings_revision: u64,
        parent: &IndexedDocument,
        ids: &[String],
    ) -> Result<bool, SearchError> {
        let membership = self.0.reader.inbox_membership(authority, ids).await?;
        if membership.len() != 1 {
            return Err(SearchError::Invalid);
        }
        if membership[0] {
            return Ok(true);
        }
        let authority = authority.clone();
        let grant_epoch = grant_epoch.to_owned();
        let parent = parent.clone();
        self.edit(move |state| {
            source_authority(state, &authority.agent_id, &authority.connector_id)
                .filter(|current| current.enabled && current.generation == authority.generation)
                .ok_or(SearchError::Conflict)?;
            let index = state
                .connected_search
                .sources
                .get_mut(&authority.connector_id)
                .ok_or(SearchError::Conflict)?;
            if index.grant_epoch != grant_epoch || index.settings_revision != settings_revision {
                return Err(SearchError::Conflict);
            }
            index.documents.retain(|document| {
                document.citation_id != parent.citation_id || document.revision != parent.revision
            });
            if index
                .historical
                .get(&parent.citation_id)
                .is_some_and(|document| document.revision == parent.revision)
            {
                index.historical.remove(&parent.citation_id);
            }
            index.verified_membership.remove(&parent.citation_id);
            retain_enrichment(index);
            Ok(())
        })
        .await?;
        Ok(false)
    }

    async fn revalidate_history(&self, agent: &str, id: &str) -> Result<(), SearchError> {
        let state = self.state()?;
        let (authority, grant_epoch, settings_revision, pass_id, entries, last, completed) = {
            let _tx = self.0.transactions.lock().await;
            let state = state.read().await;
            let authority = source_authority(&state, agent, id)
                .filter(|a| a.enabled)
                .ok_or(SearchError::Conflict)?;
            let index = state
                .connected_search
                .sources
                .get(id)
                .ok_or(SearchError::Conflict)?;
            let Some(progress) = index.history.as_ref() else {
                return Ok(());
            };
            if index.historical.is_empty() {
                return Ok(());
            }
            let cursor = progress.verification_cursor.as_deref();
            let mut entries: Vec<_> = index
                .historical
                .iter()
                .filter(|(key, _)| cursor.is_none_or(|last| key.as_str() > last))
                .take(8)
                .map(|(key, document)| {
                    (
                        key.clone(),
                        document.external_id.clone(),
                        document.revision.clone(),
                    )
                })
                .collect();
            if entries.is_empty() {
                entries = index
                    .historical
                    .iter()
                    .take(8)
                    .map(|(key, document)| {
                        (
                            key.clone(),
                            document.external_id.clone(),
                            document.revision.clone(),
                        )
                    })
                    .collect();
            }
            let last = entries.last().map(|entry| entry.0.clone());
            let completed = last
                .as_ref()
                .is_none_or(|last| !index.historical.keys().any(|key| key > last));
            (
                authority,
                index.grant_epoch.clone(),
                index.settings_revision,
                progress.pass_id.clone(),
                entries,
                last,
                completed,
            )
        };
        let ids = entries
            .iter()
            .map(|entry| entry.1.clone())
            .collect::<Vec<_>>();
        let present = self.0.reader.inbox_membership(&authority, &ids).await?;
        if present.len() != entries.len() {
            return Err(SearchError::Invalid);
        }
        let instance = self.0.instance.clone();
        self.edit(move |state| {
            source_authority(state, &authority.agent_id, &authority.connector_id)
                .filter(|current| current.enabled && current.generation == authority.generation)
                .ok_or(SearchError::Conflict)?;
            let index = state
                .connected_search
                .sources
                .get_mut(&authority.connector_id)
                .ok_or(SearchError::Conflict)?;
            if index.grant_epoch != grant_epoch
                || index.settings_revision != settings_revision
                || !index
                    .history
                    .as_ref()
                    .is_some_and(|history| history.pass_id == pass_id)
            {
                return Err(SearchError::Conflict);
            }
            for ((citation, _, revision), present) in entries.into_iter().zip(present) {
                if !present
                    && index
                        .historical
                        .get(&citation)
                        .is_some_and(|document| document.revision == revision)
                {
                    index.historical.remove(&citation);
                    index
                        .documents
                        .retain(|document| document.citation_id != citation);
                    index.verified_membership.remove(&citation);
                } else if present {
                    index.verified_membership.insert(citation, instance.clone());
                }
            }
            retain_enrichment(index);
            let history = index.history.as_mut().ok_or(SearchError::Conflict)?;
            history.verification_cursor = if completed { None } else { last };
            if completed {
                history.coverage.verified_at_ms = Some(crate::connectors::gcalendar::now_ms());
            }
            history.coverage.retained_count = index.historical.len();
            Ok(())
        })
        .await
    }

    async fn refresh_history(&self, agent: &str, id: &str) -> Result<(), SearchError> {
        let state = self.state()?;
        let (mut authority, grant_epoch, settings_revision, pass_id, request) = {
            let _tx = self.0.transactions.lock().await;
            let state = state.read().await;
            let authority = source_authority(&state, agent, id)
                .filter(|a| a.enabled)
                .ok_or(SearchError::Conflict)?;
            let index = state
                .connected_search
                .sources
                .get(id)
                .ok_or(SearchError::Conflict)?;
            let Some(progress) = index.history.as_ref() else {
                return Ok(());
            };
            if index.settings.history_paused
                || !matches!(
                    progress.coverage.state,
                    HistoryStatus::Pending | HistoryStatus::Error
                )
            {
                return Ok(());
            }
            let request = HistoryPageRequest {
                from_ms: progress
                    .coverage
                    .requested_from_ms
                    .ok_or(SearchError::Invalid)?,
                before_ms: progress
                    .coverage
                    .requested_before_ms
                    .ok_or(SearchError::Invalid)?,
                cursor: progress.cursor.clone(),
            };
            (
                authority,
                index.grant_epoch.clone(),
                index.settings_revision,
                progress.pass_id.clone(),
                request,
            )
        };
        authority.sync_started_at_ms = crate::connectors::gcalendar::now_ms();
        let fetched = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            let mut page = self.0.reader.read_history(&authority, &request).await?;
            if !page.documents.is_empty() {
                let ids = page
                    .documents
                    .iter()
                    .map(|document| document.external_id.clone())
                    .collect::<Vec<_>>();
                let membership = self.0.reader.inbox_membership(&authority, &ids).await?;
                if membership.len() != page.documents.len() {
                    return Err(SearchError::Invalid);
                }
                page.documents = page
                    .documents
                    .into_iter()
                    .zip(membership)
                    .filter_map(|(document, present)| present.then_some(document))
                    .collect();
            }
            Ok(page)
        })
        .await
        .unwrap_or(Err(SearchError::Unavailable));
        let captured = authority.clone();
        let captured_epoch = grant_epoch.clone();
        let captured_pass = pass_id.clone();
        let captured_cursor = request.cursor.clone();
        let instance = self.0.instance.clone();
        let result = self
            .edit(move |state| {
                source_authority(state, &captured.agent_id, &captured.connector_id)
                    .filter(|current| current.enabled && current.generation == captured.generation)
                    .ok_or(SearchError::Conflict)?;
                let index = state
                    .connected_search
                    .sources
                    .get_mut(&captured.connector_id)
                    .ok_or(SearchError::Conflict)?;
                let current = index.history.as_ref().ok_or(SearchError::Conflict)?;
                if index.grant_epoch != captured_epoch
                    || index.settings_revision != settings_revision
                    || index.settings.history_paused
                    || current.pass_id != captured_pass
                    || current.cursor != captured_cursor
                {
                    return Err(SearchError::Conflict);
                }
                let page = fetched?;
                let verified = page
                    .documents
                    .iter()
                    .map(|document| {
                        normalize(document.clone(), &captured, 0)
                            .map(|document| document.citation_id)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                apply_history_page(
                    index,
                    &captured,
                    page,
                    crate::connectors::gcalendar::now_ms(),
                )?;
                for citation in verified {
                    index.verified_membership.insert(citation, instance.clone());
                }
                if serde_json::to_vec(&state.connected_search)
                    .map_err(|_| SearchError::Invalid)?
                    .len()
                    > 12 * 1024 * 1024
                {
                    return Err(SearchError::Capacity);
                }
                Ok(())
            })
            .await;
        if let Err(error) = result {
            if error != SearchError::Conflict {
                let captured = authority.clone();
                let captured_epoch = grant_epoch;
                let captured_pass = pass_id;
                let _ = self
                    .edit(move |state| {
                        let Some(index) = state
                            .connected_search
                            .sources
                            .get_mut(&captured.connector_id)
                            .filter(|index| {
                                index.generation == captured.generation
                                    && index.grant_epoch == captured_epoch
                                    && index.settings_revision == settings_revision
                            })
                        else {
                            return Ok(());
                        };
                        let Some(progress) = index
                            .history
                            .as_mut()
                            .filter(|progress| progress.pass_id == captured_pass)
                        else {
                            return Ok(());
                        };
                        progress.coverage.state = if error == SearchError::Capacity {
                            HistoryStatus::Partial
                        } else {
                            HistoryStatus::Error
                        };
                        progress.coverage.reason = Some(
                            if error == SearchError::Capacity {
                                "capacity"
                            } else {
                                "provider"
                            }
                            .into(),
                        );
                        progress.coverage.last_attempt_at_ms = Some(captured.sync_started_at_ms);
                        Ok(())
                    })
                    .await;
            }
        }
        result
    }
    pub(crate) async fn query(
        &self,
        agent: &str,
        q: &SearchQuery,
    ) -> Result<SearchResponse, SearchError> {
        self.reconcile(agent).await?;
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        query(&s, agent, q, Some(&self.0.instance))
    }
    pub(crate) async fn item(&self, agent: &str, id: &str) -> Result<SearchItem, SearchError> {
        self.reconcile(agent).await?;
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        item(&s, agent, id, Some(&self.0.instance))
    }
    pub(crate) async fn model_query(
        &self,
        agent: &str,
        q: &SearchQuery,
    ) -> Result<SearchResponse, SearchError> {
        self.reconcile(agent).await?;
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        if !s
            .agents
            .get(agent)
            .is_some_and(|r| r.config().allows_tool("connected_search"))
        {
            return Err(SearchError::Unavailable);
        }
        query(&s, agent, q, Some(&self.0.instance))
    }
    pub(crate) async fn model_item(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<SearchItem, SearchError> {
        self.reconcile(agent).await?;
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        if !s
            .agents
            .get(agent)
            .is_some_and(|r| r.config().allows_tool("connected_search"))
        {
            return Err(SearchError::Unavailable);
        }
        item(&s, agent, id, Some(&self.0.instance))
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_waiters_do_not_publish_provisional_content_or_cancel_rollback() {
        let (state, agent, _reader, service) = super::super::tests::fixture();
        super::super::tests::enable(&service, &agent, "mail-gmail").await;
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("blocked");
        std::fs::write(&blocked, b"file").unwrap();
        state.write().await.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(blocked.join("state.json")),
        ));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        *service.0.save_gate.lock().unwrap() = Some((entered.clone(), release.clone()));
        let editing = {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .edit(|s| {
                        s.connected_search
                            .sources
                            .get_mut("mail-gmail")
                            .unwrap()
                            .error = Some("provisional secret".into());
                        Ok(())
                    })
                    .await
            })
        };
        entered.notified().await;
        let mut reading = {
            let service = service.clone();
            let agent = agent.clone();
            tokio::spawn(async move { service.sources(&agent).await })
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut reading)
                .await
                .is_err(),
            "Read must wait while provisional index save is unsettled"
        );
        editing.abort();
        release.notify_one();
        let response = reading.await.unwrap().unwrap();
        let source = response
            .sources
            .iter()
            .find(|s| s.connector_id == "mail-gmail")
            .unwrap();
        assert!(
            source.error.is_none(),
            "Owned rollback survives caller cancellation; provisional content is never observable"
        );
        assert!(state.read().await.connected_search.sources["mail-gmail"]
            .error
            .is_none());
    }
}
