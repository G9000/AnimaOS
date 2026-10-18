---
name: codex
description: "Drive the OpenAI Codex CLI (codex) as a coding worker: codex exec one-shots with JSONL events, exec resume, codex review, long background runs, the TUI in an app session, or ACP via codex-acp. Use when the owner wants Codex to build, fix or review code."
version: 1.1.0
tags: [coding-agent, codex, openai, delegation, cli]
---

# Codex CLI

Checked against codex-cli 0.163.0 `--help` (`codex`, `exec`, `exec resume`, `exec review`, `login`), a probe of `login status` and `exec` without a login, and the official non-interactive docs on 2026-10-19. The copy installed on the dev machine was broken, so the help came from `npx @openai/codex@0.163.0`.

You stay responsible for the result. Give Codex a clear brief, give it only the freedom the task needs, then check its work yourself. Never commit, push or open a PR unless the owner asked for that. For briefs, picking a CLI and parallel worktrees, also load the `coding-agent-handoff` skill if it is installed.

Tool calls below use Anima's tools: `bash {command, timeout?, cwd?}`, `bg_*`, `app_*`, `acp_*`. `cwd` is workspace-relative.

## 1. Find it

```
bash {command: "command -v codex && codex --version"}
```

Windows: `where codex`. Expect `codex-cli 0.x`.

- Missing: tell the owner. Installing is their call (`npm install -g @openai/codex`, or Homebrew). Don't install it yourself.
- Broken npm install: `codex --version` prints `Error: spawn .../@openai/codex-darwin-arm64/vendor/.../codex ENOENT`. The JS shim is there, but its native binary package is missing. Ask the owner to reinstall it (`npm install -g @openai/codex@latest`). Don't patch `node_modules`. ACP mode (section 9) brings its own Codex binary, so it can work even while this install is broken.
- `codex doctor` diagnoses the install, config, auth and runtime. Report its output when something is off.

## 2. Check login

```
bash {command: "codex login status"}
```

Without a login it prints `Not logged in` and exits 1 (verified). Exit 0 means a login is configured.

If you skip this check, `codex exec` without a login doesn't fail fast. It emits `{"type":"error","message":"Reconnecting... 2/5 (unexpected status 401 Unauthorized ...)"}` events and retries for a while.

Login failed: stop and ask the owner to run `codex login` in their own terminal (browser ChatGPT login, or `codex login --device-auth`). An API key is the owner's choice: through Anima's per-agent key passthrough (`CODEX_API_KEY` or `OPENAI_API_KEY`), or `codex login --with-api-key`, which reads the key from stdin. Never pipe, type or echo a key yourself, and never read `~/.codex/auth.json`.

## 3. One-shot run

```
bash {
  command: "codex exec --json -s workspace-write - < ../briefs/fix-parser.md",
  cwd: "repo",
  timeout: 110000
}
```

- The prompt is an argument, or `-` to read it from stdin (shown above, good for long briefs written with `write_file`).
- **Always give it stdin.** If stdin is open and not a file, `exec` prints `Reading additional input from stdin...` and waits to append it. With an inline prompt, add `< /dev/null`.
- **It needs a git repo.** Outside one it exits 1 with `Not inside a trusted directory and --skip-git-repo-check was not specified.` Use `--skip-git-repo-check` only for scratch folders.
- `-C <dir>` sets the agent's working root, if it should differ from `cwd`.
- `exec` runs in a **read-only** sandbox by default. Without `-s workspace-write` it can't change files, even if it says it did.

`--json` prints one event per line (JSONL):

| Event | What to take from it |
|---|---|
| `{"type":"thread.started","thread_id":"..."}` | the session id for resume |
| `turn.started` | |
| `item.started` / `item.completed` with `item.type` | `agent_message` (`item.text`), `reasoning`, `command_execution` (`command`, `status`), `file_change`, `error` |
| `turn.completed` | done; carries `usage` |
| `turn.failed` | failed; read its error |
| `error` | transport or API problem (for example 401s while retrying) |

The final answer is the last `item.completed` whose item is an `agent_message`. To get only that text, add `-o <file>` (`--output-last-message`) and read the file.

