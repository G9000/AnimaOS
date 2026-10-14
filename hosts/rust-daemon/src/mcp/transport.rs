//! Bounded MCP adaptation of rmcp 3.5.1. The SDK owns MCP mechanics.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};
use std::time::Duration;

use anima_core::{
    external_tools::{self, ExternalToolDescriptor, ExternalToolResult},
    DataValue,
};
use axum::http::{HeaderName, HeaderValue};
use futures::{
    future::BoxFuture,
    stream::{self, BoxStream},
    StreamExt,
};
use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, ClientConfig, ClientJsonRpcMessage, ClientRequest,
        ContentBlock, PaginatedRequestParams, ProtocolVersion, ServerJsonRpcMessage, ServerResult,
    },
    service::{
        serve_client_with_lifecycle_and_ct, ClientLifecycleMode, PeerRequestOptions,
        RunningService, ServiceError,
    },
    transport::{
        common::client_side_sse::SseRetryPolicy,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpClientTransportConfig, StreamableHttpError,
            StreamableHttpPostResponse,
        },
        StreamableHttpClientTransport, Transport,
    },
    RoleClient,
};
use serde_json::Value;
use sse_stream::{Sse, SseStream};
use tokio::sync::{Mutex, Notify};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(super) const PROTOCOL: &str = "2025-11-25";
pub(super) const RAW_TOTAL: usize = 256 * 1024;
pub(super) const RAW_EVENT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum McpTransportError {
    Unavailable,
    UnsupportedProtocol,
    WireLimit,
    Malformed,
    SchemaRejected,
    RemoteError,
    UnsupportedResult,
    Cancelled,
    TimedOut,
    OutcomeUnknown,
}
impl std::fmt::Display for McpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "External connection is unavailable",
            Self::UnsupportedProtocol => "External protocol version is unsupported",
            Self::WireLimit => "External response exceeds its wire budget",
            Self::Malformed => "External response is malformed",
            Self::SchemaRejected => "External tool schema or data is invalid",
            Self::RemoteError => "External server returned an error",
            Self::UnsupportedResult => "External result type is unsupported",
            Self::Cancelled => "External operation was cancelled",
            Self::TimedOut => "External operation exceeded its deadline",
            Self::OutcomeUnknown => "External call outcome is unknown; effects may have occurred",
        })
    }
}
impl std::error::Error for McpTransportError {}

