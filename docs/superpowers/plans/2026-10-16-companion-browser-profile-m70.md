# Companion browser profile launch repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run one fresh isolated M70 copy of the M69 actual browser report-download case with private short Windows profile paths so the frozen Edge executable can launch and save the same exact synthetic report bytes.

**Architecture:** Keep M69 and its failed case immutable. Create one new short runtime root `.superpowers/r70/<UUID>` with conventional profile/AppData mappings and a precreated private Edge User Data default directory; launch Playwright with a separate explicit temporary user-data directory under that short case's `temp`. Reuse the M69 policy, fixture, oracle, actual Rust route, preview and M61 process ownership unchanged, while a new M70 spec binds the fresh runtime layout and release.

**Tech Stack:** Rust daemon, Vite built preview, Playwright with the frozen Microsoft Edge executable, TypeScript e2e fixture/policy, Python M61 Windows Job Object process guard.

**Status:** PARTIAL. Profile18/policy47/console65pass3liveskips/typecheck pass. The one actual case failed supervisor process capture before a browser witness; all14 known identities retired/portsfree/profile witness stable, but final descendant capture failed. No browser-save or profile-fix acceptance; preserve case and diagnose a separately versioned guard.

---

## Evidence and boundary

M69 source `206ddc4d5dcdd97b15db7fff28802dad9b5455cb` settled in PR75 and main merge `3b2bc6c0ccd9e3079887233229667cd0b795ef06`; verified main tree `71da6c8d561de8495c34255cadc17f2d9e83210d` is recorded by root. The one M69 case `9b09c8ee-5dee-4ed1-8c3c-73a304ee66ed` failed before browser context/navigation: Edge launch failed, 14 process identities retired, ports were free after 12.969 seconds, source was preserved, and no report was saved. Its `failed-case-seal.json`, consumed release, all runtime inputs, and result stay immutable. This is the accepted failure evidence; do not launch or grade it again.

M70 repairs only the browser runtime/profile setup. It reuses the exact M69 frozen daemon, Edge and Node image hashes; exact list/preview/download policy; fixture bytes/oracle (84 bytes, SHA-256 `8FEEFE05A78114AC5648616031900111D679834277BC8272EF757B6215BE6FEE`); M61 `process_guard.py`; and M61 preview entry. No production SDK/web/Rust/dependency change, new browser, provider, account, model, service, DB/container, public network, or M64 retry.

## Frozen M70 acceptance contract

- Root creates and releases one new UUID v4 and one-use 64-hex nonce manifest, expires within five minutes, exact full runtime/input/image hashes. A consumed UUID is never retried, replayed, rebaselined or regraded. Preserve any failed case and issue a fresh UUID only after root review.
- Case root is exactly `.superpowers/r70/<UUID>` under the repository. Before creating it, reject existing files/directories, symlink/junction/reparse redirection, and any canonical path outside the repo. Use short path segments and do not put the runtime case under the long M69 SDD tree.
- Create case-local workspace, stores, logs, IPC, browser output, `temp`, and isolated `profile` trees. Redirect `USERPROFILE`/`HOME`, `APPDATA`, `LOCALAPPDATA`, `HOMEDRIVE`/`HOMEPATH`, `TEMP` and `TMP` into this case; do not inherit ordinary user profile/environment values.
- Precreate only the private expected default Edge profile directory under `profile/AppData/Local/Microsoft/Edge/User Data/Default`. This path and all profile mappings must be rooted under the new case and validated before any child starts.
- Launch a separate, exclusively created Playwright browser profile via its persistent-context API at an explicit fresh path under the short case `temp` (exactly `temp/pw`). It must not use the ordinary Edge default profile or the precreated default directory. Browser downloads and transient files stay within this case.
- Keep the built preview/daemon loopback ports exactly `127.0.0.1:18753` and `127.0.0.1:18752`; prove free before launch and owned while running. Run preview with `CI` unset and explicit `UI_BACKEND_ORIGIN=http://127.0.0.1:18752`; runner gets `CI=1`.
- Preserve one supervisor-only `PUT /api/workspace` to configure only the case workspace and synthetic mission; no agent/bootstrap/model/provider calls. M70 quiet proof allows exactly that workspace config and requires all other persisted collections and activity metrics zero.
- Browser must use built preview UI, service workers blocked, and make exactly one each in order: `GET /api/workspace/files`; `GET /api/workspace/file?path=reports%2Fledger.csv`; after one explicit click, `GET /api/workspace/file/download?path=reports%2Fledger.csv`. No other real daemon browser method/path, no redirect, no browser mutation, no extra download or external request.
- Require actual Playwright download event + `saveAs` to fresh case-local `browser-output/ledger.csv`; prove exact independent 84 bytes/hash against predeclared fixture and unchanged source. Keep the downloaded synthetic content out of final supervisor result/logs.
- Reuse exact M69 continuation/response-listener behavior; do not fulfill or reconstruct successful real daemon responses. Keep M69 static policy tests and all M61 behavior unchanged.
- One finite case with <=300 seconds total and >=30 seconds reserved for cleanup. Start only frozen daemon, preview, one runner and owned descendants in the retained Job. Retire every process identity and log reader, prove both ports free, then publish one immutable M70 result. No automatic retries.

