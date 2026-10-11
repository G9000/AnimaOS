# Companion Console M10: Deployment and Docs — Lean Plan

> **Active plan (2026-10-12).** M9 PR #15 merged at `647d606`; implementation uses `feat/companion-console-m10`, cut from that merged main. The owner authorized Codex subagents and unsigned commits. The controller owns binding rulings, staging and publication.

**Goal:** Finish the existing Rust-daemon/web deployment package and its operator
documentation, replace stale browser journeys with simulated session/event-stream
journeys, close the master plan's M10 carry-forwards, and verify locally without
starting services (spec §13.4 and §17; master M10 T10.1–T10.3).

**Architecture:** Keep the one-owner Caddy/loopback daemon topology and its existing
persistent volumes. Explicitly store history beside control-plane and memory data.
Use held ownership/generation fences around Telegram delivery, existing graceful
shutdown hooks and bounded provider transport. Browser E2E fulfills built static
assets and fake APIs inside Playwright; its fake SSE stream is browser-local and
controllable. Documentation derives from the actual router/OpenAPI/SDK behavior.

**Stack:** Existing Rust/tokio/reqwest/axum/serde/utoipa, existing Bun/Nx/Playwright,
Docker Compose configuration parsing only, existing Caddy config, Markdown.
No dependencies, dependency features, daemons, dev servers, databases or containers.

## Authority and reuse

- Read `AGENTS.md` before implementing.
- Read Start here in `docs/superpowers/handover/2026-09-28-companion-console.md`.
- Binding product spec: `docs/superpowers/specs/2026-09-23-companion-console-design.md`.
- M10 sources: spec §13.4, §17; master M10 and Carried from M3.
- Also spec §4.8 recovery, §6 events, §13 persistence and §14 authorization.
- Read M9 final plan/rulings and its actual merged implementation before branching.
- Working rules: `.superpowers/sdd/2026-09-23-companion-console-m10/rules.md`.
- Decisions: `.superpowers/sdd/2026-09-23-companion-console-m10/progress.md`.
- This draft uses existing M8/M9 lean structure; interfaces/rules/tests are precise.
- Codex subagents are explicitly authorized in place of unavailable Sonnet/Opus.
- Reuse Hermes code or behavior where it helps existing runtime delivery.
- Do not interpret the voice instruction as a request for a new memory feature.

**Hermes reference:** `C:/Users/leoca/AppData/Local/hermes/hermes-agent`.
Verified revision: `f3ef9db68332063a3625661f3a9b259f60abdb4e`; the controller
independently checked nested Git with read-only escalation. Source files were read
successfully. License is MIT, copyright 2025 Nous Research. M9 already retains the
full license/provenance in `packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE`.

Focused reusable behavior inspected:

- `gateway/session_recovery.py`: recovered durable rows remain scoped to their
  requested session/profile identity; recovery is not indiscriminate resurrection.
- `gateway/run.py`: SIGINT and SIGTERM enter an orderly drain/stop path.
- `gateway/systemd_stop_mark.py`: supervised intentional stops have explicit
  identity; a PID-file fallback cannot safely identify another process.
- Hermes split session modules demonstrate focused lifecycle/persistence boundaries.
- Translate those boundaries to existing Rust/TypeScript ownership; do not embed Python.
- Copy code only where an existing small pure piece materially saves implementation.
- For copied/substantially adapted code, retain copyright/MIT notice and provenance.
- Do not import Hermes' broad integrations, memory subsystem, supervisor or plugins.
- If nothing is copied, record behavior reuse in the plan/ledger; do not fabricate a copy.
- Preserve the existing M9 notice crediting provider formats and MIME routing.

## Boundaries and execution policy

- The prepared draft is now published on the authorized M10 branch.
- Drafting is complete; execute the tasks below.
- Implementation begins only after M9 is explicitly merged by the owner.
- Continue the original M9→M10 objective; do not add a third feature milestone.
- One milestone branch from main after previous milestone merge.
- The controller publishes final plan at
  `docs/superpowers/plans/2026-09-23-companion-console-m10.md`.
- No worktree/stash/reset/restore/checkout--path operations.
- Preserve unrelated `nx.json` and `anima.yaml`; never stage them.
- Controller stages only explicit owned paths and uses per-command unsigned commits, authorized by the owner on 2026-10-12.
- Use git -c commit.gpgsign=false commit; do not change global signing configuration.
- No installs, manifest dependency additions or dependency feature changes.
- Cargo always starts with `$env:CARGO_INCREMENTAL='0'` in PowerShell.
- Dockerfile cargo invocation gains `CARGO_INCREMENTAL=0` too.
- No new CARGO_TARGET_DIR; use existing Nx configured targets.
- No daemon/dev-server/database/container starts, including isolated smoke containers.
- Never inspect, stop, start or mutate Docker Postgres on port 55432.
- Compose config parsing is allowed: it does not start or contact a running service.
- Never print a production interpolated Compose config or real environment secrets.
- Tests use fake credentials, temporary files, in-process router and browser fixtures.
- Do not run `deploy/vps/gateway.test.ts`: it launches Caddy and Bun servers.
- Do not run default/live E2E target until configured no-service mode is selected.
- Do not run Playwright install; use installed browser binaries, report if absent.
- Do not run live provider or connector requests.
- No new integration parity, OAuth scope or cloud provisioning work.
- Existing Telegram/native connectors remain; optional community connector lifecycle
  for Telegram/Discord/Gmail/Calendar/Drive is future work, currently not implemented.
- Do not describe that future lifecycle as a shipped plugin/adapter capability.
- Preserve engine/host/app boundaries from AGENTS.md.
- Large files gain wiring after extraction; new behavior lives in focused modules.
- Lock order remains control-plane transaction → state → leaf mutexes.
- No std lock across await; owned mutations remain drop-safe.
- Keep model text rendering and hidden-Unicode rules intact.
- Keep string constants named and tests non-racy.
- No new browser `act()` warnings, console noise or unhandled rejections.
- No separate pre-flight audit, repeated review cycles or per-task owner check-in.
- Record each judgment as `Ruling: <what> — <why> — <cost if wrong>`.
- Batched daemon review, one final web+SDK/browser review, one fix wave, then gates.

## Inspected current state

