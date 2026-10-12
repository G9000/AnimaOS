# M30 Explicit Removal of One Stale Own Fact — Implementation Plan

**Status:** Root-bound on `g9000/companion-memory-m30`, cut after reviewed/gated M29 PR35 merged at `c8df3087870b829092116a84e72838b3e4a825fd`. Root owns gates/Git; source-only implementation and synthetic fixtures only. No actual owner-data deletion, services, provider call or profiles/credentials.

**Purpose:** Close one narrow memory-lifecycle gap: the agent can add, search, and exact-content update its own visible memories, but has no model tool to explicitly remove a stale own Fact. Preserve owner scoping, visibility filters, per-agent tool grants, configured approval policy, and all unrelated memory records.

## Read-only finding

- `hosts/rust-daemon/src/tools/memory.rs` has `memory_search`, `memory_add`, `memory_update`, and `recent_memories`; there is no model-facing remove/forget tool.
- `memory_search` returns visible records with exact IDs, types, and full content. `memory_update` checks the exact `agent_id`, hidden-text visibility, and unchanged full `expected_content` while holding the memory write lock.
- `MemoryManager::delete_memory` already hard-removes one ID, updates its text index, strips that ID from citations, removes agent relationships left without evidence, and retains temporal facts/relationships while updating their evidence links.
- The owner-only `DELETE /api/memories/{memory_id}` route calls `delete_memory`, persists through `MemoryMutation`, then removes embeddings. It does not require expected current content and is not exposed as an assistant tool.
- Core has `MemoryRevision::forget(memory_id, expected_revision, reason)` and `MemoryPort::revise`, but the daemon uses the legacy `MemoryManager`. The explicit bridge says this manager is not a `MemoryPort` because it cannot enforce revision CAS, correction lineage, or auditable forgetting. Do not present the portable core contract as an implemented daemon path.
- Conclusion: the capability gap is real. The smallest honest daemon feature is a one-record **hard removal**. Prefer tool name `memory_remove` over `memory_forget` so its name does not imply the core port's retained tombstone/reason semantics.

## Narrow recommendation

Add one optional, explicit tool named `memory_remove` for a single Fact record. The model must receive the record ID and exact full current content from `memory_search`; the handler verifies both again under the same memory write lock before removal.

The handler may act only when all of these are true:

1. The ID exists in the current manager.
2. The stored record's `agent_id` exactly matches the calling `AgentState.id`.
3. The record type is exactly `MemoryType::Fact`.
4. Existing `visible_memory` rejects no part of its ID, content, or tags.
5. The supplied `expected_content` is byte-for-byte equal to stored content; never trim or normalize before comparison.
6. The current per-agent config includes the exact `memory_remove` grant and the existing approval evaluator permits the write.

Missing, foreign, non-Fact, hidden, or stale records fail without mutation. Prefer one generic not-found/not-eligible error for records the caller cannot remove; never disclose hidden or foreign text. Do not accept a batch of IDs, a type/scope override, a search query in place of an ID, wildcard/partial content, or a force flag.

Do not require an unpersisted “reason” argument. The legacy store has no audit field for it; returning success must not imply tombstone history or auditable forgetting. The action is permanent for that memory record under current `MemoryManager` semantics.

## Existing interfaces to reuse

