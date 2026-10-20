# Synthetic violet community bundle

This small Windows example reuses the existing deterministic MCP test peer and one
procedural skill. It exercises reviewed configuration and content; it makes no claim
about third-party compatibility, publisher authenticity or model task quality.
The peer returns a fixed synthetic result. The manifest requests one remote tool
name and no native tools; requests never create grants.

Use an isolated temporary workspace and an ordinary owner-controlled agent. The
adjacent SKILL.md is the readable source of the inline `skillFile` in manifest.json;
it is not an extra reference-file installation mechanism. Its source bytes are
included exactly in the manifest. Attribution and licensing status are documented
in ATTRIBUTION.md and the manifest. Repository metadata declares MIT, while this
checkout has no root LICENSE text or separate example license grant.

## Prepare the controlled peer

From the repository root, the operator explicitly builds the existing fixture once:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='1'
bun --no-env-file x nx run rust-daemon:build --excludeTaskDependencies --skipNxCache --args='--example mcp_stdio_peer'
```

The resulting executable is `target/cargo/debug/examples/mcp_stdio_peer.exe`. This build
command is operator preparation and never runs from review or installation. Bind
the program reference `synthetic-peer` to the actual prebuilt absolute executable
path in Community bundles. For this checkout the binding is:

```json
{
  "synthetic-peer": "C:\\Users\\leoca\\OneDrive\\Desktop\\anima\\animaos-kit\\target\\debug\\examples\\mcp_stdio_peer.exe"
}
```

Use your own checkout's absolute path if it differs. The declared literal argv is
`["--mode","normal"]`; there is no shell, argument substitution, environment,
working-directory override, installer, downloader or package-manager hook. Trust
the installed program separately. A manifest or review digest does not pin its
executable bytes or provide a sandbox.

## Review and adopt explicitly

1. Paste the exact manifest.json text into Community bundles and enter the owner
   binding. Inspect the manifest, compatibility, requested permissions, attribution,
   literal argv and original/canonical skill text. Check the local-code trust box
   only after inspecting that launch. Review bundle is read-only.
2. Install the exact host-issued review ID/digest before its expiry. Confirm that
   the bundle is installed, its MCP connection is disabled and ungranted, and its
   owned skill has a pending draft. Review/install/list/restore must start no child,
   call no tool and approve no skill. Installation does not install the executable.
3. Follow Review skill drafts to the existing Skills screen. Inspect and separately
   approve the exact canonical managed draft. Editing that draft body requires a
   fresh bundle review/update or an independent skill; an unrelated ordinary draft
   keeps its existing editing behavior. The adopted skill is workspace-global.
4. Follow Manage external tools. Separately enable the owned connection, explicitly
   start it, inspect discovery, review the exact descriptor and grant only the
   `synthetic_stdio_record` alias. The host derives the alias from the connection
   and descriptor; the manifest name does not establish an executable alias.
5. For controlled acceptance, request one actual executor call with
   `{"value":"violet"}`. Use normal Exec Ask/AllowOnce approval and inspect the
   durable receipt, connection/descriptor provenance and synthetic result. Do not
   invoke it automatically from the skill, installation or a review preview.

No provider, model, account, secret or ordinary profile is needed. A successful
synthetic call demonstrates this selected local configuration only. Opening the
example or installing its metadata is not evidence that the call passed.

## Update, disable and remove

Inspect the saved bundle before selecting Review update. A new review pins its
exact owner, bundle ID, current version and content/bindings. Lifecycle actions pin
the current version and reviewed component-state digest. Expired, stale or uncertain
results require explicit reads and fresh review; mutation requests are not retried.

For a changed-launch example, change the manifest version, skill text and literal
argv to `["--mode","split-frames"]`, then review the complete new manifest with
the owner binding again. The new connection receives a fresh disabled ID without
transferred descriptor review or grants; the old disabled predecessor's existing
grants remain dormant until explicit removal. Changed skill content becomes another
pending draft, leaving previously approved bytes until explicit adoption. Unchanged
components retain their exact IDs/configuration. Existing connection/draft quotas
may refuse the update without evicting unrelated work. Existing skill keys cannot
be removed, renamed or changed to another kind by an update; remove and review a
new bundle for that topology change.

Disable revokes availability and retires the transport; re-enable does not start it,
grant tools or approve new text. Remove affects only exact owned components and
retained predecessors, moving owned skill files to workspace trash. Manual edits
conflict instead of being overwritten. Owned skill disable/remove affects every
agent in this workspace. Active/uncertain cleanup remains unavailable until exact
reconciliation; completed effects are not undone. Restoring metadata never replays
an installation or launches the peer.

For RPC-count evidence, root may use a private instrumented copy that declares
`--marker` and an absolute temporary marker path as additional literal arguments.
Those changed bytes require a fresh full review/digest and distinct launch identity;
do not substitute arguments behind an existing receipt. Label public-example
evidence separately from that instrumented acceptance copy.

## Contribute another local example

Keep manifests declarative, bounded and versioned. Preserve exact inline skill
source, attribution/notices, literal argv and explicit preinstalled owner bindings.
List compatibility and permission requests honestly. Do not add executable artifacts,
archives, reference files, scripts, automatic launch, OAuth, package-manager hooks,
model-derived authority or default grants. Reuse the existing MCP/skill APIs and
approval screens rather than introducing a second approval path.
