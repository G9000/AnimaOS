# Continuous skill learning implementation plan

The owner approved implementation of the preceding comparison on 2026-10-21. Implementation stayed on main without commits until the owner authorized a scoped local commit on 2026-10-21. Existing notices remain; new product wording is Anima-specific. No change to live owner profiles is needed for acceptance.

**Goal:** A corrected or useful workflow improves a reusable skill, preserves approval and undo, and demonstrably helps an independent fresh session.

**Architecture:** Extend the existing bounded, tool-free Rust learning reviewer with structured skill candidates. Resolve and re-read approved packages on the host, pin all modifications to their reviewed hash, and publish through the existing skill draft/approval/ledger services. Reusable patch, evidence and usage primitives belong in anima-core; filesystem and persistence stay in rust-daemon. No Python runtime is imported.

**Scope and controls:** Prefer patching the skill actually used, then a matching existing skill, then creating a skill. Retain Strict/Custom review and Free automatic activation with undo. A failed or unverified task is never proof of a reliable procedure. No forced update when nothing useful was learned. No automatic deletion, archiving or permission changes. Maintenance reports duplicates/staleness; owner controls already disable/remove skills.

## 1. Reusable learning primitives

- [x] Add exact, bounded targeted text patches, rejecting absent/ambiguous/no-op matches.
- [x] Add bounded per-version usage records separating loaded, completed, failed, owner-corrected and explicitly verified outcomes. Completion never means verified success.
- [x] Add conservative evidence filtering and meaningful regression tests, observing red before green.

## 2. Durable host experience and maintenance

- [x] Record successful native skill loads against the exact package hash, agent, run and session. Deduplicate repeated file loads in a run; persist without changing approved bytes.
- [x] Settle terminal run outcomes; retain bounded evidence across restart and version updates. Expose statistics and maintenance suggestions through existing skill details.
- [x] Owner-only feedback must name an existing exact (agent, session, run, slug, hash) observation. Only a completed run may receive explicit independent verification; a model cannot attest its own success.
- [x] Identify duplicate procedures and stale versions without mutating user skills. Preserve existing permissions, hashes, bundles and undo.

## 3. Automatic skill refinement

- [x] Include bounded selected tool evidence, loaded skill versions and approved candidate content in task reviews. Do not forward arbitrary private file results or secrets.
- [x] Admit skill-use/correction signals below the ordinary five-action threshold while retaining shared budgets and cancellation.
- [x] Accept structured create/patch candidates with exact bases, anchors and reasons. Freshly read the target, validate bounded changes, preserve supporting files, and reuse the existing draft pipeline.
- [x] Link each candidate to durable review lineage and deduplicate retries. Strict/Custom remain pending; Free uses the existing ledger approval and undo path. Recheck source, current policy, workspace and hash before activation.
- [x] Automatic activation and the full before-package/undo ledger entry share one persisted control-plane snapshot. Never report automatic activation if its undo record could not be saved.
- [x] Add normal-turn authoring guidance and update /learn for supporting files. Prefer concrete procedures, prerequisites, pitfalls and verification over incident logs.

## 4. Regression and review

- [x] Cover automatic create/refine, Strict pending/approve/reject, Free undo, stale hashes, duplicate/no-op proposals, secret filtering, cancellation, restart, supporting file preservation and honest outcome labels.
- [x] Run full `bun x nx run rust-daemon:test --skipNxCache` in a validated Windows Nx environment, isolated Cargo target if needed; run Rust lint/format targets. Fix failures and obtain independent spec and code review.
- [x] Update learning/durable-work documentation to describe actual behavior and limits.

## 5. Fresh-session acceptance

- [x] Use an isolated daemon/store/workspace and the configured real model without printing credentials or altering the owner's normal agent/profile.
- [x] Establish a baseline skill and an independently checkable fixture. Run a task with a workflow correction that yields a skill refinement through normal runtime/reviewer paths.
- [x] Start a fresh empty session with a different fixture; ask for the task without quoting the learned steps or naming the skill. Verify exact revised skill load, independent output bytes and durable usage lineage. Compare the baseline failure/correction with the fresh result.
- [x] Exclude reflection/core-memory injection as an alternate learning channel for the fresh task, using a fresh agent with equivalent config and the shared approved skill. Cover both explicit skill context and native load_skill observations.
- [x] Verify approval/undo and restart persistence separately. Retain bounded evidence and state exactly what was demonstrated; a scripted adapter check is not live-provider proof.

Execution uses the planning and subagent-driven development skills, with bounded independent review. User authorization supersedes additional approval/commit/worktree gates in skills. The original implementation was unstaged and uncommitted; the owner subsequently authorized a scoped local commit. No push or destructive cleanup is authorized.

## Evidence recorded during execution

- Core exact-patch/redaction/experience regressions: 13 passed after observed failures.
- Initial integrated daemon pass: 2,785 passed, 29 ignored; one API inventory classification assertion was corrected.
- Live Anthropic claude-sonnet-4-6 acceptance: baseline 16,500, corrected 12,500, independent fresh fixture 14,975. Native exact revised-hash load, empty fresh agent/session, source-linked automatic draft/ledger, supporting files, restart persistence and undo passed. Evidence: `.tmp/skill-learning-live-anthropic-2026-10-21/acceptance-proof.json`. All owned daemons stopped.
- Moonshot attempt received provider HTTP 401 before tools or token usage; retained separately.
- Independent review fixes include fresh source eligibility, exact version labeling, recovery handoffs, atomic terminal observations, atomic activation plus undo, and a hash fence for inferred supporting files.
- Final full `rust-daemon:test --skipNxCache` passed, including core-rust:test, 2,793 daemon library tests plus 150 enabled integration tests (35 host tests intentionally ignored across the targets). Windows validation used an isolated Cargo target, OS temp folder, short Nx socket path, and RUST_TEST_THREADS=4. One existing 500 ms document-read test passed alone after a parallel-load failure; its code and assertions were unchanged.
- Final Rust formatting, harness syntax/self-tests and diff checks passed. Implementation validation completed on main before staging; unrelated UI edits were preserved.
