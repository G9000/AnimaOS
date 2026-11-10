import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { WorkspaceContextView } from '@animaos/sdk';
import { WorkspaceContextPanel } from './WorkspaceContextPanel';

const companion: WorkspaceContextView = {
  revision: 'r1',
  status: 'configured',
  binding: {
    connectionId: 'c1',
    tool: 'mother_get_context',
    mode: 'companion',
    workspaceId: 'w',
    generation: 'g',
    subject: 'a',
  },
  connections: [
    {
      id: 'c1',
      label: 'M.O.T.H.E.R',
      state: 'disconnected',
      tool: 'mother_get_context',
    },
  ],
};
const independent: WorkspaceContextView = {
  revision: 'r2',
  status: 'independent',
  binding: null,
  connections: [],
};

describe('per-agent workspace context settings', () => {
  it('shows an invalid saved binding as repairable rather than independent', async () => {
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => ({ ...independent, status: 'configured' })}
      />,
    );
    expect(await screen.findByText('Connection needs attention')).toBeVisible();
    expect(screen.queryByText('Independent agent')).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Use this agent independently' }),
    ).toBeEnabled();
  });
  it('explains same-chat fallback and disables only the selected agent with revision checking', async () => {
    const save = vi.fn().mockResolvedValue(independent);
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => companion}
        save={save}
      />,
    );
    expect(await screen.findByText('M.O.T.H.E.R companion')).toBeVisible();
    expect(screen.getByText(/same chat/)).toBeVisible();
    expect(screen.getByText(/Primary memory/)).toBeVisible();
    fireEvent.click(
      screen.getByRole('button', { name: 'Use this agent independently' }),
    );
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith('a', {
        expectedRevision: 'r1',
        binding: null,
      }),
    );
    expect(await screen.findByText('Independent agent')).toBeVisible();
  });

  it('keeps the local personality for shared-context mode', async () => {
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => ({
          ...companion,
          binding: { ...companion.binding!, mode: 'shared' },
        })}
      />,
    );
    expect(await screen.findByText('Shared context only')).toBeVisible();
    expect(screen.getByText(/Keeps its own personality/)).toBeVisible();
  });

  it('does not show another agent’s late response or grant', async () => {
    let finish!: (view: WorkspaceContextView) => void;
    const first = new Promise<WorkspaceContextView>((resolve) => {
      finish = resolve;
    });
    const load = vi.fn((id: string) =>
      id === 'a' ? first : Promise.resolve(independent),
    );
    const { rerender } = render(
      <WorkspaceContextPanel agentId="a" load={load} />,
    );
    rerender(<WorkspaceContextPanel agentId="b" load={load} />);
    expect(await screen.findByText('Independent agent')).toBeVisible();
    await act(async () => finish(companion));
    expect(screen.queryByText('M.O.T.H.E.R companion')).not.toBeInTheDocument();
  });

  it('requires inspection after an uncertain save instead of replaying it', async () => {
    const save = vi.fn().mockRejectedValue(new Error('Response lost'));
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => companion}
        save={save}
      />,
    );
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Use this agent independently',
      }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Reload to check/,
    );
    expect(
      screen.getByRole('button', { name: 'Use this agent independently' }),
    ).toBeDisabled();
    expect(save).toHaveBeenCalledTimes(1);
  });
});
