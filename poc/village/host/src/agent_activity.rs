//! One authored supper exchange; no offline simulation or general scheduler.
use crate::{
    engine_bridge::{self, StageBinding, VillageIdentity},
    Error, Result, Service, Store,
};
use anima_harness::anima_core::{execution_limits::*, EngineRunOutcome, ExecutionStore, RunState};
use anima_harness::durable::*;
use anima_village::{GameState, Language, NpcId, Phase, PlayedTurn};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const PRESENCE_TTL_MS: u64 = 30_000;
const SUPPER_FACT: &str =
    "Tavi prepared Mira's two loaves for the village supper of vegetable soup and bread.";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub session_id: String,
    pub expires_at_ms: u64,
    pub world_time_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidentReply {
    pub event_id: String,
    pub npc_id: NpcId,
    pub text: String,
    pub translation: String,
    pub reading: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    #[default]
    Pending,
    Completed,
    BudgetExhausted,
    RecoveryRequired,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub interaction_id: String,
    pub source_event_id: String,
    pub replies: Vec<ResidentReply>,
    pub complete: bool,
    pub status: ActivityStatus,
    pub terminal_reason: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct SupperActivity {
    id: String,
    source: String,
    ids: VillageIdentity,
    allowance: ExecutionScope,
    stages: Vec<StageBinding>,
    replies: Vec<ResidentReply>,
    authored_replies: Vec<ResidentReply>,
    language: Language,
    #[serde(default)]
    status: ActivityStatus,
    #[serde(default)]
    terminal_reason: Option<String>,
}
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS village_presence(save_id TEXT PRIMARY KEY REFERENCES village_saves(id),session_id TEXT NOT NULL,expires_ms INTEGER NOT NULL,last_tick_ms INTEGER NOT NULL,world_ms INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS village_supper_activity(id TEXT PRIMARY KEY,save_id TEXT NOT NULL UNIQUE REFERENCES village_saves(id),source_event TEXT NOT NULL UNIQUE,payload TEXT NOT NULL,token TEXT,complete INTEGER NOT NULL DEFAULT 0);") .map_err(|_|Error::storage())?;
    // A restarted host has no visible browser until a new server-owned presence.
    db.execute("UPDATE village_presence SET expires_ms=0", [])
        .map_err(|_| Error::storage())?;
    db.execute("UPDATE village_supper_activity SET token=NULL", [])
        .map_err(|_| Error::storage())?;
    Ok(())
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| Error::storage())
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| Error::storage())
}
impl Store {
    pub(crate) fn presence_active(&self, save: &str, now: u64) -> Result<bool> {
        self.db()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM village_presence WHERE save_id=?1 AND expires_ms>?2)",
                params![save, now],
                |r| r.get(0),
            )
            .map_err(|_| Error::storage())
    }
    pub fn open_presence(&self, save: &str, now: u64) -> Result<Presence> {
        self.get(save)?;
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let previous: Option<(u64, u64, u64)> = tx
            .query_row(
                "SELECT expires_ms,last_tick_ms,world_ms FROM village_presence WHERE save_id=?1",
                [save],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| Error::storage())?;
        let world = previous
            .map(|(expires, last, world)| {
                world.saturating_add(now.min(expires).saturating_sub(last))
            })
            .unwrap_or(0);
        let session = Uuid::new_v4().to_string();
        let expires = now.saturating_add(PRESENCE_TTL_MS);
        tx.execute("INSERT INTO village_presence VALUES(?1,?2,?3,?4,?5) ON CONFLICT(save_id) DO UPDATE SET session_id=excluded.session_id,expires_ms=excluded.expires_ms,last_tick_ms=excluded.last_tick_ms,world_ms=excluded.world_ms",params![save,session,expires,now,world]).map_err(|_|Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(Presence {
            session_id: session,
            expires_at_ms: expires,
            world_time_ms: world,
        })
    }
    pub fn renew_presence(
        &self,
        save: &str,
        session: &str,
        now: u64,
        close: bool,
    ) -> Result<Presence> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let row:Option<(String,u64,u64,u64)>=tx.query_row("SELECT session_id,expires_ms,last_tick_ms,world_ms FROM village_presence WHERE save_id=?1",[save],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|Error::storage())?;
        let Some((stored, expires, last, world)) = row else {
            return Err(Error::new(
                410,
                "Open this village session before sending presence",
            ));
        };
        // A replaced tab must not reopen and steal the current session. Empty
        // is a host-only closed marker: issued session IDs are always UUIDs.
        if stored.is_empty() || stored != session {
            return Err(Error::new(
                409,
                "This village session has closed or been replaced",
            ));
        }
        if expires <= now {
            return Err(Error::new(
                410,
                "This village session has expired; reopen it",
            ));
        }
        let world = world.saturating_add(now.saturating_sub(last));
        let expires = if close {
            now
        } else {
            now.saturating_add(PRESENCE_TTL_MS)
        };
        let retained_session = if close { "" } else { session };
        tx.execute("UPDATE village_presence SET expires_ms=?1,last_tick_ms=?2,world_ms=?3,session_id=?4 WHERE save_id=?5",params![expires,now,world,retained_session,save]).map_err(|_|Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(Presence {
            session_id: session.into(),
            expires_at_ms: expires,
            world_time_ms: world,
        })
    }
    pub fn activity(&self, save: &str) -> Result<Option<ActivityView>> {
        self.get(save)?;
        let payload: Option<String> = self
            .db()?
            .query_row(
                "SELECT payload FROM village_supper_activity WHERE save_id=?1",
                [save],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| Error::storage())?;
        payload
            .map(|payload| {
                let a: SupperActivity = decode(&payload)?;
                let complete = a.stages.len() == 2 && a.replies.len() == 2;
                let status = if complete {
                    ActivityStatus::Completed
                } else {
                    a.status
                };
                Ok(ActivityView {
                    interaction_id: a.id,
                    source_event_id: a.source,
                    replies: a.replies,
                    complete,
                    status,
                    terminal_reason: a.terminal_reason,
                })
            })
            .transpose()
    }
    fn claim_activity(&self, save: &str, now: u64) -> Result<Option<(SupperActivity, String)>> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let present: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM village_presence WHERE save_id=?1 AND expires_ms>?2)",
                params![save, now],
                |r| r.get(0),
            )
            .map_err(|_| Error::storage())?;
        if !present {
            return Ok(None);
        }
        let player_busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM village_requests WHERE save_id=?1 AND status='pending')",[save],|r|r.get(0)).map_err(|_|Error::storage())?;
        if player_busy {
            return Ok(None);
        }
        let row: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT payload,token FROM village_supper_activity WHERE save_id=?1 AND complete=0",
                [save],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| Error::storage())?;
        let Some((payload, None)) = row else {
            return Ok(None);
        };
        let activity: SupperActivity = decode(&payload)?;
        if activity.status != ActivityStatus::Pending {
            return Ok(None);
        }
        let token = Uuid::new_v4().to_string();
        tx.execute(
            "UPDATE village_supper_activity SET token=?1 WHERE id=?2",
            params![token, activity.id],
        )
        .map_err(|_| Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(Some((activity, token)))
    }
    fn release_activity(&self, id: &str, token: &str) -> Result<()> {
        self.db()?
            .execute(
                "UPDATE village_supper_activity SET token=NULL WHERE id=?1 AND token=?2",
                params![id, token],
            )
            .map_err(|_| Error::storage())?;
        Ok(())
    }
    fn finish_activity(
        &self,
        activity: &mut SupperActivity,
        token: &str,
        status: ActivityStatus,
        reason: &str,
    ) -> Result<()> {
        activity.status = status;
        activity.terminal_reason = Some(reason.into());
        let changed=self.db()?.execute("UPDATE village_supper_activity SET payload=?1,complete=1,token=NULL WHERE id=?2 AND token=?3",params![encode(activity)?,activity.id,token]).map_err(|_|Error::storage())?;
        if changed != 1 {
            return Err(Error::new(
                409,
                "This resident activity no longer owns its completion",
            ));
        }
        Ok(())
    }
    fn commit_reply(
        &self,
        save: &str,
        token: &str,
        activity: &mut SupperActivity,
        reply: ResidentReply,
    ) -> Result<()> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let current: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT payload,token FROM village_supper_activity WHERE id=?1 AND save_id=?2",
                params![activity.id, save],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| Error::storage())?;
        let Some((payload, current_token)) = current else {
            return Err(Error::storage());
        };
        let stored: SupperActivity = decode(&payload)?;
        if current_token.as_deref() != Some(token) || stored.replies.len() != activity.replies.len()
        {
            return Err(Error::new(
                409,
                "This resident reply no longer owns its activity",
            ));
        }
        let speaker = reply.npc_id;
        let other = if speaker == NpcId::Tavi {
            NpcId::Mira
        } else {
            NpcId::Tavi
        };
        for observer in [speaker, other] {
            let event = reply.event_id.clone();
            let observation = engine_bridge::AcceptedObservation {
                event: event.clone(),
                identity: activity.ids.resident(observer).clone(),
                text: SUPPER_FACT.into(),
                kind: if observer == speaker {
                    anima_memory::EvidenceKind::WitnessedEvent
                } else {
                    anima_memory::EvidenceKind::ReceivedMessage
                },
                at_ms: activity.allowance.created_at_ms,
                sender: if observer == speaker {
                    None
                } else {
                    Some(activity.ids.resident(speaker).individual_id().clone())
                },
                relationship_target: Some(
                    activity
                        .ids
                        .resident(if observer == speaker { other } else { speaker })
                        .individual_id()
                        .clone(),
                ),
                revision: None,
                forget_only: false,
                callback_memories: Default::default(),
            };
            tx.execute("INSERT INTO village_memory_outbox(event_id,save_id,observer,payload) VALUES(?1,?2,?3,?4)",params![event,save,observation.identity.individual_id().as_str(),encode(&observation)?]).map_err(|_|Error::storage())?;
        }
        activity.replies.push(reply);
        if activity.replies.len() == 2 {
            activity.status = ActivityStatus::Completed;
        }
        tx.execute(
            "UPDATE village_supper_activity SET payload=?1,complete=?2 WHERE id=?3",
            params![encode(activity)?, activity.replies.len() == 2, activity.id],
        )
        .map_err(|_| Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(())
    }
}
/// The sole authored activation is an accepted delivery changing Day to Evening.
pub(crate) fn accept_supper(
    db: &Connection,
    save: &str,
    event: &str,
    ids: &VillageIdentity,
    previous: &GameState,
    played: &PlayedTurn,
    now: u64,
) -> Result<()> {
    if !Store::new_admissions(db)? {
        return Ok(());
    }
    if previous.phase != Phase::Day
        || played.state.phase != Phase::Evening
        || played.state.npc_id != NpcId::Mira
    {
        return Ok(());
    }
    let id = Uuid::new_v4().to_string();
    let mut limits = ExecutionLimits {
        max_model_attempts: 2,
        max_tool_calls: 0,
        max_retries: 0,
        max_inflight_operations: 1,
        ..Default::default()
    };
    limits.budget.max_wall_time_ms = Some(60_000);
    limits.budget.max_input_tokens = Some(32_000);
    limits.budget.max_output_tokens = Some(2_000);
    limits.budget.max_total_tokens = Some(34_000);
    let scope = ApplicationScope::new(ids.owner, ids.world).map_err(|_| Error::storage())?;
    let mut stages = vec![];
    for npc in [NpcId::Tavi, NpcId::Mira] {
        let binding = DurableRunBinding::new(
            ids.owner,
            ids.resident(npc).clone(),
            ids.session_for(npc),
            Uuid::new_v4(),
            scope.clone(),
        )
        .map_err(|_| Error::storage())?;
        let language = if played.state.language == Language::Ja {
            "Japanese"
        } else {
            "Mandarin Chinese (simplified characters)"
        };
        let instruction = if npc == NpcId::Tavi {
            "Deliberately tell Mira the supplied supper-preparation fact."
        } else {
            "Respond to Tavi about the supplied supper-preparation fact."
        };
        let system=format!("You are {} in a village language-learning game. {instruction} Speak one concise beginner-friendly sentence in {language}. Return ONLY JSON with text (target language), translation (English), reading (romaji or pinyin), expression (warm|thoughtful|happy). You have no tools or world authority. Share only this host-authored public fact: {SUPPER_FACT}. Private player preferences and private conversations are excluded.",npc.name());
        stages.push(StageBinding {
            definition: engine_bridge::definition(npc, &system, &played.runtime)?,
            binding,
            input: SUPPER_FACT.into(),
            max_tokens: 900,
        });
    }
    let allowance = ExecutionScope::new(
        format!("village-supper:{id}"),
        stages[0].binding.run.to_string(),
        limits,
        now,
    )
    .map_err(|_| Error::storage())?;
    let authored_replies = [NpcId::Tavi, NpcId::Mira]
        .into_iter()
        .map(|npc| authored_reply(npc, played.state.language))
        .collect();
    let activity = SupperActivity {
        id: id.clone(),
        source: event.into(),
        ids: ids.clone(),
        allowance,
        stages,
        replies: vec![],
        authored_replies,
        language: played.state.language,
        status: ActivityStatus::Pending,
        terminal_reason: None,
    };
    db.execute("INSERT OR IGNORE INTO village_supper_activity(id,save_id,source_event,payload) VALUES(?1,?2,?3,?4)",params![id,save,event,encode(&activity)?]).map_err(|_|Error::storage())?;
    Ok(())
}
impl Service {
    pub async fn run_resident_activity(&self, save: &str) -> Result<Option<ActivityView>> {
        let _work = self.track()?;
        // Keep the large durable-stage future off this custody wrapper's stack.
        Box::pin(self.run_resident_activity_reserved(save)).await
    }
    pub(crate) async fn run_resident_activity_reserved(
        &self,
        save: &str,
    ) -> Result<Option<ActivityView>> {
        let Some(bridge) = &self.durable else {
            return Ok(None);
        };
        // Each admission checks current server presence. Current work settles if
        // the browser closes, while the next reply remains paused until reopen.
        // One resident turn per activation; a later visible-session heartbeat
        // resumes the same interaction for the other resident.
        if let Some((mut activity, token)) =
            self.store.claim_activity(save, bridge.clock.now_ms())?
        {
            let index = activity.replies.len();
            if index >= 2 {
                self.store.release_activity(&activity.id, &token)?;
                return self.store.activity(save);
            }
            let npc = if index == 0 { NpcId::Tavi } else { NpcId::Mira };
            let outcome = async {
                bridge.project(&self.store, &activity.ids).await?;
                let accepted = &activity.stages[index];
                let generated = bridge
                    .supper_outcome(
                        accepted,
                        &activity.allowance,
                        self.store.clone(),
                        &activity.ids,
                        npc,
                    )
                    .await?;
                let generated = match generated {
                    EngineRunOutcome::Completed { content }
                        if !content.text.trim().is_empty() && content.text.len() <= 16_384 =>
                    {
                        content.text
                    }
                    EngineRunOutcome::PausedForBudget => {
                        let reason = if bridge.clock.now_ms() >= activity.allowance.deadline_at_ms {
                            "The supper conversation reached its time limit."
                        } else {
                            "The supper conversation reached its model allowance."
                        };
                        self.store.finish_activity(
                            &mut activity,
                            &token,
                            ActivityStatus::BudgetExhausted,
                            reason,
                        )?;
                        return Ok(());
                    }
                    _ => {
                        self.store.finish_activity(
                            &mut activity,
                            &token,
                            ActivityStatus::RecoveryRequired,
                            "The supper conversation needs review before it can continue.",
                        )?;
                        return Ok(());
                    }
                };
                if anima_village::parse_generated_dialogue(activity.language, &generated).is_err() {
                    self.store.finish_activity(
                        &mut activity,
                        &token,
                        ActivityStatus::RecoveryRequired,
                        "The supper conversation needs review before it can continue.",
                    )?;
                    return Ok(());
                }
                // As with the existing chapter callback, a valid generation is
                // required but only the admitted authored utterance may become
                // a shared event. Model text cannot disclose a private claim.
                let reply = activity.authored_replies[index].clone();
                self.store
                    .commit_reply(save, &token, &mut activity, reply)?;
                bridge.project(&self.store, &activity.ids).await?;
                Ok::<_, Error>(())
            }
            .await;
            self.store.release_activity(&activity.id, &token)?;
            outcome?;
        }
        let current = self.store.activity(save)?;
        if current.is_some() {
            // Terminal scheduling never reclaims/replays a resident reply, but
            // accepted game events still own a retryable canonical-memory outbox.
            let ids = self.store.resident_identities(save)?;
            bridge.project(&self.store, &ids).await?;
        }
        Ok(current)
    }
    pub fn open_game_presence(&self, save: &str) -> Result<Presence> {
        let bridge = self
            .durable
            .as_ref()
            .ok_or_else(|| Error::new(503, "Resident activity is not enabled for this profile"))?;
        self.store.open_presence(save, bridge.clock.now_ms())
    }
    pub fn update_game_presence(&self, save: &str, session: &str, close: bool) -> Result<Presence> {
        let bridge = self
            .durable
            .as_ref()
            .ok_or_else(|| Error::new(503, "Resident activity is not enabled for this profile"))?;
        self.store
            .renew_presence(save, session, bridge.clock.now_ms(), close)
    }
}
fn authored_reply(npc: NpcId, language: Language) -> ResidentReply {
    let (text, translation, reading) = match (npc, language) {
        (NpcId::Tavi, Language::Ja) => (
            "ミラ、夕食のパンを二つ用意したよ。",
            "Mira, I prepared your two loaves for supper.",
            "Mira, yūshoku no pan o futatsu yōi shita yo.",
        ),
        (NpcId::Mira, Language::Ja) => (
            "ありがとう、タヴィ。野菜スープと一緒に出すね。",
            "Thank you, Tavi. I will serve them with vegetable soup.",
            "Arigatō, Tavi. Yasai sūpu to issho ni dasu ne.",
        ),
        (NpcId::Tavi, Language::Zh) => (
            "米拉，晚餐的两个面包准备好了。",
            "Mira, I prepared your two loaves for supper.",
            "Mǐlā, wǎncān de liǎng ge miànbāo zhǔnbèi hǎo le.",
        ),
        (NpcId::Mira, Language::Zh) => (
            "谢谢，塔维。我会配上蔬菜汤。",
            "Thank you, Tavi. I will serve them with vegetable soup.",
            "Xièxie, Tǎwéi. Wǒ huì pèi shàng shūcài tāng.",
        ),
    };
    ResidentReply {
        event_id: Uuid::new_v4().to_string(),
        npc_id: npc,
        text: text.into(),
        translation: translation.into(),
        reading: reading.into(),
    }
}

