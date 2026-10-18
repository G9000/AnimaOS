pub(crate) mod lifecycle;
pub(crate) mod persistence;

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use anima_core::{DatabaseAdapter, ModelAdapter};
use async_trait::async_trait;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::{RwLock, Semaphore};

use self::{lifecycle::shutdown_signal, persistence::configure_persistence};
use crate::agent_runs::AgentRunCoordinator;
use crate::connectors::credentials::{
    InMemoryCredentialStore, OsKeyringCredentialStore, TelegramBotToken,
};
use crate::connectors::runtime::{ConnectorManager, TelegramTransport};
use crate::connectors::telegram::{
    TelegramClient, TelegramSentMessage, TelegramTransportError, TelegramUpdateBatch,
};
use crate::connectors::{
    TelegramBotIdentity, TelegramChatKind, TelegramChatMetadata, TelegramSenderMetadata,
};
use crate::events::{EventFanout, DEFAULT_EVENT_BUFFER};
use crate::jobs::JobService;
use crate::model::DeterministicModelAdapter;
use crate::routes;
use crate::runtime_model::RuntimeModelAdapter;
use crate::schedules::SchedulerService;
use crate::state::DaemonState;
use crate::tools::DEFAULT_MAX_BACKGROUND_PROCESSES;

pub(crate) type SharedDaemonState = Arc<RwLock<DaemonState>>;

struct DaemonRuntime {
    voice: crate::voice::VoiceService,
    speech: crate::speech::SpeechService,
    run_limiter: Arc<Semaphore>,
    agent_runs: AgentRunCoordinator,
    connectors: ConnectorManager,
    discord: crate::connectors::discord::DiscordManager,
    drive: crate::connectors::drive::DriveManager,
    calendar: crate::connectors::gcalendar::CalendarManager,
    mail: crate::connectors::mail::MailManager,
    oauth_apps: crate::connectors::oauth_apps::OAuthAppService,
    scheduler: SchedulerService,
    jobs: JobService,
    history: crate::history::HistoryWorker,
}

const DEFAULT_MAX_CONCURRENT_RUNS: usize = 8;
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistenceMode {
    Memory,
    Postgres,
}

impl PersistenceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaemonConfig {
    pub max_request_bytes: usize,
    pub request_timeout: Duration,
    /// Agent/tool runs can legitimately outlast ordinary API requests.
    pub run_request_timeout: Duration,
    pub persistence_mode: PersistenceMode,
    pub max_concurrent_runs: usize,
    /// Concurrent runs of one agent across different conversation rooms
    /// (`ANIMAOS_RS_MAX_RUNS_PER_AGENT`); generated helpers are fixed at 1.
    pub max_runs_per_agent: usize,
    pub max_background_processes: usize,
    /// Postgres connection pool size when `persistence_mode` is `Postgres`.
    /// Should comfortably exceed `max_concurrent_runs` to leave headroom for
    /// background snapshot saves and step-log writes.
    pub db_max_connections: u32,
    /// Capacity of the in-process broadcast channels backing SSE event
    /// streams. Lagged consumers receive a synthetic gap marker rather than
    /// silent drops; this controls the burst buffer before that triggers.
    pub event_buffer: usize,
    /// Events buffered per agent for the live event stream before a slow
    /// subscriber lags (`ANIMAOS_RS_SESSION_EVENT_BUFFER`, spec §6).
    pub session_event_buffer: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            max_request_bytes: 64 * 1024,
            request_timeout: Duration::from_secs(30),
            run_request_timeout: Duration::from_secs(600),
            persistence_mode: PersistenceMode::Memory,
            max_concurrent_runs: DEFAULT_MAX_CONCURRENT_RUNS,
            max_runs_per_agent: crate::runs::DEFAULT_MAX_RUNS_PER_AGENT,
            max_background_processes: DEFAULT_MAX_BACKGROUND_PROCESSES,
            db_max_connections: DEFAULT_DB_MAX_CONNECTIONS,
            event_buffer: DEFAULT_EVENT_BUFFER,
            session_event_buffer: crate::live::DEFAULT_SESSION_EVENT_BUFFER,
        }
    }
}

/// Builds a router wired to a [`DeterministicModelAdapter`] — the in-process
/// mock model that just echoes the input.
///
/// **For tests and library embedding only.** This does NOT perform LLM calls.
/// To run a real daemon that talks to providers, use [`serve`]. See also
/// [`app_with_config`] / [`app_with_database`], which share the same caveat.
pub fn app() -> Router {
    app_with_config(DaemonConfig::default())
}

