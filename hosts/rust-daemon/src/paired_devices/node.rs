//! A deliberately small TLS-only, read-only MCP node host.
use anima_service_adapters::paired_devices::*;
use atomicwrites::{AllowOverwrite, AtomicFile};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::{self, Read, Write},
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_rustls::{
    rustls::{
        pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
        ServerConfig,
    },
    TlsAcceptor,
};
use tokio_util::sync::CancellationToken;

fn loopback() -> SocketAddr {
    "127.0.0.1:8443".parse().expect("literal")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NodeConfig {
    pub(crate) identity: String,
    pub(crate) generation: u64,
    #[serde(default = "loopback")]
    pub(crate) bind: SocketAddr,
    #[serde(default)]
    pub(crate) remote_bind_approved: bool,
    pub(crate) root: PathBuf,
    pub(crate) certificate: PathBuf,
    pub(crate) private_key: PathBuf,
    pub(crate) state_file: PathBuf,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeState {
    identity: String,
    token_hash: Option<String>,
    #[serde(default)]
    pair_manifest_digest: Option<String>,
    revoked: bool,
    replay: BTreeSet<String>,
}
struct LiveState {
    durable: NodeState,
    ticket_hash: String,
    ticket_expires_ms: u64,
}
pub(crate) struct NodeHost {
    config: NodeConfig,
    manifest: NodeManifest,
    root: Dir,
    state: Mutex<LiveState>,
}
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "paired node request unavailable or rejected",
    )
}
fn secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn open_directory(path: &Path) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for part in path.components() {
        match part {
            Component::Prefix(prefix) => {
                #[cfg(windows)]
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) {
                    return Err(invalid());
                }
                #[cfg(not(windows))]
                let _ = prefix;
                anchor.push(part.as_os_str());
            }
            Component::RootDir => anchor.push(part.as_os_str()),
            Component::Normal(name) => names.push(name),
            _ => return Err(invalid()),
        }
    }
    let mut dir = Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?;
    for name in names {
        let before = dir.symlink_metadata(name)?;
        if crate::tools::workspace_bytes::linked(&before) || !before.is_dir() {
            return Err(invalid());
        }
        dir = dir.open_dir_nofollow(name)?;
        let after = dir.dir_metadata()?;
        if crate::tools::workspace_bytes::linked(&after) || !after.is_dir() {
            return Err(invalid());
        }
    }
    Ok(dir)
}
fn ordinary_file(dir: &Dir, name: &std::ffi::OsStr) -> io::Result<cap_std::fs::File> {
    let before = dir.symlink_metadata(name)?;
    if crate::tools::workspace_bytes::linked(&before) || !before.is_file() {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = dir.open_with(name, &options)?;
    let meta = file.metadata()?;
    if crate::tools::workspace_bytes::linked(&meta) || !meta.is_file() || meta.nlink() != 1 {
        return Err(invalid());
    }
    Ok(file)
}
fn external_file(path: &Path, root: &Path, must_exist: bool) -> io::Result<()> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid());
    }
    let parent = path.parent().ok_or_else(invalid)?;
    let dir = open_directory(parent)?;
    let parent = parent.canonicalize()?;
    if parent.starts_with(root) {
        return Err(invalid());
    }
    match dir.symlink_metadata(path.file_name().ok_or_else(invalid)?) {
        Ok(_) => {
            ordinary_file(&dir, path.file_name().ok_or_else(invalid)?)?;
        }
        Err(error) if !must_exist && error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}
