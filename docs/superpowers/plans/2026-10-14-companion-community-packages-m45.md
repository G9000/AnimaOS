# M45 reviewed community MCP and skill bundles: implementation plan

Status: M45 scoped implementation and controlled acceptance complete on g9000/companion-community-packages-m45, cut from verified M44/PR50 main15e40c397a6ef961639e9375fe50ee513d09c9bf. Current relevant gates passed; exact publication/PR/verified merge is recorded in the M45 ledger.
The starting plan and risk proposals below are retained as history; controller rulings and the final evidence section bind current behavior. Complete PAR-04/Hermes parity is not claimed.

**Goal:** Install, inspect, update, disable and remove one reviewed declarative bundle containing MCP configuration proposals and standard SKILL.md content.

**Architecture:** Reuse M42–M44 MCP records, lifecycle, grants and dispatcher; reuse the current skill import/draft/approval path.
A small host-neutral manifest validator and digest helper belongs under `packages/*`; owner orchestration, storage and UI remain in their current host/app boundaries.
An installed bundle records exact ownership and review evidence, without installing executable dependencies or creating new execution authority.

**Tech stack:** Existing Rust serde/serde_json, anima-core serde_jcs/SHA-256, daemon control-plane transactions, skill services, rmcp-backed MCP manager, SDK and web controls.

Execution followed five bound controller risks, meaningful core/owner/UI RED, frozen native interfaces and bounded disjoint implementation lanes.
One native batch and final SDK/web review fed one combined fix wave. A later actual idle-scan acceptance failure justified one bounded correction and fresh native gates.

## Source-backed starting point

- `AGENTS.md`: reusable runtime code stays under `packages/*`; runnable host code stays under `hosts/*`; root owns current required Nx gates.
- `docs/companion/parity-roadmap.md`, PAR-04: manifest, compatible runtime, permissions, digest, attribution and a reproducible lifecycle example remain queued.
- The roadmap separates implementation, local acceptance, account acceptance and host acceptance; one fixture cannot establish general package safety or parity.
- `docs/superpowers/plans/2026-10-14-companion-mcp-stdio-m44.md`: immutable absolute stdio launch, disabled saves, separate connect, exact reviewed descriptors, four global connections and shared call policy.
- Its current refinement makes stdio execution Windows-only; other hosts fail before lookup/reservation/spawn. HTTP remains portable.
- `hosts/rust-daemon/src/mcp/{records,service,transport,stdio}.rs`: bundle integration must call existing validated services rather than introduce a dispatcher.
- `packages/sdk/src/mcp.ts`: strict HTTP/stdio create union, version CAS, separate review/grant actions and encoded owner paths already exist.
- `hosts/rust-daemon/src/skills/drafts.rs`: `SkillService::import(bytes, slug)` stores a capped pending import draft; it does not write or approve a skill.
- `skills/file.rs`: existing parse/compose/hash helpers define standard SKILL.md and the bytes pinned by approval.
- `skills/service.rs`: direct `save` approves content; `set_enabled` preserves its approved hash; `delete` uses workspace trash with failure recovery.
- `skills/registry.rs`: generations fence stale scans; only exact active approved bytes load.
- `packages/sdk/src/skills.ts`: import, draft review, approval, enable and removal already have owner-facing contracts.
- Imported drafts expose `baseHash/currentHash/stale`; M45 must explicitly fence its bundle revision and expected skill base instead of assuming every import already has M39's learning-source CAS.
- `anima-core/Cargo.toml` already supplies serde_jcs, SHA-256 and JSON schema validation; no new dependency is justified by this draft.

## Reuse decision and alternatives

1. **Recommended: declarative JSON plus inline SKILL.md.** Reuse existing component services and content formats; add only bundle identity, validation, ownership and coordinated settlement.
2. **Directory/archive import.** Could carry binaries/references, but adds traversal, symlink, decompression, permissions and extraction transactions; defer it.
3. **Registry/package-manager installer.** Could fetch dependencies, but introduces install scripts, mutable resolver state and executable authority; outside this milestone.

