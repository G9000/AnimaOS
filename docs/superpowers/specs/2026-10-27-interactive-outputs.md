# Interactive conversation outputs

## Goal

Let the companion create dashboards and editable trackers beside the conversation, refine them through chat, and reopen their saved contents after reload. Extend the existing conversation results, ownership, pinning, revisions and durable saves.

## Closed payload contract

The existing `show_result` and `list_results` tools remain the agent interface. `show_result` creates or replaces the full payload using its exact result ID, current `expectedRevision` and a fresh `requestKey`. New results use revision zero. A result remains untrusted display data, never independent proof of an action.

Add these payloads:

```typescript
type TrackerCell = string | number | boolean;
type TrackerPayload = {
  kind: 'tracker';
  columns: { key: string; label: string; kind: 'text' | 'number' | 'checkbox' }[];
  rows: { id: string; cells: Record<string, TrackerCell> }[];
};
type DashboardPayload = {
  kind: 'dashboard';
  widgets: { id: string; title: string; payload: DashboardWidgetPayload }[];
};
// DashboardWidgetPayload is chart, comparison, document, calculator or tracker.
// Dashboard nesting is rejected.
```

Column keys, row IDs and widget IDs are 1–40 ASCII letters, digits or underscores. IDs are unique within their collection. Trackers have 1–12 columns and at most 100 rows. Every row contains exactly the column keys, and each cell matches its column kind. Text cells allow at most 2,048 UTF-8 bytes, labels and widget titles 160 bytes. Numbers use the existing finite safe-number rules. Dashboards contain 1–8 widgets. Existing 32 KiB payload, 32-result and 256 KiB conversation limits apply to the complete dashboard.

## Persistence and editing

The Rust daemon validates every payload and owns all durable state. Existing session JSON persists the new closed enum variants without a schema migration or separate output shelf. Existing owner `POST /api/agents/{agent}/sessions/{session}/results` saves a complete edited payload with `clearGeneration`, `expectedRevision` and `requestKey`. A stale owner or agent edit fails without replacing newer data. Successful saves return their acknowledged revision; failures retain the previous payload. Pin updates continue to participate in the same revision counter.

The web UI renders the existing closed widgets, makes tracker cells editable, and saves owner changes through the thin SDK. Tracker edits remain drafts until the daemon acknowledges the save. Calculator inputs retain their existing local preview behavior. A reset generation or different conversation invalidates in-flight responses. The pinned panel remains the live preview beside chat; an owner can export its structured JSON.

## Execution boundary

These outputs contain structured display data and the existing closed arithmetic tree. They accept no HTML, CSS, JavaScript, URLs as executable configuration, arbitrary expressions or embedded frames. No generated script runs, and outputs receive no network, owner API, cookie or storage bridge. Ordinary strings that resemble markup are rendered as text or through the existing Markdown renderer.

## Verification

- Accept mixed dashboards, typed tracker cells and empty trackers; preserve stable IDs and values through serialization and durable reload.
- Reject nested dashboards, duplicate IDs, invalid keys, mismatched cell types, missing/extra cells, unsafe numbers, unknown fields and all collection/byte limits.
- Save owner edits with revision comparison; reject older payloads, wrong conversations, reset generations and untrusted request origins without losing saved edits.
- Keep model `show_result`/`list_results` scoped to direct owner conversations and bounded source paging.
- Verify the UI renders and edits without source execution, retains tracker save drafts on failure, ignores late responses after a scope change, and uses daemon revisions after save and pin changes.

The full Rust and relevant SDK/web verification runs at the final frozen tree.
