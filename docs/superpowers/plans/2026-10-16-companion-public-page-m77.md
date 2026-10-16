# M77: One public static-page browser case

## Goal and boundary

Add one fresh, finite acceptance case for Anima's existing operator-pinned HTTPS `BrowserPort`, using only `https://httpbin.org/html`. The case checks one public page through the real Rust worker and actual browser observation. It does not change the product runtime, SDK, UI, grants, worker policy, or browser limits.

The M64 Example Domain case and its failed heading precondition remain immutable and **NOT RUN**. Do not replay, regrade, repair, or relax M64's oracle. There is no fallback site or second public attempt. If M77's own source, DNS, TLS, or browser precondition fails, record this case as not run/partial and stop.

The root has independently checked the candidate page's source template: one H1, one complete public-domain paragraph, and no scripts or page assets. Before any M77 case release, root must separately perform a bounded raw HTML read (normal TLS, no redirects, at most 16 KiB, 15-second timeout), verify exactly one H1 and one paragraph with no active assets, and record a normalized full-paragraph SHA-256/UTF-8 length plus the literal heading. Root must also perform one DNS metadata lookup, validate the selected canonical public IPv4 through the current M63 exclusions, and freeze the single pin. This plan authorizes no such network operation or browser launch by implementers.

## Reuse and provenance

Reuse the existing browser contracts and real `BrowserService` in `hosts/rust-daemon/src/tools/browser_tests.rs`, especially the ordinary owner approval helper `m64_allow`, the existing M64 setup/evidence/cleanup flow, and the worker's operator-pinned HTTPS policy. Reuse is constrained to helpers that do not mutate M64's config, contract, oracle, or test body. M77 has its own namespaced module and case contract.

Use the current accepted `.superpowers/sdd/2026-10-16-companion-retirement-observation-m74/supervisor.py` process-ownership pattern and unchanged `apps/web-e2e/tools/windows_process_guard.py`. Do not reuse the obsolete M61 guard or modify M74 artifacts. No dependency or upstream code copy is needed; the selected integration is the existing Anima Rust `BrowserPort` and local paired browser installation.

## Owned source paths

- `hosts/rust-daemon/src/tools/browser_tests.rs`: append only the `public_page_m77` module declaration. Preserve all existing M64 helper bodies byte-for-byte.
- New hosts/rust-daemon/src/tools/browser_tests/public_page_m77.rs: M77-only child contract, bounded file admission/oracle checks, deterministic unit tests, and one ignored Windows actual case. Import parent helpers with `use super::*`; do not duplicate the executor or browser service.
- New public plan only: this file. Root owns the private M77 release/supervisor/evidence directory, binding rulings, all commands, formatting, Git, and any actual-case release.

## Frozen case contract

The single ignored Windows test uses a new root-issued UUID and nonce, expires within five minutes, admits exactly once, and has an outer 180-second deadline with 20 seconds reserved for cleanup. It validates the exact pinned Rust test binary, Node/worker/browser program hashes, operator config hash, source/oracle hashes, environment and case paths before creating the real service. Ordinary test runs cannot start a public request.

The policy contains only canonical `https://httpbin.org`, one reviewed public IPv4 pin, exact `GET /html` route ID `page`, and `inputIds: []`. The worker connects only to the pin with normal hostname TLS verification and uses the existing static-page restrictions: no redirects, scripts, cookies, external assets, selected uploads, or fallback resolution. The test makes exactly two ordinary approved actions: Navigate `page`, then Observe with screenshots false. The expected successful public request count is exactly one, for `/html`; no clicks, extra route requests, artifacts, writes, accounts, helpers, or model calls are allowed.

The oracle is frozen before release and compares only fields actually present in `BrowserObservation`: route ID `page`, positive document generation, the exact visible H1 role/label, and the complete normalized paragraph as represented in visible text nodes. Compare the joined target text's SHA-256 and UTF-8 byte length to the independent pre-case oracle; do not use worker internals or infer unobserved DOM. Require two successful tool receipts, unchanged browser grants, zero model calls, and retained process/service/scope cleanup evidence.

Unnamed structural containers may be omitted. Record observed shortened/omitted counts, but refuse acceptance if the H1 or target paragraph is shortened, absent, or incomplete. The result must not claim complete DOM coverage, JavaScript compatibility, general page quality, OS egress isolation, or autonomous planning.