| Area                    | Confirmed state                                                                | Planned M10 outcome                                         |
| ----------------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------- |
| Compose                 | gateway+daemon, only 80/443 published, daemon loopback in gateway namespace    | Preserve topology and owner boundary                        |
| Compose state           | `/state/control-plane.json`, `/state/memory.sqlite`                            | Add explicit `/state/history.sqlite`                        |
| Volumes                 | daemon_state/workspace/daemon_home/caddy_data/caddy_config                     | Backup all together with credentials and revision           |
| Gateway                 | basic auth, Origin checks, internal separate keys, `flush_interval -1`         | Preserve; offline contract check protects it                |
| Entry point             | validates three distinct 32-character secrets, starts keyring, execs daemon    | Document unchanged fail-closed vault behavior               |
| VPS README              | earlier container smoke evidence; history/uploads absent                       | Separate dated old evidence from new local/manual evidence  |
| Web README              | still says browser playground, agencies/swarms                                 | Describe current companion/session console                  |
| E2E README              | stale `ui-e2e`/`ui` names and live websocket dashboard                         | Correct names and separate offline console/live suites      |
| Provider adapter        | default constructor uses `Client::new()`                                       | Bound connect/whole request timeout                         |
| Shutdown                | `app/lifecycle.rs` waits only for Ctrl+C                                       | Unix SIGTERM enters same graceful drain                     |
| Shutdown order          | accepted runs drain before jobs/scheduler/pollers stop                         | Signal every producer before waiting for runs               |
| Shutdown classification | `is_shutting_down` matches message text                                        | Internal typed cause, unchanged response shape              |
| Helper deadline         | `CancelSignal.cancel()` shares owner-stop tool text                            | Typed cancel cause and accurate helper-deadline result      |
| Telegram send           | `deliver_pending_once_with_token` holds mutation lock during send_message      | Release transaction across network with ownership fence     |
| Session listing         | collect_candidates scans/messages/matches under state read lock                | Capture owned snapshot; derive outside lock                 |
| Compatibility           | Health status hook handles 404; global banner checks session/stream/run only   | Status 404 must trigger Update the daemon globally          |
| Router/OpenAPI          | router in `routes/mod.rs`, utoipa registrations there                          | Verify every console route schema/auth/docs and aliases     |
| E2E specs               | companion/independent-agents still mock legacy `/run`                          | Use sessions/runs/event stream fixtures                     |
| E2E configs             | default+companion configs start Vite preview                                   | Add no-webServer console config/target                      |
| Nx project              | `@animaOS-SWARM/web-e2e` resolves e2e/typecheck/e2e-live                       | Add explicit offline console target in its package metadata |
| Dependencies            | Bun/Nx/Playwright already installed                                            | No install required by draft                                |
| Core reasoning          | adapter already shapes max_completion_tokens and omits unsupported temperature | Verify existing behavior; avoid rewriting solved risk       |
| Silent pairs            | `sessions/pruning.rs` already prunes old mirrored hidden turns since M6        | Retain verified behavior; do not reimplement                |
| Session records         | spec has explicit owner deletion, no automatic record TTL                      | Preserve records; document bounded-query/cost limitation    |

The master M10 includes carried cleanup and runtime work beyond T10.1–T10.3.
This plan retains that authorized scope; it does not quietly drop those items.

## Contracts and reviewable outcomes

### Deployment configuration

```text
daemon.environment:
  ANIMAOS_RS_CONTROL_PLANE_FILE: /state/control-plane.json
  ANIMAOS_RS_MEMORY_SQLITE_FILE: /state/memory.sqlite
  ANIMAOS_RS_HISTORY_SQLITE_FILE: /state/history.sqlite       // explicit new line

daemon volumes:
  daemon_state:/state
  workspace:/workspace
  daemon_home:/home/anima

gateway reverse proxy:
  127.0.0.1:8080
  flush_interval -1
  server-side API/admin credential injection
```

- History store remains separate from semantic memory.
- `PERSISTENCE_MODE=memory` with explicit files is durable JSON/SQLite configuration.
- No PostgreSQL service/profile is added to the Compose package.
- No optional environment override can point history outside the persisted volume.
- Leave the example's required secret fields empty; test config uses dummy values.
- Do not copy real .env or Hermes profile secrets into test fixtures/docs.
- Existing root path `/workspace` remains stable for M9 attachment root bindings.
- Include uploads and skills/workspace state in backups, not only JSON/SQLite.
- Backup history.sqlite plus sidecars while services are stopped.
- Restore control plane/history/memory/workspace/vault as one consistent generation.
- Snapshot version is read from merged code, expected v11 after M9.
- M10 does not itself add a persisted feature or justify a version bump.
- If delivery safety requires a durable new state, controller records a version ruling
  before implementing; prefer existing state and a bounded in-memory claim.
- Never describe same-key provider retry as exactly-once external delivery.

### Offline verification levels

| Level                     | Allowed evidence                                       | Limitation                                           |
| ------------------------- | ------------------------------------------------------ | ---------------------------------------------------- |
| Pure deployment contracts | Parse source and assertions; no process/server         | Does not prove Caddy provision/TLS/container build   |
| Compose config            | `docker compose ... config --quiet` with dummy fixture | Parser-only, does not prove volumes/network/keyring  |
| Rust in-process           | router/SQLite temp files/scripted model/transport      | No real providers, Telegram or HTTP daemon listener  |
| Browser fixture           | built assets via route fulfill, fake APIs, fake stream | Real UI behavior, simulated transport and backend    |
| Existing previous smoke   | dated previous revision evidence                       | Not current-M10 deployment validation                |
| Manual deferred           | isolated container + Caddy/TLS/reboot/off-site restore | Must remain explicitly pending under no-service rule |

M10 local implementation can finish with all permitted local gates green.
The master-plan row and PR must explicitly retain container/VPS manual acceptance
as deferred, not claim spec §17's real container smoke happened.
User's no-service instruction overrides the older container-smoke instruction.

### Browser offline harness

```text
playwright.console.config.ts:
  testDir: ./src/console
  webServer: absent
  workers: 1
  retries: 0
  Chromium desktop + mobile viewport coverage
  serviceWorkers: block
  baseURL: http://anima-console.test

installConsoleFixture(page, state) -> ConsoleFixtureController:
  install local static-asset and API routes
  install fetch event-stream interceptor before app scripts
  expose sendEvent, disconnect, setSnapshot, calls, setRouteFailure
  retain fixture backend state across page reload
```

- Static files come from built `apps/web/dist`, not a running Vite server.
- Fulfill only the declared fake host; unexpected network URLs are aborted.
- Bound asset path resolution within dist; no filesystem traversal/symlink escaping.
- Use correct JS/CSS/image MIME for the actual built asset files.
- Requests to `/api/**` are validated fixtures, not blanket success fallbacks.
- Unimplemented endpoint returns a visible fixture failure, not arbitrary `{}`.
- No request can reach 127.0.0.1:8080 or Docker/network infrastructure.
- SSE interception happens only for exact `/api/agents/{id}/events` requests.
- Other fetches use Playwright route fixtures, including byte Blob downloads.
- Event Response contains real encoded SSE frames and remains open until controlled.
- SDK parser/reducer still runs; do not bypass it by injecting React state.
- Browser fixture uses exact exported SDK `AgentEvent`/Run/Session/Approval shapes.
- Event seq starts at 1 per connection snapshot; later frames increase strictly.
- Each reconnect has a fresh stream.snapshot matching retained fixture backend state.
- Support UTF-16 step.delta offsets, tools, lifecycle transitions and approvals.
- Controller barriers/poll assertions drive events; no timer guesses or sleeps.
- Stream cancellation detaches listeners and pending promises.
- Page reload retains backend fixture state but creates fresh browser stream instance.
- The fake stream is explicit: this does not prove real HTTP chunking through Caddy.
- Browser process is allowed; app/dev/server/DB/container processes are not launched.

### Older-daemon compatibility

- `GET /api/status` returning 404 must show Update the daemon at console level.
- Existing missing-session/stream/run compatibility behavior remains.
- 403/401/5xx/network failure must not be mislabeled as old daemon.
- The banner describes the daemon update, preserves draft, and prevents unsupported sends.
- A newer daemon response clears compatibility state after explicit refresh/reload.
- No legacy `/run` fallback that hides a stale daemon behind a broken console.
- Keep SDK changes additive; existing constructor/method names continue working.

## Task sequence and ownership

1. Deployment configuration + offline contracts/target.
2. Provider timeout + SIGTERM, producer-first shutdown and typed cancellation causes.
3. Telegram delivery outside global control-plane transaction.
4. Session scan snapshot + retention reconciliation + mechanical large-file extraction.
5. No-service Playwright static/API/event-stream harness.
6. Rewrite legacy journeys + required console/browser acceptance fixtures.
7. Older-daemon status compatibility.
8. OpenAPI/route-table/web documentation reconciliation.
9. Backup/upgrade/operator acceptance procedure and reuse provenance.
10. Reviews, one fix wave, permitted gates and publication.

