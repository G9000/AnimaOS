# Shared project spaces Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans after coordinator release. Track steps with checkboxes. Do not spawn agents/chats, create worktrees, switch branches, commit, push or deploy in this shared-checkout effort.

**Goal:** Deliver one owner-controlled project shared by multiple existing agents, with selected context, canonical resource links and revocable membership while private agent stores/native authority remain separate.

**Architecture:** Store profile-owned project/member metadata and source refs in the existing snapshot, with one canonical project field on each agent-owned session. Use private pinned-source capabilities and revocable host leases for cross-agent selected projection and provider admission. Keep accepted bytes immutable through compaction/retries while honoring live revocation.

**Tech Stack:** Existing Rust daemon/Serde/Provider/history/safe filesystem capture, Bun SDK, React console, Vitest and static console Playwright. No new dependency, service or content database.

---

## Review checkpoint and constraints

Owned-file implementation released on 2026-10-28. This plan and [the spec](../specs/2026-10-27-project-spaces-design.md) incorporate the human's explicit **Shared by multiple agents** decision and accepted reviewer correction for durable creation receipts. Generic state/schema v33/route registrations/SDK client+index/web daemon-api integration were authored by the shared writer; this project chat owns WorkspaceShell/ViewHarness glue. Continuity owns runs/turn.rs, agent_runs/recovery.rs and SDK runs.ts. Independent source/runtime review is clear as reported by the coordinator. Final current-tree `rust:test` physically exited 0 (daemon: 3,661 passed, 0 failed, 38 ignored), `rust:lint` exited 0 with warnings, and the full default SDK suite exited 0 (51 files, 291 tests passed). The earlier selected native proof subsets remain recorded below. Full web stays red at 2,284 passed/165 failed/1 skipped; unsupported broad journey/race matrices remain unverified. Source is frozen and root final cleanup is underway. The root coordination plan is authoritative for combined status. No further human scope approval is needed.

Use cmd.exe with login:false. Preserve unrelated dirty cache/dev tooling/package scripts/docs/results-panel work. No real account/credential access, service restart or generated inventories. No Rust compile/test/lint without coordinator build slot; one Cargo process at a time, repository wrappers, absolute shared validation target, 32 GiB ceiling. No feature target directories, cache-limit overrides or cleanup of other workers' outputs.

Each task uses meaningful behavioral regression tests before implementation. Full current-tree verification remains required after focused tests. Recheck scripts/fixture conventions because other chats share the checkout.

## File ownership map

Proposed feature-owned new files:
- `hosts/rust-daemon/src/projects/{mod,records,registry,service,access,context,tests,access_tests,context_tests}.rs`
- `hosts/rust-daemon/src/workspace_file_preview.rs`
- `hosts/rust-daemon/src/routes/projects.rs`, `routes/project_tests.rs` (nested from projects.rs)
- `packages/sdk/src/projects.ts`, `projects.spec.ts`
- `apps/web/src/hooks/useProjects.ts`, `useProjects.test.tsx`
- `apps/web/src/components/projects/{ProjectsSidebar,ProjectHome,ProjectEditor,ProjectSources,ProjectOutputs,ProjectWorkflows,ProjectMembers,MoveChatDialog,ProjectContextDetails}.tsx`, adjacent tests, `projects.css`
- `apps/web-e2e/src/console/projects.spec.ts`

Exact shared paths and responsibilities are enumerated in the spec. Coordinator assigns ordered ownership for state/snapshot/session admission, runtime/compaction, run/recovery DTOs, events, routes, SDK exports and shell UI. Continuity owner wires supplied project hooks. Document owner agrees safe preview extraction before either feature modifies it. No dirty ChatResultsPanel.tsx/InteractiveResults.test.tsx edits are needed.

## Interface checkpoint

Public API: spec's profile-owned /api/projects table. There is no single project owner agent; native targets include ownerAgentId. Normal reads need current member selector; metadata management remains independent owner authority without source hydration. The spec's **Concrete host integration contract (2026-10-28)** is authoritative for every persisted field/type, registry, opaque lease, restore hook and event. It includes profile-level ProjectCreationReceipt snapshots. This summary uses the same signatures.