This is a configuration/content bundle installer, not a server-program installer.
The owner binds a declared local program reference to a preinstalled absolute executable through the existing trusted-local-code review.
The manifest digest covers bundled content/configuration, not the bytes or safety of that external installed program; show this boundary explicitly.
No upstream installer is copied. Existing rmcp/process ownership and standard SKILL.md remain the reused interfaces.
For a later external example or substantive copy, record pinned source, exact paths, license and retained notices before publication; current historical Hermes comparison is not a package-format compatibility claim.

## Scope

- One selected ordinary agent owns each bundle's MCP connections; skill content continues to use the existing workspace-global skill registry.
- Bundle install saves disabled MCP records and pending skill drafts only.
- Existing explicit skill approval may activate a new skill; retain that behavior and its precise content review rather than silently calling `save` during install.
- Separate MCP enable/connect/discover/review/grant and normal Exec approval remain required.
- Update, disable and remove operate only on exact recorded bundle-owned components and show their affected scope.
- No OAuth, credentials, secret references, native/in-process plugins, connector contribution loading or dynamic library loading.
- No downloads, registries, archives, scripts, extra reference files, dependency solver, package manager, shell, environment/cwd override, auto-start or automatic retry.
- No model tool can install/configure bundles, supply owner bindings or authorize permissions.
- PAR-04 remains partial; compiled connector contributions and executable artifact installation are distinct future work.

## Proposed v1 manifest and validation

The following names are draft contracts to bind, not existing API fields.
Use strict typed variants with `deny_unknown_fields`; reject duplicate JSON keys before canonicalization, unknown schema versions, explicit nulls and mixed variants.
Proposed caps: manifest 128 KiB UTF-8, four components total, at most two MCP and two skill components, 16 installed bundles per workspace.
Existing four-connection, ten-pending-import and skill content limits still apply jointly; a bundle does not add quota.
IDs/component keys are bounded printable identifiers, never filesystem paths; reject duplicates, reserved names, NUL/control/hidden text and ambiguous Unicode identifiers.
Version is a strict three-integer release identifier; runtime compatibility uses inclusive minimum/exclusive maximum plus required capability names, avoiding permissive range expressions.

```json
{
  "schemaVersion": 1,
  "id": "example.synthetic-violet",
  "version": "1.0.0",
  "name": "Synthetic violet procedure",
  "description": "A reproducible local MCP and skill example",
  "compatibility": {
    "host": "rust-daemon",
    "minVersion": "0.1.0",
    "maxVersionExclusive": "0.2.0",
    "requiredCapabilities": [
      "mcp.owner.v1",
      "mcp.stdio.windows.v1",
      "skills.import-draft.v1"
    ]
  },
  "permissions": {
    "nativeToolNames": [],
    "externalTools": [
      { "component": "violet-server", "remoteName": "synthetic_stdio_record" }
    ]
  },
  "attribution": [
    {
      "component": "example",
      "source": "repository-local example",
      "revision": "release-recorded",
      "license": "repository-license",
      "notice": "Retain the shipped example notice"
    }
  ],
  "components": [
    {
      "key": "violet-server",
      "kind": "mcpStdio",
      "label": "Synthetic violet",
      "programRef": "synthetic-peer",
      "args": ["--mode", "normal"]
    },
    {
      "key": "violet-procedure",
      "kind": "skill",
      "skillFile": "---\nname: Violet procedure\ndescription: Check an approved synthetic MCP result\n---\n\nUse only tools already granted. Request the existing Exec approval, then quote the returned connection and descriptor provenance.\n"
    }
  ]
}
```

