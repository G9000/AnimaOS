import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AgentDetail } from '../../lib/types';
import {
  daemon,
  toAgentDetail,
  type AgentUpdateInput,
  type DaemonProvider,
  type DaemonSnapshot,
} from '../../lib/daemon-api';
import type { DelegationPolicy } from '@animaos/sdk';
import userEvent from '@testing-library/user-event';
import { DelegationSettings } from './DelegationSettings';
import { SettingsPanel } from '../SettingsPanel';

vi.mock('./ProviderKeys', () => ({ ProviderKeys: () => null }));
vi.mock('./SideModel', () => ({ SideModel: () => null }));

const agent: AgentDetail = {
  id: 'agent-main',
  name: 'Anima',
  provider: 'openai',
  model: 'gpt-4.1',
  toolNames: [],
  created_at_ms: 1,
  status: 'Idle',
  token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  messages: [],
};

beforeEach(() => {
  vi.spyOn(daemon, 'capabilities').mockResolvedValue({
    schemaVersion: 1,
    tools: [],
    persistence: {
      controlPlane: 'json',
      memory: 'ephemeral',
      executionJournal: false,
    },
    extensions: [],
    limitations: [],
  });
});
afterEach(() => vi.restoreAllMocks());

const makeSave = () =>
  vi.fn<(patch: AgentUpdateInput) => Promise<boolean>>(async () => true);

function panel(
  current: AgentDetail,
  section: 'model' | 'access',
  save = makeSave(),
) {
  return (
    <SettingsPanel
      agent={current}
      providers={[]}
      saving={false}
      resetting={false}
      saveError={null}
      resetError={null}
      saveSettings={save}
      resetAgent={vi.fn()}
      sections={[section]}
    />
  );
}

const BACKGROUND_TOOLS = [
  'spawn_helper_async',
  'spawn_helpers',
  'wait_helper_runs',
  'list_helper_runs',
  'steer_helper',
  'stop_helper',
];

describe('Helper routing and limits', () => {
  it('offers owner delegation controls on the model settings page', () => {
    render(
      <SettingsPanel
        agent={agent}
        providers={[]}
        saving={false}
        resetting={false}
        saveError={null}
        resetError={null}
        saveSettings={vi.fn(async () => true)}
        resetAgent={vi.fn()}
        sections={['model']}
      />,
    );
    expect(
      screen.getByRole('heading', { name: 'Helper routing and limits' }),
    ).toBeVisible();
  });
  it('grants background helper tools only after the owner changes the separate ability', async () => {
    const save = makeSave();
    render(
      panel(
        { ...agent, toolNames: ['read_file', 'custom_tool'] },
        'access',
        save,
      ),
    );
    fireEvent.click(screen.getByLabelText('Allow background helpers'));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save.mock.calls[0][0]).toEqual({
      tools: ['read_file', 'custom_tool', ...BACKGROUND_TOOLS],
    });
  });

  it('preserves a partial legacy helper grant when another access setting changes', async () => {
    const save = vi.fn(async () => true);
    render(
      panel(
        { ...agent, toolNames: ['read_file', 'spawn_helper_async'] },
        'access',
        save,
      ),
    );
    expect(screen.getByLabelText('Allow background helpers')).not.toBeChecked();
    fireEvent.click(screen.getByRole('radio', { name: /Observe/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    const patch = save.mock.calls[0][0] as { tools: string[] };
    expect(
      patch.tools.filter((tool) => BACKGROUND_TOOLS.includes(tool)),
    ).toEqual(['spawn_helper_async']);
  });

  it('removes only the background helper group when the owner turns it off', async () => {
    const save = makeSave();
    render(
      panel(
        {
          ...agent,
          toolNames: ['read_file', 'custom_tool', ...BACKGROUND_TOOLS],
        },
        'access',
        save,
      ),
    );
    expect(screen.getByLabelText('Allow background helpers')).toBeChecked();
    fireEvent.click(screen.getByLabelText('Allow background helpers'));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        tools: ['read_file', 'custom_tool'],
      }),
    );
  });

  it('keeps background helper permission unavailable for helper agents', async () => {
    await act(async () => {
      render(panel({ ...agent, workspaceRole: 'helper' }, 'access'));
    });
    expect(screen.getByLabelText('Allow background helpers')).toBeDisabled();
  });
});

const PROVIDERS: DaemonProvider[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    configured: true,
    requiresKey: true,
    apiKeyEnvs: [],
  },
  {
    id: 'anthropic',
    label: 'Anthropic',
    configured: true,
    requiresKey: true,
    apiKeyEnvs: [],
  },
  {
    id: 'google',
    label: 'Google',
    configured: true,
    requiresKey: true,
    apiKeyEnvs: [],
  },
];
const PRIMARY = { provider: 'openai', model: 'gpt-4.1' };
const DEFAULTS: DelegationPolicy = {
  maxConcurrent: 4,
  maxStartsPerRun: 4,
  maxDepth: 1,
  allowContextFork: false,
  maxToolIterations: 8,
  timeoutMs: 120000,
  maxOutputTokens: 4096,
  routes: [],
};
function editor(
  policy?: DelegationPolicy,
  save = vi.fn<(policy: DelegationPolicy) => Promise<boolean>>(
    async () => true,
  ),
  disabled = false,
  key = 'main',
) {
  return (
    <DelegationSettings
      key={key}
      primary={PRIMARY}
      policy={policy}
      providers={PROVIDERS}
      disabled={disabled}
      save={save}
    />
  );
}

