# Packaged Anima Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Anima as one installable `anima` program. It serves the daemon and the web console on localhost, runs from login under a tray-owning supervisor, and updates itself from signed GitHub Releases.

**Architecture:** A new host package, `hosts/anima`, wraps the existing `anima-daemon` library.
- `anima supervise` runs the supervisor (tray, control API, autostart, updater). It starts `anima run` as a child: the daemon plus the embedded console, configured through env vars that the supervisor computes from `~/.animaos-swarm/config.toml`.
- The daemon library gains a few things:
  - library entry points;
  - a UI fallback hook;
  - a keychain namespace;
  - a drain / shutdown API;
  - a relay to the supervisor for the console;
  - host-control approval detection.
- cargo-dist builds tagged releases. A follow-up job adds minisign signatures, which the updater verifies.

**Tech Stack:**
- Rust (edition 2021), axum 0.8, tokio, reqwest (rustls), process-wrap.
- New crates: `rust-embed`, `toml`, `clap`, `dialoguer`, `tray-icon`, `tao`, `minisign-verify`, `self-replace`, `semver`, `flate2` + `tar`, `zip`, and `winreg` (Windows only).
- cargo-dist (`dist`), GitHub Actions, minisign.
- Web: React + vitest, `@animaOS-SWARM/sdk`.

**Spec:** `docs/superpowers/specs/2026-10-19-packaged-daemon-design.md`

## Global Constraints

