//! Real-host opt-in configuration. Embeddings never inspect ambient startup state.
use super::*;
use crate::{app::SharedDaemonState, profile_owner::ProfileResource};
use hosted::StoragePaths;
use std::{io, path::PathBuf};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartupConfig {
    version: u32,
    storage: StoragePaths,
    outbox: PathBuf,
    profiles: Vec<SelectedProfile>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedProfile {
    agent_id: String,
    definition_version: u32,
    mode: Mode,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Active,
    Draining,
}
fn invalid() -> io::Error {
    io::Error::other("Durable profile startup configuration is unavailable, invalid or unsupported")
}
impl StartupConfig {
    pub(crate) fn from_env() -> io::Result<Option<Self>> {
        let Some(path) = std::env::var_os("ANIMAOS_RS_DURABLE_PROFILE_FILE") else {
            return Ok(None);
        };
        let path = PathBuf::from(path);
        hosted::safe_path(&path)?;
        let bytes = std::fs::read(&path).map_err(|_| invalid())?;
        if bytes.len() > 65536 {
            return Err(invalid());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        config.validate()?;
        Ok(Some(config))
    }
    fn validate(&self) -> io::Result<()> {
        if self.version != 1 || self.profiles.len() > 256 {
            return Err(invalid());
        }
        let mut ids = std::collections::HashSet::new();
        for profile in &self.profiles {
            if profile.agent_id.is_empty()
                || profile.agent_id.len() > 128
                || profile.agent_id.trim() != profile.agent_id
                || profile.definition_version == 0
                || !ids.insert(&profile.agent_id)
            {
                return Err(invalid());
            }
        }
        self.storage.validate(std::slice::from_ref(&self.outbox))?;
        Ok(())
    }
    pub(crate) fn resources(&self) -> Vec<ProfileResource> {
        vec![
            ProfileResource::Sqlite(self.storage.execution.clone()),
            ProfileResource::Sqlite(self.storage.memory.clone()),
            ProfileResource::Sqlite(self.outbox.clone()),
            ProfileResource::File(self.storage.owner.clone()),
            ProfileResource::File(self.storage.key.clone()),
        ]
    }
    pub(crate) async fn install(self, state: &SharedDaemonState) -> io::Result<()> {
        // Resolve and reject unsupported selections before touching durable stores.
        let (selected, model) = {
            let guard = state.read().await;
            let mut selected = vec![];
            for profile in &self.profiles {
                let runtime = guard.agents.get(&profile.agent_id).ok_or_else(invalid)?;
                let config = runtime.config().clone();
                if config
                    .provider
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
                    || config.model.trim().is_empty()
                    || config.tools.as_ref().is_some_and(|v| !v.is_empty())
                    || config.plugins.as_ref().is_some_and(|v| !v.is_empty())
                    || !config.fallbacks.is_empty()
                    || crate::agent_runs::is_helper_config(&config)
                {
                    return Err(invalid());
                }
                selected.push((profile.clone(), config));
            }
            (selected, guard.profile_model_adapter())
        };
        let material = self.storage.validate(std::slice::from_ref(&self.outbox))?;
        let clock = Arc::new(anima_core::execution_limits::SystemBudgetClock);
        let execution = Arc::new(
            SqliteExecutionStore::open(
                &self.storage.execution,
                material.protection.clone(),
                clock.clone(),
            )
            .await
            .map_err(|_| invalid())?,
        );
        let memory = SqliteMemoryStore::open(&self.storage.memory, clock.clone())
            .await
            .map_err(|_| invalid())?;
        let mut bridge = DaemonEngineBridge::open(
            material.owner,
            execution,
            memory,
            &self.outbox,
            material.protection,
            model,
            clock,
        )
        .await
        .map_err(|_| invalid())?;
        bridge.startup_owns_stores = true;
        bridge.drain_all_profiles().map_err(|_| invalid())?;
        for (profile, config) in selected {
            bridge
                .register_dialogue(&profile.agent_id, config, profile.definition_version)
                .await
                .map_err(|_| invalid())?;
            bridge
                .set_admission(&profile.agent_id, matches!(profile.mode, Mode::Active))
                .map_err(|_| invalid())?;
        }
        state.write().await.engine_bridge = Some(Arc::new(bridge));
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn config(root: &Path) -> StartupConfig {
        StartupConfig {
            version: 1,
            storage: StoragePaths {
                execution: root.join("execution.sqlite"),
                memory: root.join("memory.sqlite"),
                owner: root.join("owner"),
                key: root.join("key"),
            },
            outbox: root.join("outbox.sqlite"),
            profiles: vec![],
        }
    }
    #[test]
    fn engine_bridge_startup_config_is_strict_and_requires_material() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = config(dir.path());
        assert!(c.validate().is_err());
        std::fs::write(&c.storage.owner, Uuid::new_v4().to_string()).unwrap();
        std::fs::write(&c.storage.key, [71u8; 32]).unwrap();
        assert!(c.validate().is_ok());
        c.outbox = c.storage.memory.clone();
        assert!(c.validate().is_err());
        assert!(serde_json::from_value::<StartupConfig>(
            serde_json::json!({"version":1,"unexpected":true})
        )
        .is_err());
    }
    #[tokio::test]
    async fn engine_bridge_startup_unsupported_selection_does_not_open_stores() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = config(dir.path());
        c.profiles.push(SelectedProfile {
            agent_id: "missing".into(),
            definition_version: 1,
            mode: Mode::Active,
        });
        let state = Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        assert!(c.install(&state).await.is_err());
        assert!(!dir.path().join("execution.sqlite").exists());
    }
    struct StartupModel(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait]
    impl ModelAdapter for StartupModel {
        fn provider(&self) -> &str {
            "fixture"
        }
        fn single_attempt_contract(&self) -> bool {
            true
        }
        fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
            true
        }
        async fn generate(
            &self,
            config: &AgentConfig,
            request: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            assert_eq!(config.provider.as_deref(), Some("fixture"));
            assert_eq!(config.model, "selected-model");
            assert!(request.system.contains("Authored startup persona"));
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ModelGenerateResponse {
                content: Content {
                    text: "Accepted".into(),
                    ..Default::default()
                },
                tool_calls: None,
                usage: TokenUsage::default(),
                stop_reason: ModelStopReason::End,
            })
        }
    }
    #[tokio::test]
    async fn engine_bridge_startup_installs_owned_selection_retains_omitted_profiles_and_rejects_drift(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut c = config(dir.path());
        std::fs::write(&c.storage.owner, Uuid::new_v4().to_string()).unwrap();
        std::fs::write(&c.storage.key, [72u8; 32]).unwrap();
        let lease = crate::profile_owner::ProfileOwner::acquire_resources(c.resources()).unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let model = Arc::new(StartupModel(calls.clone()));
        let profile:AgentConfig=serde_json::from_value(serde_json::json!({"name":"Startup fixture","provider":"fixture","model":"selected-model","system":"Authored startup persona"})).unwrap();
        let runtime = AgentRuntime::new(profile, model.clone());
        let id = runtime.state().id.clone();
        let mut state = crate::state::DaemonState::with_model_adapter(model);
        state.profile_owner = Some(lease.clone());
        state.agents.insert(id.clone(), runtime);
        let state = Arc::new(tokio::sync::RwLock::new(state));
        c.profiles.push(SelectedProfile {
            agent_id: id.clone(),
            definition_version: 1,
            mode: Mode::Active,
        });
        c.clone().install(&state).await.unwrap();
        let bridge = state.read().await.engine_bridge.clone().unwrap();
        let request = DialogueRequest {
            request_id: Uuid::new_v4(),
            room_id: Uuid::new_v4(),
            text: "I prefer tea".into(),
        };
        let accepted = bridge.submit(&id, request.clone()).await.unwrap();
        bridge.shutdown_startup_stores().await.unwrap();
        state.write().await.engine_bridge = None;
        drop(bridge);
        let mut omitted = c.clone();
        omitted.profiles.clear();
        omitted.install(&state).await.unwrap();
        let bridge = state.read().await.engine_bridge.clone().unwrap();
        assert!(bridge.handles(&id));
        assert_eq!(bridge.submit(&id, request.clone()).await.unwrap(), accepted);
        assert_eq!(
            bridge
                .submit(
                    &id,
                    DialogueRequest {
                        request_id: Uuid::new_v4(),
                        ..request.clone()
                    }
                )
                .await
                .unwrap_err(),
            BridgeError::Unavailable
        );
        assert!(!bridge.recall(&id, "tea").await.unwrap().is_empty());
        bridge.shutdown_startup_stores().await.unwrap();
        state.write().await.engine_bridge = None;
        drop(bridge);
        let mut changed = state
            .write()
            .await
            .agents
            .get(&id)
            .unwrap()
            .config()
            .clone();
        changed.system = Some("Changed persona with unchanged definition version".into());
        let runtime = AgentRuntime::new(changed, state.read().await.profile_model_adapter());
        state.write().await.agents.insert(id.clone(), runtime);
        assert!(c.clone().install(&state).await.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
