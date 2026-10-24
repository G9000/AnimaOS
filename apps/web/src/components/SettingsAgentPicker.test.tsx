import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { toolNamesForProfile } from '../lib/agent-access';
import type { AgentDetail } from '../lib/types';
import { SettingsAgentPicker } from './SettingsAgentPicker';

function agent(
  id: string,
  name: string,
  workspaceRole: AgentDetail['workspaceRole'] = 'lead',
): AgentDetail {
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
    workspaceRole,
  };
}

describe('settings agent picker', () => {
  it('lists companions, omits helpers, and switches the settings agent', async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <SettingsAgentPicker
        agent={agent('main', 'Nova')}
        agents={[
          agent('main', 'Nova'),
          agent('scout', 'Scout'),
          agent('helper', 'Background helper', 'helper'),
        ]}
        onSelect={onSelect}
      />,
    );

    const trigger = screen.getByRole('button', { name: 'Settings agent: Nova' });
    await user.click(trigger);

    const menu = screen.getByRole('menu', { name: 'Choose an agent for settings' });
    expect(menu).toBeVisible();
    expect(
      within(menu).getByRole('menuitemradio', { name: 'Nova' }),
    ).toHaveAttribute('aria-checked', 'true');
    expect(
      within(menu).getByRole('menuitemradio', { name: 'Scout' }),
    ).toBeVisible();
    expect(
      within(menu).queryByRole('menuitemradio', { name: 'Background helper' }),
    ).toBeNull();

    await user.click(screen.getByRole('menuitemradio', { name: 'Scout' }));

    expect(onSelect).toHaveBeenCalledExactlyOnceWith('scout');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it('closes with Escape and restores focus to the picker', async () => {
    const user = userEvent.setup();
    render(
      <SettingsAgentPicker
        agent={agent('main', 'Nova')}
        agents={[agent('main', 'Nova'), agent('scout', 'Scout')]}
        onSelect={vi.fn()}
      />,
    );

    const trigger = screen.getByRole('button', { name: 'Settings agent: Nova' });
    await user.click(trigger);
    await user.keyboard('{Escape}');

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });
});
