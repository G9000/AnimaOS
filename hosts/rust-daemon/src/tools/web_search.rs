//! Anonymous, explicitly granted search. Excerpts remain untrusted data.
//! Labelled Exa fields follow the pinned public Hermes adapter; see HERMES-NOTICE.
use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anima_core::{
    external_tools, AgentState, CancelSignal, Content, DataValue, Message, MessageRole, TaskResult,
    TaskStatus, ToolCall,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{oneshot, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::ToolExecutionContext;
use crate::mcp::transport::{
    DefaultMcpTransportFactory, McpCall, McpTransportFactory, McpTransportSession,
    TransportControl, ValidatedMcpEndpoint, ValidatedMcpLaunch,
};

pub(crate) const ENDPOINT: &str = "https://mcp.exa.ai/mcp";
#[cfg(test)]
#[path = "web_search_tests.rs"]
pub(crate) mod tests;
const REMOTE_TOOL: &str = "web_search_exa";
const MAX_ENVELOPE: usize = 16 * 1024;
const MAX_SOURCES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebSource {
    pub(crate) title: String,
    pub(crate) url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchRow {
    title: String,
    url: String,
    excerpt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchEnvelope {
    version: u8,
    provider: String,
    mode: String,
    untrusted: bool,
    results: Vec<SearchRow>,
    truncated: bool,
}

fn public_url(raw: &str) -> bool {
    let absolute = raw
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || raw
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"));
    if !absolute
        || raw.len() > 2048
        || raw
            .chars()
            .any(|c| c <= '\u{20}' || ('\u{7f}'..='\u{9f}').contains(&c))
        || raw.trim() != raw
    {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    if name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".internal")
    {
        return false;
    }
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(ip) = literal.parse::<IpAddr>() else {
        return !name.is_empty();
    };
    fn safe_ip(ip: IpAddr) -> bool {
        if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
            return false;
        }
        match ip {
            IpAddr::V4(v4) => {
                let o = v4.octets();
                !v4.is_private()
                    && !v4.is_link_local()
                    && !v4.is_broadcast()
                    && o[0] != 0
                    && o[0] < 224
                    && !(o[0] == 100 && (64..=127).contains(&o[1]))
                    && !(o[0] == 198 && (18..=19).contains(&o[1]))
                    && !(o[0] == 192 && o[1] == 0 && matches!(o[2], 0 | 2))
                    && !(o[0] == 198 && o[1] == 51 && o[2] == 100)
                    && !(o[0] == 203 && o[1] == 0 && o[2] == 113)
            }
            IpAddr::V6(v6) => {
                let segments = v6.segments();
                (0x2000..=0x3fff).contains(&segments[0])
                    && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
            }
        }
    }
    safe_ip(ip)
}

fn valid_source(source: &WebSource) -> bool {
    !source.title.trim().is_empty() && source.title.len() <= 256 && public_url(&source.url)
}

pub(crate) fn sources_from_envelope(text: &str) -> Option<Vec<WebSource>> {
    if text.len() > MAX_ENVELOPE {
        return None;
    }
    let envelope: SearchEnvelope = serde_json::from_str(text).ok()?;
    if envelope.version != 1
        || envelope.provider != "exa"
        || envelope.mode != "anonymous"
        || !envelope.untrusted
        || envelope.results.len() > 5
    {
        return None;
    }
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    for row in envelope.results {
        let source = WebSource {
            title: row.title,
            url: row.url,
        };
        if !valid_source(&source) || row.excerpt.len() > 1200 {
            return None;
        }
        let key = reqwest::Url::parse(&source.url).ok()?.to_string();
        if !seen.insert(key) {
            return None;
        }
        sources.push(source);
    }
    (serde_json::to_vec(&sources).ok()?.len() <= MAX_SOURCES).then_some(sources)
}

fn source_value(sources: &[WebSource]) -> DataValue {
    DataValue::Array(
        sources
            .iter()
            .map(|source| {
                DataValue::Object(BTreeMap::from([
                    ("title".into(), DataValue::String(source.title.clone())),
                    ("url".into(), DataValue::String(source.url.clone())),
                ]))
            })
            .collect(),
    )
}

/// Reserved metadata is reconstructed, never trusted from a tool handler.
pub(super) fn project_result(name: &str, mut result: TaskResult<Content>) -> TaskResult<Content> {
    if let Some(content) = &mut result.data {
        if let Some(metadata) = &mut content.metadata {
            metadata.remove("webSources");
        }
        if name == "web_search" && result.status == TaskStatus::Success && result.error.is_none() {
            if let Some(sources) =
                sources_from_envelope(&content.text).filter(|sources| !sources.is_empty())
            {
                content
                    .metadata
                    .get_or_insert_with(BTreeMap::new)
                    .insert("webSources".into(), source_value(&sources));
            }
        }
    }
    result
}

pub(crate) fn history_sources(message: &Message) -> Option<Value> {
    if message.role != MessageRole::Tool {
        return None;
    }
    let metadata = message.content.metadata.as_ref()?;
    if metadata.get("toolStatus") != Some(&DataValue::String("success".into())) {
        return None;
    }
    if let Some(DataValue::Object(result)) = metadata.get("taskResult") {
        if result.get("status") != Some(&DataValue::String("success".into())) {
            return None;
        }
    }
    let sources = sources_from_envelope(&message.content.text)?;
    if sources.is_empty() || metadata.get("webSources") != Some(&source_value(&sources)) {
        return None;
    }
    serde_json::to_value(sources).ok()
}

fn trim_bytes(text: &str, max: usize) -> (String, bool) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), end < text.len())
}