~~~rust
// Narrow host integration contracts; not production implementation.
pub(crate) fn capture_run_witness(
    state: &DaemonState, agent: &str, session: &str,
) -> Result<QueuedProjectWitness, ProjectError>;
pub(crate) fn validate_queued_witness(
    state: &DaemonState, run: &RunRecord,
) -> Result<(), ProjectError>;
pub(crate) fn capture_context_scope(
    state: &DaemonState, run: &RunRecord,
) -> Result<Option<CapturedProjectScope>, ProjectError>;
pub(crate) async fn load_context(
    state: &SharedDaemonState, scope: &CapturedProjectScope,
    budget: ProjectRenderBudget, run_cancel: CancelSignal,
    deadline: std::time::Instant,
) -> Result<LoadedProjectContext, ProjectError>;
pub(crate) fn recheck_context(
    state: &DaemonState, run: &RunRecord,
    scope: &CapturedProjectScope, loaded: LoadedProjectContext,
) -> Result<Arc<AdmittedProjectContext>, ProjectError>;
pub(crate) fn validate_active_lease(
    state: &DaemonState, run: &RunRecord,
) -> Result<(), ProjectError>;
pub(crate) fn context_change_summary(
    state: &DaemonState, run: &RunRecord,
) -> ProjectContextChange;
~~~

QueuedProjectWitness records unassigned session-binding revision as well as optional project ID/revision/epoch/member incarnation. AdmittedProjectContext holds immutable text/estimate/content-free manifest **and live lease**. RunBuild/RunCompaction carry Option<Arc<AdmittedProjectContext>> and never reread project state.

PinnedSourceCapability is private and host-constructed after requester/donor/resource validation. It binds exact donor/session/generation/message IDs and cannot become a general cross-agent HistoryStore client. The access registry publishes member epochs and cancellation. Reuse NativeActionJournal::check_owner(starting=true), existing checkpoint/document/execution/RPC revalidation and runs/turn::active; outcome settlement stays allowed after cancellation. The project ModelAdapter is below attachment/fallback wrappers and uses existing ModelCallContext cancellation. No new core authority type or duplicate tools.rs permission mechanism is needed.

## Task 1: Profile records, members and canonical links

**Files:** create projects/{mod,records,registry,tests}.rs; assigned main.rs, state.rs, control_plane_store.rs, sessions/mod.rs edits.

- [ ] Test one shared A+B project, no synthetic agent, member projection, and native chat ownership. C cannot become a member merely by knowing IDs.
- [x] Verify removal/re-add assigns a fresh member incarnation while retaining project identity and the remaining member's generation (`projects::tests::membership_readd_has_a_new_incarnation_and_keeps_one_project_identity`).
- [ ] Complete the remaining member matrix: 32-member cap and duplicate/helper/missing/cross-profile rejection; the selected incarnation test does not establish these cases.
- [ ] Test same-ID A/B session/result/workflow cannot cross-match; private donor chat cannot be pinned until moved into project; main/helper/connector/check-in/job restrictions.
- [ ] After a build slot, confirm intended failures:
  `bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test --manifest-path hosts/rust-daemon/Cargo.toml -p anima-daemon projects::tests`
- [ ] Implement typed bounded records, checked revision/accessEpoch, new membership generations, exact full target identity and candidate CAS. No copied content/memory/workflow state.
- [ ] Add defaulted snapshot projects and session project ID/binding revision via shared owner. Derive counts from canonical sessions.
- [ ] Shared integration writer owns the single version33 increment/exact backup/default migration. Supply ProjectRecord/ProjectCreationReceipt snapshots and restore hooks; test old defaults, JSON round-trip, PostgreSQL snapshot parity, unsafe/duplicate rejection and unavailable refs.
- [x] Verify exact create replay, mismatched replay refusal, delete -> serialized restart -> retry denial, and retained receipt without recreated membership (`creation_replay_survives_delete_and_restart_without_recreating_membership`).
- [x] Verify full receipt history rejects creation before candidate mutation without eviction, and isolated candidate creation leaves the original registry/receipts unchanged (`creation_history_capacity_rejects_before_any_candidate_mutation`, `candidate_mutation_does_not_publish_into_the_original_registry`; both in `projects::tests`).
- [x] Record the native driver's selected green results from the two logs linked below; this bookkeeping step does not claim the full suite or initial red-test phase.

