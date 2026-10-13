# M39 reviewed learning skill adoption implementation draft

> For agentic workers: execute the root-bound canonical plan task by task;
> root owns RED, serial gates, services, real acceptance and publication.

**Goal:** turn one stored reviewed lesson into an ordinary owner-approved skill,
then prove that skill is loaded and useful on two fresh related tasks.
**Architecture:** reuse SkillDraft, its existing approval/file/hash machinery,
and existing task skill loading; add a narrow learning-to-draft handoff.
**Stack:** existing Rust daemon, control-plane snapshots, SDK and React console.
**State:** implementation milestone; exact gates and acceptance are recorded in its private ledger.

## Scope and current evidence

1. Canonical handover Start here identifies PAR-02 as the next work after M38.
2. PAR-01 is implemented and locally accepted; advice remains non-authoritative.
3. PAR-02 is larger than this milestone; M39 proves its skill adoption wave only.
4. M38 already persists source run/session, digest, anchors, proposals and versions.
5. SkillDraft already supports new slugs and replacements with a stored base hash.
6. Owner approval writes SKILL.md and pins the exact saved file hash.
7. SkillService load/load_slug rereads disk and refuses missing/changed/disabled files.
8. The coordinator attaches approved skill instructions to a fresh isolated runtime.
9. A saved draft or a successful load alone does not prove useful learning.
10. Existing M38 acceptance observed three suggestions, not adoption or correctness.

## Root scope ruling received during planning

- Implement reviewed SKILL.md handoff, existing approval, two fresh related tasks,
  actual load_skill/use/output verification and one quiet same-store reload.
- Keep profile/agent-note views, preference corrections, generic conversation
  memory adoption and unattended curation on the explicit PAR-02 backlog.
- No new model call to compose a draft: the owner reviews/edits its exact bytes.
- Pending or acknowledged proposal can be handed off; dismissed cannot.
- Handoff does not silently acknowledge advice or approve/enable a skill.
- No second skill approval engine, no review-runtime fork and no new grants.

## Source seams and constraints

| Seam                        | Current source anchor         | M39 use                                                   |
| --------------------------- | ----------------------------- | --------------------------------------------------------- |
| Source eligibility/digest   | learning/evidence.rs:30,118   | Fresh capture outside state guards; compare stored digest |
| Review owner/version checks | learning/service.rs:128,265   | Reuse ownership, Completed state and proposal version     |
| Durable review/proposal DTO | learning/records.rs:78,88     | Optional durable draft pointer; existing advice unchanged |
| Proposal create/update      | skills/drafts.rs:29,123       | Reuse validation, slug/base selection and pending cap     |
| Approval                    | skills/drafts.rs:220          | Keep existing sole approval route and file save           |
| Skill draft lineage         | skills/mod.rs:392,402         | Existing ProposedBy plus optional learningSource          |
| Serialized settlement       | skills/service.rs:215,238     | Shared transaction and detached cancellation-safe save    |
| Existing file precheck      | skills/service.rs:142,157,317 | Existing FileReview is not a stored-base CAS substitute   |
| Fresh hash-pinned load      | skills/service.rs:654,674,717 | Reuse load/load_slug and load_record                      |
| Task context injection      | agent_runs/skills.rs:26       | No transcript clone; exact slug task path remains         |
| Exact own memory CAS        | tools/memory.rs:321,350       | Existing backlog seam; no new memory write in M39         |

All daemon paths above are under hosts/rust-daemon/src/.
Source anchors are this planning snapshot; implementation rechecks exact lines.
Important concrete gap: approve_draft currently records a draft base_hash but its
FileReview check accepts any current registered skill. Linked learning updates
must compare the stored base hash and fresh source at approval, not merely display
the stale badge. Root owns that narrow linked-draft approval guard.

## Frozen candidate public handoff

- POST /api/agents/{agentId}/learning-reviews/{reviewId}/proposals/{proposalId}/skill-draft
- Strict JSON body, deny unknown fields and honor existing bounded HTTP body read:
  {expectedVersion, evidenceDigest, skill:{slug,name,description,body,expectedHash}}.
