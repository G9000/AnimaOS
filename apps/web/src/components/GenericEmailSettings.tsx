import { useResetOnChange } from '../hooks/useResetOnChange';
import { useState } from 'react';
import type {
  ConnectGenericEmailInput,
  EmailEndpoint,
  GenericEmailSettings as EmailSettings,
} from '@animaos/sdk';
import { primaryBtnCls } from './ui-bits';

const endpoint = (port: number): EmailEndpoint => ({
  host: '',
  port,
  security: 'tls',
  username: '',
});

/** Passwords stay only in this form's lifetime and the submitted request. */
export function GenericEmailSettings({
  agentId,
  online,
  epoch,
  connected,
  settings,
  busy,
  onConnect,
}: {
  agentId: string;
  online: boolean;
  epoch: number;
  connected: boolean;
  settings?: EmailSettings | null;
  busy: boolean;
  onConnect: (input: ConnectGenericEmailInput) => void;
}) {
  const [address, setAddress] = useState('');
  const [imap, setImap] = useState(() => endpoint(993));
  const [smtp, setSmtp] = useState(() => endpoint(465));
  const [imapPassword, setImapPassword] = useState('');
  const [smtpPassword, setSmtpPassword] = useState('');
  const [automatic, setAutomatic] = useState(false);
  const [allowedSenders, setAllowedSenders] = useState('');
  const [authservId, setAuthservId] = useState('');
  const [error, setError] = useState<string | null>(null);
  const savedSettings = settings ? JSON.stringify(settings) : '';

  useResetOnChange([agentId, online, epoch], () => {
    setImapPassword('');
    setSmtpPassword('');
    setError(null);
  });

  useResetOnChange([agentId, savedSettings], () => {
    const saved: EmailSettings | null = savedSettings
      ? JSON.parse(savedSettings)
      : null;
    setImapPassword('');
    setSmtpPassword('');
    setAddress(saved?.address ?? '');
    setImap(saved?.imap ?? endpoint(993));
    setSmtp(saved?.smtp ?? endpoint(465));
    setAutomatic(saved?.autoReply.enabled ?? false);
    setAllowedSenders(saved?.autoReply.allowedSenders.join(', ') ?? '');
    setAuthservId(saved?.autoReply.authservId ?? '');
  });

  const validAddress = (value: string) =>
    /^[^\s<>@,;]+@[^\s<>@,;]+\.[^\s<>@,;]+$/.test(value);
  const submit = () => {
    if (busy || !online) return;
    setError(null);
    const senders = [
      ...new Set(
        allowedSenders
          .split(/[,\n]/)
          .map((value) => value.trim().toLowerCase())
          .filter(Boolean),
      ),
    ];
    if (automatic && !senders.length) {
      setError(
        'Add at least one allowed sender before enabling automatic replies.',
      );
      return;
    }
    if (senders.some((sender) => !validAddress(sender))) {
      setError(
        'Allowed senders must be individual email addresses, separated by commas or new lines.',
      );
      return;
    }
    if (automatic && !authservId.trim()) {
      setError('Receiving mail server ID is required for automatic replies.');
      return;
    }
    if (
      !validAddress(address.trim()) ||
      !imap.host.trim() ||
      !smtp.host.trim() ||
      !imap.username.trim() ||
      !smtp.username.trim() ||
      !imapPassword ||
      !smtpPassword
    ) {
      setError(
        'Enter an email address, both server hosts, usernames, and passwords.',
      );
      return;
    }
    if (
      [imap.port, smtp.port].some(
        (port) => !Number.isInteger(port) || port < 1 || port > 65535,
      )
    ) {
      setError('Server ports must be whole numbers between 1 and 65535.');
      return;
    }
    const input: ConnectGenericEmailInput = {
      address: address.trim(),
      imap: { ...imap, host: imap.host.trim(), username: imap.username.trim() },
      smtp: { ...smtp, host: smtp.host.trim(), username: smtp.username.trim() },
      imapPassword,
      smtpPassword,
      autoReply: {
        enabled: automatic,
        allowedSenders: senders,
        authservId: authservId.trim(),
      },
    };
    setImapPassword('');
    setSmtpPassword('');
    onConnect(input);
  };

  return (
    <form
      className="space-y-4"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <p className="text-xs leading-relaxed text-ink-3">
        Works with any mailbox. Connecting only checks your login; it sends
        nothing. Passwords stay in your system’s credential vault.
      </p>
      <fieldset className="min-w-0 space-y-4" disabled={busy || !online}>
        <label className="block text-xs text-ink-2">
          Email address
          <input
            aria-label="Email address"
            className="field mt-1 w-full"
            type="email"
            value={address}
            onChange={(event) => setAddress(event.target.value)}
            autoComplete="off"
          />
        </label>
        <div className="grid min-w-0 gap-4">
        {(
          [
            ['IMAP', imap, setImap, imapPassword, setImapPassword],
            ['SMTP', smtp, setSmtp, smtpPassword, setSmtpPassword],
          ] as const
        ).map(([name, server, setServer, password, setPassword]) => (
          <fieldset
            key={name}
            className="grid min-w-0 gap-3 rounded-xl border border-line p-3 sm:grid-cols-2"
          >
            <legend className="px-1 text-xs font-medium text-ink">
              {name === 'IMAP' ? 'Incoming mail (IMAP)' : 'Outgoing mail (SMTP)'}
            </legend>
            <label className="block min-w-0 text-xs text-ink-2 sm:col-span-2">
              Server
              <input
                aria-label={`${name} host`}
                className="field mt-1 w-full"
                value={server.host}
                onChange={(event) =>
                  setServer({ ...server, host: event.target.value })
                }
                autoComplete="off"
              />
            </label>
            <label className="block min-w-0 text-xs text-ink-2">
              Port
              <input
                aria-label={`${name} port`}
                className="field mt-1 w-full"
                type="number"
                min={1}
                max={65535}
                value={server.port || ''}
                onChange={(event) =>
                  setServer({ ...server, port: Number(event.target.value) })
                }
              />
            </label>
            <label className="block min-w-0 text-xs text-ink-2">
              Encryption
              <select
                aria-label={`${name} security`}
                className="field mt-1 w-full"
                value={server.security}
                onChange={(event) =>
                  setServer({
                    ...server,
                    security: event.target.value as EmailEndpoint['security'],
                  })
                }
              >
                <option value="tls">SSL/TLS</option>
                <option value="starttls">STARTTLS</option>
              </select>
            </label>
            <label className="block min-w-0 text-xs text-ink-2 sm:col-span-2">
              Username
              <input
                aria-label={`${name} username`}
                className="field mt-1 w-full"
                value={server.username}
                onChange={(event) =>
                  setServer({ ...server, username: event.target.value })
                }
                autoComplete="off"
              />
            </label>
            <label className="block min-w-0 text-xs text-ink-2 sm:col-span-2">
              Password
              <input
                aria-label={`${name} password`}
                className="field mt-1 w-full"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </label>
          </fieldset>
        ))}
        </div>
        <details className="space-y-3 rounded-xl border border-line p-3">
          <summary className="cursor-pointer text-xs font-medium text-ink">
            Automatic replies (advanced)
          </summary>
          <label className="flex items-start gap-2 text-xs text-ink-2">
            <input
              type="checkbox"
              checked={automatic}
              onChange={(event) => setAutomatic(event.target.checked)}
            />
            Reply automatically to the senders below
          </label>
          <p className="text-xs leading-relaxed text-ink-3">
            Replies go out without asking you first, and only to mail that
            arrives after you connect.
          </p>
          <label className="block text-xs text-ink-2">
            Allowed senders
            <textarea
              aria-label="Allowed senders"
              className="field mt-1 w-full"
              placeholder="friend@example.com, colleague@example.com"
              value={allowedSenders}
              onChange={(event) => setAllowedSenders(event.target.value)}
            />
          </label>
          <label className="block text-xs text-ink-2">
            Mail server ID
            <input
              aria-label="Receiving mail server ID"
              className="field mt-1 w-full"
              value={authservId}
              onChange={(event) => setAuthservId(event.target.value)}
              aria-describedby={`email-authserv-${agentId}`}
            />
          </label>
          <p
            id={`email-authserv-${agentId}`}
            className="text-xs leading-relaxed text-ink-3"
          >
            Your provider’s authentication server ID, such as mx.google.com.
            Only turn this on if your provider removes forged sender headers.
          </p>
        </details>
        <button type="submit" className={primaryBtnCls}>
          {connected ? 'Reconnect Email' : 'Connect Email'}
        </button>
      </fieldset>
      {error && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
    </form>
  );
}
