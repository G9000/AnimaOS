# One-owner companion on a VPS

This package builds the web UI and Rust daemon from this checkout. Caddy serves the UI over HTTPS, authenticates the owner, and proxies the API and streaming responses. The daemon listens only on loopback inside Caddy's network namespace. Only ports 80 and 443 are published.

The package is intended for one trusted owner, one daemon, and one VPS. It is not a multi-user service or a sandbox for hostile agent code. Anyone with the owner login can operate this workspace and its connected accounts. Do not scale either service or share these volumes between live daemons.

## Requirements and current verification

Use a Linux VPS with Docker Engine and Docker Compose v2.20 or newer, a domain pointed at its public IP, and inbound TCP 80/443 available. Keep SSH restricted separately. Allow outbound HTTPS for certificate issuance, model providers, and connectors. Building the Rust image is resource intensive; use a build machine if the VPS is too small. The build uses the locked dependencies; the Rust/Debian base tags receive updates, so record the resulting image IDs for rollback.

Historical evidence recorded in revision `285f92d` (2026-09-17): both images built on Linux containers, and an isolated smoke test covered readiness, the Rust OS-keyring backend, workspace bootstrap/files, SQLite memory, authenticated routing and data/dummy-credential restoration after restart and container recreation. Gateway regressions covered Origin/Fetch Metadata checks, callback exceptions and SSE. That evidence predates the companion-console milestones and does not verify the current images.

M10 checks inspect deployment contracts and parse Compose with synthetic inputs. Browser journeys exercise the built console and actual SDK against simulated APIs/SSE bytes. No daemon, database, dev server or container is started for these checks. Current container/keyring/network recovery, public TLS, real providers/connectors, microphone behavior, VPS reboot and off-site restore remain manual acceptance checks. A healthy proxy alone does not establish a healthy daemon or a configured model.

## First installation

Clone or copy the repository to the VPS without local secrets, dependencies, or data. From the repository root:

```sh
cd deploy/vps
cp -n .env.example .env
chmod 600 .env
docker run --rm -it caddy:2.10.2-alpine caddy hash-password
openssl rand -hex 32
openssl rand -hex 32
openssl rand -hex 32
```

Edit `.env` with these values:

- `ANIMA_DOMAIN`: a bare hostname such as `companion.example.com`; no scheme, port, path, wildcard, or IP.
- `ACME_EMAIL`: your certificate contact email.
- `ANIMA_OWNER_USER`: one login name, using letters, numbers, or underscores.
- `ANIMA_OWNER_PASSWORD_HASH`: the Caddy hash, surrounded by **single quotes** so Compose preserves its `$` characters. Retain the password in your password manager.
- `ANIMA_INTERNAL_API_KEY`, `ANIMA_LOCAL_ADMIN_TOKEN`, `ANIMA_KEYRING_PASSWORD`: the three distinct random outputs. These are mandatory, with no public defaults. Keep the keyring password for the lifetime of the encrypted vault.
- At least one supported model API key, or connect ChatGPT from the UI after startup. The sample exposes OpenAI, Anthropic, and OpenRouter variables; other providers require adding their actual environment variables to the daemon service. No model provider key is sent to the gateway or included in the web build.

Do not paste these secrets into chat, commit `.env`, or print the fully interpolated Compose configuration. Docker administrators can inspect container environment variables; `.env` and VPS administrator access are part of the trust boundary. The keyring password is provided through the container environment and removed from the daemon child environment after unlocking.

```sh
docker compose config --quiet
docker compose build --pull
docker compose run --rm --no-deps gateway caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
docker compose up -d
docker compose ps
docker compose logs --tail=80 daemon
```

Open `https://YOUR_DOMAIN` and enter the owner login. Your browser remembers HTTP Basic authentication; use a private browser session on a shared device. There is no application-level logout or MFA in this package. Configure the companion and choose a real provider/model. The deterministic provider is a mock and does not call an AI service.

Never add an `8080` port mapping or change the daemon bind to `0.0.0.0`. The daemon's local-owner policy intentionally rejects non-loopback requests and forwarded headers. The gateway checks browser Origin and Fetch Metadata, requires a same-origin Origin on mutations and WebSocket upgrades, then translates the authenticated owner into separate internal API and admin credentials. Clients using the public API must use the owner Basic login and, on writes, `Origin: https://YOUR_DOMAIN`. Internal daemon keys do not bypass the gateway login.

