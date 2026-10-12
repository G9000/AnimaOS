# Anima daily-use setup

Anima is a Rust companion with persistent context and work that can run while the
console is closed. The selected feature set is implemented and fixture-tested.
Isolated local testing on 2026-10-12 also verified real ChatGPT replies, identity,
workspace file reading, memory recall, Stop, and persistence across daemon restart.
Messaging accounts, microphone and always-on deployment acceptance remain to be done.
Harness acceptance also verified a real multi-step report with a helper and saved
tasks, correction of existing memories, approved skill reuse, automatic context
compaction, and explicit continuation after an in-flight restart. Completed effects
were preserved; interrupted work was flagged for review before continuation.
Memory context now selects relevant own facts ahead of recent reflections, with
quoted, bounded excerpts. One real fresh-chat check recalled the corrected
preference after twenty newer reflections and eight matching foreign facts,
without using a search tool. Selection is lexical; semantic recall and automatic
memory consolidation remain unverified. A real one-shot scheduled file task also
restored before due, fired without a manual trigger and retained one completed
write after another restart. Exact bytes and modification time stayed unchanged;
the schedule was left disabled and the isolated daemon stopped.

## Start with one companion

Use the existing [local workflow](../../AGENTS.md#dev-workflow) or
[single-owner VPS installation](../../deploy/vps/README.md#first-installation).
Those are operator instructions. Live acceptance used only an approved isolated
local web console and Rust daemon with synthetic data and no messaging connectors.
Choose a real configured model during onboarding, create the companion and select
its workspace. Confirm a real reply before relying on tools or schedules.

For durable local use, configure control-plane/history and memory persistence as
specified in the [daemon startup examples](../../hosts/rust-daemon/README.md#starting-the-daemon). An ephemeral preview is not a persistent assistant. Keep the
same workspace path and private credential vault across restart.

## Identity and everyday work

Place UTF-8 `SOUL.md` in the configured workspace root for the lead companion's
identity. Use `ANIMA.md` or `AGENTS.md` for project instructions. The console's
instruction preview shows what loaded; it does not edit those files. See
[standard Markdown files](../../hosts/rust-daemon/README.md#standard-markdown-agent-files)
for priority, limits and protected writes. File contents cannot override owner
permissions. Upstream profile/token files are never required.

Start a chat, give it a concrete task, and watch the tool cards and run status.
Use Stop when needed. Save a useful fact to memory; review and edit it in Memory.
With Collaborate or Operate access, the agent can correct its own visible memories
using their exact IDs and current content. Existing tool selections need an explicit
permission update to enable this new tool; saving unrelated settings does not add it.
For corrections, search for the complete current record before updating it; a
context excerpt is not the full text. Save compact durable facts and preferences,
use Tasks for temporary progress, and review reusable procedures as skills.
Explicit search now filters eligible records before its lexical result limit, so
stronger foreign matches cannot hide your matching facts. Search and recent reads
exclude hidden text in record IDs, content and tags before limiting results;
corrections refuse those records too. Semantic vector candidate coverage remains
unverified.
Use Files to inspect the chosen workspace. Attach text or an image when useful;
vision requires a compatible model. PDF/Office attachments can be stored and
retrieved, but their contents are not parsed by this feature set.

Review tool permission requests in Approvals. Review saved mail drafts and
Calendar changes in Connectors before sending or applying them. Check the exact
request before allowing it. Configure a small schedule in Automations and inspect its
history. Schedules belong to the daemon, so they need that process running; a
closed browser is fine. An interrupted action needs review before retry.

Browser dictation/read-aloud depends on browser speech support and permissions.
An unavailable microphone or speech API does not prevent typed chat. Web fetch
and optional EXA search require explicit tool grants/configuration; they do not
provide interactive browser or desktop control. Preserve custom connector/web
permissions when changing access profiles: a standard profile replaces custom
tool selections, as the settings warning explains.

## Connect only what you use

- Telegram: configure and pair through Connectors. The native daemon handles
  polling, delivery and recovery; no separate gateway is needed.
- Discord: choose a text channel and permitted users, then explicitly activate.
  Read the [Discord setup and recovery limits](../../hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md#selected-discord-channel).
  This is REST text polling, without Gateway voice or slash-command support.
- Gmail and Calendar: configure the Google OAuth app in Connectors → Google OAuth app, register the
  displayed callback URLs, then connect the account. Mail sends and Calendar
  writes follow their approval/recovery policies.
- Drive: enable the Google Drive API, paste 1–16 file IDs or recognized links and
  consent. [Selected Drive setup](../../hosts/rust-daemon/README.md#selected-drive-files)
  explains broader Google readonly authorization versus Anima's selected IDs,
  filename-only search and Google Docs/plaintext reads. Disconnect is local;
  account-wide Google revocation can also affect Gmail and Calendar.

Community connectors are reviewed source compiled into the daemon, with full
host privileges and a rebuild. [Contribution boundaries](../../hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md)
are explicit; there is no dynamic module installer or sandbox claim.

## Acceptance before everyday reliance

These checks are **not executed** by the local fixture gates. Perform them when
real service/account testing is authorized, using your own selected accounts:

1. Receive a real streamed model response and stop another run. Confirm the
   conversation and status agree after reconnecting the console.
2. Save a memory and workspace file. Restart the daemon during a quiet period;
   confirm identity, conversation, memory, file and connector state remain.
3. Allow one low-impact external action and verify its actual result. Reject a
   second action and verify it did not happen. Never infer delivery from a tool
   card alone.
4. Send/receive a message through the selected Telegram/Discord channel. Inspect
   unknown send outcomes before retrying. Telegram is at-least-once; Discord/mail
   uncertainty is not an exactly-once guarantee.
5. Read an allowed Drive text file, search its name, and try an unselected ID.
   The last request must be refused. Check unsupported formats are explained.
6. Fire a small schedule and inspect its history. After an interrupted external
   attempt, inspect side effects before making another attempt.
7. For hosted use, follow the [VPS acceptance checklist](../../deploy/vps/README.md#acceptance-checks-before-relying-on-it),
   including reboot and a stopped complete backup/off-site restore rehearsal.

Long-running means durable identity, memory, schedules, progress and recovery.
It does not mean a model process runs forever or an interrupted network action
can resume halfway through. The daemon must remain available, credentials must
be valid, and the owner must review uncertain effects.
