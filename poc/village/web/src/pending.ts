import type { Dialogue, NpcId, TurnInput } from './types';

export function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
export function writeStorage(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* Session remains playable without storage. */
  }
}
export function removeStorage(key: string) {
  try {
    localStorage.removeItem(key);
  } catch {
    /* Storage is optional. */
  }
}
export interface PendingRequest {
  id: string;
  kind: 'visit' | 'turn';
  npcId?: NpcId;
  input?: TurnInput;
  dialogue?: Dialogue;
}
function object(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === 'object');
}
function npc(value: unknown): value is NpcId {
  return value === 'tavi' || value === 'mira';
}
function dialogue(value: unknown): value is Dialogue {
  return (
    object(value) &&
    typeof value.text === 'string' &&
    typeof value.translation === 'string' &&
    typeof value.reading === 'string' &&
    ['warm', 'thoughtful', 'happy'].includes(String(value.expression)) &&
    ['model', 'reviewed'].includes(String(value.source)) &&
    Array.isArray(value.suggestions) &&
    value.suggestions.every(
      (item) =>
        object(item) &&
        typeof item.id === 'string' &&
        typeof item.text === 'string' &&
        typeof item.translation === 'string',
    )
  );
}
export function savedRequest(saveId: string): PendingRequest | null {
  try {
    const parsed: unknown = JSON.parse(
      readStorage(`village.pending.${saveId}`) ?? 'null',
    );
    if (
      !object(parsed) ||
      typeof parsed.id !== 'string' ||
      !parsed.id ||
      (parsed.npcId !== undefined && !npc(parsed.npcId))
    )
      return null;
    const cachedDialogue = dialogue(parsed.dialogue)
      ? parsed.dialogue
      : undefined;
    if (parsed.kind === 'visit')
      return {
        id: parsed.id,
        kind: 'visit',
        ...(npc(parsed.npcId) ? { npcId: parsed.npcId } : {}),
        ...(cachedDialogue ? { dialogue: cachedDialogue } : {}),
      };
    if (
      parsed.kind === 'turn' &&
      object(parsed.input) &&
      parsed.input.requestId === parsed.id &&
      typeof parsed.input.message === 'string' &&
      (parsed.input.npcId === undefined || npc(parsed.input.npcId))
    )
      return {
        id: parsed.id,
        kind: 'turn',
        input: parsed.input as unknown as TurnInput,
        ...(npc(parsed.npcId) ? { npcId: parsed.npcId } : {}),
        ...(cachedDialogue ? { dialogue: cachedDialogue } : {}),
      };
  } catch {
    /* Invalid local metadata cannot change the server save. */
  }
  return null;
}
