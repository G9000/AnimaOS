import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

import {
  DEFAULT_APPEARANCE_PREFERENCES,
  saveAppearancePreferences,
} from '../lib/appearance-preferences';
import { daemon } from '../lib/daemon-api';
import { applyLook, localLook } from '../lib/owner-look';
import { useOwnerLookSync } from './useOwnerLookSync';

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  saveAppearancePreferences(DEFAULT_APPEARANCE_PREFERENCES);
});

it('loads the daemon’s look online and saves the owner’s own switches, not its echo', async () => {
  const remote = { ...localLook(), accent: 'amber' as const };
  const load = vi.spyOn(daemon, 'look').mockResolvedValue(remote);
  const save = vi
    .spyOn(daemon, 'setLook')
    .mockImplementation(async (look) => look);
  renderHook(() => useOwnerLookSync(null, true));

  await waitFor(() => expect(localLook().accent).toBe('amber'));
  expect(load).toHaveBeenCalledTimes(1);

  vi.useFakeTimers();
  act(() => applyLook({ ...remote, theme: 'light' }));
  act(() => vi.advanceTimersByTime(500));
  expect(save).not.toHaveBeenCalled();

  act(() => saveAppearancePreferences({ showWork: true }));
  act(() => vi.advanceTimersByTime(500));
  expect(save).toHaveBeenCalledTimes(1);
  expect(save.mock.calls[0][0]).toMatchObject({
    showWork: true,
    theme: 'light',
  });
});

it('does nothing offline', () => {
  const load = vi.spyOn(daemon, 'look');
  renderHook(() => useOwnerLookSync(null, false));
  expect(load).not.toHaveBeenCalled();
});
