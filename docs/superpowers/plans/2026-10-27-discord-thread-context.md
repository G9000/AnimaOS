# Discord thread context implementation plan

Goal: Let an agent inspect its current Discord thread and distinguish other bots and human participants.

- Add a read-only, current-thread-scoped history capability with bounded pagination and stable sender identities.
- Keep other bots from triggering runs; treat returned thread text as untrusted context.
- Integrate the capability with tool discovery, existing approval and session authority checks.
- Test native Discord paging, model-visible bot messages, scope revocation and loop prevention.
- Run focused tests and repository-required Rust verification; report live activation separately.