**Acceptance:** One durable project supports several existing agents, with fully qualified native refs and safe legacy defaults.

## Task 2: Durable membership mutation and live fence

**Files:** projects/{access,service,access_tests}.rs; assigned state/agent deletion/run-control hooks.

- [ ] Barrier tests: remove member during file/history hydration, serialization, fallback/retry and shared-result publication. Old leases cannot publish afterward or revive on re-add.
- [x] Verify the hydration lease gate revokes both removed and remaining members' old leases, signals cancellation, and does not revive the old lease on re-add (`projects::access::tests::member_removal_fences_hydration_and_readd_cannot_revive_a_lease`). This is lease-level evidence, not a paused physical file/history hydration race.
- [x] Verify a revoked lease prevents a later guarded attempt from dispatching and retains an already-entered attempt until actual settlement (`revoked_lease_blocks_a_later_model_attempt_before_transport_dispatch`, `revocation_retains_inflight_transport_custody_until_real_settlement`; both in `projects::access::tests`).
- [x] Verify known failed create saves publish neither project nor receipt, and known failed membership saves retain old grants, chat binding, unstopped run and valid lease (`projects::service::tests::failed_create_save_publishes_neither_project_nor_creation_receipt`, `known_failed_membership_save_keeps_old_grants_binding_and_run_authority`).
- [x] Verify uncertain publication closes existing/new access and an unrelated registry cannot grant access (`projects::access::tests::uncertain_publication_stays_closed_and_wrong_registry_cannot_supply_access`).
- [ ] Complete uncertain-save readback/reconciliation and acknowledged-revocation rollback/usage/receipt matrices; the selected lease and known-save tests do not establish all of them.
- [ ] Implement ProjectAccessRegistry/private leases bound to profile/project/requester incarnation/epoch/donor incarnations, with cancel signals and short publication critical sections.
- [ ] Implement private candidate member mutation: validate owner/revision/set, increment revision/epoch, detach removed agents' project chats/bump bindings, retain revoked refs internally, persist, then publish state+fence.
- [ ] Conservative v1: every member-set change cancels all old project leases/runs/hydration. Acknowledge after durable fence and cancellation requests; never claim physical stop or undo.
- [ ] Agent deletion invokes the same membership fence without deleting remaining project or reparenting data. Source unshare/removal also fences immediately; normal instruction additions/edits affect later admission.
- [x] Verify the agent-removal candidate hook preserves the acknowledged project/receipt/remaining member generation and does not restore deleted sessions; known failure rolls back stops, uncertainty retains stops and fences access, and final-member removal keeps a zero-member project/receipt (all four named `projects::agent_removal::tests` cases in the project-selected log). Full lifecycle/source-deletion interleavings remain unverified.
- [x] Record all four `projects::access::tests` passing in the native driver's selected log; independent source review is separately clear.

**Acceptance:** Membership revocation works during in-flight hydration/dispatch, including uncertainty and re-add races, without transferring native authority.

## Task 3: Owner/member API and chat assignment

**Files:** routes/projects.rs, routes/tests/projects.rs; assigned routes/{mod,capabilities,sessions}.rs, routes/contracts/sessions.rs, sessions/views.rs, state/session_state.rs.

