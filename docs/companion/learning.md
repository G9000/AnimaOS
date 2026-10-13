# Task reviews and learning advice

M38 adds a bounded advice stage. It does not automatically save facts, edit skills or change instructions. Reviewed adoption and later reuse remain the next [PAR-02 milestone](parity-roadmap.md#par-02-close-the-learning-loop-through-reviewed-adoption).

After an eligible completed task, use **Review this task** next to its accepted reply. Open the companion's settings to inspect **Task reviews**. You can refresh status, read the evidence, cancel a pending review, acknowledge advice or dismiss it. Generated advice is unverified. Acknowledgement records your decision; it does not create a Fact, approve a skill or execute its suggestion.

**Automatically review completed tasks** is off by default. Enabling it changes only the separate review policy. Existing model configuration, tool permissions, memory and skills keep their current authority. Disabling it stops new automatic dispatch and cancels queued automatic reviews. Manual requests remain explicit actions.

Reviews require a completed ordinary task with an accepted reply and settled persistent native action receipts. Missing or foreign evidence, helpers, peer tasks, interrupted or failed tasks and silent check-ins are ineligible. A completed reply and a successful tool receipt do not prove that the whole goal succeeded.

Each review freezes at most 12 KiB of evidence from that one task: bounded request/reply excerpts, source references and up to eight action summaries, with omitted or shortened evidence marked. Raw tool arguments/results, unrelated conversation history, identities and canonical memory are not passed into the review. One no-tools generation uses the companion's configured provider and model, with no retry or ordinary task/memory evaluator fork. Advice must cite supplied evidence anchors; malformed, oversized, truncated, hidden-text or tool-call output is rejected.

Manual and automatic reviews share **two durable reservations per workspace per UTC day**, one physical reviewer slot, ten pending records and a 20-second returned-result deadline. Failed, cancelled or interrupted attempts still consume their reservation. An identical request returns the same attempt; changed source evidence conflicts. The store retains up to 1,000 attempts and refuses additional admissions rather than forgetting reservation or deduplication keys. A future retention policy must preserve those controls.

A timeout or cancellation can still incur provider charges. A review records available usage or explicitly unknown/incomplete usage; the deadline is not a hard billing cap. Cancelled or invalidated results cannot publish advice. After restart, queued/running attempts become interrupted; they are never automatically replayed. Terminal reviews and owner decisions remain inspectable in durable storage. Ephemeral installations refuse admission before a model call.

The owner policy response reports whether the physical reviewer is still active. A quiet restart waits for that request to exit, even if its visible result has already timed out. Missing provider counts stay unknown rather than displaying a fabricated zero cost.

Owner routes are documented in the daemon OpenAPI contract: learning-policy GET/PATCH, completed-run learning-review POST, review list/detail GET, cancellation POST and versioned proposal decision PATCH. The SDK exposes these through `client.learning`. Requests may choose only the source run or owner decision; evidence, provider, policy version and limits stay server-owned.

The implementation follows the [reuse-first policy](parity-roadmap.md#reuse-first-implementation-policy): a narrow attributed port of the upstream request-exit/cancellation pattern, integrated with Anima's existing streaming model adapter and durable owner boundaries. Full Python agent forks with automatic memory/skill writes would violate this stage's contract. See the host learning notice for pinned source and MIT attribution.

Verification and real acceptance are recorded in [harness acceptance](harness-acceptance.md). Advice alone does not establish a lifelong learning loop, general insight quality, real-account readiness or full Hermes parity.
