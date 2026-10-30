import {
  fireEvent,
  act,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatGptClient, type ChatGptModelsSnapshot } from '@animaos/sdk';

import {
  ACCESS_PROFILES,
  toolNamesForProfile,
  type AccessProfile,
} from '../lib/agent-access';
import {
  daemon,
  type AgentUpdateInput,
  type DaemonProvider,
} from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import { SettingsPanel, type SettingsSection } from './SettingsPanel';

// Preview transport/lifecycle are covered by AgentFilesPanel.test.tsx. Keep
// settings focus and save tests independent of an owner network request.
vi.mock('./AgentFilesPanel', () => ({
  AgentFilesPanel: ({ agentId }: { agentId: string }) => (
    <section aria-label="Agent files" data-agent-id={agentId} />
  ),
}));
// The active editor owns its read/save lifecycle, covered in PersonalityEditor.test.tsx.
vi.mock('./personality/PersonalityEditor', () => ({
  PersonalityEditor: ({
    agentId,
    disabled,
  }: {
    agentId: string;
    disabled: boolean;
  }) => (
    <section
      aria-label="Active personality editor"
      data-agent-id={agentId}
      data-disabled={String(disabled)}
    />
  ),
}));

it('renders the active personality editor instead of editing an inactive legacy prompt', () => {
  renderPanel(agent({ system: 'This can be shadowed by SOUL.md.' }), {
    sections: ['profile'],
  });
  expect(
    screen.getByRole('region', { name: 'Active personality editor' }),
  ).toHaveAttribute('data-agent-id', 'agent-main');
  expect(screen.queryByLabelText('System prompt')).not.toBeInTheDocument();
  expect(
    screen.queryByRole('radio', { name: 'Direct & practical' }),
  ).not.toBeInTheDocument();
});

const providers: DaemonProvider[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    requiresKey: true,
    configured: true,
    apiKeyEnvs: ['OPENAI_API_KEY'],
  },
  {
    id: 'anthropic',
    label: 'Anthropic',
    requiresKey: true,
    configured: true,
    apiKeyEnvs: ['ANTHROPIC_API_KEY'],
  },
];

// The model page also lists API keys and the side-task model; keep these
// tests independent of those owner reads.
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
    programmaticTools: {
      status: 'configured',
      mode: 'one_shot',
      interpreterEnvironment: 'ANIMAOS_RS_TOOL_RPC_PYTHON_PATH',
    },
  });
  vi.spyOn(daemon, 'providerKeys').mockResolvedValue({ providers: [] });
  vi.spyOn(daemon, 'sideModel').mockResolvedValue(null);
});

it('grants programmatic chains separately while preserving current custom permissions', async () => {
  const current = agent({ toolNames: ['read_file', 'bash', 'legacy_custom'] });
  const saveSettings = vi.fn(async () => true);
  renderPanel(current, { sections: ['access', 'builtins'], saveSettings });
  const toggle = await screen.findByLabelText('Allow programmatic tool chains');
  fireEvent.click(toggle);
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() =>
    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...current.toolNames, 'execute_code'],
    }),
  );
  expect(screen.getByText(/ANIMAOS_RS_TOOL_RPC_PYTHON_PATH/)).toBeVisible();
});
it('keeps tool chains unavailable without the existing shell grant', async () => {
  vi.mocked(daemon.capabilities).mockResolvedValue({
    schemaVersion: 1,
    tools: [],
    persistence: {
      controlPlane: 'json',
      memory: 'ephemeral',
      executionJournal: false,
    },
    extensions: [],
    limitations: [],
    programmaticTools: {
      status: 'unconfigured',
      mode: 'one_shot',
      interpreterEnvironment: 'ANIMAOS_RS_TOOL_RPC_PYTHON_PATH',
    },
  });
  renderPanel(agent({ toolNames: ['read_file'] }), { sections: ['access', 'builtins'] });
  expect(
    screen.getByLabelText('Allow programmatic tool chains'),
  ).toBeDisabled();
  await waitFor(() =>
    expect(screen.getByText(/Python is not configured/)).toBeVisible(),
  );
});
afterEach(() => vi.restoreAllMocks());

it('grants existing-document editing only by an explicit separate switch', async () => {
  const saveSettings = vi.fn(async () => true);
  const current = agent({ toolNames: ['read_file', 'custom_permission'] });
  renderPanel(current, { sections: ['builtins'], saveSettings });
  const editing = await screen.findByRole('switch', { name: 'Edit existing documents' });
  expect(editing).not.toBeChecked();
  fireEvent.click(editing);
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(saveSettings).toHaveBeenCalledWith({ tools: [
    ...current.toolNames, 'inspect_office_document', 'propose_document_edit', 'apply_document_edit',
  ] }));
  for (const profile of ['observe', 'collaborate', 'operate'] as const)
    expect(toolNamesForProfile(profile)).not.toContain('apply_document_edit');
});

it('preserves a partial document grant across access-profile changes', async () => {
  const saveSettings = vi.fn(async () => true);
  renderPanel(agent({ toolNames: [...toolNamesForProfile('collaborate'), 'inspect_office_document'] }), { sections: ['access', 'builtins'], saveSettings });
  fireEvent.click(screen.getByRole('radio', { name: /^Look only/ }));
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(saveSettings).toHaveBeenCalledWith({ tools: [...toolNamesForProfile('observe'), 'inspect_office_document'] }));
});

it('does not offer document-editing grants to helpers', async () => {
  renderPanel(agent({ workspaceRole: 'helper', toolNames: [] }), { sections: ['builtins'] });
  expect(await screen.findByRole('switch', { name: 'Edit existing documents' })).toBeDisabled();
});

