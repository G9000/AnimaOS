import { useEffect, useState } from 'react';
import { useLatest } from '../../hooks/useLatest';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import type {
  ChatModels,
  ChatModelPreference,
  ChatModelPreferences,
  ChatModelPreferencesView,
  ChatReasoningEffort,
  ResolvedChatModel,
} from '@animaos/sdk';

type Api = Pick<ChatModels, 'get' | 'update'>;
const key = (choice: Pick<ResolvedChatModel, 'provider' | 'model'>) =>
  `${choice.provider}:${choice.model}`;
const selectionKey = (choice: ChatModelPreference) =>
  choice.mode === 'inherit' ? '' : key(choice);

/** Conversation choices apply when the daemon starts the next turn. */
export function ChatModelControls({
  agentId,
  sessionId,
  api,
  onSaved,
  clearGeneration,
}: {
  agentId: string;
  sessionId: string;
  api: Api;
  clearGeneration?: number;
  onSaved?: (view: ChatModelPreferencesView) => void;
}) {
  const [view, setView] = useState<ChatModelPreferencesView | null>(null);
  const [draft, setDraft] = useState<ChatModelPreferences | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scope = `${agentId}\0${sessionId}\0${clearGeneration ?? ''}`;
  const current = useLatest(scope);
  useResetOnChange([agentId, sessionId, api, clearGeneration], () => {
    setView(null);
    setDraft(null);
    setError(null);
    setBusy(false);
  });
  useEffect(() => {
    const controller = new AbortController();
    let live = true;
    void api.get(agentId, sessionId, { signal: controller.signal }).then(
      (value) => {
        if (live) {
          setView(value);
          setDraft(value.preferences);
        }
      },
      (caught: unknown) => {
        if (live)
          setError(caught instanceof Error ? caught.message : String(caught));
      },
    );
    return () => {
      live = false;
      controller.abort();
    };
  }, [agentId, sessionId, api, clearGeneration]);

  if (!view || !draft)
    return (
      <section aria-label="Chat model settings">
        <p role={error ? 'alert' : 'status'}>
          {error ?? 'Loading model settings…'}
        </p>
      </section>
    );

  const choose = (target: keyof ChatModelPreferences, selected: string) => {
    const choice = view.availableModels.find(
      (model) => key(model) === selected,
    );
    setDraft({
      ...draft,
      [target]: choice
        ? { mode: 'override', provider: choice.provider, model: choice.model }
        : { mode: 'inherit' },
    });
  };
  const editor = (
    target: keyof ChatModelPreferences,
    label: string,
    inherited: string,
    resolved: ResolvedChatModel | null,
  ) => {
    const value = draft[target];
    const selected = value.mode === 'override' ? value : resolved;
    const options = view.availableModels.slice();
    if (selected && !options.some((option) => key(option) === key(selected)))
      options.push({ ...selected, reasoningEfforts: [] });
    const efforts = selected
      ? (options.find((option) => key(option) === key(selected))
          ?.reasoningEfforts ?? [])
      : [];
    return (
      <fieldset disabled={busy}>
        <legend>{label}</legend>
        <label>
          {label} model
          <select
            className="field"
            aria-label={target === 'primary' ? 'Chat model' : 'Reviewer model'}
            value={selectionKey(value)}
            onChange={(event) => choose(target, event.target.value)}
          >
            <option value="">{inherited}</option>
            {options.map((choice) => (
              <option key={key(choice)} value={key(choice)}>
                {choice.provider} / {choice.model}
              </option>
            ))}
          </select>
        </label>
        {efforts.length > 0 && selected && (
          <label>
            Reasoning effort
            <select
              className="field"
              aria-label={
                target === 'primary'
                  ? 'Chat reasoning effort'
                  : 'Reviewer reasoning effort'
              }
              value={
                value.mode === 'override' ? (value.reasoningEffort ?? '') : ''
              }
              onChange={(event) => {
                const reasoningEffort = event.target
                  .value as ChatReasoningEffort;
                setDraft({
                  ...draft,
                  [target]: {
                    mode: 'override',
                    provider: selected.provider,
                    model: selected.model,
                    ...(reasoningEffort ? { reasoningEffort } : {}),
                  },
                });
              }}
            >
              <option value="">
                {value.mode === 'inherit'
                  ? 'Inherit default reasoning'
                  : 'Provider default'}
              </option>
              {efforts.map((effort) => (
                <option key={effort} value={effort}>
                  {effort}
                </option>
              ))}
            </select>
          </label>
        )}
      </fieldset>
    );
  };
  const changed = (target: keyof ChatModelPreferences) =>
    JSON.stringify(draft[target]) !== JSON.stringify(view.preferences[target]);
  const save = async () => {
    if (busy || (!changed('primary') && !changed('reviewer'))) return;
    setBusy(true);
    setError(null);
    const patch = {
      ...(changed('primary') ? { primary: draft.primary } : {}),
      ...(changed('reviewer') ? { reviewer: draft.reviewer } : {}),
      clearGeneration: view.clearGeneration,
    };
    try {
      const result = await api.update(agentId, sessionId, patch);
      if (current.current === scope) {
        setView(result);
        setDraft(result.preferences);
        onSaved?.(result);
      }
    } catch (caught) {
      if (current.current === scope)
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (current.current === scope) setBusy(false);
    }
  };
  return (
    <section aria-label="Chat model settings">
      <p>
        Saved changes apply to the next turn. A reply already running keeps its
        model.
      </p>
      <p>
        Current chat choice: {view.primary.provider} / {view.primary.model}
        {view.primary.reasoningEffort
          ? ` · ${view.primary.reasoningEffort}`
          : ''}
      </p>
      {editor('primary', 'Chat', 'Use agent default', view.primary)}
      {editor(
        'reviewer',
        'Reviewer',
        'Use a different side-task model',
        view.reviewer,
      )}
      {view.reviewerUnavailableReason && (
        <p role="status">{view.reviewerUnavailableReason}</p>
      )}
      <button
        type="button"
        disabled={busy || (!changed('primary') && !changed('reviewer'))}
        onClick={() => void save()}
      >
        {busy ? 'Saving…' : 'Save model settings'}
      </button>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
