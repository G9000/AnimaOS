use crate::{backend, conflict, projection, relationships, store, BoundMemoryStore};
use anima_core::{IdentityBinding, MemoryPortError, MemoryRecord};
use anima_memory::{
    legacy_memory_to_core, LegacySnapshotArchive, MemoryAuthority, MemoryEffectReceipt,
    MemoryManagerSnapshot, MemoryProjectionOutcome, RelationshipObservation,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Host-prepared copied fixture manifest. Import reads no legacy database and changes no old reader.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryImportManifest {
    id: String,
    identity: IdentityBinding,
    records: Vec<MemoryRecord>,
    relationships: Vec<RelationshipObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_archive: Option<LegacySnapshotArchive>,
}
impl MemoryImportManifest {
    /// Imports only a copied, explicitly selected profile. Auxiliary legacy rows retain their
    /// exact source representation without inventing visibility, identities or generated facts.
    pub fn from_legacy_snapshot(
        id: impl Into<String>,
        identity: IdentityBinding,
        legacy_agent_id: impl Into<String>,
        snapshot: MemoryManagerSnapshot,
    ) -> Result<Self, MemoryPortError> {
        let id = id.into();
        let archive = LegacySnapshotArchive::new(id.clone(), legacy_agent_id, snapshot)?;
        let records = legacy_records(&identity, &archive)?;
        let mut manifest = Self::new(id, identity, records, vec![])?;
        manifest.legacy_archive = Some(archive);
        if store::json(&manifest)?.len() > 4 * 1024 * 1024 {
            return Err(MemoryPortError::invalid(
                "import manifest exceeds byte limit",
            ));
        }
        Ok(manifest)
    }
    fn validate(&self) -> Result<(), MemoryPortError> {
        Self::new(
            self.id.clone(),
            self.identity.clone(),
            self.records.clone(),
            self.relationships.clone(),
        )?;
        if let Some(archive) = &self.legacy_archive {
            archive.validate()?;
            if archive.source_id() != self.id || !self.relationships.is_empty() {
                return Err(MemoryPortError::invalid(
                    "legacy archive source or relationship mapping mismatch",
                ));
            }
            let records = legacy_records(&self.identity, archive)?;
            if records != self.records {
                return Err(MemoryPortError::invalid(
                    "legacy archive canonical memory mapping mismatch",
                ));
            }
        }
        if store::json(self)?.len() > 4 * 1024 * 1024 {
            return Err(MemoryPortError::invalid(
                "import manifest exceeds byte limit",
            ));
        }
        Ok(())
    }
    pub fn new(
        id: impl Into<String>,
        identity: IdentityBinding,
        records: Vec<MemoryRecord>,
        relationships: Vec<RelationshipObservation>,
    ) -> Result<Self, MemoryPortError> {
        let id = id.into();
        if id.trim().is_empty()
            || id.trim() != id
            || id.len() > 256
            || records.len() > 1000
            || relationships.len() > 1000
        {
            return Err(MemoryPortError::invalid("invalid bounded import manifest"));
        }
        let mut ids = std::collections::BTreeSet::new();
        if records.iter().any(|r| !ids.insert(r.id().to_owned())) {
            return Err(MemoryPortError::invalid(
                "import contains duplicate memory IDs",
            ));
        }
        let manifest = Self {
            id,
            identity,
            records,
            relationships,
            legacy_archive: None,
        };
        if store::json(&manifest)?.len() > 4 * 1024 * 1024 {
            return Err(MemoryPortError::invalid(
                "import manifest exceeds byte limit",
            ));
        }
        Ok(manifest)
    }
}
impl BoundMemoryStore {
    pub async fn import_manifest(
        &self,
        manifest: MemoryImportManifest,
    ) -> Result<MemoryProjectionOutcome, MemoryPortError> {
        let authority = self.authority.clone();
        self.store.call(true,move |connection,_| {
            manifest.validate()?;
            if &manifest.identity != authority.identity() { return Err(anima_memory::access::unauthorized()); }
            for record in &manifest.records { store::ensure_write(&authority,record.scope())?; }
            for observation in &manifest.relationships { store::ensure_write(&authority,observation.scope())?; }
            if let Some(archive) = &manifest.legacy_archive {
                archive.validate()?;
                store::ensure_write(&authority, &anima_core::MemoryScope::agent(authority.identity().individual_id().as_str())?)?;
            }
            let payload = store::json(&manifest)?;
            if let Some(receipt) = projection::receipt(connection,&authority,"import",&manifest.id,&payload)? { return Ok(MemoryProjectionOutcome { receipt, replayed: true }); }
            if let Some(archive) = &manifest.legacy_archive {
                let candidate = archive.rows()?;
                for old in archives(connection, &authority)? {
                    if old.legacy_agent_id() != archive.legacy_agent_id() { continue; }
                    for (key, value) in old.rows()? {
                        if candidate.get(&key).is_some_and(|next|next != &value) { return Err(conflict("import legacy typed ID collision")); }
                    }
                }
            }
            for record in &manifest.records {
                store::ensure_write(&authority,record.scope())?;
                let existing: Option<String> = connection.query_row("SELECT record FROM memory_records WHERE owner=?1 AND id=?2",params![store::owner(&authority),record.id()],|r|r.get(0)).optional().map_err(backend)?;
                if let Some(existing) = existing { if existing != store::json(record)? { return Err(conflict("import memory ID collision")); } } else {
                    store::insert_record(connection,&authority,record,&store::json(&store::write_from_record(record)?)?,"legacy_import")?;
                }
            }
            for record in &manifest.records {
                for target in record.supersedes() {
                    let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM memory_supersessions WHERE owner=?1 AND superseder=?2 AND target=?3)",params![store::owner(&authority),record.id(),target],|r|r.get(0)).map_err(backend)?;
                    if !exists {
                        // Historical links survive later expiry/forgetting of their targets.
                        store::validate_lineage(connection,&authority,record,target,None)?;
                        connection.execute("INSERT INTO memory_supersessions(owner,superseder,target) VALUES(?1,?2,?3)",params![store::owner(&authority),record.id(),target]).map_err(backend)?;
                    }
                }
            }
            for observation in &manifest.relationships { relationships::insert(connection,&authority,observation,None)?; }
            let receipt = MemoryEffectReceipt { source_event_id: manifest.id.clone(), identity: authority.identity().clone(), memory_ids: manifest.records.iter().map(|r|r.id().into()).collect(), relationship_ids: manifest.relationships.iter().map(|r|r.id().into()).collect() };
            projection::save_receipt(connection,&authority,"import",&manifest.id,&payload,&receipt)?;
            Ok(MemoryProjectionOutcome { receipt, replayed: false })
        }).await
    }
}