## File ownership and tasks

Root owns private short-runtime descriptor/layout validation, controller, UUID/manifest creation and release. Astra owns the new M70 private supervisor only after root releases that lane. One web author owns the single new public spec; existing M69 policy/fixture files remain immutable inputs for M70.

### Task 1: Add M70 profile-layout checks and spec

**Files:**

- Create: apps/web-e2e/src/console/m70-workspace-download.spec.ts (new file after plan publication).
- Read/reuse only: `apps/web-e2e/src/console/m69-workspace-download.spec.ts`
- Read/reuse only: `apps/web-e2e/src/console/fixtures/api.ts` and `workspace-files-policy.ts`

- [ ] Bind the new spec to `M70_REPORT_BROWSER`, `M70_CASE_DIR`, `M70_RELEASE_FILE`, and `M70_NONCE`; require a new UUID, five-minute expiry, exclusive consumed marker and exact image/source hashes before opening Edge.
- [ ] Validate canonical case root `.superpowers/r70/<UUID>`, every environment profile mapping, precreated private `Microsoft/Edge/User Data/Default`, and the explicit temporary Playwright user-data path under the case `temp`. Fail closed if any resolved path leaves the new case or hits a reparse redirection.
- [ ] Copy only the M69 assertions needed for the same UI list/preview/click/saveAs behavior, three exact HTTP responses, 84-byte independent oracle and unchanged source. Do not mutate M69 spec or its case evidence.
- [ ] Use `playwright.chromium.launchPersistentContext(tempUserDataDir, { executablePath: frozenEdge, headless: true, acceptDownloads: true, serviceWorkers: 'block', ... })`; keep context teardown bounded, then verify counters and write a fresh M70 browser witness exclusively after context closure.
- [ ] Offline failure evidence is the sealed M69 Edge launch failure; do not re-run M69. Keep the new spec opt-in so ordinary e2e runs skip before starting a browser.

### Task 2: Create M70 short-runtime supervisor and release (root/Astra lanes)

**Private files (root-owned unless explicitly delegated):** new M70 descriptor/controller/release under `.superpowers/sdd/2026-10-16-companion-browser-profile-m70/`; runtime directory only under `.superpowers/r70/<UUID>`.

- [ ] Reuse unchanged `.superpowers/sdd/2026-10-16-companion-memory-http-m61/process_guard.py` primitives; freeze and verify their exact hash. Never edit/replay M61 or M69 supervisor/runner artifacts.
- [ ] Before process launch, validate release shape, new UUID/nonce, <=5-minute expiry, exact hashes, fixed image identities, free ports, absent new runtime path, no junction/symlink/reparse redirection, and exact short-root containment.
- [ ] Create the fresh runtime and all stores/output/profile folders exclusively. Populate the declared CSV bytes by copy from the pinned M69 fixture; verify 84-byte length/hash before and after. Precreate the private conventional Edge User Data/Default directory; never touch ordinary profile data.
- [ ] Scrub environment by name allowlist, redirect every profile/temp/store/workspace variable into M70 case, and launch Edge only through Playwright's explicit temp user-data path. Pin process executable and retain complete process/descendant identities in the owned Job.
- [ ] Configure workspace using one actual-daemon owner PUT; preserve M69's empty-nonworkspace-state proof and exact HTTP method budgets. Keep supervisor setup/readiness requests separate from browser request counts.
- [ ] Verify exact browser result schema, all three real GET witnesses, no forbidden routes, save path/hash/source equality, final quiet state, process/log retirement and free ports; write one final result with success/failure and no raw report content.

### Task 3: Review and gates

- [ ] One owner review of supervisor/profile paths and launch ownership; one final e2e review of the new M70 spec and reused M69 fixtures; one combined fix wave.
- [ ] Run meaningful offline profile/layout tests and M69 policy tests, full web-console e2e, and `@animaOS-SWARM/web-e2e` typecheck. Use existing Nx targets and verify any focused argument syntax from target help before running it.
- [ ] Root checks source hashes and runtime manifest, reviews final result schema, and alone decides whether to release the single M70 case. No new actual browser case before the offline gates and final reviews pass.

## Suggested verification commands (root executes)

