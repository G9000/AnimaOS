import { useEffect, useId, useRef, useState } from 'react';
import { MODEL_SUGGESTIONS } from '../lib/daemon-api';
import { BoltIcon, CheckIcon, ChevronIcon, ListChecksIcon } from './icons';

/** Plan or Do for what the owner sends next. */
export interface ComposerModeState {
  plan: boolean;
  /** Plan needs the plan tool, which comes with editing files. */
  canPlan: boolean;
  onChange(plan: boolean): void;
}

/** The model the companion thinks with, and a way to change it. */
export interface ComposerModelState {
  provider: string;
  current: string;
  busy?: boolean;
  choices?: readonly string[];
  moreLabel?: string;
  error?: string | null;
  onPick(model: string): void;
  /** Every other model lives in Settings. */
  onMore?(): void;
  /** How hard the chosen model thinks, when it offers a choice. */
  efforts?: readonly string[];
  /** null: the model's own default. */
  effort?: string | null;
  onEffort?(effort: string | null): void;
}

const EFFORT_LABELS: Record<string, string> = {
  none: 'Off',
  minimal: 'Min',
  low: 'Low',
  medium: 'Med',
  high: 'High',
  xhigh: 'XHigh',
  max: 'Max',
};

/**
 * The chip in the composer's controls row: "Do · model" or "Plan · model".
 * It opens a small menu to choose how the companion works (Do it, or Plan
 * first and wait for a go) and which model it uses.
 */
export function ComposerMode({
  mode,
  model,
}: {
  mode?: ComposerModeState;
  model?: ComposerModelState;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.stopPropagation();
      setOpen(false);
      trigger.current?.focus();
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', escape, true);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', escape, true);
    };
  }, [open]);

  if (!mode && !model) return null;

  const planning = mode?.plan === true && mode.canPlan;
  const models = model
    ? [
        ...new Set([
          model.current,
          ...(model.choices ?? MODEL_SUGGESTIONS[model.provider] ?? []),
        ]),
      ].filter(Boolean)
    : [];

  return (
    <div ref={root} className="composer-mode">
      <button
        ref={trigger}
        type="button"
        className="composer-chip"
        data-plan={planning || undefined}
        aria-label={`${mode ? (planning ? 'Plan' : 'Do') : 'Model'}, ${model ? model.current || model.provider : 'mode'}. Change how it works`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        {mode &&
          (planning ? <ListChecksIcon size={13} /> : <BoltIcon size={13} />)}
        {mode && <span>{planning ? 'Plan' : 'Do'}</span>}
        {mode && model && <i aria-hidden>·</i>}
        {model && (
          <span className="composer-chip-model">
            {model.current || model.provider}
          </span>
        )}
        <ChevronIcon size={12} />
      </button>
      {open && (
        <div id={menuId} role="menu" className="composer-menu">
          {mode && (
            <div role="group" aria-label="How it works">
              <span className="composer-menu-label">How it works</span>
              <button
                type="button"
                role="menuitemradio"
                aria-checked={!planning}
                className="composer-menu-item"
                onClick={() => {
                  mode.onChange(false);
                  setOpen(false);
                }}
              >
                <BoltIcon size={14} />
                <span>
                  <strong>Do</strong>
                  <small>Gets on with it</small>
                </span>
                {!planning && <CheckIcon size={13} />}
              </button>
              <button
                type="button"
                role="menuitemradio"
                aria-checked={planning}
                aria-disabled={!mode.canPlan || undefined}
                disabled={!mode.canPlan}
                className="composer-menu-item"
                onClick={() => {
                  mode.onChange(true);
                  setOpen(false);
                }}
              >
                <ListChecksIcon size={14} />
                <span>
                  <strong>Plan</strong>
                  <small>
                    {mode.canPlan
                      ? 'Shows the steps, waits for your go'
                      : 'Needs the Edit files level'}
                  </small>
                </span>
                {planning && <CheckIcon size={13} />}
              </button>
            </div>
          )}
          {model && (
            <div role="group" aria-label="Model">
              <span className="composer-menu-label">Model</span>
              {models.map((name) => (
                <button
                  key={name}
                  type="button"
                  role="menuitemradio"
                  aria-checked={name === model.current}
                  disabled={model.busy}
                  className="composer-menu-item composer-menu-item--model"
                  onClick={() => {
                    setOpen(false);
                    if (name !== model.current) model.onPick(name);
                  }}
                >
                  <span>
                    <strong>{name}</strong>
                  </span>
                  {name === model.current && <CheckIcon size={13} />}
                </button>
              ))}
              {model.onEffort && model.efforts && model.efforts.length > 0 && (
                <div
                  className="composer-thinking"
                  role="radiogroup"
                  aria-label="Thinking"
                >
                  <span>Thinking</span>
                  {[null, ...model.efforts].map((effort) => (
                    <button
                      key={effort ?? 'default'}
                      type="button"
                      role="radio"
                      aria-checked={(model.effort ?? null) === effort}
                      disabled={model.busy}
                      onClick={() => {
                        if ((model.effort ?? null) !== effort)
                          model.onEffort?.(effort);
                      }}
                    >
                      {effort === null ? 'Auto' : (EFFORT_LABELS[effort] ?? effort)}
                    </button>
                  ))}
                </div>
              )}
              {model.onMore && (
                <button
                  type="button"
                  role="menuitem"
                  className="composer-menu-item composer-menu-more"
                  onClick={() => {
                    setOpen(false);
                    model.onMore?.();
                  }}
                >
                  {model.moreLabel ?? 'All models and providers…'}
                  <span aria-hidden="true">→</span>
                </button>
              )}
              {model.error && (
                <p role="alert" className="composer-menu-error">
                  {model.error}
                </p>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