it('grants finished document creation only when the owner enables Files & documents', async () => {
  const saveSettings = vi.fn(async () => true);
  renderPanel(agent({ toolNames: ['read_file', 'custom_permission'] }), {
    sections: ['builtins'],
    saveSettings,
  });
  fireEvent.click(await screen.findByRole('switch', { name: 'Files & documents' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(expect.objectContaining({
    tools: expect.arrayContaining(['read_file', 'custom_permission', 'write_document', 'read_document_source']),
  })));
});

function agent(overrides: Partial<AgentDetail> = {}): AgentDetail {
  return {
    id: 'agent-main',
    name: 'Nova',
    provider: 'openai',
    model: 'gpt-4.1',
    toolNames: toolNamesForProfile('collaborate'),
    created_at_ms: 1,
    status: 'Idle',
    token_usage: {
      prompt_tokens: 1,
      completion_tokens: 2,
      total_tokens: 3,
    },
    system: 'Be precise',
    messages: [],
    ...overrides,
  };
}

function renderPanel(
  currentAgent: AgentDetail,
  overrides: Partial<{
    saving: boolean;
    resetting: boolean;
    saveError: string | null;
    resetError: string | null;
    saveSettings: (patch: AgentUpdateInput) => Promise<boolean>;
    resetAgent: () => void;
    sections: readonly SettingsSection[];
    isMainAgent: boolean;
  }> = {},
) {
  const props = {
    agent: currentAgent,
    providers,
    saving: false,
    resetting: false,
    saveError: null,
    resetError: null,
    saveSettings: vi.fn(async () => true),
    resetAgent: vi.fn(),
    // Every part at once, so access and the built-in tool abilities that
    // live on the Tools page can be checked together.
    sections: ['profile', 'model', 'access', 'builtins'] as const,
    ...overrides,
  };

  const view = render(<SettingsPanel {...props} />);
  revealFoldedControls(view.container);
  return { ...view, props };
}

/** The page folds the name fields behind Change and the individual
 *  abilities behind Specific permissions; open both, as the owner would. */
function revealFoldedControls(container: HTMLElement) {
  const identity = container.querySelector<HTMLButtonElement>(
    '.profile-identity-row > .settings-item-action[aria-expanded="false"]',
  );
  if (identity && !identity.disabled) fireEvent.click(identity);
  for (const details of container.querySelectorAll<HTMLDetailsElement>(
    'details.settings-tools-advanced',
  ))
    details.open = true;
}

/** Choosing an access level keeps the separate conversation-reading grant
 *  as it was; these agents never had it. */
const withoutRecall = (tools: readonly string[]) =>
  tools.filter((tool) => tool !== 'read_conversation_window');

describe('workflow chat permissions', () => {
  const workflowTools = ['create_workflow', 'list_workflows'];
  const workflowToggle = () =>
    screen.getByRole('checkbox', { name: 'Allow workflows in chat' });

  it('defaults off and grants exactly the workflow pair while preserving custom permissions', async () => {
    const original = ['read_file', 'legacy_custom', 'web_search'];
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(agent({ toolNames: original }), {
      saveSettings,
      sections: ['access', 'builtins'],
    });
    expect(workflowToggle()).not.toBeChecked();
    fireEvent.click(workflowToggle());
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({
        tools: [...original, ...workflowTools],
      }),
    );
    for (const profile of ['observe', 'collaborate', 'operate'] as const) {
      expect(toolNamesForProfile(profile)).not.toContain('create_workflow');
      expect(toolNamesForProfile(profile)).not.toContain('list_workflows');
    }
  });

  it('revokes both workflow grants and preserves unrelated grants', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(
      agent({ toolNames: ['legacy_custom', ...workflowTools, 'read_file'] }),
      {
        saveSettings,
        sections: ['access', 'builtins'],
      },
    );
    expect(workflowToggle()).toBeChecked();
    fireEvent.click(workflowToggle());
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({
        tools: ['legacy_custom', 'read_file'],
      }),
    );
  });

  it.each([false, true])(
    'keeps an explicitly enabled workflow grant through profile selection (stored: %s)',
    async (stored) => {
      const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
      renderPanel(
        agent({
          toolNames: [
            ...toolNamesForProfile('collaborate'),
            ...(stored ? workflowTools : []),
          ],
        }),
        {
          saveSettings,
          sections: ['access', 'builtins'],
        },
      );
      expect(screen.getByRole('radio', { name: /^Edit files/ })).toBeChecked();
      if (!stored) fireEvent.click(workflowToggle());
      fireEvent.click(screen.getByRole('radio', { name: /^Look only/ }));
      expect(workflowToggle()).toBeChecked();
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenCalledWith({
          tools: [...toolNamesForProfile('observe'), ...workflowTools],
        }),
      );
    },
  );

  it.each(workflowTools)(
    'preserves a partial %s grant on unrelated saves without completing the pair',
    async (partial) => {
      const original = [
        ...toolNamesForProfile('collaborate'),
        partial,
        'legacy_custom',
      ];
      const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
      renderPanel(agent({ toolNames: original }), { saveSettings });
      expect(workflowToggle()).not.toBeChecked();
      fireEvent.change(screen.getByLabelText('Name'), {
        target: { value: 'Workflow Nova' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenLastCalledWith({
          name: 'Workflow Nova',
        }),
      );
      fireEvent.click(
        screen.getByRole('checkbox', { name: 'Allow removing saved facts' }),
      );
      fireEvent.click(
        screen.getByRole('button', { name: /^(Save changes|Saved ✓)$/ }),
      );
      await waitFor(() =>
        expect(saveSettings).toHaveBeenLastCalledWith({
          name: 'Workflow Nova',
          tools: [...original, 'memory_remove'],
        }),
      );
    },
  );

  it.each(workflowTools)(
    'preserves a partial %s grant through profile changes until the workflow toggle is changed',
    async (partial) => {
      const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
      renderPanel(
        agent({ toolNames: [...toolNamesForProfile('collaborate'), partial] }),
        { saveSettings, sections: ['access', 'builtins'] },
      );
      expect(workflowToggle()).not.toBeChecked();
      fireEvent.click(screen.getByRole('radio', { name: /^Look only/ }));
      expect(workflowToggle()).not.toBeChecked();
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenCalledWith({
          tools: [...toolNamesForProfile('observe'), partial],
        }),
      );
    },
  );

  it.each(workflowTools)(
    'completes a partial %s grant only after opting in',
    async (partial) => {
      const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
      renderPanel(agent({ toolNames: ['legacy_custom', partial] }), {
        saveSettings,
        sections: ['access', 'builtins'],
      });
      expect(workflowToggle()).not.toBeChecked();
      fireEvent.click(workflowToggle());
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenCalledWith({
          tools: [
            'legacy_custom',
            partial,
            ...workflowTools.filter((tool) => tool !== partial),
          ],
        }),
      );
    },
  );

  it('can revoke a partial grant by toggling on and back off', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(agent({ toolNames: ['legacy_custom', 'create_workflow'] }), {
      saveSettings,
      sections: ['access', 'builtins'],
    });
    fireEvent.click(workflowToggle());
    fireEvent.click(workflowToggle());
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({ tools: ['legacy_custom'] }),
    );
  });

  it('disables workflow permissions for helpers', () => {
    renderPanel(agent({ workspaceRole: 'helper' }), { sections: ['access', 'builtins'] });
    expect(workflowToggle()).toBeDisabled();
  });

  it('reseeds workflow drafts on stored tool changes and agent changes', () => {
    const original = agent({ toolNames: ['read_file'] });
    const view = renderPanel(original, { sections: ['access', 'builtins'] });
    fireEvent.click(workflowToggle());
    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={{ ...original, toolNames: ['read_file', 'create_workflow'] }}
      />,
    );
    expect(workflowToggle()).not.toBeChecked();
    expect(screen.getByRole('button', { name: 'No changes' })).toBeDisabled();
    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={agent({
          id: 'another-agent',
          toolNames: [...toolNamesForProfile('observe'), ...workflowTools],
        })}
      />,
    );
    expect(workflowToggle()).toBeChecked();
    expect(screen.getByRole('radio', { name: /^Look only/ })).toBeChecked();
    expect(screen.getByRole('button', { name: 'No changes' })).toBeDisabled();
  });
});

