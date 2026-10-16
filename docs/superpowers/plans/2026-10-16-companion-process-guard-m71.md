# Companion Windows process-guard retirement-race repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Windows Job Object descendant capture tolerate only a confirmed already-retired child whose image lookup fails with Win32 error 31, while preserving fail-closed identity, image-root, and cleanup guarantees.

**Architecture:** Add a new reusable testable Windows guard module under apps/web-e2e/tools (new folder after publication); never modify the consumed M61 guard or M69/M70 acceptance. Keep the existing kernel-object call shape injectable; avoid a second broad wrapper layer. Inject a narrow Win32 API adapter so deterministic unit tests exercise the retirement race and cleanup ordering; use the new guard in a fresh M71 supervisor while the M71 browser spec changes only its milestone/release/runtime bindings.

**Tech Stack:** Python 3, `ctypes` Win32 process/job APIs, `unittest` with fake Win32 API, Playwright TypeScript e2e.

**Status:** implemented and locally gated; actual whole-Job cleanup accepted, browser save PARTIAL. The one case failed at job-member OpenProcess87/PID568 before a handle; original case/raw inputs sealed. No retry/regrade.

---

## Evidence and scope

M70 source `a308d0b7273cd3deab2774902e113fc876d31085` merged to main `b94878c010cf61a35febdd6ac7c2d389b962bde4`, tree `4c7bb8f625b9b5f7b9733b58ac6562c743bba022`. Its one case `db8c54c7-0756-404f-8a83-2f09783c8947` failed before any browser witness: `capture_descendants` failed, 14 known captured identities were retired, ports were free after 12.141 seconds, profile witness remained stable, and no report was saved. The consumed release, M70 seal/result, runtime inputs, and M69/M70 cases remain immutable. Cleanup is not accepted as complete because final descendant capture failed; the initial exception must not be guessed.

Separate ordinary M69 validation observed a retiring-child `QueryFullProcessImageNameW` error 31 race. This is evidence for the narrow guard behavior, not proof that the M70 initial exception was that race. The intended repair accepts a missing image only when the retained process handle is confirmed signaled and the exact image-query error is 31; every live-process lookup failure and every wait/time/identity failure remains fatal.

M71 adds no product feature, dependency, process policy relaxation, provider/model/account call, ordinary profile access, service, DB/container, public network, or browser-image change. The future M71 browser acceptance uses the same fixed Edge/Node/daemon images, exact M70/M69 CSV bytes/oracle, workspace setup and three ordered real GETs, UI click, persistent short Edge profile, download/save/hash assertions, and a new UUID only after unit tests/reviews/gates. Never replay M69 or M70 cases. M64 remains NOTRUN and M52 remains PARTIAL.

## Guard API contract

Create apps/web-e2e/tools/windows_process_guard.py (new after publication) as an isolated Windows ownership module. Keep import side-effect free. Separate a small `WinApi` protocol/wrapper (process/job calls, last-error retrieval) from ownership logic so tests can inject a deterministic fake without loading `kernel32`.

- A retained `OwnedProcess` owns its open process handle from construction until one close. Capture PID and creation time while the handle is valid. The caller retains ownership until construction succeeds; constructor failure paths in launch/capture close the handle exactly once. Preserve this existing transfer convention.
- For a live identity, require `WaitForSingleObject(handle, 0) == WAIT_TIMEOUT`, successful `GetProcessTimes`, successful `QueryFullProcessImageNameW`, and exact pinned-image/root admission. Any wait error, time-query failure, image-query error, image change, PID/creation-time mismatch, or strict-root mismatch fails closed.
- For a process that is already retired, require `WaitForSingleObject(handle, 0) == WAIT_OBJECT_0` before considering an image-query failure. The only tolerated failure is `QueryFullProcessImageNameW` error 31; record `image: null`, the exact error code, and confirmed-retired status. A successful image query still undergoes strict pinned-root admission. Do not infer retirement from PID-table absence, `OpenProcess` failure, timeout, or error 31 alone.
- `GetProcessTimes` failure is fatal even for a retired process. Revalidation (`verify`) must retain PID/creation-time checks and reject identity changes; image-null is valid only for the explicit retired/error-31 witness and may not be accepted for a live process or as a substitute for root admission of expected primary processes.
- Keep strict exact executable admission for launched daemon/Node/runner and the required observed Edge image. Known descendant images remain canonical observations bound to retained Job membership; this module does not invent a new universal descendant executable allowlist. A live image-query failure or wrong launched executable fails closed. A retired error-31 child may be recorded without an image only after Job membership and signaled-wait proof; do not broaden launch allowlists.
- Job cleanup retains the Job handle through bounded termination and active-process count verification. Use `TerminateJobObject` for the owned job, poll/query active count until zero within deadline, then close the Job handle. If explicit termination/query fails, retain diagnostics and invoke kill-on-close as the bounded fallback; cleanup errors remain visible and cannot produce a passed result.
- Preserve bounded structured diagnostics (operation, Win32 code captured immediately, PID/creation time, wait state, image nullable, retirement reason). Never log environment values, file contents, credentials, or raw exception text. Preserve `GetLastError` immediately at the failing Win32 call before any other native call.

