use crate::{backend, conflict, relationships, store, BoundMemoryStore};
use anima_core::MemoryPortError;
use anima_memory::{
    MemoryAuthority, MemoryEffectReceipt, MemoryProjectionOutcome, MemoryProjectionPort,
    ValidatedMemoryEffect,
};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn receipt(
    connection: &Connection,
    authority: &MemoryAuthority,
    namespace: &str,
    source_id: &str,
    payload: &str,
) -> Result<Option<MemoryEffectReceipt>, MemoryPortError> {
    let existing: Option<(String,String)> = connection.query_row("SELECT payload,receipt FROM memory_receipts WHERE owner=?1 AND observer=?2 AND namespace=?3 AND source_id=?4",params![store::owner(authority),authority.identity().individual_id().as_str(),namespace,source_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(backend)?;
    match existing {
        Some((stored, receipt)) if stored == payload => Ok(Some(store::decode(&receipt)?)),
        Some(_) => Err(conflict("source effect already has a different payload")),
        None => Ok(None),
    }
}
pub(crate) fn save_receipt(
    connection: &Connection,
    authority: &MemoryAuthority,
    namespace: &str,
    source_id: &str,
    payload: &str,
    receipt: &MemoryEffectReceipt,
) -> Result<(), MemoryPortError> {
    connection.execute("INSERT INTO memory_receipts(owner,observer,namespace,source_id,payload,receipt) VALUES(?1,?2,?3,?4,?5,?6)",params![store::owner(authority),authority.identity().individual_id().as_str(),namespace,source_id,payload,store::json(receipt)?]).map_err(backend)?;
    Ok(())
}
#[async_trait]
impl MemoryProjectionPort for BoundMemoryStore {
    async fn apply_effect(
        &self,
        validated: ValidatedMemoryEffect,
    ) -> Result<MemoryProjectionOutcome, MemoryPortError> {
        let authority = self.authority.clone();
        self.store
            .call(true, move |connection, now| {
                let effect = validated.effect();
                if effect.identity() != authority.identity() {
                    return Err(anima_memory::access::unauthorized());
                }
                // A previously validated payload and receipt never widen the current handle's grant.
                for candidate in effect.candidates() {
                    store::ensure_write(&authority, candidate.write().scope())?;
                }
                let scope_for = |id: &str| -> Result<anima_core::MemoryScope, MemoryPortError> {
                    if let Some(candidate) = effect
                        .candidates()
                        .iter()
                        .find(|candidate| candidate.write().id() == id)
                    {
                        Ok(candidate.write().scope().clone())
                    } else {
                        Ok(store::record(connection, &authority, id)?.scope().clone())
                    }
                };
                for revision in effect.revisions() {
                    let scope = scope_for(revision.memory_id())?;
                    store::ensure_write(&authority, &scope)?;
                    for target in revision.supersedes() {
                        let target_scope = scope_for(target)?;
                        store::ensure_write(&authority, &target_scope)?;
                        if target_scope != scope {
                            return Err(anima_memory::access::unauthorized());
                        }
                    }
                }
                for observation in effect.relationships() {
                    store::ensure_write(&authority, observation.scope())?;
                }
                let payload = store::json(effect)?;
                if let Some(receipt) = receipt(
                    connection,
                    &authority,
                    "effect",
                    effect.source_event_id(),
                    &payload,
                )? {
                    return Ok(MemoryProjectionOutcome {
                        receipt,
                        replayed: true,
                    });
                }
                for candidate in effect.candidates() {
                    store::write(connection, &authority, candidate.write().clone())?;
                }
                for revision in effect.revisions() {
                    store::revise(connection, &authority, revision.clone(), now)?;
                }
                for observation in effect.relationships() {
                    relationships::insert(connection, &authority, observation, Some(now))?;
                }
                let receipt = MemoryEffectReceipt {
                    source_event_id: effect.source_event_id().into(),
                    identity: authority.identity().clone(),
                    memory_ids: effect
                        .candidates()
                        .iter()
                        .map(|c| c.write().id().into())
                        .chain(effect.revisions().iter().map(|r| r.memory_id().into()))
                        .collect(),
                    relationship_ids: effect
                        .relationships()
                        .iter()
                        .map(|r| r.id().into())
                        .collect(),
                };
                save_receipt(
                    connection,
                    &authority,
                    "effect",
                    effect.source_event_id(),
                    &payload,
                    &receipt,
                )?;
                Ok(MemoryProjectionOutcome {
                    receipt,
                    replayed: false,
                })
            })
            .await
    }
}