- expectedVersion is the exact current proposal decisionVersion.
- evidenceDigest is the exact stored source digest, freshly revalidated server-side.
- expectedHash is required, nullable: null for a new skill, string for replacement.
- Slug/name/description/body reuse current skill validation and size limits.
- Server selects agent/session/run lineage; request cannot supply another owner,
  source run, draft ID, grant, approval state, scripts or files.
- First successful handoff: 201 {draft: existing SkillDraftResponse}.
- Exact duplicate: 200 same draft envelope, no new record/save/file/model request.
- Changed body/name/description/slug/base/digest/version conflicts with 409.
- Missing/foreign source or record is generic 404; invalid JSON/text is 400.
- Stale source, dismissed proposal or stale target hash is 409.
- Durable-store unavailable/save failure is 503; do not report a saved draft.
- Local-owner auth precedes body parsing; every response is no-store.

## Additive persisted lineage

- LearningProposal.skillDraftId: optional durable pointer, absent on old snapshots.
- SkillDraft.learningSource: optional immutable server-generated lineage.
- Lineage contains agentId, sourceRunId, sessionId, reviewId, proposalId,
  decisionVersion and evidenceDigest; exact field names root freezes before RED.
- Existing ProposedBy still names the real source run, never a fabricated review run.
- Keep DraftSource compatible with the existing Agent draft pipeline and caps;
  learningSource distinguishes an explicit owner handoff from model tool proposals.
- Do not alter observation/suggestion/anchors or invent source conclusions.
- Persist draft plus proposal pointer in ONE control-plane snapshot settlement.
- Pointer remains after draft decision/pruning; a missing pointed draft returns a
  typed conflict rather than creating another draft and losing dedup history.
- Changed duplicate bytes are compared against the linked draft's exact composed
  content, not a substring or normalized prose match.
- Retain existing 1,000 learning attempts, draft pending limits and decision retention.
- Older stores restore optional fields as absent; validate linked identities/bounds.
- No new canonical memory store, sidecar approval database or attempt-pruning refund.

## Native handoff sequence

1. Validate strict body and bounded skill fields using existing validators.
2. Read exact eligible source/history without holding state read/write guards.
3. Reuse a detached data-only settlement so owner HTTP cancellation cannot half-commit.
4. Take the SAME control-plane transaction used by skills and learning.
5. Recheck live owner/session, Completed review, exact proposal identity/version,
   non-dismissed decision, stored digest and freshly captured source digest.
6. Resolve an existing pointer first; enforce exact duplicate/conflict semantics.
7. Check target current approved hash equals expectedHash including null/new-target.
8. Preserve FileReview/unreviewed-file safeguards; do not overwrite a discovered file.
9. Enforce existing pending-per-agent cap and all registry validation.
10. Create one ordinary pending SkillDraft with baseHash and immutable lineage.
11. Set one proposal pointer; stage both registries, create one snapshot and save.
12. Publish success/update notification only after acknowledged durable settlement.
13. Restore both staged registries on definite precommit failure; preserve the host's
    uncertain-save behavior and avoid claiming exactly-once filesystem authority.
14. No SKILL.md write, model request, grant or automatic enable occurs in handoff.

Avoid calling public SkillService.propose while already holding its transaction:
that would reacquire the same lock; calling it before the pointer save would split
the invariant. Use the existing locked/apply pattern with one narrow joint change.
Root binds uncertainty handling before production; do not add a generic coordinator.

## Existing approval and fresh reuse

- Existing /api/skill-drafts/{id}/approve remains the only installation operation.
- Linked drafts require fresh lineage/source checks before approval and inside its
  settlement, plus exact stored baseHash versus current approved hash.
- Approval retains owner-editable body behavior; exact final reviewed bytes are saved.
- Approval edits do not change the immutable source lineage or pretend the edited
  procedure was generated/endorsed by the reviewer.
- Rejected draft stays rejected; duplicate handoff cannot reopen it.
- Keep existing enabled-state behavior, scans, missing/changed file refusal and rollback.
- Skills cannot grant filesystem/terminal/connectors or execute supporting scripts.
- Fresh acceptance must show a real successful load_skill of the approved slug,
  relevant application and independently checked output on each new task.
