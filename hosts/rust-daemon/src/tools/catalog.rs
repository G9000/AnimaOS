//! Immutable run-local discovery. Model visibility never grants dispatch authority.
use crate::app::SharedDaemonState;
use crate::runs::RunLink;
use crate::state::DaemonState;
use anima_core::{
    AgentConfig, AgentState, CancelSignal, Content, DataValue, Message, ModelToolSelector,
    TaskResult, ToolCall, ToolDescriptor,
};
use futures::future::BoxFuture;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::RwLock;

const SEARCH: &str = "search_tools";
const DESCRIBE: &str = "describe_tool";
const MAX_HYDRATED: usize = 8;
const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_RESULT_BYTES: usize = 4096;
const ESSENTIALS: &[&str] = &["clarify", "get_current_time", "todo_list", "memory_search"];
const REFUSED: &str = "Run tool catalog authority changed or is unavailable";

struct Entry {
    descriptor: ToolDescriptor,
    bytes: usize,
    origin: Origin,
}
enum Origin {
    Explicit(ToolDescriptor),
    MemoryAdd(ToolDescriptor),
    HostDerived,
    MainChatClear,
    OwnerChatControl,
    Mcp(crate::mcp::McpDispatchStamp),
}
/// A captured ceiling over the final, filtered run descriptors. Its only
/// mutable state is bounded advertisement history, never grants or schemas.
pub(crate) struct RunToolCatalog {
    state: Weak<RwLock<DaemonState>>,
    run: RunLink,
    cancel: CancelSignal,
    agent: AgentState,
    entries: BTreeMap<String, Entry>,
    mcp: Option<crate::mcp::McpManager>,
    hydrated: Mutex<VecDeque<String>>,
    discovery_enabled: bool,
    delegated_parent: Option<String>,
    peer_sources: Vec<String>,
}
impl RunToolCatalog {
    pub(crate) fn new(
        state: &SharedDaemonState,
        run: RunLink,
        cancel: CancelSignal,
        original: &AgentConfig,
        agent: &AgentState,
        mcp: Option<crate::mcp::McpManager>,
    ) -> Self {
        let registry = super::ToolRegistry::new();
        let mut entries = BTreeMap::new();
        for descriptor in agent.config.tools.iter().flatten() {
            if matches!(descriptor.name.as_str(), SEARCH | DESCRIBE) {
                continue;
            }
            let explicit = original
                .tools
                .iter()
                .flatten()
                .find(|tool| tool.name == descriptor.name);
            let origin = if descriptor.name.starts_with("mcp_") {
                let call = ToolCall {
                    id: "catalog".into(),
                    name: descriptor.name.clone(),
                    args: Default::default(),
                };
                let Some(stamp) = mcp
                    .as_ref()
                    .and_then(|manager| manager.capture_dispatch(agent, &call, &run).ok())
                else {
                    continue;
                };
                Origin::Mcp(stamp)
            } else {
                // Only the host's registered exact native descriptors enter discovery.
                if registry.descriptor(&descriptor.name).as_ref() != Some(descriptor) {
                    continue;
                }
                match explicit {
                    _ if descriptor.name == "clear_main_chat" => Origin::MainChatClear,
                    _ if matches!(
                        descriptor.name.as_str(),
                        "rename_chat"
                            | "compact_chat"
                            | "export_chat"
                            | "manage_mcp"
                            | "show_result"
                            | "list_results"
                    ) =>
                    {
                        Origin::OwnerChatControl
                    }
                    Some(source) => Origin::Explicit(source.clone()),
                    None if descriptor.name == "core_memory"
                        && original.allows_tool("memory_add") =>
                    {
                        Origin::MemoryAdd(
                            original
                                .tools
                                .iter()
                                .flatten()
                                .find(|tool| tool.name == "memory_add")
                                .expect("captured memory_add source")
                                .clone(),
                        )
                    }
                    None if matches!(
                        descriptor.name.as_str(),
                        "list_workspace_agents"
                            | "send_message"
                            | "broadcast_message"
                            | "delegate_to_agent"
                            | "spawn_helper"
                    ) =>
                    {
                        Origin::HostDerived
                    }
                    None => continue,
                }
            };
            let bytes = serde_json::to_vec(descriptor)
                .map(|value| value.len())
                .unwrap_or(usize::MAX);
            entries.insert(
                descriptor.name.clone(),
                Entry {
                    descriptor: descriptor.clone(),
                    bytes,
                    origin,
                },
            );
        }
        Self {
            state: Arc::downgrade(state),
            run,
            cancel,
            agent: agent.clone(),
            entries,
            mcp,
            hydrated: Mutex::new(VecDeque::new()),
            discovery_enabled: original
                .settings
                .as_ref()
                .and_then(|settings| settings.additional.get("toolDiscovery"))
                == Some(&DataValue::Bool(true)),
            delegated_parent: None,
            peer_sources: Vec::new(),
        }
    }

