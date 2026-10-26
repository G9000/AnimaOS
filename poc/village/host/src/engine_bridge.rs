//! Opt-in persistent resident profile. The game receipt, not generated prose,
//! owns world state. Execution and canonical memory are separate durable stores.
use crate::{Error, Result, Store};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::anima_core::execution_limits::*;
use anima_harness::{anima_core::*, durable::*};
use anima_memory::{
    CandidateEvidence, EvidenceKind, IdentityRegistry, MemoryAuthority, MemoryCandidate,
    MemoryEffect, MemoryProjectionPolicy, MemoryProjectionPort, RelationshipObservation,
};
use anima_memory_store_sqlite::SqliteMemoryStore;
use anima_village::{
    GameInput, GameState, NpcId, NpcStage, NpcStageExecutor, PlayedTurn, RuntimeInfo,
};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub const EXECUTION_VERSION: &str = "durable-v1";
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS village_engine_identities(save_id TEXT PRIMARY KEY REFERENCES village_saves(id), payload TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS village_engine_requests(save_id TEXT NOT NULL, request_id TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(save_id,request_id), FOREIGN KEY(save_id,request_id) REFERENCES village_requests(save_id,request_id));
        CREATE TABLE IF NOT EXISTS village_engine_stages(save_id TEXT NOT NULL, request_id TEXT NOT NULL, stage TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(save_id,request_id,stage), FOREIGN KEY(save_id,request_id) REFERENCES village_requests(save_id,request_id));
        CREATE TABLE IF NOT EXISTS village_memory_outbox(event_id TEXT NOT NULL, save_id TEXT NOT NULL REFERENCES village_saves(id), observer TEXT NOT NULL, payload TEXT NOT NULL, acknowledged INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(event_id,observer));
        CREATE TABLE IF NOT EXISTS village_request_profiles(save_id TEXT NOT NULL, request_id TEXT NOT NULL, version TEXT NOT NULL, PRIMARY KEY(save_id,request_id), FOREIGN KEY(save_id,request_id) REFERENCES village_requests(save_id,request_id));
        INSERT OR IGNORE INTO village_request_profiles SELECT save_id,request_id,'legacy' FROM village_requests;
        CREATE TABLE IF NOT EXISTS village_memory_edits(save_id TEXT NOT NULL REFERENCES village_saves(id), request_id TEXT NOT NULL, payload TEXT NOT NULL, response TEXT NOT NULL, PRIMARY KEY(save_id,request_id));
        CREATE TABLE IF NOT EXISTS village_engine_attachment(singleton INTEGER PRIMARY KEY CHECK(singleton=1),execution_store TEXT NOT NULL,memory_store TEXT NOT NULL);")
        .map_err(|_| Error::storage())?;
    crate::legacy_memory::migrate(db)?;
    crate::agent_activity::migrate(db)
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::storage())
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|_| Error::storage())
}
fn unavailable() -> Error {
    Error::new(503,"Persistent resident execution or memory is unavailable; retry this exact request or use reviewed recovery")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VillageIdentity {
    pub owner: Uuid,
    pub player: Uuid,
    pub world: Uuid,
    pub session: Uuid,
    pub tavi_session: Uuid,
    pub mira_session: Uuid,
    pub tavi: IdentityBinding,
    pub mira: IdentityBinding,
}
impl VillageIdentity {
    pub fn resident(&self, npc: NpcId) -> &IdentityBinding {
        match npc {
            NpcId::Tavi => &self.tavi,
            NpcId::Mira => &self.mira,
        }
    }
    pub fn session_for(&self, npc: NpcId) -> Uuid {
        match npc {
            NpcId::Tavi => self.tavi_session,
            NpcId::Mira => self.mira_session,
        }
    }
    pub fn authority(&self, npc: NpcId) -> std::result::Result<MemoryAuthority, MemoryPortError> {
        let identity = self.resident(npc).clone();
        let scope = MemoryScope::agent(identity.individual_id().as_str())?;
        MemoryAuthority::trusted(identity, vec![scope.clone()], vec![scope])
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct RequestBinding {
    version: String,
    identities: VillageIdentity,
    allowance: ExecutionScope,
    interpretation: DurableRunBinding,
    narration: DurableRunBinding,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StageBinding {
    pub binding: DurableRunBinding,
    pub definition: AgentDefinition,
    pub input: String,
    pub max_tokens: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AcceptedObservation {
    pub event: String,
    pub identity: IdentityBinding,
    pub text: String,
    pub kind: EvidenceKind,
    pub at_ms: u64,
    #[serde(default)]
    pub sender: Option<IndividualId>,
    #[serde(default)]
    pub relationship_target: Option<IndividualId>,
    #[serde(default)]
    pub revision: Option<MemoryRevision>,
    #[serde(default)]
    pub forget_only: bool,
    #[serde(default)]
    pub callback_memories: anima_village::CallbackMemories,
}
fn identities_in(db: &Connection, save: &str) -> Result<VillageIdentity> {
    let current: Option<String> = db
        .query_row(
            "SELECT payload FROM village_engine_identities WHERE save_id=?1",
            [save],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| Error::storage())?;
    if let Some(current) = current {
        return decode(&current);
    }
    let history: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM village_engine_requests WHERE save_id=?1) OR EXISTS(SELECT 1 FROM village_memory_outbox WHERE save_id=?1)",
            [save],
            |r| r.get(0),
        )
        .map_err(|_| Error::storage())?;
    if history {
        return Err(Error::new(
            503,
            "The persistent resident identity binding is missing",
        ));
    }
    let owner = Uuid::new_v4();
    let world = Uuid::new_v4();
    let ids = VillageIdentity {
        owner,
        world,
        player: Uuid::new_v4(),
        session: Uuid::new_v4(),
        tavi_session: Uuid::new_v4(),
        mira_session: Uuid::new_v4(),
        tavi: IdentityBinding::new(owner.to_string(), format!("village:{world}:tavi"))
            .map_err(|_| Error::storage())?,
        mira: IdentityBinding::new(owner.to_string(), format!("village:{world}:mira"))
            .map_err(|_| Error::storage())?,
    };
    crate::legacy_memory::freeze(db, save, &ids)?;
    db.execute(
        "INSERT INTO village_engine_identities VALUES(?1,?2)",
        params![save, encode(&ids)?],
    )
    .map_err(|_| Error::storage())?;
    Ok(ids)
}

impl Store {
    fn bind_engine_attachment(&self, execution: Uuid, memory: &str) -> Result<()> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let current:Option<(String,String)>=tx.query_row("SELECT execution_store,memory_store FROM village_engine_attachment WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|Error::storage())?;
        if let Some((accepted_execution, accepted_memory)) = current {
            if accepted_execution != execution.to_string() || accepted_memory != memory {
                return Err(Error::new(503,"The persistent village execution or memory store does not match its admitted authority"));
            }
            return Ok(());
        }
        let already_bound:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM village_engine_requests) OR EXISTS(SELECT 1 FROM village_memory_outbox)",[],|r|r.get(0)).map_err(|_|Error::storage())?;
        if already_bound {
            return Err(Error::new(
                503,
                "This village has durable history but its store authority binding is missing",
            ));
        }
        tx.execute(
            "INSERT INTO village_engine_attachment VALUES(1,?1,?2)",
            params![execution.to_string(), memory],
        )
        .map_err(|_| Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(())
    }
    pub fn resident_identities(&self, save: &str) -> Result<VillageIdentity> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        crate::store::load(&tx, save)?;
        let ids = identities_in(&tx, save)?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(ids)
    }
    pub(crate) fn persistent_request(&self, save: &str, request: &str) -> Result<bool> {
        self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM village_request_profiles WHERE save_id=?1 AND request_id=?2 AND version='durable-v1')",params![save,request],|r|r.get(0)).map_err(|_|Error::storage())
    }
    fn request_binding(
        &self,
        save: &str,
        request: &str,
        token: &str,
        npc: NpcId,
        now: u64,
        timeout: u64,
        max_tools: u64,
    ) -> Result<RequestBinding> {
        let identities = self.resident_identities(save)?;
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        ensure_generation(&tx, save, request, token)?;
        let pinned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM village_request_profiles WHERE save_id=?1 AND request_id=?2 AND version='durable-v1')",params![save,request],|r|r.get(0)).map_err(|_|Error::storage())?;
        if !pinned {
            return Err(unavailable());
        }
        let current: Option<String> = tx
            .query_row(
                "SELECT payload FROM village_engine_requests WHERE save_id=?1 AND request_id=?2",
                params![save, request],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| Error::storage())?;
        if let Some(current) = current {
            return decode(&current);
        }
        let scope =
            ApplicationScope::new(identities.owner, identities.world).map_err(|_| unavailable())?;
        let interpretation = DurableRunBinding::new(
            identities.owner,
            identities.resident(npc).clone(),
            identities.session_for(npc),
            Uuid::new_v4(),
            scope.clone(),
        )
        .map_err(|_| unavailable())?;
        let narration = DurableRunBinding::new(
            identities.owner,
            identities.resident(npc).clone(),
            identities.session_for(npc),
            Uuid::new_v4(),
            scope,
        )
        .map_err(|_| unavailable())?;
        let mut limits = ExecutionLimits {
            max_model_attempts: 2,
            max_tool_calls: max_tools,
            max_retries: 0,
            max_inflight_operations: 1,
            ..Default::default()
        };
        limits.budget.max_wall_time_ms = Some(timeout);
        limits.budget.max_input_tokens = Some(32_000);
        limits.budget.max_output_tokens = Some(2_000);
        limits.budget.max_total_tokens = Some(34_000);
        let allowance = ExecutionScope::new(
            format!("village-request:{}", Uuid::new_v4()),
            interpretation.run.to_string(),
            limits,
            now,
        )
        .map_err(|_| unavailable())?;
        let binding = RequestBinding {
            version: EXECUTION_VERSION.into(),
            identities,
            allowance,
            interpretation,
            narration,
        };
        tx.execute(
            "INSERT INTO village_engine_requests VALUES(?1,?2,?3)",
            params![save, request, encode(&binding)?],
        )
        .map_err(|_| Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(binding)
    }
    fn stage_binding(
        &self,
        save: &str,
        request: &str,
        token: &str,
        stage: &str,
        candidate: StageBinding,
    ) -> Result<StageBinding> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        ensure_generation(&tx, save, request, token)?;
        let existing:Option<String>=tx.query_row("SELECT payload FROM village_engine_stages WHERE save_id=?1 AND request_id=?2 AND stage=?3",params![save,request,stage],|r|r.get(0)).optional().map_err(|_|Error::storage())?;
        if let Some(existing) = existing {
            return decode(&existing);
        }
        tx.execute(
            "INSERT INTO village_engine_stages VALUES(?1,?2,?3,?4)",
            params![save, request, stage, encode(&candidate)?],
        )
        .map_err(|_| Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(candidate)
    }
    fn callback_sources(
        &self,
        save: &str,
        identity: &IdentityBinding,
    ) -> Result<Vec<(bool, AcceptedObservation)>> {
        let db = self.db()?;
        let mut sources = vec![];
        for field in ["bread_preference", "helped_recipe"] {
            let path = format!("$.callback_memories.{field}");
            let payload: Option<String> = db.query_row(
                "SELECT payload FROM village_memory_outbox WHERE save_id=?1 AND observer=?2 AND json_extract(payload,?3) IS NOT NULL AND json_extract(payload,?3) != 0 ORDER BY rowid DESC LIMIT 1",
                params![save, identity.individual_id().as_str(), path], |row| row.get(0)
            ).optional().map_err(|_| Error::storage())?;
            if let Some(payload) = payload {
                sources.push((field == "bread_preference", decode(&payload)?));
            }
        }
        Ok(sources)
    }
    pub(crate) fn pending_observations(
        &self,
        identity: &IdentityBinding,
    ) -> Result<Vec<AcceptedObservation>> {
        let db = self.db()?;
        let mut stmt=db.prepare("SELECT payload FROM village_memory_outbox WHERE observer=?1 AND acknowledged=0 ORDER BY rowid LIMIT 64").map_err(|_|Error::storage())?;
        let rows = stmt
            .query_map([identity.individual_id().as_str()], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|_| Error::storage())?;
        rows.map(|r| decode(&r.map_err(|_| Error::storage())?))
            .collect()
    }
    fn acknowledge_observation(&self, event: &str, identity: &IdentityBinding) -> Result<()> {
        self.db()?
            .execute(
                "UPDATE village_memory_outbox SET acknowledged=1 WHERE event_id=?1 AND observer=?2",
                params![event, identity.individual_id().as_str()],
            )
            .map_err(|_| Error::storage())?;
        Ok(())
    }
}
fn ensure_generation(db: &Connection, save: &str, request: &str, token: &str) -> Result<()> {
    let owned:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM village_requests WHERE save_id=?1 AND request_id=?2 AND status='pending' AND token=?3)",params![save,request,token],|r|r.get(0)).map_err(|_|Error::storage())?;
    if owned {
        Ok(())
    } else {
        Err(Error::new(
            409,
            "This generation no longer owns the pending request",
        ))
    }
}
/// Called inside the same winning transaction as snapshot and command receipt.
/// Generated dialogue is never projected as a world fact.
pub(crate) fn accept_observation(
    db: &Connection,
    save: &str,
    request: &str,
    played: &PlayedTurn,
) -> Result<()> {
    let binding: Option<String> = db
        .query_row(
            "SELECT payload FROM village_engine_requests WHERE save_id=?1 AND request_id=?2",
            params![save, request],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| Error::storage())?;
    let Some(binding) = binding else {
        return Ok(());
    };
    let binding: RequestBinding = decode(&binding)?;
    let payload: String = db
        .query_row(
            "SELECT payload FROM village_requests WHERE save_id=?1 AND request_id=?2",
            params![save, request],
            |r| r.get(0),
        )
        .map_err(|_| Error::storage())?;
    let (_, input): (bool, crate::Interaction) = decode(&payload)?;
    let npc = input.npc_id.unwrap_or_default();
    let text = if input.message.trim().is_empty() {
        format!(
            "The traveler selected the server-reviewed action {} with {}.",
            input.choice_id.as_deref().unwrap_or("visit"),
            npc.name()
        )
    } else {
        format!("The traveler said to {}: {}", npc.name(), input.message)
    };
    let event = Uuid::new_v4().to_string();
    let observation = AcceptedObservation {
        event: event.clone(),
        identity: binding.identities.resident(npc).clone(),
        text,
        kind: EvidenceKind::UserStatement,
        at_ms: binding.allowance.created_at_ms,
        sender: None,
        relationship_target: None,
        revision: None,
        forget_only: false,
        callback_memories: played.accepted_memories,
    };
    db.execute(
        "INSERT INTO village_memory_outbox(event_id,save_id,observer,payload) VALUES(?1,?2,?3,?4)",
        params![
            event,
            save,
            observation.identity.individual_id().as_str(),
            encode(&observation)?
        ],
    )
    .map_err(|_| Error::storage())?;
    // Domain state stays authoritative; no memory is inferred for the other resident.
    let previous = crate::store::load(db, save)?;
    crate::agent_activity::accept_supper(
        db,
        save,
        &event,
        &binding.identities,
        &previous.state,
        played,
        binding.allowance.created_at_ms,
    )?;
    Ok(())
}

