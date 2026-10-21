# Rust runtime capability expansion

The owner approved all seven capabilities on 2026-10-21, starting with background memory maintenance. Stay on main. Do not stage, commit, push, reset owner stores, or modify unrelated concurrent web/design work.

## Shared design

Engine contracts, validation, matching and transaction planning live under packages/core-rust. Filesystem, HTTP, process, credentials and lifecycle ownership stay in Rust host/adapters. Existing Strict/Custom review remains authoritative; Free may perform only enabled and permitted operations. Undo is exact, source-bound and refuses intervening owner changes. An action receipt is evidence about that action, not proof of a whole task or external delivery.

Each capability must be called by a real daemon route/tool/lifecycle hook and have executable acceptance coverage. Standalone unused modules, flags with no driver effect, and unconditional success responses do not fulfill this design. Configuration is operator owned and defaults fail closed. No normal owner profile is used as a fixture.

## 1. Background memory maintenance

Reuse MemoryManager::consolidate_facts instead of a second memory engine. Independently opt-in policies control a bounded cadence and daily attempts. A due reservation is saved before model dispatch; one cohort of 2-8 eligible same-scope non-core Facts is selected deterministically, and one physical review slot bounds concurrency. The reviewer has no tools and returns only a bounded sourced summary/reason or no change. Host-selected source IDs, semantic fingerprints and scopes cannot be widened by model output.

Current records lack authenticated provenance. Add host-owned provenance tied to memory ID plus semantic hash. Owner API saves/explicit owner source approval can establish eligibility. Model tags, imported data, and legacy memories do not establish owner authorship. Manual owner review can inspect legacy candidates, without making them automatically trusted. Protect all core-tagged entries and exclude arbitrary task journals, hidden text and flagged content.

Strict/Custom produce pending decisions; Free applies only while its enabled policy and provenance are current. Recheck mode, agent, decision revision, source hashes and core protection immediately before commit. Prepare a per-cohort write-ahead operation in the control plane before memory persistence. Journal exact before/after semantic images and a preallocated summary ID. Persist memory under MemoryMutation, then finalize the journal. Failed/uncertain persistence remains recoverable and never reports success without evidence.

Undo journals the inverse before applying it, restores only source fields changed by consolidation, and removes only the exact untouched summary when no new graph citations depend on it. Recall counters/timestamps and unrelated memories survive. Restart compares the journaled cohort: all-before means interrupted without effect; all-after finalizes; divergence requires review. Never restore an old whole-store snapshot over newer writes. Reconcile after both stores load; close admission before shutdown drain. Owner API supplies policy, source review, review/decision/undo and report surfaces.

## 2. Authenticated browser workflows

Extend the existing browser scope/grant/approval/receipt/lifetime lane. Add operator-owned dedicated persistent browser profiles outside workspace authority, exact authenticated-origin approval, status and manual-login lifecycle. A profile selection must change real browser state and cookie transport, not only metadata. Current guarded HTTP reissue strips cookies and Set-Cookie, so authenticated modes need origin-bound credential propagation with cross-origin refusal and redacted observations. Keep personal-browser attachment explicit. Verify an isolated login-required loopback application, retained login across a fresh run, revocation, logout/profile isolation, and ordinary browser regressions.

## 3. Deferred tool discovery

Separate model-visible schemas from canonical execution authority. Keep essential direct tools and expose bounded discovery/describe/dispatch controls for large authorized catalogs. Search only reviewed native/MCP descriptors; never advertise denied tools. Every actual dispatch re-enters normal tool grant, approval, MCP generation/digest/revocation and durable-receipt checks. Version/cache catalog snapshots and refuse changes between discovery and use. Measure provider request size and calls for small and large catalogs using real request capture; no universal cost-saving claim.

## 4. Isolated/remote execution

Introduce operator-approved backend profiles for local, Docker and SSH while reusing Rust process ownership, cancellation, deadlines and existing shell approvals. Bind an execution to its selected profile; do not silently fall back to local. Pin executable/host identity and only forward explicitly selected files/environment credentials. Implement actual command transport and output/cleanup semantics. Test argument boundaries, revocation, cancellation and isolation with controlled peers; separately probe configured Docker and SSH prerequisites before live claims. Arbitrary external effects are not advertised as undoable; existing file checkpoints and reviewed recovery remain intact.

## 5. Doctor and bounded repairs

Provide a common redacted diagnostics report through the owner API and native tool: persistence, memory/model availability, MCP/connectors, browser, execution profiles and device status. Report configured/unknown/live-probed separately. Known repairs use named handlers and fresh current-state checks, not arbitrary generated shell commands. Derived-cache rescans/reindexing may be repaired without changing canonical content; canonical setting changes require review and a before-image undo. Preserve existing supervisor restart/update controls. The first canonical repair is pausing one repeatedly failing automation, using its exact current revision, with a durable before-image and an Undo that refuses intervening edits. Derived-index rebuild is the only cache repair in v1.

## 6. Standing conversational intentions

Persist owner-directed keyword/event intentions with agent/session/sender scope, expiry, cooldown, fire budget, revision and explicit cancellation. Match deterministically on eligible owner turns before the primary model reply; untrusted participants cannot create or widen an intention. Use the same current-authority and persistence boundaries as existing event automations, without creating clock jobs for conversational conditions. Test restart, duplicates, cancellation, scope, expiry, budget and late-cancel/revocation races. Provide real model tools plus owner inspection/cancel controls. A match appends the saved owner instruction to that same admitted owner run, retaining ordinary tool approvals. Consume its fire budget once when the matching run admission commits; retries of that run do not consume again, and a failed run does not silently re-arm it. Order simultaneous matches by creation time and ID, with at most three instructions per turn. Cancellation before admission prevents matching; a committed run retains its normal Stop control.

## 7. Paired remote devices

Add explicit device pairing and separately approved capability surfaces, credential references, revocation and bounded invocations. Pairing alone grants no execution. Keep request scope, identity, deadlines and replay protection in Rust. Implement a usable Rust node host/transport exposing a minimal declared capability set and actual daemon dispatch, not only a manifest. Verify two isolated local peers, widened capability reapproval, token/identity mismatch, replay, revocation and cancellation. Non-loopback deployment requires an approved encrypted transport; no exposure to a network by default. Initial node capabilities are system information, listing an owner-selected directory, and bounded reading of an ordinary file inside that root. Pairing uses an expiring one-use ticket and owner review of the endpoint identity and capability digest; store the issued credential in the namespaced OS credential store. Invocation uses the existing reviewed HTTPS MCP path, with durable request IDs and capability digest/generation fencing across reconnects.

## Verification

Use TDD for meaningful behavior and independent spec/code review. Run full rust-daemon:test --skipNxCache (including core), Rust format target, relevant host/package checks, and controlled integrated acceptance. Use existing Windows Nx environment and isolated Cargo output. Keep static, simulated, controlled live-model and real external service evidence separate. Record exact unavailable prerequisites instead of treating them as passed. Update owner docs and API inventory only after actual integration.
