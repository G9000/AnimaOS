---
title: Packaged Anima
description: The installable anima program, its commands, how it supervises the daemon, and how releases and updates work
---

`hosts/anima` builds the installable `anima` program (Bun workspace `anima`, Cargo package `anima`). It embeds `anima-daemon` as a library, embeds the built web console in release builds (the `bundled-ui` feature), and adds a supervisor, a tray / menu-bar icon (macOS and Windows) and a self-updater. Contributors keep using `bun dev --host rust`; the packaged program is what end users install.

Releases are built for **macOS on Apple Silicon** (`aarch64-apple-darwin`) and **Windows x64** (`x86_64-pc-windows-msvc`). Only `hosts/anima` ships; `anima-daemon` is not published on its own (`dist = false`).

## Install

macOS:

```bash
curl -LsSf https://github.com/G9000/animaos/releases/latest/download/anima-installer.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/G9000/animaos/releases/latest/download/anima-installer.ps1 | iex
```

The installers come from cargo-dist and put `anima` in dist's default location (`~/.cargo/bin`). They cannot run commands afterwards, so they end with `Run 'anima setup' to finish installing Anima.`

`anima setup` asks for the workspace folder, the port and whether to start at login, and offers to import provider API keys found in your shell (a login-started Anima never sees the shell environment). It then starts Anima and opens the console. It is safe to re-run. `--yes` accepts the defaults and `--workspace <path>` names the folder.

Imported keys reach chat and the other model calls. Memory embeddings, speech-to-text and coding-agent key passthrough still read the environment variable, so if you use them, set the variable in your login environment as well.

Until `anima setup` has chosen a workspace, `anima`, `anima start` and `anima supervise` refuse to run and say ``Anima isn't set up yet. Run `anima setup` first.`` Anima also won't start its server in a workspace that is your home folder, contains `~/.animaos-swarm/` or is inside it, or that does not exist or is not a folder.

## Commands

| Command                                        | What it does                                                                 |
| ---------------------------------------------- | ---------------------------------------------------------------------------- |
| `anima`                                        | Start Anima if needed, then open the console                                 |
| `anima setup`                                  | First-run questions; safe to re-run. `--yes` accepts the defaults            |
| `anima status`                                 | Running / stopped, version, address, workspace, connectors, update available |
| `anima start` / `stop` / `restart`             | Start or stop the server; the tray stays                                     |
| `anima quit`                                   | Stop everything, including the tray                                          |
| `anima autostart on` / `off`                   | Start at login, or not                                                       |
| `anima logs [-f]`                              | Recent output of the server log (`logs/anima.log`, last 200 lines)           |
| `anima update`                                 | Install the latest release                                                   |
| `anima config get [key]` / `set <key> <value>` | Read or change `workspace`, `port`, `control-port`, `updates.check`          |
| `anima run`                                    | Run the server in the foreground, with no tray and no supervision            |
| `anima supervise [--background] [--no-tray]`   | Run the supervisor (what autostart launches)                                 |

`stop`, `restart`, `quit` and `update` accept `--now` to skip waiting for running work.

## On-Disk Layout and Config

Everything lives under `~/.animaos-swarm/` (`hosts/anima/src/paths.rs`, folders created private, mode `0700` on Unix):

| Path                           | Contents                                                                                                                                                          |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config.toml`                  | `workspace`, `port` (default 8787), `control_port` (default 8788), `autostart` (default false), `[updates] check` (default true). Written atomically, mode `0600` |
| `control-token`, `admin-token` | Bearer tokens for the supervisor control API and for the daemon's owner API                                                                                       |
| `data/`                        | The daemon's stores (below)                                                                                                                                       |
| `logs/`                        | `anima.log` (rotated at 10 MiB, 3 files kept)                                                                                                                     |
| `run/`                         | `update-pending.json`, `update-failed.json` during an update                                                                                                      |

Autostart is a per-user LaunchAgent `~/Library/LaunchAgents/dev.animaos.anima.plist` (label `dev.animaos.anima`) on macOS and the `AnimaOS` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows.

## What the Supervisor Gives the Daemon

`anima supervise` runs the server as a child (the same program, `anima run`), restarts it on crashes with backoff, and exposes a control API on `127.0.0.1:{control_port}` with `GET /v1/status` and `POST /v1/actions/{action}`, every request needing `Authorization: Bearer <control-token>`. The CLI and the daemon's `/api/system` relay both drive it. Stop, Restart, Quit and Update first ask the daemon to drain (see [Operations](/hosts/rust-daemon/ops/#drain-shutdown-and-the-supervisor-relay)).

The environment handed to the daemon (`hosts/anima/src/server_env.rs`; values already set in the process environment win):

| Variable                                              | Value                                                                                                                                   |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `ANIMAOS_RS_HOST` / `ANIMAOS_RS_PORT`                 | `127.0.0.1` / the configured port (8787)                                                                                                |
| `ANIMAOS_WORKSPACE_ROOT`                              | the configured workspace                                                                                                                |
| `ANIMAOS_RS_CONTROL_PLANE_FILE`                       | `data/control-plane.json`                                                                                                               |
| `ANIMAOS_RS_HISTORY_SQLITE_FILE`                      | `data/history.sqlite`                                                                                                                   |
| `ANIMAOS_RS_PROVIDER_KEYS_FILE`                       | `data/provider-keys.json`                                                                                                               |
| `ANIMAOS_RS_MEMORY_SQLITE_FILE`                       | `data/memory.sqlite` (skipped if `ANIMAOS_RS_MEMORY_FILE` is set)                                                                       |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE`            | `data/embeddings.sqlite`                                                                                                                |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR`              | `data/embeddings-cache`                                                                                                                 |
| `ANIMAOS_RS_CHECKPOINT_ROOT`                          | `data/checkpoints` (Windows only)                                                                                                       |
| `ANIMA_ALLOWED_UI_ORIGINS`                            | `http://127.0.0.1:<port>,http://localhost:<port>`                                                                                       |
| `ANIMA_PUBLIC_BASE_URL`                               | `http://127.0.0.1:<port>`                                                                                                               |
| `ANIMAOS_RS_KEYCHAIN_NAMESPACE`                       | `installed`                                                                                                                             |
| `ANIMA_LOCAL_ADMIN_TOKEN_FILE`                        | the path of `admin-token` (a path, not the token, because the environment of a process is readable by other processes of the same user) |
| `ANIMA_SUPERVISOR_URL`, `ANIMA_SUPERVISOR_TOKEN_FILE` | the control API address and the path of `control-token`                                                                                 |

