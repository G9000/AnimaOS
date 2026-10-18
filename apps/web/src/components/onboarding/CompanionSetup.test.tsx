import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent, { type UserEvent } from '@testing-library/user-event';
import { ChatGptClient, type ChatGptModelsSnapshot } from '@animaOS-SWARM/sdk';
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from 'vitest';
import { DEFAULT_APPROVAL_POLICY } from '@animaOS-SWARM/sdk';
import {
  communityBundlesOwnerApi,
  daemon,
  type DaemonProvider,
} from '../../lib/daemon-api';
import { CompanionSetup } from './CompanionSetup';

const providers: DaemonProvider[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    configured: true,
    requiresKey: true,
    apiKeyEnvs: ['OPENAI_API_KEY'],
  },
];
const freshWorkspace = {
  configured: false,
  workspace: null,
  defaultRoot: '/data/workspace',
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
});
let setAutonomy: MockInstance<typeof daemon.setAutonomy>;
beforeEach(() => {
  vi.spyOn(daemon, 'inspectWorkspace').mockResolvedValue({ found: false });
  setAutonomy = vi.spyOn(daemon, 'setAutonomy').mockImplementation(
    async (_id, patch) => ({
      mode: patch.mode ?? 'free',
      smartCheck: false,
      denyPatterns: [],
      policy: DEFAULT_APPROVAL_POLICY,
    }),
  );
});

function renderSetup(
  overrides: Partial<Parameters<typeof CompanionSetup>[0]> = {},
) {
  return render(
    <CompanionSetup
      providers={providers}
      providersError={null}
      retryProviders={vi.fn()}
      onCreated={vi.fn()}
      {...overrides}
    />,
  );
}

const button = (name: string) => screen.getByRole('button', { name });

async function start(user: UserEvent) {
  await waitFor(() => expect(button('Get started')).toBeEnabled());
  await user.click(button('Get started'));
}

async function toAccess(user: UserEvent) {
  await start(user);
  await user.click(button('Continue'));
  await user.click(button('Continue'));
  expect(
    screen.getByRole('heading', { name: 'Where anima works' }),
  ).toBeVisible();
}

async function toVoice(user: UserEvent) {
  await toAccess(user);
  await user.click(button('Continue'));
  expect(
    await screen.findByRole('heading', { name: 'Talk out loud?' }),
  ).toBeVisible();
}

async function openExisting(user: UserEvent) {
  await waitFor(() =>
    expect(button('Open an existing workspace')).toBeEnabled(),
  );
  await user.click(button('Open an existing workspace'));
}

