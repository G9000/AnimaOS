import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { saveChatBackground } from '../lib/chat-background';
import { AppearancePage } from './AppearancePage';

afterEach(() => localStorage.clear());

describe('appearance page', () => {
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
