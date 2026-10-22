import { useState } from 'react';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  ChatGptClient,
  DaemonHttpError,
  type ChatGptModelsSnapshot,
} from '@animaos/sdk';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatGptModelSelect } from './ChatGptModelSelect';

function snapshot(
  overrides: Partial<ChatGptModelsSnapshot> = {},
): ChatGptModelsSnapshot {
  return {
    state: 'ready',
    connectionRevision: 'connection-a',
    catalogRevision: 'catalog-a',
    fetchedAtMs: Date.now(),
    expiresAtMs: Date.now() + 300_000,
    models: [{ id: 'synthetic-model-a' }, { id: 'synthetic-model-b' }],
    error: null,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function Picker({
  scopeKey = 'owner-a',
  disabled = false,
  connectionPending = false,
  initialModel = '',
}: {
  scopeKey?: string;
  disabled?: boolean;
  connectionPending?: boolean;
  initialModel?: string;
}) {
  const [model, setModel] = useState(initialModel);
  const [customModel, setCustomModel] = useState('');
  return (
    <ChatGptModelSelect
      id="test-model"
      scopeKey={scopeKey}
      model={model}
      customModel={customModel}
      disabled={disabled}
      connectionPending={connectionPending}
      onModelChange={setModel}
      onCustomModelChange={setCustomModel}
    />
  );
}

beforeEach(() => {
  vi.spyOn(ChatGptClient.prototype, 'models').mockResolvedValue(snapshot());
  vi.spyOn(ChatGptClient.prototype, 'refreshModels').mockResolvedValue(
    snapshot({ catalogRevision: 'catalog-b' }),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe('ChatGPT account model picker', () => {
  it('m49_chatgpt_models_receives_metadata_without_selecting_or_refreshing_until_explicit_action', async () => {
    const user = userEvent.setup();
    render(<Picker />);
    expect(
      await screen.findByRole('option', {
        name: 'synthetic-model-a',
        exact: true,
      }),
    ).toBeVisible();
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('');
    expect(ChatGptClient.prototype.models).toHaveBeenCalledOnce();
    expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model' }),
      'synthetic-model-b',
    );
    await user.click(screen.getByRole('button', { name: 'Refresh models' }));
    await waitFor(() =>
      expect(ChatGptClient.prototype.refreshModels).toHaveBeenCalledOnce(),
    );
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'synthetic-model-b',
    );
    expect(
      screen.getByText(/may still be unavailable for generation/),
    ).toBeVisible();
  });

  it.each([
    ['empty', 'No visible models were returned.'],
    ['stale', 'Model metadata is stale.'],
    ['unavailable', 'Model metadata is unavailable.'],
    ['disconnected', 'Connect ChatGPT to refresh model metadata.'],
    ['unrequested', 'Model metadata has not been refreshed.'],
  ] as const)(
    'm49_chatgpt_models_%s_preserves_an_unlisted_current_value_and_custom_entry',
    async (state, message) => {
      vi.mocked(ChatGptClient.prototype.models).mockResolvedValue(
        snapshot({ state, models: [] }),
      );
      const user = userEvent.setup();
      render(<Picker initialModel="owner-saved/Exact:ID" />);
      expect(
        await screen.findByText(new RegExp(message.replaceAll('.', '\\.'))),
      ).toBeVisible();
      expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
        'owner-saved/Exact:ID',
      );
      expect(
        screen.getByRole('option', {
          name: 'owner-saved/Exact:ID (unverified)',
        }),
      ).toBeVisible();
      await user.selectOptions(
        screen.getByRole('combobox', { name: 'Model' }),
        '__custom__',
      );
      await user.type(
        screen.getByRole('textbox', { name: 'Custom model' }),
        'My.custom/Exact:ID',
      );
      expect(screen.getByRole('textbox', { name: 'Custom model' })).toHaveValue(
        'My.custom/Exact:ID',
      );
      expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
    },
  );

  it('m49_chatgpt_models_refresh_failure_reconciles_with_get_without_replaying_post_or_exposing_raw_error', async () => {
    vi.mocked(ChatGptClient.prototype.refreshModels).mockRejectedValue(
      new Error('private-provider-response'),
    );
    vi.mocked(ChatGptClient.prototype.models)
      .mockResolvedValueOnce(snapshot())
      .mockResolvedValue(snapshot({ state: 'stale' }));
    const user = userEvent.setup();
    render(<Picker initialModel="synthetic-model-a" />);
    await screen.findByText(/Received model metadata/);
    await user.click(screen.getByRole('button', { name: 'Refresh models' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'could not be confirmed',
    );
    await screen.findByText(/Model metadata is stale/);
    expect(ChatGptClient.prototype.refreshModels).toHaveBeenCalledOnce();
    expect(ChatGptClient.prototype.models).toHaveBeenCalledTimes(2);
    expect(
      screen.queryByText(/private-provider-response/),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'synthetic-model-a',
    );
  });

  it('m49_chatgpt_models_old_daemon_remains_custom_unverified_without_static_fallback', async () => {
    vi.mocked(ChatGptClient.prototype.models).mockRejectedValue(
      new DaemonHttpError(404, { error: 'Not found' }),
    );
    const user = userEvent.setup();
    render(<Picker />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Model metadata is unavailable.',
    );
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('');
    expect(
      screen.queryByRole('option', { name: 'gpt-5.5' }),
    ).not.toBeInTheDocument();
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model' }),
      '__custom__',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Custom model' }),
      'Owner.Explicit',
    );
    expect(ChatGptClient.prototype.models).toHaveBeenCalledOnce();
    expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
  });

  it('m49_chatgpt_models_ignores_late_owner_aba_and_auth_scope_responses', async () => {
    const old = deferred<ChatGptModelsSnapshot>();
    const middle = deferred<ChatGptModelsSnapshot>();
    vi.mocked(ChatGptClient.prototype.models)
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(middle.promise)
      .mockResolvedValue(
        snapshot({
          models: [{ id: 'current-a' }],
          connectionRevision: 'a-new',
        }),
      );
    const view = render(<Picker />);
    view.rerender(<Picker scopeKey="owner-b" />);
    view.rerender(<Picker scopeKey="owner-a" />);
    expect(
      await screen.findByRole('option', { name: 'current-a' }),
    ).toBeVisible();
    await act(async () => {
      old.resolve(snapshot({ models: [{ id: 'obsolete-a' }] }));
      middle.resolve(snapshot({ models: [{ id: 'obsolete-b' }] }));
    });
    expect(
      screen.queryByRole('option', { name: /obsolete-/ }),
    ).not.toBeInTheDocument();
    view.rerender(
      <Picker scopeKey="owner-a:auth-mutation" connectionPending />,
    );
    expect(
      screen.queryByRole('option', { name: 'current-a' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Refresh models' }),
    ).toBeDisabled();
    expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
  });

  it('m49_chatgpt_models_drops_late_refresh_on_unmount_and_does_not_repeat_double_click', async () => {
    const late = deferred<ChatGptModelsSnapshot>();
    vi.mocked(ChatGptClient.prototype.refreshModels).mockReturnValue(
      late.promise,
    );
    const view = render(<Picker initialModel="synthetic-model-a" />);
    await screen.findByText(/Received model metadata/);
    const refresh = screen.getByRole('button', { name: 'Refresh models' });
    fireEvent.click(refresh);
    fireEvent.click(refresh);
    expect(ChatGptClient.prototype.refreshModels).toHaveBeenCalledOnce();
    const signal = vi.mocked(ChatGptClient.prototype.refreshModels).mock
      .calls[0][0]?.signal;
    view.unmount();
    expect(signal?.aborted).toBe(true);
    render(<Picker scopeKey="owner-b" initialModel="owner-b-current" />);
    await screen.findByText(/Received model metadata/);
    await act(async () =>
      late.resolve(snapshot({ models: [{ id: 'old-refresh-model' }] })),
    );
    expect(
      screen.queryByRole('option', { name: 'old-refresh-model' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'owner-b-current',
    );
    expect(ChatGptClient.prototype.models).toHaveBeenCalledTimes(2);
  });

  it('m49_chatgpt_models_save_lock_fences_refresh_and_preserves_custom_draft', async () => {
    const late = deferred<ChatGptModelsSnapshot>();
    vi.mocked(ChatGptClient.prototype.refreshModels).mockReturnValue(
      late.promise,
    );
    const user = userEvent.setup();
    const view = render(<Picker />);
    await screen.findByText(/Received model metadata/);
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model' }),
      '__custom__',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Custom model' }),
      'Exact.unsaved',
    );
    await user.click(screen.getByRole('button', { name: 'Refresh models' }));
    view.rerender(<Picker disabled />);
    expect(
      screen.getByRole('textbox', { name: 'Custom model' }),
    ).toBeDisabled();
    await act(async () =>
      late.resolve(snapshot({ models: [{ id: 'late-during-save' }] })),
    );
    view.rerender(<Picker />);
    await screen.findByText(/Received model metadata/);
    expect(screen.getByRole('textbox', { name: 'Custom model' })).toHaveValue(
      'Exact.unsaved',
    );
    expect(
      screen.queryByRole('option', { name: 'late-during-save' }),
    ).not.toBeInTheDocument();
    expect(ChatGptClient.prototype.refreshModels).toHaveBeenCalledOnce();
  });

  it('m49_chatgpt_models_marks_expired_metadata_stale_without_background_requests', async () => {
    vi.useFakeTimers();
    render(<Picker initialModel="synthetic-model-a" />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText(/Received model metadata/)).toBeVisible();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300_001);
    });
    expect(screen.getByText(/Model metadata is stale/)).toBeVisible();
    expect(ChatGptClient.prototype.models).toHaveBeenCalledOnce();
    expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
  });
});

describe('ChatGptModelSelect after a connection made elsewhere', () => {
  beforeEach(() => {
    vi.spyOn(ChatGptClient.prototype, 'models');
    vi.spyOn(ChatGptClient.prototype, 'refreshModels');
  });
  afterEach(() => vi.restoreAllMocks());

  it('re-reads instead of staying stuck when the picker last saw no connection', async () => {
    vi.mocked(ChatGptClient.prototype.models)
      .mockResolvedValueOnce(snapshot({ state: 'disconnected', models: [] }))
      .mockResolvedValueOnce(snapshot({ state: 'stale' }));
    const user = userEvent.setup();
    render(<Picker />);
    await screen.findByText(/Connect ChatGPT to refresh model metadata/);
    const refresh = screen.getByRole('button', { name: 'Refresh models' });
    expect(refresh).toBeEnabled();
    await user.click(refresh);
    expect(
      await screen.findByRole('option', { name: 'synthetic-model-a (unverified)' }),
    ).toBeInTheDocument();
    expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
  });
});