Daemon batch A: Tasks 2–3, transport/shutdown/delivery authority.
Daemon batch B: Task 4 plus relevant Task 8 router/OpenAPI accuracy.
Deployment/docs batch: Tasks 1 and 9 together, without live deployment.
One final web+SDK/browser review: Tasks 5–8 together.
Accepted findings get one bounded fix wave before gates.

Core/runtime and fixture/docs workers may run independent slices concurrently.
Do not overlap mutations to ViewHarness/queue/connector runtime shared files.
Mechanical extraction must finish before behavior workers edit moved definitions.

## Commands and controller environment

PowerShell environment inherited by each tool command:

```powershell
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=(Join-Path (Get-Location) 'node_modules/.cache/nx-native-m10')
$env:CARGO_INCREMENTAL='0'
```

- Nx calls use require_escalated shell mode for diagnosed native cache/Vite access.
- This workaround changes process environment, never nx.json/anima.yaml.
- Use `Select-Object -Last 30` for long logs and inspect `$LASTEXITCODE`.
- Project discovery is permitted and was performed for web-e2e during drafting.
- No tests/builds/format commands are executed by the plan writer.
- Implementer task commands below run only after controller starts M10 implementation.
- Use existing Nx targets when present; filtered cargo tests provide focused feedback.
- Add offline targets only in existing web-e2e project metadata, not a new project.
- If an unfamiliar Nx flag is needed, consult `--help`/Nx skill before use.
- Do not use `NX_SKIP_NX_CACHE` or modify CI to mask failures.

---

### Task 1: Explicit history deployment path and offline contracts

**Owner:** deployment/docs implementer.
**Depends on:** M9 merge + final plan/rulings commit.

**Files:**

- Modify `deploy/vps/compose.yaml`.
- Modify `deploy/vps/daemon.Dockerfile` for cargo environment prefix.
- Create `deploy/vps/contracts.test.ts`.
- Create `deploy/vps/fixtures/compose.test.env` with dummy values.
- Modify `apps/web-e2e/package.json` for offline deployment-contract target.
- Do not edit the actual deployment .env, images or volumes.

**Interfaces:**

```text
Existing web-e2e Nx target: deployment-contracts
  executor nx:run-commands
  cwd repository root
  command bun test deploy/vps/contracts.test.ts
  cache disabled or valid source-only inputs
  no dependsOn live server/deployment tasks

compose.test.env:
  fake DNS hostname, fake owner, fake hash
  distinct dummy API/admin/keyring strings >=32 chars
  no provider credential required
```

**Rules:**

- Add explicit history path `/state/history.sqlite`.
- Keep daemon memory/control-plane paths and volume mounts unchanged.
- Keep daemon loopback bind and gateway namespace unchanged.
- Keep only gateway ports 80/443 published.
- No daemon public port, database service or Docker socket mount.
- Existing Caddy auth/header/origin/callback/flush behavior unchanged.
- Cargo Docker build line becomes `RUN CARGO_INCREMENTAL=0 cargo build ...`.
- Keep locked dependency build behavior and existing image tags.
- Tests inspect config only; never import gateway.test.ts.
- Test fixture is safe to commit and contains clearly synthetic secrets.
- `compose config --quiet` has no Docker-engine operations.
- Never run plain `compose config` with real interpolated values.
- Keep project name/volume prefix assumptions documented.

**Tests:**

- History/memory/control-plane paths all live under mounted state volume.
- Workspace mount stays `/workspace`.
- Home/keyring volume remains durable and uid unchanged.
- Gateway public ports exactly 80/443; daemon none.
- Distinct internal admin/API credentials never appear in web build args.
- Caddy retains owner auth before credential injection and origin guard.
- SSE flush_interval -1 retained.
- Exact callback GET exceptions unchanged; no broader prefix wildcard.
- Dockerfile cargo call prefixed; web build keeps SDK-before-web order.
- Contract target does not start processes/services.
- Dummy compose fixture passes installed Compose parser if available.

**Steps and commands:**

- [ ] Write offline assertions on specific deployment invariants.
- [ ] Add history line, cargo prefix and target.
- [ ] Run offline target and Compose parser only.
- [ ] Return validation and touched paths; controller commits without signing.

```powershell
bun x nx run @animaOS-SWARM/web-e2e:deployment-contracts --skipNxCache
docker compose --env-file deploy/vps/fixtures/compose.test.env -f deploy/vps/compose.yaml config --quiet
```

Expected: contracts/parser pass, no containers/services created.
If Compose is unavailable, record parser gate unavailable; do not install or fake success.
Commit suggestion: `feat(deploy): persist companion history and check offline contracts`.

---

### Task 2: Bounded provider transport and SIGTERM graceful shutdown

**Owner:** daemon/core implementer.
**Depends on:** M9 merged code; no deployment process required.

**Files:**

- Create focused model-adapter transport module/tests.
- Modify `packages/core-rust/crates/anima-model-adapters/src/adapter.rs` constructor wiring.
- Modify `hosts/rust-daemon/src/app/lifecycle.rs`.
- Add focused lifecycle tests; wire producer-first shutdown through `app.rs`.
- Modify existing jobs/scheduler/connector shutdown services for signal/drain split.
- Modify `agent_runs/shutdown.rs` and internal ApiError cause classification.
- Modify core `runtime/control.rs` and cancelled-tool-result selection.
- Add helper-deadline tests in focused agent-run modules; large file wiring only.
- Modify runtime-model host constructor only if error propagation needs wiring.

**Interfaces:**

```text
PROVIDER_CONNECT_TIMEOUT: Duration = 15 seconds
PROVIDER_REQUEST_TIMEOUT: Duration = 300 seconds
bounded_provider_client() -> Result<reqwest::Client, bounded error>

shutdown_signal(): waits Ctrl+C; on Unix also SIGTERM
wait_for_shutdown(ctrl_c_future, terminate_future) -> shutdown reason
  helper accepts injected futures for deterministic tests

Service.request_shutdown(): signal closing/cancel; idempotent, does not drain
Service.shutdown(): request if needed, then existing bounded drain
  jobs, scheduler, connector pollers, skills scanner, run coordinator

ApiErrorKind::DaemonShuttingDown // internal cause, no error-string comparison
CancelCause: OwnerStop | HelperDeadline | DaemonShutdown
CancelSignal.cancel(): compatibility wrapper for OwnerStop
CancelSignal.cancel_with(cause): first cancellation cause wins atomically
CancelSignal.cause(): Option<CancelCause>
```

**Rules:**

- All default ProviderModelAdapter clients use existing reqwest builder timeouts.
- Keep with_client injection for deterministic tests/custom client ownership.
- Existing public new constructor remains usable; do not introduce an SDK break.
- Prefer additive fallible constructor if host needs explicit initialization failure.
- Error strings omit credentials, endpoints with secret queries and body payloads.
- Bound entire streaming request, not just connect.
- Timeout does not imply rollback of tool/network side effects.
- ChatGPT already has 15/300-second timeouts; retain its behavior.
- Existing OpenAI reasoning request-shape tests remain; no duplicate redesign.
- Unix SIGTERM enters existing agent/jobs/scheduler/connectors/history drain.
- Request stop from every producer (jobs, scheduler, pollers, scanner) before
  awaiting accepted runs. Request coordinator close before new admission races.
