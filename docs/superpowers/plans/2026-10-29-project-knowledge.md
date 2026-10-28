# Searchable project knowledge implementation

Scope: docs/superpowers/specs/2026-10-29-project-knowledge-design.md. User selected external folders with explicit read-only grants. Preserve unrelated edits, no commits/push/restarts or live account access in this task.

## Ownership

Root owns integration: Cargo workspace/dependencies/lockfile, project records/service/context/restore, control-plane versioning, profile state wiring, routes registration, chat admission/recovery, final verification and docs. One root-owned Cargo lease; no concurrent Cargo or source edits during native gates. Clean current checkout is shared with explicit disjoint file ownership to keep one managed 32 GiB cache.

Portable worker/store implementer owns new anima-document-store-sqlite crate, anima-document-content parsing extensions, document-content adapter indexing envelope, and attachments/documents worker/protocol extraction changes. Coordinate public APIs before use. No Cargo until root grants lease.

Host implementer owns new projects/knowledge module tree and routes/project_knowledge.rs: held read-only grants, bounded traversal/index jobs, status/search/citation, host tests. Root wires existing project and state files.

UI implementer owns SDK projects methods/types/tests/exports, web daemon API facade, project Sources knowledge components/hooks/tests and citation UI. No changes to unrelated composer or Kanban.

## Tasks

- [x] Add meaningful tests first for persistent ranked passage retrieval, revision replacement/deletion, malformed queries, cross-corpus isolation, and late-document extraction. Observe relevant red failures under owned sequential validation before green implementation where practical.
- [x] Implement reusable transactional FTS5 document store and expanded bounded indexing extraction under existing physical worker custody. Preserve attachment limits and behavior.
- [x] Implement durable folder grants and bounded indexing/search/citations with source identity/hash revalidation and access fencing. Test unsafe paths/links/hardlinks, stale work, deletion/re-add, and owner/member authorization.
- [x] Add resource variant and persistence/schema validation; keep source grants in acknowledged project persistence. Integrate query retrieval into existing project budget, versioned evidence manifest, rechecks, compaction, and recovery fingerprints.
- [x] Implement SDK and simple Sources UI with explicit read-only grant, automatic indexing/status, search results/citation view, removal, and stale-result clearing.
- [x] Review spec compliance then code quality/authority/custody boundaries with independent agents, fix actionable findings.
- [x] Run relevant native focused tests, full bun run rust:test and bun run rust:lint with the managed validation cache. Run SDK tests/build/typecheck and web focused tests/build/typecheck/lint; exercise desktop/mobile Sources flow. Do not silently report prior broad web failures as new-feature green.
- [x] Record current-tree evidence, remove verified task-owned scratch artifacts, and deliver concise outcome and remaining practical limits. Leave changes uncommitted for user review.

## Verification and review log

- Initial tree clean at c9ce777b11fb601af9a548dd372aa50660009c34; branch g9000/project-knowledge. Later concurrent connection-page edits are outside this feature and preserved.
- Portable store test-first compile failed on missing new APIs as expected. Document-content test-first compile failed on missing knowledge extraction APIs; worker tests likewise failed on missing knowledge modes. These were missing-feature compile failures, not behavioral red acceptance.
- Portable document-store suite: 11 passed, including persistent reopen, isolation, actual SQLite transaction failure rollback, quotas, reconciliation, and UTF-8 excerpts. All document-content suites passed; DOCX 14, PDF 11, XLSX 23, including late passages and honest 1 MiB extraction coverage. Other generation/editing suites passed too.
- First daemon feature compile exposed one missing KnowledgeFolder record-shape arm; corrected. First executable feature run: 49 passed, 2 failed, 2 ignored. Both failures were new root fixtures using hidden default .tmp source folders, which the reader correctly excluded. Fixtures now use explicit ordinary folder prefixes; rerun pending within full native gate.
- Spec review identified and verified corrections for multiline queries, per-source offline isolation, final pre-admission hash checking, bounded queued refresh, and stable citation display during indexing. No remaining P1/P2 spec findings.
- Code quality review identified explicit Retry reusing failed extraction and stale UI status overwriting newer reads. Host now distinguishes forced refresh from automatic reuse, including queue identity; regression injects fail-once extraction. UI tests cover delayed responses, generation fences, controller cleanup, and pre-refresh response order; final review pending.
- Current required Rust formatting/correctness command passed (exit 0) using the wrapped validation cache. Existing workspace warnings remain; this is not a zero-warning claim. Full bun run rust:test currently running.
- Root SDK no-service suite: 289 passed, 4 skipped; SDK typecheck/lint passed (existing warnings). An earlier agent full SDK attempt unexpectedly started service-build hooks, was cancelled, and no owned descendants remained. Real finite SDK service acceptance still pending root's native lease.
- Root focused web run: 142 passed across 7 files before the last hook-order corrections; agents' later focused regressions/typecheck passed. Final current-tree focused run still required.
- Browser fixture acceptance: 8 passed at 1280px and 390px, covering explicit read-only grant, coverage, search, citation, keyboard close, removal, revocation and overflow. Root visually inspected both saved screenshots. Initial restricted Edge launch failed before UI assertions; identical normal-permission run passed. This is synthetic UI acceptance, not a live daemon/provider claim.
- Managed Cargo cache was 23.4 GiB before final native gates; ceiling stays 32 GiB. Existing development daemon and unrelated rust-analyzer processes were preserved.

