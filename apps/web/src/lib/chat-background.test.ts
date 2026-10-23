import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_CHAT_BACKGROUND,
  readChatBackground,
  saveChatBackground,
} from './chat-background';

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe('chat background preference', () => {
  it('is off until the owner chooses', () => {
    expect(DEFAULT_CHAT_BACKGROUND).toBe('off');
    expect(readChatBackground()).toBe('off');
  });

  it('keeps a choice, and tells the app when it changes', () => {
    const changed = vi.fn();
    window.addEventListener('anima:chat-background', changed);
    saveChatBackground('off');
    expect(readChatBackground()).toBe('off');
    expect(localStorage.getItem('anima.chatBackground')).toBe('off');
    expect(changed).toHaveBeenCalledOnce();
    window.removeEventListener('anima:chat-background', changed);
  });

  it('ignores a stored value it does not know', () => {
    saveChatBackground('full');
    localStorage.setItem('anima.chatBackground', 'rainbow');
    expect(readChatBackground()).toBe('full');
  });

  it('still holds the choice for this visit when storage is unavailable', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    });
    saveChatBackground('off');
    expect(readChatBackground()).toBe('off');
  });
  it('keeps the new choice when storage reads work but writes fail', () => {
    saveChatBackground('full');
    vi.stubGlobal('localStorage', {
      getItem: () => 'full',
      setItem: () => {
        throw new Error('quota exceeded');
      },
    });
    expect(readChatBackground()).toBe('full');
    saveChatBackground('off');
    expect(readChatBackground()).toBe('off');
  });

});