The exact GET OAuth callback paths are allowed to return from another site; their one-time state is validated by the daemon. All callback routes still require the owner login. The UI and API share one origin, and SSE is forwarded without response buffering. See the [Caddy reverse proxy documentation](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy).

## Persistence and credentials

| Named volume                   | Contents                                                                                     | Recovery requirement                                |
| ------------------------------ | -------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `anima-companion_daemon_state` | `control-plane.json`, `memory.sqlite`, `history.sqlite`, SQLite sidecars and embedding state | Back up the whole volume with the daemon stopped    |
| `anima-companion_workspace`    | Workspace files, uploaded attachments and assets under `/workspace`                          | Keep the same `/workspace` path on restore          |
| `anima-companion_daemon_home`  | Encrypted GNOME keyrings under `.local/share/keyrings`, plus daemon home files               | Restore together with the original keyring password |
| `anima-companion_caddy_data`   | TLS certificates, private keys, and ACME account state                                       | Treat backup as sensitive                           |
| `anima-companion_caddy_config` | Caddy runtime configuration storage                                                          | Restore with the same deployment                    |

The explicit JSON/SQLite paths make state durable even though `ANIMAOS_RS_PERSISTENCE_MODE=memory` selects the non-Postgres adapter. Agent state, hot conversation tails, jobs, goals, schedules, workspace settings and connector records are in the control-plane snapshot. Memory uses `/state/memory.sqlite`; full conversation history, runs, usage, approval/schedule records and attachment metadata use `/state/history.sqlite`. Upload bytes live under `/workspace`, and credentials live in the home/keyring volume. Session deletion removes attachment records but does not physically wipe workspace files. Usage may outlive deleted sessions.

The table assumes Compose project name `anima-companion` from `compose.yaml`. If you override it with `-p` or `COMPOSE_PROJECT_NAME`, replace **every** named-volume prefix in backup/restore commands with that exact project name. Do not run those commands until the resolved volumes are verified. Treat the five volumes, protected `.env`, repository revision and image IDs as one stopped backup generation.

Telegram bot tokens, ChatGPT access/refresh tokens, OAuth application secrets, and Google/Microsoft connector tokens use the daemon's existing OS-keyring backend. This image supplies a private D-Bus session and GNOME Secret Service, unlocks the persisted login keyring at startup, and verifies a write/read/delete probe before starting the daemon. A wrong password or unusable vault causes startup to fail instead of silently using transient credentials. See [GNOME keyring startup](https://wiki.gnome.org/Projects/GnomeKeyring/RunningDaemon) and the [daemon manual](https://manpages.debian.org/bookworm/gnome-keyring/gnome-keyring-daemon.1.en.html).

Never remove the home volume or replace `ANIMA_KEYRING_PASSWORD` to troubleshoot an existing vault. Changing that environment value does not re-encrypt the keyring. Losing the password or vault requires reconnecting accounts; restoring control-plane JSON alone does not restore their secrets. Keep a separate protected copy of `.env` in addition to encrypted off-site backups. Model API keys supplied by environment are in `.env`, not in the keyring.

Compose sets `ANIMA_PUBLIC_BASE_URL=https://YOUR_DOMAIN`. The daemon uses this same origin for the Google Calendar/Drive/Gmail/Outlook callback URLs shown in connector setup and sent to providers. Register the exact URLs shown in the UI in your Google/Microsoft OAuth application; application credentials and provider consent are still required. Without this setting, local development retains `http://127.0.0.1:8080`. Invalid origins fail startup; use HTTPS without a path, credentials, query, or fragment. Changing your domain also requires updating provider registrations and restarting the daemon. Telegram and ChatGPT device-code authentication do not use these redirect callbacks. Live provider consent and restart behavior still need verification on your deployment.

## Acceptance checks before relying on it

1. Confirm `docker compose ps` shows **both** services healthy. `/ready` is the daemon's readiness endpoint. Check `docker compose logs --tail=80 daemon` if startup fails; never share logs containing secrets.
2. From another machine, `curl -I https://YOUR_DOMAIN` should return `401` without credentials, and TCP port 8080 must be unreachable. Sign in and send a real message; confirm a streaming response from the selected provider.
3. Create a memory and a workspace file, and connect Telegram if needed. Run `docker compose restart daemon`; check the conversation, memory, file, and connector status after it becomes healthy. Send a fresh Telegram message and verify exactly one response. The vault probe alone verifies availability, not the successful persistence of each provider's credential.
4. Reboot the VPS during a quiet period, reconnect, and repeat a real message. `restart: unless-stopped` restarts running services after Docker returns; manually stopped services remain stopped. Health checks report failure but Docker does not automatically restart a process merely because it is unhealthy.
5. Perform a backup and restore rehearsal on a separate machine. Stop the original daemon before starting restored Telegram polling so only one daemon owns the bot.