describe('CompanionSetup', () => {
  it('uses a chosen ordinary folder for a new companion', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.spyOn(daemon, 'pickWorkspaceFolder').mockResolvedValue({
      rootPath: '/my/existing/files',
    });
    const create = vi.spyOn(daemon, 'bootstrapWorkspace').mockResolvedValue({
      agent: { state: { id: 'main' } },
    } as never);
    renderSetup();
    const user = userEvent.setup();
    await toAccess(user);
    await user.click(button('Browse…'));
    expect(screen.getByRole('textbox', { name: 'Home folder' })).toHaveValue(
      '/my/existing/files',
    );
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(daemon.inspectWorkspace).toHaveBeenCalledWith('/my/existing/files');
    expect(create.mock.calls[0][0].workspace.rootPath).toBe(
      '/my/existing/files',
    );
  });

  it('previews a saved workspace and resumes its existing agent', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.mocked(daemon.inspectWorkspace).mockResolvedValue({
      found: true,
      companyName: 'My workspace',
      mission: 'Continue my work',
      orchestrator: { name: 'Ari', provider: 'openai', model: 'gpt-test' },
      workers: [],
      providerAvailable: true,
    });
    const resume = vi.spyOn(daemon, 'resumeWorkspace').mockResolvedValue({
      orchestrator: { state: { id: 'ari' } },
    } as never);
    const bootstrap = vi.spyOn(daemon, 'bootstrapWorkspace');
    const created = vi.fn();
    renderSetup({ onCreated: created });
    const user = userEvent.setup();
    await toAccess(user);
    await user.type(
      screen.getByRole('textbox', { name: 'Home folder' }),
      '{Control>}a{/Control}/saved/workspace',
    );
    await user.click(button('Continue'));
    expect(await screen.findByText('Continue my work')).toBeVisible();
    expect(screen.getByText('Ari')).toBeVisible();
    expect(bootstrap).not.toHaveBeenCalled();
    await user.click(button('Resume workspace'));
    await waitFor(() =>
      expect(resume).toHaveBeenCalledWith('/saved/workspace'),
    );
    expect(created).toHaveBeenCalledWith(
      expect.objectContaining({ state: { id: 'ari' } }),
    );
  });

  it('can inspect and resume a saved workspace before a provider is configured', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.mocked(daemon.inspectWorkspace).mockResolvedValue({
      found: true,
      companyName: 'Saved',
      orchestrator: { name: 'Ari', provider: 'openai', model: 'gpt-test' },
      workers: [],
      providerAvailable: false,
    });
    const resume = vi.spyOn(daemon, 'resumeWorkspace').mockResolvedValue({
      orchestrator: { state: { id: 'ari' } },
    } as never);
    renderSetup({ providers: [] });
    const user = userEvent.setup();
    await openExisting(user);
    await user.click(button('Check folder'));
    expect(await screen.findByText('Saved')).toBeVisible();
    expect(
      screen.getByText(/resume offline until you add the key/),
    ).toBeVisible();
    await user.click(button('Resume workspace'));
    await waitFor(() => expect(resume).toHaveBeenCalledWith('/data/workspace'));
  });

  it('does not create a new companion from Enter while a saved workspace is previewed', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.mocked(daemon.inspectWorkspace).mockResolvedValue({
      found: true,
      companyName: 'Saved',
      orchestrator: { name: 'Ari', provider: 'openai', model: 'gpt-test' },
      workers: [],
      providerAvailable: true,
    });
    const bootstrap = vi.spyOn(daemon, 'bootstrapWorkspace');
    renderSetup();
    const user = userEvent.setup();
    await toAccess(user);
    await user.click(button('Continue'));
    expect(await screen.findByText('Saved')).toBeVisible();
    await user.click(screen.getByRole('textbox', { name: 'Home folder' }));
    await user.keyboard('{Enter}');
    expect(daemon.inspectWorkspace).toHaveBeenCalledOnce();
    expect(bootstrap).not.toHaveBeenCalled();
  });

  it('does not bootstrap when inspection fails', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.mocked(daemon.inspectWorkspace).mockRejectedValue(
      new Error('Invalid anima.yaml'),
    );
    const bootstrap = vi.spyOn(daemon, 'bootstrapWorkspace');
    renderSetup();
    const user = userEvent.setup();
    await toAccess(user);
    await user.click(button('Continue'));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Invalid anima.yaml',
    );
    expect(
      screen.getByRole('heading', { name: 'Where anima works' }),
    ).toBeVisible();
    expect(bootstrap).not.toHaveBeenCalled();
  });

  it('ignores a stale inspection after the folder changes', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    let resolveInspect!: (value: { found: false }) => void;
    vi.mocked(daemon.inspectWorkspace).mockReturnValue(
      new Promise((resolve) => {
        resolveInspect = resolve;
      }),
    );
    renderSetup();
    const user = userEvent.setup();
    await openExisting(user);
    const folder = screen.getByRole('textbox', { name: 'Home folder' });
    await user.click(button('Check folder'));
    await user.clear(folder);
    await user.type(folder, '/another/folder');
    resolveInspect({ found: false });
    await waitFor(() => expect(button('Check folder')).toBeEnabled());
    expect(
      screen.queryByText(/No saved Anima workspace found here/),
    ).not.toBeInTheDocument();
  });

  it('keeps a typed folder when the picker returns an older selection', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    let resolvePicker!: (value: { rootPath: string }) => void;
    vi.spyOn(daemon, 'pickWorkspaceFolder').mockReturnValue(
      new Promise((resolve) => {
        resolvePicker = resolve;
      }),
    );
    renderSetup();
    const user = userEvent.setup();
    await openExisting(user);
    const folder = screen.getByRole('textbox', { name: 'Home folder' });
    await user.click(button('Browse…'));
    await user.clear(folder);
    await user.type(folder, '/typed/folder');
    resolvePicker({ rootPath: '/old/picker/folder' });
    await waitFor(() => expect(button('Browse…')).toBeEnabled());
    expect(folder).toHaveValue('/typed/folder');
  });

  it('m49_chatgpt_models_connected_subscription_requires_owner_model_choice', async () => {
    vi.spyOn(ChatGptClient.prototype, 'models').mockResolvedValue({
      state: 'unrequested',
      connectionRevision: 'connection-a',
      catalogRevision: null,
      fetchedAtMs: null,
      expiresAtMs: null,
      models: [],
      error: null,
    });
    vi.spyOn(ChatGptClient.prototype, 'status').mockResolvedValue({
      connected: true,
      accountId: 'synthetic-account',
      planType: 'synthetic-plan',
      login: null,
      error: null,
    });
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const create = vi.spyOn(daemon, 'bootstrapWorkspace');
    renderSetup({
      providers: [
        ...providers,
        {
          id: 'chatgpt',
          label: 'ChatGPT subscription',
          configured: true,
          requiresKey: false,
          apiKeyEnvs: [],
        },
      ],
    });
    const user = userEvent.setup();
    await start(user);
    expect(await screen.findByText('Connected · synthetic-plan')).toBeVisible();
    expect(
      screen.getByRole('radio', { name: /Use my ChatGPT plan/ }),
    ).toBeChecked();
    expect(
      screen.getByRole('combobox', { name: 'Model', exact: true }),
    ).toHaveValue('');
    expect(button('Continue')).toBeDisabled();
    expect(create).not.toHaveBeenCalled();
  });

  it('refreshes providers once when an already-connected ChatGPT account loads', async () => {
    vi.spyOn(ChatGptClient.prototype, 'status').mockResolvedValue({
      connected: true,
      accountId: 'synthetic-account',
      planType: 'synthetic-plan',
      login: null,
      error: null,
    });
    vi.spyOn(ChatGptClient.prototype, 'models').mockResolvedValue({
      state: 'unrequested',
      connectionRevision: 'connection-a',
      catalogRevision: null,
      fetchedAtMs: null,
      expiresAtMs: null,
      models: [],
      error: null,
    });
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const retryProviders = vi.fn(async () => undefined);
    renderSetup({
      retryProviders,
      providers: [
        {
          id: 'chatgpt',
          label: 'ChatGPT subscription',
          configured: true,
          requiresKey: false,
          apiKeyEnvs: [],
        },
      ],
    });
    const user = userEvent.setup();
    await start(user);
    expect(await screen.findByText('Connected · synthetic-plan')).toBeVisible();
    await waitFor(() => expect(retryProviders).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(retryProviders).toHaveBeenCalledOnce();
    expect(screen.getByText('Connected · synthetic-plan')).toBeVisible();
  });

  it('m49_chatgpt_models_companion_preserves_explicit_custom_choice_when_metadata_arrives', async () => {
    vi.spyOn(ChatGptClient.prototype, 'status').mockResolvedValue({
      connected: true,
      accountId: 'synthetic-account',
      planType: null,
      login: null,
      error: null,
    });
    vi.spyOn(ChatGptClient.prototype, 'models').mockResolvedValue({
      state: 'unrequested',
      connectionRevision: 'connection-a',
      catalogRevision: null,
      fetchedAtMs: null,
      expiresAtMs: null,
      models: [],
      error: null,
    });
    let complete!: (snapshot: ChatGptModelsSnapshot) => void;
    const refresh = vi
      .spyOn(ChatGptClient.prototype, 'refreshModels')
      .mockReturnValue(
        new Promise((resolve) => {
          complete = resolve;
        }),
      );
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const create = vi
      .spyOn(daemon, 'bootstrapWorkspace')
      .mockResolvedValue({ agent: { state: { id: 'main' } } } as never);
    const user = userEvent.setup();
    renderSetup({
      providers: [
        {
          id: 'chatgpt',
          label: 'ChatGPT subscription',
          configured: true,
          requiresKey: false,
          apiKeyEnvs: [],
        },
      ],
    });
    await start(user);
    await screen.findByText(/Model metadata has not been refreshed/);
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Model' }),
      '__custom__',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Custom model' }),
      'Owner.Exact/Model',
    );
    await user.click(button('Refresh models'));
    await act(async () =>
      complete({
        state: 'ready',
        connectionRevision: 'connection-a',
        catalogRevision: 'catalog-a',
        fetchedAtMs: Date.now(),
        expiresAtMs: Date.now() + 300_000,
        models: [{ id: 'synthetic-model-a' }],
        error: null,
      }),
    );
    await screen.findByText(/Received model metadata/);
    await user.click(screen.getByRole('radio', { name: /Use my ChatGPT plan/ }));
    expect(screen.getByRole('textbox', { name: 'Custom model' })).toHaveValue(
      'Owner.Exact/Model',
    );
    await user.click(button('Continue'));
    const agentName = screen.getByRole('textbox', { name: 'Agent name' });
    await user.clear(agentName);
    await user.type(agentName, 'Kept name');
    await user.click(button('Continue'));
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    expect(create).toHaveBeenCalledWith(
      expect.objectContaining({
        agent: expect.objectContaining({
          name: 'Kept name',
          provider: 'chatgpt',
          model: 'Owner.Exact/Model',
        }),
      }),
    );
    expect(refresh).toHaveBeenCalledOnce();
  });

  it.each([true, false])(
    'never defaults to the mock provider (real provider configured: %s)',
    async (configured) => {
      vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
      const create = vi
        .spyOn(daemon, 'bootstrapWorkspace')
        .mockResolvedValue({ agent: { state: { id: 'main' } } } as never);
      renderSetup({
        providers: [
          {
            id: 'deterministic',
            label: 'Deterministic (mock)',
            configured: true,
            requiresKey: false,
            apiKeyEnvs: [],
          },
          {
            id: 'ollama',
            label: 'Ollama',
            configured: true,
            requiresKey: false,
            apiKeyEnvs: [],
          },
          { ...providers[0], configured },
        ],
      });
      const user = userEvent.setup();
      await start(user);
      if (configured) {
        await toVoiceFromModel(user);
        await user.click(button('Finish setup'));
        await waitFor(() => expect(create).toHaveBeenCalledOnce());
        expect(create.mock.calls[0][0].agent.provider).toBe('openai');
      } else {
        expect(button('Continue')).toBeDisabled();
        expect(create).not.toHaveBeenCalled();
      }
    },
  );

  it('creates exactly one companion using the daemon workspace directory, with no agency generation', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const create = vi
      .spyOn(daemon, 'bootstrapWorkspace')
      .mockResolvedValue({ agent: { state: { id: 'main' } } } as never);
    const generate = vi.spyOn(daemon, 'generateAgency');
    const created = vi.fn();
    renderSetup({ onCreated: created });
    const user = userEvent.setup();
    await toVoice(user);
    expect(screen.queryByText('Team')).not.toBeInTheDocument();
    await user.click(button('Finish setup'));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0]).toMatchObject({
      workspace: {
        rootPath: '/data/workspace',
        companyName: 'Personal',
        values: [],
      },
      agent: { name: 'Anima', provider: 'openai' },
    });
    expect(create.mock.calls[0][0]).not.toHaveProperty('workers');
    expect(create.mock.calls[0][0].agent.system).toContain(
      'personal companion',
    );
    expect(create.mock.calls[0][0].agent.tools).not.toContain(
      'browser_navigate',
    );
    expect(generate).not.toHaveBeenCalled();
    expect(created).toHaveBeenCalledOnce();
  });

  it('carries introductions and permissions into the new companion', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const create = vi
      .spyOn(daemon, 'bootstrapWorkspace')
      .mockResolvedValue({ agent: { state: { id: 'main' } } } as never);
    renderSetup();
    const user = userEvent.setup();
    await start(user);
    await user.click(button('Continue'));
    await user.type(screen.getByRole('textbox', { name: 'Your name' }), 'Sam');
    await user.click(screen.getByRole('radio', { name: /Driven/ }));
    expect(screen.getByText(/Good (morning|afternoon|evening), Sam\./)).toBeInTheDocument();
    await user.click(button('Continue'));
    await user.click(screen.getByRole('radio', { name: /Operate/ }));
    await user.click(screen.getByRole('checkbox', { name: /read the web/ }));
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    const agent = create.mock.calls[0][0].agent;
    expect(agent.system).toContain("The owner's name is Sam.");
    expect(agent.system).toContain('Take the lead');
    expect(agent.tools).toEqual(
      expect.arrayContaining(['bash', 'browser_navigate', 'browser_observe']),
    );
    expect(agent.tools).not.toContain('browser_act');
  });

  it('starts Free, saves it after creating, and Operate no longer changes the class policy', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.spyOn(daemon, 'bootstrapWorkspace').mockResolvedValue({
      agent: { state: { id: 'main' } },
    } as never);
    const setPolicy = vi.spyOn(daemon, 'setApprovalPolicy');
    const onCreated = vi.fn();
    renderSetup({ onCreated });
    const user = userEvent.setup();
    await toAccess(user);
    expect(screen.getByRole('radio', { name: /^Free/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /^Strict/ })).not.toBeChecked();
    await user.click(screen.getByRole('radio', { name: /Operate/ }));
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(onCreated).toHaveBeenCalledOnce());
    expect(setAutonomy).toHaveBeenCalledWith('main', { mode: 'free' });
    expect(setPolicy).not.toHaveBeenCalled();
  });

  it('saves Strict when chosen, and a failed save never blocks the new companion', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.spyOn(daemon, 'bootstrapWorkspace').mockResolvedValue({
      agent: { state: { id: 'main' } },
    } as never);
    setAutonomy.mockRejectedValue(new Error('daemon down'));
    const onCreated = vi.fn();
    renderSetup({ onCreated });
    const user = userEvent.setup();
    await toAccess(user);
    await user.click(screen.getByRole('radio', { name: /^Strict/ }));
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(onCreated).toHaveBeenCalledOnce());
    expect(setAutonomy).toHaveBeenCalledWith('main', { mode: 'strict' });
  });

  it('starts from Coder: Operate, the coding apps found, and their know-how without blocking creation', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const create = vi.spyOn(daemon, 'bootstrapWorkspace').mockResolvedValue({
      agent: { state: { id: 'coder', name: 'Coder' } },
    } as never);
    const setPolicy = vi.spyOn(daemon, 'setApprovalPolicy');
    vi.spyOn(daemon, 'codingApps').mockResolvedValue({
      apps: [
        { id: 'claude-code', name: 'Claude Code', found: true, version: '2.1.286', problem: null, bundleId: 'anima.coding.claude-code', manifestText: 'claude-manifest' },
        { id: 'codex', name: 'Codex', found: true, version: null, problem: "installed but won't start", bundleId: 'anima.coding.codex', manifestText: 'codex-manifest' },
        { id: 'opencode', name: 'OpenCode', found: false, version: null, problem: null, bundleId: 'anima.coding.opencode', manifestText: 'opencode-manifest' },
      ],
      handoff: { bundleId: 'anima.coding.delegate', manifestText: 'handoff-manifest' },
    });
    const review = vi
      .spyOn(communityBundlesOwnerApi, 'review')
      .mockImplementation(async (_agentId, input) => {
        if (input.manifestText === 'handoff-manifest') throw new Error('bad manifest');
        return { id: `review-${input.manifestText}`, reviewDigest: 'digest' } as never;
      });
    const install = vi
      .spyOn(communityBundlesOwnerApi, 'install')
      .mockResolvedValue({} as never);
    const onCreated = vi.fn();
    renderSetup({ onCreated });
    const user = userEvent.setup();
    await start(user);
    await user.click(button('Continue'));
    await user.click(button('Coder'));
    expect(screen.getByRole('textbox', { name: 'Agent name' })).toHaveValue('Coder');
    expect(screen.getByRole('textbox', { name: "What it's for" })).toHaveValue(
      'Hands coding work to the coding apps on this computer, then reads the changes and runs the checks itself.',
    );
    await user.click(button('Continue'));
    expect(screen.getByRole('radio', { name: /Operate/ })).toBeChecked();
    expect(screen.getByRole('radio', { name: /^Free/ })).toBeChecked();
    const chips = await screen.findByRole('list', { name: 'Coding apps on this computer' });
    expect(chips).toHaveTextContent('Claude CodeFound · 2.1.286');
    expect(chips).toHaveTextContent('CodexNeeds reinstall');
    expect(chips).toHaveTextContent('OpenCodeNot found');
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    expect(await screen.findByRole('status')).toHaveTextContent(
      "know-how for handing off work didn't install",
    );
    const agent = create.mock.calls[0][0].agent;
    expect(agent.name).toBe('Coder');
    expect(agent.tools).toEqual(expect.arrayContaining(['app_start', 'acp_start']));
    expect(agent.system).toContain('Hands coding work to the coding apps');
    expect(setAutonomy).toHaveBeenCalledWith('coder', { mode: 'free' });
    expect(setPolicy).not.toHaveBeenCalled();
    // Codex is found but broken, so it gets no know-how.
    expect(review.mock.calls.map(([, input]) => input.manifestText)).toEqual([
      'claude-manifest',
      'handoff-manifest',
    ]);
    expect(install).toHaveBeenCalledWith('coder', {
      reviewId: 'review-claude-manifest',
      reviewDigest: 'digest',
    });
    expect(install).toHaveBeenCalledTimes(1);
    expect(onCreated).not.toHaveBeenCalled();
    await user.click(button('Continue'));
    expect(onCreated).toHaveBeenCalledOnce();
  });

  it('uploads a chosen avatar after the companion is created', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    vi.spyOn(daemon, 'bootstrapWorkspace').mockResolvedValue({
      agent: { state: { id: 'main' } },
    } as never);
    const upload = vi.spyOn(daemon, 'setAgentAvatar').mockResolvedValue();
    vi.stubGlobal('URL', {
      ...URL,
      createObjectURL: vi.fn(() => 'blob:avatar'),
      revokeObjectURL: vi.fn(),
    });
    const created = vi.fn();
    renderSetup({ onCreated: created });
    const user = userEvent.setup();
    await start(user);
    await user.click(button('Continue'));
    const file = new File(['png'], 'me.png', { type: 'image/png' });
    await user.upload(screen.getByLabelText('Upload an image'), file);
    expect(screen.getByText(/me\.png/)).toBeVisible();
    await user.click(button('Continue'));
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(created).toHaveBeenCalledOnce());
    expect(upload).toHaveBeenCalledWith('main', file);
  });

  it.each([
    ['Finish setup', 'voice-ember'],
    ["Skip, I'll type", null],
  ])('keeps the chosen voice only on %s', async (action, stored) => {
    const voice = { voiceURI: 'voice-ember', name: 'Ember', lang: 'en-US' };
    vi.stubGlobal('speechSynthesis', {
      getVoices: () => [voice],
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      cancel: vi.fn(),
      speak: vi.fn(),
    });
    vi.stubGlobal('SpeechSynthesisUtterance', vi.fn());
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const create = vi
      .spyOn(daemon, 'bootstrapWorkspace')
      .mockResolvedValue({ agent: { state: { id: 'main' } } } as never);
    renderSetup();
    const user = userEvent.setup();
    await toVoice(user);
    await user.click(screen.getByRole('radio', { name: /Ember/ }));
    await user.click(button(action));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(localStorage.getItem('anima.readAloudVoice')).toBe(stored);
  });

  it('does not bootstrap over a workspace that failed to load', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockRejectedValue(
      new Error('Storage unavailable'),
    );
    const create = vi.spyOn(daemon, 'bootstrapWorkspace');
    renderSetup();
    expect(await screen.findByText('Storage unavailable')).toBeVisible();
    expect(button('Get started')).toBeDisabled();
    expect(create).not.toHaveBeenCalled();
    expect(button('Retry workspace')).toBeVisible();
  });

  it('reuses configured storage without overwriting its mission or existing agents', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue({
      configured: true,
      workspace: {
        rootPath: '/saved',
        companyName: 'Saved',
        mission: 'Keep me',
        values: [],
        hasAvatar: false,
      },
      defaultRoot: '/data/workspace',
    });
    const bootstrap = vi.spyOn(daemon, 'bootstrapWorkspace');
    const create = vi
      .spyOn(daemon, 'createAgent')
      .mockResolvedValue({ agent: { state: { id: 'main' } } } as never);
    renderSetup();
    const user = userEvent.setup();
    await waitFor(() => expect(button('Get started')).toBeEnabled());
    expect(
      screen.queryByRole('button', { name: 'Open an existing workspace' }),
    ).not.toBeInTheDocument();
    await toAccess(user);
    expect(screen.getByRole('textbox', { name: 'Home folder' })).toHaveAttribute(
      'readonly',
    );
    await user.click(button('Continue'));
    await user.click(await screen.findByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(bootstrap).not.toHaveBeenCalled();
    expect(daemon.inspectWorkspace).not.toHaveBeenCalled();
    expect(create).toHaveBeenCalledWith(
      expect.objectContaining({
        settings: { additional: { workspaceRole: 'lead' } },
      }),
    );
    expect(create.mock.calls[0][0].system).toContain('Workspace context: Keep me');
  });

  it('blocks progress when a selected provider becomes unavailable', async () => {
    vi.spyOn(daemon, 'getWorkspace').mockResolvedValue(freshWorkspace);
    const props = {
      providers,
      providersError: null,
      retryProviders: vi.fn(),
      onCreated: vi.fn(),
    };
    const view = render(<CompanionSetup {...props} />);
    const user = userEvent.setup();
    await start(user);
    expect(button('Continue')).toBeEnabled();
    view.rerender(
      <CompanionSetup
        {...props}
        providers={[{ ...providers[0], configured: false }]}
      />,
    );
    expect(button('Continue')).toBeDisabled();
    expect(screen.getByText(/OpenAI is unavailable right now/)).toBeVisible();
  });
});

async function toVoiceFromModel(user: UserEvent) {
  await user.click(button('Continue'));
  await user.click(button('Continue'));
  await user.click(button('Continue'));
  await screen.findByRole('heading', { name: 'Talk out loud?' });
}
