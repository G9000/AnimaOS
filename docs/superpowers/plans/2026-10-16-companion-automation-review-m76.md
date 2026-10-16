# M76: Review interrupted automation runs

## Goal

Give the owner a clear, safe review path when an automation occurrence was interrupted during daemon shutdown. Keep the existing explicit **Run now** and **Resume** controls; do not add replay, acknowledgement state, backend/SDK changes, grants, or a new action.

The scheduler already records `schedule_run_interrupted` and disables schedules whose effects are uncertain (`hosts/rust-daemon/src/schedules.rs:596–659`). The UI should identify that outcome as **Needs review**, explain that some actions may already have happened, and direct the owner to inspect the recorded thread and its effects before using the existing controls. The warning belongs to the historical occurrence and remains visible if a later occurrence succeeds.

## Reuse and boundaries

Reuse Anima's persisted automation history and session identifiers, existing history drawer, and existing Run now/Resume controls. Do not synthesize a session ID: when an interrupted occurrence has no saved `sessionId`, show that no saved thread is available. Ordinary failures and stopped outcomes keep their current meaning.

Pinned public Hermes source `1fd18277267152893a8a24792b79213e7ca3f899` is comparison material only. Do not port its Python scheduler, add a dependency, or inspect profiles.

## Files and ownership

- `apps/web/src/components/automations/AutomationHistory.tsx` and new `AutomationHistory.test.tsx`: show the interrupted-occurrence warning and thread review link; make drawer data scope-safe and fresh.
- `apps/web/src/pages/AutomationsPage.tsx` and `AutomationsPage.test.tsx`: pass the current online state to the drawer and verify the owner-facing review action and warning.
- No daemon, SDK, grant, scheduler, or other file changes.

Two web lanes may work independently: drawer behavior/tests and page integration/tests. Freeze the UI contract before implementation and keep edits within the assigned files.

## Rules

1. Classify an occurrence as interrupted only when its status is `failed` and its exact error code is `schedule_run_interrupted`; map it to **Needs review** and plainly warn that prior actions may have happened. The SDK may project the schedule's latest outcome as `error`, while the history occurrence remains `failed`. Keep the warning attached to that historical occurrence even when later schedule runs succeed.
2. Review opens only the occurrence's actual saved `sessionId`. If it is null, state that no saved thread is available. Review itself must not run, resume, retry, acknowledge, or alter the automation.
3. Before any existing Run now or Resume control is used, the owner should review the prior thread and effects. Do not disable or replace those established controls as part of M76.
4. Preserve the current display and actions for ordinary failed/stopped outcomes.
5. Key drawer requests and displayed data/errors by a collision-safe snapshot of `[agentId, automationId, version, online]` (for example, `JSON.stringify` of that tuple). Prefer a keyed inner history-content component so old rows disappear synchronously when the scope changes, including offline-to-reconnect, while keeping the aside/header/Close control mounted outside it where practical. Abort the prior request and ignore late success and error results.
6. Offline drawer state performs no history GET. Reconnection triggers a fresh GET for the current key. Close remains available in loading, error, empty, and offline states.

## Tests

Add RED tests before implementation. In `AutomationHistory.test.tsx`, cover:

- exact interrupted reason gets a persistent **Needs review** warning, including when the history also contains a later successful occurrence;
- review opens exactly the saved session ID; null session ID yields truthful no-thread text and no fabricated navigation;
- ordinary failure/stopped labels remain unchanged;
- switching agent, automation, or version hides old data immediately, aborts/ignores late success and late error, and shows only the current response;
- offline opens no request and shows no stale data; reconnect reads fresh history; Close works in each state.

In `AutomationsPage.test.tsx`, cover the review entry point for an interrupted occurrence, the warning before use of existing Run now/Resume controls, and unchanged ordinary outcome actions. Assert that opening/reviewing history causes no run, resume, acknowledgement, or mutation call.

## Validation

Use the existing Nx project target names and verify flags from project configuration/help before running:

- focused automation history and page tests;
- full `@animaOS-SWARM/web` test, typecheck, and build targets;
- Prettier check for the four owned files;
- scoped documentation link/path checks for the plan, master status, handover, parity roadmap and daily-use guidance.

Rust and SDK gates are out of scope. Root owns all commands, formatting, Git, integration, and publication.

## Top five risks

1. Treating an uncertain interrupted outcome as safe to replay could duplicate external effects; keep review informational and preserve explicit existing owner actions.
2. Losing the warning after a later success would conceal the older uncertain occurrence; render it per occurrence, not from only the automation's latest summary.
3. Inventing or deriving a thread ID could open unrelated history; use only the stored `sessionId` and handle null explicitly.
4. A late history response could show another automation's data or stale errors; abort and generation/key-check every completion, and hide mismatched data synchronously.
5. Changing ordinary failure/stopped semantics or adding backend state would broaden the feature and risk existing callers; limit changes to the four UI files and the exact interrupted reason.

## Review and completion

Root runs RED, implementation gates, formatting, and docs checks; review the frozen UI/SDK contract once, make one justified fix wave, then rerun affected gates. Record source revision, test results, and limitations in the private M76 ledger. Do not claim broader recurrence, exactly-once effects, or host recovery acceptance from this UI change.

## Controller rulings on the risks (binding)

1. Ruling: interruption review is read-only — existing Run now and Resume remain separate explicit owner actions; no automatic replay, acknowledgement or inferred rollback — cost if wrong: duplicate or hidden effects could follow an uncertain run.
2. Ruling: classify only exact recorded interruption outcomes — schedule status error/history status failed plus schedule_run_interrupted, while later successes never erase an older occurrence's warning — cost if wrong: ordinary failures or owner stops could be misrepresented.
3. Ruling: navigate only a stored occurrence session ID — null means no saved thread, with no fabricated schedule/session fallback — cost if wrong: an owner could inspect unrelated evidence.
4. Ruling: key the inner content to agent, schedule, version and connectivity — hide old rows synchronously and ignore aborted completions; offline makes no GET and reconnect loads anew — cost if wrong: stale or foreign occurrence evidence could be shown.
5. Ruling: keep the feature web-only and reuse existing dispatcher authority — no daemon/SDK mutation or extra execution button, with one final contract review and affected web gates — cost if wrong: UI claims could diverge from scheduler behavior.

Reuse decision/provenance: inspected pinned public Hermes commit
1fd18277267152893a8a24792b79213e7ca3f899, cron/executions.py and
cron/scheduler_failure_copy.py. Its execution ledger distinguishes known attempt
evidence from retry scheduling; failure copy points owners to saved runs and
explicit actions. Reuse that behavior through Anima's existing durable outcomes,
SDK history API and React drawer. Direct Python copy or a scheduler process
adapter would duplicate a working Rust subsystem and add lifecycle cost. No
literal code port, new dependency or upstream profile access; existing notices
remain. Preserve historical raw cases and pinned version evidence using the
M75 build snapshots plus pre-change M76 owned-source snapshots. No actual case.
