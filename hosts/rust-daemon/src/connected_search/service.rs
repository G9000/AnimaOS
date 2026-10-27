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
}
#[derive(Clone, Debug)]
pub(crate) struct SourceSnapshot {
    pub(crate) documents: Vec<IndexedDocument>,
    pub(crate) stream_epoch: Option<String>,
}
impl From<Vec<IndexedDocument>> for SourceSnapshot {
    fn from(documents: Vec<IndexedDocument>) -> Self {
        Self {
            documents,
            stream_epoch: None,
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
    ) -> Self {
        Self::with_reader(
            state,
            transactions,
            Arc::new(super::providers::NativeReader {
                state: Arc::downgrade(state),
                mail,
                drive,
                calendar,
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
        Ok(SourcesResponse {
            sources: source_views(&s, agent)?,
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
        self.reconcile(agent).await?;
        let views = self.sources(agent).await?;
        if connector.is_some_and(|id| !views.sources.iter().any(|v| v.connector_id == id)) {
            return Err(SearchError::NotFound);
        }
        for view in views
            .sources
            .into_iter()
            .filter(|v| v.enabled && connector.is_none_or(|id| id == v.connector_id))
        {
            let _ = self.refresh_one(agent, &view.connector_id).await;
        }
        self.sources(agent).await
    }
    /// Queue owned native reads so HTTP timeouts cannot cancel a refresh midway.
    pub(crate) async fn request_refresh(
        &self,
        agent: &str,
        connector: Option<&str>,
    ) -> Result<SourcesResponse, SearchError> {
        let response = self.sources(agent).await?;
        if connector.is_some_and(|id| !response.sources.iter().any(|s| s.connector_id == id)) {
            return Err(SearchError::NotFound);
        }
        let service = self.clone();
        let agent = agent.to_string();
        let connector = connector.map(str::to_string);
        tokio::spawn(async move {
            let _ = service.refresh(&agent, connector.as_deref()).await;
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
        let (mut capture, grant_epoch) = {
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
    pub(crate) async fn query(
        &self,
        agent: &str,
        q: &SearchQuery,
    ) -> Result<SearchResponse, SearchError> {
        self.reconcile(agent).await?;
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        query(&s, agent, q)
    }
    pub(crate) async fn item(&self, agent: &str, id: &str) -> Result<SearchItem, SearchError> {
        self.reconcile(agent).await?;
        let state = self.state()?;
        let _tx = self.0.transactions.lock().await;
        let s = state.read().await;
        item(&s, agent, id)
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
        query(&s, agent, q)
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
        item(&s, agent, id)
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
