# Conversation controls

Each direct web chat has **Model & thinking**, **Tools**, **Second opinion**, **Reports**, and **Share** controls above the conversation. These controls apply to the selected chat.

## Model and thinking

The composer model chip and Model & thinking panel set the primary model for this conversation. **Inherit** follows the companion's defaults. An explicit selection saves the provider, model and supported reasoning effort without changing those defaults. The panel lists available configured models and only offers reasoning efforts supported by the selected provider/model.

Selections apply when the next turn is durably admitted. A running turn keeps its admitted model; a queued turn resolves its selection when it starts. Helpers inherit that admitted model. Changing a selection does not change tool permissions, approvals or execution limits. Provider fallback behavior remains governed by the existing companion configuration.

Use `/model` to open the panel, `/model inherit` to restore inheritance, or `/model <provider> <model> [effort]` to request an explicit selection. Invalid or unavailable selections return an error.

## Interactive results and reports

Anima can use `show_result` to save a chart, comparison table, document, calculator, editable tracker or dashboard in the current owner chat. Results appear inline and survive reloads. **Pin report** keeps a result beside the chat on wide screens and below it on narrow screens. Pinning is durable. Anima can use `list_results` to find a result and read its current revision before updating it.

Charts support series toggles, comparison tables support filtering, and calculator inputs update a closed arithmetic expression locally. Documents render text or Markdown. Trackers have editable text, number and checkbox columns; dashboards combine up to eight charts, tables, documents, calculators or trackers. Dashboards cannot contain other dashboards. Save your tracker edits to keep them after reopening the conversation; unsaved changes remain local drafts. Calculator input changes stay local to the preview. The JSON export contains the saved structured result.

Try “Build a weekly project dashboard with a task tracker, a cost calculator and a progress chart,” then “Add an owner column to that tracker and keep my current entries.” Anima reads the current saved revision before refining the result. Save local edits before asking for a refinement so the companion can read them.

A result contains structured data and closed arithmetic. Generated JavaScript, executable HTML and embedded frames are unsupported. There are at most 32 saved results per chat, with a 32 KiB payload limit and a 256 KiB combined conversation result limit. Trackers allow 12 columns and 100 rows; their stable keys and typed cells are validated by the daemon.

Updates and pin changes use optimistic revisions. A stale update returns a conflict instead of overwriting a newer result. Request keys make retried saves idempotent. Results carry their source run and tool-call IDs and remain bound to the originating agent, conversation and clear generation. Clearing a chat removes its results.

## Second opinions

Choose a separate reviewer in Model & thinking, then request a second opinion on the latest saved answer. `/review [focus]` requests the same operation with an optional focus. Opening the panel alone does not call a model. The API can also select a saved answer by message ID.

The reviewer receives the saved answer, its preceding owner prompt and bounded status summaries of recorded actions. This is limited evidence, not independent confirmation that an action succeeded. Quoted content is treated as untrusted. The reviewer has no tools, does not run the answer again, and cannot modify the primary model or perform an action. A model alias or another transport for the same underlying model does not qualify as an independent reviewer.

Review records show running, completed, failed, cancelled, unavailable or interrupted status. They support explicit cancellation. Each provider attempt has a 60-second deadline; at most two physical review attempts run concurrently, within the existing shared execution budgets. Findings are bounded. A restart marks unfinished reviews interrupted and does not replay them. Clearing or deleting the conversation cancels its review work. A completed review is advice, not an approval.

## Effective tools

The Tools panel and `/tools` show the tools selected by the execution path, their source, availability and approval policy. When idle, the list describes the next owner turn. During a turn, it uses that turn's admitted descriptor ceiling intersected with current grants and connection readiness. New grants do not expand a running turn; revocation can remove availability.

Approval policies can depend on arguments, and normal execution safeguards still apply. The display is informational. It does not grant authority. Connection entries expose labels and states without endpoints or credentials.

## Read-only sharing

Share creates a separate viewer grant for this conversation. The owner receives a non-secret link, viewer ID and secret. The secret is shown once; give the viewer their credentials separately. Opening the link requires those credentials. The viewer's browser must be able to reach this Anima installation; creating a grant does not configure hosting or network access.

Sharing requires durable daemon storage and an owner API key configured with `ANIMAOS_RS_API_KEY`. The owner web console asks for that key when required and holds it only in page memory. Viewer credentials use a separate transport and never authorize owner APIs.

Viewers see only user and assistant text, including future messages until revocation or a chat clear. Rejected revised drafts, tool output, internal metadata, attachments, private files, interactive results and review records are omitted. The viewer page has no owner bootstrap, composer, tools or execution controls. Secrets are sent in headers and persisted only as digests; they are not included in links. Revocation is checked on every read, including after a history load. Clearing the chat ends existing grants. Issue a new grant to restore access.

## API and persistence

Owner routes are under `/api/agents/{agent_id}/sessions/{session_id}`:

| Suffix | Operations |
| --- | --- |
| `/model-preferences` | GET effective selections; PATCH primary and/or reviewer |
| `/capabilities` | GET effective tools and connection states |
| `/results` | GET saved results; POST a result with clear generation |
| `/results/{id}` | PATCH pinned state with clear generation and expected revision |
| `/reviews` | GET review records; POST a bounded review request |
| `/reviews/{id}/cancel` | POST explicit cancellation |
| `/shares` | GET grants; POST a viewer name and required clear generation |
| `/shares/{id}` | DELETE a grant |

Viewer-only GET routes are `/api/shared/conversations/{share_id}` and `/api/shared/conversations/{share_id}/messages`. Viewer headers are rejected on other routes. Message pagination accepts only a cursor belonging to the authorized conversation.

The TypeScript SDK exports `ChatModels`, `ChatCapabilitiesClient`, `ChatResultsClient`, `ConversationSharesClient` and the separate `SharedConversationClient`.

Snapshot version 29 stores conversation preferences, results, reviews and viewer grants. Upgrading version 28 preserves the exact prior snapshot as `.pre-chat-suite.bak`. Older daemons refuse the newer format. Restore procedures must use the corresponding backup rather than rewriting its version field.