- Handler: `hosts/rust-daemon/src/tools/memory.rs::execute_memory_update` is the closest model-tool precedent for argument validation, own-agent lookup, hidden-text checks, exact-content CAS, and persistence.
- Mutation: hold `context.memory.write()` across eligibility check, exact comparison, `MemoryMutation::new`, `delete_memory`, and `persist(context.memory_store.as_ref())`. Ordinary precommit errors roll back the existing snapshot; a reported committed-but-durability-uncertain save retains the committed canonical state. Preserve existing cancellation/store behavior, without promising a cross-store transaction.
- Indexes: use the established memory-to-embeddings lock order. After saved removal, remove that memory ID's embedding with existing best-effort/logging semantics. Do not claim index removal is transactionally atomic with the snapshot store.
- Record effects: preserve `MemoryManager::delete_memory` citation cleanup exactly; do not directly edit unrelated memory records or relationships as a separate sweep.
- Tool grant: register a descriptor/handler but do not add `memory_remove` to any existing agent's saved `config.tools`, access profile, default selection, helper grant, or global authorization.
- Approval: classify `memory_remove` as existing `RiskClass::Write` beside `memory_update`. Existing per-agent policy continues to decide Allow/Ask/Deny. Current default Write policy is Allow, so explicit tool grant is still required but does not itself imply an approval prompt.
- Discovery: add it to the memory category in `routes/capabilities.rs`. Capability inventory remains discovery, not a grant.
- Guidance: update only the memory tool/context text needed to tell the model to search for exact ID/current full text, remove one eligible Fact, and re-search after a stale/missing result. Do not change existing memory update/add semantics.
- Leave owner API contracts/routes, core `MemoryPort`, SDK/web DTOs, data migrations, semantic/vector ranking, consolidation, and unrelated memory routes unchanged.

## Expected handler sequence

1. Parse required `id` and `expected_content` as strings; reject missing, wrong-type, blank ID, or empty expected text before taking the lock.
2. Do not trim expected text. The exact bytes, including whitespace, are part of the stale check.
3. Acquire the memory write lock; look up the ID and verify agent ownership before returning any record detail.
4. Check `MemoryType::Fact` and the same `visible_memory` predicate used by retrieval/update. Noneligible records produce no state change and no disclosure.
5. Compare stored full content exactly to `expected_content`. A mismatch returns a stale-result error instructing a fresh search, with no mutation.
6. Create `MemoryMutation` under the lock; call existing `delete_memory`; persist the resulting manager snapshot before reporting success.
7. On ordinary persistence failure, return an error and let the existing mutation guard restore the manager/index snapshot. Preserve the store's existing uncertain-durability semantics.
8. Remove the embedding for the deleted ID using the existing lock order and best-effort warning behavior. Do not pretend vector cleanup and snapshot persistence are one transaction.
9. Return a short truthful result such as “removed one Fact memory.” Do not echo memory content, claim duplicate cleanup, or claim that all related facts were deleted.
10. Release the write lock and leave the agent's grants, approval policy, other memory contents, and tool results unchanged.

## File list if root binds

| File                                            | Bounded purpose                                                                                     |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `hosts/rust-daemon/src/tools/memory.rs`         | Add one `memory_remove` handler using the existing exact-CAS and `MemoryMutation` pattern.          |
| `hosts/rust-daemon/src/tools.rs`                | Register its schema and handler; add exact search-before-remove guidance.                           |
| `hosts/rust-daemon/src/tools/tests.rs`          | Schema, authorization boundary, CAS, mutation, persistence, citation, and preservation regressions. |
| `hosts/rust-daemon/src/approvals/policy.rs`     | Add the exact tool name to `WRITE_TOOLS`; no global policy/default change.                          |
| `hosts/rust-daemon/src/routes/capabilities.rs`  | Classify registered tool as memory, preserving the distinction between inventory and access.        |
| `hosts/rust-daemon/src/components/providers.rs` | Only if needed, update the existing memory guidance to include exact-search-before-remove.          |

No change is planned for `packages/core-rust/crates/anima-core/src/memory.rs` or `packages/core-rust/crates/anima-memory/src/core_port.rs`; those expose a different revisioned lifecycle abstraction that the legacy daemon manager does not implement.

## Focused regression list