Persisted state is not seamless resumption of arbitrary execution. Running jobs recover into `needs_review` with an interrupted-attempt explanation; inspect external effects before retrying. A streaming request or child process cannot continue across a container restart, and in-progress OAuth handshakes may need to be started again. Mail sends with uncertain outcomes require checking Sent mail before another send. This package supplies process restart and durable data, not guaranteed exactly-once execution or high availability.

Delegated helpers cannot execute shell commands or manage background processes, even when the main companion has those tools. Default provider clients have a 15-second connect timeout and a 300-second whole-request timeout, including streams; explicitly injected clients retain their caller's policy. Helper deadlines and daemon shutdown use distinct cancellation causes. Ctrl+C and Unix SIGTERM close producers before draining accepted runs (up to 30 seconds, within a shared 40-second shutdown limit and Compose's 45-second grace). If the limit expires, unsettled work requires restart/interruption review. Cancellation/timeouts cannot undo external effects: inspect status and delivery uncertainty before retrying. Telegram delivery is at-least-once; a successful send followed by failed persistence can be delivered again, and retrying an outbound message does not rerun the model.

## Backup

Run these operator commands from `deploy/vps` during a quiet period. They are documented procedures, not commands executed during M10 verification. Stop both services so JSON, both SQLite databases (including `-wal`/`-shm` sidecars), uploads and keyring files are copied consistently. The archive includes `.env`, private keys, files, memory and account credentials: encrypt it before copying it off the VPS, restrict access and never commit it. Record repository revision, image IDs, the actual snapshot version from that generation and archive identity together; resolve the supported version against `CONTROL_PLANE_STORE_VERSION` in the matching daemon source.

```sh
mkdir -p backups
chmod 700 backups
docker compose stop
docker run --rm \
  -v anima-companion_daemon_state:/snapshot/state:ro \
  -v anima-companion_workspace:/snapshot/workspace:ro \
  -v anima-companion_daemon_home:/snapshot/home:ro \
  -v anima-companion_caddy_data:/snapshot/caddy-data:ro \
  -v anima-companion_caddy_config:/snapshot/caddy-config:ro \
  -v "$PWD/.env":/snapshot/deployment.env:ro \
  -v "$PWD/backups":/backups \
  alpine:3.22 sh -c 'umask 077; tar -czf /backups/anima-$(date -u +%Y%m%dT%H%M%SZ).tar.gz -C /snapshot .'
docker compose up -d
```

Confirm the archive can be listed with `tar -tzf backups/ARCHIVE.tar.gz`, including `state/control-plane.json`, both SQLite databases, workspace uploads, home/keyring and Caddy state, before considering it successful. Retain the repository revision and image IDs (`git rev-parse HEAD`, `docker compose images`) with each backup. Encrypt and keep a protected off-site copy; rehearse restore before retiring older backups. The snapshot's `.pre-attachments.bak` file alone is not a full backup.

## Restore to a fresh VPS

Use the same repository revision and deployment project name, with **empty new volumes**. Do not extract over a live installation. Keep the original keyring password. The following assumes the archive is `backups/restore.tar.gz`; run the first three commands only on the fresh installation with no existing `.env`. If moving to a new hostname, edit the restored `ANIMA_DOMAIN` and update DNS before starting services:

```sh
umask 077
tar -xOf backups/restore.tar.gz ./deployment.env > .env
chmod 600 .env
docker compose config --quiet
docker compose build
docker compose create
docker run --rm \
  -v anima-companion_daemon_state:/snapshot/state \
  -v anima-companion_workspace:/snapshot/workspace \
  -v anima-companion_daemon_home:/snapshot/home \
  -v anima-companion_caddy_data:/snapshot/caddy-data \
  -v anima-companion_caddy_config:/snapshot/caddy-config \
  -v "$PWD/backups":/backups:ro \
  alpine:3.22 tar -xzf /backups/restore.tar.gz -C /snapshot ./state ./workspace ./home ./caddy-data ./caddy-config
docker compose up -d
docker compose ps
```

