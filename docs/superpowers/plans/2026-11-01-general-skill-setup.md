# General Skill Setup Implementation Plan

> **For agentic workers:** Execute bounded source, command-runtime and integration
> tasks with independent file ownership. Reviews and combined validation are
> coordinator-owned. The owner has already authorized implementation.

**Goal:** Let Anima inspect skill sources and follow documented setup commands
across public GitHub/HTTPS sources and installed language runtimes.

**Architecture:** Source adapters feed ordinary inactive Skills imports. A
general command workspace shares the existing native custody/approval ownership
with Python setup. The agent decides the method from original documentation.

**Tech Stack:** Rust daemon, existing HTTP/SSRF and package validators, native
process custody, SDK TypeScript, existing Skills React UI and Bun verification.

## Source inspection and import

- [x] Add failing tests for public source routing, original-file inspection,
  referenced-file imports, hostile URLs/paths, missing files and provenance Keep.
- [x] Add `skills/source.rs` and tests; extend `github.rs` for bounded original
  inspection. Reuse its immutable acquisition and direct-owner import admission.
- [x] Extend `SkillDraft` with optional `urlSource`, including original bytes,
  package hash, source URL and per-file capture hashes. Update draft persistence,
  approval, original-byte checks and imported-draft retention together.
- [x] Keep GitHub compatibility and prove untrusted input cannot autoactivate.

## General command environments

- [x] Add failing tests for command preparation, state-bound command execution,
  failure invalidation, changed source/state, cancellation and environment leaks.
- [x] Add `skills/command_runtime.rs` with status/prepare/run, exact command and
  cwd, expected state hash, bounded runtime and durable result manifest.
- [x] Extract/share custody, operation lease, environment and capture helpers
  from `python_runtime.rs`; preserve existing Python behavior and drain/Doctor
  accounting. Keep all native build execution coordinator-owned.
- [x] Preserve immutable approved source separately from mutable dependencies,
  validate bounded state hashes (including safe in-tree tool-created links), and
  retain unknown physical custody until reconciled.

## Owner tools, SDK and UI

- [x] Add owner tools `inspect_skill_source`, `import_skill`, `manage_skill_setup`
  with closed argument schemas and secret-safe previews. Inspection is read;
  import is write; effectful setup always requires fresh exact approval.
- [x] Integrate owner scope, discovery, grants, argument/cancel bindings,
  approval policy and action observations. Preserve legacy tools.
- [x] Update SDK optional URL provenance, exports and round-trip tests.
- [x] Show captured source/file provenance and original metadata in Skills review
  with safe links and accurate selected-file coverage. Test existing GitHub cards.
- [x] Replace special-case coaching with the general documented setup workflow;
  retain precise tool-specific boundaries and real verification requirements.

## Review, validation and integration

- [x] Review specification and implementation for requirements and correctness.
- [x] Run focused native regressions, real Node/Bun fixture acceptance, complete
  `bun run rust:test`, `bun run rust:lint`, relevant SDK/web tests/build/typecheck
  and whitespace checks. Reuse the main cache wrapper and private short temp.
- [x] Update user/contributor documentation and record evidence, distinctions
  between local checks and untested live model/account outcomes, and cleanup.
- [x] Integrate the verified feature, preserve any concurrent unrelated edits,
  gracefully reload an idle local app, and verify owner tool availability and
  pending import persistence. Commit within the existing requested Git scope;
  do not push or connect real accounts as a side effect.

## Verified evidence

- Full native suite: 5,783 passed, zero failures, 63 intentional ignores across
  112 summaries. Formatting and workspace Clippy correctness passed.
- Installed Node/npm/Bun offline fixture passed. Three real managed Python
  fixtures passed for assets/private environment, changed source and Stop.
- Skills UI: 77 passed. SDK: 349 passed, six service-dependent cases skipped.
  SDK/web builds, type checks, lint and whitespace checks passed.
- Independent source, runtime and result-reporting reviews approved the final
  implementation. Windows process-facing path/cwd identity is covered.
- These are local/offline checks. Real-model follow-through, account sign-in,
  arbitrary language packages and remote side effects are not measured here.

## Local completion

Feature commit `92841b75` is integrated on main. The normal local app was
gracefully reloaded only after every activity counter was zero. Instance
`fa57ff7d-98a0-44d1-9567-5b18fdde412c` advertises all three new owner controls;
the console proxy reaches the same instance. The original pending last30days
package survived with 159 total files. The owned feature worktree, verification
temporary storage and scratch logs/scripts were removed after recording evidence.
The shared native cache is 17.5 GiB, below its enforced 32 GiB cap. No push,
provider call, account sign-in or package execution in the real owner profile
was performed as part of acceptance.