/// Builds a router with the supplied [`DaemonConfig`].
///
/// **Test/embedding helper** — wires the deterministic mock model adapter.
/// Use [`serve`] for a real daemon.
pub fn app_with_config(config: DaemonConfig) -> Router {
    let state = Arc::new(RwLock::new(configured_state(
        &config,
        Arc::new(DeterministicModelAdapter),
    )));
    app_with_state(state, config)
}

/// Builds a router with a custom database adapter, default config, and the
/// deterministic mock model adapter.
///
/// **Test/embedding helper** — does not run LLM calls. Use [`serve`] for
/// production.
pub fn app_with_database(db: Arc<dyn DatabaseAdapter>) -> Router {
    let config = DaemonConfig::default();
    let mut daemon_state = configured_state(&config, Arc::new(DeterministicModelAdapter));
    daemon_state.set_database(db);
    let state = Arc::new(RwLock::new(daemon_state));
    app_with_state(state, config)
}

/// The daemon state every public constructor and [`serve`] start from, sized
/// by `config`: its event buffers and its background-process limit.
fn configured_state(config: &DaemonConfig, model_adapter: Arc<dyn ModelAdapter>) -> DaemonState {
    let mut state = DaemonState::with_model_adapter_and_events_and_limits(
        model_adapter,
        EventFanout::new(config.event_buffer),
        config.max_background_processes,
    );
    state.set_live_hub(crate::live::LiveHub::new(config.session_event_buffer));
    state
}

pub(crate) fn app_with_state(state: SharedDaemonState, config: DaemonConfig) -> Router {
    let runtime = deterministic_daemon_runtime(Arc::clone(&state), &config);
    app_with_runtime(state, config, runtime)
}

#[cfg(test)]
pub(crate) fn app_with_state_and_voice(
    state: SharedDaemonState,
    config: DaemonConfig,
    voice: crate::voice::VoiceService,
) -> Router {
    let mut runtime = deterministic_daemon_runtime(Arc::clone(&state), &config);
    runtime.voice = voice;
    app_with_runtime(state, config, runtime)
}

#[cfg(test)]
pub(crate) fn app_with_state_and_speech(
    state: SharedDaemonState,
    config: DaemonConfig,
    speech: crate::speech::SpeechService,
) -> Router {
    let mut runtime = deterministic_daemon_runtime(Arc::clone(&state), &config);
    runtime.speech = speech;
    app_with_runtime(state, config, runtime)
}

/// The router `app_with_state` builds, over a prepared runtime.
fn app_with_runtime(
    state: SharedDaemonState,
    config: DaemonConfig,
    runtime: DaemonRuntime,
) -> Router {
    // Construction-time state is uncontended, so this always succeeds in
    // practice; calendar tools simply report "unconfigured" otherwise.
    if let Ok(mut guard) = state.try_write() {
        guard.set_calendar_manager(Some(runtime.calendar.clone()));
        guard.drive_manager = Some(runtime.drive.weak());
    }
    router_with_runtime(
        state,
        config,
        runtime,
        routes::configured_bind_is_loopback(),
    )
}

pub async fn app_with_configured_persistence(config: DaemonConfig) -> io::Result<Router> {
    let state = Arc::new(RwLock::new(configured_state(
        &config,
        Arc::new(DeterministicModelAdapter),
    )));
    configure_persistence(&state, &config).await?;
    state.write().await.chatgpt_auth = crate::chatgpt_auth::ChatGptAuth::new();
    let runtime = daemon_runtime(Arc::clone(&state), &config)?;
    state
        .write()
        .await
        .set_calendar_manager(Some(runtime.calendar.clone()));
    let jobs = runtime.jobs.clone();
    let connectors = runtime.connectors.clone();
    let discord = runtime.discord.clone();
    let drive = runtime.drive.clone();
    let scheduler = runtime.scheduler.clone();
    // Install synchronous services before any producer can contend for state.
    let router = router_with_runtime(state, config, runtime, false);
    jobs.start().await.map_err(|error| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("job recovery failed: {error:?}"),
        )
    })?;
    connectors.start_restored().await;
    discord.start_restored().await;
    drive.start_restored().await;
    scheduler.start().await;
    Ok(router)
}

