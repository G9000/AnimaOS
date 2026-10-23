# Active Personality Implementation Plan

Execute in the existing checkout and preserve unrelated edits. The owner authorized verification and a scoped commit of personality and web UI changes on main. Do not push.

**Goal:** Make profile customization edit the personality used by the Rust runtime and verify persistence across chats and process restarts.

**Architecture:** Owner routes resolve the same SOUL/configuration source as runtime admission. Guided fields and raw Markdown edit one document.

## Tasks

- [x] Share legacy rendering with runtime; implement owner GET/PUT, revisions, validation, atomic SOUL saves, and durable configuration saves.
- [x] Add typed SDK reads/writes and transport tests.
- [x] Implement guided/Markdown editing, active-source display, conflict handling, saving status, and draft preservation. Remove inactive personality controls.
- [x] Complete independent backend and UI reviews and resolve their findings.
- [x] Run focused frontend/SDK checks, visual inspection, and isolated multi-chat/process-restart acceptance.
- [x] Obtain a clean full Rust host/core and web/SDK test result after the execution-budget and chat-control work is complete.

The behavior contract is in [the feature spec](../specs/2026-10-23-active-personality-design.md).

## Verified behavior

The editor exposes identity/role, personality, values/philosophy, goals, expertise, communication, boundaries, and example replies. It preserves prose, frontmatter, and unknown sections; Markdown provides access to the whole source. Workspace personalities are identified as shared. Saves affect newly admitted turns; admitted turns retain their snapshot.

The backend edits the selected workspace/profile SOUL, or persisted configuration when no SOUL is active. Saves are owner-only, no-store, conflict checked, bounded to 16,000 Unicode scalar values, and durable before success. Knowledge and unrelated settings are preserved.

## Validation evidence

- Current normal daemon executable build passed through `rust-daemon:build` with one Cargo build job, incremental compilation disabled, the existing SDK Cargo cache, and Nx task dependencies excluded. Cargo compiled the executable's Rust dependencies normally.
- Final web production build and web/SDK/UI type checks passed. Editor/parser/Settings tests passed 78/78; the final shared-source explanation also passed all eight editor tests. Six updated settings shell journeys passed.
- Desktop and 390px mobile visual checks passed with the actual component and an isolated fixture: multiline editing, saving, Markdown switching, and no horizontal overflow.
- SDK deterministic suite passed 255 tests; four service tests were skipped in that run. Personality restart acceptance ran separately and passed against the freshly built current daemon.
- Process acceptance verified two distinct chats per source (`agent_config` and `profile_soul`), before and after stopping/restarting the daemon: eight turns total. Actual HTTP model requests contained saved content and excluded displaced instructions. Files, credentials, ports, control-plane storage, and agents were isolated from owner data.
- Earlier acceptance executable: `target/cargo/sdk-daemon-integration/debug/anima-daemon.exe`, built 2026-10-23 15:07 MYT. SHA-256: `e8f20bc353bdb5c00a9a0309e2662b1298501d86b50554cc58c00519cac8c05e`.
- The current built executable's OpenAPI inventory matches the daemon README: 295 operations, including personality GET/PUT and the concurrent conversation revision/queued-edit operations.
- All 16 personality backend tests passed in the pre-crash full host run: authorization/source isolation, revisions, rejected/linked files, atomic failure, persistence failure/uncertainty, cancellation, ACL comparison, and model-request capture after state restoration. Personality route and file-primitive source survived intact.

Evidence uses a local deterministic model stub. Real-provider personality adherence and the owner's live application instance were not tested. No owner personality files or running agents were changed.

## Current full-suite verification

The fixed commit snapshot passed the required Nx Rust host test target: 3,170 daemon unit tests, 150 API/integration tests, and the complete core/harness dependency suite. Rust formatting and the normal daemon executable build passed. Web production build and web/SDK/UI type checks passed; all 2,213 enabled web tests and 255 deterministic SDK tests passed. One web test and four service tests were skipped in the ordinary suites; personality process acceptance ran separately and passed.

The final executable was built from the reviewed snapshot and checked through its dependency records. Its SHA-256 was `9937ea817d09bac2395e668dfcfabc17b95afd1211e44ead7b92aceedbfd5058`; that hash remained unchanged throughout the real-process restart test. Eight turns across configuration and profile SOUL sources captured actual HTTP model requests before and after restart. The saved instructions were present and displaced instructions were absent.

Verification corrected stale controller workflow/navigation fixtures and an attachment-upload readiness race. Independent review also found and fixed background preferences reverting when storage reads succeed but writes fail; the new regression failed before the fix and passed afterward. These results supersede the earlier full-suite failures. Temporary evidence, concurrent design edits, and later UI edits are excluded from the scoped commit.

## Crash recovery

The crash replaced multiple unsaved files with NUL bytes. The personality integration test and inventory assertion were recovered from saved edit history; shared SDK/runtime files were recovered while preserving unrelated edits. A duplicate recovered SDK field was already repaired by its owning task when checked. A test-only mutex import gate used by new runtime fields was corrected. Stale core artifacts in the shared validation cache led to serial validation in the SDK cache with incremental compilation disabled; no Cargo cache was deleted.
