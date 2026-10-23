# Anima chat controls

The owner has authorized implementation of retry/undo, conversation branching, queue editing and ordering with wait/steer choices, a context inspector, focused compaction, and natural-language rename/compact/export. All controls belong to the native Rust runtime, with thin SDK and web clients. Existing companion identity and saved memories remain shared and unchanged.

## Conversation revisions

Branch copies the selected conversation through the complete user exchange containing an optional message into a new ordinary web chat. Undo opens a revision omitting the most recent user exchange. Retry opens the same revision and restores that exchange's prompt into the composer for an explicit fresh send; edit and resend uses the same draft. These operations preserve the original transcript, action receipts and run evidence. The UI must explain that undo creates a revision and cannot reverse actions already performed. Main-chat identity stays on the original chat. A revision is navigable like another chat and records its source session. Copies get fresh message IDs, do not inherit live run IDs, request IDs, grants or pending interactions, and preserve full source history rather than just the loaded browser window. Attachment bytes stay scoped to the source: revisions mark unavailable files explicitly and retry restores the raw owner input for manual reattachment. Refuse active/queued source runs, pending deletion, helper/channel sources, missing anchors, unavailable history, and oversized copies. A save failure rolls back the new session and messages. Saved memories, other chats and the source remain intact.

## Queue controls

The owner can edit a waiting ordinary web/API prompt, move it relative to another waiting prompt, or cancel it. Never mutate an input already starting, a steer already delivered, attachment scope, a channel admission or a helper/system input. Editing retains durable provenance and idempotency: an old delivery retry must resolve to its original accepted request rather than send an edited prompt twice. Order changes persist and are used by the drainer under the same mutation boundary. Existing queue and steer semantics are exposed as explicit Wait and Steer choices in the composer. Source connector ordering remains unchanged.

## Context and compaction

Expose a context inspection response and UI with estimates for conversation, summary/checkpoint, system instructions, tool schemas, companion files and memory. Label coverage and omitted dynamic provider contributions explicitly; never label an estimate as billed or actual provider usage. Keep the existing history budget meter semantics intact. Compaction accepts a bounded keep-latest exchange count (default one) and optional focus text. Focus is untrusted summarization guidance and cannot discard required safety/task continuity. Full transcript remains exportable. Reject invalid bounds and no-op compaction, keep existing ownership, timeout, persistence rollback and room serialization guarantees.

## Owner tools

Register rename_chat, compact_chat and export_chat for direct owner web/API chat turns only, using exact run-local scope and live revalidation. Tools cannot accept an arbitrary agent or session ID. Do not expose them to helpers, delegated/peer/system follow-ups or connector turns. Rename reuses owner title validation and saves before acknowledgement. Compact reuses the runtime compactor with focus/keep options while respecting the run's existing room ownership. Export uses the full transcript renderer and returns an authenticated relative download URL and filename, not an external upload or a claim that a file was delivered. A normal web export click downloads the Markdown. Existing clear_main_chat remains available only in the main chat.

## Verification

Add behavioral regressions for source preservation and scope, branch boundaries, rollback, retry safety, queue races/provenance/order, compaction keep/focus handling, inspector coverage, and owner-tool authority. Exercise SDK serialization and web slash/menu controls. Run relevant Nx SDK/web checks and the required rust-daemon:test target against the final tree. Preserve unrelated active work and report unrelated failures precisely.