pub async fn serve(listener: TcpListener, config: DaemonConfig) -> io::Result<()> {
    // Only the real daemon reads explicit operator configuration. Embeddings
    // and deterministic constructors remain independent of ambient config.
    let tool_rpc_config =
        crate::tool_rpc::ToolRpcConfig::parse(std::env::var_os("ANIMAOS_RS_TOOL_RPC_PYTHON_PATH"))
            .map_err(io::Error::other)?;
    let checkpoint_root =
        std::env::var_os("ANIMAOS_RS_CHECKPOINT_ROOT").map(std::path::PathBuf::from);
    if checkpoint_root
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(io::Error::other(
            "Checkpoint root must be an absolute private path outside the workspace",
        ));
    }
    if checkpoint_root.is_some() && !cfg!(windows) {
        return Err(io::Error::other(
            "Native file checkpoints require the supported Windows profile",
        ));
    }
    let browser_setup = match std::env::var_os(crate::browser::config::BROWSER_CONFIG_ENV) {
        Some(path) => Some(
            tokio::task::spawn_blocking(move || {
                crate::browser::config::load_startup_setup(std::path::Path::new(&path))
            })
            .await
            .map_err(|_| io::Error::other("Browser configuration worker unavailable"))?
            .map_err(|_| io::Error::other("Browser configuration unavailable or invalid"))?,
        ),
        None => None,
    };
    let chatgpt_auth = crate::chatgpt_auth::ChatGptAuth::new();
    let state = Arc::new(RwLock::new(configured_state(
        &config,
        Arc::new(
            RuntimeModelAdapter::try_from_env(chatgpt_auth.clone()).map_err(io::Error::other)?,
        ),
    )));

    configure_persistence(&state, &config).await?;

    {
        let mut guard = state.write().await;
        guard.chatgpt_auth = chatgpt_auth;
        guard.browser_setup = browser_setup;
        guard.tool_rpc_service = crate::tool_rpc::ToolRpcService::new(tool_rpc_config);
        guard.checkpoint_root = checkpoint_root;
        // AI titles for new chats (spec §12.3): only the real daemon turns
        // them on, so test and embedding states never see an unscripted
        // model call.
        guard.set_generated_titles(true);
    }
    serve_with_state_and_voice(
        listener,
        state,
        config,
        shutdown_signal(),
        crate::runtime_model::transcription_service_from_env(),
        crate::runtime_model::speech_service_from_env(),
    )
    .await
}

/// Serves until `shutdown` resolves (Ctrl+C for [`serve`]), then shuts down
/// gracefully.
#[cfg(test)]
pub(crate) async fn serve_with_state(
    listener: TcpListener,
    state: SharedDaemonState,
    config: DaemonConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    serve_with_state_and_voice(
        listener,
        state,
        config,
        shutdown,
        crate::voice::VoiceService::disabled(),
        crate::speech::SpeechService::disabled(),
    )
    .await
}

