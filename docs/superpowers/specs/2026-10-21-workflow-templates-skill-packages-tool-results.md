# Reusable work in chat: three capability upgrades

The owner explicitly authorized building all three previously proposed capabilities, browser acceptance, and work on the existing main checkout with no staging or commits. This document records the implementation design; routine details are resolved within that scope.

## 1. Reusable workflow templates

Adapt OpenClaw/Lobster's named workflow files and argument resolution into native Rust data, rather than starting a second runtime. An agent-owned template contains a slug, title, bounded named string parameters with defaults, and the existing ordered task/approval/input steps. Use a constrained `${name}` parameter syntax with explicit missing/unknown argument errors; no executable expressions, environment expansion, or shell interpolation.

Saving is revision-checked and exact retries are idempotent. Running creates a fresh workflow snapshot with supplied values, a unique creation key and a pinned source template revision. Later template edits do not mutate prior runs or replay earlier tasks. Durable template state upgrades version 24 with an independent pre-upgrade backup and preserves all prior backups. Expose owner CRUD/run APIs, explicit save/list/run tool grants for ordinary chat, and a template picker/editor/run form in Work and chat. Owner checkpoint controls stay unchanged.

## 2. Richer skill packages

Extend the existing skill registry, draft review, file scanner and load tool. A package includes SKILL.md plus bounded UTF-8 supporting files under references/, templates/, scripts/, examples/ and assets/. Suggested limits: 32 files, 64 KiB per supporting file, 512 KiB package total. Reject traversal, absolute/device paths, symlinks/reparse escapes, duplicate paths and unsupported files. Scripts remain inert until explicitly run under existing execution permissions.

Review and pin the complete package hash, including paths and content. A changed supporting file invalidates the approved package exactly as a changed SKILL.md does. Keep existing single-file skills valid. Owner editing/import and model proposals can include supporting files; pending reviews display the complete file list and safe text previews. Load SKILL.md with a compact supporting-file index; allow explicit retrieval of a reviewed file through the existing read-only skill-loading authority. Do not inject every file into every model turn.

## 3. Durable large tool results

Adapt Hermes' documented spillover pattern: preserve bounded oversized tool results as immutable private artifacts and give the model a compact preview plus an opaque reference. Preserve settled success/error and action receipts; a failed artifact write cannot turn a completed external effect into a retriable successful action. Explicitly mark unavailable or incomplete output and retain existing review boundaries.

Use a durable, agent-scoped private store with atomic writes, integrity checking and hard byte/count quotas. Never expose arbitrary host paths or allow cross-agent reads. A dedicated owner API and chat tool page through bounded text; chat's tool card can inspect/download the full saved result. A separate opt-in permission controls model reads of retained results. No remote uploads or background deletion of referenced results. Existing tool collection limits remain explicit; storage cannot recover data a provider never returned.

## Integration and acceptance

- Reusable/core data and validation remain under packages/core-rust; host filesystem/HTTP/job integration remains in the daemon. SDK/UI remain clients.
- Parent owns shared registration, routing/OpenAPI, state snapshot integration, permission toggles and final docs to avoid conflicting edits across feature workers.
- Each capability has focused red/green regression tests; cover durable restart/replay, stale revisions, bounds, filesystem scope, grant revocation and safe rendering. Run relevant Nx SDK/web checks and the required Rust daemon suite in the combined final tree.
- Live browser acceptance uses synthetic text-only workflows and files: save a parameterized template through chat, run twice with different inputs, approve/answer in chat and reload; propose/review/load a package with a supporting template; produce and retrieve a large benign result in chat. Confirm permission controls, unchanged earlier workflow output, and actual saved full-result retrieval. Save screenshots and a test report.
- Restart only the relevant local development daemon after confirming its identity and idle state when needed to load new APIs. Preserve Hermes, unrelated processes and real user records. No commits, staging, pushes, external test sends or account/security changes.
