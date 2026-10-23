import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { toolNamesForProfile } from '../lib/agent-access';
import type { AgentDetail } from '../lib/types';
import { AgentSwitcher } from './AgentSwitcher';

function agent(id: string, name: string): AgentDetail {
  return {
    id,
    name,
    provider: 'openai',
    model: 'gpt-4.1',
    toolNames: toolNamesForProfile('collaborate'),
    created_at_ms: 1,
    status: 'Idle',
    token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
    messages: [],
  };
}

const nova = agent('a1', 'Nova');

function setup(
  props: Partial<React.ComponentProps<typeof AgentSwitcher>> = {},
  agents: AgentDetail[] = [nova],
) {
  const handlers = {
    onOpenConversations: vi.fn(),
    onImport: vi.fn(),
    onOpenMemory: vi.fn(),
    onOpenSettings: vi.fn(),
    onSelect: vi.fn(),
    onNewAgent: vi.fn(),
    onManageAgents: vi.fn(),
  };
  render(
    <AgentSwitcher
      agent={nova}
      agents={agents}
      online
      status="Ready"
      busy={false}
      {...handlers}
      {...props}
    />,
  );
  return { user: userEvent.setup(), ...handlers };
}

describe('agent switcher in the top bar', () => {
  it('names the agent with what it is doing, and marks it busy', () => {
    setup({ status: 'Working on restart recovery', busy: true });
    expect(screen.getByRole('heading', { name: 'Nova' })).toBeInTheDocument();
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Working on restart recovery');
    expect(status).toHaveClass('is-busy');
  });

  it('opens the agent menu from the name, with each item doing its own thing', async () => {
    const { user, ...handlers } = setup();
    await user.click(screen.getByRole('button', { name: 'Nova, menu' }));
    const menu = screen.getByRole('menu', { name: 'Nova menu' });
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((item) => item.textContent),
    ).toEqual([
      'Other conversations',
      'Import a conversation',
      'What Nova remembers',
      "Nova's settings",
    ]);

    await user.click(
      within(menu).getByRole('menuitem', { name: 'Other conversations' }),
    );
    expect(handlers.onOpenConversations).toHaveBeenCalledOnce();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Nova, menu' }));
    await user.click(screen.getByRole('menuitem', { name: "Nova's settings" }));
    expect(handlers.onOpenSettings).toHaveBeenCalledOnce();
    await user.click(screen.getByRole('button', { name: 'Nova, menu' }));
    await user.click(
      screen.getByRole('menuitem', { name: 'What Nova remembers' }),
    );
    expect(handlers.onOpenMemory).toHaveBeenCalledOnce();
    await user.click(screen.getByRole('button', { name: 'Nova, menu' }));
    await user.click(
      screen.getByRole('menuitem', { name: 'Import a conversation' }),
    );
    expect(handlers.onImport).toHaveBeenCalledOnce();
  });

  it('leaves out menu items that have nowhere to go', async () => {
    const { user } = setup({
      onOpenConversations: undefined,
      onImport: undefined,
    });
    await user.click(screen.getByRole('button', { name: 'Nova, menu' }));
    expect(
      screen.queryByRole('menuitem', { name: 'Other conversations' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: "Nova's settings" }),
    ).toBeVisible();
  });

  it('keeps the two menus apart and closes either on Escape', async () => {
    const { user } = setup();
    const name = screen.getByRole('button', { name: 'Nova, menu' });
    const stack = screen.getByRole('button', { name: 'Nova, switch agent' });
    await user.click(name);
    await user.click(stack);
    expect(screen.queryByRole('menu', { name: 'Nova menu' })).toBeNull();
    expect(screen.getByRole('menu', { name: 'Your agents' })).toBeVisible();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(stack).toHaveFocus();
    await user.click(name);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(name).toHaveFocus();
  });

  it('shows the other agents as coins and counts the rest', () => {
    const others = ['Coder', 'Scout', 'Writer', 'Ledger', 'Atlas', 'Echo'].map(
      (name, index) => agent(`o${index}`, name),
    );
    setup({}, [nova, ...others]);
    const stack = screen.getByRole('button', { name: 'Nova, switch agent' });
    // Four coins by name, then "+2" for the two that do not fit.
    expect(stack.querySelectorAll('.agent-coin')).toHaveLength(5);
    expect(within(stack).getByText('+2')).toBeInTheDocument();
  });

  it('switches agent from the coin list and offers a new one', async () => {
    const { user, ...handlers } = setup({}, [nova, agent('a2', 'Scout')]);
    await user.click(screen.getByRole('button', { name: 'Nova, switch agent' }));
    await user.click(screen.getByRole('menuitemradio', { name: /Scout/ }));
    expect(handlers.onSelect).toHaveBeenCalledWith('a2');
    await user.click(screen.getByRole('button', { name: 'Nova, switch agent' }));
    await user.click(screen.getByRole('menuitem', { name: 'New agent' }));
    expect(handlers.onNewAgent).toHaveBeenCalledOnce();
  });

  it('marks the avatar when something waits on the owner', () => {
    const view = render(
      <AgentSwitcher
        agent={nova}
        agents={[nova]}
        online
        status="Ready"
        busy={false}
        needsYou
        onNewAgent={vi.fn()}
        onManageAgents={vi.fn()}
      />,
    );
    expect(view.container.querySelector('.agent-switch-notch')).not.toBeNull();
  });

  it('shows a side thread by its title, without the ring', () => {
    const view = render(
      <AgentSwitcher
        agent={nova}
        agents={[nova]}
        online
        status="Ready"
        busy
        title="Saturday trip"
        subtitle="Telegram chat"
        onNewAgent={vi.fn()}
        onManageAgents={vi.fn()}
      />,
    );
    expect(screen.getByText('Saturday trip')).toBeVisible();
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Telegram chat');
    // A thread is not the agent working, so it never shows as busy.
    expect(status).not.toHaveClass('is-busy');
    expect(view.container.querySelector('.agent-switch-ring')).toHaveClass(
      'is-thread',
    );
  });
});