/// Reviewable rollback data from the current canonical writer. Historical archives are labeled
/// separately: consumers must retain canonical lifecycle and must never restore an old snapshot
/// over post-cutover facts, corrections, retention or tombstones.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProfileRollbackExport {
    pub schema_version: u32,
    pub identity: IdentityBinding,
    pub records: Vec<MemoryRecord>,
    pub relationships: Vec<RelationshipObservation>,
    pub legacy_archives: Vec<LegacySnapshotArchive>,
}

fn archives(
    connection: &rusqlite::Connection,
    authority: &MemoryAuthority,
) -> Result<Vec<LegacySnapshotArchive>, MemoryPortError> {
    let mut statement = connection.prepare("SELECT payload FROM memory_receipts WHERE owner=?1 AND observer=?2 AND namespace='import' ORDER BY source_id LIMIT 1001").map_err(backend)?;
    let rows = statement
        .query_map(
            params![
                store::owner(authority),
                authority.identity().individual_id().as_str()
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(backend)?;
    let mut archives = Vec::new();
    let mut count = 0;
    let mut bytes = 0;
    for row in rows {
        count += 1;
        if count > 1000 {
            return Err(MemoryPortError::unsupported(
                "profile export exceeds bounded import history",
            ));
        }
        let raw = row.map_err(backend)?;
        bytes += raw.len();
        if bytes > 16 * 1024 * 1024 {
            return Err(MemoryPortError::unsupported(
                "profile archive history exceeds export byte limit",
            ));
        }
        let manifest: MemoryImportManifest = store::decode(&raw)?;
        manifest.validate()?;
        if &manifest.identity != authority.identity() {
            return Err(backend("import receipt identity mismatch"));
        }
        if let Some(archive) = manifest.legacy_archive {
            store::ensure_read(
                authority,
                &anima_core::MemoryScope::agent(authority.identity().individual_id().as_str())?,
            )?;
            for record in manifest.records {
                store::ensure_read(authority, record.scope())?;
            }
            archive.validate()?;
            archives.push(archive);
        }
    }
    Ok(archives)
}
impl BoundMemoryStore {
    /// Historical inspection/export only; normal recall still uses current active canonical rows.
    /// Grants are checked again at export. Export includes inactive rows and native relationship
    /// evidence so rollback can keep this canonical reader instead of reviving a stale legacy one.
    pub async fn export_profile_for_rollback(
        &self,
    ) -> Result<MemoryProfileRollbackExport, MemoryPortError> {
        let authority = self.authority.clone();
        self.store.call(false, move |connection,_| {
            let legacy_archives = archives(connection,&authority)?;
            let mut statement = connection.prepare("SELECT record FROM memory_records WHERE owner=?1 AND writer=?2 ORDER BY id LIMIT 1001").map_err(backend)?;
            let rows = statement.query_map(params![store::owner(&authority), authority.identity().individual_id().as_str()],|row|row.get::<_,String>(0)).map_err(backend)?;
            let mut records = Vec::new();
            for row in rows {
                if records.len()>=1000 { return Err(MemoryPortError::unsupported("profile export exceeds bounded memory records")); }
                let record: MemoryRecord = store::decode(&row.map_err(backend)?)?;
                store::ensure_read(&authority,record.scope())?;
                records.push(record);
            }
            let mut statement = connection.prepare("SELECT observation FROM memory_relationships WHERE owner=?1 AND observer=?2 ORDER BY id LIMIT 1001").map_err(backend)?;
            let rows = statement.query_map(params![store::owner(&authority), authority.identity().individual_id().as_str()],|row|row.get::<_,String>(0)).map_err(backend)?;
            let mut relationships = Vec::new();
            for row in rows {
                if relationships.len()>=1000 { return Err(MemoryPortError::unsupported("profile export exceeds bounded relationship records")); }
                let observation: RelationshipObservation = store::decode(&row.map_err(backend)?)?;
                store::ensure_read(&authority,observation.scope())?;
                relationships.push(observation);
            }
            let export = MemoryProfileRollbackExport { schema_version: 1, identity: authority.identity().clone(), records, relationships, legacy_archives };
            if store::json(&export)?.len()>16*1024*1024 { return Err(MemoryPortError::unsupported("profile export exceeds byte limit")); }
            Ok(export)
        }).await
    }
}

fn legacy_records(
    identity: &IdentityBinding,
    archive: &LegacySnapshotArchive,
) -> Result<Vec<MemoryRecord>, MemoryPortError> {
    archive
        .snapshot()
        .memories
        .iter()
        .map(|memory| {
            let mut mapped = memory.clone();
            mapped.agent_id = identity.individual_id().as_str().into();
            legacy_memory_to_core(&mapped).map_err(|error| MemoryPortError::invalid(error.message))
        })
        .collect()
}