- [ ] HTTP tests: C cannot list/read/preview/inject/link/move; unauthenticated/viewer/wrong-profile/removed requester/removed donor/forged selector all deny before hydration.
- [x] Verify the limited owner/member route case rejects an untrusted-origin create and C's project read, omits instructions from member-list metadata, replays one creation, and refuses recreation after deletion (`routes::projects::tests::shared_project_routes_enforce_owner_and_members_and_keep_deleted_creation_receipts`). This does not complete the authorization matrix above.
- [ ] Assert project chat grouping contains no donor preview/transcript/tool/approval/memory fields. Own-agent session filter stays own-agent.
- [ ] Test CAS, create/add retry fingerprint mismatch, strict body/field bounds, save rollback and rate-limited atomic project chat creation.
- [ ] Implement all spec routes: profile list/manage/read/create/edit/member set, resource CRUD/order/narrow preview/context/chat grouping, idle-only archive/delete, native move/detach/new chat.
- [ ] Reuse current session creation authorization/helper/limiter/transaction path. Project tuple must be validated atomically; no metadata inference.
- [ ] Validate current requester and donor memberships, target ownership, donor chat same-project binding/generation on add and each read; repeat before publication. Management cannot hydrate source content.
- [ ] Move/archive/delete idle checks occur inside transaction, detach without native deletion, keep linked workflows independent. Member removal uses Task 2's immediate fence.
- [ ] Integrate capability/OpenAPI through assigned owner. Run focused route tests and inspect redaction.

**Acceptance:** All reads/mutations have explicit owner/member/source authorization and deterministic error/revision contracts.

## Task 4: Safe exact shared projections

**Files:** workspace_file_preview.rs, projects source capability/context tests; assigned routes/workspace_files.rs extraction.

- [ ] Regression tests preserve current text/generated-document preview and credential/hidden/link/reparse/traversal/binary denials.
- [ ] Extract safe preview capture returning validated text/hash/coverage without broadening supported content. Keep existing route behavior.
- [ ] Enforce 64 KiB/text file, 256 KiB aggregate text reads, <=2 verified generated documents at current byte limit, common deadline/cancel. No arbitrary uploads/Office/PDF/remote extraction.
- [ ] Create private pinned-source capability only after requester/donor member/incarnation/resource/project/session/generation checks.
- [ ] Read exact donor canonical history anchors with zero neighbors; select only pinned visible user/assistant text. Ordinary history tools keep caller-agent scope.
- [x] Verify B's accepted context contains only the selected A message, excludes unpinned/neighboring text, and old donor pins remain access-removed after removal/re-add (`projects::context::tests::only_pinned_exact_donor_messages_are_read_and_old_grants_do_not_revive`).
- [x] Verify an exact donor preview excludes unselected transcript text and member removal detaches the donor chat/redacts its pin while retaining native messages (`routes::projects::tests::donor_pin_is_exact_and_member_removal_detaches_without_deleting_native_data`).
- [ ] Render selected native result payload by full donor/session/generation/result identity/revision. Workflow/template projection gives title/status/agent/navigation only, no saved prompts/effect authority.
- [ ] Test private/neighbor/system/tool/attachment/memory sentinels stay absent; wrong epoch/member/donor/same-ID collisions deny. Barrier revoke during read and final serialization.
- [ ] Run focused source/preview tests under slot; verify capture writes no source files.

**Acceptance:** Cross-agent reads expose only exact owner-selected projections and never grant general donor history/memory/account access.

## Task 5: Bounded context and live provider/native dispatch

**Files:** projects/context.rs/context_tests.rs and adapter/access leaves; owned agent_files/runtime.rs, agent_runs.rs, agent_runs/compact.rs, sessions/context.rs. Shared writer integrates state.rs session_model_adapter, state/run_commit.rs and action_receipts.rs generic starting/checkpoint authority hook. Do not edit held files or add another tools.rs permission path.

