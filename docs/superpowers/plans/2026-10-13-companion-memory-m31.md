# M31: Guard owner memory deletion against stale reviewed text

**Status:** Root-bound on `g9000/companion-memory-m31`, cut from merged M30
`95b5daf4c347297abce8c2aa30cb68d4c2a6245d`. Implementation/reviews/current local gates complete; exact evidence and publication live in the milestone ledger.

## Goal

When an owner reviews a memory in the console and confirms deletion, the daemon
must delete only when its complete current text matches the reviewed text. If it changed after it
was reviewed, the daemon returns a conflict and leaves the record untouched.
After the console refreshes, the owner must review the current text and open a
new confirmation before another delete request.

This is a narrow compare-and-delete change for `DELETE /api/memories/{memory_id}`.
It does not change memory patching, temporal facts, entities, agent tools,
profiles, approval defaults, retention, or deletion of conversations/backups.

## Verified current interfaces

- `hosts/rust-daemon/src/routes/memory_edits.rs` authorizes the local owner
  before handling the request. The current delete handler ignores the body and
  calls `apply_delete(daemon, memory_id)`.
- `apply_delete` takes the existing memory write lock, reads the canonical
  record, creates `MemoryMutation`, persists the deletion, then removes its
  embedding while preserving the memory-to-embedding lock order.
- `routes/http.rs` provides `read_limited_body` using the configured
  `max_request_bytes` and `parse_json_body`; malformed/oversized requests already
  map to the established bad-request response path.
- `MemoryPatchRequest` in `routes/contracts/memory_edits.rs` demonstrates the
  existing `camelCase` plus `deny_unknown_fields` request convention.
- `ApiError::conflict` is already used by memory edit routes. Keep conflict text
  fixed and do not include either old or current memory content in the response.
- `packages/sdk/src/memories.ts` exposes `MemoriesClient.delete(memoryId)`;
  `apps/web/src/lib/daemon-api.ts` wraps it as `deleteMemory(id)`.
- `useMemory.remove(memory)` currently calls the wrapper with only the ID.
  Its shared action handler reloads on HTTP 404 and 409 and returns `false` on
  failure.
- `MemoryList.tsx` renders the record content before confirmation. It currently
  invokes `onDelete(memory)` from the current row prop when confirmation is
  accepted; a refreshed prop could therefore silently replace what was reviewed.
  `MemoryRow` already closes confirmation after any failed action.
- Existing memory mutation persistence has a special reported
  committed-but-durability-uncertain outcome. Preserve it exactly; do not claim
  every 503 means rollback or no effect.

## Contract and safety rules

1. An empty HTTP body keeps legacy ID-only deletion working for older clients.
2. Any nonempty body must be valid JSON with exactly one required string field:
   `expectedContent`. Reject missing/null/wrong-type values, `{}`, arrays, and
   unknown fields with the existing 400 path. Do not accept `null` as legacy.
3. Compare the supplied string byte-for-byte with the canonical memory's full
   current content while holding the same memory write lock used for deletion.
   Do not trim, normalize Unicode, case-fold, reject whitespace, or cap the
   expected text at the edit limit. Existing request-body size limits bound the
   transport; stored legacy memories may exceed the edit limit.
4. If the ID is absent, retain the current 404 behavior. If the ID exists but
   `expectedContent` differs, return 409 with a stable generic message and make
   no memory mutation, citation cleanup, persistence call, or embedding change.
   The comparison and mutation must be serialized under the current lock so a
   concurrent edit cannot slip between check and delete.
5. If the text matches, use the existing deletion, persistence, citation cleanup,
   response body, embedding cleanup, owner authorization, and uncertain-commit
   handling unchanged. This is content compare-and-delete, not revision/ABA
   protection: changing text away and then back to the exact same value is not
   detected.
6. The console sends the exact content from the record snapshot whose Delete
   action opened the confirmation. Pin that snapshot on the initial click; do
   not substitute a newer row prop during an open or in-flight confirmation.
7. On 409, preserve existing behavior: the hook reloads current data, the failed
   confirmation closes, and the updated row/error becomes visible. The owner
   must review that refreshed text, click Delete again, and confirm again. Do
   not automatically retry or keep the stale confirmation open.
8. Preserve no-body use of the SDK and daemon wrapper for legacy callers. The
   owner UI path must always pass reviewed content. No API capability query,
   migration, new endpoint, secret/profile inspection, or agent-tool changes.

## Implementation sequence (test first)

### 1. Daemon request parsing and compare-and-delete

Owned files:

- `hosts/rust-daemon/src/routes/contracts/memory_edits.rs`
- `hosts/rust-daemon/src/routes/memory_edits.rs`
- `hosts/rust-daemon/src/routes/tests/memory_edits.rs`

Add a strict `MemoryDeleteRequest` with `expected_content: String`, serialized
as `expectedContent`, `deny_unknown_fields`, and schema documentation. In the
route, authorize first, read the body through the existing bounded helper, and
select legacy behavior only when the resulting byte vector is empty. Parse all
nonempty bytes as the strict request and pass `Some(expected_content)` to the
locked mutation. Keep malformed input on the established 400/no-store path.

Extend the delete route and OpenAPI response docs with 400 malformed request
and 409 stale reviewed content. Under the existing write lock, look up the
memory, return 404 if absent, compare the optional expected string exactly,
return conflict before constructing `MemoryMutation` on mismatch, and otherwise
continue the current delete/persist/embedding flow. Use one fixed conflict
message; do not echo expected or actual content.

First add the red route regression using only existing request/router helpers:
seed a record, submit a nonmatching expected string, assert 409, then verify the
memory, citations, vectors, and persistence-visible state remain unchanged.
Root observes this meaningful RED before implementation. Do not introduce a
missing production helper in the test.

Then add a compact request matrix:

- Empty body still deletes through the legacy path.
- Exact expected text deletes successfully and preserves the existing response,
  citation cleanup, embedding removal, cache header, and no-store behavior.
- Exact empty, whitespace, and Unicode content are accepted when they match;
  a one-character or normalization-only difference conflicts.
- Nonempty `{}`, `null`, missing field, wrong type, extra key, invalid JSON, and
  invalid UTF-8 return 400 without mutation. Existing body size enforcement
  remains authoritative.
- A stale request returns 409; after a fresh GET/review, the exact current text
  can be submitted explicitly and succeeds. The daemon does not retry itself.
- A concurrent edit/delete regression, if the existing harness can coordinate
  both operations without test-only production hooks, demonstrates that check
  and delete share the write lock. Do not add a lock/test framework for this.
- Keep existing persistence rollback and uncertain-commit fixtures; make no new
  blanket no-effect assertion for persistence failures.

### 2. SDK and web request threading

Owned files:

- `packages/sdk/src/memories.ts`
- `packages/sdk/src/memories.spec.ts`
- `apps/web/src/lib/daemon-api.ts`
- `apps/web/src/hooks/useMemory.ts`
- `apps/web/src/hooks/useMemory.test.tsx`
- `apps/web/src/components/memory/MemoryList.tsx`
- `apps/web/src/pages/MemoryPage.test.tsx`

Add an optional second `expectedContent?: string` argument to SDK
`MemoriesClient.delete` and the existing daemon API wrapper. When omitted,
continue sending DELETE with no body. When provided—including `""`—send exactly
`{ expectedContent }`. Keep the response DTO unchanged. In `useMemory.remove`,
pass `memory.content`; retain the current 404/409 refresh/error handling.

Pin the `Memory` object selected when the owner clicks Delete in the row. The
confirmation must use that object even if the list refreshes while it is open;
clear the snapshot when the owner keeps/cancels, after a failure, or after
success. Do not add a second confirmation, new dialog, hidden text rule, or
optimistic row removal.

Add focused SDK tests for both omitted content (no request body) and present
content, including empty text. Add hook coverage that a 409 reloads data and
passes the reviewed string. Add UI coverage for: confirmation uses the click-time
snapshot after props update; stale 409 closes the confirmation and displays the
refreshed content/error; owner must explicitly reopen and reconfirm; ordinary
matching deletion keeps its existing success behavior. Update existing
`deleteMemory` call assertions to include the second argument where appropriate.

### 3. Final contract review

Review the route schema, SDK signature, wrapper, hook, and UI together. Confirm
that old callers can still omit a body, the owner UI never does, the server
compares the full exact string under lock, and all stale/error paths leave the
owner with current information and no automatic retry. Do not extend scope to
other memory mutations.

## Principal risks and controls

1. **TOCTOU race:** comparison outside the write lock could delete text the
   owner did not review. Keep lookup, comparison, and mutation in one locked
   operation; test that stale text leaves every relevant store untouched.
2. **Accidental legacy break:** JSON `null` or `{}` must not be conflated with an
   empty body. Branch on actual body bytes before strict deserialization; retain
   no-body compatibility only.
3. **Text normalization or size truncation:** trimming, Unicode normalization,
   edit-limit reuse, or preview comparison could accept a different record.
   Compare the complete string exactly and let the existing request byte limit
   be the only transport bound.