- Producer stop means no new task production; preserve admitted task settlement.
- Drain accepted runs/jobs after all close signals are delivered, then close live
  streams after settlement events and flush/drain history last.
- Existing service shutdown entry points remain compatible wrappers.
- Internal shutdown refusal carries a typed kind; never compare message text in
  `is_shutting_down`. Public error body/code/status compatibility remains.
- Cancellation cause is host-agnostic core state, not an HTTP/string dependency.
- Preserve cancel() as owner-stop compatibility wrapper for existing callers.
- Helper timeout calls cancel_with(HelperDeadline); tool cancellations say helper
  deadline rather than owner stop. Preserve partial usage/tool-result pairing.
- Cause publication and wakeup are atomic/ordered; readers cannot see cancelled
  without its cause. First cancellation wins; later callers cannot relabel it.
- Existing owner Stop text stays unchanged; named new deadline/shutdown strings.
- No persistent cancel-cause field/snapshot bump unless implementation evidence
  requires it and controller records the decision first.
- Windows retains Ctrl+C; no Unix import leaks onto Windows builds.
- Failed signal registration must not immediately terminate a healthy daemon.
- First valid signal wins; shutdown sequence runs once.
- Preserve streams long enough for existing run settlement events.
- Keep existing 30-second run drain vs Compose 45-second stop grace documentation.
- No std/state lock around waiting for signals or provider requests.
- Reuse Hermes orderly-stop behavior, not Python supervisor markers/process management.

**Tests:**

- Default client construction uses both named timeout constants.
- Injected-client path remains unchanged.
- Scripted timeout is classified as transport failure without token leakage.
- Streaming partial usage remains recorded after timeout/failure.
- Injected Ctrl+C future triggers graceful reason.
- Injected SIGTERM future triggers same path.
- Failed signal registration is represented without false immediate shutdown.
- Both completion futures cannot double-run drain.
- Every producer close signal observed before accepted-run drain barrier is released.
- Closing producers cannot enqueue a new run during the shutdown drain.
- Typed shutdown classification remains correct if human-readable error text changes.
- Owner stop retains owner tool text; helper timeout emits deadline tool text.
- First-cause-wins regression for owner/deadline/shutdown races.
- Cancelled tools still get a result, partial text/usage retained.
- Request-shutdown is idempotent and compatible with later service.shutdown calls.
- Existing graceful shutdown/coordinator tests remain green.
- No actual signal is sent to the agent or a real daemon process.

**Commands:**

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-model-adapters --lib -- transport 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- lifecycle 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-core --lib -- control 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- shutdown 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- helper 2>&1 | Select-Object -Last 30
```

Expected: fixture tests pass without HTTP listeners or external requests.
Commit suggestion: `fix(runtime): bound provider calls and gracefully handle SIGTERM`.

---

### Task 3: Release global transaction during Telegram delivery

**Owner:** concurrency-sensitive daemon implementer.
**Depends on:** Task 2 transport assumptions; inspect current connector fencing.

**Files:**

- Create `hosts/rust-daemon/src/connectors/runtime/delivery.rs`.
- Create focused delivery race/regression tests.
- Modify `connectors/runtime.rs` for module/helper wiring only.
- Reuse existing outbound state/attempt counters and worker-generation fencing.
- Do not add Telegram media transport or new connector lifecycle features.

**Proposed boundary:**

```text
DeliveryClaim:
  connector identity + approved chat + selected outbound immutable copy
  credential/worker generation + previous attempt/state identity

claim_delivery(): transaction + state snapshot + scoped claim fence
send_claim(): transport call with no control-plane transaction/state guard
settle_delivery(): reacquire transaction, validate claim identity, save/undo once
```

**Rules:**

- Preserve existing owned tokio::spawn mutation entry and owner-close policy.
- Global control-plane mutation lock cannot span send_message.await.
- No state/std mutex guard spans transport await.
- A scoped per-connector delivery gate prevents two in-process sends of one row.
- Claim considers only existing Pending/Failed rows in current order.
- Capture chat, token and connector/worker generation once under existing authority.
- Revocation/deletion/replacement before claim prevents sending.
- A revocation after a network request starts cannot unsend it; document uncertainty.
- Settlement checks row still belongs to claim before applying attempts/state.
- Never overwrite a newer connector generation or recreate a deleted row.
- Preserve current at-least-once delivery semantics; no exactly-once claim.
- Successful send plus failed save remains uncertain; do not silently rerun model.
- Existing retry uses outbound text, never starts a fresh model call.
- If necessary, bound ambiguous retry behavior with existing Failed/error status;
  any new persistent state needs an explicit controller snapshot-version ruling.
- Preserve delivery ordering and MAX_UNDELIVERED_OUTBOUND accounting.
- Lifecycle lock may protect token deletion independently, but avoid replacing the
  global transaction stall with another global lock blocking all connectors.
- Helpers/jobs/session runs must progress while one Telegram send is blocked.
- No new dependencies, features, database schema or private transport implementation.
- Honor original source attachment metadata/content from M9 accepted Telegram turns.
- No real Telegram requests or token reads from user's keyring in tests.

**Tests:**

- Blocked fake send does not block unrelated control-plane mutation.
- Two delivery callers send one selected row once in process.
- Outbound ordering preserved over success/failure.
- Connector deletion/replacement between claim and settlement does not resurrect data.
- Credential generation mismatch cannot send using uncommitted replacement token.
- Stop/shutdown while send blocked retains owned settlement correctness.
- Successful transport + persistence failure leaves safe retry/uncertain status.
- Retry does not call model again.
- Revoked credential updates status consistently.
- Existing final-owner snapshot rollback cannot deliver uncommitted outbound rows.
- Use barriers/channels, not sleep or live network.

**Commands:**

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- delivery 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- connector 2>&1 | Select-Object -Last 30
```

Expected: deterministic delivery/connector tests pass.
Commit suggestion: `fix(daemon): release control-plane transaction during Telegram sends`.

**Review batch A:** Tasks 2–3, one review covering transport, graceful drain and delivery races.

---

### Task 4: Session scan snapshot and mechanical large-file extraction

**Owner:** daemon structure implementer, then web structure implementer sequentially.
**Depends on:** Task 3 delivery module extraction to avoid shared-file conflicts.

**Files:**

- Create `hosts/rust-daemon/src/sessions/list_snapshot.rs` and focused tests.
- Modify `sessions/views.rs` for capture/derive wiring.
- Inspect `sessions/pruning.rs` existing silent-turn retention and focused tests.
- Modify retention only for an observed regression; never invent a destructive TTL.
- Extract inline `agent_runs.rs` tests into `agent_runs/tests.rs` or focused siblings.
- Extract remaining connector production responsibilities to focused runtime siblings.
- Extract connector inline tests into `connectors/runtime/tests/` modules.
- Extract focused orchestration from `apps/web/src/ViewHarness.tsx` into hooks.
- Extract ViewHarness test helpers/journeys into focused test modules.
- Preserve existing public imports and test entry points where practical.

**Proposed interface:**

```text
SessionListSnapshot:
  owned selected SessionRecord values
  hot message seeds for selected agents/rooms only
  mirrored IDs needed for count reconciliation
  active-run/pending-approval counts and automation availability
  history store Arc

capture_list_snapshot(&DaemonState, agent_id, include_helpers, filters)
  -> Option<SessionListSnapshot>
derive_candidates(snapshot, search_tokens) -> Vec<Candidate>
```

