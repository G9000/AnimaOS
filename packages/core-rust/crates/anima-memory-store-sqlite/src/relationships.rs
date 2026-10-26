//! Evidence-backed directed relationship perspectives.
use crate::{backend, conflict, store, BoundMemoryStore};
use anima_core::{IdentityBinding, MemoryPortError};
use anima_memory::{MemoryAuthority, RelationshipObservation};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Debug, PartialEq)]
pub struct RelationshipPerspective {
    pub observer: IdentityBinding,
    pub observation: RelationshipObservation,
}

pub(crate) fn insert(
    connection: &Connection,
    authority: &MemoryAuthority,
    observation: &RelationshipObservation,
    active_at: Option<u64>,
) -> Result<(), MemoryPortError> {
    store::ensure_write(authority, observation.scope())?;
    let target_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_identities WHERE owner=?1 AND individual=?2)",
            params![store::owner(authority), observation.target().as_str()],
            |r| r.get(0),
        )
        .map_err(backend)?;
    if !target_exists {
        return Err(MemoryPortError::invalid(
            "relationship target must be an explicitly registered individual",
        ));
    }
    let observer_world: Option<String> = connection
        .query_row(
            "SELECT world FROM memory_residents WHERE owner=?1 AND individual=?2",
            params![
                store::owner(authority),
                authority.identity().individual_id().as_str()
            ],
            |r| r.get(0),
        )
        .optional()
        .map_err(backend)?;
    let target_world: Option<String> = connection
        .query_row(
            "SELECT world FROM memory_residents WHERE owner=?1 AND individual=?2",
            params![store::owner(authority), observation.target().as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(backend)?;
    if observer_world != target_world {
        return Err(anima_memory::access::unauthorized());
    }
    for id in observation.evidence_memory_ids() {
        let evidence = store::record(connection, authority, id)?;
        if evidence.scope() != observation.scope() {
            return Err(anima_memory::access::unauthorized());
        }
        if let Some(now) = active_at {
            if !store::active(connection, authority, id, now)? {
                return Err(anima_memory::access::unauthorized());
            }
        }
    }
    let raw = store::json(observation)?;
    let existing: Option<String> = connection
        .query_row(
            "SELECT observation FROM memory_relationships WHERE owner=?1 AND observer=?2 AND id=?3",
            params![
                store::owner(authority),
                authority.identity().individual_id().as_str(),
                observation.id()
            ],
            |r| r.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(existing) = existing {
        return if existing == raw {
            Ok(())
        } else {
            Err(conflict(
                "relationship ID already has a different observation",
            ))
        };
    }
    connection.execute("INSERT INTO memory_relationships(owner,observer,id,scope,observed,observation) VALUES(?1,?2,?3,?4,?5,?6)",params![store::owner(authority),authority.identity().individual_id().as_str(),observation.id(),store::json(observation.scope())?,store::millis(observation.observed_at_ms()),raw]).map_err(backend)?;
    Ok(())
}
impl BoundMemoryStore {
    /// Returns only this individual's perspective; evidence must remain visible and active.
    pub async fn relationships(
        &self,
        limit: usize,
    ) -> Result<Vec<RelationshipPerspective>, MemoryPortError> {
        if limit == 0 || limit > 1000 {
            return Err(MemoryPortError::invalid(
                "relationship limit must be between 1 and 1000",
            ));
        }
        let authority = self.authority.clone();
        self.store.call(false,move |connection,now| {
            let mut parameters = vec![rusqlite::types::Value::Text(store::owner(&authority).into()),rusqlite::types::Value::Text(authority.identity().individual_id().as_str().into())];
            for scope in authority.read_scopes() { parameters.push(rusqlite::types::Value::Text(store::json(scope)?)); }
            let placeholders = (3..parameters.len()+1).map(|index|format!("?{index}")).collect::<Vec<_>>().join(",");
            let sql = format!("SELECT id,scope,observed,observation FROM memory_relationships WHERE owner=?1 AND observer=?2 AND scope IN ({placeholders}) ORDER BY observed DESC,id LIMIT 4096");
            let mut statement = connection.prepare(&sql).map_err(backend)?;
            let rows = statement.query_map(rusqlite::params_from_iter(parameters),|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).map_err(backend)?;
            let mut perspectives = Vec::new();
            for row in rows {
                let (id,scope,observed,raw) = row.map_err(backend)?;
                let observation: RelationshipObservation = store::decode(&raw)?;
                if observation.id() != id || store::json(observation.scope())? != scope || store::millis(observation.observed_at_ms()) != observed { return Err(backend("canonical relationship columns disagree")); }
                if !authority.allows_read(observation.scope()) || observation.valid_from_ms().is_some_and(|from|from > now) || observation.valid_to_ms().is_some_and(|to|to <= now) { continue; }
                let mut visible = true;
                for id in observation.evidence_memory_ids() {
                    match store::record(connection,&authority,id) {
                        Ok(evidence) if evidence.scope() == observation.scope() => { if !store::active(connection,&authority,id,now)? { visible = false; break; } }
                        Err(error) if error.code == anima_core::MemoryPortErrorCode::Backend => return Err(error),
                        Err(error) if error.code == anima_core::MemoryPortErrorCode::NotFound => return Err(backend("canonical relationship evidence is missing")),
                        _ => { visible = false; break; }
                    }
                }
                if visible { perspectives.push(RelationshipPerspective { observer: authority.identity().clone(), observation }); }
                if perspectives.len() == limit { break; }
            }
            Ok(perspectives)
        }).await
    }
}
