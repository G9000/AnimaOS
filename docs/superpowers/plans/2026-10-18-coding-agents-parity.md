# Coding agents: parity — Plan

**Goal:** an AnimaOS agent can hand coding work to Claude Code, Codex, OpenCode and Gemini CLI (and any similar CLI), follow up on it, watch it, stop it, and check its result — at least as well as the reference agent does today. It does this with general abilities plus **skills** (know-how), never with a vendor-specific tool.
**Builds on:** [`2026-10-18-agent-uses-any-app.md`](2026-10-18-agent-uses-any-app.md) (the "any app" handoff plan). This plan keeps that plan's decisions and steps, and amends or adds to them where the reference agent shows a gap. Step numbers below like "AUA-3" point at that plan.
**Reference:** an open-source reference agent, checked out locally. We study its behaviour; we don't copy its text or code (license hygiene, same rule as AUA-6).
**Rules (unchanged):** design in `design/design.pen` before UI code; engine code in `packages/*`, host wiring in `hosts/rust-daemon`; no `unsafe`; commit nothing unless the owner asks.

## How the reference agent does it (summary)

The reference agent has **no** "run Claude Code" tool either. Its agent drives coding CLIs with two generic tools and a set of skills:

| Reference piece | What it is | Where |
|---|---|---|
| `terminal` tool | One command. `background`, `pty`, `timeout` (180 s default, 600 s foreground max, then auto-promoted to a background job), `workdir`, `notify` (wake the agent on exit or on regex matches), `heartbeat` | `tools/terminal_tool.py:1512` |
| `process_manage` tool | Actions on background jobs: `list`, `poll`, `log` (paged), `wait` (with timeout), `write` (raw bytes), `submit` (text + Enter), `close` (EOF), `kill`, `handoff` | `tools/process_registry.py:2582` |
| PTY backend | `ptyprocess`/`winpty`, 30×120, `PAGER=cat`/`GIT_PAGER=cat`, 200 k-char rolling buffer | `process_registry.py:1152` |
| Terminal query responder | Answers DSR / cursor-position / window-size / DECRQM queries so TUIs (Codex) don't hang waiting for a reply | `tools/pty_query_responder.py` |
| Tree kill | Re-checks PID start time, snapshots descendants, TERM → grace → KILL; `taskkill /T /F` on Windows | `process_registry_termination.py:33` |
| Env scrubbing | Strips secrets but deliberately keeps the CLI's own login token | `tools/environments/local_env_policy.py:85` |
| Skills | `claude-code` (print mode first, tmux for interactive, dialog handling, resume, budgets), `codex` (`exec`, sandbox modes, git-repo requirement, `codex review`), `opencode` (`run`, TUI quirks), a skill for driving another copy of itself, optional `grok`, `openhands`, `antigravity-cli`, `blackbox` | `skills/autonomous-ai-agents/*`, `optional-skills/autonomous-ai-agents/*` |
| Structured runtimes (extras) | `codex app-server` JSON-RPC runtime; ACP **client** for `copilot --acp`; ACP **server** (its own `acp` subcommand) for Zed/VS Code/JetBrains; an MCP server mode | `agent/transports/`, `agent/copilot_acp_client.py`, `acp_adapter/` |
| Parallelism | `delegate_task` fan-out with optional git-worktree isolation per subagent | `tools/delegate_tool.py`, `tools/subagent_worktree.py` |

## Where we stand (HEAD `88a6d1f`)

- **Done:** the vendor tools are gone. `anima-process::Custody` exists (Job Object / cgroup v2 / process group) with tests, but **nothing uses it yet**. The daemon doesn't depend on the crate.
- **Have, roughly equal to the reference agent:** `bash` (≈ foreground `terminal`), `bg_start/bg_output/bg_stop/bg_list` (≈ `process_manage` without PTY, wait, write or submit), the approval policy with `CommandPrefix` rules (≈ its allowlists), skills plus community bundles, helpers/delegation (≈ `delegate_task`), and an MCP client.
- **Missing** (AUA steps 1–8, none started): PTY app sessions, `app_*` tools, Custody in `bash`/`bg_*`, Operate = free, the skills bundle, the chat terminal view, and the README.