describe('model resilience settings', () => {
  it('saves fallback models through the agent update, and shows keys and the side-task model', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(
      agent({
        fallbacks: [{ provider: 'anthropic', model: 'claude-haiku-4-5' }],
      }),
      { saveSettings, sections: ['model'] },
    );
    expect(screen.getByRole('heading', { name: 'API keys' })).toBeVisible();
    expect(
      screen.getByRole('heading', { name: 'Side-task model' }),
    ).toBeVisible();
    expect(screen.getByLabelText('Fallback 1 model')).toHaveValue(
      'claude-haiku-4-5',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Remove fallback 1' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save fallbacks' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({ fallbacks: [] }),
    );
  });
  it('drops an unsaved fallback draft when another agent is shown', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    const { rerender, props } = renderPanel(agent({ id: 'agent-a' }), {
      saveSettings,
      sections: ['model'],
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add fallback' }));
    expect(screen.getByLabelText('Fallback 1 model')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Save fallbacks' }),
    ).toBeEnabled();

    rerender(
      <SettingsPanel
        {...props}
        sections={['model']}
        agent={agent({ id: 'agent-b', name: 'Bo' })}
      />,
    );
    expect(screen.queryByLabelText('Fallback 1 model')).toBeNull();
    expect(
      screen.getByRole('button', { name: 'Save fallbacks' }),
    ).toBeDisabled();
    expect(saveSettings).not.toHaveBeenCalled();
  });
});

describe('m83 independent owner web permissions', () => {
  it('recognizes the saved Run commands profile with search', () => {
    renderPanel(agent({ toolNames: toolNamesForProfile('operate') }));
    expect(screen.getByRole('radio', { name: /^Run commands/ })).toBeChecked();
  });

  it('includes search and discovery when choosing Run commands', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(agent({ toolNames: ['read_file'] }), { saveSettings });
    fireEvent.click(screen.getByRole('radio', { name: /^Run commands/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => {
      const names = saveSettings.mock.calls[0][0].tools ?? [];
      expect(names).toEqual(expect.arrayContaining(['web_search', 'search_tools', 'describe_tool']));
    });
  });

  afterEach(() => vi.restoreAllMocks());

  it.each([
    ['Allow web search', 'web_search', 'Allow web page reads'],
    ['Allow web page reads', 'web_fetch', 'Allow web search'],
  ])(
    'saves only the explicitly selected %s grant without probing the network',
    async (label, tool, otherLabel) => {
      const network = vi
        .spyOn(globalThis, 'fetch')
        .mockRejectedValue(new Error('No network in owner settings'));
      const original = ['read_file', 'custom_permission', 'exa_search'];
      const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
      renderPanel(agent({ toolNames: original }), { saveSettings });
      const selected = screen.getByRole('checkbox', { name: label });
      expect(selected).not.toBeChecked();
      expect(
        screen.getByRole('checkbox', { name: otherLabel }),
      ).not.toBeChecked();
      fireEvent.click(selected);
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenCalledWith({
          tools: [...original, tool],
        }),
      );
      expect(network).not.toHaveBeenCalled();
      expect(
        screen.getByRole('checkbox', { name: otherLabel }),
      ).not.toBeChecked();
      expect(screen.getByText(/anonymous public service/i)).toBeVisible();
      expect(screen.getByText(/rate.limit/i)).toBeVisible();
    },
  );

  it('recognizes stored optional grants without widening profiles and preserves custom grants on unrelated saves', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    const original = [
      ...toolNamesForProfile('collaborate'),
      'web_search',
      'web_fetch',
    ];
    const view = renderPanel(agent({ toolNames: original }), { saveSettings });
    expect(screen.getByRole('radio', { name: /^Edit files/ })).toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow web search' }),
    ).toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow web page reads' }),
    ).toBeChecked();
    fireEvent.change(screen.getByRole('textbox', { name: 'Name' }), {
      target: { value: 'Nova with search' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({ name: 'Nova with search' }),
    );
    expect(saveSettings.mock.calls[0][0]).not.toHaveProperty('tools');
    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={agent({ toolNames: ['custom_permission', 'web_search'] })}
      />,
    );
    fireEvent.click(
      screen.getByRole('checkbox', { name: 'Allow web page reads' }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: /^(Save changes|Saved ✓)$/ }),
    );
    await waitFor(() =>
      expect(saveSettings).toHaveBeenLastCalledWith({
        tools: ['custom_permission', 'web_search', 'web_fetch'],
      }),
    );
    for (const profile of ['observe', 'collaborate'] as const) {
      expect(toolNamesForProfile(profile)).not.toContain('web_search');
    }
    expect(toolNamesForProfile('operate')).toContain('web_search');
    for (const profile of ['observe', 'collaborate', 'operate'] as const) {
      expect(toolNamesForProfile(profile)).not.toContain('web_fetch');
    }
  });

  it('clears optional web grants on deliberate profile selection and resets them on agent changes', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    const view = renderPanel(
      agent({ toolNames: ['custom_permission', 'web_search', 'web_fetch'] }),
      { saveSettings },
    );
    fireEvent.click(screen.getByRole('radio', { name: /^Look only/ }));
    expect(
      screen.getByRole('checkbox', { name: 'Allow web search' }),
    ).not.toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow web page reads' }),
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({
        tools: withoutRecall(toolNamesForProfile('observe')),
      }),
    );
    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={agent({ id: 'next-agent', toolNames: ['web_fetch'] })}
      />,
    );
    expect(
      screen.getByRole('checkbox', { name: 'Allow web search' }),
    ).not.toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow web page reads' }),
    ).toBeChecked();
    expect(
      screen.getByRole('button', { name: /^(No changes|Saved ✓)$/ }),
    ).toBeDisabled();
  });

  it('retains failed grant drafts and locks the web controls during save and reset', async () => {
    const view = renderPanel(agent(), {
      saveSettings: vi.fn(async () => false),
    });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Allow web search' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    view.rerender(
      <SettingsPanel {...view.props} saveError="Grant save failed" />,
    );
    expect(
      screen.getByRole('checkbox', { name: 'Allow web search' }),
    ).toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow web page reads' }),
    ).not.toBeChecked();
    expect(view.props.agent.toolNames).not.toContain('web_search');
    for (const busy of [{ saving: true }, { resetting: true }]) {
      view.rerender(<SettingsPanel {...view.props} {...busy} />);
      expect(
        screen.getByRole('checkbox', { name: 'Allow web search' }),
      ).toBeDisabled();
      expect(
        screen.getByRole('checkbox', { name: 'Allow web page reads' }),
      ).toBeDisabled();
    }
  });
});