1. Registry includes `memory_remove` with exactly required `id` and `expected_content`; both are bounded nonempty strings, with no bulk or force parameters.
2. `memory_remove` is explicitly classified as `RiskClass::Write`; policy Allow/Ask/Deny behavior follows the existing evaluator.
3. Tool registration does not add a grant to an existing agent config or default profile; an unconfigured `memory_remove` remains refused before handler execution.
4. Exact current own visible Fact is removed, no longer returned by search, and its embedding removal path is requested.
5. A neighboring own Fact survives unchanged; repeated identical facts are not swept or silently deduplicated.
6. A related relationship/fact loses only the removed memory's citation according to `delete_memory` semantics; unrelated citation records remain byte-for-byte/state-equal.
7. Stale, paraphrased, trimmed, or whitespace-changed expected content fails without changing snapshot, search index, embeddings, or citations.
8. Foreign ID and missing ID return the same generic error and preserve both records.
9. Own Observation, TaskResult, and Reflection are ineligible and remain unchanged; do not broaden removal to “all own memories.”
10. Hidden ID, content, or tag is refused generically with no hidden content in the result and no mutation.
11. Persistence failure rolls back the manager and search index; no success response is emitted. Preserve preexisting uncertain-commit behavior.
12. Invalid/missing fields and nonstring values fail before any lock-backed mutation; no target is inferred from text or recent-memory order.
13. Two concurrent requests using the same ID/current text cannot double-delete or delete a changed replacement; write-lock sequencing yields one success and a safe second failure.
14. Existing `memory_update`, `memory_add`, search visibility/limit, owner API delete, and approval tests remain unchanged and passing.
15. No unrelated memory record is edited, deleted, reclassified, re-embedded, consolidated, or backfilled as a side effect.

## Five binding risks for root

1. **Destructive action naming:** Bind hard deletion semantics and the `memory_remove` name, or defer. Do not call the legacy manager path revisioned/auditable “forget” just because the portable `MemoryPort` has a separate forget revision type.
2. **Ownership and visibility:** Require exact agent ID, Fact type, and existing hidden-text filter while holding the write lock. Generic errors for missing/foreign/noneligible records prevent enumeration; any uncertainty means no mutation.
3. **Staleness and concurrent edits:** Compare exact current full text, without trim/normalization, at the mutation boundary. The legacy contract is string-content CAS, not revision CAS; if protection against ABA/revision history is required, defer until the host adopts a real `MemoryPort` implementation.
4. **Persistence and related data:** Use snapshot rollback, existing citation cleanup and embedding-removal behavior. Preserve independent memories; document best-effort embedding cleanup and current uncertain-durability semantics rather than promising a cross-store transaction.
5. **Authority and approval:** Register the capability without granting it to saved agents. Use existing write classification and per-agent policy; default Write currently allows, so root must accept that existing policy or defer rather than quietly changing defaults/global approvals.

## Root verification if bound

- First add focused meaningful tests and observe RED in the existing daemon tool suite; do not run them during planning.
- Root runs the existing Rust host target once after implementation: `bun x nx run rust-daemon:test --skipNxCache`. It covers daemon tool, route, storage, and reusable core dependencies; avoid a duplicate full core aggregate gate.
- Run `bun x nx run rust-daemon:lint --skipNxCache` after confirming the current Nx target/flags. Use existing configured target directories and serialize Cargo per the current root procedure.
- SDK contracts remain unchanged. The final P2 fix below adds a bounded web opt-in, so run current web test/typecheck/build targets; do not repeat unchanged SDK tests.
- No service, provider, model, account, profile, credentials, broad memory evaluation, vector quality test, or live deletion is needed. Synthetic unit fixtures must own their temporary memory state and clean it up.
- One bounded native review and at most one fix wave; no broad memory audit, consolidation work, migration, or secondary feature bundled into M30.

**Canonical plan:** This root-bound document governs execution; the ignored draft is historical. Controller rulings and root ownership below are binding.

## Controller rulings on the risks (binding)