The archive retains numeric ownership; the daemon runs as UID/GID 10001. Verify the acceptance checks before deleting any old installation or backup. Do not use `docker compose down -v`, which removes the named volumes.

## Updates and rollback

Make a verified backup first. Review and obtain the desired repository revision, then rebuild:

```sh
docker compose build --pull
docker compose down
docker compose up -d
docker compose ps
```

`down` without `-v` preserves volumes and deliberately recreates both services so the daemon joins the replacement gateway network namespace. Do not replace the gateway alone while leaving a running daemon attached to its old namespace. If an update fails, stop services, select recorded binaries/images and restore the complete matching pre-upgrade backup generation into fresh volumes. Restoring only control-plane JSON while keeping newer history can resurrect rows. Keep the same canonical `/workspace` root and original vault password; stop the original Telegram poller before starting a restored copy. Do not assume older binaries can read newer snapshots.

The first M9 upgrade writes `<control-plane-file>.pre-attachments.bak` (v10 input; Postgres uses `control_plane.backup.10`) before saving v11. This is only one rollback component. Never edit a snapshot version integer to bypass refusal, and never delete history tables or migration records as a routine M9/M10 rollback. Older version-specific SQL rollback guidance in the daemon README applies only to those historical versions. Restore the full recorded generation instead of mixing schemas/data.

For a provider API-key change, edit `.env` and recreate the daemon with `docker compose up -d --force-recreate daemon`. For gateway login/internal-token changes, recreate both services using the update sequence. Vault-password rotation requires a deliberate keyring password migration and is not supplied by this package.

## Local gateway regression checks

The following gateway suite starts real listeners and is excluded by the current no-service milestone rule. Run it only when that runtime work is explicitly authorized.

With Bun 1.3.8 and Caddy 2.10.2 installed:

```sh
CADDY_BINARY=/absolute/path/to/caddy bun test deploy/vps/gateway.test.ts
```

Run this from the repository root. It launches Caddy only on loopback with test credentials and a harmless stub upstream, and checks authentication, forwarding-header removal, Origin/Fetch Metadata protection, exact callback exceptions, and SSE delivery. It does not validate live providers, the Linux keyring, Docker networking, or certificate issuance.

## Offline milestone checks

From the repository root, with the Nx daemon and Cloud disabled:

```sh
NX_DAEMON=false NX_ISOLATE_PLUGINS=false NX_NO_CLOUD=true bun x nx run @animaOS-SWARM/web-e2e:deployment-contracts --skipNxCache
docker compose --env-file deploy/vps/fixtures/compose.test.env -f deploy/vps/compose.yaml config --quiet
```

The fixture contains synthetic parser inputs and cannot authenticate a real deployment. These checks never start Docker services. For real acceptance, also verify rejected cross-origin writes, unbuffered selected-model streaming, session/history/upload persistence after recreation, interrupted-run review without automatic tool replay, approvals/automations/skills/memory/usage, vault access, SIGTERM drain, reboot and encrypted off-site restore. Mark each result with its revision and target host; none is implied by offline passes.

Hermes scoped-recovery and orderly-stop behavior informed the Rust console design; Anima retains its own runtime. The focused M9 provider-format port retains its upstream MIT notice and revision in [HERMES-NOTICE](../../packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE). M13 adds selected-channel Discord REST chat inside the daemon; configure its bot, channel and permitted users through the owner console, then explicitly activate it. Its source port retains [Discord attribution](../../hosts/rust-daemon/src/connectors/discord/HERMES-NOTICE); the [connector guide](../../hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md#selected-discord-channel) explains polling and uncertain-send limits. Snapshot v12 backs up v11 input as `.pre-discord.bak` before upgrading; restore a complete recorded backup generation for rollback. Live Discord/account/hosting acceptance has not been performed by the local code gates. M14 adds selected Drive filename search and Google Docs/plaintext reads; see the [Drive setup and limits](../../hosts/rust-daemon/README.md#selected-drive-files). Snapshot v13 backs up v12 input as `.pre-drive.bak`. Google consent is broader than the native selected-file allowlist; local Drive disconnect does not revoke Google-wide scopes. Installable community modules remain deferred. Live Google/hosting acceptance is still separate.
