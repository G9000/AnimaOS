# Chat controls implementation plan

> Execution: implement the user-authorized scope in this session with bounded subsystem ownership and independent review. Do not commit unrelated concurrent work.

**Goal:** Give Anima native owner chat management and make web conversation revision, queuing and context controls dependable.

**Architecture:** Rust coordinates durable mutations and validates source/run scope; SDK exposes typed APIs; web renders controls and drafts. Undo/retry use preserved revisions so completed action evidence is retained. Existing clear remains separate.

**Technology:** Rust daemon, existing history/control-plane stores, TypeScript SDK and React web console, Nx validation.

- [x] Review this plan and the companion spec for implementation-blocking gaps.
- [x] Implement conversation revision coordinator and owner API. Add failing regressions for whole-history branch, optional boundary, last-exchange omission, new identity/metadata, source preservation, active/scope refusal and failed persistence rollback. Reuse full_transcript and history outbox. Register routes/OpenAPI/inventory. Return `{session, retryPrompt, notice}` so UI can navigate and restore the draft without automatically repeating actions.
- [x] Implement queued edit/reorder in the coordinator with durable provenance/order, owner-source restrictions and a drainer mutation boundary. Add regression tests for already-starting races, save failures, idempotent delivery retries and stable channel admission. Register scoped owner APIs.
- [x] Extend compaction selection with validated keepLatestExchanges and focus; test selection/summary request and integration, including current-room owner tools. Preserve existing automatic compaction defaults.
- [x] Add scoped native rename_chat/compact_chat/export_chat tools and context inspector. Reuse owner title validation/full export/runtime compaction and exact tool-catalog live scope. Test forbidden origins, save rollback and honest export results. Keep prompt breakdown estimates explicit.
- [x] Add SDK types/methods and request tests. Add web revision/slash/menu actions, queue editing/order and explicit Wait/Steer controls, inspector display and compact argument handling. Test navigation/drafts, scope guards, errors and stale response handling.
- [x] Review spec compliance then code quality; fix findings and rerun affected checks. Update documentation and task checkboxes with measured results.
- [x] Run required Rust Nx tests, SDK tests/build/typecheck and web focused tests/build/typecheck, preserving unrelated dirty files. Verify the task-only Git snapshot and prepare publication under the user's existing push authorization.

Validation on the isolated task-only snapshot:

- Required `rust-daemon:test --skipNxCache`: passed, including the full core suite and 3,027 daemon library tests (33 existing ignored tests), with the canceled-queue-sibling regression included.
- SDK: 39 suites and 252 unit tests passed; build and typecheck passed.
- Web: 212 focused tests passed (one existing skip), plus all three retry integration journeys; build and typecheck passed.
- Independent reviews approved owner-tool scope, revision preservation, queue mutation boundaries, literal retry handling, stale navigation protection and scoped authenticated export links.
- Live provider acceptance and a restart of the user's running Anima instance were not part of this validation.
