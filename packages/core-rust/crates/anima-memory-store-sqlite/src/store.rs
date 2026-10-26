use crate::{backend, conflict};
use anima_core::{
    MemoryPortError, MemoryPortErrorCode, MemoryRecord, MemoryRetention, MemoryRevision,
    MemoryScope, MemoryWrite,
};
use anima_memory::{access::unauthorized, MemoryAuthority};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub(crate) fn json<T: Serialize>(value: &T) -> Result<String, MemoryPortError> {
    serde_json::to_string(value)
        .map_err(|_| MemoryPortError::invalid("memory value cannot be serialized"))
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, MemoryPortError> {
    serde_json::from_str(value).map_err(|_| backend("invalid canonical memory record"))
}
pub(crate) fn millis(value: u64) -> String {
    format!("{value:020}")
}
pub(crate) fn owner(authority: &MemoryAuthority) -> &str {
    authority.identity().owner_id().as_str()
}
pub(crate) fn ensure_read(
    authority: &MemoryAuthority,
    scope: &MemoryScope,
) -> Result<(), MemoryPortError> {
    if authority.allows_read(scope) {
        Ok(())
    } else {
        Err(unauthorized())
    }
}
pub(crate) fn ensure_write(
    authority: &MemoryAuthority,
    scope: &MemoryScope,
) -> Result<(), MemoryPortError> {
    if authority.allows_write(scope) {
        Ok(())
    } else {
        Err(unauthorized())
    }
}
pub(crate) fn record(
    connection: &Connection,
    authority: &MemoryAuthority,
    id: &str,
) -> Result<MemoryRecord, MemoryPortError> {
    let raw: Option<String> = connection
        .query_row(
            "SELECT record FROM memory_records WHERE owner=?1 AND id=?2",
            params![owner(authority), id],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    let record: MemoryRecord = decode(&raw.ok_or_else(|| MemoryPortError {
        code: MemoryPortErrorCode::NotFound,
        message: "memory not found".into(),
    })?)?;
    ensure_read(authority, record.scope())?;
    Ok(record)
}
pub(crate) fn active(
    connection: &Connection,
    authority: &MemoryAuthority,
    id: &str,
    now: u64,
) -> Result<bool, MemoryPortError> {
    let record = record(connection, authority, id)?;
    if record.forgotten_at_ms().is_some()
        || record
            .retention()
            .deadline_ms()
            .is_some_and(|deadline| deadline <= now)
    {
        return Ok(false);
    }
    let superseded: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_supersessions WHERE owner=?1 AND target=?2)",
            params![owner(authority), id],
            |r| r.get(0),
        )
        .map_err(backend)?;
    Ok(!superseded)
}
pub(crate) fn write(
    connection: &Connection,
    authority: &MemoryAuthority,
    write: MemoryWrite,
) -> Result<MemoryRecord, MemoryPortError> {
    ensure_write(authority, write.scope())?;
    if write.retention().policy() == anima_core::RetentionPolicy::Ephemeral {
        return Err(MemoryPortError::unsupported(
            "ephemeral memory must use a request-local store; durable persistence is refused",
        ));
    }
    let existing: Option<(String, String)> = connection
        .query_row(
            "SELECT original_write,record FROM memory_records WHERE owner=?1 AND id=?2",
            params![owner(authority), write.id()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let original = json(&write)?;
    if let Some((stored, raw)) = existing {
        let current: MemoryRecord = decode(&raw)?;
        ensure_write(authority, current.scope())?;
        if stored != original {
            return Err(conflict(
                "memory ID was already written with a different payload",
            ));
        }
        return Ok(current);
    }
    let record = MemoryRecord::from_write(write, 1)?;
    insert_record(connection, authority, &record, &original, "write")?;
    Ok(record)
}
pub(crate) fn insert_record(
    connection: &Connection,
    authority: &MemoryAuthority,
    record: &MemoryRecord,
    original: &str,
    mutation: &str,
) -> Result<(), MemoryPortError> {
    ensure_write(authority, record.scope())?;
    if record.retention().policy() == anima_core::RetentionPolicy::Ephemeral {
        return Err(MemoryPortError::unsupported(
            "ephemeral memory cannot be imported into durable canonical storage",
        ));
    }
    let raw = json(record)?;
    connection.execute("INSERT INTO memory_records(owner,id,writer,scope,revision,observed,deadline,forgotten,original_write,record) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![owner(authority),record.id(),authority.identity().individual_id().as_str(),json(record.scope())?,record.revision().to_string(),millis(record.provenance().observed_at_ms()),record.retention().deadline_ms().map(millis),record.forgotten_at_ms().is_some(),original,raw]).map_err(backend)?;
    history(connection, authority, record, mutation)
}
pub(crate) fn history(
    connection: &Connection,
    authority: &MemoryAuthority,
    record: &MemoryRecord,
    mutation: &str,
) -> Result<(), MemoryPortError> {
    connection.execute("INSERT INTO memory_history(owner,id,revision,actor,mutation,record) VALUES(?1,?2,?3,?4,?5,?6)",params![owner(authority),record.id(),record.revision().to_string(),authority.identity().individual_id().as_str(),mutation,json(record)?]).map_err(backend)?;
    Ok(())
}
pub(crate) fn validate_lineage(
    connection: &Connection,
    authority: &MemoryAuthority,
    source: &MemoryRecord,
    target: &str,
    active_at: Option<u64>,
) -> Result<(), MemoryPortError> {
    if source.id() == target {
        return Err(MemoryPortError::invalid("memory cannot supersede itself"));
    }
    let target_record = record(connection, authority, target)?;
    ensure_write(authority, target_record.scope())?;
    if source.scope() != target_record.scope() {
        return Err(unauthorized());
    }
    if let Some(now) = active_at {
        if !active(connection, authority, target, now)? {
            return Err(conflict("superseded target is no longer active"));
        }
    }
    // Every new edge is checked for a path back to its source.
    let cycle: bool = connection.query_row("WITH RECURSIVE lineage(id) AS (SELECT target FROM memory_supersessions WHERE owner=?1 AND superseder=?2 UNION SELECT s.target FROM memory_supersessions s JOIN lineage l ON s.superseder=l.id WHERE s.owner=?1) SELECT EXISTS(SELECT 1 FROM lineage WHERE id=?3)",params![owner(authority),target,source.id()],|r|r.get(0)).map_err(backend)?;
    if cycle {
        return Err(conflict("memory lineage cycle"));
    }
    Ok(())
}
pub(crate) fn revise(
    connection: &Connection,
    authority: &MemoryAuthority,
    revision: MemoryRevision,
    now: u64,
) -> Result<MemoryRecord, MemoryPortError> {
    let current = record(connection, authority, revision.memory_id())?;
    ensure_write(authority, current.scope())?;
    if current.revision() != revision.expected_revision() {
        return Err(conflict("stale memory revision"));
    }
    if current.forgotten_at_ms().is_some() {
        return Err(conflict("forgotten memory cannot be revised"));
    }
    if !revision.supersedes().is_empty() && !active(connection, authority, current.id(), now)? {
        return Err(conflict("inactive memory cannot supersede another"));
    }
    for target in revision.supersedes() {
        validate_lineage(connection, authority, &current, target, Some(now))?;
    }
    let mut value =
        serde_json::to_value(&current).map_err(|_| backend("record encoding failed"))?;
    value["revision"] = current
        .revision()
        .checked_add(1)
        .ok_or_else(|| conflict("memory revision exhausted"))?
        .into();
    if let Some(reason) = revision.forget_reason() {
        value["forgotten_at_ms"] = now.into();
        value["forget_reason"] = reason.into();
    } else {
        let mut supersedes = current.supersedes().to_vec();
        for target in revision.supersedes() {
            if !supersedes.contains(target) {
                supersedes.push(target.clone());
            }
        }
        value["supersedes"] =
            serde_json::to_value(supersedes).map_err(|_| backend("lineage encoding failed"))?;
        if let Some(corrects) = revision.corrects() {
            value["corrects"] = corrects.into();
        }
    }
    let next: MemoryRecord = serde_json::from_value(value)
        .map_err(|_| MemoryPortError::invalid("invalid lifecycle mutation"))?;
    let changed = connection.execute("UPDATE memory_records SET record=?1,revision=?2,forgotten=?3 WHERE owner=?4 AND id=?5 AND revision=?6",params![json(&next)?,next.revision().to_string(),next.forgotten_at_ms().is_some(),owner(authority),next.id(),current.revision().to_string()]).map_err(backend)?;
    if changed != 1 {
        return Err(conflict("stale memory revision"));
    }
    for target in revision.supersedes() {
        connection
            .execute(
                "INSERT INTO memory_supersessions(owner,superseder,target) VALUES(?1,?2,?3)",
                params![owner(authority), next.id(), target],
            )
            .map_err(backend)?;
    }
    history(connection, authority, &next, &json(&revision)?)?;
    Ok(next)
}
pub(crate) fn recent(
    connection: &Connection,
    authority: &MemoryAuthority,
    scopes: &[MemoryScope],
    now: u64,
    limit: usize,
) -> Result<Vec<MemoryRecord>, MemoryPortError> {
    if scopes.is_empty() || scopes.len() > 64 || limit == 0 || limit > 4096 {
        return Err(MemoryPortError::invalid("invalid bounded recall request"));
    }
    let mut parameters = vec![
        rusqlite::types::Value::Text(owner(authority).into()),
        rusqlite::types::Value::Text(millis(now)),
        rusqlite::types::Value::Integer(limit as i64),
    ];
    for scope in scopes {
        ensure_read(authority, scope)?;
        parameters.push(rusqlite::types::Value::Text(json(scope)?));
    }
    // Exact scopes and active lifecycle are selected before one global candidate bound/ranking.
    let placeholders = (4..parameters.len() + 1)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT id,scope,revision,observed,deadline,record FROM memory_records r WHERE owner=?1 AND scope IN ({placeholders}) AND forgotten=0 AND (deadline IS NULL OR deadline>?2) AND NOT EXISTS(SELECT 1 FROM memory_supersessions s WHERE s.owner=r.owner AND s.target=r.id) ORDER BY observed DESC,id ASC LIMIT ?3");
    let mut statement = connection.prepare(&sql).map_err(backend)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(backend)?;
    let mut records = Vec::new();
    for row in rows {
        let (id, scope, revision, observed, deadline, raw) = row.map_err(backend)?;
        let record: MemoryRecord = decode(&raw)?;
        if record.id() != id
            || json(record.scope())? != scope
            || record.revision().to_string() != revision
            || millis(record.provenance().observed_at_ms()) != observed
            || record.retention().deadline_ms().map(millis) != deadline
            || record.forgotten_at_ms().is_some()
        {
            return Err(backend(
                "canonical memory columns do not match the validated record",
            ));
        }
        ensure_read(authority, record.scope())?;
        records.push(record);
    }
    Ok(records)
}
pub(crate) fn write_from_record(record: &MemoryRecord) -> Result<MemoryWrite, MemoryPortError> {
    let mut write = MemoryWrite::new(
        record.id(),
        record.kind(),
        record.scope().clone(),
        record.content(),
        record.confidence(),
        record.provenance().clone(),
    )?;
    if record.retention() != &MemoryRetention::default() {
        // Preserve every validated retention variant, including legacy ephemeral records.
        let mut raw = serde_json::to_value(&write).map_err(|_| backend("write encoding failed"))?;
        raw["retention"] = serde_json::to_value(record.retention())
            .map_err(|_| backend("retention encoding failed"))?;
        write = serde_json::from_value(raw).map_err(|_| backend("retention decoding failed"))?;
    }
    Ok(write)
}
