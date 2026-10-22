# TUI approvals and clarifications implementation plan

> Execute inline using superpowers:executing-plans. User authorization covers implementation; no commit or push is requested.

**Goal:** Resolve session approvals and clarification requests from the terminal.

**Architecture:** Optional review API on the existing daemon adapter, one review hook consuming the existing event subscription, focused keyboard forms inside mounted session chat.

**Tech stack:** TypeScript, React, Ink, existing AnimaOS SDK, Vitest through Nx.

## 1. Review state and API

- [x] Add meaningful regression tests for version merging, scope filtering and form validation.
- [x] Add `session-review-state.ts` with pure helpers for literal display, validation and merges.
- [x] Add `TuiReviewApi` to `session-api.ts` and export it from `index.ts`.

## 2. Review lifecycle

- [x] Add hook tests for initial reads, SSE reconciliation, navigation, polling cleanup, revision conflicts and duplicate writes.
- [x] Add `hooks/use-session-review.ts` with two-second reads, source errors, scoped updates and guarded writes.
- [x] Add an optional event observer to `use-session-workspace.ts`, retained in a ref without reconnecting the subscription.

## 3. Keyboard forms

- [x] Add approval and clarification keyboard interaction tests.
- [x] Add `components/session-review.tsx`, `approval-review.tsx`, and `clarification-review.tsx`; separate edit and confirmation modes.
- [x] Keep chat mounted while review is visible, deactivate its input and preserve its draft.

## 4. Product wiring

- [x] Add `/review`, `/approvals`, `/clarifications`, Ctrl+R, attention count, and help entries.
- [x] Wire existing SDK approvals/interactions through the CLI adapter, preserving optional injection compatibility.
- [x] Add App and CLI regression coverage for the new paths.

## 5. Validation and review

- [x] Run `bun x nx run-many -t test typecheck -p @animaos/tui @animaos/cli --skipNxCache --output-style=static`.
- [x] Run `bun x nx run @animaos/cli:build --skipNxCache --output-style=static` (includes SDK/TUI dependencies).
- [x] Review the final diff, preserve unrelated staged/unstaged changes, and run `git diff --check`.
- [x] Record validation evidence and limitations here.

## Validation evidence (2026-10-23)

- TUI: 210 tests passed across 22 files.
- CLI: 83 tests passed across 9 files.
- TUI, CLI, and dependent SDK typechecks passed.
- CLI build and dependent SDK/TUI builds passed, run after the typechecks.
- Final code review approved after fixes for reconnect readiness, stale pending revision merges, late form navigation, and request-specific conflict invalidation. Regression tests reproduced each issue before its fix.
- Prettier checks passed for the new TUI files and changed TUI integration files; `git diff --check` and staged diff checks passed. No files were staged or committed by this work.
- Tests exercise real Ink rendering and keyboard input with injected daemon API responses. No live provider approval/clarification run was performed. Workspace-wide policy/rule management and separate helper sessions remain outside this milestone.
- Nx required normal native-cache access outside the restricted sandbox, as in the preceding milestone. No dependency or cache configuration changes were needed.
