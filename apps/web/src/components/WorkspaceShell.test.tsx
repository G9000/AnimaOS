import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState, type ComponentProps } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { toolNamesForProfile } from '../lib/agent-access';
import { daemon } from '../lib/daemon-api';
import type { HashPage, HashRoute } from '../lib/hash-route';
import type { AgentDetail } from '../lib/types';
import { sessionFixture } from '../test/sessions';
import { SessionSidebar } from './sessions/SessionSidebar';
import { WorkspaceShell } from './WorkspaceShell';

function agent(
  id: string,
  name: string,
  createdAt: number,
  overrides: Partial<AgentDetail> = {},
): AgentDetail {
  return {
    id,
    name,
    provider: 'openai',
    model: 'gpt-4.1',
    toolNames: toolNamesForProfile('collaborate'),
    created_at_ms: createdAt,
    status: 'Idle',
    token_usage: {
      prompt_tokens: 3,
      completion_tokens: 5,
      total_tokens: 8,
    },
    messages: [],
    ...overrides,
  };
}

type ShellProps = Partial<ComponentProps<typeof WorkspaceShell>> & {
  initialRoute?: HashRoute;
};

/** Holds the route the way `useHashRoute` does in the app. */
function Shell({ initialRoute = { kind: 'home' }, ...props }: ShellProps) {
  const [route, setRoute] = useState<HashRoute>(initialRoute);
  const main = props.mainAgent ?? agent('agent-main', 'Nova', 1);
  return (
    <WorkspaceShell
      {...props}
      mainAgent={main}
      agents={props.agents ?? [main]}
      connection={props.connection ?? 'online'}
      conversation={props.conversation ?? <div>Workspace canvas</div>}
      route={route}
      navigate={(next) => setRoute(next)}
    />
  );
}

/** The agent's menu in the top bar: open it and pick an item. */
async function pickFromAgentMenu(
  user: ReturnType<typeof userEvent.setup>,
  name: string | RegExp,
) {
  await user.click(screen.getByRole('button', { name: /, menu$/ }));
  await user.click(screen.getByRole('menuitem', { name }));
}

