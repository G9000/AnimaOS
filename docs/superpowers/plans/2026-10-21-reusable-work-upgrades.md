# Reusable work upgrades implementation plan

> Use the design, test-first, Nx task and scoped review workflows. The owner authorized all three capabilities. Work on main and leave changes unstaged. Each feature has separate ownership; the parent integrates shared files.

**Goal:** reusable workflow templates, multi-file skill packages and durable large-result retrieval, usable from chat.

**Architecture:** extend existing Rust workflow/skill/tool services with bounded native data and storage; expose them through shared owner APIs and SDK-backed chat/Work controls. Preserve existing execution and checkpoint authority.

## Task 1 — Workflow templates

- [x] Inspect local OpenClaw/Lobster argument/file handling and current native workflow implementation; report exact integration contracts.
- [x] Write pure template materialization, missing/unknown/default/escaping/bounds and immutable fresh-run/replay tests first; observe red.
- [x] Implement pure core types plus daemon template service/routes/tools, SDK methods and shared template UI components. Add source revision metadata without changing old workflow meanings.
- [x] Parent wires state migration, registration, route/OpenAPI inventory, SDK exports, permissions and Work/chat mounting.
- [x] Run focused tests and perform spec then quality review.

## Task 2 — Skill packages

- [x] Inspect Hermes directory structure and current skill service/scanner/draft review.
- [x] Add failing package hash/change detection, path scope, proposal/review/load and old single-file compatibility tests.
- [x] Implement supporting-file storage and complete-package review in existing skill APIs/tools, SDK, skill editor/draft review and chat loading paths. No automatic script execution.
- [x] Parent wires shared tool descriptor additions and any common state/contracts needed; keep package-specific code owned by the feature worker.
- [x] Run focused tests and perform spec then quality review.

## Task 3 — Large tool results

- [x] Trace native result capture, receipts and model/UI projection; inspect Hermes spillover documentation and reuse applicable source patterns.
- [x] Add failing immutable artifact/pagination/integrity/agent isolation/quota/oversized-result projection tests.
- [x] Implement private durable artifact storage, owner read APIs and model read tool, compact result projection and chat tool-card viewer/download. Preserve action status and explicit output incompleteness.
- [x] Parent wires host context, route/OpenAPI/tool registration, SDK exports and opt-in permission toggle.
- [x] Run focused tests and perform spec then quality review.

## Task 4 — Combined validation and browser acceptance

- [x] Run current-tree Rust daemon test via Nx with isolated Windows validation outputs, plus focused SDK/web tests, typechecks/builds and relevant docs build.
- [x] Load updated development runtime safely and use CUA browser UI for all three real chat flows, including reload, fresh template runs, package review and artifact paging/download.
- [x] Fix evidenced defects with regression tests and repeat only affected checks. Finish combined review, record screenshots/results and update this checklist.
- [x] Confirm main, empty index and preserved unrelated edits. Do not stage/commit/push.

## Verification evidence

- Required complete `bun x nx run rust-daemon:test --skipNxCache` passed in the final tree, including the core dependency target. Daemon tests: 2923 passed, 35 ignored. Log: target/reusable-final-rust.log.
- Final web: 260 passed, 1 existing skip in 12 suites; SDK: 11 passed. Web/SDK typechecks, web production build and 30-page guide build passed. Logs: target/reusable-final-web.log, reusable-sdk.log, reusable-web-typecheck.log, reusable-web-build.log, reusable-docs-build.log.
- Actual CUA acceptance covered chat save/run, owner approve/answer, distinct inputs, pinned revisions, exact replay after edit, reviewed supporting-file save/load, model artifact reads, owner pagination/download, permission revoke/reload/restore, and the Work template picker. Evidence: target/reusable-work-browser/browser-results.md and screenshots.
- Browser found a numeric argument conversion defect; valid model whole-number revisions now normalize safely before strict deserialization. Integration checks also caught and fixed helper/search structured output compatibility and learning proposal replay.
- At the original unstaged handoff, main remained at a220f1facfc9da1808e09961c2896ae35c219c6c with an empty index and no feature commits or pushes. Validation caches remained on disk and ignored.

## Authorized publication integration

The owner subsequently authorized committing and pushing to main, with explicit care for newer GitHub commits. Fetched and fast-forwarded to 203d213eb6d40dbf63d1fa75ea84215a6bec4ad5, preserving both upstream commits (design refresh and Astro landing page). Their 62 changed paths did not overlap the feature work; all 79 local files matched their recorded SHA-256 hashes after integration.

The new landing app was missing from the root TypeScript project references. `nx sync:check` identified only that reference, and `nx sync` added it. The subsequent sync check passed. Frozen-lockfile dependency installation left the upstream lockfile unchanged.

Repeated combined-tree verification passed: complete Rust daemon/core Nx test target (daemon: 2923 passed, 35 ignored), focused web tests (260 passed, 1 skipped), SDK tests (11 passed), web/SDK/UI typechecks, SDK/web production builds, docs build, and the fetched landing app test/build targets. Incoming landing/design files and bun.lock remain identical to origin/main.
