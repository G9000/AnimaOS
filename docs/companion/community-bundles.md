# Reviewed community bundles

Anima community bundles contain reviewed MCP configuration and inline SKILL.md content.
They belong to one existing ordinary agent in a configured workspace; their skills are workspace-global.
Installation saves disabled MCP connections and pending skill drafts. It adds no effective grants,
approves no skill, starts no transport and installs no executable. Helpers obtain no bundle authority.
M45 scoped implementation and controlled local acceptance have passed. Exact source, PR and verified
merge settlement is recorded in the M45 ledger. M44/PR50 is verified
merged at `15e40c397a6ef961639e9375fe50ee513d09c9bf`.

Fresh gates: Rust/core **2,619 / nine intentional ignores / 40 suites / test/lint/build**, **16 M45 native cases / 11 pure-core / 187 compiled README operations**; SDK **145 / three intentional service skips / build/typecheck**; web **1,291 / 113 files / typecheck/build/18 panel cases**.
The actual built-console/SDK/owner-HTTP workflow reviewed, installed, separately adopted/granted, updated,
quietly restarted, disabled and removed the synthetic bundle while preserving native grants and a neighbor.
Browser peer counts were **1 initialize / 1 initialized / 1 list / 0 calls**; the separate native executor
used Ask/AllowOnce for exactly one call and disable fenced further RPC. See [harness evidence](harness-acceptance.md)
for exact hashes, source freeze, scanner regression, cleanup and remaining quality/acceptance limits.

The [local synthetic violet example](../../examples/community-bundles/synthetic-violet/README.md)
contains a complete manifest, readable skill source, operator preparation and attribution.
Use an owner-bound preinstalled Windows `.exe` or an existing anonymous Streamable HTTP endpoint.
HTTP permits HTTPS or literal loopback HTTP under the [existing MCP rules](mcp.md).
Local process execution is Windows-only. Supply absolute executable bindings independently of the manifest;
no PATH search, shell, environment/cwd override, download or package-manager hook is available.
Digest equality does not authenticate a publisher, pin executable bytes or provide a sandbox.

The strict v1 JSON declaration requires every member below; unknown, duplicate and explicit null members
are rejected, including nested declarations. The manifest chooses no resource IDs, slugs or effective grants.

| Required member                        | Meaning                                                                               |
| -------------------------------------- | ------------------------------------------------------------------------------------- |
| `schemaVersion`                        | Exactly `1`                                                                           |
| `id`, `version`, `name`, `description` | Publisher identity/release and plain display text                                     |
| `compatibility`                        | `{host,minVersion,maxVersionExclusive,requiredCapabilities}`                          |
| `permissions`                          | `{nativeToolNames,externalTools:[{component,remoteName}]}`; descriptive requests only |
| `attribution`                          | Nonempty `[{component,source,revision,license,notice}]`; untrusted claims             |
| `components`                           | One to four strict variants below                                                     |

| Component kind | Exact members                                             |
| -------------- | --------------------------------------------------------- |
| `mcpHttp`      | `{key,kind,label,endpoint}`                               |
| `mcpStdio`     | `{key,kind,label,programRef,args}`                        |
| `skill`        | `{key,kind,skillFile}`; complete inline standard SKILL.md |

Versions are exactly three decimal unsigned integers. Runtime minimum is inclusive and maximum exclusive;
required capabilities must match the daemon's actual report, rather than being inferred from its version.
The supported host is `rust-daemon`; capabilities include `mcp.owner.v1`, `skills.import-draft.v1`
and, on Windows, `mcp.stdio.windows.v1`. Original skill bytes and canonical installed bytes have separate hashes.
Manifest identity uses strict canonical JSON/SHA-256; review identity also binds the owner/workspace,
resolved launches, canonical skills, capabilities, generated identities and current component witnesses.

- Manifest input: 128 KiB UTF-8, JSON depth 16 and 4,096 nodes; at most two MCP and two skill components.
- Identifiers: bounded lowercase ASCII data, never paths; component keys 24 bytes, bundle/program IDs 64.
- Stdio: at most 32 literal arguments, 2,048 bytes each; executable 2,048 bytes and combined launch content 16 KiB.
- `[]`, `[""]` and empty members remain distinct; spaces and quotes are literal, without tokenization or trimming.
- Inline skill source: 64 KiB import cap; existing 32 KiB body and 4 KiB front-matter limits still apply.
- Joint workspace limits: 16 bundles, four configured MCP connections, ten pending imports and 200 approved skills.
- At most ten review receipts, expiring after ten minutes. Unavailable bundles and disabled predecessors retain quota.

In Connectors, use Community bundles to review and install explicitly:

1. Paste the exact manifest and bind each declared `programRef` to a preinstalled absolute executable.
   Bindings must match exactly the distinct references; arguments come only from the manifest.
   Inspect literal argv and confirm trust in those local programs. Any edited input requires a fresh review.
2. Choose Review bundle. Inspect original/canonical skills, hashes, compatibility, permission requests and
   plain attribution. Review creates a bounded receipt, with no connection, skill adoption or tool call.
3. Install that exact receipt before expiry. The bundle permits separate adoption, while new MCP rows remain
   disabled/unreviewed/ungranted and owned skills remain pending drafts.
