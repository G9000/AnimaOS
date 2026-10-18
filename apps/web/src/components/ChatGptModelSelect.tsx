import type { RefObject } from 'react';
import { useChatGptModels } from '../hooks/useChatGptModels';
import { labelCls } from './ui-bits';

export function ChatGptModelSelect({
  id,
  scopeKey,
  connectionPending = false,
  model,
  customModel,
  onModelChange,
  onCustomModelChange,
  disabled = false,
  modelSelectRef,
  customModelInputRef,
  customModelValidationErrorId,
}: {
  id: string;
  scopeKey: string;
  connectionPending?: boolean;
  model: string;
  customModel: string;
  onModelChange(model: string): void;
  onCustomModelChange(model: string): void;
  disabled?: boolean;
  modelSelectRef?: RefObject<HTMLSelectElement | null>;
  customModelInputRef?: RefObject<HTMLInputElement | null>;
  customModelValidationErrorId?: string;
}) {
  const metadata = useChatGptModels(scopeKey, !connectionPending && !disabled);
  const snapshot = metadata.snapshot;
  const entries = snapshot?.models ?? [];
  const listed = entries.some((entry) => entry.id === model);
  const fresh = snapshot?.state === 'ready';
  const status = metadata.busy
    ? 'Reading ChatGPT model metadata…'
    : snapshot?.state === 'ready'
      ? 'Received model metadata. A listed model may still be unavailable for generation.'
      : snapshot?.state === 'empty'
        ? 'No visible models were returned. Custom model availability is unverified.'
        : snapshot?.state === 'stale'
          ? 'Model metadata is stale. Availability is unverified.'
          : snapshot?.state === 'disconnected'
            ? 'Connect ChatGPT to refresh model metadata. Custom model availability is unverified.'
            : snapshot?.state === 'unrequested'
              ? 'Model metadata has not been refreshed. Choose Refresh models to request it.'
              : 'Model metadata is unavailable. Custom model availability is unverified.';
  return (
    <div className="space-y-3">
      <div>
        <label htmlFor={id} className={labelCls}>
          Model
        </label>
        <select
          id={id}
          ref={modelSelectRef}
          className="field"
          value={model}
          disabled={disabled}
          aria-describedby={`${id}-metadata`}
          onChange={(event) => onModelChange(event.target.value)}
        >
          <option value="" disabled>
            Choose a model
          </option>
          {model && model !== '__custom__' && !listed && (
            <option value={model}>{model} (unverified)</option>
          )}
          {entries.map(({ id: value }) => (
            <option key={value} value={value}>
              {value}
              {fresh ? '' : ' (unverified)'}
            </option>
          ))}
          <option value="__custom__">Custom model</option>
        </select>
      </div>
      {model === '__custom__' && (
        <div>
          <label htmlFor={`${id}-custom`} className={labelCls}>
            Custom model
          </label>
          <input
            id={`${id}-custom`}
            ref={customModelInputRef}
            className="field"
            value={customModel}
            disabled={disabled}
            onChange={(event) => onCustomModelChange(event.target.value)}
            placeholder="Provider model identifier"
            required
            aria-invalid={Boolean(customModelValidationErrorId)}
            aria-describedby={customModelValidationErrorId}
          />
          <p className="text-xs text-ink-3">
            Custom model availability is unverified.
          </p>
        </div>
      )}
      <p id={`${id}-metadata`} role="status" className="text-xs text-ink-3">
        {status}
      </p>
      {model && model !== '__custom__' && (!listed || !fresh) && (
        <p className="text-xs text-ink-3">
          Your selected model is kept; availability is unverified.
        </p>
      )}
      {metadata.error && (
        <p role="alert" className="text-xs text-danger">
          {metadata.error}
        </p>
      )}
      <button
        type="button"
        className="rounded-xl border border-line px-3 py-2 text-sm disabled:opacity-50"
        disabled={disabled || connectionPending || metadata.busy}
        // While the picker believes ChatGPT is disconnected, the button
        // re-reads instead: the account may have been connected elsewhere.
        onClick={() =>
          void (snapshot?.state === 'disconnected'
            ? metadata.reload()
            : metadata.refresh())
        }
      >
        Refresh models
      </button>
      <p className="text-xs text-ink-3">
        Refresh contacts ChatGPT for model metadata without generating a reply.
      </p>
    </div>
  );
}
