# Discord conversations

Connect a bot in **Connectors → Discord**. Paste its bot token and choose **Check bot**. The check verifies the application, shows its bot identity and Message Content intent status, and provides an **Add to Discord** link. The token stays with the daemon; it is never included in the invite link.

Use the invite to install the bot in your server, then save and activate the connection. A selected channel is optional. When no users are entered, creation uses the application's verified owner IDs. You can also explicitly allow users or server roles. An empty channel restriction allows authorized people to mention the bot wherever it has server access.

## Replies and threads

- Server-channel messages require a mention by default. A mention selects the bot to answer; the sender must still be authorized.
- Automatic threads give a new request its own conversation. Once the bot participates in a thread, authorized people can follow up without mentioning it again. Enable **Require mentions in threads** when that fits your server.
- New automatic thread names omit raw user, role, and channel mentions. After the first successful reply, Anima attempts an AI-generated title in the background, respecting the agent's `autoTitle` setting. It checks Discord's current name before renaming and preserves a manually changed name. If generation or renaming fails, the temporary title stays; later replies and restarts do not repeat the attempt. Existing threads are left alone.
- To change a title later, ask Anima inside that Discord thread. It can rename only the current thread and confirms the change after Discord accepts it and the daemon saves the result. An explicit rename can replace a manual title and prevents a pending automatic title from overwriting your choice. If the outcome is uncertain, inspect the title in Discord before retrying. The bot must be the thread creator or have Manage Threads permission.
- Free-response channels answer authorized messages without a mention and reply inline.
- Allowed-channel restrictions include threads under the allowed parent channel. Each channel or thread keeps its own conversation history and replies return to that destination.
- If creating a thread fails, the request does not silently move into a parent-channel conversation. Inspect the connector warning before retrying.

Bot messages and webhooks cannot trigger the assistant. Channel names, display names and message text are untrusted conversation content; they never grant permission.

When asked about a discussion or another participant, Anima can read recent messages in its current Discord thread, including messages from other bots. The reader identifies each account by its Discord ID and marks Anima's own bot account, so matching display names do not merge participants. It reads bounded pages and can request older messages; long messages may be marked as truncated. Discord's channel permissions and ordinary read approval rules still apply. Reading other bots' messages does not make their messages start runs or automatically trigger replies.

## Files and images

Authorized participants can attach files to a mention in a server channel, or upload without a caption in a thread where Anima already participates (unless thread mentions are required). A configured workspace is required. Files are saved under `uploads/YYYY-MM-DD/` and bound to that conversation; saved inputs survive restart. Existing sender, role and channel restrictions apply before downloading.

Supported inputs are PNG/JPEG/GIF/WebP images (10 MiB each), UTF-8 text/Markdown/CSV/JSON/YAML/XML and common source files (1 MiB each), and common PDF/Office documents (25 MiB each). A message admits up to ten files and 25 MiB in total. Unsupported, malformed or oversized inputs produce an explicit notice. Voice/audio and video processing are not included.

Vision-capable models receive verified images through the existing attachment pipeline; other models receive an unavailable-image notice. Small text files (up to 16 KiB, within the message budget) are included as untrusted quoted data. Larger text files can be opened with `read_file` when the agent has that tool. Selected PDF/DOCX/XLSX documents use the existing bounded extraction limits; PDF excerpts cover the text layer only. Other saved Office formats are not automatically interpreted.

Ask Anima in a Discord thread to create and send a report. It can write a workspace file and call `send_discord_file(path, caption?)`, for example `reports/summary.md`. The tool sends one supported file to the current thread, with a 10 MiB upload ceiling and the same per-type validation limits above. Absolute paths, traversal and linked files are refused. It is unavailable to web runs and delegated helpers. Anima confirms delivery only after Discord returns a matching message and attachment receipt. Failed or uncertain uploads do not confirm success and are not automatically retried; inspect Discord before retrying an uncertain upload. Existing run approval policies apply.

The bot needs **Attach Files** permission to send uploads. New invite links request it; grant it in the server/channel for an existing installation. Console follow-ups to Discord sessions still accept text only. Native Discord file inputs are admitted by the connector.

## Home channel

The optional home channel is a destination for proactive output. Choose **Discord** as an automation's delivery target to send its non-silent results there. A home channel does not restrict incoming conversations or redirect ordinary replies. Configure a home before choosing Discord delivery.

Changing the home affects future output. Already prepared deliveries retain their original destination. If a connection is disabled or deleted, it cannot deliver scheduled results.

## Permissions and recovery

The invite requests View Channel, Send Messages, Attach Files, Read Message History, Create Public Threads and Send Messages in Threads. Enable Message Content intent in the Developer Portal. Private threads also require the bot to have access to that thread. This connector does not implement Discord slash commands or voice.

The daemon owns the persistent Gateway connection and reconciles messages through bounded history scans. Each discovered conversation has its own durable cursor. Very large backlogs and missing permissions can pause reconciliation and appear in status. Message edits or messages deleted before catch-up are not guaranteed to be observed.

New channels are discovered from messages observed while connected. Offline
history recovery covers saved conversations; it does not scan every previously
unseen channel in every server.

Completed inbound messages are not rerun during recovery. Uncertain or partly delivered output is not automatically resent; inspect Discord and resolve the uncertainty in Connectors. A local test pass does not establish acceptance by Discord or your model provider.

Existing selected-channel connections retain their saved history and original inline response behavior until their settings are changed. To use mentions across channels, delete the old selected-channel connection and create a new connection without a selected channel. Deleting the connection retains its saved conversation history; the new connection starts separate conversations. The same bot cannot have overlapping selected-channel and bot-wide connections for one agent.

The conversation format uses control-plane store version 22. The first startup
with a version 21 store preserves an exact `.pre-discord-conversations.bak`
backup before upgrading; earlier upgrade backups are retained. Older binaries
refuse the new version. This code change does not itself open or upgrade a live
store; the running daemon must be restarted to load it.
