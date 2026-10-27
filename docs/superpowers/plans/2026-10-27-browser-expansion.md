# Browser Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Let an owner authorize ordinary public websites, use forms and downloads, and take over a dedicated browser for sign-in before returning control.

**Architecture:** Extend the existing Rust browser service and maintained Playwright worker. Owner-created destinations freeze reviewed HTTPS origins and public address pins; every network request uses the existing guarded native transport. Preserve fixed-site mode, opaque DOM references, approvals, exclusive profile custody, physical Stop, and final publication checks.

**Tech Stack:** Rust daemon and reusable browser adapter, pinned Playwright worker, TypeScript SDK, React console.

### Task 1: Destination and execution contracts

**Files:** `packages/core-rust/crates/anima-service-adapters/src/browser.rs`, `hosts/rust-daemon/src/browser/{records,config,service,profiles}.rs`, new focused destination module, `hosts/rust-daemon/src/routes/browser.rs`, route registration and OpenAPI inventory.

- [x] Add failing validation tests: explicit HTTPS origins only; reject credentials/private or mixed DNS answers, extra ports, duplicate origins, unbounded paths, and stale versions. Existing fixed-site tests remain unchanged.
- [x] Add owner setup status and destination create/list/revoke operations. Creation resolves and freezes at most eight explicit origins without fetching website content; digests bind all pins and the installed worker witness. Existing scopes retain their immutable policy.
- [x] Add same-page takeover/resume with expected-version fencing. A busy action returns a conflict; refresh after its receipt settles, without replaying it. Owner control blocks model observations/actions, invalidates DOM references, and keeps the original deadlines and budgets.
- [x] Add focused service/route tests for authorization, generation/version conflicts, Stop, and revocation. Register exact contracts in README/OpenAPI inventory.

### Task 2: Worker functionality

**Files:** `packages/core-rust/browser-worker/worker.mjs`, focused worker helper/test modules and synthetic fixtures, browser adapter tests, daemon browser/tool tests.

- [x] First demonstrate failing tests for JavaScript resources, query strings, same-policy redirects, forms, typed select/key actions, takeover, and bounded document downloads.
- [x] Enable these features only in the new interactive policy. Intercept all requests, block service workers and browser-origin network escapes, verify TLS against the original hostname while connecting to the frozen public address, and cap per-resource/aggregate bytes separately from IPC.
- [x] Keep the deny proxy and owned socket/process teardown. Reject out-of-scope redirects, subframes/popups/external protocols, executable downloads, stale DOM refs, and model operations during owner control.
- [x] Demonstrate a real synthetic JavaScript form, download, profile sign-in/takeover/resume, blocked egress, and Stop. Record which native installed-runtime cases are actually available; do not imply live account acceptance.

### Task 3: SDK and owner controls

**Files:** `packages/sdk/src/browser.ts`, `browser.spec.ts`, `apps/web/src/components/sessions/BrowserTaskControls.tsx`, new focused destination/profile controls and tests.

- [x] Write failing SDK transport tests and UI tests for create/select destination, dedicated profile sign-in, explicit takeover/resume, Stop, missing setup, stale response suppression, and downloads.
- [x] Expose typed owner APIs without automatic retries or worker startup on reads. Pass profile selection and expected versions exactly.
- [x] Show simple website/origin controls, explicit grants, sign-in readiness and actual control state. Keep API keys, cookies, paths and implementation details out of the owner flow.
- [x] Run focused SDK/UI tests and refresh browser documentation, including maintained runtime setup and bounded support.

### Task 4: Review and verification

- [x] Independently review the completed change for scope, network confinement, ownership/races and maintainability; fix concrete findings before final source freeze.
- [x] Run `bun run --filter core-rust browser-worker-test`, full `bun run rust:test`, `bun run rust:lint`, SDK tests/build/typecheck/lint, focused relevant web tests and web build/typecheck/lint.
- [x] Record evidence and remaining limitations in `test-output`; check `git diff --check`, remove exact task-owned scratch files and mark plan steps accurately.

Implementation notes:

- The public anonymous and dedicated-account paths both use fresh owned full-Chromium contexts with the same private preferences. Neither attaches to a personal browser.
- Proxy CONNECT sockets always receive empty refusals and never forward. The narrowly matched navigation classification remains charged; owner-control origin hints are untrusted metadata with a two-second/three-use bound, rather than proof of a human gesture.
- Browser-routed cookies come from the actual included browser header, constrained to the owned primary profile. Chromium handles validated routed `Set-Cookie` so `credentials:omit` remains effective. Unsupported foreign fetch redirects fail closed.
- The navigation schema declares exactly one of a fixed `routeId` or approved HTTPS `url`; runtime validation remains authoritative. Internal UI request state includes the visit generation so returning to a chat cannot revive an old pending operation.

Final validation: Rust 5121 passed, 0 failed, 51 intentionally ignored across 104 summaries; browser worker 71 passed with zero skips; SDK 282 passed with zero skips; 106 focused web checks passed. Rust lint, SDK/web build/typecheck/lint, both documentation builds, native synthetic browser acceptance and diff checks passed. See test-output/browser-outputs-validation.json.
