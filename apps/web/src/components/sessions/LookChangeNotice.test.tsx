import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';

import {
  DEFAULT_APPEARANCE_PREFERENCES,
  readAppearancePreferences,
  saveAppearancePreferences,
} from '../../lib/appearance-preferences';
import { saveChatBackground } from '../../lib/chat-background';
import { daemon } from '../../lib/daemon-api';
import { applyLook, type OwnerLook } from '../../lib/owner-look';
import { LookChangeNotice } from './LookChangeNotice';

const BASE: OwnerLook = {
  ...DEFAULT_APPEARANCE_PREFERENCES,
  chatBackground: 'quiet',
};
const ADVANCED: OwnerLook = { ...BASE, showWork: true, advancedInput: true };
const CHANGE = {
  look: ADVANCED,
  previous: BASE,
  changed: ['advancedInput', 'showWork'] as (keyof OwnerLook)[],
};

afterEach(() => {
  vi.restoreAllMocks();
  saveAppearancePreferences(DEFAULT_APPEARANCE_PREFERENCES);
  saveChatBackground('quiet');
});

it('undoes a look changed from chat by saving the one before', async () => {
  const user = userEvent.setup();
  applyLook(ADVANCED);
  const save = vi
    .spyOn(daemon, 'setLook')
    .mockImplementation(async (look) => look);
  render(<LookChangeNotice change={CHANGE} />);

  expect(
    screen.getByRole('note', { name: 'Look changed: Advanced view' }),
  ).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Undo' }));

  expect(save).toHaveBeenCalledWith(BASE);
  expect(readAppearancePreferences().showWork).toBe(false);
  // The change is no longer in effect, so there is nothing to undo.
  expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull();
});

it('puts the change back when the undo cannot be saved', async () => {
  const user = userEvent.setup();
  applyLook(ADVANCED);
  vi.spyOn(daemon, 'setLook').mockRejectedValue(new Error('offline'));
  render(<LookChangeNotice change={CHANGE} />);

  await user.click(screen.getByRole('button', { name: 'Undo' }));

  expect(readAppearancePreferences().showWork).toBe(true);
  expect(await screen.findByRole('status')).toBeVisible();
});

it('offers no Undo once the look has moved on', () => {
  render(<LookChangeNotice change={CHANGE} />);
  expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull();
  act(() => applyLook(ADVANCED));
  expect(screen.getByRole('button', { name: 'Undo' })).toBeVisible();
});
