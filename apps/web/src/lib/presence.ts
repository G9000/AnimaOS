import { useSyncExternalStore } from 'react';

/** What the voice controls report to the orb: whether a reply is being read
 *  aloud. One store for the whole console. Dictation stays in the composer:
 *  its words go into the input, not onto the orb. */
export interface PresenceSignals {
  speaking: boolean;
}

let signals: PresenceSignals = { speaking: false };
const listeners = new Set<() => void>();

function emit(next: PresenceSignals) {
  signals = next;
  for (const listener of listeners) listener();
}

export function setPresenceSpeaking(speaking: boolean) {
  if (signals.speaking === speaking) return;
  emit({ ...signals, speaking });
}

export function resetPresence() {
  emit({ speaking: false });
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
  | 'attention';

/** A pending approval outranks everything, because anima is waiting on the
 *  owner. */
export function presenceMode({
  signals,
  running,
  waiting,
}: {
  signals: PresenceSignals;
  running: boolean;
  waiting: boolean;
}): PresenceMode {
  if (waiting) return 'attention';
  if (signals.speaking) return 'speaking';
  if (running) return 'thinking';
  return 'calm';
}

export function getPresenceSnapshotForTest(): PresenceSignals {
  return signals;
}
