# Packaged Anima: one installable program that updates itself

Date: 2026-10-19
Status: draft for owner review

## Intent

Today, running Anima means cloning the repo and running `bun dev --host rust`. That starts the Rust daemon, the Vite dev server for the web console, and the playground, with settings coming from environment variables. The owner wants Anima to behave like an installed product instead, the way OpenClaw and Hermes do:

- Install it once with a single command.
- It runs in the background all the time and starts at login, so Telegram connectors stay up.
- You open the web console in your normal browser on localhost.
- You can stop it, or shut it down completely, from the web console, the terminal, or a tray / menu-bar icon (like Docker Desktop).
- It remembers where your workspace is, so there are no environment variables to set.
- When the owner publishes a new version on GitHub, installed copies can update themselves.

The owner's development loop stays the same: change code locally, push, and cut a release. Installed copies pick up **published releases**, not local builds.

## Goals

1. **One program.** A single `anima` executable contains the daemon *and* the built web console, and serves both on localhost (default port `8787`). No Node, Bun or Vite on the installed machine.
2. **One-line install** on macOS and Windows, from GitHub Releases.
3. **Always on, with an off switch.** It starts at login, restarts after a crash, and can be stopped (server off, icon stays) or quit (everything off).
4. **Three control surfaces that agree:** terminal commands, a tray / menu-bar icon, and a "System" section in the web console.
5. **Persistent by default.** Installed Anima keeps all its data under a per-user data folder. Today the daemon defaults to in-memory, ephemeral storage.
6. **Self-update from GitHub Releases**, with signature verification. It never interrupts work in progress without the owner's say-so.
7. **The dev workflow is unchanged.** `bun dev --host rust`, the Nx targets, `tools/workspace-dev`, the `anima-daemon` binary and the Dockerfile keep working as they do now.

## Non-goals (v1)

- A native desktop window (Tauri, Electron or a webview). The owner explicitly chose the browser on localhost.
- Updating from local builds. Updates come only from published GitHub Releases.
- Apple notarization, a paid Developer ID, or a Windows code-signing certificate. Installs happen through a terminal command, which avoids most Gatekeeper and SmartScreen prompts. Paid signing can be added later.
- Bundling optional external tooling the daemon can drive: the Playwright browser worker, ACP coding agents through `npx`, the Python tool RPC, and Postgres. Those stay "bring your own", as they are today.
- The playground app. Only the `apps/web` console is bundled.
- Linux tray support. If a Linux build ships at all, it is headless (see "Left to the builder").
- Changing the Docker image. It keeps running `anima-daemon` with env configuration.
- Auto-installing updates without the owner's say-so.

## Anti-goals (would count as failure even if it technically works)

- An update, restart or stop that kills running work or loses data without the owner choosing "stop now".
- An update that is not verified as coming from the owner's release key.
- On macOS, keychain "Always Allow" prompts coming back after **every** update. Connector credentials go through `keyring`, and the dev launcher already works around exactly this by re-signing debug builds (`tools/workspace-dev/src/rust-daemon-dev.ts`).
- A console window left open on Windows while Anima runs in the background.
- Any web page that isn't the console being able to stop, restart or update Anima (CSRF).
- An agent being able to read the control token or provider keys, or stop, update or reconfigure its own host without an owner approval.
- Breaking `bun dev`, the existing env-var configuration, or the Docker image.
- Moving reusable engine code into `hosts/*` or making `anima-core` depend on any of this (AGENTS.md).

## What the owner experiences

### Install

```bash
curl -LsSf https://github.com/G9000/animaOS-SWARM/releases/latest/download/anima-installer.sh | sh
```

```powershell
irm https://github.com/G9000/animaOS-SWARM/releases/latest/download/anima-installer.ps1 | iex
```

The script puts `anima` on the PATH and tells the user to run `anima setup` (cargo-dist installers cannot run post-install commands).

### `anima setup` (first run; can be re-run any time)

Asks a few things, with defaults:

1. **Workspace folder.**
   - Suggested default: `~/anima-workspace`.
   - The home folder itself is refused, and so is any folder that contains the data folder.
   - This becomes the workspace root.
