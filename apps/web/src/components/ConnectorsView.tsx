import { ownerFetch } from '../lib/owner-fetch';
import {
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  createDaemonClient,
  type MailProvider,
  type MailDraft,
  type OAuthAppProvider,
} from '@animaos/sdk';
import { primaryBtnCls } from './ui-bits';
import { GoogleDriveCard } from './GoogleDriveCard';
import { ConnectedAccountsPanel } from './connected-accounts/ConnectedAccountsPanel';
import { GenericEmailSettings } from './GenericEmailSettings';
import { McpConnectionsPanel } from './mcp/McpConnectionsPanel';
import { CommunityBundlesPanel } from './community-bundles/CommunityBundlesPanel';
import { useConnectorLifetime } from '../hooks/useConnectorLifetime';
import { useLatest } from '../hooks/useLatest';
import { useResetOnChange } from '../hooks/useResetOnChange';

const client = createDaemonClient({ baseUrl: '', fetch: ownerFetch }).connectors;
const button =
  'rounded-lg border border-line px-3 py-2 text-xs text-ink-2 hover:bg-white/[0.04] disabled:cursor-not-allowed disabled:opacity-50';

const ConnectorContext = createContext({
  owner: '',
  online: true,
  epoch: 0,
  disabled: false,
});

// Reads and submitted writes belong to one owner/connection lifetime.
function useService<T>(
  load: (explicit: boolean, current: () => boolean) => Promise<T>,
) {
  const context = useContext(ConnectorContext);
  const lifetime = useConnectorLifetime(
    context.owner,
    context.online,
    context.epoch,
  );
  const allowed = useLatest(!context.disabled);
  const loader = useLatest(load);
  const alive = useRef(false),
    generation = useRef({ value: 0 }),
    locked = useRef(false);
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  async function refresh(clearError = true, explicit = false, initial = false) {
    if (!lifetime.isCurrent(lifetime.token) || locked.current) return;
    const counter = generation.current;
    const current = ++counter.value;
    const valid = () =>
      alive.current &&
      current === counter.value &&
      lifetime.isCurrent(lifetime.token);
    if (!initial) setLoading(true);
    try {
      const next = await loader.current(explicit, valid);
      if (valid()) {
        setData(next);
        if (clearError) setError(null);
      }
    } catch {
      if (valid()) setError('Could not refresh this connection. Try again.');
    } finally {
      if (valid()) setLoading(false);
    }
  }
  useResetOnChange([lifetime.token], () => {
    setData(null);
    setError(null);
    setBusy(false);
    setLoading(context.online);
  });
  useLayoutEffect(() => {
    const counter = generation.current;
    alive.current = true;
    counter.value++;
    locked.current = false;
    if (!context.online)
      return () => {
        alive.current = false;
        counter.value++;
      };
    void refresh(true, false, true);
    const timer = window.setInterval(() => {
      if (document.visibilityState !== 'hidden') void refresh();
    }, 15000);
    return () => {
      alive.current = false;
      counter.value++;
      window.clearInterval(timer);
    };
    // Refresh captures this exact connection lifetime.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lifetime.token]);
  async function run<R>(
    action: () => Promise<R>,
    onSuccess?: (result: R) => void,
  ) {
    if (
      !lifetime.isCurrent(lifetime.token) ||
      !allowed.current ||
      locked.current ||
      !alive.current
    )
      return false;
    locked.current = true;
    const counter = generation.current;
    const submittedGeneration = ++counter.value;
    const current = () =>
      alive.current &&
      submittedGeneration === counter.value &&
      lifetime.isCurrent(lifetime.token);
    setBusy(true);
    setError(null);
    let success = false;
    try {
      const result = await action();
      success = true;
      if (current()) onSuccess?.(result);
    } catch (cause) {
      if (current())
        setError(
          cause instanceof Error &&
            cause.message ===
              'The provider returned an invalid authorization link.'
            ? cause.message
            : 'This action could not be completed. Refresh the connection before trying again.',
        );
    } finally {
      if (current()) {
        locked.current = false;
        await refresh(false);
        if (lifetime.isCurrent(lifetime.token)) setBusy(false);
      }
    }
    return success && lifetime.isCurrent(lifetime.token) && alive.current;
  }
  return {
    data,
    error,
    busy: busy || !context.online || context.disabled,
    loading,
    refresh,
    run,
  };
}

function authorizationUrl(value: string, provider: 'google' | 'outlook') {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error('The provider returned an invalid authorization link.');
  }
  const origin =
    provider === 'google'
      ? 'https://accounts.google.com'
      : 'https://login.microsoftonline.com';
  if (url.origin !== origin || url.username || url.password)
    throw new Error('The provider returned an invalid authorization link.');
  return url.href;
}
function Card({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className="min-w-0 space-y-4 connections-card">
      <div>
        <h2 className="text-base font-semibold text-ink">{title}</h2>
        <p className="mt-1 text-xs leading-relaxed text-ink-3">{description}</p>
      </div>
      {children}
    </section>
  );
}
function ConnectionControls({
  name,
  configured,
  connected,
  status,
  account,
  busy,
  loading,
  error,
  consent,
  provider,
  onConnect,
  onDisconnect,
  onRefresh,
}: {
  name: string;
  configured: boolean;
  connected: boolean;
  status: string;
  account?: string | null;
  busy: boolean;
  loading: boolean;
  error: string | null;
  consent: string | null;
  provider: 'google' | 'outlook';
  onConnect: () => void;
  onDisconnect: () => void;
  onRefresh: () => void;
}) {
  return (
    <>
      <div className="space-y-1 text-xs">
        <p role="status" className="font-medium text-accent">
          {loading && !status ? 'Loading connection…' : status}
        </p>
        {account && <p className="break-all text-ink-2">{account}</p>}
      </div>
      {status === 'Setup required' && !loading && (
        <p className="text-xs leading-relaxed text-ink-3">
          OAuth is not configured on this daemon. Ask the workspace owner to
          configure {provider === 'google' ? 'Google' : 'Microsoft'}{' '}
          credentials, then refresh.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <button
          className={primaryBtnCls}
          disabled={busy || loading || !configured}
          onClick={onConnect}
        >
          {connected ? `Reconnect ${name}` : `Connect ${name}`}
        </button>
        {connected && (
          <button className={button} disabled={busy} onClick={onDisconnect}>
            Disconnect {name}
          </button>
        )}
        <button
          className={button}
          disabled={busy || loading}
          onClick={onRefresh}
          aria-label={`Refresh ${name}`}
        >
          {loading ? 'Refreshing…' : 'Refresh'}
        </button>
      </div>
      {consent && (
        <a
          href={consent}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex text-sm text-accent underline"
        >
          Continue to {provider === 'google' ? 'Google' : 'Microsoft'}
        </a>
      )}
      {error && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
    </>
  );
}
function MailCard({
  agentId,
  provider,
}: {
  agentId: string;
  provider: MailProvider;
}) {
  const context = useContext(ConnectorContext);
  const name =
    provider === 'email' ? 'Email' : provider === 'gmail' ? 'Gmail' : 'Outlook';
  const [consent, setConsent] = useState<string | null>(null);
  const [to, setTo] = useState(''),
    [subject, setSubject] = useState(''),
    [body, setBody] = useState('');
  const resource = useService(async (explicit, current) => {
    const status = await client.mailStatus(agentId, provider);
    if (!current()) throw new Error('Obsolete connection');
    const id = status.connector?.id;
    const [messages, drafts] = await Promise.allSettled([
      id && status.connector?.status === 'active'
        ? client.mailMessages(agentId, provider, id, { refresh: explicit })
        : Promise.resolve([]),
      id ? client.mailDrafts(agentId, provider, id) : Promise.resolve([]),
    ]);
    return {
      ...status,
      messages: messages.status === 'fulfilled' ? messages.value : [],
      drafts: drafts.status === 'fulfilled' ? drafts.value : [],
      inboxError: messages.status === 'rejected',
      draftsError: drafts.status === 'rejected',
    };
  });
  const connection = resource.data?.connector;
  useResetOnChange([connection?.status], () => {
    if (connection?.status === 'active') setConsent(null);
  });
  useResetOnChange([context.online, context.epoch, agentId], () => {
    setConsent(null);
  });
  const status = !resource.data
    ? resource.error
      ? 'Status unavailable'
      : ''
    : provider !== 'email' && !resource.data.configured
      ? 'Setup required'
      : !connection
        ? 'Not connected'
        : connection.status === 'active'
          ? 'Connected'
          : connection.status === 'reauthRequired'
            ? 'Reconnect required'
            : 'Awaiting authorization';
  return (
    <Card
      title={name}
      description="Read recent mail and prepare messages for your review."
    >
      {provider === 'email' ? (
        <>
          <div className="space-y-1 text-xs">
            <p role="status" className="font-medium text-accent">
              {resource.loading && !status ? 'Loading connection…' : status}
            </p>
            {connection?.accountLabel && (
              <p className="break-all text-ink-2">{connection.accountLabel}</p>
            )}
            {connection?.email?.autoReply.enabled && (
              <p className="text-ink-3">
                Automatic replies enabled for{' '}
                {connection.email.autoReply.allowedSenders.length} allowed
                sender(s).
              </p>
            )}
          </div>
          <GenericEmailSettings
            agentId={agentId}
            online={context.online}
            epoch={context.epoch}
            connected={!!connection}
            settings={connection?.email}
            busy={resource.busy || resource.loading}
            onConnect={(input) => {
              void resource.run(() =>
                client.connectGenericEmail(agentId, input),
              );
            }}
          />
          <div className="flex flex-wrap gap-2">
            {connection && (
              <button
                className={button}
                disabled={resource.busy}
                onClick={() => {
                  void resource.run(() =>
                    client.disconnectMail(agentId, provider, connection.id),
                  );
                }}
              >
                Disconnect Email
              </button>
            )}
            <button
              className={button}
              disabled={resource.busy || resource.loading}
              aria-label="Refresh Email"
              onClick={() => void resource.refresh(true, true)}
            >
              {resource.loading ? 'Refreshing…' : 'Refresh'}
            </button>
          </div>
          {resource.error && (
            <p role="alert" className="text-xs text-danger">
              {resource.error}
            </p>
          )}
        </>
      ) : (
        <ConnectionControls
          name={name}
          configured={resource.data?.configured ?? false}
          connected={!!connection}
          status={status}
          account={connection?.accountLabel}
          {...resource}
          consent={consent}
          provider={provider === 'gmail' ? 'google' : 'outlook'}
          onConnect={() => {
            setConsent(null);
            void resource.run(async () => {
              const result = await client.connectMail(agentId, provider);
              const url = authorizationUrl(
                result.consentUrl,
                provider === 'gmail' ? 'google' : 'outlook',
              );
              return url;
            }, setConsent);
          }}
          onDisconnect={() => {
            setConsent(null);
            void resource.run(() =>
              client.disconnectMail(agentId, provider, connection!.id),
            );
          }}
          onRefresh={() => void resource.refresh(true, true)}
        />
      )}
      {connection?.error && (
        <p role="alert" className="text-xs text-danger">
          {connection.error}
        </p>
      )}
      <p className="text-xs leading-relaxed text-ink-3">
        {provider === 'email'
          ? 'Manual drafts stay in animaOS until you approve Send. Automatic replies require the separate opt-in above.'
          : 'Drafts stay in animaOS until you approve Send. Connecting an account never sends mail.'}
      </p>
      {connection && (
        <>
          <div className="space-y-3 border-t border-line pt-4">
            <h3 className="text-sm font-medium text-ink">Recent inbox</h3>
            {resource.data?.inboxError && (
              <p role="alert" className="text-xs text-danger">
                Could not load recent inbox. Refresh to try again.
              </p>
            )}
            {!resource.data?.inboxError &&
              resource.data?.messages.length === 0 && (
                <p className="text-xs text-ink-3">
                  {connection.status === 'active'
                    ? 'No recent messages.'
                    : 'Reconnect to read recent messages.'}
                </p>
              )}
            {resource.data?.messages.map((message) => (
              <article
                key={message.id}
                className="min-w-0 rounded-xl border border-line p-3 text-xs"
              >
                <p className="break-words font-medium text-ink">
                  {message.subject || '(No subject)'}
                </p>
                <p className="mt-1 break-all text-ink-3">
                  {message.from} · {message.receivedAt}
                </p>
                <p className="mt-2 whitespace-pre-wrap break-words text-ink-2">
                  {message.preview}
                </p>
              </article>
            ))}
          </div>
          <form
            className="space-y-3 border-t border-line pt-4"
            onSubmit={(event) => {
              event.preventDefault();
              void resource
                .run(() =>
                  client.createMailDraft(agentId, provider, connection.id, {
                    to: to
                      .split(',')
                      .map((value) => value.trim())
                      .filter(Boolean),
                    subject,
                    body,
                  }),
                )
                .then((ok) => {
                  if (ok) {
                    setTo('');
                    setSubject('');
                    setBody('');
                  }
                });
            }}
          >
            <h3 className="text-sm font-medium text-ink">New local draft</h3>
            <label className="block text-xs text-ink-2">
              To (comma-separated)
              <input
                className="field mt-1 w-full"
                aria-label={`${name} draft recipients`}
                value={to}
                onChange={(e) => setTo(e.target.value)}
                required
                disabled={resource.busy}
              />
            </label>
            <label className="block text-xs text-ink-2">
              Subject
              <input
                className="field mt-1 w-full"
                aria-label={`${name} draft subject`}
                value={subject}
                onChange={(e) => setSubject(e.target.value)}
                maxLength={998}
                disabled={resource.busy}
              />
            </label>
            <label className="block text-xs text-ink-2">
              Message
              <textarea
                className="field mt-1 min-h-28 w-full resize-y"
                aria-label={`${name} draft message`}
                value={body}
                onChange={(e) => setBody(e.target.value)}
                required
                maxLength={100000}
                disabled={resource.busy}
              />
            </label>
            <button
              className={button}
              type="submit"
              disabled={resource.busy || !to.trim() || !body.trim()}
            >
              Save local draft
            </button>
          </form>
          <div className="space-y-3 border-t border-line pt-4">
            <h3 className="text-sm font-medium text-ink">Saved drafts</h3>
            {resource.data?.draftsError && (
              <p role="alert" className="text-xs text-danger">
                Could not load saved drafts. Refresh to try again.
              </p>
            )}
            {!resource.data?.draftsError && !resource.data?.drafts.length && (
              <p className="text-xs text-ink-3">No saved drafts yet.</p>
            )}
            {resource.data?.drafts.map((draft) => (
              <DraftReview
                key={draft.id}
                draft={draft}
                busy={resource.busy}
                canSend={connection.status === 'active'}
                approve={() =>
                  void resource.run(() =>
                    client.approveMailDraft(
                      agentId,
                      provider,
                      connection.id,
                      draft.id,
                    ),
                  )
                }
                reject={() =>
                  void resource.run(() =>
                    client.rejectMailDraft(
                      agentId,
                      provider,
                      connection.id,
                      draft.id,
                    ),
                  )
                }
              />
            ))}
          </div>
        </>
      )}
    </Card>
  );
}
function DraftReview({
  draft,
  busy,
  canSend,
  approve,
  reject,
}: {
  draft: MailDraft;
  busy: boolean;
  canSend: boolean;
  approve: () => void;
  reject: () => void;
}) {
  return (
    <article className="min-w-0 space-y-2 rounded-xl border border-line p-3 text-xs">
      <p className="break-words font-medium text-ink">
        {draft.subject || '(No subject)'}
      </p>
      <p className="break-all text-ink-3">To: {draft.to.join(', ')}</p>
      <p className="max-h-64 overflow-y-auto whitespace-pre-wrap break-words text-ink-2">
        {draft.body}
      </p>
      <p className="capitalize text-ink-3">{draft.state}</p>
      {draft.error && (
        <p role="alert" className="text-danger">
          {draft.error}
        </p>
      )}
      {draft.state === 'unknown' && (
        <p className="text-danger">
          Delivery is uncertain. Check your sent mail before creating another
          draft.
        </p>
      )}
      {draft.state === 'pending' && (
        <div className="flex flex-wrap gap-2">
          <button
            className={primaryBtnCls}
            disabled={busy || !canSend}
            onClick={approve}
          >
            Send saved draft
          </button>
          <button className={button} disabled={busy} onClick={reject}>
            Reject draft
          </button>
        </div>
      )}
    </article>
  );
}

function OAuthAppCard({
  provider,
  onChanged,
}: {
  provider: OAuthAppProvider;
  onChanged: () => void;
}) {
  const context = useContext(ConnectorContext);
  const name = provider === 'google' ? 'Google' : 'Microsoft';
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [tenant, setTenant] = useState('common');
  const [validationError, setValidationError] = useState<string | null>(null);
  const resource = useService(() => client.oauthAppStatus(provider));
  const status = resource.data;
  const environmentManaged = status?.source === 'environment';
  useResetOnChange([context.owner, context.online, context.epoch], () => {
    setClientSecret('');
    setValidationError(null);
  });

  useResetOnChange([provider, status?.tenant], () => {
    if (provider === 'microsoft' && status?.tenant) setTenant(status.tenant);
  });

  const submit = () => {
    setValidationError(null);
    const submittedClientId = clientId.trim();
    const submittedClientSecret = clientSecret.trim();
    const submittedTenant = tenant.trim();
    if (!submittedClientId || !submittedClientSecret) {
      setValidationError('Client ID and client secret are required.');
      return;
    }
    setClientSecret('');
    void resource
      .run(() =>
        client.configureOauthApp(provider, {
          clientId: submittedClientId,
          clientSecret: submittedClientSecret,
          ...(provider === 'microsoft' && submittedTenant
            ? { tenant: submittedTenant }
            : {}),
        }),
      )
      .then((ok) => {
        if (!ok) return;
        setClientId('');
        setClientSecret('');
        onChanged();
      });
  };

  return (
    <section
      aria-label={`${name} OAuth app`}
      className="min-w-0 space-y-3 rounded-xl border border-line p-4"
    >
      <div>
        <h3 className="text-sm font-semibold text-ink">{name} OAuth app</h3>
        <p className="mt-1 text-xs leading-relaxed text-ink-3">
          {provider === 'google'
            ? 'One OAuth app enables Gmail, Google Calendar, and selected Google Drive files.'
            : 'Enables Outlook.'}
        </p>
      </div>
      <p role="status" className="text-xs font-medium text-accent">
        {resource.loading && !status
          ? 'Loading setup…'
          : status?.configured
            ? 'Configured'
            : resource.error
              ? 'Status unavailable'
              : 'Setup required'}
      </p>
      {status?.source && (
        <p className="text-xs text-ink-3">
          Source: {status.source === 'vault' ? 'Daemon vault' : 'Environment'}
        </p>
      )}
      {status?.clientIdHint && (
        <p className="break-all text-xs text-ink-2">{status.clientIdHint}</p>
      )}
      <div className="space-y-1 text-xs text-ink-2">
        <p>Redirect {status?.redirectUris.length === 1 ? 'URI' : 'URIs'}</p>
        {status?.redirectUris.map((uri) => (
          <code
            key={uri}
            className="block overflow-x-auto rounded-lg bg-black/20 px-2 py-1.5 text-ink-3"
          >
            {uri}
          </code>
        ))}
      </div>
      {environmentManaged && (
        <div className="space-y-1 text-xs leading-relaxed text-ink-3">
          <p>Managed through the daemon environment.</p>
          <p>These credentials cannot be replaced or removed in the UI.</p>
        </div>
      )}
      <form
        className="grid min-w-0 gap-3 sm:grid-cols-2"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className="block min-w-0 text-xs text-ink-2">
          Client ID
          <input
            className="field mt-1 w-full"
            aria-label={`${name} client ID`}
            value={clientId}
            onChange={(event) => setClientId(event.target.value)}
            disabled={resource.busy || environmentManaged}
          />
        </label>
        <label className="block min-w-0 text-xs text-ink-2">
          Client secret
          <input
            className="field mt-1 w-full"
            aria-label={`${name} client secret`}
            type="password"
            autoComplete="new-password"
            value={clientSecret}
            onChange={(event) => setClientSecret(event.target.value)}
            disabled={resource.busy || environmentManaged}
          />
        </label>
        {provider === 'microsoft' && (
          <label className="block min-w-0 text-xs text-ink-2 sm:col-span-2">
            Tenant
            <input
              className="field mt-1 w-full"
              aria-label="Microsoft tenant"
              value={tenant}
              onChange={(event) => setTenant(event.target.value)}
              disabled={resource.busy || environmentManaged}
            />
          </label>
        )}
        <div className="flex flex-wrap gap-2 sm:col-span-2">
          <button
            className={primaryBtnCls}
            type="submit"
            disabled={resource.busy || resource.loading || environmentManaged}
          >
            Save {name} OAuth app
          </button>
          {status?.source === 'vault' && (
            <button
              className={button}
              type="button"
              disabled={resource.busy}
              onClick={() => {
                setValidationError(null);
                setClientSecret('');
                void resource
                  .run(() => client.removeOauthApp(provider))
                  .then((ok) => {
                    if (ok) onChanged();
                  });
              }}
            >
              Remove {name} OAuth app
            </button>
          )}
        </div>
      </form>
      {(validationError || resource.error) && (
        <p role="alert" className="text-xs text-danger">
          {validationError || resource.error}
        </p>
      )}
    </section>
  );
}

function CalendarCard({ agentId }: { agentId: string }) {
  const context = useContext(ConnectorContext);
  const [consent, setConsent] = useState<string | null>(null);
  const resource = useService(async (_explicit, current) => {
    const status = await client.calendarStatus(agentId);
    if (!current()) throw new Error('Obsolete connection');
    const [writes] = await Promise.allSettled([
      status.connector
        ? client.calendarWrites(agentId, status.connector.id)
        : Promise.resolve([]),
    ]);
    return {
      ...status,
      writes: writes.status === 'fulfilled' ? writes.value : [],
      writesError: writes.status === 'rejected',
    };
  });
  const connection = resource.data?.connector;
  const status = !resource.data
    ? resource.error
      ? 'Status unavailable'
      : ''
    : !resource.data.configured
      ? 'Setup required'
      : !connection
        ? 'Not connected'
        : connection.status === 'active'
          ? 'Connected'
          : connection.status === 'reauthRequired'
            ? 'Reconnect required'
            : 'Awaiting authorization';
  useResetOnChange([connection?.status], () => {
    if (connection?.status === 'active') setConsent(null);
  });
  useResetOnChange([context.online, context.epoch, agentId], () => {
    setConsent(null);
  });
  return (
    <Card
      title="Google Calendar"
      description="See your schedule. Review calendar changes before they are applied."
    >
      <ConnectionControls
        name="Google Calendar"
        configured={resource.data?.configured ?? false}
        connected={!!connection}
        status={status}
        account={connection?.accountLabel}
        {...resource}
        consent={consent}
        provider="google"
        onConnect={() => {
          setConsent(null);
          void resource.run(async () => {
            const result = await client.connectCalendar(agentId);
            return authorizationUrl(result.consentUrl, 'google');
          }, setConsent);
        }}
        onDisconnect={() => {
          setConsent(null);
          void resource.run(() =>
            client.disconnectCalendar(agentId, connection!.id),
          );
        }}
        onRefresh={() => void resource.refresh()}
      />
      {connection && (
        <div className="space-y-3 border-t border-line pt-4">
          <h3 className="text-sm font-medium text-ink">Calendar approvals</h3>
          {resource.data?.writesError && (
            <p role="alert" className="text-xs text-danger">
              Could not load calendar approvals. Refresh to try again.
            </p>
          )}
          {!resource.data?.writesError && !resource.data?.writes.length && (
            <p className="text-xs text-ink-3">
              No calendar changes awaiting review.
            </p>
          )}
          {resource.data?.writes.map((write) => (
            <article
              key={write.id}
              className="space-y-2 rounded-xl border border-line p-3 text-xs"
            >
              <p className="break-words font-medium text-ink">
                {write.summary}
              </p>
              <p className="text-ink-3">
                {write.operation} · {write.state}
              </p>
              <p className="break-words text-ink-2">{write.draft.title}</p>
              <p className="break-words text-ink-3">
                {write.draft.start} → {write.draft.end}
              </p>
              <p className="break-all text-ink-3">
                Calendar: {write.draft.calendarId}
              </p>
              {write.draft.location && (
                <p className="break-words text-ink-2">{write.draft.location}</p>
              )}
              {write.draft.description && (
                <p className="whitespace-pre-wrap break-words text-ink-2">
                  {write.draft.description}
                </p>
              )}
              {write.error && (
                <p role="alert" className="text-danger">
                  {write.error}
                </p>
              )}
              {write.state === 'pending' && (
                <div className="flex flex-wrap gap-2">
                  <button
                    className={primaryBtnCls}
                    disabled={resource.busy || connection.status !== 'active'}
                    onClick={() =>
                      void resource.run(() =>
                        client.approveCalendarWrite(
                          agentId,
                          connection.id,
                          write.id,
                        ),
                      )
                    }
                  >
                    Approve calendar change
                  </button>
                  <button
                    className={button}
                    disabled={resource.busy}
                    onClick={() =>
                      void resource.run(() =>
                        client.rejectCalendarWrite(
                          agentId,
                          connection.id,
                          write.id,
                        ),
                      )
                    }
                  >
                    Reject change
                  </button>
                </div>
              )}
            </article>
          ))}
        </div>
      )}
    </Card>
  );
}
export function ConnectorsView({
  agentId,
  agentName = 'anima',
  mode = 'all',
  telegram,
  discord,
  disabled = false,
  online = true,
  epoch = 0,
  onMcpGrantsChanged,
  onMcpBusyChanged,
  onMcpPermissionsUncertain,
  mcpPermissionsUncertain = false,
  mcpPermissionsBusy = false,
}: {
  agentId: string;
  agentName?: string;
  mode?: 'all' | 'connections' | 'tools';
  telegram: ReactNode;
  discord?: ReactNode;
  disabled?: boolean;
  online?: boolean;
  epoch?: number;
  onMcpGrantsChanged?: (toolNames: string[]) => void | Promise<void>;
  onMcpBusyChanged?: (busy: boolean) => void;
  onMcpPermissionsUncertain?: () => void;
  mcpPermissionsUncertain?: boolean;
  mcpPermissionsBusy?: boolean;
}) {
  const lifetime = useConnectorLifetime(agentId, online, epoch);
  const [ownerBusy, setOwnerBusy] = useState(false);
  const [telegramDetail, setTelegramDetail] = useState(false);
  const busyClaims = useRef(0);
  const [mcpRefreshRequired, setMcpRefreshRequired] = useState(false);
  useResetOnChange([lifetime.token], () => {
    setOwnerBusy(false);
    setMcpRefreshRequired(false);
    setTelegramDetail(false);
  });
  useLayoutEffect(() => {
    busyClaims.current = 0;
  }, [lifetime.token]);
  const reportOwnerBusy = (busy: boolean) => {
    if (!lifetime.matches(lifetime.token)) return;
    busyClaims.current = Math.max(0, busyClaims.current + (busy ? 1 : -1));
    const active = busyClaims.current > 0;
    if (lifetime.isCurrent(lifetime.token)) setOwnerBusy(active);
    onMcpBusyChanged?.(active);
  };
  const [oauthGeneration, setOauthGeneration] = useState({
    google: 0,
    microsoft: 0,
  });
  const oauthChanged = (provider: OAuthAppProvider) =>
    setOauthGeneration((current) => ({
      ...current,
      [provider]: current[provider] + 1,
    }));

  return (
    <ConnectorContext.Provider
      value={{ owner: agentId, online, epoch, disabled }}
    >
      <section aria-label="Connectors" className="connections-page">
        <div className="connections-stack">
          <h1 className="sr-only">Connectors</h1>
          {!online && (
            <p role="status" className="text-xs text-ink-3">
              Offline — reconnect to review current connection status.
            </p>
          )}
          {mode === 'connections' && telegramDetail ? (
            <section className="connections-telegram-detail" aria-labelledby="telegram-settings-title">
              <button type="button" className="connections-back" onClick={() => setTelegramDetail(false)}>
                <span aria-hidden="true">←</span> Connections
              </button>
              <header>
                <h2 id="telegram-settings-title">Telegram</h2>
                <p>
                  Talk to {agentName} from Telegram. Your private chat joins the
                  main chat; groups get their own thread.
                </p>
              </header>
              <div className="connections-card">{telegram}</div>
            </section>
          ) : (
          <>
          {(mode === 'all' || mode === 'tools') && (
            <section className="connections-section" aria-labelledby="mcp-tools-heading">
              <div className="connections-section-heading">
                <div>
                  <h2 id="mcp-tools-heading">From MCP servers</h2>
                  <p>Tools from servers you connect, with their current permission state.</p>
                </div>
              </div>
              <McpConnectionsPanel
                key={`${agentId}:mcp`}
                agentId={agentId}
                online={online}
                epoch={epoch}
                disabled={disabled || ownerBusy || mcpRefreshRequired}
                permissionsUncertain={mcpPermissionsUncertain}
                permissionsBusy={mcpPermissionsBusy || ownerBusy}
                onGrantsChanged={async (names) => {
                  await onMcpGrantsChanged?.(names);
                  if (lifetime.isCurrent(lifetime.token))
                    setMcpRefreshRequired(false);
                }}
                onBusyChanged={reportOwnerBusy}
                onPermissionsUncertain={onMcpPermissionsUncertain}
              />
              {mcpRefreshRequired && (
                <p className="text-xs text-ink-3">
                  A bundle changed its saved components. Use Refresh status in
                  External tools before editing those connections; your entered
                  fields are kept.
                </p>
              )}
              <CommunityBundlesPanel
                key={`${agentId}:bundles`}
                agentId={agentId}
                online={online}
                epoch={epoch}
                disabled={disabled || ownerBusy}
                permissionsUncertain={mcpPermissionsUncertain}
                permissionsBusy={mcpPermissionsBusy || ownerBusy}
                onGrantsChanged={async (names) => {
                  await onMcpGrantsChanged?.(names);
                  if (lifetime.isCurrent(lifetime.token))
                    setMcpRefreshRequired(true);
                }}
                onBusyChanged={reportOwnerBusy}
                onPermissionsUncertain={onMcpPermissionsUncertain}
              />
            </section>
          )}
          {(mode === 'all' || mode === 'connections') && (
            <>
              <details className="connections-oauth-setup">
                <summary>OAuth app setup</summary>
                <p>
                  Add the daemon credentials used to start Google and Microsoft
                  account connections.
                </p>
                <div className="grid min-w-0 items-start gap-4 lg:grid-cols-2">
                  <OAuthAppCard
                    key={`${agentId}:google`}
                    provider="google"
                    onChanged={() => oauthChanged('google')}
                  />
                  <OAuthAppCard
                    key={`${agentId}:microsoft`}
                    provider="microsoft"
                    onChanged={() => oauthChanged('microsoft')}
                  />
                </div>
              </details>
              <section className="connections-section" aria-labelledby="service-connections-heading">
                <div className="connections-section-heading">
                  <div>
                    <h2 id="service-connections-heading">Services</h2>
                    <p>Accounts and services this agent can work with.</p>
                  </div>
                </div>
                <div className="connections-service-list">
                  <MailCard
                    key={`${agentId}:gmail:${oauthGeneration.google}`}
                    agentId={agentId}
                    provider="gmail"
                  />
                  <MailCard
                    key={`${agentId}:email`}
                    agentId={agentId}
                    provider="email"
                  />
                  <CalendarCard
                    key={`${agentId}:calendar:${oauthGeneration.google}`}
                    agentId={agentId}
                  />
                  <GoogleDriveCard
                    key={`${agentId}:drive:${oauthGeneration.google}`}
                    agentId={agentId}
                    online={online}
                    epoch={epoch}
                    disabled={disabled}
                  />
                  <MailCard
                    key={`${agentId}:outlook:${oauthGeneration.microsoft}`}
                    agentId={agentId}
                    provider="outlook"
                  />
                </div>
              </section>
              <ConnectedAccountsPanel
                agentId={agentId}
                online={online}
                epoch={epoch}
                disabled={disabled || ownerBusy}
                onGrantsChanged={onMcpGrantsChanged}
              />
              <section className="connections-section" aria-labelledby="chat-app-connections-heading">
                <div className="connections-section-heading">
                  <div>
                    <h2 id="chat-app-connections-heading">Chat apps</h2>
                    <p>
                      Talk to {agentName} from apps you already use. Same memory
                      everywhere.
                    </p>
                  </div>
                </div>
                <div className="connections-chat-list">
                  <div className="connections-app-row">
                    <div>
                      <strong>Telegram</strong>
                      <p>Private chats join your main chat; groups get their own thread.</p>
                    </div>
                    <button type="button" onClick={() => setTelegramDetail(true)}>
                      Manage
                    </button>
                  </div>
                  {discord && (
                    <div className="min-w-0 connections-card">{discord}</div>
                  )}
                  {(['WhatsApp', 'Slack'] as const).map((app) => (
                    <div key={app} className="connections-app-row is-unavailable">
                      <div>
                        <strong>{app}</strong>
                        <p>Not connected · setup is not available yet</p>
                      </div>
                      <button type="button" disabled aria-label={`Connect ${app} (not available yet)`}>
                        Connect
                      </button>
                    </div>
                  ))}
                </div>
              </section>
            </>
          )}
          </>
          )}
        </div>
      </section>
    </ConnectorContext.Provider>
  );
}
