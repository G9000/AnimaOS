# Chat workflow visibility

The owner approved hiding completed/cancelled workflow history and template management from the main chat. Chat should display only running workflows, approval/input checkpoints and uncertain tasks needing owner review. When none remain, render no workflow heading, manager, template picker or empty-state panel. Preserve owner decision retries and meaningful loading errors.

Keep the existing full workflow list, creation form and reusable template management under Work -> Workflows. Do not delete or mutate saved history or change execution/permission semantics. Apply filtering and conditional UI in the shared WorkflowAgentView; the ChatWorkflows wrapper delegates without static management copy.

Verification: regressions for history-only and empty chat, mixed statuses, completion/cancellation transitions, activity refresh, approval/answer/review controls, exact retries and unchanged Work history/templates; Nx focused web tests, typecheck/build, docs build, then real browser checks of clean chat and owner checkpoint completion. Stay on main; preserve unrelated work and leave the fix unstaged.

Work and chat may stay mounted together. Successful owner mutations and template runs must invalidate all mounted views for the originating agent, even if the source view was closed or changed agents while the response was in flight. Local view updates remain guarded against stale context.
