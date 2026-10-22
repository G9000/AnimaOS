# TUI approvals and clarifications

Date: 2026-10-22
Status: Implemented and validated on 2026-10-23.

## Outcome and approach

Add a current-session review queue to persistent chat. The preferred approach is an attention count plus an explicitly opened review pane: it preserves composer focus and gives each decision enough room. Alternatives are inline actionable cards (compete with the composer for keyboard input) or a global approvals/settings screen (more navigation and a larger scope).

`/review` opens the combined queue; `/approvals` and `/clarifications` filter it. Ctrl+R opens review while preserving the current chat draft. Escape returns to chat. Incoming requests never steal focus. The chat component stays mounted with its input inactive while review is open. The queue is oldest first, with stable IDs and separate detail forms. No approval key is active in the ordinary composer.

## Approval form

Show tool, risk class, owner agent/session/run, reason, expiry, literal arguments, truncation, and revision. Escape terminal control codes and hidden direction/zero-width characters in request text. Tab/Shift+Tab moves fields; arrows change enumerated values; text fields accept typing. The decision choices are Allow once, Allow for session, Always allow, and Deny. Scoped decisions include an editable matcher, restricted to the daemon's matcher kinds, with an explicit description of authority. Notes are optional, up to 1,000 characters. Matcher values are at most 512 characters. Truncated arguments or an empty non-any matcher disable scoped grants. Broad scopes receive a visible warning. An explicit confirmation shows the decision, scope and note before sending. No authority is granted by opening or selecting a request.

## Clarification form

Show every question with single-choice, multi-select, or free-text input, plus optional Other text. Single-choice options and free text are mutually exclusive; multi-select may combine both. Tab moves fields, arrows select a choice, Space toggles it, and text is capped at 4,096 characters per question. Unanswered questions are explicitly shown as skipped in the confirmation summary. Submit requires a separate confirmation. Success says the answer was saved; it does not claim interrupted work resumed.

## Client boundaries and synchronization

Add optional `review: TuiReviewApi` to `TuiSessionApi`; the CLI adapter supplies existing SDK approval and interaction operations. Keep execution and authorization in the daemon. No Rust routes or SDK contracts change.

A focused review hook owns reads, pending records, errors, and writes. It consumes approval snapshot/requested/resolved events through the existing agent subscription; it never creates a second stream. Poll both endpoints immediately and every two seconds while a session is active to recover missing events and discover clarifications (which have no SSE event). Abort reads and dispose timers on scope changes, offline transitions, and unmount. Keep the most recent same-session records on transient errors; disable actions for the affected source until a successful refresh.

The review scope is the selected agent/session, plus helper approvals and clarifications whose run ownership is already associated with that same session by the existing stream. Separate helper sessions and workspace-wide policy/rule management are outside this milestone. Pending approvals are read globally and filtered by this exact ownership; clarification reads use each known same-session agent's endpoint. Primary and helper owners are shown explicitly.

Revision-aware merges prevent stale pending reads from restoring resolved requests. Reads begun before a stream/mutation update cannot overwrite it. Session generations prevent late reads or writes from updating a newly selected session. Deduplicate writes synchronously, send the displayed revision, and never retry a decision automatically. On 404/409, block the stale request and refresh. On transient write errors, retain form drafts for an explicit retry. Any new revision invalidates the old form and confirmation. Offline, loading, expired, resolved, stale and in-flight requests cannot submit. Escape can leave an in-flight request; its result remains scoped to its original session.

Conflict invalidation is keyed by request kind and ID, so a background failure cannot reset another request's draft. Reconnect resets readiness until fresh reads succeed. A confirmation for an unchanged pending revision can be rebuilt after an authoritative conflict refresh, including when the daemon rejected a scoped grant because its rule list was full.

## Acceptance

- Open combined/filtered queues from chat and help, return without losing the composer draft, and retain swarm commands.
- Approve once, grant scoped authority with a matcher, deny with a note, and require an explicit confirmation.
- Answer single/multi/free-text questions, skip questions explicitly, and confirm the complete answer payload.
- Cover snapshot/events/polling, expiry, truncation, hidden terminal characters, stale revisions, request disappearance, errors, duplicate submit, offline/reconnect, helper attribution, navigation races and timer/stream cleanup.
- Run the relevant Nx TUI and CLI tests, typechecks and builds in the current tree. Preserve unrelated work and create no commits.