**Rules:**

- Capture under read lock; derive search/snippets/previews/unread/cursors outside it.
- Copy needed message fields, not unrelated agents/full runtime snapshots.
- Bound work by selected records/hot messages; avoid cloning attachments/base64 bytes.
- Full history remains read through store after state guard is released.
- Same snapshot supplies counts/records/helper relationships to avoid mixed authority.
- Preserve silent check-in grouping and whole-turn visibility semantics.
- Preserve helper-parent filtering, archived/kind filters and search behavior.
- Preserve stable cursor order and hot/store deduplication.
- Do not move DB drivers/HTTP host logic into core packages.
- The lock optimization is a meaningful behavior-risk change with tests.
- Test/module extraction is mechanical: no altered fixture text or assertions.
- Production module extraction preserves ConnectorManager existing method signatures.
- Private visibility may become pub(super) only where child-module access requires it.
- Avoid broad public API exports to make tests compile.
- Keep owner mutations/drop-safe spawn wrappers intact.
- ViewHarness extraction moves existing blocks into focused hooks with equivalent inputs.
- Do not add new UI navigation, memory features or connector parity during cleanup.
- Do not rewrite all component state merely to hit an arbitrary line-count target.
- Record moved responsibilities and before/after file sizes; no padded wrapper modules.
- Silent check-in pairs already leave the hot tail once old and mirrored since M6;
  verify whole-pair/history/rollback protection rather than implementing it twice.
- Keep newest 200 visible messages and 24-hour hot-tail policy, active-run/approval/
  undelivered outbound protections and ephemeral/reconciliation guards.
- Session-record automatic TTL is not specified. Preserve owner-created/deleted
  session semantics and historical ownership references; do not silently discard
  records to meet a guessed retention count or age.
- Scope session-record retention follow-up explicitly: M10 documents current retained
  metadata/query cost and existing owner delete/archive; automatic expiry remains
  pending a product retention policy because deletion could orphan usage/history.
- This is a controller ruling to record, not a claim that unbounded records are solved.
- If a safe mechanical split is more costly than a bounded alternative, controller
  records that scope decision explicitly; do not silently skip master carry-forwards.

**Tests:**

- Session list output matches existing fixtures for every kind/helper relation.
- Search/cursor/archived/silent-check-in/unread/count behavior unchanged.
- State write can proceed while a fake snapshot-derivation gate is held outside lock.
- History call occurs after guard release.
- Existing `silent_checkin_pairs_leave_a_small_room_once_old_and_mirrored` passes.
- Retained visible/tool pairs, active references and failed-prune rollback protected.
- Session records survive hot-tail pruning; owner deletion still removes scoped history.
- Missing agent remains None and no foreign message seeds included.
- Concurrent deletion/updates produce a coherent captured page, next refresh sees new state.
- Existing queue/steer/approval/connector tests retain names and outcomes after moves.
- Existing ViewHarness/WorkspaceShell tests retain equivalent coverage after extraction.
- No new act warnings or console output from moved fixtures.

**Steps and commands:**

- [ ] Write session lock-contention regression using barriers.
- [ ] Implement owned snapshot/derive split; run session-focused tests.
- [ ] Mechanically move daemon test/production responsibilities in explicit batches.
- [ ] Mechanically extract focused web orchestration/test helpers.
- [ ] Run relevant Rust/web checks once after the extraction series.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- sessions 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- agent_runs 2>&1 | Select-Object -Last 30
bun x nx run-many -t test,typecheck -p @animaOS-SWARM/web --skipNxCache
```

Expected: session/contention and existing extracted suites pass.
Commit suggestions: separate `refactor(daemon)` and `refactor(web)` explicit-path commits.

---

### Task 5: No-service Playwright harness and explicit console target

**Owner:** browser fixture implementer.
**Depends on:** built M9 web/SDK contract; independent of daemon internal cleanup.

**Files:**

- Create `apps/web-e2e/playwright.console.config.ts`.
- Create `apps/web-e2e/src/console/fixtures/{static,api,stream,state}.ts`.
- Create focused fixture helper tests where useful for parsing/bounds.
- Modify `apps/web-e2e/package.json` with `e2e-console` target.
- Preserve live/default configs for separately authorized workflows.

**Interfaces:**

```text
ConsoleFixtureState: typed Session/Run/Approval/status/message records
  mutable only through fixture controller methods

installConsoleFixture(page, state): Promise<ConsoleFixtureController>
  controller.emit(agentEvent)
  controller.disconnectStream()
  controller.snapshotOnNextConnection(snapshot)
  controller.calls(): validated request log, no secrets
  controller.failNext(method, path, status, body)

e2e-console target:
  dependsOn @animaOS-SWARM/sdk:build and @animaOS-SWARM/web:build
  command playwright test --config=playwright.console.config.ts
  parallelism false; proper source/build inputs and report outputs