- No `unsafe` in our code. `hosts/anima/Cargo.toml` has `[lints] workspace = true` (`unsafe_code = "forbid"`). Platform glue comes only from crates.
- Edition 2021, using `version.workspace` / `edition.workspace` / `license.workspace` / `authors.workspace`.
- `anima-core` and `packages/core-rust/*` get no new dependencies. All new code lives in `hosts/anima`, or in `hosts/rust-daemon` where it's a daemon feature.
- **Unchanged:** the `anima-daemon` binary, `hosts/rust-daemon/Dockerfile`, `tools/workspace-dev` and `bun dev --host rust`. They behave exactly as today, and each new daemon env var is a no-op when unset.
- **Default ports:** server `8787`, supervisor control `8788`. `bun dev` stays on `8080`.
- **Per-user folder** `~/.animaos-swarm/` (Windows: `%USERPROFILE%\.animaos-swarm\`):
  - `config.toml`, `control-token`, `admin-token`
  - `data/`, `logs/`, `run/`
  - Directories are `0700` and files `0600` on Unix.
- **Data files**, all in `data/`:
  - `control-plane.json`, `history.sqlite`, `provider-keys.json`
  - `memory.sqlite`, `embeddings.sqlite`, `embeddings-cache/`
  - `checkpoints/` (Windows only)
- **Precedence:** process env var > `config.toml` > packaged default.
- Keychain namespace for the installed copy: `installed`. Service names become `installed.<existing service>`.
- macOS code-signing identifier: `dev.animaos.anima`. LaunchAgent label and file: `dev.animaos.anima` at `~/Library/LaunchAgents/dev.animaos.anima.plist`.
- Windows autostart: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `AnimaOS`.
- v1 release targets: `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. The release trigger is a `v*` tag.
- Update check: at supervisor start, then every 6 hours, unless `[updates] check = false`.
- Workspace rules: never the home folder itself, never a folder that contains `~/.animaos-swarm/`, never a folder inside `~/.animaos-swarm/`. Suggested default `~/anima-workspace`.
- **Console copy:**
  - while draining: "Waiting for N tasks to finish"
  - stopped: "Anima is stopped. Start it from the tray or run `anima start`."
  - Telegram conflict: "This bot is already connected somewhere else."
- Verification:
  - `bun x nx run rust-daemon:test --skipNxCache`
  - `bun x nx run anima:test --skipNxCache`
  - `bun x nx run @animaOS-SWARM/web:test --skipNxCache`
  - `bun x nx run @animaOS-SWARM/sdk:test --skipNxCache`
  - On Windows, use the `CI=1 CARGO_TARGET_DIR=…` fallback from AGENTS.md.

## Review Focus

1. **A port is already taken.** This can be 8787 held by another program, or a second `anima supervise`.
   - A second supervisor prints "Anima is already running" and exits 0.
   - A server that can't bind shows "Port 8787 is in use", with capped backoff and no tight crash loop.
   - Pinned in Task 8.
2. **A release exists but its signature or platform asset isn't uploaded yet.** No update is offered, and no error is shown. Pinned in Task 15.
3. **The workspace folder was deleted, moved or unmounted after setup.** The server still starts, and status / console show "Workspace folder not found: <path>". Pinned in Tasks 3 and 8.
4. **Agent shells must not inherit the tokens.** `ANIMA_SUPERVISOR_TOKEN` and `ANIMA_LOCAL_ADMIN_TOKEN` never reach agent shells. Pinned in Task 7.
5. **Two actions at once.** Examples: Update clicked twice, or Stop while an update is draining. One pending action at a time; a second gets 409 naming the pending one, and Cancel clears it. Pinned in Task 8.

---

## Milestone A: headless packaged server (`anima run`)

### Task 1: Daemon library entry points and UI fallback hook

**Files:**
- Modify: `hosts/rust-daemon/src/main.rs` (move env parsing out)
- Create: `hosts/rust-daemon/src/app/entry.rs`
- Modify: `hosts/rust-daemon/src/app.rs` (`serve` → `serve_with`), `hosts/rust-daemon/src/lib.rs` (exports)
- Test: `hosts/rust-daemon/src/app/entry_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn config_from_env() -> io::Result<DaemonConfig>`: today's `main.rs` parsing, moved verbatim with its tests.
  - `pub fn bind_addr_from_env() -> String`: `ANIMAOS_RS_HOST` default `127.0.0.1`, `ANIMAOS_RS_PORT` default `8080`.
  - `pub struct ServeOptions { pub ui: Option<axum::Router>, pub shutdown: Option<Arc<tokio::sync::Notify>> }` (`Default`).
  - `pub async fn serve_with(listener: TcpListener, config: DaemonConfig, options: ServeOptions) -> io::Result<()>`. `serve(l, c)` becomes `serve_with(l, c, ServeOptions::default())`.
  - `pub async fn run_from_env(options: ServeOptions) -> io::Result<()>`: init tracing, parse config, bind, log the startup line, then `serve_with`.
- **Semantics:**
  - `ui` is installed as the router's fallback, so every daemon route wins over the UI.
  - Shutdown begins on Ctrl+C / SIGTERM **or** when `shutdown` is notified.

- [ ] **Step 1: Write the failing tests.**
  - `ui_fallback_serves_unmatched_paths`: with `ui = Router::new().fallback(|| async { "UI" })`, `GET /` → 200 `"UI"`, and `GET /health` still returns the daemon's health JSON.
  - `shutdown_notify_stops_server`: notify → `serve_with` returns `Ok(())` within 5 s.

  Use the existing `serve_with_state` test helpers' pattern for binding on `127.0.0.1:0`.
- [ ] **Step 2: Run them to make sure they fail.** Run `cargo test -p anima-daemon entry_tests`. Expected: compile errors for `serve_with` / `ServeOptions`.
- [ ] **Step 3: Implement.**
  - Move the env parsing into `app/entry.rs`.
  - Thread `ui` into `router_with_runtime`'s final router via `.fallback_service`.
  - Race `lifecycle::shutdown_signal()` with `notify.notified()`.
  - `main.rs` becomes the document-worker check, then `run_from_env(ServeOptions::default())`.
- [ ] **Step 4: Run `bun x nx run rust-daemon:test --skipNxCache`.** Expected: all PASS, including the moved `main.rs` tests.
- [ ] **Step 5: Commit** `feat(daemon): library entry points and UI fallback hook`.

### Task 2: Keychain namespace

**Files:**
- Create: `hosts/rust-daemon/src/keychain.rs`
- Modify: the seven keyring call sites:
  - `chatgpt_auth.rs:65`
  - `connectors/oauth_apps.rs:11`
  - `connectors/credentials.rs:9`
  - `connectors/discord/credentials.rs:12`
  - `connectors/gcalendar/store.rs:15`
  - `connectors/drive/store.rs:11`
  - `mcp/credentials.rs:12`
- Test: inline `#[cfg(test)] mod tests` in `keychain.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn service_name(base: &'static str) -> Cow<'static, str>`. It reads `ANIMAOS_RS_KEYCHAIN_NAMESPACE` once (a `OnceLock`) and returns `"{ns}.{base}"` when set.
  - `pub(crate) fn validate_namespace(raw: &str) -> Result<(), &'static str>`. Allowed: `[a-z0-9-]{1,32}`.
  - Startup (`run_from_env`) fails with `ANIMAOS_RS_KEYCHAIN_NAMESPACE must be 1-32 of a-z, 0-9, -` on an invalid value.

- [ ] **Step 1: Write the failing tests.**
  - `service_name_without_namespace_is_unchanged`: `service_name_with(None, "animaos.mcp.bearer") == "animaos.mcp.bearer"`.
  - `service_name_with_namespace_prefixes`: `service_name_with(Some("installed"), "anima.chatgpt.subscription.v1") == "installed.anima.chatgpt.subscription.v1"`.
  - `invalid_namespaces_rejected`: `""`, `"Installed"`, `"a.b"` and 33 × `"a"` are each `Err`.

  Factor out a pure `service_name_with(ns: Option<&str>, base)` so the tests don't touch env.
- [ ] **Step 2: Run `cargo test -p anima-daemon keychain`.** Expected: FAIL (module missing).
- [ ] **Step 3: Implement**, and route every `keyring::Entry::new(SERVICE, …)` through `service_name(SERVICE)`.
- [ ] **Step 4: Run `bun x nx run rust-daemon:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(daemon): optional keychain namespace`.

### Task 3: `hosts/anima` package, paths and config

**Files:**
- Modify: root `Cargo.toml` (`members += "hosts/anima"`)
- Create: `hosts/anima/Cargo.toml` (package `anima`, bin `anima`, `[lints] workspace = true`, features `bundled-ui = ["dep:rust-embed"]`)
- Create: `hosts/anima/project.json`. Nx project `anima` with `build` (`cargo build -p anima`), `test` (`cargo test -p anima`) and `lint` (`cargo fmt --all --check`); inputs mirror `hosts/rust-daemon/project.json`.
- Create: `hosts/anima/src/main.rs`, `src/paths.rs`, `src/config.rs`, `src/server_env.rs`
- Test: inline module tests in each file

**Interfaces:**
- Produces:
  - `pub struct AnimaPaths { pub root, pub config_file, pub control_token, pub admin_token, pub data, pub logs, pub run }` (all `PathBuf`), with:
    - `AnimaPaths::from_home(home: &Path) -> Self`
    - `AnimaPaths::discover() -> io::Result<Self>` (`directories::BaseDirs` home)
    - `fn ensure(&self) -> io::Result<()>`: creates the dirs with user-only permissions.
  - `pub struct Config { pub workspace: Option<PathBuf>, pub port: u16, pub control_port: u16, pub autostart: bool, pub updates: UpdatesConfig }` and `pub struct UpdatesConfig { pub check: bool }`. Serde/TOML. Defaults: port 8787, control_port 8788, autostart true, check true.
  - `Config::load(path) -> io::Result<Config>` (a missing file gives defaults) and `Config::save(&self, path) -> io::Result<()>` (atomic write, 0600).
  - `pub enum WorkspaceProblem { IsHome, ContainsDataRoot, InsideDataRoot, Missing(PathBuf) }`
  - `pub fn validate_workspace(ws: &Path, home: &Path, paths: &AnimaPaths) -> Result<(), WorkspaceProblem>`, which canonicalizes where it exists.
  - `pub fn server_env(cfg: &Config, paths: &AnimaPaths, process_env: &BTreeMap<String, String>) -> BTreeMap<String, String>`. It returns only the keys the process env doesn't already set:
    - Bind: `ANIMAOS_RS_HOST=127.0.0.1`, `ANIMAOS_RS_PORT={port}`.
    - `ANIMAOS_WORKSPACE_ROOT` (if configured).
    - Data files: `ANIMAOS_RS_CONTROL_PLANE_FILE`, `ANIMAOS_RS_HISTORY_SQLITE_FILE`, `ANIMAOS_RS_PROVIDER_KEYS_FILE`, `ANIMAOS_RS_MEMORY_SQLITE_FILE`, `ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE`, `ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR` (paths under `data/`).
    - `ANIMAOS_RS_CHECKPOINT_ROOT` (Windows only).
    - `ANIMA_ALLOWED_UI_ORIGINS=http://127.0.0.1:{port},http://localhost:{port}`.
    - `ANIMA_PUBLIC_BASE_URL=http://127.0.0.1:{port}`.
    - `ANIMAOS_RS_KEYCHAIN_NAMESPACE=installed`.
    - `ANIMA_LOCAL_ADMIN_TOKEN` (contents of `admin-token`, if present).

    It skips `ANIMAOS_RS_MEMORY_SQLITE_FILE` when the process env sets `ANIMAOS_RS_MEMORY_FILE`, because the daemon rejects both being set.

  > Note: if the memory-overhaul work (`docs/superpowers/specs/2026-10-19-memory-overhaul-design.md`) has landed and renamed the memory env vars or files, follow the names it uses.

- [ ] **Step 1: Write the failing tests.**
  - `defaults_when_config_missing`: load from a nonexistent path → port 8787, control_port 8788, autostart true, `updates.check` true, workspace None.
  - `config_round_trips_and_is_private`: save then load is equal. On Unix, `mode & 0o777 == 0o600`.
  - `server_env_fills_packaged_defaults`: for a temp home, `ANIMAOS_RS_PORT == "8787"`, `ANIMAOS_RS_CONTROL_PLANE_FILE` ends with `data/control-plane.json`, `ANIMAOS_RS_KEYCHAIN_NAMESPACE == "installed"`, and `ANIMA_PUBLIC_BASE_URL == "http://127.0.0.1:8787"`.
  - `process_env_wins`: with process env `ANIMAOS_RS_PORT=9000`, the result has no `ANIMAOS_RS_PORT` key.
  - `memory_json_env_suppresses_sqlite_default`: process env `ANIMAOS_RS_MEMORY_FILE=x` → no `ANIMAOS_RS_MEMORY_SQLITE_FILE`.
  - `workspace_rules`:
    - home → `IsHome`
    - home's parent → `ContainsDataRoot`
    - `root/data` → `InsideDataRoot`
    - nonexistent → `Missing`
    - `home/anima-workspace` (created) → `Ok`
- [ ] **Step 2: Run `cargo test -p anima`.** Expected: FAIL (types missing).
- [ ] **Step 3: Implement.** `main.rs` stays a stub `fn main() {}` until Task 4.
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS. Run `bun x nx show projects --json` and confirm `anima` is listed.
- [ ] **Step 5: Commit** `feat(anima): package scaffold, paths and config`.

### Task 4: Embedded console and `anima run`

**Files:**
- Create: `hosts/anima/src/ui.rs`, `hosts/anima/src/cli.rs`, `hosts/anima/src/commands/run.rs`
- Modify: `hosts/anima/src/main.rs`, `hosts/anima/Cargo.toml` (depends on `anima-daemon`, `axum`, `tokio`, `clap`)
- Test: `hosts/anima/tests/run_cli.rs`

**Interfaces:**
- Consumes: `anima_daemon::{run_from_env, ServeOptions, document_worker_entry}` (Task 1); `AnimaPaths`, `Config` and `server_env` (Task 3).
- Produces:
  - `pub fn ui_router() -> Option<axum::Router>`.
    - With `bundled-ui`, it is `rust-embed` over `../../apps/web/dist`. Exact path → that file with a guessed content type. `assets/*` gets `Cache-Control: public, max-age=31536000, immutable`. Any other path → `index.html`, `no-cache`.
    - Without the feature: `None`.
  - `enum Command` (clap). The variants are listed here so later tasks share one parser: `Run`, `Supervise { background: bool, no_tray: bool }`, `Setup { yes: bool, workspace: Option<PathBuf> }`, `Status`, `Start`, `Stop { now: bool }`, `Restart { now: bool }`, `Quit { now: bool }`, `Logs { follow: bool }`, `Update { now: bool }`, `Autostart { on_off: OnOff }`, `Config { get / set }`, and no subcommand → `Open`. Unimplemented variants print `not yet available` and exit 2 until their task lands.
  - `commands::run::run() -> io::Result<()>`:
    1. Load config.
    2. For each key in `server_env`, call `std::env::set_var` **before** building the tokio runtime.
    3. `run_from_env(ServeOptions { ui: ui_router(), shutdown: None })`.
- `main.rs` checks `--document-worker-v2` first, exactly as `hosts/rust-daemon/src/main.rs` does, so worker re-exec works under the `anima` binary.

- [ ] **Step 1: Write the failing integration test** `anima_run_serves_health_with_temp_home`:
  - Spawn `env!("CARGO_BIN_EXE_anima") run` with `HOME` / `USERPROFILE` set to a tempdir, `ANIMAOS_RS_PORT` set to a free port, and the `ANIMAOS_RS_KEYCHAIN_NAMESPACE=test` env.
  - Poll `GET /health` until 200 (20 s cap).
  - Assert that the child's startup log line contains `json:` followed by a path ending in `.animaos-swarm/data/control-plane.json`. That is the control-plane store label `run_from_env` logs.
  - Kill the child.
- [ ] **Step 2: Write the failing unit test** `ui_router_none_without_feature`, `#[cfg(not(feature = "bundled-ui"))]` → `ui_router().is_none()`.
- [ ] **Step 3: Run `cargo test -p anima`.** Expected: FAIL.
- [ ] **Step 4: Implement** `ui.rs`, `cli.rs` and `commands/run.rs`.
- [ ] **Step 5: Run `cargo test -p anima`.** Expected: PASS.
- [ ] **Step 6: Bundled check.**
  1. Run `bun x nx run @animaOS-SWARM/web:build`, then `cargo build -p anima --features bundled-ui`.
  2. Run the binary with a temp HOME and confirm that `curl -s http://127.0.0.1:8787/ | grep -c '<div id="root"'` prints `1`.
  3. Add a `#[cfg(feature = "bundled-ui")]` test `ui_router_falls_back_to_index`: `GET /some/deep/path` → 200 with a body containing `<div id="root"`.
- [ ] **Step 7: Commit** `feat(anima): embedded console and anima run`.

---

## Milestone B: supervisor, controls, tray, setup

### Task 5: Daemon drain and shutdown API

**Files:**
- Create: `hosts/rust-daemon/src/system/mod.rs`, `src/system/drain.rs`, `hosts/rust-daemon/src/routes/system.rs`
- Modify:
  - `src/agent_runs/shutdown.rs` (draining flag next to `closing`)
  - `src/agent_runs.rs`, `src/jobs.rs`, `src/schedules/*`, `src/state/swarm_runtime.rs`, `src/tools/process.rs`, `src/tools/process/live.rs`: one in-flight count accessor each
  - `src/routes/mod.rs` (routes)
  - `src/app.rs` (hold a `DrainController` in runtime; take `ServeOptions.shutdown`)
- Test: `hosts/rust-daemon/src/system/drain_tests.rs`, `src/routes/system_tests.rs`

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)] #[serde(rename_all = "camelCase")] pub struct InFlight { pub chat_runs: u32, pub queued_runs: u32, pub swarm_runs: u32, pub helper_runs: u32, pub jobs: u32, pub background_processes: u32, pub app_sessions: u32, pub acp_sessions: u32 }` with `pub fn total(&self) -> u32`. It is re-exported as `anima_daemon::system::InFlight`.
  - `pub(crate) struct DrainController` with:
    - `begin() -> InFlight`
    - `cancel()`
    - `is_draining() -> bool`
    - `in_flight() -> InFlight`
    - `request_shutdown()`, which notifies the `ServeOptions.shutdown` `Notify` or an internal one.
  - Routes, all behind `state.local_owner.authorize` (reads behind `authorize_read`):
    - `POST /api/system/drain` → 202 `{ "draining": true, "inFlight": {…} }`
    - `GET /api/system/drain` → 200 `{ "draining": bool, "inFlight": {…} }`
    - `DELETE /api/system/drain` → 204
    - `POST /api/system/shutdown` → 202, then graceful shutdown (the existing path)
