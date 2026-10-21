# Chat workflows implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement task by task. Keep the user's existing `main` checkout and leave changes unstaged.

**Goal:** Create, approve, and answer durable workflows without leaving companion chat.

**Architecture:** Reuse the current daemon tools, owner workflow API, and React workflow controller/cards. Add an opt-in permissions pair and a transcript-tail surface refreshed by chat activity. Keep checkpoint decisions as direct owner actions.

**Tech Stack:** TypeScript, React, existing SDK, Nx/Vitest, browser CUA.

## Task 1: Permission toggle

Files: `apps/web/src/lib/agent-access.ts`, `apps/web/src/components/SettingsPanel.tsx`, `apps/web/src/components/SettingsPanel.test.tsx`.

- [x] Write failing tests for the toggle granting/revoking exactly `create_workflow` + `list_workflows`, preserving unrelated grants, profile changes, partial grants, helper disabling, and saved-state reseeding.
- [x] Run `bun x nx run @animaOS-SWARM/web:test --skipNxCache --args=SettingsPanel.test.tsx --run` and confirm the new tests fail for the missing toggle.
- [x] Add a shared workflow permission constant and implement the toggle with existing abilities, tool dirty state, and save filtering. Profile selections retain explicit workflow opt-in. No default enablement.
- [x] Rerun the focused permission suite, inspect the diff, and review spec compliance then quality.

## Task 2: Shared chat surface

Files: `apps/web/src/components/WorkspaceWorkflows.tsx`, new `apps/web/src/components/sessions/ChatWorkflows.tsx` and test, `apps/web/src/components/ChatScreen.tsx`, `apps/web/src/components/sessions/SessionView.tsx`, their focused tests.

- [x] Write failing chat integration tests covering persisted approval -> answer -> completion, checkpoint remount, external refresh, exact lost-response replay, agent/session switch, offline/stale response rejection, revocation preserving owner controls, and no creation form in chat.
- [x] Run the new focused suite and confirm missing-feature failures.
- [x] Expose the existing workflow controller with a chat surface and activity refresh key; reuse identical revision and pending-request behavior. Collapse terminal cards in chat and leave the Work surface unchanged.
- [x] Add a transcript-tail React slot to MessageList and mount an agent/session-keyed ChatWorkflows section only in ordinary web chat. Trigger reads after new messages/run status and poll running work with the existing bounded policy.
- [x] Run chat + Work + session integration suites, inspect the diff, and review spec compliance then quality.

## Task 3: Delivery and live acceptance

Files: `hosts/rust-daemon/src/tools.rs`, `hosts/rust-daemon/src/tools/workflows.rs`, `apps/docs/src/content/docs/companion-workflows.mdx`, `docs/companion/durable-work.md`, ignored `target/workflows-chat-browser/` evidence.

- [x] Update model-facing create/list descriptions and list output to explain owner controls in chat as well as Work. Document the toggle, natural-language creation, inline approval/answer controls and retained permission/owner boundaries.
- [x] Run web typecheck/build, appropriate docs build, and focused tests through Nx in the final tree. Run the required `bun x nx run rust-daemon:test --skipNxCache` with the established Windows validation target directory and bounded test concurrency.
- [x] Use CUA browser UI to enable the toggle, create a text-only workflow via chat, reload at approval, approve, answer, and observe completion with unchanged first job identity. Verify cancellation and revoke/re-enable behavior without sending external messages.
- [x] Save screenshots and a concise acceptance report; leave a useful browser view open. Record exact live limitations. Do not stage, commit, or push new changes.

## Final evidence

- Permission tests: 60 passed, red/green regression evidence under `target/chat-workflow-permissions-*.log`.
- Chat, session, message list and Work workflow tests: 134 passed, 1 existing skip; `target/chat-workflows-green.log`. Review regressions reproduced 4 failures before the helper-role and scroll fixes.
- Final web typecheck and production build passed; `target/chat-workflows-web-typecheck.log`, `target/chat-workflows-web-build.log`.
- Required Rust daemon Nx test passed: 2,887 passed, 35 ignored; `target/chat-workflows-rust-full.log`.
- Docs build passed: 30 pages plus Pagefind; `target/chat-workflows-docs-build.log`.
- Both tasks passed spec and code quality review. No new checkpoint authority was exposed to the model.
- Actual browser acceptance passed chat creation, checkpoint reload without replay, permission grant/revoke, inline approval/answer, final completion with saved context, and chat cancellation before task admission. Final reload retained Completed and Cancelled records. Evidence: `target/workflows-chat-browser/browser-results.md`.
- Initial delivery left changes unstaged on main. The owner subsequently authorized committing and pushing this extension to GitHub on main.