An HTTP variant supplies only the existing anonymous endpoint; a stdio variant supplies a bounded logical programRef and literal ordered args.
Owner review supplies `{programRef:absoluteExecutablePath}` independently; never resolve PATH, inspect ordinary profile locations or fetch a declared source URL.
The resulting launch must pass all M44 validation, including empty argv versus singleton-empty argv, Windows .exe and forbidden launchers.
Skill slug is generated from the internal bundle UUID and component key, using existing slug validation; publishers cannot overwrite unrelated skills by naming them.
Accept inline SKILL.md only; parsing/canonical composition uses the existing helpers and limits.
The manifest can request descriptive capability names, but cannot specify effective grants, approval classifications, helpers, enable flags or connect actions.
Attribution values are displayed as untrusted plain text; source/license strings are claims until controller provenance checks record evidence.

## Digest and review identity

- Compute `manifestDigest = sha256(JCS(strictly parsed v1 manifest))`; formatting alone does not change identity.
- Compute source skill-byte digests and canonical installed-skill-byte digests separately; show any normalization before approval.
- Compute an owner review digest over manifestDigest, resolved validated launches, canonical skill bytes, target agent/workspace identity, expected component versions/hashes and capability report.
- Digests are host-computed evidence of exact content, not publisher authentication or permission.
- Bind the review to a bounded server-side receipt and expiry; proposed ten pending receipts/10-minute lifetime, with static errors and no raw argument logging.
- Install/update must recheck receipt, owner/workspace generation, bundle version, skill base hashes and component versions under the existing transaction boundary.
- Changed bytes, owner binding, runtime capability or authority version require a fresh review; no inference from a label or semantic similarity.

## Runtime compatibility and permissions

- Report actual daemon version and explicit supported bundle capabilities; package version alone cannot prove that a 0.1.0 daemon contains M44.
- Reject unsupported schema/host/capability/platform before staging any component.
- Windows stdio support is inherited from M44; preserve fail-closed behavior on other platforms until separately justified.
- MCP aliases remain derived from connection UUID and full discovered descriptor digest, not manifest remoteName.
- Manifest permissions are a review checklist; install/update creates zero new effective tool grants.
- Review/discovery still makes no `tools/call`; remote hints cannot reduce Exec classification.
- Workspace-global skill visibility and disable effects must be clear in the owner UI; bundle targeting does not create per-agent skill privacy.
- Helpers and foreign/ineligible agents cannot obtain bundle MCP authority through a manifest.

## Install/update/disable/remove settlement

1. **Review:** validate bounded uploaded manifest, normalize skill bytes, validate owner bindings and present proposed changes; zero child/network starts, tools calls or active skill changes.
2. **Install:** commit bundle metadata, disabled MCP candidates and pending import drafts through one coordinated durable candidate; only then publish acknowledged component identities.
3. **Skill adoption:** link to the existing owner draft review/approval. Show the exact written hash; loadability remains governed by the skill registry and current tool grants.
4. **MCP use:** link to existing separate enable/start/review/grant controls; package UI must not create a second launch button or bypass that flow.
5. **Update:** unchanged validated MCP config keeps its connection ID, reviews and grants; unchanged skills keep their approved hash and enabled state.
6. Changed MCP launch/transport receives a fresh disabled connection ID; old exact reviews/grants stay dormant on the disabled predecessor until explicit removal.
7. Changed skill content becomes a new pending draft with expected bundle/skill base hashes; old approved bytes remain effective until existing explicit approval succeeds.
8. Insufficient connection/draft capacity refuses the update without eviction; the owner may explicitly remove a displayed predecessor and review again.
9. **Disable:** retire only bundle-owned MCP transports through joined cleanup and disable only its owned skills; retain hashes, reviews, grants and provenance. Cancel/reject owned pending adoption so it cannot reactivate a disabled bundle later.
10. **Re-enable:** only exact still-approved skill bytes and unchanged saved MCP metadata can be enabled; no process starts and no tool grant is added.
11. **Remove:** confirm exact owned IDs, versions and hashes; use existing MCP removal and skill trash semantics, removing only owned aliases/resources.
12. Manual skill/content/ownership changes conflict or detach only through an explicit reviewed ownership decision; never delete a neighbor, overwrite an owner edit or claim rollback after uncertainty.