- **Semantics:**
  - While draining, new chat messages, swarm starts and job / goal admissions are refused with 503 and the message `Anima is restarting; send the message again in a moment`.
  - Queued runs keep running to completion. The draining flag must **not** set the existing `closing` flag.
  - Scheduler firings due during the drain are deferred, not dropped. They fire after restart via the scheduler's existing catch-up, or stay pending on cancel.
  - Cancel restores normal admission.

- [ ] **Step 1: Write the failing tests.**
  - `drain_refuses_new_messages_but_finishes_queued`: one going run and one queued run. `begin()` → a new message gets 503 with the copy above. Release the going run → the queued run starts and completes. `in_flight().total()` reaches 0.
  - `cancel_restores_admission`: `begin()`, `cancel()` → a new message is accepted.
  - `drain_counts_background_processes`: start a `bg_start` job → `in_flight().background_processes == 1`.
  - `drain_routes_require_local_owner`: no Origin and no admin token → 403. Allowed Origin → 202.
  - `shutdown_route_stops_server`: `POST /api/system/shutdown` → the server future resolves within `SHUTDOWN_BUDGET`.
- [ ] **Step 2: Run `cargo test -p anima-daemon drain`.** Expected: FAIL.
- [ ] **Step 3: Implement** the draining flag and per-subsystem counters, then `DrainController` and the routes.
- [ ] **Step 4: Run `bun x nx run rust-daemon:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(daemon): drain and shutdown API`.

