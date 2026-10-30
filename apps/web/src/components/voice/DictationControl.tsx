import { useCallback, useEffect, useRef } from 'react';
import { useBrowserDictation } from '../../hooks/useBrowserDictation';
import { revealInvisible } from '../../lib/skills';
import { useVoiceSettings } from '../../lib/voice-settings';
import { MicIcon } from '../icons';

export interface DictationControlProps {
  /** An icon button for the composer bar; its status floats above the bar. */
  compact?: boolean;
  disabled?: boolean;
  conversationKey?: string | null;
  onTranscript(text: string): void;
  /** Words still being heard; the composer shows them in the input. */
  onInterim?(text: string): void;
  onPushToTalkStop?: () => void;
}
export function DictationControl({
  compact = false,
  disabled = false,
  conversationKey,
  onTranscript,
  onInterim,
  onPushToTalkStop,
}: DictationControlProps) {
  const { pushToTalk } = useVoiceSettings();
  const pushToTalkRef = useRef(false);
  const transcriptRef = useRef(false);
  const wasStopping = useRef(false);
  const dictation = useBrowserDictation((text) => {
    if (pushToTalk && pushToTalkRef.current) transcriptRef.current = true;
    onTranscript(text);
  }, {
    conversationKey,
    disabled,
  });
  const startDictation = dictation.start;
  const stopDictation = dictation.stop;
  const active = dictation.state !== 'idle';
  const beginPushToTalk = useCallback(() => {
    if (!pushToTalk || disabled || pushToTalkRef.current) return;
    transcriptRef.current = false;
    pushToTalkRef.current = true;
    startDictation();
  }, [pushToTalk, disabled, startDictation]);
  const finishPushToTalk = useCallback(() => {
    if (!pushToTalkRef.current) return;
    stopDictation();
  }, [stopDictation]);
  useEffect(() => {
    if (dictation.state === 'stopping') wasStopping.current = true;
    if (dictation.state !== 'idle' || !wasStopping.current) return;
    wasStopping.current = false;
    if (pushToTalkRef.current && transcriptRef.current)
      onPushToTalkStop?.();
    pushToTalkRef.current = false;
    transcriptRef.current = false;
  }, [dictation.state, onPushToTalkStop]);
  useEffect(() => {
    if (!pushToTalk || !compact) return;
    const inComposer = (target: EventTarget | null) =>
      target instanceof Element &&
      target.closest('[data-workspace-composer]') !== null;
    const down = (event: KeyboardEvent) => {
      if (
        event.code !== 'Space' ||
        event.repeat ||
        !inComposer(event.target) ||
        (event.target instanceof HTMLElement && event.target.closest('button'))
      )
        return;
      event.preventDefault();
      beginPushToTalk();
    };
    const up = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || !pushToTalkRef.current) return;
      event.preventDefault();
      finishPushToTalk();
    };
    document.addEventListener('keydown', down);
    document.addEventListener('keyup', up);
    return () => {
      document.removeEventListener('keydown', down);
      document.removeEventListener('keyup', up);
    };
  }, [pushToTalk, compact, disabled, beginPushToTalk, finishPushToTalk]);
  // The composer shows the words being heard in the input itself.
  const interim = active ? dictation.interim : '';
  useEffect(() => {
    onInterim?.(interim);
  }, [onInterim, interim]);
  if (!dictation.supported) return null;
  const label =
    pushToTalk && !active
      ? 'Hold to talk'
      : dictation.state === 'starting'
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
        onClick={pushToTalk ? (event) => event.preventDefault() : active ? dictation.stop : dictation.start}
        onPointerDown={pushToTalk ? (event) => {
          event.preventDefault();
          event.currentTarget.setPointerCapture(event.pointerId);
          beginPushToTalk();
        } : undefined}
        onPointerUp={pushToTalk ? finishPushToTalk : undefined}
        onPointerCancel={pushToTalk ? finishPushToTalk : undefined}
        onLostPointerCapture={pushToTalk ? finishPushToTalk : undefined}
        onKeyDown={pushToTalk ? (event) => {
          if ((event.code === 'Space' || event.key === ' ') && !event.repeat) {
            event.preventDefault();
            beginPushToTalk();
          }
        } : undefined}
        onKeyUp={pushToTalk ? (event) => {
          if (event.code === 'Space' || event.key === ' ') {
            event.preventDefault();
            finishPushToTalk();
          }
        } : undefined}
        className={`studio-dictation-button${compact ? ' composer-icon' : ''}`}
      >
        {compact ? <MicIcon size={16} /> : label}
      </button>
      {active && (!compact || pushToTalk) && (
        <div role="status" className="mt-2 text-xs text-ink-3">
          {pushToTalk
            ? 'Hold Space or the microphone to talk. Releasing sends the transcript.'
            : dictation.state === 'starting'
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