    /// Captures host origin ceilings before this catalog is shared with the run.
    pub(crate) fn with_origins(mut self, parent: Option<String>, peers: Vec<String>) -> Self {
        self.delegated_parent = parent;
        self.peer_sources = peers;
        self
    }

    pub(super) fn derived_control(
        &self,
        name: &str,
        parent: Option<&str>,
        peers: &[String],
    ) -> bool {
        self.discovery_enabled
            && matches!(name, SEARCH | DESCRIBE)
            && self.delegated_parent.as_deref() == parent
            && self.peer_sources.as_slice() == peers
    }

    fn scope_current(&self, state: &DaemonState) -> Result<(), &'static str> {
        if self.cancel.is_cancelled()
            || !state.agents.contains_key(&self.run.agent_id)
            || !state
                .sessions
                .contains(&self.run.agent_id, &self.run.session_id)
            || !state.runs.get(&self.run.run_id).is_some_and(|record| {
                record.agent_id == self.run.agent_id
                    && record.session_id == self.run.session_id
                    && record.status.is_in_flight()
                    && record.stop.is_none()
            })
            || self.peer_sources.len() > anima_core::MAX_AGENT_COMMUNICATION_HOPS
            || self
                .peer_sources
                .iter()
                .any(|id| !state.agents.contains_key(id))
        {
            return Err(REFUSED);
        }
        if let Some(parent) = &self.delegated_parent {
            let config = state.agents.get(parent).ok_or(REFUSED)?.config();
            if config
                .settings
                .as_ref()
                .and_then(|settings| settings.additional.get("workspaceRole"))
                != Some(&DataValue::String("lead".into()))
            {
                return Err(REFUSED);
            }
        }
        Ok(())
    }

    fn entry_current(&self, state: &DaemonState, name: &str) -> Result<(), &'static str> {
        let entry = self.entries.get(name).ok_or(REFUSED)?;
        let canonical = state
            .agents
            .get(&self.run.agent_id)
            .ok_or(REFUSED)?
            .config();
        match &entry.origin {
            Origin::Explicit(source) | Origin::MemoryAdd(source) => {
                if !canonical
                    .tools
                    .iter()
                    .flatten()
                    .any(|current| current == source)
                {
                    return Err(REFUSED);
                }
            }
            Origin::MainChatClear => {
                let run = state.runs.get(&self.run.run_id).ok_or(REFUSED)?;
                let session = state
                    .sessions
                    .get(&self.run.agent_id, &self.run.session_id)
                    .ok_or(REFUSED)?;
                if !self.peer_sources.is_empty()
                    || self.delegated_parent.is_some()
                    || crate::agent_runs::is_helper_config(canonical)
                    || run.agent_id != self.run.agent_id
                    || run.session_id != self.run.session_id
                    || !matches!(
                        run.source,
                        crate::runs::RunSource::Web | crate::runs::RunSource::Api
                    )
                    || run.system_followup.unwrap_or(false)
                    || run.parent_run_id.is_some()
                    || !run.status.is_in_flight()
                    || !session.main_chat
                    || !session.has_main_chat_identity()
                {
                    return Err(REFUSED);
                }
            }
            Origin::OwnerChatControl => {
                if !self.peer_sources.is_empty()
                    || self.delegated_parent.is_some()
                    || crate::agent_runs::chat_controls::validate_owner_chat(state, &self.run)
                        .is_err()
                {
                    return Err(REFUSED);
                }
            }
            Origin::HostDerived => {} // Ordinary host-derived team grants remain unchanged.
            Origin::Mcp(stamp) => {
                if !canonical
                    .tools
                    .iter()
                    .flatten()
                    .any(|current| current == &entry.descriptor)
                {
                    return Err(REFUSED);
                }
                let call = ToolCall {
                    id: "catalog".into(),
                    name: name.into(),
                    args: Default::default(),
                };
                let current = self
                    .mcp
                    .as_ref()
                    .ok_or(REFUSED)?
                    .capture_dispatch(&self.agent, &call, &self.run)
                    .map_err(|_| REFUSED)?;
                if current.connection_id != stamp.connection_id
                    || current.descriptor_digest != stamp.descriptor_digest
                    || current.connection_version != stamp.connection_version
                    || current.wire_generation != stamp.wire_generation
                    || current.grant_epoch != stamp.grant_epoch
                {
                    return Err(REFUSED);
                }
            }
        }
        if let Some(parent) = &self.delegated_parent {
            if !state
                .agents
                .get(parent)
                .ok_or(REFUSED)?
                .config()
                .allows_tool(name)
            {
                return Err(REFUSED);
            }
        }
        for source in &self.peer_sources {
            let config = state.agents.get(source).ok_or(REFUSED)?.config();
            if !matches!(
                name,
                "list_workspace_agents" | "send_message" | "broadcast_message"
            ) && !config.allows_tool(name)
            {
                return Err(REFUSED);
            }
        }
        Ok(())
    }

    async fn scope(&self, agent: &AgentState, run: &RunLink) -> Result<SharedDaemonState, String> {
        if run != &self.run || agent.id != self.run.agent_id {
            return Err(REFUSED.into());
        }
        let state = self.state.upgrade().ok_or(REFUSED)?;
        {
            let guard = state.read().await;
            self.scope_current(&guard).map_err(str::to_string)?;
        }
        Ok(state)
    }

    pub(crate) async fn check(
        &self,
        agent: &AgentState,
        run: &RunLink,
        name: &str,
    ) -> Result<(), String> {
        if run != &self.run || agent.id != self.run.agent_id {
            return Err(REFUSED.into());
        }
        let state = self.state.upgrade().ok_or(REFUSED)?;
        let guard = state.read().await;
        self.scope_current(&guard).map_err(str::to_string)?;
        if matches!(name, SEARCH | DESCRIBE) {
            return self
                .discovery_enabled
                .then_some(())
                .ok_or_else(|| REFUSED.into());
        }
        self.entry_current(&guard, name).map_err(str::to_string)
    }

    fn hydrate(&self, names: &[String]) {
        let mut hydrated = self
            .hydrated
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for name in names {
            hydrated.retain(|current| current != name);
            hydrated.push_back(name.clone());
            while hydrated.len() > MAX_HYDRATED
                || hydrated
                    .iter()
                    .filter_map(|name| self.entries.get(name))
                    .map(|entry| entry.bytes)
                    .sum::<usize>()
                    > MAX_SCHEMA_BYTES
            {
                hydrated.pop_front();
            }
        }
    }

    pub(crate) async fn search(
        &self,
        agent: &AgentState,
        run: &RunLink,
        query: &str,
        limit: usize,
    ) -> Result<String, String> {
        if query.chars().count() > 256 || query.trim().is_empty() || !(1..=5).contains(&limit) {
            return Err(
                "search_tools requires a query of 1 to 256 characters and limit from 1 to 5".into(),
            );
        }
        self.scope(agent, run).await?;
        let query = query.trim().to_lowercase();
        let words: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let mut ranked = self
            .entries
            .values()
            .filter_map(|entry| {
                let name = entry.descriptor.name.to_lowercase();
                let description = entry.descriptor.description.to_lowercase();
                let name_words: Vec<_> = name.split(|c: char| !c.is_alphanumeric()).collect();
                let score = if name == query {
                    1_000_000
                } else {
                    words
                        .iter()
                        .map(|word| {
                            if name_words.contains(word) {
                                10_000
                            } else if name.contains(*word) {
                                1_000
                            } else if description.contains(*word) {
                                1
                            } else {
                                0
                            }
                        })
                        .sum()
                };
                (score > 0 && entry.bytes <= MAX_SCHEMA_BYTES).then_some((score, entry))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(a, left), (b, right)| {
            b.cmp(a)
                .then_with(|| left.descriptor.name.cmp(&right.descriptor.name))
        });
        let mut summaries = Vec::new();
        let mut names = Vec::new();
        let mut schema_bytes = 0;
        for (_, entry) in ranked {
            if names.len() == limit {
                break;
            }
            if schema_bytes + entry.bytes > MAX_SCHEMA_BYTES
                || self
                    .check(agent, run, &entry.descriptor.name)
                    .await
                    .is_err()
            {
                continue;
            }
            summaries.push(json!({"name":entry.descriptor.name,"description":clip(&entry.descriptor.description,384),"hydrated":true}));
            let result = json!({"tools":summaries,"schemaVisibility":"Exact schemas are advertised on the next model request; execution still requires current grants and approval."}).to_string();
            if result.len() > MAX_RESULT_BYTES {
                summaries.pop();
                continue;
            }
            names.push(entry.descriptor.name.clone());
            schema_bytes += entry.bytes;
        }
        // Cancellation/deletion during the scan cannot publish a partial catalog.
        self.scope(agent, run).await?;
        self.hydrate(&names);
        Ok(json!({"tools":summaries,"schemaVisibility":"Exact schemas are advertised on the next model request; execution still requires current grants and approval."}).to_string())
    }

    pub(crate) async fn describe(
        &self,
        agent: &AgentState,
        run: &RunLink,
        name: &str,
    ) -> Result<String, String> {
        if name.is_empty() || name.len() > 256 {
            return Err("describe_tool requires an exact name of 1 to 256 bytes".into());
        }
        self.check(agent, run, name).await?;
        let entry = self.entries.get(name).ok_or(REFUSED)?;
        if entry.bytes > MAX_SCHEMA_BYTES {
            return Err("Tool schema exceeds the run hydration budget".into());
        }
        let result = json!({"name":entry.descriptor.name,"description":clip(&entry.descriptor.description,384),"hydrated":true,"schemaVisibility":"The exact descriptor is advertised on the next model request. Current grants and ordinary approval remain required."}).to_string();
        if result.len() > MAX_RESULT_BYTES {
            return Err("Tool summary exceeds the result budget".into());
        }
        self.hydrate(&[name.into()]);
        Ok(result)
    }
}
impl ModelToolSelector for RunToolCatalog {
    fn visible_tool_names(&self) -> BTreeSet<String> {
        // Never hold the LRU mutex while reading daemon or MCP authority.
        let mut names: BTreeSet<String> = self
            .hydrated
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .cloned()
            .collect();
        names.extend(
            ESSENTIALS
                .iter()
                .filter(|name| self.entries.contains_key(**name))
                .map(|name| (*name).into()),
        );
        if self.discovery_enabled {
            names.extend([SEARCH.into(), DESCRIBE.into()]);
        }
        let Some(state) = self.state.upgrade() else {
            return BTreeSet::new();
        };
        // Provider request construction is synchronous: contention hides schemas
        // instead of blocking the daemon or advertising unchecked authority.
        let Ok(guard) = state.try_read() else {
            return BTreeSet::new();
        };
        if self.scope_current(&guard).is_err() {
            return BTreeSet::new();
        }
        // A captured control for this exact owner/main-chat run, revalidated
        // independently of ordinary catalog grants on every model request.
        if self.entry_current(&guard, "clear_main_chat").is_ok() {
            names.insert("clear_main_chat".into());
        }
        for name in [
            "rename_chat",
            "compact_chat",
            "export_chat",
            "manage_mcp",
            "show_result",
            "list_results",
        ] {
            if self.entry_current(&guard, name).is_ok() {
                names.insert(name.into());
            }
        }
        names.retain(|name| {
            if matches!(name.as_str(), SEARCH | DESCRIBE) {
                self.discovery_enabled
            } else {
                self.entry_current(&guard, name).is_ok()
            }
        });
        names
    }
}
fn clip(value: &str, max: usize) -> &str {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(super) fn register(registry: &mut super::ToolRegistry) {
    let mut query = super::non_blank_string_parameter(
        "Literal tool name, name words or description terms, at most 256 characters",
    );
    if let DataValue::Object(schema) = &mut query {
        schema.insert("maxLength".into(), DataValue::Number(256.0));
    }
    let mut name = super::non_blank_string_parameter(
        "Exact tool name returned by search_tools, at most 256 bytes",
    );
    if let DataValue::Object(schema) = &mut name {
        schema.insert("maxLength".into(), DataValue::Number(256.0));
    }
    registry.register(super::tool_descriptor(SEARCH,"Find permitted tools by literal lexical search. Returns at most five compact summaries and advertises their exact schemas on the next request. This grants no execution permission.",super::object_parameters(vec![super::required_parameter("query",query),super::optional_parameter("limit",super::bounded_integer_parameter("Maximum summaries, default 3",1,5))])),execute);
    registry.register(super::tool_descriptor(DESCRIBE,"Advertise one exact permitted tool schema on the next model request and return its compact description. Current grants and ordinary approval remain required.",super::object_parameters(vec![super::required_parameter("name",name)])),execute);
}
fn execute(
    context: super::ToolExecutionContext,
    agent: AgentState,
    user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let (Some(catalog), Some(run)) = (&context.tool_catalog, &context.run_link) else {
            return TaskResult::error("Run tool discovery context is unavailable", 0);
        };
        if user.agent_id != agent.id {
            return TaskResult::error(REFUSED, 0);
        }
        let result = match call.name.as_str() {
            SEARCH => {
                if call
                    .args
                    .keys()
                    .any(|key| !matches!(key.as_str(), "query" | "limit"))
                {
                    return TaskResult::error("Unknown search_tools argument", 0);
                }
                let query = match call.args.get("query") {
                    Some(DataValue::String(value)) => value,
                    _ => return TaskResult::error("search_tools query must be a string", 0),
                };
                let limit = match call.args.get("limit") {
                    None => 3,
                    Some(DataValue::Number(value))
                        if value.is_finite()
                            && value.fract() == 0.0
                            && (1.0..=5.0).contains(value) =>
                    {
                        *value as usize
                    }
                    _ => {
                        return TaskResult::error(
                            "search_tools limit must be an integer from 1 to 5",
                            0,
                        )
                    }
                };
                catalog.search(&agent, run, query, limit).await
            }
            DESCRIBE => {
                if call.args.keys().any(|key| key != "name") {
                    return TaskResult::error("Unknown describe_tool argument", 0);
                }
                let name = match call.args.get("name") {
                    Some(DataValue::String(value)) => value,
                    _ => return TaskResult::error("describe_tool name must be a string", 0),
                };
                catalog.describe(&agent, run, name).await
            }
            _ => Err(REFUSED.into()),
        };
        match result {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Default::default()
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