- [ ] Fake-provider test: B gets shared instructions/pinned A project excerpt/output and B identity/memory, never A private transcript/memory/config/tools. Nonmember C/unrelated chats get no project provider.
- [ ] Test caps/UTF-8/order/whole instructions/omissions/dedup/once-only estimate and history reservation.
- [x] Verify only selected native-history IDs are deduplicated, the accepted text/manifest stay frozen when history selection changes, and resource removal revokes its lease (`projects::context::tests::own_chat_deduplication_uses_only_selected_history_and_stays_frozen_for_compaction`). This does not establish the complete cap/accounting or runtime-compaction matrix.
- [ ] Barrier tests: ordinary edit retries once then refuses; revocation hard-refuses without fresh-grant retry; failed manifest save invokes no provider/tool.
- [ ] Integrate combined file/project admission with existing five-second deadline/one retry, no I/O under state transaction, final requester/donor epoch/project/session/generation/result witness checks.
- [ ] Render immutable owner guidance separately from quoted sources. Project cap min(3000,floor(remaining usable tokens/4)), <=12,000 chars total, <=2,000/source; instructions whole or refuse.
- [ ] Reserve candidate allocation before history selection, remove duplicate exact current-chat IDs afterward without refill loops, record actual rendered estimate/coverage once.
- [ ] Suppress automatic incoming-agent provider in project turns only. Keep requesting agent identity/memory/private recall/tools/root unchanged.
- [ ] Persist content-free manifest/witness before model work; carry identical Arc+live lease through compaction/fallback/retry/steer.
- [ ] Validate live lease before provider attempts/tool boundary, after approval/before handler entry, and before accepting stream/output/shared publication. Preserve physical handler custody/action-journal settlement once entered. Instruction edit freezes bytes, membership/source revocation cancels lease; retain legitimate usage/receipts.
- [ ] Add safe context report/estimate projection; never expose raw provider text/config/grant hashes.
- [ ] Run focused admission/provider/compaction tests under slot.

**Acceptance:** Shared context is explicit/bounded/immutable per turn while access remains revocable and effect authority unchanged.

## Task 6: Queue/restart/continuity integration

**Files:** project hooks; continuity-owned runs/{ledger,turn}.rs, agent_runs/recovery.rs, routes/contracts/runs.rs and tests.

- [ ] Agree hook signatures/shared ownership. Persist session-binding witness for unassigned queued requests too.
- [ ] Test queue then instructions change, membership removal/re-add, donor removal, archive/delete and unchanged binding. Stale scope performs no model/tools and retains saved input.
- [ ] Wire queued/current/recovery checks to validate_queued_witness; active checks use live lease. Project witness never makes an unsafe native recovery eligible.
- [ ] Restart rebuilds current publication registry and no old leases. Every resumed attempt acquires fresh access and validates durable witness.
- [ ] Explicit reviewed changed-context continuation uses continuity's receipt/budget/approval rules; until supported, block with saved-input guidance. No automatic context substitution or donor approval reuse.
- [x] Verify project review accepts only revision changes within the same grant; missing queued witnesses and access-epoch/member/project/session-binding changes are unavailable (`projects::context::tests::recovery_review_is_limited_to_revision_only_changes_in_the_same_grant`, `missing_queued_witness_on_a_project_chat_is_unavailable`, and `agent_runs::recovery::tests::project_review_binds_current_witness_and_refuses_access_changes`).
- [x] Verify recovery rechecks the latest reviewed revision/fingerprint, replays only an already-saved child after access changes, and refuses a new continuation after member removal/re-add (`agent_runs::recovery::tests::project_recovery_rechecks_review_and_replays_only_saved_child`). This does not prove every queue/restart/budget/approval interleaving above.
- [ ] Verify cancellation/shared-result behavior preserves usage/action receipts. Run focused queue/recovery tests.

**Acceptance:** Restart cannot revive stale grants or silently alter queued/recovered context.

## Task 7: Metadata events, SDK and stale UI fencing

**Files:** SDK projects/hook files; assigned SDK {client,index,sessions,runs,capabilities,events}.ts, daemon live/events.rs/state/live_state.rs, web lib/daemon-api.ts/hooks/useAgentEvents.ts/lib/session-events.ts/hooks/useCompanionSessions.ts.

