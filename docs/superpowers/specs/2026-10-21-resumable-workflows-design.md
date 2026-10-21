# Resumable workflows

## Outcome and scope

An owner or an ordinary companion conversation can start a saved sequence of agent tasks, approval checkpoints, and questions. Completed step results stay saved. Approval and input checkpoints survive restart; continuing advances the existing workflow rather than starting over. An interrupted or failed task requires explicit owner review. No automatic retry of uncertain effects is allowed.

Implement in the existing main checkout. Leave all changes unstaged and uncommitted. Reuse Anima's Rust job dispatcher, run coordinator, permissions, action receipts, snapshot transactions, SDK client, and Work hub. Study the local OpenClaw workflow behavior; do not import its JavaScript runtime or copy upstream text. No new attribution files or changes to existing notices.

## Approach

Use a native workflow state machine in `anima-core`, with daemon orchestration over the existing durable job service. Each task step creates one existing agent job. A workflow only advances when that exact job finishes successfully with a complete saved output. This preserves the existing tool grants, owner policies, stop controls, process lifecycle, and run receipts. Approval of a workflow checkpoint never grants additional tool permissions or approves a connector's separate pending send.

Alternatives considered: a second external DSL/process runtime would duplicate custody and persistence; a large live model turn would keep approvals tied to one process and lose exact step progress. Native saved job steps fit the current product and are the selected implementation.

This first version is a sequential agent-task workflow, not a shell scripting language, DAG engine, or zero-model-call tool pipeline. No new dependency, cloud service, automatic schedule, or real-account operation is needed.

## Public contract

All fields use camelCase; enum values use snake_case. A step specification is `{ id, title, kind, prompt }`, where kind is `task`, `approval`, or `input`. Task prompts are instructions; checkpoint prompts describe the exact decision or question. Step IDs are unique within the immutable definition.

- `GET /api/agents/{agentId}/workflows` -> `{ workflows: WorkflowRun[] }`.
- `POST /api/agents/{agentId}/workflows` with `{ title, requestKey, steps }` -> `WorkflowRun`. Exact repeated agent/requestKey/input returns the same workflow; changed input conflicts.
- `GET /api/agents/{agentId}/workflows/{workflowId}` -> `WorkflowRun`.
- `POST .../{workflowId}/resume` with `{ revision, action, value?, acknowledgeUncertain? }` -> `WorkflowRun`. Actions: `approve`, `answer`, `retry`, `resolve`. Exact replay of a saved decision is a no-op returning current state; changed decisions at the same revision conflict.
- `POST .../{workflowId}/cancel` with `{ revision }` -> `WorkflowRun`.

Reads and mutations use the existing local-owner guard, with mutations checking Origin before body parsing. Cross-agent identities return not found. Responses are no-store. Invalid input is 400, stale decisions/idempotency/state/capacity conflicts are 409, unavailable persistence is 503.

`WorkflowRun` contains `id`, `agentId`, `title`, `requestKey`, `status`, `revision`, `currentStep`, `steps`, `decisions`, `createdAtMs`, `updatedAtMs`, and nullable `error`. Each saved step includes its specification plus `status`, nullable `jobId`, `attempt`, `attempts` (historical job IDs), and nullable `result`. Workflow statuses: `running`, `awaiting_approval`, `awaiting_input`, `needs_review`, `completed`, `cancelled`. Step statuses: `pending`, `running`, `awaiting_approval`, `awaiting_input`, `needs_review`, `completed`, `cancelled`. Saved decisions include the exact server-recorded `stepIndex`, source revision, decision time, and request, retaining step-specific evidence for safe request replay.

## Execution and recovery

Creation atomically saves the immutable definition. The existing job dispatcher reconciles workflows before claiming jobs, queues at most one task for a workflow, and atomically saves the step/job linkage. It rechecks workflow state before claiming a linked queued job. Tasks run under their current agent configuration. Helpers and jobs cannot recursively create workflows. The task prompt includes bounded prior completed results as labeled untrusted data, with explicit omissions when the context budget is reached.

Save the queued job and workflow link together before dispatch. When its result is durable, atomically record that result and advance to the next task or checkpoint. A completed task is never dispatched again by resume, restart, duplicate requests, or worker reconciliation. Failed, cancelled, interrupted, stopped, truncated, or uncertain job results put the workflow in `needs_review` and do not dispatch a later step.