impl NodeHost {
    pub(crate) fn open(config: NodeConfig) -> io::Result<(Arc<Self>, String)> {
        if !valid_text(&config.identity)
            || config.generation == 0
            || (!config.bind.ip().is_loopback() && !config.remote_bind_approved)
        {
            return Err(invalid());
        }
        let root = open_directory(&config.root)?;
        let canonical = config.root.canonicalize()?;
        external_file(&config.state_file, &canonical, false)?;
        // Certificate and key are required by serve(), while open() permits a
        // controlled in-process fixture without generating credential files.
        for path in [&config.certificate, &config.private_key] {
            external_file(path, &canonical, false)?;
        }
        let manifest = NodeManifest::minimal(
            config.identity.clone(),
            config.generation,
            digest_bytes(canonical.to_string_lossy().as_bytes()),
        );
        manifest.validate().map_err(|_| invalid())?;
        let current_manifest_digest = manifest.digest().map_err(|_| invalid())?;
        let mut durable = if config.state_file.exists() {
            let bytes = std::fs::read(&config.state_file)?;
            if bytes.len() > 512 * 1024 {
                return Err(invalid());
            }
            let state: NodeState = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if state.identity != config.identity
                || state.replay.len() > 4096
                || state.replay.iter().any(|k| !valid_digest(k))
                || state.token_hash.as_ref().is_some_and(|k| !valid_digest(k))
            {
                return Err(invalid());
            }
            state
        } else {
            NodeState {
                identity: config.identity.clone(),
                ..Default::default()
            }
        };
        // A bearer authorizes only the exact manifest the owner paired. Public
        // discovery and freshly forged metadata cannot broaden that bearer.
        // Legacy records without this binding also lose their authority while
        // every replay admission remains intact for future pairings.
        if durable.pair_manifest_digest.as_deref() != Some(current_manifest_digest.as_str()) {
            durable.token_hash = None;
            durable.pair_manifest_digest = None;
            durable.revoked = true;
        }
        let ticket = secret();
        let host = Arc::new(Self {
            config,
            manifest,
            root,
            state: Mutex::new(LiveState {
                durable,
                ticket_hash: digest_bytes(ticket.as_bytes()),
                ticket_expires_ms: now_ms() + 300_000,
            }),
        });
        host.save(&host.state.lock().map_err(|_| invalid())?.durable)?;
        Ok((host, ticket))
    }
    pub(crate) fn manifest(&self) -> &NodeManifest {
        &self.manifest
    }
    fn save(&self, state: &NodeState) -> io::Result<()> {
        let bytes = serde_json::to_vec(state).map_err(|_| invalid())?;
        AtomicFile::new(&self.config.state_file, AllowOverwrite)
            .write(|file| {
                file.write_all(&bytes)?;
                file.sync_all()
            })
            .map_err(|_| invalid())?;
        #[cfg(unix)]
        open_directory(self.config.state_file.parent().ok_or_else(invalid)?)?
            .into_std_file()
            .sync_all()?;
        Ok(())
    }
    pub(crate) fn pair(&self, ticket: &str, identity: &str, digest: &str) -> io::Result<String> {
        let mut state = self.state.lock().map_err(|_| invalid())?;
        if ticket.len() != 64
            || state.ticket_expires_ms <= now_ms()
            || state.ticket_hash != digest_bytes(ticket.as_bytes())
            || identity != self.manifest.identity
            || digest != self.manifest.digest().map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        let token = secret();
        state.ticket_expires_ms = 0;
        state.durable.token_hash = Some(digest_bytes(token.as_bytes()));
        state.durable.pair_manifest_digest = Some(digest.to_owned());
        state.durable.revoked = false;
        // Consume the ticket and settle token hash before returning the bearer.
        if self.save(&state.durable).is_err() {
            state.durable.revoked = true;
            return Err(invalid());
        }
        Ok(token)
    }
    pub(crate) fn authenticate(&self, token: &str) -> io::Result<()> {
        let state = self.state.lock().map_err(|_| invalid())?;
        self.auth(&state.durable, token)
    }
    fn auth(&self, state: &NodeState, token: &str) -> io::Result<()> {
        let current_manifest_digest = self.manifest.digest().map_err(|_| invalid())?;
        if token.len() != 64
            || state.revoked
            || state.token_hash.as_deref() != Some(digest_bytes(token.as_bytes()).as_str())
            || state.pair_manifest_digest.as_deref() != Some(current_manifest_digest.as_str())
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn revoke(&self, token: &str) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| invalid())?;
        self.auth(&state.durable, token)?;
        state.durable.revoked = true;
        self.save(&state.durable)
    }
    pub(crate) fn invoke(
        &self,
        token: &str,
        name: &str,
        args: Value,
        invocation: &NodeInvocation,
    ) -> io::Result<Value> {
        invocation
            .validate(&self.manifest, now_ms())
            .map_err(|_| invalid())?;
        let descriptor = self
            .manifest
            .tools
            .iter()
            .find(|t| t.remote_name == name && t.descriptor_digest == invocation.descriptor_digest)
            .ok_or_else(invalid)?;
        anima_core::validate_external_arguments(descriptor, &args).map_err(|_| invalid())?;
        let mut state = self.state.lock().map_err(|_| invalid())?;
        self.auth(&state.durable, token)?;
        if state.durable.replay.len() >= 4096
            || !state.durable.replay.insert(invocation.replay_key())
        {
            return Err(invalid());
        }
        // Durable at-most-once admission survives actual node restarts. Failed
        // or interrupted reads retain the receipt and are never replayed.
        self.save(&state.durable)?;
        let result = match name {
            "node_system_info" => {
                json!({"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"identity":self.manifest.identity})
            }
            "node_list_root" => {
                let mut entries = Vec::new();
                for entry in self.root.entries()?.take(MAX_NODE_ENTRIES + 1) {
                    let entry = entry?;
                    let name = entry.file_name();
                    let Some(name_text) = name
                        .to_str()
                        .filter(|name| name.len() <= 255 && name.chars().all(|c| !c.is_control()))
                    else {
                        continue;
                    };
                    let meta = self.root.symlink_metadata(&name)?;
                    if crate::tools::workspace_bytes::linked(&meta) {
                        continue;
                    }
                    if meta.is_file() && ordinary_file(&self.root, &name).is_err() {
                        continue;
                    }
                    if meta.is_dir() || meta.is_file() {
                        entries.push(json!({"name":name_text,"kind":if meta.is_dir(){"directory"}else{"file"}}));
                    }
                }
                if entries.len() > MAX_NODE_ENTRIES {
                    return Err(invalid());
                }
                entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                json!({"entries":entries})
            }
            "node_read_file" => {
                let path = args["path"].as_str().ok_or_else(invalid)?;
                let parts = path.split('/').collect::<Vec<_>>();
                if parts.is_empty()
                    || parts.len() > 32
                    || parts.iter().any(|p| {
                        p.is_empty()
                            || *p == "."
                            || *p == ".."
                            || p.contains(['\\', ':'])
                            || p.chars().any(char::is_control)
                            || p.ends_with([' ', '.'])
                            || matches!(
                                p.split('.')
                                    .next()
                                    .unwrap_or("")
                                    .to_ascii_uppercase()
                                    .as_str(),
                                "CON"
                                    | "PRN"
                                    | "AUX"
                                    | "NUL"
                                    | "COM1"
                                    | "COM2"
                                    | "COM3"
                                    | "COM4"
                                    | "COM5"
                                    | "COM6"
                                    | "COM7"
                                    | "COM8"
                                    | "COM9"
                                    | "LPT1"
                                    | "LPT2"
                                    | "LPT3"
                                    | "LPT4"
                                    | "LPT5"
                                    | "LPT6"
                                    | "LPT7"
                                    | "LPT8"
                                    | "LPT9"
                            )
                    })
                {
                    return Err(invalid());
                }
                let mut dir = self.root.try_clone()?;
                for part in &parts[..parts.len() - 1] {
                    let meta = dir.symlink_metadata(part)?;
                    if crate::tools::workspace_bytes::linked(&meta) || !meta.is_dir() {
                        return Err(invalid());
                    }
                    dir = dir.open_dir_nofollow(part)?;
                    if crate::tools::workspace_bytes::linked(&dir.dir_metadata()?) {
                        return Err(invalid());
                    }
                }
                let file = ordinary_file(&dir, std::ffi::OsStr::new(parts[parts.len() - 1]))?;
                let before = file.metadata()?;
                if before.len() > MAX_NODE_READ_BYTES as u64 {
                    return Err(invalid());
                }
                let mut bytes = Vec::new();
                (&file)
                    .take(MAX_NODE_READ_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                let after = file.metadata()?;
                if bytes.len() > MAX_NODE_READ_BYTES
                    || after.nlink() != 1
                    || before.len() != after.len()
                    || bytes.len() as u64 != after.len()
                    || before.modified()? != after.modified()?
                {
                    return Err(invalid());
                }
                json!({"path":path,"text":String::from_utf8(bytes).map_err(|_| invalid())?})
            }
            _ => return Err(invalid()),
        };
        if now_ms() >= invocation.deadline_unix_ms {
            return Err(invalid());
        }
        if serde_json::to_vec(&result).map_err(|_| invalid())?.len() > MAX_NODE_WIRE_BYTES / 2 {
            return Err(invalid());
        }
        Ok(result)
    }
    pub(crate) fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/manifest", get(manifest_route))
            .route("/pair", post(pair_route))
            .route("/revoke", post(revoke_route))
            .route(
                "/mcp",
                post(mcp_route)
                    .delete(delete_route)
                    .get(|| async { StatusCode::METHOD_NOT_ALLOWED }),
            )
            .with_state(self.clone())
    }
}
fn denied() -> Response {
    (StatusCode::FORBIDDEN, "paired node request rejected").into_response()
}
async fn bounded(request: Request) -> Result<(String, Value), Response> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        axum::body::to_bytes(request.into_body(), MAX_NODE_WIRE_BYTES),
    )
    .await
    .map_err(|_| denied())?
    .map_err(|_| denied())?;
    let value = serde_json::from_slice(&bytes).map_err(|_| denied())?;
    Ok((token, value))
}
async fn manifest_route(State(node): State<Arc<NodeHost>>) -> Response {
    Json(json!({"manifest":node.manifest(),"manifestDigest":node.manifest().digest().unwrap_or_default()})).into_response()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PairRequest {
    ticket: String,
    expected_identity: String,
    expected_manifest_digest: String,
}
async fn pair_route(State(node): State<Arc<NodeHost>>, request: Request) -> Response {
    let Ok((_, value)) = bounded(request).await else {
        return denied();
    };
    let Ok(input) = serde_json::from_value::<PairRequest>(value) else {
        return denied();
    };
    match node.pair(
        &input.ticket,
        &input.expected_identity,
        &input.expected_manifest_digest,
    ) {
        Ok(token) => Json(json!({"bearerToken":token})).into_response(),
        Err(_) => denied(),
    }
}
async fn revoke_route(State(node): State<Arc<NodeHost>>, request: Request) -> Response {
    let Ok((token, _)) = bounded(request).await else {
        return denied();
    };
    if node.revoke(&token).is_ok() {
        StatusCode::NO_CONTENT.into_response()
    } else {
        denied()
    }
}
async fn delete_route(State(node): State<Arc<NodeHost>>, request: Request) -> Response {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    if node.authenticate(token).is_ok() {
        StatusCode::NO_CONTENT.into_response()
    } else {
        denied()
    }
}
async fn mcp_route(State(node): State<Arc<NodeHost>>, request: Request) -> Response {
    let Ok((token, value)) = bounded(request).await else {
        return denied();
    };
    if node.authenticate(&token).is_err() || value["jsonrpc"] != "2.0" {
        return denied();
    }
    let id = value.get("id").cloned();
    let method = value["method"].as_str().unwrap_or("");
    if method.starts_with("notifications/") {
        return StatusCode::ACCEPTED.into_response();
    }
    let Some(id) = id.filter(|id| id.is_string() || id.is_i64()) else {
        return denied();
    };
    let result = match method {
        "initialize" => Ok(
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"anima-node","version":"1"}}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(
            json!({"tools":node.manifest.tools.iter().map(|t|json!({"name":t.remote_name,"description":t.description,"inputSchema":t.input_schema})).collect::<Vec<_>>()}),
        ),
        "tools/call" => {
            let input = serde_json::from_value::<NodeInvocation>(
                value["params"]["_meta"][NODE_META_KEY].clone(),
            );
            input.map_err(|_|invalid()).and_then(|input|node.invoke(&token,value["params"]["name"].as_str().unwrap_or(""),value["params"]["arguments"].clone(),&input)).map(|result|json!({"content":[{"type":"text","text":result.to_string()}],"isError":false}))
        }
        _ => Err(invalid()),
    };
    match result {
        Ok(result)=>{
            let response=json!({"jsonrpc":"2.0","id":id,"result":result});
            if serde_json::to_vec(&response).is_ok_and(|bytes|bytes.len()<=MAX_NODE_WIRE_BYTES) { Json(response).into_response() } else { denied() }
        },
        Err(_)=>Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"paired node request rejected"}})).into_response()
    }
}
pub(crate) struct TlsListener {
    tcp: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
    slots: Arc<tokio::sync::Semaphore>,
}
impl TlsListener {
    pub(crate) fn new(tcp: tokio::net::TcpListener, acceptor: TlsAcceptor) -> Self {
        Self {
            tcp,
            acceptor,
            slots: Arc::new(tokio::sync::Semaphore::new(16)),
        }
    }
}
pub(crate) struct NodeIo {
    stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    deadline: std::pin::Pin<Box<tokio::time::Sleep>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl NodeIo {
    fn expired(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        std::future::Future::poll(self.deadline.as_mut(), cx).is_ready()
    }
}
impl tokio::io::AsyncRead for NodeIo {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.expired(cx) {
            return std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "node connection deadline",
            )));
        }
        tokio::io::AsyncRead::poll_read(std::pin::Pin::new(&mut this.stream), cx, buf)
    }
}
impl tokio::io::AsyncWrite for NodeIo {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.expired(cx) {
            return std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "node connection deadline",
            )));
        }
        tokio::io::AsyncWrite::poll_write(std::pin::Pin::new(&mut this.stream), cx, bytes)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.expired(cx) {
            return std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "node connection deadline",
            )));
        }
        tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut this.stream), cx)
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.expired(cx) {
            return std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "node connection deadline",
            )));
        }
        tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(&mut this.stream), cx)
    }
}
impl axum::serve::Listener for TlsListener {
    type Io = NodeIo;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let permit = self
                .slots
                .clone()
                .acquire_owned()
                .await
                .expect("node admission never closes");
            match self.tcp.accept().await {
                Ok((stream, addr)) => {
                    if let Ok(Ok(tls)) =
                        tokio::time::timeout(Duration::from_secs(5), self.acceptor.accept(stream))
                            .await
                    {
                        return (
                            NodeIo {
                                stream: tls,
                                deadline: Box::pin(tokio::time::sleep(Duration::from_secs(30))),
                                _permit: permit,
                            },
                            addr,
                        );
                    }
                }
                Err(_) => tokio::task::yield_now().await,
            }
        }
    }
    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}
