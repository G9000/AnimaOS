use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anima_memory::{MemoryManager, MemoryType, RecentMemoryOptions};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::{info, warn};

use super::{DaemonConfig, PersistenceMode, SharedDaemonState};
use crate::control_plane_store::{
    load_control_plane_snapshot, write_pre_upgrade_backup, ControlPlaneStoreConfig,
    CONTROL_PLANE_STORE_VERSION,
};
use crate::history::{
    HistoryService, HistoryStore, MemoryHistoryStore, PostgresHistoryStore, SqliteHistoryStore,
};
use crate::memory_embeddings::{spawn_model_load, MemoryEmbeddingRuntime};
use crate::memory_store::{load_memory_snapshot, MemoryMutation, MemoryStoreConfig};
use crate::postgres::SqlxPostgresAdapter;
use crate::profile_owner::{ProfileOwner, ProfileResource};
use crate::state::{memory_query_expander_from_env, memory_text_analyzer_from_env};

/// Resolve every profile payload before opening any of them. The retained
/// handle is acquired before model/auth/browser setup in the production host.
pub(crate) struct PersistencePreflight {
    owner: ProfileOwner,
    memory: Option<MemoryStoreChoice>,
    control_plane: Option<PathBuf>,
    history: Option<PathBuf>,
}

pub(crate) fn prepare_persistence(config: &DaemonConfig) -> io::Result<PersistencePreflight> {
    prepare_persistence_with_durable(config, None)
}
pub(crate) fn prepare_persistence_with_durable(
    config: &DaemonConfig,
    durable: Option<&crate::engine_bridge::startup::StartupConfig>,
) -> io::Result<PersistencePreflight> {
    let postgres = matches!(config.persistence_mode, PersistenceMode::Postgres);
    let control_plane = non_empty_env_path("ANIMAOS_RS_CONTROL_PLANE_FILE")?;
    let memory = memory_store_choice(
        non_empty_env_path("ANIMAOS_RS_MEMORY_FILE")?,
        non_empty_env_path("ANIMAOS_RS_MEMORY_SQLITE_FILE")?,
        postgres,
        control_plane.as_deref(),
    )?;
    let history = non_empty_env_path(HISTORY_SQLITE_FILE_ENV)?
        .or_else(|| control_plane.as_deref().map(default_history_sqlite_path));
    let embedding = non_empty_env_path("ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE")?;
    let checkpoint = non_empty_env_path("ANIMAOS_RS_CHECKPOINT_ROOT")?;
    let canonical_control_plane = control_plane
        .as_ref()
        .map(|path| ProfileOwner::resolve_path(path))
        .transpose()?;
    let explicit_tool_root = non_empty_env_path("ANIMAOS_RS_TOOL_RESULTS_DIR")?;
    let mut resources = typed_profile_resources(
        canonical_control_plane.as_deref(),
        memory.as_ref(),
        postgres,
        history.as_deref(),
        embedding.as_deref(),
        checkpoint.as_deref(),
        explicit_tool_root.as_deref(),
    );
    if let Some(startup) = durable {
        let additions = startup.resources();
        for added in &additions {
            let added = match added {
                ProfileResource::File(p)
                | ProfileResource::Sqlite(p)
                | ProfileResource::Directory(p) => ProfileOwner::resolve_path(p)?,
            };
            for existing in &resources {
                let existing = match existing {
                    ProfileResource::File(p)
                    | ProfileResource::Sqlite(p)
                    | ProfileResource::Directory(p) => ProfileOwner::resolve_path(p)?,
                };
                if added.starts_with(&existing) || existing.starts_with(&added) {
                    return Err(io::Error::other(
                        "Durable profile resources overlap existing profile storage",
                    ));
                }
            }
        }
        resources.extend(additions);
    }
    let owner = ProfileOwner::acquire_resources(resources)?;
    Ok(PersistencePreflight {
        owner,
        memory,
        control_plane,
        history,
    })
}

pub(crate) async fn configure_persistence(
    state: &SharedDaemonState,
    config: &DaemonConfig,
    preflight: PersistencePreflight,
) -> io::Result<()> {
    let PersistencePreflight {
        owner,
        memory,
        control_plane,
        history,
    } = preflight;
    let postgres_pool = configure_database(state, config, &owner).await?;
    let memory_store = memory
        .and_then(|choice| match choice {
            MemoryStoreChoice::Json(path) => Some(MemoryStoreConfig::Json(path)),
            MemoryStoreChoice::Sqlite(path) => Some(MemoryStoreConfig::Sqlite(path)),
            MemoryStoreChoice::Postgres => postgres_pool.clone().map(MemoryStoreConfig::Postgres),
        })
        .map(|store| store.with_owner(owner.clone()))
        .transpose()?;
    let control_plane_store = control_plane
        .map(ControlPlaneStoreConfig::Json)
        .or_else(|| postgres_pool.clone().map(ControlPlaneStoreConfig::Postgres))
        .map(|store| store.with_owner(owner.clone()))
        .transpose()?;
    state.write().await.profile_owner = Some(owner.clone());
    let memory_store = configure_memory_store(state, memory_store).await?;
    configure_memory_embeddings(state, memory_store.as_ref(), owner).await?;
    configure_control_plane_and_history(state, control_plane_store, history).await?;
    write_core_memory_copies(state).await?;
    Ok(())
}

async fn configure_database(
    state: &SharedDaemonState,
    config: &DaemonConfig,
    owner: &ProfileOwner,
) -> io::Result<Option<PgPool>> {
    match config.persistence_mode {
        PersistenceMode::Memory => {
            if std::env::var_os("DATABASE_URL").is_some() {
                warn!(
                    "DATABASE_URL is set but ANIMAOS_RS_PERSISTENCE_MODE=memory; starting without Postgres persistence"
                );
            } else {
                info!("starting in memory persistence mode");
            }
            Ok(None)
        }
        PersistenceMode::Postgres => {
            let database_url = std::env::var("DATABASE_URL").map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "DATABASE_URL must be set when ANIMAOS_RS_PERSISTENCE_MODE=postgres",
                )
            })?;
            let pool = PgPoolOptions::new()
                .max_connections(config.db_max_connections)
                .connect(&database_url)
                .await
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("failed to connect to Postgres: {error}"),
                    )
                })?;

            owner.attach_postgres(&pool).await?;
            let mut writer = owner.postgres_writer().await?;
            sqlx::migrate!("./migrations")
                .run_direct(&mut *writer)
                .await
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("failed to run Postgres migrations: {error}"),
                    )
                })?;

            writer.complete()?;
            drop(writer);
            let adapter = Arc::new(SqlxPostgresAdapter::new_owned(pool.clone(), owner.clone()));
            state.write().await.set_database(adapter);
            info!("Postgres connected, migrations applied");
            Ok(Some(pool))
        }
    }
}

