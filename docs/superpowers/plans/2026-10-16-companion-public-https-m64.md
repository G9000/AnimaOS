# M64 one pinned public HTTPS DOM acceptance

Status: partial; source-only driver gated, actual public acceptance NOT RUN after independent heading precondition failure.

## Proposed acceptance rules and interfaces

The only proposed live destination is `https://example.com/`, with a single
operator-frozen public IPv4 address. The case exercises the installed Windows
Node/worker/Chromium transport, ordinary browser owner grant and approval,
verified HTTPS transport, and JavaScript-disabled DOM observation. It does not
exercise model planning, an account, providers, a daemon API, a web UI, arbitrary
browsing, or OS-wide egress isolation.

1. **Oracle.** Before case activation, root independently reads the current
   public source and fixes one small set of `BrowserObservation.nodes`
   `(role, label)` expectations in a release. Prefer a heading named `Example
Domain` and one distinctive visible paragraph/link label only if those
   exact roles and labels are observable with the pinned renderer. Root records
   the independent source and literal expected strings. A changed page fails;
   no post-observation rebaseline. No title, paragraph element, link `href`,
   DOM selector or document text field is asserted: the interface has none.
2. **DNS pin.** Root makes one pre-release DNS metadata lookup for
   `example.com`, chooses one canonical dotted public IPv4, records the lookup
   evidence, validates it against M63 special-use exclusions, and freezes it
   into the private operator policy/release. No worker DNS, pin refresh,
   fallback address, retry, proxy, or alternate origin. DNS failure/no safe
   IPv4 means no case activation.
3. **Action and request ceiling.** One `GET /` public route, `inputIds: []`,
   two actions exactly (`browser_navigate`, then `browser_observe` with
   `screenshot:false`), one successful worker public GET total. No click, download,
   screenshot, CSV, upload, POST, redirect, subresource, or helper call. Root
   fixes the smallest viable response byte/time limits before release and
   refuses a site that needs a second request or larger budget.
4. **Release and process owner.** Fresh UUID case, nonce, expiry, fixed hashes
   and a one-shot root release. One fixed Rust test executable and its
   descendants run under a kill-on-close Windows job using M61's
   `process_guard.py` primitives. A timed-out or uncertain retirement fails
   without a replacement run.
5. **Evidence claim.** A passing result requires the public pin/hostname/TLS
   path, untruncated observed node oracle, exact counters, ordinary approvals,
   zero model calls, and joined process cleanup. No inference about provider
   quality, live accounts, general public-site availability, or autonomous
   browser choice. Root records source hashes and final status explicitly.

## Exact reusable interfaces

- `hosts/rust-daemon/src/tools/browser_tests.rs` contains ignored
  `m46_windows_real_browser_quote_oracle` at line 1440. It loads an explicit
  `ANIMA_BROWSER_ACCEPTANCE_CONFIG_FILE`, obtains `VerifiedBrowserSetup` via
  `browser::config::load_startup_setup`, creates `BrowserService::new_with_factory`,
  saves a run-owned scope, builds `gated_context(...).with_browser(...)
.with_action_journal(...)`, and invokes `real_allow` through the regular
  tool executor. `real_allow` waits for the ordinary approval and issues
  `AllowOnce`; it retains the spawned task through its bounded wait. M64 can
  reuse those helpers and service setup. It must not call M46's
  `ControlledFixture::start()` or `fixture_policies()`: the latter rewrites
  policy origin/routes to the loopback quote app.
- `hosts/rust-daemon/src/browser/config.rs::load_startup_setup` accepts an
  absolute, bounded schema-v1 JSON file with one verified installation and
  policy templates. Its installation records Node, worker and browser absolute
  executable/script paths plus SHA-256, and revalidates all three before use.
  The private M64 config has one `InstalledBrowserPolicy` with canonical
  `origin: "https://example.com"`, `publicIpv4Pin` set to the frozen address,
  one route `{id:"page",method:"GET",path:"/"}`, and empty inputs. No
  public pin enters a tool argument, model prompt, SDK, API or UI DTO.