/// A reviewed winner is its own accepted source, never the abandoned generation.
pub(crate) fn accept_reviewed_observation(
    db: &Connection,
    save: &str,
    original: &str,
    reviewed: &str,
    input: &GameInput,
    played: &PlayedTurn,
    at_ms: u64,
) -> Result<()> {
    let durable:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM village_request_profiles WHERE save_id=?1 AND request_id=?2 AND version='durable-v1')",params![save,original],|r|r.get(0)).map_err(|_|Error::storage())?;
    if !durable {
        return Ok(());
    }
    let ids = identities_in(db, save)?;
    let npc = input.speaker();
    let choice = match input.action() {
        GameInput::Turn {
            choice_id: Some(choice),
            ..
        } => choice.as_str(),
        _ => "visit",
    };
    let event = format!("reviewed:{}", Uuid::new_v4());
    let observation=AcceptedObservation{event:event.clone(),identity:ids.resident(npc).clone(),text:format!("The traveler accepted reviewed recovery {reviewed} with {} using the server-validated action {choice}.",npc.name()),kind:EvidenceKind::WitnessedEvent,at_ms,sender:None,relationship_target:None,revision:None,forget_only:false,callback_memories:played.accepted_memories};
    db.execute(
        "INSERT INTO village_memory_outbox(event_id,save_id,observer,payload) VALUES(?1,?2,?3,?4)",
        params![
            event,
            save,
            observation.identity.individual_id().as_str(),
            encode(&observation)?
        ],
    )
    .map_err(|_| Error::storage())?;
    let previous = crate::store::load(db, save)?;
    crate::agent_activity::accept_supper(db, save, &event, &ids, &previous.state, played, at_ms)
}