```

**Rules:**

- No webServer config, child app process or HTTP listener.
- Fulfill navigation/index/assets from built dist via page/context route.
- Strict dist containment and declared MIME; fail missing asset clearly.
- No external image/font/network requests; abort unexpected URLs deterministically.
- Reject arbitrary path traversal/drive paths/symlink file escape in asset helper.
- Fake hostname prevents accidental selection of real local service.
- Fixture router validates request method/path/body/idempotency key/IDs.
- Unknown fixture endpoint returns a loud failure and records it.
- Stream interceptor returns browser ReadableStream Response with exact SSE framing.
- Encode events through TextEncoder; SDK parses normal bytes.
- Include snapshot first, monotonic seq/at/run/session IDs from typed fixtures.
- Feed delta offsets in UTF-16 units, including non-BMP regression.
- Keep stream open until explicit disconnect/end; no always-finite fulfilled SSE body.
- Controller commands release deterministic barriers; no page.waitForTimeout sleeps.
- Reset browser stream ownership on reload while retaining simulated server state.
- AbortSignal closes reader/listeners and rejects pending reads cleanly.
- Restore fixture fetch wrapper only within page; no production client changes.
- Take screenshots/traces only on failure unless existing scenario artifact needed.
- Keep Chromium installed-browser requirement; no hidden install/download.
- Default/live E2E target remains documented as service-starting and excluded here.
- Do not silently retarget all old live tests into this fixture suite.

**Tests:**

- Static built app boot with blocked real network.
- Asset MIME/containment/missing-file helper cases.
- Exact event request intercepted; ordinary APIs pass through fixture router.
- SSE snapshot/delta/final frames parsed by actual SDK.
- Abort/unmount/reload leaves no old stream pushing into new session.
- Reconnect gets fresh snapshot, not previous seq continuation assumptions.
- Fixture backend state remains across page reload.
- Unexpected request method/body/path fails and appears in test diagnostics.
- Configuration has no webServer/service dependency.

**Commands:**

```powershell
bun x nx run @animaOS-SWARM/web-e2e:typecheck --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache
```

Expected: browser-only fixture suite runs; no daemon/dev server/DB/container starts.
Commit suggestion: `test(e2e): add serverless companion console fixture harness`.

---

### Task 6: Session/event journeys and legacy browser spec replacement

**Owner:** browser fixture implementer.
**Depends on:** Task 5.

**Files:**

- Create `src/console/live-run.spec.ts`, `approval-stop.spec.ts`.
- Create `reload-mobile.spec.ts`, `attachments-compatibility.spec.ts`.
- Replace legacy `/run` mocking in `src/companion.spec.ts` and `independent-agents.spec.ts`.
- Factor shared companion fixtures into Task 5 helpers instead of copy/paste.
- Keep existing independent-agent data retained behind one companion front door.

**Required journeys:**

1. Send → 202 queued run → run.started → step.delta → tool card → terminal reply.
2. Pending approval → Allow once/Deny → tool completion/status settling.
3. Stop mid-run → stopped partial reply → no tool side effect replay.
4. Reload mid-run → new snapshot → rejoin same run → final reply once.
5. Mobile drawer opens/closes and session selection remains usable.

**Additional bounded coverage:**

- One attachment-only send retains IDs and authenticated thumbnail download route.
- Older-daemon status 404 shows update instruction.
- Independent retained/helper data does not become the default visible companion.
- No test directly posts legacy blocking `/run` for main console send behavior.
- Existing low-cost happy-path onboarding remains, using current responses.
- Include simulated queue/429 or save-failure recovery only if existing coverage needs
  adaptation; do not build every runtime invariant again in E2E.

**Rules:**

- Test user-observable outcomes and request payloads, not component internals.
- Typed fixture terminal records match actual current SDK shapes.
- Server fixtures own message persistence; reducer refreshing real API fixture pages
  must not manufacture reply text from run.status alone.
- Tool started/finished and approval events use proper step/toolCall/run/session IDs.
- Do not emit fields absent from the real stream contract.
- Reload test preserves idempotency key and asserts no duplicated POST.
- Stop fixture records request and settles state before terminal event.
- Approval fixture asserts owner action/body scope and settles exactly once.
- Mobile uses viewport 390x844; desktop 1280x900 (existing dimensions).
- No network timeouts used as successful waiting conditions.
- Assert no unexpected fixture requests, page errors or console warnings.
- Dictation/read-aloud remain browser-mocked unit coverage; no real microphone use.
- Screenshots provide failure context, not manual acceptance claims.

**Tests:**

- Each required journey passes at its appropriate viewport.
- Attachment route request names/IDs and Blob preview are owner transport paths.
- Tool card persists/reconciles after message page refresh.
- Stopped run remains stopped after reload, no automatic rerun.
- Rejoined active snapshot text offsets do not duplicate displayed deltas.
- Stream disconnect displays Reconnecting, reconnect snapshot restores.
- Mobile navigation preserves draft/attachment session ownership.
- Existing onboarding/independent-data intent survives legacy fixture rewrite.

**Commands:**

```powershell
bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:typecheck --skipNxCache
```

Expected: required simulated console journeys pass; no legacy run mocks remain in migrated paths.
Commit suggestion: `test(e2e): cover session streams approvals stop reload and mobile`.

---

### Task 7: Status-driven older-daemon compatibility

**Owner:** web implementer after Task 4 web extraction.
**Depends on:** Task 4 ViewHarness extraction; existing daemon.status wrapper.

**Files:**

- Create focused `apps/web/src/hooks/useDaemonCompatibility.ts` and tests.
- Modify extracted console orchestration hook for compatibility composition.
- Modify `ViewHarness.tsx` for wiring only.
- Modify compatibility banner tests and Task 6 fixture case.
- No SDK breaking change; reuse `StatusTooOldError` and `daemon.status()`.

**Interface:**

```text
useDaemonCompatibility({ enabled, epoch }):
  statusUnsupported: boolean
  check(): Promise<void>

global daemonTooOld = existing sessions/stream/runs missing OR statusUnsupported
```

**Rules:**

- Spec §13.4 specifically names GET /api/status 404.
- Probe current status without requiring owner to open Health page first.
- Reuse existing client error type; no guessed response heuristic.
- 404 sets unsupported; 401/403/5xx/network error does not.
- Avoid duplicating 15-second Health polling across every component.
- One bounded probe per connection/epoch, explicit refresh if needed.
- Stale/unmounted probe cannot change current console state.
- Compatibility does not clear drafts/recovery/upload metadata.
- Disable unsupported main-console sends while allowing safe read/navigation.
- New valid daemon response clears unsupported on explicit reconnect/reload.
- Existing session/stream/run older-daemon banners remain valid.
- No fallback to legacy /run and no fake success for absent APIs.

**Tests:**

- Status 404 alone shows Update the daemon without opening Health.
- Status 200 does not block sends.
- 401/403/500/network error not mislabeled older daemon.
- Probe cleanup/stale epoch respected.
- Draft/attachments remain when banner appears.
- Existing missing-run/session/stream compatibility retained.
- New response after reconnect clears prior status unsupported.
- Browser fixture covers top-level visible update instruction.

**Commands:**

```powershell
bun x nx run-many -t test,typecheck -p @animaOS-SWARM/web --skipNxCache
```

Expected: web compatibility and existing console tests pass without warnings.
Commit suggestion: `fix(web): detect older daemons from missing status endpoint`.

---

### Task 8: OpenAPI route inventory and README reconciliation

**Owner:** docs/API implementer.
**Depends on:** Tasks 1–7 final interfaces.

**Files:**

- Modify `hosts/rust-daemon/README.md` route/environment/compatibility sections.
- Modify `apps/web/README.md`.
- Modify `apps/web-e2e/README.md`.
- Create focused `hosts/rust-daemon/src/routes/tests/docs_contracts.rs`.
- Modify existing route/OpenAPI test registration.
- Modify `routes/mod.rs` annotations/registration only for observed missing entries.
- Touch Astro docs only when a direct current-implementation mismatch must be fixed.

**Inventory contract:**

- Use actual Router registration and `ApiDoc::openapi()` as sources of truth.
- Normalize route placeholders (`agent_id`, `session_id`, attachment_id) consistently.
- Document aliases separately; do not label route aliases as missing API features.
- Generate/update one marked README route table block from sorted method/path entries.
- Keep detailed behavior sections human-authored and verified from handlers/tests.
- Read-only in-process docs tests compare README method/path inventory to OpenAPI.
- Missing companion routes in OpenAPI require real annotations/registration fix.
- In-process route existence/auth probes ensure documented operations are mounted.
- No HTTP listener required to inspect OpenAPI; do not call a real /openapi.json.
- Do not add a public endpoint merely to export documentation.
- A small test-only inventory output helper is sufficient for regeneration.

**Daemon README content:**

- Sessions/runs/events, approvals, skills, automations, memory, usage/logs/status,
  attachments and relevant legacy routes in current router.
- Owner authorization/read no-store behavior and API-key distinction.
- Summary/hot-tail behavior versus session full history.
- Route limits/error codes/key equality where already implemented.
- Async accepted runs versus legacy blocking compatibility route.
- Stop/interrupted/restart semantics without exactly-once external-effects claim.
- MIME/image limits, reference-only transcript and saved-not-read documents.
- Browser voice is client feature, not daemon voice API.
- History vs memory storage paths and ephemeral limits.
- Snapshot current version and pre-upgrade backup/rollback requirement.
- Updated provider timeout/SIGTERM behavior and known ignored Postgres tests.

**Web/E2E README content:**

- Current companion console, session navigation, active reply/queue/steer/Stop.
- Review approvals/skills/automations/memory/usage/logs/health.
- Attachment upload/preview/download and browser support for voice.
- Normal local workflow `bun dev --host rust` is an operator instruction only.
- Distinguish web from playground, core-ts, Rust daemon and placeholder hosts.
- Correct Nx names @animaOS-SWARM/web and @animaOS-SWARM/web-e2e.
- Offline e2e-console target and its no-service guarantees.
- Default/live suites explicitly start services and are not run in this milestone.
- Mock API/stream evidence does not prove real provider/Caddy/TLS delivery.
- Older-daemon Update the daemon behavior and additive SDK compatibility.
- No advertised future community connector lifecycle or new memory feature.

**Tests:**

- Method/path route table matches current OpenAPI inventory for console routes.
- Mounted handlers/OpenAPI tag/schemas align.
- Every M0–M9 new route is present; public aliases accounted for.
- Touched Markdown links and repository paths resolve.
- No stale ui/ui-e2e names or legacy-run main-console claims in touched docs.
- No secret defaults, keyring password examples or absolute private root leakage.
- Commands use actual resolved project targets.
- Dockerfile cargo prefix and README matching transport/shutdown constants.

**Commands:**

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- docs_contracts 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- openapi 2>&1 | Select-Object -Last 30
```

