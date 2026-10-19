import { useEffect, useMemo, useState } from 'react';

import {
  isMockProvider,
  MODEL_SUGGESTIONS,
  type DaemonProvider,
  type ModelChoice,
} from '../../lib/daemon-api';
import { ghostBtnCls, primaryBtnCls } from '../ui-bits';

/** The daemon accepts at most this many. */
export const MAX_FALLBACKS = 3;

interface Row extends ModelChoice {
  key: number;
}

function sameChoice(a: ModelChoice, b: ModelChoice): boolean {
  return a.provider === b.provider && a.model.trim() === b.model.trim();
}

/** Why a row can't be saved, or null when it can. */
function problem(
  row: Row,
  index: number,
  rows: readonly Row[],
  primary: ModelChoice,
): string | null {
  if (!row.model.trim()) return 'Choose a model.';
  if (sameChoice(row, primary)) return 'That is already the main model.';
  if (rows.findIndex((other) => sameChoice(other, row)) !== index)
    return 'That model is already in the list.';
  return null;
}

/**
 * The models tried, in order, when the main one fails before it starts
 * replying (spec 2026-10-19). Order matters, so rows move up and down.
 */
export function FallbackModels({
  primary,
  fallbacks,
  providers,
  save,
  disabled = false,
}: {
  primary: ModelChoice;
  fallbacks: readonly ModelChoice[];
  providers: readonly DaemonProvider[] | null;
  /** Resolves true when the daemon took the new list. */
  save: (fallbacks: ModelChoice[]) => Promise<boolean>;
  disabled?: boolean;
}) {
  const savedKey = JSON.stringify(fallbacks);
  const [next, setNext] = useState(0);
  const [rows, setRows] = useState<Row[]>(() =>
    fallbacks.map((choice, key) => ({ ...choice, key })),
  );
  const [busy, setBusy] = useState(false);

  // An accepted save, or another agent, replaces the draft.
  useEffect(() => {
    setRows(
      (JSON.parse(savedKey) as ModelChoice[]).map((choice, key) => ({
        ...choice,
        key,
      })),
    );
    setNext(JSON.parse(savedKey).length);
  }, [savedKey]);

  const providerIds = useMemo(
    () =>
      (providers ?? [])
        .map((provider) => provider.id)
        .filter((id) => !isMockProvider(id))
        // A row keeps showing its own provider, even an unlisted or mock one.
        .concat(rows.map((row) => row.provider))
        .filter((id, i, all) => all.indexOf(id) === i),
    [providers, rows],
  );
  const providerLabel = (id: string) =>
    providers?.find((provider) => provider.id === id)?.label ?? id;

  const update = (key: number, patch: Partial<ModelChoice>) =>
    setRows((current) =>
      current.map((row) => (row.key === key ? { ...row, ...patch } : row)),
    );
  const move = (index: number, by: -1 | 1) =>
    setRows((current) => {
      const copy = [...current];
      const [row] = copy.splice(index, 1);
      copy.splice(index + by, 0, row);
      return copy;
    });
  const add = () => {
    // The first configured provider other than the primary's, else the first
    // real one.
    const real = (providers ?? []).filter((p) => !isMockProvider(p.id));
    const provider =
      real.find((p) => p.configured && p.id !== primary.provider)?.id ??
      real[0]?.id ??
      providerIds[0] ??
      primary.provider;
    setRows((current) => [
      ...current,
      { key: next, provider, model: MODEL_SUGGESTIONS[provider]?.[0] ?? '' },
    ]);
    setNext((value) => value + 1);
  };

  const problems = rows.map((row, i) => problem(row, i, rows, primary));
  const choices = rows.map(({ provider, model }) => ({
    provider,
    model: model.trim(),
  }));
  const dirty = JSON.stringify(choices) !== savedKey;
  const canSave = dirty && problems.every((p) => p === null) && !busy;

  const submit = async () => {
    if (!canSave) return;
    setBusy(true);
    try {
      await save(choices);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="fallback-models" aria-labelledby="settings-fallbacks">
      <h4 id="settings-fallbacks" className="settings-group-title">
        If this model fails
      </h4>
      <p className="settings-group-help">
        Tried in order, only before the reply starts.
      </p>
      {rows.length > 0 && (
        <ol className="fallback-list">
          {rows.map((row, index) => {
            const n = index + 1;
            return (
              <li key={row.key} className="fallback-row">
                <span className="fallback-number" aria-hidden="true">
                  {n}
                </span>
                <select
                  aria-label={`Fallback ${n} provider`}
                  className="field"
                  value={row.provider}
                  disabled={disabled || busy}
                  onChange={(event) =>
                    update(row.key, {
                      provider: event.target.value,
                      model: MODEL_SUGGESTIONS[event.target.value]?.[0] ?? '',
                    })
                  }
                >
                  {providerIds.map((id) => (
                    <option key={id} value={id}>
                      {providerLabel(id)}
                    </option>
                  ))}
                </select>
                <input
                  aria-label={`Fallback ${n} model`}
                  className="field"
                  list={`fallback-models-${row.key}`}
                  value={row.model}
                  disabled={disabled || busy}
                  placeholder="model id"
                  onChange={(event) =>
                    update(row.key, { model: event.target.value })
                  }
                />
                <datalist id={`fallback-models-${row.key}`}>
                  {(MODEL_SUGGESTIONS[row.provider] ?? []).map((model) => (
                    <option key={model} value={model} />
                  ))}
                </datalist>
                <div className="fallback-actions">
                  <button
                    type="button"
                    className={ghostBtnCls}
                    aria-label={`Move fallback ${n} up`}
                    disabled={disabled || busy || index === 0}
                    onClick={() => move(index, -1)}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    className={ghostBtnCls}
                    aria-label={`Move fallback ${n} down`}
                    disabled={disabled || busy || index === rows.length - 1}
                    onClick={() => move(index, 1)}
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    className={ghostBtnCls}
                    aria-label={`Remove fallback ${n}`}
                    disabled={disabled || busy}
                    onClick={() =>
                      setRows((current) =>
                        current.filter((other) => other.key !== row.key),
                      )
                    }
                  >
                    Remove
                  </button>
                </div>
                {problems[index] ? (
                  <p role="alert" className="key-error fallback-problem">
                    {problems[index]}
                  </p>
                ) : null}
              </li>
            );
          })}
        </ol>
      )}
      <div className="fallback-footer">
        {rows.length < MAX_FALLBACKS && (
          <button
            type="button"
            className={ghostBtnCls}
            disabled={disabled || busy}
            onClick={add}
          >
            Add fallback
          </button>
        )}
        <button
          type="button"
          className={primaryBtnCls}
          disabled={disabled || !canSave}
          onClick={() => void submit()}
        >
          {busy ? 'Saving…' : 'Save fallbacks'}
        </button>
      </div>
    </section>
  );
}
