import { useLayoutEffect, useRef, useState } from 'react';
import type {
  DiscordConnector,
  CreateDiscordConnectorInput,
  DiscordConnectorSettings,
  DiscordBotCheck,
} from '../lib/daemon-api';
import {
  discordIdList,
  discordSettings,
  discordStatusLabel,
  discordWarning,
  isDiscordSnowflake,
} from '../lib/discord';
import { SectionTitle, primaryBtnCls } from './ui-bits';

interface DiscordSettingsProps {
  online?: boolean;
  epoch?: number;
  disabled?: boolean;
  agentId: string;
  connectors: DiscordConnector[];
  busy: string | null;
  error: string | null;
  loading: boolean;
  available?: boolean;
  create: (input: CreateDiscordConnectorInput) => Promise<boolean>;
  check: (token: string) => Promise<DiscordBotCheck | null>;
  configure: (
    id: string,
    settings: DiscordConnectorSettings,
    approvedUserIds: string[],
  ) => Promise<boolean>;
  activate: (id: string) => Promise<boolean>;
  replace: (id: string, token: string) => Promise<boolean>;
  setEnabled: (id: string, enabled: boolean) => Promise<boolean>;
  restart: (id: string) => Promise<boolean>;
  retry: (id: string) => Promise<boolean>;
  suppress: (id: string) => Promise<boolean>;
  disconnect: (id: string) => Promise<boolean>;
  refresh: () => Promise<void>;
  openConversation: (connector: DiscordConnector, roomId?: string) => void;
}
const button =
  'rounded-lg border border-line px-3 py-1.5 text-xs text-ink-2 disabled:opacity-50';

interface RulesDraft {
  users: string;
  roles: string;
  allowed: string;
  free: string;
  home: string;
  requireMention: boolean;
  autoThread: boolean;
  threadRequireMention: boolean;
}
function rulesDraft(connector?: DiscordConnector): RulesDraft {
  const settings = discordSettings(connector);
  return {
    users: connector?.approvedUserIds.join(', ') ?? '',
    roles: settings.approvedRoleIds.join(', '),
    allowed: settings.allowedChannelIds.join(', '),
    free: settings.freeResponseChannelIds.join(', '),
    home: settings.homeChannelId ?? '',
    requireMention: settings.requireMention,
    autoThread: settings.autoThread,
    threadRequireMention: settings.threadRequireMention,
  };
}
function parseRules(draft: RulesDraft) {
  const users = discordIdList(draft.users);
  const roles = discordIdList(draft.roles);
  const allowed = discordIdList(draft.allowed);
  const free = discordIdList(draft.free);
  if (
    !users ||
    !roles ||
    !allowed ||
    !free ||
    (draft.home && !isDiscordSnowflake(draft.home))
  )
    return null;
  return {
    users,
    settings: {
      approvedRoleIds: roles,
      allowedChannelIds: allowed,
      freeResponseChannelIds: free,
      homeChannelId: draft.home || null,
      requireMention: draft.requireMention,
      autoThread: draft.autoThread,
      threadRequireMention: draft.threadRequireMention,
    },
  };
}
function DiscordRules({
  draft,
  setDraft,
  disabled,
  creating = false,
}: {
  draft: RulesDraft;
  setDraft: (draft: RulesDraft) => void;
  disabled: boolean;
  creating?: boolean;
}) {
  const textField = (
    key: 'users' | 'roles' | 'allowed' | 'free' | 'home',
    label: string,
  ) => (
    <label className="block text-xs text-ink-3">
      {label}
      <input
        className="field mt-1"
        value={draft[key]}
        disabled={disabled}
        onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
      />
    </label>
  );
  const checkbox = (
    key: 'requireMention' | 'autoThread' | 'threadRequireMention',
    label: string,
  ) => (
    <label className="flex gap-2 text-xs text-ink-2">
      <input
        type="checkbox"
        disabled={disabled}
        checked={draft[key]}
        onChange={(e) => setDraft({ ...draft, [key]: e.target.checked })}
      />
      {label}
    </label>
  );
  return (
    <div className="space-y-3">
      <p className="text-xs font-semibold text-ink-2">
        Who can talk to the bot
      </p>
      {textField('users', 'Approved user IDs')}
      {textField('roles', 'Approved role IDs')}
      <p className="text-xs text-ink-3">
        {creating
          ? 'Leave users empty to use the verified application owners. '
          : ''}
        Only approved users or members of approved roles can start a reply. A
        mention does not grant access. Separate IDs with commas or spaces.
      </p>
      <p className="text-xs font-semibold text-ink-2">
        When and where it replies
      </p>
      {checkbox('requireMention', 'Require a mention in channels')}
      {checkbox('autoThread', 'Create a thread for mentions')}
      {checkbox(
        'threadRequireMention',
        'Require mentions in participating threads',
      )}
      <p className="text-xs text-ink-3">
        Follow-up messages in a participating thread continue naturally unless
        mentions are required there.
      </p>
      {textField('allowed', 'Allowed channel IDs (optional)')}
      <p className="text-xs text-ink-3">
        Leave empty for channels the bot can access. Allowed parent channels
        include their threads.
      </p>
      {textField('free', 'Channels that reply without a mention (optional)')}
      {textField('home', 'Home channel ID (optional)')}
      <p className="text-xs text-ink-3">
        Home is the default destination for proactive messages. Replies stay in
        their original channel or thread.
      </p>
      {!parseRules(draft) && (
        <p role="alert" className="text-xs text-danger">
          Use unique positive decimal Discord IDs without leading zeroes.
        </p>
      )}
    </div>
  );
}

