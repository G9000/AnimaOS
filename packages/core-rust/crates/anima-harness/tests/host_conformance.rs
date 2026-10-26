#![cfg(feature = "test-support")]
use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::{conformance::*, durable::*};
use anima_memory::{IdentityRegistry, MemoryAuthority};
use anima_memory_store_sqlite::SqliteMemoryStore;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
mod common;

struct Ready;
#[async_trait]
impl MemoryProjectionReadiness for Ready {
    async fn ensure_projected(&self, _: &IdentityBinding) -> Result<(), MemoryPortError> {
        Ok(())
    }
}
struct ReferenceHost {
    directory: tempfile::TempDir,
    owner: Uuid,
    clock: Arc<ManualExecutionClock>,
    misconfigured: bool,
}
impl ReferenceHost {
    fn new(misconfigured: bool) -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
            owner: Uuid::new_v4(),
            clock: Arc::new(ManualExecutionClock::default()),
            misconfigured,
        }
    }
    fn identity(&self) -> IdentityBinding {
        IdentityBinding::new(self.owner.to_string(), "reference-assistant").unwrap()
    }
    fn authority(&self) -> MemoryAuthority {
        let scope = MemoryScope::agent("reference-assistant").unwrap();
        MemoryAuthority::trusted(self.identity(), vec![scope.clone()], vec![scope]).unwrap()
    }
    async fn memory(&self) -> SqliteMemoryStore {
        let memory = SqliteMemoryStore::open(
            self.directory.path().join("memory.sqlite"),
            self.clock.clone(),
        )
        .await
        .unwrap();
        memory.register_identity(self.identity()).await.unwrap();
        memory
    }
}
#[async_trait]
impl PersistentHostFixture for ReferenceHost {
    async fn submit(&mut self, request: &str, input: &str) -> Result<HostReply, String> {
        let store = Arc::new(
            SqliteExecutionStore::open(
                self.directory.path().join("execution.sqlite"),
                PersistenceProtection::allow_model_payloads(
                    PersistenceSnapshotSealKey::new([42; 32]).unwrap(),
                    vec![],
                )
                .unwrap(),
                self.clock.clone(),
            )
            .await
            .map_err(|e| e.to_string())?,
        );
        let run = Uuid::new_v5(&self.owner, request.as_bytes());
        let session = Uuid::new_v5(&run, b"session");
        let binding = DurableRunBinding::new(
            self.owner,
            self.identity(),
            session,
            run,
            ApplicationScope::new(self.owner, Uuid::new_v5(&self.owner, b"scope")).unwrap(),
        )
        .unwrap();
        let scope_id = run.to_string();
        let port = match store.bind_budget(self.owner, &scope_id).await {
            Ok(port) => port,
            Err(_) => store
                .register_budget(
                    self.owner,
                    ExecutionScope::new(
                        scope_id,
                        run.to_string(),
                        ExecutionLimits::default(),
                        self.clock.now_ms(),
                    )
                    .unwrap(),
                )
                .await
                .map_err(|e| e.to_string())?,
        };
        let scope = port.snapshot().await.map_err(|e| e.to_string())?;
        let control = DurableControl::new();
        let execution = control
            .execution_context(&scope, run, Arc::new(port), self.clock.clone())
            .unwrap();
        let memory = self.memory().await;
        let bound = memory.bind(self.authority()).await.unwrap();
        let persistent =
            PersistentAgentMemory::new(self.authority(), Arc::new(bound), Arc::new(Ready));
        let mut definition = common::definition();
        definition.memory.enabled = true;
        let harness = DurableHarness::new(
            store.clone(),
            Arc::new(common::Model(Arc::new(Mutex::new(vec![])))),
            Arc::new(common::Definitions(definition.clone())),
            Arc::new(common::Policy),
            Arc::new(CapabilityRegistry::new(ManifestCatalog::default())),
            self.clock.clone(),
            Arc::new(common::Live),
        );
        let result = async {
            harness
                .admit_persistent(&binding, &definition, input, &execution, &persistent)
                .await
                .map_err(|e| e.to_string())?;
            let outcome = harness
                .run_persistent(&binding, &control, &execution, &persistent)
                .await
                .map_err(|e| e.to_string())?;
            let EngineRunOutcome::Completed { content } = outcome else {
                return Err("host did not complete".into());
            };
            Ok(HostReply {
                run_id: if self.misconfigured {
                    Uuid::new_v4().to_string()
                } else {
                    run.to_string()
                },
                individual_id: self.identity().individual_id().as_str().into(),
                text: content.text,
            })
        }
        .await;
        store.shutdown().await.map_err(|e| e.to_string())?;
        memory.shutdown().await.map_err(|e| e.to_string())?;
        result
    }
    async fn accept_preference(
        &mut self,
        event: &str,
        text: &str,
    ) -> Result<AcceptedPreference, String> {
        let memory = self.memory().await;
        let bound = memory.bind(self.authority()).await.unwrap();
        let id = format!("memory:{event}");
        let write = MemoryWrite::new(
            &id,
            MemoryKind::Preference,
            MemoryScope::agent("reference-assistant").unwrap(),
            text,
            1.0,
            MemoryProvenance::new("accepted-user-statement", event, self.clock.now_ms()).unwrap(),
        )
        .unwrap();
        let result = bound.write(write).await.map_err(|e| e.to_string());
        memory.shutdown().await.map_err(|e| e.to_string())?;
        result.map(|_| AcceptedPreference {
            memory_id: id,
            source_event_id: event.into(),
        })
    }
    async fn recall(&mut self, text: &str) -> Result<Vec<MemoryRecord>, String> {
        let memory = self.memory().await;
        let bound = memory.bind(self.authority()).await.unwrap();
        let authority = self.authority();
        let result = bound
            .query(
                &authority.access_context().unwrap(),
                MemoryQuery::new(text, authority.read_scopes().to_vec(), 16).unwrap(),
            )
            .await
            .map_err(|e| e.to_string());
        memory.shutdown().await.map_err(|e| e.to_string())?;
        result.map(|result| {
            result
                .hits()
                .iter()
                .map(|hit| hit.record().clone())
                .collect()
        })
    }
    async fn reopen(&mut self) -> Result<(), String> {
        Ok(())
    } // each boundary already cold-opens the same files
}
#[tokio::test]
async fn reference_host_passes_shared_persistent_basics() {
    assert_persistent_host_basics(&mut ReferenceHost::new(false))
        .await
        .unwrap();
}
#[tokio::test]
async fn suite_rejects_deliberately_regenerated_retry_identity() {
    assert!(assert_persistent_host_basics(&mut ReferenceHost::new(true))
        .await
        .unwrap_err()
        .contains("exact request retry"));
}
