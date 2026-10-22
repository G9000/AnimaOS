# TUI Session Chat and Live Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add persistent daemon-backed chat sessions and live run controls to the Ink TUI while preserving its existing swarm workflow.

**Status (2026-10-22):** Implementation and final validation passed. TUI: 181 tests; CLI: 82 tests; SDK: 253 tests, including three real-daemon integration tests. SDK, TUI, and CLI Nx builds and typechecks passed. Initial expected-red steps remain unchecked because the first test commands failed before target execution; their original red state was not retroactively verified.

**Architecture:** The CLI injects a narrow `TuiSessionApi` backed by the existing `DaemonClient`. A TUI hook owns agent/session selection, transcript loading, run actions, and a single agent event stream; focused Ink components render the session picker and conversation. The existing swarm event bus and view remain independent.

**Tech Stack:** TypeScript, React 19, Ink, Vitest, `@animaos/sdk`, Nx, Bun.

---

## File map

| File | Responsibility |
|---|---|
| `packages/tui/src/session-api.ts` | Narrow typed daemon operations the TUI needs for sessions and live runs. |
| `packages/tui/src/session-state.ts` | Pure session-message reconciliation and live-event reduction helpers. |
| `packages/tui/src/session-state.spec.ts` | Prove message IDs deduplicate and relevant event updates are applied. |
| `packages/tui/src/hooks/use-session-workspace.ts` | Load agents/sessions/transcripts, own selection and run actions, and manage stream lifecycle. |
| `packages/tui/src/hooks/use-session-workspace.spec.tsx` | Exercise loading, sending, stop, and stream recovery with a fake `TuiSessionApi`. |
| `packages/tui/src/components/session-list.tsx` | Keyboard-navigable agent/session picker and create-session action. |
| `packages/tui/src/components/session-list.spec.tsx` | Cover selection, empty, loading, error, and multiple-agent states. |
| `packages/tui/src/components/session-chat.tsx` | Render persisted messages, live run/tool state, mode, composer, and stop action. |
| `packages/tui/src/components/session-chat.spec.tsx` | Cover transcript order, live progress, queue/steer toggle, and stop wording. |
| `packages/tui/src/input-commands.ts` | Register `/chat`, `/sessions`, `/new`, `/open`, `/agent`, `/queue`, `/steer`, and `/stop` where valid. |
| `packages/tui/src/input-commands.spec.ts` | Assert command visibility and mode-sensitive availability. |
| `packages/tui/src/app.tsx` | Add session views and route slash commands to the hook/components. |
| `packages/tui/src/app.spec.tsx` | Verify session commands navigate correctly and current swarm commands still work. |
| `packages/tui/src/index.ts` | Export the public TUI API type if needed by CLI consumers. |
| `packages/cli/src/commands/launch.ts` | Adapt `DaemonClient` operations to `TuiSessionApi` and pass it into the TUI. |
| `packages/cli/src/commands/launch.spec.ts` | Verify the adapter is passed and existing launch behavior is retained. |

## Task 1: Define the TUI session API and wire the CLI client

**Files:**
- Create: `packages/tui/src/session-api.ts`
- Modify: `packages/tui/src/index.ts`
- Modify: `packages/cli/src/commands/launch.ts`
- Test: `packages/cli/src/commands/launch.spec.ts`

- [x] **Step 1: Add a failing launch test for session API injection**

Extend the existing fake TUI runtime in `launch.spec.ts` to capture `App` props. Execute an interactive launch with the existing fake daemon client and assert the TUI receives functions for agent listing, session listing/creation, message loading, run listing/start/stop, and event streaming.

- [ ] **Step 2: Run the focused CLI test and confirm the expected failure**

Run: `bun x nx test @animaos/cli --skipNxCache`

Expected: the launch test fails because no `sessionApi` prop is currently passed; unrelated existing tests remain unchanged.

- [x] **Step 3: Add the narrow `TuiSessionApi` interface and client adapter**

In `session-api.ts`, define an interface with these operations and domain types imported from `@animaos/sdk`:

```ts
export interface TuiSessionApi {
  listAgents(signal?: AbortSignal): Promise<AgentSummary[]>;
  listSessions(agentId: string, cursor?: string, signal?: AbortSignal): Promise<SessionPage>;
  createSession(agentId: string, title?: string): Promise<Session>;
  getMessages(agentId: string, sessionId: string, before?: string, signal?: AbortSignal): Promise<SessionMessagePage>;
  listRuns(agentId: string, sessionId: string, signal?: AbortSignal): Promise<Run[]>;
  startRun(agentId: string, sessionId: string, input: StartRunInput, idempotencyKey: string): Promise<StartRunResult>;
  stopRun(agentId: string, runId: string): Promise<Run>;
  streamEvents(agentId: string, signal: AbortSignal): AsyncIterable<AgentEvent>;
}
```

Adapt each operation in `launch.ts` to the existing `client.agents`, `client.sessions`, `client.runs`, and `client.events` clients. Use `client.agents.listSummaries()` to avoid fetching agent transcripts just to show an agent picker. Do not add a second HTTP client or new daemon endpoints. Export the type from the TUI package entrypoint.

- [x] **Step 4: Run the focused CLI test and confirm it passes**

Run: `bun x nx test @animaos/cli --skipNxCache`

Expected: the new adapter assertion and all existing CLI tests pass.

> Final validation passed: CLI 82/82 and SDK 253/253. The initial native-binding blocker was SWC cache access inside the agent sandbox; running Nx outside the sandbox allowed the installed binding to load.

## Task 2: Implement transcript reconciliation and event reduction

**Files:**
- Create: `packages/tui/src/session-state.ts`
- Create: `packages/tui/src/session-state.spec.ts`

- [x] **Step 1: Write failing tests for message reconciliation**

Test that merging an authoritative daemon page with live messages deduplicates by `SessionMessage.id`, preserves the authoritative message content for matching IDs, and sorts by `createdAtMs`, the daemon's numeric message ordinal encoded in runtime message IDs, then ID.

- [ ] **Step 2: Run the TUI test target and confirm the helper is missing**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: the new import or assertion fails because the reconciliation helper does not exist.

- [x] **Step 3: Implement pure reconciliation and event reduction helpers**

Add `mergeSessionMessages(current, authoritative)` and a reducer for the selected session's events. Seed live run/tool state from the initial `stream.snapshot`, scoping each snapshot run by `run.sessionId`; snapshot runs may belong to helpers. Keep the subscribed stream agent distinct from the run's owning agent, since forwarded helper events retain the helper `agentId`. Reject events from a different source stream or session, and accept helper-owned run events only when their run-to-agent/session association is known or established by a lifecycle event on the selected stream. Track lifecycle events, run progress, step deltas, and tool start/finish. When an event omits `sessionId`, associate it through its `runId` and the run-to-session map. Key tool cards by run, step, and call ID. Treat `stream.resync` as a signal for the hook to reload authoritative messages and runs rather than guessing which events were missed.

- [x] **Step 4: Test live event filtering and resync signaling**

Add tests that unrelated events do not alter the active session, repeated `message.created`/reconciliation does not duplicate transcript entries, and `stream.resync` requests a refresh. The initial `stream.snapshot` must seed active runs and tool state for the chosen session, including helper-owned runs when their session matches. Since `EventBase.sessionId` is optional, associate snapshot and run events by each `Run.sessionId`, and associate later run-scoped events that omit `sessionId` through run-to-session and run-to-agent maps.

- [x] **Step 5: Run the focused TUI tests and confirm they pass**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: reconciliation and event-reduction tests pass.

> Final validation passed: the full TUI suite passed 181/181, including all 10 reconciliation/event-reducer tests.

## Task 3: Add the session workspace hook

**Files:**
- Create: `packages/tui/src/hooks/use-session-workspace.ts`
- Create: `packages/tui/src/hooks/use-session-workspace.spec.tsx`

- [x] **Step 1: Write failing hook tests for agent/session loading**

Cover no agents, automatic selection of exactly one agent, explicit selection when there are several, listing only non-archived `chat` sessions, creating a session, and opening a session with persisted messages and recent runs.