async fn serve_with_state_and_voice(
    listener: TcpListener,
    state: SharedDaemonState,
    config: DaemonConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
    voice: crate::voice::VoiceService,
    speech: crate::speech::SpeechService,
) -> io::Result<()> {
    let bind_is_loopback = listener.local_addr()?.ip().is_loopback();
    let mut runtime = daemon_runtime(Arc::clone(&state), &config)?;
    runtime.voice = voice;
    runtime.speech = speech;
    state
        .write()
        .await
        .set_calendar_manager(Some(runtime.calendar.clone()));
    let connectors = runtime.connectors.clone();
    let discord = runtime.discord.clone();
    let drive = runtime.drive.clone();
    let scheduler = runtime.scheduler.clone();
    let agent_runs = runtime.agent_runs.clone();
    let jobs = runtime.jobs.clone();
    let history = runtime.history.clone();
    let skills = crate::skills::SkillScanner::new(runtime.agent_runs.skills());
    let live = state.read().await.live.clone();
    let browser_state = state.clone();
    let router = router_with_runtime(state, config, runtime, bind_is_loopback);
    // Router installation uses uncontended construction-time state. Restored
    // workers and the scanner may acquire it immediately after being started.
    jobs.start().await.map_err(|error| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("job recovery failed: {error:?}"),
        )
    })?;
    connectors.start_restored().await;
    discord.start_restored().await;
    drive.start_restored().await;
    scheduler.start().await;
    // Spec §8.1: the startup rescan, then one every 60 seconds.
    skills.start();
    let shutdown_live = live.clone();
    let drain_expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let workflow_expired = Arc::clone(&drain_expired);
    let (started, deadline_started) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        shutdown.await;
        let deadline = tokio::time::Instant::now() + lifecycle::SHUTDOWN_BUDGET;
        let _ = started.send(deadline);
        // All closes, admitted work, history and HTTP completion share this deadline.
        let drained = lifecycle::run_shutdown_until(
            tokio::time::sleep_until(deadline),
            lifecycle::producer_first_shutdown(
                async {
                    agent_runs.request_shutdown();
                    jobs.request_shutdown().await;
                    scheduler.request_shutdown().await;
                    skills.request_shutdown();
                    connectors.request_shutdown().await;
                    discord.request_shutdown().await;
                },
                async {
                    agent_runs.shutdown().await;
                    let browser = browser_state.read().await.browser_service.clone();
                    if let Some(browser) = browser {
                        browser.shutdown().await;
                    }
                    jobs.shutdown().await;
                    scheduler.shutdown().await;
                    skills.shutdown().await;
                    connectors.shutdown().await;
                    discord.shutdown().await;
                    history.shutdown().await;
                },
            ),
        )
        .await;
        workflow_expired.store(!drained, std::sync::atomic::Ordering::SeqCst);
        shutdown_live.close();
    });
    let outcome = lifecycle::run_server_until_shutdown(async { server.await }, async {
        let deadline = match deadline_started.await {
            Ok(deadline) => deadline,
            Err(_) => std::future::pending().await,
        };
        tokio::time::sleep_until(deadline)
    })
    .await;
    if outcome.is_none() || drain_expired.load(std::sync::atomic::Ordering::SeqCst) {
        tracing::warn!("shutdown budget expired; admitted work may have uncertain effects and requires restart review");
        live.close();
    }
    outcome.unwrap_or(Ok(()))
}

fn daemon_runtime(state: SharedDaemonState, config: &DaemonConfig) -> io::Result<DaemonRuntime> {
    let public_origin = match std::env::var("ANIMA_PUBLIC_BASE_URL") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ANIMA_PUBLIC_BASE_URL must be valid Unicode",
            ))
        }
    };
    let oauth_apps =
        crate::connectors::oauth_apps::OAuthAppService::new_for_origin(public_origin.as_deref())?;
    let run_limiter = Arc::new(Semaphore::new(config.max_concurrent_runs));
    let agent_runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::clone(&run_limiter))
        .with_max_runs_per_agent(config.max_runs_per_agent);
    let agent_runs = coordinator_with_mcp_credentials(
        &state,
        agent_runs,
        Arc::new(crate::mcp::credentials::OsKeyringMcpCredentialStore::new()),
    );
    let transport = TelegramClient::new()
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
    let transport: Arc<dyn TelegramTransport> = Arc::new(transport);
    let connectors = ConnectorManager::new(
        Arc::clone(&state),
        agent_runs.clone(),
        Arc::new(OsKeyringCredentialStore::new()),
        Arc::clone(&transport),
    );
    let discord_transport =
        crate::connectors::discord::client::DiscordClient::new().map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "Discord transport configuration failed",
            )
        })?;
    let discord = crate::connectors::discord::DiscordManager::new(
        Arc::clone(&state),
        agent_runs.clone(),
        Arc::new(crate::connectors::discord::credentials::OsKeyringDiscordCredentialStore::new()),
        Arc::new(
            crate::connectors::registry::ConnectorRegistry::with_discord(
                transport,
                Arc::new(discord_transport),
            ),
        ),
    );
    let discord = discord.with_gateway(Arc::new(
        crate::connectors::discord::gateway::NativeGatewayFactory,
    ));
    let google_transport = crate::connectors::gcalendar::client::GoogleCalendarClient::new()
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
    let calendar = crate::connectors::gcalendar::CalendarManager::new(
        &state,
        agent_runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::OsKeyringGoogleCredentialStore::new()),
        Arc::new(google_transport),
        oauth_apps.clone(),
    );
    let mail = crate::connectors::mail::MailManager::new(
        &state,
        agent_runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::OsKeyringGoogleCredentialStore::new()),
        Arc::new(crate::connectors::mail::client::MailClient::new()),
        oauth_apps.clone(),
    );
    let drive = crate::connectors::drive::DriveManager::new(
        &state,
        agent_runs.control_plane_transactions(),
        Arc::new(crate::connectors::drive::store::OsKeyringDriveCredentialStore::new()),
        Arc::new(
            crate::connectors::drive::client::DriveClient::new()
                .map_err(|_| io::Error::other("Drive configuration failed"))?,
        ),
        oauth_apps.clone(),
    );
    let jobs = JobService::new(Arc::clone(&state), agent_runs.clone());
    let history = crate::history::HistoryWorker::new(
        Arc::clone(&state),
        agent_runs.control_plane_transactions(),
    );
    let scheduler = SchedulerService::new(state, agent_runs.clone(), connectors.clone());
    Ok(DaemonRuntime {
        voice: crate::voice::VoiceService::disabled(),
        speech: crate::speech::SpeechService::disabled(),
        run_limiter,
        agent_runs,
        connectors,
        discord,
        drive,
        calendar,
        mail,
        oauth_apps,
        scheduler,
        jobs,
        history,
    })
}

