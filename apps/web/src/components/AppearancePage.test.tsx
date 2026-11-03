import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_APPEARANCE_PREFERENCES,
  readAppearancePreferences,
  saveAppearancePreferences,
} from '../lib/appearance-preferences';
import { AppearancePage } from './AppearancePage';

afterEach(() => localStorage.clear());

describe('appearance page', () => {
  it('offers the theme, accent, text, density, and motion controls from Pen', () => {
    render(<AppearancePage />);

    expect(screen.getByRole('radio', { name: /Dark/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /Light/ })).not.toBeChecked();
    expect(screen.getByRole('radio', { name: /Match system/ })).not.toBeChecked();
    expect(
      screen.getByRole('radio', { name: 'Anima pink' }),
    ).toBeChecked();
    expect(screen.getByRole('radio', { name: /Default/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /Comfortable/ })).toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: /Reduce motion/ }),
    ).not.toBeChecked();
  });

  it('takes any colour as the accent and keeps it while a preset is chosen', async () => {
    const user = userEvent.setup();
    saveAppearancePreferences(DEFAULT_APPEARANCE_PREFERENCES);
    render(<AppearancePage />);

    fireEvent.change(screen.getByLabelText('Custom accent'), {
      target: { value: '#14B8A6' },
    });
    expect(readAppearancePreferences()).toMatchObject({
      accent: 'custom',
      customAccent: '#14b8a6',
    });
    expect(screen.getByText('Custom · #14b8a6')).toBeVisible();
    expect(screen.getByRole('radio', { name: 'Anima pink' })).not.toBeChecked();

    await user.click(screen.getByRole('radio', { name: 'Sage' }));
    expect(readAppearancePreferences()).toMatchObject({
      accent: 'sage',
      customAccent: '#14b8a6',
    });
    // Choosing the custom swatch again brings the owner's colour back.
    await user.click(screen.getByLabelText('Custom accent'));
    expect(readAppearancePreferences().accent).toBe('custom');
    saveAppearancePreferences(DEFAULT_APPEARANCE_PREFERENCES);
  });

  it('persists the selected appearance preferences', async () => {
    const user = userEvent.setup();
    render(<AppearancePage />);

    await user.click(screen.getByRole('radio', { name: /Light/ }));
    await user.click(screen.getByRole('radio', { name: 'Lavender' }));
    await user.click(screen.getByRole('radio', { name: /Large/ }));
    await user.click(screen.getByRole('radio', { name: /Compact/ }));
    await user.click(screen.getByRole('checkbox', { name: /Reduce motion/ }));
    await user.click(screen.getByRole('checkbox', { name: /Advanced input/ }));
    await user.click(screen.getByRole('checkbox', { name: /Show work/ }));

    expect(JSON.parse(localStorage.getItem('anima.appearance') ?? '{}')).toEqual(
      {
        theme: 'light',
        accent: 'lavender',
        textSize: 'large',
        density: 'compact',
        reduceMotion: true,
        advancedInput: true,
        customAccent: '#e94dab',
        showWork: true,
      },
    );
  });

  it('has no chat background to choose: the chat sits on a plain field', () => {
    render(<AppearancePage />);
    expect(
      screen.queryByRole('group', { name: 'Chat background' }),
    ).not.toBeInTheDocument();
  });
});