### Task 6: Daemon `/api/system` relay to the supervisor

**Files:**
- Modify: `hosts/rust-daemon/src/routes/system.rs`, `hosts/rust-daemon/src/system/mod.rs`
- Test: `hosts/rust-daemon/src/routes/system_tests.rs`

**Interfaces:**
- Consumes: env `ANIMA_SUPERVISOR_URL` (e.g. `http://127.0.0.1:8788`) and `ANIMA_SUPERVISOR_TOKEN`, both read once at startup.
- Produces:
  - `GET /api/system` (`authorize_read`):
    - With a supervisor configured, it proxies `GET {url}/v1/status` with `Authorization: Bearer {token}` and returns the body unchanged.
    - Otherwise it returns `{ "supervised": false, "version": env!("CARGO_PKG_VERSION") }`.
  - `POST /api/system/actions/{action}` (`authorize`), where `action ∈ start|stop|restart|quit|update|check-update|cancel`. The body `{ "force": bool }` is optional. It proxies to `POST {url}/v1/actions/{action}` and passes through status and body.
    - Unknown action → 404.
    - No supervisor → 409 `{"error":"Anima is not running under the supervisor"}`.
  - Supervisor unreachable → 502 `{"error":"The Anima supervisor is not responding"}`.

- [ ] **Step 1: Write the failing tests** against a fake supervisor (an axum app on `127.0.0.1:0` that records the requests it receives):
  - `status_proxies_with_bearer`
  - `status_unsupervised_reports_version`
  - `action_proxied_with_force`
  - `unknown_action_404`
  - `action_requires_owner`: a disallowed Origin → 403.
  - `supervisor_down_502`
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement** using the existing `reqwest` client (5 s timeout).
- [ ] **Step 4: Run `bun x nx run rust-daemon:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(daemon): relay console system actions to the supervisor`.

### Task 7: Host-control approval detection and token isolation

**Files:**
- Modify: `hosts/rust-daemon/src/approvals/detection.rs` (`hardline`, next to the `pkill`/`launchctl` arms near line 523)
- Test: the existing detection test module in that file; `hosts/rust-daemon/src/tools/process/shell.rs` tests

**Interfaces:**
- Produces:
  - A `hardline` arm. Program `anima` (or `anima.exe`) whose first non-flag word is one of `stop|quit|restart|update|config|autostart|setup|start|supervise` → `Some("controls anima itself")`.
  - `launchctl … dev.animaos.anima` is already covered by the existing arm. Extend it to `kickstart` as well as `bootout|unload`.
  - `reg … CurrentVersion\Run … AnimaOS` → `Some("controls anima itself")`.

- [ ] **Step 1: Write the failing tests.**
  - `hardline` flags `anima stop`, `anima update --now`, `/usr/local/bin/anima quit`, `anima.exe autostart off` and `launchctl kickstart -k gui/501/dev.animaos.anima`.
  - It does **not** flag `anima status`, `anima logs` or `animate`.
  - `shell_env_excludes_anima_tokens`: with `ANIMA_SUPERVISOR_TOKEN` and `ANIMA_LOCAL_ADMIN_TOKEN` set in the daemon process env, the built shell env contains neither key. Assert on the function that builds the env from `SHELL_ENV_ALLOWLIST`.
- [ ] **Step 2: Run them.** Expected: the detection tests FAIL. The env test may already pass; keep it as a regression pin.
- [ ] **Step 3: Implement** the arms.
- [ ] **Step 4: Run `bun x nx run rust-daemon:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(daemon): treat anima lifecycle commands as host control`.

### Task 8: Supervisor core (child lifecycle, gate, control API)

**Files:**
- Create: `hosts/anima/src/supervisor/mod.rs`, `state.rs`, `child.rs`, `control_api.rs`, `tokens.rs`, `logs.rs`
- Modify: `hosts/anima/src/cli.rs` (`Supervise` without tray: `--no-tray`)
- Test: `hosts/anima/src/supervisor/state_tests.rs`, `hosts/anima/tests/supervisor.rs`

**Interfaces:**
- Consumes:
  - `server_env` and `validate_workspace` (Task 3).
  - The server's `POST/GET/DELETE /api/system/drain` and `POST /api/system/shutdown`, called with `Authorization: Bearer <admin-token>` (Task 5).
  - `InFlight` (Task 5).
