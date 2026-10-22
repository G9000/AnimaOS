# TUI Session Chat and Live Controls

**Status:** Implemented and validated on 2026-10-23. Launch and usage instructions are in `packages/tui/README.md`.

## Context

The Ink TUI currently centers on YAML-configured swarm launches. It has agent, history, trace, and result views, saved-run commands, and daemon health handling. The web console additionally has durable agent sessions, persistent transcripts, live run controls, approvals, clarifications, attachments, and agent-management surfaces.

The shared `@animaos/sdk` already exposes daemon agent listing, session listing/creation/transcript loading, run start/stop/listing, and agent event streaming. The CLI launch path already constructs a `DaemonClient` and injects the current TUI runtime. The first parity slice can therefore stay in the TUI and CLI client wiring without adding daemon endpoints.

## Goal

Add a persistent chat lane to the existing TUI so an operator can select a daemon-managed agent, open or create a session, continue its transcript, and control an active run. Preserve the existing swarm, trace, agent-profile, and saved-run workflows.

This is the first milestone toward web-console parity. It targets the main chat workflow first; it does not claim full or numerical half parity by itself.

## Scope

### Included

- Enter the chat lane with `/chat` and browse sessions with `/sessions`.
- Load daemon-managed agents. If there is exactly one, select it automatically; if there are several, require an explicit choice before creating or opening a session.
- List, open, and create non-archived chat sessions for the selected agent.
- Load a session's persisted messages on open so work can continue after restarting the TUI.
- Send text messages through `runs.start`, with the current mode shown and selectable as `queue` or `steer`.
- Stop the active run through `runs.stop` and show the daemon-confirmed run state. While a stop request is pending, label it as requested rather than claiming the run has stopped.
- Stream agent events into the active session view and reconcile the transcript from the daemon after a dropped connection.
- Keep the existing launch view and all current swarm-related commands available without changing their behavior.

### Not included in this milestone

- Approval and clarification response cards.
- File or media attachments, audio, and browser-only interactions.
- MCP or connector management.
- Memory, skills, automations, runtime, usage, logs, or diagnostic pages.
- New daemon endpoints or changes to the web console.
- Replacing the TUI's current swarm workflow with a new shell.

These areas remain candidates for later parity milestones after this chat workflow is stable.

## Approaches considered

1. **Add a session lane to the current TUI (recommended).** Reuse the existing app shell and add a session picker and chat view. This gives the user the largest missing workflow while preserving launch behavior and containing the first change.
2. **Replace the shell with a session-first layout.** This would make the TUI match the web console's main entry point more directly, but would change the current launch experience and broaden the first milestone.
3. **Share React presentation code with Ink.** This could reduce some duplicated behavior, but the web console's hooks and visual components are browser-specific. Building a cross-surface layer would be a larger architecture project before the TUI gains the needed workflows.

## Proposed design

### TUI API boundary

Add a narrow, typed session API dependency to the TUI app for the operations it needs: agent list, session list/create/read, run start/stop/list, and agent event streaming. `packages/cli` supplies those operations from its existing `DaemonClient`. The TUI does not construct HTTP requests or import browser hooks. Tests provide a small deterministic implementation of this interface.

The current swarm event bus remains responsible for swarm rendering. Session event streaming has its own lifecycle so switching between the swarm and chat views cannot accidentally terminate or duplicate the existing swarm subscription.

### Views and interaction

- `/chat` opens the selected session, or the most recently active session when one is available.
- `/sessions` opens a keyboard-navigable list with session title, last activity, and run/approval attention state when those fields are available.
- From the list, the user can open a session or create one. A daemon agent chooser is shown only when the daemon reports more than one available agent.
- The chat view displays persisted messages, live assistant/tool progress supported by the event contract, run status, and the text composer.
- Queue/steer is a visible mode with a keyboard toggle and a matching slash command. `/stop` is available only when a run can be stopped.
- The current swarm view remains available through the existing launch flow and is not silently replaced by chat mode.

### Data flow

1. The CLI creates the existing daemon client and passes the narrow session API to the TUI.
2. The TUI loads available daemon agents. It auto-selects the sole agent or asks the user to select one when there are multiple.
3. The TUI lists that agent's non-archived chat sessions. Opening a session loads its transcript and its recent runs, then starts one agent event stream.
4. Sending a message calls `runs.start` with a fresh idempotency key and the selected queue/steer mode. Events update visible progress and the transcript.
5. Stopping calls `runs.stop`; the TUI shows the returned status or a pending stop request if the run has not reached a terminal state.
6. If the event stream closes, the TUI marks the view disconnected, reconnects, and reloads the session transcript and recent runs to reconcile missed events.

### Failure behavior

- If the daemon is unavailable, session-changing actions are disabled and the existing health/reconnect status is shown.
- If no daemon agents are available, explain that an agent must be configured before chat can start; do not create or mutate one implicitly.
- If a selected session disappears or an API request fails, keep the user in the session list or current transcript as appropriate and show a recoverable error.
- A successful `runs.start` response means the message/run was accepted, not that it completed. Completion is shown only after a terminal run event or a confirmed refreshed run state.
- A stop request is not described as completed until the daemon reports a terminal run state.

## Acceptance criteria

1. From the existing TUI, a user can enter the chat lane without losing access to the current swarm and saved-run views.
2. A user can choose a daemon agent, list its chat sessions, create a session, and reopen an existing session.
3. Reopening a session renders its daemon-persisted transcript.
4. A user can send a text message, see live progress, choose queue or steer mode, and stop an active run.
5. Reconnecting after an event-stream interruption reloads authoritative session and run state without duplicating a sent message.
6. Empty-agent, offline-daemon, missing-session, and failed-request states are visible and recoverable.
7. Existing TUI swarm, agent, trace, and saved-run behavior continues to pass its relevant tests.

## Validation plan

- Add focused TUI tests for agent/session selection, transcript restoration, send mode, live event handling, stop-state wording, stream reconnection, and recoverable failures.
- Add CLI wiring tests proving the current daemon client is supplied to the TUI and existing launch behavior remains available.
- Run the TUI and CLI Nx test/build targets after confirming their resolved project names.
- Do not treat mocked event delivery as proof of live daemon or model acceptance; a separate live acceptance check is needed before claiming the complete user-facing flow works against a running daemon.

## Later parity milestones

After this slice, the next high-value group is human review and task continuity: approvals, clarifications, run recovery, and attachment references. The broader operator group can then add memory, skills, automations, MCP/connections, files, usage, logs, and diagnostics. The UI also contains browser-native workflows that may need terminal-appropriate equivalents rather than direct visual ports.
