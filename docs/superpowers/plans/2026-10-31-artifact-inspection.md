# Artifact inspection implementation plan

> For agentic workers. Use superpowers:subagent-driven-development for bounded ownership and independent review. The coordinator has resolved the shared design checkpoint. No further human design gate applies.

Goal is to inspect exact captured Office bytes before completion using actual local rendering and independent formula recalculation.

Architecture uses one native inspection operation, immutable reports and current source authority. Pure domain/admission/report validation lives in `anima-document-visual::inspection`. The daemon owns the finite worker, source/candidate leases, bounded private records and authenticated page access. Initial Office admission rejects every external relationship/hyperlink, active content and unknown formula function. Job limits and UNO flags are accurately described as resource/lifetime controls, not OS network isolation. Stronger AppContainer execution is deferred.

Tech stack is existing Rust document crates, Windows Job custody, trusted bundled LibreOffice Python/UNO and existing Windows PDF rendering. All native or converter execution remains parent-only in the combined copy and canonical primary Cargo target.

## Throughput checkpoint

- Blocking first steps. Freeze the public domain and worker signatures, then author behavioral tests before logic. Native red/green execution is delegated to the coordinator by explicit task constraint.
- Independent workstreams. Domain/admission/tests and worker/UNO/tests use disjoint owned modules. The lead owns daemon source service/routes, SDK, integration and documentation.
- Shared mutable state. Every helper edits only its assigned files in this worktree. Parent alone merges shared feature roots and runs native checks. Each physical inspection has its own private profile and output directory.
- Smallest safe decomposition. One domain owner and one worker owner meet through a single typed output; one lead joins authority, bounded records and authenticated routes. No scheduler or separate database is added.

## Tasks

- [x] Add `anima-document-visual/src/inspection.rs`, module export and behavioral tests for exact hashes, full/partial pages, formula inventory, unsupported content and deterministic pixel observations.
- [x] Add `hosts/rust-daemon/src/document_inspections/worker.rs` and a fixed trusted UNO controller, with bounded framing, explicit source admission and checked whole-tree resource custody. Author worker/protocol tests without invoking the engine locally.
- [x] Add a service with current owner/project/workspace/permission checks before capture, after worker retirement and before every report/page access. Support workspace, selected attachment and private candidate references only through their existing custody rules.
- [x] Keep bounded immutable inspection records under existing feature-owned private persistence. Stale source/grant/candidate changes prevent fresh delivery. Report digests and bounded observation locators grant no authority.
- [x] Add authenticated create/read/page routes, the native read-only inspection tool and leaf SDK methods without new general UI controls.
- [x] Document exact supported engine setup, inert-subset limits, formula/view coverage and unavailable/partial outcomes.
- [x] Obtain independent source review, repair findings and send a frozen integration manifest. No commit, push, provider call, engine launch or native check is permitted in this child.

## Parent validation

Run sequentially in the combined parent copy. Existing wrapper resolves the absolute primary `target/cargo/validation`; do not create child targets.

```powershell
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-document-visual inspection
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path Cargo.toml -p anima-daemon document_inspections
bun run rust:test
```

Physical acceptance uses the pinned real LibreOffice backend and synthetic DOCX/PPTX/XLSX blank/clipped/stale-cache/hidden-error/unprinted-error fixtures. Fake protocol workers prove framing and retirement only. No-clipping judgments remain separately labeled; deterministic blank pixels or page coverage never become a generic quality pass.