async fn configure_control_plane_store(
    state: &SharedDaemonState,
    config: Option<ControlPlaneStoreConfig>,
) -> io::Result<()> {
    let Some(config) = config else {
        state.write().await.set_control_plane_store(None);
        return Ok(());
    };

    if let Some(path) = config.file_path() {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    let snapshot = load_control_plane_snapshot(&config).await?;
    if let Some(loaded) = &snapshot {
        if loaded.version < CONTROL_PLANE_STORE_VERSION {
            // Spec §13.3 step 1: back up the untouched snapshot before anything
            // is written in the new version.
            let backup = write_pre_upgrade_backup(&config, loaded.version).await?;
            info!(
                backup = %backup,
                from_version = loaded.version,
                to_version = CONTROL_PLANE_STORE_VERSION,
                "saved the pre-upgrade control-plane backup"
            );
        }
    }
    let (restored_agents, restored_swarms, legacy_pins) = if let Some(mut snapshot) = snapshot {
        let legacy_pins = std::mem::take(&mut snapshot.legacy_pins);
        let (agents, swarms) = state
            .write()
            .await
            .restore_control_plane_snapshot(snapshot)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        (agents, swarms, legacy_pins)
    } else {
        (0, 0, Vec::new())
    };
    // Before the save below, which drops the pins from the control plane.
    if !legacy_pins.is_empty() {
        migrate_legacy_pins(state, &legacy_pins).await?;
    }

    {
        let mut guard = state.write().await;
        // Spec §13.3 step 5: tools added since these agents were created.
        let granted = guard.apply_pending_tool_grants(crate::sessions::migration::TOOL_GRANTS);
        if !granted.is_empty() {
            info!(
                agents = granted.len(),
                "granted newly added tools to existing agents"
            );
        }
        guard.set_control_plane_store(Some(config.clone()));
        guard.control_plane_persist_request()
    }
    .save()
    .await?;

    info!(
        control_plane_store = %config.location_label(),
        storage = config.storage_label(),
        restored_agents,
        restored_swarms,
        "runtime control plane store configured"
    );
    Ok(())
}

/// Folds the memory-view pins of an older control plane into core memory and
/// persists the memory store. A failed save refuses boot with the control
/// plane still unsaved, so the pins are kept and the next boot retries.
async fn migrate_legacy_pins(
    state: &SharedDaemonState,
    pins: &[crate::core_memory::migration::LegacyPins],
) -> io::Result<()> {
    let (memory, memory_store, live_agents) = {
        let guard = state.read().await;
        (
            guard.memory_handle(),
            guard.memory_store_config(),
            guard.agents.keys().cloned().collect(),
        )
    };
    let mut manager = memory.write().await;
    let mut mutation = MemoryMutation::new(&mut manager);
    let report = crate::core_memory::migration::migrate_pins(&mut mutation, pins, &live_agents);
    for skipped in &report.skipped {
        info!(
            agent_id = %skipped.agent_id,
            memory_id = %skipped.memory_id,
            reason = %skipped.reason,
            "left a memory pin out of core memory"
        );
    }
    if !report.moved.is_empty() {
        mutation
            .persist(memory_store.as_ref())
            .await
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("could not save memory pins moved into core memory: {error}"),
                )
            })?;
    }
    info!(
        moved = report.moved.len(),
        skipped = report.skipped.len(),
        "moved memory pins into core memory"
    );
    Ok(())
}

/// Writes every agent's core memory file copy once at boot.
async fn write_core_memory_copies(state: &SharedDaemonState) -> io::Result<()> {
    let (memory, workspace_root, agents, owner) = {
        let guard = state.read().await;
        (
            guard.memory_handle(),
            guard
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone()),
            guard.agents.keys().cloned().collect::<Vec<_>>(),
            guard.profile_owner(),
        )
    };
    if let Some(owner) = &owner {
        owner.verify_owned().await?;
    }
    let manager = memory.read().await;
    for agent_id in agents {
        if let Some(owner) = &owner {
            owner.ensure_owned()?;
        }
        crate::core_memory::files::write_copies(workspace_root.as_deref(), &manager, &agent_id);
    }
    Ok(())
}

/// The SQLite history file (spec §13.1); defaults beside the control plane.
pub(crate) const HISTORY_SQLITE_FILE_ENV: &str = "ANIMAOS_RS_HISTORY_SQLITE_FILE";

/// Opens the history store before the control plane is loaded or saved: a
/// history file that cannot be opened refuses boot while the snapshot is still
/// untouched, so an older daemon can start on it (spec §16).
async fn configure_control_plane_and_history(
    state: &SharedDaemonState,
    control_plane: Option<ControlPlaneStoreConfig>,
    history_sqlite: Option<PathBuf>,
) -> io::Result<()> {
    let store = history_store_for(control_plane.as_ref(), history_sqlite).await?;
    configure_control_plane_store(state, control_plane).await?;
    let label = store.label();
    state.write().await.set_history(HistoryService::new(store));
    info!(history_store = label, "runtime history store configured");
    Ok(())
}

/// The history store that goes with the control-plane store: SQLite beside a
/// JSON control plane (or at the explicit path), Postgres tables in Postgres
/// mode, and bounded memory tables in ephemeral mode.
pub(crate) async fn history_store_for(
    control_plane: Option<&ControlPlaneStoreConfig>,
    sqlite_override: Option<PathBuf>,
) -> io::Result<Arc<dyn HistoryStore>> {
    let owner = control_plane
        .and_then(ControlPlaneStoreConfig::owner)
        .cloned();
    let sqlite_path = match (
        control_plane.map(ControlPlaneStoreConfig::store),
        sqlite_override,
    ) {
        (None, Some(_)) => {
            warn!("ANIMAOS_RS_HISTORY_SQLITE_FILE is ignored without a durable control plane; history stays in memory");
            return Ok(Arc::new(MemoryHistoryStore::new()));
        }
        (None, None) => return Ok(Arc::new(MemoryHistoryStore::new())),
        (Some(_), Some(path)) => path,
        (Some(ControlPlaneStoreConfig::Owned { .. }), _) => unreachable!(),
        (Some(ControlPlaneStoreConfig::Json(file)), None) => default_history_sqlite_path(file),
        (Some(ControlPlaneStoreConfig::Postgres(pool)), None) => {
            return Ok(Arc::new(match owner {
                Some(owner) => PostgresHistoryStore::new_owned(pool.clone(), owner),
                None => PostgresHistoryStore::new(pool.clone()),
            }))
        }
    };
    let sqlite_path = owner
        .as_ref()
        .map(|owner| owner.owned_sqlite_path(&sqlite_path))
        .transpose()?
        .unwrap_or(sqlite_path);
    let store = SqliteHistoryStore::open_with_owner(sqlite_path, owner)
        .await
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("failed to open the history store: {error}"),
            )
        })?;
    Ok(Arc::new(store))
}

pub(crate) fn default_history_sqlite_path(control_plane_file: &Path) -> PathBuf {
    control_plane_file.with_file_name("history.sqlite")
}

async fn configure_memory_store(
    state: &SharedDaemonState,
    config: Option<MemoryStoreConfig>,
) -> io::Result<Option<MemoryStoreConfig>> {
    let Some(config) = config else {
        state.write().await.set_memory_store(None);
        return Ok(None);
    };

    if let Some(path) = config.file_path() {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    let query_expander = memory_query_expander_from_env();
    let text_analyzer = memory_text_analyzer_from_env();
    let mut manager = match query_expander {
        Some(query_expander) => {
            MemoryManager::with_text_analyzer_and_query_expander(text_analyzer, query_expander)
        }
        None => MemoryManager::with_text_analyzer(text_analyzer),
    };
    if let Some(snapshot) = load_memory_snapshot(&config).await? {
        manager.replace_snapshot(snapshot);
    }
    let repaired_count = match repair_saved_from_chat_sessions(&mut manager, &config).await {
        Ok(count) => count,
        Err(error) => {
            // A failed save restores the loaded state; the repair retries on the next boot.
            warn!(%error, "could not repair saved-from-chat memories; retrying on the next boot");
            0
        }
    };
    if repaired_count > 0 {
        info!(
            repaired_count,
            "detached saved-from-chat memories from their chat sessions"
        );
    }
    let loaded_count = manager.size();
    {
        let mut guard = state.write().await;
        guard.replace_memory(manager);
        guard.set_memory_store(Some(config.clone()));
    }

    info!(
        memory_store = %config.location_label(),
        storage = config.storage_label(),
        loaded_count,
        "runtime memory store configured"
    );
    Ok(Some(config))
}

/// Tag the web console puts on facts the owner saves from a chat message.
const SAVED_FROM_CHAT_TAG: &str = "saved-from-chat";

/// Facts saved from chat used to carry the chat's session, which keeps them out of every
/// run's context. Clear that session and persist once; a clean store is a no-op.
async fn repair_saved_from_chat_sessions(
    manager: &mut MemoryManager,
    config: &MemoryStoreConfig,
) -> io::Result<usize> {
    let stuck: Vec<String> = manager
        .get_recent_filtered(
            RecentMemoryOptions {
                limit: Some(usize::MAX),
                ..RecentMemoryOptions::default()
            },
            |memory| {
                memory.memory_type == MemoryType::Fact
                    && memory.session_id.is_some()
                    && memory
                        .tags
                        .as_ref()
                        .is_some_and(|tags| tags.iter().any(|tag| tag == SAVED_FROM_CHAT_TAG))
            },
        )
        .into_iter()
        .map(|memory| memory.id)
        .collect();
    if stuck.is_empty() {
        return Ok(0);
    }
    let mut mutation = MemoryMutation::new(manager);
    for id in &stuck {
        mutation
            .detach_session(id)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error.message()))?;
    }
    mutation.persist(Some(config)).await?;
    Ok(stuck.len())
}

