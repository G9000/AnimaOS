import { useCallback, useEffect, useState } from 'react';

import {
  daemon,
  type DaemonProvider,
  type KeyStrategy,
  type ProviderKey,
  type ProviderKeys as ProviderKeysView,
} from '../../lib/daemon-api';
import { ghostBtnCls } from '../ui-bits';
import {
  SettingsCard,
  SettingsEmpty,
  SettingsRow,
  SettingsSection,
} from '../settings/SettingsKit';
import { AddKeyForm, KEYS_NOT_PERSISTED_NOTE } from './AddKeyForm';

const STRATEGIES: readonly { value: KeyStrategy; label: string }[] = [
  { value: 'fill_first', label: 'Use the first until it rests' },
  { value: 'round_robin', label: 'Take turns' },
  { value: 'random', label: 'Pick at random' },
  { value: 'least_used', label: 'Use the least used' },
];

function errorText(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

/** A clock time, with the day when it is not today. */
export function restTime(ms: number, now: number = Date.now()): string {
  const at = new Date(ms);
  const time = at.toLocaleTimeString([], {
    hour: 'numeric',
    minute: '2-digit',
  });
  return at.toDateString() === new Date(now).toDateString()
    ? time
    : `${at.toLocaleDateString([], { month: 'short', day: 'numeric' })}, ${time}`;
}

export function keyStatus(key: ProviderKey): string {
  if (key.status !== 'resting') return 'Ready';
  const until =
    key.restingUntilMs === null
      ? 'Resting'
      : `Resting until ${restTime(key.restingUntilMs)}`;
  return key.lastError ? `${until} · ${key.lastError}` : until;
}

/** "2 keys · 1 resting", or "1 key · Ready". */
function poolSummary(pool: ProviderKeysView): string {
  const count = `${pool.keys.length} ${pool.keys.length === 1 ? 'key' : 'keys'}`;
  const resting = pool.keys.filter((key) => key.status === 'resting').length;
  return resting > 0 ? `${count} · ${resting} resting` : `${count} · Ready`;
}

/**
 * The provider key pool (spec 2026-10-19): add and remove keys, see which
 * ones are resting and why, wake one early ("Try now", for example after
 * topping up credit), and pick how a provider takes turns. Keys come
 * back masked and are never shown whole.
 */
export function ProviderKeys({
  providers,
  onChanged,
  disabled = false,
}: {
  providers: readonly DaemonProvider[] | null;
  /** Keys changed: whoever shows `configured` reads the providers again. */
  onChanged?: () => void | Promise<void>;
  disabled?: boolean;
}) {
  const [pools, setPools] = useState<ProviderKeysView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const { providers: loaded } = await daemon.providerKeys();
      setPools(loaded);
      setError(null);
    } catch (caught) {
      setError(errorText(caught));
    }
  }, []);

  useEffect(() => {
    // State updates follow the awaited provider-key response.
    // eslint-disable-next-line react/set-state-in-effect
    void load();
  }, [load]);

  const changed = async () => {
    await load();
    await onChanged?.();
  };
  const remove = async (provider: string, key: ProviderKey) => {
    try {
      await daemon.removeProviderKey(provider, key.id);
      setError(null);
      await changed();
    } catch (caught) {
      setError(errorText(caught));
    }
  };
  const wake = async (provider: string, key: ProviderKey) => {
    try {
      await daemon.wakeProviderKey(provider, key.id);
      setError(null);
      await changed();
    } catch (caught) {
      setError(errorText(caught));
    }
  };
  const pick = async (provider: string, strategy: KeyStrategy) => {
    try {
      const saved = await daemon.setKeyStrategy(provider, strategy);
      setPools(
        (current) =>
          current?.map((pool) => (pool.id === saved.id ? saved : pool)) ?? null,
      );
      setError(null);
    } catch (caught) {
      setError(errorText(caught));
    }
  };

  const label = (id: string) =>
    providers?.find((provider) => provider.id === id)?.label ?? id;
  const shown = (pools ?? []).filter(
    (pool) => providers?.find((p) => p.id === pool.id)?.requiresKey !== false,
  );
  const connected = shown.filter((pool) => pool.keys.length > 0);
  // A new provider is the likelier choice, so those come first.
  const addable = [
    ...shown.filter((pool) => pool.keys.length === 0),
    ...connected,
  ].map((pool) => ({ id: pool.id, label: label(pool.id) }));

  return (
    <SettingsSection
      id="settings-api-keys"
      title="API keys"
      description="Keys stay in the operating-system keychain, never in anima's files."
    >
      {shown.some((pool) => !pool.persisted) && (
        <p role="note" className="key-warning">
          {KEYS_NOT_PERSISTED_NOTE}
        </p>
      )}
      {error ? (
        <p role="alert" className="key-error">
          {error}
        </p>
      ) : null}
      <SettingsCard>
        {pools === null && !error ? (
          <SettingsEmpty>Loading keys…</SettingsEmpty>
        ) : null}
        {connected.length > 0 && (
          <ul className="key-providers" aria-label="Providers with keys">
            {connected.map((pool) => (
              <SettingsRow
                as="li"
                key={pool.id}
                className="key-provider"
                title={label(pool.id)}
                description={poolSummary(pool)}
                control={
                  // Taking turns only means something with two keys.
                  pool.keys.length > 1 && (
                    <select
                      aria-label={`How ${label(pool.id)} picks a key`}
                      className="field key-strategy"
                      value={pool.strategy}
                      disabled={disabled}
                      onChange={(event) =>
                        void pick(pool.id, event.target.value as KeyStrategy)
                      }
                    >
                      {STRATEGIES.map((strategy) => (
                        <option key={strategy.value} value={strategy.value}>
                          {strategy.label}
                        </option>
                      ))}
                    </select>
                  )
                }
              >
                <ul className="key-list">
                  {pool.keys.map((key) => (
                    <li
                      key={key.id}
                      className={`key-row${key.status === 'resting' ? ' is-resting' : ''}`}
                    >
                      <code className="key-masked">{key.masked}</code>
                      {key.label ? (
                        <span className="key-label">{key.label}</span>
                      ) : null}
                      {key.source === 'env' ? (
                        <span className="key-label">From the environment</span>
                      ) : null}
                      <span className="key-status">{keyStatus(key)}</span>
                      {key.status === 'resting' && (
                        <button
                          type="button"
                          className={ghostBtnCls}
                          aria-label={`Try key ${key.masked} now`}
                          disabled={disabled}
                          onClick={() => void wake(pool.id, key)}
                        >
                          Try now
                        </button>
                      )}
                      {key.source === 'file' && (
                        <button
                          type="button"
                          className={ghostBtnCls}
                          aria-label={`Remove key ${key.masked}`}
                          disabled={disabled}
                          onClick={() => void remove(pool.id, key)}
                        >
                          Remove
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              </SettingsRow>
            ))}
          </ul>
        )}
        {addable.length > 0 && (
          <SettingsRow
            className="key-add-panel"
            title={connected.length > 0 ? 'Add a key' : 'Add your first key'}
            description="Pick a provider and paste its key."
          >
            <AddKeyForm
              providers={addable}
              buttonLabel="Add key"
              disabled={disabled}
              onAdded={changed}
            />
          </SettingsRow>
        )}
      </SettingsCard>
    </SettingsSection>
  );
}
