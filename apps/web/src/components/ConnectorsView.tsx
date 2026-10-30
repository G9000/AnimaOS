import { ownerFetch } from '../lib/owner-fetch';
import {
  createContext,
  useContext,
  useEffect,
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
import { ServiceRow } from './ServiceRow';
import {
  SERVICE_GROUPS,
  ServiceDirectoryContext,
  isListed,
  isSettled,
  useServiceDirectory,
  type ServiceGroup,
} from './ServiceDirectory';
import { BrandIcon } from './BrandIcon';
import { OAuthGuide } from './OAuthGuide';
import {
  OAuthSetupContext,
  OAuthSetupLink,
  type OAuthProvider,
} from './OAuthSetupLink';
import {
  AtSignIcon,
  CheckIcon,
  CopyIcon,
  PlusIcon,
  RefreshIcon,
  SearchIcon,
  XIcon,
} from './icons';
import { GoogleDriveCard } from './GoogleDriveCard';
import { MicrosoftFilesCard } from './MicrosoftFilesCard';
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
  id,
  group,
  title,
  description,
  status,
  icon,
  children,
}: {
  id: string;
  group: ServiceGroup;
  title: string;
  description: string;
  status: string;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <ServiceRow
      id={id}
      group={group}
      title={title}
      summary={description}
      status={status}
      icon={icon}
    >
      {children}
    </ServiceRow>
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
  const provName = provider === 'google' ? 'Google' : 'Microsoft';
  const expired = /reconnect/i.test(status);
  const unfinished = /awaiting/i.test(status);
  const needsAction = !connected || expired || unfinished;
  return (
    <div className="conn-panel">
      {account && (
        <p className="conn-account">
          <span>Account</span>
          <strong>{account}</strong>
        </p>
      )}
      {status === 'Setup required' && !loading && (
        <OAuthSetupLink
          provider={provider === 'google' ? 'google' : 'microsoft'}
        />
      )}
      {consent ? (
        <div className="conn-next">
          <p>One more step: finish signing in with {provName}.</p>
          <a
            href={consent}
            target="_blank"
            rel="noopener noreferrer"
            className="conn-btn conn-btn--primary"
          >
            Continue to {provName}
          </a>
        </div>
      ) : (expired || unfinished) && !loading ? (
        <p className="conn-note">
          {expired
            ? `${provName} needs you to sign in again.`
            : `Signing in with ${provName} was not finished. Reconnect to start again.`}
        </p>
      ) : null}
      <div className="conn-actions">
        <button
          type="button"
          className={`conn-btn${needsAction && !consent ? ' conn-btn--primary' : ''}`}
          disabled={busy || loading || !configured}
          onClick={onConnect}
        >
          {connected ? `Reconnect ${name}` : `Connect ${name}`}
        </button>
        {connected && (
          <button
            type="button"
            className="conn-btn"
            disabled={busy}
            onClick={onDisconnect}
          >
            Disconnect {name}
          </button>
        )}
        <button
          type="button"
          className="conn-btn conn-btn--icon"
          disabled={busy || loading}
          data-loading={loading || undefined}
          onClick={onRefresh}
          aria-label={`Refresh ${name}`}
          title="Refresh"
        >
          <RefreshIcon size={15} />
        </button>
      </div>
      {error && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
    </div>
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
      id={provider}
      group={
        provider === 'gmail'
          ? 'google'
          : provider === 'outlook'
            ? 'microsoft'
            : 'email'
      }
      title={name}
      description={
        provider === 'email' ? 'Any mailbox (IMAP/SMTP)' : 'Read mail, draft replies'
      }
      icon={
        provider === 'email' ? (
          <AtSignIcon size={18} />
        ) : (
          <BrandIcon brand={provider} />
        )
      }
      status={resource.loading && !status ? 'Loading connection…' : status}
    >
      {provider === 'email' ? (
        <>
          {connection?.accountLabel && (
            <p className="conn-account">
              <span>Account</span>
              <strong>{connection.accountLabel}</strong>
            </p>
          )}
          {connection?.email?.autoReply.enabled && (
            <p className="conn-note">
              Automatic replies enabled for{' '}
              {connection.email.autoReply.allowedSenders.length} allowed
              sender(s).
            </p>
          )}
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
          <div className="conn-actions">
            {connection && (
              <button
                type="button"
                className="conn-btn"
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
              type="button"
              className="conn-btn conn-btn--icon"
              disabled={resource.busy || resource.loading}
              data-loading={resource.loading || undefined}
              aria-label="Refresh Email"
              title="Refresh"
              onClick={() => void resource.refresh(true, true)}
            >
              <RefreshIcon size={15} />
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
      <p className="conn-foot">
        {provider === 'email'
          ? 'Drafts wait for your approval before sending.'
          : 'Nothing is sent until you approve it.'}
      </p>
      {connection && (
        <>
          {connection.status === 'active' && (
            <section className="conn-section">
              <h3 className="conn-title">Recent inbox</h3>
              {resource.data?.inboxError && (
                <p role="alert" className="text-xs text-danger">
                  Could not load recent inbox. Refresh to try again.
                </p>
              )}
              {!resource.data?.inboxError &&
                resource.data?.messages.length === 0 && (
                  <p className="conn-empty">No recent messages.</p>
                )}
              <div className="conn-messages">
                {resource.data?.messages.map((message) => (
                  <article key={message.id} className="conn-message">
                    <p className="conn-message-subject">
                      {message.subject || '(No subject)'}
                    </p>
                    <p className="conn-message-meta">
                      {message.from} · {message.receivedAt}
                    </p>
                    <p className="conn-message-preview">{message.preview}</p>
                  </article>
                ))}
              </div>
            </section>
          )}
          {(connection.status === 'active' ||
            Boolean(resource.data?.drafts.length) ||
            resource.data?.draftsError) && (
            <section className="conn-section">
              {connection.status === 'active' && (
                <details className="conn-compose">
                  <summary>
                    <PlusIcon size={14} />
                    New local draft
                  </summary>
                  <form
                    onSubmit={(event) => {
                      event.preventDefault();
                      void resource
                        .run(() =>
                          client.createMailDraft(
                            agentId,
                            provider,
                            connection.id,
                            {
                              to: to
                                .split(',')
                                .map((value) => value.trim())
                                .filter(Boolean),
                              subject,
                              body,
                            },
                          ),
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
                    <label className="conn-field">
                      To (comma-separated)
                      <input
                        className="field"
                        aria-label={`${name} draft recipients`}
                        value={to}
                        onChange={(e) => setTo(e.target.value)}
                        required
                        disabled={resource.busy}
                      />
                    </label>
                    <label className="conn-field">
                      Subject
                      <input
                        className="field"
                        aria-label={`${name} draft subject`}
                        value={subject}
                        onChange={(e) => setSubject(e.target.value)}
                        maxLength={998}
                        disabled={resource.busy}
                      />
                    </label>
                    <label className="conn-field">
                      Message
                      <textarea
                        className="field min-h-28 resize-y"
                        aria-label={`${name} draft message`}
                        value={body}
                        onChange={(e) => setBody(e.target.value)}
                        required
                        maxLength={100000}
                        disabled={resource.busy}
                      />
                    </label>
                    <div className="conn-actions">
                      <button
                        className="conn-btn conn-btn--primary"
                        type="submit"
                        disabled={resource.busy || !to.trim() || !body.trim()}
                      >
                        Save local draft
                      </button>
                    </div>
                  </form>
                </details>
              )}
              <h3 className="conn-title">Saved drafts</h3>
              {resource.data?.draftsError && (
                <p role="alert" className="text-xs text-danger">
                  Could not load saved drafts. Refresh to try again.
                </p>
              )}
              {!resource.data?.draftsError && !resource.data?.drafts.length && (
                <p className="conn-empty">No saved drafts yet.</p>
              )}
              <div className="flex flex-col gap-2">
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
            </section>
          )}
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
    <article className="conn-draft">
      <p className="break-words text-[13px] font-medium text-ink">
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
            type="button"
            className="conn-btn conn-btn--primary"
            disabled={busy || !canSend}
            onClick={approve}
          >
            Send saved draft
          </button>
          <button
            type="button"
            className="conn-btn"
            disabled={busy}
            onClick={reject}
          >
            Reject draft
          </button>
        </div>
      )}
    </article>
  );
}

/** The addresses the daemon registers at its default origin. Shown only when
 *  the daemon cannot answer, so the owner is never left without them. */
const DEFAULT_REDIRECT_URIS: Record<OAuthAppProvider, readonly string[]> = {
  google: [
    'http://127.0.0.1:8080/api/connectors/mail/gmail/callback',
    'http://127.0.0.1:8080/api/connectors/gcalendar/callback',
    'http://127.0.0.1:8080/api/connectors/gdrive/callback',
  ],
  microsoft: [
    'http://127.0.0.1:8080/api/connectors/mail/outlook/callback',
    'http://127.0.0.1:8080/api/connectors/microsoft-files/callback',
  ],
};

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

  const assumed = !status && Boolean(resource.error);
  const uris = status?.redirectUris ?? (assumed ? DEFAULT_REDIRECT_URIS[provider] : []);
  const statusText =
    resource.loading && !status
      ? 'Loading setup…'
      : status?.configured
        ? 'Configured'
        : resource.error
          ? 'Status unavailable'
          : 'Not set up';
  return (
    <section aria-label={`${name} OAuth app`} className="oauth-card">
      <header className="oauth-head">
        <div>
          <h3>{name} OAuth app</h3>
          <p>
            {provider === 'google'
              ? 'One OAuth app enables Gmail, Google Calendar, and selected Google Drive files.'
              : 'Enables Outlook and Microsoft files.'}
          </p>
        </div>
        <span
          role="status"
          className="connections-row-status"
          data-tone={
            status?.configured
              ? 'ok'
              : resource.error
                ? 'attention'
                : 'idle'
          }
        >
          {statusText}
        </span>
      </header>
      {(status?.source || status?.clientIdHint) && (
        <dl className="oauth-facts">
          {status.source && (
            <div>
              <dt>Stored in</dt>
              <dd>{status.source === 'vault' ? 'Daemon vault' : 'Environment'}</dd>
            </div>
          )}
          {status.clientIdHint && (
            <div>
              <dt>Client ID</dt>
              <dd>
                <code>{status.clientIdHint}</code>
              </dd>
            </div>
          )}
        </dl>
      )}
      <OAuthGuide
        provider={provider}
        configured={status ? status.configured : null}
      />
      {uris.length > 0 && (
        <div className="oauth-uris">
          <p className="oauth-label">
            Redirect {uris.length === 1 ? 'URI' : 'URIs'}
          </p>
          {uris.map((uri) => (
            <div className="oauth-uri" key={uri}>
              <code>{uri}</code>
              <CopyButton text={uri} />
            </div>
          ))}
          {assumed && (
            <p className="oauth-hint">
              These are the default addresses. The daemon could not confirm
              them, so a custom public address would differ.{' '}
              <button
                type="button"
                className="oauth-inline"
                onClick={() => void resource.refresh(true, true)}
              >
                Try again
              </button>
            </p>
          )}
        </div>
      )}
      {environmentManaged && (
        <div className="oauth-note">
          <p>Managed through the daemon environment.</p>
          <p>These credentials cannot be replaced or removed in the UI.</p>
        </div>
      )}
      <form
        className="oauth-form"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className="oauth-field">
          Client ID
          <input
            className="field"
            aria-label={`${name} client ID`}
            value={clientId}
            onChange={(event) => setClientId(event.target.value)}
            disabled={resource.busy || environmentManaged}
          />
        </label>
        <label className="oauth-field">
          Client secret
          <input
            className="field"
            aria-label={`${name} client secret`}
            type="password"
            autoComplete="new-password"
            value={clientSecret}
            onChange={(event) => setClientSecret(event.target.value)}
            disabled={resource.busy || environmentManaged}
          />
        </label>
        {provider === 'microsoft' && (
          <label className="oauth-field oauth-wide">
            Tenant
            <input
              className="field"
              aria-label="Microsoft tenant"
              value={tenant}
              onChange={(event) => setTenant(event.target.value)}
              disabled={resource.busy || environmentManaged}
            />
            <span className="oauth-hint">
              “common” works for work and personal accounts.
            </span>
          </label>
        )}
        <div className="oauth-actions oauth-wide">
          <button
            className="oauth-save"
            type="submit"
            disabled={resource.busy || resource.loading || environmentManaged}
          >
            Save {name} OAuth app
          </button>
          {status?.source === 'vault' && (
            <button
              className="oauth-remove"
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

/** Copies a value and says so for a moment. */
function CopyButton({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return (
    <button
      type="button"
      className="oauth-copy"
      aria-label={done ? 'Copied' : `Copy ${text}`}
      title={done ? 'Copied' : 'Copy'}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setDone(true);
          window.clearTimeout(timer.current);
          timer.current = window.setTimeout(() => setDone(false), 1500);
        } catch {
          // Without clipboard access the URI is still there to select.
        }
      }}
    >
      {done ? <CheckIcon size={15} /> : <CopyIcon size={15} />}
    </button>
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
      id="calendar"
      group="google"
      title="Google Calendar"
      description="See your schedule, review changes"
      icon={<BrandIcon brand="google-calendar" />}
      status={resource.loading && !status ? 'Loading connection…' : status}
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
function openCatalog(element: HTMLDialogElement | null) {
  if (!element || element.open) return;
  if (typeof element.showModal === 'function') element.showModal();
  else element.setAttribute('open', '');
  element.querySelector<HTMLInputElement>('input[type="search"]')?.focus();
}
function closeCatalog(element: HTMLDialogElement | null) {
  if (!element) return;
  if (typeof element.close === 'function') element.close();
  else element.removeAttribute('open');
}
export function ConnectorsView({
  agentId,
  agentName = 'anima',
  mode = 'all',
  telegram,
  telegramStatus = null,
  discord,
  discordStatus = null,
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
  /** Where Telegram stands, or null when it is not connected. */
  telegramStatus?: string | null;
  discord?: ReactNode;
  /** Where Discord stands, or null when it is not connected. */
  discordStatus?: string | null;
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
  const directory = useServiceDirectory();
  const catalog = useRef<HTMLDialogElement>(null);
  const oauthSetup = useRef<HTMLDetailsElement>(null);
  /** Opens OAuth app setup and puts the cursor in this provider's first field,
   *  once a closing drawer has handed focus back. */
  const openOAuthSetup = (provider: OAuthProvider) => {
    const element = oauthSetup.current;
    if (!element) return;
    element.open = true;
    const name = provider === 'google' ? 'Google' : 'Microsoft';
    requestAnimationFrame(() => {
      element.scrollIntoView?.({ block: 'start', behavior: 'smooth' });
      element
        .querySelector<HTMLInputElement>(`input[aria-label="${name} client ID"]`)
        ?.focus({ preventScroll: true });
    });
  };
  const [query, setQuery] = useState('');
  const settled = directory.entries.length > 0 && directory.entries.every((entry) => isSettled(entry.status));
  const nothingListed = !directory.entries.some((entry) => isListed(entry.status));
  const busyClaims = useRef(0);
  const [mcpRefreshRequired, setMcpRefreshRequired] = useState(false);
  useResetOnChange([lifetime.token], () => {
    setOwnerBusy(false);
    setMcpRefreshRequired(false);
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
      <ServiceDirectoryContext.Provider value={directory}>
      <OAuthSetupContext.Provider value={openOAuthSetup}>
      <section aria-label="Connectors" className="connections-page" data-mode={mode}>
        <div className="connections-stack">
          <h1 className="sr-only">Connectors</h1>
          {!online && (
            <p role="status" className="text-xs text-ink-3">
              Offline — reconnect to review current connection status.
            </p>
          )}
          {(mode === 'all' || mode === 'tools') && (
            <section className="connections-section" aria-label="External tools">
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
              <section className="connections-section" aria-labelledby="service-connections-heading">
                <div className="connections-section-heading connections-section-bar">
                  <div>
                    <h2 id="service-connections-heading">Connections</h2>
                    <p>What {agentName} is connected to.</p>
                  </div>
                  <button
                    type="button"
                    className="connections-add"
                    aria-haspopup="dialog"
                    onClick={() => openCatalog(catalog.current)}
                  >
                    <PlusIcon size={14} />
                    Add connection
                  </button>
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
                  <MicrosoftFilesCard
                    key={`${agentId}:microsoft-files:${oauthGeneration.microsoft}`}
                    agentId={agentId}
                    online={online}
                    epoch={epoch}
                    disabled={disabled || ownerBusy}
                  />
                  <ServiceRow
                    id="telegram"
                    group="chat"
                    title="Telegram"
                    summary="Chat from your phone"
                    status={telegramStatus ?? 'Not connected'}
                    icon={<BrandIcon brand="telegram" />}
                  >
                    <p className="text-xs leading-relaxed text-ink-3">
                      Private chats join your main chat; groups get their own
                      thread.
                    </p>
                    {telegram}
                  </ServiceRow>
                  {discord && (
                    <ServiceRow
                      id="discord"
                      group="chat"
                      title="Discord"
                      summary="Chat from your servers"
                      status={discordStatus ?? 'Not connected'}
                      icon={<BrandIcon brand="discord" />}
                    >
                      {discord}
                    </ServiceRow>
                  )}
                </div>
                {settled && nothingListed && (
                  <p className="connections-empty">
                    Nothing is connected yet. Use Add connection to link an
                    account or a chat app.
                  </p>
                )}
              </section>
              <ConnectedAccountsPanel
                agentId={agentId}
                online={online}
                epoch={epoch}
                disabled={disabled || ownerBusy}
                onGrantsChanged={onMcpGrantsChanged}
              />
              <details
                className="sk-disclosure connections-oauth-setup"
                ref={oauthSetup}
              >
                <summary>OAuth app setup</summary>
                <p>
                  Add the daemon credentials used to start Google and Microsoft
                  account connections.
                </p>
                <div className="oauth-grid">
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
              <dialog
                ref={catalog}
                className="connections-catalog"
                aria-label="Add a connection"
                onClose={() => setQuery('')}
                onClick={(event) => {
                  if (event.target === event.currentTarget)
                    closeCatalog(catalog.current);
                }}
              >
                <header className="connections-catalog-head">
                  <div>
                    <p className="connections-drawer-title">Add a connection</p>
                    <p>Pick what {agentName} can use.</p>
                  </div>
                  <button
                    type="button"
                    className="connections-drawer-close"
                    aria-label="Close the catalogue"
                    onClick={() => closeCatalog(catalog.current)}
                  >
                    <XIcon size={16} />
                  </button>
                </header>
                <label className="connections-catalog-search">
                  <SearchIcon size={14} />
                  <input
                    type="search"
                    aria-label="Search integrations"
                    placeholder="Search integrations"
                    autoComplete="off"
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                  />
                </label>
                <div className="connections-catalog-body">
                  {(() => {
                    const needle = query.trim().toLowerCase();
                    const all = [
                      ...directory.entries.map((entry) => ({
                        ...entry,
                        unavailable: false,
                      })),
                      {
                        id: 'whatsapp',
                        title: 'WhatsApp',
                        summary: 'Not available yet',
                        group: 'chat' as const,
                        status: '',
                        icon: <BrandIcon brand="whatsapp" />,
                        unavailable: true,
                      },
                      {
                        id: 'slack',
                        title: 'Slack',
                        summary: 'Not available yet',
                        group: 'chat' as const,
                        status: '',
                        icon: <BrandIcon brand="slack" />,
                        unavailable: true,
                      },
                    ];
                    const groups = SERVICE_GROUPS.map((group) => ({
                      ...group,
                      items: all.filter(
                        (entry) =>
                          entry.group === group.id &&
                          `${entry.title} ${entry.summary} ${group.label}`
                            .toLowerCase()
                            .includes(needle),
                      ),
                    })).filter((group) => group.items.length > 0);
                    if (groups.length === 0)
                      return (
                        <p className="connections-empty">
                          Nothing matches “{query.trim()}”.
                        </p>
                      );
                    return groups.map((group) => (
                      <section key={group.id} aria-label={group.label}>
                        <h3 className="connections-catalog-group">
                          {group.label}
                        </h3>
                        <ul>
                          {group.items.map((entry) => (
                            <li
                              key={entry.id}
                              className={
                                entry.unavailable ? 'is-unavailable' : undefined
                              }
                            >
                              <span className="connections-row-icon">
                                {entry.icon}
                              </span>
                              <div>
                                <strong>{entry.title}</strong>
                                <p>{entry.summary}</p>
                              </div>
                              {entry.unavailable ? (
                                <button
                                  type="button"
                                  disabled
                                  aria-label={`Add ${entry.title} (not available yet)`}
                                >
                                  Soon
                                </button>
                              ) : (
                                <button
                                  type="button"
                                  aria-label={`${isListed(entry.status) ? 'Manage' : 'Add'} ${entry.title}`}
                                  onClick={() => {
                                    closeCatalog(catalog.current);
                                    directory.ask(entry.id);
                                  }}
                                >
                                  {isListed(entry.status) ? 'Manage' : 'Add'}
                                </button>
                              )}
                            </li>
                          ))}
                        </ul>
                      </section>
                    ));
                  })()}
                </div>
              </dialog>
            </>
          )}
        </div>
      </section>
      </OAuthSetupContext.Provider>
      </ServiceDirectoryContext.Provider>
    </ConnectorContext.Provider>
  );
}
