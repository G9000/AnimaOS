import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { AutomationEditor } from './AutomationEditor';

afterEach(() => vi.restoreAllMocks());

describe('Discord automation destinations', () => {
  it('creates an event trigger without inventing a next fire time', async () => {
    vi.spyOn(daemon, 'previewAutomation').mockResolvedValue([]);
    const save = vi.fn(async () => true);
    const user = userEvent.setup();
    render(
      <AutomationEditor
        automation={null}
        telegramConnectorId={null}
        onSave={save}
        onCancel={() => {}}
      />,
    );
    await user.type(screen.getByLabelText('Prompt'), 'Summarize the event');
    await user.click(screen.getByLabelText('Run when an event arrives'));
    await user.type(screen.getByLabelText('Event topic'), 'files.changed');
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Create automation' }),
      ).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Create automation' }));
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        trigger: { type: 'event', topic: 'files.changed' },
      }),
    );
    expect(screen.getByText(/Waits for an authenticated event/)).toBeVisible();
  });
  it('saves the selected bot home through the generic connector target', async () => {
    vi.spyOn(daemon, 'previewAutomation').mockResolvedValue([
      Date.now() + 60_000,
    ]);
    const save = vi.fn(async () => true);
    const user = userEvent.setup();
    render(
      <AutomationEditor
        automation={null}
        telegramConnectorId={null}
        discordDestinations={[
          { connectorId: 'dc-1', label: 'Nova · #home' },
          { connectorId: 'dc-2', label: 'Other · #updates' },
        ]}
        onSave={save}
        onCancel={() => {}}
      />,
    );
    await user.type(screen.getByLabelText('Prompt'), 'Daily progress');
    await user.type(screen.getByLabelText('When'), 'every day at 9am');
    await user.selectOptions(screen.getByLabelText('Runs in'), 'discord:dc-2');
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Create automation' }),
      ).toBeEnabled(),
    );
    await user.click(screen.getByRole('button', { name: 'Create automation' }));
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        target: { type: 'connector', connectorId: 'dc-2' },
      }),
    );
  });
});

describe('Automation destination wording', () => {
  it('offers the main chat, with Telegram or a Discord note, and says the main chat always gets one', () => {
    render(
      <AutomationEditor
        automation={null}
        telegramConnectorId="tg-1"
        discordDestinations={[{ connectorId: 'dc-1', label: 'Nova · #home' }]}
        onSave={vi.fn(async () => true)}
        onCancel={() => {}}
      />,
    );
    const options = within(screen.getByLabelText('Runs in'))
      .getAllByRole('option')
      .map((option) => option.textContent);
    expect(options).toEqual([
      'Main chat',
      'Main chat + Telegram',
      'Discord #home (note in main chat)',
    ]);
    expect(
      screen.getByText(/main chat always gets a card or a note/i),
    ).toBeVisible();
    expect(screen.queryByText(/its own thread/i)).toBeNull();
  });
});