function DiscordConnection({
  connector: c,
  ...p
}: Omit<DiscordSettingsProps, 'connectors' | 'create'> & {
  connector: DiscordConnector;
}) {
  const [token, setToken] = useState('');
  const [replacing, setReplacing] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [draft, setDraft] = useState(() => rulesDraft(c));
  const [editing, setEditing] = useState(false);
  useLayoutEffect(() => {
    setToken('');
    setReplacing(false);
    setReviewing(false);
    setDeleting(false);
    setDraft(rulesDraft(c));
    setEditing(false);
  }, [p.agentId, c.id, p.online, p.epoch]);
  useLayoutEffect(() => {
    if (c.unknownOutbound === 0) setReviewing(false);
  }, [c.unknownOutbound]);
  const disabled =
    p.disabled || p.online === false || p.busy !== null || p.loading;
  const warning = discordWarning(c.warning);
  const submitReplacement = () => {
    const value = token.trim();
    setToken('');
    setReplacing(false);
    if (value) void p.replace(c.id, value);
  };
  return (
    <article className="space-y-3 rounded-xl border border-line bg-white/[0.02] p-4">
      <div>
        <p className="text-sm font-semibold text-ink">
          {c.bot.displayName ?? c.bot.username ?? 'Discord bot'} ·{' '}
          {c.channel
            ? `#${c.channel.name ?? c.channel.id}`
            : 'Discord bot connection'}
        </p>
        <p className="mt-1 text-xs text-ink-3">
          {p.available === false
            ? 'Status unavailable — refresh to verify'
            : discordStatusLabel(c.status)}
        </p>
      </div>
      <p className="text-xs text-ink-3">
        {c.channel && (
          <>
            Server {c.channel.guildId} · Channel {c.channel.id}
            <br />
          </>
        )}
        Approved users: {c.approvedUserIds.join(', ')}
      </p>
      <p className="text-xs text-ink-3">
        Approved roles:{' '}
        {discordSettings(c).approvedRoleIds.join(', ') || 'None'}
      </p>
      <p className="text-xs text-ink-3">
        {discordSettings(c).requireMention
          ? 'Mention the bot to start a channel reply.'
          : 'Replies without mentions in allowed channels.'}{' '}
        {discordSettings(c).autoThread
          ? 'Mentions open a thread.'
          : 'Replies stay inline.'}
      </p>
      {c.inviteUrl && (
        <a
          className="text-xs underline"
          href={c.inviteUrl}
          target="_blank"
          rel="noreferrer"
        >
          Add to Discord
        </a>
      )}
      <details
        open={editing}
        onToggle={(e) => {
          if (e.currentTarget.open && !editing) {
            setDraft(rulesDraft(c));
            setEditing(true);
          }
          if (!e.currentTarget.open) setEditing(false);
        }}
      >
        <summary className="cursor-pointer text-xs text-ink-2">
          Edit Discord rules
        </summary>
        {editing && (
          <div className="mt-3 space-y-3">
            <DiscordRules
              draft={draft}
              setDraft={setDraft}
              disabled={Boolean(disabled)}
            />
            <button
              type="button"
              className={button}
              disabled={
                disabled ||
                !parseRules(draft) ||
                (!draft.users.trim() && !draft.roles.trim())
              }
              onClick={async () => {
                const parsed = parseRules(draft);
                if (
                  parsed &&
                  (await p.configure(c.id, parsed.settings, parsed.users))
                )
                  setEditing(false);
              }}
            >
              Save Discord rules
            </button>
          </div>
        )}
      </details>
      {!c.enabled && (
        <p className="text-xs text-ink-2">
          First activation starts with new messages after activation. Earlier
          channel history is not imported. Use Activate to start or resume
          listening.
        </p>
      )}
      <p className="text-xs text-ink-3">
        {c.pendingInbound} inbound · {c.pendingOutbound} outbound ·{' '}
        {c.unknownOutbound} uncertain
      </p>
      {warning && (
        <p role="status" className="text-xs text-danger">
          {warning}
        </p>
      )}
      {c.deliveredMessageIds.length > 0 && (
        <p className="break-all text-xs text-ink-3">
          Delivered message IDs: {c.deliveredMessageIds.join(', ')}
        </p>
      )}
      {c.unknownOutbound > 0 && (
        <div className="space-y-2 rounded-lg border border-danger/30 p-3">
          <p className="text-xs text-ink-2">
            A reply may already be in Discord. Automatic resend and Retry are
            blocked.
          </p>
          {reviewing ? (
            <>
              <p className="text-xs text-ink-2">
                Check Discord and the delivered IDs above. Suppression keeps
                delivered receipts and full conversation history, skips
                uncertain output and its unsent remainder, and allows new
                conversation work. It never resends this reply.
              </p>
              <button
                type="button"
                disabled={disabled}
                className={button}
                onClick={() => void p.suppress(c.id)}
              >
                I checked Discord — suppress unsent remainder
              </button>
              <button
                type="button"
                className={button}
                onClick={() => setReviewing(false)}
              >
                Cancel review
              </button>
            </>
          ) : (
            <button
              type="button"
              disabled={disabled}
              className={button}
              onClick={() => setReviewing(true)}
            >
              Review uncertain delivery
            </button>
          )}
        </div>
      )}
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          disabled={disabled}
          className={button}
          onClick={() =>
            c.enabled ? void p.setEnabled(c.id, false) : void p.activate(c.id)
          }
        >
          {c.enabled ? 'Disable' : 'Activate'}
        </button>
        <button
          type="button"
          disabled={disabled}
          className={button}
          onClick={() => void p.restart(c.id)}
        >
          Restart
        </button>
        <button
          type="button"
          disabled={
            disabled || c.unknownOutbound > 0 || c.pendingOutbound === 0
          }
          className={button}
          onClick={() => void p.retry(c.id)}
        >
          Retry safe delivery
        </button>
        {c.channel && (
          <button
            type="button"
            disabled={disabled}
            className={button}
            onClick={() => p.openConversation(c)}
          >
            Open conversation
          </button>
        )}
        {c.conversations
          ?.filter(
            (conversation) => !c.channel || conversation.roomId !== c.roomId,
          )
          .map((conversation) => (
            <button
              key={conversation.roomId}
              type="button"
              className={button}
              disabled={disabled}
              onClick={() => p.openConversation(c, conversation.roomId)}
            >
              Open #{conversation.name ?? conversation.channelId}
            </button>
          ))}
        <button
          type="button"
          disabled={disabled}
          className={button}
          onClick={() => {
            setToken('');
            setReplacing((v) => !v);
          }}
        >
          Replace token
        </button>
        <button
          type="button"
          disabled={disabled}
          className={button}
          onClick={() =>
            deleting ? void p.disconnect(c.id) : setDeleting(true)
          }
        >
          {deleting ? 'Confirm delete connection' : 'Delete connection'}
        </button>
      </div>
      {replacing && (
        <div>
          <p className="mb-2 text-xs text-ink-3">
            Replacement must belong to the same bot. The daemon stores the token
            in the operating-system credential vault.
          </p>
          <label
            className="text-xs text-ink-3"
            htmlFor={`discord-replacement-${c.id}`}
          >
            Replacement Discord bot token
          </label>
          <input
            className="field"
            id={`discord-replacement-${c.id}`}
            type="password"
            autoComplete="off"
            disabled={disabled}
            value={token}
            onChange={(e) => setToken(e.target.value)}
          />
          <button
            type="button"
            className={`${button} mt-2`}
            disabled={disabled || !token.trim()}
            onClick={submitReplacement}
          >
            Save replacement token
          </button>
        </div>
      )}
    </article>
  );
}

