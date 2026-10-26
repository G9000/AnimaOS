//! Explicit player corrections/forgetting through the accepted-event outbox.
use crate::{engine_bridge::AcceptedObservation, Error, Result, Service};
use anima_harness::anima_core::*;
use anima_memory::EvidenceKind;
use anima_village::NpcId;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryEdit {
    pub request_id: String,
    pub npc_id: NpcId,
    pub memory_id: String,
    pub expected_revision: u64,
    pub replacement: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEditReceipt {
    pub event_id: String,
    pub memory_id: String,
}
impl Service {
    pub async fn edit_resident_memory(
        &self,
        save: &str,
        input: MemoryEdit,
    ) -> Result<MemoryEditReceipt> {
        let _work = self.track()?;
        let _guard = self.memory_edit_gate.lock().await;
        let bridge = self
            .durable
            .as_ref()
            .ok_or_else(|| Error::new(503, "Persistent resident memory is not enabled"))?;
        if input.request_id.is_empty()
            || input.request_id.len() > 128
            || !input
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || input.expected_revision == 0
            || input
                .replacement
                .as_ref()
                .is_some_and(|s| s.trim().is_empty() || s.len() > 4096)
        {
            return Err(Error::new(400, "Invalid resident memory edit"));
        }
        let ids = self.store.resident_identities(save)?;
        bridge.project(&self.store, &ids).await?;
        let payload = serde_json::to_string(&input).map_err(|_| Error::storage())?;
        let prior:Option<(String,String)>=self.store.db()?.query_row("SELECT payload,response FROM village_memory_edits WHERE save_id=?1 AND request_id=?2",params![save,input.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|Error::storage())?;
        if let Some((stored, response)) = prior {
            if stored != payload {
                return Err(Error::new(
                    409,
                    "This memory edit requestId already has a different payload",
                ));
            }
            return serde_json::from_str(&response).map_err(|_| Error::storage());
        }
        let authority = ids.authority(input.npc_id).map_err(|_| Error::storage())?;
        let bound = bridge
            .memory
            .bind(authority.clone())
            .await
            .map_err(|_| Error::new(503, "Resident memory is unavailable"))?;
        // Recall limits do not limit lifecycle authority. Correction requires
        // an active target; explicit forgetting may include expired/superseded history.
        let target = if input.replacement.is_some() {
            bound.get_active(&input.memory_id).await
        } else {
            bound.get(&input.memory_id).await
        }
        .map_err(|error| match error.code {
            MemoryPortErrorCode::NotFound | MemoryPortErrorCode::UnauthorizedScope => {
                Error::new(404, "Memory not found for this resident")
            }
            MemoryPortErrorCode::Conflict => Error::new(409, "This memory is no longer active"),
            MemoryPortErrorCode::InvalidInput => Error::new(400, "Invalid memory identifier"),
            MemoryPortErrorCode::Unsupported | MemoryPortErrorCode::Backend => {
                Error::new(503, "Resident memory is unavailable")
            }
        })?;
        if target.revision() != input.expected_revision {
            return Err(Error::new(409, "This memory has a newer revision"));
        }
        if input.replacement.is_none() && target.forgotten_at_ms().is_some() {
            return Err(Error::new(409, "This memory is already forgotten"));
        }
        let event = Uuid::new_v4().to_string();
        let replacement_id = format!(
            "village:{event}:{}",
            authority.identity().individual_id().as_str()
        );
        let (revision, memory_id, text, forget) = if let Some(replacement) = &input.replacement {
            (
                MemoryRevision::correction(
                    &replacement_id,
                    1,
                    &input.memory_id,
                    "Explicit player correction",
                ),
                replacement_id,
                replacement.clone(),
                false,
            )
        } else {
            (
                MemoryRevision::forget(
                    &input.memory_id,
                    input.expected_revision,
                    "Explicit player forgetting",
                ),
                input.memory_id.clone(),
                String::new(),
                true,
            )
        };
        let revision = revision.map_err(|_| Error::new(400, "Invalid memory revision"))?;
        let observation = AcceptedObservation {
            event: event.clone(),
            identity: authority.identity().clone(),
            text,
            kind: EvidenceKind::UserStatement,
            at_ms: bridge.clock.now_ms(),
            sender: None,
            relationship_target: None,
            revision: Some(revision),
            forget_only: forget,
            callback_memories: Default::default(),
        };
        let receipt = MemoryEditReceipt {
            event_id: event.clone(),
            memory_id,
        };
        {
            let mut db = self.store.db()?;
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| Error::storage())?;
            // Serialize with game admission. A correction cannot race a generated
            // turn carrying a now-obsolete accepted memory snapshot.
            let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM village_requests WHERE save_id=?1 AND status='pending') OR EXISTS(SELECT 1 FROM village_supper_activity WHERE save_id=?1 AND token IS NOT NULL)",[save],|r|r.get(0)).map_err(|_|Error::storage())?;
            if busy {
                return Err(Error::new(
                    409,
                    "Finish or recover the active conversation before editing its memory",
                ));
            }
            tx.execute(
                "INSERT INTO village_memory_edits VALUES(?1,?2,?3,?4)",
                params![
                    save,
                    input.request_id,
                    payload,
                    serde_json::to_string(&receipt).map_err(|_| Error::storage())?
                ],
            )
            .map_err(|_| Error::new(409, "This memory edit request was already accepted"))?;
            tx.execute("INSERT INTO village_memory_outbox(event_id,save_id,observer,payload) VALUES(?1,?2,?3,?4)",params![event,save,authority.identity().individual_id().as_str(),serde_json::to_string(&observation).map_err(|_|Error::storage())?]).map_err(|_|Error::storage())?;
            tx.commit().map_err(|_| Error::storage())?;
        }
        bridge.project(&self.store, &ids).await?;
        Ok(receipt)
    }
}