/// Builds the embeddings runtime on hashed vectors. A configured model loads
/// later, in the background, through [`start_memory_model_load`].
async fn configure_memory_embeddings(
    state: &SharedDaemonState,
    memory_store: Option<&MemoryStoreConfig>,
    owner: ProfileOwner,
) -> io::Result<()> {
    let mut embeddings = MemoryEmbeddingRuntime::from_env_owned(
        memory_store.and_then(MemoryStoreConfig::embedding_store_default),
        memory_store.and_then(default_model_cache_dir),
        Some(owner),
    )?;
    let memories = {
        let memory = { state.read().await.memory_handle() };
        let memory_guard = memory.read().await;
        memory_guard.get_recent(RecentMemoryOptions {
            limit: Some(usize::MAX),
            ..RecentMemoryOptions::default()
        })
    };
    let report = embeddings
        .rebuild_from_memories(&memories)
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
    let status = embeddings.status();
    state.write().await.replace_memory_embeddings(embeddings);

    info!(
        enabled = status.enabled,
        provider = %status.provider,
        model = %status.model,
        status = status.load.label(),
        dimension = status.dimension,
        vector_count = status.vector_count,
        persisted = status.persisted,
        loaded_vectors = report.loaded_vectors,
        rebuilt_vectors = report.rebuilt_vectors,
        removed_stale_vectors = report.removed_stale_vectors,
        "runtime memory embeddings configured"
    );
    Ok(())
}

/// The search model is cached in `models/` beside the memory store file
/// (spec "Embeddings"); without a store file, fastembed picks its own cache.
fn default_model_cache_dir(memory_store: &MemoryStoreConfig) -> Option<PathBuf> {
    memory_store
        .file_path()
        .map(|path| path.with_file_name(MODEL_CACHE_DIR_NAME))
}

const MODEL_CACHE_DIR_NAME: &str = "models";

/// Starts loading the configured search model in the background. Boot never
/// waits for it: recall runs on hashed vectors and BM25 meanwhile.
pub(crate) async fn start_memory_model_load(state: &SharedDaemonState) {
    let (memory, embeddings) = {
        let guard = state.read().await;
        (guard.memory_handle(), guard.memory_embeddings_handle())
    };
    let Some(loader) = embeddings.write().await.take_model_loader() else {
        return;
    };
    // A thread that cannot start leaves search on hashed vectors, like a failed load.
    let fallback = Arc::clone(&embeddings);
    let loading = spawn_model_load(memory, embeddings, loader);
    match loading {
        Ok(_) => info!("loading the memory search model in the background"),
        Err(error) => {
            warn!(%error, "could not start loading the memory search model");
            fallback.write().await.fall_back(&error.to_string());
        }
    }
}

/// Embedded and test daemons never download a model; they keep basic search.
pub(crate) async fn skip_memory_model_load(state: &SharedDaemonState) {
    let embeddings = { state.read().await.memory_embeddings_handle() };
    let mut runtime = embeddings.write().await;
    if runtime.take_model_loader().is_some() {
        runtime.fall_back("the search model loads only in the served daemon");
    }
}

/// File name of the memory store kept next to the control-plane file when none is configured.
const DEFAULT_MEMORY_SQLITE_FILE: &str = "memory.sqlite";

/// Which memory store the daemon uses. Pure over already-read values so the choice can be
/// tested without touching process env, and so the startup log shares the decision.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MemoryStoreChoice {
    Json(PathBuf),
    Sqlite(PathBuf),
    Postgres,
}

#[cfg(test)]
fn profile_resource_paths(
    control_plane: Option<&Path>,
    memory: Option<&MemoryStoreChoice>,
    postgres: bool,
    history_override: Option<&Path>,
    embedding_override: Option<&Path>,
    checkpoint: Option<&Path>,
) -> Vec<PathBuf> {
    typed_profile_resources(
        control_plane,
        memory,
        postgres,
        history_override,
        embedding_override,
        checkpoint,
        None,
    )
    .into_iter()
    .map(|resource| match resource {
        ProfileResource::File(path)
        | ProfileResource::Sqlite(path)
        | ProfileResource::Directory(path) => path,
    })
    .collect()
}

fn typed_profile_resources(
    control_plane: Option<&Path>,
    memory: Option<&MemoryStoreChoice>,
    postgres: bool,
    history_override: Option<&Path>,
    embedding_override: Option<&Path>,
    checkpoint: Option<&Path>,
    explicit_tool_root: Option<&Path>,
) -> Vec<ProfileResource> {
    let mut paths = Vec::new();
    if let Some(path) = control_plane {
        paths.push(ProfileResource::File(path.to_path_buf()));
        if let Some(root) = default_tool_results_root(path) {
            paths.push(ProfileResource::Directory(root));
        }
    }
    if let Some(root) = explicit_tool_root {
        paths.push(ProfileResource::Directory(root.to_path_buf()));
    }
    let memory_sqlite = match memory {
        Some(MemoryStoreChoice::Json(path)) => {
            paths.push(ProfileResource::File(path.clone()));
            None
        }
        Some(MemoryStoreChoice::Sqlite(path)) => {
            paths.push(ProfileResource::Sqlite(path.clone()));
            Some(path)
        }
        _ => None,
    };
    if control_plane.is_some() || postgres {
        if let Some(path) = history_override {
            paths.push(ProfileResource::Sqlite(path.to_path_buf()));
        } else if let Some(control) = control_plane {
            paths.push(ProfileResource::Sqlite(default_history_sqlite_path(
                control,
            )));
        }
    }
    if let Some(path) = embedding_override.or(memory_sqlite.map(PathBuf::as_path)) {
        paths.push(ProfileResource::Sqlite(path.to_path_buf()));
    }
    if let Some(path) = checkpoint {
        paths.push(ProfileResource::Directory(path.to_path_buf()));
    }
    paths
}

pub(crate) fn default_tool_results_root(control_plane: &Path) -> Option<PathBuf> {
    let mut name = control_plane.file_name()?.to_os_string();
    name.push(".tool-results");
    Some(control_plane.with_file_name(name))
}

/// Explicit JSON or SQLite file wins, then Postgres, then `memory.sqlite` next to the
/// control-plane file. No control-plane file means memory-only, as before.
fn memory_store_choice(
    json: Option<PathBuf>,
    sqlite: Option<PathBuf>,
    postgres: bool,
    control_plane_file: Option<&Path>,
) -> io::Result<Option<MemoryStoreChoice>> {
    match (json, sqlite) {
        (Some(_), Some(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "set only one of ANIMAOS_RS_MEMORY_FILE or ANIMAOS_RS_MEMORY_SQLITE_FILE",
        )),
        (Some(path), None) => Ok(Some(MemoryStoreChoice::Json(path))),
        (None, Some(path)) => Ok(Some(MemoryStoreChoice::Sqlite(path))),
        (None, None) if postgres => Ok(Some(MemoryStoreChoice::Postgres)),
        (None, None) => Ok(control_plane_file.map(|control_plane| {
            let dir = control_plane.parent().unwrap_or_else(|| Path::new(""));
            MemoryStoreChoice::Sqlite(dir.join(DEFAULT_MEMORY_SQLITE_FILE))
        })),
    }
}

fn memory_store_choice_from_env(postgres: bool) -> io::Result<Option<MemoryStoreChoice>> {
    let json = non_empty_env_path("ANIMAOS_RS_MEMORY_FILE")?;
    let sqlite = non_empty_env_path("ANIMAOS_RS_MEMORY_SQLITE_FILE")?;
    let control_plane = non_empty_env_path("ANIMAOS_RS_CONTROL_PLANE_FILE")?;
    memory_store_choice(json, sqlite, postgres, control_plane.as_deref())
}