- [ ] Test project.updated union old/new member fanout contains metadata only. No donor message/run events/payloads; revoked notification reveals no new private data.
- [ ] Implement typed encoded IDs/member selectors/preview/member replacement/CAS/epoch/abort operations; additive Session/Run/capability contracts.
- [ ] Hook tests: scope profile/project/acting-agent/member-generation/epoch/local-generation; abort/clear on revoke/switch/resync/disconnect; reject delayed promises from old scope.
- [ ] Implement event-first invalidation, hidden cached shared payload while disconnected, focus/reconnect revalidation, visible-only 10-second recovery poll.
- [ ] Test management metadata is no preview bypass, selectors are not auth and older daemon has clear unavailable state.
- [x] Run focused SDK project/session tests and SDK build: 28 tests passed and build passed in the earlier verification window.
- [x] Record the full default SDK test suite: 51 files and 291 tests passed, physical exit 0. The three real-daemon integration tests and one restart/personality persistence test actually ran; this is not a fixture-only or skipped-integration result.
- [ ] Record SDK typecheck/lint and any remaining native-event matrix evidence separately; the final supplied SDK log establishes tests, not those other commands.

**Acceptance:** Stale replies cannot restore revoked content after observed epoch invalidation; server independently fences publication.

## Task 8: Usable shared project UI

**Files:** project components/tests/CSS; assigned ViewHarness.tsx, WorkspaceShell.tsx, SessionSidebar.tsx, SessionView.tsx, SessionContinuity.tsx, lib/hash-route.ts and tests.

- [ ] Test create A+B, stable project identity across Working as selector, C nonmember denial/Add member owner flow, removal explanation and zero-member management repair.
- [ ] Implement stable #/p/projectId route, member sidebar and Chats/Sources/Outputs/Workflows/Members tabs. Project New chat explicitly chooses member; ordinary New chat unassigned.
- [ ] Grouped chat rows show original agent/title without transcript preview. Opening original is a deliberate owner-native action.
- [ ] Pin files/exact excerpt/results; explicit Use in chat context; only linked output refs, read-only shared preview, native editor navigation. Do not merge all outputs or edit dirty results-panel files.
- [ ] Workflow links open original executing-agent controls and saved-step semantics; project card has no delegated approval/run/resume authority.
- [ ] Member changes/source unsharing explain stopped project replies and retained completed actions. Archive/delete idle constraints and independently running workflows are clear.
- [ ] Retain CAS editor drafts; redact revoked donor resources; no fallback by title/ID. Use existing keyboard/focus/responsive patterns. Viewer mounts no project data.
- [x] Run project UI/routing/lifetime regressions and inspect synthetic desktop/narrow layouts: 19 project checks passed in the final full web suite; all 4 static browser cases passed at desktop/mobile sizes with document and project-container overflow assertions.

**Acceptance:** Owner can use requested shared groupings/context end-to-end without creating agents or crossing private stores.

## Implementation handoff and observed validation (2026-10-28)

The owned source is implemented in `projects/{records,registry,access,context,restore,service,source_changes}.rs`, with registry/access/context tests and nested public-route tests in `routes/project_tests.rs`. SDK project operations, native session project selection/filtering/moves, project metadata events, context inspector estimates/coverage, project pages/source picker/member management, and chat move controls are authored. The source picker supports exact messages, native results, workspace files, workflow links and template links; only explicit selected context is injected.

`agent_runs/queue.rs` now captures the witness under the existing acceptance transaction before portable turn binding and the first save, including reviewed recovery children. Active steers check the existing run's lease. Leftover steers retain their parent's witness and become interrupted when the old scope no longer matches. `agent_files/runtime.rs` captures outside the transaction, rechecks project/source scope within admission, freezes initial-history deduplication through the supplied builder, and saves the admitted manifest before runtime custody returns. `agent_runs/compact.rs` retains that same accepted context. Automatic incoming-conversation injection is suppressed for project chats.

Native source-chat deletion uses `ProjectSourceChange` to save source invalidation plus stop plans with deletion, publish/fence after acknowledgment, roll back known failures, and fence uncertain outcomes. The direct human approval on 2026-10-28 resolved the earlier automatic-review scope rejection. Generic agent deletion now has the owned `ProjectAgentRemovalCandidate` hook in `projects/agent_removal.rs`, exported from projects/mod.rs, with four tests in agent_removal_tests.rs. The shared lifecycle writer calls `prepare(&mut state, agent_id, now_ms)` before native removals, captures `persistence_request(&mut state)` after removals, calls save_project_candidate, then publish before close_agent. Known failure restores native records before candidate.rollback; uncertainty calls candidate.fence_uncertain without reversing stops. Empty binding overlays never recreate deleted sessions. The four selected candidate-hook tests now pass; complete lifecycle and source-deletion interleavings remain pending shared validation.

