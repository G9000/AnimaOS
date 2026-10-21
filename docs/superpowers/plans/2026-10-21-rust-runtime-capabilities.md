# Rust runtime capabilities implementation plan

> For agentic workers: use the planning, TDD, bounded delegation and independent review workflows. The owner authorized implementation in this chat on main with no staging or commits.

**Goal:** Deliver all seven approved capabilities with native Rust authority, current approvals, durable undo, and evidence-backed verification, beginning with background memory maintenance.

**Architecture:** Extend the existing host-agnostic memory/core contracts and rust-daemon services. Reuse the shared control-plane transaction and memory write/embedding lock order; keep operator adapters explicit and fail closed. Each capability has an owner/API/tool path and executable tests.

**Tech stack:** Existing Rust workspace, Tokio/Axum/Serde, native memory and model adapters, daemon API/MCP, existing browser and process adapters.

Spec: docs/superpowers/specs/2026-10-21-rust-runtime-capabilities-design.md

## 1. Background memory maintenance (first)

- [x] Review the spec against current memory/persistence/lifecycle seams.
- [x] Add failing core tests for semantic cohort prepare/apply/undo, exact-source changes, unrelated writes, protected sources and new graph citations.
- [x] Implement reusable per-cohort primitives in anima-memory; add host-controlled provenance/image conversion where needed.
- [x] Add typed maintenance policy/review/journal state, bounded validation, version-26 backup/migration and restart reconciliation.
- [x] Add failing host tests for durable reservation, tool-free bounded review, Strict/Custom pending, Free current-policy activation, disable/cancel, each cross-store failure boundary and persisted undo.
- [x] Implement the lifecycle service and owner policy/source-review/decision/undo/report routes; wire production startup and shutdown.
- [x] Verify deterministic integration and an isolated real-model merge/fresh recall/restart/undo scenario; keep normal owner data unchanged.

