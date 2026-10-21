# Workflows in companion chat

The owner requested creation, checkpoint approval, and question answers inside chat, an explicit permission toggle, and live browser acceptance. This extends the existing sequential workflow engine; no new runtime or checkpoint authority is needed.

## Design

- Add **Allow workflows in chat** to Permissions. It grants `create_workflow` and `list_workflows` together, preserves unrelated tools, supports revocation and profile changes, and is unavailable for helpers. The default profiles remain opt-in.
- In ordinary web chat, render an agent-scoped **Workflows** section at the end of the transcript, inside its scroll area. It uses the existing persisted workflow list and shared workflow cards. Anima creates workflows from natural-language requests using the granted tool. Owner-operated buttons approve checkpoints; an inline answer field submits question answers. These controls call the existing owner API directly and do not let the model make owner decisions.
- Reuse the existing workflow controller and cards for Work and chat. Chat hides the manual creation form, explains the permission prerequisite, and refreshes after chat activity as well as bounded polling while tasks run. Terminal workflows are collapsed in chat so the transcript stays usable. A workflow belongs to the selected agent, and the section says so; it is not falsely labelled as originating in this session.
- Preserve exact saved requests, revision conflict handling, offline disabling, agent/lifecycle guards, uncertain-result review, and cancellation semantics. Update existing model-facing workflow descriptions to point to the owner's inline chat controls as well as Work. No new model resume tool, automatic permission grant, or new schema/migration.

The alternative of adding model-callable approval/answer tools was rejected because generated text would then decide owner checkpoints. A separate chat-only workflow implementation would duplicate the durable decision and replay rules.

## Acceptance

1. The permission toggle grants/revokes both tools and preserves other grants, including across profile selection.
2. A natural-language chat request creates a real workflow and its saved card appears without visiting Work.
3. In chat, approve a checkpoint, answer a question, and observe final completion using the earlier saved result.
4. Reload at a checkpoint; keep the same job/result/attempt. Repeat a lost-response decision with the exact payload. Reject stale revisions and prevent offline or cross-agent actions.
5. Cancellation and review controls remain available through shared cards. Disabling chat creation does not revoke the owner's access to saved workflow controls.
6. Focused unit/integration checks, relevant web typecheck/build, and browser acceptance pass. No external test sends or source commits are part of this request.
