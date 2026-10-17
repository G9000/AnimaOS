import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { revealInvisible } from '../lib/skills';
import { preferredVoice } from '../lib/voice-preference';

export const READ_ALOUD_ERROR =
  'Read aloud could not continue. Try again when you’re ready.';

export interface ReadAloudController {
  supported: boolean;
  disabled?: boolean;
  speakingMessageId: string | null;
  error: string | null;
  read(messageId: string, text: string): void;
  stop(): void;
}

function browserSpeech() {
  if (
    typeof window === 'undefined' ||
    typeof window.speechSynthesis?.speak !== 'function' ||
    typeof window.speechSynthesis?.cancel !== 'function' ||
    typeof window.SpeechSynthesisUtterance !== 'function'
  )
    return null;
  return {
    synthesis: window.speechSynthesis,
    Constructor: window.SpeechSynthesisUtterance,
  };
}

function detach(utterance: SpeechSynthesisUtterance) {
  utterance.onstart = null;
  utterance.onend = null;
  utterance.onerror = null;
}

export function useReadAloud(
  conversationKey?: string | null,
  disabled = false,
): ReadAloudController {
  const [speakingMessageId, setSpeakingMessageId] = useState<string | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  const boundary = useRef({ conversationKey, disabled, generation: 0 });
  if (
    boundary.current.conversationKey !== conversationKey ||
    boundary.current.disabled !== disabled
  ) {
    boundary.current = {
      conversationKey,
      disabled,
      generation: boundary.current.generation + 1,
    };
  }
  const generation = boundary.current.generation;
  const current = useRef<{
    utterance: SpeechSynthesisUtterance;
    synthesis: SpeechSynthesis;
    messageId: string;
  } | null>(null);

  const release = useCallback((cancel: boolean) => {
    const owned = current.current;
    current.current = null;
    if (!owned) return false;
    detach(owned.utterance);
    if (cancel) {
      try {
        owned.synthesis.cancel();
      } catch {
        /* Browser already stopped. */
      }
    }
    return true;
  }, []);

  useEffect(() => {
    mounted.current = true;
    setSpeakingMessageId(null);
    setError(null);
    return () => {
      mounted.current = false;
      release(true);
    };
  }, [conversationKey, disabled, release]);

  const activeBoundary = useCallback(
    () =>
      mounted.current &&
      !boundary.current.disabled &&
      boundary.current.generation === generation,
    [generation],
  );

  const stop = useCallback(() => {
    if (!activeBoundary()) return;
    release(true);
    setSpeakingMessageId(null);
    setError(null);
  }, [activeBoundary, release]);

  const read = useCallback(
    (messageId: string, text: string) => {
      const speech = browserSpeech();
      if (!speech || !activeBoundary() || !text.trim()) return;
      if (current.current?.messageId === messageId) {
        stop();
        return;
      }
      const hadPrevious = release(true);
      setSpeakingMessageId(null);
      setError(null);
      try {
        if (!hadPrevious) speech.synthesis.cancel();
        const utterance = new speech.Constructor(revealInvisible(text).text);
        const voice = preferredVoice(speech.synthesis);
        if (voice) utterance.voice = voice;
        current.current = { utterance, synthesis: speech.synthesis, messageId };
        const active = () =>
          activeBoundary() && current.current?.utterance === utterance;
        utterance.onstart = () => {
          if (active()) setSpeakingMessageId(messageId);
        };
        utterance.onend = () => {
          if (!active()) return;
          release(false);
          setSpeakingMessageId(null);
        };
        utterance.onerror = () => {
          if (!active()) return;
          release(false);
          setSpeakingMessageId(null);
          setError(READ_ALOUD_ERROR);
        };
        speech.synthesis.speak(utterance);
      } catch {
        release(true);
        setSpeakingMessageId(null);
        setError(READ_ALOUD_ERROR);
      }
    },
    [activeBoundary, release, stop],
  );

  const supported = Boolean(browserSpeech());
  return useMemo(
    () => ({
      supported,
      disabled,
      speakingMessageId,
      error,
      read,
      stop,
    }),
    [supported, disabled, speakingMessageId, error, read, stop],
  );
}