- One quiet restart retains pointer, lineage, draft decision and approved file hash;
  reload itself performs no model task, review generation or automatic adoption.

## Compact implementation ownership

1. Root: learning records/exports, SDK/OpenAPI contract, owner route registration,
   snapshot validation and narrow linked-draft approval validation.
2. Native implementer: ONLY learning/adoption.rs (~200–350 lines) after meaningful
   root RED and frozen interfaces; no production edits in this planning pass.
3. Root or assigned skill worker: narrow skills/drafts.rs and service seams needed
   for joint registry save/FileReview reuse; avoid broad visibility expansion.
4. SDK/web lane: additive handoff method/types, LearningReviewPanel review action,
   existing Skills page/editor/card; show exact proposed bytes and source links.
5. UI preserves custom settings/grants, saving/error controls and explicit approval;
   conflict refresh requires deliberate renewed review, with no automatic retry.
6. Native independent reviewer: authority, CAS, dedup, cancellation/save/recovery.
7. Final SDK/web reviewer: nullable required expectedHash, source display, no auto apply.

## Meaningful RED and local fixture tests

- Root first uses existing owner HTTP router and a persisted own Completed review,
  valid exact source/reply/receipts and existing skills workspace; valid handoff
  expects 201 versus missing route404. No missing-symbol-only RED.
- Positive new and update handoffs retain exact bytes, pointer and source lineage;
  no skill file write, canonical memory change or model invocation at handoff.
- Concurrent exact duplicates create one draft/pointer; changed duplicate409.
- Stale evidence/version/base/foreign/missing/dismissed/failed review cases write nothing.
- Pending-cap/unreviewed disk target/hidden text/oversize/unknown-body cases refuse.
- Source change or deletion between read and settle/approve fails closed.
- Definite save failure rolls back both registries; interrupted owner request settles
  once; uncertain storage case reports its actual retained state without duplicate retry.
- Linked update approval conflicts after another approval changes its base hash.
- Legacy draft approval/load and old snapshots without lineage remain compatible.
- SDK transport asserts exact body incl explicit null; web test reviews bytes, saves
  once, preserves cancellation/error and routes conflict back to deliberate review.

## Actual acceptance and cost boundary

- Root chooses ONE useful bounded procedural lesson with a declared independent oracle.
- Reuse an existing eligible synthetic M38 review when useful; no paid regeneration
  merely to manufacture a better proposal, and no personal source/profile reads.
- Owner reviews a short skill body; handoff and existing approval cost zero model calls.
- Exactly TWO fresh related tasks, empty independent sessions, no copied transcript.
- Both tasks must actually load_skill and satisfy the predeclared output oracle;
  do not score a claimed usage sentence or copied final text as proof.
- Root predeclares aggregate calls/steps/time/reported-token ceilings, including any
  existing model/runtime retries. No helper, silent background review or repeat by default.
- If quality fails, record incomplete acceptance; fix only an evidenced source gap.
- Capture exact output, loaded skill identity/hash, run/messages/receipts, unchanged
  neighbors/grants/memory, then one quiet reload and exact-owned-process cleanup.
- This closes the selected skill learning cycle; broader PAR-02 remains incomplete.

## Reuse decision/provenance

- Inspected public installed Hermes pin 1fd18277267152893a8a24792b79213e7ca3f899,
  agent/background_review.py and tools/skill_manager_tool.py; never personal .hermes.
- Root LICENSE is MIT, Copyright (c) 2025 Nous Research.
- \_review_tool_whitelist (background_review.py:1079) allows skills/read-only reads
  and optionally memory for its fork; M39 adds no such autonomous writing fork.
- build_memory_write_metadata (:736) retains task/session/write-origin provenance;
  use the same source-lineage principle without copying Python runtime/profile fields.
- \_guarded_write (skill_manager_tool.py:395) guards read-before-write and rollback.
- \_apply_skill_write_gate (:673) stages full write payload; apply_skill_pending (:698)
  replays an approved staged action with a gate bypass. Anima reuses its existing
  SkillDraft/save/approval/hash code, so no equivalent second replay engine is needed.
- Direct Python reuse or process adapter would add a foreign profile/store/lifecycle
  and approval authority for a small Rust registry operation; greater total cost.
