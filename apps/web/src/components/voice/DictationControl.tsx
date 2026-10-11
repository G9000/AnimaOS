import { useBrowserDictation } from '../../hooks/useBrowserDictation';
import { revealInvisible } from '../../lib/skills';

export interface DictationControlProps {
  disabled?: boolean;
  conversationKey?: string | null;
  onTranscript(text: string): void;
}
export function DictationControl({
  disabled = false,
  conversationKey,
  onTranscript,
}: DictationControlProps) {
  const dictation = useBrowserDictation(onTranscript, {
    conversationKey,
    disabled,
  });
  if (!dictation.supported) return null;
  const active = dictation.state !== 'idle';
  return (
    <div className="studio-dictation-control">
      <button
        type="button"
        disabled={disabled || dictation.state === 'stopping'}
        aria-pressed={active}
        onClick={active ? dictation.stop : dictation.start}
        className="studio-dictation-button"
      >
        {dictation.state === 'starting'
          ? 'Cancel dictation'
          : dictation.state === 'stopping'
            ? 'Finishing dictation…'
            : active
              ? 'Stop dictation'
              : 'Dictate message'}
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
