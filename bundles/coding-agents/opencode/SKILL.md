---
name: opencode
description: "Drive the OpenCode CLI (opencode) as a coding worker: opencode run one-shots with JSON events, session resume, long background runs, the TUI in an app session, or ACP via opencode acp. Use when the owner wants OpenCode to build, fix or review code."
version: 1.1.0
tags: [coding-agent, opencode, delegation, cli]
---

# OpenCode

Checked on 2026-10-19 against opencode 1.19.3 (`npx opencode-ai@1.19.3`): its `--help` for `run`, `acp`, `providers`, `session` and `debug`, its resolved config (`opencode debug config`, `opencode debug agent plan`), and its `run` source for the JSON events and permission handling.

You stay responsible for the result. Give OpenCode a clear brief, give it only the freedom the task needs, then check its work yourself. Never commit, push or open a PR unless the owner asked for that. For briefs, picking a CLI and parallel worktrees, also load the `coding-agent-handoff` skill if it is installed.

Tool calls below use Anima's tools: `bash {command, timeout?, cwd?}`, `bg_*`, `app_*`, `acp_*`. `cwd` is workspace-relative.

## 1. Find it

```
bash {command: "which -a opencode; opencode --version"}
```

Windows: `where opencode`. Expect a plain version such as `1.19.3`.

- Several copies are common: npm `opencode-ai`, Homebrew, and the install script's `~/.opencode/bin/opencode`. `which -a` lists all of them. If behavior differs from what the owner sees, call the one they use by its full path.
- Check that `opencode --help` lists `run`, `acp` and `providers`. If it doesn't, it is a different, unrelated program with the same name.
- Missing: tell the owner. Installing is their call (`npm install -g opencode-ai`, Homebrew, or the install script). Don't install it yourself.

## 2. Check login

```
bash {command: "opencode providers list"}
```

`opencode auth list` is the same command. It shows the stored credentials and the providers configured from the environment. OpenCode works with many providers, so "logged in" means the model you plan to use has a credential.

End-to-end check (one tiny request):

```
bash {command: "opencode run --format json 'Reply with exactly: OK' < /dev/null", cwd: "repo"}
```

Success: a `text` event whose `part.text` is `OK`. Failure: an `{"type":"error",...}` event and exit code 1.

Login failed: stop and ask the owner to run `opencode providers login` in their own terminal. Never type keys or tokens, and never read OpenCode's auth files.

## 3. One-shot run

`opencode run` needs no terminal (PTY).

```
bash {
  command: "opencode run --format json --title fix-parser \"$(cat ../briefs/fix-parser.md)\" < /dev/null",
  cwd: "repo",
  timeout: 110000
}
```

With `--format json`, every line is `{"type": ..., "timestamp": ..., "sessionID": "ses_...", ...}` (from the 1.19.3 source):

| `type` | Payload |
|---|---|
| `step_start` / `step_finish` | `part`, one model step |
| `text` | `part.text`, assistant text; the last one is the answer |
| `reasoning` | `part.text`, only with `--thinking` |
| `tool_use` | `part.tool`, `part.state.status` (`completed` or `error`), and its input and output |
| `error` | `error`, the session error; exit code 1 |

`sessionID` is on every line; keep it for follow-ups. The run ends when the session goes idle.

Other `run` flags from 1.19.3: `-m provider/model`, `--agent <name>`, `--variant <high|max|minimal...>` (provider-specific reasoning effort), `-f <file>` (attach files), `--dir <path>`, `--title <name>`, `--command <cmd>` (run a slash command), `--thinking`, `--pure` (no external plugins).

## 4. Follow-up

```
bash {command: "opencode run --format json -s <sessionID> 'Now add a regression test for the empty input case' < /dev/null", cwd: "repo"}
```

- `-c/--continue` picks the last session; prefer the explicit `-s <sessionID>`.
- `--fork` (with `-s` or `-c`) branches into a new session.
- `opencode session list --format json -n 10` lists recent sessions. `opencode export <sessionID>` dumps one as JSON.

## 5. Long run

```
bg_start {command: "opencode run --format json --title refactor-auth \"$(cat ../briefs/refactor-auth.md)\" < /dev/null", cwd: "repo", notify_on_exit: true}
```

- With `notify_on_exit` you may end your turn; a note wakes you when it exits. Tell the owner what is running.
- To hear about something mid-run (a failing test, a question it prints), add `notify_on: ["FAILED", "/error|panic/"]` (up to 4 patterns; each wakes you once while it keeps running).
- To wait inside the turn: `bg_wait {id, timeout_ms: 600000}`, which waits for the exit. No single event marks the end, so wait for the exit rather than for a pattern.
- `bg_output {id}` shows recent events. `bg_stop {id}` ends the whole process tree.

## 6. Interactive (app session)

```
app_start {command: "opencode", cwd: "repo", cols: 120, rows: 40}
app_read {id, wait_ms: 5000}
```

- `opencode --mini` starts a minimal interface that is easier to read in `app_read`. `opencode -s <sessionID>` opens an earlier session.
- Send a task: `app_send {id, text: "..."}`, then `app_send {id, keys: ["Enter"]}`. If the text stays in the input box, send Enter once more.
- Wait with `app_read {id, wait_ms: 30000}`. It returns after 30 s of quiet (max 120 s), when `until` text appears, or when the app exits. Waiting for input looks like this: the input box is empty and nothing is streaming.
- `Tab` cycles the primary agents (`build` and `plan`). Check the status line for which one is active before sending edits.
- Permission questions appear on screen. Read them, approve only what the brief covers, and ask the owner about the rest. Never approve on a timer.
- **Exit with Ctrl+C** (`keys: ["Ctrl+C"]`), not `/exit`, which earlier versions treated as an agent picker. Then always `app_stop {id}`. An agent has at most 4 live app and ACP sessions, and idle ones stop after 30 min.

