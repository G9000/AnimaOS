use crate::{Error, Interaction, Result};
use anima_village::{
    reviewed_dialogue, reviewed_turn, Dialogue, GameInput, GameState, GameView, Language, Outcome,
    Phase, PlayedTurn, RuntimeInfo, Traveler,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Store {
    pub path: PathBuf,
    _lease: std::fs::File,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SaveRecord {
    version: u32,
    pub state: GameState,
    pub dialogue: Dialogue,
    pub runtime: RuntimeInfo,
}
impl SaveRecord {
    fn view(&self, id: &str) -> GameView {
        GameView::new(
            id.into(),
            &self.state,
            self.dialogue.clone(),
            self.runtime.clone(),
        )
    }
}
pub(crate) enum Admission {
    Start {
        record: Box<SaveRecord>,
        token: String,
    },
    Replay(Box<GameView>),
    Pending,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|_| Error::storage())?;
        }
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(".owner.lock");
        let lease = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PathBuf::from(sidecar))
            .map_err(|_| Error::storage())?;
        lease.try_lock().map_err(|_| {
            Error::new(
                409,
                "This village save database is already open in another host",
            )
        })?;
        let store = Self {
            path: path.into(),
            _lease: lease,
        };
        let db = store.db()?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS village_saves (id TEXT PRIMARY KEY, snapshot TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS village_requests (
                save_id TEXT NOT NULL, request_id TEXT NOT NULL, payload TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('pending','committed')), token TEXT,
                response TEXT, PRIMARY KEY(save_id,request_id),
                FOREIGN KEY(save_id) REFERENCES village_saves(id));",
        )
        .map_err(|_| Error::storage())?;
        // The process owns the DB lease, so pending tokens belonged to a stopped
        // host. Keep their identity/payload and permit an explicit exact retry.
        db.execute(
            "UPDATE village_requests SET token=NULL WHERE status='pending'",
            [],
        )
        .map_err(|_| Error::storage())?;
        Ok(store)
    }
    fn db(&self) -> Result<Connection> {
        let db = Connection::open(&self.path).map_err(|_| Error::storage())?;
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|_| Error::storage())?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
            .map_err(|_| Error::storage())?;
        Ok(db)
    }
    pub fn create(&self, language: Language, runtime: RuntimeInfo) -> Result<GameView> {
        let id = uuid::Uuid::new_v4().to_string();
        let state = GameState::new(language);
        let record = SaveRecord {
            version: 1,
            dialogue: reviewed_dialogue(&state, Outcome::Welcome),
            state,
            runtime: runtime.reviewed(),
        };
        self.db()?
            .execute(
                "INSERT INTO village_saves(id,snapshot) VALUES(?1,?2)",
                params![id, encode(&record)?],
            )
            .map_err(|_| Error::storage())?;
        Ok(record.view(&id))
    }
    pub fn get(&self, id: &str) -> Result<GameView> {
        Ok(load(&self.db()?, id)?.view(id))
    }
    pub(crate) fn customize(
        &self,
        id: &str,
        request: &str,
        payload: &str,
        traveler: Traveler,
    ) -> Result<GameView> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let mut record = load(&tx, id)?;
        if let Some((stored, status, response)) = receipt(&tx, id, request)? {
            if stored != payload {
                return Err(Error::new(
                    409,
                    "This requestId already has a different payload",
                ));
            }
            if status == "committed" {
                return decode(&response.ok_or_else(Error::storage)?);
            }
            return Err(Error::new(
                409,
                "This requestId belongs to a pending conversation",
            ));
        }
        let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM village_requests WHERE save_id=?1 AND status='pending')",[id],|r|r.get(0)).map_err(|_|Error::storage())?;
        if busy {
            return Err(Error::new(
                409,
                "A request is pending for this save; retry it or use reviewed recovery",
            ));
        }
        record.state.traveler = Some(traveler);
        let view = write_snapshot(
            &tx,
            id,
            PlayedTurn {
                state: record.state,
                dialogue: record.dialogue,
                runtime: record.runtime,
            },
        )?;
        tx.execute("INSERT INTO village_requests(save_id,request_id,payload,status,response) VALUES(?1,?2,?3,'committed',?4)",params![id,request,payload,encode(&view)?]).map_err(|_|Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(view)
    }
    pub(crate) fn begin(&self, id: &str, request: &str, payload: &str) -> Result<Admission> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let record = load(&tx, id)?;
        if let Some((stored, status, response)) = receipt(&tx, id, request)? {
            if stored != payload {
                return Err(Error::new(
                    409,
                    "This requestId already has a different payload",
                ));
            }
            if status == "committed" {
                return Ok(Admission::Replay(Box::new(decode(
                    &response.ok_or_else(Error::storage)?,
                )?)));
            }
            let owned:bool=tx.query_row("SELECT token IS NOT NULL FROM village_requests WHERE save_id=?1 AND request_id=?2",params![id,request],|r|r.get(0)).map_err(|_|Error::storage())?;
            if owned {
                return Ok(Admission::Pending);
            }
            let token = uuid::Uuid::new_v4().to_string();
            tx.execute("UPDATE village_requests SET token=?1 WHERE save_id=?2 AND request_id=?3 AND status='pending'",params![token,id,request]).map_err(|_|Error::storage())?;
            tx.commit().map_err(|_| Error::storage())?;
            return Ok(Admission::Start {
                record: Box::new(record),
                token,
            });
        }
        let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM village_requests WHERE save_id=?1 AND status='pending')",[id],|r|r.get(0)).map_err(|_|Error::storage())?;
        if busy {
            return Err(Error::new(
                409,
                "A request is pending for this save; retry it or use reviewed recovery",
            ));
        }
        let token = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO village_requests(save_id,request_id,payload,status,token) VALUES(?1,?2,?3,'pending',?4)",params![id,request,payload,token]).map_err(|_|Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(Admission::Start {
            record: Box::new(record),
            token,
        })
    }
    pub(crate) fn committed(&self, id: &str, request: &str) -> Result<Option<GameView>> {
        match receipt(&self.db()?, id, request)? {
            Some((_, status, Some(response))) if status == "committed" => {
                Ok(Some(decode(&response)?))
            }
            _ => Ok(None),
        }
    }
    pub(crate) fn retryable(&self, id: &str, request: &str) -> Result<bool> {
        self.db()?.query_row("SELECT status='pending' AND token IS NULL FROM village_requests WHERE save_id=?1 AND request_id=?2",params![id,request],|r|r.get(0)).map_err(|_|Error::storage())
    }
    pub(crate) fn release(&self, id: &str, request: &str, token: &str) -> Result<Option<GameView>> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        if let Some((_, status, Some(response))) = receipt(&tx, id, request)? {
            if status == "committed" {
                return Ok(Some(decode(&response)?));
            }
        }
        tx.execute("UPDATE village_requests SET token=NULL WHERE save_id=?1 AND request_id=?2 AND status='pending' AND token=?3",params![id,request,token]).map_err(|_|Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(None)
    }
    pub(crate) fn commit(
        &self,
        id: &str,
        request: &str,
        token: &str,
        played: PlayedTurn,
    ) -> Result<GameView> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let (status,stored_token,response):(String,Option<String>,Option<String>)=tx.query_row("SELECT status,token,response FROM village_requests WHERE save_id=?1 AND request_id=?2",params![id,request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|Error::storage())?;
        if status == "committed" {
            return decode(&response.ok_or_else(Error::storage)?);
        }
        if stored_token.as_deref() != Some(token) {
            return Err(Error::new(
                409,
                "This generation no longer owns the pending request",
            ));
        }
        let view = write_snapshot(&tx, id, played)?;
        finish_receipt(&tx, id, request, &view)?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(view)
    }
    pub(crate) fn recover(
        &self,
        id: &str,
        request: &str,
        payload: &str,
        original: &str,
        input: &GameInput,
        runtime: &RuntimeInfo,
    ) -> Result<GameView> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::storage())?;
        let record = load(&tx, id)?;
        if let Some((stored, status, response)) = receipt(&tx, id, request)? {
            if stored != payload {
                return Err(Error::new(
                    409,
                    "This requestId already has a different payload",
                ));
            }
            if status == "committed" {
                return decode(&response.ok_or_else(Error::storage)?);
            }
            return Err(Error::new(409, "The recovery request is already pending"));
        }
        let (original_payload, status, response) = receipt(&tx, id, original)?
            .ok_or_else(|| Error::new(404, "Original request not found in this save"))?;
        let (was_visit, original_input): (bool, Interaction) =
            serde_json::from_str(&original_payload).map_err(|_| {
                Error::new(
                    409,
                    "Only a conversation request supports reviewed recovery",
                )
            })?;
        if original_input.npc_id.unwrap_or_default() != input.speaker() {
            return Err(Error::new(
                409,
                "Reviewed recovery must use the original NPC",
            ));
        }
        let view = if status == "committed" {
            decode(&response.ok_or_else(Error::storage)?)?
        } else {
            // Invalidate ownership before evaluating the reviewed action. Both this
            // change and the result remain inside the same SQLite transaction.
            tx.execute(
                "UPDATE village_requests SET token=NULL WHERE save_id=?1 AND request_id=?2",
                params![id, original],
            )
            .map_err(|_| Error::storage())?;
            let state = record.state;
            let callback = was_visit
                && state.phase == Phase::Evening
                && input.speaker() == anima_village::NpcId::Tavi;
            let played = if was_visit {
                let visited = reviewed_turn(state, &GameInput::Visit.at(input.speaker()), runtime)
                    .map_err(|e| Error::new(400, e))?;
                if callback {
                    visited
                } else {
                    reviewed_turn(visited.state, input, runtime).map_err(|e| Error::new(400, e))?
                }
            } else {
                reviewed_turn(state, input, runtime).map_err(|e| Error::new(400, e))?
            };
            let view = write_snapshot(&tx, id, played)?;
            finish_receipt(&tx, id, original, &view)?;
            view
        };
        tx.execute("INSERT INTO village_requests(save_id,request_id,payload,status,response) VALUES(?1,?2,?3,'committed',?4)",params![id,request,payload,encode(&view)?]).map_err(|_|Error::storage())?;
        tx.commit().map_err(|_| Error::storage())?;
        Ok(view)
    }
}
fn load(db: &Connection, id: &str) -> Result<SaveRecord> {
    let body: Option<String> = db
        .query_row(
            "SELECT snapshot FROM village_saves WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| Error::storage())?;
    let record: SaveRecord =
        decode(&body.ok_or_else(|| Error::new(404, "Village save not found"))?)?;
    if record.version != 1 {
        return Err(Error::new(503, "This save uses an unsupported version"));
    }
    Ok(record)
}
fn receipt(
    db: &Connection,
    id: &str,
    request: &str,
) -> Result<Option<(String, String, Option<String>)>> {
    db.query_row(
        "SELECT payload,status,response FROM village_requests WHERE save_id=?1 AND request_id=?2",
        params![id, request],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(|_| Error::storage())
}
fn write_snapshot(db: &Connection, id: &str, played: PlayedTurn) -> Result<GameView> {
    let record = SaveRecord {
        version: 1,
        state: played.state,
        dialogue: played.dialogue,
        runtime: played.runtime,
    };
    let view = record.view(id);
    db.execute(
        "UPDATE village_saves SET snapshot=?1 WHERE id=?2",
        params![encode(&record)?, id],
    )
    .map_err(|_| Error::storage())?;
    Ok(view)
}
fn finish_receipt(db: &Connection, id: &str, request: &str, view: &GameView) -> Result<()> {
    db.execute("UPDATE village_requests SET status='committed',token=NULL,response=?1 WHERE save_id=?2 AND request_id=?3",params![encode(view)?,id,request]).map_err(|_|Error::storage())?;
    Ok(())
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::storage())
}
fn decode<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|_| Error::storage())
}