4. **UI prop replacement:** a background reload can change the row while a
   confirmation remains open. Store the click-time snapshot and make a 409
   require a fresh owner action after the existing reload.
5. **Overclaiming delete/persistence semantics:** citations and empty-evidence
   relationships have established cleanup, embeddings are a separate store,
   and uncertain persistence may report an error after canonical commit. Keep
   current behavior and language; do not claim full erasure, cross-store
   atomicity, or rollback on every error.

## Verification owned by root

Use focused targets first, then the repository-required Rust host aggregate and
relevant web/SDK targets. Confirm target names from current Nx project metadata
before running; do not guess flags. Expected coverage includes:

- Rust: `bun x nx run rust-daemon:test --skipNxCache` (the required aggregate).
- Web/SDK: current Nx test, typecheck, and build targets for the affected
  projects, with `--skipNxCache` where the target supports it.
- Lint/format checks only through existing workspace targets and root-owned
  gates. Do not run duplicate core aggregates or introduce new targets.

No live provider/model call, running daemon, normal profile, credential access,
or actual owner memory deletion is needed. Synthetic seeded route fixtures and
the existing UI/SDK mock clients are sufficient. Record exact commands and
counts in the M31 ledger after root gates; this plan itself is not validation
evidence.

## Controller rulings on the risks (binding)

1. **TOCTOU:** Compare the complete reviewed string inside the existing memory write lock before `MemoryMutation` and deletion. Keep existing cancellation-safe task/persist/index ordering. Cost if wrong: an untested race may delete changed content; content ABA and metadata-only edits remain outside this comparator.
2. **Compatibility:** Exactly zero HTTP body bytes retain ID-only owner deletion. Every nonempty body requires a string `expectedContent` and rejects missing/null/unknown fields. The SDK optional second string argument preserves omitted-body calls. Cost if wrong: clients that previously sent irrelevant JSON now receive400; legacy no-body callers intentionally remain unguarded.
3. **Exact text:** Never trim, normalize, truncate or reuse edit/model-tool character caps. Blank, Unicode, hidden and long existing text are valid comparator values within existing transport byte limits. Keep current owner auth and memory types. Cost if wrong: exceptionally large stored text exceeding transport limits needs an explicit alternate workflow, without automatic unguarded fallback.
4. **UI review:** Pin the selected Memory snapshot on Delete click, submit that text even after background reload, and clear it on cancellation/completion. Existing409 flow reloads and closes confirmation; the owner must review/reopen before retry. Cost if wrong: a changed record requires an additional owner action; no automatic retry is allowed.
5. **Persistence:** Preserve existing precommit rollback and committed-durability-uncertain error behavior, separate embeddings and citation cleanup. State active-record removal only, no revision CAS, erasure or whole-store atomicity claim. Cost if wrong: uncertainty may require inspecting Memory before another explicit action.

Root owns all tests/Cargo/Nx/services/Git. First meaningful synthetic route RED and UI snapshot/transport RED precede production. One native batch review, one final SDK/web contract review, one justified fix wave, then current Rust/SDK/web gates. No real records/accounts/services/provider task is required. The ledger records every scope and test judgment.

## Current verification and publication

Two meaningful synthetic REDs reproduced stale DELETE200 versus409 and console
ID-only deletion versus complete reviewed text after a background refresh.
Four new grouped native cases and reused guarded/legacy persistence/citation,
404, rollback and OpenAPI fixtures pass. Four new web cases and one SDK case
cover request threading, click-time display/submission, conflict reload and
explicit reconfirmation, cancellation and duplicate confirmation.

One native batch and final SDK/web contract review found no actionable defects;
no code fix wave was needed. Fresh required Rust **2,423 passed / eight ignored /
40 suites**, followed by strictly sequential formatting, passed. Web **1,201**
tests/typecheck/build and SDK **128** unit tests/typecheck/build passed; three
SDK service tests were explicitly skipped. The first SDK run mistakenly entered
its integration build while native source was editing and failed before any
daemon start; its failed log is retained and the SDK target passed after applying
the existing skip setting. No repeated unchanged build/core gate was required.
All13reviewed source hashes are unchanged across gates.

No real memory, model task, service, account or private profile was used. These
fixtures establish native contracts, not browser acceptance, mixed-version host
negotiation, semantic recall, content ABA detection or complete erasure.
Exact tested source, PR/verified merge and resume are recorded in
`.superpowers/sdd/2026-10-13-companion-memory-m31/progress.md`. Publish only20owned
public paths; preserve unrelated `nx.json`, manifests and private evidence.
