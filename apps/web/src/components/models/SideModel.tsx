import { useEffect, useState } from 'react';

import {
  daemon,
  isMockProvider,
  MODEL_SUGGESTIONS,
  type DaemonProvider,
  type ModelChoice,
} from '../../lib/daemon-api';
import { primaryBtnCls } from '../ui-bits';
import {
  SettingsCard,
  SettingsRow,
  SettingsSection,
} from '../settings/SettingsKit';

function errorText(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

/**
 * The model for small background tasks (spec 2026-10-19): titles, summaries,
 * the smart check and task reviews. Empty means each agent's own model.
 */
export function SideModel({
  providers,
  disabled = false,
}: {
  providers: readonly DaemonProvider[] | null;
  disabled?: boolean;
}) {
  const [saved, setSaved] = useState<ModelChoice | null | undefined>(undefined);
  const [provider, setProvider] = useState('');
  const [model, setModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [flash, setFlash] = useState(false);

  useEffect(() => {
    let live = true;
    daemon
      .sideModel()
      .then((choice) => {
        if (!live) return;
        setSaved(choice);
        setProvider(choice?.provider ?? '');
        setModel(choice?.model ?? '');
      })
      .catch((caught) => {
        if (live) setError(errorText(caught));
      });
    return () => {
      live = false;
    };
  }, []);

  const choice: ModelChoice | null = provider
    ? { provider, model: model.trim() }
    : null;
  const unchanged =
    saved === undefined ||
    (saved === null
      ? choice === null
      : choice?.provider === saved.provider && choice.model === saved.model);
  const valid = choice === null || choice.model !== '';
  const listed = (providers ?? [])
    .map((p) => p.id)
    .filter((id) => !isMockProvider(id));
  // The saved choice keeps showing, even an unlisted or mock one.
  const ids = listed.concat(
    provider && !listed.includes(provider) ? [provider] : [],
  );

  const submit = async () => {
    if (unchanged || !valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await daemon.setSideModel(choice);
      setSaved(result);
      setFlash(true);
      setTimeout(() => setFlash(false), 2000);
    } catch (caught) {
      setError(errorText(caught));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingsSection
      id="settings-side-model"
      title="Side-task model"
      description="Titles, summaries, the smart check and task reviews. Leave empty to use each agent's own model."
    >
      <SettingsCard>
        <SettingsRow title="Model for small background tasks">
          <div className="side-model-row">
            <select
              aria-label="Side-task provider"
              className="field"
              value={provider}
              disabled={busy || disabled}
              onChange={(event) => {
                const next = event.target.value;
                setProvider(next);
                setModel(next ? (MODEL_SUGGESTIONS[next]?.[0] ?? '') : '');
              }}
            >
              <option value="">Each agent's own model</option>
              {ids.map((id) => (
                <option key={id} value={id}>
                  {providers?.find((p) => p.id === id)?.label ?? id}
                </option>
              ))}
            </select>
            <input
              aria-label="Side-task model id"
              className="field"
              list="side-model-suggestions"
              value={model}
              disabled={busy || disabled || !provider}
              placeholder="model id"
              onChange={(event) => setModel(event.target.value)}
            />
            <datalist id="side-model-suggestions">
              {(MODEL_SUGGESTIONS[provider] ?? []).map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
            <button
              type="button"
              className={primaryBtnCls}
              disabled={unchanged || !valid || busy || disabled}
              onClick={() => void submit()}
            >
              {busy ? 'Saving…' : flash ? 'Saved ✓' : 'Save'}
            </button>
          </div>
        </SettingsRow>
      </SettingsCard>
      {error ? (
        <p role="alert" className="key-error">
          {error}
        </p>
      ) : null}
    </SettingsSection>
  );
}
