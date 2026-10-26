# Durable canonical memory

`SqliteMemoryStore` stores memory independently of execution checkpoints. Canonical records,
original writes, revision snapshots, lifecycle mutations, supersession edges, directed
relationship observations, identities, optional resident mappings and projection/import receipts
commit in SQLite transactions. WAL plus `synchronous=FULL` is used. One dedicated worker and a
32-entry queue keep SQLite I/O off asynchronous executor threads. Cancelled reply receivers do
not cancel admitted writes. Shutdown closes admission and acknowledges after draining and closing
the connection; independent opens serialize writes with `BEGIN IMMEDIATE`.

Reads and rejected lifecycle operations also persist the greatest observed clock
value. Rejected candidate mutations roll back independently of that clock floor.
A record or relationship hidden by an observed retention deadline
cannot reappear when the host clock moves backward or the store reopens with an
earlier clock. Clock metadata is required on recognized databases; it is never
silently recreated. This assumes trusted local administration and excludes
restoring an older backup without an explicit host recovery procedure.

Initial schema creation also commits a stable store UUID. `store_id()` exposes it
for host attachment pins alongside the execution-store UUID. Recognized databases
with missing or invalid identity metadata fail closed; reopening never invents a
replacement authority for already acknowledged memories.

Register host-assigned `IdentityBinding`s through `IdentityRegistry`, then call `bind` with a
host-issued `MemoryAuthority`. Only `BoundMemoryStore` implements `MemoryPort` and
`MemoryProjectionPort`. All queries and lifecycle targets stay within its exact grants and owner
partition. A supplied `KnowledgeAccessContext` can narrow those grants. Shared Session/Workspace
membership must be explicitly authenticated by the host; a Workspace ID is an opaque space and
grants no filesystem access. Agent and Owner scopes cannot name another individual or owner.
Resident mappings are optional, immutable, and unique per individual; different worlds need
different individual IDs. Relationship targets must be registered in the same owner/world partition.

Memory IDs are unique within an owner. Rewriting an ID with the exact original payload returns
its current record without changing lifecycle state; another payload conflicts. Revision operations
require the current revision. Corrections and supersessions validate every target, remain within
one scope, and reject cycles. Original writer and lifecycle actor are stored separately from
caller-supplied provenance. All five portable memory kinds are supported without changing their
validated core wire format.

Trusted lifecycle code can use `get(id)` to inspect an exact authorized record,
including inactive history, and `get_active(id)` for active correction targets
or relationship evidence. Both retain the same owner/scope boundary. Recall's
recent-record limit does not limit which known records can be corrected or forgotten.

Ephemeral retention is explicitly refused for durable writes/imports. Such data
belongs to request-local storage; it is never silently converted into persistent
memory. Persistent and deadline retention use the canonical lifecycle/clock checks.

Recall applies ownership, exact scopes, forgetting, expiry and supersession before constructing
an in-memory BM25 corpus using `anima-memory`'s existing algorithm. The corpus contains the most
recent `max(512, query.limit)` authorized active records, capped at 1,000. `recent` returns up to
1,000 active records. This is bounded lexical/recent recall, with no embeddings or claim of full
history semantic search. Relationship reads preserve the observer's directed perspective, temporal
validity, and evidence IDs; inactive or invisible evidence removes derived relationship hits.
Derived state is rebuilt from canonical rows, so correction/forgetting needs no stale index cleanup.

Build `MemoryCandidate`s with evidence classified as witnessed event, user statement, received
message (with sender), or inference. `MemoryEffect` can be serialized into the host's authenticated
outbox and deserialized through validated, bounded constructors. It still requires
`MemoryProjectionPolicy` validation against the current authority to become a
`ValidatedMemoryEffect`; that type cannot be deserialized. Candidate provenance must name the
effect's source event. Effects have at most 64 combined items and 128 KiB of serialized payload.
Host policy must explicitly approve candidates, revisions, and relationship observations.

