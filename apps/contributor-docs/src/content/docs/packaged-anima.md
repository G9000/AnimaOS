---
title: Packaged Anima
description: The installable anima program, its commands, and how releases and updates work
---

`hosts/anima` builds the installable `anima` program. It wraps `rust-daemon`, embeds the built web console, and adds a supervisor, a tray / menu-bar icon and a self-updater. Contributors keep using `bun dev --host rust`; the packaged program is what end users install.

## Install

macOS:

```bash
curl -LsSf https://github.com/G9000/animaOS-SWARM/releases/latest/download/anima-installer.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/G9000/animaOS-SWARM/releases/latest/download/anima-installer.ps1 | iex
```

The installers come from cargo-dist and put `anima` in dist's default location (`~/.cargo/bin`). They cannot run commands afterwards, so they end with `Run 'anima setup' to finish installing Anima.`

`anima setup` asks for the workspace folder, the port and whether to start at login, and offers to import provider API keys found in your shell (a login-started Anima never sees the shell environment). It then starts Anima and opens the console. It is safe to re-run.

Imported keys reach chat and the other model calls. Memory embeddings, speech-to-text and coding-agent key passthrough still read the environment variable, so if you use them, set the variable in your login environment as well.

Until `anima setup` has chosen a workspace, `anima`, `anima start` and `anima supervise` refuse to run and say ``Anima isn't set up yet. Run `anima setup` first.`` Anima also won't start its server in a workspace that is your home folder, contains `~/.animaos-swarm/` or is inside it.

## Commands

| Command | What it does |
|---|---|
| `anima` | Start Anima if needed, then open the console |
| `anima setup` | First-run questions; safe to re-run. `--yes` accepts the defaults |
| `anima status` | Running / stopped, version, address, workspace, connectors, update available |
| `anima start` / `stop` / `restart` | Start or stop the server; the tray stays |
| `anima quit` | Stop everything, including the tray |
| `anima autostart on` / `off` | Start at login, or not |
| `anima logs [-f]` | Recent log output |
| `anima update` | Install the latest release |
| `anima config get [key]` / `set <key> <value>` | Read or change `workspace`, `port`, `control-port`, `updates.check` |
| `anima run` | Run the server in the foreground, with no tray and no supervision |

`stop`, `restart`, `quit` and `update` accept `--now` to skip waiting for running work.

## Stop vs Quit

- **Stop** turns the server off. The tray icon stays, shows "Stopped", and has a Start item.
- **Quit** exits everything, tray included. Anima returns at next login (if autostart is on) or when you run `anima`.

Restart, Stop, Quit and Update wait for running tasks first ("Waiting for N tasks to finish"). While it waits, the console shows a button that skips the wait ("Stop now", "Restart now", "Quit now" or "Update now") next to **Cancel**, the tray offers **Cancel**, and on the CLI you use `--now`. While an update is downloading, Stop, Restart, Quit and Cancel abandon the download.

Agents need owner approval to run `anima stop|quit|restart|update|config|autostart|setup|start|supervise`, or any command that names `/api/system` or the control port (`127.0.0.1:8788`). Anima's token, config and provider-key files count as secrets, like `~/.ssh` keys.

## Side by side with `bun dev`

| | Installed `anima` | `bun dev --host rust` |
|---|---|---|
| Console / API port | 8787 (control port 8788) | 8080 |
| Data | `~/.animaos-swarm/` | its own dev data |
| Keychain namespace | `installed` | its own |

The two never share data or credentials. A Telegram bot can only be connected to one copy; the other shows "This bot is already connected somewhere else."

## Updating

Releases are published from tags, and installed copies update only from published releases, never from local builds. Archives are verified against a minisign signature before anything is swapped in. `anima update` (or Update in the tray or console) installs the latest release; a failed update rolls back automatically.

Updates are disabled until the owner replaces the placeholder public key in `hosts/anima/release-key.pub`. Until then a build reports "Updates are not configured for this build" and people reinstall with the installer.

## Releasing

Push a tag such as `v0.2.0`. The pipeline, signing keys, repository secrets, signature convention and the dry-release checklist are in [`tools/release/README.md`](https://github.com/G9000/animaOS-SWARM/blob/main/tools/release/README.md).

Not yet verified: Windows paths, and a real CI release run (the owner's dry release).
