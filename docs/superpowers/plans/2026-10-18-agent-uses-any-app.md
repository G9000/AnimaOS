# The agent uses any app, like a person — Handoff Plan

**Goal:** like a person, the AnimaOS agent can use **any app on the computer** (Claude Code, Codex, OpenCode, git, Python, a REPL, a full-screen terminal app) through general abilities plus know-how. No app needs a built-in integration. It's free by default, and the owner makes it stricter through settings.
**Decided with the owner (2026-10-18):** an agent on Operate runs commands without asking. Vendor-specific coding tools are gone. Interactive app sessions are in scope now. Follow-ups use each CLI's own session resume. The agent picks a freedom level per task; the owner's ceiling is the Run commands policy plus rules.
**Coding agent parity:** [`2026-10-18-coding-agents-parity.md`](2026-10-18-coding-agents-parity.md) amends steps 2, 3, 4 and 6 and adds wake-on-exit, login passthrough, per-CLI bundles and an ACP client, all in the first round.
**Rules:** update `design/design.pen` (pen.dev MCP) **before** any UI code. Keep reusable engine code in `packages/*` and host wiring in `hosts/rust-daemon`. `unsafe_code` is forbidden workspace-wide. Commit nothing unless the owner asks.

## Already done (cleanup, uncommitted)

- **Removed** the vendor-specific `codex_task` / `claude_code_task` workers everywhere:
  - the `anima-service-adapters` `coding` module
  - the daemon `tools/coding.rs`, its registry entries, approvals and receipts
  - the completion-review branch and `CompletionObservation::VerificationRequired`
  - the README section, the web Settings toggles/tests/labels, and their design frame
- **Kept the reusable part** in a new crate, [`packages/core-rust/crates/anima-process`](../../../packages/core-rust/crates/anima-process). It provides `Custody`: launch a program so its whole tree can be stopped and proven gone.
  - Containment: Windows uses a Job Object. Linux uses a process group plus a private cgroup v2 leaf when the daemon's cgroup is delegated; the worker joins it through a `/bin/sh` wrapper, so no unsafe code. macOS and undelegated Linux use a process group.
  - Tests pass on Windows (3). The Unix `setsid`-escape test runs on Linux/macOS.
  - The crate is registered in the root `Cargo.toml` and in the `core-rust` build/test/fmt targets.
- Things that are **not ours** and must be left alone: other sessions' in-progress work in `agent_runs/action_receipts.rs` and `action_receipt_tests.rs` (superseded-intent tests, test save-gate), `state.rs`, `anima-core/src/lib.rs` and `self_recovery.rs`.

## What already exists and should be reused

- `bash` (`hosts/rust-daemon/src/tools/process/shell.rs`): any command. The default timeout is 120 s with no maximum. `SHELL_ENV_ALLOWLIST` strips secrets but keeps `USERPROFILE`/`APPDATA`/`HOME`, so CLI logins work.
- `bg_start`/`bg_output`/`bg_stop`/`bg_list` (`tools/process/background.rs`): background commands. **Gap:** these use `std::process` and kill only the leader.
- The approval policy per agent: Write/Exec/Network/Delegate = Allow/Ask/Deny (`approvals/mod.rs`, default `exec: Ask`). Rules use matchers (`approvals/policy.rs`: `matcher_kinds`, `CommandPrefix` for `bash`/`bg_start`). The web UI is `apps/web/src/pages/ApprovalsPage.tsx`, the API `daemon.setApprovalPolicy`.
- Access profiles: `apps/web/src/lib/agent-access.ts` (Observe/Collaborate/Operate) and `components/SettingsPanel.tsx`. Onboarding is `components/onboarding/{OnboardingFlow,CompanionSetup}.tsx`.
- Skills are `skills/<slug>/SKILL.md`, owner-approved, loaded by `load_skill`. Community bundles carry up to 2 skills (`anima-service-adapters/src/community_bundles.rs`, `ManifestComponent::Skill`, 64 KiB each) and import as drafts the owner approves.
- Both CLIs resume non-interactively: `claude -p --resume <session_id>` and `codex exec resume <id>`. So follow-ups need no special tool.

## Steps

### 1. Design first (`design/design.pen`)
- Settings · Permissions (`iooUv`): the Operate card reads "Edits files, runs commands and uses any app on this computer without asking". Add a line under Default access saying picking Operate sets Run commands to Allow ("Change in Approvals").
- New frame **Chat · Using an app**: the agent working in Claude Code. An activity card with the app/command, a live terminal screen (monospace, glass panel, 2–4px corners), a running/exited status and a Stop button.
- Settings · Tools (`b4qtXd`): the Built-in grid's "Development" group becomes **Apps & terminals**.

### 2. App sessions engine (`anima-process`, new `session` module)
- Dependencies: `portable-pty` (Windows ConPTY, Unix PTY) and `vt100` (screen model).
- `AppSession::start(program, args, cwd, env, cols, rows)` supports `write(bytes)`, named keys (Enter, Tab, Esc, Up/Down/Left/Right, Backspace, Ctrl+C, Ctrl+D), `screen()` (visible rows plus cursor), `output_since(cursor)`, `exit_status()` and `stop()`.
- Limits: 120×40 by default, a 1 MiB output ring, 2,000 history lines, and stop after 30 minutes idle.
- Custody:
  - Unix: run the program under `Custody::prepare` (cgroup wrapper). portable-pty's `setsid` makes the PTY child a group leader, so the group kill works.
  - Windows: right after spawn, assign the ConPTY child to a `win_custody::Job` (kill-on-close; the daemon already depends on `win-custody = "=0.1.1"`). **Verify** that portable-pty exposes the child's Windows process handle. Document the gap before assignment: only the app's own startup, before the agent types anything.
