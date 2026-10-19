import { createRef } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { ChatGptClient } from '@animaOS-SWARM/sdk';
import { daemon } from '../../lib/daemon-api';
import { ModelStep, type ModelStepProps } from './ModelStep';

vi.mock('../ChatGptConnection', () => ({
  ChatGptConnection: ({
    onConnectionChange,
  }: {
    onConnectionChange(): void;
  }) => <button onClick={onConnectionChange}>Complete ChatGPT sign-in</button>,
}));

it('lets a subscription-only user connect and select ChatGPT without another provider', async () => {
  const models = vi.spyOn(ChatGptClient.prototype, 'models').mockResolvedValue({
    state: 'unrequested',
    connectionRevision: 'connection-a',
    catalogRevision: null,
    fetchedAtMs: null,
    expiresAtMs: null,
    models: [],
    error: null,
  });
  const props: ModelStepProps = {
    providers: [
      {
        id: 'chatgpt',
        label: 'ChatGPT',
        configured: false,
        requiresKey: false,
        apiKeyEnvs: [],
      },
      {
        id: 'anthropic',
        label: 'Anthropic',
        configured: false,
        requiresKey: true,
        apiKeyEnvs: ['ANTHROPIC_API_KEY'],
      },
    ],
    catalogState: 'empty',
    providerError: null,
    provider: '',
    model: '',
    customModel: '',
    onProviderChange: vi.fn(),
    onModelChange: vi.fn(),
    onCustomModelChange: vi.fn(),
    onRetryProviders: vi.fn(),
    modelSelectRef: createRef(),
    customModelInputRef: createRef(),
  };
  const { rerender } = render(<ModelStep {...props} />);
  expect(
    screen.getByRole('heading', {
      name: 'Start with your ChatGPT subscription',
    }),
  ).toBeVisible();
  expect(
    screen.getByRole('button', { name: 'Use ChatGPT subscription' }),
  ).toBeDisabled();
  fireEvent.click(
    screen.getByRole('button', { name: 'Complete ChatGPT sign-in' }),
  );
  expect(props.onRetryProviders).toHaveBeenCalledOnce();

  const connectedProps: ModelStepProps = {
    ...props,
    catalogState: 'ready',
    providers: props.providers!.map((item) => ({
      ...item,
      configured: item.id === 'chatgpt',
    })),
  };
  rerender(<ModelStep {...connectedProps} />);
  fireEvent.click(
    screen.getByRole('button', { name: 'Use ChatGPT subscription' }),
  );
  expect(props.onProviderChange).toHaveBeenCalledWith('chatgpt');
  rerender(
    <ModelStep {...connectedProps} provider="chatgpt" model="gpt-5.5" />,
  );
  expect(
    screen.getByRole('button', { name: 'ChatGPT subscription selected' }),
  ).toHaveAttribute('aria-pressed', 'true');
  expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
    'gpt-5.5',
  );
  expect(
    screen.getByText('Other AI providers (optional)').closest('details'),
  ).not.toHaveAttribute('open');
  await screen.findByText(/Model metadata has not been refreshed/);
  models.mockRestore();
});

it('onboarding_accepts_a_key_instead_of_an_env_hint', async () => {
  const added = vi.spyOn(daemon, 'addProviderKey').mockResolvedValue({
    id: 'key_abc123def456',
    label: null,
    masked: 'sk-ant…wxyz',
    source: 'file',
    status: 'ready',
    restingUntilMs: null,
    lastError: null,
    uses: 0,
  });
  const onRetryProviders = vi.fn();
  render(
    <ModelStep
      providers={[
        {
          id: 'anthropic',
          label: 'Anthropic',
          configured: false,
          requiresKey: true,
          apiKeyEnvs: ['ANTHROPIC_API_KEY'],
        },
      ]}
      catalogState="ready"
      providerError={null}
      provider=""
      model=""
      customModel=""
      onProviderChange={vi.fn()}
      onModelChange={vi.fn()}
      onCustomModelChange={vi.fn()}
      onRetryProviders={onRetryProviders}
      modelSelectRef={createRef()}
      customModelInputRef={createRef()}
    />,
  );
  expect(screen.queryByText(/in the daemon environment/)).toBeNull();
  const field = screen.getByLabelText('API key for Anthropic');
  expect(field).toHaveAttribute('type', 'password');
  fireEvent.change(field, { target: { value: 'sk-ant-secret-wxyz' } });
  fireEvent.click(screen.getByRole('button', { name: 'Add API key' }));
  await vi.waitFor(() =>
    expect(added).toHaveBeenCalledWith('anthropic', {
      key: 'sk-ant-secret-wxyz',
    }),
  );
  await vi.waitFor(() => expect(onRetryProviders).toHaveBeenCalledOnce());
  await vi.waitFor(() => expect(field).toHaveValue(''));
});

it('warns in onboarding when added keys will not survive a restart', async () => {
  const keys = vi.spyOn(daemon, 'providerKeys').mockResolvedValue({
    providers: [
      { id: 'anthropic', strategy: 'fill_first', persisted: false, keys: [] },
    ],
  });
  const props = {
    catalogState: 'ready' as const,
    providerError: null,
    provider: '',
    model: '',
    customModel: '',
    onProviderChange: vi.fn(),
    onModelChange: vi.fn(),
    onCustomModelChange: vi.fn(),
    onRetryProviders: vi.fn(),
    modelSelectRef: createRef<HTMLSelectElement>(),
    customModelInputRef: createRef<HTMLInputElement>(),
    providers: [
      {
        id: 'anthropic',
        label: 'Anthropic',
        configured: false,
        requiresKey: true,
        apiKeyEnvs: ['ANTHROPIC_API_KEY'],
      },
    ],
  };
  const { unmount } = render(<ModelStep {...props} />);
  expect(
    await screen.findByText(
      'Keys added here last until the daemon restarts. Set ANIMAOS_RS_PROVIDER_KEYS_FILE to keep them.',
    ),
  ).toBeVisible();
  unmount();

  keys.mockResolvedValue({
    providers: [
      { id: 'anthropic', strategy: 'fill_first', persisted: true, keys: [] },
    ],
  });
  render(<ModelStep {...props} />);
  await act(async () => {
    await Promise.resolve();
  });
  expect(keys).toHaveBeenCalledTimes(2);
  expect(screen.queryByText(/last until the daemon restarts/)).toBeNull();
  keys.mockRestore();
});
