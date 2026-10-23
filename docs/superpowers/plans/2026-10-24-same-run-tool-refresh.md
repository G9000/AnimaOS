# Same-run MCP tool refresh

**Goal:** Continue the original owner request in the same run after connecting, discovering, or granting MCP tools.

**Design:** Add a host-agnostic tool refresh hook before model calls. The daemon requests a refresh only after a successful owner MCP operation or explicit `manage_mcp refresh`. Rebuild the run's MCP descriptors, discovery catalog, and execution registry together; retain native tool ceilings, conversation, budgets, approval gates, and live dispatch checks. Never retry an external mutation as part of refresh.

- [x] Add a failing integration test for connect, discover, and call within one owner run, with tool discovery both enabled and disabled.
- [x] Add the core refresh hook and daemon-owned MCP snapshot refresh; preserve discovery hydration and update live capability inspection.
- [x] Remove next-owner-message guidance and expose explicit refresh through `manage_mcp`.
- [x] Cover reviewed grants, failed setup, explicit refresh, and revocation without widening helper or peer scope.
- [x] Run focused regressions, `bun run rust:test`, and `bun run rust:lint`; review the final diff and remove task-owned verification scratch.

## Verification

- The connect/use regression failed before implementation because the next model request did not advertise the connected tool, then passed with discovery both enabled and disabled.
- Full `bun run rust:test` passed with `CI=1`, `CARGO_TARGET_DIR=target/cargo/validation-rust-daemon`, and `RUST_TEST_THREADS=4`: 4800 passed, 47 ignored, zero failures across core, daemon, supervisor, integration, and doc tests.
- `bun run rust:lint` and `git diff --check` passed.
- Default parallelism exposed an existing Docker timeout-test timing failure; its isolated recheck and the complete four-thread rerun passed.
- Source review found no remaining issues after fixing registry replacement and the MCP-only live capability update.
- Task-owned verification logs were removed after recording these results; wrapper-owned temporary directories were cleaned.