fn deterministic_daemon_runtime(state: SharedDaemonState, config: &DaemonConfig) -> DaemonRuntime {
    deterministic_daemon_runtime_with_mail_transport(
        state,
        config,
        Arc::new(DeterministicMailTransport),
    )
}

pub(crate) fn coordinator_with_mcp(
    state: &SharedDaemonState,
    coordinator: AgentRunCoordinator,
) -> AgentRunCoordinator {
    coordinator_with_mcp_credentials(
        state,
        coordinator,
        Arc::new(crate::mcp::credentials::InMemoryMcpCredentialStore::default()),
    )
}
fn coordinator_with_mcp_credentials(
    state: &SharedDaemonState,
    coordinator: AgentRunCoordinator,
    credentials: Arc<dyn crate::mcp::credentials::McpCredentialStore>,
) -> AgentRunCoordinator {
    let mut guard = state
        .try_write()
        .expect("runtime construction requires an uncontended daemon state");
    let manager = guard.mcp_manager.clone().unwrap_or_else(|| {
        crate::mcp::McpManager::with_credentials(
            Arc::downgrade(state),
            coordinator.control_plane_transactions(),
            Arc::clone(&guard.mcp_publication),
            Arc::new(crate::mcp::transport::DefaultMcpTransportFactory::default()),
            credentials,
        )
    });
    guard.mcp_manager = Some(manager.clone());
    drop(guard);
    coordinator.with_mcp(manager)
}

fn deterministic_daemon_runtime_with_mail_transport(
    state: SharedDaemonState,
    config: &DaemonConfig,
    mail_transport: Arc<dyn crate::connectors::mail::client::MailTransport>,
) -> DaemonRuntime {
    let run_limiter = Arc::new(Semaphore::new(config.max_concurrent_runs));
    let agent_runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::clone(&run_limiter))
        .with_max_runs_per_agent(config.max_runs_per_agent);
    let agent_runs = coordinator_with_mcp(&state, agent_runs);
    let connectors = ConnectorManager::new(
        Arc::clone(&state),
        agent_runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(DeterministicTelegramTransport),
    );
    let discord = crate::connectors::discord::DiscordManager::unconfigured(
        Arc::clone(&state),
        agent_runs.clone(),
    );
    let oauth_apps = crate::connectors::oauth_apps::OAuthAppService::in_memory();
    let calendar = crate::connectors::gcalendar::CalendarManager::new(
        &state,
        agent_runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(crate::connectors::gcalendar::client::UnconfiguredGoogleTransport),
        oauth_apps.clone(),
    );
    let mail = crate::connectors::mail::MailManager::new(
        &state,
        agent_runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        mail_transport,
        oauth_apps.clone(),
    );
    let drive = crate::connectors::drive::DriveManager::unconfigured(
        &state,
        agent_runs.control_plane_transactions(),
        oauth_apps.clone(),
    );
    let jobs = JobService::new(Arc::clone(&state), agent_runs.clone());
    let history = crate::history::HistoryWorker::new(
        Arc::clone(&state),
        agent_runs.control_plane_transactions(),
    );
    let scheduler = SchedulerService::new(state, agent_runs.clone(), connectors.clone());
    DaemonRuntime {
        voice: crate::voice::VoiceService::disabled(),
        speech: crate::speech::SpeechService::disabled(),
        run_limiter,
        agent_runs,
        connectors,
        discord,
        drive,
        calendar,
        mail,
        oauth_apps,
        scheduler,
        jobs,
        history,
    }
}

