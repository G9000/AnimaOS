# Click-to-edit previews

Date: 2026-10-29. The owner authorized targeted edits from previews together with connected project sources and requested subagents. Base: `170207f3274f0bd01dabfd4364f003a1ad6dbde1`.

## Outcome

The owner selects supported content in the existing preview, adds that exact selection to the current chat as a removable context chip, and writes an ordinary instruction such as “shorten this paragraph.” Selection never sends a message or writes a file. Sending uses the existing chat; the agent proposes a version-bound edit, and the owner reviews the exact before/after change before applying it. Conversation/composer, preview resize, explicit close and fullscreen close behavior remain intact.

Supported initial selections are exact text ranges from an inert HTML/HTM content view and server-inventoried editable Office targets such as Word paragraphs, spreadsheet cells/chart targets, and slide text. Read-only or unsupported content is labelled honestly. This does not claim pixel-perfect Office rendering, arbitrary PDF editing, or universal graphical chart manipulation.

## Selection boundary

Use a bounded versioned `PreviewEditContext`:

```text
version: 1
source: { kind: workspace, path, sha256, scope }
selection:
  { kind: html_text, start_utf8, end_utf8, expected_text }
  | { kind: office_target, target_id, label, value }
```

The path is a current workspace-relative source, at most 1 KiB; exact selected text is at most 8 KiB. HTML is at most 1 MiB. The selection is quoted data, never executable instruction text. Context is scoped to the current agent, session, clear generation and preview version. Close, reload, another artifact, or an authority/scope change invalidates the chip and pending selection operations. External source changes are rejected by hash/CAS at proposal and apply.

HTML executable previews keep their opaque sandbox. Do not accept selection messages from their running scripts as source proofs or remove iframe sandboxing. A distinct opt-in inert content mode renders safely escaped original source text runs with exact UTF-8 source offset mapping; scripts, styles, comments and unsupported offsets are not selectable content. Raw selected source text preserves entity spelling. Displayed values may never manufacture workspace paths, authority or editable ranges.

Office selection uses the server's paged source inventory: exact SHA256, opaque target ID, editable flag and current typed value. Selection buttons identify whole editable targets without pretending the content inventory is a page-rendering engine. A read-only owner source-inspection operation must retain current profile/workspace/session custody and never manufacture a model run or tool grant.

## Draft and review flow

Keep the user's literal draft untouched. Render a removable selection chip immediately above the existing composer/attachments and focus the composer after explicit “Add to draft.” Empty instructions cannot be sent solely because a chip exists. Serialize the bounded selection as quoted structured context only on the owner's ordinary send. Prompt guidance directs the model to inspect the current source, use native document proposals and wait for the exact review/apply approval. The chip is not a write grant.

Existing Office proposal/review/apply machinery retains exact AllowOnce approvals, source/destination hashes, cancellation, candidate lifetime, receipts, checkpoints and uncertain-effect recovery. Extend that same custody pipeline for HTML text edits rather than relying on generic `edit_file` auto-allow behavior. HTML replacement validates source SHA256, exact UTF-8 boundaries, expected bytes, nonoverlapping bounded operations and untouched-byte preservation. New text and raw markup must have distinct semantics; text replacements escape HTML where appropriate. The owner sees complete bounded before/after changes before publication. Stale source/destination or changed authority requires a new proposal/review. Existing checkpoint recovery remains the available undo mechanism; do not advertise automatic rollback where it is unavailable.

## Runtime review boundary

The owner source-inventory response includes `source_scope`, an opaque digest of the server-owned profile, canonical workspace, relative source path and source SHA256. A click selection must copy it into `source.scope`; admission recomputes it against current custody. This fences switching workspaces before Send even when both contain identical bytes at the same relative path. Ordinary published Office inventory pages may omit it, but selection requires a current owner-source page.

Prompt guidance alone does not enforce review: existing generic write/process grants can bypass a document-specific tool. Add optional typed `documentEditContext` to ordinary owner web run admission and persist the complete versioned context with the native run. Admission validates the current exact source/selection and includes context in idempotency identity. Queued work, automatic retries, failed-send restoration and recovery retain this field; invalid context is refused rather than downgraded to an unrestricted send.

For a native run carrying this context, central tool dispatch admits only `inspect_office_document`, `propose_document_edit` and `apply_document_edit`, still subject to existing tool grants and exact apply approval. Generic filesystem writes, process execution, delegation and dynamic tool escalation are not available on that dedicated edit run. Proposals bind the selected source/hash and supported range/target. Steer cannot introduce/remove/change the constraint on an unrelated active run; send a new queued edit run instead. Native enforcement reads the stored run, never arbitrary prompt text or client message metadata. A host-owned durable scope witness also binds the profile, canonical workspace, session clear generation and project witness, so an identical file in another workspace cannot replace the selected source across queuing or recovery.

HTML uses the additive `html` format, native `html_text` targets with absolute UTF-8 offsets, and `html_replace_text` operations. Expected text retains raw source entity spelling; replacement `new_text` is literal text escaped for HTML. At most 128 nonoverlapping operations fit one admitted selected text range. The existing 64 KiB complete-review limit is an explicit refusal boundary, not permission to omit changes.

## Validation

Native tests cover Unicode/entity/range handling, hash mismatch, overlap, bounds, untouched bytes, candidate/review binding, exact approvals, mutation races, cancellation, physical worker custody and checkpoint/restart behavior. UI tests cover inert HTML selection, hostile markup/frame messages, exact Office target selection, stale source/scope responses, preserved drafts, no automatic sends/writes, keyboard operation and invalidation. Browser checks on desktop and mobile exercise selection → draft → explicit send → exact proposal review and retained preview/fullscreen close behavior.

Run full wrapped native tests/lint, SDK test/build/typecheck/lint, focused web tests and web build/typecheck/lint. The final report states actual supported formats and validation boundaries. Do not restart the user's active daemon, publish files to external systems, commit, or push without separate authorization.
