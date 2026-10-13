# M41 bounded own-history discovery and anchored reading — implementation plan

> Execute only after root publishes binding risks and observes meaningful RED.
> Root owns branch cut, serial gates, isolated services/model acceptance and Git.

**Goal:** discover one older own conversation with explicit filters and read a
bounded anchored window whose real roles, identities and omissions are visible.
**Architecture:** reuse current sessions/history/coordinator/tool policy; add a
small bounded forward-page port where existing older-only paging cannot suffice.

## Frozen integration contracts

- history/mod.rs: `MessageAfterQuery {agent_id:String,session_id:String,after:MessageOrder,limit:usize,include_hidden:bool}`; `FilteredSessionSearchQuery {agent_id:String,session_ids:Vec<String>,excluded_message_ids:Vec<String>,query:String,after_ms:Option<u64>,before_ms:Option<u64>,limit:usize}`. New trait methods `page_messages_after(&MessageAfterQuery)` and `search_sessions_filtered(&FilteredSessionSearchQuery)` return `Result<Vec<HistoryMessage>,HistoryError>`. Defaults report unsupported, never scan all history; memory/SQLite/Postgres implement them. Filter/time predicates precede grouping and limit.
- sessions/history_read.rs: `DiscoveryRequest {query:String,limit:usize,kind:Option<SessionKind>,after_ms:Option<u64>,before_ms:Option<u64>}` and `WindowRequest {session_id:String,anchor_message_id:String,before_count:usize,after_count:usize}`. Free async functions `discover_conversations(state:&SharedDaemonState,agent_id:&str,current:Option<&str>,request:DiscoveryRequest)` and `read_conversation_window(state:&SharedDaemonState,agent_id:&str,request:WindowRequest)` return `Result<serde_json::Value,HistoryReadError>`. `HistoryReadError` has static Display text and no source content. Typed expected absence is successful JSON status not_found; infrastructure/scope limits are errors. Module functions provide bounded final JSON; tool adapter serializes without extra unbounded wrapping.
- New coordinator facade names exactly `discover_conversations` and `read_conversation_window`, forwarding those inputs without changing the existing `search_conversations` method or owner session paging. New module registered by root in sessions/mod.rs.
- Ordinary current-session windows are allowed by exact own read grant; helper/delegated/hidden/deleted sources are refused. Discovery still excludes current session. Missing/foreign share not_found. Title-only matches without time bounds are labelled navigation/anchorUnavailable; with matching-message time bounds, a title alone does not qualify.
- One five-second returned-result deadline per lookup, no retries. Window source queries are exact anchor plus one before and one after, each <=20 rows including at most one continuation witness; no page walking. Failed cold queries report unavailable rather than complete emptiness. Time inputs are integer UTC milliseconds in0..9007199254740991, after inclusive/before exclusive.
- Tool worker owns tools/conversations.rs, tools.rs, tools/tests.rs, agent_runs/conversation_tests.rs and approvals/policy.rs; history worker owns history/mod.rs, memory.rs, sqlite.rs, postgres.rs, conformance.rs; window worker owns sessions/history_read.rs plus its contained/submodule tests. Root owns agent_runs/conversations.rs, agent_runs.rs, helper tests, module registration, docs, all commands and publication. Production edits begin only after the root-observed meaningful search-anchor RED.
  **State:** active after verified M40 merge cfd346e8f40d86acd019aeff0bbe4ceaec08ba43; implementation and scoped acceptance complete. Exact gates and publication remain in the M41 ledger.

## Current implementation and demonstrated source gaps

- tools/conversations.rs already exposes search_conversations(query,limit).
- It searches other own sessions, includes archived sessions, excludes helpers,
  and frames excerpts as data; default5/max10 results, query<=200 characters.
- SessionMatch already stores message_id and snippet, but excerpts() prints only
  session ID/title/kind/snippet. The model cannot reliably name the matched anchor.
- sessions/views.rs search_conversations merges archived/unarchived lists and sorts
  newest activity first; no model-visible kind/time filter currently exists.
- Existing store search_sessions groups by session before the500-session limit.
- Existing owner session message paging resolves before IDs and merges hot/cold rows.
- HistoryStore has exact get_message(agent,session,message) and older-only pages;
  it has no bounded newer-than-anchor read. Repeated latest-page walking is unsuitable.
- SessionPrunedThrough marks hot-tail pruning; it is NOT proof the archived message
  was deleted. Historical read must try the durable exact source first.
