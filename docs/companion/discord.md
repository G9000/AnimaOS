# Discord conversations

Connect a bot in **Connectors → Discord**. Paste its bot token and choose **Check bot**. The check verifies the application, shows its bot identity and Message Content intent status, and provides an **Add to Discord** link. The token stays with the daemon; it is never included in the invite link.

Use the invite to install the bot in your server, then save and activate the connection. A selected channel is optional. When no users are entered, creation uses the application's verified owner IDs. You can also explicitly allow users or server roles. An empty channel restriction allows authorized people to mention the bot wherever it has server access.

## Replies and threads

- Server-channel messages require a mention by default. A mention selects the bot to answer; the sender must still be authorized.
- Automatic threads give a new request its own conversation. Once the bot participates in a thread, authorized people can follow up without mentioning it again. Enable **Require mentions in threads** when that fits your server.
- New automatic thread names omit raw user, role, and channel mentions. After the first successful reply, Anima attempts an AI-generated title in the background, respecting the agent's `autoTitle` setting. It checks Discord's current name before renaming and preserves a manually changed name. If generation or renaming fails, the temporary title stays; later replies and restarts do not repeat the attempt. Existing threads are left alone.
- Free-response channels answer authorized messages without a mention and reply inline.
- Allowed-channel restrictions include threads under the allowed parent channel. Each channel or thread keeps its own conversation history and replies return to that destination.
- If creating a thread fails, the request does not silently move into a parent-channel conversation. Inspect the connector warning before retrying.

Bot messages and webhooks cannot trigger the assistant. Channel names, display names and message text are untrusted conversation content; they never grant permission.

## Home channel

The optional home channel is a destination for proactive output. Choose **Discord** as an automation's delivery target to send its non-silent results there. A home channel does not restrict incoming conversations or redirect ordinary replies. Configure a home before choosing Discord delivery.

Changing the home affects future output. Already prepared deliveries retain their original destination. If a connection is disabled or deleted, it cannot deliver scheduled results.

## Permissions and recovery

The invite requests View Channel, Send Messages, Read Message History, Create Public Threads and Send Messages in Threads. Enable Message Content intent in the Developer Portal. Private threads also require the bot to have access to that thread. This connector does not implement Discord slash commands or voice.

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
