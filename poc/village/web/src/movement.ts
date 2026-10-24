import type { NpcId } from './types';

export interface Position {
  x: number;
  z: number;
}
export function movePlayer(
  position: Position,
  direction: Position,
  seconds: number,
  paused: boolean,
): Position {
  const length = Math.hypot(direction.x, direction.z);
  if (paused || length === 0) return position;
  const distance = Math.max(0, Math.min(seconds, 0.08)) * 3.2;
  const x = Math.max(
    -7.2,
    Math.min(7.2, position.x + (direction.x / length) * distance),
  );
  const z = Math.max(
    1.2,
    Math.min(8.3, position.z + (direction.z / length) * distance),
  );
  return { x, z };
}
export function nearTavi(position: Position): boolean {
  return Math.hypot(position.x + 0.6, position.z - 1) < 2.05;
}
export const npcPositions: Record<NpcId, Position> = {
  tavi: { x: -0.6, z: 0.95 },
  mira: { x: 4.3, z: 5.4 },
};
export function npcApproach(npc: NpcId): Position {
  return npc === 'tavi' ? { x: 0.5, z: 1.8 } : { x: 3.25, z: 5.7 };
}
export function nearestNpc(position: Position): NpcId | null {
  let nearest: NpcId | null = null;
  let distance = 2.05;
  for (const npc of ['tavi', 'mira'] as const) {
    const point = npcPositions[npc];
    const current = Math.hypot(position.x - point.x, position.z - point.z);
    if (current < distance) {
      nearest = npc;
      distance = current;
    }
  }
  return nearest;
}
export function shouldSubmitDialogue(event: {
  key: string;
  isComposing: boolean;
  keyCode: number;
  shiftKey: boolean;
}): boolean {
  return (
    event.key === 'Enter' &&
    !event.shiftKey &&
    !event.isComposing &&
    event.keyCode !== 229
  );
}
