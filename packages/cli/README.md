# @animaos/cli

Command-line interface for animaos.

This package builds the local `animaos` binary used for single-agent scaffolding and daemon-backed commands such as `run`, `chat`, `launch`, and `agents`.

`launch` is the primary TUI workflow. Use `--no-tui` only for automation, CI, or plain-text runs.

Plain-text launch supports both one-shot and interactive flows. In interactive `--no-tui` mode, the prompt supports `exit`, `/help`, and `/health`, and it prints daemon warning or recovery lines when connectivity changes.

Current CLI coverage includes:

- single-agent workspace scaffolding through `create agent`
- daemon-backed execution through `run`, `chat`, `launch`, `agents`, and `assistant`
- TUI and plain-text launch flows, including `/help` and `/health` in interactive `--no-tui` mode
- provider and API-key forwarding into daemon-backed runs
- exported commander entrypoints via `buildProgram()` and `main()`

## Quick Example

```bash
# Scaffold a single-agent workspace
bun run animaos create agent helper -d "Answer support questions"

# Run a daemon-backed task
bun run animaos run "Summarize the current agency setup"

# Inspect daemon-backed agents
bun run animaos agents list

# Chat with the persistent assistant agent (requires the daemon to run
# with ANIMAOS_RS_ASSISTANT_ENABLED=1)
bun run animaos assistant chat --user-name Leo

# Read proactive assistant messages
bun run animaos assistant inbox --since 1756000000000 --limit 10
```

## Build

Run `bun run build:cli-sdk` to build the CLI and its SDK dependency, or `bun run --filter @animaos/cli build` to build only this package.

## Run

Run `bun run animaos --help` or `bun run animaos launch "your task"`.

```bash
bun run animaos create agent helper -d "Research medical literature"
```

`create agent` writes a launchable workspace (`anima.yaml`, an org chart, and a profile). It uses a configured remote provider when credentials are available and records `deterministic` otherwise; pass `-p openai`, `-p anthropic`, or another provider to choose one. Multi-agent teams are authored by hand in `anima.yaml`; `launch` runs them.

```bash
# Single-shot plain-text launch
bun run animaos launch --no-tui "your task"

# Interactive plain-text launch
bun run animaos launch --no-tui
```

## Test

Run `bun run --filter @animaos/cli test`.
