# M74: Confirm descendant retirement after image-query errors

## Evidence and scope

M73’s browser result passed the one-page, three-route, exact 84-byte save, and post-context checks. Its overall result remains PARTIAL: descendant PID 26340 had process-image error 5 at creation time `134366078117796641`; the initial wait returned 258, and no post-error wait was captured. The cause of that image-query failure is unknown. Cleanup still proved 11 known processes retired, Job active count zero, no errors, ports free, and 13.063 seconds. Preserve the M73 case and source snapshots as historical evidence; do not replay or regrade it.

M74 resolves only image-query errors 5 (`ACCESS_DENIED`) and 31 (`ERROR_GEN_FAILURE`) for discovered descendants already held and verified as members of the same retained Job. If the initial wait did not already confirm retirement, capture the image error and perform exactly one nonblocking `WaitForSingleObject` on that same retained handle. Accept an image-unavailable retirement observation only if the wait returns `WAIT_OBJECT_0` (0) and creation-time lookup succeeded. Never retry image lookup, reopen by PID, sleep, or infer identity from PID.

Keep `OwnedProcess.identity`’s existing five identity fields. On confirmed retirement, report `image: null`, the captured `imageQueryError`, and `retiredAtCapture: true`; retain the successful PID and creation time. Preserve the image-query error if the wait changes last-error state. If the post-error wait fails, retain its native error alongside the original image error.

## Failure rules

- Initial signaled wait plus image error 5 or 31 may yield the explicit unavailable retirement observation for an admitted descendant.
- Initial wait 258 followed by image error 5 or 31 gets one same-handle, zero-time wait. Only a signaled result is accepted.
- Still-active 258, failed or unexpected wait values, missing/failed creation time, and every other image error remain fatal.
- Primary launches remain strict: exact pinned executable image must be observed. Never allow an unavailable image to pass primary executable admission.
- Existing prior-image identity checks, duplicate handling, Job membership, log ownership, process/gap caps, deadlines, cleanup order, and error-free Job-drain rules remain unchanged.

## RED tests and implementation

Add focused fake-WinAPI tests to the existing guard suite before changing guard code. First RED selector: an `OwnedProcess(..., allow_retired_image=True)` descendant with initial wait 258, successful creation time, image error 5, and a subsequent wait on the same handle returning 0 must currently fail on image lookup. The regression should assert exactly one zero-time post-error wait, no second image lookup, and the unavailable observation with original creation identity.

Cover the initial-signaled error-5 case; image errors 5 and 31 while wait stays 258; wait failure and unexpected wait values with preserved diagnostics; other image errors with no second wait; creation-time failure; and primary-launch strictness. Confirm cleanup still reports the original known image and creation time for owners whose images were captured successfully. All tests use injected fake APIs; no native process or Win32 launch is allowed.

Guard ownership is the existing reusable Windows guard and its tests. Root mechanically derives the M73 supervisor/spec into a new M74 release and case binding, retaining all browser assertions and the paired Chromium runtime. No production, SDK, UI, dependency, provider, model, or account changes are in scope.

## Validation and bounded acceptance

After one ownership review, one final e2e review, and one combined fix wave, run the focused guard tests, existing profile and workspace-files policy checks, full offline console suite, e2e typecheck, and archive/provenance checks. Confirm M71/M72/M73 source snapshots, pinned Git blobs, and cases remain unchanged.

Only after those gates may root authorize one fresh finite paired-Chromium case with a new UUID/nonce and one-use release, no retry/replay/regrade, the existing 300-second ceiling and 30-second cleanup reserve, exact fixture oracle, and owned-process/port/profile cleanup. Preserve the accepted browser boundary: this proves the bounded Anima owner-file browser workflow, not autonomous browser quality or general retirement causality.

## Top risks

1. **Error clobbering:** capture the image error before waiting; preserve both native errors if the wait fails.
2. **False retirement:** accept only a signaled wait on the same retained handle; PID disappearance or a second open is not evidence.
3. **Primary-image bypass:** permit unavailable image only for an already admitted descendant path; primary launches remain exact-image strict.
4. **Identity loss:** retain creation time and all prior-image checks; never synthesize an image or infer an owner from PID alone.
5. **Overclaiming evidence:** the M73 cause remains unknown and its result remains PARTIAL; only a new M74 case can test the narrow fix.

This plan authorizes only the bounded descendant observation change and, after review and gates, one new separately bound acceptance case.

## Controller rulings on the risks (binding)

1. Ruling: capture native errors before another call — preserve original image5/31; failed post-wait reports its own native error plus original-image context — cost if wrong: diagnostics could conceal the actual failure.
2. Ruling: one zero-time observation on the retained handle only — initial signaled0 is sufficient, initial258 requires exactly one post-image wait0; 258/failed/unexpected values reject with no sleep/image retry/open retry — cost if wrong: an active child could be mislabeled retired.
3. Ruling: primary executable checks stay strict — the existing allow_retired_image descendant path alone may accept null image; initial primary launch never receives fallback, and null images cannot satisfy the pinned browser witness — cost if wrong: arbitrary executables could gain false admission.
4. Ruling: preserve successful creation identity and prior image — require GetProcessTimes success, keep the five-field identity shape, existing held-handle verification, membership, caps, gap rules and zero-error Job drain — cost if wrong: PID reuse or missing evidence could invalidate ownership.
5. Ruling: maintain versioned libraries and immutable outcomes — preserve M71/M72/M73 raw source snapshots, pinned Git blobs and original cases; new public guard/tests may advance normally, and root copies final M73 case mechanically with only M74 bindings — cost if wrong: provenance/comparison gates block release, no old-case regrade.

Reuse decision: reuse Anima's existing retained-Windows-Job and accepted paired Playwright boundary. The public Hermes session implementation pinned at1fd18277267152893a8a24792b79213e7ca3f899 uses a separate Python/agent-browser lifecycle; importing that lifecycle would duplicate the existing ownership boundary. No literal source port, new dependency or runtime expansion is needed; notices remain preserved.

## Recorded outcome

Offline paired-runtime admission10 passed; Guard 34, profile 18, policy 47 and offline console gates/typecheck passed. One ownership review, one final E2E review, one combined fix wave and mechanical case reuse are recorded. Fresh case 2de6769f-ecbe-4969-862c-d004469a5dcf completed in 14.391s: browser save ACCEPTED, 1 saved reports, 11 known identities retired, 0 discovery gaps covered by zero-error Job drain, active count zero before close, ports free, stable private profile and exact 84-byte source preserved. The installed engine1.59.1/headless-shell1217 was used with the original single-page, route/save and cleanup assertions. Old M72 remains PARTIAL; no cause is inferred for its second page. M73 browser flow passed but its overall supervisor case remains historical PARTIAL; this new case never regrades it. Descendant image5/31 now requires a confirmed signaled observation on the same retained handle; still-active, unknown-error and primary-image paths remain strict.

Original case and raw ten source inputs are sealed; preserve historical Git versions when maintaining reusable guard/tests. No production, provider, account, ordinary service or dependency change.
