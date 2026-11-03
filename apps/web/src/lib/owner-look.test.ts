import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  DEFAULT_APPEARANCE_PREFERENCES,
  readAppearancePreferences,
  saveAppearancePreferences,
} from './appearance-preferences';
import { readChatBackground, saveChatBackground } from './chat-background';
import { daemon } from './daemon-api';
import {
  applyLook,
  applyingDaemonLook,
  describeLookChange,
  localLook,
  lookChangeResult,
  syncLookFromDaemon,
  type OwnerLook,
} from './owner-look';

const BASE: OwnerLook = {
  ...DEFAULT_APPEARANCE_PREFERENCES,
  chatBackground: 'quiet',
};

afterEach(() => {
  vi.restoreAllMocks();
  saveAppearancePreferences(DEFAULT_APPEARANCE_PREFERENCES);
  saveChatBackground('quiet');
});

function result(body: unknown) {
  return lookChangeResult('change_look', 'success', JSON.stringify(body));
}

describe('lookChangeResult', () => {
  it('reads the look a change set, the one before, and what changed', () => {
    const look = { ...BASE, theme: 'light' };
    expect(result({ look, previous: BASE, changed: ['theme'] })).toEqual({
      look,
      previous: BASE,
      changed: ['theme'],
    });
  });

  it('ignores a read, a failure, another tool, and choices it does not know', () => {
    expect(result({ look: BASE, changed: [] })).toBeNull();
    expect(
      lookChangeResult('change_look', 'error', 'The look could not be saved'),
    ).toBeNull();
    expect(
      lookChangeResult('rename_chat', 'success', JSON.stringify({ look: BASE })),
    ).toBeNull();
    expect(
      result({ look: { ...BASE, accent: 'teal' }, previous: BASE, changed: ['accent'] }),
    ).toBeNull();
  });
});

describe('describeLookChange', () => {
  it('names the advanced and simple views when both switches move together', () => {
    const advanced = { ...BASE, showWork: true, advancedInput: true };
    expect(
      describeLookChange({
        look: advanced,
        previous: BASE,
        changed: ['advancedInput', 'showWork'],
      }),
    ).toBe('Advanced view');
    expect(
      describeLookChange({
        look: BASE,
        previous: advanced,
        changed: ['advancedInput', 'showWork'],
      }),
    ).toBe('Simple view');
  });

  it('lists each other change in plain words', () => {
    expect(
      describeLookChange({
        look: { ...BASE, theme: 'light', accent: 'lavender', chatBackground: 'off' },
        previous: BASE,
        changed: ['theme', 'accent', 'chatBackground'],
      }),
    ).toBe('Light theme · Lavender accent · Background off');
  });
});

describe('applyLook', () => {
  it('shows the look here without counting as the owner’s own change', () => {
    const seen: boolean[] = [];
    const listen = () => seen.push(applyingDaemonLook());
    window.addEventListener('anima:appearance', listen);
    applyLook({ ...BASE, theme: 'light', chatBackground: 'off' });
    window.removeEventListener('anima:appearance', listen);
    expect(readAppearancePreferences().theme).toBe('light');
    expect(readChatBackground()).toBe('off');
    expect(seen).toEqual([true]);
    expect(applyingDaemonLook()).toBe(false);
  });
});

describe('syncLookFromDaemon', () => {
  it('shows the daemon’s look when it has one', async () => {
    vi.spyOn(daemon, 'look').mockResolvedValue({ ...BASE, accent: 'sage' });
    const save = vi.spyOn(daemon, 'setLook');
    await syncLookFromDaemon();
    expect(readAppearancePreferences().accent).toBe('sage');
    expect(save).not.toHaveBeenCalled();
  });

  it('saves this browser’s look the first time', async () => {
    saveAppearancePreferences({ theme: 'light' });
    vi.spyOn(daemon, 'look').mockResolvedValue(null);
    const save = vi
      .spyOn(daemon, 'setLook')
      .mockImplementation(async (look) => look);
    await syncLookFromDaemon();
    expect(save).toHaveBeenCalledWith(localLook());
    expect(save.mock.calls[0][0].theme).toBe('light');
  });
});
