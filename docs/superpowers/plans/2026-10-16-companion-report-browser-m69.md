# M69 actual browser report save — lean plan

**Status:** PARTIAL. Offline policy/console/typecheck pass; the one actual case failed Edge launch before navigation. No actual browser save accepted; all 14 captured identities retired. Preserve this case and continue a separately bound M70 repair.

## Objective and proof boundary

Add one finite acceptance case proving a built web console can list, preview, and download one synthetic report through an actual Rust daemon, then save the actual browser download with exact expected bytes. This proves the owner web-to-daemon HTTP/download path and browser save semantics. It does not prove model/provider generation, report quality, production readiness, or arbitrary workspace behavior.

No production feature change is proposed. M68 is settled at `cf5af4307b6af59c0d32478ed757b790cc5eb29d`; use that tree as the base after root revalidates it. Preserve M61 evidence/spec unchanged and M64 public-browser NOTRUN status.

## Frozen acceptance contract

- One new UUID v4, one-use release manifest with 64-hex nonce, expiry no more than 30 minutes, exact frozen-input hashes, no retry/replay/rebaseline/regrade. Failed attempt remains immutable; a future attempt needs a fresh UUID/manifest and root ruling.
- Fixed loopback endpoints only: Rust daemon `127.0.0.1:18752`, Vite preview `127.0.0.1:18753`. Preview gets `CI` unset and `UI_BACKEND_ORIGIN=http://127.0.0.1:18752`; Playwright runner gets `CI=1`.
- New case-local `workspace`, stores, logs, temp, IPC and browser output. Root/supervisor writes exactly one immutable synthetic CSV before launch, using declared bytes with UTF-8, BOM, CRLF, Unicode, quoting, and known SHA-256/length. No credential-like names or contents. Never overwrite existing case/output paths.
- Supervisor makes exactly one owner `PUT /api/workspace` to the real daemon with the case-local root and synthetic company/mission only. `routes/workspace.rs::handle_put_workspace` validates/persists only `WorkspaceConfig`; route registration in `routes/mod.rs` remains behind the normal mutation gate. No bootstrap agent, agents, tools, provider, model, account or ordinary workspace setup.
- M69 quiet-state oracle permits exactly this declared workspace config/root; all other control-plane collections and activity metrics remain empty/zero before and after browser work. Do not weaken or edit M61 `quiet()`.
- Browser uses built preview and real daemon only for these exact requests, once each and in order:
  1. `GET /api/workspace/files`
  2. `GET /api/workspace/file?path=reports%2Fledger.csv` (selection-triggered preview)
  3. After one explicit UI click, `GET /api/workspace/file/download?path=reports%2Fledger.csv`
- Require one actual Playwright download event and `saveAs` under the new case output. Compare saved bytes and source file to independent prelaunch expected bytes, length and SHA-256; assert filename `ledger.csv`, successful response, no redirects, no unexpected method/path, no additional download, no browser mutations, and no provider/model calls.
- Browser routing must forward actual requests with `route.continue()` and response listeners, following the working `realMemory` path in `apps/web-e2e/src/console/fixtures/api.ts`. Never synthesize success with `route.fulfill()` or use `route.fetch()` to reconstruct the daemon response; the earlier M61 attempt lost fetch metadata and received 403.

## Reuse and ownership

- Reuse immutable M61 ownership code `.superpowers/sdd/2026-10-16-companion-memory-http-m61/process_guard.py`: `OwnedJob`, suspended process creation/assignment, retained identity, bounded logs, listener ownership, kill-on-close, descendant capture, port retirement. Do not edit the M61 module or consumed artifacts. A new M69 supervisor may import these primitives; its own fixed input list must hash the exact imported file.
- Use `.superpowers/sdd/2026-10-16-companion-memory-http-m61/supervisor.py` as a structural reference only. It hardcodes M61 release/env/quiet assumptions, so author a separate M69 no-argument supervisor and M69 result schema. Reuse `preview-entry.mjs` only after including its hash and ensuring the preview env has `CI` removed.
- E2E target discovery recorded in `.superpowers/sdd/2026-10-16-companion-report-downloads-m68/e2e-targets-attempt-1.log`: project is `@animaos/web-e2e`; focused console target is `e2e-console` and depends on SDK/web builds; `typecheck` exists. The target uses `playwright.console.config.ts` and is uncached. Confirm exact target invocation/override syntax before running; do not guess a Nx flag.
- Reuse `apps/web-e2e/src/console/memory-http-restart.spec.ts` conventions: opt-in gate, release file/nonce validation, browser contexts with service workers blocked, hard timeouts, immutable result, selected single-worker/no-retry Playwright invocation. Add a new M69 spec; never alter M61 assertions.
- Extend only `apps/web-e2e/src/console/fixtures/api.ts` for narrowly admitted real workspace file traffic and, if needed, a small private M69 supervisor/fixture driver. `ConsoleFixtureOptions` currently supports `realStatic` and `realMemory`; fixture catch-all blocks unknown requests. New mode allows only the three exact GETs above, fixed daemon origin, bounded response sizes, response/status counting, redirects/errors rejected, external traffic refused. Leave all other mocks untouched.
- Public route and existing server oracle: `hosts/rust-daemon/src/routes/mod.rs` registers `GET /api/workspace/file/download`; `hosts/rust-daemon/src/routes/tests/workspace_download.rs` has M68 exact-byte coverage for BOM/UTF-8/CRLF/CSV quoting and unchanged source. No production/server changes needed.
- UI flow is in `apps/web/src/components/WorkspaceFiles.tsx`: file selection invokes `daemon.readWorkspaceFile(path)`; the explicit button calls `daemon.downloadWorkspaceFile(path, signal)`. Exercise that control in Playwright; do not call SDK/facade directly from acceptance code.