- Produces:
  - `pub enum ServerState { Starting, Running, Draining { action: Action, in_flight: InFlight }, Stopping, Stopped, Crashed { message: String, retry_in_secs: u64 }, Updating { stage: String } }` (serde, `tag = "kind"`, camelCase).
  - `pub enum Action { Start, Stop, Restart, Quit, Update, CheckUpdate, Cancel }`.
  - `pub struct SupervisorStatus { pub version: String, pub state: ServerState, pub server_url: String, pub workspace: Option<PathBuf>, pub workspace_problem: Option<String>, pub data_dir: PathBuf, pub autostart: bool, pub connectors: Option<u32>, pub pending: Option<Action>, pub update: UpdateStatus }` (camelCase JSON).
  - `pub struct UpdateStatus { pub available: Option<String>, pub last_error: Option<String> }`.
  - `pub fn next(state: &ServerState, pending: Option<Action>, requested: Action, force: bool) -> Result<Transition, Conflict>`. This is the pure transition function. `Conflict { pending: Action }` maps to HTTP 409.
  - Control API on `127.0.0.1:{control_port}`. Every route requires `Authorization: Bearer <control-token>`; otherwise 401.
    - `GET /v1/status` → `SupervisorStatus`.
    - `POST /v1/actions/{action}` with body `{ force?: bool }` → 202 `SupervisorStatus`, or 409 `{"error":"Another action is in progress","pending":"<action>"}`.
  - `tokens::ensure(paths) -> io::Result<(ControlToken, AdminToken)>`: 32 random bytes, hex, `0600`, created if missing.
  - The child is `current_exe() run`:
    - Env = `server_env(...)` + `ANIMA_SUPERVISOR_URL` + `ANIMA_SUPERVISOR_TOKEN`.
    - Stdout/stderr go to `logs/anima.log`, rotated at 10 MiB, keeping 3 files.
    - Spawned through `process-wrap`: a job object on Windows plus `CREATE_NO_WINDOW`, and a process group on Unix.
  - **Readiness:** `GET /health` is 200 within 60 s. Otherwise the state is `Crashed`.
  - **Crash backoff:** 1, 2, 4, 8, 16, 30 s (capped). It resets after 10 min of `Running`.
  - **Port in use:** if the child's log tail contains `Address already in use` / `os error 10048`, the state is `Crashed { message: "Port {port} is in use" }`.
  - **Gate:** for Stop / Restart / Quit / Update:
    1. Drain, polling every 1 s into `Draining.in_flight`.
    2. When `total() == 0` or `force`, call shutdown and wait for the child to exit (40 s, then kill).
    3. Then: Stop → `Stopped`; Restart → spawn again; Quit → exit 0; Update → hand off to the updater (Task 16).
    4. Cancel → `DELETE` drain and return to `Running`.
  - **Singleton:** binding `control_port` fails → print `Anima is already running` and exit 0.
  - **Workspace problem:** a `validate_workspace` → `Missing` sets `workspace_problem = "Workspace folder not found: <path>"`. The server still starts.

- [ ] **Step 1: Write the failing pure tests** (`state_tests.rs`):
  - `stop_while_running_begins_drain`
  - `second_action_while_pending_conflicts`: pending Update + requested Stop → `Err(Conflict { pending: Update })`.
  - `cancel_clears_pending`
  - `force_skips_waiting`
  - `start_when_stopped_spawns`
  - `quit_from_stopped_exits`
  - `backoff_sequence_caps_at_30`
- [ ] **Step 2: Write the failing integration tests** (`tests/supervisor.rs`, temp HOME, free ports, `--no-tray`):
  - `supervise_starts_child_and_reports_running`: poll `/v1/status` until `state.kind == "running"`.
  - `missing_token_is_401`.
  - `second_supervisor_exits_already_running`: exit code 0, stdout contains `Anima is already running`.
  - `stop_then_start`: POST stop → `stopped`, and the server port refuses connections. POST start → `running`.
  - `server_port_in_use_reports_crash`: occupy the server port with a `TcpListener` → status is `crashed` with message `Port {port} is in use`, and two consecutive polls 1 s apart show non-decreasing `retryInSecs`.
  - `missing_workspace_still_runs`: config workspace → a nonexistent path → `running` with `workspaceProblem` starting `Workspace folder not found`.
- [ ] **Step 3: Run `cargo test -p anima supervisor`.** Expected: FAIL.
- [ ] **Step 4: Implement** `state.rs` (pure), then `tokens.rs`, `logs.rs`, `child.rs` and `control_api.rs`, then the `mod.rs` loop.
- [ ] **Step 5: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 6: Commit** `feat(anima): supervisor with drain gate and control API`.

### Task 9: CLI client commands and `anima` (open)

**Files:**
- Create: `hosts/anima/src/client.rs`, `hosts/anima/src/commands/{status,lifecycle,logs,config_cmd,open}.rs`
- Test: `hosts/anima/tests/cli_client.rs`

**Interfaces:**
- Consumes: the control API and `SupervisorStatus` (Task 8); `Config` (Task 3).
- Produces:
  - `client::SupervisorClient::connect(paths, cfg) -> io::Result<Option<Self>>`. It returns `None` if `control_port` refuses.
  - `client::SupervisorClient` methods: `status()` and `action(Action, force)`.
  - **Commands:**
    - `status`: prints `Running · v{version} · http://127.0.0.1:{port} · {n} connectors`, then `Workspace: …` and `Update available: v…` when set. Not running → `Anima is not running` and exit 3.
    - `start`:
      - If no supervisor: when the LaunchAgent is installed (macOS), `launchctl kickstart gui/{uid}/dev.animaos.anima`; otherwise spawn `current_exe() supervise --background` detached (Windows: `DETACHED_PROCESS | CREATE_NO_WINDOW` via `CommandExt::creation_flags`).
      - Then wait ≤ 20 s for `running`.
      - If a supervisor is up and the state is `stopped`, send `Start`.
    - `stop` / `restart` / `quit` / `update`: send the action with `force = --now`. Print `Waiting for N tasks to finish…`, refreshed every 1 s until done, or the 409 message.
    - `logs [-f]`: the last 200 lines of `logs/anima.log`; `-f` follows.
    - `config get [key]` / `config set <key> <value>`:
      - Keys: `workspace` (validated with `validate_workspace`; refused with the problem text), `port`, `control-port`, `updates.check`.
      - Prints `Restart Anima for this to take effect` when running.
    - `Open` (no subcommand): `start` if needed, then open `http://127.0.0.1:{port}/` with the `open` crate.
  - **Windows double-click:** when `Open` runs and the parent process is `explorer.exe` (checked with the `sysinfo` crate), it relaunches itself detached and exits, so no console window stays open.

- [ ] **Step 1: Write the failing tests** (spawn a supervisor `--no-tray` with a temp HOME):
  - `status_prints_running_line`: stdout starts with `Running · v`.
  - `status_not_running_exit_3`
  - `stop_command_reaches_stopped`
  - `config_set_workspace_rejects_home`: exit ≠ 0, stderr contains the `IsHome` text.
  - `config_set_port_persists`
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(anima): CLI lifecycle, status, logs and config commands`.

### Task 10: Autostart

**Files:**
- Create: `hosts/anima/src/autostart/{mod,macos,windows}.rs`, `hosts/anima/src/commands/autostart.rs`
- Test: inline tests in `macos.rs`; `#[cfg(windows)]` tests in `windows.rs`