## Gap list: the reference agent has it, AUA doesn't cover it yet

| # | Gap | Why it matters | Fix (step below) |
|---|---|---|---|
| G1 | Terminal query responder | Codex and other TUIs block on a DSR/cursor query; `vt100` only parses output and never answers | P1 |
| G2 | Wait-for-output | Without a blocking wait the agent spends turns polling a long `claude -p` | P2 (`bg_wait`, `app_read` `until`) |
| G3 | Wake on finish | The reference agent's `notify` re-enters the agent when a background job ends; ours needs the agent to poll | P3 |
| G4 | `PAGER=cat`, `GIT_PAGER=cat` in spawned env | `git log` inside an agent session opens `less` and hangs | P1, P4 |
| G5 | `bg_start` has no `env_clear` | It leaks every daemon env var, provider API keys included, into the child. A security gap, not just parity | P4 |
| G6 | `bash` on Unix kills only the direct child (audit M11) | A timed-out `claude -p` leaves its tree running | P4 |
| G7 | Login passthrough | `SHELL_ENV_ALLOWLIST` has no `ANTHROPIC_API_KEY`/`OPENAI_API_KEY`. Subscription logins (keychain, `~/.codex/auth.json`) work via `HOME`; API-key users can't | P5 |
| G8 | Skill coverage | AUA-6 plans `claude-code` + `codex` only; the reference agent also covers OpenCode, agent-to-agent, and optional others (no Gemini CLI skill) | P6, plus a Gemini CLI bundle beyond the reference |
| G9 | Bundle limit: max **2** skills per bundle | AUA-6's single bundle can't hold 3+ skills | P6 (one bundle per CLI) |
| G10 | Parallel coding with worktrees | The reference agent isolates each parallel coder in a git worktree | P6 (skill know-how; no new code) |
| G11 | Structured protocol drive (ACP / `codex app-server`) | Typed events, real cancel, permission requests routed to *our* approvals instead of the CLI's yolo flags | P8 (first round) |
| G12 | Anima as an ACP server / MCP server for editors | Zed or Claude Code could use Anima as an agent | Out of scope (separate plan) |

Things the reference agent has that we deliberately **skip**: shadow-git checkpoints, `process handoff` (our helpers have no process tools), and reading foreign Claude Code / Codex transcripts.

## Steps

The first round is AUA-1…8 with the amendments below folded in, plus the ACP client (P8). Do them in AUA order; P8 can start once P4 lands. P-steps say what changes.

### P1. App sessions engine — amends AUA-2
- Add a **query responder** next to the `vt100` parser: scan PTY output for `ESC[6n` (cursor position), `ESC[5n` (status), `ESC[18t` (window size), `ESC[c` (primary DA) and DECRQM, and write the answer to the PTY. Unit-test each query against a fake PTY, and add one live test that starts a TUI that queries the cursor on start.
- Spawn env: the `bash` allowlist plus `TERM=xterm-256color`, `PAGER=cat`, `GIT_PAGER=cat`, `NO_COLOR` **unset** (TUIs need colour to draw selection state).
- Windows `submit` sends `\r`, Unix sends `\r`. Test both, because the reference agent needed `\r\n` on winpty. Verify with ConPTY.
- Keep AUA-2's limits (120×40, 1 MiB ring, 2,000 lines, 30 min idle).

### P2. Waiting instead of polling — amends AUA-3 and AUA-4
- `app_read {id, wait_ms?, until?}`: `until` is an optional literal string or a short regex (bounded length, compiled with a size limit). Return when it appears on screen, when output has been quiet for `wait_ms`, or when the app exits. Cap `wait_ms` at 120 s.
- New `bg_wait {id, timeout_ms, until?}`: Read class, the same return rules for background jobs, and returns the tail plus exit status. Add it to `is_process_tool`, `OPERATE_TOOLS` and `QUIET_TOOLS`, and to the policy table as Read.

