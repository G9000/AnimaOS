# Coding agent bundles

Community bundles that teach an Anima agent how to delegate code work to coding
CLIs and check the result. Each bundle holds one skill. Install only the ones for the
CLIs you actually have.

| Bundle | Skill | For |
|---|---|---|
| `anima.coding.claude-code` | `claude-code` | Claude Code (`claude`) |
| `anima.coding.codex` | `codex` | OpenAI Codex CLI (`codex`) |
| `anima.coding.opencode` | `opencode` | OpenCode (`opencode`) |
| `anima.coding.gemini-cli` | `gemini-cli` | Gemini CLI (`gemini`) |
| `anima.coding.delegate` | `coding-agent-handoff` | Any of them: picking a CLI, briefs, parallel worktrees, review, reporting |

The four CLI skills share one section order: find it, check login, one-shot
run, follow-up, long run, interactive, freedom per task, isolation, ACP mode,
verify yourself, gotchas. They use the daemon's `bash`, `bg_*`, `app_*` and `acp_*`
tools. Each skill states the CLI version and date it was checked against.

The skills never have the agent type credentials. On a login failure, they tell the agent to ask
you to log in. They also tell the agent never to commit or push unless you ask.

## Install

In the web console, open **Settings › Tools › Community bundles** and import the
`manifest.json` of the bundle you want, for example
`bundles/coding-agents/claude-code/manifest.json`. Review it (requested tools,
attribution, the skill text), then install. The skill arrives as a draft for the
agent's workspace and needs your approval like any imported skill. The tools a manifest
lists are requests, not grants. The agent's own approval policy still decides.

The agent needs Operate-level tools (`bash`, background jobs, app sessions and,
for ACP, `acp_*`) for these skills to be useful.

## Edit and regenerate

`<cli>/SKILL.md` is the source of truth. Each `manifest.json` inlines the exact
SKILL.md text, so after editing a skill, regenerate every manifest:

```
node bundles/coding-agents/build.mjs
```

The script has no dependencies. It checks the skill limits the daemon enforces
(front matter at most 4 KiB, body at most 32 KiB, a one-line description of at most 300 characters, no
hidden characters), then writes the manifests. Bundle metadata (ids, names,
requested tools, attribution) lives at the top of `build.mjs`.

Then run the daemon test that checks every manifest against its SKILL.md, the
manifest validator and the skill parser:

```
cargo test -p anima-daemon coding_agent_bundles
```

When you re-check a skill against a newer CLI, update the "Checked against ..."
line at its top.

License and third-party notes are in [ATTRIBUTION.md](ATTRIBUTION.md).