Other flags from 0.163.0 `exec --help`: `-m <model>`, `-c key=value` (TOML config override), `-p <profile>` (a config profile, **not** a prompt), `--output-schema <file>` (JSON Schema for the final answer), `-i <image>`, `--add-dir <dir>` (extra writable folder), `--ephemeral` (no session files, cannot be resumed).

### Code review

```
bash {command: "codex exec review --base main < /dev/null", cwd: "repo", timeout: 110000}
```

`--uncommitted` reviews staged, unstaged and untracked changes; `--base <branch>` reviews against a branch; `--commit <sha>` reviews one commit. Add custom instructions as the prompt (or `-` for stdin). `codex review` is the same thing at the top level.

## 4. Follow-up

```
bash {command: "codex exec resume <thread_id> --json -c 'sandbox_mode=\"workspace-write\"' 'Now add a regression test for the empty input case' < /dev/null", cwd: "repo"}
```

- `exec resume` has **no `-s` flag**. Set the sandbox with `-c sandbox_mode="..."` (a real config key in 0.163.0).
- `--last` picks the most recent session. It is filtered to the current folder unless you add `--all`. Prefer the explicit id.
- `exec fork <id>` branches into a new session and leaves the original alone.

## 5. Long run

```
bg_start {command: "codex exec --json -s workspace-write - < ../briefs/refactor-auth.md", cwd: "repo", notify_on_exit: true}
```

- With `notify_on_exit` you may end your turn; a note wakes you when it exits. Tell the owner what is running.
- To hear about something mid-run (a failing test, a question it prints), add `notify_on: ["FAILED", "/error|panic/"]` (up to 4 patterns; each wakes you once while it keeps running).
- To wait inside the turn: `bg_wait {id, timeout_ms: 600000, until: "turn\\.(completed|failed)"}`.
- `bg_output {id}` shows recent events. A quiet run is not a stuck run.
- `bg_stop {id}` ends the whole process tree.

## 6. Interactive (app session)

```
app_start {command: "codex --no-daemon -s workspace-write -a on-request", cwd: "repo", cols: 120, rows: 40}
app_read {id, wait_ms: 5000}
```

- `--no-daemon` runs without the shared background app-server, so `app_stop` really ends everything.
- `-a on-request` lets the model ask before risky commands. Those questions appear on screen.
- First-run screens: a sign-in screen means you stop and ask the owner to log in themselves. A folder-trust or update question gets read and answered for the folder the owner asked you to work in; skip updates.
- Waiting for input: an empty input box at the bottom of the screen. Read the screen to learn this version's exact wording before you rely on it as an `until` value.
- Send a task: `app_send {id, text: "..."}`, then `app_send {id, keys: ["Enter"]}`, then `app_read {id, wait_ms: 30000}`. That returns after 30 s of quiet (max 120 s), when `until` text appears, or when the app exits.
- Approval questions: read the whole command first. Approve only what the brief covers, and ask the owner about anything else. Never approve on a timer.
- Finish with `keys: ["Ctrl+C"]` (twice if the first only clears the input), then always `app_stop {id}`. An agent has at most 4 live app and ACP sessions, and idle ones stop after 30 min.
- Resume later in the TUI: `codex resume <id>` or `codex resume --last`.

## 7. Freedom per task

| Task | Flags |
|---|---|
| Read-only: review, explain, plan | `-s read-only` (the `exec` default), or `codex exec review` |
| Edit inside the repo | `-s workspace-write`: writes stay in the workspace and commands run sandboxed |
| Full freedom | `-s danger-full-access` or `--dangerously-bypass-approvals-and-sandbox`, only inside an isolated worktree or container the owner approved for this task |

- `--approve-for-me` routes approval requests through Codex's automatic review, using the workspace-write sandbox. Treat it as "edits plus Codex's own judgment"; you still verify.
- `--full-auto` is no longer supported (docs). Use `-s workspace-write`.
- Inside the sandbox, commands that need the network (installs, some tests) may fail. Report that to the owner; don't escalate to full access on your own.
- Never use `--dangerously-bypass-hook-trust`.