- Native stores are in-memory, SQLite and Postgres; no service/database server is
  started for this plan or inferred from embedded SQLite fixtures.
- Existing contracts already preserve toolCalls/toolCallId/runId/stepId/toolStatus;
  taskResult raw metadata is intentionally hidden from owner responses.

| Source seam           | Current anchor                      | Intended reuse/change                                   |
| --------------------- | ----------------------------------- | ------------------------------------------------------- |
| Search tool/parser    | tools/conversations.rs:13,40        | Add anchors + optional strict filters                   |
| Tool descriptor       | tools.rs:587                        | Add filtered fields and new exact read descriptor       |
| Own search            | sessions/views.rs:352               | Scope filtered candidates before result limit           |
| Exact merged pages    | sessions/views.rs:402               | Reuse owner checks/hot precedence/hidden handling       |
| Session match         | sessions/views.rs:122               | Preserve real matching message ID                       |
| Exact history lookup  | history/mod.rs:260                  | Resolve requested anchor under exact owner/session      |
| Older page port       | history/mod.rs:273                  | Retain existing method, add bounded forward query       |
| Stable order          | history/mod.rs:62,74                | createdAtMs/ordinal/ID total ordering                   |
| Store implementations | history/{memory,sqlite,postgres}.rs | Focused query/conformance additions                     |
| Hidden turns          | sessions/mod.rs:614                 | Keep silent-checkin visibility semantics                |
| Prune watermark       | sessions/mod.rs:194                 | Hot-only marker; never invent durable deletion          |
| Deletion outbox       | state.rs:1638                       | Respect pending source/agent deletion before/after I/O  |
| Read policy           | approvals/policy.rs:31              | Exact configured descriptor grant; Read tools never ask |
| Helper isolation      | agent_runs.rs:112,119               | New history read excluded like existing search          |

All native paths are under hosts/rust-daemon/src/; line numbers are this planning
snapshot, refreshed before implementation. No host-only history driver moves to core.

## Minimum milestone and exclusions

1. Extend existing search_conversations with stable session/message anchors and
   explicit kind/time bounds; preserve existing query/limit calls and scope defaults.
2. Add read_conversation_window as a separate named read-only tool, max20 messages
   and16KiB final serialized text, no full-session export or unlimited scrolling.
3. Reuse current own session registry and history store, never installed Hermes DBs.
4. Prove one fresh actual task discovers and reads a declared cold conversation
   amid distractors and answers a source-specific question correctly.
5. No new history database, session lineage rewriting, automatic summaries/scout,
   model-assisted search, profile/identity mutation, or external account access.
6. No binary attachment reading; this is text evidence and pairing metadata only.
7. SDK/web source changes only if an exposed owner contract needs them; existing
   messages UI/opaque tool results can remain unchanged for a tool-only milestone.
8. A read result does not approve writes, grant tools or verify old assistant claims.

## Candidate discovery contract (root freezes before RED)

- Existing tool name search_conversations; keep required query and optional limit.
- Add optional kind enum: chat|telegram|discord|checkin|job; helper is refused.
- Add optional after_ms and before_ms: finite integer UTC epoch milliseconds in
  supported signed-store/JS-safe range, after inclusive/before exclusive.
- Define range over MATCHING MESSAGE createdAtMs, not mutable lastActivity or
  session-start time; disclose this in descriptor and output to avoid ambiguity.
- after>=before is invalid; absence preserves current all-time behavior.
- Query remains nonblank<=200 Unicode chars, with cheap byte precheck/hidden checks.
- Unknown argument keys, wrong types, fractional/NaN values and invalid enums refuse.
- Runtime-derived own agent and current session exclusion only; no agent_id argument,
  include_helpers, include_hidden, alternate profile, DB path or resource override.
- Eligible session IDs come from current own SessionRecords after kind/scope policy;
  history does not reconstruct a deleted or unknown session from its orphaned rows.
- Time/kind/hidden/ownership predicates apply BEFORE newest-match-per-session grouping
  and limit, in both native store and hot candidates. Never post-filter only topN.
- Results contain sessionId, title, kind, archived, matchMessageId, matchedAtMs,
  role and bounded snippet; matchKind distinguishes title versus message if retained.
- A title-only match has no invented matching message. Either return anchorUnavailable
  honestly or resolve one real visible bookend as an explicitly labelled navigation anchor.