2. **Port.** Default `8787`, so it never collides with `bun dev` on `8080`.
3. **Start at login?** Default yes.
4. **Import provider keys.** If provider API keys (e.g. `OPENAI_API_KEY`) are set in the current shell, offer to import them into the daemon's provider-key store. A login-started Anima doesn't see the shell environment, so without this it would be missing those keys.

It then writes the config, creates the control token, registers autostart if chosen, starts Anima, and opens the console in the browser.

### Day to day

- After login, Anima is already running and the tray / menu-bar icon is visible.
- Running `anima` with no arguments, or double-clicking `anima.exe` on Windows, starts Anima if it isn't running, then opens the console in the browser.

### Two ways to turn it off

- **Stop.** The server and its connectors stop. The tray icon stays, showing "Stopped", with a **Start** item.
- **Quit.** Everything exits, including the icon. It comes back at next login if autostart is on, or when the owner runs `anima` / `anima start`.

### Commands

| Command | What it does |
|---|---|
| `anima` | Start Anima if needed, then open the console |
| `anima setup` | First-run questions; safe to re-run |
| `anima status` | Running / stopped / not running, version, address, workspace, connector count, update available |
| `anima start` / `stop` / `restart` | Start or stop the server; the tray stays |
| `anima quit` | Stop everything, including the tray |
| `anima autostart on` / `off` | Start at login, or not |
| `anima logs [-f]` | Recent log output |
| `anima update` | Install the latest release (see "Updates") |
| `anima config get` / `set <key> <value>` | Read or change settings (workspace, port, update checks) |
| `anima run` | Run the server in the foreground with no tray and no supervision, for servers and debugging |

### Tray / menu-bar icon

The icon has a status dot: running, starting, stopped, waiting for work to finish, or update ready.

Menu:

- **Open Console** (disabled while stopped)
- A status line, e.g. "Running · v0.2.0 · 2 connectors"
- **Start** or **Stop**, depending on state
- **Restart**
- **Update to vX.Y.Z** (only when an update is available), otherwise **Check for updates**
- **Start at login** ✓
- **Quit**

On macOS there is no Dock icon. The icon lives in the menu bar only.

### Web console: "System" section

Shows the version, update status, workspace path, data folder and autostart state, with **Update**, **Restart** and **Stop** buttons.

After Stop, the page says Anima is stopped and can be started from the tray or with `anima start`.

### Stopping while work is running

Restart, Stop, Quit and Update all go through the same gate:

- If nothing is running, they happen right away.
- Otherwise the console and tray show "Waiting for N tasks to finish". The console lists what is running, and the owner can either keep waiting or pick **Stop now**.
- **Stop now** uses today's shutdown behaviour: a 30-second wait, after which queued runs are interrupted and recovered on the next start.

## Constraints found in the codebase

- **Configuration is env-only today.** Things the packaged defaults must cover:
  - Bind address: `ANIMAOS_RS_HOST` and `ANIMAOS_RS_PORT` (`main.rs`).
  - Workspace: `ANIMAOS_WORKSPACE_ROOT`, which falls back to the current directory (`tools/workspace.rs`).
  - Control plane: `ANIMAOS_RS_CONTROL_PLANE_FILE`. History (`ANIMAOS_RS_HISTORY_SQLITE_FILE`) and provider keys (`ANIMAOS_RS_PROVIDER_KEYS_FILE`) default to files next to the control-plane file.
  - Memory: `ANIMAOS_RS_MEMORY_SQLITE_FILE` / `ANIMAOS_RS_MEMORY_FILE` (`app/persistence.rs`).
  - Embeddings: `ANIMAOS_RS_MEMORY_EMBEDDINGS_*` (`memory_embeddings.rs`).
  - Checkpoints: `ANIMAOS_RS_CHECKPOINT_ROOT`.
  - Persistence: `ANIMAOS_RS_PERSISTENCE_MODE` and `DATABASE_URL`.
  - Origins and access: `ANIMA_ALLOWED_UI_ORIGINS` and `ANIMA_LOCAL_ADMIN_TOKEN` (`routes/http.rs`), and `ANIMAOS_RS_API_KEY`.
  - Public base URL: `ANIMA_PUBLIC_BASE_URL` (`app.rs`).
  - Optional extras: `ANIMAOS_RS_BROWSER_CONFIG_FILE`, plus OAuth client IDs and provider API keys from env.
