# AI agents and Skills

Use the desktop app's agent composer to create designs or change the current
canvas. Choose a model beside the send button. See
[Authentication](/getting-started/authentication) for account and provider setup.

Use GitHub Copilot chat in VS Code or Cursor's chat with
[pen.dev's MCP integration](/getting-started/ai-integration#connect-an-external-mcp-client).
For terminal workflows, see the [CLI reference](/for-developers/pen-cli).

## Sessions and models

Open the **Agent** tab to read the conversation and tool activity. Use the
session menu at the top of the panel to return to an earlier conversation.
Select **New** (+) to start a separate conversation.

Use the **×** beside a session to remove it from history.

Choose a model from a connected provider in the model picker. Open the arrow
beside a model to choose its reasoning effort. If the picker warns
that switching providers will lose conversation context, include the needed
instructions in your next request.

Select **Stop generation** to interrupt a running agent. Stopping leaves its
completed canvas changes in place. Inspect the result before sending a
follow-up request or using [Undo](/core-concepts/pencil-interface#undo--redo).

**Settings (⚙️) → Chat → Notifications** controls alerts when an agent
finishes while the app is in the background.

## Add context

Canvas selections appear in the composer as context. Select the layers you
want the agent to change, then describe the change.

Use **Add to context (+) → Add image or file** to attach a reference image or
text file, such as a PNG, JPEG, Markdown file, or source file. You can also
drop files into the composer. Check that each attachment appears before
sending the request.

## Review permissions and usage

When an agent shows a **Permission Request**, review the requested action and
path or command. **Allow** approves that request. **Deny** rejects it.

For supported Claude models, **Switch to Auto** approves the current request
and lets Claude evaluate future permission requests automatically. This setting
carries over to future sessions. Turn it off with **Auto permission mode** in
**Settings (⚙️) → Agents → Anthropic Claude Code**.

If a tool reports **auto mode unavailable for this model**, turn off **Auto
permission mode** and resend the request. Use **Allow** or **Deny** when the
permission request appears.

**Settings (⚙️) → Chat → Dangerously skip permissions** lets agents run without
permission prompts, including file and command actions. Keep it off when you
want to review those requests.

Open **Context usage** beside the model picker to inspect token usage. The
content breakdown is an estimate. **Est. cost at API rates**, when shown, is
an estimate rather than an invoice.

When a response shows **Designed for…**, expand it to inspect the reported usage.

## Use skills

Skills add instructions to an agent request. MCP tools let the agent read or
change the document.

1. Type `/` in the desktop composer, or select **Add to context (+) → Pick a skill**
2. Choose a skill from the menu
3. Add your request and send it

To add your own skill, choose **Add SKILL.md file…** in the slash menu. Select
a `SKILL.md` file or its containing folder. The folder name becomes the skill's
name in the menu, so use a unique folder name. Read the skill before adding it:
its instructions are sent with requests that use it.

Keep the skill in that location. If you edit its contents, add the same file
again to reload it. To remove a custom skill from the menu, point to its row
and select the trash icon. Removing it leaves the folder and `SKILL.md` on disk.

For an external MCP client, ask the connected agent to call `read_skill()` to
read pen.dev's design instructions, then read the referenced files needed for
the task. The desktop slash menu's custom skills are separate from this tool.

## Generate alternatives

In **Parallel agents** (⚡), choose an agent count from one to six and a mode:

- **Split Work** asks agents to work on different parts of the task
- **Side by Side** asks agents to work on the same task to create alternatives

In **Side by Side**, each agent has a model selector. Use **Reset to main model**
to use the main agent's model for the other agents.

To compare two signup cards, select **2x** and **Side by Side**, then ask for
a signup card with a heading and a **Join** button. Inspect the results on the
canvas. Use the agent-name menu in the **Agent** tab to read each agent's
conversation.

**Let it cook** generates successive variants after the first design. Choose
**Layout** to explore layouts while preserving the visual style, or **Style**
to explore visual styles. Set **Variants** to between two and six, or choose
**Don't iterate** for a single design.

With several agents running, select **Stop generation**, then **Stop all**.
To stop only the selected agent, choose **Stop current** while it is running.
If one agent fails, inspect its conversation and the canvas
before retrying. Keep the result you want and delete unwanted frames from
**Layers**. Additional agents and iterations make additional model requests.

## If an agent cannot continue

For authentication errors or a missing model, check the provider in
**Settings (⚙️) → Agents** and follow the
[connection troubleshooting steps](/getting-started/authentication#troubleshooting).
Then return to the conversation and resend the request.

If a tool fails, open its **Show details** control to inspect the error. Check
the current canvas before asking the agent to retry the failed action.
