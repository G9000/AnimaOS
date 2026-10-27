# Connected accounts implementation plan

Implements ../specs/2026-10-27-connected-accounts.md. Existing uncommitted browser/output work is a preserved, verified baseline.

1. Review the design and exact JSON contracts before implementation. Resolve provider API documentation through Context7.
2. Connected-search owner: normalized bounded CP state, native source adapters, grants/authority, refresh loop and transactional admission hook, routes and connected_search tool; synthetic regression tests. Own connected_search modules, provider extensions, CP/state/app/tool wiring.
3. Account-workflows owner: rule model/service/routes, validation/bindings, durable idempotent admission and queued dispatch authority, tests. Own account_workflows modules and jobs changes. Coordinate CP wiring with search owner, without editing shared state files concurrently.
4. Root: typed SDK clients/tests, Connections search/source/rules UI and focused tests, route inventory and companion docs. Freeze JSON contracts early. UI starts from source opt-in and makes retained scope/polling semantics visible.
5. Independently review combined implementation, fix material issues, then run focused Rust regressions before expensive full verification. No concurrent Cargo builds in one cache.
6. Run required Rust/SDK/web/docs gates; synthetic UI/provider acceptance; record results and remove exact task-owned scratch files. Report remaining limits accurately, without claiming live-provider acceptance.

## Progress

- [x] Design and API contracts reviewed.
- [x] Native search, opt-in/index persistence, cited reads, sync and source lifecycle implemented.
- [x] Account rules, pinned workflow admission, replay protection and dispatch authority implemented.
- [x] SDK clients, Connections interface, rule/history review and documentation implemented.
- [x] Independent backend and UI review findings resolved.
- [x] Focused source/workflow/provider and owner-route regressions passed; synthetic browser acceptance passed at desktop/mobile widths.
- [x] Rust formatting/lint, SDK build/typecheck/lint, web build/typecheck/lint and 56 focused web tests passed.
- [x] Full Rust suite, including packaged app, core and daemon integration tests.
- [x] Final full SDK suite: 284 passed across 48 files, zero skipped; real daemon fixtures ran.
- [x] Final validation record and exact task-owned scratch cleanup.

Final evidence: `test-output/connected-accounts-validation.json`. Full Rust: 5,150 passed, zero failed, 51 intentionally ignored across 104 summaries. Rust format/Clippy passed. Web: 56 focused tests plus build/typecheck/lint passed. SDK build/typecheck/lint passed. Documentation build, 331-operation OpenAPI inventory and diff whitespace checks passed. Synthetic browser acceptance passed at desktop/mobile widths without overflow or page errors.

Limits remain explicit: bounded retained content and observed polling, selected Drive files only, no attachment indexing, no arbitrary MCP background reads. No live account data read, no personal browser profile used and no development service restarted.
