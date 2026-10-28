# AnimaOS Kit Agent Guide

This repo is a host-agnostic agent runtime workspace. Keep the engine/runtime packages separate from runnable backend hosts.

# Coding Agent Workflow

Shared guidance for GPT-6 Astra and GPT-5.6 coding agents, adapted from the official [Astra guide](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra) and [GPT-5.6 guide](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.6).

- For explanation, review, diagnosis, or planning requests, inspect and report; implement only when requested. For change requests, carry the authorized work through implementation and relevant validation.
- Resolve routine details from context. Ask when missing information materially affects correctness or scope, and continue independent work while awaiting the answer.
- Prior authorization remains valid. Prepare a reviewable result before requesting any still-required approval for external, destructive, or scope-expanding actions.
- Within system and developer constraints, explicit user instructions take precedence over skill guidelines. Do not infer an extra approval gate from a skill when the work is already authorized. If a skill blocks progress, link the exact `SKILL.md`, quote the blocking instruction, and explain its applicability.
- Preserve the active objective when handling corrections and side questions unless the user changes it. Preserve unrelated local edits.
- Lead with the outcome, use plain language, and report meaningful progress, validation evidence, and blockers. Match detail to the task; avoid repeated narration and stock phrases.
- When delegation is permitted, delegate bounded, independent work only when it improves speed or quality. Assign clear ownership, keep messages readable, and verify the combined result.
- Complete the required checks below. Add tests for meaningful behavior or regression risks; avoid tests that merely duplicate trivial edits. Repeat or broaden checks only when changes, failures, or unresolved concerns justify it.
- Keep instructions lean: state each policy once and retain repository-specific constraints when simplifying prompts.

# Repo Boundaries

- `packages/core-rust` is the Rust host-agnostic core workspace. It contains reusable Rust engine crates such as `anima-core`, `anima-memory`, `anima-swarm`, `anima-schedule`, and `anima-harness` (an embeddable agent-harness crate composing `anima-core` + `anima-model-adapters`).
- `packages/*` is for reusable libraries, SDKs, runtime ports, and shared tooling. The TypeScript packages (`sdk`, `cli`, `tui`, `ui`) are thin clients of the Rust daemon; execution lives in Rust, not TypeScript.
- `hosts/*` is for runnable backend host processes only. Hosts wrap a core implementation and expose it over a runtime boundary such as HTTP, WebSocket, SSE, jobs, or another process interface.
- `hosts/rust-daemon` is the ready Rust host. It embeds the Telegram connector runtime, including polling, pairing, delivery, persistence, and restart recovery.
- `hosts/anima` is the installable `anima` program (supervisor, tray, updater) wrapping `rust-daemon`.
- `apps/*` is for user-facing app surfaces such as the `web` runtime console, `playground`, `web-e2e`, and the Astro docs apps.
- `tools/*` is for workspace tooling such as the `workspace-dev` launcher.

# Architecture Rules

- Do not move reusable engine/runtime library code into `hosts/*`.
- If code is intended to be reused by multiple hosts, put it under `packages/*`.
- Do not make `anima-core` depend on an HTTP framework, DB driver, or host-specific runtime.
- Keep host selection centralized in `tools/workspace-dev` rather than hardcoding host ports or project names in UI/client packages.

# Dev Workflow

- Use `bun dev` (or `bun dev --host rust`) for the normal local workflow. It runs the Rust daemon/API and `@animaos/web` through `workspace-dev`, then automatically opens a fresh approved owner page after readiness. `bun dev --console` reopens the approved console; `--no-open` prints a manual link instead. Optional root commands are `bun run dev:docs` (both docs sites), `bun run dev:game` (Village host/browser), `bun run dev:playground` (daemon/playground), and `bun run dev:all` (all services). The web console talks only to the daemon on `127.0.0.1:8080`. The daemon automatically starts restored Telegram connectors; no separate gateway process or token environment variable is used.
- The only supported host key is `rust`.
- Use Bun workspace scripts for focused work: `bun run --filter <workspace-name> <script>`. Inspect the workspace's `package.json` before assuming a script name.
- Use `bun run daemon`, `bun run rust:build`, `bun run rust:test`, and `bun run rust:lint` for daemon development, build, tests, and formatting.
- Use `bun run --filter @animaos/docs build` and `bun run --filter @animaos/contributor-docs build` for docs work.
- Use `bun run --filter workspace-dev test` when changing host selection, process orchestration, or dev launcher behavior.
- Use root `bun run build`, `bun run test`, `bun run typecheck`, and `bun run lint` for workspace-wide checks.
- Prefer the repository's Bun scripts over direct tool commands when a matching script exists.

# Verification

- For Rust host/core changes, run `bun run rust:test` (the full core, daemon, and packaged supervisor suites).
- Cargo outputs stay under `target/cargo`. Managed build/test commands enforce a combined 32 GiB maximum before, during, and after commands: inactive Cargo outputs are cleaned automatically, and a build that exceeds the budget is stopped without terminating unrelated running apps. Checks during builds run every 15 seconds, so transient overshoot is possible. `ANIMA_RUST_CACHE_BUDGET_GIB` can lower the limit; higher environment or programmatic values are capped at 32 GiB. Never bypass this ceiling to get a check to finish. First remove inactive outputs or run focused checks sequentially.
- Use the repository's wrapped Rust commands. Validation reuses the absolute `target/cargo/validation` target automatically, including on Windows while the development daemon is running. Do not create per-feature validation directories or set relative target paths from package folders. Explicit external targets are preserved and are outside automatic cleanup.
- Use `bun run rust:cache:status` to inspect the cache and `bun run rust:cache:clean` for explicit full cleanup after stopping programs/builds using it. Never delete isolated incremental/fingerprint files to bypass Cargo's dependency bookkeeping.
- For dev launcher changes, run `bun run --filter workspace-dev test`.
- For Astro docs changes, run the relevant build target: `bun run --filter @animaos/docs build` or `bun run --filter @animaos/contributor-docs build`.
- For TypeScript package changes, use the relevant workspace's `test`, `build`, and `typecheck` scripts.
- Do not claim completion unless the relevant verification commands have passed in the current tree.
- The task wrapper (`tools/workspace-dev/scripts/run-task.ts`) gives each invocation an owned directory under OS-temp `anima-task-*`, passes that directory to child tasks, and cleans it after completion, including failed tests. Keep reports in the configured `test-output` paths. After each verification, remove task-owned scratch scripts, snapshots, logs, and screenshots once their results are recorded; inspect exact paths and active users before cleanup. Hard-killed runs may require manual cleanup. Leave tool download caches untouched; Cargo outputs follow the automatic budget above.