pub(crate) fn tls_acceptor(cert: &[u8], key: &[u8]) -> io::Result<TlsAcceptor> {
    let certs = CertificateDer::pem_slice_iter(cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid())?;
    let key = PrivateKeyDer::from_pem_slice(key).map_err(|_| invalid())?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| invalid())?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
pub fn node_serve(config_path: &Path) -> io::Result<()> {
    let bytes = std::fs::read(config_path)?;
    if bytes.len() > 16 * 1024 {
        return Err(invalid());
    }
    let config: NodeConfig = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let root = config.root.canonicalize()?;
    external_file(config_path, &root, true)?;
    let (node, ticket) = NodeHost::open(config.clone())?;
    external_file(&config.certificate, &root, true)?;
    external_file(&config.private_key, &root, true)?;
    let acceptor = tls_acceptor(
        &std::fs::read(&config.certificate)?,
        &std::fs::read(&config.private_key)?,
    )?;
    tokio::runtime::Runtime::new()?.block_on(async move {
        let tcp=tokio::net::TcpListener::bind(config.bind).await?;
        println!("Node identity: {}\nManifest digest: {}\nTLS address: {}\nOne-use pair ticket (expires in 5 minutes): {}",node.manifest.identity,node.manifest.digest().map_err(|_|invalid())?,tcp.local_addr()?,ticket);
        let stop=CancellationToken::new(); let signal=stop.clone();
        tokio::spawn(async move { let _=tokio::signal::ctrl_c().await; signal.cancel(); });
        axum::serve(TlsListener::new(tcp,acceptor),node.router()).with_graceful_shutdown(stop.cancelled_owned()).await
    })
}