- Selected lowest-cost approach: existing Anima service reuse plus narrow native
  handoff/lineage/CAS glue; no wholesale Hermes implementation or policy parity claim.
- Retain M38 notice; any substantial copied/ported helper must add exact upstream
  source/pin/license/local changes to the relevant notice and independent review brief.

## Top five risk candidates for root binding

1. Joint draft/pointer durability: choose atomic shared-snapshot settlement, exact
   duplicate semantics and definite versus uncertain failure retention before coding.
2. Source authority changes: fresh eligible source digest, same live owner/session,
   exact version and non-dismissed status at handoff AND linked approval.
3. Stale skill replacement: expectedHash nullable/new-target and approval base CAS;
   FileReview's registered-record pass does not itself protect a changed base.
4. False reuse claim: two new task histories with actual hash-pinned load and checked
   task outputs; no inference from saved advice, installed bytes or load alone.
5. Scope/cost drift: owner-edited advice remains data until existing approval; no
   auto memory/skill writes, extra model drafting, grant changes or broad PAR-02 completion.

## Root-only commands and completion evidence

- Serial focused Rust fixture commands chosen from existing rust-daemon target/filter;
  root records exact command and RED/GREEN exit, with CARGO_INCREMENTAL=0 and jobs1.
- Required aggregate: bun x nx run rust-daemon:test --skipNxCache; then native lint.
- Relevant SDK/web Nx test/typecheck/build targets; SDK ANIMA_SKIP_SERVICE_TESTS=1.
- Root runs format, one native batch + final SDK/web review, one justified fix wave,
  required current-tree gates, then bounded declared acceptance and public docs.
- Preserve unrelated nx.json; no worktree/stash/reset/restore, new services/account
  effects, ordinary profiles, dependency containers or public deployment.
- This draft contains no gate result or implementation/acceptance claim.

## Controller rulings on the risks (binding)

1. **Joint durability:** settle pending SkillDraft and permanent proposal pointer under the existing shared control-plane transaction in a detached task and one snapshot save. Restore staged registries on reported save failure, matching existing host semantics; a failure can still be uncertain on disk. Never create a replacement for a pruned pointer. This is idempotent host admission, not an exactly-once disk guarantee.
2. **Source authority:** fresh exact eligible run/reply digest, live own agent/session, completed review and undismissed proposal are required at handoff and linked approval. Handoff additionally checks the displayed decision version and digest. A later acknowledgement is harmless; dismissal blocks a still-pending installation. It does not uninstall a previously approved skill.
3. **Replacement CAS:** require explicit expectedHash, including null for a new slug, and compare it with the current approved record. Linked approval compares the stored base against the current record and actual file bytes. A changed base/source requires a fresh review and independently created draft; the old pointer is never silently reset. Existing ordinary draft behavior remains compatible.
4. **Reuse proof:** two bounded fresh related tasks must actually load the approved skill and satisfy independent receipt/output oracles. Use retained M38 advice to avoid a new scout call. A proposal, saved file, reported success or skill load alone is insufficient. Record model cost and narrow quality limits separately.
5. **Scope and reuse:** reuse Anima's established SkillDraft/load/hash machinery, informed by the pinned MIT upstream guards/provenance. A Python process/profile adapter adds another approval/store lifecycle and exceeds the cost of narrow Rust glue. M39 is the procedural skill slice; source-backed preference/profile views, dialogue-only memory curation and broader PAR-02 remain explicit follow-ups.

## Verified implementation and acceptance

The selected M39 wave is complete. Current local gates: Rust 2,494 / nine ignored / 40 suites and lint; SDK 130 / three intentional service skips; web 1,231 / 111 files; SDK/web typecheck/build. Two fresh tasks selected the owner-adapted approved skill and used load/write/read with exact independently checked outputs, across one quiet same-store restart. Eight model steps reported 12,297 tokens. Original configurations/grants and 74 earlier memories were preserved; all 14 owned test process identities are gone. An initial missing-header 400 admitted no model work before its single operator correction. Source-backed profile/preference work remains the next PAR-02 wave. Exact hashes, captures, publication and resume are in the private M39 ledger.