## Ownership and phases

1. **Offline RED first:** add one focused console Playwright regression/spec with an opt-in gate and fixture-level observable contract. Before adding real proxy behavior, prove that the UI selection/download requests are exactly list + preview + download and that an unapproved route is refused; do not use a missing helper/TypeError as the failure. Root reviews the concrete RED evidence.
2. **Harness integration:** add the narrowly scoped `realWorkspaceFiles` fixture path using continuation/listeners, plus M69 spec controls and result assertions. New private M69 supervisor owns release validation, fixture creation, PUT setup, process tree, exact metrics/routes and cleanup.
3. **Root-only launch:** no launch, test services or release until root binds, verifies hashes, and explicitly releases the single-use case. Implementer does not run acceptance activation.

Bound public paths: `apps/web-e2e/src/console/fixtures/api.ts`, plus new apps/web-e2e/src/console/fixtures/workspace-files-policy.ts, apps/web-e2e/src/console/workspace-files-policy.spec.ts and apps/web-e2e/src/console/m69-workspace-download.spec.ts. Private supervisor/config/result/fixture stay under `.superpowers/sdd/2026-10-16-companion-report-browser-m69/`. Avoid production files and M61 source/artifacts.

## Process, environment and cleanup rules

- Ports must be unoccupied before launch and proven owned by exact retained process identity while active. Keep fixed ports only if free; never kill unrelated listeners or reuse a live M61 case.
- Start only the frozen daemon executable, Vite preview, and one Playwright runner through retained `OwnedJob` handles. Every child starts suspended, joins the Job before resume, logs are capped and joined, descendants captured, kill-on-close enforced, identities retired, ports free at completion.
- Environment is a name-only allowlist, with TEMP/TMP and every store/workspace path redirected to the fresh case. No dotenv, provider/model credentials, ordinary user profile data, vault, DB server, container, or other service. Disable embeddings/STT/TTS and model pathways. No public browser/network request; block external origins and service workers.
- The supervisor's one config PUT is the only mutation and must be counted separately. Browser has zero mutating requests. Validate before and after that workspace path equals this case path, and all other persisted collections/metrics remain empty. Do not call `/bootstrap`, create an agent, or run a task.
- Use a finite total deadline <=300s with >=30s reserved for cleanup. Do not retry a failing request or rerun the same release. Always close Playwright context, close Job, join all log readers, verify every child/descendant exited and both ports retired, then write a single immutable result even on failure.

## Commands and checks (future; do not execute until bound)

- Static RED/fixture test: `bun x nx run @animaos/web-e2e:e2e-console --skipNxCache` with exact focused Playwright spec override confirmed from target config/help. Avoid whole-console suite for routine iterations.
- E2E typecheck: `bun x nx run @animaos/web-e2e:typecheck --skipNxCache`.
- Only if Rust production code unexpectedly changes, run `bun x nx run rust-daemon:test --skipNxCache`; expected scope has no Rust edits.
- Actual isolated browser acceptance is a separately released single-use supervisor invocation, not a normal Nx test or an automatic retry. Root owns command/activation.
- Success requires exact byte/hash comparison from independent fixture to source and browser-saved file; exactly 1 supervisor PUT, 1 each of the 3 browser GETs, 0 other real daemon browser calls, 0 model calls, empty non-workspace state, one immutable case result, clean child/job/log cleanup, and free ports.

## Top five risks / binding checks

1. **Quiet-state exception:** confirm serialized control-plane shape for a configured workspace and explicitly check every non-workspace collection remains empty; do not assume the M61 empty-store predicate still fits.
2. **Owner mutation gate:** prove the supervisor's loopback `PUT /api/workspace` supplies only required origin/headers and succeeds through the ordinary gate; no auth bypass or fake route. Confirm response/config does not create implicit agents.
3. **Proxy fidelity:** route continuation must preserve browser request metadata and binary body; count response bytes without retaining/reporting file contents. Any 403, redirect, unexpected path, or content mismatch fails the immutable case.
4. **Browser save semantics:** use real Chromium download event plus `saveAs`; verify output path is fresh and owned, MIME/filename expected, bytes exactly match independent fixture, and original source remains unchanged.
5. **Process and environment scope:** M61 guard primitives are reusable, but its supervisor is M61-specific. M69 must pin executable/source hashes, use isolated workspace/stores, enforce no provider/account/ordinary-profile paths, retain all process identities, and prove cleanup without touching unrelated listeners.

