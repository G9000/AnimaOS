# Local HTTP MCP Bearer Implementation Plan

**Goal:** Connect AnimaOS to authenticated local MCP servers, including PropShifu, using the existing Rust transport and credential vault.

**Design:** Accept `http://localhost`, `http://127.0.0.1` and `http://[::1]`; preserve its normalized hostname while pinning `localhost` dialing to fixed IPv4/IPv6 loopback addresses. Keep remote HTTP, ambiguous IP spellings, URL credentials, queries, fragments, redirects and proxies disallowed. Use one canonical endpoint predicate at credential storage, record restore, service mutation and transport boundaries. Keep bearer entry in the owner password control and secrets out of chat setup tools.

**Tech stack:** Rust, reqwest, rmcp, existing OS credential vault, React and Vitest. Hermes's URL/header forwarding is a behavioral reference; no Python code or runtime is imported.

- [x] Add failing native regressions for localhost normalization, local credential binding/restore, endpoint fencing and actual authenticated loopback HTTP discovery.
- [x] Update shared endpoint validation, credential store, service, transport and chat setup guidance.
- [x] Update the owner credential control and regressions for local HTTP and remote HTTP rejection; preserve unrelated local lint changes.
- [x] Update the MCP guide to document the local exception.
- [x] Run `bun run rust:test`, `bun run rust:lint`, and web test/build/typecheck; resolve failures caused by this change.
- [x] Verify the existing PropShifu service, then initialize and discover through AnimaOS's actual Rust MCP transport with the authorized token. Never call discovered tools during this connection test.
- [x] Review the diff, record verification, and clean task-owned scratch files without removing shared caches or unrelated work.

Live connection checks require host access: sandboxed networking can report connection refusal for running host services. PropShifu already answered an unauthenticated host-side request with HTTP 401, so reuse its current instance.

## Verification notes

- Native local bearer regressions: 3 passed; the separate live test remains opt-in.
- Actual rebuilt Rust transport initialized PropShifu and discovered 46 tools with protocol 2025-11-25 through both `http://127.0.0.1:3001/mcp` and `http://localhost:5173/mcp`; zero tools were called. A temporary local test token with sending disabled was revoked afterward.
- Web suite: 2,284 passed / one intentional skip across 192 files (`--maxWorkers=4`); initial two transient failures also passed in a targeted rerun. Web build/typecheck, SDK MCP tests (9), SDK typecheck and Rust formatting passed.
- Shared Cargo target integration compilation hit incompatible metadata; full native validation is being repeated under `target/cargo/validation-rust-daemon` with `CI=1`. Shared caches are retained.
- User requested removal of milestone labels: renamed MCP test identifiers, helpers, fixtures, SDK/web test descriptions and current setup-guide labels to descriptive names. Shared TLS certificate fixture paths retain their real filenames.
- Focused independent implementation review approved; restart regression reconnects through a fresh manager and retained credential store.

The isolated full daemon run passed 3,249 tests with 38 intentional ignores; one unrelated Docker-executor fixture test missed its 150 ms second-execution deadline under default concurrency. Repeating the full gate with `RUST_TEST_THREADS=4`; no executor code or deadline was changed.

Final full Rust gate passed with CI=1, absolute isolated CARGO_TARGET_DIR and RUST_TEST_THREADS=4: 4784 passed / 47 intentional ignores / zero failures across 69 suites. The daemon suite passed 3,250 tests. The temporary credential was revoked, all task-owned scratch scripts and raw logs were removed after recording results, and Cargo caches were retained. The configured report is test-output/local-mcp-bearer/verification.md. The existing AnimaOS daemon still needs a restart to load the backend change; connection verification used the rebuilt native transport and did not save a permanent connection or agent grants.
