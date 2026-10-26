//! Frozen copied legacy conversations. This historical source never overrides canonical lifecycle.
use crate::{
    engine_bridge::{DurableVillage, VillageIdentity},
    Error, Result, Store,
};
use anima_harness::anima_core::*;
use anima_memory::MemoryEffectReceipt;
use anima_memory_store_sqlite::MemoryImportManifest;
use anima_village::NpcId;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PreparedResident {
    identity: IdentityBinding,
    manifest: MemoryImportManifest,
    expected_receipt: MemoryEffectReceipt,
}
#[derive(Serialize, Deserialize)]
struct FrozenSource {
    conversation: Vec<String>,
    mira_conversation: Vec<String>,
    residents: Vec<PreparedResident>,
}
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS village_legacy_memory_sources(save_id TEXT PRIMARY KEY REFERENCES village_saves(id),payload TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS village_legacy_memory_receipts(save_id TEXT NOT NULL REFERENCES village_legacy_memory_sources(save_id),observer TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(save_id,observer));").map_err(|_|Error::storage())
}
/// Called only in the transaction creating a save's FIRST durable identities.
/// Existing durable identities/history never retro-import their mutable caches.
pub(crate) fn freeze(db: &Connection, save: &str, ids: &VillageIdentity) -> Result<()> {
    let state = crate::store::load(db, save)?.state;
    let mut residents = vec![];
    for (npc, history) in [
        (NpcId::Tavi, &state.conversation),
        (NpcId::Mira, &state.mira_conversation),
    ] {
        let source = format!("village-legacy:{}:{}", ids.world, npc.name().to_lowercase());
        let records = history
            .iter()
            .enumerate()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(index, text)| {
                MemoryRecord::from_write(
                    MemoryWrite::new(
                        format!("{source}:{index}"),
                        MemoryKind::Episode,
                        MemoryScope::agent(ids.resident(npc).individual_id().as_str())?,
                        text.clone(),
                        0.5,
                        MemoryProvenance::new("legacy-unknown", &source, 0)?,
                    )?,
                    1,
                )
            })
            .collect::<std::result::Result<Vec<_>, MemoryPortError>>()
            .map_err(|_| Error::storage())?;
        let expected_receipt = MemoryEffectReceipt {
            source_event_id: source.clone(),
            identity: ids.resident(npc).clone(),
            memory_ids: records.iter().map(|r| r.id().into()).collect(),
            relationship_ids: vec![],
        };
        let manifest =
            MemoryImportManifest::new(source, ids.resident(npc).clone(), records, vec![])
                .map_err(|_| Error::storage())?;
        residents.push(PreparedResident {
            identity: ids.resident(npc).clone(),
            manifest,
            expected_receipt,
        });
    }
    let source = FrozenSource {
        conversation: state.conversation,
        mira_conversation: state.mira_conversation,
        residents,
    };
    db.execute(
        "INSERT INTO village_legacy_memory_sources VALUES(?1,?2)",
        params![
            save,
            serde_json::to_string(&source).map_err(|_| Error::storage())?
        ],
    )
    .map_err(|_| Error::storage())?;
    Ok(())
}
impl DurableVillage {
    pub(crate) async fn import_legacy(&self, store: &Store, ids: &VillageIdentity) -> Result<()> {
        let row:Option<(String,String)>=store.db()?.query_row("SELECT s.save_id,s.payload FROM village_legacy_memory_sources s JOIN village_engine_identities i ON i.save_id=s.save_id WHERE i.payload=?1",[serde_json::to_string(ids).map_err(|_|Error::storage())?],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|Error::storage())?;
        let Some((save, payload)) = row else {
            return Ok(());
        };
        let source: FrozenSource = serde_json::from_str(&payload).map_err(|_| Error::storage())?;
        if source.residents.len() != 2 {
            return Err(Error::storage());
        }
        for (npc, prepared) in [NpcId::Tavi, NpcId::Mira].into_iter().zip(source.residents) {
            if prepared.identity != *ids.resident(npc) {
                return Err(Error::storage());
            }
            let bound = self
                .memory
                .bind(ids.authority(npc).map_err(|_| Error::storage())?)
                .await
                .map_err(|_| Error::storage())?;
            // Always replay the frozen manifest at the canonical writer. Its source receipt
            // returns before any insert, preserving later corrections and tombstones.
            let outcome = bound
                .import_manifest(prepared.manifest)
                .await
                .map_err(|_| Error::storage())?;
            if outcome.receipt != prepared.expected_receipt {
                return Err(Error::storage());
            }
            let receipt = serde_json::to_string(&outcome.receipt).map_err(|_| Error::storage())?;
            let db = store.db()?;
            db.execute(
                "INSERT OR IGNORE INTO village_legacy_memory_receipts VALUES(?1,?2,?3)",
                params![save, prepared.identity.individual_id().as_str(), receipt],
            )
            .map_err(|_| Error::storage())?;
            let accepted:String=db.query_row("SELECT payload FROM village_legacy_memory_receipts WHERE save_id=?1 AND observer=?2",params![save,prepared.identity.individual_id().as_str()],|r|r.get(0)).map_err(|_|Error::storage())?;
            if accepted != receipt {
                return Err(Error::storage());
            }
        }
        Ok(())
    }
}
