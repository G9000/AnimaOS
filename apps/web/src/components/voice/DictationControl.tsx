import { useEffect } from 'react';
import { useBrowserDictation } from '../../hooks/useBrowserDictation';
import { setPresenceListening } from '../../lib/presence';
import { revealInvisible } from '../../lib/skills';
import { MicIcon } from '../icons';

export interface DictationControlProps {
  /** An icon button for the composer bar; its status floats above the bar. */
  compact?: boolean;
  disabled?: boolean;
  conversationKey?: string | null;
  onTranscript(text: string): void;
}
export function DictationControl({
  compact = false,
  disabled = false,
  conversationKey,
  onTranscript,
}: DictationControlProps) {
  const dictation = useBrowserDictation(onTranscript, {
    conversationKey,
    disabled,
  });
  const active = dictation.state !== 'idle';
  // The composer's microphone drives the orb: it shows the words as heard.
  useEffect(() => {
    if (!compact) return;
    setPresenceListening(active ? dictation.interim : null);
  }, [compact, active, dictation.interim]);
  useEffect(
    () => () => {
      if (compact) setPresenceListening(null);
    },
    [compact],
  );
  if (!dictation.supported) return null;
  const label =
    dictation.state === 'starting'
      ? 'Cancel dictation'
      : dictation.state === 'stopping'
        ? 'Finishing dictation…'
        : active
          ? 'Stop dictation'
          : 'Dictate message';
  return (
    <div className={`studio-dictation-control${compact ? ' is-compact' : ''}`}>
      <button
        type="button"
        disabled={disabled || dictation.state === 'stopping'}
        aria-pressed={active}
        aria-label={compact ? label : undefined}
        title={compact ? label : undefined}
        onClick={active ? dictation.stop : dictation.start}
        className={`studio-dictation-button${compact ? ' composer-icon' : ''}`}
      >
        {compact ? <MicIcon size={16} /> : label}
      </button>
      {active && (
        <div role="status" className="mt-2 text-xs text-ink-3">
          {dictation.state === 'starting'
            ? 'Waiting for microphone access…'
            : dictation.state === 'stopping'
              ? 'Finishing the last words…'
              : 'Listening…'}
          {dictation.interim && (
            <p className="break-words">
              {revealInvisible(dictation.interim).text}
            </p>
          )}
        </div>
      )}
      {dictation.error && (
        <p role="alert" className="mt-2 text-xs text-danger">
          {dictation.error}
        </p>
      )}
    </div>
  );
}
