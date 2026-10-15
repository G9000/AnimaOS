# M60 owner-visible memory index status

## Goal and existing evidence

Add an explicit Check memory search status control to the existing Memory page.
Reuse GET /api/memories/readiness and MemoryClient.readiness(), which already
exist. This closes one PAR-12 visibility gap, without a second dashboard or changes
to memory storage, recall, provider configuration, grants, SDK or daemon contracts.

M59/PR65 merged at1eabbd186c3e61b329dface7d1884daccacba614, source
3dbd4882670ed2c189ff8cbc970c1ec823e83166, identical tree
feb17df0628b093f73498632b34ef3a51ef0af71. Fresh Rust2856/26 intentional ignores/
45 suites/lint/build passed; all30 M58 source inputs remained exact. M58's unchanged
SDK183/three service skips and web1468/120files/type/build evidence stays recorded.
M52 strict quality remains PARTIAL; consumed actual cases are immutable.

The source-only planner found MemoryPage has no index status, although the endpoint
and typed SDK already exist. MCP/browser/STT/TTS have status in their own controls.
The controller verified routes/memories.rs:295-309: fixed baseline fixture checks
plus embeddings.status(), with no provider request or saved-memory mutation.
eval_harness.rs:170 constructs a fresh MemoryManager per fixture. The endpoint is
daemon-wide; memory_embeddings.rs:235 reports persisted from store.is_some().

## Reuse decision

Directly reuse the existing typed SDK method, centralized web daemon facade,
MemoryPage and current control styles. Existing native memory/index/eval behavior
is the required integration, so copying another memory engine adds no value here.
Keep pinned Hermes notices and Anima branding; no upstream code or personal
profiles/credentials imported in this milestone. No new dependency.

## Interfaces and owned paths

Only six source/test paths may change after the existing-page RED:

- apps/web/src/lib/daemon-api.ts: add memoryReadiness delegating to
  setupClient.memories.readiness(), with inferred existing SDK return type.
- apps/web/src/lib/daemon-api.test.ts: exact existing SDK GET forwarding case.
- apps/web/src/components/memory/MemorySearchStatus.tsx: small new status section.
- apps/web/src/components/memory/MemorySearchStatus.test.tsx: status/ownership cases.
- apps/web/src/pages/MemoryPage.tsx: mount section with agentId, online, epoch.
- apps/web/src/pages/MemoryPage.test.tsx: initial missing-action RED and integration.

No SDK/native/core/launcher/CSS/config/dependency changes. Root owns plans/docs,
fixtures, all commands/tests/format/Git/processes. Source-only Codex author and one
final web/SDK batch review, one fix wave, then local gates. No native review or
Cargo rerun for byte-identical native inputs; preserve M59's gate/hash evidence.

## Behavior and safe projection

- Show Check memory search status without requesting status on render, search,
  tab changes, content Refresh, agent switch, resync or StrictMode mounting.
- One explicit click makes one typed SDK GET, disabled while pending. No queue,
  mutation, rebuild, retention/consolidation, inference, grant or service start.
- Label this daemon-wide vector index, not a count of this companion's private facts.
  Show index enabled/disabled, configured provider/model, dimension/vector count,
  disk cache configured/not configured, and separate local fixture check counts.
- Off/empty vectors do not turn saved memory or lexical search off. Enabled does
  not establish configured-provider reachability or semantic recall/answer quality.
  Disk cache configured does not mean its latest write succeeded or canonical
  memory is durable. Never label the combined readiness.passed flag as overall ready.
- Validate only the projected fields: booleans; nonnegative safe integer counts;
  passedChecks <= totalChecks; bounded provider/model strings. Invalid/missing data
  yields generic unavailable, without displaying storageFile, raw response/error,
  evaluation failureMessages/cases/details, paths or unbounded diagnostic text.
- Treat zero total checks as no checks reported; do not label 0/0 as passed.
  Report failed local checks distinctly from vector off and connection failure.
- Keep the status ephemeral. Preserve memory list, query, filters, active tab and
  editing/deletion controls. No response/payload in storage, console or URLs.
- Fence status/error updates at committed agent/online/epoch/unmount boundaries.
  A response for the old scope cannot reappear after a switch away and back.
- Retain one module/page-wide pending read until its actual promise settles.
  Reuse the existing small retained-owner/subscription pattern where useful;
  mounted replacement panels must observe release. No timeout or unmount silently
  frees an unsettled read, no automatic retry, and no general new request harness.
  The current SDK has no abort option; generic busy/cleanup messaging may suggest
  reload if the request remains unsettled. Do not change the SDK to add abort here.

## Test-first sequence and local gates

1. Publish this bound plan/master/handover. Add only an existing MemoryPage test
   expecting the explicit status action, using existing render/mocks; do not import
   new production APIs before the RED. Root records genuine missing-control failure.
2. Release the six exact source/test paths. Focused tests cover no automatic read,
   one click/exact GET, double click/no queue, on/off/empty/disk config, separate
   fixture checks/fail/0-of-0, malformed data, generic error/retry and hidden paths.
