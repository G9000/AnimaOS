# M61 isolated built-console memory HTTP and restart acceptance

## Scope and provenance

Prove the M60 built Memory page can read status, edit/delete one synthetic memory
through actual Rust HTTP and recover its exact saved row across one quiet forced
daemon restart. The existing console fixture supplies shell/bootstrap/SSE data;
only the declared memory endpoints reach the isolated daemon. This is selective
memory integration evidence, not a full real-backend shell, provider/semantic
quality, graceful shutdown, reboot or external-account acceptance.

M60/PR66 merged at c0ebc9f89afb797826a293245880ff76abe1d858, source
521e09bb2be4f9441e3a961501bbcfd052cc6ab0, exact tree
192cb6087f4f645a79fede8b3ed114688ad43560. Fresh web1491/121files, SDK183/three service
skips/type/build/docs/format pass; native inputs retain M59 Rust2856/26 ignores/
45 suites/lint/build. All protected hashes/nx unchanged. M52 remains PARTIAL;
consumed M47/M52 profiles/cases are immutable and never reused or regraded.

Reuse existing console fixture/static build/stream helpers and Playwright Edge
fresh contexts; extend its origin/static option narrowly rather than another UI
harness. Reuse M52's name-only environment scrub, fresh JSON/SQLite-file store
configuration and kernel process identity/quiet forced-stop principles, ported to
native Python because PowerShell stalls here. Use existing Windows Job Object
ownership patterns from the native worker implementation. No new dependency,
third-party copy, credential/profile import or product/runtime API change.

Installed Vite node.js:33616 uses preview?.proxy ?? server.proxy. Current web config
proxies /api and /health to UI_BACKEND_ORIGIN when CI is unset; preview inherits it.
Set UI_BACKEND_ORIGIN=http://127.0.0.1:18752, preview127.0.0.1:18753 and unset CI in
preview only. LocalOwnerPolicy supports ANIMA_ALLOWED_UI_ORIGINS; add exactly
http://127.0.0.1:18753 for this isolated process, preserving owner checks.

## Exact ownership and two phases

Branch g9000/companion-memory-http-m61 from verified M60 main. Root alone executes
commands/processes/format/Git. Publish this plan before implementation. Public
test changes only apps/web-e2e/src/console/fixtures/api.ts and new
apps/web-e2e/src/console/memory-http-restart.spec.ts. Default console fixtures remain
fully offline; the real case is skipped unless explicitly opted into the fresh
M61 release. No normal dev startup, source runtime/SDK/config/dependency changes.

Private root .superpowers/sdd/2026-10-16-companion-memory-http-m61 contains process
guard/supervisor, logs, release, synthetic stores, request counters and result.
No private artifacts are staged. No M52 profile/evidence data is read or copied.

Phase1: source-only authors implement one bounded supervisor and UI case; root
runs a harmless finite child ownership self-test and offline existing console
regressions. One process-ownership batch review and one final web/SDK/test review,
one combined fix wave. No real daemon/preview before root startup-release.json.
Phase2: root verifies frozen inputs/reviews/dry-run/ports, writes release, then
one actual finite case. Existing user approval permits isolated local testing;
the release authorizes no ordinary profile/account/model/provider/device action.

## Process and profile contract

- Exactly numeric-loopback ports18752 daemon/18753 preview, prove both free first.
  Never touch8080/4200/55432 or ordinary listeners. Fresh live/stores/workspace
  directories under this task, refuse any existing nonempty profile on first start.
- Reuse M52 env configuration: ANIMAOS_RS_PERSISTENCE_MODE=memory,
  ANIMAOS_RS_CONTROL_PLANE_FILE=<fresh>/control-plane.json,
  ANIMAOS_RS_HISTORY_SQLITE_FILE=<fresh>/history.sqlite,
  ANIMAOS_RS_MEMORY_FILE=<fresh>/memory.json,
  ANIMAOS_RS_MEMORY_EMBEDDINGS=disabled, query expander off, STT/TTS disabled,
  run ceilings2/1. Strip inherited ANIMA/provider/database/connector credential
  configuration by NAME without printing values; never repurpose HOME/CODEX_HOME.
- Launch only current built target/debug/anima-daemon.exe and current Node/Vite
  preview CLI, with exact binary/asset hashes recorded. Direct already-built
  processes keep ownership smaller than Cargo/Nx launcher trees; Nx supplies build
  and test gates. No PowerShell, dependency installation or global configuration.
- Use a fresh Windows kill-on-close Job Object. Create each owned child suspended,
  assign before resume, retain real process handles and verify image/creation time
  and listener owner. On assignment failure terminate the still-suspended child.
  A helper crash closes the job and retires only its descendants. Keep helpers
  hidden, bounded, no shell-built process commands or PID-only/global kills.
