# Bounded post-task learning M38 Implementation Plan

> **For agentic workers:** Follow this lean plan with root-owned RED/gates, disjoint source ownership and the binding rulings below.

**Status:** implementation and one bounded real review/restart acceptance complete from merged M37 main `6626428`. Meaningful native owner-route RED (404 instead of 202) and two UI missing-control REDs preceded production. Exact tested source, publication/merge and resumption are in the private M38 ledger.
**Goal:** review one settled task into inspectable, durable advice without silently changing memory, skills or grants.

- This is the scout/advice stage only. The later explicit skill/memory handoff plus verified fresh reuse completes a bounded learning loop; M38 alone is not full self-learning.
  Baseline: M36 main `7d3eb52`; current ReflectionMemoryEvaluator and existing skill drafts stay intact.

## Reuse decision/provenance

Follow the canonical [reuse-first policy](../../companion/parity-roadmap.md#reuse-first-implementation-policy) in every worker brief and combined review. Inspected upstream: `NousResearch/hermes-agent`, pinned `1fd18277267152893a8a24792b79213e7ca3f899`, `agent/background_review.py`, MIT. Reuse its bounded admission/cancellation/request-completion design through a narrow attributed Rust port. Direct Python runtime integration would require a second runtime and its agent fork can write memory/skills; both conflict with M38's one-generation advice-only boundary. Existing Rust model streaming, owner routes and durable snapshot components provide the lower-cost integration. Preserve the host notice and record actual local adaptations; UI uses existing Anima components rather than importing an incompatible desktop stack.

## Proposed small contract

Source refresh locks the candidate routes below. Add separate owner GET/PATCH `/api/agents/{id}/learning-policy`, PATCH `{autoEnabled:boolean}`, response `{autoEnabled,dailyLimit:2,pendingLimit:10,timeoutMs:20000}`. No AgentConfig or tool-grant changes. Admission POST `{}` returns 202 `{review}` for new and identical deduped attempts; list `{reviews}`, get/cancel/decision `{review}`. Decision PATCH `{decision:"dismissed"|"acknowledged",expectedVersion:number}`; stale version returns 409, unknown fields 400, foreign/missing source 404, quota/evidence conflict 409, unavailable persistence/record capacity 503.

The policy response also exposes `physicalReviewerActive` (optional for older SDK peers). A terminal result timeout can precede actual transport exit; isolated restart/cleanup checks this diagnostic instead of treating a failed state as physical quiescence. Missing provider counts remain unknown; prior partial usage is not replaced by normalized default zero.

Frozen review fields (camelCase): `id,agentId,sourceRunId,sessionId,trigger,policyVersion,evidenceDigest,evidenceBrief,evidenceAnchors,state,reservedAtMs,startedAtMs,finishedAtMs,model,provider,usage,usageComplete,error,proposals`. Optional values are nullable. Each proposal has `id,observation,suggestion,anchors,decision,decisionVersion` (initial 0). Model JSON is strictly `{proposals:[{observation,suggestion,anchors}]}`. HTTP/control-plane DTOs are host-owned; additive SDK types mirror them.

`LearningRegistry` persists snapshot lists `learningReviews` and `learningPolicies` with version 14 and independent `.pre-learning.bak` for version 13. The registry retains reservation timestamps and attempt keys; after 1,000 records it refuses new admissions instead of forgetting keys. Physical transport permits are shared in `DaemonState` across coordinators. Existing direct `ModelAdapter::stream` supplies a bounded no-tools sink; its default fallback can ignore sink errors, so the service independently checks retained rejection and usage state. No core package change or new dependency is needed.

- Owner API: request a review of an existing own run, list/get review status, dismiss/acknowledge a proposal, cancel pending/running review.
- Candidate routes: POST `/api/agents/{id}/runs/{runId}/learning-review`, GET `/api/agents/{id}/learning-reviews`, GET `/api/agents/{id}/learning-reviews/{reviewId}`, POST `.../{reviewId}/cancel` and PATCH `.../{reviewId}/proposals/{proposalId}` for a versioned owner decision.
- `LearningReviewRecord`: review/agent/source-run IDs, server policy version, evidence SHA, frozen brief, state/timestamps, configured model/provider, reported-or-unknown usage, safe error and bounded proposal IDs.
- `LearningProposal`: ID, observation/suggestion text, supplied source anchors, pending/dismissed/acknowledged decision and decision version; it is not a Memory or executable Skill.
- Client inputs identify the source and owner decision only; model/provider, quota, grants, policy version and evidence selection are server-owned.
- One explicit request authorizes one review. Optional automatic review requires a visible per-agent opt-in, off by default.
- Eligibility: settled Completed own ordinary run with tool activity and available persisted evidence; exclude review/helper recursion, silent check-ins and stopped/failed/interrupted source runs.
- The saved reply proves response completion, not goal success; record actual receipt outcomes and uncertainty in the brief.
- Admission requires durable control-plane storage and a server-owned budget; ephemeral mode returns unavailable without a model call.
- Proposed states: queued, running, completed, cancelled, interrupted, failed; proposal decision pending/dismissed/acknowledged.
- Admission key: agent ID + source run ID + review-policy version. Evidence digest is recorded; a changed digest conflicts rather than creating another automatic review.
- Persist the reserved attempt and frozen bounded brief before dispatch; simultaneous/manual/automatic requests return the same admitted record.
- Source/run deletion before dispatch cancels; after dispatch, result publication rechecks source identity/ownership and fails closed if authority disappeared.
- On startup, pending/in-flight records become interrupted rather than issuing another model call. A future explicit retry policy must consume a fresh server reservation; model output and clients cannot manufacture a new policy version to bypass dedup. M38 has no automatic retry.

## Input / output and authority

- Brief <=12KiB UTF-8 total: source run/session/step references, request excerpt <=1.5KiB, accepted reply <=3KiB, up to8 settled receipt summaries <=0.5KiB each.
- Server chooses evidence; no caller-selected agent, arbitrary session, filesystem, attachment, connector or raw provider response.
- Include exact evidence digests, shortening/missing markers and actual success/failure/unknown status. Do not equate receipt success with whole-task success.
- Freeze only this brief; do not clone the full transcript, system prompt, active identity, tool arguments with secrets, or unrelated memory.
- Model output: at most3 advice items, <=2KiB/item and <=6KiB total; each cites supplied evidence anchors and distinguishes observation from suggestion.
- Unknown keys, invalid structure, missing anchors, tool calls, empty/truncated generation or hidden/control text reject publication with a safe diagnostic.
- Use the existing configured model through a no-tools shadow request with max_retries=0; exactly one generation attempt, no evaluator or task-result/memory side effects.
- No `AgentRuntime.run` fork that installs the ordinary ReflectionMemoryEvaluator; reusable request/transport cancellation seams remain core-owned.
- No executable instructions become active from the output. No direct Fact add/update/remove/consolidate, skill file write, profile mutation or automatic grant.
- UI labels generated advice as unverified; acknowledgement is not Fact creation or approval of a skill rewrite.
- PAR-02 later provides explicit owner action to turn a reviewed item into an existing pending SkillDraft; approval/hash pinning remains separate.
- PAR-02 also provides a separate explicit own-memory handoff under current scope/CAS/approval rules. Optional unattended Fact curation requires its own opted policy/grants/budget and semantic acceptance, never M38 automatic writes.

## Locked initial cost and cancellation bounds

- Initial quota: <=2 reviews per workspace/UTC day, <=1 concurrent reviewer, <=10 pending records; reservations persist across restarts.
- Owner-triggered and automatic reviews consume the same quota; clients cannot increase it through request fields.
- One20-second returned-result deadline, max1 model request, zero tools/retries; output cap is enforced while collecting generated text.
- A timeout/drop may not stop provider billing; record available usage or unknown, never claim a hard token/billing ceiling.
- Cancellation wins before dispatch/publication; a late result is discarded. No automatic retry, budget refund that enables duplicate dispatch, or unresolved-provider replay.
- Automatic opt-out blocks new dispatches and cancels queued reviews; original completed task/history/receipts are never altered.
- Unavailable model, depleted quota, storage failure or unknown source evidence results in a visible skipped/failed outcome without weakening the primary task.

## Disjoint future ownership

- Native worker: new `learning/` service and bounded persisted records; coordinator completed-run hook, state persistence/recovery and owner routes/contracts/tests.
- Candidate native paths: `learning/{mod,records,service,tests}.rs`, `routes/learning_reviews.rs`, `routes/contracts/learning_reviews.rs`, existing coordinator/state persistence seams; root freezes the inventory before parallel edits.
- Core worker only if needed: reusable no-tools request/cancellation seam; do not move review records, quota or HTTP/storage into anima-core.
- SDK/web worker: additive review DTO/client, default-off settings action, review list/detail/cancel/decision UI and tests.
- Skills worker: no source change in M38; PAR-02 owns explicit review-to-draft handoff afterwards.
- Root: public plan/five risks, observed RED, combined review/fix wave, serialized gates, real acceptance, source hashes and explicit-path publication.

## Meaningful RED and focused tests

1. Exercise the existing completed-run route/event with opt-in disabled: no review request and no new Fact/skill/identity file; this is the authority control.
2. Enabled eligible task + fake configured model: assert one durable pending review/proposal is exposed; observe the missing behavior fail before implementation.

- RED must use a valid existing run/owner boundary and a test-only review-policy fixture, not a missing production symbol or compiler error; root binds its exact observable before source changes.

3. Same run requested concurrently/manual/auto: one reserved attempt, one model invocation and one proposal set; changed evidence rejects a second attempt.
4. Quota exhaustion/restart/opt-out/owner Stop: no extra dispatch, durable budget retained, pending/running becomes interrupted without replay.
5. Provider error/timeout/late result/storage failure/malformed/tool-call output: primary task unchanged, safe terminal review outcome, no canonical memory/skill write.
6. Evidence pruning/deleted agent/foreign run/prompt-injection text: ownership checked, bounds held, missing anchors rejected, no full history/profile exposed.
7. Accepted advice round-trip: exact proposal/source digest/usage/decision survives reload; current pending-source references are not invented audit guarantees.
8. SDK/UI: optional DTO fields, explicit opt-in, default-off, busy/cancel/error/dismiss controls, failed saves retain draft and no implied grant change.

## Bounded real acceptance after fixture gates

- Root predeclares one synthetic task with a useful procedural mistake/correction and a verifiable final artifact; existing loop/grants remain the primary execution authority.
- First prove the task itself and inspect its real evidence. Submit one explicitly authorized review of that task, not an extra autonomous child run.
- Judge whether at least one anchored advice item describes the actual correction without new factual claims; otherwise record failed quality, no repeat/repair.
- Compare canonical memory, skills/instruction bytes, primary run/messages/receipts and grants before/after: no learning side effects beyond the persisted review/proposals.
- One quiet same-store restart must retain the review and original task without dispatch/replay. Stop exact owned identities and restore only changed test fields.
- Report the additional configured generation count/usage separately from task costs. No ordinary profile, account delivery, remote embedding, credential operation or paid probe.
- This proves one proposal workflow, not lifelong autonomous learning, universal insight quality or full Hermes policy parity.

## Root-owned checks / resumption

- Root observes RED before production, then targeted learning/route/SDK/UI tests after combined source freeze.
- Required native gate: `bun x nx run rust-daemon:test --skipNxCache`; sequential daemon lint and supported formatting checks.
- Root selects exact existing SDK/web Nx test/type/build targets; no new target directory or parallel Cargo process.
- Use mocks/local temporary stores first; existing isolated synthetic chat-model testing is authorized; root binds its exact case and call budget before execution. Ordinary accounts/profiles and external actions remain separately scoped.
- Read the [locked parity roadmap](../../companion/parity-roadmap.md) and these root rulings before implementation; if source moved, revise this detailed plan before RED.
- Exact next implementation resume: this plan, on a new `g9000/companion-learning-m38` branch from verified merged main; ledger `.superpowers/sdd/2026-10-13-companion-learning-m38/progress.md`.

## Architecture and file map

Reusable inference/cancellation primitives remain in `packages/*` only when needed. The Rust host owns review records, source evidence selection, quota/admission and owner routes; SDK/web expose additive controls and advice. Use the existing model adapter, run/history store and skill approval contracts. Dependencies are allowed when justified; none is chosen for this plan.

Candidate new `hosts/rust-daemon/src/learning/` modules separate records, evidence, service/budget and recovery. Existing `agent_runs.rs`/coordinator, state and control-plane persistence get small integration hooks only after current source paths are confirmed. Register owner-authorized/no-store documented routes in the existing router. Keep pending/running recovery additive and source-version migration tested with recorded backups. Root freezes exact route/type contracts before parallel native and SDK/web workers. No runtime change is implemented merely because this plan is published.

Task order: (1) source/contract refresh and meaningful route/runtime + UI RED; (2) native evidence/admission/storage/no-tools service/recovery, plus disjoint core seam only if needed; (3) additive SDK/web default-off review controls after interfaces freeze; (4) combined review, one justified fix wave and current gates; (5) one bounded synthetic review/restart acceptance; (6) docs/explicit-path publication and verified merge. PAR-02 later closes adoption/reuse; M38 advice alone is not the full learning loop.

## Controller rulings on the risks (binding)

1. Ruling: no-tools proposal/advice service, without ordinary runtime/evaluator forks or automatic memory/skill/instruction writes — generated conclusions must not silently become canonical authority — cost if wrong: M38 only scouts; PAR-02 must separately prove approved adoption and fresh reuse.
2. Ruling: one admitted attempt per own source run/policy, persisted before dispatch, with two shared daily reservations and one physically active reviewer — concurrent/manual/automatic paths must share cost control — cost if wrong: timeout/cancel can still bill; no reservation refund or slot release permits another physical request before the first exits.
3. Ruling: freeze a server-selected bounded brief and exact evidence digest, reject unknown anchors/oversized/malformed/tool-call output, and preserve missing/unknown status — one successful tool receipt is not whole-task proof — cost if wrong: advice is visibly failed/unverified rather than invented or silently repaired.
4. Ruling: queued/running reviews become interrupted on restart; cancelled, opted-out or deleted-source results cannot publish, and no automatic replay occurs — review recovery must preserve primary task effects and authority — cost if wrong: owner must inspect the recorded outcome; no duplicate review to obtain a pass.
5. Ruling: ordinary primary tasks and default settings remain unchanged; use meaningful fixture REDs, current relevant Nx gates and one already-authorized bounded synthetic acceptance — user wants cheap continuous progress — cost if wrong: general insight quality/full parity remains open; new accounts, private profiles or external actions require concrete scope, not a blanket renewed approval gate.

## Implementation and acceptance evidence

Current gates: Rust **2,485 passed / nine ignored / 40 suites**, sequential formatting lint, SDK 129 / three intentional service skips, web 1,225 / 111 files with verified two-worker limit, SDK/web typecheck/build. Twenty-four focused native cases include actual owner routes and completed-task hooks, quota/dedup/failed storage, cancellation/timeout physical-slot retention, interrupted recovery, panic successor progress and seven rejected-output variants through the core default stream. Independent native and final SDK/web reviews led to one fix wave. Full gates also caught a normal-build import and the README/OpenAPI inventory; both were corrected, with final gates passing.

The real case reused the already accepted M35 synthetic multi-step consolidation/correction task instead of paying for another primary prompt. One manual review produced three supplied-anchor advice items in 6,878 ms, with 805 prompt / 394 completion/**1,199 total reported tokens**. It correctly distinguished actual tool-call success/order from unverified omitted arguments/results. Advice is repetitive and unverified; a suggestion to log exact source content is not automatically adopted. This case demonstrates supported workflow advice, not a newly induced procedural failure or broad insight quality.

One quiet same-store restart retained the exact terminal review and proposal IDs without another attempt. Original primary run/messages, canonical memory hash, agent snapshots/config/grants, skill records/draft, SOUL.md and approved skill bytes and default-off policy stayed exact. All 14 freshly recorded daemon/runner/wrapper identities are gone; 8080/4200 free. No configuration restoration was needed because none was changed. No live accounts, databases, containers, ordinary profile import or extra generation/probe. Next is PAR-02 reviewed adoption and actual later reuse on a separate branch from merged main; M38 advice alone does not close that learning loop.