- JSON text has fixed evidence-only guidance; titles/snippets/queries are escaped data.
- Default5/max10 results, individual snippet current160 chars; final rendered JSON<=8KiB.
- Preserve whole usable IDs; if bytes run out, omit a whole result or shorten text,
  and report resultShortened/hasMore rather than emit a partial anchor.
- Empty results do not prove absence outside the selected filters/indexed text cap.
- Store failure cannot masquerade as complete empty history; return typed unavailable
  or partial:true/storeUnavailable with explicit hot-only coverage, never silently complete.

## Candidate anchored window contract

- Tool: read_conversation_window.
- Required strings session_id, anchor_message_id, each exact nonblank<=256 bytes.
- Optional before_count(default5) and after_count(default5), integers0..19;
  sum+1<=20 including anchor. Zero neighbors still returns one valid anchor.
- No raw cursor accepted as authority, no user-supplied MessageOrder or agent identity.
- Resolve anchor by exact own session+ID from hot/cold; refuse current helper/delegated
  session, hidden turns and pending deletion. Current session policy root binds explicitly.
- Return one chronological window with requested anchor present, bounded neighbor
  counts, and typed status found|not_found|unavailable (pruned only with actual proof).
- Missing and foreign session/anchor use the same generic not_found; never discover
  an anchor's actual owner, parent session or another agent to satisfy the request.
- Stable fields: sessionId, anchorMessageId, messages[], hasEarlier, hasLater,
  shortened, sourceCoverage and pairingIncomplete; no fabricated total count.
- Each row: messageId, role, createdAtMs, text, textShortened, runId?/stepId?,
  toolCalls summary?/toolCallId?/toolStatus?, attachmentContentOmitted boolean.
- Preserve existing stopped/incomplete/revised/budgetWrapup provenance flags;
  partial historical text must not become an apparent accepted final answer.
- Tool-call summaries contain real ID/name only; arbitrary args/raw taskResult
  metadata are omitted. Tool message text remains bounded quoted historical evidence.
- Roles include user/assistant/tool; system instructions are excluded, not reinserted
  as active system messages. Empty assistant call rows may still carry useful pairing.
- Every included row has its real complete anchor; no model-generated summary row.
- A selected boundary can split call/result pairs. Report counterpart absent/unknown,
  rather than silently claim a complete execution trace or exceed20 rows to fill pairs.
- Prefer JSON output with fixed evidence-only guidance and UTF8-safe text excerpts.
- Per-message text<=2KiB initially; entire serialized Content.text<=16KiB including
  JSON escaping, identities, flags and guidance, not merely raw text lengths.
- Keep the anchor and its real identity; trim neighbor text/rows deterministically
  under byte budget, set shortening flags, and never split a Unicode code point.
- Attachment IDs/count presence can be reported; no bytes, remote fetch or implicit file read.
- Invalid args produce ordinary static Tool Error; found/expected not_found can use
  successful lookup result with typed status. Infrastructure unavailable is an honest
  error/partial result, never a retry loop inside the history tool.

## Native interfaces and exact merge plan

- Add daemon module sessions/history_read.rs for bounded discovery/window assembly;
  existing owner messages paging stays source-compatible unless a small shared helper suffices.
- Candidate coordinator methods discover_conversations(agent,current,query)->Result<...>
  and read_conversation_window(agent,current,request)->Result<WindowResult,HistoryReadError>.
- Add HistoryStore::page_messages_after(&MessageAfterQuery)->Result<Vec<HistoryMessage>,HistoryError>:
  exact agent/session/after MessageOrder/limit/include_hidden=false, oldest-first.
- Keep page_messages and MessagePageQuery unchanged to avoid changing every old caller.
- Default implementation may return explicit unsupported/unavailable, never full-history
  scans; all three native stores implement it and share conformance fixtures.
- New filtered search request carries exact own allowed_session_ids, time bounds,
  query and grouped result limit; native SQL scopes/filter predicates before grouping.
- Keep old search_sessions compatibility as delegation to unfiltered request or retain
  the old entry plus a new default-unavailable filtered method; root selects smallest diff.
- Bound allowed-session ID payload; recommended500 per request. If more eligible
  sessions exist, typed scope_limit/incomplete asks for narrower kind/time or explicit
  bounded continuation. Never truncate silently or mislabel empty as a full search.
- Material design decision: session kind lives in control plane, not history rows;
  either pass bounded server-scoped IDs or add a native metadata join. Do not infer
  authority/kind from model-supplied prefixes or introduce a second session registry.
