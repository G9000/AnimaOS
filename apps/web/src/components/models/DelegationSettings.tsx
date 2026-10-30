import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useEffect, useId, useRef, useState } from 'react';
import type { DelegationPolicy, ModelChoice } from '@animaos/sdk';

import {
  isMockProvider,
  MODEL_SUGGESTIONS,
  type DaemonProvider,
} from '../../lib/daemon-api';
import { ghostBtnCls, primaryBtnCls } from '../ui-bits';
import {
  SettingsCard,
  SettingsEmpty,
  SettingsRow,
  SettingsSection,
} from '../settings/SettingsKit';

type LimitName =
  | 'maxConcurrent'
  | 'maxStartsPerRun'
  | 'maxToolIterations'
  | 'timeoutMs'
  | 'maxOutputTokens';
const LIMITS: {
  name: LimitName;
  label: string;
  hint: string;
  initial: number;
  min: number;
  max: number;
  scale: number;
}[] = [
  {
    name: 'maxConcurrent',
    label: 'Concurrent helpers',
    hint: 'How many helpers can work at the same time.',
    initial: 4,
    min: 1,
    max: 8,
    scale: 1,
  },
  {
    name: 'maxStartsPerRun',
    label: 'Helpers per run',
    hint: 'The most one reply can bring in.',
    initial: 4,
    min: 1,
    max: 16,
    scale: 1,
  },
  {
    name: 'maxToolIterations',
    label: 'Tool turns per helper',
    hint: 'Steps a helper can take before it stops.',
    initial: 8,
    min: 1,
    max: 32,
    scale: 1,
  },
  {
    name: 'timeoutMs',
    label: 'Time limit (seconds)',
    hint: 'How long one helper can keep working.',
    initial: 120000,
    min: 1000,
    max: 600000,
    scale: 1000,
  },
  {
    name: 'maxOutputTokens',
    label: 'Output tokens per reply',
    hint: 'The longest answer a helper can write.',
    initial: 4096,
    min: 1,
    max: 16384,
    scale: 1,
  },
];

interface RouteDraft extends ModelChoice {
  key: string;
  id: string;
  fallbacks: (ModelChoice & { key: string })[];
}
interface Draft {
  limits: Record<LimitName, string>;
  routes: RouteDraft[];
}

function normalize(policy?: DelegationPolicy): DelegationPolicy {
  return {
    ...Object.fromEntries(
      LIMITS.map((limit) => [
        limit.name,
        policy?.[limit.name] ?? limit.initial,
      ]),
    ),
    maxDepth: 1,
    allowContextFork: false,
    routes: (policy?.routes ?? []).map((route) => ({
      id: route.id,
      provider: route.provider,
      model: route.model.trim(),
      fallbacks: (route.fallbacks ?? []).map((choice) => ({
        provider: choice.provider,
        model: choice.model.trim(),
      })),
    })),
  };
}

function draftFrom(policy: DelegationPolicy): Draft {
  return {
    limits: Object.fromEntries(
      LIMITS.map((limit) => [
        limit.name,
        String((policy[limit.name] ?? limit.initial) / limit.scale),
      ]),
    ) as Record<LimitName, string>,
    routes: (policy.routes ?? []).map((route, i) => ({
      ...route,
      key: 'route-' + i,
      fallbacks: (route.fallbacks ?? []).map((choice, n) => ({
        ...choice,
        key: 'fallback-' + n,
      })),
    })),
  };
}

function policyFrom(draft: Draft): DelegationPolicy {
  return {
    ...Object.fromEntries(
      LIMITS.map((limit) => [
        limit.name,
        limit.scale === 1
          ? Number(draft.limits[limit.name])
          : Math.round(Number(draft.limits[limit.name]) * limit.scale),
      ]),
    ),
    maxDepth: 1,
    allowContextFork: false,
    routes: draft.routes.map((route) => ({
      id: route.id,
      provider: route.provider,
      model: route.model.trim(),
      fallbacks: route.fallbacks.map((choice) => ({
        provider: choice.provider,
        model: choice.model.trim(),
      })),
    })),
  };
}