fn labelled_rows(text: &str, shortened: bool) -> (Vec<(String, String, String)>, bool) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let blocks: Vec<_> = normalized.split("\n---\n").collect();
    let mut rows = Vec::new();
    let mut omitted = false;
    for (index, block) in blocks.iter().enumerate() {
        if shortened && index + 1 == blocks.len() {
            omitted = true;
            break;
        }
        if block.trim().is_empty() {
            continue;
        }
        let (mut title, mut url) = (None, None);
        let mut highlights = Vec::new();
        let mut in_highlights = false;
        let mut ambiguous = false;
        for line in block.lines() {
            let line = line.trim();
            if in_highlights {
                // A result excerpt cannot introduce another source record.
                highlights.push(line);
            } else if let Some(value) = line.strip_prefix("Title:") {
                if title.replace(value.trim().to_string()).is_some() {
                    ambiguous = true;
                }
            } else if let Some(value) = line.strip_prefix("URL:") {
                if url.replace(value.trim().to_string()).is_some() {
                    ambiguous = true;
                }
            } else if let Some(value) = line
                .strip_prefix("Highlights:")
                .or_else(|| line.strip_prefix("Text:"))
            {
                in_highlights = true;
                highlights.push(value.trim());
            }
        }
        match (title, url) {
            (Some(title), Some(url)) if !ambiguous => {
                rows.push((title, url, highlights.join("\n")))
            }
            _ => omitted = true,
        }
    }
    (rows, omitted)
}