pub struct DurableVillage {
    pub execution: Arc<SqliteExecutionStore>,
    pub memory: SqliteMemoryStore,
    pub clock: Arc<dyn ExecutionClock>,
    model: Arc<dyn ModelAdapter>,
    runtime: RuntimeInfo,
    #[cfg(test)]
    capability_profile: Option<Arc<TestCapabilityProfile>>,
}
#[cfg(test)]
pub(crate) struct TestCapabilityProfile {
    pub registry: Arc<CapabilityRegistry>,
    pub capabilities: Vec<ResolvedCapability>,
    pub policy: Arc<dyn Fn(&DurableRunBinding) -> Arc<dyn CurrentPolicyResolver> + Send + Sync>,
}
impl DurableVillage {
    pub fn new(
        execution: Arc<SqliteExecutionStore>,
        memory: SqliteMemoryStore,
        clock: Arc<dyn ExecutionClock>,
        model: Arc<dyn ModelAdapter>,
        provider: String,
        model_name: String,
    ) -> Self {
        Self {
            execution,
            memory,
            clock,
            model,
            runtime: RuntimeInfo {
                mode: "live".into(),
                engine: "anima-harness".into(),
                provider,
                model: model_name,
            },
            #[cfg(test)]
            capability_profile: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_test_capabilities(mut self, profile: TestCapabilityProfile) -> Self {
        self.capability_profile = Some(Arc::new(profile));
        self
    }
    pub fn runtime(&self) -> RuntimeInfo {
        self.runtime.clone()
    }
    fn tool_allowance(&self) -> u64 {
        #[cfg(test)]
        if self.capability_profile.is_some() {
            return 1;
        }
        0
    }
    fn stage_definition(&self, npc: NpcId, system: &str) -> Result<AgentDefinition> {
        let definition = definition(npc, system, &self.runtime)?;
        #[cfg(test)]
        if let Some(profile) = &self.capability_profile {
            let mut definition = definition;
            definition.resolved_capabilities = profile.capabilities.clone();
            definition.source_profile.profile_id = "village-capability-fixture".into();
            definition.limits.max_turns = 4;
            return Ok(definition);
        }
        Ok(definition)
    }
    fn capabilities(&self) -> Arc<CapabilityRegistry> {
        #[cfg(test)]
        if let Some(profile) = &self.capability_profile {
            return profile.registry.clone();
        }
        Arc::new(CapabilityRegistry::new(ManifestCatalog::default()))
    }
    fn current_policy(&self, binding: &DurableRunBinding) -> ResidentPolicy {
        #[cfg(test)]
        if let Some(profile) = &self.capability_profile {
            return ResidentPolicy((profile.policy)(binding));
        }
        let _ = binding;
        ResidentPolicy(Arc::new(NoCapabilities))
    }
    pub async fn attach(&self, store: &Store) -> Result<()> {
        let memory = self.memory.store_id().await.map_err(|_| unavailable())?;
        store.bind_engine_attachment(self.execution.store_id(), &memory)
    }
    async fn register(&self, ids: &VillageIdentity) -> Result<()> {
        for npc in [NpcId::Tavi, NpcId::Mira] {
            self.memory
                .register_identity(ids.resident(npc).clone())
                .await
                .map_err(|_| unavailable())?;
            self.memory
                .register_resident(
                    WorldResidentBinding::new(
                        ids.resident(npc).clone(),
                        ids.world.to_string(),
                        npc.name(),
                    )
                    .map_err(|_| unavailable())?,
                )
                .await
                .map_err(|_| unavailable())?;
        }
        Ok(())
    }
    pub async fn play(
        self: &Arc<Self>,
        store: Arc<Store>,
        save: &str,
        request: &str,
        token: &str,
        state: GameState,
        input: GameInput,
        timeout: u64,
    ) -> Result<PlayedTurn> {
        self.attach(&store).await?;
        let binding = store.request_binding(
            save,
            request,
            token,
            input.speaker(),
            self.clock.now_ms(),
            timeout,
            self.tool_allowance(),
        )?;
        if binding.version != EXECUTION_VERSION {
            return Err(unavailable());
        }
        self.register(&binding.identities).await?;
        let stage = ResidentStages {
            bridge: self.clone(),
            store,
            save: save.into(),
            request: request.into(),
            token: token.into(),
            binding,
        };
        anima_village::play_with(&stage, state, input)
            .await
            .map_err(|_| unavailable())
    }
    pub(crate) async fn callback_memories(
        &self,
        store: &Store,
        save: &str,
        ids: &VillageIdentity,
        npc: NpcId,
    ) -> Result<anima_village::CallbackMemories> {
        self.project(store, ids).await?;
        let mut facts = anima_village::CallbackMemories::default();
        if npc != NpcId::Tavi {
            return Ok(facts);
        }
        let authority = ids.authority(npc).map_err(|_| unavailable())?;
        let bound = self
            .memory
            .bind(authority.clone())
            .await
            .map_err(|_| unavailable())?;
        for (preference_source, source) in store.callback_sources(save, authority.identity())? {
            if source.identity != *authority.identity() {
                return Err(unavailable());
            }
            let id = format!(
                "village:{}:{}",
                source.event,
                source.identity.individual_id().as_str()
            );
            let current = match bound.get_active(&id).await {
                Ok(record) => record,
                Err(error)
                    if matches!(
                        error.code,
                        MemoryPortErrorCode::NotFound
                            | MemoryPortErrorCode::Conflict
                            | MemoryPortErrorCode::UnauthorizedScope
                    ) =>
                {
                    continue
                }
                Err(_) => return Err(unavailable()),
            };
            if current.revision() != 1
                || current.content() != source.text.trim()
                || current.provenance().source_identity() != source.event
            {
                continue;
            }
            if preference_source {
                facts.bread_preference = source.callback_memories.bread_preference;
            } else {
                facts.helped_recipe = source.callback_memories.helped_recipe;
            }
        }
        Ok(facts)
    }
    pub async fn project(&self, store: &Store, ids: &VillageIdentity) -> Result<()> {
        self.attach(store).await?;
        self.register(ids).await?;
        self.import_legacy(store, ids).await?;
        for npc in [NpcId::Tavi, NpcId::Mira] {
            let authority = ids.authority(npc).map_err(|_| unavailable())?;
            let bound = self
                .memory
                .bind(authority.clone())
                .await
                .map_err(|_| unavailable())?;
            loop {
                let pending = store.pending_observations(authority.identity())?;
                if pending.is_empty() {
                    break;
                }
                for observation in pending {
                    if observation.identity != *authority.identity() {
                        return Err(unavailable());
                    }
                    if observation.forget_only {
                        let revision = observation.revision.clone().ok_or_else(unavailable)?;
                        let effect = MemoryEffect::new(
                            &observation.event,
                            observation.identity.clone(),
                            vec![],
                            vec![revision],
                            vec![],
                        )
                        .map_err(|_| unavailable())?;
                        bound
                            .apply_effect(
                                effect
                                    .validate(&authority, &AcceptedPolicy(&observation))
                                    .map_err(|_| unavailable())?,
                            )
                            .await
                            .map_err(|_| unavailable())?;
                        store.acknowledge_observation(&observation.event, &observation.identity)?;
                        continue;
                    }
                    let scope = MemoryScope::agent(observation.identity.individual_id().as_str())
                        .map_err(|_| unavailable())?;
                    let write = MemoryWrite::new(
                        format!(
                            "village:{}:{}",
                            observation.event,
                            observation.identity.individual_id().as_str()
                        ),
                        MemoryKind::Episode,
                        scope,
                        observation.text.clone(),
                        1.0,
                        MemoryProvenance::new(
                            "village-accepted-event",
                            &observation.event,
                            observation.at_ms,
                        )
                        .map_err(|_| unavailable())?,
                    )
                    .map_err(|_| unavailable())?;
                    let candidate = MemoryCandidate::new(
                        write,
                        CandidateEvidence::new(
                            observation.kind,
                            &observation.event,
                            observation.sender.clone(),
                        )
                        .map_err(|_| unavailable())?,
                    )
                    .map_err(|_| unavailable())?;
                    let relationships = match observation.relationship_target.clone() {
                        Some(target) => vec![RelationshipObservation::new(
                            format!(
                                "village:{}:{}",
                                observation.event,
                                observation.identity.individual_id().as_str()
                            ),
                            candidate.write().scope().clone(),
                            target,
                            "supper-preparation",
                            "Discussed preparation of the village supper",
                            0.5,
                            1.0,
                            observation.at_ms,
                            vec![candidate.write().id().into()],
                        )
                        .map_err(|_| unavailable())?],
                        None => vec![],
                    };
                    let effect = MemoryEffect::new(
                        &observation.event,
                        observation.identity.clone(),
                        vec![candidate],
                        observation.revision.clone().into_iter().collect(),
                        relationships,
                    )
                    .map_err(|_| unavailable())?;
                    let validated = effect
                        .validate(&authority, &AcceptedPolicy(&observation))
                        .map_err(|_| unavailable())?;
                    bound
                        .apply_effect(validated)
                        .await
                        .map_err(|_| unavailable())?;
                    store.acknowledge_observation(&observation.event, &observation.identity)?;
                }
            }
        }
        Ok(())
    }
}
impl DurableVillage {
    pub(crate) async fn generate(
        &self,
        accepted: &StageBinding,
        allowance: &ExecutionScope,
        store: Arc<Store>,
        ids: &VillageIdentity,
        npc: NpcId,
    ) -> Result<String> {
        match self
            .generate_outcome(accepted, allowance, store, ids, npc)
            .await?
        {
            EngineRunOutcome::Completed { content }
                if !content.text.trim().is_empty() && content.text.len() <= 16_384 =>
            {
                Ok(content.text)
            }
            _ => Err(unavailable()),
        }
    }
    pub(crate) async fn generate_outcome(
        &self,
        accepted: &StageBinding,
        allowance: &ExecutionScope,
        store: Arc<Store>,
        ids: &VillageIdentity,
        npc: NpcId,
    ) -> Result<EngineRunOutcome> {
        self.attach(&store).await?;
        let port = self
            .execution
            .register_budget(ids.owner, allowance.clone())
            .await
            .map_err(|_| unavailable())?;
        let scope = port.snapshot().await.map_err(|_| unavailable())?;
        let control = DurableControl::new();
        let execution = control
            .execution_context(
                &scope,
                accepted.binding.run,
                Arc::new(port),
                self.clock.clone(),
            )
            .map_err(|_| unavailable())?;
        let authority = ids.authority(npc).map_err(|_| unavailable())?;
        let memory = Arc::new(
            self.memory
                .bind(authority.clone())
                .await
                .map_err(|_| unavailable())?,
        );
        let persistent =
            PersistentAgentMemory::new(authority, memory, Arc::new(OutboxReadiness(store.clone())));
        #[cfg(test)]
        if self.capability_profile.is_some()
            && self
                .execution
                .load_authoritative_policy(
                    ids.owner,
                    &accepted.definition.id,
                    accepted.definition.version,
                )
                .await
                .map_err(|_| unavailable())?
                .is_none()
        {
            self.execution
                .apply_authoritative_policy(
                    ids.owner,
                    AuthoritativePolicyChange::create(
                        AuthoritativePolicyState::active(
                            ids.owner,
                            &accepted.definition.id,
                            accepted.definition.version,
                            1,
                            None,
                        )
                        .map_err(|_| unavailable())?,
                    ),
                )
                .await
                .map_err(|_| unavailable())?;
        }
        let harness = DurableHarness::new(
            self.execution.clone(),
            Arc::new(ResidentModel {
                inner: self.model.clone(),
                max_tokens: accepted.max_tokens,
            }),
            Arc::new(OneDefinition(accepted.definition.clone())),
            Arc::new(self.current_policy(&accepted.binding)),
            self.capabilities(),
            Arc::new(ResidentClock(self.clock.clone())),
            Arc::new(QuietEvents),
        );
        harness
            .admit_persistent(
                &accepted.binding,
                &accepted.definition,
                &accepted.input,
                &execution,
                &persistent,
            )
            .await
            .map_err(|_| unavailable())?;
        harness
            .run_persistent(&accepted.binding, &control, &execution, &persistent)
            .await
            .map_err(|_| unavailable())
    }
}

struct AcceptedPolicy<'a>(&'a AcceptedObservation);
impl MemoryProjectionPolicy for AcceptedPolicy<'_> {
    fn validate(
        &self,
        authority: &MemoryAuthority,
        candidate: &MemoryCandidate,
    ) -> std::result::Result<(), MemoryPortError> {
        if authority.identity() != &self.0.identity
            || candidate.evidence().source_event_id() != self.0.event
            || candidate.write().content() != self.0.text.trim()
            || candidate.evidence().kind() != self.0.kind
        {
            return Err(MemoryPortError::invalid(
                "observation is not an accepted host event",
            ));
        }
        Ok(())
    }
    fn validate_relationship(
        &self,
        _: &MemoryAuthority,
        relationship: &RelationshipObservation,
    ) -> std::result::Result<(), MemoryPortError> {
        if self.0.relationship_target.as_ref() != Some(relationship.target())
            || relationship.evidence_memory_ids()
                != [format!(
                    "village:{}:{}",
                    self.0.event,
                    self.0.identity.individual_id().as_str()
                )]
        {
            return Err(MemoryPortError::invalid(
                "relationship is not supported by this accepted event",
            ));
        }
        Ok(())
    }
    fn validate_revision(
        &self,
        _: &MemoryAuthority,
        revision: &MemoryRevision,
    ) -> std::result::Result<(), MemoryPortError> {
        if self.0.revision.as_ref() != Some(revision) {
            return Err(MemoryPortError::invalid(
                "revision is not this accepted player action",
            ));
        }
        Ok(())
    }
}
struct ResidentStages {
    bridge: Arc<DurableVillage>,
    store: Arc<Store>,
    save: String,
    request: String,
    token: String,
    binding: RequestBinding,
}
#[async_trait]
impl NpcStageExecutor for ResidentStages {
    fn runtime(&self) -> RuntimeInfo {
        self.bridge.runtime()
    }
    fn requires_persistent_memory(&self) -> bool {
        true
    }
    async fn callback_memories(
        &self,
        npc: NpcId,
    ) -> std::result::Result<anima_village::CallbackMemories, String> {
        self.bridge
            .callback_memories(&self.store, &self.save, &self.binding.identities, npc)
            .await
            .map_err(|error| error.message)
    }
    async fn call(
        &self,
        stage: NpcStage,
        npc: NpcId,
        system: &str,
        input: &str,
        max_tokens: u32,
    ) -> std::result::Result<String, String> {
        self.call_stage(stage, npc, system, input, max_tokens)
            .await
            .map_err(|e| e.message)
    }
}
impl ResidentStages {
    async fn call_stage(
        &self,
        stage: NpcStage,
        npc: NpcId,
        system: &str,
        input: &str,
        max_tokens: u32,
    ) -> Result<String> {
        self.bridge
            .project(&self.store, &self.binding.identities)
            .await?;
        let (name, binding) = match stage {
            NpcStage::Interpretation => ("interpretation", self.binding.interpretation.clone()),
            NpcStage::Narration => ("narration", self.binding.narration.clone()),
        };
        if binding.individual != *self.binding.identities.resident(npc) {
            return Err(unavailable());
        }
        let candidate = StageBinding {
            definition: self.bridge.stage_definition(npc, system)?,
            binding,
            input: input.into(),
            max_tokens,
        };
        let accepted =
            self.store
                .stage_binding(&self.save, &self.request, &self.token, name, candidate)?;
        self.bridge
            .generate(
                &accepted,
                &self.binding.allowance,
                self.store.clone(),
                &self.binding.identities,
                npc,
            )
            .await
    }
}
pub(crate) fn definition(
    npc: NpcId,
    system: &str,
    runtime: &RuntimeInfo,
) -> Result<AgentDefinition> {
    serde_json::from_value(serde_json::json!({"schema_version":1,"id":format!("village-{}",npc.name()),"version":1,"name":npc.name(),"display_name":npc.name(),"description":"Persistent village resident","persona":"You are a village resident.","system":system,
        "model":{"provider":runtime.provider,"model":runtime.model,"credential_reference":null,"temperature":null},"source_profile":{"profile_id":"village-dialogue","profile_version":1},"resolved_capabilities":[],"memory":{"enabled":true,"namespace":"village-private","retention_days":null},"approval_policy_id":"village-host","approval_policy_revision":1,"approval_restrictions":[],"limits":{"max_turns":1,"timeout_ms":60000,"max_concurrent_tasks":1},"lifecycle":{"auto_start":false,"restart_on_failure":false,"max_restarts":0,"allows_concurrent_sessions":false},"host_requirements":[]})).map_err(|_|unavailable())
}
struct OneDefinition(AgentDefinition);
#[async_trait]
impl DefinitionResolver for OneDefinition {
    async fn resolve(
        &self,
        _: Uuid,
        pin: &DefinitionPin,
    ) -> std::result::Result<AgentDefinition, EngineError> {
        if DefinitionPin::from_definition(&self.0)
            .map_err(|_| EngineError::new(EngineErrorCode::DefinitionUnavailable))?
            != *pin
        {
            return Err(EngineError::new(EngineErrorCode::DefinitionUnavailable));
        }
        Ok(self.0.clone())
    }
}
struct NoCapabilities;
struct ResidentPolicy(Arc<dyn CurrentPolicyResolver>);
#[async_trait]
impl CurrentPolicyResolver for ResidentPolicy {
    async fn resolve(
        &self,
        request: EnginePolicyRequest,
    ) -> std::result::Result<CurrentPolicyResolution, EngineError> {
        self.0.resolve(request).await
    }
}
#[async_trait]
impl CurrentPolicyResolver for NoCapabilities {
    async fn resolve(
        &self,
        _: EnginePolicyRequest,
    ) -> std::result::Result<CurrentPolicyResolution, EngineError> {
        Err(EngineError::new(EngineErrorCode::Policy))
    }
}
struct QuietEvents;
#[async_trait]
impl EngineLiveEventSink for QuietEvents {
    async fn emit(&self, _: EngineLiveEvent) -> std::result::Result<(), EngineError> {
        Ok(())
    }
}
struct ResidentClock(Arc<dyn ExecutionClock>);
impl ExecutionClock for ResidentClock {
    fn now_ms(&self) -> u64 {
        self.0.now_ms()
    }
}
struct ResidentModel {
    inner: Arc<dyn ModelAdapter>,
    max_tokens: u32,
}
#[async_trait]
impl ModelAdapter for ResidentModel {
    fn provider(&self) -> &str {
        self.inner.provider()
    }
    fn single_attempt_contract(&self) -> bool {
        self.inner.single_attempt_contract()
    }
    fn generated_usage_is_complete(&self, response: &ModelGenerateResponse) -> bool {
        self.inner.generated_usage_is_complete(response)
    }
    async fn generate(
        &self,
        c: &AgentConfig,
        r: &ModelGenerateRequest,
    ) -> std::result::Result<ModelGenerateResponse, String> {
        let mut r = r.clone();
        r.max_tokens = Some(r.max_tokens.unwrap_or(self.max_tokens).min(self.max_tokens));
        self.inner.generate(c, &r).await
    }
    async fn stream_controlled(
        &self,
        c: &AgentConfig,
        r: &ModelGenerateRequest,
        s: &dyn ModelStreamSink,
        context: &ModelCallContext,
    ) -> std::result::Result<(), String> {
        let mut r = r.clone();
        r.max_tokens = Some(r.max_tokens.unwrap_or(self.max_tokens).min(self.max_tokens));
        self.inner.stream_controlled(c, &r, s, context).await
    }
}
struct OutboxReadiness(Arc<Store>);
#[async_trait]
impl MemoryProjectionReadiness for OutboxReadiness {
    async fn ensure_projected(
        &self,
        identity: &IdentityBinding,
    ) -> std::result::Result<(), MemoryPortError> {
        if !self
            .0
            .pending_observations(identity)
            .map_err(|_| MemoryPortError::unsupported("memory event source unavailable"))?
            .is_empty()
        {
            return Err(MemoryPortError::unsupported(
                "accepted observations still pending",
            ));
        }
        Ok(())
    }
}
