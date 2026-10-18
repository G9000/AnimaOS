---
name: claude-code
description: "Drive the Claude Code CLI (claude) as a coding worker: one-shot -p runs with JSON results, resume, long background runs, the TUI in an app session, or ACP. Use when the owner wants Claude Code to build, fix, refactor or review code."
version: 1.1.0
tags: [coding-agent, claude-code, delegation, cli]
---

# Claude Code

Checked against Claude Code 2.1.286 `--help`, a live `-p` run, the first-run TUI screens and the ACP adapter handshake on 2026-10-19. If the installed version differs a lot, run `claude --help` and trust it over this skill.

You stay responsible for the result. Give Claude Code a clear brief, give it only the freedom the task needs, then check its work yourself. Never commit, push or open a PR unless the owner asked for that. For briefs, picking a CLI and parallel worktrees, also load the `coding-agent-handoff` skill if it is installed.

Tool calls below use Anima's tools: `bash {command, timeout?, cwd?}`, `bg_*`, `app_*`, `acp_*`. `cwd` is workspace-relative.

## 1. Find it

```
bash {command: "command -v claude && claude --version"}
```

Windows: `where claude`. Expect `2.1.x (Claude Code)`.

- Missing: tell the owner. Installing is their call (`npm install -g @anthropic-ai/claude-code`, or the native installer). Don't install it yourself.
- Broken (stack trace, `ENOENT` for a native binary, a version that won't print): run `claude doctor` and report its output. Don't repair global installs yourself.

## 2. Check login

```
bash {command: "claude auth status"}
```

JSON with `"loggedIn": true` and an `authMethod` (`claude.ai` is a subscription login). It also prints the account email; don't repeat it needlessly.

End-to-end check (one tiny request):

```
bash {command: "claude -p 'Reply with exactly: OK' --output-format json --tools '' < /dev/null", cwd: "repo"}
```

Check `is_error` first. The final `result` object can have `subtype: "success"` while `is_error: true`, for example with an expired login; then `result` holds the error text.

Login failed: stop and ask the owner to run `claude` (or `claude auth login`) in their own terminal. Never type credentials, tokens or keys, never run `claude setup-token` for them, and never read credential files under `~/.claude`.

## 3. One-shot run

The standard shape (edits allowed, nothing else; see section 7 for other modes):

```
bash {
  command: "claude -p \"$(cat ../briefs/fix-parser.md)\" --output-format json --permission-mode acceptEdits --permission-prompts none --setting-sources user --settings '{\"disableAllHooks\":true,\"autoMemoryEnabled\":false}' < /dev/null",
  cwd: "repo",
  timeout: 110000
}
```

- Write long briefs to a file first (`write_file`) and pass them with `"$(cat file)"`; keep the file outside the repo, or delete it before you read the diff.
- `< /dev/null`: with `-p`, piped stdin is read as extra input. Close it when you aren't piping anything.
- Anything that would take longer than about 2 minutes belongs in section 5, not `bash`.

The output is one JSON object. Fields that matter (verified on 2.1.286):

| Field | Meaning |
|---|---|
| `is_error` | the real success flag |
| `subtype` | `success`, or an error kind; not enough on its own |
| `result` | the final text answer |
| `session_id` | keep it for follow-ups |
| `permission_denials` | tool calls that were refused; tell the owner if the task needed them |
| `num_turns`, `total_cost_usd`, `terminal_reason` | effort, list-price cost estimate, why it stopped |

For progress events use `--output-format stream-json --verbose`: newline-delimited events (`system` with subtype `init` carrying `session_id`, `assistant`, `user` tool results, then `result`). `system` events such as `task_summary` can come after `result`, so find the `result` line rather than taking the last line.

Other useful flags (all in 2.1.286 `--help`): `--model <alias|name>`, `--effort low|medium|high|xhigh|max`, `--max-budget-usd <n>` (print mode only; caps spend), `--append-system-prompt <text>`, `--add-dir <dirs>`, `--json-schema <schema>` (structured answer), `--fallback-model <model>`, `-n/--name <name>`, `--no-session-persistence` (cannot be resumed). `--max-turns` is not listed in 2.1.286 `--help`; don't rely on it.

## 4. Follow-up

```
bash {command: "claude -p 'Now add a regression test for the empty-input case' --resume <session_id> --output-format json --permission-mode acceptEdits --permission-prompts none --setting-sources user --settings '{\"disableAllHooks\":true,\"autoMemoryEnabled\":false}' < /dev/null", cwd: "repo"}
```

- Run it from the same directory as the first run; sessions are stored per project folder.
- Repeat the mode and isolation flags; they are per launch.
- `--fork-session` with `--resume` branches into a new session id and leaves the original alone.
- `-c/--continue` resumes the most recent session in the folder. Prefer the explicit id, because another run may have been more recent.

## 5. Long run

```
bg_start {
  command: "claude -p \"$(cat ../briefs/refactor-auth.md)\" --output-format stream-json --verbose --permission-mode acceptEdits --permission-prompts none --setting-sources user --settings '{\"disableAllHooks\":true,\"autoMemoryEnabled\":false}' < /dev/null",
  cwd: "repo",
  notify_on_exit: true
}
```

- With `notify_on_exit` you may end your turn; a note wakes you when the job exits. Tell the owner what is running.
- To hear about something mid-run (a failing test, a question it prints), add `notify_on: ["FAILED", "/error|panic/"]` (up to 4 patterns; each wakes you once while it keeps running).
- To wait inside the turn: `bg_wait {id, timeout_ms: 600000, until: "\"type\":\"result\""}`. It returns the tail and status. Check `is_error` in the result line.
- `bg_output {id}` shows recent progress. A quiet run is not a stuck run; look for recent tool events before stopping it.
- `bg_stop {id}` ends the whole process tree.
- Don't use Claude Code's own `--bg` / `claude agents` background sessions: they live outside Anima's process custody, so Stop can't end them.

## 6. Interactive (app session)

Use the TUI when the owner wants to watch, for several back-and-forth turns, or for TUI-only features such as slash commands.

```
app_start {command: "claude --permission-mode acceptEdits --setting-sources user --settings '{\"disableAllHooks\":true,\"autoMemoryEnabled\":false}'", cwd: "repo", cols: 120, rows: 40}
app_read {id, wait_ms: 5000}
```

First-run screens:
- **Folder trust** (first launch in a folder): "Quick safety check: Is this a project you created or one you trust?" In 2.1.286 the highlighted default is `❯ No, exit`, and `Yes, I trust this folder` is second. Trust only folders the owner asked you to work in: `app_send {id, keys: ["Down"]}`, `app_read` to confirm the cursor is on "Yes", then `app_send {id, keys: ["Enter"]}`.
- **Login, theme picker or onboarding** on a fresh install: stop and ask the owner to finish setup in their own terminal.
- **Bypass-permissions warning** (only with the dangerous flags): its default is the safe "No, exit". Leave it unless section 7 allows full freedom.

Waiting for input looks like this: a prompt line starting with `❯` between two horizontal rules, with the permission mode in the status bar below (for example `⏵⏵ accept edits on`). While it works, progress lines appear above the box.

Send a task, then wait until the screen goes quiet:

```
app_send {id, text: "Fix the failing parser tests in src/parse. Don't touch the public API."}
app_send {id, keys: ["Enter"]}
app_read {id, wait_ms: 30000}
```

`app_read` returns when the screen has been quiet for `wait_ms` (max 120 s), when `until` text appears, or when the app exits. The `❯` box is always on screen, so it is a poor `until` value. Prefer quiet periods or a phrase you expect in the answer.

Permission prompts in the TUI: read the whole prompt first. Approve only what the brief covers (Enter picks the highlighted option; Esc declines). Ask the owner about anything outside the brief, such as push, deleting files, installs or network access. Never approve prompts blind or on a timer.

To finish, send `/exit` and Enter (or `keys: ["Ctrl+D"]`), then always `app_stop {id}` to free the slot. An agent can have at most 4 live app and ACP sessions, and idle sessions stop after 30 minutes. Resume later with `claude --resume <session_id>`.

## 7. Freedom per task

| Task | Flags |
|---|---|
| Read-only: review, explain, plan | `--permission-mode plan`, or `--tools "Read,Grep,Glob"`; for an untrusted repo add `--restricted` |
| Edit files, no shell | `--permission-mode acceptEdits --permission-prompts none` |
| Edit files and run the project's checks | the above plus `--allowedTools "Bash(npm test *)" "Bash(git diff *)"` (patterns like `Bash(git *)` are from `--help`) |
| Full freedom | `--permission-mode bypassPermissions` or `--dangerously-skip-permissions`, only inside an isolated git worktree or throwaway container the owner approved for this task |

- `--permission-prompts none` makes anything that would prompt in `-p` mode be denied instead of waiting; the denials show in `permission_denials`.
- Modes in 2.1.286: `manual` (prompts; older versions called it `default`), `acceptEdits`, `plan`, `auto` (a classifier decides), `dontAsk` (anything not pre-allowed is denied), `bypassPermissions`.
- `--restricted` removes the command-running tools and WebFetch (unless `--tools` names them), ignores user, project and local settings files, and keeps file tools inside the working folders.
- Never use full freedom in the owner's main checkout, and never when secrets are in the environment. `--help` itself recommends it only for sandboxes with no internet access.

## 8. Isolation

Add these to every launch:

```
--setting-sources user --settings '{"disableAllHooks":true,"autoMemoryEnabled":false}'
```

- `--setting-sources user` loads only the owner's `~/.claude` settings. The repo's own `.claude/settings*.json`, hooks and project MCP config stay out, so a cloned repo can't run its hooks through you.
- `disableAllHooks` also turns off the user's hooks; `autoMemoryEnabled: false` keeps Claude Code from writing memory notes about this project.
- The repo's `CLAUDE.md` is still read. That is usually wanted, because it holds project conventions. Add extra rules with `--append-system-prompt`.
- Avoid `--bare` with subscription logins. It never reads OAuth or the keychain (2.1.286 `--help`: auth is strictly `ANTHROPIC_API_KEY` or `apiKeyHelper`), so a working claude.ai login looks logged out. Use it only when the owner turned on an API-key passthrough.
- `--strict-mcp-config` with no `--mcp-config` runs with no MCP servers. `--safe-mode` disables all customizations, which helps with a broken config.

## 9. ACP mode

Claude Code speaks ACP through an adapter: npm package `@agentclientprotocol/claude-agent-acp`, binary `claude-agent-acp`. The older `@zed-industries/claude-code-acp` and `@zed-industries/claude-agent-acp` packages are deprecated. Version 0.93.0 completed `initialize` here and reused the existing Claude Code login; it reported no auth methods to run.

```
acp_start {command: "npx -y @agentclientprotocol/claude-agent-acp", cwd: "repo"}
acp_prompt {id, text: "<brief>", wait_ms: 60000}
acp_read {id, wait_ms: 60000}
acp_cancel {id}
acp_stop {id}
```

- The first `npx` run downloads the adapter from npm. If the owner hasn't set it up, ask before the first download. A global install (`claude-agent-acp` on PATH) is their choice.
- `acp_prompt` returns the events so far, or the stop reason if it finished within `wait_ms`. Long prompts keep running, so follow them with `acp_read`.
- Claude Code's permission requests become the owner's normal Anima approval prompts. Don't add bypass flags; that supervision is the point of ACP.
- To continue an earlier ACP session, pass `session_id` to `acp_start` (the adapter advertises `loadSession`).
- **Always `acp_stop` when the task is done**, before you report, and again after every follow-up round. Stop it even if the owner may want more: `acp_start` with `session_id` picks the conversation back up. An idle session still holds one of the 4 live slots until the 30-minute idle timeout.
- Prefer ACP for multi-turn work the owner wants to supervise, with per-action approvals and a real cancel. Prefer `-p` for quick scripted tasks, JSON results and parallel batch runs.

## 10. Verify yourself

Claude Code's "done" and "all tests pass" are claims, not evidence.

1. `git status --porcelain` and `git diff --stat`, then read the diff of every file that matters, including new untracked files.
2. Run the project's own checks yourself (tests, lint, typecheck) with `bash` or `bg_start`.
3. If the change has a web page or other screen, logic tests don't show what the page does. Run the project's own browser or end-to-end tests if it has them. If you have the browser tools and the owner installed a route for the page, open it with `browser_navigate` and look with `browser_observe`. Otherwise read the page's load and event code and work out what it shows on first load with the default values, after valid input, and after invalid or cleared input. Either way, tell the owner whether you opened the page; if you didn't, say so and name what they should click to check.
4. Compare the result with the brief. Look for scope creep, deleted or skipped tests, disabled lint rules, new dependencies, changed lockfiles or config, and leftover TODOs.
5. Look for leftovers: servers it started (`bg_list`, `ps`), stray files, and the brief file.
6. Stop every ACP or app session you started for this task (`acp_stop`, `app_stop`) before you report, including after a follow-up round.
7. Report to the owner: what changed (files), the checks you ran and their results, `total_cost_usd`, the session id for follow-ups, and open questions. Don't commit or push unless asked.

If the work is wrong, resume the session with specific feedback (section 4) rather than starting over.

## 11. Gotchas

- `subtype: "success"` with `is_error: true` is a failure.
- In stream-json, `system` events can follow `result`.
- `-p` skips the folder-trust dialog, so use it only in folders the owner trusts. In `-p` mode, settings files that fail validation are silently ignored.
- Resume needs the same working folder. `--no-session-persistence` runs can't be resumed.
- `-w/--worktree` makes Claude Code's own worktree under `.claude/worktrees/`. Prefer creating the worktree yourself, so you control its path and cleanup.
- `--permission-mode default` is `manual` in 2.1.286.
- `total_cost_usd` is a list-price estimate, even on subscription plans.
- Quote JSON for `--settings` in single quotes inside the shell command, as shown above.
- List flags (`--allowedTools`, `--disallowedTools`, `--tools`, `--add-dir`, `--mcp-config`) take every following word. Put the prompt right after `-p`, as in the examples, so it isn't swallowed as a tool name.
- Don't run `claude update`, `claude install`, `claude mcp add`, `claude plugin ...` or `claude auth logout` unless the owner asked; they change the owner's setup.