- [ ] **Step 2: Run TUI tests to confirm the missing hook behavior**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: the new hook tests fail because the hook is not implemented.

- [x] **Step 3: Implement selection, loading, and send actions**

Implement hook state for agents, selected agent, session page/cursor, active session, persisted messages, recent runs, current run mode, loading/error state, and stream status. An offline/failed load must retain the last usable state and expose an error; while offline, disable session-changing actions. If the active session is deleted, leave the chat view, reload the session list, and show a recoverable message. Generate a fresh idempotency key for each send. Start messages with the selected `queue` or `steer` mode and refresh state from the daemon after acceptance or terminal events.

- [x] **Step 4: Add failing tests for queue/steer, stop, and reconnect**

Assert `startRun` receives the selected mode and a non-empty idempotency key; `/stop` behavior calls `stopRun` only for a stoppable run; an initial `stream.snapshot` restores active run/tool state for the selected session; run-scoped events without `sessionId` are matched through `runId`; after the event iterator closes, the hook reloads session messages and runs before resuming the stream. Also cover offline-daemon actions being disabled, missing/deleted sessions returning to the list, and failed API requests showing a recoverable error while retaining existing transcript content.

- [x] **Step 5: Implement stop and event-stream lifecycle**

Keep one event stream per selected agent, abort it when the agent changes or the TUI unmounts, and reconnect after an unexpected close. Apply the initial `stream.snapshot` before live events so an already-running session immediately shows its run/tool state and stop availability. Show a disconnected/reconnecting state while reconnecting. Reconcile refreshed messages by ID so reconnect cannot duplicate messages. Represent a pending stop as requested until the daemon reports a terminal run state.

- [x] **Step 6: Run the focused TUI tests and confirm they pass**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: hook tests pass for loading, start modes, stop confirmation, event filtering, deduplication, and reconnect recovery.

> Final validation passed: 34/34 hook tests and 181/181 full TUI tests. Async tests await observable session/request state; the shared Ink harness also waits for render output to flush.

## Task 4: Build the session picker and chat views

**Files:**
- Create: `packages/tui/src/components/session-list.tsx`
- Create: `packages/tui/src/components/session-list.spec.tsx`
- Create: `packages/tui/src/components/session-chat.tsx`
- Create: `packages/tui/src/components/session-chat.spec.tsx`

- [x] **Step 1: Write failing picker tests**

Use the existing Ink test harness to cover session title/activity rows, agent selection for multiple agents, create/open callbacks, and empty/loading/error states.

- [x] **Step 2: Implement the session picker**

Render a keyboard-navigable agent/session list. Show last activity and active-run/pending-approval counts when present. Offer create-session and open-session actions; paginate with the returned cursor rather than silently omitting later sessions.

- [x] **Step 3: Run the focused TUI tests and confirm the picker passes**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: picker behavior and all prior TUI tests pass.

- [x] **Step 4: Write failing chat-view tests**

Cover chronological user/assistant/tool messages, streamed assistant text and tool progress, current run state, a visible queue/steer mode, text submission, and stop availability only for a stoppable active run. Verify pending stop is labelled “stop requested,” not “stopped.”

- [x] **Step 5: Implement the session chat view**

Render the transcript and current live progress in focused components. Keep the composer text-only in this milestone. Bind the mode toggle, send, and stop callbacks to the workspace hook.

- [x] **Step 6: Run the focused TUI tests and confirm the chat view passes**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: chat-view tests and all prior TUI tests pass.

> Final validation passed: all picker/chat component tests and the full TUI suite passed.

## Task 5: Integrate views and slash commands into the TUI shell

**Files:**
- Modify: `packages/tui/src/input-commands.ts`
- Modify: `packages/tui/src/input-commands.spec.ts`
- Modify: `packages/tui/src/app.tsx`
- Modify: `packages/tui/src/app.spec.tsx`

- [x] **Step 1: Write failing command tests**

Verify `/chat`, `/sessions`, `/new`, `/open`, `/agent`, `/queue`, `/steer`, and `/stop` are suggested only in applicable interactive views, while existing swarm commands and help text remain available.