**Interfaces:**
- Produces:
  - `pub trait Autostart { fn enable(&self, exe: &Path) -> io::Result<()>; fn disable(&self) -> io::Result<()>; fn is_enabled(&self) -> io::Result<bool>; }` and `pub fn platform(paths: &AnimaPaths) -> Box<dyn Autostart>`.
  - `macos::plist(exe: &Path, logs: &Path) -> String`. Label `dev.animaos.anima`; ProgramArguments `[exe, "supervise", "--background"]`; `RunAtLoad` true; `KeepAlive` `<dict><key>SuccessfulExit</key><false/></dict>`; `StandardOutPath` / `StandardErrorPath` = `logs/supervisor.log`.
    - `enable` writes the plist and runs `launchctl bootstrap gui/{uid} <plist>`, treating "already loaded" as success.
    - `disable` runs `launchctl bootout gui/{uid}/dev.animaos.anima` and removes the file.
  - `windows`: the `winreg` value `AnimaOS` under the HKCU Run key = `"\"{exe}\" supervise --background"`.
  - `anima autostart on|off` toggles it and saves `config.autostart`. The supervisor exposes it in status.

- [ ] **Step 1: Write the failing tests.**
  - `plist_contains_label_args_and_keepalive`: the parsed plist (`plist` crate) has Label `dev.animaos.anima`, args `[exe,"supervise","--background"]`, and `KeepAlive.SuccessfulExit == false`.
  - Windows: `enable_then_is_enabled_then_disable`, using a test-only key path override.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS. Manually on macOS: `anima autostart on`, then `launchctl print gui/$(id -u)/dev.animaos.anima | grep state` shows `running`.
- [ ] **Step 5: Commit** `feat(anima): start at login on macOS and Windows`.

### Task 11: Tray / menu-bar icon

**Files:**
- Create: `hosts/anima/src/tray/{mod,menu}.rs`, `hosts/anima/assets/tray/{running,starting,stopped,waiting,update}.png` (16×16 and 32×32 @2x template images on macOS)
- Modify: `hosts/anima/src/supervisor/mod.rs`. `supervise` runs the tao event loop on the main thread, and the supervisor loop on a tokio runtime in a background thread. `--no-tray` / `--background` without a desktop session skips the tray.
- Test: `hosts/anima/src/tray/menu_tests.rs`

**Interfaces:**
- Consumes: `SupervisorStatus`, `Action` (Task 8); `Autostart` (Task 10).
- Produces:
  - `pub fn menu_model(status: &SupervisorStatus) -> Vec<MenuItemModel>`, where `MenuItemModel { id: &'static str, label: String, enabled: bool, checked: Option<bool> }`. In order:
    1. `open` "Open Console" (disabled unless `running` / `draining`)
    2. `status` (disabled), e.g. `Running · v0.2.0 · 2 connectors`, or `Waiting for N tasks to finish`, `Stopped`, `Port 8787 is in use`
    3. separator
    4. `start` "Start" (only when stopped / crashed) **or** `stop` "Stop"
    5. `restart` "Restart"
    6. `update` "Update to vX.Y.Z" when `update.available`, else `check` "Check for updates"
    7. `cancel` "Cancel" (only when `pending` is some)
    8. separator
    9. `autostart` "Start at login" (checked = `autostart`)
    10. `quit` "Quit"
  - `pub fn icon_for(state: &ServerState, update_available: bool) -> TrayIconKind`.
  - On macOS the event loop uses `ActivationPolicy::Accessory`, so there's no Dock icon. The menu rebuilds when the status changes.

- [ ] **Step 1: Write the failing tests.**
  - `running_menu_items`: ids in order are `open,status,sep,stop,restart,check,sep,autostart,quit`.
  - `stopped_shows_start_and_disables_open`
  - `update_available_shows_update_label`: label `Update to v0.2.1`.
  - `draining_shows_waiting_and_cancel`: status label `Waiting for 3 tasks to finish`, and `cancel` is present.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement** `menu.rs` (pure), then wire `tray-icon` / `tao` in `mod.rs`.
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS. Manually on macOS, `anima supervise`: the icon appears in the menu bar with no Dock icon, Stop / Start work, and Quit ends both processes (`pgrep -f "anima run"` is empty).
- [ ] **Step 5: Commit** `feat(anima): tray and menu-bar icon`.

### Task 12: `anima setup`

**Files:**
- Create: `hosts/anima/src/commands/setup.rs`
- Modify: `hosts/rust-daemon/src/lib.rs` (export `pub fn provider_env_key_names() -> Vec<&'static str>`, which wraps `runtime_model::provider_env_keys()`)
- Test: `hosts/anima/tests/setup.rs`

**Interfaces:**
- Consumes: `AnimaPaths`, `Config`, `validate_workspace`, `tokens::ensure`, `Autostart`, and the `start` / `Open` commands. Also the existing `POST /api/provider-keys/{provider}` with body `{ "label": "Imported from shell", "key": "…" }` and `Authorization: Bearer <admin-token>`.
- Produces: `anima setup [--yes] [--workspace <path>]`.
  - **Prompts** (`dialoguer`), skipped with `--yes` (defaults or flags):
    1. Workspace: default `~/anima-workspace`, created if missing. It re-asks on a `WorkspaceProblem`; with `--yes`, it fails with exit 2.
    2. Port: default 8787.
    3. Start at login: default yes.
    4. For each provider env key in the process env (the list `anima_daemon::provider_env_key_names()`, a new `pub fn` re-exporting `runtime_model::provider_env_keys()` names): "Import OPENAI_API_KEY into Anima?" (default yes; `--yes` imports all).
  - **Then:**
    1. `paths.ensure()` and `tokens::ensure`.
    2. Save the config.
    3. Enable or disable autostart.
    4. `start`.
    5. Import the keys over HTTP.
    6. `Open`.
  - Re-running keeps the existing tokens and offers the current values as defaults.

- [ ] **Step 1: Write the failing tests.**
  - `setup_yes_writes_config_and_tokens`: temp HOME, `--yes --workspace <tmp>/ws`, `ANIMA_SETUP_NO_AUTOSTART=1` and `ANIMA_SETUP_NO_OPEN=1` (test-only env switches). Then `config.toml` has the workspace, port 8787, and `control-token` / `admin-token` exist with mode 0600.
  - `setup_yes_rejects_home_workspace`: exit 2.
  - `setup_imports_provider_key`: process env `OPENAI_API_KEY=sk-test-123` → after setup, `GET /api/provider-keys` with the admin bearer lists an `openai` key labelled `Imported from shell`.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache` and `bun x nx run rust-daemon:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(anima): interactive setup`.

### Task 13: Console "Anima app" card

**Files:**
- Create: `packages/sdk/src/system.ts`, `packages/sdk/src/system.spec.ts`
- Modify: `packages/sdk/src/client.ts` (`readonly system: SystemClient`), `packages/sdk/src/index.ts`
- Create: `apps/web/src/components/AnimaAppCard.tsx`, `apps/web/src/components/AnimaAppCard.test.tsx`
- Modify: `apps/web/src/pages/HealthPage.tsx` (render the card at the top), `apps/web/src/system.css`

**Interfaces:**
- Consumes: `GET /api/system` and `POST /api/system/actions/{action}` (Task 6); `SupervisorStatus` JSON (Task 8).
- Produces:
  - SDK:
    - `export type SystemStatus = { supervised: false; version: string } | ({ supervised?: true } & SupervisorStatusJson)`.
    - `SystemClient.status(): Promise<SystemStatus>`.
    - `SystemClient.action(action: 'start'|'stop'|'restart'|'quit'|'update'|'check-update'|'cancel', opts?: { force?: boolean }): Promise<SystemStatus>`.
  - `AnimaAppCard`:
    - It polls `status()` every 2 s and shows the version, state, workspace (plus the `workspaceProblem` warning), data folder and autostart.
    - Buttons:
      - **Update to vX** when available, otherwise **Check for updates**;
      - **Restart**;
      - **Stop**.
    - While draining it shows `Waiting for N tasks to finish`, the non-zero `inFlight` categories, and **Stop now** (`force: true`) and **Cancel**.
    - After a Stop succeeds, or when status fetches fail after a Stop / Quit, it shows `Anima is stopped. Start it from the tray or run \`anima start\`.`
    - With `supervised: false`, it shows only `v{version} (running without the Anima app)`.

