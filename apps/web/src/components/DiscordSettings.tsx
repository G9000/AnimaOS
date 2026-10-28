import { useResetOnChange } from '../hooks/useResetOnChange';
import { useCallback, useLayoutEffect, useRef, useState } from 'react';
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
import { primaryBtnCls } from './ui-bits';

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
  'rounded border border-line px-3 py-1.5 text-xs text-ink-2 hover:border-ink-3 hover:text-ink disabled:cursor-not-allowed disabled:opacity-50';
const buttonPrimary =
  'rounded border border-accent bg-accent/10 px-3 py-1.5 text-xs text-ink hover:bg-accent/20 disabled:cursor-not-allowed disabled:opacity-50';
const buttonDanger =
  'rounded border border-danger px-3 py-1.5 text-xs text-danger disabled:cursor-not-allowed disabled:opacity-50';

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
    <label className="block text-xs text-ink-2">
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
    <div className="space-y-5">
      <div className="space-y-3">
        <p className="discord-group-title">Who can talk to the bot</p>
        {textField('users', 'Approved user IDs')}
        {textField('roles', 'Approved role IDs')}
        <p className="discord-hint">
          {creating
            ? 'Leave users empty to use the verified application owners. '
            : ''}
          Only approved users or members of approved roles can start a reply. A
          mention does not grant access. Separate IDs with commas or spaces.
        </p>
      </div>
      <div className="space-y-3">
        <p className="discord-group-title">When and where it replies</p>
        {checkbox('requireMention', 'Require a mention in channels')}
        {checkbox('autoThread', 'Create a thread for mentions')}
        {checkbox(
          'threadRequireMention',
          'Require mentions in participating threads',
        )}
        <p className="discord-hint">
          Follow-up messages in a participating thread continue naturally unless
          mentions are required there.
        </p>
        {textField('allowed', 'Allowed channel IDs (optional)')}
        {textField('free', 'Channels that reply without a mention (optional)')}
        {textField('home', 'Home channel ID (optional)')}
        <p className="discord-hint">
          Leave allowed channels empty for every channel the bot can access;
          parent channels include their threads. Home is the default
          destination for proactive messages. Replies stay in their original
          channel or thread.
        </p>
      </div>
      {!parseRules(draft) && (
        <p role="alert" className="text-xs text-danger">
          Use unique positive decimal Discord IDs without leading zeroes.
        </p>
      )}
    </div>
  );
}