- **With no env set, storage is ephemeral.** The control plane is `ephemeral` and memory is `memory-only`. The packaged program must fill in file-backed defaults.
- **OAuth callbacks.** `connectors/oauth_apps.rs` defaults the public origin to `http://127.0.0.1:8080` unless `ANIMA_PUBLIC_BASE_URL` is set. The packaged program must pass its configured port through, or OAuth redirects break when the port changes.
- **The web console already uses relative URLs.** It uses `baseUrl: ''`, and in dev Vite proxies `/api` and `/health` to `:8080`. It uses hash routing, so serving `apps/web/dist` from the daemon's own origin needs no client changes, and a root `index.html` fallback is enough.
- **Allowed UI origins** default to `localhost` / `127.0.0.1` on ports `4200` and `4201` (`DEFAULT_LOCAL_UI_ORIGINS`, `routes/http.rs`). The daemon's own origin (`http://127.0.0.1:<port>` and `http://localhost:<port>`) must also be allowed when it serves the console.
- **Document worker.** The daemon re-runs its own executable with `--document-worker-v2` (`main.rs`). The server process must keep handling that flag. On Windows, re-running the executable must keep working after a self-update has renamed the old binary.
- **Shutdown today** (`agent_runs/shutdown.rs`, `app.rs::serve`):
  - New messages are refused with a 503.
  - Accepted runs are waited on for up to `ACCEPTED_RUN_SHUTDOWN_WAIT_MS` (30 s).
  - Queued runs are interrupted on the next start (`restart_before_start`).

  This is the "Stop now" path. The patient gate is new.
- **Process-global state** (process runtime, live sessions, shell fuse, log buffer, connector rate state) means `serve()` is not safely restartable inside one process. Stop/Start must restart the server **process**.
- **Agent self-stop detection** (`approvals/detection.rs`) catches `pkill`/`killall anima…` and `launchctl bootout|unload`, but not the new `anima` subcommands.
- **macOS keychain trust is tied to the code signature.** Without a stable signing identity, every update brings back keychain prompts. See decision 12.
- **Existing per-user folder.** `~/.animaos-swarm/` already holds global `secrets.json` and `permissions.json` (`packages/tools`). Workspaces use a local `.animaos-swarm/` folder.
- **The repo is public** (`G9000/animaOS-SWARM`), so the update check needs no token.
- **There are no tags and no release workflow yet.** The workspace version is `0.1.0` (root `Cargo.toml`). CI is `.github/workflows/ci.yml`, which runs Nx on Linux.
- **`unsafe_code = "forbid"`** applies at the workspace level. Platform glue has to come from crates rather than local `unsafe`.
- **cargo-dist names installers and archives after the Cargo package**, so the `anima` program must be its own package.
- **Repo boundaries** (AGENTS.md): host-specific concerns live in `hosts/*`; reusable code lives under `packages/*`.
- **Memory overhaul in progress.** A spec and plan for a memory overhaul are in progress (`docs/superpowers/specs/2026-10-19-memory-overhaul-design.md`, untracked). It may change which memory files exist. The packaged defaults should follow whatever that work settles on.

## Decisions

### Made by the owner

- Package only the Rust backend. The web console is served by it and opened in the normal browser. No Tauri or webview.
- Run all the time, start at login, and provide controls to stop the server or close the whole thing.
- Control from three places: terminal, tray / menu-bar icon, and web console.
- Updates come from GitHub. The owner pushes and releases, and installed copies pick it up.
- Do it the "production standard" way, modelled on how OpenClaw and Hermes ship.

### Made by Claude (owner may override)