- `bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache`
- `bun x nx run @animaOS-SWARM/web-e2e:typecheck --skipNxCache`
- For the focused profile-layout/policy test, confirm supported Nx `e2e-console` argument forwarding from the project target/help before using a file filter; do not guess a flag. The full console run remains required.
- The M70 actual case is not a routine Nx test invocation: only the root-released, no-argument, single-use M70 supervisor may activate the finite owned process tree.

## Top five risks

1. **Edge profile path compatibility:** the repair must address what Edge/Playwright checks at launch. Validate exact private profile hierarchy and canonical paths before starting any child; keep Playwright's active profile distinct and short under case temp.
2. **No ordinary profile access:** sanitized environment must redirect every known home/AppData/temp variable; child launch must not inherit unapproved environment variables. A mismatch fails before launch.
3. **Path redirection/length:** a preexisting path, junction or reparse point could escape the private short root or restore a long path. Reject before creation and verify resolved path containment for every folder.
4. **Evidence preservation:** M69 failed seal remains PARTIAL/immutable; M70 must use entirely new UUID/release/result paths and the same independent fixture oracle. No fallback that calls M69 proof successful.
5. **Process cleanup and scope:** reuse M61 retained Job identity/kill-on-close rules; no global PID kill, ordinary listener interference, provider/account, public network, DB or container. Preserve >=30s cleanup reserve under the 300s cap.

Expected completion claim is limited to whether this one fresh isolated M70 Edge browser case launches, performs the actual owner file list/preview/download path, saves the exact synthetic bytes, and cleans up. M69 remains failed before navigation; M64 remains NOTRUN; M52 remains PARTIAL; general browser/provider/account quality remains open.

## Controller rulings on the risks (binding)

1. Ruling: use the short fresh case root plus conventional private profile/AppData/Local and Roaming, precreated Microsoft/Edge/User Data/Default and active temp/pw — M69 observed an inconsistent profile layout and a261character cache path, so these are concrete repair candidates rather than a proven cause — cost if wrong: retain a failed fresh case without retry/regrade or disabling Edge safeguards. No ordinary profile inheritance.
2. Ruling: use launchPersistentContext only for this newly created disposable temp/pw directory, then retain the same blocked service workers, explicit downloads, fixture routes and UI/save assertions — this API provides an exact short user-data path without adding conflicting Chromium flags — cost if wrong: offline/review or the actual case fails; no desktop/session/profile compatibility claim or changed product persistence.
3. Ruling: root .superpowers/r70 and every ancestor/leaf must resolve without symlink/junction/reparse redirection; exclusive UUID creation, conventional folders and projected cache path below240characters are verified before launch and in browser — containment and path length are necessary constraints of the repair — cost if wrong: refuse the new case before process activation; never delete or reuse an existing case.
4. Ruling: reuse the exact M69 policy/fixture/image/independent84byte oracle and immutable M61 guard/preview, copying only a new M70 spec/supervisor with explicit new bindings — failed M69 is meaningful observed RED and must remain failed/immutable — cost if wrong: input/seal verification blocks release; no model/report-quality/full-parity claim. One offline profile-contract regression set, focused policy, full console/typecheck; no repeated producer gates with unchanged code.
5. Ruling: root alone releases one new UUID/nonce/<=5minute manifest after one ownership review/final E2E review/combined fix wave and offline gates, with all children suspended-assigned to the retained Job and a300s total/30s cleanup reserve — prior user authorization covers finite isolated local testing and standing merge/continue — cost if wrong: save immutable failure/cleanup witnesses and stop that case; no automatic retries, ordinary services/accounts/providers/DB/containers/public-network activation.

Root publishes this plan before source. All decisions and failure receipts stay in the M70 ledger. The M69 consumed case and its runtime closure stay byte-identical; M64 public browser NOT RUN and M52 PARTIAL remain unchanged. No production code or new dependency is in scope.

## Recorded outcome

M70 offline gates pass: 18 profile regressions, 47 policy tests, full console 65 passed / 3 opt-in live skips and E2E typecheck. Its one actual case db8c54c7-0756-404f-8a83-2f09783c8947 failed during supervisor process capture before any browser-result witness. All 14 known captured identities retired in 12.141 seconds and both ports are free, but final descendant capture failed, so complete cleanup evidence is not accepted. The private profile identity witness stayed stable; the independent 84-byte CSV is unchanged and no report was saved. All 2,026 runtime inputs, fixed images, original M69 inputs/case and immutable M61 inputs remain preserved.

One ownership batch and final E2E review, one combined fix wave and frozen-source gates are recorded. Profile/path/env/stale/reparse tests pass. The actual capture failure remains failed; future versioned guard repair must pass deterministic regressions without changing this oracle or consumed evidence. No production Rust/SDK/web/dependency change.
