# M72: Bounded discovery-gap accounting for Windows job cleanup

## Objective and evidence

When the Windows guard enumerates members of its retained Job and `OpenProcess` returns Win32 error 87 (`ERROR_INVALID_PARAMETER`), the process may have exited between enumeration and opening its handle. M71 observed error 87 for PID 568 without obtaining a handle or creation time. This is consistent with an enumeration/open race, but its cause is unproven. The Job still drained successfully, but the case had no handle or creation-time evidence for that member, so the supervisor conservatively rejected it.

M72 should resolve only that narrow race. It must re-query the same retained Job once after capturing error 87. Continue only when the PID is absent from that bounded member list. A still-present PID, failed or oversized re-query, or any other `OpenProcess` error remains fail-closed. Do not retry `OpenProcess`.

M71 is settled evidence: PR 77 is merged at source `67174b35e8fb4d2d7360a7c2f172ef14538116b6`, main `ca733936e55833a4e1859f6d72f2250d320f98f3`, tree `f6cb62df2737af994f50b5a2e56373ea100ba4d2`. Its finite browser case failed during supervisor capture with error 87; seven Edge images were captured, but there was no browser-result, navigation, or save witness; it produced no browser result and no save. Cleanup itself was accepted: 14 known processes retired, Job active count reached zero before handle close, no cleanup errors, ports free, stable profile witness, 12.64 seconds. Preserve this result and its seal as M71 history; do not replay, regrade, or rewrite it.

The M71 source snapshots, pinned source Git blobs, M69/M70/M61 one-off cases, and their historical outcomes remain immutable. The two reusable guard source files may receive the M72 fix. Update any current-source checker to validate archived M71 provenance against its snapshots/Git blobs rather than requiring the new guard bytes to match M71’s old current-source hashes.

## Boundaries

- Change only the reusable Windows process guard and its deterministic fake-WinAPI tests: `apps/web-e2e/tools/windows_process_guard.py` and `apps/web-e2e/tools/test_windows_process_guard.py`.
- Root owns the M72 supervisor/schema and fresh case bindings. Mechanically derive the browser spec from the final M71 spec, changing only M72 release, plan, runtime root, and fixture bindings. Do not rewrite its UI, route, download, profile, or result assertions.
- Do not alter production code, dependencies, process caps, pinned-image admission, primary process handling, M71’s error-31 semantics, logs, deadlines, or port ownership rules.
- No provider/model/account activity, ordinary services, containers, or shared user profiles. The acceptance remains a single isolated local browser/download case.

## Guard behavior

On a failed member `OpenProcess`:

1. Capture the Win32 error immediately, before another API call can overwrite it.
2. For error 87 only, perform exactly one bounded member-list query against the same retained Job.
3. If the bounded list is valid and no longer contains the PID, append one bounded discovery-gap record and continue. Do not fabricate an `OwnedProcess`, image, creation time, or pinned Edge identity.
4. If the PID remains, the query fails, the result exceeds the existing 128-member bound, or the error is not 87, fail closed and preserve the diagnostic.

The gap record must identify the PID, captured error 87, confirmed absence, and unavailable image/creation identity. Cap records at 128. It is evidence of a discovered-but-unopenable Job member, not an owned process identity.

A gap becomes `coveredByJobDrain` only after the retained Job reports active process count zero and the entire cleanup has zero errors. Any unresolved gap blocks case acceptance. Keep the Job handle authoritative through the existing cleanup deadline and preserve its current kill-on-close fallback. Do not weaken known-process verification, log joins, or the existing requirement to prove Job drain before closing the handle.

## Ownership and tests

The test owner should first add a meaningful fake-WinAPI RED test for error 87 when a PID is absent after the single re-query. It should assert the gap evidence, no second `OpenProcess`, and no invented owned identity. Then cover:

