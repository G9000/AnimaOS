# MCP agent permissions implementation plan

**Goal:** Enabled and connected MCPs expose validated tools automatically in Free mode; Strict requires exact review and grants; Custom follows its execution-class Allow/Ask/Deny setting.

**Design:** Derive access from the current agent autonomy and execution policy. Keep automatic access in the live catalog, separate from durable reviews and grants. Publication, run selection, capabilities, discovery and dispatch must agree. Mode/policy changes fence old dispatch stamps and refresh the catalog, including rollback on failed saves. Disabling, disconnecting, deleting, changing schemas and restarting retain existing lifecycle fences. Helpers and other agents receive no inherited access.

**Implementation:** Rust daemon owns policy and dispatch. SDK exposes the access state; the existing MCP panel explains automatic/review/denied access without manual grant controls in automatic mode. Existing owner connection enable/connect and credential controls remain in place.

- [x] Add failing manager/executor regression tests for Free, Strict, Custom Allow/Ask/Deny, policy transitions, ownership and lifecycle fences.
- [x] Implement derived access, transient publication, run selection and current-policy dispatch checks without persisting automatic reviews/grants.
- [x] Integrate autonomy and approval-policy mutations with catalog refresh and stale-call invalidation, including persistence rollback.
- [x] Add typed access metadata, panel behavior and tests; update MCP setup guidance and companion documentation.
- [x] Run focused regressions, full `bun run rust:test`, Rust formatting, and relevant SDK/web test, typecheck and build scripts.
- [x] Review the combined change and record validation.

Delivery: commit and push the verified changes as requested.

**Validation:** Tests must demonstrate a real synthetic transport call without manual review/grant in Free and Custom Allow, zero transport calls for unreviewed Strict/Custom Ask and Custom Deny, and stale-call refusal after Free → Strict → Free. Automatic exposure must disappear on disable, disconnect, replacement discovery and restart; persisted reviews/grants must stay unchanged. UI tests must show accurate access and preserve manual controls for review mode.

## Verification results

- Full `bun run rust:test`: 4,794 passed, 47 intentional ignores, 69 suites.
- Final exact-tree MCP permission regressions: 10 passed.
- SDK full serial host run: 267 passed across 47 files, including real-daemon and restart integration.
- Web full suite: 2,286 passed, one existing skip, 192 files.
- SDK/web build, typecheck and lint; Rust formatting; staged diff checks passed.
- An initial SDK service run hit sandbox loopback isolation and a parallel Windows executable lock; the serial host run using the isolated validation target passed.
- Review findings about restored/no-op access metadata were fixed by deriving every owner view from the current state guard.