/// Builds the router and starts the history loop it owns, when a Tokio
/// runtime is present. The loop runs while the router or one of its clones
/// lives, or until `HistoryWorker::shutdown`. It starts after construction,
/// which relies on `try_write` finding the state uncontended.
fn router_with_runtime(
    state: SharedDaemonState,
    config: DaemonConfig,
    runtime: DaemonRuntime,
    bind_is_loopback: bool,
) -> Router {
    let history = runtime.history;
    let history_owner = crate::history::HistoryWorkerOwner::new();
    let router = routes::router_with_all_integrations(
        state,
        config,
        runtime.run_limiter,
        runtime.agent_runs,
        runtime.connectors,
        runtime.calendar,
        runtime.mail,
        runtime.oauth_apps,
        runtime.scheduler,
        runtime.jobs,
        history_owner.clone(),
        crate::logs::global(),
        bind_is_loopback,
        runtime.discord,
        runtime.drive,
        runtime.voice,
        runtime.speech,
    );
    if tokio::runtime::Handle::try_current().is_ok() {
        history.start(&history_owner);
    }
    router
}

/// Deterministic Telegram boundary paired with the deterministic model in the
/// public test/embedding router helpers. Production `serve` always uses the
/// fixed-origin `TelegramClient` and OS credential vault.
pub(crate) struct DeterministicTelegramTransport;

pub(crate) struct DeterministicMailTransport;

#[async_trait]
impl crate::connectors::mail::client::MailTransport for DeterministicMailTransport {
    async fn exchange(
        &self,
        _config: &crate::connectors::mail::OAuthConfig,
        _code: &str,
        _verifier: &str,
    ) -> Result<
        crate::connectors::gcalendar::store::GoogleOAuthTokens,
        crate::connectors::mail::MailError,
    > {
        Err(crate::connectors::mail::MailError::Unconfigured)
    }

    async fn refresh(
        &self,
        _config: &crate::connectors::mail::OAuthConfig,
        _refresh: &str,
    ) -> Result<
        crate::connectors::gcalendar::store::GoogleOAuthTokens,
        crate::connectors::mail::MailError,
    > {
        Err(crate::connectors::mail::MailError::Unconfigured)
    }

    async fn account(
        &self,
        _provider: crate::connectors::mail::Provider,
        _access: &str,
    ) -> Result<String, crate::connectors::mail::MailError> {
        Err(crate::connectors::mail::MailError::Unconfigured)
    }

    async fn inbox(
        &self,
        _provider: crate::connectors::mail::Provider,
        _access: &str,
    ) -> Result<Vec<crate::connectors::mail::MailMessage>, crate::connectors::mail::MailError> {
        Err(crate::connectors::mail::MailError::Unconfigured)
    }

    async fn send(
        &self,
        _provider: crate::connectors::mail::Provider,
        _access: &str,
        _draft: &crate::connectors::mail::MailDraft,
    ) -> Result<(), crate::connectors::mail::MailError> {
        Err(crate::connectors::mail::MailError::Unconfigured)
    }
}

#[async_trait]
impl TelegramTransport for DeterministicTelegramTransport {
    async fn get_me(
        &self,
        _token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        Ok(TelegramBotIdentity {
            id: "900001".to_string(),
            username: Some("anima_test_bot".to_string()),
            display_name: Some("Anima Test Bot".to_string()),
        })
    }

    async fn get_updates(
        &self,
        _token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        if offset <= 1 {
            return Ok(TelegramUpdateBatch {
                updates: vec![crate::connectors::telegram::TelegramTextUpdate {
                    update_id: 1,
                    text: "pair deterministic Telegram chat".to_string(),
                    sender: TelegramSenderMetadata {
                        id: "700001".to_string(),
                        username: Some("local_owner".to_string()),
                        display_name: Some("Local Owner".to_string()),
                    },
                    chat: deterministic_chat(),
                }],
                next_update_id: 2,
            });
        }
        Ok(TelegramUpdateBatch {
            updates: Vec::new(),
            next_update_id: offset,
        })
    }

    async fn send_message(
        &self,
        _token: &TelegramBotToken,
        _chat_id: &str,
        _text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        Ok(vec![TelegramSentMessage {
            message_id: "800001".to_string(),
            chat: deterministic_chat(),
        }])
    }
}