- `BrowserObservation` in `packages/core-rust/crates/anima-core/src/browser.rs`
  has `document_generation`, `route_id`, `nodes: Vec<BrowserNode>` with only
  `node`, `role`, `label`, `shortened`, `omitted_nodes`, and `artifacts`. The
  worker's `snapshotEvidence` converts a maintained Playwright AI aria
  snapshot to those role/label pairs, and `boundObservation` can remove whole
  nodes under size caps. Assert literal node tuples, route `page`,
  `shortened == false`, `omitted_nodes == 0`, `artifacts.is_empty()` and a
  positive document generation; never derive `href` or HTML title from them.
- `packages/core-rust/browser-worker/worker.mjs` disables page JavaScript for
  `scope.publicIpv4Pin`, blocks service workers/downloads, retains certificate
  verification, and routes public GET through `createPinnedHttpsRequest` with
  `hostname=pin`, `servername=example.com`, `Host=example.com`, port 443,
  `rejectUnauthorized:true`, `agent:false`, identity encoding. The worker's
  route gate admits only configured main-frame navigation; hidden requests
  retire or raise counters. It returns a normal observation for Navigate and
  Observe. The source+installation hashes are the JavaScript-disabled-mode
  witness; the observation itself has no setting flag.
- `.superpowers/sdd/2026-10-16-companion-memory-http-m61/process_guard.py`
  is import-safe source to reuse in a new M64 private controller. `OwnedJob`
  launches suspended, assigns to its job before resume, retains process
  identities, captures descendants, closes the job, joins logs, and reports
  retired identities. Its bounded file hashing and listener helpers can be
  reused without invoking M61's supervisor, live case, or release.

## Smallest implementation and ownership

Public source addition: one new ignored Windows-only sibling test in
`hosts/rust-daemon/src/tools/browser_tests.rs`, named distinctly for M64.
Do not change M46's body or quote oracle. Keep any shared extraction minimal;
prefer directly using its existing `real_allow`, `evidence`, `process_witness`
and `MetadataFixture` helpers. New private M64 files live under a fresh
`.superpowers/sdd/2026-10-16-companion-public-https-m64/` directory:
controller, release contract, bounded evidence, and case-local results only.
No production runtime/SDK/web/config changes are expected.

The test loads only the M64 config named by the fixed environment variable.
It verifies the expected policy ID/origin/route/pin and declared low limits
before constructing the service. Create `ScriptedModel::new(vec![])` and one
`MetadataFixture::with_tools_model` containing only `browser_navigate` and
`browser_observe` grants. Do not attach files or start a listening fixture.
Save the public scope with `selected_input_ids: vec![]`, using its synthetic
agent/session/run and the installed operator policy. The agent's configured
browser tool grants and approval policy are ordinary M46 ones, including
per-action `AllowOnce`; no extra public grant or bypass. Capture the original
grant set and assert it is unchanged after Stop.

Call `browser_navigate` with only `{routeId:"page"}` and then
`browser_observe` with only `{screenshot:false}`. Require both normal tool
results. Assert both observations name route `page`; the final one matches
the predeclared role/label oracle with no truncation or artifacts. Obtain the
authoritative saved scope from `service.get`, assert `actions_used == 2`,
`requests_used == 1`, no selected inputs/output artifacts, and run action
receipts succeeded. Check the retained `model.requests()` after all test-owned
work joins; it remains empty. Tool arguments contain no origin, IP, path,
headers, URL, selector or provider choice.

Stop the scope with its current version and assert `Closed` and
`physical_lease_held == false`. Use M46's `catch_unwind` shape so
`service.shutdown().await` is attempted and awaited even when an assertion
panics. Preserve and rethrow the acceptance failure after cleanup. The M64
controller has a second OS job owner if a Rust future stalls or cleanup is
uncertain. Do not use wall-clock sleeps as proof of worker retirement.

## Private controller and release contract

Root creates a new UUID case directory and a one-shot release only after
M63 merge and the source/test/config are frozen. Suggested release fields are
version, milestone `M64`, UUID, 64-hex nonce, expiry, explicit authorized flag,
fixed host/origin/route/pin, exact role/label oracle, limits, and SHA-256
manifest. The manifest includes the test executable, private config, Node
22.12.0, `worker.mjs`, paired Chromium, controller and relevant Rust/worker
source. The controller accepts one exact release file, refuses changed or
unknown fields, verifies all files before child launch, and consumes the
release atomically so it cannot start a second case. No release is prepared
by this held plan.

