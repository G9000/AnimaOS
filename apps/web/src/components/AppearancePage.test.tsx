import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { saveChatBackground } from '../lib/chat-background';
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

  it('persists the selected appearance preferences', async () => {
    const user = userEvent.setup();
    render(<AppearancePage />);

    await user.click(screen.getByRole('radio', { name: /Light/ }));
    await user.click(screen.getByRole('radio', { name: 'Lavender' }));
    await user.click(screen.getByRole('radio', { name: /Large/ }));
    await user.click(screen.getByRole('radio', { name: /Compact/ }));
    await user.click(screen.getByRole('checkbox', { name: /Reduce motion/ }));
    await user.click(screen.getByRole('checkbox', { name: /Advanced input/ }));

    expect(JSON.parse(localStorage.getItem('anima.appearance') ?? '{}')).toEqual(
      {
        theme: 'light',
        accent: 'lavender',
        textSize: 'large',
        density: 'compact',
        reduceMotion: true,
        advancedInput: true,
      },
    );
  });

  it('offers the three backgrounds with the current one chosen', () => {
    saveChatBackground('quiet');
    render(<AppearancePage />);
    const group = screen.getByRole('group', { name: 'Chat background' });
    expect(group).toBeVisible();
    expect(screen.getByRole('radio', { name: /Full orb/ })).not.toBeChecked();
    expect(screen.getByRole('radio', { name: /Quiet/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /Off/ })).not.toBeChecked();
  });

  it('changes and remembers the choice', async () => {
    saveChatBackground('quiet');
    render(<AppearancePage />);
    await userEvent.click(screen.getByRole('radio', { name: /Off/ }));
    expect(screen.getByRole('radio', { name: /Off/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /Quiet/ })).not.toBeChecked();
    expect(localStorage.getItem('anima.chatBackground')).toBe('off');
  });
});