Do not compose public POST/PUT calls and describe their partial success as atomic.
Expose a narrow internal candidate seam reusing existing validators/registry logic and the ordered control-plane transaction/persist path; avoid recursive service-lock acquisition.
Persist operation identity/settlement sufficient for interrupted install/update/disable/remove to remain fail-closed after restart, without replaying launches or approvals.
On precommit failure preserve the old published state; on durability uncertainty retain observed candidate identities, retire affected execution and report unavailable for explicit reconciliation.
Restart validates bundle ownership and component hashes before publication, restores MCP disconnected, never approves drafts, and never replays install steps or calls.
Propose the next control-plane version with an independent `.pre-community-bundles.bak`; bind its exact number after verified M44 main, preserving CP18 HTTP/stdio/skill rows and earlier backups.

## Proposed owner API and UI

- Namespace: `/api/agents/{agentId}/community-bundles`; same local-owner authorization and no-store policy as MCP.
- `POST /reviews`: strict `{manifestText,programBindings,expectedBundleVersion?}`; return exact review receipt/digests, compatibility and component/permission preview.
- `POST` install: `{reviewId,reviewDigest}`; return acknowledged bundle plus disabled connection IDs and pending draft IDs.
- `POST /{bundleId}/update`: `{expectedVersion,reviewId,reviewDigest}`; strict CAS and one host settlement; no mutation retry.
- `PATCH /{bundleId}`: `{expectedVersion,enabled}`; permission-neutral disable/re-enable only, with explicit global-skill effects.
- `DELETE /{bundleId}`: `{expectedVersion,reviewedComponentStateDigest}`; remove only confirmed owned resources, with preserved failure/uncertainty evidence.
- `GET` collection/detail: current component status plus effective authority, never infer running/approved from installed metadata.
- SDK uses the shared DaemonClient, encoded paths and exact envelopes; read-only refresh must reconcile Settings/MCP grant uncertainty without overwriting unrelated settings.
- Web uses plain-text manifest/skill/argv/provenance review, explicit local-code trust, pending approval links and lifecycle actions; no model-driven authority or optimistic success.

## File ownership proposal

- `packages/core-rust/crates/anima-core/src/community_bundles.rs`: pure strict manifest types, bounded validation, canonical digests and compatibility comparison; no filesystem/HTTP/DB/process dependency.
- Its `lib.rs` export and focused unit tests reuse existing serde_jcs/SHA-256; a new crate is unnecessary for this slice.
- `hosts/rust-daemon/src/community_bundles/{mod,records,service,tests}.rs`: owner records, review receipts, component ownership and coordinated settlement.
- Existing `mcp/service.rs`, `skills/drafts.rs`, `skills/registry.rs` and `state.rs`: only narrow candidate/CAS/approval ownership seams demonstrated necessary; retain the existing engines.
- `control_plane_store.rs` and restore tests: new bundle ledger/version/independent exact backup, with no rewrite of existing component formats.
- `routes/community_bundles.rs`, `routes/contracts/community_bundles.rs`, route tests/schema registration: strict owner contracts.
- `packages/sdk/src/community-bundles.ts`/spec and exports: typed owner client; `apps/web/src/components/community-bundles/` and existing Connectors surface: reviewed UI.
- `examples/community-bundles/synthetic-violet/`: manifest, canonical SKILL.md source and attribution/contribution instructions; reuse the controlled native peer, without shipping a new installer.
- Root owns branch, interfaces, migration number, dependencies if needed, example build, all commands/gates, documentation publication and aggregate integration.

## Bounded implementation sequence after release

### Task 1: strict manifest RED and frozen contracts