function count(n: number, noun: string) {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
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
  useResetOnChange([p.agentId, c.id, p.online, p.epoch], () => {
    setToken('');
    setReplacing(false);
    setReviewing(false);
    setDeleting(false);
    setDraft(rulesDraft(c));
    setEditing(false);
  });
  useResetOnChange([c.unknownOutbound], () => {
    if (c.unknownOutbound === 0) setReviewing(false);
  });
  const disabled =
    p.disabled || p.online === false || p.busy !== null || p.loading;
  const warning = discordWarning(c.warning);
  const rules = discordSettings(c);
  const submitReplacement = () => {
    const value = token.trim();
    setToken('');
    setReplacing(false);
    if (value) void p.replace(c.id, value);
  };
  const unavailable = p.available === false;
  const tone = unavailable
    ? 'attention'
    : c.status === 'ready'
      ? 'ok'
      : ['error', 'degraded', 'credentialRequired'].includes(c.status)
        ? 'attention'
        : 'idle';
  const who = [
    count(c.approvedUserIds.length, 'user'),
    rules.approvedRoleIds.length > 0
      ? count(rules.approvedRoleIds.length, 'role')
      : null,
  ]
    .filter(Boolean)
    .join(' · ');
  const queued = c.pendingInbound + c.pendingOutbound + c.unknownOutbound > 0;
  return (
    <article className="discord-card">
      <header className="discord-head">
        <div>
          <p className="discord-title">
            {c.bot.displayName ?? c.bot.username ?? 'Discord bot'}
          </p>
          <p className="discord-sub">
            {c.channel
              ? `#${c.channel.name ?? c.channel.id}`
              : 'Discord bot connection'}
          </p>
        </div>
        <span className="connections-row-status" data-tone={tone}>
          {unavailable
            ? 'Status unavailable — refresh to verify'
            : discordStatusLabel(c.status)}
        </span>
      </header>
      {!c.enabled && (
        <p className="discord-note">
          First activation starts with new messages after activation. Earlier
          channel history is not imported. Use Activate to start or resume
          listening.
        </p>
      )}
      {warning && (
        <p role="status" className="text-xs text-danger">
          {warning}
        </p>
      )}
      <dl className="discord-facts">
        <div>
          <dt>Who can reply</dt>
          <dd>{who}</dd>
        </div>
        <div>
          <dt>In channels</dt>
          <dd>
            {rules.requireMention
              ? 'Needs a mention'
              : 'Replies without a mention'}
            {' · '}
            {rules.autoThread ? 'opens a thread' : 'replies inline'}
          </dd>
        </div>
        {queued && (
          <div>
            <dt>Queue</dt>
            <dd>
              {c.pendingInbound} inbound · {c.pendingOutbound} outbound ·{' '}
              {c.unknownOutbound} uncertain
            </dd>
          </div>
        )}
      </dl>
      {c.unknownOutbound > 0 && (
        <div className="space-y-2 rounded border border-danger/30 p-3">
          <p className="text-xs text-ink-2">
            A reply may already be in Discord. Automatic resend and Retry are
            blocked.
          </p>
          {c.deliveredMessageIds.length > 0 && (
            <p className="break-all text-xs text-ink-3">
              Already delivered, by message ID:{' '}
              {c.deliveredMessageIds.join(', ')}
            </p>
          )}
          {reviewing ? (
            <>
              <p className="text-xs text-ink-2">
                Check Discord and the delivered IDs above. Suppression keeps
                delivered receipts and full conversation history, skips
                uncertain output and its unsent remainder, and allows new
                conversation work. It never resends this reply.
              </p>
              <div className="flex flex-wrap gap-2">
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
              </div>
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
      <div className="discord-actions">
        {c.channel && (
          <button
            type="button"
            disabled={disabled}
            className={buttonPrimary}
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
          onClick={() =>
            c.enabled ? void p.setEnabled(c.id, false) : void p.activate(c.id)
          }
        >
          {c.enabled ? 'Disable' : 'Activate'}
        </button>
        {c.inviteUrl && (
          <a
            className="discord-link"
            href={c.inviteUrl}
            target="_blank"
            rel="noreferrer"
          >
            Add to Discord
          </a>
        )}
      </div>
      <details
        className="discord-fold"
        open={editing}
        onToggle={(e) => {
          if (e.currentTarget.open && !editing) {
            setDraft(rulesDraft(c));
            setEditing(true);
          }
          if (!e.currentTarget.open) setEditing(false);
        }}
      >
        <summary>Edit Discord rules</summary>
        {editing && (
          <div className="mt-3 space-y-4">
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
      <details className="discord-fold">
        <summary>IDs</summary>
        <p className="discord-ids">
          {c.channel && (
            <>
              Server {c.channel.guildId} · Channel {c.channel.id}
              <br />
            </>
          )}
          Approved users: {c.approvedUserIds.join(', ')}
          <br />
          Approved roles: {rules.approvedRoleIds.join(', ') || 'None'}
        </p>
      </details>
      <details className="discord-fold">
        <summary>More</summary>
        <div className="discord-actions mt-3">
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
            className={deleting ? buttonDanger : button}
            onClick={() =>
              deleting ? void p.disconnect(c.id) : setDeleting(true)
            }
          >
            {deleting ? 'Confirm delete connection' : 'Delete connection'}
          </button>
        </div>
      </details>
      {replacing && (
        <div className="space-y-2">
          <p className="text-xs text-ink-3">
            Replacement must belong to the same bot. The daemon stores the token
            in the operating-system credential vault.
          </p>
          <label
            className="block text-xs text-ink-2"
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
            className={button}
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
  const invalidateCheck = useCallback(() => {
    request.current += 1;
  }, []);
  useResetOnChange([p.agentId], () => {
    setGuild('');
    setChannel('');
    setDraft(rulesDraft());
  });
  useResetOnChange([p.agentId, p.online, p.epoch], () => {
    setToken('');
    setChecked(null);
    setChecking(false);
  });
  useLayoutEffect(() => {
    invalidateCheck();
    return invalidateCheck;
  }, [p.agentId, p.online, p.epoch, invalidateCheck]);
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
    <section className="space-y-4" aria-label="Discord connection setup">
      <p className="text-xs leading-relaxed text-ink-2">
        Mention the bot in a server channel to talk to it. Replies stay in the
        channel or thread where each message arrives, and each keeps its own
        history.
      </p>
      {p.connectors.map((c) => (
        <DiscordConnection key={p.agentId + ':' + c.id} {...p} connector={c} />
      ))}
      <details
        className={p.connectors.length === 0 ? 'discord-setup' : 'discord-fold'}
        open={p.connectors.length === 0}
      >
        <summary>Connect a Discord bot</summary>
        <div className="mt-3 space-y-4">
          <details className="discord-guide">
            <summary>How to create a bot</summary>
            <p>
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
              Content intent in the Bot settings. The bot needs View Channel,
              Read Message History, and Send Messages, plus Create Public
              Threads and Send Messages in Threads for automatic threads.
            </p>
          </details>
          <div className="space-y-3">
            <label className="block text-xs text-ink-2">
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
            <p className="discord-hint">
              Check the bot, add it to Discord, then save and Activate. No
              channel ID is needed. The daemon stores your token in the
              operating-system credential vault.
            </p>
            <button
              type="button"
              className={button}
              disabled={disabled || !token.trim()}
              onClick={() => void check()}
            >
              {checking ? 'Checking bot…' : 'Check bot token'}
            </button>
          </div>
          {checked && (
            <div className="discord-card">
              <header className="discord-head">
                <div>
                  <p className="discord-title">
                    {checked.bot.displayName ??
                      checked.bot.username ??
                      checked.bot.id}
                  </p>
                  <p className="discord-sub">Verified bot</p>
                </div>
                <a
                  className="discord-link"
                  href={checked.inviteUrl}
                  target="_blank"
                  rel="noreferrer"
                >
                  Add to Discord
                </a>
              </header>
              <dl className="discord-facts">
                <div>
                  <dt>Message Content</dt>
                  <dd>
                    {checked.messageContentEnabled
                      ? 'Message Content intent is enabled.'
                      : 'Enable Message Content intent in Discord Developer Portal → Bot → Privileged Gateway Intents before activation.'}
                  </dd>
                </div>
                <div>
                  <dt>Owners</dt>
                  <dd>
                    {checked.ownerUserIds.join(', ') ||
                      'None found; add approved users or roles below.'}
                  </dd>
                </div>
              </dl>
              <details className="discord-fold">
                <summary>Access and reply rules</summary>
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
          <details className="discord-fold">
            <summary>Advanced: limit this connection to one channel</summary>
            <div className="mt-3 space-y-3">
              <label className="block text-xs text-ink-2">
                Server ID
                <input
                  className="field mt-1"
                  disabled={disabled}
                  value={guild}
                  onChange={(e) => setGuild(e.target.value)}
                />
              </label>
              <label className="block text-xs text-ink-2">
                Channel ID
                <input
                  className="field mt-1"
                  disabled={disabled}
                  value={channel}
                  onChange={(e) => setChannel(e.target.value)}
                />
              </label>
              <p className="discord-hint">
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
      <div className="discord-foot">
        <button
          type="button"
          disabled={disabled}
          className={button}
          onClick={() => void p.refresh()}
        >
          {p.loading ? 'Loading Discord…' : 'Refresh Discord status'}
        </button>
      </div>
      {p.error && (
        <p role="alert" className="text-xs text-danger">
          {p.error}
        </p>
      )}
    </section>
  );
}