### P3. Wake the agent when a background job ends (new)
- When a `bg_start` job or app session the agent started exits, post a system note into that agent's next turn ("bg job 3 `claude -p …` exited 0 after 4m12s"). First check whether durable jobs or run supervision already offer a wake hook (`docs/superpowers/specs/2026-09-08-durable-jobs-design.md`, `…-run-supervision-design.md`). Use that if it exists, or else add the smallest hook.
- Opt-in per call: `bg_start {…, notify_on_exit?: bool}` (default true for commands longer than the foreground timeout).

### P4. Process hygiene — amends AUA-4
- `bg_start` moves to `Custody` (as AUA-4 says) **and** uses the same `env_clear` + allowlist as `bash` (G5). Add a test that a daemon env var such as `ANIMA_TEST_SECRET` doesn't reach a `bg_start` child.
- `bash` on Unix: run under `Custody` so timeout, cancel and Stop retire the tree. This closes audit M11. Add a test that a `bash` timeout kills a grandchild.
- Add `PAGER=cat`, `GIT_PAGER=cat` to `bash` and `bg_start` env.

### P5. CLI logins (new; passthrough decided with the owner, 2026-10-19)
- Default: rely on each CLI's own login (keychain, `~/.claude`, `~/.codex/auth.json`). These already work through `HOME`/`USERPROFILE`. The skills tell the agent how to detect "not logged in" and to ask the owner to run the login. **Never** have the agent type credentials.
- Owner opt-in: a per-agent **env passthrough** list in Approvals, limited to a fixed set of known names: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, `OPENROUTER_API_KEY`. Values come from the daemon's own secret store (the BYOK store from `2026-08-02-secure-byok-ollama-design.md`), never from the model. It's off by default, set per agent by the owner only, and limited to those known names (no free-form variables).

### P6. Know-how bundles — replaces AUA-6
One community bundle **per CLI**, each holding the CLI skill (and room for a second, related skill). Owners install only what they have. Files go under `bundles/coding-agents/`:

| Bundle | Skill(s) | Covers |
|---|---|---|
| `anima.coding.claude-code` | `claude-code` | everything in AUA-6 for Claude Code, plus the items below |
| `anima.coding.codex` | `codex` | AUA-6's Codex items, plus: needs a git repo (or `--skip-git-repo-check`), `codex review`, `codex exec resume`, sandbox notes |
| `anima.coding.opencode` | `opencode` | `opencode run` (no PTY needed), `--format json`, `-c`/`-s <id>` resume, the TUI via app tools, exiting with Ctrl+C (not `/exit`), `which -a opencode` binary mix-ups |
| `anima.coding.gemini-cli` | `gemini-cli` | `gemini -p` one-shots, `--output-format json`/`stream-json`, `--approval-mode` (default / auto-edit / yolo) and `--sandbox`, session resume, `--include-directories`, Google login vs `GEMINI_API_KEY`, the TUI via app tools, ACP mode. Not in the reference agent; every flag is checked against the installed CLI's `--help` |
| `anima.coding.delegate` | `coding-agent-handoff` (generic) | how to pick a CLI, write a good brief, isolate parallel coders with `git worktree add`, read the diff, run the project's checks, report. Never commit or push unless the owner asks |

Every CLI skill uses the same section order so they read as a family:
1. **Find it**: `command -v` / `where`, `--version`; what to do if it's missing or broken (for example the npm shim with a missing native binary, which this machine has today for `codex`)
2. **Check login**: a cheap call and how a login failure looks (Claude Code: the `result` event has `is_error: true` while `subtype` is `"success"`)
3. **One-shot run**: the exact command, JSON output, how to read the final result and the session id
4. **Follow-up**: resume by id
5. **Long run**: `bg_start` + `bg_wait`, with `notify_on_exit`
6. **Interactive**: `app_start` / `app_send` / `app_read until=…`; first-run trust and permission dialogs; the TUI's "waiting for input" cue
7. **Freedom per task**: map Anima's intent to the CLI's own modes. Read-only review uses `plan` / `--sandbox read-only`; edits use `acceptEdits` / `workspace-write`; full freedom only inside an isolated worktree
8. **Isolation**: `--setting-sources user`, `--settings '{"disableAllHooks":true,"autoMemoryEnabled":false}'`, and why to avoid `--bare` with subscription logins (it never reads OAuth/keychain, verified on 2.1.286)
9. **Verify yourself**: read `git diff`, run the checks, and never trust the CLI's own "done"
10. **Gotchas**