Root reads the public page independently before the case, records the source
identity/time and literal minimal oracle, then performs the one DNS metadata
lookup and freezes exactly one safe IPv4. The controller only verifies these
release values; it never resolves DNS or discovers a replacement. It uses
an allowlisted child environment with no account/provider/vault variables,
no dotenv loading and only the M64 config path/case ID/non-secret nonce and
ordinary test runtime settings. Fixed argv targets only the M64 ignored test;
the fresh case directory owns logs/temp/profile. No external daemon, web
preview, fixture service or account is started.

Use a fixed two-to-three-minute outer deadline that reserves at least ten
seconds for job close and log joins, with shorter inner navigation/observation
deadlines. Bounded stdout/stderr and a small exclusive `case-result.json`
record the declared pin, observation tuples, counters, grant/approval/model
evidence, process identities and cleanup result. Logs may contain only
bounded test diagnostics; do not print page HTML, arbitrary URLs, environment,
secrets, command lines or personal browser profile paths. A protocol/TLS/
DNS/site/redirect/denied-subresource failure remains a failed result; no
retry, alternate pin, second launch or oracle relaxation.

M61 `OwnedJob.launch` assigns the child before resume. Retain the Rust child
and all observed Node/Chromium descendants, then close the job, await each
identity's retirement and join pipe threads. Record identity by PID, image
and creation time, not PID alone. Verify no case-local private browser root,
cookies/cache/downloads/artifacts survives after cleanup; preserve only
bounded evidence. A missing descendant sample is not proof of absence, so
the job's final member query and close result are authoritative. Check only
known case-specific listeners before and after; do not broadly kill unrelated
processes. Cleanup uncertainty fails acceptance.

## Tests and commands for a later bound milestone

- Before any live release, run the normal M64 source-only unit/focused tests
  for config, public scope, worker transport and role/label projection. Add
  only a narrow regression if this case exposes a real contract defect.
- Run `bun x nx run rust-daemon:test --skipNxCache` for the full host/core
  gate. Existing ignored M46/M47/M52 cases remain ignored. Run the worker's
  existing Nx test target resolved from the workspace, then relevant SDK/web
  gates only if those packages actually change. Root checks target names via
  `bun x nx show projects --json` before fixing invocation paths.
- After source freeze and a separate root release, the controller invokes the
  exact ignored-test shape used by M46:
  `bun --no-env-file x nx run rust-daemon:test --skipNxCache --excludeTaskDependencies "--args=--lib tools::browser_tests::m64_windows_public_example_dom_oracle -- --ignored --nocapture"`.
  This is one child in `OwnedJob`, with fixed `CI`, Cargo target and Nx
  settings; the controller, not an interactive shell, owns its deadline.
- Assert one M64 case result and OS retirement proof; never interpret a
  passing generic suite with the ignored case skipped as live acceptance.

## Top five risks to carry into binding

1. **Site drift:** even a static example page can change role/label text.
   Independent pre-release source and literal oracle are required; drift
   fails without a second read.
2. **Pin/TLS mismatch:** DNS metadata may rotate or resolve only disallowed
   addresses. Keep one reviewed pin, SNI/Host/certificate name `example.com`,
   normal trust, and refusal rather than DNS refresh or trust bypass.
3. **Hidden browser traffic:** favicon, redirects, subresources or a renderer
   side channel may add a request. The worker must deny it and the case must
   fail the one-request invariant; never widen the route list to accommodate.
4. **Observation mismatch:** aria snapshot nodes may omit title, link target,
   or large labels; `boundObservation` can truncate. Assert only declared
   role/label tuples and explicit no-truncation, never invented DOM APIs.
5. **Cleanup uncertainty:** stalled socket, Rust future or browser descendant
   can outlive a tool result. Await service shutdown and physical job/process
   retirement; if any lease, identity or log join is uncertain, retain a
   failed result and do not launch another case.

## Controller rulings on the risks (binding)

