import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  clearOwnerApiKey,
  ownerApiKeyHeaders,
  setOwnerApiKey,
} from '../lib/owner-api-key';

async function ownerAccessStatus(
  suppliedKey: string | undefined,
  signal: AbortSignal,
): Promise<number> {
  const response = await fetch('/api/capabilities', {
    method: 'GET',
    credentials: 'same-origin',
    cache: 'no-store',
    redirect: 'error',
    referrerPolicy: 'origin',
    headers: {
      accept: 'application/json',
      ...(suppliedKey === undefined
        ? ownerApiKeyHeaders()
        : { 'x-api-key': suppliedKey }),
    },
    signal,
  });
  await response.body?.cancel().catch(() => undefined);
  return response.status;
}

/** Owner bootstrap and console launch-code exchange must live inside this gate. */
export function OwnerApiKeyGate({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<
    'checking' | 'ready' | 'login' | 'unavailable'
  >('checking');
  const [key, setKey] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const version = useRef(0);
  const active = useRef<AbortController | null>(null);

  const verify = useCallback((suppliedKey?: string) => {
    const current = ++version.current;
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    return ownerAccessStatus(suppliedKey, controller.signal)
      .then((status) => {
        if (current !== version.current || controller.signal.aborted) return;
        if (status === 200) {
          if (suppliedKey !== undefined) setOwnerApiKey(suppliedKey);
          setKey('');
          setPhase('ready');
        } else if (status === 401) {
          clearOwnerApiKey();
          setPhase('login');
          if (suppliedKey !== undefined)
            setError('That owner API key was not accepted.');
        } else if (status === 403) {
          clearOwnerApiKey();
          setPhase('unavailable');
          setError(
            'This browser is not approved as the local owner. Open the configured Anima console on this computer.',
          );
        } else {
          setPhase('unavailable');
          setError(
            'Anima could not verify owner access. Check the daemon connection and retry.',
          );
        }
      })
      .catch(() => {
        if (current !== version.current || controller.signal.aborted) return;
        setPhase('unavailable');
        setError(
          'Anima could not verify owner access. Check the daemon connection and retry.',
        );
      })
      .finally(() => {
        if (current === version.current) setBusy(false);
      });
  }, []);

  const startVerify = (suppliedKey?: string) => {
    setBusy(true);
    setError(null);
    void verify(suppliedKey);
  };

  useEffect(() => {
    void verify();
    return () => {
      version.current += 1;
      active.current?.abort();
    };
  }, [verify]);

  if (phase === 'ready') return <>{children}</>;
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-md flex-col justify-center gap-4 px-6 py-12 text-ink">
      <h1 className="font-serif text-3xl">Open your Anima console</h1>
      {phase === 'checking' && <p role="status">Checking owner access…</p>}
      {phase === 'login' && (
        <form
          className="grid gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (!busy && key.trim()) startVerify(key.trim());
          }}
        >
          <p>
            This installation requires its owner API key. The key stays in this
            page’s memory until you close or reload it, or open a shared
            conversation.
          </p>
          <label className="grid gap-2">
            Owner API key
            <input
              className="field"
              type="password"
              autoComplete="off"
              value={key}
              maxLength={4096}
              onChange={(event) => setKey(event.target.value)}
              required
            />
          </label>
          <button
            className="studio-button-primary"
            type="submit"
            disabled={busy || !key.trim()}
          >
            {busy ? 'Checking…' : 'Unlock owner console'}
          </button>
        </form>
      )}
      {phase === 'unavailable' && (
        <button
          className="studio-tool-button"
          type="button"
          disabled={busy}
          onClick={() => startVerify()}
        >
          Retry connection
        </button>
      )}
      {error && <p role="alert">{error}</p>}
    </main>
  );
}
