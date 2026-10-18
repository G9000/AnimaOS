# M46 isolated interactive browser: implementation plan

Status: scoped implementation and controlled Windows acceptance complete on g9000/companion-browser-m46, cut from verified M45/PR51 main802bb366539ee3533f4f5ec04b8fed38fd3e0497. One native batch, final SDK/web review and combined fix wave are complete; evidence-driven physical corrections followed. Current Rust/core2,661/11intentional ignores/40suites/test/lint/build, SDK148/3service-skips/type/build, web1,301/114files/type/build and42worker tests pass. The exact POST/CSV/JPEG task and four lifecycle/hostile cases pass with26 independently observed controlled processes, five kernel process/image witnesses, zero remaining processes and all private directories removed. Publication/verified merge is recorded in the milestone ledger.
M45 tested source7afc26961382cf91d381bdb40fb22e7379867727 and merged main have identical trees; current source/authority seams are refreshed before release.
The five Controller rulings below bind adapter, authority, lifetime, bounds and initial acceptance.
The original design proposals are retained below; binding refinements supersede them. No PAR-05/Hermes completion claim.

**Goal:** one ordinary agent can perform a bounded interactive browser task in an
isolated context, through existing owner grants, Exec approval and action receipts.
**Architecture:** prefer a narrow host-neutral BrowserPort over a maintained
Playwright engine; keep launch, ownership, policy and process retirement in the host.
Reuse existing MCP protocol/process machinery where it fits; create no new engine.
**Tech stack:** existing Rust runtime/approval/journal; candidate Playwright worker.
Use maintained Playwright-core1.59.1 already in bun.lock, Node>=18 (local22.12), Apache-2.0 and the existing Windows process-wrap10.0.1 ownership. A direct production dependency declaration and exact runtime layout remain root-owned preparation.

## Baseline evidence and limits (before M46 implementation)

- The PAR-05 roadmap locks interactive browser work after PAR-04.
- It requires navigation, accessibility/screenshots, click/type/scroll and selected
  upload/download, task isolation, guarded destinations, cancellation and an oracle.
- At the M45 baseline, `BrowserPort` occurred only as a proposed fallback seam.
  M46 now defines the reusable port and data helpers in `packages/core-rust/crates/anima-core/src/browser.rs`.
- The baseline `hosts/rust-daemon/src/tools.rs` registered `web_fetch`/`exa_search`; M46 adds four separately granted browser tools.
- `tools/web.rs` remains fetch/search; preserve it rather than call fetch interactive.
- `ToolExecutionContext::execute_tool` owns configuration checks, gates and journaling.
- `approvals/policy.rs` has static Read/Write/Exec/Network/Delegate classification.
- Dynamic MCP calls retain conservative Exec classification and exact dispatch stamps.
- `agent_runs/action_receipts.rs` saves native evidence and stops on evidence failure.
- M42–M45 already supply reviewed MCP metadata, stdio ownership and declarative bundles.
- M44's physical stdio guarantee is Windows-only; do not infer Unix descendant cleanup.
- M45 configuration/skill installation neither supplies a browser nor grants authority.
- Root identified public installed Hermes source at
  `C:/Users/leoca/AppData/Local/hermes/hermes-agent`, pinned
  `1fd18277267152893a8a24792b79213e7ca3f899`, and cached six tracked public blobs.
- This draft selectively inspected fewer than1000 relevant cached lines, including
  the MIT license; `public-upstream-source/source-proof.json` records root/path/pin.
- Source inspection proves code patterns only, not successful Hermes operation or parity.
- Personal Hermes/browser profiles, configuration, cookies and credentials were not read.

## Reuse decision for root

Compare only two practical choices; reject a bespoke browser/JSON-RPC implementation.

The pinned public Hermes comparison provides concrete reuse lessons:

| Public source anchor                              | Observed pattern                                                                             | Anima disposition                                                                                                           |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `tools/browser_tool_session.py:365–420`           | Task-keyed cached lookup, health/expiry checks and session replacement                       | Reuse task ownership intent; refuse implicit replacement after expiry/Stop/uncertainty                                      |
| `tools/browser_tool_snapshot.py:36–114`           | Bounded line truncation, force-redacted content-hashed spill and visible shortening          | Preserve bounds/shortening; use owned opaque artifacts rather than a profile-global cache/read_file expansion               |
| `tools/browser_tool_lifecycle.py:480–555`         | Whole-tree kill intent because descendants can keep capture pipes open; best-effort fallback | Keep the hazard/regression rationale, but require maintained Job Object/process ownership and joined uncertainty accounting |
| `tools/browser_tool_lifecycle.py:697–745`         | Stop CDP supervisor before backend close; exact session-key teardown                         | Join protocol/readers before releasing the owned task lease; no CDP attachment/profile adoption                             |
| `tests/tools/test_browser_process_tree.py:25–150` | Group leadership checks and a real shared-group child witness                                | Preserve concrete descendant/held-pipe tests; do not claim those upstream tests were run locally                            |
| `tools/browser_tool_origin.py:1–95`               | Lazy lookup of facade-owned module state                                                     | This is module provenance, not URL-origin enforcement; do not mistake its name for a network guard                          |

The cached `LICENSE` is MIT, copyright2025 Nous Research. Comparison currently copies no runtime code; any actual adapted source retains its notice and exact source pin.
The Python facade's profile globals, cloud/CDP backends, installers, sandbox bypass,
secret-home scope and auto-renewal are intentionally excluded. Prefer maintained
Playwright/browser/protocol components over porting that orchestration wholesale.

| Choice                                   | Reuse                                                                                     | Remaining work and decision                                                                                                          |
| ---------------------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Reviewed official Playwright MCP process | Existing M44 connection, schema review, grants, Exec, receipts and process owner          | Broad tool/profile behavior needs task isolation and independently enforced guards; generic MCP enable/connect alone is insufficient |
| Narrow BrowserPort over Playwright       | Maintained automation engine and context APIs; existing host execution/process boundaries | Small typed operation/policy adapter, scope admission and bounded observations; recommended if root accepts this necessary glue      |