describe('DelegationSettings draft', () => {
  it('shows finite defaults and focused-helper guidance without unsupported controls', () => {
    render(editor());
    expect(screen.getByLabelText('Concurrent helpers')).toHaveValue(4);
    expect(screen.getByLabelText('Helpers per run')).toHaveValue(4);
    expect(screen.getByLabelText('Tool turns per helper')).toHaveValue(8);
    expect(screen.getByLabelText('Time limit (seconds)')).toHaveValue(120);
    expect(screen.getByLabelText('Output tokens per reply')).toHaveValue(4096);
    expect(screen.getByText(/without the main personality/)).toBeVisible();
    expect(
      screen.queryByLabelText(/context fork|maximum depth/i),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Save helper settings' }),
    ).toBeDisabled();
  });

  it('saves routing policy without granting tools or saving another settings draft', async () => {
    const save = makeSave();
    render(
      <SettingsPanel
        agent={{ ...agent, toolNames: ['read_file'] }}
        providers={[]}
        saving={false}
        resetting={false}
        saveError={null}
        resetError={null}
        saveSettings={save}
        resetAgent={vi.fn()}
        sections={['model', 'access']}
      />,
    );
    fireEvent.click(screen.getByLabelText('Allow background helpers'));
    fireEvent.change(screen.getByLabelText('Concurrent helpers'), {
      target: { value: '6' },
    });
    fireEvent.click(
      screen.getByRole('button', { name: 'Save helper settings' }),
    );
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        settings: { delegation: { ...DEFAULTS, maxConcurrent: 6 } },
      }),
    );
    expect(save).toHaveBeenCalledOnce();
    expect(
      screen.getByRole('button', { name: 'Save helper settings' }),
    ).toBeDisabled();
  });

  it('keeps saved custom providers and models and persists fallback order with one policy save', async () => {
    const user = userEvent.setup();
    const save = vi.fn<(policy: DelegationPolicy) => Promise<boolean>>(
      async () => true,
    );
    render(
      editor(
        {
          routes: [
            {
              id: 'research',
              provider: 'private-provider',
              model: 'custom-research',
              fallbacks: [
                { provider: 'anthropic', model: 'claude-sonnet-4-6' },
                { provider: 'google', model: 'gemini-2.5-pro' },
              ],
            },
          ],
        },
        save,
      ),
    );
    expect(screen.getByLabelText('Route 1 provider')).toHaveValue(
      'private-provider',
    );
    expect(screen.getByLabelText('Route 1 custom model')).toHaveValue(
      'custom-research',
    );
    await user.click(
      screen.getByRole('button', { name: 'Move route 1 fallback 2 up' }),
    );
    expect(save).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole('button', { name: 'Save helper settings' }),
    );
    expect(save).toHaveBeenCalledWith({
      ...DEFAULTS,
      routes: [
        {
          id: 'research',
          provider: 'private-provider',
          model: 'custom-research',
          fallbacks: [
            { provider: 'google', model: 'gemini-2.5-pro' },
            { provider: 'anthropic', model: 'claude-sonnet-4-6' },
          ],
        },
      ],
    });
  });

  it('adds named routes with selectable and custom models and limits their fallback count', async () => {
    const user = userEvent.setup();
    const save = vi.fn<(policy: DelegationPolicy) => Promise<boolean>>(
      async () => true,
    );
    render(editor(undefined, save));
    await user.click(screen.getByRole('button', { name: 'Add route' }));
    await user.selectOptions(
      screen.getByLabelText('Route 1 model'),
      '__custom__',
    );
    await user.clear(screen.getByLabelText('Route 1 custom model'));
    await user.type(
      screen.getByLabelText('Route 1 custom model'),
      'fast-model',
    );
    for (let i = 0; i < 3; i += 1) {
      await user.click(
        screen.getByRole('button', { name: 'Add route 1 fallback' }),
      );
      await user.selectOptions(
        screen.getByLabelText('Route 1 fallback ' + (i + 1) + ' model'),
        '__custom__',
      );
      const input = screen.getByLabelText(
        'Route 1 fallback ' + (i + 1) + ' custom model',
      );
      await user.clear(input);
      await user.type(input, 'backup-' + i);
    }
    expect(
      screen.queryByRole('button', { name: 'Add route 1 fallback' }),
    ).not.toBeInTheDocument();
    expect(save).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole('button', { name: 'Save helper settings' }),
    );
    expect(save).toHaveBeenCalledWith({
      ...DEFAULTS,
      routes: [
        {
          id: 'fast',
          provider: 'openai',
          model: 'fast-model',
          fallbacks: [
            { provider: 'anthropic', model: 'backup-0' },
            { provider: 'anthropic', model: 'backup-1' },
            { provider: 'anthropic', model: 'backup-2' },
          ],
        },
      ],
    });
  });

  it('rejects invalid or duplicate names, duplicate models, and out-of-range limits', () => {
    render(
      editor({
        routes: [
          { id: 'research', ...PRIMARY },
          { id: 'research', ...PRIMARY },
        ],
      }),
    );
    expect(screen.getByText('Route names must be unique.')).toBeVisible();
    fireEvent.change(screen.getByLabelText('Route 2 name'), {
      target: { value: 'Bad name' },
    });
    expect(screen.getByText(/Route 2 needs a name starting/)).toBeVisible();
    fireEvent.change(screen.getByLabelText('Route 2 name'), {
      target: { value: 'fast' },
    });
    fireEvent.change(screen.getByLabelText('Concurrent helpers'), {
      target: { value: '9' },
    });
    expect(
      screen.getByText(
        'Concurrent helpers must be a whole number from 1 to 8.',
      ),
    ).toBeVisible();
    fireEvent.change(screen.getByLabelText('Concurrent helpers'), {
      target: { value: '1.5' },
    });
    expect(
      screen.getByRole('button', { name: 'Save helper settings' }),
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Concurrent helpers'), {
      target: { value: '6' },
    });
    fireEvent.click(
      screen.getByRole('button', { name: 'Add route 1 fallback' }),
    );
    fireEvent.change(screen.getByLabelText('Route 1 fallback 1 provider'), {
      target: { value: 'openai' },
    });
    fireEvent.change(screen.getByLabelText('Route 1 fallback 1 model'), {
      target: { value: 'gpt-4.1' },
    });
    expect(
      screen.getByText(
        'Route 1 must use different models for its primary and fallbacks.',
      ),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Save helper settings' }),
    ).toBeDisabled();
  });

  it('bounds the number of named routes and supports removing them', () => {
    render(
      editor({
        routes: Array.from({ length: 8 }, (_, i) => ({
          id: 'route-' + i,
          ...PRIMARY,
        })),
      }),
    );
    expect(
      screen.queryByRole('button', { name: 'Add route' }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Remove route 8' }));
    expect(screen.getByRole('button', { name: 'Add route' })).toBeVisible();
  });

  it('retains a failed draft for deliberate retry and blocks duplicate pending saves', async () => {
    let finish: (value: boolean) => void = () => undefined;
    const save = vi.fn<(policy: DelegationPolicy) => Promise<boolean>>(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    render(editor(undefined, save));
    fireEvent.change(screen.getByLabelText('Helpers per run'), {
      target: { value: '5' },
    });
    fireEvent.click(
      screen.getByRole('button', { name: 'Save helper settings' }),
    );
    expect(screen.getByLabelText('Helpers per run')).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Saving helper settings…' }),
    ).toBeDisabled();
    expect(save).toHaveBeenCalledOnce();
    await act(async () => finish(false));
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Your changes are still here.',
    );
    expect(screen.getByLabelText('Helpers per run')).toHaveValue(5);
    expect(
      screen.getByRole('button', { name: 'Save helper settings' }),
    ).toBeEnabled();
    fireEvent.click(
      screen.getByRole('button', { name: 'Discard helper changes' }),
    );
    expect(screen.getByLabelText('Helpers per run')).toHaveValue(4);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('resets all drafts on an agent transition and ignores an old save completion', async () => {
    let rejectSave: (reason: Error) => void = () => undefined;
    const save = vi.fn<(policy: DelegationPolicy) => Promise<boolean>>(
      () =>
        new Promise((_resolve, reject) => {
          rejectSave = reject;
        }),
    );
    const view = render(editor(undefined, save));
    fireEvent.change(screen.getByLabelText('Concurrent helpers'), {
      target: { value: '7' },
    });
    fireEvent.click(
      screen.getByRole('button', { name: 'Save helper settings' }),
    );
    view.rerender(
      editor({ maxConcurrent: 2 }, undefined, false, 'other-agent'),
    );
    await act(async () => rejectSave(new Error('private transport details')));
    expect(screen.getByLabelText('Concurrent helpers')).toHaveValue(2);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('disables routing and limit changes when the surrounding settings are busy', () => {
    const save = vi.fn<(policy: DelegationPolicy) => Promise<boolean>>(
      async () => true,
    );
    render(editor(undefined, save, true));
    expect(screen.getByLabelText('Concurrent helpers')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add route' })).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Save helper settings' }),
    ).toBeDisabled();
    expect(save).not.toHaveBeenCalled();
  });

  it('retains typed owner policy in the agent snapshot mapping', () => {
    const policy = { ...DEFAULTS, routes: [{ id: 'fast', ...PRIMARY }] };
    const snapshot: DaemonSnapshot = {
      state: {
        id: 'main',
        name: 'Anima',
        status: 'idle',
        createdAtMs: 1,
        tokenUsage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
        config: { name: 'Anima', ...PRIMARY, settings: { delegation: policy } },
      },
      messages: [],
      messageCount: 0,
      eventCount: 0,
    };
    expect(toAgentDetail(snapshot).delegation).toEqual(policy);
  });
});
