import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useLatest } from '../../hooks/useLatest';
import type {
  McpConnection,
  McpOAuthStart,
  McpOAuthStatus,
  mcpOwnerApi,
} from '../../lib/daemon-api';

export type McpOAuthApi = Pick<
  typeof mcpOwnerApi,
  'startOAuth' | 'getOAuthStatus' | 'cancelOAuth' | 'signOutOAuth'
>;
const button =
  'rounded-lg border border-line px-3 py-2 text-xs disabled:opacity-50';
const loopback = (host: string) =>
  ['127.0.0.1', '[::1]', 'localhost'].includes(host);

function safeAuthorizationUrl(raw: string, resource: string): boolean {
  try {
    const url = new URL(raw);
    const endpoint = new URL(resource);
    return (
      !url.username &&
      !url.password &&
      (url.protocol === 'https:' ||
        (url.protocol === 'http:' &&
          loopback(url.hostname) &&
          loopback(endpoint.hostname)))
    );
  } catch {
    return false;
  }
}

/** Account credentials and callback codes stay in the daemon. Only owner status is polled here. */
export function McpOAuthAccount({
  connection,
  api,
  lifetime,
  blocked,
  stale,
  runAction,
  onSettled,
}: {
  connection: McpConnection;
  api: McpOAuthApi;
  lifetime: object;
  blocked: boolean;
  stale: boolean;
  runAction: (action: () => Promise<void>) => Promise<void>;
  onSettled: () => void;
}) {
  const [status, setStatus] = useState<McpOAuthStatus | null>(null);
  const [attempt, setAttempt] = useState<McpOAuthStart | null>(null);
  const [clientId, setClientId] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(true);
  const [readEpoch, setReadEpoch] = useState(0);
  const current = useRef<object | null>(null);
  const settled = useLatest(onSettled);
  useResetOnChange(
    [connection.id, connection.endpoint, connection.version, lifetime],
    () => {
      setStatus(null);
      setAttempt(null);
      setClientId('');
      setError(null);
      setChecking(true);
    },
  );
  useLayoutEffect(() => {
    const token = {};
    current.current = token;
    return () => {
      if (current.current === token) current.current = null;
    };
  }, [connection.id, connection.endpoint, connection.version, lifetime, stale]);

  useEffect(() => {
    if (stale) return;
    const token = current.current;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let wasPending = !!attempt;
    async function read() {
      try {
        const next = await api.getOAuthStatus(
          connection.agentId,
          connection.id,
          { signal: controller.signal },
        );
        if (controller.signal.aborted || current.current !== token) return;
        setChecking(false);
        setStatus(next);
        if (attempt && next.attemptId !== attempt.attemptId) setAttempt(null);
        if (next.state === 'pending') {
          wasPending = true;
          timer = setTimeout(() => void read(), 2000);
        } else {
          setAttempt(null);
          if (wasPending) settled.current();
        }
      } catch {
        if (controller.signal.aborted || current.current !== token) return;
        setChecking(false);
        setAttempt(null);
        setError(
          'Account status is unavailable. Refresh status before trying again.',
        );
      }
    }
    // After start, leave time for the browser action before the first poll.
    if (attempt) timer = setTimeout(() => void read(), 2000);
    else void read();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [
    api,
    connection.agentId,
    connection.id,
    connection.endpoint,
    connection.version,
    lifetime,
    stale,
    readEpoch,
    attempt,
    settled,
  ]);

  async function start() {
    const token = current.current;
    await runAction(async () => {
      // Open during the owner's click so browsers can preserve user activation.
      // Remove the opener before sending the tab to a provider-controlled page.
      let popup: Window | null = null;
      try {
        popup = window.open('about:blank', '_blank');
        if (popup) {
          popup.opener = null;
          const referrer = popup.document.createElement('meta');
          referrer.name = 'referrer';
          referrer.content = 'no-referrer';
          popup.document.head.append(referrer);
        }
      } catch {
        /* The explicit link below also works with popup blocking. */
        popup?.close();
        popup = null;
      }
      try {
        const next = await api.startOAuth(connection.agentId, connection.id, {
          expectedVersion: connection.version,
          ...(clientId.trim() ? { clientId: clientId.trim() } : {}),
        });
        if (current.current !== token) {
          popup?.close();
          return;
        }
        if (!safeAuthorizationUrl(next.authorizationUrl, connection.endpoint)) {
          popup?.close();
          setError(
            'The sign-in link could not be verified. Refresh status before trying again.',
          );
          return;
        }
        setAttempt(next);
        setStatus({
          state: 'pending',
          attemptId: next.attemptId,
          expiresAt: next.expiresAt,
        });
        setError(null);
        try {
          popup?.location.replace(next.authorizationUrl);
        } catch {
          popup?.close();
        }
      } catch (caught) {
        popup?.close();
        if (current.current === token)
          setError(
            'Sign-in could not be confirmed. Refresh status before trying again.',
          );
        throw caught;
      }
    });
  }

  async function end(signOut: boolean) {
    const token = current.current;
    await runAction(async () => {
      try {
        if (signOut)
          await api.signOutOAuth(connection.agentId, connection.id, {
            expectedVersion: connection.version,
          });
        else if (status?.attemptId)
          await api.cancelOAuth(connection.agentId, connection.id, {
            expectedVersion: connection.version,
            attemptId: status.attemptId,
          });
        else return;
        if (current.current !== token) return;
        setAttempt(null);
        setStatus({ state: 'signed_out' });
        setError(null);
        settled.current();
      } catch (caught) {
        if (current.current === token)
          setError(
            'The account change could not be confirmed. Refresh status before trying again.',
          );
        throw caught;
      }
    });
  }

  const pending = status?.state === 'pending';
  const signedIn = status?.state === 'connected';
  const uncertain = stale || checking || !!error;
  return (
    <div
      className="space-y-2 rounded-lg border border-line p-3 text-xs"
      aria-label="Account sign-in"
    >
      <p className="font-medium">Account connection</p>
      <p role="status" className="text-ink-3">
        {stale
          ? 'Account status unavailable — refresh to verify'
          : checking
            ? 'Checking account status…'
            : pending
              ? 'Waiting for browser sign-in'
              : signedIn
                ? 'Account signed in'
                : status?.state === 'needs_sign_in'
                  ? 'Sign in again to reconnect this account'
                  : status?.state === 'failed'
                    ? 'Sign-in did not finish. Try again.'
                    : 'No account signed in'}
      </p>
      <p className="text-ink-3">
        Sign in to this server in your browser. Account access follows the
        agent’s tool permissions.
      </p>
      {error && <p role="alert">{error}</p>}
      {!uncertain &&
        pending &&
        attempt &&
        status.attemptId === attempt.attemptId && (
          <a
            className="inline-block underline"
            href={attempt.authorizationUrl}
            target="_blank"
            rel="noopener noreferrer"
          >
            Continue sign-in in browser
          </a>
        )}
      <div className="flex flex-wrap gap-2">
        {!pending && (
          <button
            className={button}
            disabled={blocked || uncertain}
            onClick={() => void start()}
          >
            {signedIn ? 'Sign in again' : 'Sign in'}
          </button>
        )}
        {pending && (
          <button
            className={button}
            disabled={blocked || uncertain || !status.attemptId}
            onClick={() => void end(false)}
          >
            Cancel sign-in
          </button>
        )}
        {(signedIn || connection.hasOAuthCredential) && (
          <button
            className={button}
            disabled={blocked || uncertain}
            onClick={() => void end(true)}
          >
            Sign out
          </button>
        )}
        {error && (
          <button
            className={button}
            disabled={blocked || stale}
            onClick={() => {
              setError(null);
              setChecking(true);
              setReadEpoch((value) => value + 1);
            }}
          >
            Check account status
          </button>
        )}
      </div>
      {!pending && (
        <details>
          <summary>Advanced sign-in settings</summary>
          <label className="mt-2 block">
            Client ID (optional)
            <input
              className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm"
              value={clientId}
              maxLength={2048}
              autoComplete="off"
              disabled={blocked || uncertain}
              onChange={(event) => setClientId(event.target.value)}
            />
          </label>
          <p className="mt-1 text-ink-3">
            Only needed when the provider requires a pre-registered app. Use a
            public desktop client.
          </p>
        </details>
      )}
      {(signedIn || connection.hasOAuthCredential) && (
        <p className="text-ink-3">
          Disconnect server keeps this sign-in. Sign out removes credentials
          from this device; manage other sessions with the provider.
        </p>
      )}
    </div>
  );
}