impl engine_bridge::DurableVillage {
    /// Storage/projection failures remain retryable. Only classify exhausted
    /// authority after a successful exact engine-result lookup.
    async fn supper_outcome(
        &self,
        accepted: &StageBinding,
        allowance: &ExecutionScope,
        store: std::sync::Arc<Store>,
        ids: &VillageIdentity,
        npc: NpcId,
    ) -> Result<EngineRunOutcome> {
        match self
            .generate_outcome(accepted, allowance, store, ids, npc)
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let run = self
                    .execution
                    .load_run(ids.owner, accepted.binding.run)
                    .await
                    .map_err(|_| Error::storage())?;
                if let Some(run) = run {
                    if run.run().state() == RunState::Completed {
                        let content = self
                            .execution
                            .load_completed_output(ids.owner, accepted.binding.run)
                            .await
                            .map_err(|_| Error::storage())?
                            .ok_or_else(Error::storage)?;
                        return Ok(EngineRunOutcome::Completed { content });
                    }
                    if run.run().state() == RunState::RecoveryRequired {
                        return Ok(EngineRunOutcome::RecoveryRequired);
                    }
                }
                let scope = self
                    .execution
                    .bind_budget(ids.owner, &allowance.id)
                    .await
                    .map_err(|_| Error::storage())?
                    .snapshot()
                    .await
                    .map_err(|_| Error::storage())?;
                let usage = scope.consumption().map_err(|_| Error::storage())?;
                if usage.inflight_operations == 0
                    && (self.clock.now_ms() >= scope.deadline_at_ms
                        || usage.model_attempts >= scope.limits.max_model_attempts)
                {
                    Ok(EngineRunOutcome::PausedForBudget)
                } else {
                    Err(error)
                }
            }
        }
    }
}
