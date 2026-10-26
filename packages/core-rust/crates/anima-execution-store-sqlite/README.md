# SQLite execution store

`SqliteExecutionStore::open(path, protection, clock)` takes a host-selected data
file, a complete core `PersistenceProtection` configuration, and an authoritative
clock. It does not read environment credentials, allocate keys, or create an agent
workspace. The host must protect the parent directory and keep its seal key outside
this database.

Each call goes to a dedicated storage thread through a 32-job bounded queue. The
worker uses `BEGIN IMMEDIATE`, restores the latest sealed reference-store image,
applies one public operation, exports the candidate and commits before replying.
Independent connections/processes share SQLite's writer lock (5 second busy
timeout). No model, network, tool, or approval wait runs inside these transactions.
There is no mutable authority cache. Caller cancellation after admission does not
cancel the job. Its receipt/state remains discoverable through the public port.
`shutdown()` closes admission for all clones and drains admitted operations;
dropping all senders also drains the queue.

Host admissions use the same explicit model-payload permission, declared-secret
replacement and integrity seal as snapshots. Their signed envelopes bind owner
and run; failed protection leaves no raw admission payload behind. Scope mappings
and shared `ExecutionScope` allowance rows are separate from the execution blob.
Allowance handles bind owner, scope and a store UUID created atomically on first
open. Reopen never creates a replacement UUID for missing/corrupt metadata, and
the facade rejects switching a run to a fresh independent allowance store.

Allowance metadata contains identifiers and counters and assumes trusted local
administration. Its validated JSON is not a cryptographically authenticated
distributed ledger. A copied/older backup needs an explicit host recovery policy.

Schema creation and initial snapshot commit together. Unsupported schemas,
missing authority rows, wrong keys, and corrupt/protection-incompatible snapshots
fail closed. There is no automatic repair, pruning, key replacement, or legacy
run import. Every operation includes `retry_run`, not the trait's default rejection.

The snapshot's integrity seal authenticates its content; **it is not encryption**.
`allow_model_payloads` explicitly retains bounded input/transcript plaintext in
the authenticated snapshot. Capability payloads require complete manifest/secret
inventories; declared secrets use the core replacement policy. Never put provider
credentials in definitions or model messages. SQLite files, WAL, backups, and
host data directories need the host's normal access/retention policy. Deleting a
file is not secure erasure. Application memory/world data belong in separate stores.

The clock floor is persisted even when an operation observes expiry and returns
an error. Host clock rollback cannot revive an observed expired fence. Metadata
and SQLite locking assume a trusted local host; restoring an older backup or
editing the database is an administrative recovery operation, not an authenticated
distributed clock. No database driver lives in `anima-core`.

This is a bounded local full-snapshot adapter. Core snapshot size limits remain
enforced and failures retain the previous generation. Reads also advance the
clock floor and therefore take a short write transaction. Cost grows with total
retained execution history. It is not certified for real workloads until a measured
latency/size envelope and all host migration gates are recorded. No multiplayer
capacity, live provider acceptance, or production cutover follows from store tests.

The temporary debug-build capacity fixture used 32,768-byte inputs and retained
241 runs at 16,708,208 of the 16,777,216-byte snapshot limit. Eight real admissions
near that limit measured p50 2,074.994 ms and maximum/p95 2,900.802 ms on this
machine. The next admission failed without changing generation or prior content.
This records the full-image adapter's cost; it does not establish a production
latency target or justify unbounded history retention.

Tests use only temporary files and scripted models. `engine_recovery` kills real
child processes at named handshake barriers, then reconciles a separate durable
effect ledger. Token deltas are ephemeral; semantic events come from committed
store pages. Existing host profiles still use their legacy runners.