## 8. Isolation

- `--ignore-user-config` skips `$CODEX_HOME/config.toml` but keeps the login. Use it when the owner's config (custom MCP servers, profiles, permissive defaults) shouldn't apply.
- `--ignore-rules` skips user and project execpolicy `.rules` files.
- `--strict-config` errors on unknown config fields instead of ignoring them.
- `--ephemeral` leaves no session files behind (but nothing to resume).
- The repo's `AGENTS.md` is read as project instructions. That is usually wanted, but read it first in an unfamiliar repo.
- Keep each Codex run in its own checkout. Two writers in one folder collide.

## 9. ACP mode

Codex speaks ACP through `codex-acp`: npm package `@agentclientprotocol/codex-acp` (2.2.2 on 2026-10-19), binary `codex-acp`. `@zed-industries/codex-acp` is deprecated. The package bundles a compatible Codex binary; set `CODEX_PATH=/path/to/codex` to use another one.

```
acp_start {command: "npx -y @agentclientprotocol/codex-acp", cwd: "repo"}
acp_prompt {id, text: "<brief>", wait_ms: 60000}
acp_read {id, wait_ms: 60000}
acp_cancel {id}
acp_stop {id}
```

- The first `npx` run downloads the adapter from npm. If the owner hasn't set it up, ask before the first download.
- `acp_start` returns the agent's auth methods: ChatGPT login, or an API key from `CODEX_API_KEY` / `OPENAI_API_KEY`. If it needs auth, ask the owner to log in or to turn on key passthrough. Don't do it yourself.
- Codex's permission requests become the owner's normal Anima approval prompts. Don't add bypass flags.
- To continue an earlier ACP session, pass `session_id` to `acp_start` when the agent advertises session loading.
- **Always `acp_stop` when the task is done**, before you report, and again after every follow-up round. Stop it even if the owner may want more: `acp_start` with `session_id` picks the conversation back up. An idle session still holds one of the 4 live slots until the 30-minute idle timeout.
- Prefer ACP for supervised multi-turn work with per-action approvals and a real cancel. Prefer `exec --json` for quick scripted tasks, reviews and parallel batch runs.

## 10. Verify yourself

Codex's "done" is a claim, not evidence.

1. `git status --porcelain` and `git diff --stat`, then read the diff of every file that matters, including new untracked files.
2. Run the project's checks yourself (tests, lint, typecheck).
3. If the change has a web page or other screen, logic tests don't show what the page does. Run the project's own browser or end-to-end tests if it has them. If you have the browser tools and the owner installed a route for the page, open it with `browser_navigate` and look with `browser_observe`. Otherwise read the page's load and event code and work out what it shows on first load with the default values, after valid input, and after invalid or cleared input. Either way, tell the owner whether you opened the page; if you didn't, say so and name what they should click to check.
4. Compare the result with the brief. Look for scope creep, removed tests, new dependencies and config changes.
5. If nothing changed, check whether it ran in the default read-only sandbox.
6. Stop every ACP or app session you started for this task (`acp_stop`, `app_stop`) before you report, including after a follow-up round.
7. Report to the owner: changed files, the checks you ran and their results, `usage` from `turn.completed`, the thread id for follow-ups, and open questions. Don't commit or push unless asked.

## 11. Gotchas

- `-p` is `--profile` in Codex, not "prompt" as in Claude Code or Gemini CLI.
- An open stdin makes `exec` wait (`Reading additional input from stdin...`). Use `< /dev/null` or `-` with a file.
- Not a git repo: exit 1. The `--skip-git-repo-check` flag is for scratch folders only.
- `exec resume` takes no `-s`; use `-c sandbox_mode="..."`.
- A missing login shows up as retrying 401 `error` events, not an immediate failure.
- Don't run `codex apply` (it applies a Codex Cloud diff to the working tree), `codex delete`, `codex logout`, `codex update`, or `codex mcp ...` / `codex plugin ...` changes unless the owner asked.
- `codex cloud`, `codex app` and `remote-control` reach outside this machine. Don't use them for delegated work.