Observed checks and coordinator-reported outcomes:

- Earlier SDK project/session tests: **28 passed**; SDK build passed. The later full default SDK test run now supersedes the earlier test-subset limitation: **51 files, 291 tests passed, physical exit 0**, including the three real-daemon integration tests and one restart/personality persistence test actually executed.
- Independent source/runtime review: **clear**, as reported by the coordinator; no native execution is implied.
- Native project-filter-selected log: **23 passed, 0 failed**, with 3,638 filtered out. Of the 23, **20** are under `projects::` or `routes::projects::`; three are unrelated attachment/history/completion matches. The checked subsets above cite the passing cases rather than treating all 23 as project-specific or claiming the full A/B/C/race matrices.
- Native recovery-selected log: **30 passed, 0 failed**, with 3,631 filtered out. It includes the two project recovery integration cases and the project context revision-only review case (the latter also appears in the project-selected log). These are overlapping selections, not 53 distinct project tests or a full daemon suite.
- Final current-tree full repository `rust:test`: **physical exit 0**, with the daemon library reporting **3,661 passed, 0 failed, 38 ignored**. The log also records the core and packaged supervisor suites; 3,661 is the daemon library count, not an aggregate Rust total. Ignored cases are not claimed passing.
- Final `rust:lint`: **physical exit 0**, as reported by the native driver and recorded in its final log; existing compiler/Clippy warnings and the nom future-incompatibility notice remain. Separately, the coordinator reported the vendor custody unit test **1/1 passed**; that result is not added to the daemon count or treated as a complete race matrix.
- Final full web run project checks: **19 passed, 0 failed** (ProjectHome 4, ProjectSourcePicker 2, useProjects 4, hash-route 7, SessionContinuity 2). Selected feature suites totaled **143 passed, 0 failed**.
- Final static project browser run: coordinator reported **4 passed**, covering two journeys at **1280×900** and **390×844**. Both document and project-container overflow assertions passed, alongside stable navigation/reload/private draft retention and invalidation removing a cached preview. These fixtures prove only those journeys, not the entire A/B/C or native acceptance matrix. No application server was started.
- Final full web suite: **red, exit 1: 2,284 passed, 165 failed, 1 skipped** across 204 files (2,450 tests). ViewHarness was 112 passed/36 failed; all eight corrected detached-textarea cases passed. The earlier focused ViewHarness run was 116 passed/32 failed. One unhandled rejection remains, Alpha disconnected at `apps/web/src/ViewHarness.test.tsx:748`; no worker startup error was observed.
- Web typecheck and lint both passed after that final full run; lint reported warnings. The earlier web production build and web-e2e typecheck passed; production assets were not rebuilt for test-only changes.
- The fixture correction was limited to openLiveSession and eight direct textarea captures, preserving behavior assertions. No broader source/test rewrites followed. Remaining full-suite failures are unresolved; they are not assumed wholesale to be baseline failures.
- No Cargo/Rust compile/test/lint ran in this chat. The native driver completed the full Rust test/lint commands and full default SDK tests above. Those green commands do not establish unsupported physical hydration/serialization/shared-publication races, complete A/B/C journeys, migration/PostgreSQL parity or every other unchecked matrix item. SDK typecheck/lint are not newly claimed from a test-only log. Source is frozen; root final cleanup owns the remaining coordination work.
- Live-provider, real-account and credential checks remain outside authorized scope and were not performed.

Evidence: [final full web report](../../../apps/web/test-output/vitest/project-combined-web-final.json), [exact failure comparison and selected feature results](../../../apps/web/test-output/vitest/project-combined-web-final-triage.json), and [focused ViewHarness comparison](../../../apps/web/test-output/vitest/project-view-harness-after-eight-triage.json). Configured test-output reports are retained; task-owned diagnostic copies were removed after recording results.

