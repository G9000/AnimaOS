# Explicit durable daemon profiles

`DaemonEngineBridge::open` accepts an authenticated owner UUID, injected SQLite execution and
memory stores, a separate host outbox path, persistence protection, model adapter and clock.
`register_dialogue` opts one stable existing agent ID into required private memory. `register_clock`
opts in the same profile with the single `daemon.clock.read` capability. Pass the resulting service
to `app_with_engine_bridge`, or explicitly install it in the daemon state when embedding.
Default app/server constructors do not install this bridge.
The protected outbox proof pins both stores' persistent UUIDs. Replacing either file with a fresh
database, mixing owners, or supplying the wrong protection key fails before admission.

The registered agent ID is the individual's stable ID. The host persists the owner, individual,
opaque application scope, full accepted configuration and definition, session/run IDs, execution
and memory format versions and original finite allowance before dispatch. Authored personality,
knowledge, provider/model settings and accepted instructions are frozen. A retry uses those
accepted values and the original owner/store/scope allowance binding. It cannot allocate replacement
allowance or fall back to a legacy runner. Persisted profile selection reloads when reopening the
explicit bridge. Durable request IDs are rejected by an app that has not installed their profile.
An explicit increased definition version can select a changed model/persona for new runs while
preserving the individual and application scope; prior runs keep their original complete profile.
Sessions pin a definition version. Start a new `roomId` when selecting a new
definition version; the individual and its private canonical memory remain stable.

The regular authenticated `POST /api/agents/{agent_id}/run` handler dispatches registered profiles.
Their bounded request is `{ "requestId": "<uuid>", "roomId": "<uuid>", "text": "..." }`.
The original UUID and exact body must be retained for retries. Responses identify `engineFormat`,
`memoryFormat`, `runId`, `sessionId`, `individualId` and a `result` containing status/data/error.
Existing legacy clients require a separately reviewed adapter for this explicit profile contract.

The durable worker holds the actual daemon-wide run permit and one per-profile gate. An HTTP
disconnect does not abandon the spawned worker. Core controlled model calls and capabilities use
one persistent allowance for physical attempts, input/output tokens, retries, tools, in-flight work
and deadline. The bridge never constructs `AgentRuntime` inside a capability or charges a second
legacy budget. Unsupported tools, plugins, helpers and fallbacks refuse profile registration.
Provider/model selection must be explicit. Legacy tool-round limits include one final model response
when converted to durable model turns: a dialogue with zero tool rounds remains valid. Clock profiles
with zero allowed tool rounds refuse registration before accepting any source event.

Accepted user statements become private episodes (or preferences when the statement says
"prefer"), labelled as user statements rather than authoritative world facts. The source event,
immutable versioned run mapping and pending memory effect commit in one host SQLite transaction.
The host projector validates the effect against the actual accepted text, applies it using the
shared memory component's stable event/observer receipt, then acknowledges in the host outbox.
Readiness reads that authoritative pending queue. Projection/ack failure prevents dependent model
generation and returns an explicit memory-unavailable error. Generated output is not automatically
promoted into memory facts. No legacy memory writer is called for these runs.

Additional authenticated endpoints are:

- `POST /api/agents/{agent_id}/durable-memory-events`: accept/project a user statement using the
  same request contract, without invoking the model. Returns canonical memory/source event IDs.
- `GET /api/agents/{agent_id}/durable-memory?q=...`: scoped canonical recall.
- `POST /api/agents/{agent_id}/durable-memory-sources`: an authenticated explicit owner source
  for correction, forgetting, or a directed user-statement relationship observation. Its body is
  `{ "sourceEventId": "<uuid>", "operation": { "kind": "...", ... } }`. Observer, owner and
  scope come from the registered profile and cannot be supplied by the caller. Exact source
  retries replay their durable outcome, and another payload under that source ID conflicts.
- `GET /api/agents/{agent_id}/durable-relationships`: this individual's evidence-backed perspective.
- `POST /api/agents/{agent_id}/durable-runs/{run_id}/control/{stop|pause|resume}`: persist the control
  request and signal an active run. A paused run is reattached by retrying its original run request.
- `GET /api/agents/{agent_id}/durable-runs/{run_id}/events`: bounded semantic SSE replay. `after`
  or `Last-Event-ID` resumes a cursor; `follow=true` also streams live text/semantic events with
  an explicit gap event for a lagged subscriber.

Clock registration enables only a read-only, bounded, run-referenced capability. Its manifest has
no filesystem, process, secret or network grants. Callers must inject protection containing the exact
`clock_manifest()` secret inventory (empty secret fields). Current policy binds owner, observing
individual, accepted definition and the opaque application scope. The executor validates the execution
fence before reading time and commits one protected result/receipt per logical invocation in the host
ledger. Recovery returns the retained result; missing uncertain receipts require recovery review.
This read-only contract does not authorize arbitrary effect executors.

Lifecycle source operations use these fields:

| Operation kind | Required operation fields | Semantics |
| --- | --- | --- |
| `correction` | `memoryId`, `expectedRevision`, `corrects`, `reason` | The replacement is an already accepted memory record; CAS applies to that replacement's current revision, matching the portable correction contract. Both records must be active and private to the observer. |
| `forget` | `memoryId`, `expectedRevision`, `reason` | CAS-guarded canonical forgetting, including authorized expired or superseded records. A new source cannot forget an already-forgotten record again; the original source still replays its receipt. Audit history and receipts remain; recall and derived relationship evidence disappear. |
| `relationship` | `targetAgentId`, `relationshipType`, `summary`, `strength`, `confidence`, `evidenceMemoryIds` | A directed owner user-statement observation toward another registered same-owner individual, with existing active authorized evidence. The target receives no reciprocal knowledge. |

Mutation admission and projection hold the observing profile's gate. Trusted exact lifecycle reads
and store-floored active checks reject private foreign records and inactive/expired/superseded
evidence before source acceptance. Accepted source payload and pending effect are protected and
committed together. Canonical memory remains the only writer of revisions and relationship state.
An accepted effect that loses a canonical CAS retains a durable explicit rejected outcome; it is
not discarded or retried as an unrelated observation. Transient failures keep it pending and block
dependent turns until projection and acknowledgement settle.

The bridge's host mapping is separate from the legacy `RunRecord` ledger. Existing records retain
their existing reader and runner. No durable checkpoint or new format is fabricated from legacy
snapshots. Existing transcript/history UI, connector/schedule/helper flows, legacy approvals and
coding profiles remain on the old path. Full legacy imports, transcript/response/UI compatibility,
production keys/providers, workload capacity, coordinated production shutdown and live-provider
acceptance still require their product migration gates. This constructor is an opt-in integration
boundary, not a default production cutover or a statement that all daemon agents are migrated.

Tests use copied configurations, injected keys, temporary files, the real authenticated HTTP router,
the shared persistent-host contract, storage commit/ack failures and physical child-process death.
They do not discover credentials, read existing saves, restart a user's daemon or call live providers.