Each linked job has `maxAttempts: 1`. All current and historical workflow job IDs form a unique reverse linkage checked during restoration. Generic job retry/approve/cancel/review mutations refuse linked jobs and direct the owner to workflow controls, so neither old attempts nor generic retries can bypass workflow state or its three-attempt limit. Dispatcher claim checks that the workflow still authorizes this exact step/job. This first version disables helper spawning and delegation for workflow task runs; one tracked ordinary job owns each task's execution. It cannot create pending asynchronous origin chains.

Workflow progression observes only durably settled job records in its own transaction. Ordinary job/run completion callbacks do not mutate workflows, so their existing rollback cannot accidentally revert a later owner decision. If the workflow save fails, its completed job remains settled and the next reconciliation can safely observe it again.

`approve` completes only the current approval checkpoint. `answer` requires nonempty bounded text and completes only the current input checkpoint. `retry` requires `needs_review`, explicit `acknowledgeUncertain: true`, and remaining step attempts; it retains prior job identities and creates a fresh attempt, never mutating old receipts. `resolve` requires the same acknowledgement and a nonempty owner-supplied result, recording that owner resolution before advancing. Maximum three task attempts per step.

Cancellation prevents all future task admission and cancels a linked queued job in the same save. For an already running job, persist cancellation first, then request its existing owner-stop path. A result racing cancellation cannot advance the workflow. Shutdown/drain retains the existing dispatcher behavior. On restore, validate definitions, transitions, decision revisions, IDs, and job ownership/linkage before use. Existing running jobs reconcile to uncertainty; pending approval/input and successful steps remain intact. Reject malformed or oversized snapshots rather than silently repairing or forgetting workflow progress.

The complete cancel-save-then-stop operation runs to completion independently of a dropped HTTP response; durable reconciliation also retires any still-live task of a cancelled workflow. Deleted agents retain workflow/job audit history, matching existing jobs; restore accepts consistent historical links for missing agents, owner APIs return not found, and dispatch skips the deleted owner.

## Persistence and bounds

Extend the existing JSON/PostgreSQL control-plane snapshot, version 24, with default-empty workflows and an exact pre-upgrade `.pre-workflows.bak` backup. Older daemons must reject the newer state version. Never touch the user's active runtime store during tests.

At most 50 workflows in retained history, 16 steps per workflow, 64 ASCII alphanumeric/underscore/hyphen bytes per nonempty step ID, 160 characters per title, 128 bytes per request key, 8 KiB per step prompt, 32 KiB total definition text, 8 KiB per owner input, 64 KiB per result, three task attempts per step. Existing global job history (200) and active job (8) limits still apply. Temporary active-job capacity leaves the workflow running with a visible waiting message and no admitted job; the next dispatcher pass checks capacity again. Exhausted retained job history sets `needs_review` with no new attempt, permits owner retry with acknowledgement after capacity is available, and never evicts records or dispatches untracked work. No running model/tool execution happens under the state or persistence transaction lock.

## Owner and agent surfaces

Add a Work -> Workflows tab following the current Work hub styles. Owners can choose an agent, create an ordered workflow using task/approval/question steps, inspect saved results and linked jobs, answer/approve the current checkpoint, retry or resolve a reviewed task with explicit effect acknowledgement, and cancel. Show honest offline/loading/error states and refresh from persisted state after a mutation; suppress stale responses when agent membership/connectivity changes. Reuse existing UI controls; no new visual system.

Expose typed `client.workflows` methods and corresponding daemon facade methods. Add `create_workflow` and `list_workflows` tools for explicitly granted ordinary companion runs; derive owner agent identity from the run, reject helper/system contexts, and never allow a model to approve, answer, retry, or resolve checkpoints. Continue/approve actions remain owner API/UI operations. Register tools in discovery and appropriate permission classifications; do not silently broaden existing grants.

## Validation

Write meaningful RED tests before implementation: core progression and decision replay; transactional step/job linkage; restart at approval/input; an observed completed first task is never rerun after resume; uncertain step blocks later dispatch; concurrent/stale decisions; owner/cross-agent authorization; snapshot upgrade/backup/invalid state; cancellation race and persistence failure rollback. UI tests cover owner checkpoint actions and offline/stale response handling; SDK tests cover encoded paths and exact payloads.

Run required `bun x nx run rust-daemon:test --skipNxCache` in the current tree and relevant SDK/web test/typecheck/build and docs build targets. Focused filters establish development feedback, not full acceptance. Distinguish deterministic synthetic workflow evidence from live provider or real-mail acceptance. No real sends, active Hermes changes, service restarts, Git staging, or commits.