describe('M49 Settings subscription models', () => {
  function metadata(
    overrides: Partial<ChatGptModelsSnapshot> = {},
  ): ChatGptModelsSnapshot {
    return {
      state: 'ready',
      connectionRevision: 'connection-a',
      catalogRevision: 'catalog-a',
      fetchedAtMs: Date.now(),
      expiresAtMs: Date.now() + 300_000,
      models: [{ id: 'synthetic-model-a' }],
      error: null,
      ...overrides,
    };
  }
  beforeEach(() => {
    vi.spyOn(ChatGptClient.prototype, 'status').mockResolvedValue({
      connected: true,
      accountId: 'synthetic-account',
      planType: null,
      login: null,
      error: null,
    });
    vi.spyOn(ChatGptClient.prototype, 'models').mockResolvedValue(metadata());
    vi.spyOn(ChatGptClient.prototype, 'refreshModels').mockResolvedValue(
      metadata(),
    );
  });
  afterEach(() => vi.restoreAllMocks());

  it('m49_chatgpt_models_settings_keeps_saved_model_dirty_fields_and_grants_during_refresh', async () => {
    const external = `mcp_${'1'.repeat(32)}_${'a'.repeat(16)}`;
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    const user = userEvent.setup();
    renderPanel(
      agent({
        provider: 'chatgpt',
        model: 'Saved.Exact/Model',
        toolNames: ['read_file', external, 'browser_observe'],
      }),
      { saveSettings },
    );
    await screen.findByText(/Received model metadata/);
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'Saved.Exact/Model',
    );
    expect(screen.getByRole('button', { name: 'No changes' })).toBeDisabled();
    let complete!: (value: ChatGptModelsSnapshot) => void;
    vi.mocked(ChatGptClient.prototype.refreshModels).mockReturnValue(
      new Promise((resolve) => {
        complete = resolve;
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Refresh models' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Name' }), {
      target: { value: 'Unsaved name' },
    });
    await act(async () =>
      complete(metadata({ models: [{ id: 'different-model' }] })),
    );
    expect(screen.getByRole('textbox', { name: 'Name' })).toHaveValue(
      'Unsaved name',
    );
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'Saved.Exact/Model',
    );
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(saveSettings).toHaveBeenCalledWith({
      name: 'Unsaved name',
    });
    expect(saveSettings.mock.calls[0][0]).not.toHaveProperty('tools');
    expect(saveSettings.mock.calls[0][0]).not.toHaveProperty('model');
  });

  it('m49_chatgpt_models_settings_clears_only_new_subscription_default_and_retains_api_defaults', async () => {
    const user = userEvent.setup();
    const view = renderPanel(agent());
    view.rerender(
      <SettingsPanel
        {...view.props}
        providers={[
          ...providers,
          {
            id: 'chatgpt',
            label: 'ChatGPT subscription',
            configured: true,
            requiresKey: false,
            apiKeyEnvs: [],
          },
        ]}
      />,
    );
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Provider' }),
      'chatgpt',
    );
    await screen.findByText(/Received model metadata/);
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model' }),
      '__custom__',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Custom model' }),
      'Owner.Custom/Exact',
    );
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(view.props.saveSettings).toHaveBeenCalledWith({
      provider: 'chatgpt',
      model: 'Owner.Custom/Exact',
    });
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Provider' }),
      'openai',
    );
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'gpt-4o',
    );
    expect(ChatGptClient.prototype.refreshModels).not.toHaveBeenCalled();
  });

  it('m49_chatgpt_models_settings_auth_and_owner_changes_fence_late_refresh_without_losing_custom_draft', async () => {
    const user = userEvent.setup();
    const view = renderPanel(
      agent({ provider: 'chatgpt', model: 'Saved.Exact' }),
    );
    await screen.findByText(/Received model metadata/);
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model' }),
      '__custom__',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Custom model' }),
      'Unsaved.Custom',
    );
    let complete!: (value: ChatGptModelsSnapshot) => void;
    vi.mocked(ChatGptClient.prototype.refreshModels).mockReturnValue(
      new Promise((resolve) => {
        complete = resolve;
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Refresh models' }));
    vi.mocked(ChatGptClient.prototype.models).mockResolvedValue(
      metadata({
        state: 'disconnected',
        models: [],
        connectionRevision: null,
        catalogRevision: null,
      }),
    );
    vi.spyOn(ChatGptClient.prototype, 'disconnect').mockResolvedValue({
      connected: false,
      accountId: null,
      planType: null,
      login: null,
      error: null,
    });
    await user.click(
      screen.getByRole('button', { name: 'Disconnect ChatGPT' }),
    );
    await screen.findByText(/Connect ChatGPT to refresh model metadata/);
    expect(screen.getByRole('textbox', { name: 'Custom model' })).toHaveValue(
      'Unsaved.Custom',
    );
    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={agent({
          id: 'agent-b',
          name: 'Other owner',
          provider: 'chatgpt',
          model: 'Other.Exact',
        })}
      />,
    );
    await act(async () =>
      complete(metadata({ models: [{ id: 'old-account-model' }] })),
    );
    // Switching agents closes the name editor and shows the new identity.
    expect(await screen.findByText('Other owner')).toBeVisible();
    expect(
      screen.queryByRole('textbox', { name: 'Name' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(
      'Other.Exact',
    );
    expect(
      screen.queryByRole('option', { name: 'old-account-model' }),
    ).not.toBeInTheDocument();
    expect(ChatGptClient.prototype.refreshModels).toHaveBeenCalledOnce();
    expect(view.props.saveSettings).not.toHaveBeenCalled();
  });
});

describe('SettingsPanel access', () => {
  it('M41_opt_in_adds_only_conversation_window_read_and_preserves_custom_grants', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'search_conversations',
      'memory_remove',
      'legacy_workspace_tool',
      'mail_list_messages',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: currentTools }), {
      saveSettings,
      sections: ['builtins'],
    });
    await user.click(screen.getByText('Specific permissions'));

    const allowHistoryRead = screen.getByRole('checkbox', {
      name: 'Allow reading conversation context',
    });
    expect(allowHistoryRead).not.toBeChecked();
    await user.click(allowHistoryRead);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...currentTools, 'read_conversation_window'],
    });
  });

  it('M41_opt_out_removes_only_conversation_window_read_from_mixed_grants', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'search_conversations',
      'read_conversation_window',
      'memory_remove',
      'legacy_workspace_tool',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: currentTools }), {
      saveSettings,
      sections: ['builtins'],
    });
    await user.click(screen.getByText('Specific permissions'));

    const allowHistoryRead = screen.getByRole('checkbox', {
      name: 'Allow reading conversation context',
    });
    expect(allowHistoryRead).toBeChecked();
    await user.click(allowHistoryRead);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: currentTools.filter((tool) => tool !== 'read_conversation_window'),
    });
  });

  describe.each([true, false])('recall profile changes, main agent: %s', (isMainAgent) => {
    it.each([
      { stored: true, draft: true },
      { stored: true, draft: false },
      { stored: false, draft: false },
      { stored: false, draft: true },
    ])('preserves recall reading from stored $stored to draft $draft', async ({ stored, draft }) => {
      const user = userEvent.setup();
      const saveSettings = vi.fn(async () => true);
      renderPanel(agent({
        toolNames: toolNamesForProfile('observe').filter(
          (tool) => tool !== 'read_conversation_window' || stored,
        ),
      }), {
        saveSettings,
        isMainAgent,
        sections: isMainAgent ? ['access', 'builtins'] : ['profile', 'builtins'],
      });
      await user.click(screen.getByText('Specific permissions'));
      const allowHistoryRead = screen.getByRole('checkbox', {
        name: 'Allow reading conversation context',
      });
      expect(allowHistoryRead).toHaveProperty('checked', stored);
      if (stored !== draft) await user.click(allowHistoryRead);

      for (const profileLabel of [/^Edit files/, /^Run commands/, /^Look only/]) {
        await user.click(screen.getByRole('radio', { name: profileLabel }));
        expect(allowHistoryRead).toHaveProperty('checked', draft);
      }
      await user.click(screen.getByRole('radio', { name: /^Edit files/ }));
      await user.click(screen.getByRole('button', { name: 'Save changes' }));
      expect(saveSettings).toHaveBeenCalledWith({
        tools: toolNamesForProfile('collaborate').filter(
          (tool) => tool !== 'read_conversation_window' || draft,
        ),
      });
    });
  });

  it('recall reading remains disabled for generated helpers through profile changes', async () => {
    const user = userEvent.setup();
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ workspaceRole: 'helper', toolNames: ['read_file'] }), {
      sections: ['profile', 'builtins'],
      saveSettings,
    });
    await user.click(screen.getByText('Specific permissions'));
    const allowHistoryRead = screen.getByRole('checkbox', {
      name: 'Allow reading conversation context',
    });
    expect(allowHistoryRead).toBeDisabled();
    await user.click(screen.getByRole('radio', { name: /^Edit files/ }));
    expect(allowHistoryRead).not.toBeChecked();
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(saveSettings).toHaveBeenCalledWith({
      tools: toolNamesForProfile('collaborate').filter(
        (tool) => tool !== 'read_conversation_window',
      ),
    });
  });

  it('M41_failed_save_keeps_conversation_read_draft_without_changing_the_agent', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: ['read_file', 'legacy_workspace_tool'],
    });
    const saveSettings = vi.fn(async () => false);
    const view = renderPanel(currentAgent, {
      saveSettings,
      sections: ['builtins'],
    });
    await user.click(screen.getByText('Specific permissions'));
    const allowHistoryRead = screen.getByRole('checkbox', {
      name: 'Allow reading conversation context',
    });

    await user.click(allowHistoryRead);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    view.rerender(
      <SettingsPanel {...view.props} saveError="Unsupported tool" />,
    );

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...currentAgent.toolNames, 'read_conversation_window'],
    });
    expect(allowHistoryRead).toBeChecked();
    expect(currentAgent.toolNames).not.toContain('read_conversation_window');
    expect(screen.getByRole('alert')).toHaveTextContent('Unsupported tool');
  });

  it('M41_helper_cannot_enable_conversation_history_read', () => {
    renderPanel(
      agent({
        workspaceRole: 'helper',
        toolNames: ['read_file'],
      }),
      { sections: ['builtins'] },
    );
    fireEvent.click(screen.getByText('Specific permissions'));
    const allowHistoryRead = screen.getByRole('checkbox', {
      name: 'Allow reading conversation context',
    });
    expect(allowHistoryRead).not.toBeChecked();
    expect(allowHistoryRead).toBeDisabled();
  });

  it('M34_opt_in_adds_only_memory_consolidate_and_preserves_existing_custom_grants', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'memory_remove',
      'legacy_workspace_tool',
      'mail_list_messages',
      'drive_read',
      'web_fetch',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });

    const consolidateFacts = screen.getByRole('checkbox', {
      name: 'Allow consolidating saved facts',
    });
    expect(consolidateFacts).not.toBeChecked();
    await user.click(consolidateFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...currentTools, 'memory_consolidate'],
    });
  });

  it('M46_browser_grants_are_separate_and_preserve_neighbors_and_partial_browser_access', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'legacy_workspace_tool',
      'browser_observe',
    ];
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });
    expect(
      screen.getByRole('checkbox', { name: 'Allow browser observation' }),
    ).toBeChecked();
    const navigate = screen.getByRole('checkbox', {
      name: 'Allow browser navigation',
    });
    expect(navigate).not.toBeChecked();
    await user.click(navigate);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    const tools = saveSettings.mock.calls[0][0].tools ?? [];
    expect([...tools].sort()).toEqual(
      [...currentTools, 'browser_navigate'].sort(),
    );
    expect(tools).not.toContain('browser_act');
    expect(tools).not.toContain('browser_transfer');
  });

  it('M34_opt_out_removes_only_memory_consolidate_from_a_mixed_grant_set', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'memory_remove',
      'memory_consolidate',
      'legacy_workspace_tool',
      'drive_read',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });

    const consolidateFacts = screen.getByRole('checkbox', {
      name: 'Allow consolidating saved facts',
    });
    expect(consolidateFacts).toBeChecked();
    await user.click(consolidateFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: currentTools.filter((tool) => tool !== 'memory_consolidate'),
    });
  });

  it('M34_standard_profile_clears_both_optional_grants_but_allows_deliberate_reenable', async () => {
    const user = userEvent.setup();
    const saveSettings = vi.fn(async () => true);
    renderPanel(
      agent({
        toolNames: [
          'read_file',
          'legacy_workspace_tool',
          'memory_remove',
          'memory_consolidate',
        ],
      }),
      { saveSettings },
    );

    const removeFacts = screen.getByRole('checkbox', {
      name: 'Allow removing saved facts',
    });
    const consolidateFacts = screen.getByRole('checkbox', {
      name: 'Allow consolidating saved facts',
    });
    await user.click(screen.getByRole('radio', { name: /^Edit files/ }));
    expect(removeFacts).not.toBeChecked();
    expect(consolidateFacts).not.toBeChecked();
    expect(toolNamesForProfile('collaborate')).not.toContain('memory_remove');
    expect(toolNamesForProfile('collaborate')).not.toContain(
      'memory_consolidate',
    );

    await user.click(consolidateFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...withoutRecall(toolNamesForProfile('collaborate')), 'memory_consolidate'],
    });
  });

  it('M34_failed_save_keeps_the_consolidation_draft_without_changing_the_agent_grant', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: ['read_file', 'legacy_workspace_tool'],
    });
    const saveSettings = vi.fn(async () => false);
    const view = renderPanel(currentAgent, { saveSettings });

    const consolidateFacts = screen.getByRole('checkbox', {
      name: 'Allow consolidating saved facts',
    });
    await user.click(consolidateFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    view.rerender(
      <SettingsPanel {...view.props} saveError="Unsupported tool" />,
    );

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...currentAgent.toolNames, 'memory_consolidate'],
    });
    expect(consolidateFacts).toBeChecked();
    expect(currentAgent.toolNames).not.toContain('memory_consolidate');
    expect(screen.getByRole('alert')).toHaveTextContent('Unsupported tool');
  });

  it('memory_removal explicitly adds only the opt-in to a custom tool set', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'legacy_workspace_tool',
      'mail_list_messages',
      'drive_read',
      'web_fetch',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });

    const removeFacts = screen.getByRole('checkbox', {
      name: 'Allow removing saved facts',
    });
    expect(removeFacts).not.toBeChecked();
    await user.click(removeFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...currentTools, 'memory_remove'],
    });
  });

  it('memory_removal opt-out preserves every other configured tool', async () => {
    const user = userEvent.setup();
    const currentTools = [
      'read_file',
      'memory_remove',
      'legacy_workspace_tool',
      'mail_list_messages',
      'drive_read',
      'web_fetch',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });

    const removeFacts = screen.getByRole('checkbox', {
      name: 'Allow removing saved facts',
    });
    expect(removeFacts).toBeChecked();
    await user.click(removeFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: currentTools.filter((tool) => tool !== 'memory_remove'),
    });
  });

  it('memory_removal stays opt-in when a standard profile replaces custom access', async () => {
    const user = userEvent.setup();
    const saveSettings = vi.fn(async () => true);
    renderPanel(
      agent({
        toolNames: ['read_file', 'legacy_workspace_tool', 'memory_remove'],
      }),
      { saveSettings },
    );

    expect(screen.getByText('Custom access')).toBeVisible();
    const removeFacts = screen.getByRole('checkbox', {
      name: 'Allow removing saved facts',
    });
    expect(removeFacts).toBeChecked();
    await user.click(screen.getByRole('radio', { name: /^Edit files/ }));
    expect(removeFacts).not.toBeChecked();
    expect(
      screen.getByText(/permanent removal of one active Fact only/i),
    ).toBeVisible();
    await user.click(removeFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...withoutRecall(toolNamesForProfile('collaborate')), 'memory_remove'],
    });
    expect(toolNamesForProfile('collaborate')).not.toContain('memory_remove');
  });

  it('memory_removal keeps an unsaved opt-in draft after a failed save', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: ['read_file', 'legacy_workspace_tool'],
    });
    const saveSettings = vi.fn(async () => false);
    const view = renderPanel(currentAgent, { saveSettings });

    const removeFacts = screen.getByRole('checkbox', {
      name: 'Allow removing saved facts',
    });
    await user.click(removeFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    view.rerender(
      <SettingsPanel {...view.props} saveError="Unsupported tool" />,
    );

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...currentAgent.toolNames, 'memory_remove'],
    });
    expect(removeFacts).toBeChecked();
    expect(currentAgent.toolNames).not.toContain('memory_remove');
    expect(screen.getByRole('alert')).toHaveTextContent('Unsupported tool');
  });

  it('memory_removal selection reseeds from an accepted tool configuration change', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: ['read_file', 'memory_remove'],
    });
    const view = renderPanel(currentAgent);
    const removeFacts = screen.getByRole('checkbox', {
      name: 'Allow removing saved facts',
    });
    expect(removeFacts).toBeChecked();
    await user.click(removeFacts);

    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={{ ...currentAgent, toolNames: ['read_file'] }}
      />,
    );

    expect(removeFacts).not.toBeChecked();

    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={{
          ...currentAgent,
          id: 'agent-next',
          toolNames: ['read_file', 'memory_remove'],
        }}
      />,
    );

    expect(removeFacts).toBeChecked();
  });

  it('shows only the sections its page is about', () => {
    const view = renderPanel(agent(), { sections: ['access', 'builtins'] });
    expect(screen.getByRole('radio', { name: /^Edit files/ })).toBeVisible();
    expect(screen.queryByRole('textbox', { name: 'Name' })).toBeNull();
    expect(screen.queryByRole('combobox', { name: 'Provider' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reset' })).toBeNull();

    view.rerender(<SettingsPanel {...view.props} sections={['model']} />);
    // The main model shows as one item; its controls open under Change model.
    expect(screen.getByTestId('main-model')).toBeVisible();
    fireEvent.click(screen.getByText('Change'));
    expect(screen.getByRole('combobox', { name: 'Provider' })).toBeVisible();
    expect(screen.queryByRole('radio')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reset' })).toBeNull();

    view.rerender(<SettingsPanel {...view.props} sections={['profile']} />);
    revealFoldedControls(view.container);
    expect(screen.getByRole('textbox', { name: 'Name' })).toBeVisible();
    expect(
      screen.getByRole('region', { name: 'Active personality editor' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Reset…' })).toBeVisible();
    expect(screen.queryByRole('combobox', { name: 'Provider' })).toBeNull();
  });

  it.each<AccessProfile>(['observe', 'collaborate', 'operate'])(
    'derives and displays the %s profile regardless of tool order',
    (profileName) => {
      renderPanel(
        agent({ toolNames: toolNamesForProfile(profileName).toReversed() }),
      );

      const copy = {
        observe: ['Look only', 'Reads files and todos. Changes nothing.'],
        collaborate: ['Edit files', 'Reads and edits files. Never runs commands.'],
        operate: [
          'Run commands',
          'Edits files, runs commands, uses apps and searches the web.',
        ],
      }[profileName];
      const accessGroup = screen.getByRole('group', { name: 'Access profile' });
      expect(
        within(accessGroup).getByRole('radio', {
          name: new RegExp(`^${copy[0]}`),
        }),
      ).toBeChecked();
      expect(within(accessGroup).getByText(copy[1])).toBeVisible();
    },
  );

  it('keeps an unmatched, duplicate, legacy tool set as explicit Custom access when saving other fields', async () => {
    const user = userEvent.setup();
    const customTools = [
      'read_file',
      'legacy_workspace_tool',
      'read_file',
      'memory_remove',
      'memory_consolidate',
    ];
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: customTools }), { saveSettings });

    expect(screen.getByText('Custom access')).toBeVisible();
    expect(
      screen.getByText(
        'This agent has a custom tool set. Choose a standard profile to replace it when you save.',
      ),
    ).toBeVisible();
    expect(screen.getByText('5 tools configured')).toBeVisible();
    expect(screen.getByText(customTools.join(', '))).toBeVisible();
    expect(
      screen.getByRole('checkbox', { name: 'Allow removing saved facts' }),
    ).toBeChecked();
    expect(
      screen.getByRole('checkbox', {
        name: 'Allow consolidating saved facts',
      }),
    ).toBeChecked();
    for (const radio of screen.getAllByRole('radio')) {
      expect(radio).not.toBeChecked();
    }

    const name = screen.getByDisplayValue('Nova');
    await user.clear(name);
    await user.type(name, 'Nova Prime');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({ name: 'Nova Prime' });
    expect(saveSettings.mock.calls[0]?.[0]).not.toHaveProperty('tools');
  });

  it('sends an exact ordered profile tool list only after deliberate selection', async () => {
    const user = userEvent.setup();
    const saveSettings = vi.fn(async () => true);
    renderPanel(agent({ toolNames: ['read_file', 'legacy_workspace_tool'] }), {
      saveSettings,
    });

    await user.click(screen.getByRole('radio', { name: /^Run commands/ }));
    const name = screen.getByDisplayValue('Nova');
    await user.clear(name);
    await user.type(name, 'Nova Prime');
    const provider = screen.getByRole('combobox', { name: 'Provider' });
    const model = screen.getByRole('combobox', { name: 'Model' });
    await user.selectOptions(provider, 'anthropic');
    await user.selectOptions(model, '__custom__');
    await user.type(
      screen.getByPlaceholderText('model id, e.g. llama3.1'),
      'anthropic/custom-model',
    );
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      name: 'Nova Prime',
      provider: 'anthropic',
      model: 'anthropic/custom-model',
      tools: withoutRecall(toolNamesForProfile('operate')),
    });
  });

  it('saves only the tools when Operate is chosen: Autonomy decides when it asks', async () => {
    const user = userEvent.setup();
    const saveSettings = vi.fn(async () => true);
    const network = vi.spyOn(globalThis, 'fetch');
    renderPanel(agent(), { saveSettings, sections: ['access', 'builtins'] });
    await user.click(screen.getByRole('radio', { name: /^Run commands/ }));
    expect(
      screen.getByText(
        'Helpers get the same access as Nova. Commands run in a Docker sandbox.',
      ),
    ).toBeVisible();
    expect(
      screen.getByRole('link', { name: 'Change in Runtime' }),
    ).toHaveAttribute('href', '#/runtime');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({
        tools: toolNamesForProfile('operate'),
      }),
    );
    expect(network).not.toHaveBeenCalled();
  });

  it('retains the draft and focuses an announced save error', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: ['read_file', 'legacy_workspace_tool'],
    });
    const saveSettings = vi.fn(async () => false);
    const view = renderPanel(currentAgent, { saveSettings });

    const name = screen.getByDisplayValue('Nova');
    await user.clear(name);
    await user.type(name, 'Unsaved Nova');
    await user.click(screen.getByRole('radio', { name: /^Look only/ }));
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    view.rerender(<SettingsPanel {...view.props} saveError="PATCH failed" />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('PATCH failed');
    expect(alert).toHaveAttribute('aria-live', 'assertive');
    expect(alert).toHaveAttribute('id', 'settings-save-error');
    expect(alert).toHaveFocus();
    expect(
      screen.getByRole('button', { name: 'Save changes' }),
    ).toHaveAttribute('aria-describedby', 'settings-save-error');
    expect(screen.getByDisplayValue('Unsaved Nova')).toBeVisible();
    expect(screen.getByRole('radio', { name: /^Look only/ })).toBeChecked();
  });

  it('focuses and associates reset failures only with Reset', () => {
    const view = renderPanel(agent());

    view.rerender(<SettingsPanel {...view.props} resetError="DELETE failed" />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('DELETE failed');
    expect(alert).toHaveAttribute('id', 'settings-reset-error');
    expect(alert).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Reset…' })).toHaveAttribute(
      'aria-describedby',
      'settings-reset-error',
    );
    expect(
      screen.getByRole('button', { name: 'No changes' }),
    ).not.toHaveAttribute('aria-describedby');
  });

  it('does not clobber an unsaved draft when a known tool set is reordered', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: [
        ...toolNamesForProfile('collaborate'),
        'memory_remove',
        'memory_consolidate',
      ],
    });
    const view = renderPanel(currentAgent);

    const name = screen.getByDisplayValue('Nova');
    await user.clear(name);
    await user.type(name, 'Unsaved Nova');
    view.rerender(
      <SettingsPanel
        {...view.props}
        agent={{
          ...currentAgent,
          toolNames: currentAgent.toolNames.toReversed(),
        }}
      />,
    );

    expect(screen.getByDisplayValue('Unsaved Nova')).toBeVisible();
    expect(screen.getByRole('radio', { name: /^Edit files/ })).toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow removing saved facts' }),
    ).toBeChecked();
    expect(
      screen.getByRole('checkbox', {
        name: 'Allow consolidating saved facts',
      }),
    ).toBeChecked();
  });

  it('locks every editable control and reset while saving or resetting', () => {
    const currentAgent = agent({ name: 'Nova draft' });
    const view = renderPanel(currentAgent, { saving: true });

    expect(screen.getByRole('button', { name: 'Saving…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Reset…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Change' })).toBeDisabled();
    for (const control of [
      ...screen.queryAllByRole('textbox'),
      ...screen.getAllByRole('combobox'),
      ...screen.getAllByRole('radio'),
      ...screen.getAllByRole('checkbox'),
    ]) {
      expect(control).toBeDisabled();
    }

    view.rerender(<SettingsPanel {...view.props} saving={false} resetting />);

    expect(screen.getByRole('button', { name: 'No changes' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Working…' })).toBeDisabled();
    for (const control of [
      ...screen.queryAllByRole('textbox'),
      ...screen.getAllByRole('combobox'),
      ...screen.getAllByRole('radio'),
      ...screen.getAllByRole('checkbox'),
    ]) {
      expect(control).toBeDisabled();
    }
    expect(screen.getByText('Resetting agent…', { exact: true })).toBeVisible();
  });
});

it('M43 keeps native, dormant external and custom permissions during unrelated saves and optional toggles', async () => {
  const external = `mcp_${'1'.repeat(32)}_${'a'.repeat(16)}`;
  const original = ['read_file', external, 'unknown_custom'];
  const saveSettings = vi.fn(async () => true);
  renderPanel(agent({ toolNames: original }), { saveSettings });
  expect(
    screen.getByText(
      /Choosing a standard access profile replaces these permissions/,
    ),
  ).toBeVisible();
  const name = screen.getByDisplayValue('Nova');
  await userEvent.clear(name);
  await userEvent.type(name, 'Nova Two');
  await userEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  expect(saveSettings).toHaveBeenLastCalledWith({ name: 'Nova Two' });
  await userEvent.click(
    screen.getByRole('checkbox', { name: 'Allow removing saved facts' }),
  );
  await userEvent.click(
    screen.getByRole('button', { name: /^(Save changes|Saved ✓)$/ }),
  );
  expect(saveSettings).toHaveBeenLastCalledWith({
    name: 'Nova Two',
    tools: [...original, 'memory_remove'],
  });
});

it('M43 deliberate standard access selection replaces external grants without reinstalling them', async () => {
  const external = `mcp_${'1'.repeat(32)}_${'a'.repeat(16)}`;
  const saveSettings = vi.fn(async () => true);
  renderPanel(agent({ toolNames: ['read_file', external, 'unknown_custom'] }), {
    saveSettings,
  });
  await userEvent.click(screen.getByRole('radio', { name: /Edit files/ }));
  await userEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  expect(saveSettings).toHaveBeenCalledWith({
    tools: withoutRecall(toolNamesForProfile('collaborate')),
  });
});

it('keys the active personality editor to the selected agent and disables it during reset', () => {
  const view = renderPanel(agent(), { sections: ['profile'] });
  view.rerender(
    <SettingsPanel
      {...view.props}
      agent={agent({ id: 'agent-next' })}
      resetting
    />,
  );
  const editor = screen.getByRole('region', {
    name: 'Active personality editor',
  });
  expect(editor).toHaveAttribute('data-agent-id', 'agent-next');
  expect(editor).toHaveAttribute('data-disabled', 'true');
});

it('m67_read_document_is_off_when_absent_and_unrelated_saves_do_not_grant_it', async () => {
  const saveSettings = vi.fn(async () => true);
  const currentTools = ['read_file', 'memory_remove', 'legacy_workspace_tool'];
  renderPanel(agent({ toolNames: currentTools }), { saveSettings });

  expect(
    screen.getByRole('checkbox', {
      name: 'Allow reading workspace documents',
    }),
  ).not.toBeChecked();
  const name = screen.getByDisplayValue('Nova');
  await userEvent.clear(name);
  await userEvent.type(name, 'Nova Two');
  await userEvent.click(screen.getByRole('button', { name: 'Save changes' }));

  expect(saveSettings).toHaveBeenCalledWith({ name: 'Nova Two' });
  expect(saveSettings.mock.calls[0]?.[0]).not.toHaveProperty('tools');
});

it('m67_read_document_adds_only_its_opt_in_to_a_custom_tool_set', async () => {
  const user = userEvent.setup();
  const currentTools = ['read_file', 'memory_remove', 'legacy_workspace_tool'];
  const saveSettings = vi.fn(async () => true);
  renderPanel(agent({ toolNames: currentTools }), { saveSettings });

  const allowRead = screen.getByRole('checkbox', {
    name: 'Allow reading workspace documents',
  });
  expect(allowRead).not.toBeChecked();
  await user.click(allowRead);
  await user.click(screen.getByRole('button', { name: 'Save changes' }));

  expect(saveSettings).toHaveBeenCalledWith({
    tools: [...currentTools, 'read_document'],
  });
});

it('m67_read_document_reseeds_from_saved_opt_in_changes', async () => {
  const currentAgent = agent({
    toolNames: ['read_file', 'read_document', 'legacy_workspace_tool'],
  });
  const view = renderPanel(currentAgent);
  const allowRead = screen.getByRole('checkbox', {
    name: 'Allow reading workspace documents',
  });
  expect(allowRead).toBeChecked();
  await userEvent.click(allowRead);

  view.rerender(
    <SettingsPanel
      {...view.props}
      agent={{ ...currentAgent, toolNames: ['read_file'] }}
    />,
  );
  expect(allowRead).not.toBeChecked();

  view.rerender(
    <SettingsPanel
      {...view.props}
      agent={{
        ...currentAgent,
        id: 'agent-next',
        toolNames: ['read_file', 'read_document'],
      }}
    />,
  );
  expect(allowRead).toBeChecked();
});

it('m67_read_document_profile_replacement_clears_the_opt_in_until_reselected', async () => {
  const user = userEvent.setup();
  const saveSettings = vi.fn(async () => true);
  const currentTools = ['read_file', 'read_document', 'legacy_workspace_tool'];
  renderPanel(agent({ toolNames: currentTools }), { saveSettings });

  const allowRead = screen.getByRole('checkbox', {
    name: 'Allow reading workspace documents',
  });
  expect(allowRead).toBeChecked();
  await user.click(screen.getByRole('radio', { name: /^Edit files/ }));
  expect(allowRead).not.toBeChecked();
  await user.click(allowRead);
  await user.click(screen.getByRole('button', { name: 'Save changes' }));

  expect(saveSettings).toHaveBeenCalledWith({
    tools: [...withoutRecall(toolNamesForProfile('collaborate')), 'read_document'],
  });
  expect(toolNamesForProfile('collaborate')).not.toContain('read_document');
});

it('keeps avatar and name controls beside the active personality editor', async () => {
  const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
  renderPanel(agent({ system: 'Keep my project instructions.' }), {
    sections: ['profile'],
    saveSettings,
  });
  expect(
    screen.getByRole('button', { name: 'Change agent avatar' }),
  ).toBeVisible();
  expect(
    screen.getByRole('region', { name: 'Active personality editor' }),
  ).toBeVisible();
  fireEvent.change(screen.getByLabelText('Name'), {
    target: { value: 'Alice' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() =>
    expect(saveSettings).toHaveBeenCalledWith({ name: 'Alice' }),
  );
});

it('keeps profile drafts when switching models is unavailable during unsaved changes', () => {
  renderPanel(agent(), { sections: ['profile'] });
  fireEvent.change(screen.getByLabelText('Name'), {
    target: { value: 'Unsaved Nova' },
  });
  const link = screen.getByRole('link', { name: 'Change' });
  expect(link).toHaveAttribute('aria-disabled', 'true');
  expect(fireEvent.click(link)).toBe(false);
  expect(screen.getByLabelText('Name')).toHaveValue('Unsaved Nova');
});

describe('reusable work permissions', () => {
  const templates = [
    'save_workflow_template',
    'list_workflow_templates',
    'run_workflow_template',
  ];
  it.each([
    ['Allow workflow templates in chat', templates],
    ['Allow saved tool result reads', ['read_tool_result']],
  ] as const)(
    'grants and revokes %s without widening existing workflows',
    async (label, tools) => {
      const original = [
        'read_file',
        'create_workflow',
        'list_workflows',
        'legacy_custom',
      ];
      const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
      const view = renderPanel(agent({ toolNames: original }), {
        sections: ['access', 'builtins'],
        saveSettings,
      });
      const toggle = screen.getByRole('checkbox', { name: label });
      expect(toggle).not.toBeChecked();
      fireEvent.click(toggle);
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenLastCalledWith({
          tools: [...original, ...tools],
        }),
      );
      view.unmount();
      renderPanel(agent({ toolNames: [...original, ...tools] }), {
        sections: ['access', 'builtins'],
        saveSettings,
      });
      fireEvent.click(screen.getByRole('checkbox', { name: label }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(saveSettings).toHaveBeenLastCalledWith({ tools: original }),
      );
      for (const profile of ['observe', 'collaborate', 'operate'] as const)
        for (const tool of tools)
          expect(toolNamesForProfile(profile)).not.toContain(tool);
    },
  );
  it('preserves partial template grants and saved-result access through a profile change', async () => {
    const saveSettings = vi.fn(async (_patch: AgentUpdateInput) => true);
    renderPanel(
      agent({
        toolNames: [
          ...toolNamesForProfile('collaborate'),
          'list_workflow_templates',
          'read_tool_result',
        ],
      }),
      { sections: ['access', 'builtins'], saveSettings },
    );
    expect(
      screen.getByRole('checkbox', {
        name: 'Allow workflow templates in chat',
      }),
    ).not.toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Allow saved tool result reads' }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole('radio', { name: /^Look only/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(saveSettings).toHaveBeenCalledWith({
        tools: [
          ...toolNamesForProfile('observe'),
          'list_workflow_templates',
          'read_tool_result',
        ],
      }),
    );
  });
});