## Files and ownership

- Create: apps/web-e2e/tools/windows_process_guard.py (new after publication) — new tested guard; do not edit `.superpowers/sdd/2026-10-16-companion-memory-http-m61/process_guard.py`.
- Create: apps/web-e2e/tools/test_windows_process_guard.py (new after publication) — deterministic fake API tests, no Windows process launch.
- Create later under root/Astra ownership: a new M71 private supervisor/release/runtime descriptor that imports the M71 guard and freezes all inputs; leave M61/M69/M70 supervisors and consumed evidence unchanged.
- Create: apps/web-e2e/src/console/m71-workspace-download.spec.ts (new after publication) — narrow M70 spec copy changing only M71 release/plan/environment/runtime-case bindings. Keep M69/M70 source specs unchanged.
- No changes to SDK, web, Rust daemon, dependencies, or M69/M70 shared fixture/policy. If the new spec needs a corrective shared change, report it for a separate root ruling rather than broadening this plan.

## Tasks

### Task 1: Write deterministic failing guard tests

- [ ] Provide a fake Win32 API with retained fake handles, controlled wait states, creation times, image paths, error codes and ordered call recording. Tests use ordinary Python `unittest`, no process/PID/listener APIs.
- [ ] Prove retired-child case: process is confirmed signaled; image lookup returns error 31; capture records nullable image plus retirement/error evidence, remains within the owning Job, and does not attempt PID-based termination.
- [ ] Prove fail-closed cases: live child + error 31; wait failure; `GetProcessTimes` failure; other image-query error; successful but wrong pinned launched image; changed image; changed creation time; process not in Job; image-null on a live primary process.
- [ ] Prove ownership/cleanup: constructor failure paths close the caller-retained handle exactly once; repeated capture does not duplicate/close retained identities; wait/query diagnostics preserve the original last-error value; cleanup calls job termination, observes active count zero, then closes Job; timeout/query failure is retained and uses bounded kill-on-close fallback.
- [ ] Run the focused unit tests to confirm these fail on the new implementation contract for expected behavioral reasons, not import or missing-method errors. Root owns test commands; retain this RED receipt before implementation.

### Task 2: Implement the narrow guard repair

- [ ] Implement injectable wrapper and explicit handle ownership first; keep production Win32 bindings local to the guard and initialize only when constructing a Windows-backed guard.
- [ ] Add retirement-aware identity capture using wait state before image lookup; permit only the exact signaled + error-31 retired case to produce `image: null`.
- [ ] Preserve successful image and creation-time checks, strict pinned launched-image checks, Job membership proof, and live-process rejection. Do not use null image to admit any expected primary executable.
- [ ] Make close/cleanup ordering observable through the fake API: bounded TerminateJobObject, active-process-zero proof, Job close, with kill-on-close fallback and errors recorded on failures.
- [ ] Pass all deterministic tests without changing M61 or M70 code. Add focused tests only for uncovered safety conditions introduced by implementation; avoid tests that merely mirror helper internals.

### Task 3: Bind the fresh M71 browser spec and supervisor (root/Astra/web lanes)

- [ ] Root authors fresh descriptor/release for one new UUID only after unit RED is reviewed; no existing M70 UUID/path is reused. Runtime remains short and private, with the established M70 profile contract.
- [ ] Astra creates an M71 supervisor using the new guard and the exact owned-process cleanup protocol; freeze the new guard, supervisor, spec, plan, fixture/oracle, browser/runtime dependencies, and fixed executable image hashes before release.
- [ ] Web author copies M70's spec to `m71-workspace-download.spec.ts`, updating only the milestone/release/plan/runtime environment bindings. Preserve the same 84-byte independent oracle, exact list/preview/download order and counts, UI action, filename, actual browser `saveAs`, source equality, result schema except `milestone: M71`, and closed-context witness ordering.
- [ ] Root reviews process ownership and the spec as separate bounded reviews, conducts one combined fix wave, then runs unit/policy tests, full console, e2e typecheck and manifest/source checks. No live browser case before these pass.
- [ ] Only root may release one fresh finite actual case after checks. Case cap <=300s with >=30s cleanup reserve, five-minute nonce expiry, one-use marker consumed before child launch, no retry/replay/rebaseline/regrade. Any failure is sealed and remains partial.

## Commands and verification (root executes)

- Focused unit tests (new files only): `python -m unittest discover -s apps/web-e2e/tools -p test_windows_process_guard.py -v`.
- Existing M69 offline policy and new M71 profile/browser spec through the confirmed Nx console target; consult Nx help/target config before adding a file filter, and do not guess argument syntax.
- `bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache` for the full console gate.
- `bun x nx run @animaOS-SWARM/web-e2e:typecheck --skipNxCache`.
- The real M71 run is not a routine test command; only a root-released no-argument single-use supervisor may launch the finite owned process tree.
- Unit success requires retired/error31 narrowly accepted; every live/image/wait/time/identity/root violation rejected; constructor and cleanup handle order exact; all diagnostics bounded and preserve the native error observed at source. Browser success requires the unchanged 84-byte CSV oracle, exactly one workspace PUT and three actual ordered GETs, exact saved bytes/source hash, complete owned cleanup, and free ports.