- PID still present after re-query: fail closed; no retry.
- Error other than 87: fail closed without re-query.
- Re-query failure and oversized member list: fail closed; retain original-open error 87 as context and the actual member-query failure code when that query fails.
- A discovery gap is not covered when Job drain fails or cleanup has any error; it is covered only with active count zero and no cleanup errors.
- The 128-gap bound rejects an additional record without admitting an image-less process as Edge.
- Existing error-31 retired-member handling, pinned primary-image admission, duplicate identity handling, constructor ownership, and cleanup ordering remain covered.

The guard owner then implements the minimal behavior against those tests. Keep Win32 calls injected/faked in unit tests; never create a native process or call real Win32 APIs from the unit suite.

## Validation and finite acceptance

Before release, run the focused deterministic guard tests, the existing profile and workspace-files policy suites, the full offline console suite, the e2e typecheck, and the repository’s archive/provenance closure checks. Confirm M71 raw source snapshots, pinned historical Git blobs, and case artifacts remain byte-for-byte unchanged. Current reusable guard/test source may change under M72 provenance. Root owns all commands and reviews.

After the source fix, one independent ownership review, one final public-e2e review, and one combined fix wave, root may bind a single fresh M72 case. Use a new UUID and nonce, an exclusive one-use release, a five-minute expiry, a fresh `.superpowers/r72/<UUID>` short runtime, the M72 private `HERE/fixture.csv` and oracle, and a 300-second case limit with a 30-second cleanup reserve. No automatic retry, replay, rebaseline, or regrade.

The browser case remains the settled M71 scenario: one supervisor-owned workspace setup; three ordered real Rust GET routes for list, preview, and download; owner-visible explicit download click; actual browser save; exact immutable 84-byte fixture oracle; source unchanged; one exclusive result after context and child cleanup. The guard’s newly observed member gap must be accounted for and fully covered by zero-error Job drain before the result can be accepted.

## Top risks and rulings

1. **TOCTOU race remains:** a second `OpenProcess` attempt would repeat the race and add ambiguity. The allowed resolution is one same-Job membership re-query after error 87, never an open retry.
2. **PID reuse:** absence from the current Job membership list is required; do not infer process identity from PID alone or attach the gap to an earlier process record.
3. **False coverage:** never claim individual retirement for an absent PID. Only whole-Job drain can cover the gap, after active processes reach zero and cleanup reports no errors.
4. **Error clobbering:** retain error 87 before querying membership; preserve it even if the re-query or later cleanup changes Win32 last-error state.
5. **Historical evidence drift:** preserve M71’s failed case and source snapshots as immutable history while allowing current guard source to advance under M72 provenance.

This plan authorizes only the bounded guard/test fix and a new separately bound acceptance case after reviews and gates. It does not claim M72 implementation, case execution, or acceptance.

## Controller rulings on the risks (binding)

1. **Ruling: exact error 87 only** — capture the failed open error immediately, then perform one bounded member-list requery against the same retained Job handle; continue only if the PID is absent, without retrying OpenProcess — cost if wrong: an unowned process could escape the evidence boundary.
2. **Ruling: all other failures remain fatal** — other open errors, PID still present, failed requery, or more than 128 members reject capture; a failed query records its own native error with original-open 87 context — cost if wrong: diagnostics could conceal access denial or incomplete enumeration.
3. **Ruling: at most 128 distinct gap events** — do not deduplicate by PID, invent an OwnedProcess, image, creation time, or individual retirement, or let a gap satisfy a pinned Edge image — cost if wrong: PID reuse could create false identity or browser evidence.
4. **Ruling: only error-free whole-Job drain covers gaps** — require jobEmptyBeforeClose true, activeProcessesBeforeClose zero, and every cleanup error absent; unresolved gaps reject acceptance — cost if wrong: a partial cleanup could be published as complete.
5. **Ruling: version reusable code normally and preserve old evidence** — keep raw M71 snapshots, pinned Git blobs and original cases exact; root mechanically derives the new supervisor/spec from FINAL M71 with only milestone bindings and the explicit gap acceptance assertion — cost if wrong: historical results or route/save assertions could drift unnoticed.