- [ ] Test unknown/mixed/null/duplicate fields, version/capability mismatch, canonical digests, literal argv and skill normalization; observe actual meaningful RED.
- [ ] Implement the pure reusable validator; freeze caps, DTOs, identity/ownership and all five controller rulings before parallel source work.

### Task 2: native review/install and candidate settlement

- [ ] Test review/save/list/restore with zero starts, calls, grants and approved skill writes; inject precommit and uncertain-save failures.
- [ ] Implement one candidate seam using existing registry/import logic; test stale workspace/skill/component versions and capacity races.

### Task 3: update/disable/remove and recovery

- [ ] Test unchanged-ID grant preservation, changed-launch fresh disabled IDs, pending skill adoption, active-call revocation and owner-edited neighbors.
- [ ] Implement the narrow ledger/recovery transitions; test interrupted operations, exact backups, quarantined cleanup and no replay on restore.

### Task 4: SDK/web and example acceptance

- [ ] After native wire freeze, add exact SDK serialization/no-retry tests and owner UI RED; retain M43/M44 Settings locks, dirty edits and generation fences.
- [ ] Add the single reproducible example and run the declared local lifecycle through the actual package API, then existing approval/dispatch paths.
- [ ] Run one native batch review, one SDK/web review, one combined justified fix wave, then fresh relevant gates; no additional broad audit loop.

## One reproducible local example

Use the existing `mcp_stdio_peer` controlled executable on Windows, built once by root through its Nx fixture target; no nested compiler or package-manager execution from install.
Owner binds `synthetic-peer` to that exact prebuilt absolute path in an isolated temporary workspace and reviews the displayed argv/content.
The example manifest contains one MCP component and one short procedural skill; source and canonical skill hashes are independently checked.
Install proves disabled/no-child/no-grant/pending-draft state; explicit existing skill approval proves exact hash pin and actual skill load.
Separate existing MCP enable/start/discovery/review/grant precedes one Exec Ask/AllowOnce call through the actual executor, using fixed synthetic arguments and durable receipt/provenance assertions.
Update changes the skill text and one launch argument; old permissions do not transfer, the new draft/connection stay unapproved/disabled, and unchanged neighbors retain exact IDs/config/grants.
Disable during a blocked controlled call verifies joined retirement and unknown effects; remove verifies owned trash/alias cleanup, exact neighbor preservation and quiet restore.
Zero providers/models/accounts/profile reads; selected local package evidence does not establish general community compatibility or semantic task quality.

## Top five risks for controller binding

| Risk                                                            | Proposed ruling                                                                                                                                                | Cost if wrong / required evidence                                                                                               |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Review digest misses normalized bytes, bindings or TOCTOU       | Strict v1 + duplicate-key rejection; host JCS/content/review digests include exact canonical skills and owner bindings; fence current versions/hashes          | Re-review on any mismatch; mutation/normalization/expiry/race tests must write nothing                                          |
| Component services publish partial authority or deadlock        | One coordinated durable candidate and narrow internal reuse seams; existing ordered transactions, uncertainty and restart rules                                | More settlement glue; injected failure/drop/restart tests must never approve, grant, launch or falsely roll back                |
| Global skills/colliding package names delete owner work         | Generated owned slugs/internal UUIDs; exact ownership/hash checks; explicit workspace-global effects and conflicts for manual edits                            | Some operations require reconciliation; neighbor/manual-edit/delete tests and trash proof                                       |
| Updates inherit old execution authority or bypass caps          | Preserve unchanged IDs only; changed launch gets fresh disabled ID and separate exact review/grant; old grants dormant, no eviction                            | A full four-connection workspace may refuse an update; test capacity, stale approvals, active revoke and no permission transfer |
| Publisher claims or installed program drift become trusted code | Declarative-only install; runtime capability checks; existing owner trust for preinstalled programs, with binary-integrity boundary explicit; preserve notices | Digest is integrity, not authentication/sandboxing; executable installation/pinning/signatures need a separate ruled slice      |