function problemsFor(draft: Draft): string[] {
  const problems: string[] = [];
  for (const limit of LIMITS) {
    const value = Number(draft.limits[limit.name]) * limit.scale;
    if (
      !draft.limits[limit.name].trim() ||
      !Number.isFinite(value) ||
      Math.abs(value - Math.round(value)) > 0.0000001 ||
      value < limit.min ||
      value > limit.max
    ) {
      problems.push(
        limit.label +
          ' must be ' +
          (limit.scale === 1 ? 'a whole number ' : '') +
          'from ' +
          limit.min / limit.scale +
          ' to ' +
          limit.max / limit.scale +
          '.',
      );
    }
  }
  if (draft.routes.length > 8) problems.push('Keep at most eight routes.');
  const ids = new Set<string>();
  draft.routes.forEach((route, i) => {
    const label = 'Route ' + (i + 1);
    if (!/^[a-z][a-z0-9_-]{0,31}$/.test(route.id))
      problems.push(
        label +
          ' needs a name starting with a lowercase letter, using up to 32 lowercase letters, numbers, underscores or hyphens.',
      );
    if (ids.has(route.id)) problems.push('Route names must be unique.');
    ids.add(route.id);
    if (!route.provider || !route.model.trim())
      problems.push(label + ' needs a provider and model.');
    if (route.fallbacks.length > 3)
      problems.push(label + ' can have at most three fallbacks.');
    const choices = new Set([
      JSON.stringify([route.provider, route.model.trim()]),
    ]);
    route.fallbacks.forEach((choice, n) => {
      if (!choice.provider || !choice.model.trim())
        problems.push(
          label + ' fallback ' + (n + 1) + ' needs a provider and model.',
        );
      const key = JSON.stringify([choice.provider, choice.model.trim()]);
      if (choices.has(key))
        problems.push(
          label + ' must use different models for its primary and fallbacks.',
        );
      choices.add(key);
    });
  });
  return problems;
}

function ModelPicker({
  label,
  choice,
  providers,
  onChange,
}: {
  label: string;
  choice: ModelChoice;
  providers: readonly DaemonProvider[] | null;
  onChange: (choice: ModelChoice) => void;
}) {
  const models = MODEL_SUGGESTIONS[choice.provider] ?? [];
  const [custom, setCustom] = useState(!models.includes(choice.model));
  useResetOnChange([choice.provider, choice.model], () => {
    if (!models.includes(choice.model)) setCustom(true);
  });
  const providerIds = [
    ...(providers ?? []).filter((p) => !isMockProvider(p.id)).map((p) => p.id),
    choice.provider,
  ].filter((id, i, all) => id && all.indexOf(id) === i);
  return (
    <>
      <label className="helper-field">
        <span>{label} provider</span>
        <select
          aria-label={label + ' provider'}
          className="field"
          value={choice.provider}
          onChange={(event) =>
            onChange({
              provider: event.target.value,
              model: MODEL_SUGGESTIONS[event.target.value]?.[0] ?? '',
            })
          }
        >
          {providerIds.map((id) => (
            <option key={id} value={id}>
              {providers?.find((p) => p.id === id)?.label ?? id}
            </option>
          ))}
        </select>
      </label>
      <label className="helper-field">
        <span>{label} model</span>
        <select
          aria-label={label + ' model'}
          className="field"
          value={custom ? '__custom__' : choice.model}
          onChange={(event) => {
            const isCustom = event.target.value === '__custom__';
            setCustom(isCustom);
            if (!isCustom)
              onChange({
                provider: choice.provider,
                model: event.target.value,
              });
          }}
        >
          {models.map((model) => (
            <option key={model} value={model}>
              {model}
            </option>
          ))}
          <option value="__custom__">custom…</option>
        </select>
      </label>
      {custom && (
        <label className="helper-field helper-field-wide">
          <span>{label} custom model</span>
          <input
            aria-label={label + ' custom model'}
            className="field"
            value={choice.model}
            placeholder="model id"
            onChange={(event) =>
              onChange({ ...choice, model: event.target.value })
            }
          />
        </label>
      )}
    </>
  );
}

