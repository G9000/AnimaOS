import { useSyncExternalStore } from 'react';

/** What sits behind the conversation: the orb behind the chat as it was, the
 *  orb resting at the edge so the text stays clear, or nothing. */
export const CHAT_BACKGROUNDS = ['full', 'quiet', 'off'] as const;
export type ChatBackground = (typeof CHAT_BACKGROUNDS)[number];

export const DEFAULT_CHAT_BACKGROUND: ChatBackground = 'quiet';

const STORAGE_KEY = 'anima.chatBackground';
const CHANGED = 'anima:chat-background';

// Kept when storage is unavailable (a private window): the choice still holds
// for this visit.
let remembered: ChatBackground | null = null;
let storageWriteFailed = false;

function isChatBackground(value: unknown): value is ChatBackground {
  return (
    typeof value === 'string' &&
    (CHAT_BACKGROUNDS as readonly string[]).includes(value)
  );
}

export function readChatBackground(): ChatBackground {
  if (storageWriteFailed && remembered !== null) return remembered;
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (isChatBackground(stored)) return stored;
  } catch {
    /* fall through to what this visit remembers */
  }
  return remembered ?? DEFAULT_CHAT_BACKGROUND;
}

export function saveChatBackground(value: ChatBackground): void {
  remembered = value;
  try {
    localStorage.setItem(STORAGE_KEY, value);
    storageWriteFailed = false;
  } catch {
    storageWriteFailed = true;
    /* Storage can be unavailable in private windows; the choice still holds. */
  }
  window.dispatchEvent(new Event(CHANGED));
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener(CHANGED, onChange);
  window.addEventListener('storage', onChange);
  return () => {
    window.removeEventListener(CHANGED, onChange);
    window.removeEventListener('storage', onChange);
  };
}

/** The chat background and a way to change it, in step across the app. */
export function useChatBackground(): [
  ChatBackground,
  (value: ChatBackground) => void,
] {
  const value = useSyncExternalStore(
    subscribe,
    readChatBackground,
    () => DEFAULT_CHAT_BACKGROUND,
  );
  return [value, saveChatBackground];
}
