//! Explicit executable activation; missing/unsupported model access never selects legacy.
use crate::{
    connection::ResolvedConnection, engine_bridge::DurableVillage, Error, Result, Service, Store,
};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::{
    anima_core::execution_limits::SystemBudgetClock,
    durable::hosted::{StorageLease, StoragePaths},
};
use anima_memory_store_sqlite::SqliteMemoryStore;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc, time::Duration};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: u32,
    storage: StoragePaths,
    saves: PathBuf,
    mode: Mode,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Active,
    Draining,
}
fn invalid() -> Error {
    Error::new(
        503,
        "Persistent village startup configuration or authority is unavailable or invalid",
    )
}
impl Config {
    pub fn from_env() -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("VILLAGE_DURABLE_CONFIG_FILE") else {
            return Ok(None);
        };
        let path = PathBuf::from(path);
        anima_harness::durable::hosted::safe_path(&path).map_err(|_| invalid())?;
        let bytes = std::fs::read(path).map_err(|_| invalid())?;
        if bytes.len() > 16384 {
            return Err(invalid());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        config.validate()?;
        Ok(Some(config))
    }
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(invalid());
        }
        self.storage
            .validate(std::slice::from_ref(&self.saves))
            .map_err(|_| invalid())?;
        Ok(())
    }
    fn preflight_save(&self, owner: uuid::Uuid, stores: Option<(uuid::Uuid, &str)>) -> Result<()> {
        if !self.saves.exists() {
            return Ok(());
        }
        if !self.storage.execution.is_file() || !self.storage.memory.is_file() {
            return Err(invalid());
        }
        let db = rusqlite::Connection::open_with_flags(
            &self.saves,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|_| invalid())?;
        let accepted: String = db
            .query_row(
                "SELECT owner FROM village_startup_owner WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .map_err(|_| invalid())?;
        if accepted != owner.to_string() {
            return Err(invalid());
        }
        if let Some((execution, memory)) = stores {
            let (e,m):(String,String)=db.query_row("SELECT execution_store,memory_store FROM village_engine_attachment WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|invalid())?;
            if e != execution.to_string() || m != memory {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub async fn service(
        self,
        connection: ResolvedConnection,
        deadline: Duration,
    ) -> Result<Service> {
        self.validate()?;
        let model=connection.adapter.ok_or_else(||Error::new(503,"The explicitly configured persistent village requires the selected existing model connection"))?;
        let material = self
            .storage
            .validate(std::slice::from_ref(&self.saves))
            .map_err(|_| invalid())?;
        let lease = Arc::new(StorageLease::acquire(&self.storage.paths()).map_err(|_| invalid())?);
        let save_lease = Store::acquire_lease(&self.saves)?;
        self.preflight_save(material.owner, None)?;
        let clock = Arc::new(SystemBudgetClock);
        let execution = Arc::new(
            SqliteExecutionStore::open(&self.storage.execution, material.protection, clock.clone())
                .await
                .map_err(|_| invalid())?,
        );
        let memory = SqliteMemoryStore::open(&self.storage.memory, clock.clone())
            .await
            .map_err(|_| invalid())?;
        let memory_id = memory.store_id().await.map_err(|_| invalid())?;
        if let Err(error) =
            self.preflight_save(material.owner, Some((execution.store_id(), &memory_id)))
        {
            let _ = memory.shutdown().await;
            let _ = execution.shutdown().await;
            return Err(error);
        }
        let store = Store::open_with_lease(&self.saves, save_lease)?;
        {
            let db = store.db()?;
            db.execute_batch("CREATE TABLE IF NOT EXISTS village_startup_owner(singleton INTEGER PRIMARY KEY CHECK(singleton=1),owner TEXT NOT NULL);").map_err(|_|invalid())?;
            db.execute(
                "INSERT OR IGNORE INTO village_startup_owner VALUES(1,?1)",
                [material.owner.to_string()],
            )
            .map_err(|_| invalid())?;
        }
        let bridge = Arc::new(DurableVillage::new(
            execution,
            memory,
            clock,
            model,
            connection.runtime.provider,
            connection.runtime.model,
        ));
        let mut service = Service::new(store, connection.engine, deadline)
            .with_durable(bridge)
            .await?
            .with_admission(matches!(self.mode, Mode::Active))?;
        service.startup_lease = Some(lease);
        Ok(service)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn startup_config_unavailable_model_fails_before_save_creation() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            version: 1,
            storage: StoragePaths {
                execution: dir.path().join("execution"),
                memory: dir.path().join("memory"),
                owner: dir.path().join("owner"),
                key: dir.path().join("key"),
            },
            saves: dir.path().join("saves"),
            mode: Mode::Draining,
        };
        std::fs::write(&config.storage.owner, uuid::Uuid::new_v4().to_string()).unwrap();
        std::fs::write(&config.storage.key, [81u8; 32]).unwrap();
        let connection = ResolvedConnection {
            engine: None,
            adapter: None,
            runtime: anima_village::RuntimeInfo::unavailable(),
            reason: Some("fixture unavailable".into()),
        };
        assert!(config
            .service(connection, Duration::from_secs(1))
            .await
            .is_err());
        assert!(!dir.path().join("saves").exists());
    }
    struct NoCalls;
    #[async_trait::async_trait]
    impl anima_harness::anima_core::ModelAdapter for NoCalls {
        fn provider(&self) -> &str {
            "fixture"
        }
        fn single_attempt_contract(&self) -> bool {
            true
        }
        async fn generate(
            &self,
            _: &anima_harness::anima_core::AgentConfig,
            _: &anima_harness::anima_core::ModelGenerateRequest,
        ) -> std::result::Result<anima_harness::anima_core::ModelGenerateResponse, String> {
            Err("No model calls belong in startup validation".into())
        }
    }
    fn connection() -> ResolvedConnection {
        ResolvedConnection {
            engine: None,
            adapter: Some(Arc::new(NoCalls)),
            runtime: anima_village::RuntimeInfo {
                mode: "live".into(),
                engine: "anima-harness".into(),
                provider: "fixture".into(),
                model: "fixture".into(),
            },
            reason: None,
        }
    }
    fn pending(path: &std::path::Path) -> (Option<String>, bool) {
        let db =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        (
            db.query_row(
                "SELECT token FROM village_requests WHERE request_id='pending'",
                [],
                |r| r.get(0),
            )
            .unwrap(),
            db.query_row(
                "SELECT active FROM village_runtime_mode WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .unwrap(),
        )
    }
    #[tokio::test]
    async fn startup_wrong_owner_key_and_store_do_not_clear_pending_tokens_or_reset_draining() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            version: 1,
            storage: StoragePaths {
                execution: dir.path().join("execution.sqlite"),
                memory: dir.path().join("memory.sqlite"),
                owner: dir.path().join("owner"),
                key: dir.path().join("key"),
            },
            saves: dir.path().join("saves.sqlite"),
            mode: Mode::Active,
        };
        let owner = uuid::Uuid::new_v4().to_string();
        std::fs::write(&config.storage.owner, &owner).unwrap();
        std::fs::write(&config.storage.key, [82u8; 32]).unwrap();
        let service = config
            .clone()
            .service(connection(), Duration::from_secs(1))
            .await
            .unwrap();
        let save = service.create(anima_village::Language::Ja).unwrap();
        service
            .store
            .begin_profile(
                &save.save_id,
                "pending",
                "fixture",
                crate::engine_bridge::EXECUTION_VERSION,
            )
            .unwrap();
        let service = service.with_admission(false).unwrap();
        let accepted = pending(&config.saves);
        assert!(accepted.0.is_some());
        assert!(!accepted.1);
        service.shutdown().await.unwrap();
        drop(service);
        std::fs::write(&config.storage.owner, uuid::Uuid::new_v4().to_string()).unwrap();
        assert!(config
            .clone()
            .service(connection(), Duration::from_secs(1))
            .await
            .is_err());
        assert_eq!(pending(&config.saves), accepted);
        std::fs::write(&config.storage.owner, &owner).unwrap();
        std::fs::write(&config.storage.key, [83u8; 32]).unwrap();
        assert!(config
            .clone()
            .service(connection(), Duration::from_secs(1))
            .await
            .is_err());
        assert_eq!(pending(&config.saves), accepted);
        std::fs::write(&config.storage.key, [82u8; 32]).unwrap();
        let mut alternate = config.clone();
        alternate.storage.execution = dir.path().join("alternate.sqlite");
        let material = alternate
            .storage
            .validate(std::slice::from_ref(&alternate.saves))
            .unwrap();
        let store = SqliteExecutionStore::open(
            &alternate.storage.execution,
            material.protection,
            Arc::new(SystemBudgetClock),
        )
        .await
        .unwrap();
        store.shutdown().await.unwrap();
        assert!(alternate
            .service(connection(), Duration::from_secs(1))
            .await
            .is_err());
        assert_eq!(pending(&config.saves), accepted);
    }
}