export function DelegationSettings({
  primary,
  policy,
  providers,
  disabled = false,
  save,
}: {
  primary: ModelChoice;
  policy?: DelegationPolicy;
  providers: readonly DaemonProvider[] | null;
  disabled?: boolean;
  save: (policy: DelegationPolicy) => Promise<boolean>;
}) {
  const headingId = useId();
  const savedKey = JSON.stringify(normalize(policy));
  const [baseline, setBaseline] = useState(savedKey);
  const [draft, setDraft] = useState(() => draftFrom(normalize(policy)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const nextKey = useRef(0);
  const generation = useRef(0);
  const pending = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current += 1;
    };
  }, []);
  useResetOnChange([savedKey], () => {
    setDraft(draftFrom(JSON.parse(savedKey) as DelegationPolicy));
    setBaseline(savedKey);
    setBusy(false);
    setError(null);
    setSaved(false);
  });
  useEffect(() => {
    generation.current += 1;
    pending.current = false;
  }, [savedKey]);

  const update = (change: (current: Draft) => Draft) => {
    setDraft(change);
    setError(null);
    setSaved(false);
  };
  const updateRoute = (
    key: string,
    change: (route: RouteDraft) => RouteDraft,
  ) =>
    update((current) => ({
      ...current,
      routes: current.routes.map((route) =>
        route.key === key ? change(route) : route,
      ),
    }));
  const realProviders = (providers ?? []).filter((p) => !isMockProvider(p.id));
  const newChoice = (main: ModelChoice, fallback: boolean): ModelChoice => {
    const provider = fallback
      ? (realProviders.find((p) => p.configured && p.id !== main.provider)
          ?.id ??
        realProviders[0]?.id ??
        main.provider)
      : !isMockProvider(main.provider)
        ? main.provider
        : (realProviders.find((p) => p.configured)?.id ??
          realProviders[0]?.id ??
          main.provider);
    return {
      provider,
      model:
        !fallback && provider === main.provider
          ? main.model
          : (MODEL_SUGGESTIONS[provider]?.[0] ?? ''),
    };
  };
  const next = policyFrom(draft);
  const problems = problemsFor(draft);
  const dirty = JSON.stringify(next) !== baseline;
  const canSave = dirty && !disabled && !busy && problems.length === 0;
  const submit = async () => {
    if (!canSave || pending.current) return;
    const currentGeneration = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const accepted = await save(next);
      if (!mounted.current || currentGeneration !== generation.current) return;
      if (accepted) {
        setBaseline(JSON.stringify(next));
        setSaved(true);
      } else
        setError(
          'Helper settings could not be saved. Your changes are still here.',
        );
    } catch {
      if (mounted.current && currentGeneration === generation.current)
        setError(
          'Helper settings could not be saved. Your changes are still here.',
        );
    } finally {
      if (mounted.current && currentGeneration === generation.current) {
        pending.current = false;
        setBusy(false);
      }
    }
  };

  const addRoute = () => {
    let id = 'fast';
    for (let n = 2; draft.routes.some((route) => route.id === id); n += 1)
      id = 'route-' + n;
    const route: RouteDraft = {
      ...newChoice(primary, false),
      id,
      key: 'new-route-' + nextKey.current++,
      fallbacks: [],
    };
    update((current) => ({ ...current, routes: [...current.routes, route] }));
  };

  return (
    <fieldset
      disabled={disabled || busy}
      className="helper-settings sk-fieldset"
    >
      <legend className="sr-only">Helper limits and routes</legend>
      <SettingsSection
        id={headingId}
        title="Helpers"
        description="Helpers take on focused tasks with their own playbook, without the main personality. These limits keep them in check."
      >
        <SettingsCard>
          {LIMITS.map((limit) => (
            <SettingsRow
              key={limit.name}
              title={limit.label}
              description={limit.hint}
              control={
                <input
                  aria-label={limit.label}
                  className="field"
                  type="number"
                  min={limit.min / limit.scale}
                  max={limit.max / limit.scale}
                  step={limit.scale === 1 ? 1 : 0.001}
                  value={draft.limits[limit.name]}
                  onChange={(event) =>
                    update((current) => ({
                      ...current,
                      limits: {
                        ...current.limits,
                        [limit.name]: event.target.value,
                      },
                    }))
                  }
                />
              }
            />
          ))}
        </SettingsCard>
      </SettingsSection>
      <SettingsSection
        title="Routes"
        description="Name a route, such as fast or research, to give those tasks their own model. Without one, helpers use the current model."
        action={
          draft.routes.length < 8 && (
            <button type="button" className={ghostBtnCls} onClick={addRoute}>
              Add route
            </button>
          )
        }
      >
        <SettingsCard>
          {draft.routes.length === 0 && (
            <SettingsEmpty>No routes yet.</SettingsEmpty>
          )}
          {draft.routes.map((route, i) => {
            const label = 'Route ' + (i + 1);
            return (
              <div key={route.key} className="sk-row helper-route">
                <div className="helper-route-grid">
                  <label className="helper-field">
                    <span>{label} name</span>
                    <input
                      aria-label={label + ' name'}
                      className="field"
                      value={route.id}
                      maxLength={32}
                      placeholder="fast or research"
                      onChange={(event) =>
                        updateRoute(route.key, (current) => ({
                          ...current,
                          id: event.target.value,
                        }))
                      }
                    />
                  </label>
                  <ModelPicker
                    key={route.key + route.provider}
                    label={label}
                    choice={route}
                    providers={providers}
                    onChange={(choice) =>
                      updateRoute(route.key, (current) => ({
                        ...current,
                        ...choice,
                      }))
                    }
                  />
                </div>
                {route.fallbacks.map((fallback, n) => (
                  <div key={fallback.key} className="helper-route-fallback">
                    <div className="helper-route-grid">
                      <ModelPicker
                        key={fallback.key + fallback.provider}
                        label={label + ' fallback ' + (n + 1)}
                        choice={fallback}
                        providers={providers}
                        onChange={(choice) =>
                          updateRoute(route.key, (current) => ({
                            ...current,
                            fallbacks: current.fallbacks.map((item) =>
                              item.key === fallback.key
                                ? { ...item, ...choice }
                                : item,
                            ),
                          }))
                        }
                      />
                    </div>
                    <div className="fallback-actions">
                      {([-1, 1] as const).map((by) => (
                        <button
                          key={by}
                          type="button"
                          className={ghostBtnCls}
                          aria-label={
                            'Move ' +
                            label.toLowerCase() +
                            ' fallback ' +
                            (n + 1) +
                            (by === -1 ? ' up' : ' down')
                          }
                          disabled={
                            by === -1
                              ? n === 0
                              : n === route.fallbacks.length - 1
                          }
                          onClick={() =>
                            updateRoute(route.key, (current) => {
                              const fallbacks = [...current.fallbacks];
                              const [moved] = fallbacks.splice(n, 1);
                              fallbacks.splice(n + by, 0, moved);
                              return { ...current, fallbacks };
                            })
                          }
                        >
                          {by === -1 ? '↑' : '↓'}
                        </button>
                      ))}
                      <button
                        type="button"
                        className={ghostBtnCls}
                        aria-label={
                          'Remove ' +
                          label.toLowerCase() +
                          ' fallback ' +
                          (n + 1)
                        }
                        onClick={() =>
                          updateRoute(route.key, (current) => ({
                            ...current,
                            fallbacks: current.fallbacks.filter(
                              (item) => item.key !== fallback.key,
                            ),
                          }))
                        }
                      >
                        Remove fallback
                      </button>
                    </div>
                  </div>
                ))}
                <div className="helper-route-actions">
                  {route.fallbacks.length < 3 && (
                    <button
                      type="button"
                      className={ghostBtnCls}
                      aria-label={'Add ' + label.toLowerCase() + ' fallback'}
                      onClick={() => {
                        const fallback = {
                          ...newChoice(route, true),
                          key: 'new-fallback-' + nextKey.current++,
                        };
                        updateRoute(route.key, (current) => ({
                          ...current,
                          fallbacks: [...current.fallbacks, fallback],
                        }));
                      }}
                    >
                      Add fallback
                    </button>
                  )}
                  <button
                    type="button"
                    className={ghostBtnCls}
                    aria-label={'Remove ' + label.toLowerCase()}
                    onClick={() =>
                      update((current) => ({
                        ...current,
                        routes: current.routes.filter(
                          (item) => item.key !== route.key,
                        ),
                      }))
                    }
                  >
                    Remove route
                  </button>
                </div>
              </div>
            );
          })}
        </SettingsCard>
      </SettingsSection>
      {problems.length > 0 && (
        <div role="alert" className="key-error">
          <ul>
            {problems.map((problem, i) => (
              <li key={i}>{problem}</li>
            ))}
          </ul>
        </div>
      )}
      {error && (
        <p role="alert" className="key-error">
          {error}
        </p>
      )}
      {saved && <p role="status">Helper settings saved.</p>}
      <div className="model-actions">
        <button
          type="button"
          className={ghostBtnCls}
          disabled={disabled || busy || !dirty}
          onClick={() => {
            setDraft(draftFrom(JSON.parse(baseline) as DelegationPolicy));
            setError(null);
            setSaved(false);
          }}
        >
          Discard helper changes
        </button>
        <button
          type="button"
          className={primaryBtnCls}
          disabled={!canSave}
          onClick={() => void submit()}
        >
          {busy ? 'Saving helper settings…' : 'Save helper settings'}
        </button>
      </div>
    </fieldset>
  );
}
