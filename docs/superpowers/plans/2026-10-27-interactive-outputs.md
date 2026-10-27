# Interactive Outputs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Let the agent create and refine dashboards and editable trackers that owners can use beside chat and reopen with saved edits.

**Architecture:** Extend the existing conversation result model, tools, revision checks and pinned result shelf. Use bounded structured widgets rendered by native UI; keep generated data separate from application code. Owner tracker edits save through the existing owner result API with exact agent/session/clear-generation/revision fencing.

**Tech Stack:** Rust daemon result store/tools, TypeScript SDK, React console.

### Task 1: Result contracts and persistence

**Files:** `hosts/rust-daemon/src/chat_results.rs` and focused validation helper/tests, `hosts/rust-daemon/src/tools/chat_results.rs`, `hosts/rust-daemon/src/agent_runs/chat_results.rs`, result route tests.

- [x] Add failing tests for `tracker` and `dashboard` payloads, rejecting duplicate IDs, kind-mismatched cells, non-finite values, unknown fields, nested dashboards and exceeded budgets.
- [x] Implement tracker columns `{key,label,kind: text|number|checkbox}` and rows `{id,cells}` with at most 12 columns and 100 rows. Enforce stable identifiers, bounded cell text, exact column coverage and existing session byte quotas.
- [x] Implement dashboard widgets `{id,title,payload}` with at most eight widgets. Payloads may be existing document/chart/comparison/calculator or tracker, never a nested dashboard.
- [x] Extend `show_result` and result discovery schema/instructions; preserve existing direct-owner, run/publication, idempotency and compare-and-swap checks. Add persistence/restart, owner edit, stale update, cleared/deleted conversation and model refinement tests.

### Task 2: SDK and live result UI

**Files:** `packages/sdk/src/results.ts`, `results.spec.ts`, `apps/web/src/components/results/ChatResultsPanel.tsx`, focused tracker/dashboard components and tests.

- [x] Add failing transport and UI tests for rendering, editable text/number/checkbox cells, save failure/conflict, reopening saved edits, dashboard composition and refinement.
- [x] Add precise discriminated SDK types matching Rust and reuse existing `save` transport. No new storage authority or arbitrary script execution.
- [x] Render dashboard widgets using existing result components. Give trackers local draft edits with explicit Save/Discard, validated values, exact current revision and generation, bounded request keys and cross-conversation async guards.
- [x] Provide open/close live side preview, pin/reopen and JSON source download. Keep selected preview alive across ordinary transcript refresh; fence pending edits when the agent publishes a newer revision or the owner switches conversations.

### Task 3: Review and verification

- [x] Independently review contracts, bounds, saved edits and UI races; resolve findings before final source freeze.
- [x] Run full `bun run rust:test` and `bun run rust:lint` together with browser work; run SDK full tests/build/typecheck/lint and focused result/session web tests plus web build/typecheck/lint.
- [x] Verify a real browser can edit/save/reopen a tracker and calculator/dashboard controls work beside chat using synthetic fixtures. Record evidence without claiming live provider acceptance.
- [x] Refresh result documentation, record current validation, run `git diff --check`, clean exact task scratch files and mark finished steps.

Implementation notes:

- Result controls are derived from current ordinary owner-chat eligibility. The normal scripted chat regression starts with only `calculate` in saved grants, receives `show_result` in the isolated run configuration, publishes a dashboard, and checks durable state without changing those saved grants.
- MCP catalog projection tests compare MCP/discovery bytes, while also checking that native controls remain byte-identical during hydration. Saved MCP-only configuration is not a valid denominator for the entire provider request containing native controls.
- HTTP file-download fixtures use isolated four-slot pools to avoid cross-test contention. Production still uses the shared four-slot retained-read pool; a deterministic test covers busy refusal, independent fixtures and recovery without relaxing response assertions.

Final validation: Rust 5121 passed, 0 failed, 51 intentionally ignored across 104 summaries; browser worker 71 passed with zero skips; SDK 282 passed with zero skips; 106 focused web checks passed. Rust lint, SDK/web build/typecheck/lint, both documentation builds, native synthetic browser acceptance and diff checks passed. See test-output/browser-outputs-validation.json.