## 7. Freedom per task

OpenCode's defaults are permissive. In 1.19.3 the `build` agent allows **everything**, including edits and every shell command, except `doom_loop` and `external_directory` (both "ask"). In `opencode run`, every "ask" is **auto-rejected** (it prints `permission requested: ...; auto-rejecting`), and the question tools are denied. So pick the limits up front with inline config (`OPENCODE_CONFIG_CONTENT`; check the result with `opencode debug config`):

| Task | Command shape |
|---|---|
| Read-only review | `OPENCODE_CONFIG_CONTENT='{"permission":{"edit":"deny","bash":{"*":"deny","git diff*":"allow","git log*":"allow","git status*":"allow"},"webfetch":"deny"}}' opencode run --agent plan ...` |
| Edits, no shell | `OPENCODE_CONFIG_CONTENT='{"permission":{"edit":"allow","bash":"deny"}}' opencode run ...` |
| Edits and the project's checks | `OPENCODE_CONFIG_CONTENT='{"permission":{"edit":"allow","bash":{"*":"deny","npm test*":"allow","git diff*":"allow"}}}' opencode run ...` |
| Full freedom | the default `build` agent with no limits, or `--auto` (approves every "ask" too), only inside an isolated worktree or container the owner approved for this task |

- In 1.19.3 the `plan` agent denies edits (except its own plan files) but still **allows shell commands**. That is why the read-only row also limits `bash`.
- Rules are matched in order and the last match wins, so put `"*"` first and the exceptions after it.
- A denied or auto-rejected tool shows up as a `tool_use` event with `status: "error"`. Tell the owner if the task needed it.

## 8. Isolation

- A repo's own `opencode.json` and `.opencode/` folder (agents, commands, plugins, MCP servers, permissions) load automatically. In an unfamiliar repo, read them first, or set `OPENCODE_DISABLE_PROJECT_CONFIG=1`.
- `--pure` runs without external plugins.
- OpenCode also loads Claude Code material (such as `~/.claude/skills` and `CLAUDE.md`). Set `OPENCODE_DISABLE_CLAUDE_CODE=1` to keep the owner's Claude Code setup out of the run.
- `opencode run` starts a local server on a random port (`--port` to fix it). Don't use `serve`, `web` or `--mdns` for delegated work; they listen on the network.
- Keep each OpenCode run in its own checkout. Two writers in one folder collide.

## 9. ACP mode

OpenCode has ACP built in: `opencode acp` speaks ACP over stdin/stdout (`--cwd <dir>` sets the working folder).

```
acp_start {command: "opencode acp", cwd: "repo"}
acp_prompt {id, text: "<brief>", wait_ms: 60000}
acp_read {id, wait_ms: 60000}
acp_cancel {id}
acp_stop {id}
```

- OpenCode's permission requests become the owner's normal Anima approval prompts. Leave the "ask" defaults in place; that supervision is the point of ACP.
- The same provider credentials apply. If the first prompt fails with an auth error, ask the owner to log in.
- To continue an earlier ACP session, pass `session_id` to `acp_start` when the agent advertises session loading.
- **Always `acp_stop` when the task is done**, before you report, and again after every follow-up round. Stop it even if the owner may want more: `acp_start` with `session_id` picks the conversation back up. An idle session still holds one of the 4 live slots until the 30-minute idle timeout.
- Prefer ACP for supervised multi-turn work with per-action approvals and a real cancel. Prefer `opencode run --format json` for quick scripted tasks and parallel runs.

## 10. Verify yourself

OpenCode's "done" is a claim, not evidence.

1. `git status --porcelain` and `git diff --stat`, then read the diff of every file that matters, including new untracked files.
2. Run the project's checks yourself (tests, lint, typecheck).
3. If the change has a web page or other screen, logic tests don't show what the page does. Run the project's own browser or end-to-end tests if it has them. If you have the browser tools and the owner installed a route for the page, open it with `browser_navigate` and look with `browser_observe`. Otherwise read the page's load and event code and work out what it shows on first load with the default values, after valid input, and after invalid or cleared input. Either way, tell the owner whether you opened the page; if you didn't, say so and name what they should click to check.
4. Compare the result with the brief. Look for scope creep, removed tests, new dependencies and config changes, including edits to `opencode.json` or `.opencode/`.
5. Scan the JSON events for `tool_use` with `status: "error"`; those are steps it could not do.
6. Stop every ACP or app session you started for this task (`acp_stop`, `app_stop`) before you report, including after a follow-up round.
7. Report to the owner: changed files, the checks you ran and their results, the session id for follow-ups, and open questions. Don't commit or push unless asked.

## 11. Gotchas

- In `run`, `-p` is `--password` (for `--attach`), not a prompt. The message is the positional argument.
- Default permissions allow everything, and "ask" rules are silently auto-rejected in `run`. Decide the limits before launching.
- `which -a opencode` first when behavior is odd; several installs often coexist.
- Exit the TUI with Ctrl+C, not `/exit`.
- `opencode pr <number>` checks out a PR branch in the working tree. Don't run it in the owner's checkout without asking.
- Don't run `opencode upgrade`, `uninstall`, `providers logout`, `plugin`, `mcp ...`, `session delete` or `db` unless the owner asked.
- `--variant` values depend on the provider. An unknown value is an error, not a fallback.