These are proposals. Root must bind schema/caps, coordinated approval/disable fencing, ownership/removal, predecessor retention/capacity policy and the external-program integrity boundary before implementation.

## Planned verification and honest status

- Root resolves current Nx project/target names, then runs fresh Rust host/core tests, lint/build and applicable SDK/web test/typecheck/build targets under repository constraints.
- If reusable core code changes, include its meaningful manifest/schema/digest tests through the existing relevant workspace target.
- Acceptance must name the exact revision, controlled package bytes/digests, effects/receipts, restart preservation and cleanup limitations.
- No M45 command, source change, dependency change, fixture run, provider request, public documentation update or Git action has occurred for this draft.
- Next step: controller risk rulings and refresh against verified merged M44 main; only then cut and release the implementation milestone.

## Controller rulings on the risks (binding)

Ruling: Use strict typed declarative v1 JSON with recursive duplicate/unknown/null rejection,128KiB input,four components(max two MCP/two skills),16 installed bundles and existing joint quotas; canonical JCS/SHA256 identity must bind original/canonical skill bytes, literal args, independently supplied executable bindings, actual host capability set, target owner and component base versions/hashes — normalization or an unbound launch can change what was reviewed — cost if wrong: fresh review on any changed inputs, finite pending receipts and exact mismatch/expiry failure without writes. Identifiers are bounded ASCII data, never paths; versions use strict three integers and explicit min/inclusive,max/exclusive comparisons. No external fetch or permissive range parser. Reuse anima-core dependencies and skill parser, no new crate or dependency.

Ruling: Install/update metadata as one detached save-before-publication candidate using the existing shared control-plane transaction and narrow MCP/skill registry seams; do not chain public mutations or recursively acquire service locks — cancellation and partial component publication can create unreviewed authority — cost if wrong: some coordinator glue and a CP19 independent .pre-community-bundles.bak, rather than silent partial success. Precommit failure preserves prior publication; uncertain durability retains observed identities fail-closed and retires affected execution. Persistence/cleanup intent must make interrupted disable/remove explicitly unavailable until exact reconciliation; restore never launches, approves or replays an install. No filesystem I/O while holding state/control-plane guards.

Ruling: Generate bundle-owned skill slugs from internal bundle UUID and component key, retain exact ownership/content hashes and make workspace-global effects visible; manual edits conflict on update/disable/remove and no neighboring resource is overwritten — existing skills are workspace-global while MCP connections belong to one ordinary agent — cost if wrong: owner must reconcile an edited component, rather than deleting personal work. Pending bundle draft approval must check the current enabled bundle/revision and exact base; disable/remove invalidates those drafts so later approval cannot reactivate the bundle. Adopted files use existing hash-pinned approval/trash semantics and explicit affected-scope review.

Ruling: Preserve component identity only when validated configuration/content is unchanged; changed MCP launch creates a fresh disabled ID with no review/grant transfer, changed skill becomes a pending hash-fenced update while current approved bytes remain until explicit adoption — bundles cannot expand tool authority or evade immutable launches/capacity — cost if wrong: a full four-connection or pending-draft quota refuses update without eviction. Disable revokes availability and joins transport retirement; re-enable saves metadata/exact still-approved skills only, never starts a process or grants. Keep old reviews/grants dormant on explicitly retained disabled predecessors until removal, and fence stale pending approvals/calls.

Ruling: This milestone installs configuration and reviewed content, not executable artifacts; use only owner-bound preinstalled absolute programs via M44 or existing anonymous HTTP, and display publisher attribution/capabilities/permissions as plain untrusted claims — manifest digests do not authenticate publishers, pin executable bytes or provide a sandbox — cost if wrong: owner must separately acquire/trust a server and real third-party compatibility remains open. No archives/reference files/install scripts/downloads/registries/package managers/OAuth/secrets/connector or native plugin loading, helper grants or model configuration. Keep notices and Anima branding; use one repository-local synthetic example with zero models/accounts.

