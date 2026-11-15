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
const additive: WorkspaceContextView = {
  ...companion,
  binding: null,
  providerBinding: {
    version: 1,
    provider: 'mother',
    connectionId: 'c1',
    workspaceId: 'w',
    generation: 'g',
    subject: 'a',
    publication: 'selected',
    personaSource: 'workspace',
    accessMode: 'companion',
  },
};

describe('per-agent workspace context settings', () => {
  it.each(['companion', 'shared'] as const)(
    'previews %s conversion before one revision-checked save',
    async (mode) => {
      const save = vi.fn().mockResolvedValue(additive);
      render(
        <WorkspaceContextPanel
          agentId="a"
          load={async () => ({
            ...companion,
            binding: { ...companion.binding!, mode },
          })}
          save={save}
        />,
      );
      fireEvent.click(
        await screen.findByRole('button', {
          name: 'Review additive connection',
        }),
      );
      expect(save).not.toHaveBeenCalled();
      expect(
        screen.getByRole('region', { name: 'Review additive connection' }),
      ).toHaveTextContent('c1');
      expect(
        screen.getByRole('region', { name: 'Review additive connection' }),
      ).toHaveTextContent(mode);
      expect(
        screen.getByText(
          /Existing chats, local data and grants stay unchanged/,
        ),
      ).toBeVisible();
      expect(
        screen.getByText(/Native memory, capture and learning resume/),
      ).toBeVisible();
      expect(
        screen.getByText(
          mode === 'companion'
            ? /Keep the M.O.T.H.E.R personality/
            : /Keep this agent’s local personality/,
        ),
      ).toBeVisible();
      fireEvent.click(
        screen.getByRole('button', { name: 'Confirm additive connection' }),
      );
      await waitFor(() =>
        expect(save).toHaveBeenCalledExactlyOnceWith('a', {
          expectedRevision: 'r1',
          binding: null,
          providerBinding: {
            ...additive.providerBinding,
            accessMode: mode,
            personaSource: mode === 'companion' ? 'workspace' : 'local',
          },
        }),
      );
    },
  );

  it('keeps native memory active while the additive connection is offline and changes personality separately', async () => {
    const save = vi.fn().mockResolvedValue({
      ...additive,
      revision: 'r2',
      providerBinding: {
        ...additive.providerBinding!,
        personaSource: 'local',
      },
    });
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => additive}
        save={save}
      />,
    );
    expect(await screen.findByText('Native memory active')).toBeVisible();
    expect(screen.getByText('disconnected')).toBeVisible();
    fireEvent.change(
      screen.getByRole('combobox', { name: 'Personality source' }),
      { target: { value: 'local' } },
    );
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole('button', { name: 'Save personality source' }),
    );
    await waitFor(() =>
      expect(save).toHaveBeenCalledExactlyOnceWith('a', {
        expectedRevision: 'r1',
        binding: null,
        providerBinding: {
          ...additive.providerBinding,
          personaSource: 'local',
        },
      }),
    );
  });

  it('does not offer workspace personality for a shared grant', async () => {
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => ({
          ...additive,
          providerBinding: {
            ...additive.providerBinding!,
            accessMode: 'shared',
            personaSource: 'local',
          },
        })}
      />,
    );
    expect(await screen.findByText('Native memory active')).toBeVisible();
    expect(
      screen.queryByRole('option', { name: 'M.O.T.H.E.R workspace' }),
    ).not.toBeInTheDocument();
  });

  it('explicitly disables the provider while preserving its remote key and records', async () => {
    const save = vi.fn().mockResolvedValue(independent);
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => additive}
        save={save}
      />,
    );
    expect(
      await screen.findByText(
        /does not revoke its key or delete remote records/,
      ),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole('button', { name: 'Disable M.O.T.H.E.R' }),
    );
    await waitFor(() =>
      expect(save).toHaveBeenCalledExactlyOnceWith('a', {
        expectedRevision: 'r1',
        binding: null,
        providerBinding: null,
      }),
    );
  });

  it('reloads after an uncertain conversion without replaying the write', async () => {
    const save = vi.fn().mockRejectedValue(new Error('Response lost'));
    const load = vi
      .fn()
      .mockResolvedValueOnce(companion)
      .mockResolvedValueOnce(additive);
    render(<WorkspaceContextPanel agentId="a" load={load} save={save} />);
    fireEvent.click(
      await screen.findByRole('button', { name: 'Review additive connection' }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Confirm additive connection' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Reload to check/,
    );
    expect(
      screen.getByRole('button', { name: 'Confirm additive connection' }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Reload connection' }));
    expect(await screen.findByText('Native memory active')).toBeVisible();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it('ignores a conversion response after switching agents', async () => {
    let finish!: (view: WorkspaceContextView) => void;
    const save = vi.fn(
      () =>
        new Promise<WorkspaceContextView>((resolve) => {
          finish = resolve;
        }),
    );
    const load = vi.fn((id: string) =>
      Promise.resolve(id === 'a' ? companion : independent),
    );
    const { rerender } = render(
      <WorkspaceContextPanel agentId="a" load={load} save={save} />,
    );
    fireEvent.click(
      await screen.findByRole('button', { name: 'Review additive connection' }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Confirm additive connection' }),
    );
    rerender(<WorkspaceContextPanel agentId="b" load={load} save={save} />);
    expect(await screen.findByText('Independent agent')).toBeVisible();
    await act(async () => finish(additive));
    expect(
      screen.queryByText('M.O.T.H.E.R garden enabled'),
    ).not.toBeInTheDocument();
  });
  it('shows an invalid saved binding as repairable rather than independent', async () => {
    const save = vi.fn().mockResolvedValue(independent);
    render(
      <WorkspaceContextPanel
        agentId="a"
        load={async () => ({ ...independent, status: 'configured' })}
        save={save}
      />,
    );
    expect(await screen.findByText('Connection needs attention')).toBeVisible();
    expect(screen.queryByText('Independent agent')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Unlink' })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: 'Unlink' }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledExactlyOnceWith('a', {
        expectedRevision: 'r2',
        binding: null,
        providerBinding: null,
      }),
    );
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
    fireEvent.click(screen.getByRole('button', { name: 'Unlink' }));
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
        name: 'Unlink',
      }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Reload to check/,
    );
    expect(screen.getByRole('button', { name: 'Unlink' })).toBeDisabled();
    expect(save).toHaveBeenCalledTimes(1);
  });
});
