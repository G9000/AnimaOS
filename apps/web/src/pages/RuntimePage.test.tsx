import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import type { DaemonWorkspaceState } from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import { RuntimePage } from './RuntimePage';

const agent = {
  id: 'agent-main',
  name: 'Nova',
  provider: 'openai',
  model: 'gpt-5.6',
  toolNames: [],
  created_at_ms: 1,
  status: 'Idle',
  token_usage: { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 },
  system: '',
  messages: [],
} as unknown as AgentDetail;

const configured: DaemonWorkspaceState = {
  configured: true,
  workspace: {
    rootPath: '/workspaces/northwind',
    companyName: 'Northwind Research',
    mission: 'Map supply chains',
    values: ['rigor'],
  },
  defaultRoot: '/workspaces',
};

describe('RuntimePage', () => {
  it('marks only the working options as in use', () => {
    render(<RuntimePage agent={agent} workspace={configured} online />);
    const home = screen.getByRole('list', { name: 'Where Nova lives' });
    const current = within(home)
      .getAllByRole('listitem')
      .filter((item) => item.getAttribute('aria-current') === 'true');
    expect(current).toHaveLength(1);
    expect(current[0]).toHaveTextContent('This computer');
    expect(
      within(home).getByText('A server you own').closest('li'),
    ).toHaveTextContent('Not available yet');
    const commands = screen.getByRole('list', { name: 'Where commands run' });
    expect(within(commands).getAllByText('Not available yet')).toHaveLength(2);
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('shows the server as running and lists the real anima commands', () => {
    render(<RuntimePage agent={agent} workspace={configured} online />);
    expect(screen.getByText('Running')).toBeVisible();
    const cli = screen.getByRole('heading', { name: 'Command line' });
    expect(cli).toBeVisible();
    expect(screen.getByText('anima logs --follow')).toBeVisible();
    expect(screen.getByText('anima update')).toBeVisible();
  });

  it('shows the configured workspace read-only', () => {
    render(<RuntimePage agent={agent} workspace={configured} online />);
    expect(screen.getByRole('heading', { name: 'Workspace' })).toBeVisible();
    expect(screen.getByText('Northwind Research')).toBeVisible();
    expect(screen.getByText('Map supply chains')).toBeVisible();
    expect(screen.getByText('/workspaces/northwind')).toBeVisible();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it.each([null, { configured: false, workspace: null, defaultRoot: '/w' }])(
    'hides the workspace section when workspace state is %o',
    (workspace) => {
      render(
        <RuntimePage
          agent={agent}
          workspace={workspace as DaemonWorkspaceState | null}
          online={false}
        />,
      );
      expect(screen.queryByRole('heading', { name: 'Workspace' })).toBeNull();
      expect(screen.getByText('Offline')).toBeVisible();
    },
  );
});
