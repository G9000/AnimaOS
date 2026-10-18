---
name: coding-agent-handoff
description: "Hand coding work to a coding CLI (Claude Code, Codex, OpenCode, Gemini CLI): pick the CLI, write a good brief, run parallel coders in separate git worktrees, read the diff, run the checks yourself and report. Use before delegating any implementation or review task."
version: 1.1.0
tags: [coding-agent, delegation, git-worktree, review]
---

# Coding agent handoff

How to delegate code work to a coding CLI and stay accountable for the result. The per-CLI skills (`claude-code`, `codex`, `opencode`, `gemini-cli`) have the exact commands; load the matching one with `load_skill` before you launch anything. Written for Anima on 2026-10-19.

Ground rules, always:
- **Never commit, push, merge, tag or open a PR** unless the owner asked for that step. Coders don't do it either; say so in every brief.
- **Never type credentials.** On a login failure, stop and ask the owner to log in in their own terminal.
- **Never trust a coder's "done".** You read the diff and run the checks.
- Give each coder the **least freedom** the task needs. Full freedom only inside an isolated worktree the owner approved.

## 1. Decide whether to delegate

Delegate when the task is a real implementation, refactor, test-writing or review job that benefits from a coding agent's own loop. Do it yourself (`read_file`, `edit_file`, `bash`) when it is a few lines, or when explaining the change would take longer than making it.

## 2. Pick a CLI

1. **Owner preference first.** If the owner named a CLI, use it.
2. **What is installed and logged in.** For each candidate, run section 1 ("Find it") and section 2 ("Check login") of its skill. `command -v claude codex opencode gemini` is a quick first pass.
3. **Fit:**

| Need | Good fit |
|---|---|
| Fine-grained tool limits in a one-shot (`--allowedTools "Bash(npm test *)"`) | Claude Code |
| OS-level sandbox for edits and commands; built-in diff review (`codex exec review`) | Codex |
| A model from another provider, or a local model | OpenCode |
| Read-only planning mode and a container sandbox in one flag set | Gemini CLI |
| Supervised multi-turn work with every action going through Anima approvals | any of them over ACP (section 9 of each skill) |

If none is installed or logged in, tell the owner what you found and stop. Don't install CLIs yourself.

## 3. Write the brief

A coder only knows what you tell it plus what it reads in the repo. Write the brief to a file with `write_file`, outside the repo or in an ignored path, and pass it in. Template:

```
Goal: <one or two sentences: the outcome, not the steps>

Context:
- Repo: <path>, branch <name>. Relevant files: <paths>.
- What exists today and why it is wrong / missing: <facts, error output, issue text>.
- Decisions already made: <constraints from the owner, prior discussion>.

Constraints:
- Only change files under <paths>. Don't touch <paths>.
- No new dependencies. No changes to lockfiles, CI or config unless listed above.
- Do not commit, push, create branches or open PRs.
- Do not delete or skip tests to make them pass.

Done when:
- <observable behavior>
- <checks that must pass, with the exact commands, e.g. `npm test -- parser`>

Report back:
- Files changed and why, commands you ran with their results, anything you could not do, open questions.
```

Good briefs are specific: exact paths, exact commands, the actual error text. If you can't fill in "Done when", you don't understand the task well enough to delegate it yet; investigate first or ask the owner.

## 4. Pick how to run it

| Situation | Mode |
|---|---|
| Quick, bounded task (under about 2 min) | one-shot with `bash` (section 3 of the CLI skill) |
| Anything longer | `bg_start` with `notify_on_exit: true`; end your turn or `bg_wait` (section 5) |
| Owner wants to watch, or several back-and-forth turns | TUI in an app session: `app_start` / `app_send` / `app_read` (section 6) |
| Owner wants to approve the changes ("let me approve it", "show me first") | Run the CLI in a worktree (section 5), review the diff, then show the owner the diff and apply it only after they say yes. This works whatever the owner's permissions are. |
| Owner wants to approve each step as it happens | ACP: `acp_start` / `acp_prompt` / `acp_read` (section 9 of the CLI skill). Its edits become approval cards only when the owner's Write permission is Ask; under Operate they go through without asking, so prefer the worktree route above unless the owner set Ask. |

Load the CLI skill **before** you propose a plan, so the plan names the real steps. Propose one plan; don't propose a second one after loading skills. Each reply has a limited number of tool rounds.

Tell the owner what you started, where, and how long you expect it to take.

## 5. Parallel coders: one worktree each

Never run two writers in the same checkout. Give each coder its own git worktree on its own branch. Worktrees must be inside the workspace, because the tools' `cwd` is workspace-relative.

```
bash {command: "git status --porcelain && git rev-parse --abbrev-ref HEAD", cwd: "repo"}
bash {command: "grep -qxF '.worktrees/' .git/info/exclude || echo '.worktrees/' >> .git/info/exclude", cwd: "repo"}
bash {command: "git worktree add -b anima/parser-fix .worktrees/parser-fix HEAD", cwd: "repo"}
bash {command: "git worktree add -b anima/docs-update .worktrees/docs-update HEAD", cwd: "repo"}
```