- [x] **Step 2: Implement command registration**

Add commands for opening chat/session list, creating/opening a session, selecting an agent, switching send mode, and stopping the active run. Choose command argument syntax as `/open <session-id-or-title>` and a no-argument `/queue` or `/steer` mode toggle; keep mode visible in the composer footer.

- [x] **Step 3: Run the focused TUI tests and confirm the command tests pass**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: command tests pass and the existing command suggestions remain unchanged outside the new session views.

> Final validation passed: all command tests and the full TUI suite passed.

- [x] **Step 4: Write failing app navigation tests**

Use `renderInk` to verify `/chat` opens the currently selected session; if none is selected, it opens the most recently active non-archived chat for the selected agent; if that agent has no sessions, it opens the picker with create-session focused; if multiple agents exist and none is selected, it opens the agent picker. `/sessions` always opens the picker, `/new` creates and opens a session, `/open <session-id-or-title>` resolves the requested session, and `/back` returns to the prior view. Confirm missing `sessionApi` yields a helpful message and does not break existing launches.

- [x] **Step 5: Wire the hook, picker, and chat views into `App`**

Add `sessionApi?: TuiSessionApi` to `AppProps`, route the commands to hook actions, and render the new views. Keep the initial view as the existing swarm view and preserve current `/agents`, `/history`, `/trace`, `/resume`, `/retry`, and `/health` behavior.

- [x] **Step 6: Run the full TUI tests and confirm existing swarm behavior remains intact**

Run: `bun x nx test @animaos/tui --skipNxCache`

Expected: all TUI tests pass, including existing swarm and saved-run tests.

> Final validation passed: 181/181 TUI tests, including existing swarm/saved-run flows and all 67 app tests, with no unhandled errors.

## Task 6: Verify CLI wiring and build outputs

**Files:**
- Modify if needed: `packages/cli/src/commands/launch.ts`
- Modify if needed: `packages/cli/src/commands/launch.spec.ts`

- [x] **Step 1: Extend launch regression tests**

Assert interactive and one-shot launch paths continue to pass the same event bus, swarm runner, profiles, and health callbacks while also supplying the daemon-backed session API.

- [x] **Step 2: Run the CLI test target**

Run: `bun x nx test @animaos/cli --skipNxCache`

Expected: all CLI launch tests pass, including the new adapter assertions.

> Final validation passed: CLI 82/82, including all 34 launch tests. SDK 253/253, including three real-daemon integration tests. The SDK fixture now uses Vitest 5's `{ concurrent: false }` suite option in place of the removed `describe.sequential` API.

- [x] **Step 3: Build both packages**

Run:

```powershell
bun x nx build @animaos/tui --skipNxCache
bun x nx build @animaos/cli --skipNxCache
```

Expected: both packages build successfully with the exported API types resolved from the workspace package source.

> Final validation passed: SDK, TUI, and CLI Nx builds and typechecks. Rebuilding dependencies cleared the stale declaration errors. The CLI required-text validator now handles Clack's undefined empty-input value before calling `trim()`.

- [x] **Step 4: Check the final diff and report evidence**

Run `git diff --check`, inspect `git diff` and `git diff --cached`, and report test/build outcomes separately from live daemon/model acceptance. Do not stage or commit as part of this plan.

## Environment note

The installed SWC Windows binding rejected the sandbox cache's ACLs and could not write its fallback cache (`ERR_SWC_NATIVE_CACHE`). The package was present; no dependency reinstall or manifest change was needed. Running Nx with permitted access outside the sandbox resolved graph loading. Final commands used `NX_SOCKET_DIR=C:\tmp`, `NX_DAEMON=false`, `NX_SKIP_NATIVE_FILE_CACHE=true`, `NX_ISOLATE_PLUGINS=false`, and `CI=1`.

The SDK daemon integration tests reused `CARGO_TARGET_DIR=target/cargo`, used an isolated in-memory workspace and a local model stub, and passed. This does not establish manual interactive TUI or external-provider acceptance.
