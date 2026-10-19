import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ProviderKeys as ProviderKeysView } from '@animaOS-SWARM/sdk';

import { daemon, type DaemonProvider } from '../../lib/daemon-api';
import { ProviderKeys } from './ProviderKeys';

afterEach(() => vi.restoreAllMocks());

const PROVIDERS: DaemonProvider[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    requiresKey: true,
    configured: true,
    apiKeyEnvs: ['OPENAI_API_KEY'],
  },
];

const ENV_KEY = {
  id: 'env',
  label: null,
  masked: 'sk-env…0000',
  source: 'env' as const,
  status: 'ready' as const,
  restingUntilMs: null,
  lastError: null,
  uses: 3,
};

function pool(over: Partial<ProviderKeysView> = {}): ProviderKeysView {
  return {
    id: 'openai',
    strategy: 'fill_first',
    persisted: true,
    keys: [ENV_KEY],
    ...over,
  };
}

describe('ProviderKeys', () => {
  it('adds_and_removes_a_key_and_shows_it_masked', async () => {
    const user = userEvent.setup();
    let current = pool();
    vi.spyOn(daemon, 'providerKeys').mockImplementation(async () => ({
      providers: [current],
    }));
    const add = vi
      .spyOn(daemon, 'addProviderKey')
      .mockImplementation(async () => {
        const created = {
          ...ENV_KEY,
          id: 'key_abc123def456',
          masked: 'sk-pro…abcd',
          source: 'file' as const,
          uses: 0,
        };
        current = pool({ keys: [ENV_KEY, created] });
        return created;
      });
    const remove = vi
      .spyOn(daemon, 'removeProviderKey')
      .mockImplementation(async () => {
        current = pool();
        return { deleted: true };
      });

    render(<ProviderKeys providers={PROVIDERS} />);
    expect(await screen.findByText('sk-env…0000')).toBeVisible();
    expect(screen.getByRole('heading', { name: 'API keys' })).toBeVisible();
    expect(
      screen.getByText(
        'Several keys for one provider take turns. A key that hits a limit rests and the next one is used.',
      ),
    ).toBeVisible();

    const input = screen.getByLabelText('API key for OpenAI');
    expect(input).toHaveAttribute('type', 'password');
    await user.type(input, 'sk-proj-secret-value-abcd');
    await user.click(screen.getByRole('button', { name: 'Add key' }));

    expect(add).toHaveBeenCalledWith('openai', {
      key: 'sk-proj-secret-value-abcd',
    });
    expect(await screen.findByText('sk-pro…abcd')).toBeVisible();
    // The key is cleared from the field and never shown back.
    expect(input).toHaveValue('');
    expect(screen.queryByText(/sk-proj-secret-value-abcd/)).toBeNull();

    // The environment key is read-only; the added one can be removed.
    expect(
      screen.queryByRole('button', { name: 'Remove key sk-env…0000' }),
    ).toBeNull();
    await user.click(
      screen.getByRole('button', { name: 'Remove key sk-pro…abcd' }),
    );
    expect(remove).toHaveBeenCalledWith('openai', 'key_abc123def456');
    await waitFor(() => expect(screen.queryByText('sk-pro…abcd')).toBeNull());
  });

  it('shows_resting_keys_with_their_reason', async () => {
    const restingUntilMs = new Date(2026, 9, 19, 14, 5).getTime();
    vi.spyOn(daemon, 'providerKeys').mockResolvedValue({
      providers: [
        pool({
          persisted: false,
          keys: [
            ENV_KEY,
            {
              ...ENV_KEY,
              id: 'key_aaaaaaaaaaaa',
              masked: 'sk-two…beef',
              source: 'file',
              status: 'resting',
              restingUntilMs,
              lastError: 'rate limited',
            },
          ],
        }),
      ],
    });
    render(<ProviderKeys providers={PROVIDERS} />);
    const resting = await screen.findByText(
      /^Resting until .+ · rate limited$/,
    );
    expect(resting).toBeVisible();
    const row = resting.closest('li') as HTMLElement;
    expect(within(row).getByText('sk-two…beef')).toBeVisible();
    // The other key is ready.
    expect(screen.getByText('Ready')).toBeVisible();
    expect(
      screen.getByText(
        'Keys added here last until the daemon restarts. Set ANIMAOS_RS_PROVIDER_KEYS_FILE to keep them.',
      ),
    ).toBeVisible();
  });

  it('wakes_a_resting_key_with_try_now_including_the_env_key', async () => {
    const user = userEvent.setup();
    const resting = {
      ...ENV_KEY,
      status: 'resting' as const,
      restingUntilMs: Date.now() + 3_600_000,
      lastError: 'out of credit',
    };
    let current = pool({ keys: [resting] });
    vi.spyOn(daemon, 'providerKeys').mockImplementation(async () => ({
      providers: [current],
    }));
    const wake = vi
      .spyOn(daemon, 'wakeProviderKey')
      .mockImplementation(async () => {
        current = pool();
        return ENV_KEY;
      });
    const changed = vi.fn();

    render(<ProviderKeys providers={PROVIDERS} onChanged={changed} />);
    await user.click(
      await screen.findByRole('button', { name: 'Try key sk-env…0000 now' }),
    );

    expect(wake).toHaveBeenCalledWith('openai', 'env');
    expect(await screen.findByText('Ready')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Try key sk-env…0000 now' }),
    ).toBeNull();
    expect(changed).toHaveBeenCalled();
  });

  it('offers_try_now_only_on_resting_keys', async () => {
    vi.spyOn(daemon, 'providerKeys').mockResolvedValue({ providers: [pool()] });
    render(<ProviderKeys providers={PROVIDERS} />);
    expect(await screen.findByText('Ready')).toBeVisible();
    expect(screen.queryByRole('button', { name: /^Try key/ })).toBeNull();
  });

  it('shows the daemon error when a key is refused', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'providerKeys').mockResolvedValue({ providers: [pool()] });
    vi.spyOn(daemon, 'addProviderKey').mockRejectedValue(
      new Error('that key is already in the pool'),
    );
    render(<ProviderKeys providers={PROVIDERS} />);
    await user.type(
      await screen.findByLabelText('API key for OpenAI'),
      'sk-dup',
    );
    await user.click(screen.getByRole('button', { name: 'Add key' }));
    expect(
      await screen.findByText('that key is already in the pool'),
    ).toBeVisible();
  });

  it('changes how a provider picks between its keys', async () => {
    const user = userEvent.setup();
    vi.spyOn(daemon, 'providerKeys').mockResolvedValue({ providers: [pool()] });
    const strategy = vi
      .spyOn(daemon, 'setKeyStrategy')
      .mockResolvedValue(pool({ strategy: 'round_robin' }));
    render(<ProviderKeys providers={PROVIDERS} />);
    await user.selectOptions(
      await screen.findByLabelText('How OpenAI picks a key'),
      'round_robin',
    );
    expect(strategy).toHaveBeenCalledWith('openai', 'round_robin');
  });
});