## RED and GREEN tests

Add deterministic M77-only tests before implementation is considered complete:

- accept the exact frozen child contract once and reject wrong milestone/nonce, expired or reused release, changed program/config/oracle hashes, wrong origin/route/pin, and malformed or oversized contained files before service startup;
- accept an observation only when it has the exact route, a positive generation, one visible exact heading, and the complete normalized target paragraph matching both digest and byte length;
- reject missing/duplicate heading, changed fact text, partial/omitted target text or wrong target roles, wrong route, zero generation, extra actions/requests, and model calls;
- keep one ignored Windows integration test with the exact two Navigate/Observe actions, one public GET, existing approval/grant checks, Stop, awaited service shutdown, task join, child retirement, and zero-model witness.

No test may make an unapproved public request. The ignored case may run only after root has frozen source and DNS evidence, built and hashed the exact binary, and separately issued the one-shot release.

## Validation and ownership

Root runs the focused native filter for `m77_` through the Nx `rust-daemon:test` target, Rust formatting, and the required full `rust-daemon:test`, `rust-daemon:lint`, and `rust-daemon:build` targets with the repository's Windows isolation settings. Run the actual ignored case only after all reviews and offline gates pass and root explicitly releases the fresh one-shot case. Preserve all raw M64/M74 snapshots and archives; never use their case artifacts as M77 input.

## Top five risks

1. A source or pin mismatch could send the worker to a different destination; freeze the exact hostname, single validated address, TLS identity, route, and hashes before the case.
2. A drifting page could invalidate the oracle; any mismatch ends M77 as not run/partial, with no replacement page, changed pin, or relaxed expected text under the same release.
3. Browser observations may omit or shorten page text; require the complete target paragraph and report structural omissions without claiming complete DOM coverage.
4. An ignored test can accidentally make a public request during ordinary gates; require a fresh, one-use contract and environment, and refuse before service construction on any admission mismatch.
5. A successful task result alone does not prove process retirement; preserve the existing bounded owner-job, Stop, join, cleanup and zero-model/grant witnesses, failing the case if cleanup is uncertain.

## Controller rulings on the risks (binding)

1. Ruling: bind only httpbin.org/html and one reviewed public IPv4 — normal hostname TLS, exact GET /html and no proxy, redirect, DNS refresh or fallback in the worker — cost if wrong: a request could reach the wrong destination; admission and TLS must fail closed.
2. Ruling: source evidence precedes pin and case release — one bounded independent raw HTML read freezes the exact heading and full normalized paragraph digest/length; one explicit DNS pin lookup follows only acceptance — cost if wrong: the single case becomes NOT RUN or PARTIAL, with no alternate page, pin, second launch or later oracle adjustment.
3. Ruling: verify complete selected content through supported fields only — require the exact visible heading and whole normalized paragraph text digest/length; report structural omission/shortening counts without asserting complete DOM coverage — cost if wrong: a bounded observation could be mistaken for arbitrary page comprehension. No per-node hidden/shortened flag is invented, and M64's stricter oracle remains unchanged.
4. Ruling: ordinary gates cannot activate the public case — a separate ignored Windows test needs contained bounded files, fresh UUID/nonce/expiry, exact program/config/source hashes and an atomic one-use release before any service construction — cost if wrong: ordinary tests could create unintended public requests.
5. Ruling: reuse settled physical ownership without another guard rewrite — unchanged current public Windows Job guard, a short owned private case/temp path, exact paired shell1217 and retained action tasks; total180s with20s cleanup reserve, Stop/service shutdown/zero-model/grant and Job-empty witnesses — cost if wrong: uncertain cleanup fails the case and remains recorded.

The source/oracle contract has no HTML title, URL, href, selector or hidden-node
field. Complete normalized paragraph text must be independently frozen; matching
only a prefix or keyword is insufficient. Structural omissions remain visible in
the result and never establish full page or autonomous reasoning parity.

Reuse alternatives: direct Hermes Python copy and a second browser process/API
adapter would duplicate Anima's maintained Playwright BrowserPort boundary.
Reuse the existing implementation and source-only test helpers instead. The public
Hermes browser session comparison remains pinned to
1fd18277267152893a8a24792b79213e7ca3f899; no literal upstream code is copied and no
dependency or notice change is needed. No normal profile, account, credential,
provider, model, daemon, web preview, database or container is activated.