- A worktree starts from a **commit**. The owner's uncommitted changes are not in it. If the task depends on them, say so and ask the owner how to proceed. Don't commit or stash their work yourself.
- `.git/info/exclude` is local and never committed, so the worktrees don't show up as untracked files in the owner's checkout.
- Each worktree needs its own setup (for example `npm install`, build caches). Dependencies are not shared. Ask before long or networked installs if the owner hasn't approved them.
- Watch for shared resources: dev-server ports, databases, fixed temp paths. Tell each coder which port to use, or run their checks one at a time.
- Launch each coder with `cwd: "repo/.worktrees/<name>"`, usually through `bg_start` with `notify_on_exit: true`. Track the ids with `bg_list`. An agent can have at most 4 live app and ACP sessions in total; background one-shots don't count toward that limit.
- Split work so the coders touch **different files**. Overlapping edits make conflicts you will have to explain to the owner.
- For edits that don't need a coding CLI, `spawn_helper` / `spawn_helper_async` with `worktree: true` (and `repo: "<path>"` when the repo isn't the workspace root) gives the helper its own worktree that Anima creates and cleans up. The result lists the changed files and the branch. Review the diff before anything reaches the owner's checkout. Helpers can't run git or commands, so merging is your job.

## 6. While it runs

- Let it work. A quiet run is not a stuck run. Check `bg_output` (or `app_read`) for recent activity before you stop anything.
- If it asks a question (TUI or ACP), answer from the brief. If the answer isn't in the brief, ask the owner.
- Stop it (`bg_stop`, `app_stop`, `acp_cancel` / `acp_stop`) when it goes off-brief: touching files outside scope, trying to push, installing things, or looping.

## 7. Review the result yourself

In the worktree (or the repo, for a single coder):

```
bash {command: "git status --porcelain && git diff --stat HEAD", cwd: "repo/.worktrees/parser-fix"}
bash {command: "git diff HEAD", cwd: "repo/.worktrees/parser-fix"}
```

1. Read every changed file that matters, and every **untracked** file (`git status` shows them; `git diff` does not).
2. Run the checks from the brief yourself (`bash` or `bg_start`), plus any related ones the coder might have skipped. Use the project's own commands (look at `package.json`, `Makefile`, `Cargo.toml`, CI config, `AGENTS.md`).
3. If the change has a web page or other screen, logic tests don't show what the page does. Run the project's own browser or end-to-end tests if it has them. If you have the browser tools and the owner installed a route for the page, open it with `browser_navigate` and look with `browser_observe`. Otherwise read the page's load and event code and work out what it shows on first load with the default values, after valid input, and after invalid or cleared input. Either way, tell the owner whether you opened the page; if you didn't, say so and name what they should click to check.
4. Compare against the brief: scope creep, deleted or skipped tests, weakened assertions, disabled lint rules, new dependencies, lockfile or config churn, hard-coded values, leftover debug output and TODOs.
5. Check for leftovers: processes it started (`bg_list`, `ps`), ACP or app sessions still open (stop them with `acp_stop` / `app_stop`, including after a follow-up round), temp files, the brief file.
6. If something is wrong, send **specific** feedback in the same session (section 4 "Follow-up" of the CLI skill): what is wrong, where, and what "done" means. Then review again. Two or three rounds is normal; after that, report to the owner instead of looping.

## 8. Report to the owner

Keep it short and factual:
- **Outcome**: done, partly done, or blocked, in one line.
- **Changes**: files changed, with one line each on what and why. For parallel work, list each worktree and branch.
- **Evidence**: the checks you ran and their results (pass/fail counts, the key error lines if any failed).
- **Cost and ids**: the cost the CLI reported if it reported one, and the session ids for follow-ups.
- **Open questions and risks**: what you are unsure about, and what you didn't verify, including whether you opened the page for UI changes.
- **Next step for the owner**: for example "review the diff in `.worktrees/parser-fix`; say *commit* and I'll commit it", or "want me to merge `anima/docs-update` into your branch?"

## 9. Integrate and clean up, only when asked

Bringing changes into the owner's branch (commit, merge, cherry-pick, applying a patch) changes history or their working tree. Do it only after the owner says so, and only the way they asked.

Removing a worktree or deleting a branch can throw away work. Before running `git worktree remove .worktrees/<name>` and `git branch -D anima/<name>`, confirm the owner has what they need, or that they said to discard it. `git worktree list` shows what exists; `git worktree prune` only cleans up records of folders that are already gone.

## 10. Safety checklist

- Untrusted repo (cloned from elsewhere, unknown author): use read-only modes and each CLI's isolation flags (section 8 of its skill), and read the repo's agent config (`.claude/`, `.codex/`, `.opencode/`, `opencode.json`, `.gemini/`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`) before you trust it.
- Secrets: keep coders away from `.env` files and credential stores. If the task needs a secret, ask the owner; never paste one into a brief.
- No full-freedom flags (`--dangerously-*`, `bypassPermissions`, `danger-full-access`, `yolo`, `--auto`) outside an isolated worktree the owner approved.
- Nothing leaves the machine (push, PR, cloud tasks, sharing) without the owner's explicit request.