1. **Release trigger is a version tag** (`vX.Y.Z`), not every push to `main`. Half-finished work never reaches installed copies.
2. **Release tooling is cargo-dist.** It generates the GitHub Actions release workflow, builds per platform, and produces the `curl | sh` and PowerShell installers plus checksums.
3. **v1 platforms** are macOS Apple Silicon (`aarch64-apple-darwin`) and Windows x64 (`x86_64-pc-windows-msvc`). Others come later.
4. **New package `hosts/anima`** (binary `anima`). It is a thin host wrapper depending on the `anima-daemon` library: CLI, supervisor, tray, autostart, update and embedded UI. `hosts/rust-daemon`, its `anima-daemon` binary, the Dockerfile and `workspace-dev` stay as they are. Any shared change the wrapper needs from the daemon library (e.g. a config struct instead of env reads, a drain API, extra allowed origins) is added to `anima-daemon` without changing its env-driven defaults.
5. **The web console is embedded** in `hosts/anima` at release build time from `apps/web/dist`, behind a Cargo feature (`bundled-ui`) that is off by default. `rust-daemon:test` and dev builds don't require building the web app.
6. **Per-user folder is `~/.animaos-swarm/`** on both platforms, matching the existing convention. The packaged program adds:
   - `config.toml`
   - `control-token`
   - `data/` (control plane, history, provider keys, memory, embeddings and cache, checkpoints)
   - `logs/`
   - `run/`

   The folder and its contents are user-only (0700/0600 on macOS; a per-user ACL on Windows).

   **Precedence:** env var > `config.toml` > packaged default. The plain `anima-daemon` binary keeps today's ephemeral defaults.
7. **Two processes.**
   - The **supervisor** (`anima`) owns the tray, the control endpoint, autostart, update checks and binary swapping.
   - It runs the **server** as a child process (the same executable in server mode). That child is today's daemon, configured by the supervisor.
   - Stop/Start/Restart are child-process lifecycle. Quit ends both.
   - A server crash is restarted by the supervisor with backoff. A supervisor crash is restarted by the OS (see 8).
   - Without a desktop session (`anima run`), there is no supervisor.
8. **Autostart:**
   - On macOS, a per-user LaunchAgent at `~/Library/LaunchAgents/dev.animaos.anima.plist` with `KeepAlive` set to restart only on an unsuccessful exit, so Quit stays quit.
   - On Windows, a per-user logon entry (Task Scheduler task or `HKCU\…\Run`), so the tray has access to the user's desktop session. Not a Windows Service.
9. **Control channel.** CLI commands talk to the supervisor over loopback HTTP, authenticated with `~/.animaos-swarm/control-token`.
   - The web console's System section goes through the server, which relays to the supervisor. Reads need the local-owner checks (allowed origins). Actions, drain and shutdown also need a console session: the tray and `anima` open the console with a one-time launch code signed with the admin token, and the console trades it for an `HttpOnly` cookie plus a token it sends as a header (`console_session.rs`). An Origin header alone is not enough, because any local process can send one. Launch codes last 15 seconds and sessions 12 hours, and the cookie is a browser-session cookie. Remaining risk: the code travels in a URL (the opener's arguments, the browser's history), so a same-user process that watches at the right moment can trade it first. The owner's console then warns that the link was already used. Reading browser profile and cookie stores needs approval. An installed copy allows only its own origins, not the dev console's ports 4200/4201.
   - The tray calls the supervisor directly.
   - All three use the same supervisor functions.
10. **Work-in-progress gate.** A new drain mode in the server for Restart, Stop, Quit and Update. While draining, the server:
    - stops accepting new runs (as shutdown already does);
    - postpones scheduled firings and job / goal ticks until after the restart;
    - reports what is still in flight: chat runs, swarm runs, async helpers, jobs, ACP sessions, background processes and app sessions.

    It waits with no timeout until the work is done or the owner picks **Stop now**. "Stop now" falls through to the existing 30-second shutdown.
11. **Update flow:**
    - The supervisor checks GitHub Releases at startup and every 6 hours. This can be turned off in config.
    - Updating:
      1. Download the right archive.
      2. Verify the checksum **and** an ed25519 signature made with the owner's release key. The public key is compiled into the binary.
      3. Run the drain gate.
      4. Swap the binary. On Windows this means renaming the running exe first, and the old binary is kept as `anima.old`.
      5. The supervisor re-executes itself on the new version and starts the server.
    - If `/health` doesn't come up, roll back to `anima.old` and report the failure.
