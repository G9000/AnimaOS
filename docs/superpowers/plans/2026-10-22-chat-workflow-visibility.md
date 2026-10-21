# Chat Workflow Visibility Implementation Plan

> Implement the owner-approved narrow visibility correction inline with test-first and verification workflows. Preserve unrelated main edits; no commit or push is requested for this fix.

**Goal:** Chat shows active workflow work or owner actions; Work retains templates and history.

**Architecture:** Filter terminal runs only on the chat surface in the existing shared WorkflowAgentView. Remove the static chat management wrapper and gate template management/empty state to Work. Keep shared mutation, permissions and exact-retry behavior.

**Tech stack:** React, TypeScript, Vitest, Nx and CUA browser inspection.

- [x] Add regressions in ChatWorkflows.test.tsx and WorkspaceWorkflows.test.tsx; run Nx web:test and observe the requested visibility failures.
- [x] Implement the minimal chat filter, conditional template/empty UI and hidden idle wrapper in WorkspaceWorkflows.tsx and sessions/ChatWorkflows.tsx. Update the user guide.
- [x] Run focused Nx web tests, web:typecheck, web:build and docs:build. Review the scoped diff.
- [x] Browser: verify no completed/cancelled/template section in idle chat after reload; confirm Work retains existing history/templates; create an owner-only synthetic approval/question workflow, approve and answer from chat, verify it disappears on completion and remains in Work.
- [x] Record evidence and confirm only scoped changes are attributable to this fix, with no staging/commit/push.

## Results

- Ten requested visibility regressions failed on the original implementation, then passed after chat filtering and Work-only manager controls.
- Browser testing found mounted chat stayed stale after a Work-created workflow; a concurrent Work/chat regression failed before successful-change notifications were added. Notifications are agent-scoped and also survive the originating view changing before a successful response. Deferred create/template-run regressions failed before fixing notification ordering.
- Final Nx checks passed: 147 web tests across five suites, one existing skip; web typecheck including SDK/UI dependencies; web production build; docs build. Scoped git diff whitespace check passed.
- Browser: reload with six terminal runs showed no workflow manager/templates in chat. Work retained those six and the template picker. Created Chat visibility check 2026-10-22 with owner approval and input checkpoints only; approved and answered VISIBILITY-CHECK-OK from chat, observed the completed card disappear, and verified revision 3 plus its saved answer under Work. Created Chat cancellation visibility check 2026-10-22 under Work, returned to the still-mounted chat without reloading and saw it immediately, cancelled it, and observed the panel disappear. Work retained all eight runs. Final reload kept chat clean; browser console errors were empty. No executable tasks or external messages were involved.
- Evidence: target/chat-workflow-browser/active-checkpoint.png, completed-hidden.png, work-history.png, chat-clean-after-reload.png and verification.json.
- Read-only design and implementation review passed, including the two notification lifecycle regressions. At the initial handoff the fix remained unstaged on main, with unrelated work preserved.

## Commit authorization

The owner subsequently requested committing this fix. Commit scope is limited to the visibility implementation, regressions, user guide and this design/plan. Preserve unrelated daemon changes and do not push without a new request.