export function DiscordSettings(p: DiscordSettingsProps) {
  const [token, setToken] = useState('');
  const [guild, setGuild] = useState('');
  const [channel, setChannel] = useState('');
  const [draft, setDraft] = useState(() => rulesDraft());
  const [checked, setChecked] = useState<DiscordBotCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const request = useRef(0);
  useLayoutEffect(() => {
    setGuild('');
    setChannel('');
    setDraft(rulesDraft());
  }, [p.agentId]);
  useLayoutEffect(() => {
    request.current++;
    setToken('');
    setChecked(null);
    setChecking(false);
    return () => {
      request.current++;
    };
  }, [p.agentId, p.online, p.epoch]);
  const disabled = Boolean(
    p.disabled ||
    p.online === false ||
    p.busy !== null ||
    p.loading ||
    checking,
  );
  const tokenDisabled = Boolean(
    p.disabled || p.online === false || (p.busy !== null && p.busy !== 'check'),
  );
  const parsed = parseRules(draft);
  const valid =
    token.trim() &&
    checked &&
    parsed &&
    ((!guild && !channel) ||
      (isDiscordSnowflake(guild) && isDiscordSnowflake(channel)));
  const check = async () => {
    const revision = ++request.current;
    setChecked(null);
    setChecking(true);
    try {
      const result = await p.check(token.trim());
      if (revision === request.current) setChecked(result);
    } finally {
      if (revision === request.current) setChecking(false);
    }
  };
  const create = () => {
    if (!valid || !parsed) return;
    const secret = token.trim();
    request.current++;
    setToken('');
    setChecked(null);
    void p.create({
      botToken: secret,
      ...(channel ? { guildId: guild, channelId: channel } : {}),
      approvedUserIds: parsed.users,
      settings: parsed.settings,
    });
  };
  return (
    <section className="space-y-3" aria-label="Discord connection setup">
      <SectionTitle>Discord</SectionTitle>
      <p className="text-xs leading-relaxed text-ink-2">
        Connect one bot and mention it in a server channel to start talking.
        Replies stay in the channel or thread where each message arrives. Each
        channel and thread keeps its own conversation history.
      </p>
      <p className="text-xs leading-relaxed text-ink-2">
        Create a bot in the{' '}
        <a
          className="underline"
          href="https://discord.com/developers/applications"
          target="_blank"
          rel="noreferrer"
        >
          Discord Developer Portal
        </a>
        , then check its token below to get an invitation. Enable Message
        Content intent in the Bot settings. The bot needs View Channel, Read
        Message History, and Send Messages, plus Create Public Threads and Send
        Messages in Threads for automatic threads.
      </p>
      {p.connectors.map((c) => (
        <DiscordConnection key={p.agentId + ':' + c.id} {...p} connector={c} />
      ))}
      <details open={p.connectors.length === 0}>
        <summary className="cursor-pointer text-xs text-ink-2">
          Connect a Discord bot
        </summary>
        <div className="mt-3 space-y-3">
          <p className="text-xs text-ink-3">
            Check the bot, add it to Discord, then save disabled and Activate.
            No channel ID is required. The daemon stores your token in the
            operating-system credential vault.
          </p>
          <label className="block text-xs text-ink-3">
            Discord bot token
            <input
              className="field mt-1"
              type="password"
              autoComplete="off"
              disabled={tokenDisabled}
              value={token}
              onChange={(e) => {
                request.current++;
                setToken(e.target.value);
                setChecked(null);
                setChecking(false);
              }}
            />
          </label>
          <button
            type="button"
            className={button}
            disabled={disabled || !token.trim()}
            onClick={() => void check()}
          >
            {checking ? 'Checking bot…' : 'Check bot token'}
          </button>
          {checked && (
            <div className="space-y-3 rounded-lg border border-line p-3">
              <p className="text-xs text-ink-2">
                Verified bot:{' '}
                {checked.bot.displayName ??
                  checked.bot.username ??
                  checked.bot.id}
              </p>
              <a
                className="text-xs underline"
                href={checked.inviteUrl}
                target="_blank"
                rel="noreferrer"
              >
                Add to Discord
              </a>
              <p className="text-xs text-ink-3">
                {checked.messageContentEnabled
                  ? 'Message Content intent is enabled.'
                  : 'Enable Message Content intent in Discord Developer Portal → Bot → Privileged Gateway Intents before activation.'}
              </p>
              <p className="text-xs text-ink-3">
                Verified application owners:{' '}
                {checked.ownerUserIds.join(', ') ||
                  'None found; add approved users or roles below.'}
              </p>
              <details>
                <summary className="cursor-pointer text-xs text-ink-2">
                  Access and reply rules
                </summary>
                <div className="mt-3">
                  <DiscordRules
                    draft={draft}
                    setDraft={setDraft}
                    disabled={disabled}
                    creating
                  />
                </div>
              </details>
            </div>
          )}
          <details>
            <summary className="cursor-pointer text-xs text-ink-2">
              Advanced: limit this connection to one channel
            </summary>
            <div className="mt-3 space-y-3">
              <label className="block text-xs text-ink-3">
                Server ID
                <input
                  className="field mt-1"
                  disabled={disabled}
                  value={guild}
                  onChange={(e) => setGuild(e.target.value)}
                />
              </label>
              <label className="block text-xs text-ink-3">
                Channel ID
                <input
                  className="field mt-1"
                  disabled={disabled}
                  value={channel}
                  onChange={(e) => setChannel(e.target.value)}
                />
              </label>
              <p className="text-xs text-ink-3">
                Both fields are optional. Use Discord Developer Mode to copy IDs
                when setting a specific channel or rule.
              </p>
            </div>
          </details>
          <button
            type="button"
            className={primaryBtnCls}
            disabled={
              disabled ||
              !valid ||
              Boolean(
                checked &&
                !checked.ownerUserIds.length &&
                !draft.users.trim() &&
                !draft.roles.trim(),
              )
            }
            onClick={create}
          >
            Save Discord connection
          </button>
        </div>
      </details>
      <button
        type="button"
        disabled={disabled}
        className={button}
        onClick={() => void p.refresh()}
      >
        {p.loading ? 'Loading Discord…' : 'Refresh Discord status'}
      </button>
      {p.error && (
        <p role="alert" className="text-xs text-danger">
          {p.error}
        </p>
      )}
    </section>
  );
}
