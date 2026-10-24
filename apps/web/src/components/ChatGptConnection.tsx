import { ownerFetch } from '../lib/owner-fetch';
import { useCallback, useEffect, useRef, useState } from 'react';
import { createDaemonClient, type ChatGptStatus } from '@animaos/sdk';
import { useLatest } from '../hooks/useLatest';

const client = createDaemonClient({ baseUrl: '', fetch: ownerFetch }).chatgpt;
const verificationUrl = 'https://auth.openai.com/codex/device';
const buttonClass =
  'rounded-xl border border-line px-3 py-2 text-sm text-ink disabled:opacity-50';
const compactButtonClass =
  'onboarding__button onboarding__button--secondary onboarding__button--small';

export function ChatGptConnection({
  onConnectionChange,
  onModelScopeChange,
  compact = false,
}: {
  /** Drops the card chrome and the long explanation, for use inside a choice. */
  compact?: boolean;
  onConnectionChange?: () => void | Promise<void>;
  /** Invalidates picker requests before auth work; settled events reconcile cache only. */
  onModelScopeChange?: (pending: boolean) => void;
}) {
  const [status, setStatus] = useState<ChatGptStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [now, setNow] = useState(Date.now);
  const [poll, setPoll] = useState(0);
  const generation = useRef(0);
  const connected = useRef<boolean | null>(null);
  const account = useRef<string | null>(null);
  const onChange = useLatest(onConnectionChange);
  const modelScopeChange = useLatest(onModelScopeChange);

  const request = useCallback(
    (
      action: 'status' | 'login' | 'cancel' | 'disconnect',
      foreground = true,
    ) => {
      const version = ++generation.current;
      if (foreground) {
        modelScopeChange.current?.(true);
      }
      return client[action]()
        .then((next) => {
          if (version !== generation.current) return;
          setStatus(next);
          if (
            (connected.current !== next.connected ||
              account.current !== next.accountId) &&
            (connected.current !== null || next.connected)
          ) {
            void Promise.resolve(onChange.current?.()).catch(() => {
              if (version === generation.current)
                setError(
                  'Account updated, but the provider catalog could not refresh. Retry the provider catalog.',
                );
            });
          }
          if (
            !foreground &&
            (connected.current !== next.connected ||
              account.current !== next.accountId)
          ) {
            modelScopeChange.current?.(false);
          }
          connected.current = next.connected;
          account.current = next.accountId;
        })
        .catch((cause: unknown) => {
          if (version === generation.current)
            setError(
              cause instanceof Error
                ? cause.message
                : 'Unable to update ChatGPT connection.',
            );
        })
        .finally(() => {
          if (version === generation.current) {
            if (foreground) modelScopeChange.current?.(false);
            setBusy(false);
            setNow(Date.now());
            setPoll((value) => value + 1);
          }
        });
    },
    [onChange, modelScopeChange],
  );

  const beginRequest = (
    action: 'status' | 'login' | 'cancel' | 'disconnect',
  ) => {
    setBusy(true);
    setError(null);
    void request(action);
  };

  useEffect(() => {
    void request('status');
    return () => {
      generation.current += 1;
    };
  }, [request]);

  const login = status?.login;
  const expired = Boolean(login && now >= login.expiresAtMs);
  useEffect(() => {
    if (!login || expired || busy) return;
    const timer = window.setTimeout(
      () => {
        setNow(Date.now());
        setError(null);
        void request('status', false);
      },
      Math.min(3000, Math.max(0, login.expiresAtMs - Date.now())),
    );
    return () => window.clearTimeout(timer);
  }, [login, expired, busy, poll, request]);

  return (
    <section
      aria-label="ChatGPT subscription"
      className={
        compact
          ? 'onboarding__connection'
          : 'space-y-3 rounded-xl border border-line bg-white/[0.02] p-4'
      }
    >
      {compact ? null : (
        <>
          <h3 className="text-sm font-semibold text-ink">
            ChatGPT subscription
          </h3>
          <p className="text-xs leading-relaxed text-ink-2">
            Use your ChatGPT plan. Subscription limits apply; model availability
            depends on your account. This connection does not use API-key
            billing.
          </p>
        </>
      )}
      <p className="text-xs leading-relaxed text-ink-3">
        {compact
          ? 'Uses your plan’s limits, not API-key billing. Turn on device-code login in your ChatGPT security settings first, or ask your workspace administrator.'
          : 'Enable device-code login in your ChatGPT security settings, or ask your workspace administrator to allow it, before connecting.'}
      </p>
      <p role="status" className="text-sm text-ink-2">
        {busy
          ? 'Updating ChatGPT connection…'
          : !status
            ? 'ChatGPT connection status is unavailable.'
            : status.connected
              ? `Connected${status.planType ? ` · ${status.planType}` : ''}`
              : expired
                ? 'Sign-in expired. Start again for a new code.'
                : login
                  ? 'Waiting for you to finish signing in…'
                  : 'ChatGPT is not connected.'}
      </p>
      {status?.connected && status.accountId && (
        <p className="break-all text-xs text-ink-3">
          Account: {status.accountId}
        </p>
      )}
      {(error || status?.error) && (
        <p role="alert" className="text-sm text-danger">
          {error || status?.error}
        </p>
      )}
      {login && !expired && (
        <div className="space-y-2 text-sm text-ink">
          <p>
            Enter this code on OpenAI:{' '}
            <strong className="select-all font-mono">{login.userCode}</strong>
          </p>
          <p className="text-xs text-ink-3">
            Expires at {new Date(login.expiresAtMs).toLocaleTimeString()}.
          </p>
          {login.verificationUrl === verificationUrl ? (
            <a
              href={verificationUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-block text-accent underline"
            >
              Continue on OpenAI
            </a>
          ) : (
            <p role="alert" className="text-danger">
              The daemon returned an unexpected verification address. Cancel and
              try again.
            </p>
          )}
        </div>
      )}
      <div className="flex flex-wrap gap-2">
        {status?.connected ? (
          <button
            type="button"
            className={compact ? compactButtonClass : buttonClass}
            disabled={busy}
            onClick={() => beginRequest('disconnect')}
          >
            Disconnect ChatGPT
          </button>
        ) : !login || expired ? (
          <button
            type="button"
            className={compact ? compactButtonClass : buttonClass}
            disabled={busy}
            onClick={() => beginRequest('login')}
          >
            Connect ChatGPT
          </button>
        ) : null}
        {login && (
          <button
            type="button"
            className={compact ? compactButtonClass : buttonClass}
            disabled={busy}
            onClick={() => beginRequest('cancel')}
          >
            Cancel sign-in
          </button>
        )}
        {error && (
          <button
            type="button"
            className={compact ? compactButtonClass : buttonClass}
            disabled={busy}
            onClick={() => beginRequest('status')}
          >
            Refresh connection
          </button>
        )}
        {!status && error && (
          <button
            type="button"
            className={compact ? compactButtonClass : buttonClass}
            disabled={busy}
            onClick={() => beginRequest('disconnect')}
          >
            Clear saved ChatGPT connection
          </button>
        )}
      </div>
    </section>
  );
}