Because `ANIMA_SUPERVISOR_URL` is set, the daemon accepts only the configured origins for owner actions, not the dev origins on ports 4200 and 4201.

## Stop vs Quit

- **Stop** turns the server off. The tray icon stays, shows "Stopped", and has a Start item.
- **Quit** exits everything, tray included. Anima returns at next login (if autostart is on) or when you run `anima`.

Restart, Stop, Quit and Update wait for running tasks first ("Waiting for N tasks to finish"). While it waits, the console shows a button that skips the wait ("Stop now", "Restart now", "Quit now" or "Update now") next to **Cancel**, the tray offers **Cancel**, and on the CLI you use `--now`. While an update is downloading, Stop, Restart, Quit and Cancel abandon the download.

The console's Restart, Stop, Quit and Update buttons work only in a console opened from the tray's **Open Console** or with `anima`. Those open it with a one-time code, valid for 15 seconds, which the console trades for a session: a browser-session cookie plus a token the console sends with each request. A console opened from a bookmark can show status but not act, and says where to open it from. Sessions last 12 hours and survive restarts. If the console says the launch link was already used, something else traded it first and may hold a session. Open Anima again. To end every session, quit Anima, delete `~/.animaos-swarm/admin-token`, and start it again. The code passes through the program that opens the browser and the browser's history, so a same-user process watching at the right moment can still take it.

Agents need owner approval to run `anima stop|quit|restart|update|config|autostart|setup|start|supervise`, or any command that names `/api/system` (in any path spelling), the supervisor's actions, or its control port on any host spelling (`127.0.0.1:8788`, `127.1:8788`, `[::1]:8788`, or the port in `ANIMA_SUPERVISOR_URL`). Reading Anima's token, config or provider-key files, the `~/.animaos-swarm` folder as a whole, or a browser's profile and cookie stores (Chrome, Chromium, Edge, Brave, Firefox, Safari) also needs approval, even when nothing is sent anywhere. The supervisor hands the server its tokens as file paths, not values, so they don't show in the process list.

## Side by Side with `bun dev`

|                    | Installed `anima`        | `bun dev --host rust`                               |
| ------------------ | ------------------------ | --------------------------------------------------- |
| Console / API port | 8787 (control port 8788) | 8080                                                |
| Data               | `~/.animaos-swarm/data/` | by default `~/.animaos-swarm/data/` too (see below) |
| Keychain namespace | `installed`              | none (historical service names)                     |

Ports and credential-store items never collide. **Data does by default**: `tools/workspace-dev` points the dev daemon's control-plane file at `~/.animaos-swarm/data/control-plane.json`, which is the installed program's file, so `history.sqlite` and `memory.sqlite` beside it are shared too. If both could run against one folder, give the dev daemon its own `ANIMAOS_RS_CONTROL_PLANE_FILE` (and `ANIMAOS_RS_PROVIDER_KEYS_FILE`) or use `ANIMAOS_DEV_EPHEMERAL=1`. A Telegram bot can only be connected to one copy; the other shows "This bot is already connected somewhere else."

## Updating

Releases are published from tags, and installed copies update only from published releases, never from local builds. An update is offered only when the latest stable GitHub release has the archive for this platform plus its `.sha256` and `.minisig` files. Before anything is swapped in, the archive must match its published SHA-256 and a detached **minisign** signature that verifies against the public key compiled into the binary (`hosts/anima/release-key.pub`), and the signature's trusted comment must name that exact release so an older signed build cannot be republished under a newer tag.

`anima update` (or Update in the tray or console) downloads, verifies, swaps the binary in and re-executes it. The old binary is kept; the new supervisor writes `run/update-pending.json` and has 60 seconds to get its server running. If it does not, the old binary is put back, `run/update-failed.json` is written, and the restored version reports "Update to vX failed to start; rolled back". A failed update therefore rolls back automatically.

Updates are disabled until the owner replaces the placeholder public key in `hosts/anima/release-key.pub` (it currently starts with `untrusted comment: PLACEHOLDER`). Until then a build reports "Updates are not configured for this build" and people reinstall with the installer.

## Releasing

Push a tag such as `v0.2.0`. `dist-workspace.toml` configures cargo-dist 0.32.0; the Release workflow builds both targets (embedding the web console), signs the macOS binary, and after publishing, `.github/workflows/sign-release.yml` signs each archive with minisign using `tools/release/sign-assets.sh`. The pipeline, signing keys, repository secrets, signature convention, tag protection and the dry-release checklist are in [`tools/release/README.md`](https://github.com/G9000/animaos/blob/main/tools/release/README.md).

Not yet verified: Windows paths, and a real CI release run (the owner's dry release).

## Development

```bash
bun run --filter anima build      # cargo build -p anima
bun run --filter anima test       # cargo test -p anima
bun run --filter anima lint       # cargo fmt --all --check
```

Tests use fakes for `launchctl`, the registry and the network, and a throwaway `minisign` key for update verification.
