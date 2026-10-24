# Runtime Security Hardening Implementation Plan

> For agentic workers: use subagent-driven development with independent file ownership and a final security review.

**Goal:** Close the six concrete gaps identified in the 2026-10-24 runtime review without claiming general Hermes/OpenClaw parity.

**Architecture:** Keep enforcement in the Rust daemon and reusable process boundaries in `anima-process`. Owner identity must be independent of browser Origin and agent execution. Network policy must govern the actual connection. Tool results are sanitized before model context and durable projections. Local host execution is an explicit operator capability; isolated execution must not silently fall back to the host. Approval evidence binds the code being launched. Unsupported process containment fails closed.

**Tech stack:** Rust, Axum, reqwest 0.12, process-wrap, Bun, TypeScript thin clients.

## Scope and constraints

- User approved implementation of all six findings and parallel delegation. Preserve all pre-existing edits; do not commit or publish as part of this task.
- Inspect local Hermes and OpenClaw sources as references; do not modify their configuration or stop their services.
- No external messages, provider requests, or ordinary-profile destructive acceptance tests.
- Regression tests must exercise real enforcement and demonstrate refusal before effects.

## Tasks

- [x] Inventory local reference repositories and record relevant source revisions.
- [x] Owner authentication: require signed owner session or admin credential for owner reads/writes; maintain Origin/Host/CSRF guards; integrate legitimate dev and installed client bootstrap. Test forged Origin, missing/mismatched/expired sessions, valid native/browser requests and approval decisions.
- [x] Outbound requests: replace hostname-only SSRF checks in `hosts/rust-daemon/src/tools/web.rs` with checked/pinned resolution, safe redirects and explicit proxy behavior. Test private DNS answers, address families, redirects, resolution failure and public destinations.
- [x] Execution boundary: enforce an immutable operator host-execution policy at dispatch across shell, background, Python, interactive/ACP, and configured local targets. Keep isolated targets available; never fall back to host execution. Document migration and actual limits.
- [x] Secret handling: sanitize tool success/error text and structured data before model context, action receipts and spill storage; protect credential-file reads/writes. Preserve ordinary numeric accounting and nonsecret outputs. Add nested/PEM/token regression tests.
- [x] Approval binding: capture resolved executable/script identity before waiting, revalidate before dispatch, reject changed bytes/path resolution and unbindable interpreter invocations. Cover shell, background and interactive launch surfaces; preserve exact instruction-write controls.
- [x] Process containment: prevent execution with process-group/tree-only custody when complete descendant containment is required; ensure Stop cannot report complete retirement on weaker mechanisms. Test unsupported-platform/fallback refusal before effects and supported custody retirement.
- [x] Review each independent result for scope and correctness; repair integration issues.
- [x] Run `bun run rust:test`, `bun run rust:lint`, affected TypeScript workspace test/build/typecheck scripts, and `bun run --filter workspace-dev test` for launcher changes.
- [x] Update owner-facing hardening documentation and record limitations supported by tests. Remove task-owned temporary artifacts after recording results.

## Acceptance

An uncredentialed local program cannot decide owner approvals. A public hostname cannot direct web fetches to private services. Untrusted execution cannot acquire host authority implicitly. Recognizable secrets do not enter model-visible tool results or durable result projections. Approval waits cannot authorize replaced executables/scripts. Unsupported whole-tree containment refuses launch rather than presenting a false Stop guarantee. Existing authorized browser/native flows pass their verification suites.

## Recorded validation — 2026-10-25

- Windows `bun run rust:test`: 4,897 passed, 47 ignored, zero failures across core, daemon and supervisor.
- `bun run rust:lint`: passed with warnings; no correctness errors.
- SDK 269, CLI 86, TUI 212, workspace-dev 44, playground 4, and browser authentication 32 tests passed (647 scoped client/launcher tests). Affected builds and typechecks passed.
- Full web suite: 2,134 passed, 168 failed, one skipped across 19 failed files. These remaining UI assertions are reported to the separate UI chat. Its hook-order regression was repaired and the suite rechecked.
- No live Docker/SSH acceptance or native Linux/macOS run. No ordinary-profile daemon restart or commit.
- Structured evidence: `test-output/runtime-security-validation.json`. Shared UI failure reports remain available to the owning chat.
