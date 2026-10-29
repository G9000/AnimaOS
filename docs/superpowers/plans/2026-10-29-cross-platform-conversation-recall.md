# Cross-platform conversation recall

The owner authorized implementation of Hermes-style recall across an agent's own Discord, Telegram, and web conversations. The recorded failure has stored messages and search hits, but the agent lacks the separate reader grant. Both access-profile radio handlers can clear that grant even though standard profiles include it.

## Design

Extend the existing tools rather than introducing a second history service or permission model. `search_conversations` without a topic browses recent own conversations. `read_conversation_window` without an anchor opens bounded recent messages. Exact anchors continue to support scrolling. Search and browse attach context to their top result only when the reader is granted. Preserve the exact `WindowRequest` used by owner transcript routes.

Both independent pstack design candidates selected this shape. A unified search/read tool would either bypass the separate reader opt-out or conceal two permissions behind one name. Keep ownership, hidden/deleted-message filtering, cold/hot authority, archived history, deadlines, and byte/message bounds in the existing history module. Evidence includes actual message IDs and timestamps, with explicit truncation and continuation information.

Preserve the recall toggle across access-profile changes, including unsaved on/off selections. Repair the currently authorized agent through the settings API. Do not blanket-regrant reading on startup because existing removals may be deliberate.

## Throughput checkpoint

- Blocking first steps: trace saved runs and source, compare Hermes, compare two design candidates, then write regressions.
- Independent workstreams: the Rust owner implements history/tools and coordinator tests; the web owner fixes settings and its rendered-control regression tests; the lead owns integration, live verification, and this record.
- Shared mutable state: workers have disjoint source ownership. Cargo validation runs serialize against the managed validation target. Live settings change through the daemon API, never by rewriting its active snapshot.
- Smallest safe decomposition: two implementation owners, one per independent layer. The lead reviews their combined result and runs the full required verification.

## Execution

- [x] Diagnose the screenshot runs and compare Hermes's browse/read/scroll implementation.
- [x] Independently compare designs and select existing-tool extensions.
- [x] Observe failing Rust regressions before implementation.
- [x] Implement bounded cross-platform browse, session reading, scrolling, and conditional top-result context.
- [x] Observe failing settings regression and preserve on/off recall selections across profile changes.
- [x] Run focused Rust and settings checks.
- [x] Run `bun run rust:test`, Rust formatting/lint, and relevant web test/build/typecheck/lint commands.
- [x] Independently review the final change and resolve actionable findings.
- [x] Restore the current agent's missing reader through the settings API.
- [x] Build and gracefully restart the local daemon; verify health, both available recall tools, and loaded browse guidance through the live API.
- [ ] Run one real-provider recall check with the owner's saved Discord conversation. Automatic approval review requires explicit permission for transmitting private context to the configured ChatGPT provider.
- [x] Record verification results and clean completed verification logs. Retain the pending test script and active service logs.

The workspace contains unrelated project-source and document-preview changes. Keep the recall change separate when committing and publishing it.

## Verification results

- Rust regression before implementation failed at `browsing needs no guessed keywords`, with `error` instead of `found`. The managed validation build completed normally before the expected failure.
- Settings regressions before the source fix had four expected failures and five passing controls. After removing the two profile resets, the focused settings group passed 14 tests. The full pre-fix/post-fix comparison improved from 63 failures to 55, with no new failure names. Remaining failures use stale unrelated settings UI assumptions. Reports are `test-output/recall-settings-{baseline,focused,after}.json`.
- Web typecheck, production build, and lint passed. Lint and bundle-size warnings predate this change.
- The running settings API added only `read_conversation_window` to Anima, retaining all 69 previous tools. A fresh snapshot confirms the 70-tool configuration persisted.
- Browser automation stopped because it could not verify the native browser address. Further verification uses the local daemon API and real agent-runtime paths; no browser success is claimed.
- Independent review found and the implementation addressed final-result races after asynchronous reads, continuation after a skipped browse candidate, and uncapped preview scanning. Final context authorization and history revalidation now share one state lock.
- The first full Rust run had two test-only failures. One generated fixture exceeded the registry's 200-byte session identity limit; the registry test still required query/anchor fields that are now optional. Both fixtures/contracts were corrected without production changes. The subsequent full `bun run rust:test` passed: 5,476 tests, zero failures, 52 ignored. The new real-catalog and cold-history regressions for all three platforms passed. The validation summary is `test-output/recall-validation.json`.
- The final independent review found no remaining issues in the repaired permission/finalization boundary, byte bounds, paging, or scrolling pointers.
- Final Rust formatting/lint passed after the fixture corrections. The updated daemon binary build passed. The previous process shut down through its lifecycle API, and the new daemon is healthy with all 70 tools retained. Live capability inspection confirms both history tools are available and the new no-keyword browse guidance is loaded. Runtime validation is `test-output/recall-runtime-validation.json`.
- Real-provider verification remains pending explicit approval to transmit private conversation context. No real-provider test request was sent. Automated coordinator tests exercise the actual discovery catalog and cold-history browse, read, and scroll flow for all three platforms.
- The running daemon uses the standard managed validation target. Its two redirected runtime log files remain open under `test-output/recall-runtime.*.log` and are intentionally retained while the service is running.
