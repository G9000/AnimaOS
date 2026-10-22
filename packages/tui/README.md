# @animaos/tui

Primary operator surface for animaos.

This package powers the Ink-based terminal experience behind `animaos launch`. It exports the terminal app shell, agent and tool panels, result views, status components, and event-log hooks used to inspect and steer swarm execution without leaving the terminal.

Current TUI coverage includes:

- the `App` shell that powers interactive launch sessions
- reusable panels for agents, history, trace, result, message, and tool inspection
- keyboard-first input and resume history surfaces for saved runs and slash commands
- daemon-aware operator states including preflight warnings, `/health`, reconnect recovery, and command-only paused input while the daemon is down
- persistent daemon chat sessions, transcript resume, queue/steer modes, and live stop controls
- approval decisions with explicit confirmation and clarification forms for choices, multiple selections, and free text
- small pure helpers and hooks that keep event-log, command-surface, and displayed-agent logic directly testable

## Launch from the repository checkout

Run these commands from the repository root. In the first terminal, start the daemon and browser surfaces:

```powershell
bun dev --host rust
```

In a second terminal, build the CLI and its dependencies:

```powershell
bun x nx run @animaos/cli:build
```

`launch` currently requires a folder containing `anima.yaml`. If you do not have one, create it once:

```powershell
bun run animaos create agent terminal -d "Personal assistant"
```

This creates `terminal/anima.yaml`. Add `--provider` and `--model` to choose settings for the scaffolded agent; without configured provider credentials, creation defaults to the deterministic provider. Persistent chat uses the selected daemon agent's saved model settings.

Launch the interactive TUI without a positional task:

```powershell
bun run animaos launch --dir ./terminal
```

For an existing workspace, replace `./terminal` with the folder containing its `anima.yaml`.

### Session and review controls

| Command or key | Action |
| --- | --- |
| `/chat` | Open the current or most recent chat; choose an agent when required. |
| `/agent`, `/sessions` | Choose a daemon agent or browse its sessions. |
| `/new`, `/open <id-or-title>` | Create or resume a persistent session. |
| `/queue`, `/steer`, Tab in chat | Select the next message's send mode when supported. |
| `/stop`, Ctrl+X | Request that the active run stop. |
| `/review`, Ctrl+R in chat | Open pending approvals and clarifications without losing the chat draft. |
| `/approvals`, `/clarifications` | Open a filtered review queue. |
| `/back`, `/help`, `/exit` | Navigate back, list commands, or quit. |

In review, use the arrow keys and Enter to open a request. Tab/Shift+Tab moves between fields. Approval forms use arrows to change the decision and matcher kind; clarification forms use Up/Down and Space to select choices. Enter on **Review decision** or **Review answers** opens a summary; a second Enter confirms submission. Escape goes back or returns from the summary to editing. Requests appear only when the daemon asks for owner input; chat messages alone do not grant tool authority.

## Quick Example

```tsx
import React from 'react';
import { render } from 'ink';
import { App, EventBus } from '@animaos/tui';

const eventBus = new EventBus();

render(
  <App
    eventBus={eventBus}
    strategy="supervisor"
    interactive
    agentProfiles={[{ name: 'manager', role: 'orchestrator' }]}
    onTask={async (task) => ({
      status: 'success',
      data: { text: `Handled: ${task}` },
      durationMs: 0,
    })}
  />
);
```

Further parity work includes memory drill-down, workspace-wide approval policy management, and additional session operations.

## Build

Run `bun x nx build @animaos/tui`.

## Test

Run `bun x nx test @animaos/tui`.