The [official MCP README](https://github.com/microsoft/playwright-mcp) describes
isolated mode but defaults to persistent profiles; its origin controls explicitly
do not form a security boundary or cover redirects. Its idle behavior can relaunch.
Those defaults are unsuitable as proof of Anima task containment or quiet restore.
The recommendation is an inference from those documented gaps, not an upstream defect claim.

Playwright's [Browser API](https://playwright.dev/docs/api/class-browser#browser-new-context)
offers separate contexts and advises closing owned contexts before the browser.
The [context routing API](https://playwright.dev/docs/api/class-browsercontext#browser-context-route)
warns about service-worker bypass; it also documents early WebSocket routing.
Use maintained APIs, then test the actual guard/cleanup composition locally.
These APIs do not themselves establish OS sandboxing or whole-browser network isolation.

Before implementation root records the exact selected package/revision, license,
Node/browser requirements, executable layout and supported OS in the release plan.
Retain notices for copied code; comparison alone is not a source-code port.
If reviewed MCP can meet every bound rule at lower cost, root may choose it instead.
Reject any shortcut that replaces mandatory guards with prompt instructions or flags alone.

## Installed dependency boundary

- Operator preparation selects a preinstalled absolute worker runtime and browser binary.
- Record package/browser versions and hashes as available evidence, with integrity limits.
- Do not run `npx`, an installer, package manager or browser download from a tool/request.
- No PATH lookup, ambient user profile, owner shell string or executable supplied by a model.
- Reuse M44 direct launch, cleared environment, private cwd and Windows Job Object ownership.
- Fixed worker arguments/configuration come from the host's reviewed installation/scope.
- No extension connection, CDP attachment to an existing browser or persistent profile path.
- No storage-state/cookie import, password manager, account login or client certificates.
- No disabling browser sandbox/CSP/TLS verification to make the example pass.
- Missing installation/support returns a static unavailable status before a child starts.
- Browser descendants must remain in the recorded owned process tree; prove this explicitly.
- A selected runtime/library and external executable are separate dependencies and claims.

## Proposed reusable interface

Names below are candidates, not current APIs or implementation instructions.
The core port carries data/cancellation only; no HTTP/DB/Playwright/Node dependency.

```text
BrowserPort.open(validatedScope, control) -> OwnedBrowserSession
OwnedBrowserSession.navigate(checkedUrl, control) -> BrowserObservation
OwnedBrowserSession.observe(kind, control) -> BrowserObservation
OwnedBrowserSession.act(checkedAction, control) -> BrowserObservation
OwnedBrowserSession.transfer(checkedAssetRequest, control) -> BrowserArtifact
OwnedBrowserSession.close() -> BrowserPhysicalClose
```

`BrowserControl` reuses run cancellation/deadline semantics and shared byte counters.
`BrowserPhysicalClose` is produced only after actual context/protocol/process cleanup.
Close has an owned uncancelled completion path; caller timeout/drop is not a witness.

```text
ValidatedBrowserScope: host-selected ownerAgentId/runId, scopeId/revision,
  grantGeneration, exact origin, allowed request methods/routes, selected asset IDs,
  immutable installation identity, expiry, action/request/page/artifact quotas
BrowserAction: Click(nodeRef) | Type(nodeRef, literalText) | Scroll(boundedDelta)
BrowserObservation: scopeId, contextId, pageId, documentGeneration, actionId,
  currentUrl, observationDigest, bounded accessibility nodes, shortened,
  omittedNodeCount, optional owned screenshot artifact ID
BrowserArtifact: opaque ID, kind, byteLength, digest, bounded safe display name,
  declared source/destination provenance, availability and shortening/error state
```

No action accepts JavaScript, arbitrary Playwright code, selector programs, browser
flags, cookies, environment, filesystem roots, process commands or another context ID.
Node refs are issued by the current bounded observation, scoped to that document/run.
The adapter resolves them uniquely and rejects stale/foreign/multiple matches.
Typing preserves literal text; node/page content never supplies new effective grants.

## Host admission, tools and owner surface

- Candidate native tools: `browser_navigate`, `browser_observe`, `browser_act`,
  `browser_transfer`; exact names/DTOs require root freeze before SDK/web work.
- Use configured ordinary-agent tools plus a host-issued task scope; neither substitutes
  for the existing Exec gate. Initial browser operations stay conservatively Exec.
- Owner admission selects one exact run and destination; saving a scope starts nothing.
- First approved navigation may open its owned context; discovery/preview/list do not.
- Scope assignment/status/Stop UI belong to existing run details, not a second task engine.
- Candidate owner namespace: `/api/agents/{agentId}/runs/{runId}/browser-scope`.
- Freeze strict omitted/null/unknown rules and server-issued revision/CAS before coding.
- Run/owner/helper eligibility is server selected; no model can assign scope or installation.
- Capture the current grant/scope/document witnesses before asking; recheck after approval,
  receipt admission and immediately before dispatch. Revocation prevents stale execution.
- Scope expiry/Stop/replacement retires the context; no replacement during uncertain cleanup.
- Reuse existing approval policy, action journal, result pairing and receipt failure behavior.
- Owner Stop/cleanup must not wait for a new model call or another execution approval.
- UI reports absent, disabled, scoped, active, stopping or unavailable from actual state.
- Preserve Settings/Connectors shared permission uncertainty, owner generations and dirty edits.
- Installed bundle/configuration, successful launch and useful task completion remain distinct.

## Network and page guards

- First acceptance allows only one root-owned `http://127.0.0.1:<fixturePort>` origin.
- The numeric host and exact port are frozen per task; `localhost`/all-loopback is not a wildcard.
- No external origin, ordinary local service, personal account or real form submission.
- Validate navigation before dispatch and every redirect/subresource request before continuation.
- Cover popup/frame navigation, form submit, fetch/XHR, beacon and download destinations.
- Block service workers, WebSockets, WebRTC, extensions and browser permissions initially.
- Forbid file/data/javascript/custom protocols, URLs with credentials and origin confusion.
- No arbitrary proxy config, certificate exceptions, page-injected tools or browser devtools.
- Install policy hooks before creating pages; popup/new-page activity cannot race admission.
- One page initially; unexpected popup is refused/retired, not silently made authoritative.
- A denied secondary loopback sink must record zero received requests in regression evidence.
- Observe actual outgoing behavior; a navigation argument check alone is insufficient.
- Browser background traffic and interception coverage remain explicit risks to demonstrate.
- If those guarantees cannot be established, keep the capability unavailable or tighten scope.
- Arbitrary public browsing/effective egress isolation requires a later separately ruled case.
- Never label these application controls a general-purpose OS or executable sandbox.

## Proposed initial bounds

Root may tighten these once against actual current defaults and the chosen worker.

| Resource                        | Proposed limit                                                                                       |
| ------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Physically active browser tasks | One per host; one owned context per selected ordinary run                                            |
| Process allowance               | Reuse shared M44 four-child accounting where worker transport permits; no independent multiplied cap |
| Task lifetime                   | Two minutes absolute; 30 seconds idle; no auto restart/retry                                         |
| Action deadline/count           | Ten seconds per operation; at most 20 accepted actions                                               |
| Network requests                | At most 32; at most three redirects; strict task-origin/method/route policy                          |
| Pages                           | One; unexpected popups denied and cleaned                                                            |
| Tool arguments                  | Existing 16 KiB serialized ceiling; typed text at most 4 KiB                                         |
| Accessibility output            | At most 200 nodes/12 KiB content; 16 KiB complete model envelope                                     |
| Protocol stdout                 | Existing 64 KiB frame and 256 KiB operation/idle wire budgets                                        |
| Screenshot                      | Viewport only, at most 1280x720 and 1 MiB; at most two per task                                      |
| Transfers                       | One declared small upload and one download, each at most 1 MiB                                       |
| Total artifacts                 | Four MiB across the task; bounded owned temporary storage                                            |

No full-page unbounded capture, response-body dump, trace/HAR/video or console history.
Artifacts use opaque IDs and hashes; raw host paths are not model or log output.
Downloads get safe host-generated names, refuse traversal/symlinks/overwrite and remain
in task-owned storage until the declared oracle/retention action; never auto-execute/open.
Uploads resolve only explicitly selected task-owned bytes through existing path/asset checks.
Oversized content fails before unbounded accumulation; partial results retain honest limits.
Missing screenshot/vision support cannot be silently described as visual task acceptance.

## Lifetime, effects and quiet restore

- An owned task lease covers startup, live operations, retirement and quarantine.
- Stop, owner deletion, grant/scope revoke, deadline or output breach closes logical authority.
- Then join context, page listeners, protocol writer/reader work and browser descendants.
- A native leader exit is not a descendant exit; retain M44's whole-Job-Object witness.
- Kill/wait/context-close uncertainty retains the lease/permit and static unavailable state.
- Repeated close/Stop is idempotent; dropped initialization/call still has tracked cleanup.
- Do not release or transfer a slot simply because a row/run was deleted or a future timed out.
- Once dispatched, cancellation may leave form/network effects unknown; preserve that outcome.
- Store bounded action provenance in existing receipts; avoid a second durable browser ledger.
- Persist only reviewed installation/scope metadata if root demonstrates a need and migration.
- Restart marks old tasks closed/unavailable and starts no worker/context/page/network request.
- Never restore cookies, resume tabs, replay form submissions or automatically retry an action.

## Controlled useful task and independent oracle

One private loopback fixture provides a small interactive supplier quote page.
It uses DOM updates so plain fetch cannot complete the declared workflow.
The task uploads a tiny declared `request.csv`: quantity 3, maximum delivery 2 days.
The page exposes three offers: A at12.50/stock7/delivery2, B at11/stock20/delivery5,
and C at9.50/stock1/delivery1. Only A satisfies the quantity/delivery constraints.
Use bounded observations, fill the controls, filter, select A and create one local quote.
The controlled final click posts only to the declared fixture `/quote` route.
Download its CSV into task-owned output and take one bounded final observation.

Predeclared output is A, quantity3, unit12.50, total37.50, delivery2; fixture source
defines exact CSV bytes before execution. The oracle independently checks those bytes,
hash, page confirmation, exactly one quote POST and no forbidden-destination request.
Check actual source-linked observations and action receipts, not a fluent final sentence.
Adversarial variants attempt a hostile redirect, popup, bogus node, hidden instructions,
oversized output/download, pending approval revocation and a blocked operation plus Stop.
The page's instructions are untrusted data; synthetic calls are deterministically authored.
Initial acceptance uses zero model requests and no provider/account/ordinary profile.
Later root may predeclare one bounded provider-driven instance on the same fixture with
fresh context, exact action/request/token ceilings and the same independent oracle.
Do not run that later case automatically or infer task quality from deterministic dispatch.

## Candidate files and ordered release work

All paths are candidates until root accepts adapter/ownership; create no files now.

| Concern                   | Proposed source boundary                                                                 |
| ------------------------- | ---------------------------------------------------------------------------------------- |
| Port/data/validation      | `packages/core-rust/crates/anima-core/src/browser.rs` and module export only if needed   |
| Maintained-engine adapter | Narrow host worker under `hosts/rust-daemon/`; exact packaging entrypoint frozen by root |
| Runtime ownership/scope   | `hosts/rust-daemon/src/browser/{mod,service,tests}.rs`                                   |
| Tools/gate/journal seam   | `hosts/rust-daemon/src/tools/browser.rs`, existing tools/policy/coordinator seams        |
| Owner API                 | Existing run routes/contracts; one bounded task-scope/status seam                        |
| SDK/UI                    | Existing run client/details/approval surfaces after native wire freeze                   |
| Oracle                    | Private controlled-loopback browser fixture/test in existing daemon tests                |

1. Verify M45 merged exact main and record selected runtime/browser installation provenance.
2. Bind all five rulings; publish a short tracked plan only when root releases that work.
3. Add typed skeleton and a configured owner/tool/route RED that fails for missing behavior,
   not a missing import, compiler failure or a mock-only expectation.
4. Root observes actual RED, freezes interfaces, then releases bounded native/SDK lanes.
5. Implement one guarded port/context path using maintained code and existing authority.
6. Prove actual page effects, bounds, Stop, unknown outcome and physical cleanup in one batch.
7. Perform one native batch/final SDK-web review and one justified fix wave.
8. Run fresh relevant gates and the zero-model controlled task; label limits precisely.
9. Publish only accepted current-tree evidence; retain PAR-05 account/host/quality boundaries.

## Focused regression set and root-owned commands

- Missing/helper/foreign run, expired scope, absent grant, Exec Deny and Ask before dispatch.
- Grant/scope/DOM generation changes while pending approval; zero subsequent stale action.
- Exact-origin redirects/subresources/forms/popups/WebSockets/service-workers denied.
- No ambient profile/cookies/config; two fresh tasks share no browser storage/node refs.
- Literal typing, duplicate/stale node refs, malformed/large protocol/data/artifacts.
- Startup/call drop, deadline, leader exit/descendant-held pipe, repeated close and quarantine.
- Receipt-save failure stops later actions; dispatched effects retain unknown evidence.
- Restore/list/scope preview starts nothing; one actual useful fixture task/oracle passes.
- UI owner change/unmount/uncertainty retains permission fences and neighboring dirty edits.

Root confirms projects/flags via `bun x nx show projects --json` and target help first.
Required Rust gate: `bun x nx run rust-daemon:test --skipNxCache` in current tree.
Use the documented Windows validation target directory if a live executable locks it.
Root serializes Cargo with `CARGO_INCREMENTAL=0`; no worker runs nested compilers.
SDK gates use `ANIMA_SKIP_SERVICE_TESTS=1`; run relevant test/typecheck/build targets.
Run relevant web test/typecheck/build targets after native/SDK contract freeze.
Launcher changes additionally require the documented `workspace-dev` gate.
Browser preparation/test target names must be discovered, not invented in this draft.
No command above was executed during preparation.

## Five proposed root rulings and costs if wrong

| Risk requiring a ruling         | Proposed decision                                                                                               | Cost if wrong                                                                                       |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Adapter/dependency scope        | Narrow Playwright-backed port; reviewed MCP only if independently guarded; exact installed runtime/browser pins | Small adapter maintenance or root rejects scope rather than pretending generic MCP defaults suffice |
| Destination/action authority    | Exact loopback scope initially, typed actions, all relevant channels checked, existing Exec and exact grants    | Capability refuses tasks until guards are demonstrated; no external-send/profile activation         |
| Isolation/process lifetime      | Fresh task context, one browser task, shared permits, Windows whole-job cleanup and no resume                   | Quarantine can block another task; no unproved Unix/browser-tree claim                              |
| Output/artifact/resource bounds | Fixed action/time/wire/page/asset caps, selected owned files, bounded evidence/receipts                         | Useful large tasks are deferred; no truncation presented as complete evidence                       |
| Release/acceptance boundary     | M45 verified main, meaningful RED, frozen interfaces, one review/fix wave and zero-model oracle first           | Provider/account/host parity stays open; root can reject an overlarge first cut                     |

These historical proposals are superseded by the five binding controller rulings below. Root observes meaningful RED and freezes actual interfaces before production release.

## Controller rulings on the risks (binding)

1. **Adapter and dependency scope.** Ruling: Use a small host-neutral BrowserPort backed by a host-owned Playwright-core1.59.1 worker, with typed bounded domain IPC; reuse existing maintained Windows process ownership rather than invent browser automation or a custom MCP implementation — generic Playwright MCP defaults cannot establish the required task lifecycle/authority, while the engine is already locked and installed — cost if wrong: maintain one narrow adapter; refuse availability until actual runtime/browser compatibility passes. Declare the already-locked engine as a direct runtime dependency using Bun, with no unrelated upgrade. Fixed absolute Node/worker/browser paths are operator preparation; no request/model installer, PATH lookup, shell, CDP attachment, ambient profile/cookies or arbitrary flags. Require `chromiumSandbox:true`, ordinary TLS/CSP and fresh private storage. Local vendor Edge155.0.4283.45 is an available initial candidate, not an untested compatibility claim; record actual selected path/version/hash before launch. A maintained paired browser can be prepared separately inside the ignored validation directory if necessary; no ordinary cache/profile import.
2. **Destination and action authority.** Ruling: Initial release accepts only an exact live ordinary-run owner scope for a root-controlled numeric127.0.0.1 fixture origin and explicit methods/routes, separate existing tool grants and conservative Exec approval — page text, scope preview, package install and successful prior calls cannot supply authority — cost if wrong: requests remain unavailable/refused rather than broadening the task. Saving/listing scope starts nothing. Recheck grant, scope revision, cancellation and current document/node witnesses after approval and before dispatch/result admission. Reject every redirect initially, including same-origin redirects; an initial-route argument check or upstream origin flag is insufficient. Install context hooks before pages, block service workers/WebSockets/popups and forbid credentials/protocol escapes. Test a denied sink for zero requests and preserve any dispatched effects as unknown. These application controls are not OS egress isolation or arbitrary-public-page safety; no account, public browsing, external form/send or personal local-service destination in M46 acceptance.
3. **Isolation and physical lifetime.** Ruling: One global ordinary-run browser lease on Windows, fresh nonpersistent context and host-private cwd/temp, recorded process-wrap Job Object for worker/browser descendants, uncancelled joined close and quarantine on uncertainty — dropping a future/run row or killing only the leader cannot establish disposal — cost if wrong: the occupied/quarantined slot blocks a replacement until physical evidence. Scope expiry/revoke/owner Stop must retire without another model call/approval; no session renewal, reconnect, auto retry or restored worker/tab/action. Other hosts fail closed before spawn until separately proven. Receipt failure prevents later effects; prior network/form effects cannot be undone or inferred absent from timeout.
4. **Resource and evidence bounds.** Ruling: Freeze strict typed operations and limits before implementation: one page, at most40actions/80requests,120-second scope,64KiBperIPCframe/1MiBsessionwire, bounded nodes/text, screenshots and selected input/output assets; exact opaque artifact IDs stay in task-owned storage — unbounded HTML/base64/filesystem output or model-selected paths would widen authority and defeat cheap acceptance — cost if wrong: large tasks return bounded unavailable/shortened evidence with omitted counts, not fabricated completion. No JavaScript/evaluate/selector program accepted from an agent. Current uniquely resolved host-issued node refs bind run/document/observation generation; literal text remains literal. Existing durable action receipts carry bounded provenance; introduce no separate durable browser journal or persisted cookies/session state. Tighten proposed limits if actual implementation cannot enforce them.
5. **Release and acceptance.** Ruling: Proceed from verified M45 main through actual core/owner/executor RED, frozen native/worker wire, bounded independent implementers, one native batch/final SDK-web review/one fix wave, current gates and a zero-model useful task with an independent exact oracle — deterministic browser dispatch cannot establish provider planning quality or all Hermes browser behavior — cost if wrong: PAR-05 stays partial and no broader release claim. The fixture performs real DOM interaction and one declared local quote submission/download; expected supplierA/quantity3/unit12.50/total37.50/delivery2 and exact CSV bytes are predeclared. Include hostile redirects/popups/instructions, stale nodes/approvals, oversized output, Stop and descendant/pipe cleanup. Use pinned public Hermes task ownership/bounded observation/teardown patterns only; no private profiles or Python-runtime port. Preserve notices for any actual code adaptation and Anima product naming.

No worker runs Cargo/Nx/Git/fmt/models or starts services. Root owns commands and observes actual exits. No pre-flight audit or between-task check-in is added; uncertainties and implementation judgments are recorded as Ruling entries in the M46 ledger. Dependencies are permitted by the user's later instruction; ordinary services/accounts/DBservers/containers/Docker55432/public deployment remain excluded.

## Initial contract refinements (binding)

Core declarations and eight borrowed helper signatures are frozen with11passing focused cases after an actual missing-feature RED. The actual owner route RED compiled and failed404expected200 on a configured live ordinary run; Node's three parser/bootstrap cases failed explicit MissingFeature, not imports or a browser launch. Broader gates, actual engine compatibility and browser/task acceptance remain pending.

- Reusable Node adapter lives at `packages/core-rust/browser-worker/worker.mjs`, tested by the existing core project’s `browser-worker-test` target. Root linked exactly the already-locked Playwright-core1.59.1 runtime dependency; frozen install preserved the lock except that direct declaration. No browser download or model/runtime process started.
- Metadata selects a generic installed operator `policyId`, never a fixture-only product API or model URL/command. The policy table is immutable startup configuration, not a second registry/journal. Core/host authority and exact workspace/ordinary-run/live controls remain separate from digests.
- IPC is ABW1+u32BE, total header+payload64KiB and aggregate1MiB both directions, JSON depth8/nodes2048, safe positive IDs with separate incoming/outgoing order. Replies echo only the exact pending request. Bootstrap paths are host-private fixed admission data; finite agent operations accept no paths/selectors/code/options.
- Type is literal UTF-8 at most4,096bytes; Scroll is nonzero±2,048. Observation128nodes/8KiB role/label text drops whole oversized nodes with honest omitted counts. At most one CSV and one screenshot per observation/four opaque artifacts per session; input/CSV16KiB and screenshot256KiB. Existing attachment storage retains accepted output after context disposal; no new blob store.
- First useful criteria CSV is `quantity,max_delivery_days\n3,2\n`, with no supplier answer in the input. The actual fixture generates from received selection/quantity and the three-offer table; independently expected output is `supplier,quantity,unit_price,total,delivery_days\nsupplierA,3,12.50,37.50,2\n` and exactly one declared quote POST.
- Maintained public ARIA snapshot/ref APIs are confirmed in installed1.59.1. Interception uses bounded manual-redirect streaming before fulfillment; automatic route continuation or eagerly buffered fetch cannot establish redirect/body bounds. Application interception is not OS egress isolation. Native owned whole-job/IO proof, not an engineClosed acknowledgement, releases the physical slot.

Exact exits, file ownership and decisions are in `.superpowers/sdd/2026-10-14-companion-browser-m46/progress.md`. One combined review/fix wave resolved seven P2 findings, including three reproduced UI regressions, cumulative fetched-byte accounting, distinct-operation cancellation, grant ABA, output-publication ordering and failed-usage retention. Fresh full gates and controlled browser acceptance pass. Subsequent physical failures led to the binding corrections below. No full PAR-05/Hermes claim.

## Runtime and chat refinements (binding)

- Normal owner messages may explicitly select `browserSelection:{policyId,selectedInputIds}`. Only an immediately eligible new ordinary Chat/Web run receives it. Reserve the real existing run ticket under acceptance serialization and bind scope after acknowledged Running/live registration, before compaction or the first model/tool step. Omission preserves existing queue/steer/connector behavior. Matching message-key replay returns the original run without binding, renewal or restart restoration.
- Trusted installation is explicit startup-only `ANIMAOS_RS_BROWSER_CONFIG_FILE`, with strict version1 installation program/script paths and SHA witnesses plus immutable policies. Deterministic constructors do not load it. A per-task immutable factory receives scope, policy, installation, expiry, selected bytes and fixed output slots; a shared native runtime owns the physical lease. The same actor checks the explicitly approved document witness before IPC.
- The console separates browser tool grants in Settings from one-use chat policy/file selection. Selection is omitted from recovered-draft storage. Public output IDs are accepted owned attachment IDs; private worker slot tokens never become download authority. The [operator guide](../../companion/browser-work.md) documents setup and current limits.
- The real acceptance uses one embedded, joined numeric-loopback fixture and the exact independent31-byte criteria input/75-byte CSV output oracle. Its handler derives prices from actual posted choice/quantity and the offer table, with separate wrong-choice/quantity checks. There is no standalone Node fixture package or external account. Scripted adapter tests and zero-provider deterministic browser calls do not establish autonomous provider planning quality.

## Physical acceptance corrections (binding)

- Windows Node22.12 reports path device0 while the opened file handle reports its volume. Use exact BigInt identities and timestamps, regular/no-symlink/pre-post checks, containment, length and hash; compare every available nonzero device witness. Do not claim an absent path-volume witness was verified.
- Typed Download resolves the current pinned anchor and exact configured GET route, then uses the same guarded request/stream budget and exclusive output writer as other resources. It never clicks a browser download or accepts a model URL. Actual Chrome saved an uncounted native download outside context routing, so browser events cannot establish accepted artifact authority.
- Native downloads remain disabled and unexpected download events retire the task. This is insufficient to prevent initiating HTTP: a real page-initiated download reached the denied fixture sink. Release requires an owned deny-only loopback proxy for unrouted browser HTTP, with no forwarding/CONNECT/credentials/bypass, bounded connections/headers/time and joined listener/socket disposal. Maintained routing still fulfills allowed guarded responses; this is not an OS egress or arbitrary-browser safety claim.
- The exact useful oracle retains one actual POST map `{supplier:supplierA,quantity:3,maxDeliveryDays:2}`, independently recomputes the retained75-byte CSV hash after cleanup, and records actual counters/receipts/model calls. A hostile-download test must reject and automatically retire before cleanup; forced test Stop cannot satisfy its zero-sink assertion.
- Assertion failures await service retirement and fixture join before propagating panic. Physical witnesses use kernel process identity/image/creation times and actual ancestry. Manually removed diagnostic residue remains separately recorded and does not count as automatic cleanup.

Accepted installation: Node22.12.0, Playwright-core1.59.1 and its already installed paired headless shell revision1217 (package metadata147.0.7727.15), executable hash `f7c1ef91a3e287b64509a9733fc8fd43678cf4e2765d67f4d0eceb0e39ecd026`. Branded Edge/Chrome155 are not accepted by this record. Every deny-proxy attempt is refused and counted; ordinary denial before the first admitted action does not cancel a trusted blank startup, while limits/errors always retire and later refusals retain strict retirement. No hostname exception or forwarding is introduced.

Final real oracle: one exact supplierA/quantity3/maxDeliveryDays2 POST; retained75-byte CSV SHA-256 `6ae52000e58f4c8faa1a4e1daf9100186ef053795dd2fccd33be258f5b812b35`;12 actions/12 succeeded receipts, four guarded requests,36,480 wire bytes, zero model calls, one refused redirect and zero sink requests. Scope Stop, ordinary Run Stop, stale grants and snapshot restore preserve no-replay behavior; snapshot reconstruction is not a real daemon reboot claim. The final32.74-second case passed with automatic hostile retirement and all five private roots removed. A faster read-only Windows census corrected a missed short-lived redirect observation; it resolves the nvm alias to the exact same physical executable and records kernel creation times, not basename-only identity.