## Top five risks

1. **Retirement classification:** error 31 may be transient or unrelated. Accept it only after a successful zero-time wait reports signaled and Job membership is established; all other states fail closed.
2. **Identity continuity:** PID reuse and constructor races can corrupt evidence. Keep the handle open, bind creation time, verify retained identity, and close transferred handles exactly once on every failure path.
3. **Root allowlist weakening:** a nullable image must never admit a live process or replace strict image checks for expected executables. Allow the exception only for an already-retired Job descendant with the exact native error and explicit diagnostic.
4. **Incomplete cleanup:** merely observing free ports does not prove all descendants exited. Terminate the retained Job, wait for active-process count zero, retain every identity/log-reader outcome, then close; a failed capture or cleanup blocks acceptance.
5. **Evidence contamination/replay:** M70 failed before a browser witness and is not a retry target. M71 must use a fresh UUID, exact frozen inputs and immutable result, preserve all previous seals, avoid ordinary profile/service/provider access, and make no general browser reliability claim.

This plan covers one native guard fix plus one fresh browser acceptance case only. It does not certify general Windows process supervision, every Edge launch, browser quality, providers, accounts, or M64/M52 evidence. Root adds binding rulings and publishes the plan before source work.

## Controller rulings on the risks (binding)

1. Ruling: allow image:null only for a Job descendant with successfully retained creation time, image-query error31 captured immediately, and a signaled retained process handle; live/wait/times/other-error and launched-image failures remain fatal — earlier ordinary validation proves this retirement race while M70 exact cause is unknown — cost if wrong: deterministic strict negative tests or one fresh case fails; never invent an image or regrade M70.
2. Ruling: preserve existing caller-owned-until-construction-success handles, launch suspended assignment/exact executable checks, Job membership and creation-time continuity; inject the existing kernel call shape for unit tests rather than adding a general process abstraction — the original guard already supports narrow fake kernels at its seams — cost if wrong: handle-order/identity tests fail before any activation; no constructor double-close or PID-based termination.
3. Ruling: retain the Job through explicit TerminateJobObject, prove known process waits/identities and join logs, close those retired process handles, then query active count to zero before Job close; retain kill-on-close and all errors on failure — Microsoft documents that ActiveProcesses can retain exited members until process references are released — cost if wrong: conservative cleanup failure blocks acceptance under the existing deadline, with immutable diagnostics. See [Job accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information) and [TerminateJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject).
4. Ruling: capture bounded structured operation/native-error/PID/creation/wait diagnostics immediately, preserve previous image observations separately from a later unavailable query, and make image-null incapable of satisfying fixed Edge observation — M70 lost its initial error detail and free ports did not prove complete capture — cost if wrong: fake-API evidence or actual result is failed/partial; no environment/file/provider content in diagnostics.
5. Ruling: publish plan, run a meaningful retired31 behavioral RED against a NEW copy of old guard (compatible unused parameters allowed), then source-only implementation/new M71 short-profile supervisor/spec, one ownership batch/final E2E review/combined fix wave and unit/profile/policy/fullconsole/typecheck/frozen-input gates before one new finite release — this is a concrete fix, not replay/pass hunting — cost if wrong: retain failure and stop that case; M61/M69/M70 sources, seals and consumed cases remain unchanged. Root owns commands and release; no ordinary profile/account/provider/model/DB/container/public-network/deployment activation.

The new runtime root is exactly .superpowers/r71/UUID, active disposable profile temp/pw, with the same profile containment/path budget and independent84byte CSV. M71 supervisor must require jobEmptyBeforeClose and zero cleanup errors; nullable descendant image observations cannot satisfy the existing pinned browser-image witness. It freezes the NEW public guard and immutable private bound-plan snapshot, never the mutable public outcome plan. Root copies the exact predeclared fixture/oracle into the M71 private task before source; both supervisor and spec use those new identical pinned copies. Existing producer code/images, M61 preview and all old closures remain unchanged. No new dependency.

## Recorded outcome

Guard18/profile18/policy47/fullconsole65pass4liveskips/E2Etypecheck pass. One ownership batch and final E2E review, one combined fix wave, mechanical reuse proof and2,028 frozen runtime inputs are recorded. The one actual case5a8ee9a8-5969-4e06-8406-8ed7d4f4d82e failed at job-member OpenProcess error87/PID568 before obtaining a handle or creation time. All14 known identities retired in12.64s; explicit retained-Job termination/drain reached ActiveProcesses0 beforeclose with zero cleanup/log errors and both ports free. Profile witness and independent84byte source stayed unchanged; no browser-result/save occurred.

Exact raw source snapshots and release/case evidence are retained. Future normal maintenance of the reusable public guard/tests must pin the old committed version and snapshots; never modify/regrade the consumed case or claim its image-null/open87 observation was browser-save success. Existing M61/M69/M70 scripts/cases stay byte-identical.
