# Shared project spaces with selected context

Date: 2026-10-27
Status: Implementation and shared integration authored; independent source review clear. Project SDK/UI/browser checks passed. The final full web suite remains red; native verification is pending the coordinator's native driver.
Integration revision: 2026-10-28. Concrete host contracts below supersede earlier shorthand signatures.
Human decision: **One project is shared by multiple existing agents.** This replaces the earlier one-agent proposal.
Plan: [Implementation plan](../plans/2026-10-27-project-spaces.md)

## Outcome and v1 scope

The owner creates one project identity, selects member agents, writes shared instructions and pins shared sources. The project groups canonical chats, workspace files, outputs and workflows across its members. Each agent retains its identity, private memory, transcript ownership, model configuration, tool grants, credentials and approval authority. Projects create no agent, separate filesystem root or duplicate content store.

The complete v1 includes member management, project navigation, chat create/move/detach, selected files and exact excerpts, linked native outputs/workflows, bounded shared context, concurrency/restart protection and live revocation. Shared context means **owner-pinned material**, not an automatic merge of all transcripts. Native workflows use their saved steps; linking never starts them, changes their agent or transfers approval authority.

Private agent memory is neither read by a source adapter nor copied into project storage. The requesting agent still uses its own memory. Existing explicit recall tools remain scoped to their original agent; project membership is not an alternate credential for them.

## Inspected implementation

Inspected HEAD: `21bcf8457bbfe343ed38685fe2ed99abc6e4d260`. Preserve unrelated dirty cache/dev tooling, package-script/docs and results-panel work. Source inspection is not runtime acceptance.

| Source | Existing behavior / integration consequence |
| --- | --- |
| `hosts/rust-daemon/src/sessions/mod.rs` | Agent-owned SessionRecord contains clear generation, allowances, summary/checkpoint and chat_results. Preserve these stores; add canonical project membership here. |
| `routes/sessions.rs`, `state/session_state.rs` | Session admission has owner authorization, helper restriction, rate limiter and transaction/save/rollback. Reuse it; never infer project from caller metadata. |
| `control_plane_store.rs`, `state.rs` | Inspected snapshot version is 32; JSON/PostgreSQL use the same owned snapshot and persistence ordering. Coordinator combines one schema increment. |
| `agent_files/runtime.rs` | Captures scope, reads outside state locks, rechecks under transaction with five-second deadline/one retry. Current project_relative is empty and only selects directory instruction files. It is not a project identity. |
| `state/run_commit.rs`, `agent_runs/compact.rs` | Isolated runtime budgets history and retains accepted file/tool/adapter bindings through compaction. Retain frozen project bytes and live access lease in the same way. |
| `sessions/history_read.rs` | Exact bounded anchors validate agent/room/visible message identity across hot and durable history. Use a narrow host-approved pinned-source capability; do not relax general cross-agent history access. |
| `sessions/interactions.rs` | Automatic incoming-agent context spans this agent's conversations. Suppress that automatic provider in project turns so added project material is explicit; preserve own-agent memory/recall authority. |
| `routes/workspace_files.rs`, `tools/workspace_bytes.rs` | Safe preview rejects traversal, links/reparse/hidden/excluded/binary/credential content. Generated document preview requires verified Anima documents. Extract safe capture, do not duplicate or broaden policy. |
| `chat_results.rs` | Native results bind owning agent, session, clear generation and revision. Full identity belongs in a reference. |
| `runs/{ledger,turn}.rs`, `agent_runs/recovery.rs` | Current queued/recovery witnesses bind agent/input/workspace/config, not projects. Add host project/member witnesses with continuity-owned hooks. |
| `live/{events,fanout}.rs` | Agent streams support sessionless metadata events. Fan out project invalidations to old/new members, never another member's private run/transcript events. |
| `packages/sdk/src/{sessions,results,workflows,workspace}.ts` | Existing clients own canonical stores; add project client and narrow projections. |
| `apps/web/src/ViewHarness.tsx`, `lib/hash-route.ts`, `hooks/useCompanionSessions.ts` | Own routing/agent/chat composition; project route must preserve one identity across member switches. |

