# Full skill packages and managed Python setup

The owner authorized upgrading setup to the Hermes/OpenClaw standard after a
read-only comparison of the installed Hermes source and current OpenClaw source.
The earlier text-only importer cannot preserve the last30days package.

## Design

Keep the existing skill registry, owner review, hashes and rollback. Expand its
inert package representation to portable text files and encoded binary media,
with hard decoded-byte limits (256 files, 8 MiB/file, 32 MiB/package). Preserve
original root files. Retrieve every file at one immutable commit, verify Git
blob identities, and reject incomplete or unsupported packages atomically.

Separate installation from environment preparation and successful execution.
An owner-chat-only `manage_skill_runtime` accepts status, prepare and run.
Preparation creates a private package snapshot and dedicated Python venv outside
the agent workspace. Dependencies come from reviewed requirements and fixed
PyPI wheel installs. Run accepts an approved relative Python entrypoint and
argument array. Both effectful actions require exact owner approval, including
Free mode. A venv isolates dependencies; code has the local user's OS access.
This does not enable the general host shell. No ambient account secrets are
inherited. Strong process custody, deadlines, Stop, current owner authority,
package hashes and native action receipts apply. Successful checks describe
only the exact entrypoint and arguments observed, never account readiness.

Alternative considered: delegate all setup to the general shell, as Hermes can.
Rejected because it would require enabling the existing global execution gate
and would lose package-specific setup and readiness evidence. A Docker-only
path was also considered; this would require an already installed image and
would not provide a usable default on this Windows installation.

## Ownership and validation

- Package model and nofollow disk transactions: package upgrade agent.
- Complete pinned GitHub acquisition: GitHub upgrade agent.
- Python setup, snapshot custody and subprocess lifecycle: runtime agent.
- Tool contracts, exact approvals, UI/SDK, live backend and integration: coordinator.

Parallel source edits have disjoint ownership. The coordinator runs native
validation sequentially in the managed cache, then SDK/UI checks and an
independent review. No production account sign-in or provider call is needed.
Refresh the idle local development backend only after successful checks, and
verify its advertised tool contracts. The owner subsequently requested a local
commit of the completed upgrade.

Throughput checkpoint: large package reads and validation stay bounded; no
binary bytes enter model context; source and runtime operations retain separate
owner decisions; incomplete downloads and failed dependencies never become
ready. Full native compilation is the main validation cost and runs once the
implementation is integrated.

## Acceptance

Full package, SDK/UI, native, real Python/dependency/Stop, and pinned public
last30days acquisition checks passed. A public Metricool discovery check exposed
its missing issuer-response advertisement; a separately reviewed exact-provider
compatibility profile was added and verified while retaining strict behavior for
arbitrary providers. See `test-output/pstack-full-skills/acceptance.md` for exact
commands, evidence and live-account limits. The standard local development app
was rebuilt and restarted; the existing owner chat advertises all three setup
tools and the owner frontend responds successfully.
