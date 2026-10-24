import { describe, expect, it } from 'vitest';
import {
  movePlayer,
  nearestNpc,
  npcApproach,
  nearTavi,
  shouldSubmitDialogue,
  type Position,
} from './movement';

describe('shop exploration controls', () => {
  it('finds the resident in reach and guides to a reachable spot for each', () => {
    expect(nearestNpc({ x: -0.6, z: 1.8 })).toBe('tavi');
    expect(nearestNpc({ x: 4.3, z: 5.4 })).toBe('mira');
    expect(nearestNpc({ x: -5, z: 7 })).toBeNull();
    expect(nearestNpc(npcApproach('tavi'))).toBe('tavi');
    expect(nearestNpc(npcApproach('mira'))).toBe('mira');
  });
  it('moves at the same speed diagonally and along one axis', () => {
    const start = { x: 0, z: 3 };
    const straight = movePlayer(start, { x: 1, z: 0 }, 0.1, false);
    const diagonal = movePlayer(start, { x: 1, z: 1 }, 0.1, false);
    expect(straight.x).toBeGreaterThan(start.x);
    expect(Math.hypot(diagonal.x - start.x, diagonal.z - start.z)).toBeCloseTo(
      straight.x - start.x,
    );
  });
  it('stops at the shop wall while allowing movement along it', () => {
    const start: Position = { x: 0, z: 1.25 };
    const result = movePlayer(start, { x: 1, z: -1 }, 0.1, false);
    expect(result.z).toBeGreaterThanOrEqual(1.2);
    expect(result.x).toBeGreaterThan(0);
  });
  it('pauses movement while dialogue is focused and clamps long frame gaps', () => {
    const start = { x: 0, z: 3 };
    expect(movePlayer(start, { x: 1, z: 1 }, 0.1, true)).toEqual(start);
    expect(movePlayer(start, { x: 1, z: 0 }, 100, false).x).toBeLessThan(0.4);
  });
  it('offers interaction only within reach of Tavi', () => {
    expect(nearTavi({ x: 0.7, z: 1.6 })).toBe(true);
    expect(nearTavi({ x: -5, z: 5 })).toBe(false);
  });
});

describe('Japanese and Chinese composition', () => {
  it('does not submit while Enter confirms an IME candidate', () => {
    expect(
      shouldSubmitDialogue({
        key: 'Enter',
        isComposing: true,
        keyCode: 13,
        shiftKey: false,
      }),
    ).toBe(false);
    expect(
      shouldSubmitDialogue({
        key: 'Enter',
        isComposing: false,
        keyCode: 229,
        shiftKey: false,
      }),
    ).toBe(false);
    expect(
      shouldSubmitDialogue({
        key: 'Enter',
        isComposing: false,
        keyCode: 13,
        shiftKey: false,
      }),
    ).toBe(true);
    expect(
      shouldSubmitDialogue({
        key: 'Enter',
        isComposing: false,
        keyCode: 13,
        shiftKey: true,
      }),
    ).toBe(false);
  });
});