Context7 checked `/serde-rs/serde` for additive stored fields and strict requests: use defaulted stored fields and deny_unknown_fields on mutation bodies. [Serde derive attributes](https://github.com/serde-rs/serde/blob/master/_autodocs/api-reference/serde_derive.md). No new dependency/provider API is proposed.

## Alternatives

1. **Recommended: profile-owned projects, explicit member grants, exact source references and revocable host leases.** Meets the human's multi-agent choice and preserves canonical stores.
2. Workspace-wide tags with implicit access lack a dependable private/shared boundary or revocation behavior.
3. A synthetic project agent or per-agent cloned projects duplicate identity/instructions/reference state and contradict the single shared project requirement.

## Ownership invariants

- Only the profile owner mutates projects, memberships, instructions and pins. Agents/public viewers cannot enroll themselves or forge an acting-agent identity.
- Every member is an existing non-helper agent in this profile. Each grant has a fresh incarnation on re-add.
- A chat has zero/one project and retains its original `(agent_id,session_id)`. Eligible direct chats are non-main kind=chat, origin web/api/import. Main/connector/check-in/job/helper rooms keep their established semantics.
- Each project list/read/link/move/hydration/context dispatch checks current requesting-agent membership. A chat/result/workflow target also checks its owning agent remains a member and the canonical object belongs to that agent.
- Project chat lists expose grouping metadata only: original agent, chat ID/title, archive/activity counts. No transcript preview, message, tool/approval data or memory is returned.
- Cross-agent reads resolve only owner-pinned exact project resources. Private donor chats must first be moved into this project before an excerpt/result can be pinned. No donor history enumeration, adjacent messages, private memory, attachments or connector calls are available.
- The human owner can separately open an original chat/workflow in its native agent surface under existing owner authority. This is not a member-read fallback or a grant to another agent.
- Projects never transfer tool grants, account enrollment, session allowances, approval decisions, working roots or model configuration.
- Removal/archive/delete never cascade into native chats/files/results/workflows/effects. Revocation stops future direct access; it cannot retract text already delivered or erase prior authorized replies, summaries, memory or completed effects.

## Persisted and public types

Place host-owned records/access/admission in `hosts/rust-daemon/src/projects/`, alongside session orchestration. No HTTP/database dependency enters anima-core.

~~~ts
interface ProjectMember {
  agentId: string;
  generation: number; // assigned from monotonic project accessEpoch
  joinedAtMs: number;
}
interface ProjectSummary {
  id: string; // project_<uuid>; never reused
  title: string;
  status: 'active' | 'archived';
  revision: number; // CAS on every mutation
  accessEpoch: number; // membership/access-reducing mutation fences old leases
  members: ProjectMember[];
  createdAtMs: number;
  updatedAtMs: number;
  chatCount: number; // derived from canonical current-member sessions
  resourceCount: number;
}
type ProjectTarget =
  | { kind: 'workspace_file'; path: string }
  | { kind: 'chat_excerpt'; ownerAgentId: string; sessionId: string;
      clearGeneration: number; messageIds: string[] }
  | { kind: 'chat_result'; ownerAgentId: string; sessionId: string;
      clearGeneration: number; resultId: string }
  | { kind: 'workflow'; ownerAgentId: string; workflowId: string }
  | { kind: 'workflow_template'; ownerAgentId: string; slug: string };
interface ProjectResource {
  id: string;
  target: ProjectTarget | null; // null when donor access has been removed
  state: ProjectResourceState;
  role: 'reference' | 'output';
  label: string | null;
  useInContext: boolean; // forbidden for workflow/template
  createdAtMs: number;
}
interface Project extends ProjectSummary {
  instructions: string;
  resources: ProjectResource[]; // owner order; revoked donor targets redacted
}
interface ProjectSessionFields {
  projectId: string | null;
  projectBindingRevision: number;
}
interface ProjectChatLink {
  ownerAgentId: string; sessionId: string; title: string;
  archived: boolean; lastActivityAtMs: number; activeRuns: number;
  projectBindingRevision: number;
} // not a donor Session DTO and contains no message preview
type ProjectResourceState = 'ready' | 'missing' | 'not_in_project'
  | 'access_removed' | 'source_changed' | 'workspace_changed' | 'excluded'
  | 'unsafe_content' | 'unsupported' | 'unavailable' | 'budget_omitted'
  | 'already_in_current_chat';
interface ProjectContextReport {
  projectId: string; projectRevision: number; accessEpoch: number;
  memberGeneration: number; estimatedTokens: number;
  basis: 'current_preview' | 'admitted'; runId?: string;
  resources: {resourceId: string; state: ProjectResourceState;
    includedChars: number; shortened: boolean}[];
}
~~~

Persist `projects: Vec<ProjectRecord>` in the existing snapshot; records store references, not source bytes/counts/workflow execution state. Workspace refs additionally store an internal host-captured SHA-256 key of the canonical configured workspace root. Agent-owned refs capture their donor membership generation; chat refs also capture the donor session's project-binding revision. These hidden pin-time bindings prevent old resources reviving after remove/re-add/re-attachment. Equal relative paths in another root must not silently follow.

SessionRecord adds defaulted project ID/binding revision. RunRecord adds a defaulted `QueuedProjectWitness` and content-free `ProjectContextManifest`. Witness is `{sessionBindingRevision, project: null | {projectId,projectRevision,accessEpoch,memberGeneration}}`. Include unassigned membership too, preventing a queued turn from silently acquiring a project later. Manifest records accepted IDs/epochs, donor grant generations, instruction/source digests/native revisions and coverage, never source text or private configuration. Run DTO can expose only the safe context report.

Limits: 100 projects/profile; 32 current members/project; >=1 on create; 1–120 plain-text title chars; instructions <=4,000 Unicode scalar values and 16 KiB; 24 resources, <=12 auto-context, <=6 excerpt refs, 1–4 exact visible user/assistant messages/excerpt; labels <=120 chars. Reject duplicate full normalized targets. Revisions/epochs are checked safe positive integers. Each membership mutation increments epoch; a newly added member gets that fresh generation without unbounded tombstones. Last-member removal leaves an owner-managed zero-member project.

Owner management retains well-formed broken refs. Member projections redact removed donor target/title/content, returning generic unavailable rows. Invalid IDs/duplicates/unsafe stored paths/impossible ownership reject restore; missing objects are unavailable refs. A session with a missing project is unresolved and cannot run until explicit repair.

## HTTP and SDK contracts

All project APIs require existing owner-console authorization, no-store, bounded bodies and strict request parsing. Normal reads additionally require current member selector; the runtime instead obtains agent identity from its host-bound run. asAgentId never authenticates a caller. Viewer/unauthenticated checks precede lookup; nonmember/cross-profile IDs have nondisclosing errors.

| Operation | Contract |
| --- | --- |
| Member list | `GET /api/projects?memberAgentId=a&status=active|archived` -> bounded/sorted `{projects: ProjectSummary[]}` for current member a. |
| Owner management | `GET /api/projects/manage` -> metadata/member summaries including zero-member projects, no source payloads or transcript bypass. |
| Create | `POST /api/projects {requestKey,title,memberAgentIds,instructions?}` -> `201 {project}`; exact retry returns original, mismatched replay 409. |
| Read | `GET /api/projects/p?asAgentId=a` -> current member projection `{project}`. |
| Edit/archive/restore | `PATCH /api/projects/p {expectedRevision,title?,instructions?,status?}` -> `{projectSummary}`; archived content read-only except title/restore. |
| Replace members | `PUT /api/projects/p/members {expectedRevision,agentIds}` -> atomic set, `{projectSummary,accessEpoch,detachedChats,stopRequestedRunIds}`. No private text. |
| Delete | `DELETE /api/projects/p {expectedRevision}` -> `{deleted:true,detachedChatCount}`, no native deletion. |
| Add resource | `POST .../resources {expectedRevision,requestKey,asAgentId,target,role,label?,useInContext}` -> `{project}`, validates requester/donor/target. |
| Edit/remove | `PATCH/DELETE .../resources/r` with expectedRevision/asAgentId and label/role/useInContext patch -> `{project}`; target immutable. Remove/unshare fences access. |
| Order | `PUT .../resource-order {expectedRevision,asAgentId,resourceIds}` -> `{project}`, exact current ID set. |
| Pinned preview | `GET .../resources/r/preview?asAgentId=a` -> stored pinned projection only, bounded, epoch/member envelope; no arbitrary target argument. |
| Context preview | `GET .../context?asAgentId=a` -> ProjectContextReport/current_preview, no provider/account calls. |
| Chat grouping | `GET .../chats?asAgentId=a&cursor&limit` -> bounded ProjectChatLink page, membership before pagination, no transcript previews. |
| Move/detach | Existing-agent `PUT /api/agents/a/sessions/s/project {projectId:string|null,expectedBindingRevision,expectedProjectRevision?}` -> `{session}`; current chat-owner membership required for target, target revision required. |
| New project chat | Existing `POST /api/agents/a/sessions {title?,projectId?,projectRevision?,memberGeneration?}` -> `{session}`; project tuple together, current member/limiter checks atomically. |
| Own-agent filter | Existing `GET /api/agents/a/sessions?projectId=p` -> SessionPage for a only, verifies a's project membership; never broadens donor scope. |

Metadata/member/status/delete administrative operations use independent owner authority and cannot hydrate sources. Normal content endpoints have no member-bypass switch. Preserve existing native session/history API authorization; do not add projectId as an alternate credential.

Creation keys/fingerprints/project identities are retained in bounded profile-level creation receipts, including after deletion/restart. Exact replay of a deleted creation returns a deleted outcome/conflict, never recreates a project or membership; mismatched replay is 409. Reserve receipt capacity before mutation, fail closed at 10,000 receipts, and never silently evict. Resource-add exact retry can resolve a surviving resource; after removal its original expectedRevision is stale and returns 409 rather than promising historical replay. Known save failure publishes nothing. Uncertain save closes project access pending durable readback, never silently reopens revoked access. Use existing ErrorBody. SDK adds ProjectsClient, abortable reads, membership/preview types, additive Session/Run fields and capability detection for older daemons.

## Live membership and access revocation

A host ProjectAccessRegistry publishes current profile/project epochs/member incarnations. Private ProjectAccessLeases bind requesting member, accessEpoch and selected donor incarnations. They do not grant tools and cannot be constructed from model arguments.

Member-set mutation under the existing control-plane transaction:

1. Validate owner/revision/agents. Prepare a private candidate with new member set, incremented revision/epoch and atomic detachment/binding-revision increment of removed agent chats. Keep stale donor refs internally for repair, redact their member projections.
2. Save candidate before publication. Known failure leaves prior state; uncertainty fences closed pending reconciliation.
3. Publish candidate state and epoch under a short publication fence, invalidate old leases, cancel in-flight hydration and signal existing CancelSignal controls. **Conservative v1: every member-set change fences all project leases/runs**, including remaining members whose shared context can depend on a departing donor. Re-add never revives old callbacks.
4. Acknowledge only after the durable fence and cancellation requests. No wait/claim of physical provider/process termination or undo; retain billed usage/action receipts. New dispatch must acquire current authorization. An attempt authorized before the fence is already in flight and is cancelled cooperatively.
5. Publish content-free `project.updated {projectId,revision,accessEpoch,deleted,membershipChanged}` to the union of old/new member-agent streams, and own-agent session.updated for detached chats. Never broadcast donor message/run events or source payloads.

Check the lease immediately before each provider attempt/fallback/retry, at stream/output acceptance, before project-aware native tool dispatch/shared publication and after source hydration. Late blocking workers may finish I/O but cannot publish bytes to model/API/shared outputs after revocation. A stopped run may persist usage/receipts/historical evidence under existing stop semantics, not a new successful shared result from stale access.

Source removal/unsharing also advances epoch and fences immediately; it does not wait for idle. Instruction edits/additions/reorder affect later admissions while an active accepted snapshot stays frozen. Archive/delete remain idle-only in v1, refuse queued/running/approval/recovery-in-flight project chat work, then fence access. Linked workflows remain independently governed and are not cancelled or deleted by grouping actions.

## Context admission, selection and budget

1. Queue acceptance captures current session binding, project revision/epoch/member generation. Reject nonmember/unresolved/archived state. A changed witness before execution requires review of saved input; never silently substitute newer context.
2. Capture refs and acquire lease under short state access. Read outside locks within existing admission's five-second deadline/cancellation and one ordinary content-race retry. Revocation is hard refusal, not a retry under a new grant.
3. File adapter reuses safe UTF-8 or verified Anima-generated document preview. Text cap 64 KiB/file, 256 KiB aggregate; <=2 generated documents within existing document byte cap/common deadline. No arbitrary uploaded Office/PDF, remote attachment, connector call or recursive folder selection.
4. Private pinned-source capability binds project/requester grant/epoch/resource/donor grant/session/generation/exact IDs. Read donor's canonical history only after capability validation, zero-neighbor anchors, selected visible user/assistant text only. Ordinary caller-scoped history tools remain unchanged. No adjacent/system/tool/approval/attachment/memory content.
5. Results bind full donor agent/session/generation/result identity and current result revision. Workflow/template previews expose selected title/status/agent/navigation only, no private saved-step prompts or effect authority.
6. Final recheck requester/donor memberships/epoch/project revision/session bindings/generations/result revisions before admission/serialization. One ordinary content race can retry; repeated race or revoked access refuses before provider/tool work.
7. Build immutable Arc<AdmittedProjectContext> with separately framed owner instructions and quoted data, content-free manifest and live lease. Reserve current input/reply/summary/checkpoint/accepted identity files first. Project cap is min(3,000 estimated tokens, floor(remaining usable tokens/4)), <=12,000 chars total, <=2,000/source. Instructions fit whole or admission fails. Owner order determines optional source inclusion; report every omission/shortening.
8. Reserve bounded candidate project allocation before history selection. Remove duplicate exact active-chat IDs afterward, mark already_in_current_chat, do not refill history/sources in a feedback loop. Count actual rendered project text once.
9. Persist manifest/witness before provider work; save failure publishes no admission. Install project_context provider, keep requesting agent's identity/memory/config/tools. Compaction/retry/fallback/steer retain identical bytes **and the revocable lease**, not fresh project reads.

File edits after a captured read affect a later turn; hashes/revisions explain captured content. Context inspector shows project coverage separately from its existing conversation-only meter. Preview is a current estimate, not billed usage or a guaranteed future prompt.

## UI, concurrency and stale replies

- Sidebar lists projects for selected agent membership. Stable home `#/p/projectId` has Working as member selector, retaining one shared identity across A/B. A nonmember switch clears shared payloads and offers owner-managed Add member, never automatic enrollment.
- New project selects existing agents/name/instructions. Copy: “Members share project instructions and the sources you choose. Their private memory, chats and permissions stay separate.”
- Home tabs: Chats, Sources, Outputs, Workflows, Members. New chat explicitly selects a member; ordinary New chat stays unassigned. Chat rows show owning agent/title, no transcript preview; original chat opens as a deliberate owner action in native context.
- Owner moves an eligible idle chat into project before pinning exact messages. Sources explicitly share linked projections; Use in chat context separately controls automatic injection. No automatic whole transcript merge.
- Outputs contain only owner-linked native result/file refs, not every member's payloads. Shared preview is read-only; original editing remains native. Workflow links open original agent controls and explain saved steps; no approval/resume permission transfers.
- Member changes warn: “Changing members stops current project replies. Existing chats and completed actions are kept.” Removed agent chats detach; re-add does not silently reattach. Zero-member projects remain in owner metadata management for repair/delete.
- Hook scope is (profile,projectId,actingAgentId,memberGeneration,accessEpoch,localRequestGeneration). On newer epoch/removal/switch/resync/disconnect, abort requests, clear shared preview/payload caches, advance generation, then revalidate. Reject late responses with older scope/epoch/generation.
- While stream disconnected, hide cached shared source payloads and disable context-sensitive actions until membership revalidates. Focus/reconnect revalidate; visible-only 10-second polling is recovery, not access enforcement. Server gates response publication independently. Previously delivered bytes cannot be retracted and an unseen network event cannot be detected instantaneously.
- CAS conflict retains unsaved editor text keyed by project/owner operation. Missing/revoked donor resources are generic unavailable to members; owner management can repair retained refs. No lookup fallback by title/ID/path.
- Archive blocks new project turns until restore/detach. Delete atomically detaches chats and removes only project metadata; linked workflows/automations continue. Normal flows use plain language and existing keyboard/focus/responsive patterns.

## Restart and dangling references

Restore registry/members/incarnations/session bindings/manifests from validated owned snapshot. Rebuild access publication from current durable state, never restore live leases. No project provider/account/background scan starts at boot. Automatic recovery requires unchanged current member/project/epoch/session witnesses and all existing native recovery conditions. Explicit changed-context continuation must reacquire context through continuity's reviewed receipt/budget checks; until integrated, block it with saved-input guidance.

Agent deletion removes its membership through the same fence; remaining project survives. Source move/clear/delete invalidates binding/generation; a revised/branched chat starts unassigned. Access-reducing source changes fence leases. Workspace changes invalidate file refs until explicit rebind. Missing project sessions remain unresolved; malformed ownership rejects restore.

Already authorized shared text can remain in receiving agent replies/summaries/memory. Revocation protects future direct access, not derivative historical erasure. Project membership never reads another agent's private memory or changes the receiving agent's existing memory policy.

## Concrete host integration contract (2026-10-28)

This is the interface freeze candidate for the shared integration writer and continuity owner. It replaces the plan's earlier shorthand signatures. All types below are daemon-only `pub(crate)`; lease/scope/loaded/accepted internals are private to `projects`. They are never imported by anima-core or serialized into portable TurnBinding/TurnCheckpoint. Normal Rust doc comments, derives and error implementations accompany implementation; this section fixes names, fields, ownership and call order.

### Persisted records and exact central fields

Definitions in `projects/records.rs`, individually re-exported from `projects/mod.rs`:

~~~rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectRecord {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) status: ProjectStatus,
    pub(crate) revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) members: Vec<ProjectMemberGrant>, // sorted by agent_id
    pub(crate) instructions: String,
    pub(crate) resources: Vec<ProjectResourceRecord>, // owner order
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectStatus { Active, Archived }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectMemberGrant {
    pub(crate) agent_id: String,
    pub(crate) generation: u64,
    pub(crate) joined_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectMutationReplay {
    pub(crate) request_key: String,
    pub(crate) request_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectCreationReceipt {
    pub(crate) request_key: String,
    pub(crate) request_sha256: String,
    pub(crate) project_id: String,
    pub(crate) created_at_ms: u64,
    pub(crate) deleted_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectDonorBinding {
    pub(crate) agent_id: String,
    pub(crate) member_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectResourceRole { Reference, Output }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub(crate) enum ProjectResourceTarget {
    WorkspaceFile { path: String },
    ChatExcerpt {
        owner_agent_id: String, session_id: String,
        clear_generation: u64, message_ids: Vec<String>,
    },
    ChatResult {
        owner_agent_id: String, session_id: String,
        clear_generation: u64, result_id: String,
    },
    Workflow { owner_agent_id: String, workflow_id: String },
    WorkflowTemplate { owner_agent_id: String, slug: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectResourceRecord {
    pub(crate) id: String,
    pub(crate) target: ProjectResourceTarget,
    pub(crate) role: ProjectResourceRole,
    pub(crate) label: Option<String>,
    pub(crate) use_in_context: bool,
    pub(crate) created_at_ms: u64,
    pub(crate) add_replay: ProjectMutationReplay,
    pub(crate) donor: Option<ProjectDonorBinding>,
    pub(crate) source_project_binding_revision: Option<u64>,
    pub(crate) workspace_key: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectRegistry {
    records: std::collections::BTreeMap<String, ProjectRecord>,
    creation_receipts: std::collections::BTreeMap<String, ProjectCreationReceipt>,
}
impl ProjectRegistry {
    pub(crate) fn snapshot(&self) -> Vec<ProjectRecord>;
    pub(crate) fn creation_receipts_snapshot(&self) -> Vec<ProjectCreationReceipt>;
    pub(crate) fn get(&self, project_id: &str) -> Option<&ProjectRecord>;
}
~~~

Record validation enforces exact option shapes: workspace target has workspace_key only; chat targets have donor plus source_project_binding_revision only; workflow/template targets have donor only. Donor agent must equal target owner. No wire request can supply these hidden pin-time bindings. Hashes are lowercase 64-hex; request keys are nonempty <=128 bytes; message IDs follow existing <=256-byte bounded history IDs. Fingerprints hash canonical normalized caller input, not changing workspace/content state. Fresh pin captures donor generation/session binding; re-add/re-attachment requires an explicit remove/add pin to get new bindings.

Integration writer adds exactly:

~~~rust
// ControlPlaneSnapshot; serde(default)
pub(crate) projects: Vec<crate::projects::ProjectRecord>,
#[serde(default)]
pub(crate) project_creation_receipts: Vec<crate::projects::ProjectCreationReceipt>,

// DaemonState, initialized with Default::default()
pub(crate) projects: crate::projects::ProjectRegistry,
pub(crate) project_access: crate::projects::ProjectAccessRegistry,

// SessionRecord; each field serde(default)
pub(crate) project_id: Option<String>,
pub(crate) project_binding_revision: u64,

// RunRecord; each field serde(default, skip_serializing_if = "Option::is_none")
pub(crate) project_witness: Option<crate::projects::QueuedProjectWitness>,
pub(crate) project_context: Option<crate::projects::ProjectContextManifest>,
~~~

New session constructors set None/0; run constructors start None/None and the ordinary host acceptance path fills project_witness before its first durable queue/start save. `None` witness means legacy/unbound, while `Some` with project=None is a captured unassigned binding. The shared writer owns the single version **33** increment and its exact backup; this workstream will not edit the schema version/central constructors in parallel.

### Queue witnesses, manifest and accepted context

~~~rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueuedProjectWitness {
    pub(crate) session_binding_revision: u64,
    pub(crate) project: Option<RunProjectBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunProjectBinding {
    pub(crate) project_id: String,
    pub(crate) project_revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) member_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub(crate) enum ProjectSourceWitness {
    WorkspaceFile {
        workspace_key: String, path: String,
        capture_sha256: String, complete_source: bool,
    },
    ChatExcerpt {
        donor: ProjectDonorBinding, session_id: String,
        clear_generation: u64, project_binding_revision: u64,
        source_state_sha256: String, message_ids: Vec<String>,
    },
    ChatResult {
        donor: ProjectDonorBinding, session_id: String,
        clear_generation: u64, project_binding_revision: u64,
        result_id: String, result_revision: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectSourceState {
    Ready, Missing, NotInProject, AccessRemoved, SourceChanged,
    WorkspaceChanged, Excluded, UnsafeContent, Unsupported, Unavailable,
    BudgetOmitted, AlreadyInCurrentChat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSourceAdmission {
    pub(crate) resource_id: String,
    pub(crate) state: ProjectSourceState,
    pub(crate) included_chars: u32,
    pub(crate) shortened: bool,
    pub(crate) included_sha256: Option<String>,
    pub(crate) witness: Option<ProjectSourceWitness>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectContextManifest {
    pub(crate) version: u32, // exactly 1
    pub(crate) run_id: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) session_binding_revision: u64,
    pub(crate) project: RunProjectBinding,
    pub(crate) captured_at_ms: u64,
    pub(crate) instructions_sha256: String,
    pub(crate) rendered_sha256: String,
    pub(crate) estimated_tokens: u64,
    pub(crate) sources: Vec<ProjectSourceAdmission>, // <=12 selected refs
}

pub(crate) struct AdmittedProjectContext {
    text: String,
    manifest: ProjectContextManifest,
    lease: ProjectAccessLease,
}
impl AdmittedProjectContext {
    pub(crate) fn text(&self) -> &str;
    pub(crate) fn manifest(&self) -> &ProjectContextManifest;
    pub(crate) fn estimated_tokens(&self) -> u64;
    pub(crate) fn lease(&self) -> &ProjectAccessLease;
    pub(crate) fn for_selected_history(&self, history: &[anima_core::Message]) -> std::sync::Arc<Self>;
}
~~~

Source-state digest is a domain-separated hash of the donor session's source-relevant binding/generation/creation/activity/pruning/revision visibility and selected hot-message visibility bits. Recheck it under state read at final acceptance; a native change affecting excerpt visibility must change this stamp. Durable messages remain keyed by donor agent/session/exact ID. No private raw configuration/approval/memory enters the stamp input. A file capture hash identifies the bounded captured bytes, not a claimed full-file hash when complete_source=false; included_sha256 identifies the exact rendered source slice. UI projects only resourceId/state/chars/shortened, never these internal witnesses/hashes.

### Access registry and opaque lease

Definitions in `projects/access.rs` (non-serializable, no public constructors for lease):

~~~rust
#[derive(Clone, Default)]
pub(crate) struct ProjectAccessRegistry {
    inner: std::sync::Arc<ProjectAccessInner>,
}
#[derive(Default)]
struct ProjectAccessInner {
    published: std::sync::RwLock<
        std::collections::BTreeMap<String, PublishedProjectAccess>
    >,
}
struct PublishedProjectAccess {
    revision: u64,
    access_epoch: u64,
    status: ProjectStatus,
    members: std::collections::BTreeMap<String, u64>,
    epoch_cancel: anima_core::CancelSignal,
    closed_uncertain: bool,
}
#[derive(Clone, Copy)]
enum ProjectAccessPurpose { MemberRead, Run }

#[derive(Clone)]
pub(crate) struct ProjectAccessLease {
    registry: std::sync::Weak<ProjectAccessInner>,
    project_id: String,
    requesting_agent_id: String,
    member_generation: u64,
    access_epoch: u64,
    purpose: ProjectAccessPurpose,
    epoch_cancel: anima_core::CancelSignal,
}
impl ProjectAccessLease {
    pub(crate) fn check(&self) -> Result<(), ProjectError>;
    pub(crate) async fn cancelled(&self);
}
impl ProjectAccessRegistry {
    // Call only after a durable candidate is published under daemon write guard.
    pub(crate) fn publish_records(
        &self, records: &ProjectRegistry,
    ) -> Result<(), ProjectError>;
    // Cancels old leases, rejects new acquisition until explicit reconciliation.
    pub(crate) fn fence_uncertain(&self, project_id: &str);
}
~~~

Registry instance identity binds leases to one DaemonState/profile; private Weak fields cannot be forged or transferred by serialized IDs. Lock order is daemon state guard -> registry publication lock; lease.check takes only the short registry read lock. No guard spans await/I/O. Lock poisoning is fatal/fail-closed, never recovered into an open gate. acquire is private to projects and requires checked current bindings.

publish_records preserves the epoch signal on ordinary same-epoch edits; any changed epoch/removal permanently cancels the old signal. Same epoch with changed member set/status, epoch regression or malformed publication closes affected access and errors. On post-save publication failure the mutation is unavailable, not acknowledged as successful. Deleted records disappear from the map; old leases fail lookup as well as cancellation. Archive allows a newly acquired MemberRead lease for read-only historical project views, but no Run lease. Uncertainty is a separate closed state for both purposes. Reconciliation may issue new leases only after durable state is known; old cancelled signals remain cancelled.

### Continuity-owned hook signatures and semantics

~~~rust
pub(crate) fn capture_run_witness(
    state: &crate::state::DaemonState,
    agent_id: &str,
    session_id: &str,
) -> Result<QueuedProjectWitness, ProjectError>;

pub(crate) fn validate_queued_witness(
    state: &crate::state::DaemonState,
    run: &crate::runs::RunRecord,
) -> Result<(), ProjectError>;

pub(crate) fn validate_active_lease(
    state: &crate::state::DaemonState,
    run: &crate::runs::RunRecord,
) -> Result<(), ProjectError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectChangeReason {
    MissingWitness, SessionBindingChanged, ProjectMissing, ProjectArchived,
    MemberRemoved, MemberGenerationChanged, AccessEpochChanged,
    ProjectRevisionChanged, AccessUnavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectContextChange {
    Unchanged,
    ReviewRequired(ProjectChangeReason),
    Unavailable(ProjectChangeReason),
}
pub(crate) fn context_change_summary(
    state: &crate::state::DaemonState,
    run: &crate::runs::RunRecord,
) -> ProjectContextChange;

pub(crate) fn current_authorized_witness(
    state: &crate::state::DaemonState,
    run: &crate::runs::RunRecord,
) -> Result<QueuedProjectWitness, ProjectError>;
~~~

- capture_run_witness requires canonical session and owner agent; validates eligible project session, active project, current member and publication availability. Nongrouped native/helper/job sessions yield a captured unassigned witness, never inherited project membership. Call during native acceptance before first durable save and before portable turn binding.
- validate_queued_witness compares exact session binding, project ID/revision/epoch/member incarnation, current eligible membership/status and publication. Missing legacy witness succeeds only when current session is unassigned with binding revision 0. Otherwise it is review-required/fail-closed. No state mutation, I/O or implicit rebind.
- validate_active_lease is deliberately `(state, run)`, not a lease-only function: existing turn/action/checkpoint hooks have those arguments. It checks saved witness/current session binding/project ID/access epoch/member incarnation plus an admitted manifest matching run identity and witness. It ignores **current project revision** after admission, allowing ordinary instruction edits to affect later turns only. It rejects removed/moved/archived/uncertain grants. Unassigned turns need no manifest; legacy unassigned revision-0 behavior remains compatible.
- current_authorized_witness captures current evidence but requires saved/current session-binding revision, project ID, accessEpoch and member incarnation to match. Legacy witness absence is permitted only for unassigned revision-0 sessions. Only ordinary revision changes (instructions/reorder/additions) may be reviewed. It never hydrates or mutates state. Moves/detachments/access reductions/replaced grants fail closed, including remove/re-add/re-attach histories and source unshare.
- context_change_summary performs no hydration/mutation; exact match -> Unchanged; revision-only change in the same retained scope -> ReviewRequired; epoch/member/binding change or missing/revoked/archived/uncertain scope -> Unavailable. Continuity keeps unchanged/legacy recovery fingerprints, binds owner-reviewed revision-only evidence to serialized current QueuedProjectWitness and rechecks inside admission. Old receipts remain owner-inspectable after access change but are never auto-fed into a newly authorized scope. No parallel owner token is added; valid explicit continuation captures a fresh witness/lease while retaining native budgets and historical-child replay rules.

ProjectError is a daemon-local Debug/Display/Error type with these matching variants: InvalidRecord, NotFound, NotMember, UnresolvedBinding, Archived, StaleRevision, ScopeChanged, AccessRevoked, AccessUnavailable, InstructionBudget, Cancelled, Deadline, Persistence. ScopeChanged alone is eligible for one same-grant I/O retry; AccessRevoked/Unavailable never silently retry under a new grant. API maps to existing ErrorBody; hook owners use error.to_string() for existing String contracts. No new global error taxonomy/core enum is required.

### Context capture, load and final recheck

~~~rust
pub(crate) struct CapturedProjectScope {
    run_id: String,
    agent_id: String,
    session_id: String,
    witness: QueuedProjectWitness, // project is Some
    project: ProjectRecord,       // ephemeral bounded copy, not another store
    workspace_root: Option<std::path::PathBuf>,
    workspace_key: Option<String>,
    lease: ProjectAccessLease,
}
pub(crate) struct ProjectRenderBudget {
    max_tokens: u64,
    max_total_chars: usize,
    max_source_chars: usize,
    calibration_permille: u32,
}
impl ProjectRenderBudget {
    pub(crate) fn from_remaining(
        remaining_input_tokens: u64, calibration_permille: u32,
    ) -> Self;
}
pub(crate) struct LoadedProjectContext {
    text: String,
    manifest: ProjectContextManifest,
}

pub(crate) fn capture_context_scope(
    state: &crate::state::DaemonState,
    run: &crate::runs::RunRecord,
) -> Result<Option<CapturedProjectScope>, ProjectError>;

pub(crate) async fn load_context(
    state: &crate::app::SharedDaemonState,
    scope: &CapturedProjectScope,
    budget: ProjectRenderBudget,
    run_cancel: anima_core::CancelSignal,
    deadline: std::time::Instant,
) -> Result<LoadedProjectContext, ProjectError>;

pub(crate) fn recheck_context(
    state: &crate::state::DaemonState,
    run: &crate::runs::RunRecord,
    scope: &CapturedProjectScope,
    loaded: LoadedProjectContext,
) -> Result<std::sync::Arc<AdmittedProjectContext>, ProjectError>;

pub(crate) async fn preview_context(
    state: &crate::app::SharedDaemonState,
    project_id: &str,
    requesting_agent_id: &str,
) -> Result<ProjectContextReport, ProjectError>;
~~~

capture validates queued witness and privately acquires Run lease; returns None only for a valid unassigned session. load performs bounded I/O outside locks and cannot publish/mutate run state; it returns **Loaded**, not Admitted. The runtime alone calls recheck under its existing transaction/state write guard, transferring the loaded bytes and cloning the same live lease only after identities/source stamps are current. It then saves `accepted.manifest().clone()` onto run.project_context with context-trim changes before any provider work. Failure rolls back/not-admits. recheck is synchronous and does no history/file I/O.

Additive shared builders are `build_run_runtime_with_files_and_project(..., files: &RenderedAgentFiles, project: Option<Arc<AdmittedProjectContext>>)` and `build_run_runtime_with_bindings_and_project(..., files, tools, adapter, project)`; old builder signatures remain wrappers passing None. RunBuild adds `project_context: Option<Arc<AdmittedProjectContext>>`. Initial selection reserves candidate project tokens, then calls for_selected_history on that snapshot to remove exact current-chat message duplication, and uses its final estimate/provider/manifest. The method is idempotently frozen after first use; compaction passes the returned Arc unchanged through the bindings builder. Owned agent_files/runtime.rs persists the final RunBuild manifest, not the pre-dedup estimate. Private admitted internals additionally retain bounded ephemeral rendering fragments/budget for this one initial dedup step; none is serialized.

Budget constructor enforces min(3000, remaining/4), 12000 total chars, 2000/source, calibration bounds; callers cannot raise caps. Shared five-second deadline belongs to the outer combined file/project admission, not a fresh timer per helper/retry. preview_context uses a separate MemberRead lease and the same bounded readers, does not fabricate a RunRecord/manifest or invoke a model. It returns only current-preview DTO after final epoch/revision check.

### Existing authority hooks; no parallel project permission system

The integration writer also owns the existing ordered snapshot extension:

~~~rust
// In ControlPlanePersistRequest; creates an unpublished project-only overlay.
pub(crate) fn with_project_candidate(
    self,
    records: Vec<crate::projects::ProjectRecord>,
    creation_receipts: Vec<crate::projects::ProjectCreationReceipt>,
    bindings: Vec<crate::projects::ProjectSessionBindingPatch>,
) -> Option<Self>;
pub(crate) async fn save_project_candidate(self) -> std::io::Result<()>;
~~~

ProjectSessionBindingPatch is `{agent_id:String, session_id:String, created_at_ms:u64, expected_binding_revision:u64, project_id:Option<String>, project_binding_revision:u64}`. It updates only a matching existing canonical session, never allowances/transcripts/results. The candidate uses require-current ordering; only typed pre-I/O supersession permits rebuild. The acknowledged-snapshot merger retains the latest project records/creation receipts and monotonically newer membership patches in later ordinary saves prepared before project publication, without resurrecting deleted sessions. This prevents stale ordinary writes from losing deletion receipts or resurrecting membership. All project metadata candidates use this path. Ordinary creation of a new project chat can keep its existing native session-create save; no second project chat list or metadata mutation is necessary for its derived count.

Integration writer supplies `projects::bind_model_adapter(inner: Arc<dyn anima_core::ModelAdapter>, lease: Option<ProjectAccessLease>) -> Arc<dyn anima_core::ModelAdapter>` at the session adapter's **inner profile adapter**, below attachment/fallback wrappers. The host wrapper preserves provider/single_attempt_contract and implements generate/stream/controlled methods using existing ModelCallContext, cancellation and stream sink APIs. Every fallback/physical retry sees the same revocable gate, and stream forwarding rechecks it. It neither replaces the native execution budget nor mutates core types.

Use the existing `NativeActionJournal::check_owner(state, starting)` in `agent_runs/action_receipts.rs`: when starting=true, call validate_active_lease after locating canonical RunRecord. Existing revalidate/native_file_authority/execution_authority/rpc_authority already reuse this gate, including checkpoint/document publication. Outcome settlement (starting=false) must remain permitted after revocation so real effects/usage/receipts retain custody. No project field or new dispatch path is needed in tools.rs merely to duplicate this check. The continuity owner adds validate_queued_witness to current_admission and validate_active_lease to runs/turn::active. Existing TurnCheckpointSink then reuses it; portable TurnBinding/TurnCheckpoint remains unchanged.

No project membership/context automatically propagates to helper/delegation/workflow rooms. An assignment carrying already authorized shared text is subject to existing delegation/tool controls; it grants no new project reader. Parent cancellation must use existing child-run cancellation semantics rather than grant a helper membership. Explicitly selected specialist source access still requires its own current project membership.

### Restore and project.updated contract

New `projects/restore.rs` exports:

~~~rust
pub(crate) struct RestoredProjects {
    pub(crate) registry: ProjectRegistry,
    pub(crate) access: ProjectAccessRegistry,
}
pub(crate) fn restore_projects(
    records: Vec<ProjectRecord>,
    creation_receipts: Vec<ProjectCreationReceipt>,
    state_with_agents_and_sessions: &crate::state::DaemonState,
) -> Result<RestoredProjects, ProjectError>;
pub(crate) fn validate_run_project_fields(
    run: &crate::runs::RunRecord,
) -> Result<(), ProjectError>;
~~~

Boot-only call after canonical agents/sessions are restored, before API/model admission: `let restored = projects::restore_projects(snapshot.projects, snapshot.project_creation_receipts, self)?; self.projects = restored.registry; self.project_access = restored.access;`. Shared writer maps ProjectError to its existing restore String result. Validate bounds/IDs/uniqueness/member existing non-helper agent, numeric safety/hidden option shapes/replay hashes, manifest version/run-witness identity and source bounds. Creation receipts have unique request keys/project IDs, are <=10,000, and correspond exactly to a surviving project when deleted_at_ms=None; deleted receipts cannot point to a surviving project. Do not compare historical manifests against current revision/membership as a structural restore requirement; revoked historical attempts remain valid history but cannot dispatch. Missing donor objects, old pin-time grants and missing-project/nonmember session bindings remain derived unavailable/unresolved, never silently removed or reparented. All new registry signals are fresh; no live lease is restored. Live snapshot replacement requires the existing drain/ownership workflow, never this boot helper while work is active.

~~~rust
#[derive(Clone, Debug)]
pub(crate) struct ProjectInvalidation {
    pub(crate) project_id: String,
    pub(crate) revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) deleted: bool,
    pub(crate) membership_changed: bool,
    pub(crate) destinations: std::collections::BTreeSet<String>,
}
pub(crate) fn publish_project_invalidation(
    state: &crate::state::DaemonState,
    change: &ProjectInvalidation,
);
~~~

Mutation service constructs this notice from old/new records while preparing the candidate. destinations is the old/new member union, internal only. Deletion's final checked revision/epoch is old+1 even though its record disappears. After durable state/access publication, publisher sends one `LiveEvent::new(destination, LiveEventBody::ProjectUpdated { project_id, revision, access_epoch, deleted, membership_changed })` per destination. Body is exactly those five scalar fields; wire also has existing type=project.updated, agentId, seq, at, and no session/run/member list/source payload. Instruction edits increment revision with same epoch; access reductions increment both. Client clears local request generations on either, clears shared payload on epoch/removal/resync/disconnect and revalidates. Initial stream.snapshot/resync re-reads current member project list; no project payload is inserted into run snapshots. Normal GET/preview response envelopes include current project revision/epoch/member generation for stale-reply comparison.

## Exact ownership proposal and shared files

Feature-owned new paths:
- `hosts/rust-daemon/src/projects/{mod,records,registry,service,access,context,tests,access_tests,context_tests}.rs`
- `hosts/rust-daemon/src/workspace_file_preview.rs`
- `hosts/rust-daemon/src/routes/projects.rs`, `routes/tests/projects.rs`
- `packages/sdk/src/projects.ts`, `projects.spec.ts`
- `apps/web/src/hooks/useProjects.ts`, `useProjects.test.tsx`
- `apps/web/src/components/projects/{ProjectsSidebar,ProjectHome,ProjectEditor,ProjectSources,ProjectOutputs,ProjectWorkflows,ProjectMembers,MoveChatDialog,ProjectContextDetails}.tsx`, adjacent tests, `projects.css`
- `apps/web-e2e/src/console/projects.spec.ts`

Exact existing shared paths needing coordinator assignment/order; adjacent tests follow their owner:
1. Daemon `src/main.rs`, `state.rs`, `control_plane_store.rs`: registration, registry/publication init, snapshot/restore/agent deletion, one combined migration.
2. `src/sessions/mod.rs`, `sessions/views.rs`, `routes/sessions.rs`, `routes/contracts/sessions.rs`, `state/session_state.rs`: membership, own-agent filters and atomic creation/moves.
3. `src/agent_files/runtime.rs`, `state/run_commit.rs`, `agent_runs.rs`, `agent_runs/compact.rs`, `sessions/context.rs`: combined admission/budget/accepted bytes/live lease.
4. `src/runs/ledger.rs`, `runs/turn.rs`, `agent_runs/recovery.rs`, `routes/contracts/runs.rs`: host witnesses, safe reports and continuity hooks.
5. `src/routes/workspace_files.rs`, `routes/mod.rs`, `routes/capabilities.rs`: preview service delegation/route/OpenAPI/capability.
6. `src/live/events.rs`, `state/live_state.rs`: metadata invalidation fanout. Existing per-agent publish should avoid changing live/fanout.rs.
7. `src/state.rs` session_model_adapter composition, `src/state/run_commit.rs` bindings, and `src/tools.rs` ToolExecutionContext::live_checks/execute_tool_inner (including its post-approval/pre-handler check): bind/check the access lease using a new projects/access.rs adapter. Preserve handler custody and action-journal settlement after entry; revocation must not abandon a real in-flight effect. No execution-budget authority change is proposed.
8. SDK `src/{client,index,sessions,runs,capabilities,events}.ts`: exports, membership/report/event contracts.
9. Web `src/ViewHarness.tsx`, `lib/hash-route.ts`, `lib/daemon-api.ts`, `hooks/useCompanionSessions.ts`, `hooks/useAgentEvents.ts`, `lib/session-events.ts`, `components/sessions/{SessionSidebar,SessionView,SessionContinuity}.tsx`, `components/WorkspaceShell.tsx`.
10. `apps/web-e2e/src/console/fixtures/{api,state}.ts` if shared fixture adapter additions are assigned.

Coordinator owns combined version bump from inspected 32. Continuity owner integrates capture_run_witness/validate_queued_witness/validate_active_lease/context_change_summary, preserving receipts/budgets. Document owner agrees preview extraction; unsupported uploads/proposals are not automatically shared. Account search gets no automatic project enrollment/backfill/grant widening. No package/Cargo manifests, credentials, browser authority, portable checkpoint schemas, results-panel internals or generated inventories are proposed.

## Required verification

Synthetic agents A/B/C, fake providers/history and temporary roots:
- A+B share same project; C cannot list/read/preview/inject/link/move. Wrong profile/helper/deleted members deny.
- B gets only pinned A project messages/output, B identity/memory; A private transcript/memory/config/tools/adjacent/system/tool/attachment sentinels stay absent. Ordinary B recall cannot read A using projectId.
- Same-ID cross-agent collisions, wrong project/generation, removed donor/requester and forged selectors deny before hydration.
- Barrier revoke during file/history hydration/serialization/fallback prevents late publication/new attempt; re-add never revives callbacks. Retain legitimate usage/receipts from pre-fence work.
- Stale UI responses cannot repaint after epoch invalidation; resync/reconnect revalidates. Viewer mounts no project reads.
- CAS/replay/save failure/uncertain fence; old snapshot defaults/exact backup; JSON/PostgreSQL parity; restart restores no leases.
- Delete detaches all members' chats, retains native stores/effects; agent removal leaves project/other members; archive/delete idle conflict.
- Context cap/whole instructions/UTF-8/order/omissions/dedup/once-only accounting; same bytes through compaction with live revocation; stale recovery never auto-runs.
- Desktop/narrow synthetic full flow: A+B project, A pins/B uses, member switch/reload, tab conflict, pending-read revocation, re-add without old chat/lease revival, archive/delete retained content.

Native verification remains assigned to the coordinator's native driver: repository `bun run rust:lint` and full `bun run rust:test` sequentially with the shared 32 GiB cap. Source review and browser fixtures do not prove native hydration, dispatch, persistence or restart behavior. Coordinator owns combined validation and failure comparison.

## Observed validation (2026-10-28)

| Check | Verified outcome |
| --- | --- |
| Earlier focused SDK project/session checks | 28 passed; SDK build passed. This is not a full SDK-suite result. |
| Independent source/runtime review | Clear, as reported by the coordinator. This is source review, not native execution. |
| Project Vitest checks in the final full web run | 19 passed: ProjectHome 4, ProjectSourcePicker 2, useProjects 4, hash-route 7, SessionContinuity 2. |
| Project static browser checks | Coordinator reported all 4 passed in `apps/web-e2e/src/console/projects.spec.ts`: two journeys at 1280×900 and 390×844, with both document and project-container overflow assertions. They cover navigation/reload/private draft retention and invalidation clearing a cached preview; the broader synthetic/native acceptance flow above is still unverified. |
| Final full web suite | **Red: 2,284 passed, 165 failed, 1 skipped**, 2,450 tests across 204 files; exit 1. Selected feature suites totaled 143 passed, 0 failed. One unhandled rejection remains: Alpha disconnected at `apps/web/src/ViewHarness.test.tsx:748`; no worker startup error was observed. |
| Web typecheck and lint | Both passed after the final full suite; lint reported warnings. The earlier production build passed and was not repeated for test-only changes. |
| Native compilation/tests/lint | Pending. This chat ran no Cargo/Rust compilation, test or lint commands. Authored native regressions are not claimed passing. |

The final full run had 112 passed and 36 failed in ViewHarness; all eight corrected detached-textarea cases passed. The preceding focused run had 116 passed and 32 failed. The fixture correction was limited to the existing openLiveSession helper and eight direct textarea captures, preserving behavior assertions. No broader source/test rewrites followed; the remaining failures are unresolved, not classified wholesale as baseline failures.

Evidence: [final full web report](../../../apps/web/test-output/vitest/project-combined-web-final.json), [exact failure comparison and selected feature results](../../../apps/web/test-output/vitest/project-combined-web-final-triage.json), and [focused ViewHarness comparison](../../../apps/web/test-output/vitest/project-view-harness-after-eight-triage.json). Reports remain in configured test-output storage. Live-provider, real-account and credential checks remain outside the authorized scope; static browser checks start no application server. Release acceptance is incomplete while native verification is pending and the full web suite is red.

## Review tradeoffs

Shared multi-agent membership is the settled human choice. Proposed v1 tradeoffs: exact owner pins, private native agent stores, link-only native workflows, supported local previews and conservative cancellation of all project replies on membership/access reduction. Coordinator reviews these integration choices without another human implementation approval. Remote sources, retroactive erasure and workflow-context inheritance are separate scope.