- [ ] **Step 1: Write the failing SDK tests** (`system.spec.ts`): `status` GETs `/api/system`; `action('stop', { force: true })` POSTs `/api/system/actions/stop` with `{"force":true}`.
- [ ] **Step 2: Write the failing component tests** (`AnimaAppCard.test.tsx`, mocked client):
  - running → the Restart / Stop buttons are visible;
  - draining with `inFlight.chatRuns=2, jobs=1` → the text `Waiting for 3 tasks to finish`;
  - clicking **Stop now** calls `action('stop', { force: true })`;
  - after Stop → the stopped copy is exact;
  - unsupervised → no buttons.
- [ ] **Step 3: Run `bun x nx run @animaOS-SWARM/sdk:test --skipNxCache` and `bun x nx run @animaOS-SWARM/web:test --skipNxCache`.** Expected: FAIL.
- [ ] **Step 4: Implement**, following the `chatgpt.ts` client pattern and the HealthPage styling in `system.css`.
- [ ] **Step 5: Run both test targets and `bun x nx run @animaOS-SWARM/web:typecheck`.** Expected: PASS.
- [ ] **Step 6: Commit** `feat(web): Anima app controls on the Health page`.

### Task 14: Telegram "already connected elsewhere"

**Files:**
- Modify: `hosts/rust-daemon/src/connectors/telegram.rs` (`TelegramTransportError`, around line 319), and the connector runtime status mapping that surfaces errors to `/api/connectors`
- Modify: `apps/web/src/components/TelegramSettings.tsx` (show the copy)
- Test: the existing tests in `telegram.rs` and `TelegramSettings.test.tsx`

**Interfaces:**
- Produces:
  - A new variant `TelegramTransportError::PollConflict`, mapped from `UpstreamApi { code: Some(409) }` on `getUpdates`.
  - The connector status error string: `This bot is already connected somewhere else.`
  - The poller backs off 60 s on `PollConflict` instead of the normal retry delay.

- [ ] **Step 1: Write the failing tests.**
  - The fake Telegram server returns `{"ok":false,"error_code":409,"description":"Conflict: terminated by other getUpdates request"}` → the transport error is `PollConflict`, and the connector status error equals the copy.
  - The web test renders the copy for that status.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `bun x nx run rust-daemon:test --skipNxCache` and `bun x nx run @animaOS-SWARM/web:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(connectors): clear message when a Telegram bot is polled elsewhere`.

---

## Milestone C: releases and self-update

### Task 15: Updater library (check, download, verify, swap)

**Files:**
- Create: `hosts/anima/src/update/{mod,release,verify,install}.rs`, `hosts/anima/release-key.pub` (minisign public key, committed)
- Test: `hosts/anima/src/update/tests.rs` with fixtures under `hosts/anima/tests/fixtures/update/`. Generate a **test** minisign keypair for the fixtures; the test public key is passed in, never the embedded one.

**Interfaces:**
- Produces:
  - `pub struct ReleaseInfo { pub version: semver::Version, pub archive_url: String, pub sha256_url: String, pub sig_url: String }`.
  - `pub fn select_release(api_json: &str, current: &semver::Version, target: &str) -> Option<ReleaseInfo>`.
    - It parses the GitHub `releases/latest` response.
    - The asset names follow cargo-dist: `anima-{target}.tar.gz` (Unix) or `anima-{target}.zip` (Windows), plus `.sha256`, plus our `.minisig`.
    - It returns `None` when the release is not newer, is a draft / prerelease, or **any of the three assets for `target` is missing**.
  - `pub async fn check(client: &reqwest::Client, repo: &str, current, target) -> Result<Option<ReleaseInfo>, UpdateError>`. `repo = "G9000/animaOS-SWARM"`, sent with `User-Agent: anima/{version}`.
  - `pub fn verify(archive: &[u8], sha256_file: &str, minisig: &str, public_key: &str) -> Result<(), UpdateError>`. Both the checksum and the signature must pass.
  - `pub fn extract_binary(archive: &[u8], target: &str) -> Result<Vec<u8>, UpdateError>`. It finds the `anima` / `anima.exe` entry.
  - `pub fn swap(new_binary: &[u8], current_exe: &Path) -> Result<PathBuf, UpdateError>`.
    1. Copy the current exe to `{exe}.old` (overwriting).
    2. `self_replace::self_replace` from a temp file in the same dir.
    3. Set the exec bit on Unix.
    4. Return the `.old` path.
  - `pub fn rollback(current_exe: &Path, old: &Path) -> Result<(), UpdateError>`.
  - `pub const RELEASE_PUBLIC_KEY: &str = include_str!("../../release-key.pub")`.

- [ ] **Step 1: Write the failing tests.**
  - `newer_release_selected_for_target`
  - `same_version_not_offered`
  - `prerelease_ignored`
  - `missing_sig_asset_not_offered`: Review Focus 2.
  - `missing_target_asset_not_offered`
  - `verify_accepts_good_signature`
  - `verify_rejects_tampered_archive`: one byte flipped → `Err(UpdateError::Checksum)`.
  - `verify_rejects_wrong_key`: `Err(UpdateError::Signature)`.
  - `extract_finds_binary_in_tar_gz_and_zip`
  - `swap_keeps_old_and_rollback_restores`: done on a copied dummy executable in a tempdir, using the `swap_at(path)` variant that takes the target path.