## Frozen initial release

- Reusable strict manifest validation/digests under anima-core; daemon bundle registry/review candidate and owner route seam; SDK/web wire follows native freeze. No core HTTP/filesystem/process dependency.
- Owner namespace /api/agents/{agentId}/community-bundles, same local-owner/no-store authority as MCP. Review binds raw manifestText and programBindings plus optional current bundle version; install/update pins host-created review ID/digest. Version CAS and component-state digest fence disable/remove. Receipt cap10/lifetime10minutes; no mutation retry. Fields remain strict and explicitly validated before public schema registration.
- Generated component identities, exact version/hash witnesses and ownership ledger persist in CP19. Existing MCP/skills services retain authority; bundle orchestration prepares candidates instead of granting, approving or running tools. Required native/UI meaningful RED precedes production; root freezes detailed DTOs before SDK/web release.
- Three bounded implementation lanes maximum; root owns commands/Cargo/Nx/fmt/Git/services/model budget, native batch/final SDK-web review/one justified fix wave and fresh gates. Isolated controlled lifecycle/native executor acceptance only; no provider task, ordinary credentials/services or whole-Hermes claim.

Ruling: Review requests for update pin an explicit bundleId/expectedBundleVersion pair, both absent for install; require manifest identity match and exact ordinary owner — an external manifest name or version alone cannot select installed authority — cost if wrong: a small strict DTO refinement and renewed review, rather than cross-bundle target confusion. Initial pure-core interfaces are parse_manifest(&[u8])->Result<CommunityManifest,ManifestError>, validate_manifest(&CommunityManifest,&RuntimeCompatibility)->Result<ValidatedManifest,ManifestError>, and host-computed canonical manifest digest; no server-issued receipt in core. Native worker freezes DTO/envelopes/candidate IDs after the actual owner-route RED before SDK/web production.

## Binding seam refinements after actual RED

All three initial behavior REDs are observed: actual configured owner route404expected200, actual Connectors region missing, and valid-manifest parser MissingFeature; no fixture/import/type failure. Detailed native146-line/core/web notes are frozen in the M45 ledger and accepted with these refinements:

- Keep the existing ApiError JSON `{error:string}` and owner status/no-store rules, using fixed bounded messages; no new global error envelope.
- Managed bundle draft approval must match its declared canonical bytes. A changed body requires fresh bundle review/update or an independent skill; ordinary unrelated draft editing remains unchanged.
- The existing transaction may span ordered CP durability save. New bundle workspace reads/writes/trash happen outside NEW CP/state guards; existing separate SkillService approval keeps its inherited behavior and rechecks enabled/current bundle lineage within its authority transaction. No global skill-I/O rewrite, external-editor atomic CAS or undo-effects claim.
- Mutation detail returns current canonical owner `toolNames`; successful removal returns `{bundleId,removed:true,trashPaths,toolNames}` for functional permission reconciliation. SDK/web uses existing shared owner mutation/uncertainty fences and makes no grant/approval or optimistic-success assumption.

Ruling: Updates that remove, rename or change kind of existing skill component keys return409; same-key content changes and bounded additions remain supported — metadata-only update cannot silently retire owned global files and the frozen view has only MCP predecessors — cost if wrong: owner removes/reviews a new bundle for topology change; no hidden global artifact or new retirement schema.

## Binding durability and owner-lifecycle boundary

Ruling: Preserve the existing single atomic control-plane candidate, and distinguish live fail-closed uncertainty from immediate-crash recovery — an atomic replacement may have committed before its acknowledgement is classified uncertain, including separate skill adoption and final removal; another ready-state write moves that same ambiguity — cost if wrong: the owner inspects canonical state after an unknown response instead of assuming rollback, and this slice does not claim an unconditional restart-unavailable marker. No new journal or retry is introduced. Live uncertainty retains observed identities unavailable and retires execution. Restart validates the complete snapshot actually present: new bundle connections remain disabled, draft content remains unapproved, and no process, grant, adoption or install is replayed. Prior separately approved exact skills and grants can survive; a completed removal can already be absent. Durable disable/removal intentions that remain in the snapshot stay unavailable; this is distinct from a final replacement that already committed. Immediate-reload regressions must read actual saved bytes without a follow-up save that manufactures the expected marker.

