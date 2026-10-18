# Task reviews and learning advice

M38 adds a bounded advice stage. It does not automatically save facts, edit skills or change instructions. M39 adds owner-reviewed procedural adoption and later reuse through the existing skill approval flow.

After an eligible completed task, use **Review this task** next to its accepted reply. Open the companion's settings to inspect **Task reviews**. You can refresh status, read the evidence, cancel a pending review, acknowledge advice or dismiss it. Generated advice is unverified. Acknowledgement records your decision; it does not create a Fact, approve a skill or execute its suggestion.

**Automatically review completed tasks** is off by default. Enabling it changes only the separate review policy. Existing model configuration, tool permissions, memory and skills keep their current authority. Disabling it stops new automatic dispatch and cancels queued automatic reviews. Manual requests remain explicit actions.

Reviews require a completed ordinary task with an accepted reply and settled persistent native action receipts. Missing or foreign evidence, helpers, peer tasks, interrupted or failed tasks and silent check-ins are ineligible. A completed reply and a successful tool receipt do not prove that the whole goal succeeded.

Each review freezes at most 12 KiB of evidence from that one task: bounded request/reply excerpts, source references and up to eight action summaries, with omitted or shortened evidence marked. Raw tool arguments/results, unrelated conversation history, identities and canonical memory are not passed into the review. One no-tools generation uses the companion's configured provider and model, with no retry or ordinary task/memory evaluator fork. Advice must cite supplied evidence anchors; malformed, oversized, truncated, hidden-text or tool-call output is rejected.

Manual and automatic reviews share **two durable reservations per workspace per UTC day**, one physical reviewer slot, ten pending records and a 20-second returned-result deadline. Failed, cancelled or interrupted attempts still consume their reservation. An identical request returns the same attempt; changed source evidence conflicts. The store retains up to 1,000 attempts and refuses additional admissions rather than forgetting reservation or deduplication keys. A future retention policy must preserve those controls.

A timeout or cancellation can still incur provider charges. A review records available usage or explicitly unknown/incomplete usage; the deadline is not a hard billing cap. Cancelled or invalidated results cannot publish advice. After restart, queued/running attempts become interrupted; they are never automatically replayed. Terminal reviews and owner decisions remain inspectable in durable storage. Ephemeral installations refuse admission before a model call.

The owner policy response reports whether the physical reviewer is still active. A quiet restart waits for that request to exit, even if its visible result has already timed out. Missing provider counts stay unknown rather than displaying a fabricated zero cost.

Owner routes are documented in the daemon OpenAPI contract: learning-policy GET/PATCH, completed-run learning-review POST, review list/detail GET, cancellation POST and versioned proposal decision PATCH. The SDK exposes these through `client.learning`. Requests may choose only the source run or owner decision; evidence, provider, policy version and limits stay server-owned.

The implementation uses a narrow attributed port of the upstream request-exit/cancellation pattern, integrated with Anima's existing streaming model adapter and durable owner boundaries. Full Python agent forks with automatic memory/skill writes would violate this stage's contract. See the host learning notice for pinned source and MIT attribution.

Verification and real acceptance are recorded in [harness acceptance](harness-acceptance.md). Advice alone does not establish a lifelong learning loop, general insight quality, real-account readiness.

## Reviewed procedural adoption

A completed suggestion can create a pending skill draft through **Create skill draft**. Read the source, edit the exact name, description and Markdown body, then choose a new slug or a current approved skill as the base. The handoff pins the displayed advice version, evidence digest and expected existing file hash. It performs no model call and installs no file. Continue in **Skills** to inspect, edit, approve or reject the ordinary draft.

The daemon retains the lesson-to-draft pointer and immutable source lineage. Exact repeated handoffs return the same pending or decided draft; different original content conflicts. A pruned draft pointer cannot create another draft. Acknowledgement does not install or invalidate a draft; dismissal blocks an unapproved linked installation. Dismissal after approval does not remove an installed skill.