No broader product feature, new dependency, extra report type, second browser or additional milestone is in scope. Root decides binding and activation after reviewing this draft.

## Controller rulings on the risks (binding)

1. Ruling: use one supervisor-owned ordinary PUT /api/workspace for the exact fresh case root, then prove the persisted workspace equals that declared config and every other collection/run/connector/model metric is empty or zero — the existing config-only handler needs no agent or provider bootstrap, whereas M61's empty-workspace predicate must stay immutable — cost if wrong: fail the new case without accepting an alternate bootstrap or touching ordinary profiles.
2. Ruling: admit exactly one each of the browser list, selected-file preview and explicit download GETs, in order, at the fixed preview origin with the exact once-encoded report query; supervisor setup/metrics/readiness reads are separately budgeted, and known UI bootstrap/SSE/provider metadata remain synthetic — this establishes the actual selected boundary without treating fixture shell traffic as daemon evidence — cost if wrong: any extra real method/path/query/redirect/status or missing response fails; no route.fetch/fulfilled-success fallback or silent HTTP-only downgrade.
3. Ruling: add a small shared fixture-only workspace-file admission policy with typed deny-all RED stub, table-driven meaningful allow/deny/order/count tests, then integrate it with the existing continuation/response-listener pattern — pure offline authority regressions need no daemon/browser service and avoid copying the full memory fixture machinery — cost if wrong: preserve RED and block live release; no production SDK/web/Rust or M61 fixture behavior changes. Public ownership is api.ts plus new workspace-files-policy.ts, workspace-files-policy.spec.ts and m69-workspace-download.spec.ts under apps/web-e2e/src/console (policy helper may live in fixtures).
4. Ruling: one independently fixed CSV oracle, real download event and fresh case-local saveAs output must match raw bytes/length/SHA and preserve the source, with exactly one immutable success/failure result — a synthetic HTTP reply or rederived expected output would not prove the browser save — cost if wrong: record PARTIAL with the consumed UUID, no retry/replay/rebaseline/regrade; no report/model quality or universal browser parity claim.
5. Ruling: reuse the exact immutable M61 OwnedJob primitives and new M69 supervisor, pin imported/source/build/fixture/runtime hashes, start only suspended assigned children, retain descendant/log/listener identities and reserve cleanup time under a300s total cap — ordinary listeners, services, profiles and consumed cases must remain untouched — cost if wrong: fail closed before launch or retain failure evidence; no global PID kills, detached child, account/provider/DB/container/public-network activation. Release expiry is at most five minutes and an exclusive one-use marker is consumed before any child launch.

## Bound oracle and execution

Independent fixture relative path: reports/ledger.csv; basename ledger.csv; exact **84 bytes**, SHA-256 **8FEEFE05A78114AC5648616031900111D679834277BC8272EF757B6215BE6FEE**. Root wrote the UTF-8/BOM/CRLF/Unicode/quoted-CSV fixture before feature source; the release pins its raw hash. The supervisor copies only those bytes into a new case, never generates an expectation from browser output.

Root publishes the plan first, then source-only workers add policy RED and a private supervisor in disjoint files. Root alone runs commands/fmt/Git/activation. Confirmed e2e-console target uses static intercepted assets with no application server; policy tests require no page/browser fixture. After meaningful behavioral RED, implement policy/fixture/spec, freeze source, do one harness/native-ownership batch and one final e2e review, one combined fix wave, targeted offline/full console and e2e typecheck gates. Reuse exact M68 Rust2942/28ignores/46suites/lint/build and SDK200/3 plus web1518/typecheck/build evidence by preserved source/build hashes; rerun only if those inputs change. No Rust/dependency/production-source changes are planned. Only after offline gates and reviewed exact manifests may root release the single new isolated case.

Record the actual case, cleanup and limits without replaying M61/M64/M52 evidence. Update master status/handover, docs/format/protected hashes, explicit per-command unsigned commit/push/PR/attach/standing merge and exact tree verification. Preserve nx.json/anima.yaml/.superpowers from staging; no stash/reset/restore/path checkout/worktree. No real personal file/profile/account/provider/model/ordinary daemon/DB/container/deployment activation. A finite synthetic daemon/preview/browser started only inside the owned case is authorized. General account/quality/always-on parity remains open.

## Recorded outcome

M69 offline gates pass: 47 focused policy tests, full console 65 passed / 2 opt-in live cases skipped, and E2E typecheck. The one released actual case `9b09c8ee-5dee-4ed1-8c3c-73a304ee66ed` failed before navigation during Edge launch (Windows PathService/non-default debugging directory refusal); no browser report was saved. It retired all 14 captured process identities in 12.969 seconds, joined clean logs and freed both test ports. The independent 84-byte CSV source is unchanged; 2,026 runtime hashes, fixed executable hashes and immutable M61 inputs remain preserved.

One ownership review and final E2E review, one combined fix wave, meaningful offline RED and frozen-source gates are recorded. The launch failure is retained as failure; future repair uses a fresh plan/branch/case without changing this oracle or consumed evidence. No production Rust/SDK/web/dependency change.