- One finite supervisor budget (at most5min) owns daemon, preview and test runner/
  browser descendants. IPC accepts only one restart and one stop using a fresh
  nonce/frozen release; no arbitrary command execution or paths. Bound logs.
- Before forced restart/stop, use provider-free /metrics and saved synthetic
  snapshots to prove zero runs/queue/connectors/history pending. Do not call actual
  /api/status: its provider_responses can read the normal account vault. Capture
  exact owned identity, stop only that handle, prove18752 free, restart same fresh
  stores once. Label quiet forced restart, not graceful drain or exactly-once.
- At completion/failure close all owned handles/jobs, bounded joins, and prove both
  isolated ports free plus no recorded owned identities remain. Ordinary services
  stay untouched. Preserve synthetic evidence; no recursive deletion.

## HTTP/browser contract and finite oracle

Extend installConsoleFixture with opt-in origin/real-static behavior while
preserving default ORIGIN and offline fulfillment. Fresh Playwright context only,
service workers blocked. Use actual built preview assets on127.0.0.1:18753.
Install a strict method/path memory allowlist before navigation; all shell/SSE/
provider/account/connector/task requests stay mocked or refused before reaching
the actual daemon. Refuse nonloopback network and unknown actual routes.

Real memory methods: one seed POST /api/memories with unique synthetic content,
the fixture companion agent ID (bootstrap is mocked), bounded GET recent/search/
facts/entities/relationships and exactly two explicit readiness GETs. One UI PATCH
on the actual returned memory ID, one final UI DELETE with current expected text.
No recall expansion, evaluate/retention/consolidation/rebuild, task/run, OAuth,
voice, connector, grant or provider route. Record method/path counts and bridge
boundaries; no claim that the real daemon has a configured live companion.

Declare initial/updated content, ID, importance/tags independently. Show the row
and real disabled-index/local fixture status; assert no readiness request before
click. Edit through the built UI, check exact real saved response/file fields.
Request one quiet forced restart through bounded IPC, reload a fresh page/context
with the same synthetic bootstrap, prove the same ID/updated bytes survive, make
the second explicit status read, then delete through the built UI and verify
absence in real API/store. No model calls/tokens/messages or automatic replay.
Save desktop and390px screenshots of synthetic UI; inspect layout/console errors.

Only one live case per release/result file, no overwrite/rescue/regrade. Record
failure honestly; root may bind a distinct corrected synthetic case after a real
defect, with original evidence retained. No consumed actual M47/M52 replay.

## Controller rulings on the risks (binding)

1. Ruling: selective real memory bridge plus mocked shell/provider/account paths —
   bootstrap/status can otherwise read the normal credential vault — cost if wrong:
   activation blocks or case fails; never present this as full real-backend parity.
2. Ruling: fresh task-local stores, scrub by name, explicit loopback origins/ports,
   disabled embeddings/expander/STT/TTS and no run routes — isolated local testing
   is authorized while accounts/ordinary profiles remain excluded — cost if wrong:
   stop owned children, retain failed evidence, no remote/provider action release.
3. Ruling: suspended-create/assign/resume with real handles and kill-on-close job —
   wrapper PIDs and abort requests do not prove child retirement — cost if wrong:
   self-test/review/startup gate blocks execution; never kill unrelated processes.
4. Ruling: exact independently declared row/status/method counts and one restart —
   a rendered fixture is not actual persistence evidence — cost if wrong: case
   remains failed/partial, no retries or full memory/semantic-quality claim.
5. Ruling: frozen source/binary/assets, offline regressions and two bounded reviews
   before root release — process and UI helpers are distinct boundaries — cost if
   wrong: one combined fix wave then gates; no unreviewed activation or extra audit.

## Gates, publication and resume

Root confirms current Nx e2e target/help before forwarding filter flags; normal
web build and web-e2e:e2e-console offline suite, ownership self-test, actual finite
case, screenshots/network/store/process proofs, touched format/docs/hash checks.
Existing web1491/SDK183/native2856 results remain revision-specific for unchanged
product source; no redundant Cargo or full web unit run unless source changes.
Update master/parity/harness/handover, explicit-path unsigned commit/push/PR/attach,
standing authorized merge with exact tree/raw-hash proof. Resume private M61
progress.md. Never stage nx.json/anima.yaml/.superpowers; no stash/reset/restore/
path checkout/worktree. No real accounts/models/devices/ordinary services,
database servers/containers/Docker55432/public deployment. Full parity unclaimed.