Files: packages/core-rust/crates/anima-memory/src/memory_manager/{edit.rs,reversible.rs,reversible_tests.rs}; hosts/rust-daemon/src/memory_maintenance/*; memory_store.rs; routes/memory_maintenance.rs; state.rs; control_plane_store.rs; app.rs and persistence hooks.

Memory checkpoint (2026-10-22): 123 core memory-manager tests and the first 15 daemon maintenance tests passed; later full runs executed all new maintenance regressions successfully. Independent core and host spec/code reviews approved after fixes. Isolated Anthropic claude-sonnet-4-6 background consolidation reduced active Fact text from 378 to 174 bytes; a fresh empty session after a real daemon restart recalled 7421/Rust/Nx with zero tools. Undo restored both exact original Facts and preserved the unrelated native task_result. Both owned acceptance PIDs retired. Proof: .tmp/skill-learning-memory-live-2026-10-22/maintenance-proof.json. Full-suite contract expectations updated; final integrated host tests passed after browser integration.

## 2. Authenticated browser workflows

- [x] Refine adapter plan from current transport/profile/cookie constraints; add failing controlled authenticated-site tests.
- [x] Implement operator profile custody, actual persistent context/cookie routing, scoped owner controls and existing approval/receipt integration.
- [x] Verify retained login in a fresh run, origin/profile isolation, revocation and process cleanup.

Files: existing Rust browser modules, anima-service-adapters browser contracts, browser transport/worker adapters, native browser tools/routes, controlled acceptance fixtures.

Browser checkpoint (2026-10-22): pinned Chromium worker suite 60 passed, including actual same-origin profile isolation and form logout persisting into a fresh context. Native OS-keyring/owned-worker acceptance passed for fresh service/factory restoration, exclusive lease, stale approval and revocation. This does not claim a separate HTTP daemon process restart. Independent browser spec review approved. Evidence: .tmp/browser-auth-tdd/profile-isolation-logout-result.json and native-profile-acceptance-result.json.

## 3. Deferred tool discovery

- [x] Add failing request-capture tests for small/large approved native and MCP catalogs and revoked dispatch.
- [x] Implement model schema exposure independent of canonical grants, bounded search/describe/dispatch and per-call authority checks.
- [x] Measure request size, discovery overhead and native receipts, then document opt-in behavior.

Files: anima-core runtime/model contract, rust-daemon tools/catalog*.rs and agent_runs/tool_discovery_tests.rs, agent_runs.rs, tools registry, MCP dispatch integration.

Discovery checkpoint (2026-10-22): 26 focused tests passed, including real helper/peer run pipelines, native and MCP revocation, exact memory-source identity and current per-request visibility. Controlled large reviewed MCP tool-definition payloads measured 61,977 bytes full, 1,298 bytes initial compact request, and 16,792 bytes after exact hydration, with one discovery call and one actual MCP transport call carrying durable receipts. This measures schema bytes, not universal token or cost savings. Independent spec review approved. Evidence: .tmp/tool-discovery-review-green.log.

## 4. Isolated and remote execution

- [x] Add failing backend selection, argument isolation, cancellation and no-local-fallback tests.
- [x] Implement real approved local/Docker/SSH transport profiles through existing Rust process/tool approval lanes.
- [x] Verify controlled peers, configured Docker availability and explicit SSH prerequisites; retain accurate cleanup uncertainty.

Files: reusable execution contracts; rust-daemon execution_backends/*, tools/execution.rs and fixed worker entry points; protected operator configuration.

Execution checkpoint (2026-10-22): actual installed digest-pinned Docker image passed isolation, timeout and cancellation acceptance; each case confirmed the exact owned container absent. Actual Rust worker deadline/EOF tests passed in the host suite. Independent execution spec review approved. External SSH remains unverified because no operator endpoint/key/host-pin configuration was supplied. Evidence: .tmp/skill-learning-2026-10-21/final-live-docker-acceptance.log.

## 5. Doctor and bounded repairs

- [x] Add failing diagnostic truthfulness/redaction and stale repair/revocation tests.
- [x] Implement owner report and native doctor tool, named bounded repair handlers, current-state approval and canonical-setting undo where applicable.
- [x] Verify actual local diagnostic results without treating denied/unprobed services as down or healthy.

Files: rust-daemon doctor/*, routes/doctor.rs, tools/doctor.rs; reuse health/readiness and subsystem status APIs.

Doctor checkpoint (2026-10-22): all eight host diagnostics/approval/repair/restart/Undo regressions passed in the combined host run. Two bounded core memory-copy regressions passed, including zero-length metadata amplification. Independent spec and code quality reviews approved. Diagnostics distinguish local configuration from unprobed remote availability; derived repair changes only the local RAM index. Final current-tree host verification passed.

## 6. Standing conversational intentions

- [x] Add failing matcher/scope/expiry/budget/duplicate/cancel/restart tests.
- [x] Implement persistent owner intentions, tools/routes and ordinary-owner-run prompt integration with no model call in matching.
- [x] Verify a fresh session trigger, non-trigger, explicit cancellation and restart-safe limits.

Files: anima-core standing_intents.rs; rust-daemon standing_intents/*; agent_runs admission/context; native tools/routes; snapshot validation.

## 7. Paired remote devices

- [x] Add failing pair/capability/replay/revocation/cancel tests with two real isolated loopback peers.
- [x] Implement Rust node host/transport, protected credential references, separately approved capability surfaces and actual daemon dispatch.
- [x] Verify invocation and cleanup, then document encrypted remote deployment prerequisites.

Files: shared typed device protocol under packages/core-rust; runnable node host under hosts; rust-daemon paired_devices/*, routes/tools and operator configuration.

Paired-node checkpoint (2026-10-22): real TLS dispatch, ordinary AllowOnce approval and durable receipts passed for system/list/read capabilities; replay remained refused across literal node-process restart. Current full-host tests also passed cancellation with an unknown remote outcome, offline revocation without false remote confirmation, and old-bearer refusal after root/generation changes even with fresh invocation metadata. Pairing credentials bind to the exact manifest; replay history survives invalidation and fresh pairing. Independent spec/code reviews approved.

## 8. Integration, review and final verification

- [x] Obtain independent spec compliance and code quality review for each capability, fixing findings before closing its task.
- [x] Update owner docs and exact OpenAPI route inventory from actual handlers.
- [x] Run node .tmp/skill-learning-2026-10-21/nx.mjs run rust-daemon:test --skipNxCache and rust-daemon:lint with isolated Cargo/OS temp/4 test threads; run any additional relevant host/package targets.
- [x] Re-run controlled acceptance affected by integration changes; record real-provider/service limits plainly.
- [x] Confirm main, inspect both cached and unstaged diffs, preserve unrelated web/design changes, and leave all work uncommitted.

Final integration checkpoint (2026-10-22): the required Nx rust-daemon:test (including core-rust dependency), rust-daemon:lint, rust-daemon:build, anima:test and anima:build targets passed in the final tree. Daemon results: 2,901 unit tests plus 150 API tests passed; the packaged host passed 175 tests. Declared provider/platform ignores remain explicit in the logs. The 60-test Chromium worker suite, 21 web component tests, SDK build and SDK/UI/web typechecks passed. Real Windows keyring/browser restoration and installed digest-pinned Docker isolation/timeout/cancellation acceptances were repeated after integration and passed. External SSH remains unverified without operator endpoint/key/host pins.

The packaged CLI check exposed a Windows main-thread stack overflow and an unsafe shared executable in the existing update fixture. Heap-pinning the composed lifecycle at the shared serve boundary restored actual startup; each CLI test now owns a private executable and a real rollback backup. Independent review found no actionable issues. All 22 CLI client tests and the remaining packaged lifecycle tests passed. Red/green startup evidence: .tmp/skill-learning-2026-10-21/startup-stack-proof.json.

Final proof: .tmp/skill-learning-2026-10-21/final-validation-proof.json. All 103 recorded runtime/client source hashes matched the final verification epoch. Main remained at 8958f56905940defee1658c833929820f456ca19 with an empty index; unrelated web/design changes were preserved. No staging, commits or pushes were performed.