function mobile() {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({
      matches: false,
      media: '(min-width: 768px)',
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  );
}

const configuredWorkspace = (hasAvatar: boolean) => ({
  configured: true,
  workspace: {
    rootPath: '/workspaces/northwind',
    companyName: 'Northwind Research',
    mission: 'Map supply chains',
    values: ['rigor'],
    hasAvatar,
  },
  defaultRoot: '/workspaces',
});

beforeEach(() => {
  vi.spyOn(daemon, 'agentJobs').mockResolvedValue([]);
  vi.spyOn(daemon, 'agentTasks').mockResolvedValue({
    tasks: [],
    revision: '1',
  });
  vi.spyOn(daemon, 'listSchedules').mockResolvedValue({ schedules: [] });
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe('WorkspaceShell', () => {
  it('passes Work connectivity through the hub to the mounted Tasks editor', async () => {
    const tasks = vi.mocked(daemon.agentTasks).mockResolvedValue({
      revision: '1',
      tasks: [
        { content: 'Saved task', activeForm: 'Saved task', status: 'pending' },
      ],
    });
    const save = vi.spyOn(daemon, 'updateAgentTasks');
    const view = render(
      <Shell
        connection="offline"
        initialRoute={{ kind: 'page', page: 'work' }}
      />,
    );
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeDisabled();
    expect(tasks).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole('button', { name: 'Manage Nova’s tasks' }),
    );
    expect(
      screen.getByRole('button', { name: 'Refresh tasks' }),
    ).toBeDisabled();
    expect(tasks).not.toHaveBeenCalled();

    view.rerender(
      <Shell
        connection="online"
        initialRoute={{ kind: 'page', page: 'work' }}
      />,
    );
    const task = await screen.findByLabelText('Task 1');
    expect(tasks).toHaveBeenCalledTimes(2); // One hub list and one editor read.
    fireEvent.change(task, { target: { value: 'Unsaved Work task' } });
    view.rerender(
      <Shell
        connection="offline"
        initialRoute={{ kind: 'page', page: 'work' }}
      />,
    );
    expect(screen.getByLabelText('Task 1')).toBe(task);
    expect(task).toHaveValue('Unsaved Work task');
    expect(task).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Refresh tasks' }),
    ).toBeDisabled();
    expect(tasks).toHaveBeenCalledTimes(2);

    view.rerender(
      <Shell
        connection="online"
        initialRoute={{ kind: 'page', page: 'work' }}
      />,
    );
    await waitFor(() => expect(task).toBeEnabled());
    expect(screen.getByLabelText('Task 1')).toBe(task);
    expect(task).toHaveValue('Unsaved Work task');
    expect(tasks).toHaveBeenCalledTimes(4);
    expect(save).not.toHaveBeenCalled();
  });

  it('lands on the conversation naming the agent and what it is doing', () => {
    render(<Shell />);
    expect(screen.getByText('Workspace canvas')).toBeVisible();
    expect(
      screen.queryByRole('navigation', { name: 'Workspace navigation' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Operations' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Nova, switch agent' }),
    ).toBeVisible();
    expect(screen.getByRole('status')).toHaveTextContent('Ready');
    // Already in the main chat: no control to go back to it.
    expect(
      screen.queryByRole('button', { name: 'Main chat', exact: true }),
    ).not.toBeInTheDocument();
  });

  it('opens the inbox from the header and offers the weekly report in commands', async () => {
    const user = userEvent.setup();
    const onOpenReport = vi.fn();
    render(
      <Shell
        inbox={{
          count: 2,
          render: (close) => (
            <button type="button" onClick={close}>
              Inbox contents
            </button>
          ),
        }}
        onOpenReport={onOpenReport}
      />,
    );
    const button = screen.getByRole('button', { name: 'Inbox, 2 need you' });
    await user.click(button);
    expect(screen.getByRole('dialog', { name: 'Inbox' })).toBeVisible();
    await user.keyboard('{Escape}');
    expect(
      screen.queryByRole('dialog', { name: 'Inbox' }),
    ).not.toBeInTheDocument();
    expect(button).toHaveFocus();

    await user.click(screen.getByRole('button', { name: 'Open command menu' }));
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'weekly',
    );
    await user.keyboard('{Enter}');
    expect(onOpenReport).toHaveBeenCalled();
  });

  it('switches to another companion and keeps helpers out of the list', async () => {
    const user = userEvent.setup();
    const onSelectAgent = vi.fn();
    const main = agent('agent-main', 'Nova', 1);
    render(
      <Shell
        mainAgent={main}
        agents={[
          main,
          agent('agent-scout', 'Scout', 2, { status: 'Running' }),
          agent('helper', 'Research', 3, { workspaceRole: 'helper' }),
        ]}
        onSelectAgent={onSelectAgent}
      />,
    );
    const trigger = screen.getByRole('button', { name: 'Nova, switch agent' });
    await user.click(trigger);
    const menu = screen.getByRole('menu', { name: 'Your agents' });
    expect(
      within(menu).getByRole('menuitemradio', { name: /Nova/ }),
    ).toHaveAttribute('aria-checked', 'true');
    expect(
      within(menu).getByRole('menuitemradio', { name: /Scout/ }),
    ).toHaveTextContent('Working');
    expect(
      within(menu).queryByRole('menuitemradio', { name: /Research/ }),
    ).not.toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    await user.click(trigger);
    await user.click(screen.getByRole('menuitemradio', { name: /Scout/ }));
    expect(onSelectAgent).toHaveBeenCalledWith('agent-scout');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('names the agent and shows a side thread next to the way back to the main chat', () => {
    render(
      <Shell
        initialRoute={{ kind: 'session', sessionId: 'chat:plans' }}
        sidebar={<div>Sessions list</div>}
        sessions={[sessionFixture('chat:plans', { title: 'Weekend plans' })]}
      />,
    );
    expect(
      screen.getByRole('heading', { name: 'Nova', level: 1 }),
    ).toBeVisible();
    expect(screen.getByText('Weekend plans')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Main chat', exact: true }),
    ).not.toHaveAttribute('aria-current');
    expect(screen.getByRole('button', { name: 'Nova, menu' })).toBeVisible();
    expect(screen.queryByText('Sessions list')).not.toBeInTheDocument();
  });

  it('opens agent creation and management from the dropdown footer', async () => {
    const user = userEvent.setup();
    render(<Shell newAgent={<div>New agent setup</div>} />);
    await user.click(
      screen.getByRole('button', { name: 'Nova, switch agent' }),
    );
    await user.click(screen.getByRole('menuitem', { name: 'New agent' }));
    expect(screen.getByText('New agent setup')).toBeVisible();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    await user.click(
      screen.getByRole('button', { name: 'Nova, switch agent' }),
    );
    await user.click(screen.getByRole('menuitem', { name: 'Manage agents' }));
    expect(screen.getByRole('heading', { name: 'Team' })).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Chat with Nova' }),
    ).toBeVisible();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('shows real agent descriptions and uses shader fallbacks in the dropdown', async () => {
    const user = userEvent.setup();
    const main = agent('nova', 'Nova', 1);
    const view = render(
      <Shell
        mainAgent={main}
        agents={[
          main,
          agent('scout', 'Scout', 2, { bio: 'Research and news' }),
        ]}
      />,
    );
    await user.click(
      screen.getByRole('button', { name: 'Nova, switch agent' }),
    );
    const scout = screen.getByRole('menuitemradio', { name: /Scout/ });
    expect(scout).toHaveTextContent('Ready · Research and news');
    for (const img of view.container.querySelectorAll('.agent-switch-menu img'))
      fireEvent.error(img);
    expect(scout.querySelector('.halo-orb')).not.toBeNull();
    expect(
      view.container.querySelectorAll(
        '.agent-switch-menu .agent-switch-status-dot',
      ),
    ).toHaveLength(2);
  });

  it('keeps agent footer actions disabled while settings are saving', async () => {
    render(<Shell locked />);
    await userEvent.click(
      screen.getByRole('button', { name: 'Nova, switch agent' }),
    );
    expect(screen.getByRole('menuitem', { name: 'New agent' })).toBeDisabled();
    expect(
      screen.getByRole('menuitem', { name: 'Manage agents' }),
    ).toBeDisabled();
  });

  it('opens commands with Control K, filters actions and navigates with Enter', async () => {
    const user = userEvent.setup();
    render(<Shell connectors={<div>Manage connections</div>} />);
    await user.keyboard('{Control>}k{/Control}');
    expect(screen.getByRole('dialog', { name: 'Command menu' })).toBeVisible();
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'connections',
    );
    await user.keyboard('{Enter}');
    expect(screen.getByText('Manage connections')).toBeVisible();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('offers Export in commands only when the conversation can be exported', async () => {
    const user = userEvent.setup();
    const onExportConversation = vi.fn();
    const view = render(<Shell />);
    await user.keyboard('{Control>}k{/Control}');
    expect(
      screen.queryByRole('option', { name: /Export this conversation/ }),
    ).not.toBeInTheDocument();
    await user.keyboard('{Escape}');
    view.unmount();

    render(<Shell onExportConversation={onExportConversation} />);
    await user.keyboard('{Control>}k{/Control}');
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'export',
    );
    await user.keyboard('{Enter}');
    expect(onExportConversation).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('contains keyboard focus in commands and restores the opener on Escape', async () => {
    const user = userEvent.setup();
    render(<Shell />);
    const opener = screen.getByRole('button', { name: 'Open command menu' });
    await user.click(opener);
    const search = screen.getByRole('combobox', { name: 'Search commands' });
    expect(search).toHaveFocus();
    await user.tab();
    expect(
      screen.getByRole('button', { name: 'Close command menu' }),
    ).toHaveFocus();
    await user.tab();
    expect(search).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(opener).toHaveFocus();
  });

  it('opens saved message search with Meta K and forwards the exact selected match', async () => {
    const match = {
      sessionId: 'chat:plans',
      title: 'Saved conversation',
      messageId: 'deep-message',
      role: 'user' as const,
      createdAtMs: 1_725_000_000_000,
      snippet: 'Unique saved message match',
    };
    const searchMessages = vi.fn(async () => ({
      results: [match],
      hasMore: false,
    }));
    const onOpenMessage = vi.fn();
    render(
      <Shell searchMessages={searchMessages} onOpenMessage={onOpenMessage} />,
    );
    const user = userEvent.setup();
    await user.keyboard('{Meta>}k{/Meta}');
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'needle',
    );
    await user.click(
      await screen.findByRole('option', { name: /Unique saved message match/ }),
    );
    expect(searchMessages).toHaveBeenCalledWith(
      'needle',
      expect.any(AbortSignal),
    );
    expect(onOpenMessage).toHaveBeenCalledWith(match);
    expect(
      screen.queryByRole('dialog', { name: 'Command menu' }),
    ).not.toBeInTheDocument();
  });

  it('inserts a prompt from commands without invoking a send', async () => {
    const user = userEvent.setup();
    const pick = vi.fn();
    render(<Shell onPickPrompt={pick} />);
    await user.click(screen.getByRole('button', { name: 'Open command menu' }));
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'Plan my next hour',
    );
    await user.keyboard('{Enter}');
    expect(pick).toHaveBeenCalledWith(
      expect.stringContaining('Help me plan my next hour'),
    );
    expect(screen.getByText('Workspace canvas')).toBeVisible();
  });

  it('keeps helpers out of the navigation', () => {
    const nova = agent('agent-main', 'Nova', 1);
    render(
      <Shell
        mainAgent={nova}
        agents={[nova, agent('scout', 'Scout', 2)]}
        initialRoute={{ kind: 'page', page: 'memory' }}
      />,
    );
    expect(
      screen.queryByRole('button', { name: 'Team' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Message Scout' }),
    ).not.toBeInTheDocument();
  });

  it('opens pages in the Settings window and returns to the mounted conversation', async () => {
    render(<Shell initialRoute={{ kind: 'page', page: 'work' }} />);
    const page = screen.getByRole('region', { name: 'Settings' });
    const navigation = within(page).getByRole('navigation', {
      name: 'Workspace navigation',
    });
    expect(navigation).toHaveAttribute('data-placement', 'sidebar');
    expect(navigation).toHaveAttribute('aria-orientation', 'vertical');
    expect(
      within(navigation).getByRole('button', { name: 'Work' }),
    ).toHaveAttribute('aria-current', 'page');
    expect(screen.getByText('Workspace canvas')).not.toBeVisible();
    await userEvent.click(
      screen.getByRole('button', { name: 'Open companion chat' }),
    );
    expect(screen.getByText('Workspace canvas')).toBeVisible();
    expect(
      screen.queryByRole('region', { name: 'Settings' }),
    ).not.toBeInTheDocument();
  });

  it('shows the conversation for a page the shell does not offer', () => {
    // Every page of the route table is built now; an unknown one still falls back.
    render(
      <Shell
        initialRoute={{ kind: 'page', page: 'later' as unknown as HashPage }}
      />,
    );
    expect(screen.getByText('Workspace canvas')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Open companion chat' }),
    ).not.toBeInTheDocument();
  });

  it('groups settings by what the owner decides, the companion first', () => {
    render(<Shell initialRoute={{ kind: 'page', page: 'skills' }} />);
    const nav = screen.getByRole('navigation', {
      name: 'Workspace navigation',
    });
    expect(
      within(nav)
        .getAllByRole('button')
        .map((button) => button.textContent?.trim()),
    ).toEqual([
      'Profile',
      'Model',
      'Permissions',
      'Skills',
      'Memory',
      'Tools',
      'Connections',
      'Automations',
      'Work',
      'Files',
      'Appearance',
      'Notifications',
      'Runtime',
      'Usage',
      'Diagnostics',
      'Logs',
    ]);
    expect(
      within(nav)
        .getAllByRole('group')
        .map((group) => group.getAttribute('aria-label')),
    ).toEqual([
      'Agent',
      'Learning',
      'Abilities',
      'Activity',
      'App · all agents',
    ]);
  });

  it('opens Appearance as an app-wide page with the chat background choice', () => {
    render(<Shell initialRoute={{ kind: 'page', page: 'appearance' }} />);
    expect(screen.getByText('All agents')).toBeInTheDocument();
    expect(
      screen.getByRole('group', { name: 'Chat background' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Appearance' })).toHaveAttribute(
      'aria-current',
      'page',
    );
  });

  it('says whose a page is under its title', () => {
    const { unmount } = render(
      <Shell initialRoute={{ kind: 'page', page: 'model' }} />,
    );
    expect(screen.getByText(/only this agent/)).toBeInTheDocument();
    expect(
      screen.getByText(/what to fall back on\. Each agent has its own\./),
    ).toBeInTheDocument();
    unmount();
    render(<Shell initialRoute={{ kind: 'page', page: 'runtime' }} />);
    expect(screen.getByText('All agents')).toBeInTheDocument();
    expect(screen.queryByText(/only this agent/)).not.toBeInTheDocument();
  });

  it('opens the companion pages from the window', async () => {
    render(
      <Shell
        profile={<p>Profile page</p>}
        model={<p>Model page</p>}
        runtime={<p>Runtime page</p>}
        initialRoute={{ kind: 'page', page: 'usage' }}
      />,
    );
    for (const [label, content] of [
      ['Profile', 'Profile page'],
      ['Model', 'Model page'],
      ['Runtime', 'Runtime page'],
    ]) {
      await userEvent.click(screen.getByRole('button', { name: label }));
      expect(screen.getByText(content)).toBeVisible();
      expect(screen.getByRole('button', { name: label })).toHaveAttribute(
        'aria-current',
        'page',
      );
    }
  });

  it('holds the window on its page while a save or reset settles', () => {
    render(
      <Shell
        locked
        profile={<p>Profile page</p>}
        initialRoute={{ kind: 'page', page: 'profile' }}
      />,
    );
    const nav = screen.getByRole('navigation', {
      name: 'Workspace navigation',
    });
    for (const button of within(nav).getAllByRole('button'))
      expect(button).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Open companion chat' }),
    ).toBeDisabled();
  });

  it.each([
    ['Skills', 'Skills page'],
    ['Memory', 'Memory page'],
    ['Tools', 'Registered tools and runtime details'],
    ['Connections', 'Connections page'],
    ['Automations', 'Automations page'],
    ['Usage', 'Usage page'],
    ['Diagnostics', 'Health page'],
    ['Logs', 'Logs page'],
  ] as const)(
    'opens %s from the window navigation and the command menu',
    async (label, content) => {
      vi.spyOn(daemon, 'capabilities').mockResolvedValue({
        schemaVersion: 1,
        tools: [],
        persistence: {
          controlPlane: 'file',
          memory: 'file',
          executionJournal: false,
        },
        extensions: [],
        limitations: [],
      });
      const user = userEvent.setup();
      render(
        <Shell
          initialRoute={{ kind: 'page', page: 'usage' }}
          skills={<div>Skills page</div>}
          memory={<div>Memory page</div>}
          connectors={<div>Connections page</div>}
          automations={<div>Automations page</div>}
          usage={<div>Usage page</div>}
          health={<div>Health page</div>}
          logs={<div>Logs page</div>}
        />,
      );
      const nav = screen.getByRole('navigation', {
        name: 'Workspace navigation',
      });
      await user.click(within(nav).getByRole('button', { name: label }));
      expect(
        await screen.findByText(content),
      ).toBeVisible();
      expect(within(nav).getByRole('button', { name: label })).toHaveAttribute(
        'aria-current',
        'page',
      );
      expect(screen.getByRole('region', { name: 'Settings' })).toBeVisible();

      await user.click(
        screen.getByRole('button', { name: 'Open command menu' }),
      );
      expect(
        screen.getByRole('option', { name: new RegExp(`Go to ${label}`) }),
      ).toBeVisible();
    },
  );

  it('shows waiting approvals in the top bar and on Permissions', async () => {
    const user = userEvent.setup();
    render(
      <Shell approvals={<div>Approvals page</div>} pendingApprovals={2} />,
    );
    await user.click(
      screen.getByRole('button', { name: 'Approvals, 2 waiting' }),
    );
    expect(screen.getByText('Approvals page')).toBeVisible();
    expect(screen.getByText('Workspace canvas')).not.toBeVisible();
    expect(
      within(
        screen.getByRole('navigation', { name: 'Workspace navigation' }),
      ).getByRole('button', { name: 'Permissions, 2 waiting' }),
    ).toHaveAttribute('aria-current', 'page');
  });

  it('keeps the top bar quiet and names Permissions plainly when nothing waits', () => {
    render(
      <Shell
        approvals={<div>Approvals page</div>}
        initialRoute={{ kind: 'page', page: 'approvals' }}
      />,
    );
    expect(
      screen.queryByRole('button', { name: /waiting/ }),
    ).not.toBeInTheDocument();
    expect(
      within(
        screen.getByRole('navigation', { name: 'Workspace navigation' }),
      ).getByRole('button', { name: 'Permissions' }),
    ).toBeVisible();
  });

  it('reviews approvals and opens sessions by title from commands', async () => {
    const user = userEvent.setup();
    const onOpenSession = vi.fn();
    const plans = sessionFixture('chat:plans', { title: 'Weekend plans' });
    render(
      <Shell
        approvals={<div>Approvals page</div>}
        pendingApprovals={1}
        sessions={[
          plans,
          sessionFixture('chat:old', { title: 'Old archive', archived: true }),
        ]}
        onOpenSession={onOpenSession}
      />,
    );

    await user.keyboard('{Control>}k{/Control}');
    expect(
      screen.getByRole('option', { name: /Review tool approvals/ }),
    ).toHaveTextContent('1 waiting for you');
    expect(
      screen.getByRole('option', { name: /Go to Permissions/ }),
    ).toBeVisible();
    expect(
      screen.queryByRole('option', { name: /Old archive/ }),
    ).not.toBeInTheDocument();
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'weekend',
    );
    await user.keyboard('{Enter}');
    expect(onOpenSession).toHaveBeenCalledWith(plans);

    await user.keyboard('{Control>}k{/Control}');
    await user.type(
      screen.getByRole('combobox', { name: 'Search commands' }),
      'review',
    );
    await user.keyboard('{Enter}');
    expect(screen.getByText('Approvals page')).toBeVisible();
  });

  it('keeps workspace avatar controls in Profile while the sidebar follows the design', async () => {
    const user = userEvent.setup();
    const onChangeWorkspaceAvatar = vi.fn().mockResolvedValue(undefined);
    localStorage.clear();
    localStorage.setItem('anima.consoleSession', 'signed-header');
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation(async () =>
        new Response(new Uint8Array([1]), {
          headers: { 'content-type': 'image/png' },
        }),
      ),
    );
    Object.defineProperties(URL, {
      createObjectURL: {
        configurable: true,
        value: vi.fn(() => 'blob:workspace-avatar-preview'),
      },
      revokeObjectURL: { configurable: true, value: vi.fn() },
    });
    render(
      <Shell
        initialRoute={{ kind: 'page', page: 'profile' }}
        workspaceState={configuredWorkspace(true)}
        onChangeWorkspaceAvatar={onChangeWorkspaceAvatar}
      />,
    );
    const sidebar = screen.getByRole('complementary');
    expect(within(sidebar).queryByText('Northwind Research')).toBeNull();
    await user.click(screen.getByText('Workspace identity', { exact: true }));
    expect(screen.getByText('Northwind Research')).toBeVisible();
    await waitFor(() =>
      expect(
        screen
          .getByRole('button', { name: 'Change workspace avatar' })
          .querySelector('img'),
      ).toHaveAttribute('src', 'blob:workspace-avatar-preview'),
    );
    const file = new File(['avatar'], 'avatar.png', { type: 'image/png' });
    await user.upload(
      screen.getByLabelText('Workspace avatar image file'),
      file,
    );
    await waitFor(() =>
      expect(onChangeWorkspaceAvatar).toHaveBeenCalledWith(file),
    );
  });

  it('hides the company name when the workspace state is not configured', () => {
    render(
      <Shell
        initialRoute={{ kind: 'page', page: 'memory' }}
        workspaceState={{
          configured: false,
          workspace: null,
          defaultRoot: '/workspaces',
        }}
      />,
    );
    expect(
      within(screen.getByRole('complementary')).queryByText(
        'Northwind Research',
      ),
    ).not.toBeInTheDocument();
  });

  it('opens the thread switcher from the thread button', async () => {
    const user = userEvent.setup();
    render(<Shell sidebar={<div>Sessions list</div>} />);
    expect(screen.queryByText('Sessions list')).not.toBeInTheDocument();
    await pickFromAgentMenu(user, 'Other conversations');
    const drawer = screen.getByRole('dialog', { name: 'History' });
    expect(within(drawer).getByText('Sessions list')).toBeVisible();
    await user.click(
      within(drawer).getByRole('button', { name: 'Close history' }),
    );
    expect(
      screen.queryByRole('dialog', { name: 'History' }),
    ).not.toBeInTheDocument();
  });

  it('moves focus into the sessions drawer, keeps it there, and closes on Escape', async () => {
    mobile();
    const user = userEvent.setup();
    render(<Shell sidebar={<input aria-label="Search sessions" />} />);
    const opener = screen.getByRole('button', { name: 'Nova, menu' });
    await pickFromAgentMenu(user, 'Other conversations');
    const drawer = screen.getByRole('dialog', { name: 'History' });
    const newChat = within(drawer).getByRole('button', { name: 'Main chat' });
    const search = within(drawer).getByRole('textbox', {
      name: 'Search sessions',
    });
    expect(newChat).toHaveFocus();
    await user.tab();
    expect(
      within(drawer).getByRole('button', { name: 'Close history' }),
    ).toHaveFocus();
    await user.tab();
    expect(search).toHaveFocus();
    const importButton = within(drawer).getByRole('button', {
      name: 'Import from Claude Code or Codex',
    });
    await user.tab();
    expect(importButton).toHaveFocus();
    await user.tab();
    expect(newChat).toHaveFocus();
    await user.tab({ shift: true });
    expect(importButton).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(
      screen.queryByRole('dialog', { name: 'History' }),
    ).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('keeps the sessions drawer modal after a row action and lets row controls handle their own Escape', async () => {
    mobile();
    const user = userEvent.setup();
    const onArchive = vi.fn().mockResolvedValue(undefined);
    render(
      <Shell
        sidebar={
          <SessionSidebar
            sessions={[
              sessionFixture('chat:trip', {
                title: 'Trip ideas',
                lastActivityAtMs: Date.now(),
              }),
            ]}
            activeKey={null}
            query=""
            onQueryChange={vi.fn()}
            showArchived={false}
            onShowArchivedChange={vi.fn()}
            onOpen={vi.fn()}
            onRename={vi.fn().mockResolvedValue(true)}
            onArchive={onArchive}
            onExport={vi.fn().mockResolvedValue(undefined)}
            onDelete={vi.fn().mockResolvedValue(undefined)}
          />
        }
      />,
    );
    const opener = screen.getByRole('button', { name: 'Nova, menu' });
    await pickFromAgentMenu(user, 'Other conversations');
    const drawer = screen.getByRole('dialog', { name: 'History' });
    // Everything behind the drawer is out of reach while it is open.
    expect(opener.closest('[inert]')).not.toBeNull();
    expect(screen.queryByRole('banner')).not.toBeInTheDocument();

    // Archive closes its menu and returns focus to its own trigger (D5),
    // rather than falling through to the panel's generic body-focus rescue.
    await user.click(
      within(drawer).getByRole('button', { name: 'Actions for Trip ideas' }),
    );
    await user.click(within(drawer).getByRole('menuitem', { name: 'Archive' }));
    expect(onArchive).toHaveBeenCalledOnce();
    expect(
      within(drawer).getByRole('button', { name: 'Actions for Trip ideas' }),
    ).toHaveFocus();
    await user.tab();
    expect(drawer).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard('{Escape}');
    expect(
      screen.queryByRole('dialog', { name: 'History' }),
    ).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
    expect(opener.closest('[inert]')).toBeNull();

    // Escape in a row menu or the rename field closes only that.
    await pickFromAgentMenu(user, 'Other conversations');
    const reopened = screen.getByRole('dialog', { name: 'History' });
    await user.click(
      within(reopened).getByRole('button', { name: 'Actions for Trip ideas' }),
    );
    await user.keyboard('{Escape}');
    expect(within(reopened).queryByRole('menu')).not.toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'History' })).toBe(reopened);
    await user.click(
      within(reopened).getByRole('button', { name: 'Actions for Trip ideas' }),
    );
    await user.click(
      within(reopened).getByRole('menuitem', { name: 'Rename' }),
    );
    expect(
      within(reopened).getByRole('textbox', { name: 'Rename Trip ideas' }),
    ).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(
      within(reopened).queryByRole('textbox', { name: 'Rename Trip ideas' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'History' })).toBe(reopened);
  });

  it('moves focus to the next row in the sessions drawer once an archived row leaves the list', async () => {
    // R4 (residual round): the archived row's own trigger unmounts once the
    // refresh drops it; focus goes to the next row, not the drawer panel.
    mobile();
    const user = userEvent.setup();
    function ListedSidebar() {
      const [sessions, setSessions] = useState(() =>
        ['Trip ideas', 'Budget'].map((title, index) =>
          sessionFixture(`chat:${index}`, {
            title,
            lastActivityAtMs: Date.now() - index * 1_000,
          }),
        ),
      );
      return (
        <SessionSidebar
          sessions={sessions}
          activeKey={null}
          query=""
          onQueryChange={vi.fn()}
          showArchived={false}
          onShowArchivedChange={vi.fn()}
          onOpen={vi.fn()}
          onRename={vi.fn().mockResolvedValue(true)}
          onArchive={async (session) => {
            // The refresh after the archive no longer lists it.
            setSessions((current) =>
              current.filter((item) => item.id !== session.id),
            );
            return true;
          }}
          onExport={vi.fn().mockResolvedValue(undefined)}
          onDelete={vi.fn().mockResolvedValue(true)}
        />
      );
    }
    render(<Shell sidebar={<ListedSidebar />} />);
    await pickFromAgentMenu(user, 'Other conversations');
    const drawer = screen.getByRole('dialog', { name: 'History' });

    await user.click(
      within(drawer).getByRole('button', { name: 'Actions for Trip ideas' }),
    );
    await user.click(within(drawer).getByRole('menuitem', { name: 'Archive' }));

    await waitFor(() =>
      expect(
        within(drawer).getByRole('button', { name: 'Budget' }),
      ).toHaveFocus(),
    );
    expect(
      within(drawer).queryByRole('button', { name: 'Trip ideas' }),
    ).not.toBeInTheDocument();
  });

  it('shows working helpers as status without introducing another persona', () => {
    const main = agent('agent-main', 'Nova', 1);
    render(
      <Shell
        mainAgent={main}
        agents={[
          agent('helper', 'Research', 2, {
            status: 'Running',
            workspaceRole: 'helper',
          }),
          main,
        ]}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('1 helper working');
    expect(
      screen.queryByRole('article', { name: 'Research agent' }),
    ).not.toBeInTheDocument();
  });

  it('opens Settings on the companion profile from the top bar', async () => {
    render(<Shell profile={<p>Profile page</p>} />);
    await pickFromAgentMenu(userEvent.setup(), "Nova's settings");
    expect(screen.getByText('Profile page')).toBeVisible();
    expect(screen.getByRole('region', { name: 'Settings' })).toBeVisible();
  });

  it('puts the window navigation after the page on mobile', () => {
    mobile();
    render(
      <Shell
        initialRoute={{ kind: 'page', page: 'usage' }}
        usage={<button type="button">Usage action</button>}
      />,
    );
    const navigation = screen.getByRole('navigation', {
      name: 'Workspace navigation',
    });
    expect(navigation).toHaveAttribute('data-placement', 'bottom-dock');
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
    expect(
      within(navigation).getByRole('button', { name: 'Usage' }),
    ).toHaveAttribute('aria-current', 'page');
    expect(
      screen
        .getByRole('button', { name: 'Usage action' })
        .compareDocumentPosition(navigation) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
  });

  it('opens Connections as a page', () => {
    render(
      <Shell
        connectors={<div>Manage connections</div>}
        initialRoute={{ kind: 'page', page: 'connectors' }}
      />,
    );
    expect(screen.getByText('Manage connections')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Telegram' }),
    ).not.toBeInTheDocument();
  });
});

it('filters settings destinations and restores the complete sidebar after navigation', async () => {
  const user = userEvent.setup();
  render(
    <Shell
      initialRoute={{ kind: 'page', page: 'profile' }}
      model={<p>Model page</p>}
    />,
  );
  const search = screen.getByRole('searchbox', { name: 'Search settings' });
  await user.type(search, 'model');
  const nav = screen.getByRole('navigation', { name: 'Workspace navigation' });
  expect(within(nav).getAllByRole('button')).toHaveLength(1);
  await user.click(within(nav).getByRole('button', { name: 'Model' }));
  expect(screen.getByText('Model page')).toBeVisible();
  expect(search).toHaveValue('');
  expect(within(nav).getAllByRole('button')).toHaveLength(16);
  expect(within(nav).getByRole('button', { name: 'Model' })).toHaveAttribute(
    'aria-current',
    'page',
  );
});

it('focuses settings search with the platform shortcut and clears a failed search with Escape', async () => {
  const user = userEvent.setup();
  render(<Shell initialRoute={{ kind: 'page', page: 'memory' }} />);
  await user.keyboard('{Control>}k{/Control}');
  const search = screen.getByRole('searchbox', { name: 'Search settings' });
  expect(search).toHaveFocus();
  expect(
    screen.queryByRole('combobox', { name: 'Search commands' }),
  ).toBeNull();
  await user.type(search, 'no matching destination');
  expect(
    within(
      screen.getByRole('navigation', { name: 'Workspace navigation' }),
    ).getByRole('status'),
  ).toHaveTextContent('No settings found.');
  await user.keyboard('{Escape}');
  expect(search).toHaveValue('');
  expect(screen.getByRole('button', { name: 'Memory' })).toHaveAttribute(
    'aria-current',
    'page',
  );
});

it('keeps settings search and navigation locked during a save', () => {
  render(<Shell locked initialRoute={{ kind: 'page', page: 'profile' }} />);
  expect(
    screen.getByRole('searchbox', { name: 'Search settings' }),
  ).toBeDisabled();
  fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
  expect(
    screen.queryByRole('combobox', { name: 'Search commands' }),
  ).toBeNull();
});