- Capture live own SessionRecord, room ID, hot candidates, hidden IDs, store handle
  and relevant pending deletions under one brief state read; release before store I/O.
- Exact anchor lookup then at most one before page + one after page, each bounded
  request<=20(+one continuation witness if root permits); no history pagination loop.
- Merge by ID with hot precedence consistent with current session_messages;
  canonical MessageOrder gives deterministic same-millisecond numeric-counter order.
- Defensively recheck every returned row's owning agent/session, room/visibility,
  direction/order and limit; a noncompliant/mock port cannot expand the requested scope.
- Keep hidden policy consistent across hot/cold including whole silent-checkin turns.
- Recheck live own session/agent and pending deletion after I/O; deleted source fails closed.
- Store errors never read another store/profile. If complete neighbors cannot be
  established, mark coverage incomplete or fail unavailable; do not invent missing rows.
- Bound overall read deadline (candidate5s) with existing cancellation support;
  no waiting on a model, no provider calls or history writes.
- Serialized output/read-count bounds do not claim a hard SQLite scan/CPU limit:
  verify native query cancellation behavior; a detached store operation may finish
  after returned deadline. Reuse bounded native I/O admission, never retry on timeout.
- Pruned hot anchor present in durable store is FOUND. SessionPrunedThrough alone
  cannot justify a pruned status; absent healthy-store anchor is generic not_found.
- Any later explicit destructive-retention record can support a precise pruned reason;
  do not add a tombstone/retention subsystem merely to invent that distinction.

## Grants, Stop and history evidence authority

- Add exact native descriptor and Read category; honor the exact configured descriptor grant; existing Read tools never ask.
- Require exact configured read_conversation_window grant; no inferred authority from
  search result, source text, existing memory writes or model-provided session IDs.
- Root binds default profile advertising; recommended no helper inheritance or silent
  grant widening, with isolated acceptance enabling only selected test tools explicitly.
- Existing search default visibility/archive/current-session exclusions stay compatible.
- No secrets/system prompt/profile metadata/hidden checkin suffix inside read output.
- Historic instructions are quoted evidence, not current directives or new grants.
- Historic toolSuccess means recorded result, not verified present effect/current truth.
- Stop cancels normal dispatch/model loop; history tool has no replay or auxiliary request.
- No persistent receipt claiming a native mutation for this read-only operation.
- Owner current question wins over remembered preferences/old assistant assertions.

## Ownership and bounded tasks

1. Root: final interface/binding risks, existing-router/executor meaningful RED,
   branch cut after M40 verified merge, shared coordinator/registry/docs registration.
2. History worker: history/mod.rs request/method and memory/sqlite/postgres queries,
   conformance.rs focused cases; no DB process or canonical store redesign.
3. Window worker: sessions/history_read.rs + focused synthetic tests; reuse current
   ownership, hidden-turn and deterministic hot/cold utilities where practical.
4. Tool worker: tools/conversations.rs parser/rendering, tools.rs descriptors,
   approval category/capability inventory and helper exclusion, compact real executor tests.
5. Root assigns disjoint ownership of shared sessions/views/coordinator registration;
   do not let workers independently patch the same module or broaden HTTP exports.
6. Native independent batch review then final SDK/web contract compatibility check;
   no implementation/gates delegated by this planning-only task.

## Meaningful RED and focused tests

- Existing configured tool executor new read name returns Unknown tool instead of
  found real own window; prerequisites prove persisted exact anchor and scoped rows.
- Search regression verifies actual tool output includes usable matchMessageId and
  filters choose the valid older match despite many newer wrong-kind/out-of-range hits.
- Store conformance for before/after order, exact agent/session, hidden filtering,
  0/1/20 neighbors, same millisecond IDs9/10/100, SQL timestamp bounds and limits.
- Native window hot/cold duplicate fixture proves one chronological copy and hot
  precedence; cold-only anchor, newest/oldest and empty-neighbor cases are actual reads.
- Missing/foreign/hidden/helper/current-policy/deleted/pending-deletion sources refuse
  generically; no parent/other-session fallback, no metadata/attachment leakage.
- Hot-pruned anchor still cold-present succeeds; healthy-store missing anchor typed
  not_found, failed-store anchor unavailable, not a manufactured pruned deletion claim.
- Shortening test uses huge Unicode text/JSON escaping/tool outputs; final JSON<=16KiB,
  anchor retained, text flags correct and no raw taskResult duplication.
