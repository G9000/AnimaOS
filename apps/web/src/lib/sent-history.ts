// Messages sent from a conversation's composer, kept in session storage so ↑
// can bring one back to edit or resend. Without storage they live in memory.

const LIMIT = 50;
const memory = new Map<string, string[]>();

function storageKey(key: string): string {
  return `animaos.sent.${key.replace('\u0000', '/')}`;
}

export function loadSent(key: string): string[] {
  const cached = memory.get(key);
  if (cached) return cached;
  let items: string[] = [];
  try {
    const raw = window.sessionStorage.getItem(storageKey(key));
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (Array.isArray(parsed))
      items = parsed.filter((item): item is string => typeof item === 'string');
  } catch {
    // Blocked or corrupt storage: start from an empty history.
  }
  memory.set(key, items);
  return items;
}

/** Records a sent message; a repeat of the latest one is not added again. */
export function pushSent(key: string, text: string): void {
  const value = text.trim();
  if (!value) return;
  const items = loadSent(key);
  if (items.at(-1) === value) return;
  const next = [...items, value].slice(-LIMIT);
  memory.set(key, next);
  try {
    window.sessionStorage.setItem(storageKey(key), JSON.stringify(next));
  } catch {
    // Storage is full or blocked: the history stays in memory for this page.
  }
}

/** Test seam: forget what this page has cached. */
export function resetSentCache(): void {
  memory.clear();
}

export interface Recall {
  /** Position in the history, or -1 while the draft is the user's own. */
  cursor: number;
  /** The draft the user had before browsing, restored on the way back down. */
  stash: string;
}

export interface RecallStep {
  recall: Recall;
  draft: string;
}

/** ↑: the next older message, remembering the draft the first time. */
export function recallOlder(
  items: readonly string[],
  recall: Recall,
  draft: string,
): RecallStep | null {
  if (items.length === 0) return null;
  if (recall.cursor === -1)
    return {
      recall: { cursor: items.length - 1, stash: draft },
      draft: items[items.length - 1],
    };
  if (recall.cursor === 0) return null;
  const cursor = recall.cursor - 1;
  return { recall: { cursor, stash: recall.stash }, draft: items[cursor] };
}

/** ↓: the next newer message, then the draft the user started from. */
export function recallNewer(
  items: readonly string[],
  recall: Recall,
): RecallStep | null {
  if (recall.cursor === -1) return null;
  const cursor = recall.cursor + 1;
  if (cursor >= items.length)
    return { recall: { cursor: -1, stash: '' }, draft: recall.stash };
  return { recall: { cursor, stash: recall.stash }, draft: items[cursor] };
}
