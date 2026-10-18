import { useLayoutEffect, useState } from 'react';
import type {
  DiscordConnector,
  CreateDiscordConnectorInput,
} from '../lib/daemon-api';
import {
  discordApprovedUsers,
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
  activate: (id: string) => Promise<boolean>;
  replace: (id: string, token: string) => Promise<boolean>;
  setEnabled: (id: string, enabled: boolean) => Promise<boolean>;
  restart: (id: string) => Promise<boolean>;
  retry: (id: string) => Promise<boolean>;
  suppress: (id: string) => Promise<boolean>;
  disconnect: (id: string) => Promise<boolean>;
  refresh: () => Promise<void>;
  openConversation: (connector: DiscordConnector) => void;
}
const button =
  'rounded-lg border border-line px-3 py-1.5 text-xs text-ink-2 disabled:opacity-50';

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
  useLayoutEffect(() => {
    setToken('');
    setReplacing(false);
    setReviewing(false);
    setDeleting(false);
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
          {c.bot.displayName ?? c.bot.username ?? 'Discord bot'} · #
          {c.channel.name ?? c.channel.id}
        </p>
        <p className="mt-1 text-xs text-ink-3">
          {p.available === false
            ? 'Status unavailable — refresh to verify'
            : discordStatusLabel(c.status)}
        </p>
      </div>
      <p className="text-xs text-ink-3">
        Server {c.channel.guildId} · Channel {c.channel.id}
        <br />
        Approved users: {c.approvedUserIds.join(', ')}
      </p>
      <p className="text-xs text-ink-3">
        Channel and approved users are fixed. Delete and recreate this
        connection to change them.
      </p>
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
        <button
          type="button"
          disabled={disabled}
          className={button}
          onClick={() => p.openConversation(c)}
        >
          Open conversation
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
  const [users, setUsers] = useState('');
  const [privateConfirmed, setPrivateConfirmed] = useState(false);
  useLayoutEffect(() => {
    setToken('');
    setGuild('');
    setChannel('');
    setUsers('');
    setPrivateConfirmed(false);
  }, [p.agentId]);
  useLayoutEffect(() => {
    setToken('');
    setPrivateConfirmed(false);
  }, [p.online, p.epoch]);
  const approved = discordApprovedUsers(users);
  const disabled =
    p.disabled || p.online === false || p.busy !== null || p.loading;
  const valid =
    token.trim() &&
    isDiscordSnowflake(guild) &&
    isDiscordSnowflake(channel) &&
    approved !== null &&
    privateConfirmed;
  const create = () => {
    const secret = token.trim();
    setToken('');
    if (valid && approved)
      void p.create({
        botToken: secret,
        guildId: guild,
        channelId: channel,
        approvedUserIds: approved,
        privateChannelConfirmed: true,
      });
  };
  return (
    <section className="space-y-3" aria-label="Discord connection setup">
      <SectionTitle>Discord</SectionTitle>
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
        , install it in your server, enable Message Content intent (verified
        apps may need approval), and grant View Channel, Read Message History,
        and Send Messages for your selected guild text channel.
      </p>
      <p className="text-xs leading-relaxed text-ink-3">
        Use a private, low-volume channel and exact approved sender IDs. Privacy
        is your confirmation; Anima cannot verify who else can view it. Online
        status follows the live Discord connection. Typing appears while the
        assistant is working. Message events trigger catch-up, with periodic
        history checks for recovery. Messages deleted before catch-up can be
        missed; edits, delete events, reactions, threads, voice, and Discord
        slash commands are unavailable. Stop runs from the web conversation.
      </p>
      {p.connectors.map((c) => (
        <DiscordConnection key={`${p.agentId}:${c.id}`} {...p} connector={c} />
      ))}
      <details open={p.connectors.length === 0}>
        <summary className="cursor-pointer text-xs text-ink-2">
          Add selected channel
        </summary>
        <div className="mt-3 space-y-3">
          <p className="text-xs text-ink-3">
            Save disabled, then Activate to listen for new messages. The daemon
            stores your token in the operating-system credential vault. Enable
            Developer Mode in Discord to copy server, channel, and user IDs.
          </p>
          <label className="block text-xs text-ink-3">
            Discord bot token
            <input
              className="field mt-1"
              type="password"
              autoComplete="off"
              disabled={disabled}
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
          </label>
          <label className="block text-xs text-ink-3">
            Server ID
            <input
              className="field mt-1"
              inputMode="numeric"
              disabled={disabled}
              value={guild}
              onChange={(e) => setGuild(e.target.value)}
            />
          </label>
          <label className="block text-xs text-ink-3">
            Channel ID
            <input
              className="field mt-1"
              inputMode="numeric"
              disabled={disabled}
              value={channel}
              onChange={(e) => setChannel(e.target.value)}
            />
          </label>
          <label className="block text-xs text-ink-3">
            Approved user IDs
            <input
              className="field mt-1"
              disabled={disabled}
              value={users}
              onChange={(e) => setUsers(e.target.value)}
            />
          </label>
          <p className="text-xs text-ink-3">
            Enter 1–16 unique positive decimal user IDs, separated by commas or
            spaces. Leading zeroes are invalid.
          </p>
          <label className="flex gap-2 text-xs text-ink-2">
            <input
              type="checkbox"
              disabled={disabled}
              checked={privateConfirmed}
              onChange={(e) => setPrivateConfirmed(e.target.checked)}
            />
            I confirm this selected channel is private and appropriate for my
            assistant.
          </label>
          <button
            type="button"
            className={primaryBtnCls}
            disabled={disabled || !valid}
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
