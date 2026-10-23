import { useEffect, useState } from 'react';

import { daemon } from '../../lib/daemon-api';
import { primaryBtnCls } from '../ui-bits';

/** Shown when the daemon has no key file, so added keys are not saved. */
export const KEYS_NOT_PERSISTED_NOTE =
  'Keys added here last until the daemon restarts. Set ANIMAOS_RS_PROVIDER_KEYS_FILE to keep them.';

export interface KeyProvider {
  id: string;
  label: string;
}

function errorText(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

/**
 * A password-style field that hands an API key to the daemon. The key is held
 * only while it is typed: it is never logged or shown again, and it is cleared
 * once the daemon has taken it.
 */
export function AddKeyForm({
  providers,
  buttonLabel,
  variant = 'settings',
  disabled = false,
  warnIfNotPersisted = false,
  onAdded,
}: {
  /** Where the key can go; a picker shows when there is more than one. */
  providers: readonly KeyProvider[];
  buttonLabel: string;
  variant?: 'settings' | 'onboarding';
  disabled?: boolean;
  /** Ask the daemon and say so when keys for the chosen provider won't be saved. */
  warnIfNotPersisted?: boolean;
  onAdded?: (provider: string) => void | Promise<void>;
}) {
  const [chosen, setChosen] = useState(providers[0]?.id ?? '');
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onboarding = variant === 'onboarding';
  const [unsaved, setUnsaved] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    if (!warnIfNotPersisted) return;
    let live = true;
    daemon
      .providerKeys()
      .then(({ providers: pools }) => {
        if (live)
          setUnsaved(
            new Set(
              pools.filter((pool) => !pool.persisted).map((pool) => pool.id),
            ),
          );
      })
      // Without the answer there is nothing to warn about.
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [warnIfNotPersisted]);

  // The chosen provider can vanish once it holds a key; fall back to the first.
  const fallbackProvider = providers[0]?.id ?? '';
  if (
    !providers.some((provider) => provider.id === chosen) &&
    chosen !== fallbackProvider
  ) {
    setChosen(fallbackProvider);
  }

  const target = providers.find((provider) => provider.id === chosen);
  if (!target) return null;
  const key = value.trim();

  const add = async () => {
    if (!key || busy) return;
    setBusy(true);
    setError(null);
    try {
      await daemon.addProviderKey(target.id, { key });
      setValue('');
      await onAdded?.(target.id);
    } catch (caught) {
      setError(errorText(caught));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={onboarding ? 'onboarding__section' : 'key-add'}>
      {providers.length > 1 && (
        <select
          aria-label="Provider for the new key"
          className={
            onboarding ? 'onboarding__input' : 'field key-add-provider'
          }
          value={chosen}
          disabled={busy || disabled}
          onChange={(event) => setChosen(event.target.value)}
        >
          {providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.label}
            </option>
          ))}
        </select>
      )}
      <div className="key-add-row">
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          aria-label={`API key for ${target.label}`}
          className={
            onboarding ? 'onboarding__input onboarding__input--mono' : 'field'
          }
          placeholder="Paste an API key"
          value={value}
          disabled={busy || disabled}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              void add();
            }
          }}
        />
        <button
          type="button"
          className={
            onboarding
              ? 'onboarding__button onboarding__button--secondary'
              : primaryBtnCls
          }
          disabled={!key || busy || disabled}
          onClick={() => void add()}
        >
          {busy ? 'Adding…' : buttonLabel}
        </button>
      </div>
      {unsaved.has(target.id) ? (
        <p role="note" className="key-warning">
          {KEYS_NOT_PERSISTED_NOTE}
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="key-error">
          {error}
        </p>
      ) : null}
    </div>
  );
}
