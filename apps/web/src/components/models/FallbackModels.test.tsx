import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { DaemonProvider } from '../../lib/daemon-api';
import { FallbackModels } from './FallbackModels';

const PROVIDERS: DaemonProvider[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    requiresKey: true,
    configured: true,
    apiKeyEnvs: [],
  },
  {
    id: 'anthropic',
    label: 'Anthropic',
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
const PRIMARY = { provider: 'openai', model: 'gpt-4o' };

describe('FallbackModels', () => {
  it('reorders_and_saves_fallbacks_up_to_three', async () => {
    const user = userEvent.setup();
    const save = vi.fn(async () => true);
    render(
      <FallbackModels
        primary={PRIMARY}
        fallbacks={[
          { provider: 'anthropic', model: 'claude-sonnet-4-6' },
          { provider: 'google', model: 'gemini-2.5-pro' },
        ]}
        providers={PROVIDERS}
        save={save}
      />,
    );
    expect(
      screen.getByRole('heading', { name: 'If this model fails' }),
    ).toBeVisible();
    expect(
      screen.getByText('Tried in order, only before the reply starts.'),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Save fallbacks' }),
    ).toBeDisabled();

    // Add a third, then move it to the front.
    await user.click(screen.getByRole('button', { name: 'Add fallback' }));
    await user.selectOptions(
      screen.getByLabelText('Fallback 3 provider'),
      'openai',
    );
    await user.clear(screen.getByLabelText('Fallback 3 model'));
    await user.type(screen.getByLabelText('Fallback 3 model'), 'gpt-4o-mini');
    // Three is the most.
    expect(screen.queryByRole('button', { name: 'Add fallback' })).toBeNull();
    await user.click(
      screen.getByRole('button', { name: 'Move fallback 3 up' }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Move fallback 2 up' }),
    );

    await user.click(screen.getByRole('button', { name: 'Save fallbacks' }));
    expect(save).toHaveBeenCalledWith([
      { provider: 'openai', model: 'gpt-4o-mini' },
      { provider: 'anthropic', model: 'claude-sonnet-4-6' },
      { provider: 'google', model: 'gemini-2.5-pro' },
    ]);
  });

  it('refuses a fallback that is the primary model, and clears with none', async () => {
    const user = userEvent.setup();
    const save = vi.fn(async () => true);
    render(
      <FallbackModels
        primary={PRIMARY}
        fallbacks={[{ provider: 'anthropic', model: 'claude-haiku-4-5' }]}
        providers={PROVIDERS}
        save={save}
      />,
    );
    await user.selectOptions(
      screen.getByLabelText('Fallback 1 provider'),
      'openai',
    );
    await user.clear(screen.getByLabelText('Fallback 1 model'));
    await user.type(screen.getByLabelText('Fallback 1 model'), 'gpt-4o');
    expect(screen.getByText('That is already the main model.')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Save fallbacks' }),
    ).toBeDisabled();

    await user.click(screen.getByRole('button', { name: 'Remove fallback 1' }));
    await user.click(screen.getByRole('button', { name: 'Save fallbacks' }));
    expect(save).toHaveBeenCalledWith([]);
  });

  it('never offers the test providers and defaults a new row to a configured provider', async () => {
    const user = userEvent.setup();
    const mock = (id: string): DaemonProvider => ({
      id,
      label: id,
      requiresKey: false,
      configured: true,
      apiKeyEnvs: [],
    });
    const providers: DaemonProvider[] = [
      mock('deterministic'),
      mock('test'),
      { ...PROVIDERS[0], configured: true },
      { ...PROVIDERS[1], configured: false },
      { ...PROVIDERS[2], configured: true },
    ];
    render(
      <FallbackModels
        primary={PRIMARY}
        fallbacks={[]}
        providers={providers}
        save={vi.fn(async () => true)}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Add fallback' }));
    const select = screen.getByLabelText('Fallback 1 provider');
    // google: the first configured provider that isn't the primary's.
    expect(select).toHaveValue('google');
    const options = Array.from(
      select.querySelectorAll('option'),
      (option) => option.value,
    );
    expect(options).toEqual(['openai', 'anthropic', 'google']);
  });

  it('defaults a new row to the first real provider when none is configured', async () => {
    const user = userEvent.setup();
    const providers: DaemonProvider[] = [
      {
        id: 'deterministic',
        label: 'Deterministic',
        requiresKey: false,
        configured: true,
        apiKeyEnvs: [],
      },
      ...PROVIDERS.map((provider) => ({ ...provider, configured: false })),
    ];
    render(
      <FallbackModels
        primary={{ provider: 'deterministic', model: 'x' }}
        fallbacks={[]}
        providers={providers}
        save={vi.fn(async () => true)}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Add fallback' }));
    expect(screen.getByLabelText('Fallback 1 provider')).toHaveValue('openai');
  });
});