/// Startup-log label for the memory store, derived from the same choice the daemon makes.
/// `postgres` says whether the daemon runs with a Postgres pool.
pub fn runtime_memory_store_label(postgres: bool) -> String {
    match memory_store_choice_from_env(postgres) {
        Ok(Some(MemoryStoreChoice::Sqlite(path))) => format!("sqlite:{}", path.display()),
        Ok(Some(MemoryStoreChoice::Json(path))) => format!("json:{}", path.display()),
        Ok(Some(MemoryStoreChoice::Postgres)) => "postgres".to_string(),
        Ok(None) => "memory-only".to_string(),
        Err(error) => format!("invalid ({error})"),
    }
}

fn non_empty_env_path(name: &'static str) -> io::Result<Option<PathBuf>> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must not be empty"),
        ));
    }
    Ok(Some(PathBuf::from(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_owner_discovery_covers_default_history_and_embedding_memory_alias() {
        let control = PathBuf::from("private/profile.json");
        let memory = MemoryStoreChoice::Sqlite(PathBuf::from("private/memory.sqlite"));
        let resources =
            profile_resource_paths(Some(&control), Some(&memory), false, None, None, None);
        assert!(resources.contains(&control));
        assert!(resources.contains(&PathBuf::from("private/profile.json.tool-results")));
        assert!(resources.contains(&PathBuf::from("private/history.sqlite")));
        assert!(resources.contains(&PathBuf::from("private/memory.sqlite")));
    }

    #[test]
    fn profile_owner_discovery_covers_explicit_resources_even_with_different_control_plane() {
        let control = PathBuf::from("one/control.json");
        let memory = MemoryStoreChoice::Json(PathBuf::from("shared/memory.json"));
        let history = Path::new("shared/history.sqlite");
        let embeddings = Path::new("shared/embedding.sqlite");
        let checkpoint = Path::new("shared/checkpoints");
        let resources = profile_resource_paths(
            Some(&control),
            Some(&memory),
            false,
            Some(history),
            Some(embeddings),
            Some(checkpoint),
        );
        for path in [
            &control,
            &PathBuf::from("shared/memory.json"),
            &history.to_path_buf(),
            &embeddings.to_path_buf(),
            &checkpoint.to_path_buf(),
        ] {
            assert!(
                resources.contains(path),
                "profile discovery missed {}",
                path.display()
            );
        }
    }

    fn profile_child_command(control: &Path, ready: &Path) -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "app::persistence::tests::profile_owner_startup_child",
            "--nocapture",
        ]);
        for name in [
            "DATABASE_URL",
            "ANIMAOS_RS_MEMORY_FILE",
            "ANIMAOS_RS_MEMORY_SQLITE_FILE",
            "ANIMAOS_RS_HISTORY_SQLITE_FILE",
            "ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE",
            "ANIMAOS_RS_CHECKPOINT_ROOT",
            "ANIMAOS_RS_TOOL_RESULTS_DIR",
            "ANIMA_TEST_PROFILE_OWNER_REFUSED",
        ] {
            command.env_remove(name);
        }
        command
            .env("ANIMAOS_RS_CONTROL_PLANE_FILE", control)
            .env("ANIMAOS_RS_MEMORY_EMBEDDINGS", "off")
            .env("ANIMA_TEST_PROFILE_OWNER_STARTUP_READY", ready)
            .env(
                "ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR",
                crate::profile_owner::test_registry_path().unwrap(),
            )
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit());
        command
    }

    struct ProfileChild(std::process::Child);
    impl Drop for ProfileChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn running_profile(control: &Path, ready: &Path) -> ProfileChild {
        let mut child = ProfileChild(profile_child_command(control, ready).spawn().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "profile child exited before startup"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "profile startup did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child
    }

    #[tokio::test]
    async fn profile_owner_startup_child() {
        let Some(ready) = std::env::var_os("ANIMA_TEST_PROFILE_OWNER_STARTUP_READY") else {
            return;
        };
        let config = DaemonConfig {
            persistence_mode: PersistenceMode::Memory,
            ..DaemonConfig::default()
        };
        let preflight = prepare_persistence(&config);
        if std::env::var_os("ANIMA_TEST_PROFILE_OWNER_REFUSED").is_some() {
            assert!(
                preflight.is_err(),
                "a second process entered production persistence startup"
            );
            return;
        }
        let state = Arc::new(tokio::sync::RwLock::new(super::super::configured_state(
            &config,
            Arc::new(crate::model::DeterministicModelAdapter),
        )));
        configure_persistence(&state, &config, preflight.unwrap())
            .await
            .unwrap();
        std::fs::write(ready, b"ready").unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        drop(state);
    }

    #[test]
    fn profile_owner_two_real_startups_refuse_before_any_payload_or_backup_changes() {
        let temp = tempfile::tempdir().unwrap();
        let control = temp.path().join("profile.json");
        let first = running_profile(&control, &temp.path().join("ready-one"));
        let paths: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with(".owner.lock")
            })
            .collect();
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let entries = std::fs::read_dir(temp.path()).unwrap().count();
        let output = profile_child_command(&control, &temp.path().join("unused-ready"))
            .env("ANIMA_TEST_PROFILE_OWNER_REFUSED", "1")
            .env("ANIMAOS_RS_BIND_ADDR", "127.0.0.1:51999")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "the competing startup was not refused"
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), entries);
        for (path, original) in paths.iter().zip(before) {
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
        drop(first);
        let _successor = running_profile(&control, &temp.path().join("ready-successor"));
    }

    #[test]
    fn profile_owner_two_real_startups_with_separate_profiles_are_independent() {
        let temp = tempfile::tempdir().unwrap();
        let _one = running_profile(
            &temp.path().join("one/state.json"),
            &temp.path().join("ready-one"),
        );
        let _two = running_profile(
            &temp.path().join("two/state.json"),
            &temp.path().join("ready-two"),
        );
    }

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "anima-persistence-{label}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn chat_fact(
        manager: &mut MemoryManager,
        agent: &str,
        content: &str,
        tags: &[&str],
        session: Option<&str>,
    ) -> anima_memory::Memory {
        manager
            .add(anima_memory::NewMemory {
                agent_id: agent.into(),
                agent_name: "operator".into(),
                memory_type: MemoryType::Fact,
                content: content.into(),
                importance: 0.75,
                tags: Some(tags.iter().map(|tag| tag.to_string()).collect()),
                scope: Some(anima_memory::MemoryScope::Private),
                room_id: None,
                world_id: None,
                session_id: session.map(str::to_string),
            })
            .unwrap()
    }

    #[tokio::test]
    async fn boot_repair_detaches_saved_from_chat_facts_so_context_returns_them() {
        use anima_core::{AgentConfig, Content, Message, MessageRole};

        let dir = temp_dir("saved-from-chat");
        std::fs::create_dir_all(&dir).unwrap();
        let config = MemoryStoreConfig::Json(dir.join("memory.json"));
        let runtime = anima_core::AgentRuntime::new(
            AgentConfig {
                name: "operator".into(),
                model: "gpt-5.4".into(),
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                provider: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
                fallbacks: Vec::new(),
            },
            Arc::new(crate::model::DeterministicModelAdapter),
        );
        let mut manager = MemoryManager::new();
        let stuck = chat_fact(
            &mut manager,
            runtime.id(),
            "citron tea is unsweetened",
            &["saved-from-chat"],
            Some("session-1"),
        );
        // Facts the agent saved itself with a session, and facts with no session, stay as they are.
        let agent_saved = chat_fact(
            &mut manager,
            runtime.id(),
            "citron tea comes in a blue cup",
            &["from-agent"],
            Some("session-2"),
        );
        let clean = chat_fact(
            &mut manager,
            runtime.id(),
            "citron tea steeps for three minutes",
            &["saved-from-chat"],
            None,
        );
        let ask = |memory: Arc<tokio::sync::RwLock<MemoryManager>>| {
            let runtime = &runtime;
            async move {
                let providers = crate::components::default_providers(
                    memory,
                    Arc::new(tokio::sync::RwLock::new(MemoryEmbeddingRuntime::disabled())),
                );
                providers[0]
                    .get(
                        runtime,
                        &Message {
                            id: "m".into(),
                            agent_id: runtime.id().into(),
                            room_id: "room".into(),
                            content: Content {
                                text: "citron tea".into(),
                                attachments: None,
                                metadata: None,
                            },
                            role: MessageRole::User,
                            created_at_ms: 1,
                        },
                    )
                    .await
                    .unwrap()
                    .text
            }
        };
        let shared = Arc::new(tokio::sync::RwLock::new(MemoryManager::new()));
        shared.write().await.replace_snapshot(manager.snapshot());
        assert!(
            !ask(shared.clone()).await.contains(&stuck.id),
            "the session hides the fact before repair"
        );

        assert_eq!(
            repair_saved_from_chat_sessions(&mut manager, &config)
                .await
                .unwrap(),
            1
        );

        assert_eq!(manager.get(&stuck.id).unwrap().session_id, None);
        assert_eq!(manager.get(&stuck.id).unwrap().content, stuck.content);
        assert_eq!(
            manager.get(&agent_saved.id).unwrap().session_id.as_deref(),
            Some("session-2")
        );
        assert_eq!(manager.get(&clean.id), Some(clean));
        let saved = load_memory_snapshot(&config).await.unwrap().unwrap();
        assert_eq!(
            saved
                .memories
                .iter()
                .find(|memory| memory.id == stuck.id)
                .unwrap()
                .session_id,
            None,
            "the repair is persisted"
        );
        shared.write().await.replace_snapshot(manager.snapshot());
        assert!(ask(shared).await.contains(&stuck.id));

        let before = std::fs::metadata(dir.join("memory.json"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            repair_saved_from_chat_sessions(&mut manager, &config)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            std::fs::metadata(dir.join("memory.json"))
                .unwrap()
                .modified()
                .unwrap(),
            before,
            "a clean store is not rewritten"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn boot_moves_memory_pins_into_core_memory_once_and_writes_the_copies() {
        use crate::agent_files::selection::profile_relative;
        use crate::core_memory::files::HEADER;

        let dir = temp_dir("memory-pins");
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let memory_config = MemoryStoreConfig::Json(dir.join("memory.json"));
        let control_config = ControlPlaneStoreConfig::Json(dir.join("control-plane.json"));

        // An older control plane: one agent whose owner pinned three facts.
        let mut older = crate::state::DaemonState::new();
        let agent = older
            .create_agent(anima_core::AgentConfig {
                name: "operator".into(),
                model: "gpt-5.4".into(),
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                provider: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
                fallbacks: Vec::new(),
            })
            .unwrap()
            .state
            .id;
        older.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: workspace.clone(),
            company_name: "Test".into(),
            mission: String::new(),
            values: vec![],
        });
        let mut manager = MemoryManager::new();
        let profile = chat_fact(
            &mut manager,
            &agent,
            "prefers metric units",
            &["saved-from-chat"],
            None,
        );
        let note = chat_fact(&mut manager, &agent, "the build needs bun", &[], None);
        let too_long = chat_fact(&mut manager, &agent, &"x".repeat(600), &[], None);
        crate::memory_store::save_memory_manager(Some(&memory_config), &manager)
            .await
            .unwrap();
        let mut snapshot = serde_json::to_value(older.control_plane_snapshot()).unwrap();
        snapshot["memoryViews"] = serde_json::json!([{
            "agentId": agent,
            "version": 2,
            "userProfileIds": [profile.id, too_long.id],
            "agentNoteIds": [note.id]
        }]);
        std::fs::write(dir.join("control-plane.json"), snapshot.to_string()).unwrap();

        let boot = || async {
            let state: SharedDaemonState =
                Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
            configure_memory_store(&state, Some(memory_config.clone()))
                .await
                .unwrap();
            configure_control_plane_store(&state, Some(control_config.clone()))
                .await
                .unwrap();
            write_core_memory_copies(&state).await.unwrap();
            state
        };
        let too_long_id = too_long.id.clone();
        let blocks = |state: SharedDaemonState| {
            let agent = agent.clone();
            let too_long_id = too_long_id.clone();
            async move {
                let memory = state.read().await.memory_handle();
                let manager = memory.read().await;
                let (user, notes) = crate::core_memory::blocks_for(&manager, &agent);
                let ids = |entries: Vec<anima_memory::Memory>| {
                    entries
                        .into_iter()
                        .map(|memory| memory.id)
                        .collect::<Vec<_>>()
                };
                (ids(user), ids(notes), manager.get(&too_long_id).unwrap())
            }
        };

        let state = boot().await;
        let (user, notes, skipped) = blocks(state).await;
        assert_eq!(user, [profile.id.clone()]);
        assert_eq!(notes, [note.id.clone()]);
        assert_eq!(skipped, too_long, "an over-long pin is left untouched");
        let saved = load_memory_snapshot(&memory_config).await.unwrap().unwrap();
        assert!(saved.memories.iter().any(|memory| memory.id == profile.id
            && memory
                .tags
                .as_ref()
                .unwrap()
                .contains(&"core:user".to_string())));
        let control = std::fs::read_to_string(dir.join("control-plane.json")).unwrap();
        assert!(!control.contains("memoryViews"), "the pins are dropped");
        let profile_dir = workspace.join(profile_relative(&agent));
        assert_eq!(
            std::fs::read_to_string(profile_dir.join("USER.md")).unwrap(),
            format!("{HEADER}\n- prefers metric units\n")
        );
        assert_eq!(
            std::fs::read_to_string(profile_dir.join("MEMORY.md")).unwrap(),
            format!("{HEADER}\n- the build needs bun\n")
        );

        let memory_before = std::fs::read(dir.join("memory.json")).unwrap();
        let state = boot().await;
        assert_eq!(
            std::fs::read(dir.join("memory.json")).unwrap(),
            memory_before,
            "a second boot changes nothing"
        );
        let (user, notes, _) = blocks(state).await;
        assert_eq!((user, notes), (vec![profile.id], vec![note.id]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_default_history_file_sits_beside_the_control_plane_file() {
        assert_eq!(
            default_history_sqlite_path(Path::new("/data/control-plane.json")),
            PathBuf::from("/data/history.sqlite")
        );
        assert_eq!(
            default_history_sqlite_path(Path::new("control-plane.json")),
            PathBuf::from("history.sqlite")
        );
    }

    #[tokio::test]
    async fn the_history_store_follows_the_control_plane_store() {
        let dir = temp_dir("history");
        let control = ControlPlaneStoreConfig::Json(dir.join("control-plane.json"));

        let beside = history_store_for(Some(&control), None).await.unwrap();
        assert_eq!(beside.label(), "sqlite");
        assert!(dir.join("history.sqlite").exists());
        let custom = dir.join("custom").join("history.db");
        let chosen = history_store_for(Some(&control), Some(custom.clone()))
            .await
            .unwrap();
        assert_eq!(chosen.label(), "sqlite");
        assert!(custom.exists());
        let ephemeral = history_store_for(None, Some(custom)).await.unwrap();
        assert!(
            ephemeral.is_ephemeral(),
            "without a durable control plane history stays in memory"
        );
        assert_eq!(
            history_store_for(None, None).await.unwrap().label(),
            "memory"
        );
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://anima@127.0.0.1:1/anima")
            .unwrap();
        assert_eq!(
            history_store_for(Some(&ControlPlaneStoreConfig::Postgres(pool)), None)
                .await
                .unwrap()
                .label(),
            "postgres"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_history_file_that_cannot_open_refuses_boot_before_the_snapshot_is_touched() {
        let dir = temp_dir("unopenable");
        std::fs::create_dir_all(&dir).unwrap();
        let control_file = dir.join("control-plane.json");
        let original = r#"{"version":4,"agents":[],"swarms":[]}"#;
        std::fs::write(&control_file, original).unwrap();
        // A regular file where the history file's directory should be.
        let not_a_directory = dir.join("not-a-directory");
        std::fs::write(&not_a_directory, "").unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        let error = configure_control_plane_and_history(
            &state,
            Some(ControlPlaneStoreConfig::Json(control_file.clone())),
            Some(not_a_directory.join("history.sqlite")),
        )
        .await
        .expect_err("an unopenable history store refuses boot");

        assert!(
            error
                .to_string()
                .starts_with("failed to open the history store"),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(&control_file).unwrap(),
            original,
            "an older daemon can still start on the untouched snapshot"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_saved_history_deletion_survives_a_restart_and_is_replayed_at_boot() {
        use crate::agent_runs::{AgentRunCoordinator, AgentRunRequest, RunRoom};
        use crate::history::{HistoryDeletion, MessagePageQuery};
        use crate::runs::RunSource;
        use anima_core::primitives::now_millis;

        let dir = temp_dir("deletion-restart");
        let control = ControlPlaneStoreConfig::Json(dir.join("control-plane.json"));
        let page = |agent_id: &str, session_id: &str| MessagePageQuery {
            agent_id: agent_id.into(),
            session_id: session_id.into(),
            before: None,
            limit: 10,
            include_hidden: true,
        };
        let turn = |agent_id: &str, room: &str| AgentRunRequest {
            agent_id: agent_id.into(),
            content: anima_core::Content {
                text: "remember this".into(),
                ..anima_core::Content::default()
            },
            room: RunRoom::Stable(room.into()),
            idempotency_key: None,
            source: RunSource::Api,
            source_ref: None,
            parent: None,
        };

        // First boot: two mirrored chats, one of them deleted with its history
        // deletion saved, and the daemon stops before the store applies it.
        let first = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_and_history(&first, Some(control.clone()), None)
            .await
            .unwrap();
        let agent_id = first
            .write()
            .await
            .create_agent(anima_core::AgentConfig {
                name: "historian".into(),
                model: "deterministic".into(),
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                provider: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
                fallbacks: Vec::new(),
            })
            .unwrap()
            .state
            .id;
        let coordinator =
            AgentRunCoordinator::new(Arc::clone(&first), Arc::new(tokio::sync::Semaphore::new(1)));
        for room in ["chat:doomed", "chat:kept"] {
            coordinator.run(turn(&agent_id, room)).await.unwrap();
        }
        let history = first.read().await.history.clone();
        history
            .flush_once(
                &first,
                &coordinator.control_plane_transactions(),
                now_millis(),
            )
            .await
            .unwrap();
        assert_eq!(
            history
                .store()
                .page_messages(&page(&agent_id, "chat:doomed"))
                .await
                .unwrap()
                .len(),
            2
        );
        {
            let _transaction = coordinator.control_plane_transaction().await;
            let persist = {
                let mut guard = first.write().await;
                guard
                    .agents
                    .get_mut(&agent_id)
                    .unwrap()
                    .retain_messages(|message| message.room_id != "chat:doomed");
                guard.record_history_deletion(HistoryDeletion::session(&agent_id, "chat:doomed"));
                guard.control_plane_persist_request()
            };
            persist.save().await.unwrap();
        }
        drop((coordinator, history, first));

        // Second boot over the same files replays the deletion.
        let second = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_and_history(&second, Some(control.clone()), None)
            .await
            .unwrap();
        assert_eq!(
            second.read().await.pending_history_deletions.len(),
            1,
            "the deletion was saved with the session's removal"
        );
        let history = second.read().await.history.clone();
        let report = history
            .flush_once(&second, &tokio::sync::Mutex::new(()), now_millis())
            .await
            .unwrap();

        assert_eq!(report.deletions, 1);
        let store = history.store();
        assert!(
            store
                .page_messages(&page(&agent_id, "chat:doomed"))
                .await
                .unwrap()
                .is_empty(),
            "the replayed deletion removed the rows"
        );
        assert_eq!(
            store
                .page_messages(&page(&agent_id, "chat:kept"))
                .await
                .unwrap()
                .len(),
            2
        );
        let saved = load_control_plane_snapshot(&control)
            .await
            .unwrap()
            .unwrap();
        assert!(
            saved.pending_history_deletions.is_empty(),
            "the entry is cleared in a normal save"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn upgrader() -> anima_core::AgentConfig {
        anima_core::AgentConfig {
            name: "upgrader".into(),
            model: "deterministic".into(),
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: None,
            system: None,
            tools: None,
            plugins: None,
            settings: None,
            fallbacks: Vec::new(),
        }
    }

    /// A snapshot file as an older daemon wrote it: no sessions, and the
    /// given version field (or none).
    fn older_snapshot_file(version: Option<u32>) -> String {
        let mut source = crate::state::DaemonState::new();
        source.create_agent(upgrader()).unwrap();
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("sessions");
        match version {
            Some(version) => {
                object.insert("version".into(), version.into());
            }
            None => {
                object.remove("version");
            }
        }
        serde_json::to_string_pretty(&value).unwrap()
    }

    #[tokio::test]
    async fn upgrading_any_pre_sessions_snapshot_writes_the_backup_before_saving_the_current_version(
    ) {
        for version in [None, Some(1), Some(2), Some(3), Some(4)] {
            let dir = temp_dir("upgrade");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("control-plane.json");
            let original = older_snapshot_file(version);
            std::fs::write(&path, &original).unwrap();
            let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

            configure_control_plane_store(
                &state,
                Some(ControlPlaneStoreConfig::Json(path.clone())),
            )
            .await
            .unwrap();

            let backup = crate::control_plane_store::pre_sessions_backup_path(&path);
            assert_eq!(
                std::fs::read_to_string(&backup).unwrap(),
                original,
                "{version:?}: the backup is the untouched original"
            );
            assert!(
                !crate::control_plane_store::pre_live_runs_backup_path(&path).exists(),
                "{version:?}: a pre-sessions snapshot keeps the pre-sessions name"
            );
            assert!(
                !crate::control_plane_store::pre_approvals_backup_path(&path).exists(),
                "{version:?}: a pre-sessions snapshot writes only the pre-sessions backup"
            );
            assert!(
                !crate::control_plane_store::pre_skills_backup_path(&path).exists(),
                "{version:?}: a pre-sessions snapshot writes no skills backup"
            );
            assert!(
                !crate::control_plane_store::pre_automations_backup_path(&path).exists(),
                "{version:?}: a pre-sessions snapshot writes no automations backup"
            );
            let saved: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(
                saved["version"],
                crate::control_plane_store::CONTROL_PLANE_STORE_VERSION,
                "{version:?}"
            );
            assert_eq!(state.read().await.agent_count(), 1);
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Controller ruling (M3 pre-flight audit I2): an M2 (version-5) snapshot
    /// is backed up as `.pre-live-runs.bak` before the current version is saved, and the
    /// `.pre-sessions.bak` the M2 upgrade wrote is left as it was.
    #[tokio::test]
    async fn upgrading_a_version_five_snapshot_writes_the_live_runs_backup_and_loads_it() {
        let dir = temp_dir("upgrade-live-runs");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        let agent_id = source.create_agent(upgrader()).unwrap().state.id;
        source.sessions.insert(crate::sessions::SessionRecord::new(
            &agent_id,
            "chat:kept",
            crate::sessions::SessionKind::Chat,
            crate::sessions::SessionOrigin::Web,
            "Kept".into(),
            crate::sessions::TitleSource::Owner,
            1,
        ));
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        value["version"] = 5.into();
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let m2_backup = older_snapshot_file(Some(4));
        let pre_sessions = crate::control_plane_store::pre_sessions_backup_path(&path);
        std::fs::write(&pre_sessions, &m2_backup).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();

        let backup = crate::control_plane_store::pre_live_runs_backup_path(&path);
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            original,
            "the backup is the untouched version-5 original"
        );
        assert_eq!(
            std::fs::read_to_string(&pre_sessions).unwrap(),
            m2_backup,
            "the M2 upgrade's backup is never overwritten"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        let guard = state.read().await;
        assert_eq!(guard.agent_count(), 1);
        assert_eq!(
            guard.sessions.get(&agent_id, "chat:kept").unwrap().title,
            "Kept"
        );
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// M4: an M3 (version-6) snapshot is backed up as `.pre-approvals.bak`
    /// before version 7 is saved; the earlier upgrades' backups stay.
    #[tokio::test]
    async fn upgrading_a_version_six_snapshot_writes_the_approvals_backup_and_loads_it() {
        let dir = temp_dir("upgrade-approvals");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        let agent_id = source.create_agent(upgrader()).unwrap().state.id;
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        value["version"] = 6.into();
        let object = value.as_object_mut().unwrap();
        for key in ["approvals", "approvalPolicies", "approvalRules"] {
            object.remove(key);
        }
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let m3_backup = older_snapshot_file(Some(5));
        let pre_live_runs = crate::control_plane_store::pre_live_runs_backup_path(&path);
        std::fs::write(&pre_live_runs, &m3_backup).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();

        let backup = crate::control_plane_store::pre_approvals_backup_path(&path);
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            original,
            "the backup is the untouched version-6 original"
        );
        assert_eq!(
            std::fs::read_to_string(&pre_live_runs).unwrap(),
            m3_backup,
            "the M3 upgrade's backup is never overwritten"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        assert_eq!(saved["approvalRules"], serde_json::json!([]));
        let guard = state.read().await;
        assert_eq!(guard.agent_count(), 1);
        assert_eq!(
            guard.approvals.policy(&agent_id),
            crate::approvals::ApprovalPolicy::default()
        );
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// M5: an M4 (version-7) snapshot is backed up as `.pre-skills.bak`
    /// before version 8 is saved; the earlier upgrades' backups stay.
    #[tokio::test]
    async fn upgrading_a_version_seven_snapshot_writes_the_skills_backup_and_loads_it() {
        let dir = temp_dir("upgrade-skills");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        let agent_id = source.create_agent(upgrader()).unwrap().state.id;
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        value["version"] = 7.into();
        let object = value.as_object_mut().unwrap();
        for key in ["skills", "skillDrafts"] {
            object.remove(key);
        }
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let m4_backup = older_snapshot_file(Some(6));
        let pre_approvals = crate::control_plane_store::pre_approvals_backup_path(&path);
        std::fs::write(&pre_approvals, &m4_backup).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();

        let backup = crate::control_plane_store::pre_skills_backup_path(&path);
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            original,
            "the backup is the untouched version-7 original"
        );
        assert_eq!(
            std::fs::read_to_string(&pre_approvals).unwrap(),
            m4_backup,
            "the M4 upgrade's backup is never overwritten"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        assert_eq!(saved["skills"], serde_json::json!([]));
        assert_eq!(saved["skillDrafts"], serde_json::json!([]));
        let guard = state.read().await;
        assert_eq!(guard.agent_count(), 1);
        assert!(guard.agents.contains_key(&agent_id));
        assert!(guard.skills.records().is_empty());
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// M6: an M5 (version-8) snapshot is backed up as `.pre-automations.bak`
    /// before version 9 is saved; its automations load with the M6 fields'
    /// defaults, and the earlier upgrades' backups stay.
    #[tokio::test]
    async fn upgrading_a_version_eight_snapshot_writes_the_automations_backup_and_loads_it() {
        let dir = temp_dir("upgrade-automations");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        let agent_id = source.create_agent(upgrader()).unwrap().state.id;
        source.schedules.insert(
            "schedule-1".into(),
            crate::schedules::test_automation(&agent_id, "schedule-1"),
        );
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        value["version"] = 8.into();
        value.as_object_mut().unwrap().remove("scheduleFires");
        let stored = value["schedules"][0].as_object_mut().unwrap();
        for key in ["name", "activeHours", "createdBy", "preset", "counters"] {
            stored.remove(key);
        }
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let m5_backup = older_snapshot_file(Some(7));
        let pre_skills = crate::control_plane_store::pre_skills_backup_path(&path);
        std::fs::write(&pre_skills, &m5_backup).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();

        let backup = crate::control_plane_store::pre_automations_backup_path(&path);
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            original,
            "the backup is the untouched version-8 original"
        );
        assert_eq!(
            std::fs::read_to_string(&pre_skills).unwrap(),
            m5_backup,
            "the M5 upgrade's backup is never overwritten"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        assert_eq!(saved["scheduleFires"], serde_json::json!([]));
        let guard = state.read().await;
        let restored = &guard.schedules["schedule-1"];
        assert_eq!(restored.name, "");
        assert_eq!(restored.active_hours, None);
        assert_eq!(
            restored.created_by,
            crate::schedules::AutomationCreator::Owner
        );
        assert_eq!(restored.preset, None);
        assert_eq!(
            restored.counters,
            crate::schedules::AutomationCounters::default()
        );
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn sample_override() -> crate::usage::PricingOverride {
        crate::usage::PricingOverride {
            provider: "openai".into(),
            model: "gpt-x".into(),
            input_micros_per_mtok: 1_000_000,
            output_micros_per_mtok: 2_000_000,
            cached_input_micros_per_mtok: None,
        }
    }

    /// M8: an M7 (version-9) snapshot is backed up as `.pre-usage.bak` before
    /// version 10 is saved; it loads with no pricing overrides, and the earlier
    /// upgrades' backups stay.
    #[tokio::test]
    async fn a_version_nine_snapshot_loads_and_is_backed_up() {
        let dir = temp_dir("upgrade-usage");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        source.create_agent(upgrader()).unwrap();
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        value["version"] = 9.into();
        value.as_object_mut().unwrap().remove("pricingOverrides");
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let m6_backup = older_snapshot_file(Some(8));
        let pre_automations = crate::control_plane_store::pre_automations_backup_path(&path);
        std::fs::write(&pre_automations, &m6_backup).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();

        let backup = crate::control_plane_store::pre_usage_backup_path(&path);
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            original,
            "the backup is the untouched version-9 original"
        );
        assert_eq!(
            std::fs::read_to_string(&pre_automations).unwrap(),
            m6_backup,
            "the M6 upgrade's backup is never overwritten"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        assert_eq!(saved["pricingOverrides"], serde_json::json!([]));
        let guard = state.read().await;
        assert_eq!(guard.agent_count(), 1);
        assert!(guard.pricing_overrides.is_empty());
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn upgrading_a_version_ten_snapshot_writes_the_attachments_backup_and_loads_it() {
        let dir = temp_dir("upgrade-attachments");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        source.create_agent(upgrader()).unwrap();
        source.set_pricing_overrides(vec![sample_override()]);
        let mut value = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        value["version"] = 10.into();
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let prior = older_snapshot_file(Some(9));
        let pre_usage = crate::control_plane_store::pre_usage_backup_path(&path);
        std::fs::write(&pre_usage, &prior).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(crate::control_plane_store::pre_attachments_backup_path(
                &path
            ))
            .unwrap(),
            original,
            "the version-10 input is backed up unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(pre_usage).unwrap(),
            prior,
            "the previous usage backup survives"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        let guard = state.read().await;
        assert_eq!(guard.agent_count(), 1);
        assert_eq!(guard.pricing_overrides, vec![sample_override()]);
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_current_priced_snapshot_writes_no_backup() {
        let dir = temp_dir("current-attachments");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut source = crate::state::DaemonState::new();
        source.create_agent(upgrader()).unwrap();
        source.set_pricing_overrides(vec![sample_override()]);
        let original = serde_json::to_string_pretty(&source.control_plane_snapshot()).unwrap();
        std::fs::write(&path, original).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .unwrap();

        assert!(!crate::control_plane_store::pre_usage_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_attachments_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_discord_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_automations_backup_path(&path).exists());
        assert_eq!(
            state.read().await.pricing_overrides,
            vec![sample_override()]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_snapshot_newer_than_current_is_refused() {
        let dir = temp_dir("newer-than-current");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let mut value =
            serde_json::to_value(crate::state::DaemonState::new().control_plane_snapshot())
                .unwrap();
        value["version"] = (CONTROL_PLANE_STORE_VERSION + 1).into();
        let original = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &original).unwrap();
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));

        let error = configure_control_plane_store(
            &state,
            Some(ControlPlaneStoreConfig::Json(path.clone())),
        )
        .await
        .expect_err("a newer snapshot must be refused");

        assert!(
            error.to_string().contains(&format!(
                "unsupported control plane store version: {}",
                CONTROL_PLANE_STORE_VERSION + 1
            )),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!crate::control_plane_store::pre_usage_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_attachments_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_discord_backup_path(&path).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_current_snapshot_loads_without_a_backup() {
        let dir = temp_dir("current");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let fresh = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&fresh, Some(config.clone()))
            .await
            .unwrap();
        assert!(
            path.exists(),
            "a fresh start saves a current-format snapshot"
        );

        let restarted = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&restarted, Some(config))
            .await
            .unwrap();

        assert!(!crate::control_plane_store::pre_sessions_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_live_runs_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_approvals_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_skills_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_automations_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_usage_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_attachments_backup_path(&path).exists());
        assert!(!crate::control_plane_store::pre_discord_backup_path(&path).exists());
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["version"], CONTROL_PLANE_STORE_VERSION,
            "a current-format snapshot writes no backup"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    // Controller ruling (pre-flight audit): the backup is written only when the
    // loaded version is below CONTROL_PLANE_STORE_VERSION, so a later version
    // bump can never clobber it. This simulates a backup already on disk from a
    // prior upgrade and proves loading a current-format snapshot never rewrites it.
    #[tokio::test]
    async fn loading_a_current_snapshot_leaves_an_existing_pre_upgrade_backup_untouched() {
        let dir = temp_dir("current-with-backup");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let fresh = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&fresh, Some(config.clone()))
            .await
            .unwrap();
        assert!(
            path.exists(),
            "a fresh start saves a current-format snapshot"
        );

        let backup = crate::control_plane_store::pre_sessions_backup_path(&path);
        let preexisting_backup = "{\"version\":3,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&backup, preexisting_backup).unwrap();

        let restarted = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&restarted, Some(config))
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            preexisting_backup,
            "loading an already-current snapshot must not rewrite an existing backup"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    // Review finding (fix round 1, Important): the backup write's `?` in
    // `configure_control_plane_store` is the only thing standing between a
    // failed backup and an unprotected upgrade. Force the write to fail
    // deterministically (no permission tricks): put a directory where the
    // backup file needs to go, so `AtomicFile`'s rename onto that path fails.
    #[tokio::test]
    async fn a_failed_backup_write_refuses_boot_and_leaves_the_snapshot_untouched() {
        let dir = temp_dir("backup-failure");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control-plane.json");
        let original = older_snapshot_file(Some(4));
        std::fs::write(&path, &original).unwrap();

        let backup_path = crate::control_plane_store::pre_sessions_backup_path(&path);
        std::fs::create_dir_all(&backup_path).unwrap();

        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&state, Some(ControlPlaneStoreConfig::Json(path.clone())))
            .await
            .expect_err("a backup write that cannot replace a directory must refuse boot");

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "the original snapshot must be untouched when the backup fails"
        );
        let residue = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".atomicwrite")
            });
        assert!(
            !residue,
            "the atomic writer's temp file must not survive a failed rename"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn memory_defaults_to_sqlite_next_to_the_control_plane_file() {
        let control_plane = Path::new("/data/anima/control-plane.json");
        let choice = memory_store_choice(None, None, false, Some(control_plane)).unwrap();
        assert_eq!(
            choice,
            Some(MemoryStoreChoice::Sqlite(PathBuf::from(
                "/data/anima/memory.sqlite"
            )))
        );
        // A bare file name resolves next to it in the working directory.
        let choice = memory_store_choice(None, None, false, Some(Path::new("cp.json"))).unwrap();
        assert_eq!(
            choice,
            Some(MemoryStoreChoice::Sqlite(PathBuf::from("memory.sqlite")))
        );
    }

    #[test]
    fn an_explicit_memory_store_beats_the_default() {
        let control_plane = Path::new("/data/anima/control-plane.json");
        let json = PathBuf::from("/elsewhere/memory.json");
        let sqlite = PathBuf::from("/elsewhere/memory.db");
        assert_eq!(
            memory_store_choice(Some(json.clone()), None, false, Some(control_plane)).unwrap(),
            Some(MemoryStoreChoice::Json(json.clone()))
        );
        assert_eq!(
            memory_store_choice(None, Some(sqlite.clone()), false, Some(control_plane)).unwrap(),
            Some(MemoryStoreChoice::Sqlite(sqlite.clone()))
        );
        assert!(memory_store_choice(Some(json), Some(sqlite), false, Some(control_plane)).is_err());
        // Postgres is configured memory too, so it keeps winning over the file default.
        assert_eq!(
            memory_store_choice(None, None, true, Some(control_plane)).unwrap(),
            Some(MemoryStoreChoice::Postgres)
        );
    }

    #[test]
    fn without_a_control_plane_file_memory_stays_memory_only() {
        assert_eq!(memory_store_choice(None, None, false, None).unwrap(), None);
    }

    #[test]
    fn the_model_cache_sits_in_models_beside_the_memory_store_file() {
        let sqlite = MemoryStoreConfig::Sqlite(PathBuf::from("/data/anima/memory.sqlite"));
        assert_eq!(
            default_model_cache_dir(&sqlite),
            Some(PathBuf::from("/data/anima/models"))
        );
        let json = MemoryStoreConfig::Json(PathBuf::from("memory.json"));
        assert_eq!(
            default_model_cache_dir(&json),
            Some(PathBuf::from("models"))
        );
    }

    #[tokio::test]
    async fn boot_starts_the_model_load_without_waiting_for_it() {
        use crate::memory_embeddings::{test_support, MemoryEmbeddingLoad};

        let state: SharedDaemonState =
            Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let (loader, release) = test_support::gated_model_loader(40);
        state
            .write()
            .await
            .replace_memory_embeddings(test_support::awaiting(loader));

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            start_memory_model_load(&state),
        )
        .await
        .expect("boot does not wait for the model");
        let embeddings = state.read().await.memory_embeddings_handle();
        assert_eq!(
            embeddings.read().await.status().load,
            MemoryEmbeddingLoad::Pending
        );

        release.send(()).unwrap();
        for _ in 0..200 {
            if embeddings.read().await.status().load == MemoryEmbeddingLoad::Ready {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            embeddings.read().await.status().load,
            MemoryEmbeddingLoad::Ready
        );
        assert_eq!(embeddings.read().await.status().dimension, 40);
    }

    #[tokio::test]
    async fn an_embedded_daemon_keeps_basic_search_instead_of_downloading() {
        use crate::memory_embeddings::{test_support, MemoryEmbeddingLoad};

        let state: SharedDaemonState =
            Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let (loader, calls) = test_support::fake_model_loader(40);
        state
            .write()
            .await
            .replace_memory_embeddings(test_support::awaiting(loader));

        skip_memory_model_load(&state).await;

        let embeddings = state.read().await.memory_embeddings_handle();
        let status = embeddings.read().await.status();
        assert!(matches!(status.load, MemoryEmbeddingLoad::Fallback(_)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}

#[cfg(test)]
mod discord_upgrade_startup_tests {
    use super::*;
    #[tokio::test]
    async fn configure_store_upgrades_v11_only_after_independent_discord_backup_and_reloads_v12() {
        let path = std::env::temp_dir().join(format!(
            "anima-startup-discord-upgrade-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = "{\n  \"version\":11, \"agents\":[], \"swarms\":[]\n}\n";
        let previous_attachments = crate::control_plane_store::pre_attachments_backup_path(&path);
        std::fs::write(&previous_attachments, "older attachments backup").unwrap();
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&state, Some(config.clone()))
            .await
            .unwrap();
        let discord_backup = crate::control_plane_store::pre_discord_backup_path(&path);
        assert_eq!(std::fs::read_to_string(&discord_backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&previous_attachments).unwrap(),
            "older attachments backup"
        );
        let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert_eq!(saved.version, CONTROL_PLANE_STORE_VERSION);
        assert!(saved.discord_connectors.is_empty());
        assert!(saved.discord_credential_intents.is_empty());
        assert!(saved.discord_inbound.is_empty());
        assert!(saved.discord_outbound.is_empty());
        let restarted = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        configure_control_plane_store(&restarted, Some(config))
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(&discord_backup).unwrap(), original);
        std::fs::remove_file(discord_backup).unwrap();
        std::fs::remove_file(previous_attachments).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