4. Follow Review skill drafts to Skills. Separately approve the exact canonical managed draft.
   An edited managed body requires fresh bundle review/update or an independent skill; ordinary drafts remain editable.
5. Follow Manage external tools. Separately enable/start, inspect discovery, review the exact descriptor and
   grant its derived alias through existing MCP controls. Normal Exec approval and durable receipts still govern calls.

Inspect the current bundle before updating. Review update pins its internal ID and saved version;
Save reviewed update pins that receipt and version again. Changed bindings/content need fresh review.
Unchanged validated components retain IDs, descriptor reviews, grants and approved content/state.
A changed MCP launch gets a fresh disabled ID without authority transfer; the old disabled predecessor
keeps its own reviews/grants dormant and counts toward the four-connection limit until explicit removal.
Changed skill content becomes a pending hash-fenced draft; prior approved bytes remain until separate adoption.
Capacity refusal never evicts unrelated resources. Existing skill keys cannot be removed, renamed or changed
to another kind by update: remove/review a new bundle for that topology change.

Disable and remove require current version plus exact component-state digest. Disable revokes availability,
invalidates pending managed adoption and joins owned transport retirement. Re-enable preserves retained
preferences and exact still-approved bytes, with no process start, new grant or automatic approval.
Remove affects only recorded current/predecessor connections, their aliases and exact owned skills/drafts;
owned skill folders move to workspace trash. Manual edits/ownership/hash changes conflict instead of being overwritten.
Owned skill disable/remove affects every agent in this workspace. Neighbor resources remain independent;
completed tool effects are not undone, and external-editor read/rename races are not atomic filesystem CAS.

All routes use local-owner authorization and `Cache-Control: no-store` under
`/api/agents/{agentId}/community-bundles`. IDs in paths are encoded by the shared SDK transport.

| Operation                    | Request                                                                                            | Success                                            |
| ---------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| POST `/reviews`              | `{manifestText,programBindings,bundleId?,expectedBundleVersion?}`; target pair both absent/present | 200 `{review}`                                     |
| POST collection              | `{reviewId,reviewDigest}`                                                                          | 201 `{bundle}`                                     |
| GET collection / GET `/{id}` | No body                                                                                            | 200 `{bundles}` / `{bundle}`                       |
| POST `/{id}/update`          | `{expectedVersion,reviewId,reviewDigest}`                                                          | 200 `{bundle}`                                     |
| PATCH `/{id}`                | `{expectedVersion,reviewedComponentStateDigest,enabled}`                                           | 200 `{bundle}`                                     |
| DELETE `/{id}`               | `{expectedVersion,reviewedComponentStateDigest}`                                                   | 200 `{bundleId,removed:true,trashPaths,toolNames}` |

Mutation views carry canonical current owner `toolNames`, separate from manifest requests. Existing Settings/MCP
permission fences reconcile these names while preserving unrelated owner edits; no optimistic grant or retry is inferred.
Errors retain static `{error:string}`: malformed 400, forbidden 403, missing/foreign 404, stale/capacity 409,
and durable/cleanup uncertainty 503. Inspect canonical bundle/MCP/skill state after an unknown response.

Reviews expire after ten minutes or a semantic change to their bound records,
content, workspace or permissions. An unchanged background skill scan preserves
the review; the scanner's internal concurrency counter grants no new authority.

One atomic control-plane candidate saves metadata before publication. Live 503 uncertainty retains observed
identities fail-closed and retires affected execution; it does not prove rollback. Immediate restart validates
whichever complete prior/observed snapshot actually persisted. Previously separately approved exact hashes/grants
may survive, and final removal may already be absent. There is no universal restart-unavailable guarantee.
Restore never launches, grants, adopts or replays installation/cleanup. Remaining durable cleanup intentions stay
unavailable. Retained cleanup-intent recovery is an explicit remaining limit; there is no new reconciliation endpoint
or automatic cleanup. CP19 migration preserves CP18 shapes with an independent `.pre-community-bundles.bak`.

Contributors should reuse the [host-neutral validator](../../packages/core-rust/crates/anima-core/src/community_bundles.rs),
[owner coordinator](../../hosts/rust-daemon/src/community_bundles/service.rs), [typed SDK](../../packages/sdk/src/community-bundles.ts)
and [existing SKILL.md parser/composer](../../hosts/rust-daemon/src/skills/file.rs).
The compiled host's [OpenAPI schema](http://127.0.0.1:8080/openapi.json) describes owner envelopes;
[Cargo dependencies](../../hosts/rust-daemon/Cargo.toml) retain the official Rust MCP SDK and maintained process-wrap.
Bundles use those compiled libraries and the standard SKILL convention; they do not load native plugins.
Preserve Anima branding and actual pinned source/notices. The example's [attribution](../../examples/community-bundles/synthetic-violet/ATTRIBUTION.md)
states MIT metadata and the absent root license text; claims are not extra redistribution grants.
No private Hermes profiles are copied. Archives, registries, executable installers, authentication/credentials,
connector contributions and native plugin loading remain outside this slice. See the [M45 plan](../superpowers/plans/2026-10-14-companion-community-packages-m45.md)
and [parity roadmap](parity-roadmap.md); this selected local composition does not establish full Hermes parity.