If Astro docs are touched, add their matching docs/contributor-docs build gate.
Use the docs-code-sync source-of-truth approach; its older Python/server/RLS-specific
checker is not authority for this Rust console. Do not add unrelated changelog work
when milestone ledger/master/handover already provide requested tracking.
Commit suggestion: `docs(console): reconcile routes deployment and browser guidance`.

**Review batch B:** session snapshot/extraction and router/OpenAPI accuracy.

---

### Task 9: Backup, upgrade, rollback and manual acceptance procedure

**Owner:** deployment/docs implementer.
**Depends on:** Tasks 1–4 and current merged snapshot backup code.

**Files:**

- Modify `deploy/vps/README.md`.
- Add `deploy/vps/ACCEPTANCE.md` only if a separate concise operator checklist helps.
- Add source reuse notice if code was actually copied from Hermes.
- Controller updates final M10 plan/master/handover; implementer reports decisions.

**Operator instructions to deliver:**

1. One owner/domain/Caddy/loopback setup, existing credentials, explicit history path.
2. Stop services during backup; archive complete state/workspace/home/Caddy volumes.
3. Include history/memory SQLite sidecars, uploads, keyring and protected deployment env.
4. Record repository revision, image IDs, snapshot version and archive identity.
5. Validate archive listing and encryption/off-site retention.
6. Restore to empty fresh volumes using same root/project name/keyring password.
7. Upgrade only after verified backup; recreate both services/network namespace.
8. Rollback to recorded binaries plus consistent pre-upgrade data generation.
9. Keep one live polling owner; do not start restored Telegram alongside original.
10. Rehearse real isolated smoke only under later explicit runtime authorization.

**Rules:**

- No backup/restore/stop/create/up commands executed during this task.
- Document actual named volume prefixes and custom Compose project-name caveat.
- State/history must be consistent; restoring only JSON can resurrect old rows later.
- Snapshot pre-attachments backup restores v10 input shape but is not a whole backup.
- Never instruct editing snapshot version integer to bypass refusal.
- Never delete migrations/tables blindly as a routine rollback step.
- Existing historical pre-M2 SQL rollback instructions remain clearly version-specific.
- Preserve vault original password; changing env is not re-encryption.
- Keep `/workspace` root for M9 record binding/digest checks.
- Uploaded files may remain after session deletion; no promise of physical wipe.
- Usage survives deleted sessions as current implementation dictates.
- Healthchecks report health; restart policy restarts stopped/crashed processes.
- A hung request timeout/graceful SIGTERM does not undo external side effects.
- Telegram uncertain send requires operator review; no guaranteed exactly-once claim.
- Current M10 verification section separates parser/fixtures from real container evidence.
- Earlier smoke evidence may be retained with its date/revision scope, never reused as current pass.
- Future optional community connector lifecycle is not an operator prerequisite.
- Hermes reuse notes say what behavior/code was reused, revision and license.
- Retain complete MIT notice for copied/substantial adaptation, without unnecessary branding claims.

**Manual acceptance checklist (not executed):**

- Owner login/unauthenticated 401, rejected cross-origin write, private port 8080.
- Real selected model streaming through Caddy, no buffering.
- Session remains after daemon restart/container recreation with same volumes.
- Mid-run interruption is reviewable and never automatically replays tools.
- Upload+preview+image provider turn+saved-not-read document remain after restart.
- Approvals/schedules/skills/memory/usage retain current durable behavior.
- Vault credentials remain available; no duplicate Telegram polling instance.
- SIGTERM drains within Compose grace and readiness reflects failures.
- VPS reboot and off-site encrypted restore rehearsal.
- Public TLS and provider consent/inference checks on the actual target host.

**Verification:**

- Re-read every operator command for exact paths and destructive scope.
- Validate dummy Compose fixture only, offline contract assertions, docs path links.
- Do not label the deferred checklist passed.
- Controller marks M10 local gates done with manual deployment evidence pending.

Commit suggestion: `docs(deploy): document consistent history backup upgrade and rollback`.

---

### Task 10: Reviews, one fix wave, gates and M10 publication

**Owner:** controller.
**Depends on:** all tasks and explicit M9 merge before any implementation.

**Review workflow:**

- [ ] Batch A covers timeout/SIGTERM/Telegram delivery ownership.
- [ ] Batch B covers scan-lock/extraction/router/OpenAPI compatibility.
- [ ] Deployment/docs review covers configuration/backup/root/vault consistency.
- [ ] One final web+SDK/browser review covers harness/journeys/older-daemon behavior.
- [ ] Record accepted/rejected findings and cost judgments in ledger.
- [ ] One bounded fix wave, with explicit file ownership.
- [ ] Run focused regressions for accepted fixes.
- [ ] Run permitted full gates on current tree.
- [ ] Report unavailable/deferred manual gates honestly.

**Required local gates:**

```powershell
$env:CARGO_INCREMENTAL='0'
bun x nx run rust-daemon:test --skipNxCache
```

```powershell
bun x nx run-many -t test,typecheck,build -p @animaOS-SWARM/sdk @animaOS-SWARM/web --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:typecheck --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:deployment-contracts --skipNxCache
docker compose --env-file deploy/vps/fixtures/compose.test.env -f deploy/vps/compose.yaml config --quiet
```

```powershell
$env:CARGO_INCREMENTAL='0'
cargo fmt --all --check
bun x nx format:check --base=origin/main
```

Every command uses common Nx environment above; Nx runs require_escalated.
The verified rust-daemon:test Nx target depends on core-rust:test. Require both
core-rust and daemon tasks to pass in this same gate; no redundant standalone core
final gate is needed. Keep narrow core tests during development for fast feedback.
No service-starting e2e/e2e-live/gateway/container smoke command is run.
If Astro docs change, add corresponding docs/contributor-docs build commands.
If workspace-dev changes, add required workspace-dev test; avoid touching it otherwise.
Builds/tests must verify exit codes; record pass/ignored counts, not baseline guesses.
No Postgres service/test activation and no browser install if a binary is missing.
Known Nx Cloud CI step failure is not a local pass.

**Formatting/publication:**

- Format only touched files; inspect diff before staging.
- Preserve/exclude unrelated nx.json/anima.yaml even if global formatter detects them.
- Stage explicit owned paths and use owner-authorized unsigned conventional commits.
- Keep copied-code notices/provenance with actual adapted sources.
- Master M10 row states local implementation/gates and deferred runtime acceptance.
- Handover lists branch/PR/HEAD/gates/snapshot version/manual acceptance gap.
- Ledger states exact resume point before any stop.
- Push `feat/companion-console-m10` and open PR to main.
- Attach PR via Codex artifact tool.
- Merge only when owner explicitly says merge.
- Do not publish an unsupported VPS-ready/no-gaps claim.

