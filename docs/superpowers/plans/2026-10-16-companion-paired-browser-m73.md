# M73: Use the accepted paired headless Chromium runtime

## Goal and evidence

M72’s actual case saved the synthetic 84-byte ledger with the exact expected SHA, verified the three ordered real GET routes, and completed the explicit download checks. It then failed the final `context.pages() == [page]` assertion because an extra page existed; its cause is unknown. Cleanup retired 20 known processes, proved Job active count zero with no errors, freed the ports, and finished in 17.312 seconds. No M72 error-87 discovery gap occurred. Preserve M72’s case, raw eight source snapshots, and pinned Git versions; its overall acceptance remains PARTIAL. Do not replay or regrade it.

`docs/companion/browser-work.md` lines 61–67 already documents an accepted paired runtime: Playwright Core 1.59.1 with Chromium headless-shell revision 1217, SHA-256 `f7c1ef91a3e287b64509a9733fc8fd43678cf4e2765d67f4d0eceb0e39ecd026`. The installed binary is `C:/Users/leoca/AppData/Local/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-win64/chrome-headless-shell.exe`. Branded Edge 155 is not accepted because of native-download and background-traffic incompatibilities.

Use that already accepted local browser pair for M73. This plan does not infer why M72 opened another page. Keep the single-page assertion unchanged so the new run still tests the required browser behavior. Do not install or download browser packages or add dependencies.

## Scope and ownership

- No production changes. Reuse the existing Playwright-based browser acceptance runner and its installed paired Chromium binary.
- Root owns a mechanical M72-to-M73 copy of the current supervisor/spec/plan bindings and the new binary admission checks. Preserve all M72 UI, route, save, cleanup, and single-page assertions unchanged.
- Keep M72’s public-source history and eight raw source snapshots immutable. New M73 bindings and evidence use a fresh case and fresh manifest.
- Do not add Hermes’ Python `agent-browser` CLI. The pinned Hermes source at commit `1fd18277267152893a8a24792b79213e7ca3f899` starts isolated local headless Chromium through that CLI; Anima already has a maintained Playwright worker and a documented accepted installed browser pair. Reuse the existing Anima integration.

## Offline browser admission

Before issuing a release or starting any child process, verify that the installed paired runtime is available and matches the declared contract:

- Playwright Core version exactly `1.59.1`.
- Chromium headless-shell revision exactly `1217` and browser version `147.0.7727.15`.
- Executable resolves to the installed headless-shell path above and is a regular, non-redirected file.
- SHA-256 exactly `f7c1ef91a3e287b64509a9733fc8fd43678cf4e2765d67f4d0eceb0e39ecd026`.

Fail before release and before process launch if any file, version, revision, path, or digest check is missing or mismatched. Do not fetch a replacement or silently fall back to branded Edge. Bind the exact browser executable and digest into the fresh one-use release manifest, and include the admission code and values in the manifest closure.

## Acceptance contract

Run one fresh isolated browser case after source review and offline gates. Keep the M72 scenario and all its assertions: supervisor-owned workspace setup; exactly three ordered real Rust GET routes for list, preview, and download; owner-visible explicit download click; actual save through the browser; exact synthetic ledger bytes and SHA; unchanged source file; no other real browser requests; and one exclusive result after the persistent context and owned children have closed.

Keep the original one-page assertion. M73’s purpose is to test the scenario with the runtime pair already accepted for Anima. A second page, route drift, failed save, byte mismatch, browser traffic, or cleanup error still fails the case. Do not weaken assertions to hide the M72 symptom.

## Validation and risks

Before release, run the focused offline manifest/admission tests, guard and profile tests, workspace-files policy tests, full offline console suite, e2e typecheck, and provenance/archive closure checks. Confirm historical M72 artifacts remain unchanged and current M73 inputs are hashed in its own manifest. Root owns commands and release.

Use a new UUID and nonce, exclusive one-use release, five-minute expiry, fresh short runtime, and the established bounded case and cleanup deadlines. Permit no retry, replay, rebaseline, or regrade. No provider, model, account, external browser, ordinary profile, package install, or public network access is part of acceptance.

Top risks to bind before execution:

1. **Version mismatch:** a similarly named or stale browser binary could invalidate the accepted pair; require revision, version, absolute path, and digest together.
2. **Hidden network/install fallback:** resolve only the installed local binary; missing or changed runtime must fail before release.
3. **Changed assertion scope:** keep the extra-page assertion and every route/save check byte-for-byte from the final M72 case.
4. **Historical evidence drift:** preserve M72’s PARTIAL result and snapshots; only new M73 evidence can describe this runtime choice.
5. **Overstated result:** a successful paired local case proves this finite browser flow only, not Hermes browser-agent parity or general browser reliability.

This plan records authorized implementation work; it does not claim implementation or acceptance. A fresh browser release requires completed offline checks and reviews.

## Controller rulings on the risks (binding)

1. Ruling: use the already installed accepted pair only — validate installed engine1.59.1 metadata, unique headless-shell revision1217/browserVersion147.0.7727.15, fixed canonical regular non-reparse executable and known SHA before release and launch — cost if wrong: release rejects or a new case fails; no fallback/download.
2. Ruling: no install, network discovery or executable version probe — metadata plus the previously accepted binary digest are read offline; no executable is started by admission — cost if wrong: unavailable installation remains unavailable.
3. Ruling: mechanically preserve strict assertions — only M73 case/plan/image bindings and offline admission are changed, including the single-page assertion unchanged — cost if wrong: comparison/review blocks release rather than masking a symptom.
4. Ruling: archive every old result exactly — preserve M72 raw eight inputs, Git blobs, case and PARTIAL result, plus M71/M70/M69/M61 history; no replay/regrade — cost if wrong: provenance gate blocks release.
5. Ruling: keep scope honest — one new isolated synthetic Files download case after one review batch/final E2E review/fix wave/gates; passing would establish this flow only — cost if wrong: provider or general task quality remains unproven.

Reuse decision: use Anima's maintained Playwright integration and already accepted installed browser pair. The pinned public Hermes session implementation at1fd18277267152893a8a24792b79213e7ca3f899, tools/browser_tool_session.py, follows isolated local headless Chromium through agent-browser. Importing its Python/global CLI lifecycle would duplicate the existing boundary. No literal source port or new dependency is needed; existing MIT notices remain.