- Pairing fixture includes assistant parallel calls, tool results, error/refusal and
  window boundary: actual IDs/roles preserved, incomplete pairs labelled honestly.
- Filter tests cover inclusive-after/exclusive-before, invalid dates/numbers/enums,
  unknown keys, current-session exclusion and explicit old API compatibility.
- Query timeout/cancellation/failed cold read retain honest coverage and no writes,
  provider invocation, extra tool retry or modified primary run/approvals/receipts.
- Scope-limit fixture proves bounded incompleteness, not a false zero-result answer.

## One bounded real local acceptance

- Root creates a small declared synthetic archived conversation in isolated history,
  with exact source answer and distractors in other own sessions/kinds/time ranges.
- Arrange target anchor cold-only and neighbor context necessary to answer correctly;
  no private history, actual accounts or personal installed Hermes DBs are touched.
- One fresh empty task, normal internal provider, bounded4 tool rounds/20s runtime
  and root-declared aggregate requests/reported-token/time ceiling before dispatch.
- Task must actually search with the chosen filters, then call read_conversation_window
  with returned exact session/message anchor and give independently verified answer.
- Root captures real search/read tool witnesses, paired history and actual output;
  no fixture-only quality claim, no answer leakage in prompt or copied conversation.
- If search/read/quality fails, record incomplete; no repeat/probe/scout to manufacture PASS.
- Optional quiet restart verifies the original immutable seeded history and later
  tool/run messages survive; restart itself performs no model call or automatic replay.
- Report limits: one synthetic case does not establish broad recall quality,
  arbitrary document understanding, account access or general Hermes parity.

## Reuse decision and licensed public comparison

- Inspected installed PUBLIC tools/session_search_tool.py at parent-verified pin
  1fd18277267152893a8a24792b79213e7ca3f899; no code execution or personal profiles/DBs.
- Repository LICENSE is MIT, Copyright2025 Nous Research (previous licensed-source check).
- session_search uses discovery(query), scroll(session_id+around_message_id), full
  read(session_id) and browse; its own module states all shapes return DB data with no LLM.
- \_parse_iso_bound/:127 and \_in_time_window/:170 support UTC/relative bounds over
  session-start times; M41 instead freezes explicit message-time epoch-ms semantics.
- \_discover/:354 groups by lineage, demotes cron and scans300 hits; Anima reuses its
  own session grouping/scopes, with explicit kind filters rather than adopting ranking policy.
- \_read_session/:447 caps per-message content2000 chars but may return head/tail;
  M41 omits full-session shape and additionally enforces serialized UTF8 byte budget.
- \_scroll/:531 calls get_messages_around and can retry an anchor's owning session
  within allowed lineage; M41 requires exact own requested session and refuses fallback.
- \_shape_message/:237 formatting/scroll hints are useful narrow design references,
  but do not port numeric DB IDs, lineage walks or Python profile resolution.
- Direct package/process reuse would add foreign session/profile/DB authority and
  Python lifecycle while duplicating existing native stores/paging. Lowest total cost:
  existing Anima primitives plus narrow attributed forward-page/window glue.
- If substantially porting helper logic, add HERMES-NOTICE source path/pin/full MIT
  license/local differences. No direct copying or full upstream policy parity claim now.

## Top five risk candidates for root binding

1. Own-source authority: exact session+anchor, live owner/helper/hidden/deletion checks
   before/after I/O, generic wrong-owner errors and no lineage/profile fallback.
2. Filter completeness: apply time/kind before grouping/topN; bound server-selected
   ID scope and report limits/store degradation rather than false complete emptiness.
3. Window fidelity: MessageOrder/hot precedence, actual pairing metadata/omissions,
   hot pruning versus durable deletion distinctions and honest absent/partial status.
4. Byte/call budget:20 rows/16KiB fully serialized output, bounded per-source reads,
   no full-history scans/LLM helper/automatic tool retries or grant expansion.
5. Semantic acceptance/reuse: one source-specific real synthetic task with actual
   anchored reads and checked answer; reuse current native ports by total cost,
   retain MIT provenance and no broad recall/privacy/account parity claim.

## Root-only commands and completion gates

- Meaningful existing executor/router RED, focused native/store conformance GREEN;
  serial CARGO_INCREMENTAL=0/jobs1 with exact commands recorded by root.