Constraints:
- Each `SKILL.md` must pass `skills/file.rs::parse_skill_file`: `name` ≤ 64, a one-line `description` ≤ 300, front matter ≤ 4 KiB, body ≤ **32 KiB**. The reference agent's Claude Code skill is 754 lines, so ours must be much tighter. Extra keys (`version`, `tags`) are allowed.
- Manifests: `schemaVersion: 1`, `requiredCapabilities: ["skills.import-draft.v1"]`, `nativeToolNames` listing the tools the skill uses (`bash`, `bg_start`, `bg_wait`, `app_start`, …), and attribution to this repo.
- Write from each CLI's `--help` and official docs only. Run every flag against the installed CLI and record the version in the skill.
- Tests: a Rust unit test that `include_str!`s each manifest, runs `parse_manifest` + `validate_manifest`, and runs each inline skill through `parse_skill_file`.

### P7. A "Coder" starting point (new, small)
The reference agent auto-selects a coding toolset and guidance. Our equivalent is an **agent template** in onboarding: Operate access, the coding bundles pre-selected for install if their CLI is found on `PATH`, and a short persona line ("you delegate implementation to coding CLIs and verify their work"). This is design first (`design.pen`, onboarding frames), then `CompanionSetup.tsx`. No new runtime code.

### P8. Structured drive via ACP — first round (decided with the owner, 2026-10-18)
Driving CLIs through their terminal works for every app. A protocol adds typed events, real cancel, and permission requests that go through **our** approvals instead of `--dangerously-*` flags. The vendor-neutral choice is an **ACP (Agent Client Protocol) client** in the daemon. It ships in the same round as P1–P7, not later.

**Engine crate** `packages/core-rust/crates/anima-acp` (no HTTP, DB or daemon types):
- Dependency: the `agent-client-protocol` Rust crate (Zed). Pin an exact version after checking crates.io and its license. If its API doesn't fit tokio + `Custody`, write the JSON-RPC-over-stdio client ourselves; the protocol surface we need is small.
- `AcpSession::start(program, args, cwd, env)` runs the agent under `anima_process::Custody`, sends `initialize` (protocol version, `clientCapabilities: { fs: { readTextFile, writeTextFile }, terminal: false }`), then `session/new` or `session/load` when the agent advertises `loadSession`.
- `prompt(text)` sends `session/prompt` and streams `session/update` notifications (agent message chunks, thoughts, tool calls and their updates, plan entries) into a bounded event log (1 MiB, like app sessions) until the prompt's stop reason arrives.
- `cancel()` sends `session/cancel`; `stop()` retires the tree through `Custody`.
- Client-side handlers are traits the host implements, so the crate stays host-agnostic:
  - `fs/read_text_file` / `fs/write_text_file`: refused outside the session's cwd; the host applies its own Read/Write policy.
  - `session/request_permission`: the host returns one of the offered options.
- `terminal/*`: not offered in round 1 (`terminal: false`); agents fall back to their own shell, which still runs inside our `Custody` tree.
- Tests use a scripted fake ACP agent (a small test binary, like `tests/fixtures/mcp_stdio_peer.rs`): handshake, prompt with streamed updates, cancel mid-prompt, permission round-trip, fs refusal outside cwd, process exit mid-prompt, and Stop killing a grandchild.