### Follow-up validation corrections

- The first full Rust run stopped on a supervisor test's temporary port collision. That exact unchanged case passed on retry. The next complete run cleared supervisor/core suites, then daemon unit tests reported 3,684 passed / 3 failed / 39 ignored.
- One daemon failure was the new context regression: a valid 936-byte passage had its match at offset 901, and context admission blindly kept the first 600 characters. Admission now uses a query-centered byte-bounded excerpt. New Unicode offset and Greek contextual-lowercasing cases observed behavioral RED before correction; the complete store suite now has 14 passing tests.
- The API inventory contract correctly caught five new operations missing from its fixed count and README table. Count, explicit additions, historical-operation filters and the embedded README inventory are now updated to 360. A final full native rerun remains required.
- The third daemon failure was an existing document source-read fixture reporting unavailable under shared read contention. That exact unchanged workspace-boundary case passed in isolation (1/1).
- Current web focused suite passed 143 tests, and the final test-only lint restructuring passed 18 hook tests. Web build/typecheck/lint passed. Browser flow passed again (8/8).
- Added a real SDK project-knowledge process acceptance case: shared read-only grant, late passage, exact citation, real daemon restart, model context, and same-length/mtime-preserved stale-file rejection. Its first run and the existing personality persistence case failed before daemon startup because the existing PowerShell private-home fixture timed out; ordinary daemon integration passed 3/3. A safe native private-home fixture using the existing win-custody APIs is being prepared, preserving fresh-directory ACL and no-reparse cleanup checks.
- Cache measured 25.7 GiB during the native rerun, below the unchanged 32 GiB ceiling.

### Final focused acceptance

- Query-centered context admission and updated API documentation inventory passed together: 54 passed, 3 ignored. The store's complete 14-test suite also passed after the Unicode corrections.
- Final wrapped Rust formatting/correctness command passed, including all example targets. Existing general workspace warnings remain.
- Native private-home fixture passed 2 custody cases: fresh protected creation/collision preservation and direct-child enforcement. The shared SDK fixture no longer uses PowerShell; it retains exact owner/System permissions, held no-follow directory checks and bounded cleanup inspection.
- Explicit Windows worker acceptance passed 2/2: late DOCX passages beyond the preview cutoff and the existing process/framing/stop/timeout/drop/resource-retirement matrix. Both process fixtures were built through the managed validation target.
- Real SDK daemon integration passed 3/3, existing personality process restart passed 1/1, and the new project-knowledge process case passed 1/1 in focused runs. The new case verified both real members, late search, literal citation bytes, persisted grant/cache through process restart, citation-bearing model context using a local stub, read-only source behavior, and same-length/mtime-preserved stale-file rejection.
- The real process test first corrected two test-request assumptions: Windows canonical path prefixes are compared by physical identity, and project chat creation supplies its current member generation. Neither required a product workaround.
- Final no-service SDK run passed 289 tests with the five process cases skipped, and typecheck passed. Those five process cases have now run rather than being claimed from the skipped suite.
- The final required full Rust rerun uses RUST_TEST_THREADS=4 with bun run rust:test, matching the shared four-read test capacity after the unrelated contention failure. No production limits were raised or weakened. Source remains frozen and the supervisor portion has passed; final full result is pending.
- Cargo cache measured 30.0 GiB before the final run, still under 32 GiB. No existing application or editor process was stopped.

## Final current-tree result

- `set RUST_TEST_THREADS=4&& bun run rust:test`: exit 0; 5,417 passed / 0 failed / 52 ignored across supervisor, portable core, daemon and integration groups. Expected ignored suites are not claimed as live/provider acceptance. The two relevant physical worker cases and two new private-home custody cases were run explicitly and passed separately.
- Wrapped `bun run rust:lint` with the validation cache: exit 0. Formatting and correctness passed; existing general workspace warnings remain.
- `set ANIMA_SKIP_SERVICE_TESTS=0&& bun run --filter @animaos/sdk test --no-file-parallelism`: exit 0; all 52 files / 294 tests passed, including all five real process cases. The final project-knowledge case includes the current member witness in chat creation and passed without bypassing access checks.
- SDK build/typecheck/lint and web build/typecheck/lint: exit 0. Relevant web tests: 143 passed; final hook-only test restructuring: 18 passed. Web E2E typecheck/lint: exit 0.
- Project browser fixture suite: 8 passed at desktop and phone widths, with recorded visual inspection. These use synthetic API responses; the independent real daemon process case supplies startup/restart/model-context evidence using only temporary source files and a local model stub.
- Independent spec and final code-quality/custody reviews have no remaining confirmed P1/P2 findings.
- Final managed Cargo cache: 30.5 GiB, below the unchanged 32 GiB cap. No live accounts were accessed; no production deployment, commit or push was performed. Concurrent connections-page work is preserved.
- No `scratchboard/` version exists in this checkout; this feature's dated execution checklist is the task tracker. Task-owned test logs/scripts/screenshots are removed after recording these results.