Final evidence: [full current-tree Rust test log](../../../test-output/continuity-full-rust-formatted.log), [final Rust lint log](../../../test-output/continuity-rust-lint-final.log), and [full default SDK test log](../../../test-output/continuity-sdk-full-final.log). Earlier native subset evidence remains in the [project-filter-selected log](../../../test-output/continuity-clean-projects.log) and [recovery-selected log](../../../test-output/continuity-clean-recovery.log). The final lint result is green with warnings; the selected logs alone never established it.

## Task 9: Integrated acceptance and final checks

**Files:** apps/web-e2e/src/console/projects.spec.ts; assigned console fixtures/{api,state}.ts or a project-specific adapter.

- [ ] Synthetic A/B/C flow: shared project -> A pins file/excerpt/output -> B gets only pins -> member switch/reload/back/forward -> stale-tab CAS -> revoke A during preview -> late response ignored -> re-add does not revive old lease/chats -> archive/delete retains native content.
- [ ] Prove Rust hydration/dispatch fencing separately with fake-provider/history barriers; browser mocks alone are insufficient.
- [x] Record the native driver's completed current-tree `rust:test` repository run: physical exit 0; daemon library 3,661 passed/0 failed/38 ignored, with core and supervisor suites also present in the log.
- [x] Record the native driver's final `rust:lint`: physical exit 0 with warnings, plus its separately reported vendor custody unit result of 1/1 passed.
- [ ] Standalone `rust:cache:status` execution is not established by these supplied final logs; no cache-status command is launched by this bookkeeping update.
- [ ] Run test/build/typecheck/lint for @animaos/sdk, then @animaos/web as separate commands; coordinate fixtures. Compare unrelated baselines without altering other work.
- [x] Record the full default SDK suite: 51 files/291 tests passed, physical exit 0, including all three daemon integration cases and the restart/personality case actually run. Other SDK commands and the red full web suite remain separate from this passing test result.
- [x] Run the final full web suite and record its current red outcome: 2,284 passed/165 failed/1 skipped, exit 1. This records execution, not a passing release gate.
- [x] Run final web typecheck and lint: both exit 0; lint has warnings.
- [x] Run `bun run --filter @animaos/web-e2e e2e-console projects.spec.ts` through the coordinator: all 4 cases passed at desktop/mobile sizes with overflow assertions. Existing console config uses static fixtures, no application server.
- [x] Record the coordinator's clear independent source/runtime review, named native proof subsets and final full Rust/lint/SDK test results above. Unsupported broad journey/race acceptance remains unchecked.
- [ ] Run git diff --check, inspect exact ownership paths; coordinator reviews combined source/API/UI. Completion requires checks passing in current combined tree.
- [ ] Remove only task-owned scratch/scripts/screenshots/logs after recording results, inspecting exact absolute targets and active users. Keep configured test-output reports/download caches; wrappers manage Cargo.
- [ ] Mark implementation boxes complete only with verified acceptance. Report through this chat final response; do not message other chats or edit shared coordination plan.

## Release gate

All nine tasks define the usable v1. Human-selected multi-agent sharing and shared-file ownership are settled; implementation and shared integrations are authored and frozen. Final full current-tree Rust tests, Rust lint and default SDK tests passed with physical exit 0; the separately reported vendor custody unit passed 1/1. Release acceptance is not claimed complete: full web remains red at 2,284 passed/165 failed/1 skipped, and unsupported broad A/B/C journeys and race matrices remain unchecked. Other command results are claimed only where explicitly recorded. The 19 project UI checks and four desktop/mobile browser cases remain green evidence, not substitutes for those matrices. Root final cleanup and its authoritative coordination plan govern combined status. Explicit pins/private native stores/link-only workflows/current local previews/conservative access-change cancellation remain the accepted tradeoffs. No live accounts or providers were exercised by this feature verification; remote sources, private-memory sharing, retroactive erasure and workflow-context inheritance remain outside scope.