12. **macOS signing.** CI signs the macOS binary with a **stable self-signed code-signing identity**: a certificate and key stored as GitHub secrets, with identifier `dev.animaos.anima`. Keychain "Always Allow" then survives updates without a paid Apple account. This is the same idea as `ANIMAOS_DEV_SIGNING_IDENTITY` today. Notarization is not done.
13. **Agents can't control their host.**
    - The data folder is never inside the workspace (enforced by setup and `config set workspace`).
    - Agent file tools can't reach `~/.animaos-swarm/` except through existing approval flows.
    - `anima stop|quit|restart|update|config|autostart|setup` are added to the self-stop / host-control detection in `approvals/detection.rs`, so they always require owner approval.
    - The control token grants lifecycle actions only.
    - Reading the control and admin tokens, `config.toml` or `data/provider-keys.json` needs owner approval on its own. The supervisor passes the server both tokens as file paths (`ANIMA_LOCAL_ADMIN_TOKEN_FILE`, `ANIMA_SUPERVISOR_TOKEN_FILE`), because another process of the same user can read a process's environment.
14. **Who installs it.** v1 targets the owner and other technical users who install with the terminal one-liner. Downloading through a browser and double-clicking may trigger Gatekeeper / SmartScreen warnings until paid signing is added.

## Installed copy and `bun dev` on the same machine

**Owner's decision:** they run **side by side**. The installed app has its own port and its own data folder, and `bun dev` stays on `8080` exactly as today. Telegram bots on the installed copy stay up while the owner develops.

What that means (details are Claude's calls):

- **Port.** The installed copy defaults to **`8787`**. `bun dev` is untouched, so Vite keeps proxying to the dev daemon on `8080`. OAuth callbacks follow the installed copy's port (see Constraints).
- **Data.** The installed copy's data lives only in `~/.animaos-swarm/data/`. The dev daemon keeps using whatever its env says, which is ephemeral by default. No data is shared, so a newer dev build can never migrate the installed copy's files.
- **Keychain.** Keychain item names get a namespace taken from config. The installed copy uses e.g. `anima.installed.…` and the dev daemon keeps today's names. The two never refresh or overwrite each other's logins (e.g. the ChatGPT subscription item). The `anima-daemon` library gains this as a config value, and its default leaves today's names unchanged.
- **Same Telegram bot in both** is not supported. If a bot gets Telegram's "conflict: another poller" error, the connector shows a clear message ("this bot is already connected somewhere else") instead of retrying silently.
- **Workspace.** The two may point at the same workspace folder. That's the owner's choice in `anima setup`, and nothing prevents it.

## Left to the builder

- **Crate choices.** Options include `tray-icon` + `tao`, `rust-embed` / `include_dir`, `self_update` + `zipsign` or `axoupdater` plus separate signature verification, `self-replace`, and `directories`. Any choice is fine if it meets the requirements above and the workspace lints.
- **Windows console handling.** Background and double-click launches must show no console window, while `anima status` etc. in a terminal still print. Approaches include the GUI subsystem with console re-attach, or a detached relaunch.
- **Module layout** inside `hosts/anima`, and the exact API added to the `anima-daemon` library for config-from-struct and drain.
- **Endpoint shapes** for System status / lifecycle / update, and how the System section fits the existing settings UI.
- **A headless Linux build.** Add it if it falls out of cargo-dist cheaply. No tray, and a systemd user unit for autostart.
- **Release key handling.** Generating the ed25519 key, storing the private key as a GitHub secret, and documenting how the owner cuts a release (`git tag v0.2.0 && git push --tags`, or a release-plz style flow).
- **Nx wiring** for `hosts/anima` (`build`, `test`, `lint` targets), consistent with `rust-daemon`.
- **Tests.** Cover:
  - config precedence;
  - packaged storage defaults and folder permissions;
  - workspace validation;
  - update signature rejection and rollback;
  - the drain gate;
  - origin allowance for the embedded console;
  - control-token auth;
  - host-control approval detection.

  Verify with `bun x nx run rust-daemon:test --skipNxCache` plus the new package's tests, per AGENTS.md.
- **Contributor docs page** covering install, commands and the release process.
