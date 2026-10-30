import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { daemon, type DaemonProvider } from '../../lib/daemon-api';
import { SideModel } from './SideModel';

afterEach(() => vi.restoreAllMocks());

const PROVIDERS: DaemonProvider[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    requiresKey: true,
    configured: true,
    apiKeyEnvs: [],
  },
  {
    id: 'google',
    label: 'Google',
    requiresKey: true,
    configured: true,
    apiKeyEnvs: [],
  },
];

describe('SideModel', () => {
  it('saves_the_side_model', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'sideModel').mockResolvedValue(null);
    const set = vi
      .spyOn(daemon, 'setSideModel')
      .mockImplementation(async (choice) => choice);
    render(<SideModel providers={PROVIDERS} />);
    expect(
      await screen.findByRole('heading', { name: 'Side-task model' }),
    ).toBeVisible();
    expect(
      screen.getByText(
        "Titles, summaries, the smart check and task reviews. Leave empty to use each agent's own model.",
      ),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();

    await user.selectOptions(
      screen.getByLabelText('Side-task provider'),
      'google',
    );
    await user.clear(screen.getByLabelText('Side-task model id'));
    await user.type(
      screen.getByLabelText('Side-task model id'),
      'gemini-2.5-flash',
    );
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(set).toHaveBeenCalledWith({
      provider: 'google',
      model: 'gemini-2.5-flash',
    });
  });

  it('clears the side model when the provider is emptied', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'sideModel').mockResolvedValue({
      provider: 'openai',
      model: 'gpt-4o-mini',
    });
    const set = vi.spyOn(daemon, 'setSideModel').mockResolvedValue(null);
    render(<SideModel providers={PROVIDERS} />);
    await waitFor(() =>
      expect(screen.getByLabelText('Side-task model id')).toHaveValue(
        'gpt-4o-mini',
      ),
    );
    await user.selectOptions(screen.getByLabelText('Side-task provider'), '');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(set).toHaveBeenCalledWith(null);
  });

  it('never offers the test providers', async () => {
    vi.spyOn(daemon, 'sideModel').mockResolvedValue(null);
    const mock = (id: string): DaemonProvider => ({
      id,
      label: id,
      requiresKey: false,
      configured: true,
      apiKeyEnvs: [],
    });
    render(
      <SideModel
        providers={[mock('deterministic'), mock('test'), ...PROVIDERS]}
      />,
    );
    const select = await screen.findByLabelText('Side-task provider');
    const options = Array.from(
      select.querySelectorAll('option'),
      (option) => option.value,
    );
    expect(options).toEqual(['', 'openai', 'google']);
  });
});
