# One autonomy control: Free (default) or Strict — Design

**Status:** proposal for the owner to decide (2026-10-19). Nothing is built yet.
**Owner's direction:** "we should have total control on how the agent works… user should be able to have a stricter control, which is with the approval state. But also not everyone like this. So I think adopting [a free-by-default approach] plus an option of the strict control is a way to go."

## The idea

Two separate questions per agent, asked in onboarding and in Settings:

1. **What can it use?** This is today's Observe / Collaborate / Operate access profile, which decides which tools the agent has. It is unchanged.
2. **How much does it ask?** This is the new **Autonomy** setting:
   - **Free** (the default): acts without asking and tells you what it did, everything can be undone, and a short list of dangerous things still asks.
   - **Strict**: it asks before acting, as Anima does today.
   - **Custom**: appears automatically when you change individual rules on the Approvals page.

Today, choosing Operate quietly flips "run commands" to Allow. That side effect goes away: access controls *what* the agent can use, and autonomy controls *how much it asks*.

## What changes, gate by gate

| Gate (today) | Free | Strict |
|---|---|---|
| File edits, todos, memory, automations (Write class) | Allow, with receipts and Undo where Undo exists | Ask |
| Commands, apps, coding agents, browser, MCP tools (Exec class) | Allow, except the always-ask list below | Ask |
| Web reads and search (Network class) | Allow | Allow (choose Ask in Custom) |
| Helpers, delegation, messages to other agents (Delegate class) | Allow | Ask |
| Plan before acting (`propose_plan`) | The plan is shown and the agent starts right away; you can stop or change it | Ask (today) |
| Learning reviews | Run automatically after substantial work (8+ tool calls, or about every 10 of your messages); lessons are saved directly with "Learned … · Undo" in chat and a ledger in Settings | Lessons wait in the Inbox for your OK (today, minus the button under every message) |
| Skills the agent writes itself (`propose_skill`) | Saved and active right away, with Undo and a ledger | Draft for your OK (today) |
| Coding-agent requests over ACP (`acp_*`) | Follow the Exec/Write rows above | Ask |
| Approval timeout | Not relevant (nothing waits) | Unchanged (30 min, then deny) |

## Always asks, in both modes (the floor)

These cross from "the agent working" into "the agent acting as you outside, or doing something you can't take back":

- **Destructive commands.** A hardline list: recursive `rm` of `/`, home or system directories; `mkfs`; `dd` to devices; fork bombs; shutdown and reboot. Also deleting Anima's own data, and piping passwords into `sudo`. These are **blocked**, not asked.
- **Your own deny list.** Glob patterns you add yourself; they are always blocked.
- **Sending as you.** Email send, calendar changes, and messages to new chats.
- **Instruction files other tools read.** `AGENTS.md`, `CLAUDE.md`, `.cursor/rules`, and so on. A prompt injection could persist there.
- **Bringing in outside code.** Installing community bundles, enabling MCP servers, and granting their tools.
- **Secrets.** Handing API keys to coding apps.
- **Chat app access.** Telegram pairing and Discord allow-lists.

Each item on the floor (except the blocked hardline commands) can be relaxed one at a time in Custom. Free never relaxes them silently.

## Making Free safe to live with

- **See everything:** every allowed action already produces an action receipt. The Work page lists them, and the chat shows a one-line receipt per external action.
- **Undo:** use it where it exists (automations, learned lessons, agent-written skills, file checkpoints). The skill and lesson ledger lists each change with an Undo.
- **Stop:** the existing Stop button ends the run and its process trees.
- **Optional smart check:** a cheap model call judges each command that matches the dangerous-pattern list (approve, deny, or escalate to you). This is off by default because it costs a model call per flagged command.

## Defaults

- New agents start on **Free**. Existing agents are migrated to the Free/Strict/Custom value that matches their current settings, so nothing changes for them silently.
- Helpers keep their hard limits in both modes.

## Rough build order

1. Daemon: add `autonomy` per agent (Free/Strict/Custom) and policy presets. Add the floor (blocked list plus always-ask list). Add a "plan, then proceed" mode for `propose_plan`. Add auto-saved skills and lessons with a ledger and undo (the learning part is already in progress in the "Rework the review card" session).
2. Web: add an Autonomy picker in onboarding and Settings, and a Custom marker on the Approvals page. Remove "Operate flips exec". Add chat receipts and Undo notes.
3. Design first in `design.pen` (onboarding step, Settings › Permissions, chat notes).
4. Tests for every row of the table above, in both modes.

## Open questions for the owner

1. Default for new agents: **Free** (as proposed) or Strict?
2. Should **email send and calendar changes** stay on the always-ask floor in Free (as proposed)?
3. Do you want the optional **smart check** (an extra model call on risky commands) available in Free?

## Implementation notes (2026-10-19)

- Daemon: `approvals/autonomy.rs` holds the modes, presets, the floor (hardline blocks, dangerous-command asks, owner deny globs) and the smart-check verdict parser. `policy::evaluate_with_floor` applies the floor before the class policy; `gate.rs` refuses Blocked commands and runs the smart check only for asks the floor added. Routes: `GET/PUT /api/agents/{agent_id}/autonomy`; editing the class policy reads back as Custom.
- New agents (create, bootstrap, `anima.yaml` import) start Free; existing agents keep their policy. Helpers use their companion's autonomy.
- Skills: in Free, `propose_skill` approves at once (`decidedBy: "autonomy:free"`) and records a ledger entry; `GET /api/skill-ledger` lists them and `POST /api/skills/{slug}/revert-last` undoes one.
- Learning: Free keeps lessons with Undo; Strict and Custom leave them in the Inbox.
- Web: the autonomy card in Settings, the Free/Strict choice in onboarding, the informational plan card in Free, and the "dangerous command" reason on approval cards. The old "Operate sets Run commands to Allow" side effect is gone.
- Verified: daemon suite (only the 16 known macOS failures: 14 `fp08_host_*`, 2 `m44_*`), core-rust, sdk, web tests and typecheck. Not yet re-checked live in the test console.
