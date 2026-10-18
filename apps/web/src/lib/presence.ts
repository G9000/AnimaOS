import { useSyncExternalStore } from 'react';

/** What the voice controls report to the orb: the words being dictated, and
 *  whether a reply is being read aloud. One store for the whole console. */
export interface PresenceSignals {
  /** Set while dictating; the words heard so far (may be empty). */
  listening: { words: string } | null;
  speaking: boolean;
}

let signals: PresenceSignals = { listening: null, speaking: false };
const listeners = new Set<() => void>();

function emit(next: PresenceSignals) {
  signals = next;
  for (const listener of listeners) listener();
}

export function setPresenceListening(words: string | null) {
  const listening = words === null ? null : { words };
  if (
    signals.listening?.words === listening?.words &&
    (signals.listening === null) === (listening === null)
  )
    return;
  emit({ ...signals, listening });
}

export function setPresenceSpeaking(speaking: boolean) {
  if (signals.speaking === speaking) return;
  emit({ ...signals, speaking });
}

export function resetPresence() {
  emit({ listening: null, speaking: false });
}

export function usePresenceSignals(): PresenceSignals {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => signals,
  );
}

export type PresenceMode =
  | 'calm'
  | 'thinking'
  | 'speaking'
  | 'listening'
  | 'attention';

/** Listening outranks everything: the owner is talking. A pending approval
 *  comes next, because anima is waiting on them. */
export function presenceMode({
  signals,
  running,
  waiting,
}: {
  signals: PresenceSignals;
  running: boolean;
  waiting: boolean;
}): PresenceMode {
  if (signals.listening) return 'listening';
  if (waiting) return 'attention';
  if (signals.speaking) return 'speaking';
  if (running) return 'thinking';
  return 'calm';
}

export function getPresenceSnapshotForTest(): PresenceSignals {
  return signals;
}
