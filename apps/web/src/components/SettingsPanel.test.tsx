import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import {
  ACCESS_PROFILES,
  toolNamesForProfile,
  type AccessProfile,
} from '../lib/agent-access';
import type {
  AgentUpdateInput,
  DaemonProvider,
  DaemonWorkspaceState,
} from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import type { LearningReviewApi } from './learning/LearningReviewPanel';
import { SettingsPanel } from './SettingsPanel';

// Preview transport/lifecycle are covered by AgentFilesPanel.test.tsx. Keep
// settings focus and save tests independent of an owner network request.
vi.mock('./AgentFilesPanel', () => ({
  AgentFilesPanel: ({ agentId }: { agentId: string }) => (
    <section aria-label="Agent files" data-agent-id={agentId} />
  ),
}));

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
    workspace: DaemonWorkspaceState | null;
    learningApi: LearningReviewApi;
    saveSettings: (patch: AgentUpdateInput) => Promise<boolean>;
    resetAgent: () => void;
    close: () => void;
  }> = {},
) {
  const props = {
    agent: currentAgent,
    providers,
    saving: false,
    resetting: false,
    saveError: null,
    resetError: null,
    workspace: null,
    saveSettings: vi.fn(async () => true),
    resetAgent: vi.fn(),
    close: vi.fn(),
    learningApi: learningApiMock(),
    ...overrides,
  };

  return { ...render(<SettingsPanel {...props} />), props };
}

function learningApiMock(): LearningReviewApi {
  return {
    policy: vi.fn(async () => ({
      autoEnabled: false,
      dailyLimit: 2,
      pendingLimit: 10,
      timeoutMs: 20_000,
    })),
    setPolicy: vi.fn(async (_agentId, input) => ({
      ...{
        autoEnabled: false,
        dailyLimit: 2,
        pendingLimit: 10,
        timeoutMs: 20_000,
      },
      ...input,
    })),
    listReviews: vi.fn(async () => []),
    getReview: vi.fn(async () => {
      throw new Error('No review fixture');
    }),
    cancelReview: vi.fn(async () => {
      throw new Error('No review fixture');
    }),
    decideProposal: vi.fn(async () => {
      throw new Error('No review fixture');
    }),
    listSkills: vi.fn(async () => []),
    skill: vi.fn(async () => ({ skill: null, file: null })),
    createSkillDraft: vi.fn(async () => {
      throw new Error('No skill draft fixture');
    }),
  };
}

const configuredWorkspace: DaemonWorkspaceState = {
  configured: true,
  workspace: {
    rootPath: '/workspaces/northwind',
    companyName: 'Northwind Research',
    mission: 'Map supply chains',
    values: ['rigor'],
  },
  defaultRoot: '/workspaces',
};