fn deterministic_chat() -> TelegramChatMetadata {
    TelegramChatMetadata {
        id: "424242".to_string(),
        kind: TelegramChatKind::Private,
        title: None,
        username: Some("local_owner".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::gcalendar::store::GoogleOAuthTokens;
    use crate::connectors::mail::client::MailTransport;
    use crate::connectors::mail::{MailDraft, MailError, MailMessage, OAuthConfig, Provider};
    use crate::connectors::oauth_apps::{OAuthAppCredentials, OAuthProvider};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zeroize::Zeroizing;

    struct CountingMailTransport(Arc<AtomicUsize>);

    #[async_trait]
    impl MailTransport for CountingMailTransport {
        async fn exchange(
            &self,
            _config: &OAuthConfig,
            _code: &str,
            _verifier: &str,
        ) -> Result<GoogleOAuthTokens, MailError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(GoogleOAuthTokens::new(
                Zeroizing::new("access".to_string()),
                Zeroizing::new("refresh".to_string()),
                crate::connectors::gcalendar::now_ms() + 3_600_000,
            ))
        }

        async fn refresh(
            &self,
            _config: &OAuthConfig,
            _refresh: &str,
        ) -> Result<GoogleOAuthTokens, MailError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(MailError::Unconfigured)
        }

        async fn account(&self, _provider: Provider, _access: &str) -> Result<String, MailError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok("owner@example.com".to_string())
        }

        async fn inbox(
            &self,
            _provider: Provider,
            _access: &str,
        ) -> Result<Vec<MailMessage>, MailError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(MailError::Unconfigured)
        }

        async fn send(
            &self,
            _provider: Provider,
            _access: &str,
            _draft: &MailDraft,
        ) -> Result<(), MailError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(MailError::Unconfigured)
        }
    }

    #[tokio::test]
    async fn deterministic_runtime_routes_mail_through_its_injected_boundary() {
        let state = Arc::new(RwLock::new(DaemonState::new()));
        let agent_id = state
            .write()
            .await
            .create_agent(anima_core::AgentConfig {
                name: "mail-runtime".to_string(),
                model: "deterministic".to_string(),
                provider: None,
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
            })
            .unwrap()
            .state
            .id;
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = deterministic_daemon_runtime_with_mail_transport(
            Arc::clone(&state),
            &DaemonConfig::default(),
            Arc::new(CountingMailTransport(Arc::clone(&calls))),
        );
        runtime
            .oauth_apps
            .put(
                OAuthProvider::Google,
                OAuthAppCredentials::new(OAuthProvider::Google, "client", "secret", None).unwrap(),
            )
            .await
            .unwrap();

        let (_, consent_url) = runtime
            .mail
            .begin_connect(&agent_id, Provider::Gmail)
            .await
            .unwrap();
        let nonce = reqwest::Url::parse(&consent_url)
            .unwrap()
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        runtime
            .mail
            .complete_connect(Provider::Gmail, &nonce, "code")
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// Waits until the store holds every id, failing after `within`.
    async fn wait_until_mirrored(
        store: &dyn crate::history::HistoryStore,
        ids: &[String],
        within: Duration,
    ) {
        let deadline = tokio::time::Instant::now() + within;
        while store.existing_message_ids(ids).await.unwrap().len() < ids.len() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "{ids:?} were not mirrored within {within:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn app_with_state_routers_keep_mirroring_committed_messages() {
        use crate::history::{HistoryService, MemoryHistoryStore, HISTORY_FLUSH_INTERVAL};
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::util::ServiceExt;

        let store = Arc::new(MemoryHistoryStore::new());
        let mut daemon = DaemonState::new();
        daemon.set_history(HistoryService::new(store.clone()));
        let agent_id = daemon
            .create_agent(anima_core::AgentConfig {
                name: "historian".to_string(),
                model: "deterministic".to_string(),
                provider: None,
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
            })
            .unwrap()
            .state
            .id;
        let state = Arc::new(RwLock::new(daemon));
        let router = app_with_state(Arc::clone(&state), DaemonConfig::default());

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/agents/{agent_id}/run"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hello"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let committed = state
            .read()
            .await
            .get_agent(&agent_id)
            .unwrap()
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(committed.len(), 2);
        wait_until_mirrored(&*store, &committed, HISTORY_FLUSH_INTERVAL).await;
        drop(router);
    }

    #[tokio::test]
    async fn dropping_an_app_with_state_router_stops_its_history_loop() {
        use crate::history::{
            HistoryService, HistoryStore, MemoryHistoryStore, HISTORY_FLUSH_INTERVAL,
        };

        let store = Arc::new(MemoryHistoryStore::new());
        let mut daemon = DaemonState::new();
        daemon.set_history(HistoryService::new(store.clone()));
        let state = Arc::new(RwLock::new(daemon));
        let config = DaemonConfig::default();
        let runtime = deterministic_daemon_runtime(Arc::clone(&state), &config);
        // A handle does not keep the loop running; the router does.
        let worker = runtime.history.clone();
        let router = app_with_runtime(Arc::clone(&state), config, runtime);

        let message = |id: &str| {
            crate::history::conformance::history_message(
                id,
                "agent-1",
                "chat:one",
                anima_core::MessageRole::User,
                "hello",
                1,
            )
            .message
        };
        let history = state.read().await.history.clone();
        history.enqueue_committed("agent-1", "chat:one", &[message("msg-1-1")]);
        wait_until_mirrored(&*store, &["msg-1-1".to_string()], HISTORY_FLUSH_INTERVAL).await;
        assert!(
            !worker.has_stopped(),
            "the loop runs while its router lives"
        );

        drop(router);
        let deadline = tokio::time::Instant::now() + HISTORY_FLUSH_INTERVAL;
        while !worker.has_stopped() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the loop keeps running after its router is gone"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        history.enqueue_committed("agent-1", "chat:one", &[message("msg-2-2")]);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            store
                .existing_message_ids(&["msg-2-2".to_string()])
                .await
                .unwrap()
                .is_empty(),
            "a stopped loop writes nothing more"
        );
    }

    #[tokio::test]
    async fn the_configured_session_event_buffer_sizes_the_live_hub() {
        use crate::live::{LiveDelivery, LiveEvent, LiveEventBody};

        let config = DaemonConfig {
            session_event_buffer: 2,
            ..DaemonConfig::default()
        };
        // `app_with_config`, `app_with_database`,
        // `app_with_configured_persistence`, and `serve` all build it here.
        let hub = configured_state(&config, Arc::new(DeterministicModelAdapter)).live;
        let mut subscription = hub.subscribe("agent-1").unwrap();
        for n in 0..5u64 {
            hub.publish(
                LiveEvent::new(
                    "agent-1",
                    LiveEventBody::StepDelta {
                        step_id: "run_1:1".into(),
                        offset: n,
                        text: "x".into(),
                    },
                ),
                None,
            );
        }
        assert!(matches!(
            subscription.next().await,
            Some(LiveDelivery::Lagged(3))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn graceful_shutdown_ends_an_open_event_stream_and_serve_returns() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let state = Arc::new(RwLock::new(DaemonState::new()));
        let agent_id = state
            .write()
            .await
            .create_agent(anima_core::AgentConfig {
                name: "companion".to_string(),
                model: "deterministic".to_string(),
                provider: None,
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
            })
            .unwrap()
            .state
            .id;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (trigger, triggered) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_with_state(
            listener,
            state,
            DaemonConfig::default(),
            async move {
                let _ = triggered.await;
            },
        ));

        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET /api/agents/{agent_id}/events HTTP/1.1\r\nHost: {address}\r\nOrigin: http://localhost:4200\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut received = String::new();
        let mut buffer = [0; 4096];
        while !received.contains("event: stream.snapshot") {
            let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
                .await
                .expect("the snapshot arrives")
                .unwrap();
            assert!(
                read > 0,
                "the stream closed before its snapshot: {received}"
            );
            received.push_str(&String::from_utf8_lossy(&buffer[..read]));
        }
        assert!(received.starts_with("HTTP/1.1 200 OK"), "{received}");

        trigger.send(()).unwrap();

        loop {
            let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
                .await
                .expect("the open stream ends once shutdown starts")
                .unwrap();
            if read == 0 {
                break;
            }
        }
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("serve returns once its streams ended")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn per_agent_run_limit_defaults_to_three_and_reaches_the_coordinator() {
        assert_eq!(DaemonConfig::default().max_runs_per_agent, 3);
        let state = Arc::new(RwLock::new(DaemonState::new()));

        let runtime = deterministic_daemon_runtime(
            state,
            &DaemonConfig {
                max_runs_per_agent: 2,
                ..DaemonConfig::default()
            },
        );

        assert_eq!(runtime.agent_runs.max_runs_per_agent(), 2);
    }
}
