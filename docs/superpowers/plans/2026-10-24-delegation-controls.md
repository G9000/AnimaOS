# Delegation controls execution plan

- [x] Add typed owner delegation policy with strict validation, API and SDK support.
- [x] Integrate saved model routes and bounded per-assignment limits into synchronous and background helpers.
- [x] Persist assignment options and bind retries to the same accepted request.
- [x] Add bounded batch launch and any/all result collection.
- [x] Validate optional structured results without replaying tool effects.
- [x] Expose owner route and worker limit controls in Settings.
- [x] Assess nested coordination and explicit forks against durable lineage requirements; record remaining gaps honestly.
- [x] Remove optional external product comparisons only from today's authored prose while preserving required notices and compatibility keys.
- [x] Complete required current-tree Rust validation after review fixes.

Nested coordination and conversation forks remain deferred until root-wide admission, stop cascade and exact context boundaries are implemented. The host rejects unsupported policy settings. Route and worker-limit controls, batch admission, any/all collection and JSON result validation are implemented. Wording cleanup is limited to documents authored in this work; historical documents were restored.

## Verification

- Required Nx rust-daemon:test passed with the full core dependency and all host integration groups; host unit tests: 3247 passed, 0 failed, 37 ignored.
- Nx rust-daemon:lint passed.
- New helper Settings tests: 15 passed; existing SettingsPanel regressions: 64 passed; SDK delegation contract: 1 passed.
- Latest web production build and complete web/SDK dependency typechecks passed.
- Native scripted runs verified saved routes, stricter limits, retry equality, grant checks, batch receipts, any/all waits, cancellation and JSON result validation.
- Real JSON disk save/reload and byte-exact pre-upgrade backup tests passed.
- Main personality omission remains covered by the native helper tests. No live provider task or running demo daemon restart was performed.
- Wording cleanup covers only current authored documents and three MCP configuration sentences added earlier in this chat. Historical branding and required notices remain.
