import { useState, type ReactNode } from 'react';
import type { ChatResult, ChatResultPayload } from '@animaos/sdk';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import { validTracker } from './TrackerResult';

export function InteractiveResultEditor({
  result,
  onSave,
  children,
}: {
  result: ChatResult;
  onSave?: (
    result: ChatResult,
    payload: ChatResultPayload,
  ) => Promise<ChatResult>;
  children: (
    payload: ChatResultPayload,
    change: ((payload: ChatResultPayload) => void) | undefined,
    disabled: boolean,
  ) => ReactNode;
}) {
  const [draft, setDraft] = useState(result.payload);
  const [base, setBase] = useState(result);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const stale = base.revision !== result.revision;
  const editable =
    Boolean(onSave) &&
    (draft.kind === 'tracker' ||
      (draft.kind === 'dashboard' &&
        draft.widgets.some((widget) => widget.payload.kind === 'tracker')));
  useResetOnChange([result.revision, saving, dirty], () => {
    if (!dirty && !saving && base.revision < result.revision) {
      setBase(result);
      setDraft(result.payload);
      setMessage(null);
    }
  });
  const valid =
    draft.kind === 'tracker'
      ? validTracker(draft)
      : draft.kind === 'dashboard'
        ? draft.widgets.every(
            (widget) =>
              widget.payload.kind !== 'tracker' || validTracker(widget.payload),
          )
        : true;

  function reset() {
    setBase(result);
    setDraft(result.payload);
    setDirty(false);
    setMessage(null);
  }
  async function save() {
    if (!onSave || saving || stale || !dirty || !valid) return;
    setSaving(true);
    setMessage(null);
    try {
      const saved = await onSave(base, draft);
      setBase(saved);
      setDraft(saved.payload);
      setDirty(false);
      setMessage('Changes saved.');
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : 'Changes could not be saved.',
      );
    } finally {
      setSaving(false);
    }
  }
  return (
    <div className="space-y-3">
      {stale && dirty && (
        <div
          role="alert"
          className="rounded border border-line p-3 text-sm text-ink-2"
        >
          A newer version is available. Your draft is kept here; reload the
          saved version before editing it.
          <button
            type="button"
            className="mt-2 block underline"
            disabled={saving}
            onClick={reset}
          >
            Reload saved version
          </button>
        </div>
      )}
      {children(
        draft,
        editable
          ? (payload) => {
              setDraft(payload);
              setDirty(true);
              setMessage(null);
            }
          : undefined,
        saving || stale,
      )}
      {editable && (
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={!dirty || !valid || saving || stale}
            onClick={() => void save()}
            className="rounded border border-line px-3 py-1.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Saving…' : 'Save changes'}
          </button>
          <button
            type="button"
            disabled={!dirty || saving}
            onClick={reset}
            className="text-xs text-ink-2 underline disabled:opacity-50"
          >
            Discard changes
          </button>
          {!valid && (
            <p className="text-xs text-ink-2">
              Enter valid numbers and keep each text cell within 2,048 bytes.
            </p>
          )}
          {dirty && !message && (
            <span className="text-xs text-ink-3">Unsaved changes</span>
          )}
        </div>
      )}
      {message && (
        <p role="status" className="text-xs text-ink-2">
          {message}
        </p>
      )}
    </div>
  );
}