1. Ruling: Add one optional memory_remove for one active Fact, using native hard deletion and full current-text matching; reuse public Hermes remove/pinned-entry intent, not tolerant substring/batch matching — an explicit stale-fact removal completes a personal memory lifecycle without claiming revisioned tombstones or semantic consolidation — cost if wrong: defer/remedy the optional tool, never sweep actual records or promise erased chats/backups/receipts.
2. Ruling: Exact agent ownership, Fact type, existing hidden-text rejection and complete unchanged expected_content are verified under the same write lock as MemoryMutation/delete/persist; missing/foreign/nonFact/hidden targets get generic refusal — prevent enumeration and stale/partial/ambiguous removal — cost if wrong: meaningful no-effect fixtures block publication; content CAS does not promise revision/ABA protection.
3. Ruling: Preserve existing precommit rollback and reported committed-but-durability-uncertain canonical-state behavior; retain established delete_memory citation cleanup — independent memory record content/identity remains, related citation links may change and empty-evidence agent relationships may disappear — cost if wrong: store/citation fixtures block merge; no cross-store transaction or no-effect-on-error assertion.
4. Ruling: After successful saved removal, canonical absence recheck under memory read lock precedes best-effort embedding cleanup in existing memory-to-embeddings lock order — avoid removing a replacement record's index; stale vectors cannot resurrect a canonically absent memory — cost if wrong: omission of vector cleanup is reported, no new embedding/model service or semantic recall claim.
5. Ruling: Exact new grant must be selected explicitly, existing RiskClass::Write Allow/Ask/Deny policy retained, no saved-agent/profile/default/helper auto-grant or UI/client schema change — adding a capability is not consent to use it; handler args bounded/nonempty but comparison retains exact bytes — cost if wrong: ungranted/denied/stale tests and final SDK/web review prevent publication; no global approval-policy rewrite.

## Root execution ownership

One source worker owns daemon tools/memory.rs, tools.rs, tools/tests.rs,
approvals/policy.rs, routes/capabilities.rs (and provider guidance only if concrete
need). First add a compile-ready registry/execution regression with existing APIs,
no missing production symbols; root observes meaningful missing-tool RED before
production. Then minimal handler/descriptor/risk/category and compact synthetic
matrix. No source refactoring, store/revision framework, normal profile or real
model/data/service activity. Root owns plan/notice/docs/ledger/Cargo/Nx/Git.
Use CARGO_INCREMENTAL=0 serial Cargo and existing targets; wait for completed
session exit before any next Cargo. One native batch and final web/SDK contract
review, one fix wave, then required host aggregate plus lint; no duplicate core
aggregate or unchanged TS gates. Narrow approved runner for existing mockHTTP
fixtures avoids the established restricted-network failure. Preserve nx.json.

## Controller ruling on the final contract finding (binding)

Ruling: Fix the final review P2 with one separate web Settings opt-in for memory_remove instead of shipping API/SDK-only selection — the requested console must let the owner use the optional tool without technical API calls — cost if wrong: UI payload/permission-preservation regressions and current web gates block publication; no default or existing-agent auto-grant.

The UI starts from the current exact grant. Unrelated saves omit tools. Toggling
only memory_remove preserves every other configured name. Explicitly selecting
an existing standard profile keeps its existing replacement semantics and clears
the optional removal selection; the owner may then explicitly check it again.
Changing name/model/system or reopening settings never enables it. A failed save
keeps the unsaved draft without assuming success or changing the displayed server grant. Existing saving/resetting controls and
agent-change reseeding apply. Plain copy explains active saved-fact removal,
chats/backups retained, and existing approval rules; no new confirmation flow.
Daemon registration/validation remains authoritative, including unsupported old
hosts; no new SDK DTO, capability query, host/port hardcoding or dynamic picker
framework. This overrides the planner's no-UI-scope assumption for the concrete
P2 only. Ownership apps/web/src/components/SettingsPanel.tsx and its existing
test file; no standard-profile list change. First one meaningful UI RED before
checkbox production, then the same fix wave and current web test/typecheck/build.
SDK source stays unchanged; its historical gate remains separate evidence.

## Verified result

Required current Rust host aggregate passed 2,419 tests, eight ignored, across 40 suites; lint passed after the completed test session. Eight grouped native regressions pass after one meaningful unknown-tool RED. One native batch review found no defects. The final SDK/web review's P2 (API-only grant selection) was fixed in one wave with a separate Settings opt-in, following its own missing-checkbox RED. Full web verification passed 1,197 tests (five new cases), type checks and build plus existing transitive build/type checks. Seven source hashes stayed unchanged through gates; no default/profile grants, SDK DTO changes, services, provider calls or actual owner memory deletion. SDK unit tests remain historical unchanged-source evidence. Uncertain-commit retention uses the existing store fixture; no cross-store atomicity, revision/ABA protection, complete erasure or live model removal claim. Exact tested source, PR, verified merge and resume are recorded in the milestone ledger.
