import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { AutomationEditor } from './AutomationEditor';

afterEach(() => vi.restoreAllMocks());

describe('Discord automation destinations', () => {
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
