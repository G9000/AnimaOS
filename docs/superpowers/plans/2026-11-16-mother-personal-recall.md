# Bounded Mother reads and personal-memory recall repair

> For agentic workers: use `superpowers:executing-plans` inline. Reviewers are
> read-only. The owner explicitly authorized these repairs, preserving A6 and
> memories, followed by a new test of the existing Discord conversation.

**Goal:** Repair oversized manual context reads, personal-memory selection and
misleading transport errors, keeping Mother additive and sharing explicit.

**Architecture:** Mother bounds serialized preview responses at its MCP boundary;
the native store, identity, grants, receipts and canonical records are unchanged.
AnimaOS owns owner-intent query planning and source selection. Transport failures
retain both their cause and uncertain-outcome safety. Preserve the qualified A6
working tree and all installed data; qualify before installing either new binary.

**Tech stack:** Existing Rust, redb, MCP and serde JSON. No new dependency, database
migration, grant, persona, sharing mode or tool schema is required.

## Agreed design and alternatives

The screenshot's last run called `mother_get_context(limit:100)`. A fresh response
had 75,527 bytes of canonical JSON before its duplicate text block and RPC framing,
exceeding AnimaOS's 65,536-byte frame cap. The service retired the dispatched call
and collapsed its useful cause into `outcome_unknown`. The selected connection is
degraded. The older "19 memories" were native task-result notes, including earlier
assistant replies, not a garden inventory. Canonical pet records are present.

Prefer bounded MCP preview responses plus harness-owned selection. A client-only
silent limit clamp would leave other harnesses exposed and still mishandle a large
minimum envelope. Globally raising frame/output limits would not repair wrong
selection and would weaken existing resource bounds. Do neither.

Mother's MCP preview formatter will fit complete preview items to a small byte
budget, initially 12 KiB of canonical JSON, with accurate omitted/returned counts
and category counts, preserving category seats even when displayed newest-first.
Keep identity, persona, pinned/assigned owner context (including required lists)
and search/index coverage intact. If that required context cannot fit, return a
small explicit read-size error; do not silently cut persona or canonical records.
Full read responses must fit the MCP wire envelope or return a small explicit
size error; AnimaOS must also diagnose canonical structured reads that fit the
wire but cannot fit its smaller result envelope, rather than dropping their JSON.
The formatter must not modify the store or mutation requests/receipts. Apply it to
native service bridges and native direct MCP consistently, without changing the
advertised tool descriptors or direct owner-store/export contracts. Full-record
reads remain complete; do not present truncated full records as authoritative.

AnimaOS will recognize owner-memory questions (including cat/pet questions and
"what do you remember about me") and make bounded literal searches for useful
owner terms, constrained to the memory category when appropriate. Do not hardcode
the owner's name, pet names, record IDs or title prefixes as required schema.
Rank complete current facts over historical assistant task/checkpoint notes for
these requests. Keep pinned/assigned precedence, topic isolation, four full v1
fetches, A6 graph bounds and the total four-item/8 KiB/three-second recall limit.
Prompt guidance must distinguish native task-result text from garden facts and
require fresh targeted retrieval before claiming absence. Generic memory-health
questions use an inventory/health-only intent: no discovered full-record fetches,
hybrid fallback or graph expansion. Preserve explicitly pinned/assigned owner
context. They do not authorize loading unrelated sensitive details.

Preserve a wire-size cause through the generic MCP error path. A failed dispatched
write remains uncertain, retires its connection, and must never be auto-replayed.
Exposing a useful size cause must not imply that the remote operation did not
happen, grant a retry, weaken cancellation or reuse stale shared context. Retain
existing fresh-turn reconnection and current binding/review/permission checks.

## Steps and checks

- [x] Snapshot the exact qualified A6 diff/hashes and live profile/config/queue
  baseline privately. Create an owner-private task/temp root; no real data in Git.
- [x] Add synthetic Mother preview-size regressions with 100 Unicode-rich records,
  large required context, accurate inventory/category/omission metadata and
  unchanged full-record reads/receipts. Observe behavioral red before implementation.
- [x] Add AnimaOS owner-recall regressions with distracting newer task/checkpoint
  notes, unnamed synthetic pets, spelling boundaries, empty/denied results,
  project-topic non-regression, native tools and explicit sharing unchanged.
  Add a wire-size error regression proving uncertain-write semantics are retained.
- [x] Implement the minimal formatter, query-plan/selection and error changes.
  Preserve A6 graph modules and all existing integration regression behavior.
- [x] Review the source, then run focused checks and actual disposable native
  Mother/AnimaOS integration with reviewed schemas and synthetic scoped grants.
- [x] Qualify Mother with fmt, locked tests, locked release build and all-target
  all-feature Clippy with warnings denied; audit normal/build dependencies for
  SQLite. Qualify AnimaOS with full rust:test, rust:lint and a locked bundled build
  at frozen source hashes, serially under the 32 GiB managed cache ceiling.
- [x] Prepare backed-up native runtime upgrades and then activate only verified
  binaries, preserving existing native databases/generation, client files, agent
  settings, credentials, sharing queue and all memories/history. No new graph
  grants. Restart/review connections only as required by unchanged descriptors.
- [x] Run exactly one initially approved real-model test exchange in the original Discord session
  through AnimaOS owner API, with no exact tool name, record ID, pet name or answer
  supplied. The owner explicitly approved one exchange in "Sweet Cat Photo" after
  being told that this API path posts into Discord. Verify current full garden
  provenance, the correct answer, native memory active, no garden publication and
  successful Discord delivery. Disclose provider transfer before running. Persist
  the test allowance and unique operation/idempotency key before submission;
  reconcile an uncertain submission rather than posting another exchange.

  The first exchange exposed an additional planner bug: unquoted citation `ID`
  overrode personal-topic queries. Fixed only the Owner acronym fallback,
  preserving quoted names and genuine protocol topics; added exact citation-style
  PromptContext and real native regressions. Requalified AnimaOS (6,019 passes,
  lint, bundled build and three native cases), installed and restart-verified it.
  The owner then separately approved exactly one additional exchange. The identical
  diagnostic passed full-record provenance, correct recall, native-tool execution,
  read-only tool scope and actual Discord delivery. Separate allowances,
  operation IDs and evidence preserve both attempts; no uncertain action replayed.
- [x] Audit all original records/history and settings after restart/verification,
  document evidence and remaining limits, checkpoint Mother, and remove only owned
  scratch after checking paths and active users. Retain failed trials/backups and
  the original dirty additive worktree. Do not publish source without a request.

Owned areas: Mother's `src/mcp/` preview formatter/native handlers and synthetic
tests; AnimaOS `memory_provider/selection.rs`, `mother.rs`, workspace prompt
guidance, MCP error mapping/tests and native acceptance. Keep new responsibilities
in small modules rather than growing unrelated server or runtime code.