1. Ruling: one root-issued IPv4 DNS metadata operation for example.com and an independent public-source read precede activation; every returned IPv4 must pass the current M63 policy, then choose one deterministic address and freeze it — discovery belongs to the controller, never the worker — cost if wrong: no case release on missing/unsafe metadata, no fallback/refresh/retry, and DNS operation count is not a packet-count guarantee.
2. Ruling: exactly two ordinary owner-approved Navigate/Observe actions, one configured GET /, zero model calls, inputs/screenshots/downloads/cookies/accounts/helpers — this proves public transport plus JavaScript-disabled role/label observation through existing run/grant/approval paths — cost if wrong: the case fails; no provider-autonomy, whole-backend-UI or OS-egress-isolation claim. Use only a small independently declared current node oracle, never fields absent from BrowserObservation or post-case rebaselining.
3. Ruling: new UUID/nonce/expiry/source/program/config manifest, atomic controller consumption and an exclusive child-start marker, then execute the exact frozen prebuilt Rust test binary — normal build/test preparation stays on Nx, but compiling or resolving a binary during the live case would weaken the hash/deadline boundary — cost if wrong: refuse before launch; a failed actual case is immutable with no replacement run, rescue, relaxed oracle or changed pin under that release. Existing M46/M47/M52/M61 cases/releases are never invoked or changed.
4. Ruling: root owns a180s outer deadline reserving10s cleanup and a kill-on-close job for the fixed test process/descendants; native scope Stop/shutdown and post-join zero-model/grant checks remain mandatory — task result alone does not prove physical retirement — cost if wrong: fail and retain bounded evidence, no global/PID kill, ordinary-profile read or claimed cleanup success. Reuse M61 process-guard source only and freeze its hash, not its actual supervisor/release/case.
5. Ruling: source-only Sol driver implementation in one existing native test module, one bounded concurrency/lifecycle review/fix wave, then required native/worker and driver safety gates before the new actual release — reuse the accepted M46 owner/approval/installation seams without a new runtime abstraction — cost if wrong: no activation or acceptance claim. Existing isolated local-test and read-only research authority covers this finite headless browser GET; no actual daemon/web preview/model/account/device/DB server/container/deployment is released. No separate preflight audit or redundant SDK/web suite for unchanged inputs.

## Execution and resume

The source driver owns only hosts/rust-daemon/src/tools/browser_tests.rs, with
test-only helper extraction if strictly needed and every M46 assertion preserved.
The real M64 test stays ignored in ordinary gates. Add meaningful deterministic
release/config/oracle refusal checks before actual use; no accidental public
request from an ordinary test or missing release. The controller and evidence
stay in the fresh private M64 directory. No production fix without a demonstrated
contract failure and a distinct root binding.

Root alone runs commands/fmt/Git/processes. Cargo CARGO_INCREMENTAL=0/jobs1/threads2
serialized to actual exit, provider/service env scrubbed, approved native atomic
file permissions. Explicit paths only, never nx.json/anima.yaml/.superpowers.
Per-command unsigned commits and reviewed merge-and-continue remain authorized.
No stash/reset/restore/path checkout/worktree, ordinary credentials/profiles,
account writes, devices, DB servers/containers/Docker55432 or deployment. Preserve
M47/M52 and all four failed M61 cases; M52 PARTIAL. Update master/parity/harness/
handover/operator evidence, push/PR/attach/standing merge and exact tree/raw hashes.
Resume .superpowers/sdd/2026-10-16-companion-public-https-m64/progress.md.

## Recorded outcome

**Partial: real public browser acceptance was not run.** A single independent
bounded HTTPS source read returned200 and577bytes with normal certificate checks,
but did not establish the required heading oracle. No DNS pin lookup, startup
release, browser launch, replacement request or relaxed oracle followed.

The native test module now contains deterministic child-contract/config/oracle
admission checks and a separate ignored public-page driver. It validates fixed
case paths and hashes, refuses invalid or replayed admission, retains ordinary
owner grants and per-action approval, and keeps zero-model and shutdown assertions.
This prepares a possible future explicitly bound case; it proves no public DOM,
live TLS/browser result or autonomous task quality.

Fresh local gates: Rust/core **2,866/27 intentional ignores/45
suites**, native lint/build, **52 worker tests**, driver refusal checks and owned
docs/format/hash checks. All7 recorded worker-job identities retired.
Production, SDK, web, dependencies and standing grants are unchanged; their M63
evidence remains historical. M47/M52/four failed M61 cases stay immutable and M52
strict quality stays PARTIAL. See [harness evidence](../../companion/harness-acceptance.md).