#[derive(Debug)]
pub(crate) struct WireBudget(AtomicUsize);
impl WireBudget {
    pub(super) fn consume(&self, n: usize) -> Result<(), McpTransportError> {
        self.0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                old.checked_add(n).filter(|new| *new <= RAW_TOTAL)
            })
            .map(|_| ())
            .map_err(|_| McpTransportError::WireLimit)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct TransportControl {
    pub(crate) deadline: Instant,
    pub(crate) cancel: CancellationToken,
    pub(crate) wire_budget: Arc<WireBudget>,
    pub(super) dispatched: Arc<AtomicBool>,
}
impl TransportControl {
    pub(crate) fn new(deadline: Instant, cancel: CancellationToken) -> Self {
        Self {
            deadline,
            cancel,
            wire_budget: Arc::new(WireBudget(AtomicUsize::new(0))),
            dispatched: Arc::new(AtomicBool::new(false)),
        }
    }
    pub(super) fn check(&self) -> Result<(), McpTransportError> {
        if self.cancel.is_cancelled() {
            Err(McpTransportError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(McpTransportError::TimedOut)
        } else {
            Ok(())
        }
    }
    pub(super) async fn run<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, McpTransportError>>,
    ) -> Result<T, McpTransportError> {
        self.check()?;
        tokio::select! {biased;
            _=self.cancel.cancelled()=>Err(McpTransportError::Cancelled),
            _=tokio::time::sleep_until(self.deadline)=>Err(McpTransportError::TimedOut),
            value=future=>value,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct ValidatedMcpEndpoint(pub(crate) String);
#[derive(Clone, Debug)]
pub(crate) enum ValidatedMcpLaunch {
    Http(ValidatedMcpEndpoint),
    Stdio {
        spec: super::McpStdioSpec,
        connection_id: String,
    },
}
#[derive(Clone, Debug)]
pub(crate) struct McpCall {
    pub(crate) remote_name: String,
    pub(crate) arguments: Value,
    pub(crate) descriptor: ExternalToolDescriptor,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct McpDiscovery {
    pub(crate) protocol_version: String,
    pub(crate) tools: Vec<ExternalToolDescriptor>,
}
#[derive(Debug)]
pub(crate) struct PhysicalClose(());
impl PhysicalClose {
    pub(super) fn joined() -> Self {
        Self(())
    }
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self(())
    }
}
pub(crate) trait McpTransportFactory: Send + Sync {
    fn connect(
        &self,
        launch: ValidatedMcpLaunch,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>>;
}
pub(crate) trait McpTransportSession: Send + Sync {
    fn discover(
        &self,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>>;
    fn call_once(
        &self,
        call: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>>;
    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>>;
}

pub(super) fn lock<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
#[derive(Default)]
pub(super) struct Activity {
    count: AtomicUsize,
    changed: Notify,
}
struct Active(Arc<Activity>);
impl Active {
    fn new(activity: &Arc<Activity>) -> Self {
        activity.count.fetch_add(1, Ordering::SeqCst);
        Self(activity.clone())
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}
impl Activity {
    pub(super) async fn idle(&self) {
        loop {
            let notice = self.changed.notified();
            tokio::pin!(notice);
            notice.as_mut().enable();
            if self.count.load(Ordering::SeqCst) == 0 {
                return;
            }
            notice.await;
        }
    }
}
// SDK startup takes ownership of the transport. Dropping that startup future
// cancels WorkerTransport without joining it, so retain an owned close path.
pub(super) struct JoinedTransport<T: Transport<RoleClient> + 'static> {
    inner: Option<T>,
    retirement: Option<Active>,
}
impl<T: Transport<RoleClient> + 'static> JoinedTransport<T> {
    pub(super) fn new(inner: T, workers: &Arc<Activity>) -> Self {
        Self {
            inner: Some(inner),
            retirement: Some(Active::new(workers)),
        }
    }
}
impl<T: Transport<RoleClient> + 'static> Transport<RoleClient> for JoinedTransport<T> {
    type Error = T::Error;
    fn send(
        &mut self,
        message: ClientJsonRpcMessage,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send + 'static {
        let active = Active::new(&self.retirement.as_ref().expect("transport is live").0);
        let send = self
            .inner
            .as_mut()
            .expect("transport owned until close")
            .send(message);
        async move {
            let _active = active;
            send.await
        }
    }
    async fn receive(&mut self) -> Option<ServerJsonRpcMessage> {
        match self.inner.as_mut() {
            Some(inner) => inner.receive().await,
            None => None,
        }
    }
    async fn close(&mut self) -> Result<(), Self::Error> {
        let result = match self.inner.as_mut() {
            Some(inner) => inner.close().await,
            None => Ok(()),
        };
        self.inner.take();
        self.retirement.take();
        result
    }
}
impl<T: Transport<RoleClient> + 'static> Drop for JoinedTransport<T> {
    fn drop(&mut self) {
        if let Some(mut inner) = self.inner.take() {
            let retirement = self.retirement.take();
            tokio::spawn(async move {
                let _ = inner.close().await;
                drop(retirement);
            });
        }
    }
}
#[derive(Debug)]
struct NoRetry;
impl SseRetryPolicy for NoRetry {
    fn retry(&self, _: usize) -> Option<Duration> {
        None
    }
}

// Count the complete raw event, including comments and CR/LF across chunks.
#[derive(Default)]
struct EventLimit {
    size: usize,
    line: usize,
    cr: bool,
}
impl EventLimit {
    fn observe(&mut self, chunk: &[u8]) -> Result<(), McpTransportError> {
        for &byte in chunk {
            self.size = self.size.saturating_add(1);
            if self.size > RAW_EVENT {
                return Err(McpTransportError::WireLimit);
            }
            if self.cr {
                self.cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if byte == b'\n' || byte == b'\r' {
                if self.line == 0 {
                    self.size = 0;
                }
                self.line = 0;
                self.cr = byte == b'\r';
            } else {
                self.line += 1;
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
struct HttpBackend {
    http: reqwest::Client,
    control: Arc<StdMutex<TransportControl>>,
    error: Arc<StdMutex<Option<McpTransportError>>>,
    activity: Arc<Activity>,
    workers: Arc<Activity>,
    stop: CancellationToken,
}
impl HttpBackend {
    fn new(control: TransportControl) -> Result<Self, McpTransportError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| McpTransportError::Unavailable)?;
        Ok(Self {
            http,
            control: Arc::new(StdMutex::new(control)),
            error: Arc::new(StdMutex::new(None)),
            activity: Arc::new(Activity::default()),
            workers: Arc::new(Activity::default()),
            stop: CancellationToken::new(),
        })
    }
    fn failure(&self, error: McpTransportError) -> StreamableHttpError<McpTransportError> {
        *lock(&self.error) = Some(error);
        StreamableHttpError::Client(error)
    }
    fn begin(&self, control: TransportControl) {
        *lock(&self.control) = control;
        *lock(&self.error) = None;
    }
    async fn post(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<McpTransportError>> {
        if auth.is_some() || self.stop.is_cancelled() {
            return Err(self.failure(McpTransportError::Unavailable));
        }
        let control = lock(&self.control).clone();
        control.check().map_err(|e| self.failure(e))?;
        let active = Active::new(&self.activity);
        let is_call = matches!(&message,ClientJsonRpcMessage::Request(r) if matches!(r.request,ClientRequest::CallToolRequest(_)));
        let initialize = matches!(&message,ClientJsonRpcMessage::Request(r) if matches!(r.request,ClientRequest::InitializeRequest(_)));
        let mut request = self
            .http
            .post(uri.as_ref())
            .header("accept", "application/json, text/event-stream")
            .json(&message);
        if let Some(id) = session_id {
            request = request.header("mcp-session-id", id.as_ref());
        }
        for (name, value) in headers {
            if name.as_str() != "mcp-protocol-version" {
                return Err(self.failure(McpTransportError::Malformed));
            }
            request = request.header(name, value);
        }
        if is_call {
            control.dispatched.store(true, Ordering::SeqCst);
        }
        let response = tokio::select! {biased;
            _=self.stop.cancelled()=>return Err(self.failure(McpTransportError::Unavailable)),
            value=control.run(async {request.send().await.map_err(|_|McpTransportError::Unavailable)})=>value.map_err(|e|self.failure(e))?,
        };
        if !response.status().is_success() {
            return Err(self.failure(McpTransportError::Unavailable));
        }
        let id = response
            .headers()
            .get("mcp-session-id")
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .filter(|value| {
                        !value.is_empty()
                            && value.len() <= 256
                            && value.bytes().all(|b| b.is_ascii_graphic())
                    })
                    .map(str::to_owned)
                    .ok_or(McpTransportError::Malformed)
            })
            .transpose()
            .map_err(|e| self.failure(e))?;
        if response
            .headers()
            .iter()
            .any(|(name, value)| name.as_str().len() > 256 || value.as_bytes().len() > 4096)
            || response.headers().len() > 64
        {
            return Err(self.failure(McpTransportError::WireLimit));
        }
        if response
            .content_length()
            .is_some_and(|n| n > RAW_TOTAL as u64)
        {
            return Err(self.failure(McpTransportError::WireLimit));
        }
        let accepted = response.status() == reqwest::StatusCode::ACCEPTED
            || response.status() == reqwest::StatusCode::NO_CONTENT;
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(|v| v.trim().to_ascii_lowercase());
        if !accepted
            && mime.as_deref() != Some("application/json")
            && mime.as_deref() != Some("text/event-stream")
        {
            return Err(self.failure(McpTransportError::Malformed));
        }
        let source = response.bytes_stream().boxed();
        let stop = self.stop.clone();
        let errors = self.error.clone();
        let is_sse = mime.as_deref() == Some("text/event-stream");
        let raw=stream::try_unfold((source,EventLimit::default(),active,control,stop,errors),
            move |(mut source,mut limit,active,control,stop,errors)|async move {
                let value=tokio::select! {biased;
                    _=stop.cancelled()=>Err(McpTransportError::Unavailable),
                    v=control.run(async {source.next().await.transpose().map_err(|_|McpTransportError::Unavailable)})=>v,
                };
                let result=value.and_then(|value|match value {
                    Some(bytes)=> {control.wire_budget.consume(bytes.len())?;if is_sse {limit.observe(&bytes)?;}Ok(Some(bytes))},
                    None=>Ok(None),
                });
                match result {
                    Ok(Some(bytes))=>Ok(Some((bytes,(source,limit,active,control,stop,errors)))),
                    Ok(None)=>Ok(None),
                    Err(error)=>{*lock(&errors)=Some(error);Err(error)},
                }
            }).boxed();
        if is_sse && !accepted {
            let errors = self.error.clone();
            let events = SseStream::from_bytes_stream(raw)
                .map(move |event| {
                    let event = event?;
                    if let Some(data) = event
                        .data
                        .as_deref()
                        .filter(|data| initialize && !data.trim().is_empty())
                    {
                        let message: ServerJsonRpcMessage =
                            serde_json::from_str(data).map_err(|_| {
                                *lock(&errors) = Some(McpTransportError::Malformed);
                                sse_stream::Error::Body(Box::new(McpTransportError::Malformed))
                            })?;
                        if let Err(error) = check_initialize(&message) {
                            *lock(&errors) = Some(error);
                            return Err(sse_stream::Error::Body(Box::new(error)));
                        }
                    }
                    Ok(event)
                })
                .boxed();
            return Ok(StreamableHttpPostResponse::Sse(events, id));
        }
        let bytes = raw
            .fold(Ok(Vec::new()), |result, chunk| async move {
                result.and_then(|mut bytes| {
                    bytes.extend_from_slice(&chunk?);
                    Ok(bytes)
                })
            })
            .await
            .map_err(|e| self.failure(e))?;
        if accepted {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let message: ServerJsonRpcMessage = serde_json::from_slice(&bytes)
            .map_err(|_| self.failure(McpTransportError::Malformed))?;
        if initialize {
            check_initialize(&message).map_err(|e| self.failure(e))?;
        }
        Ok(StreamableHttpPostResponse::Json(message, id))
    }
}
pub(super) fn check_initialize(message: &ServerJsonRpcMessage) -> Result<(), McpTransportError> {
    if let ServerJsonRpcMessage::Response(response) = message {
        if let ServerResult::InitializeResult(info) = &response.result {
            if info.protocol_version.as_str() != PROTOCOL {
                return Err(McpTransportError::UnsupportedProtocol);
            }
        }
    }
    Ok(())
}
impl StreamableHttpClient for HttpBackend {
    type Error = McpTransportError;
    async fn post_message(
        &self,
        u: Arc<str>,
        m: ClientJsonRpcMessage,
        s: Option<Arc<str>>,
        a: Option<String>,
        h: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.post(u, m, s, a, h).await
    }
    async fn post_message_with_max_sse_event_size(
        &self,
        u: Arc<str>,
        m: ClientJsonRpcMessage,
        s: Option<Arc<str>>,
        a: Option<String>,
        h: HashMap<HeaderName, HeaderValue>,
        _max: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.post(u, m, s, a, h).await
    }
    async fn get_stream(
        &self,
        _u: Arc<str>,
        _s: Option<Arc<str>>,
        _last: Option<String>,
        _a: Option<String>,
        _h: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, sse_stream::Error>>, StreamableHttpError<Self::Error>>
    {
        Err(StreamableHttpError::ServerDoesNotSupportSse)
    }
    async fn delete_session(
        &self,
        u: Arc<str>,
        s: Arc<str>,
        a: Option<String>,
        h: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        if a.is_some() {
            return Err(self.failure(McpTransportError::Unavailable));
        }
        let _active = Active::new(&self.activity);
        let mut request = self
            .http
            .delete(u.as_ref())
            .header("mcp-session-id", s.as_ref());
        for (name, value) in h {
            if name.as_str() != "mcp-protocol-version" {
                return Err(self.failure(McpTransportError::Malformed));
            }
            request = request.header(name, value);
        }
        let response = tokio::time::timeout(Duration::from_secs(2), request.send())
            .await
            .map_err(|_| self.failure(McpTransportError::TimedOut))?
            .map_err(|_| self.failure(McpTransportError::Unavailable))?;
        if response.status().is_success()
            || response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED
        {
            Ok(())
        } else {
            Err(self.failure(McpTransportError::Unavailable))
        }
    }
}

#[derive(Default)]
pub(crate) struct DefaultMcpTransportFactory {
    stdio: Arc<super::stdio::StdioRuntime>,
}
struct SessionInner {
    backend: HttpBackend,
    sdk: Mutex<Option<RunningService<RoleClient, ClientConfig>>>,
    operation: Mutex<()>,
    shutdown: CancellationToken,
    closing: AtomicBool,
    closed: Notify,
    close_result: StdMutex<Option<Result<(), McpTransportError>>>,
}
#[derive(Clone)]
struct HttpSession(Arc<SessionInner>);
impl HttpSession {
    fn begin_close(&self) {
        if self.0.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        self.0.backend.stop.cancel();
        self.0.shutdown.cancel();
        let inner = self.0.clone();
        tokio::spawn(async move {
            let mut sdk = inner.sdk.lock().await.take();
            let result = match sdk.as_mut() {
                Some(sdk) => sdk
                    .close()
                    .await
                    .map(|_| ())
                    .map_err(|_| McpTransportError::Unavailable),
                None => Ok(()),
            };
            inner.backend.workers.idle().await;
            inner.backend.activity.idle().await;
            *lock(&inner.close_result) = Some(result);
            inner.closed.notify_waiters();
        });
    }
    async fn wait_closed(self) -> Result<PhysicalClose, McpTransportError> {
        self.begin_close();
        loop {
            let changed = self.0.closed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(result) = *lock(&self.0.close_result) {
                return result.map(|_| PhysicalClose(()));
            }
            changed.await;
        }
    }
    fn error(&self, fallback: McpTransportError) -> McpTransportError {
        lock(&self.0.backend.error).unwrap_or(fallback)
    }
    async fn discover_inner(
        self,
        control: TransportControl,
    ) -> Result<McpDiscovery, McpTransportError> {
        let _operation = control
            .run(async { Ok(self.0.operation.lock().await) })
            .await?;
        if self.0.closing.load(Ordering::SeqCst) {
            return Err(McpTransportError::Unavailable);
        }
        self.0.backend.begin(control.clone());
        let sdk = self.0.sdk.lock().await;
        let sdk = sdk.as_ref().ok_or(McpTransportError::Unavailable)?;
        discover_sdk(
            sdk,
            &control,
            |fallback| self.error(fallback),
            || async {
                self.0.backend.activity.idle().await;
                match *lock(&self.0.backend.error) {
                    Some(error) => Err(error),
                    None => Ok(()),
                }
            },
        )
        .await
    }
    async fn call_inner(
        self,
        call: McpCall,
        control: TransportControl,
    ) -> Result<ExternalToolResult, McpTransportError> {
        let _operation = control
            .run(async { Ok(self.0.operation.lock().await) })
            .await?;
        if self.0.closing.load(Ordering::SeqCst) || call.remote_name != call.descriptor.remote_name
        {
            return Err(McpTransportError::Unavailable);
        }
        external_tools::validate_external_arguments(&call.descriptor, &call.arguments)
            .map_err(|_| McpTransportError::SchemaRejected)?;
        self.0.backend.begin(control.clone());
        let sdk = self.0.sdk.lock().await;
        let sdk = sdk.as_ref().ok_or(McpTransportError::Unavailable)?;
        let response = request_sdk(
            sdk,
            call.remote_name,
            call.arguments,
            &control,
            |fallback| self.error(fallback),
        )
        .await?;
        // Correlation can precede the SDK's final response-stream drain.
        control
            .run(async {
                self.0.backend.activity.idle().await;
                Ok(())
            })
            .await
            .map_err(|_| McpTransportError::OutcomeUnknown)?;
        if let Some(error) = *lock(&self.0.backend.error) {
            return Err(error);
        }
        bound_sdk_result(response, &call.descriptor)
    }
}

pub(super) async fn discover_sdk<F, A, Fut>(
    sdk: &RunningService<RoleClient, ClientConfig>,
    control: &TransportControl,
    error: F,
    mut after_page: A,
) -> Result<McpDiscovery, McpTransportError>
where
    F: Fn(McpTransportError) -> McpTransportError,
    A: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), McpTransportError>>,
{
    let mut cursor = None;
    let mut cursors = HashSet::new();
    let mut names = HashSet::new();
    let mut tools = Vec::new();
    for page in 0..4 {
        let params = cursor
            .take()
            .map(|c| PaginatedRequestParams::default().with_cursor(Some(c)));
        let result = control
            .run(async {
                sdk.list_tools(params)
                    .await
                    .map_err(|_| error(McpTransportError::Malformed))
            })
            .await?;
        after_page().await?;
        for tool in result.tools {
            if tools.len() >= 64 || !names.insert(tool.name.to_string()) {
                return Err(McpTransportError::WireLimit);
            }
            tools.push(
                external_tools::prepare_external_tool(
                    &tool.name,
                    tool.description.as_deref().unwrap_or(""),
                    Value::Object((*tool.input_schema).clone()),
                    tool.output_schema
                        .map(|schema| Value::Object((*schema).clone())),
                )
                .map_err(|_| McpTransportError::SchemaRejected)?,
            );
        }
        match result.next_cursor {
            None => {
                return Ok(McpDiscovery {
                    protocol_version: PROTOCOL.into(),
                    tools,
                })
            }
            Some(c)
                if page < 3
                    && !c.is_empty()
                    && c.len() <= 256
                    && !c.chars().any(char::is_control)
                    && cursors.insert(c.clone()) =>
            {
                cursor = Some(c)
            }
            _ => return Err(McpTransportError::WireLimit),
        }
    }
    Err(McpTransportError::WireLimit)
}

pub(super) async fn request_sdk<F>(
    sdk: &RunningService<RoleClient, ClientConfig>,
    remote_name: String,
    arguments: Value,
    control: &TransportControl,
    error: F,
) -> Result<ServerResult, McpTransportError>
where
    F: Fn(McpTransportError) -> McpTransportError,
{
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(
        CallToolRequestParams::new(remote_name).with_arguments(
            arguments
                .as_object()
                .cloned()
                .ok_or(McpTransportError::SchemaRejected)?,
        ),
    ));
    let remaining = control.deadline.saturating_duration_since(Instant::now());
    let options = PeerRequestOptions::with_timeout(remaining).with_max_total_timeout(remaining);
    let response = control
        .run(async {
            let request = sdk
                .peer()
                .send_cancellable_request(request, options)
                .await
                .map_err(|_| error(McpTransportError::Unavailable))?;
            request
                .await_response()
                .await
                .map_err(|failure| match failure {
                    ServiceError::McpError(_) => McpTransportError::RemoteError,
                    ServiceError::Timeout { .. } => McpTransportError::TimedOut,
                    ServiceError::Cancelled { .. } => McpTransportError::Cancelled,
                    _ => error(McpTransportError::Unavailable),
                })
        })
        .await;
    match response {
        Err(
            McpTransportError::Cancelled
            | McpTransportError::TimedOut
            | McpTransportError::Unavailable,
        ) if control.dispatched.load(Ordering::SeqCst) => Err(McpTransportError::OutcomeUnknown),
        other => other,
    }
}

pub(super) fn bound_sdk_result(
    response: ServerResult,
    descriptor: &ExternalToolDescriptor,
) -> Result<ExternalToolResult, McpTransportError> {
    let ServerResult::CallToolResult(result) = response else {
        return Err(McpTransportError::UnsupportedResult);
    };
    let is_error = result.is_error.unwrap_or(false);
    let mut output_descriptor = descriptor.clone();
    if is_error {
        output_descriptor.output_schema = None;
    }
    let structured = match result.structured_content {
        Some(value) => {
            external_tools::validate_external_output(&output_descriptor, &value)
                .map_err(|_| McpTransportError::SchemaRejected)?;
            Some(portable(value)?)
        }
        None if !is_error && descriptor.output_schema.is_some() => {
            return Err(McpTransportError::SchemaRejected)
        }
        None => None,
    };
    let mut text_blocks = Vec::new();
    let mut omitted_blocks = 0u32;
    for block in result.content {
        match block {
            ContentBlock::Text(text) if text_blocks.len() < 16 => text_blocks.push(text.text),
            _ => omitted_blocks = omitted_blocks.saturating_add(1),
        }
    }
    external_tools::bound_external_result(
        ExternalToolResult {
            text_blocks,
            structured_data: structured,
            is_error,
            omitted_blocks,
            shortened: omitted_blocks > 0,
        },
        1024,
    )
    .map_err(|_| McpTransportError::WireLimit)
}
fn portable(value: Value) -> Result<DataValue, McpTransportError> {
    Ok(match value {
        Value::Null => DataValue::Null,
        Value::Bool(v) => DataValue::Bool(v),
        Value::String(v) => DataValue::String(v),
        Value::Number(v) => DataValue::Number(
            v.as_f64()
                .filter(|v| {
                    v.is_finite() && (v.fract() != 0.0 || v.abs() <= 9_007_199_254_740_991.0)
                })
                .ok_or(McpTransportError::SchemaRejected)?,
        ),
        Value::Array(v) => DataValue::Array(v.into_iter().map(portable).collect::<Result<_, _>>()?),
        Value::Object(v) => DataValue::Object(
            v.into_iter()
                .map(|(k, v)| portable(v).map(|v| (k, v)))
                .collect::<Result<BTreeMap<_, _>, _>>()?,
        ),
    })
}
pub(super) fn owned<T: Send + 'static>(
    control: TransportControl,
    future: impl std::future::Future<Output = Result<T, McpTransportError>> + Send + 'static,
) -> BoxFuture<'static, Result<T, McpTransportError>> {
    Box::pin(async move {
        let guard = control.cancel.clone().drop_guard();
        let task = tokio::spawn(future);
        let result = task.await.map_err(|_| McpTransportError::Unavailable)?;
        guard.disarm();
        result
    })
}
impl McpTransportSession for HttpSession {
    fn discover(
        &self,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>> {
        let session = self.clone();
        owned(control.clone(), async move {
            let result = control
                .run(session.clone().discover_inner(control.clone()))
                .await;
            if result.is_err() {
                session.begin_close();
            }
            result
        })
    }
    fn call_once(
        &self,
        call: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>> {
        let session = self.clone();
        owned(control.clone(), async move {
            let result = control
                .run(session.clone().call_inner(call, control.clone()))
                .await;
            if result.is_err() {
                session.begin_close();
            }
            match result {
                Err(McpTransportError::Cancelled | McpTransportError::TimedOut)
                    if control.dispatched.load(Ordering::SeqCst) =>
                {
                    Err(McpTransportError::OutcomeUnknown)
                }
                other => other,
            }
        })
    }
    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>> {
        let session = self.clone();
        self.begin_close();
        Box::pin(session.wait_closed())
    }
}
impl DefaultMcpTransportFactory {
    #[cfg(test)]
    pub(crate) fn stdio_child_count(&self) -> usize {
        self.stdio.child_count()
    }
    fn connect_http(
        &self,
        endpoint: ValidatedMcpEndpoint,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        owned(control.clone(), async move {
            control.check()?;
            let backend = HttpBackend::new(control.clone())?;
            let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.0)
                .reinit_on_expired_session(false)
                .max_sse_event_size(RAW_EVENT);
            config.retry_config = Arc::new(NoRetry);
            config.max_concurrent_requests = 1;
            config.channel_buffer_capacity = 4;
            let shutdown = CancellationToken::new();
            let mut info = ClientConfig::default();
            info.protocol_version = ProtocolVersion::V_2025_11_25;
            let result = control
                .run(async {
                    // Create the SDK worker only after the control future is polled:
                    // its startup wait has no queued initialize before this point.
                    let transport = JoinedTransport::new(
                        StreamableHttpClientTransport::with_client(backend.clone(), config),
                        &backend.workers,
                    );
                    serve_client_with_lifecycle_and_ct(
                        info,
                        transport,
                        ClientLifecycleMode::Initialize,
                        shutdown.clone(),
                    )
                    .await
                    .map_err(|_| lock(&backend.error).unwrap_or(McpTransportError::Malformed))
                })
                .await;
            let sdk = match result {
                Ok(sdk) => sdk,
                Err(error) => {
                    backend.stop.cancel();
                    shutdown.cancel();
                    backend.workers.idle().await;
                    backend.activity.idle().await;
                    return Err(error);
                }
            };
            let session = HttpSession(Arc::new(SessionInner {
                backend,
                sdk: Mutex::new(Some(sdk)),
                operation: Mutex::new(()),
                shutdown,
                closing: AtomicBool::new(false),
                closed: Notify::new(),
                close_result: StdMutex::new(None),
            }));
            let valid = {
                let sdk = session.0.sdk.lock().await;
                let sdk = sdk.as_ref().expect("fresh SDK");
                sdk.set_response_cache_config(rmcp::ClientCacheConfig::disabled())
                    .await;
                sdk.peer_info()
                    .is_some_and(|info| info.protocol_version.as_str() == PROTOCOL)
            };
            if !valid {
                session.begin_close();
                session.wait_closed().await?;
                return Err(McpTransportError::UnsupportedProtocol);
            }
            if let Err(error) = control
                .run(async {
                    session.0.backend.activity.idle().await;
                    Ok(())
                })
                .await
            {
                session.begin_close();
                session.wait_closed().await?;
                return Err(error);
            }
            Ok(Arc::new(session) as Arc<dyn McpTransportSession>)
        })
    }
}

impl McpTransportFactory for DefaultMcpTransportFactory {
    fn connect(
        &self,
        launch: ValidatedMcpLaunch,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        match launch {
            ValidatedMcpLaunch::Http(endpoint) => self.connect_http(endpoint, control),
            ValidatedMcpLaunch::Stdio {
                spec,
                connection_id,
            } => super::stdio::connect(self.stdio.clone(), spec, connection_id, control),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use serde_json::json;

    fn control(seconds: u64) -> TransportControl {
        TransportControl::new(
            Instant::now() + Duration::from_secs(seconds),
            CancellationToken::new(),
        )
    }
    #[test]
    fn raw_event_and_shared_aggregate_caps_precede_parsing() {
        let ctl = control(5);
        let clone = ctl.clone();
        ctl.wire_budget.consume(RAW_TOTAL - 1).unwrap();
        clone.wire_budget.consume(1).unwrap();
        assert_eq!(
            ctl.wire_budget.consume(1),
            Err(McpTransportError::WireLimit)
        );
        let mut event = EventLimit::default();
        event.observe(&vec![b':'; RAW_EVENT]).unwrap();
        assert_eq!(event.observe(b"x"), Err(McpTransportError::WireLimit));
        let mut event = EventLimit::default();
        event.observe(b"data: first\r").unwrap();
        event.observe(b"\n\r\n").unwrap();
        event.observe(&vec![b'x'; RAW_EVENT - 1]).unwrap();
        assert_eq!(event.observe(b"x"), Err(McpTransportError::WireLimit));
        let mut event = EventLimit::default();
        assert_eq!(
            event.observe(&b":x\r\n".repeat(RAW_EVENT / 4 + 1)),
            Err(McpTransportError::WireLimit)
        );
        assert!(NoRetry.retry(0).is_none());
        assert!(NoRetry.retry(100).is_none());
    }
    #[derive(Clone)]
    struct Fake {
        sse: bool,
        fail: bool,
        methods: Arc<StdMutex<Vec<String>>>,
        tool_result: Option<Value>,
    }
    async fn fake(State(fake): State<Fake>, Json(request): Json<Value>) -> Response {
        let method = request["method"].as_str().unwrap_or("").to_string();
        lock(&fake.methods).push(method.clone());
        let result = match method.as_str() {
            "initialize" => {
                assert_eq!(request["params"]["protocolVersion"], PROTOCOL);
                assert_eq!(request["params"]["capabilities"], json!({}));
                json!({"protocolVersion":PROTOCOL,"capabilities":{"tools":{}},"serverInfo":{"name":"synthetic","version":"1"}})
            }
            "notifications/initialized" | "notifications/cancelled" => {
                return axum::http::StatusCode::ACCEPTED.into_response()
            }
            "tools/list" => {
                json!({"tools":[{"name":"echo_record","description":"Synthetic echo","inputSchema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}}]})
            }
            "tools/call" => {
                if fake.fail {
                    return axum::http::StatusCode::NOT_FOUND.into_response();
                }
                assert_eq!(request["params"]["name"], "echo_record");
                fake.tool_result.clone().unwrap_or_else(||json!({"content":[{"type":"text","text":"synthetic blue record"}],"isError":false}))
            }
            _ => return axum::http::StatusCode::BAD_REQUEST.into_response(),
        };
        let value = json!({"jsonrpc":"2.0","id":request["id"],"result":result});
        if fake.sse && method == "tools/call" {
            (
                [("content-type", "text/event-stream")],
                format!("data: {value}\n\n"),
            )
                .into_response()
        } else {
            Json(value).into_response()
        }
    }
    async fn server(
        sse: bool,
        fail: bool,
    ) -> (
        ValidatedMcpEndpoint,
        Arc<StdMutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        server_with_result(sse, fail, None).await
    }
    async fn server_with_result(
        sse: bool,
        fail: bool,
        tool_result: Option<Value>,
    ) -> (
        ValidatedMcpEndpoint,
        Arc<StdMutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let methods = Arc::new(StdMutex::new(Vec::new()));
        let fake_state = Fake {
            sse,
            fail,
            methods: methods.clone(),
            tool_result,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/mcp", post(fake))
                    .with_state(fake_state),
            )
            .await
            .unwrap();
        });
        (
            ValidatedMcpEndpoint(format!("http://{addr}/mcp")),
            methods,
            task,
        )
    }
    #[tokio::test]
    async fn actual_sdk_json_and_request_sse_each_call_once_without_cached_discovery() {
        for sse in [false, true] {
            let (endpoint, methods, server) = server(sse, false).await;
            let ctl = control(5);
            let session = DefaultMcpTransportFactory::default()
                .connect(ValidatedMcpLaunch::Http(endpoint), ctl.clone())
                .await
                .unwrap();
            let first = session.discover(ctl).await.unwrap();
            assert_eq!(first.protocol_version, PROTOCOL);
            let again = session.discover(control(5)).await.unwrap();
            assert_eq!(first.tools, again.tools);
            let descriptor = first.tools.into_iter().next().unwrap();
            let result = session
                .call_once(
                    McpCall {
                        remote_name: descriptor.remote_name.clone(),
                        arguments: json!({"value":"blue"}),
                        descriptor,
                    },
                    control(5),
                )
                .await
                .unwrap();
            assert_eq!(result.text_blocks, ["synthetic blue record"]);
            assert!(!result.is_error);
            session.close().await.unwrap();
            let methods = lock(&methods).clone();
            assert_eq!(
                methods
                    .iter()
                    .filter(|m| m.as_str() == "initialize")
                    .count(),
                1
            );
            assert_eq!(
                methods
                    .iter()
                    .filter(|m| m.as_str() == "tools/list")
                    .count(),
                2
            );
            assert_eq!(
                methods
                    .iter()
                    .filter(|m| m.as_str() == "tools/call")
                    .count(),
                1
            );
            assert!(!methods.iter().any(|m| m == "server/discover"));
            server.abort();
            let _ = server.await;
        }
    }
    #[tokio::test]
    async fn sdk_error_results_keep_text_without_requiring_success_output_but_bound_error_data() {
        let mut deep = json!(null);
        for _ in 0..16 {
            deep = json!([deep]);
        }
        let cases = [
            (
                json!({"content":[{"type":"text","text":"recorded tool failure"}],"isError":true}),
                true,
            ),
            (
                json!({"content":[{"type":"text","text":"recorded tool failure"}],"structuredContent":{"reason":"not success output"},"isError":true}),
                true,
            ),
            (json!({"content":[],"isError":false}), false),
            (
                json!({"content":[],"structuredContent":{"number":9007199254740992u64},"isError":true}),
                false,
            ),
            (
                json!({"content":[],"structuredContent":{"nested":deep},"isError":true}),
                false,
            ),
        ];
        for (result, valid_error) in cases {
            let (endpoint, methods, server) = server_with_result(false, false, Some(result)).await;
            let ctl = control(5);
            let session = DefaultMcpTransportFactory::default()
                .connect(ValidatedMcpLaunch::Http(endpoint), ctl.clone())
                .await
                .unwrap();
            let source = session.discover(ctl).await.unwrap().tools.remove(0);
            let descriptor = external_tools::prepare_external_tool(
                &source.remote_name,
                &source.description,
                source.input_schema,
                Some(json!({"type":"object","properties":{"ok":{"const":true}},"required":["ok"]})),
            )
            .unwrap();
            let result = session
                .call_once(
                    McpCall {
                        remote_name: descriptor.remote_name.clone(),
                        arguments: json!({"value":"blue"}),
                        descriptor,
                    },
                    control(5),
                )
                .await;
            if valid_error {
                let result = result.unwrap();
                assert!(result.is_error);
                assert_eq!(result.text_blocks, ["recorded tool failure"]);
            } else {
                assert_eq!(result.unwrap_err(), McpTransportError::SchemaRejected);
            }
            session.close().await.unwrap();
            assert_eq!(
                lock(&methods)
                    .iter()
                    .filter(|m| m.as_str() == "tools/call")
                    .count(),
                1
            );
            server.abort();
            let _ = server.await;
        }
    }
    struct GatedRetirement {
        started: CancellationToken,
        release: CancellationToken,
        calls: Arc<AtomicUsize>,
    }
    impl Transport<RoleClient> for GatedRetirement {
        type Error = McpTransportError;
        fn send(
            &mut self,
            _: ClientJsonRpcMessage,
        ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send + 'static {
            async { Ok(()) }
        }
        async fn receive(&mut self) -> Option<ServerJsonRpcMessage> {
            std::future::pending().await
        }
        async fn close(&mut self) -> Result<(), Self::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.cancel();
            self.release.cancelled().await;
            Ok(())
        }
    }
    #[tokio::test]
    async fn dropped_sdk_startup_waits_owned_worker_retirement_even_when_http_is_idle() {
        let ctl = control(5);
        let backend = HttpBackend::new(ctl.clone()).unwrap();
        let started = CancellationToken::new();
        let release = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = CancellationToken::new();
        let entry = entered.clone();
        let transport = JoinedTransport::new(
            GatedRetirement {
                started: started.clone(),
                release: release.clone(),
                calls: calls.clone(),
            },
            &backend.workers,
        );
        let workers = backend.workers.clone();
        let http = backend.activity.clone();
        let operation = ctl.clone();
        let task = tokio::spawn(async move {
            let result = operation
                .run(async move {
                    let _transport = transport;
                    entry.cancel();
                    std::future::pending::<Result<(), McpTransportError>>().await
                })
                .await;
            workers.idle().await;
            http.idle().await;
            result
        });
        entered.cancelled().await;
        assert_eq!(backend.activity.count.load(Ordering::SeqCst), 0);
        ctl.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), started.cancelled())
            .await
            .unwrap();
        assert!(!task.is_finished());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(backend.workers.count.load(Ordering::SeqCst), 1);
        release.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap(),
            Err(McpTransportError::Cancelled)
        );
        assert_eq!(backend.workers.count.load(Ordering::SeqCst), 0);
        assert_eq!(backend.activity.count.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn session_expiry_is_unknown_and_never_reinitializes_or_replays() {
        let (endpoint, methods, server) = server(false, true).await;
        let ctl = control(5);
        let session = DefaultMcpTransportFactory::default()
            .connect(ValidatedMcpLaunch::Http(endpoint), ctl.clone())
            .await
            .unwrap();
        let descriptor = session.discover(ctl).await.unwrap().tools.remove(0);
        let error = session
            .call_once(
                McpCall {
                    remote_name: descriptor.remote_name.clone(),
                    arguments: json!({"value":"blue"}),
                    descriptor,
                },
                control(5),
            )
            .await
            .unwrap_err();
        assert_eq!(error, McpTransportError::OutcomeUnknown);
        session.close().await.unwrap();
        let methods = lock(&methods).clone();
        assert_eq!(
            methods
                .iter()
                .filter(|m| m.as_str() == "initialize")
                .count(),
            1
        );
        assert_eq!(
            methods
                .iter()
                .filter(|m| m.as_str() == "tools/call")
                .count(),
            1
        );
        server.abort();
        let _ = server.await;
    }
    #[tokio::test]
    async fn cancellation_deadline_and_cleanup_witness_do_not_release_active_work() {
        let ctl = control(5);
        ctl.cancel.cancel();
        assert_eq!(
            ctl.run(async {
                panic!("cancelled operation must not dispatch");
                #[allow(unreachable_code)]
                Ok(())
            })
            .await,
            Err(McpTransportError::Cancelled)
        );
        let ctl = TransportControl::new(
            Instant::now() + Duration::from_millis(15),
            CancellationToken::new(),
        );
        assert_eq!(
            ctl.run(std::future::pending::<Result<(), McpTransportError>>())
                .await,
            Err(McpTransportError::TimedOut)
        );
        let backend = HttpBackend::new(control(5)).unwrap();
        let active = Active::new(&backend.activity);
        let session = HttpSession(Arc::new(SessionInner {
            backend,
            sdk: Mutex::new(None),
            operation: Mutex::new(()),
            shutdown: CancellationToken::new(),
            closing: AtomicBool::new(false),
            closed: Notify::new(),
            close_result: StdMutex::new(None),
        }));
        assert!(
            tokio::time::timeout(Duration::from_millis(15), session.close())
                .await
                .is_err()
        );
        assert!(session.0.closing.load(Ordering::SeqCst));
        assert!(lock(&session.0.close_result).is_none());
        assert_eq!(session.0.backend.activity.count.load(Ordering::SeqCst), 1);
        drop(active);
        tokio::time::timeout(Duration::from_secs(1), session.close())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.0.backend.activity.count.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn actual_chunked_json_and_sse_wire_overflow_fail_before_deserialization() {
        for sse in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let app = Router::new().route(
                "/mcp",
                post(move || async move {
                    let payload = if sse {
                        format!("data: {}\n\n", "x".repeat(RAW_EVENT))
                    } else {
                        " ".repeat(RAW_TOTAL + 1)
                    };
                    let chunks = payload
                        .as_bytes()
                        .chunks(8192)
                        .map(|chunk| {
                            Ok::<_, std::io::Error>(axum::body::Bytes::copy_from_slice(chunk))
                        })
                        .collect::<Vec<_>>();
                    let body = axum::body::Body::from_stream(stream::iter(chunks));
                    Response::builder()
                        .header(
                            "content-type",
                            if sse {
                                "text/event-stream"
                            } else {
                                "application/json"
                            },
                        )
                        .body(body)
                        .unwrap()
                }),
            );
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let result = DefaultMcpTransportFactory::default()
                .connect(
                    ValidatedMcpLaunch::Http(ValidatedMcpEndpoint(format!("http://{addr}/mcp"))),
                    control(5),
                )
                .await;
            assert!(
                matches!(result, Err(McpTransportError::WireLimit)),
                "raw overflow must precede malformed JSON/SSE parsing"
            );
            server.abort();
            let _ = server.await;
        }
    }
}