- [ ] **Step 2: Run `cargo test -p anima update`.** Expected: FAIL.
- [ ] **Step 3: Implement.** Generate the real release key once with `minisign -G -p hosts/anima/release-key.pub -s ~/.minisign/anima-release.key` (owner step, documented in Task 17), and commit only the `.pub`.
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(anima): signed release check and binary swap`.

### Task 16: Updates in the supervisor, tray and CLI

**Files:**
- Modify: `hosts/anima/src/supervisor/mod.rs`, `hosts/anima/src/commands/lifecycle.rs` (`update`)
- Create: `hosts/anima/src/update/handoff.rs`
- Test: `hosts/anima/src/update/handoff_tests.rs`

**Interfaces:**
- Consumes: `check`, `verify`, `extract_binary`, `swap` and `rollback` (Task 15); the gate (Task 8).
- Produces:
  - **Periodic check:** at start, then every 6 h, when `updates.check`. The result goes into `UpdateStatus.available`. Errors go into `last_error` and are not shown as a crash.
  - `Action::CheckUpdate` runs the check now.
  - `Action::Update`:
    1. Check.
    2. Download and verify (state `Updating { stage: "Downloading v…" }` / `"Verifying"`).
    3. Run the gate (drain and shutdown).
    4. `swap`.
    5. Write `run/update-pending.json` `{ "from": "<old version>", "to": "<new version>", "old": "<path>" }`.
    6. Re-exec the new binary with the same args. Unix: `CommandExt::exec`. Windows: spawn detached, then exit 0.
  - `pub fn on_start(paths, current_version) -> HandoffOutcome`. On supervisor start, if `update-pending.json` exists and `to == current version`:
    - Wait for the child to be `running` within 60 s, then delete the file.
    - Otherwise call `rollback`, write `run/update-failed.json` `{ "to", "error" }`, and re-exec the old binary. The old binary then shows `last_error = "Update to v{to} failed to start; rolled back"`.
  - CLI `anima update` prints progress from the status stages.

- [ ] **Step 1: Write the failing tests** (pure decision logic):
  - `pending_matching_version_waits_for_health`
  - `pending_other_version_is_ignored_and_removed`
  - `health_timeout_triggers_rollback`: rollback is called, and the `update-failed.json` content is as above.
  - `failed_marker_surfaces_last_error`
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `bun x nx run anima:test --skipNxCache`.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(anima): self-update with rollback`.

### Task 17: Release pipeline

**Files:**
- Create (via `dist init`, then edit): `dist-workspace.toml` and `.github/workflows/release.yml`
- Create: `.github/workflows/sign-release.yml` (reusable workflow, run as a dist `post-announce-jobs` job)
- Create: `tools/release/README.md` (owner steps)
- Modify: `hosts/rust-daemon/Cargo.toml` (`[package.metadata.dist] dist = false`) and `hosts/anima/Cargo.toml` (`[package.metadata.dist] dist = true`, `features = ["bundled-ui"]`)

**Interfaces:**
- Produces:
  - `dist-workspace.toml`:
    - `targets = ["aarch64-apple-darwin", "x86_64-pc-windows-msvc"]`
    - `installers = ["shell", "powershell"]`
    - `unix-archive = ".tar.gz"`
    - `windows-archive = ".zip"`
    - `checksum = "sha256"`
    - `install-updater = false` (we ship our own)
    - `github-build-setup = "../build-setup.yml"`, which installs Bun, runs `bun install --frozen-lockfile` and runs `bun x nx run @animaOS-SWARM/web:build` before cargo
    - `macos-sign = true`
    - `post-announce-jobs = ["./sign-release"]`
  - **Secrets** (documented, set by the owner):
    - `CODESIGN_CERTIFICATE` (base64 .p12 of the self-signed "AnimaOS Code Signing" cert), `CODESIGN_CERTIFICATE_PASSWORD`, `CODESIGN_IDENTITY` (dist's macOS signing variables).
    - `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD`.
  - `sign-release.yml`: for each `anima-*.tar.gz` / `anima-*.zip` on the tag's release, download it, `minisign -S` it, and upload `{asset}.minisig` with `gh release upload`.
  - The installers are published as `anima-installer.sh` and `anima-installer.ps1`. dist installers can't run post-install commands, so their success message is set to `Run "anima setup" to finish installing Anima.` This deviates from the spec, where the script ran setup itself.
  - `tools/release/README.md` covers:
    - generating the minisign key;
    - creating the self-signed code-signing cert with Keychain Access → Certificate Assistant → Code Signing, then exporting it as .p12;
    - setting the secrets;
    - cutting a release: bump `version` in the root `Cargo.toml` `[workspace.package]`, commit, `git tag v0.2.0 && git push origin v0.2.0`.

- [ ] **Step 1: Run `dist init`** with the choices above, then hand-edit to match.
- [ ] **Step 2: Run `dist plan --tag v0.1.0`.** Expected output lists exactly:
  - `anima-aarch64-apple-darwin.tar.gz`
  - `anima-x86_64-pc-windows-msvc.zip`
  - their `.sha256`
  - `anima-installer.sh`
  - `anima-installer.ps1`

  It contains no `anima-daemon` artifacts.
- [ ] **Step 3: Check that `dist generate --check` exits 0.**
- [ ] **Step 4: Run the dry release** on a fork or a `v0.0.0-test.1` prerelease tag, then check:
  - the release has both archives plus `.minisig` files;
  - `codesign -dv` on the macOS binary shows `Identifier=dev.animaos.anima`;
  - the shell installer installs and `anima --version` prints the version.
- [ ] **Step 5: Commit** `ci: tagged releases with cargo-dist and minisign signatures`.

### Task 18: Docs and full verification

**Files:**
- Create: `apps/contributor-docs/src/content/docs/packaged-anima.md` (match the existing docs' frontmatter)
- Modify: `hosts/rust-daemon/README.md` (link), `AGENTS.md` (one line under Repo Boundaries: "`hosts/anima` is the installable `anima` program (supervisor, tray, updater) wrapping `rust-daemon`.")

- [ ] **Step 1: Write the docs page:** install one-liners, `anima setup`, the commands table from the spec, Stop vs Quit, running side by side with `bun dev` (ports 8787/8080; separate data and keychain; one Telegram bot per copy), updating, and the release process (link `tools/release/README.md`).
- [ ] **Step 2: Run `bun x nx run @animaOS-SWARM/contributor-docs:build --skipNxCache`.** Expected: PASS.
- [ ] **Step 3: Run the full verification:**
  - `bun x nx run rust-daemon:test --skipNxCache`
  - `bun x nx run anima:test --skipNxCache`
  - `bun x nx run @animaOS-SWARM/sdk:test --skipNxCache`
  - `bun x nx run @animaOS-SWARM/web:test --skipNxCache`
  - `bun x nx test workspace-dev --runInBand --skipNxCache`
  - `cargo fmt --all --check`

  Expected: all PASS. The `workspace-dev` run proves the dev launcher is untouched.
- [ ] **Step 4: Commit** `docs: packaged Anima install, controls and releases`.