**Daemon tools** `hosts/rust-daemon/src/tools/process/acp.rs`:
- `acp_start {command, cwd?, session_id?}`: Exec class, `CommandPrefix` rules (so "allow `claude-code-acp`" works), same env allowlist as `bash` (plus P5 passthrough when the owner turned it on). Returns our session id and the agent's name, capabilities and auth methods.
- `acp_prompt {id, text, wait_ms?}`: Exec class. Returns the events so far, or the final stop reason if it finished within `wait_ms`; long prompts keep running.
- `acp_read {id, wait_ms?}` (Read), `acp_cancel {id}` (Exec), `acp_stop {id}` (Exec), `acp_list` (Read).
- A daemon-owned manager shared with app sessions: at most 4 live sessions (app + ACP) per agent, 30 min idle stop, all retired on daemon exit. Helpers never get these tools.
- **Permission mapping** (the key piece). Map the ACP tool-call `kind` to our risk class: `read`/`search` → Read, `edit`/`delete`/`move` → Write, `execute` → Exec, `fetch` → Network, anything else → Exec. Then apply the agent's policy: Allow → pick the agent's `allow_once` option; Ask → raise a normal Anima approval card (with the ACP tool title, its file paths and its command), and answer with the owner's choice; Deny → `reject_once`. Never pick `allow_always` on the owner's behalf. Our rules apply too: `PathGlob` to edit paths, `CommandPrefix` to execute commands.
- Native action receipts for start, prompt, each permission answer and each fs write.
- Register in `is_process_tool`, `approvals/policy.rs` tables + `matcher_kinds`, `OPERATE_TOOLS`, `tools/tests.rs` schema rows; `acp_read`/`acp_list` in `QUIET_TOOLS`. Completion of a prompt posts a wake note through the P3 hook.

**Web and design:**
- `design.pen` first: the **Chat · Using an app** frame (AUA-1) gets an ACP variant: the agent's message stream, a tool-call list with status, the plan entries, and Cancel / Stop.
- Approval cards from ACP permission requests reuse `needs-you.ts`, with headline "Claude Code wants to edit `src/x.ts`". Labels: "Started a coding agent", "Asked a coding agent", "Stopped a coding agent".