Ruling: Refuse ordinary-agent deletion while it owns bundles, at the authoritative shared transaction as well as the route — otherwise global managed skills lose their owner and a preflight alone races installation — cost if wrong: settle/remove bundles before deleting their agent. Preserve stored resume preferences when unchanged components are updated while disabled, and restrict uncertain predecessor-deletion quarantine to the affected bundle; neighboring owned records and approvals remain exact.

Ruling: Public review/component-state witnesses bind exact semantic skill records, drafts and owned content hashes, rather than the scanner's internal attempt generation — actual built-console acceptance and a real unchanged-scan RED showed that an otherwise valid ten-minute review expired on an idle sixty-second scan — cost if wrong: genuine metadata/content/workspace/capacity changes still conflict, while harmless scans do not require re-review. The internal scan/load concurrency counter is preserved. This bounded acceptance correction follows the initial passing gates; fresh relevant Rust gates are required again, with unchanged SDK/web inputs retained.

## Final current evidence (2026-10-14)

Fresh Rust/core **2,619 passed / nine intentional ignores / 40 suites / lint/build**, including **16 M45 native / 11 pure-core cases** and the **187-operation compiled README**. SDK **145 passed / three intentional service skips / build/typecheck**. Web **1,291 passed / 113 files / typecheck/build**, including **18 community-panel cases**. All 33 frozen source hashes and UTC ticks remained exact through actual acceptance; no new dependency or post-gate production change.

Actual built console/current typed SDK/owner HTTP completed review-only zero writes, install without child/grant, separate exact initial and updated skill approvals, metadata enable without start, explicit MCP start/discovery/full review/grant/Stop, changed launch with fresh disabled ID and no authority transfer, quiet same-profile restart without replay, disable and confirmed recoverable removal. Exact updated canonical skill bytes were checked in workspace trash; independent neighbor metadata/bytes and `read_file` stayed exact. One peer recorded initialize1/initialized1/list1/call0. Native executor acceptance separately proves one Ask/AllowOnce call with durable provenance/receipt and pending-Ask disable fencing zero extra RPC.

The actual console exposed a within-TTL review conflict after an unchanged background scan. An actual `SkillService.scan` RED confirmed the cause. Only the raw scanner-attempt generation was removed from the public bundle observation; exact semantic records/drafts/workspace/MCP/grants/owned hashes and internal scan concurrency fencing remain. The regression also proves a real semantic enable change conflicts without writes. Relevant full native gates were rerun; unchanged SDK/web inputs retained their passing gates.

All six exact owned synthetic processes stopped, ports4200/8081 free and the created tab closed. Zero runtime model tasks/messages/tokens, ordinary profiles/accounts, database servers/containers/Docker55432 or public deployment. Quiet forced-daemon recovery is not graceful-shutdown evidence. Controlled deterministic calls do not prove provider task quality, arbitrary executable/server safety, publisher authenticity, external filesystem atomicity or full Hermes parity. Uncertain storage acknowledgements are inspected as actual committed snapshots; prior explicit approvals may remain and final erase may already be absent, as ruled above.

Exact REDs, review dispositions, corrections, proof files, publication and verified merge settlement live in `.superpowers/sdd/2026-10-14-companion-community-packages-m45/progress.md`. See [bundle use/contribution instructions](../../companion/community-bundles.md), [harness evidence](../../companion/harness-acceptance.md) and [remaining parity](../../companion/parity-roadmap.md). M46 remains held planning until verified M45 main and its five bound controller risks.
