# Connected search and account workflows

## Authorized outcome

The owner can opt existing native account connections into a shared search, inspect cited results and freshness, and create rules that start saved workflows from newly observed account activity. Native provider coverage is Gmail, Outlook mail, IMAP, selected Drive files and Google Calendar. Arbitrary MCP tools do not become background readers.

## Source and persistence boundary

Use a bounded normalized index in the existing control-plane snapshot, supporting both JSON and Postgres. Keep reusable execution in core; native provider orchestration belongs to the daemon. Index grants bind agent, connector and authorization generation. Disable/disconnect/reauthorization/scope change immediately hides stale content, clears baseline and requires renewed owner enablement. Query and sync completion recheck current connection ownership, generation, status and read grants.

Sync every 120 seconds and on owner refresh, with one in-flight refresh per source. Fetch outside state locks; commit under the existing durable mutation discipline, rolling back on failed persistence. Retain at most 100 recent inbox messages per mail source, 16 selected Drive files, 500 calendar events over the documented bounded window, 8 KiB UTF-8 text per item and 16 MiB global indexed bytes. Surface retained scope, truncation, capacity and stale/error states. Index is a bounded snapshot, not a full mailbox archive or lossless delivery log. Fetch complete pages for the retained scope; a transport/incomplete pagination failure must preserve the prior successful snapshot. Bounded retention does not imply remote deletion.

Normalized documents include external/collection ID, kind, title, text, sender, source URL, provider modification/received time, event start/end, revision and truncation. Derive citation IDs from agent, connector, generation, collection and external ID. Treat text as untrusted content, never instructions. Validate provider URLs; owner-only internal item retrieval supplies citations for sources without a public URL. Search is token-based, case-insensitive, ranked, capped at 30 hits, with source freshness/coverage warnings. Agent search requires an explicit registered connected_search tool and the current per-source read grants.

## Workflow boundary

An owner-created, revisioned rule binds one current source generation, observed event kind, bounded literal filters, saved template slug/revision and parameter bindings from literals or a closed event-field set. No arbitrary prompt target or implicit send grant. Events are newly observed mail, selected-file changes, and observed calendar changes; UI must state polling/retained coverage limits. Initial sync, new/re-enabled rule, reconnect, widened scope and invalidated baseline suppress historical workflow starts. File/calendar removals from a retained window are not remote deletion events.

Source commits call workflow admission synchronously in the same control-plane transaction. Match eligible rules, recheck source and template authority, materialize saved workflow runs, persist receipts and index together. Deterministic request keys include rule ID/revision, source generation and event identity. Capacity or persistence failure leaves prior source state and baseline intact; replay cannot duplicate a run. Bound rules and receipts. Changed template or source authorization pauses the rule until owner re-enables it. Queued work rechecks account-trigger authority before dispatch. Preserve existing workflow approval/input checkpoints and uncertain-effect recovery. Never impersonate an owner chat or reuse the IMAP automatic-reply cursor.

## Product/API

Search endpoints: GET connected-search/sources; PUT sources/{connector}; POST refresh; POST query; GET items/{citationId}, all under /api/agents/{agent}. Rule endpoints: GET/POST account-workflow-rules; PATCH/DELETE {rule}; GET {rule}/events. Owner read/mutation/origin guards, redacted errors and no-store responses follow existing route conventions. SDK exposes typed clients. Connections view contains source opt-ins, refresh, search/citations, rule editor and event history linked to workflow runs. Loading and mutation state is scoped to agent plus current render generation.

Retain stable event identities independently of the current retained window. Never silently evict identity history; surface exhaustion and require an explicit source reset. Each source enrollment has an independent persisted epoch; old in-flight reads cannot fill a disabled and re-enabled source even when provider credentials did not change. Each rule baseline excludes reads started before its enable/revision. Bound serialized metadata, source/rule counts and identity history as well as text. Owner mutations complete persistence/rollback independently of HTTP cancellation, and reads wait for that transaction to settle. Owner refresh requests enqueue an owned bounded read and return promptly.

## Acceptance

Synthetic adapters for all native providers; complete/partial pages, truncation and stale states; isolation by agent/generation; disable/reconnect/grant revocation during reads; baseline suppression; event replay after restart; invalid filters/bindings; template changes; capacity and persistence rollback; workflow checkpoints/uncertain effects. SDK path/body tests; UI opt-in/search/citation/rule flows including agent-switch races. Full Rust suite and lint; SDK test/build/typecheck/lint; focused web tests/build/typecheck/lint; companion docs build. No live accounts are read during development.