Both handoff and approval recheck the own completed source and accepted reply. Linked replacements also require the original approved record and actual file hash to match. Changed or deleted sources, changed skill bases and missing lineage refuse installation. Review the changed evidence independently; the old handoff is not silently reopened. The existing approval route writes the exact reviewed final body and pins its composed SKILL.md bytes. Owner edits remain attributed to that owner review, rather than being presented as the model's original advice. External file edits remain subject to existing changed-file/hash checks; this is not an atomic compare-and-swap against arbitrary external editors.

Control-plane version15 backs up version14 as `.pre-learning-adoption.bak`, separately from M38's `.pre-learning.bak`. Older daemons refuse the newer version. Source lineage, draft decisions and approved hashes persist on reload; restart schedules no learning replay. Reported save failures may have uncertain durable outcomes: inspect the saved records before retrying.

The additive owner route is POST `/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}/skill-draft`, exposed as `client.learning.createSkillDraft`. Required JSON fields are `expectedVersion`, `evidenceDigest` and `skill: {slug,name,description,body,expectedHash}`; `expectedHash` must explicitly be null for a new skill or the reviewed lowercase SHA-256 for an existing one. First creation returns201; an exact existing handoff returns200. Sources, draft IDs, approval and grants remain server-controlled. The standard skill loader rereads approved bytes before use, and instructions/reference files gain no new execution authority.

M39 covers the procedural adoption wave, including two fresh independently checked tasks using the approved skill. Saved advice, an installed file or a load receipt alone is insufficient. Optional dialogue-only or unattended Fact curation remains separate.

## Selected profile facts and agent notes

In **Memory → About you → Profile & agent notes**, select up to six current saved private Facts for this companion. Split them between **Your profile** and **Notes for this agent**. Search displays at most50 candidates; narrow the search when more match. These are owner-selected source records, not independently verified truths or new tool permissions. Temporal facts remain separate.

The daemon saves ordered IDs and a selection version. Current canonical memory supplies the displayed text and source hash; no second profile text store exists. Every new run resolves the eligible records again, puts selected sources ahead of ordinary relevance and deduplicates them. All sources share the existing six-record/4800-character quoted context budget, with800-character excerpts. Long sources may be shortened or omitted by that budget. A running task keeps its initial context; the next task sees a correction.

Save selection changes before correcting a selected saved fact. Correction pins the current selection version, full exact source text and all canonical metadata through its hash. It reuses the ordinary canonical memory save and search-index path. A changed source or selection returns a conflict: refresh, inspect the current source and explicitly save again. No automatic retry occurs. Selection and source correction are separate commits. Other memory editors retain their existing behavior.

Missing, deleted, foreign, hidden or ineligible selected sources are unavailable; inaccessible text is never returned. Remove unavailable references explicitly before saving a replacement selection. Missing durable memory disables context selections while owner views retain their version and unavailable references. A failed save can have an uncertain durable outcome; inspect current records before trying again.

Owner contracts are GET/PATCH `/api/agents/{agent_id}/memory-views`, GET its `/candidates?query=...` and PATCH its `/sources/{memory_id}`. SDK methods live on `client.memories`. Selection requires `expectedVersion`, ordered `userProfile` and `agentNotes` arrays of `{id,sourceHash}`. Correction requires `expectedVersion`, `expectedContent`, `expectedSourceHash` and `content`. All routes require owner access and use no-store responses. Version16 backs up version15 independently as `.pre-memory-views.bak`; older daemons refuse this format.

This M40 slice uses exact-entry guards and Anima's existing canonical store for profile facts and agent notes. Standard Markdown synchronization, automatic profile inference and unattended curation are outside this wave. Exact local gates and the two fresh corrected-preference/restart trials are recorded in [harness acceptance](harness-acceptance.md) and the [M40 plan](../superpowers/plans/2026-10-13-companion-profile-memory-m40.md).