describe('SettingsPanel workspace section', () => {
  it('shows the configured workspace as read-only company, mission, and folder rows', () => {
    renderPanel(agent(), { workspace: configuredWorkspace });

    const heading = screen.getByRole('heading', {
      level: 4,
      name: 'Workspace',
    });
    expect(heading).toBeVisible();
    expect(screen.getByText('Company')).toBeVisible();
    expect(screen.getByText('Northwind Research')).toBeVisible();
    expect(screen.getByText('Mission')).toBeVisible();
    expect(screen.getByText('Map supply chains')).toBeVisible();
    expect(screen.getByText('Folder')).toBeVisible();
    expect(screen.getByText('/workspaces/northwind')).toBeVisible();
    // Read-only: the workspace values are not editable controls.
    expect(screen.queryByDisplayValue('Northwind Research')).toBeNull();
    expect(screen.queryByDisplayValue('Map supply chains')).toBeNull();
  });

  it('places the workspace section above the identity settings', () => {
    renderPanel(agent(), { workspace: configuredWorkspace });

    const workspaceHeading = screen.getByRole('heading', {
      level: 4,
      name: 'Workspace',
    });
    const identityHeading = screen.getByRole('heading', {
      level: 4,
      name: 'Identity',
    });
    expect(
      workspaceHeading.compareDocumentPosition(identityHeading) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
  });

  it.each([null, { configured: false, workspace: null, defaultRoot: '/w' }])(
    'hides the workspace section when workspace state is %o',
    (workspace) => {
      renderPanel(agent(), { workspace });

      expect(
        screen.queryByRole('heading', { level: 4, name: 'Workspace' }),
      ).not.toBeInTheDocument();
      expect(screen.queryByText('Company')).not.toBeInTheDocument();
      expect(
        screen.getByRole('heading', { level: 4, name: 'Identity' }),
      ).toBeVisible();
    },
  );
});

describe('SettingsPanel access', () => {
  it('M38_learning_policy_checkbox_is_visible_and_defaults_off', async () => {
    renderPanel(agent());

    expect(
      await screen.findByRole('checkbox', {
        name: 'Automatically review completed tasks',
      }),
    ).not.toBeChecked();
  });

  it('M38_saves_learning_opt_in_through_its_own_policy_route', async () => {
    const user = userEvent.setup();
    const view = renderPanel(agent());
    const checkbox = await screen.findByRole('checkbox', {
      name: 'Automatically review completed tasks',
    });
    await waitFor(() => expect(checkbox).toBeEnabled());

    await user.click(checkbox);

    await waitFor(() =>
      expect(view.props.learningApi.setPolicy).toHaveBeenCalledWith(
        'agent-main',
        { autoEnabled: true },
      ),
    );
    expect(view.props.saveSettings).not.toHaveBeenCalled();
    expect(checkbox).toBeChecked();
  });

  it('M38_retains_default_off_policy_when_opt_in_save_fails', async () => {
    const user = userEvent.setup();
    const api = learningApiMock();
    api.setPolicy = vi.fn(async () => {
      throw new Error('Policy save failed');
    });
    renderPanel(agent(), { learningApi: api });
    const checkbox = await screen.findByRole('checkbox', {
      name: 'Automatically review completed tasks',
    });
    await waitFor(() => expect(checkbox).toBeEnabled());

    await user.click(checkbox);

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Policy save failed',
    );
    expect(checkbox).not.toBeChecked();
  });

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
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });

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
    renderPanel(agent({ toolNames: currentTools }), { saveSettings });

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

  it('M41_standard_profile_clears_conversation_read_but_allows_deliberate_reenable', async () => {
    const user = userEvent.setup();
    const saveSettings = vi.fn(async () => true);
    renderPanel(
      agent({
        toolNames: [
          ...toolNamesForProfile('observe'),
          'read_conversation_window',
        ],
      }),
      { saveSettings },
    );

    const allowHistoryRead = screen.getByRole('checkbox', {
      name: 'Allow reading conversation context',
    });
    await user.click(screen.getByRole('radio', { name: /^Collaborate/ }));
    expect(allowHistoryRead).not.toBeChecked();
    expect(toolNamesForProfile('collaborate')).not.toContain(
      'read_conversation_window',
    );

    await user.click(allowHistoryRead);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(saveSettings).toHaveBeenCalledWith({
      tools: [
        ...toolNamesForProfile('collaborate'),
        'read_conversation_window',
      ],
    });
  });

  it('M41_failed_save_keeps_conversation_read_draft_without_changing_the_agent', async () => {
    const user = userEvent.setup();
    const currentAgent = agent({
      toolNames: ['read_file', 'legacy_workspace_tool'],
    });
    const saveSettings = vi.fn(async () => false);
    const view = renderPanel(currentAgent, { saveSettings });
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
    );

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
    await user.click(screen.getByRole('radio', { name: /^Collaborate/ }));
    expect(removeFacts).not.toBeChecked();
    expect(consolidateFacts).not.toBeChecked();
    expect(toolNamesForProfile('collaborate')).not.toContain('memory_remove');
    expect(toolNamesForProfile('collaborate')).not.toContain(
      'memory_consolidate',
    );

    await user.click(consolidateFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...toolNamesForProfile('collaborate'), 'memory_consolidate'],
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
    await user.click(screen.getByRole('radio', { name: /^Collaborate/ }));
    expect(removeFacts).not.toBeChecked();
    expect(
      screen.getByText(/permanent removal of one active Fact only/i),
    ).toBeVisible();
    await user.click(removeFacts);
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      tools: [...toolNamesForProfile('collaborate'), 'memory_remove'],
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

  it('opens as a labelled modal and keeps keyboard focus inside it', async () => {
    const user = userEvent.setup();
    renderPanel(agent());

    const dialog = screen.getByRole('dialog', { name: 'Agent settings' });
    const close = screen.getByRole('button', { name: 'Close settings' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAttribute('aria-labelledby', 'settings-heading');
    expect(close).toHaveFocus();

    const focusable = Array.from(
      dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    );
    const last = focusable.at(-1);
    expect(last).toBeDefined();

    await user.tab({ shift: true });
    expect(last).toHaveFocus();
    await user.tab();
    expect(close).toHaveFocus();
  });

  it.each<AccessProfile>(['observe', 'collaborate', 'operate'])(
    'derives and displays the %s profile regardless of tool order',
    (profileName) => {
      renderPanel(
        agent({ toolNames: toolNamesForProfile(profileName).toReversed() }),
      );

      const accessGroup = screen.getByRole('group', { name: 'Access profile' });
      expect(
        within(accessGroup).getByRole('radio', {
          name: new RegExp(`^${ACCESS_PROFILES[profileName].label}`),
        }),
      ).toBeChecked();
      expect(
        within(accessGroup).getByText(ACCESS_PROFILES[profileName].summary),
      ).toBeVisible();
      expect(
        within(accessGroup).getByText(ACCESS_PROFILES[profileName].risk),
      ).toBeVisible();
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

    await user.click(screen.getByRole('radio', { name: /^Operate/ }));
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
    const system = screen.getByPlaceholderText(
      'Leave empty for the daemon default.',
    );
    await user.clear(system);
    await user.type(system, 'Be concise');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(saveSettings).toHaveBeenCalledWith({
      name: 'Nova Prime',
      provider: 'anthropic',
      model: 'anthropic/custom-model',
      system: 'Be concise',
      tools: toolNamesForProfile('operate'),
    });
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
    await user.click(screen.getByRole('radio', { name: /^Observe/ }));
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
    expect(screen.getByRole('radio', { name: /^Observe/ })).toBeChecked();
  });

  it('focuses and associates reset failures only with Reset', () => {
    const view = renderPanel(agent());

    view.rerender(<SettingsPanel {...view.props} resetError="DELETE failed" />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('DELETE failed');
    expect(alert).toHaveAttribute('id', 'settings-reset-error');
    expect(alert).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Reset' })).toHaveAttribute(
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
    expect(screen.getByRole('radio', { name: /^Collaborate/ })).toBeChecked();
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
    expect(screen.getByRole('button', { name: 'Reset' })).toBeDisabled();
    for (const control of [
      ...screen.getAllByRole('textbox'),
      ...screen.getAllByRole('combobox'),
      ...screen.getAllByRole('radio'),
      ...screen.getAllByRole('checkbox'),
    ]) {
      expect(control).toBeDisabled();
    }

    view.rerender(<SettingsPanel {...view.props} saving={false} resetting />);

    expect(screen.getByRole('button', { name: 'No changes' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Resetting…' })).toBeDisabled();
    for (const control of [
      ...screen.getAllByRole('textbox'),
      ...screen.getAllByRole('combobox'),
      ...screen.getAllByRole('radio'),
      ...screen.getAllByRole('checkbox'),
    ]) {
      expect(control).toBeDisabled();
    }
    expect(screen.getByText('Resetting agent…', { exact: true })).toBeVisible();
  });

  it.each([
    { busyProp: 'saving', buttonName: 'Saving…', description: /saving/i },
    {
      busyProp: 'resetting',
      buttonName: 'Resetting…',
      description: /resetting/i,
    },
  ] as const)(
    'blocks every close trigger while $busyProp and restores them after settlement',
    async ({ busyProp, buttonName, description }) => {
      const user = userEvent.setup();
      const close = vi.fn();
      const view = renderPanel(agent(), { [busyProp]: true, close });

      expect(screen.getByRole('button', { name: buttonName })).toBeDisabled();
      const closeButton = screen.getByRole('button', {
        name: 'Close settings',
      });
      expect(closeButton).toBeDisabled();
      expect(closeButton).toHaveAccessibleDescription(description);

      await user.click(closeButton);
      fireEvent.click(screen.getByTestId('settings-backdrop'));
      await user.keyboard('{Escape}');
      expect(close).not.toHaveBeenCalled();

      view.rerender(
        <SettingsPanel {...view.props} saving={false} resetting={false} />,
      );

      expect(closeButton).toBeEnabled();
      await user.keyboard('{Escape}');
      fireEvent.click(screen.getByTestId('settings-backdrop'));
      await user.click(closeButton);
      expect(close).toHaveBeenCalledTimes(3);
    },
  );
});

it('replaces a personality style saved at the start of an otherwise empty prompt', async () => {
  renderPanel(
    agent({ system: '[Personality style]\nBe calm.\n[/Personality style]' }),
  );
  fireEvent.change(screen.getByLabelText('Add a personality style'), {
    target: { value: 'direct' },
  });
  const prompt = screen.getByRole('textbox', {
    name: 'System prompt',
  }) as HTMLTextAreaElement;
  expect(prompt.value).toContain('Be direct, practical, and concise.');
  expect(prompt.value).not.toContain('Be calm.');
  expect(prompt.value.match(/\[Personality style\]/g)).toHaveLength(1);
});