fn normalize(result: external_tools::ExternalToolResult, limit: usize) -> Result<String, String> {
    if result.is_error {
        return Err(
            "Anonymous Exa search failed or was rate-limited; no sources were returned.".into(),
        );
    }
    let mut truncated = result.shortened || result.omitted_blocks > 0;
    let structured = result
        .structured_data
        .as_ref()
        .map(crate::routes::data_value_to_json);
    let mut recognized_empty = false;
    let mut raw_rows = Vec::new();
    if let Some(Value::Array(rows)) = structured.as_ref().and_then(|value| value.get("results")) {
        recognized_empty = rows.is_empty();
        for row in rows {
            if let (Some(title), Some(url)) = (row["title"].as_str(), row["url"].as_str()) {
                let excerpt = row["text"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        row["highlights"].as_array().map(|parts| {
                            parts
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                    })
                    .unwrap_or_default();
                raw_rows.push((title.to_string(), url.to_string(), excerpt));
            } else {
                truncated = true;
            }
        }
        // Structured data is atomic JSON; a shortened upstream structured
        // payload cannot establish complete URL provenance.
        if result.shortened {
            raw_rows.clear();
        }
    } else {
        for (index, block) in result.text_blocks.iter().enumerate() {
            let empty = block.trim().to_ascii_lowercase();
            if matches!(
                empty.as_str(),
                "no results found"
                    | "no results found."
                    | "no search results found"
                    | "no search results found."
            ) {
                recognized_empty = true;
                continue;
            }
            let (rows, omitted) = labelled_rows(
                block,
                result.shortened && index + 1 == result.text_blocks.len(),
            );
            raw_rows.extend(rows);
            truncated |= omitted;
        }
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for (title, url, excerpt) in raw_rows {
        let (title, title_cut) = trim_bytes(title.trim(), 256);
        let (excerpt, excerpt_cut) = trim_bytes(&excerpt, 1200);
        truncated |= title_cut || excerpt_cut;
        if !valid_source(&WebSource {
            title: title.clone(),
            url: url.clone(),
        }) {
            truncated = true;
            continue;
        }
        // Compare validated canonical URL identity while preserving the first
        // complete provider spelling in the model result and public sources.
        let key = reqwest::Url::parse(&url)
            .map_err(|_| "Invalid normalized search URL")?
            .to_string();
        if !seen.insert(key) {
            continue;
        }
        if rows.len() >= limit {
            truncated = true;
            continue;
        }
        rows.push(SearchRow {
            title,
            url,
            excerpt,
        });
    }
    if rows.is_empty() && !recognized_empty {
        return Err("Anonymous Exa search returned an unsupported or malformed result; no sources were returned.".into());
    }
    let mut envelope = SearchEnvelope {
        version: 1,
        provider: "exa".into(),
        mode: "anonymous".into(),
        untrusted: true,
        results: rows,
        truncated,
    };
    loop {
        let text =
            serde_json::to_string(&envelope).map_err(|_| "Could not normalize search results")?;
        let sources: Vec<_> = envelope
            .results
            .iter()
            .map(|row| WebSource {
                title: row.title.clone(),
                url: row.url.clone(),
            })
            .collect();
        if text.len() <= MAX_ENVELOPE
            && serde_json::to_vec(&sources)
                .map_err(|_| "Could not normalize search sources")?
                .len()
                <= MAX_SOURCES
        {
            return Ok(text);
        }
        envelope.results.pop();
        envelope.truncated = true;
    }
}

fn arguments(call: &ToolCall) -> Result<Value, String> {
    if call
        .args
        .keys()
        .any(|key| !matches!(key.as_str(), "query" | "num_results"))
    {
        return Err("web_search accepts only query and num_results".into());
    }
    let query = match call.args.get("query") {
        Some(DataValue::String(query)) if !query.trim().is_empty() && query.trim().len() <= 512 => {
            query.trim()
        }
        _ => return Err("web_search query must contain 1 through 512 UTF-8 bytes".into()),
    };
    let count = match call.args.get("num_results") {
        None => 3,
        Some(DataValue::Number(value))
            if value.is_finite() && value.fract() == 0.0 && (1.0..=5.0).contains(value) =>
        {
            *value as usize
        }
        _ => return Err("web_search num_results must be an integer from 1 through 5".into()),
    };
    Ok(json!({"query":query,"numResults":count}))
}

/// Only this fixed adapter owns these workers. A caller can stop waiting, but
/// the physical permit remains with connect/close until the session joins.
pub(crate) struct SearchExecutor {
    factory: Arc<dyn McpTransportFactory>,
    permits: Arc<Semaphore>,
    timeout: Duration,
    workers: Mutex<Vec<JoinHandle<()>>>,
    unknown_close: Mutex<Vec<Arc<dyn McpTransportSession>>>,
    #[cfg(test)]
    phases: Mutex<Vec<&'static str>>,
}

impl SearchExecutor {
    fn new(factory: Arc<dyn McpTransportFactory>, capacity: usize, timeout: Duration) -> Arc<Self> {
        Arc::new(Self {
            factory,
            permits: Arc::new(Semaphore::new(capacity)),
            timeout,
            workers: Mutex::new(Vec::new()),
            unknown_close: Mutex::new(Vec::new()),
            #[cfg(test)]
            phases: Mutex::new(Vec::new()),
        })
    }
    #[cfg(test)]
    pub(crate) fn for_test(
        factory: Arc<dyn McpTransportFactory>,
        capacity: usize,
        timeout: Duration,
    ) -> Arc<Self> {
        Self::new(factory, capacity, timeout)
    }
    #[cfg(test)]
    pub(crate) async fn join(&self) {
        // Keep handles in their owning executor until finished. If an
        // evidence deadline stops waiting, physical cleanup stays owned.
        loop {
            if self
                .workers
                .lock()
                .unwrap()
                .iter()
                .all(JoinHandle::is_finished)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        let tasks = std::mem::take(&mut *self.workers.lock().unwrap());
        for task in tasks {
            task.await.expect("search owner task joins");
        }
    }
    #[cfg(test)]
    pub(crate) fn evidence(&self) -> Value {
        let phases = self.phases.lock().unwrap().clone();
        json!({"connectAttempts":phases.iter().filter(|phase|**phase=="connect").count(),"logicalSearchCalls":phases.iter().filter(|phase|**phase=="call").count(),"phases":phases,"availablePermits":self.permits.available_permits(),"unknownClose":self.unknown_close.lock().unwrap().len(),"ownedWorkers":self.workers.lock().unwrap().len()})
    }
    async fn search(self: &Arc<Self>, args: Value, cancel: CancelSignal) -> Result<String, String> {
        if cancel.is_cancelled() {
            return Err("Web search cancelled before connecting".into());
        }
        let deadline = Instant::now() + self.timeout;
        let permit =
            self.permits.clone().try_acquire_owned().map_err(|_| {
                "Web search is busy; previous searches or cleanup still own capacity"
            })?;
        let token = CancellationToken::new();
        let _drop_cancel = token.clone().drop_guard();
        let control = TransportControl::new(deadline, token.clone());
        let (send, receive) = oneshot::channel();
        let executor = self.clone();
        let run_cancel = cancel.clone();
        let worker = tokio::spawn(async move {
            if run_cancel.is_cancelled() {
                control.cancel.cancel();
            }
            if let Err(error) = check(&control) {
                let _ = send.send(Err(error));
                return;
            }
            #[cfg(test)]
            executor.phases.lock().unwrap().push("connect");
            let connected = executor
                .factory
                .connect(
                    ValidatedMcpLaunch::Http(ValidatedMcpEndpoint(ENDPOINT.into())),
                    control.clone(),
                )
                .await;
            let result = match connected {
                Err(error) => Err(format!("Anonymous Exa search: {error}")),
                Ok(session) => {
                    let result = async {
                        check(&control)?;
                        #[cfg(test)]
                        executor.phases.lock().unwrap().push("discover");
                        let discovery = session
                            .discover_native_search(control.clone())
                            .await
                            .map_err(|e| format!("Anonymous Exa search: {e}"))?;
                        check(&control)?;
                        let mut matching = discovery
                            .tools
                            .into_iter()
                            .filter(|tool| tool.remote_name == REMOTE_TOOL);
                        let descriptor = matching
                            .next()
                            .ok_or("Anonymous Exa search is unavailable: expected tool missing")?;
                        if matching.next().is_some() {
                            return Err(
                                "Anonymous Exa search is unavailable: ambiguous tool discovery"
                                    .into(),
                            );
                        }
                        let properties = &descriptor.input_schema["properties"];
                        if properties["query"]["type"] != "string"
                            || !matches!(
                                properties["numResults"]["type"].as_str(),
                                Some("number" | "integer")
                            )
                        {
                            return Err("Anonymous Exa search schema is unsupported".into());
                        }
                        let mut remote_args = args.clone();
                        if descriptor.input_schema["required"]
                            .as_array()
                            .is_some_and(|required| required.iter().any(|name| name == "objective"))
                        {
                            if properties["objective"]["type"] != "string" {
                                return Err(
                                    "Anonymous Exa search objective schema is unsupported".into()
                                );
                            }
                            // The remote server requires a search objective; the
                            // native tool derives it from its already trimmed query.
                            remote_args["objective"] = args["query"].clone();
                        }
                        external_tools::validate_external_arguments(&descriptor, &remote_args)
                            .map_err(|_| "Anonymous Exa search schema is unsupported")?;
                        check(&control)?;
                        #[cfg(test)]
                        executor.phases.lock().unwrap().push("call");
                        let response = session
                            .call_once(
                                McpCall {
                                    remote_name: REMOTE_TOOL.into(),
                                    arguments: remote_args,
                                    descriptor,
                                },
                                control.clone(),
                            )
                            .await
                            .map_err(|e| format!("Anonymous Exa search: {e}"))?;
                        check(&control)?;
                        #[cfg(test)]
                        executor.phases.lock().unwrap().push("normalize");
                        let normalized =
                            normalize(response, args["numResults"].as_u64().unwrap_or(3) as usize)?;
                        check(&control)?;
                        Ok(normalized)
                    }
                    .await;
                    #[cfg(test)]
                    executor.phases.lock().unwrap().push("close");
                    if session.close().await.is_err() {
                        // No physical-close proof: keep bounded ownership and
                        // refuse reuse rather than claiming cleanup succeeded.
                        executor.unknown_close.lock().unwrap().push(session);
                        permit.forget();
                        let _ = send.send(Err(
                            "Anonymous Exa search cleanup is unknown; capacity remains held".into(),
                        ));
                        return;
                    }
                    result
                }
            };
            drop(permit);
            let _ = send.send(result);
        });
        {
            let mut workers = self.workers.lock().unwrap();
            workers.retain(|worker| !worker.is_finished());
            workers.push(worker);
        }
        tokio::select! { biased;
            _=cancel.cancelled()=>Err("Web search cancelled; transport cleanup retains ownership until joined".into()),
            _=tokio::time::sleep_until(deadline)=>Err("Web search deadline exceeded; transport cleanup retains ownership until joined".into()),
            result=receive=>result.unwrap_or_else(|_| Err("Anonymous Exa search worker stopped".into())),
        }
    }
}

fn check(control: &TransportControl) -> Result<(), String> {
    if control.cancel.is_cancelled() {
        Err("Web search cancelled".into())
    } else if Instant::now() >= control.deadline {
        Err("Web search deadline exceeded".into())
    } else {
        Ok(())
    }
}

fn default_executor() -> Arc<SearchExecutor> {
    static EXECUTOR: OnceLock<Arc<SearchExecutor>> = OnceLock::new();
    EXECUTOR
        .get_or_init(|| {
            SearchExecutor::new(
                Arc::new(DefaultMcpTransportFactory::default()),
                4,
                Duration::from_secs(30),
            )
        })
        .clone()
}

#[cfg(test)]
pub(crate) fn public_executor() -> Arc<SearchExecutor> {
    default_executor()
}

pub(super) fn execute(
    context: ToolExecutionContext,
    _: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let args = match arguments(&call) {
            Ok(args) => args,
            Err(error) => return TaskResult::error(error, 0),
        };
        let executor = default_executor();
        #[cfg(test)]
        let executor = context.web_search.clone().unwrap_or(executor);
        match executor
            .search(args, context.cancel.unwrap_or_default())
            .await
        {
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