- Required bun x nx run rust-daemon:test --skipNxCache and native lint once source freezes.
- Relevant SDK/web Nx targets only if their source/contracts change; no unnecessary
  unchanged frontend rerun, no new target directory or dependency without justification.
- Root formats/reviews one native batch+final contract check, one justified fix wave,
  then bounded acceptance/cleanup, public docs/handover/ledger and explicit-path publication.
- Preserve nx.json and unrelated work; no services/model calls or implementation occurred here.

## Controller rulings on the risks (binding)

**Owner control adjustment:** current Settings exposes fixed access profiles and explicit optional memory tools, with no general tool picker. Add a separate default-off **Allow reading conversation context** checkbox through the existing agent tool configuration API. Preserve custom/unrelated grants, remove only this tool on opt-out, exclude it from profile derivation, clear it on explicit standard-profile reset and allow deliberate re-enable. Failed saves retain the draft, and helpers cannot enable it. No default/profile grant widening or new SDK API. Its missing-control/opt-in test must show meaningful RED before UI production. Required web test/type/build gates now apply; SDK source remains unchanged.

Ruling: Exact own live non-helper session and full anchor identity, checked before and after asynchronous reads; no lineage/profile fallback — quoted history must not confer another source's authority — cost if wrong: migrated or deleted aliases need a future explicit compatibility rule.

Ruling: Kind and matching-message time filters precede grouping/topN, with server-selected own IDs and a bounded500-session scope; if scope exceeds that bound return explicit incomplete/unavailable instead of complete empty results — avoid false absence and unbounded SQL parameters — cost if wrong: larger workspaces need bounded pagination later.

Ruling: Keep deterministic MessageOrder and hot precedence, real roles and call/result IDs; label incomplete pairing and omitted attachments; hot pruning alone never means durable deletion — preserve evidence fidelity — cost if wrong: partial windows require owner/model judgment.

Ruling: Maximum20 rows/16KiB final serialized UTF8 text, bounded exact anchor plus neighbor queries, one returned-result deadline and no internal model/retry/full-history walking — keep lookup cheap and prevent oversized outputs — cost if wrong: large answers require a separately scoped follow-up window.

Ruling: Reuse current native stores/coordinator/tool policy, informed by pinned public Hermes session search/window shapes; one declared fresh synthetic task must actually search/read/answer, and old text remains quoted evidence — a foreign Python profile/history adapter duplicates existing authority at higher cost — cost if wrong: broad recall quality and full upstream session-lineage semantics remain unproven.

Ruling: Exclude up to5000 captured hot message identities before cold grouping, so a stale mirrored match cannot hide a valid older current match — hot content is authoritative during lookup — cost if wrong: larger hot scopes return an explicit scope-limit error; no silent fallback.

Ruling: Keep existing Read-class behavior and exact configured descriptor-name grants; there is no Read Allow/Ask/Deny policy — adding an approval schema exceeds this lookup milestone and would invent an unsupported promise — cost if wrong: owners needing per-read prompts require a separately scoped policy feature.

## Execution evidence

Meaningful executor anchor RED and optional-checkbox RED preceded production. Three disjoint implementation lanes, root port review, one native batch and final SDK/web review led to one native fix: malformed exact cold rows now return unavailable. Added deadline/future-drop and deletion-during-read regressions. Two fixture construction errors were corrected without changing runtime behavior or weakening assertions.

Fresh required Rust2525passed/9ignored/40suites/lint, Settings34 and fullweb1245/111files/typecheck/build pass. All18 source SHA256/UTCticks are frozen; SDK source/contracts remain unchanged and final opaque-result review is clean.

One admitted fresh synthetic task actually performed filteredsearch -> exactanchorwindow -> correct source-cited answer, three model steps/two tools/5420 reported tokens, zero repair/retry/scout/reviewer requests. One quiet same-store restart preserved exact run/messages/answer/source/protected memory. All85 prior canonical memories,139 history rows and25sessions stayed exact; five synthetic history messages/three sessions and two ordinary runtime journals were declared additions. Original21 grants/3 nullable bounds were restored; all14 exact process tuples are gone. No broad recall, complete provider billing, graceful wrapper shutdown or whole-Hermes parity claim.

Private runner setup faults were diagnosed before any generation: explicit Bun read modes, snapshot DTO path, descriptor guards and process-local Hashtable key ordering. Known non-admission or acknowledged configuration outcomes were proven before scoped continuation; the sole provider task was never repeated.