**Know-how:** each CLI skill (P6) gets an **ACP** section: which adapter to run (Claude Code via Zed's Claude Code ACP adapter, Codex via `codex-acp`, OpenCode via `opencode acp`, Gemini CLI via its built-in ACP flag), how to install it, and when to prefer ACP (multi-turn work the owner wants to supervise) versus `-p`/`exec` one-shots (quick, scripted tasks). Verify each adapter's package name and command at implementation time.

**Out of scope:** a Codex-only `app-server` runtime like the reference agent's (ACP covers Codex), and Anima as an ACP *server* (G12).

**Order:** P8 depends on P4 (Custody in the daemon) and P3 (wake hook). Its engine crate and fake agent can be built in parallel with P1, since they touch different files.

## Verification

Same as AUA, plus:
- `bun x nx run core-rust:test` covers the responder and wait tests (P1, P2).
- `CARGO_BUILD_JOBS=4 bun x nx run rust-daemon:test --skipNxCache` covers the env-leak, bash-tree-kill, `bg_wait`, wake-note, bundle-manifest and ACP tool/permission-mapping tests. `anima-acp` tests run in `core-rust:test`.
- Manual on macOS (this machine has Claude Code 2.1.286; **`codex` is installed but broken**, with the native binary missing, so reinstall it before the Codex checks; `opencode` and `gemini` aren't installed):
  - Claude Code: one-shot → resume follow-up → interactive in an app session watched in chat → Stop → `ps -ef | grep -i claude` is empty.
  - Codex: `codex exec --json` in a git repo; the TUI in an app session must reach its prompt (proves the query responder).
  - OpenCode: `opencode run --format json`, then the TUI, then exit with Ctrl+C.
  - Gemini CLI: `gemini -p … --output-format json`, a resume follow-up, then the TUI in an app session.
  - Long `claude -p` via `bg_start` with `notify_on_exit`: the agent is woken with no polling turns.
  - ACP: an agent with exec=Ask drives Claude Code through its ACP adapter. An edit raises an Anima approval card; Deny is honoured; Cancel stops the prompt mid-turn; Stop leaves no processes. Repeat with `opencode acp`, `codex-acp` and Gemini CLI's ACP mode once those are installed.

## Decisions (2026-10-18/19)

- OpenCode was meant by "open Claude"; OpenClaw is out of scope.
- The ACP client (P8) ships in the first round.
- API-key passthrough (P5): owner opt-in per agent, off by default, known names only.
- First-round bundles: Claude Code, Codex, OpenCode, Gemini CLI, plus the generic handoff skill. Grok, OpenHands, Antigravity, Blackbox and Aider wait.

## Implementation notes (2026-10-19, first round built, uncommitted)

Where the build differs from the steps above:
- **Windows app sessions** can't put a ConPTY child in a Job Object without `unsafe` (portable-pty only exposes a raw handle). Stop kills the tree with `taskkill /T /F`, reported as `Containment::ProcessTree`. This is not verified on Windows yet.
- **Terminal queries** are answered on Unix only; ConPTY answers them itself. The cursor reply uses the real cursor from the `vt100` screen, where the reference agent sends a fixed 1;1.
- **Live sessions** live in one process-wide registry (`tools/process/live.rs`), like the process runtime, rather than a context field. REST Stop/Cancel use the same registry.
- **ACP permissions** become approvals under four permission-only names: `acp_edit` (Write, PathGlob), `acp_execute` (Exec, CommandPrefix), `acp_fetch` (Network, Domain) and `acp_other` (Exec). Rules may name them. A request waits in a per-session queue until one of the agent's own `acp_prompt`/`acp_read` calls judges it through the run's gate. With no gate (no run), a request that would need the owner is cancelled.
- **`anima-acp`** uses its own small JSON-RPC client rather than Zed's crate (see the crate's doc comment for why).
- **Key passthrough values** come from the daemon's environment (where model providers read them), not a separate BYOK store. Which names are enabled is saved per agent in the control plane snapshot (`codingKeys`).
- **Know-how install** has no dedicated endpoint. `GET /api/coding-apps` returns each bundle's manifest text, and the web runs the normal community-bundle review → install for the new agent.
- **Coder template** lives in first-run onboarding (`CompanionSetup`). The app has no "add a second agent" dialog yet.
- **Live tool previews** are capped at 2 KiB. `app_*`/`acp_*` results get a compact, still-valid JSON preview: the bottom of the screen, the newest events.
- **Helper worktrees** (`spawn_helper{,_async}` with `worktree: true`) live in the new `anima-worktree` crate (git CLI only, hardened like the reference agent's `noninteractive_git_env`). Unlike the reference agent we refuse the spawn outside a git repo instead of silently sharing the checkout, exclude `.worktrees/` via `.git/info/exclude` instead of editing `.gitignore`, and keep dirty worktrees in place instead of archiving them to a ref. Ignored files a helper creates count as changes. Checkpointed writes follow the helper's worktree root (only for a helper config whose root is inside an `anima-helper-*` worktree).
- **Not built:** a "See the change" diff view in the ACP card.

## Second round (2026-10-19)

- **Mid-run wake-ups:** `bg_start {notify_on: [...]}` takes up to 4 patterns (text or `/regex/`). Each pattern wakes the agent once while the job keeps running, unless the agent has already read that line. The reference agent matches substrings only and rate-limits with strikes; ours fires once per pattern, so it needs no limiter.
- **Helper worktrees:** `spawn_helper`/`spawn_helper_async` take `worktree: true` (and an optional `repo`). The `anima-worktree` crate runs hardened `git`. It writes `/.worktrees/` to `.git/info/exclude` rather than `.gitignore`. It refuses instead of silently sharing the checkout, removes clean worktrees, keeps dirty ones and reports them, and sweeps on startup.
- **Importing Claude Code / Codex conversations:** owner-only routes, plus the "Import from Claude Code or Codex" dialog in the conversations drawer. An imported conversation becomes a normal chat (origin `import`) that can be searched and continued. Secrets are masked, at most 2,000 messages are kept, and importing the same conversation again opens the existing chat. Wrappers are stripped per block, so a prompt typed next to a `<system-reminder>` survives.
- **Fix:** on macOS `killpg` answers EPERM for a group of unreaped zombies. `Custody` now accepts that as long as it then proves the group empty. This removes an intermittent `bg_stop could not confirm…` failure.
- **Still not built:** Anima as an MCP server, Anima as an ACP server, extra CLI bundles (Grok, OpenHands, Antigravity, Blackbox), and importing settings or skills from those tools (the reference agent's `import-agent`).
- **Live check, 2026-10-19:** the real Claude Code ACP adapter (0.93.0) fixed a bug with no permission request at all, because it started in its own non-asking mode. `anima-acp` now switches an agent into its asking mode when it offers one (`AcpLaunch.ask_modes`, the daemon passes `default`). After that change, its edit arrived as a permission request first. The manual test is `cargo test -p anima-acp --test live_claude_code -- --ignored`.
- **End-to-end run, 2026-10-19 (macOS, throwaway workspace).** The setup: onboarding with the Coder template and Operate, then a ChatGPT `gpt-6-luna` model. The agent did each of these from chat:
  1. Fixed a bug with a one-shot `claude -p` and verified it ($0.054).
  2. Did the same over ACP. With Write set to Ask, the coding agent's edit appeared as an approval card; after Allow once it edited, and the agent verified the fix and stopped the session.
  3. Opened the Claude Code TUI with `app_start`, read its trust dialog and stopped it.

  No CLI processes were left behind. Importing a real Claude Code conversation worked, along with dedupe and search.

  Rough edges found:
  - The ChatGPT model list doesn't refresh into the dropdown; a custom model id worked.
  - An unsaved provider choice resets when the pane resizes.
  - The `acp_edit` approval card shows raw JSON arguments.
  - The ACP card showed IDLE while a permission was pending, because the latest snapshot was from `acp_start`.
  - Imported transcripts show one `[ran tool: …]` line per call.
  - A broken Codex install still counts as "found" and gets its bundle.
- **Rough edges fixed, 2026-10-19:**
  - The ChatGPT "Refresh models" button now re-reads after a connection made outside the picker.
  - ACP approval cards show File/Command/Address/Session rows, with the full request one click away.
  - The ACP card shows RUNNING while a later prompt is in flight.
  - Imported tool lines collapse (`[ran tools: Bash ×2, Read]`).
  - Broken apps get no know-how bundle.
  - Not reproduced: the provider picker reset (a resize keeps the draft).
- **Plain-language test (fresh chats, no hints):**
  - "use Claude Code to fix the failing test in calc": the agent loaded the skills, briefed `claude -p`, verified the fix, didn't commit.
  - "…but let me approve its changes": the first run planned twice and hit the 8-round reply limit. Fixes: agents holding `app_start`/`acp_start` default to `CODING_TOOL_ROUNDS = 24` (new `AgentRuntime::set_default_tool_iterations`; an explicit setting wins), and the hand-off skill now says to load skills before one plan, and that "let me approve" means worktree → diff → apply only on yes (ACP cards appear only when Write is Ask). On retest it proposed one plan, fixed the bug in `.worktrees/calc-fix`, verified it, and left the checkout untouched for approval.
- **Dev-loop note (macOS):** the ChatGPT login lives in 6 keychain entries, and every daemon rebuild produces a new ad-hoc signature, so macOS prompts again and often won't remember "Always Allow". The workaround for a dev build is to delete the entries and reconnect. A follow-up should consider a signed dev binary or one keychain entry.
- **Hidden pane:** a page in a hidden or minimized browser pane pauses its live updates, so cards and the inbox look empty until it is visible again. That is the browser, not a bug.