---

## Exactly five risks for controller rulings

The controller appends `Controller rulings on the risks (binding)` to the final
tracked M10 plan after M9 merge. This draft requests no separate pre-flight audit.

### R1: Deployment state, attachment root, vault and rollback generation drift

Adding an explicit history path is simple; backups/rollback can still lose or
resurrect messages if only control-plane JSON is restored, or invalidate upload
records when workspace root changes. Vault password/volume consistency is separate.

Proposed choice: persist history in existing state volume, back up the full stopped
state/workspace/home/Caddy generation, retain same root/keyring password, and never
treat a `.pre-attachments.bak` snapshot as a complete deployment backup.

Cost if wrong: apparent history loss, stale rows resurrected on upgrade, missing
attachments, disconnected accounts or unrecoverable vault credentials.

### R2: Telegram network transaction release weakens lifecycle fencing

The global transaction currently protects selection/send/settlement by blocking
other mutations. Releasing it creates deletion/replacement/concurrent-send races,
and a successful network send followed by failed save remains uncertain.

Proposed choice: scoped delivery claim/gate plus connector/chat/token/worker/row
identity validation before send and settlement; no global transaction across await;
preserve at-least-once semantics and never rerun the model to retry delivery.

Cost if wrong: duplicated sends, wrong-token delivery, resurrected deleted outbox,
lost response accounting or a new global stall disguised as a scoped fix.

### R3: Timeout and shutdown changes truncate useful work or fail to drain

Provider timeout changes and Unix SIGTERM support affect active streams and
settlement. Failed signal setup must not trigger immediate shutdown; tool effects
cannot be undone merely because request/grace deadline expired.

Proposed choice: named 15-second connect/300-second whole request bounds matching
ChatGPT, injected-future signal tests, signal every producer before run drain,
typed shutdown/cancel causes with first-cause-wins, and existing 30-second coordinator
settlement before Compose 45-second force-stop window.

Cost if wrong: abruptly ended legitimate calls, retained locks, lost partial usage,
uncertain tools reported as rolled back or container forced termination.

### R4: Browser fixture success hides real transport/deployment failures

No-service constraints forbid Vite/Caddy/container smoke. A fake event stream can
accidentally bypass parser/reducer or give every unknown route a success response.

Proposed choice: built static assets through route fulfillment, exact typed API
fixtures, real SSE bytes through existing SDK, deterministic open/reconnect/abort
control, fail unknown requests, and explicitly defer real HTTP chunking/TLS/container evidence.

Cost if wrong: stale console works only against invented fixtures, tool/approval
cards fail in production, or local passes are misrepresented as live deployment proof.

### R5: Branch, documentation and Hermes reuse expand scope or overclaim parity

M10 cannot start its tracked implementation before explicit M9 merge. Existing
docs overdescribe old swarm/playground behavior; broad Hermes adoption could add
new memory/integration features rather than finishing current Rust/web delivery.

Proposed choice: ignored draft now, main-based M10 branch later, router/OpenAPI-led
docs, focused Hermes scoped-recovery/orderly-stop behavior reuse, retained existing
MIT notices plus notices for actual new adaptations, existing silent-pair pruning
verification, session-record TTL honestly pending product policy, and future community
connector lifecycle labeled unimplemented.

Cost if wrong: mixed milestone branches, misleading operator instructions,
unlicensed copied snippets, unnecessary dependency/integration work or false readiness claims.

## Controller rulings on the risks (binding)

These rulings override earlier choices. No separate pre-flight audit is required.

1. **R1 — Complete persistence generation.** Set the history path explicitly under the existing state volume. Treat stopped state, workspace/uploads, home/keyring and Caddy data as one backup generation; retain the vault password and canonical workspace root. A pre-upgrade snapshot is a rollback component, not a complete backup. Do not execute destructive restore, deployment or container smoke under the current no-service rule. Preserve existing retention behavior; do not invent an automatic session-record TTL without a product policy. Cost if wrong: more retained records or deferred runtime evidence rather than avoidable data/key loss.
2. **R2 — Claim, send and fenced settlement.** Release the global transaction and every state/std guard before Telegram transport. A claim is an owned identity token covering connector/chat/credential generation, outbound row and attempt; it must prevent duplicate in-process selection without holding a leaf mutex while reacquiring the control-plane transaction. Recheck identity on settlement, never resurrect deletion or overwrite a newer generation, retain ordering and existing retry classification, and never rerun the model for delivery retry. Successful send plus failed persistence remains visibly uncertain; do not promise exactly-once delivery. Any new persisted claim semantics require an explicit snapshot/version/backup ruling. Cost if wrong: duplicate or lost accounting, lifecycle races or lock inversion; blocked-send and stale-settlement regressions are mandatory.
3. **R3 — Bounded calls and typed shutdown.** Bind default provider clients to 15-second connect and 300-second whole-request limits, matching the existing ChatGPT client; retain caller-owned injected clients and fail initialization rather than silently falling back to an unbounded client. Signal producers before waiting for accepted runs, preserve the existing 30-second drain within Compose's 45-second grace, and handle Ctrl+C/SIGTERM once through the same drain. Carry typed owner/deadline/shutdown causes, preserve partial output/usage and uncertain effects, and never classify shutdown by message text. Cost if wrong: legitimate long calls end early or settlement is incomplete; injected signal/deadline/producer-order tests must prove the behavior without starting a daemon.
4. **R4 — Real console, simulated transport.** Browser fixtures load built assets and exercise the actual SDK requests, SSE-byte parser and UI reducers. Fail unexpected requests, control reconnect/abort deterministically, and do not inject finished UI state or mock the session reducers. No webServer, Caddy/Bun listener, container, database, live provider or browser installation is permitted. This Windows host has installed Edge but no observed packaged Playwright browser directories: support the existing `msedge` channel in the no-service test configuration without touching the owner's persistent browser profile; use available packaged Chromium on other hosts. Label this simulated browser evidence; HTTP chunking, TLS, provider acceptance and isolated deployment smoke remain unverified. Cost if wrong: fixture success hides integration failures; local passes must not be advertised as deployment readiness.
5. **R5 — Branch, docs and focused reuse.** Publish/implement M10 only on a new branch cut from updated main after the owner explicitly merges M9. Derive route/docs inventory from actual handlers/OpenAPI and keep auth failures distinct from unsupported older endpoints. Reuse small Hermes code/behaviors within existing Rust boundaries, retain source revision and MIT notices, and do not claim the future Discord/Drive/community connector lifecycle is shipped. Run the Rust Nx gate once with its core dependency and confirm both tasks, rather than duplicating the same core gate. Cost if wrong: mixed milestones, misleading docs or unnecessary integration work; publication must state manual evidence gaps and the exact resume point.

**R3 refinement (implementation evidence, 2026-10-12):** The existing job/scheduler
join tails are unbounded. Apply a shared 40-second deadline to the entire shutdown
from signal through producer closure, service/history drains and HTTP server
completion, inside Compose's 45-second grace. Retain the accepted-run coordinator's
30-second drain within it. Inject deadline/server/workflow futures in tests; no
real signals, sleeps as barriers or new dependency features. At expiry close live
streams, report static uncertainty and return; do not claim rollback or exactly-once
settlement. Existing committed nonterminal records enter interruption review on
restart. Cost if wrong: unpersisted tail output can be lost and external effects
remain uncertain, explicitly surfaced rather than an indefinite shutdown.
