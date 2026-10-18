import { afterEach, describe, expect, it } from 'vitest';
import {
  presenceMode,
  resetPresence,
  setPresenceListening,
  setPresenceSpeaking,
  getPresenceSnapshotForTest,
} from './presence';

afterEach(() => resetPresence());

describe('presenceMode', () => {
  const quiet = { listening: null, speaking: false };
  it('is calm when nothing is happening', () => {
    expect(
      presenceMode({ signals: quiet, running: false, waiting: false }),
    ).toBe('calm');
  });
  it('puts the owner talking above everything else', () => {
    expect(
      presenceMode({
        signals: { listening: { words: '' }, speaking: true },
        running: true,
        waiting: true,
      }),
    ).toBe('listening');
  });
  it('shows a waiting approval before speech or work', () => {
    expect(
      presenceMode({
        signals: { ...quiet, speaking: true },
        running: true,
        waiting: true,
      }),
    ).toBe('attention');
  });
  it('prefers speaking over thinking', () => {
    expect(
      presenceMode({
        signals: { ...quiet, speaking: true },
        running: true,
        waiting: false,
      }),
    ).toBe('speaking');
    expect(
      presenceMode({ signals: quiet, running: true, waiting: false }),
    ).toBe('thinking');
  });
});

it('keeps the same snapshot when a signal does not change', () => {
  setPresenceListening('hi');
  const first = getPresenceSnapshotForTest();
  setPresenceListening('hi');
  setPresenceSpeaking(false);
  expect(getPresenceSnapshotForTest()).toBe(first);
});
