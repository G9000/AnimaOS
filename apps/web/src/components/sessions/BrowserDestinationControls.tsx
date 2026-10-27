import { useEffect, useRef, useState } from 'react';
import type {
  BrowserPolicy,
  BrowserProfile,
  BrowserScopesClient,
  BrowserSelection,
  BrowserSetup,
} from '@animaos/sdk';
import { useLatest } from '../../hooks/useLatest';

export type BrowserDestinationApi = Pick<
  BrowserScopesClient,
  | 'setup'
  | 'profiles'
  | 'createDestination'
  | 'revokeDestination'
  | 'createProfile'
  | 'startProfileLogin'
  | 'finishProfileLogin'
  | 'revokeProfile'
>;

export function BrowserDestinationControls({
  agentId,
  disabled,
  api,
  policies,
  selection,
  onSelection,
  onCreated,
  onRevoked,
  onRefresh,
}: {
  agentId: string;
  disabled: boolean;
  api: BrowserDestinationApi;
  policies: BrowserPolicy[];
  selection: BrowserSelection | null;
  onSelection(selection: BrowserSelection | null): void;
  onCreated(policy: BrowserPolicy): void;
  onRevoked(policyId: string): void;
  onRefresh(): Promise<void>;
}) {
  const mounted = useRef(true);
  const controller = useRef<AbortController | null>(null);
  const sequence = useRef(0);
  const mutating = useRef(false);
  const latest = useLatest({ selection, policies });
  const [loaded, setLoaded] = useState<{
    setup: BrowserSetup;
    profiles: BrowserProfile[];
  } | null>(null);
  const [reading, setReading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const [startUrl, setStartUrl] = useState('');
  const [origins, setOrigins] = useState('');
  const [authenticated, setAuthenticated] = useState(false);
  const [reviewed, setReviewed] = useState(false);
  const policy = policies.find((item) => item.id === selection?.policyId);
  const accounts =
    loaded?.profiles.filter(
      (item) => item.policyId === policy?.id && !item.revoked,
    ) ?? [];
  const locked = disabled || busy || reading || needsRefresh;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      controller.current?.abort();
    };
  }, []);

  const read = async (refreshStatus = false) => {
    if (disabled || mutating.current) return;
    controller.current?.abort();
    const request = new AbortController();
    controller.current = request;
    const attempt = ++sequence.current;
    const active = () =>
      mounted.current &&
      attempt === sequence.current &&
      !request.signal.aborted;
    setReading(true);
    setFailure(null);
    try {
      const [setup, profiles] = await Promise.all([
        api.setup(agentId, { signal: request.signal }),
        api.profiles(agentId, { signal: request.signal }),
        refreshStatus ? onRefresh() : Promise.resolve(),
      ]);
      if (active()) {
        setLoaded({ setup, profiles });
        setNeedsRefresh(false);
      }
    } catch {
      if (active()) {
        setLoaded(null);
        setFailure(
          'Website and account status could not be read. Refresh to check it.',
        );
      }
    } finally {
      if (active()) setReading(false);
    }
  };

  const mutate = async (
    action: (selectedAtStart: BrowserSelection | null) => Promise<void>,
  ) => {
    if (locked || mutating.current) return;
    mutating.current = true;
    controller.current?.abort();
    ++sequence.current;
    setReading(false);
    setBusy(true);
    setFailure(null);
    try {
      await action(latest.current.selection);
    } catch {
      if (mounted.current) {
        setNeedsRefresh(true);
        setFailure(
          'The browser change was not confirmed. Refresh websites and accounts before trying again.',
        );
      }
    } finally {
      mutating.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  const rememberProfile = (
    next: BrowserProfile,
    selectedAtStart: BrowserSelection | null,
  ) => {
    if (!mounted.current) return;
    setLoaded(
      (value) =>
        value && {
          ...value,
          profiles: [
            ...value.profiles.filter((item) => item.id !== next.id),
            next,
          ],
        },
    );
    const chosen = latest.current.selection;
    if (chosen?.policyId === next.policyId) {
      if (next.ready && !next.revoked && chosen === selectedAtStart)
        onSelection({ ...chosen, profileId: next.id });
      else if (chosen.profileId === next.id) {
        const { profileId: _profileId, ...withoutProfile } = chosen;
        onSelection(withoutProfile);
      }
    }
  };

  const create = () => {
    let parsed: URL;
    let allowedOrigins: string[];
    try {
      parsed = new URL(startUrl.trim());
      const additional = origins.split(/\s+/).filter(Boolean);
      allowedOrigins = [parsed.origin, ...additional];
      if (
        parsed.protocol !== 'https:' ||
        parsed.username ||
        parsed.password ||
        parsed.hash ||
        parsed.port ||
        allowedOrigins.length > 8 ||
        new Set(allowedOrigins).size !== allowedOrigins.length ||
        additional.some((origin) => {
          const value = new URL(origin);
          return (
            value.protocol !== 'https:' || value.origin !== origin || value.port
          );
        })
      )
        throw new Error('invalid');
    } catch {
      setFailure(
        'Enter a public HTTPS website and up to seven different HTTPS origins, without paths or extra ports.',
      );
      return;
    }
    if (!reviewed) return;
    void mutate(async (selectedAtStart) => {
      const created = await api.createDestination(agentId, {
        startUrl: parsed.href,
        allowedOrigins,
        authenticated,
      });
      if (!mounted.current) return;
      onCreated(created);
      if (latest.current.selection === selectedAtStart)
        onSelection({ policyId: created.id, selectedInputIds: [] });
      setStartUrl('');
      setOrigins('');
      setReviewed(false);
      setAuthenticated(false);
    });
  };

  return (
    <details
      className="rounded border border-line p-2"
      onToggle={(event) => {
        if (
          event.currentTarget.open &&
          !loaded &&
          !reading &&
          !controller.current
        )
          void read();
      }}
    >
      <summary className="cursor-pointer">Websites and sign-in</summary>
      <div className="mt-2 space-y-3">
        <button
          type="button"
          disabled={disabled || reading || busy}
          onClick={() => void read(true)}
        >
          Refresh websites and accounts
        </button>
        {reading && <p role="status">Reading websites and accounts…</p>}
        {failure && <p role="alert">{failure}</p>}
        {loaded && (
          <>
            {!loaded.setup.available ? (
              <div role="status">
                <p>Browser setup is needed on this computer.</p>
                <p>
                  Run <code>{loaded.setup.setupCommand}</code> in the AnimaOS
                  folder. Restart AnimaOS, then refresh.
                </p>
              </div>
            ) : (
              <p>Browser is ready. Sign-in uses a separate browser account.</p>
            )}
            <fieldset
              disabled={locked || !loaded.setup.available}
              className="space-y-2"
            >
              <legend>Add a website</legend>
              <label className="block">
                Starting website URL{' '}
                <input
                  type="url"
                  value={startUrl}
                  maxLength={2048}
                  onChange={(event) => {
                    setStartUrl(event.target.value);
                    setReviewed(false);
                  }}
                  className="block w-full"
                  placeholder="https://example.com"
                />
              </label>
              <label className="block">
                Additional allowed origins{' '}
                <textarea
                  value={origins}
                  maxLength={4096}
                  onChange={(event) => {
                    setOrigins(event.target.value);
                    setReviewed(false);
                  }}
                  className="block w-full"
                  placeholder="https://accounts.example.com"
                />
              </label>
              <p>
                The starting website is included. Add up to seven other HTTPS
                origins, one per line. Only these websites can be contacted.
              </p>
              <label className="block">
                <input
                  type="checkbox"
                  checked={authenticated}
                  onChange={(event) => {
                    setAuthenticated(event.target.checked);
                    setReviewed(false);
                  }}
                />{' '}
                This website needs sign-in
              </label>
              <label className="block">
                <input
                  type="checkbox"
                  checked={reviewed}
                  onChange={(event) => setReviewed(event.target.checked)}
                />{' '}
                Allow this browser task to use these websites
              </label>
              <button
                type="button"
                disabled={!reviewed || !startUrl.trim()}
                onClick={create}
              >
                Add website
              </button>
            </fieldset>
            {policies
              .filter((item) => item.interactive)
              .map((item) => (
                <div
                  key={item.id}
                  className="flex items-center justify-between gap-2"
                >
                  <span>{item.origin}</span>
                  <button
                    type="button"
                    disabled={locked}
                    aria-label={`Remove website ${item.origin}`}
                    onClick={() =>
                      void mutate(async () => {
                        await api.revokeDestination(agentId, item.id, {
                          expectedVersion: item.version,
                        });
                        if (mounted.current) onRevoked(item.id);
                      })
                    }
                  >
                    Remove website
                  </button>
                </div>
              ))}
            {policy?.authenticated && (
              <fieldset
                disabled={locked || !loaded.setup.available}
                className="space-y-2"
              >
                <legend>Dedicated account for {policy.origin}</legend>
                <p>
                  Sign in in the browser window, then finish here to save the
                  account for future tasks.
                </p>
                <label className="block">
                  Account for next message{' '}
                  <select
                    value={selection?.profileId ?? ''}
                    onChange={(event) => {
                      if (!selection) return;
                      const { profileId: _profileId, ...withoutProfile } =
                        selection;
                      onSelection(
                        event.target.value
                          ? { ...withoutProfile, profileId: event.target.value }
                          : withoutProfile,
                      );
                    }}
                  >
                    <option value="">Choose a signed-in account</option>
                    {accounts.map((account, index) => (
                      <option
                        key={account.id}
                        value={account.id}
                        disabled={!account.ready || account.loginActive}
                      >
                        Account {index + 1}
                        {account.ready ? ' — ready' : ' — sign-in needed'}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  type="button"
                  onClick={() =>
                    void mutate(async (selectedAtStart) =>
                      rememberProfile(
                        await api.createProfile(agentId, {
                          policyId: policy.id,
                        }),
                        selectedAtStart,
                      ),
                    )
                  }
                >
                  Create dedicated account
                </button>
                {accounts.map((account, index) => (
                  <div key={account.id} className="space-x-2">
                    <span>
                      Account {index + 1}:{' '}
                      {account.loginActive
                        ? 'Sign-in browser is open'
                        : account.ready
                          ? 'Ready'
                          : 'Sign-in needed'}
                    </span>
                    {account.loginActive ? (
                      <button
                        type="button"
                        onClick={() =>
                          void mutate(async (selectedAtStart) =>
                            rememberProfile(
                              await api.finishProfileLogin(
                                agentId,
                                account.id,
                                { expectedRevision: account.revision },
                              ),
                              selectedAtStart,
                            ),
                          )
                        }
                      >
                        Finish sign-in
                      </button>
                    ) : (
                      <button
                        type="button"
                        disabled={
                          !policy.routes.some((route) => route.method === 'GET')
                        }
                        onClick={() =>
                          void mutate(async (selectedAtStart) =>
                            rememberProfile(
                              await api.startProfileLogin(agentId, account.id, {
                                expectedRevision: account.revision,
                                routeId: (policy.routes.find(
                                  (route) => route.id === 'start',
                                ) ??
                                  policy.routes.find(
                                    (route) => route.method === 'GET',
                                  ))!.id,
                              }),
                              selectedAtStart,
                            ),
                          )
                        }
                      >
                        Sign in
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() =>
                        void mutate(async (selectedAtStart) =>
                          rememberProfile(
                            await api.revokeProfile(agentId, account.id, {
                              expectedRevision: account.revision,
                            }),
                            selectedAtStart,
                          ),
                        )
                      }
                    >
                      Revoke account
                    </button>
                  </div>
                ))}
              </fieldset>
            )}
          </>
        )}
      </div>
    </details>
  );
}