The host must atomically commit its accepted event and pending memory effect in its own source
database, apply the effect, and acknowledge the returned receipt afterward. This adapter commits
the memory changes and receipt atomically under `(owner, observing individual, source event)`.
Exact retry returns the committed receipt; a changed payload conflicts. Current handle grants are
rechecked on replay. A crash between application and acknowledgement therefore permits replay
without duplicating memory or relationships. There is no cross-database transaction here. The next
dependent turn must wait on the host's projection readiness contract. Abandoned/rejected model
output has no authority to create an effect.

`MemoryImportManifest` accepts explicit copied records and relationship observations, preserving
IDs, scopes, retention, provenance and lifecycle links. Import is bounded to 1,000 records and
1,000 relationships / 4 MiB and is transactional and replay-safe. It does not inspect live legacy
storage, change old readers, infer knowledge from world flags, or fabricate missing history.
Historical relationships can retain inactive evidence while remaining absent from recall. Hosts must select and verify copied profile extraction before enabling a product profile. Imported revision history starts at the imported
baseline; missing prior revisions are not invented.

`MemoryImportManifest::from_legacy_snapshot` accepts a copied `MemoryManagerSnapshot` and an
explicit legacy-agent-to-individual binding. It preserves memory IDs and private/shared/room
visibility using the existing legacy converter; missing shared world or room/session scope fails
closed. A snapshot containing another agent's memories is rejected. No live source is opened.

The exact typed snapshot is atomically retained in the import receipt as a
`LegacySnapshotArchive`, including entities, directed legacy relationships, temporal facts,
temporal relationships, original metadata and source/supersession links. Its schema is versioned;
unknown serialized fields and non-finite numbers are rejected instead of silently discarded.
Each typed collection is limited to 1,000 rows; the complete manifest remains limited to 4 MiB.
Exact retries compare the complete payload, including the archive; a changed import payload or
conflicting typed ID in the same legacy profile is rejected. No cryptographic digest substitutes
for that exact comparison.

Auxiliary legacy rows have no reliable observer/private scope in the old model. They stay labeled
`legacy/unknown` in an identity-bound archive, inspectable with the selected profile's current
private grant plus every imported record scope. They are never automatically converted into
canonical facts or individual relationship observations and never enter generation recall. This
is lossless historical preservation, not complete semantic conversion of unscoped history.

`export_profile_for_rollback` reads the current canonical writer's records (including tombstones,
retention and correction links), native relationship evidence and preserved legacy archives in
one transaction. It rechecks current grants and keeps owner/individual partitions separate.
The versioned JSON export is readable without reopening an old legacy snapshot. It is bounded to
1,000 records, 1,000 native relationships and 1,000 import receipts / 16 MiB; exceeding a limit
fails explicitly and never truncates. Archives and inactive rows are historical inspection data,
not a way to revive forgotten or superseded facts. Imported baselines and post-cutover writes
continue through the same canonical writer and active-lifecycle reader. This export is an
inspection artifact, not a standalone restore image: shared-scope corrections written by another
individual may refer to records outside this writer-filtered export. Their effective lifecycle
remains authoritative in the retained canonical database.

A host rollback must stop new durable admissions while retaining the canonical store/reader for
already accepted memory and in-flight runs. It must not switch memory back to the stale source
snapshot, dual-write legacy and canonical copies, or feed historical archives to generation.
This adapter supplies copied import and current-data export; the host owns profile cutover,
source extraction, runner routing and rollback admission policy.

Open rejects unrelated databases, unsupported schema versions and missing canonical tables.
Backend, corruption and shutdown failures are explicit `MemoryPortErrorCode::Backend` errors;
required-memory product profiles must surface them. This schema stores plaintext canonical/audit
content: the host owns private paths, access controls and any required encryption. Forgetting and
expiry invalidate recall while preserving audit records/receipts; they are not cryptographic erasure.
Ephemeral lifecycle boundaries and retention deletion remain host policy. These limits must be
included in product consent and migration review before cutover.

Tests use temporary databases, independent connections, a deferred-FK failure at real COMMIT,
cancelled callers, concurrent shutdown, and physical process death followed by fresh-process
identity/receipt/preference recall. They do not touch existing saves, credentials, real providers,
or running services.