3. Test committed agent/epoch/offline/unmount/StrictMode fences, ignored/deferred
   settlement and replacement-panel release, plus unchanged memory tab/query/list.
4. One final web/SDK source review, tests-first regressions for findings, one fix
   wave, focused GREEN, source freeze, required web test/typecheck/build and SDK
   tests/typecheck/build. Native gate evidence retained with exact source hashes.
5. Format touched TS/TSX/Markdown, docs-code scans, diff/hash preservation, explicit
   unsigned commit/push/PR/attach and standing authorized merge with exact-tree proof.

Known verified projects: @animaOS-SWARM/web, @animaOS-SWARM/sdk. Use bun --no-env-file
x nx run <project>:test --skipNxCache --args="run <existing targeted test paths>";
full suites --args=run; run-many -t typecheck,build -p the two projects --parallel=1
--skipNxCache. Clear provider/service env and keep NX_DAEMON=false,
NX_ISOLATE_PLUGINS=false, NX_NO_CLOUD=true. Root serializes heavy tasks. No real
provider/account/device/ordinary daemon/database server/container/deploy activation.

## Controller rulings on the risks (binding)

1. Ruling: explicit read only through the existing endpoint — source proves fixed
   local fixtures/status and no provider call — cost if wrong: source/focused review
   blocks release; no auto probe or permission expansion is authorized.
2. Ruling: separate global index configuration, cache configuration and local fixture
   checks — current fields do not prove live semantic quality, per-agent coverage
   or write durability — cost if wrong: misleading ready claims fail acceptance;
   M52/PAR-10/11 quality and actual-account limitations remain open.
3. Ruling: project and validate only safe bounded metadata — the diagnostic response
   includes an absolute cache path and extra details — cost if wrong: generic
   unavailable instead of exposing raw paths/errors or misleading counts.
4. Ruling: one retained pending read and commit-time generation fences, with release
   notification to replacement views — unmount/abort timing is not actual promise
   settlement — cost if wrong: Check stays busy until observed settlement/reload;
   do not admit repeated reads or revive a stale status.
5. Ruling: keep this feature inside MemoryPage/facade and preserve current memory
   workflows and native/SDK bytes — reuse is cheaper than another dashboard/runtime
   — cost if wrong: unchanged workflow/full web/type/build and hash gates block
   publication; no native/provider work is silently added.

## Resume and boundaries

Branch g9000/companion-memory-status-m60 cut from verified M59 main. Ledger:
.superpowers/sdd/2026-10-16-companion-memory-status-m60/progress.md. Explicit staging
only; never nx.json/anima.yaml/.superpowers. Per-command unsigned commits and
merge-and-continue are already authorized. No stash/reset/restore/path checkout/
worktree, ordinary profiles/credentials, actual models/accounts/devices/services,
database servers/containers/Docker55432/public deployment. Preserve unrelated nx.

## Gated outcome (2026-10-16)

The Memory page now has an explicit Check memory search status action. It reuses
the existing typed readiness SDK/API and presents a daemon-wide index snapshot:
enabled/disabled, bounded configured provider/model, dimension/vector count,
disk cache configured and separate local fixture check counts. Zero checks are
not called a pass. Saved memory and lexical search remain separate from vectors.

This is configuration/status, not a provider health probe, per-agent vector
coverage, semantic recall/answer-quality proof or evidence of a successful disk
write. The server's fixed checks use a fresh local fixture MemoryManager; they
do not evaluate or mutate the owner's saved memories. There is no automatic
request on render, search, tab/Refresh changes or resync, and no provider call,
memory mutation, index rebuild, grant change or service start.

One retained browser read spans unsettled promises and replacement panels.
Actual native FastEmbed namespace/model identifiers are accepted while absolute
paths, URLs and controls remain excluded; both compatibility regressions pass.
Committed agent/epoch/offline/unmount boundaries fence stale results; pending
requests do not queue or retry automatically. Safe metadata projection excludes
storageFile, raw diagnostics and errors; malformed data is generic unavailable.
Memory content, search, filters, tabs and editing controls remain independent.

The existing-page missing-action RED preceded production (48 existing tests
passed). One final web/SDK review and one bounded fix wave are settled. Fresh
local gates: **web 1,491 tests / 121 files**, **SDK 183 tests / 3
intentional service skips**, SDK/web typecheck/build, owned formatting/docs and
hash preservation. Native/SDK/config/dependency inputs are unchanged; current
M59 **Rust/core 2,856 / 26 intentional ignores / 45 suites / format / build**
evidence is retained for identical inputs without a redundant Cargo run.

Acceptance uses mocked metadata/deferred requests, not actual providers, model
accounts, semantic quality or a running normal profile. No real devices, ordinary
daemons, database servers, containers or deployment are activated. No dependency.
M52 remains PARTIAL; see [harness evidence](../../companion/harness-acceptance.md).
