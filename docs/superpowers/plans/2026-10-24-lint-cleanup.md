# Workspace lint repair plan

**Goal:** Make the workspace lint command pass without losing runtime behavior or test coverage.

**Approach:** Reproduce each workspace's diagnostics, repair independent areas in parallel, and distinguish actual correctness problems from analyzer limitations before choosing a fix. Preserve the Bun workspace migration and public SDK contracts. Do not blanket-disable lint rules or defer updates merely to hide diagnostics.

## Tasks

- [x] Capture machine-readable diagnostics and verify a clean starting tree.
- [x] Repair SDK import cycles and test assertions. Owner: SDK worker; files under `packages/sdk`.
- [x] Repair TUI hook, session, and test diagnostics. Owner: TUI worker; files under `packages/tui`.
- [x] Repair production web hooks. Owner: web hook worker; files under `apps/web/src/hooks`, excluding existing tests.
- [x] Repair web components, tests, libraries, landing scripts, and browser fixtures. Owner: primary agent.
- [x] Repair playground state handling and add regression coverage for transcript updates and optimistic messages.
- [x] Review combined changes and run full lint, relevant JavaScript tests, typecheck, and affected builds.
- [x] Record results and remove task-owned scratch diagnostics and scripts.

## Verification

Use existing focused Bun workspace scripts during iteration. Finish with `bun run lint`, `bun run typecheck`, and the affected package tests/builds. For fixes that change lifecycle or state behavior, add or strengthen regression tests and confirm they fail before the fix. Avoid tests that duplicate static edits.

## Results

- `bun run lint` passes across the workspace, including Rust formatting. Nonblocking warnings remain.
- `bun run typecheck` passes.
- SDK, TUI, CLI, web, landing, and playground builds pass.
- JavaScript suites pass: web 2,284; SDK 267; TUI 212; CLI 83; workspace-dev 36; landing 10; playground 2. Total: 2,894 passed, one skipped.
- The final web run used `bun run --filter @animaos/web test --maxWorkers 4`: 192 files passed. Bounded concurrency avoids contention with other local work.
- Regression coverage checks committed callback scopes, stale asynchronous responses, immutable external-store snapshots, draft preservation, and playground transcript updates. Meaningful new cases were confirmed red before their fixes.
- Independent reviews found and resolved an unnecessary chat render on each stream update and an assertion that did not distinguish rejected promises from fulfilled error-shaped values.
- Narrow, documented exceptions cover reproduced async analyzer false positives, intentional control-character sanitizers, external WebGL state, and a browser script's required module boundary. No lint rule was disabled globally; Vitest's supported optional assertion message is configured explicitly.
- An optional browser console run stops on existing missing fixture routes: `POST /api/agents/companion/main-chat` and `GET /api/agents/companion/workflows`. Browser acceptance remains unverified; this fixture repair is outside the lint cleanup.
- Concurrent local MCP bearer-support work, including Rust changes and shared MCP panel edits, is preserved.