- Tests:
  - a Node REPL: type `1+1` + Enter and read `2` from the screen
  - Ctrl+C
  - the app exiting
  - output limits and the idle stop
  - Stop/drop killing a grandchild

### 3. Daemon app-session tools (`hosts/rust-daemon/src/tools/process/app_session.rs`)
- `app_start {command, cwd?, cols?, rows?}`: Exec class. Add it to `matcher_kinds` with `CommandPrefix`, so a rule like "allow `claude`" works. Use the `bash` env allowlist plus `TERM=xterm-256color`.
- `app_send {id, text?, keys?}`: Exec class.
- `app_read {id, wait_ms?}`: Read class. Returns screen text, new output and exit status, waiting up to `wait_ms` for output to settle.
- `app_stop {id}` (Exec) and `app_list` (Read).
- A daemon-owned manager like `ProcessManager`, with at most 4 sessions per agent. Sessions outlive a single run so back-and-forth works, and die on daemon exit or idle.
- Add the tools to `is_process_tool` (helpers excluded) and to `OPERATE_TOOLS` in the web `agent-access.ts`. Start and send record native action receipts like `bash`.
- Update `approvals/policy.rs` risk tables and `tools/tests.rs` schema rows.

### 4. Full tree cleanup for background commands
Move `bg_start` to `anima_process::Custody` (process-wrap + tokio), so `bg_stop`, Stop and daemon exit retire the whole tree. A long `claude -p …` in the background must leave no processes behind.

### 5. Operate = free (web)
- `SettingsPanel`: a new `setCommandPolicy` prop from `ViewHarness.tsx` (via `daemon.setApprovalPolicy`). Saving with Operate newly selected also sets `exec: 'allow'`, and the card shows the current Run commands policy with "Change in Approvals". Other profiles leave the policy unchanged.
- Onboarding: creating an Operate agent sets `exec: 'allow'` too.
- The daemon default (`exec: Ask`) is unchanged.

### 6. Know-how: `bundles/coding-agents.json`
- A community bundle with two skills, `claude-code` and `codex`. Write them from scratch for our tools, based on each CLI's own `--help` and documentation. Don't copy text from other projects' skills, so no third-party license notice is needed.
- Cover:
  - finding the app (`where`/`command -v`)
  - one-shot runs (`claude -p --output-format json` or `stream-json --verbose`, `codex exec --json -`)
  - choosing freedom per task (`--permission-mode plan|dontAsk|acceptEdits|auto|bypassPermissions`, `--tools`/`--allowedTools`; `codex --sandbox read-only|workspace-write|danger-full-access`, `--dangerously-bypass-approvals-and-sandbox`)
  - follow-ups via resume
  - long runs via `bg_start`
  - interactive use via the app tools (including trust and first-run dialogs)
  - always reading changes and running checks yourself
  - never committing or pushing unless asked
- Verify every flag with `--help`.
- Add a unit test that validates the manifest via `include_str!`, and check each skill with the daemon skill parser.
- Claude Code facts verified on 2.1.207:
  - The final `result` event has `subtype: "success"` even when `is_error: true` (for example an expired login). Check `is_error`.
  - `system` events such as `task_summary` can follow `result`.
  - `--setting-sources user` keeps a workspace's own settings, hooks and `.mcp.json` out.
  - `--settings '{"disableAllHooks":true,"autoMemoryEnabled":false}'` disables remaining hooks and auto-memory.
  - `--bare` ignores OAuth logins, so avoid it with subscription accounts.

### 7. Watching apps in the chat
- Render `app_read` results as the designed terminal screen in the chat activity (find where tool results render; likely `apps/web/src/components/AgentWork.tsx`), with a Stop action that calls `app_stop`.
- `needs-you.ts` labels: "Started an app", "Typed into an app", "Stopped an app".

### 8. README
Add a **"Using apps"** section to `hosts/rust-daemon/README.md`:
- the policy, rules and the Operate default
- app sessions and their limits
- the bundle install path (Settings › Tools › Community bundles)
- containment per platform: Windows and delegated-cgroup Linux contain everything; macOS and undelegated Linux can't contain a descendant that calls `setsid`

## Verification

- Rust: `CARGO_BUILD_JOBS=4 bun x nx run rust-daemon:test --skipNxCache`. The machine ran out of memory once with full parallelism. `attachments::documents::admission::tests::oversized_and_failed_supported_selections_consume_the_attempt_allowance` is flaky under full-suite load but passes alone.
- `bun x nx run core-rust:test` (includes `anima-process`).
- Linux: WSL Ubuntu has Rust installed (`~/.cargo/bin/cargo`). Run `CARGO_TARGET_DIR=$HOME/anima-target cargo test --locked -p anima-process` from `/mnt/c/.../animaos-kit`. This WSL uses hybrid cgroup v1, so only the process-group path runs there. The cgroup v2 path needs a systemd-delegated Linux.
- macOS: `cargo check --target x86_64-apple-darwin -p anima-process --tests` works from Windows (target installed). The owner runs the tests on a MacBook.
- Web: `bun x nx run @animaos/web:typecheck` and `bun x nx run @animaos/web:test`.
- Manual: `bun dev --host rust`, with an Operate agent (no prompts):
  - Claude Code one-shot, then a resume follow-up
  - `claude` interactively in an app session, watched in chat
  - Stop, then confirm in Task Manager or `ps` that no Claude Code process remains
